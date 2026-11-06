//! Passkey registration from inside an app's iframe (`crossOrigin: true` + `topOrigin`).
//!
//! A small software authenticator builds real `none`-attestation registrations and signed
//! assertions (P-256), so these go through webauthn-rs end to end.
mod common;

use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use common::*;
use p256::ecdsa::{Signature, SigningKey, signature::Signer};
use reqwest::Client;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const APP_ORIGIN: &str = "https://app.example.com";

fn b64u(b: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(b)
}

fn unb64u(s: &str) -> Vec<u8> {
    URL_SAFE_NO_PAD.decode(s.trim_end_matches('=')).unwrap()
}

/// CBOR major type + length header.
fn cbor_head(major: u8, len: usize) -> Vec<u8> {
    let m = major << 5;
    match len {
        0..=23 => vec![m | len as u8],
        24..=255 => vec![m | 24, len as u8],
        _ => vec![m | 25, (len >> 8) as u8, len as u8],
    }
}

fn cbor_text(s: &str) -> Vec<u8> {
    [cbor_head(3, s.len()), s.as_bytes().to_vec()].concat()
}

fn cbor_bytes(b: &[u8]) -> Vec<u8> {
    [cbor_head(2, b.len()), b.to_vec()].concat()
}

struct Authenticator {
    key: SigningKey,
    cred_id: Vec<u8>,
    user_handle: Vec<u8>,
    rp_id: String,
}

impl Authenticator {
    fn new(rp_id: &str) -> Self {
        Authenticator {
            key: SigningKey::random(&mut rand::rngs::OsRng),
            cred_id: rand::random::<[u8; 16]>().to_vec(),
            user_handle: vec![],
            rp_id: rp_id.into(),
        }
    }

    fn cose_key(&self) -> Vec<u8> {
        let p = self.key.verifying_key().to_encoded_point(false);
        // {1: 2 (EC2), 3: -7 (ES256), -1: 1 (P-256), -2: x, -3: y}
        [
            vec![0xa5, 0x01, 0x02, 0x03, 0x26, 0x20, 0x01, 0x21],
            cbor_bytes(p.x().unwrap()),
            vec![0x22],
            cbor_bytes(p.y().unwrap()),
        ]
        .concat()
    }

    fn auth_data(&self, flags: u8, counter: u32, attested: bool) -> Vec<u8> {
        let mut d = Sha256::digest(self.rp_id.as_bytes()).to_vec();
        d.push(flags);
        d.extend_from_slice(&counter.to_be_bytes());
        if attested {
            d.extend_from_slice(&[0u8; 16]); // AAGUID
            d.extend_from_slice(&(self.cred_id.len() as u16).to_be_bytes());
            d.extend_from_slice(&self.cred_id);
            d.extend_from_slice(&self.cose_key());
        }
        d
    }

    /// `navigator.credentials.create()` as the browser would return it (`toJSON()` shape).
    /// `client_data` is the full clientDataJSON object.
    fn create(&mut self, options: &Value, client_data: Value, fmt: &str) -> Value {
        self.user_handle = unb64u(options["publicKey"]["user"]["id"].as_str().unwrap());
        // UP | UV | AT
        let auth_data = self.auth_data(0x45, 0, true);
        let att_stmt = if fmt == "none" {
            vec![0xa0]
        } else {
            // A stand-in signed statement: {"alg": -7, "sig": h'00'}
            [
                vec![0xa2],
                cbor_text("alg"),
                vec![0x26],
                cbor_text("sig"),
                cbor_bytes(&[0]),
            ]
            .concat()
        };
        let att_obj = [
            vec![0xa3],
            cbor_text("fmt"),
            cbor_text(fmt),
            cbor_text("attStmt"),
            att_stmt,
            cbor_text("authData"),
            cbor_bytes(&auth_data),
        ]
        .concat();
        json!({
            "id": b64u(&self.cred_id),
            "rawId": b64u(&self.cred_id),
            "type": "public-key",
            "response": {
                "clientDataJSON": b64u(client_data.to_string().as_bytes()),
                "attestationObject": b64u(&att_obj),
                "transports": ["internal"],
            },
            "clientExtensionResults": {},
        })
    }

    /// `navigator.credentials.get()`: a signed assertion over authData || SHA-256(clientDataJSON).
    fn get(&self, client_data: Value, counter: u32) -> Value {
        let cdj = client_data.to_string();
        let auth_data = self.auth_data(0x05, counter, false);
        let mut msg = auth_data.clone();
        msg.extend_from_slice(&Sha256::digest(cdj.as_bytes()));
        let sig: Signature = self.key.sign(&msg);
        json!({
            "id": b64u(&self.cred_id),
            "rawId": b64u(&self.cred_id),
            "type": "public-key",
            "response": {
                "clientDataJSON": b64u(cdj.as_bytes()),
                "authenticatorData": b64u(&auth_data),
                "signature": b64u(sig.to_der().as_bytes()),
                "userHandle": b64u(&self.user_handle),
            },
            "clientExtensionResults": {},
        })
    }
}

/// Client data as Chrome builds it inside a cross-origin iframe.
fn iframe_client_data(kind: &str, challenge: &str, origin: &str, top: &str) -> Value {
    json!({ "type": kind, "challenge": challenge, "origin": origin, "crossOrigin": true, "topOrigin": top })
}

fn same_origin_client_data(kind: &str, challenge: &str, origin: &str) -> Value {
    json!({ "type": kind, "challenge": challenge, "origin": origin, "crossOrigin": false })
}

async fn post(c: &Client, s: &TestServer, path: &str, body: Value) -> (u16, Value) {
    let r = c.post(s.u(path)).json(&body).send().await.unwrap();
    let status = r.status().as_u16();
    (status, r.json().await.unwrap_or(Value::Null))
}

async fn reg_start(c: &Client, s: &TestServer) -> (String, Value) {
    let (st, r) = post(c, s, "/v1/passkeys/register/start", json!({})).await;
    assert_eq!(st, 200, "{r}");
    (r["flow_id"].as_str().unwrap().to_string(), r["options"].clone())
}

fn challenge_of(options: &Value) -> String {
    options["publicKey"]["challenge"].as_str().unwrap().to_string()
}

async fn user_count(s: &TestServer) -> i64 {
    let (n,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM users")
        .fetch_one(&s.state.db)
        .await
        .unwrap();
    n
}

#[tokio::test]
async fn iframe_registration_under_a_registered_app_is_accepted_and_usable() {
    let s = spawn(|_| {}).await;
    s.create_app(&[APP_ORIGIN]).await;
    let c = s.browser();
    let mut a = Authenticator::new("localhost");

    let (flow, opts) = reg_start(&c, &s).await;
    let cd = iframe_client_data("webauthn.create", &challenge_of(&opts), &s.url, APP_ORIGIN);
    let cred = a.create(&opts, cd, "none");
    let (st, r) = post(
        &c,
        &s,
        "/v1/passkeys/register/finish",
        json!({ "flow_id": flow, "credential": cred }),
    )
    .await;
    assert_eq!(st, 200, "{r}");
    assert_eq!(r["new_user"], true);
    assert_eq!(r["credential_id"], b64u(&a.cred_id));

    // The stored credential works for a (signed, also cross-origin) passkey login.
    let anon = s.browser();
    let (st, r) = post(&anon, &s, "/v1/passkeys/login/start", json!({})).await;
    assert_eq!(st, 200);
    let cd = iframe_client_data(
        "webauthn.get",
        r["options"]["publicKey"]["challenge"].as_str().unwrap(),
        &s.url,
        APP_ORIGIN,
    );
    let (st, r) = post(
        &anon,
        &s,
        "/v1/passkeys/login/finish",
        json!({ "flow_id": r["flow_id"], "credential": a.get(cd, 1) }),
    )
    .await;
    assert_eq!(st, 200, "{r}");
    assert_eq!(r["credential_id"], b64u(&a.cred_id));
}

#[tokio::test]
async fn iframe_registration_under_an_unregistered_top_origin_is_rejected() {
    let s = spawn(|_| {}).await;
    s.create_app(&[APP_ORIGIN]).await;
    let c = s.browser();
    let before = user_count(&s).await;

    for top in [
        "https://evil.example",
        "https://app.example.com:444", // same host, other port
        "http://app.example.com",      // other scheme
        "https://sub.app.example.com", // subdomain
        "https://app.example.com/",    // not an origin serialization
    ] {
        let mut a = Authenticator::new("localhost");
        let (flow, opts) = reg_start(&c, &s).await;
        let cd = iframe_client_data("webauthn.create", &challenge_of(&opts), &s.url, top);
        let (st, r) = post(
            &c,
            &s,
            "/v1/passkeys/register/finish",
            json!({ "flow_id": flow, "credential": a.create(&opts, cd, "none") }),
        )
        .await;
        assert_eq!(st, 403, "{top}: {r}");
        assert_eq!(r["error"], "top_origin_not_registered", "{r}");
    }

    // crossOrigin without a topOrigin (older browsers) is refused too.
    let mut a = Authenticator::new("localhost");
    let (flow, opts) = reg_start(&c, &s).await;
    let cd =
        json!({ "type": "webauthn.create", "challenge": challenge_of(&opts), "origin": s.url, "crossOrigin": true });
    let (st, _) = post(
        &c,
        &s,
        "/v1/passkeys/register/finish",
        json!({ "flow_id": flow, "credential": a.create(&opts, cd, "none") }),
    )
    .await;
    assert_eq!(st, 400);
    assert_eq!(user_count(&s).await, before, "no user was created");
}

#[tokio::test]
async fn iframe_registration_keeps_every_other_check() {
    let s = spawn(|_| {}).await;
    s.create_app(&[APP_ORIGIN]).await;
    let c = s.browser();

    // (client data, attestation format, what is wrong)
    type Case = fn(&str, &str) -> Value;
    let cases: Vec<(Case, &str, &str)> = vec![
        (
            |ch, o| iframe_client_data("webauthn.get", ch, o, APP_ORIGIN),
            "none",
            "type",
        ),
        (
            |_, o| iframe_client_data("webauthn.create", &b64u(&[7u8; 32]), o, APP_ORIGIN),
            "none",
            "challenge",
        ),
        (
            |ch, _| iframe_client_data("webauthn.create", ch, APP_ORIGIN, APP_ORIGIN),
            "none",
            "origin is the app, not Lowki",
        ),
        (
            |ch, _| iframe_client_data("webauthn.create", ch, "https://evil.example", APP_ORIGIN),
            "none",
            "origin",
        ),
        (
            |ch, o| iframe_client_data("webauthn.create", ch, o, APP_ORIGIN),
            "packed",
            "signed attestation (client data can't be rewritten)",
        ),
    ];
    for (cd, fmt, what) in cases {
        let mut a = Authenticator::new("localhost");
        let (flow, opts) = reg_start(&c, &s).await;
        let cred = a.create(&opts, cd(&challenge_of(&opts), &s.url), fmt);
        let (st, r) = post(
            &c,
            &s,
            "/v1/passkeys/register/finish",
            json!({ "flow_id": flow, "credential": cred }),
        )
        .await;
        assert_eq!(st, 400, "{what}: {r}");
    }

    // Wrong RP ID: everything in the client data is right, the authenticator data is not.
    let mut a = Authenticator::new("evil.example");
    let (flow, opts) = reg_start(&c, &s).await;
    let cd = iframe_client_data("webauthn.create", &challenge_of(&opts), &s.url, APP_ORIGIN);
    let (st, _) = post(
        &c,
        &s,
        "/v1/passkeys/register/finish",
        json!({ "flow_id": flow, "credential": a.create(&opts, cd, "none") }),
    )
    .await;
    assert_eq!(st, 400, "RP ID hash is still checked by webauthn-rs");

    // A refused ceremony is burned: the flow can't be retried with fixed-up data.
    let mut a = Authenticator::new("localhost");
    let (flow, opts) = reg_start(&c, &s).await;
    let bad = iframe_client_data("webauthn.create", &challenge_of(&opts), &s.url, "https://evil.example");
    let (st, _) = post(
        &c,
        &s,
        "/v1/passkeys/register/finish",
        json!({ "flow_id": flow, "credential": a.create(&opts, bad, "none") }),
    )
    .await;
    assert_eq!(st, 403);
    let good = iframe_client_data("webauthn.create", &challenge_of(&opts), &s.url, APP_ORIGIN);
    let (st, _) = post(
        &c,
        &s,
        "/v1/passkeys/register/finish",
        json!({ "flow_id": flow, "credential": a.create(&opts, good, "none") }),
    )
    .await;
    assert_eq!(st, 400, "flow is single use");
}

#[tokio::test]
async fn same_origin_registration_is_unchanged() {
    let s = spawn(|_| {}).await;
    let c = s.browser();

    // No app registered at all: a top-level (same-origin) registration doesn't involve apps.
    for (cd_of, fmt) in [
        (
            (|ch: &str, o: &str| same_origin_client_data("webauthn.create", ch, o)) as fn(&str, &str) -> Value,
            "none",
        ),
        // Browsers that omit crossOrigin entirely.
        (
            |ch, o| json!({ "type": "webauthn.create", "challenge": ch, "origin": o }),
            "none",
        ),
    ] {
        let mut a = Authenticator::new("localhost");
        let anon = s.browser();
        let (flow, opts) = reg_start(&anon, &s).await;
        let cred = a.create(&opts, cd_of(&challenge_of(&opts), &s.url), fmt);
        let (st, r) = post(
            &anon,
            &s,
            "/v1/passkeys/register/finish",
            json!({ "flow_id": flow, "credential": cred }),
        )
        .await;
        assert_eq!(st, 200, "{r}");
        assert_eq!(r["new_user"], true);
    }

    // Same-origin checks still apply: a wrong challenge or origin is rejected by webauthn-rs.
    let mut a = Authenticator::new("localhost");
    let (flow, opts) = reg_start(&c, &s).await;
    let cd = same_origin_client_data("webauthn.create", &challenge_of(&opts), "https://evil.example");
    let (st, _) = post(
        &c,
        &s,
        "/v1/passkeys/register/finish",
        json!({ "flow_id": flow, "credential": a.create(&opts, cd, "none") }),
    )
    .await;
    assert_eq!(st, 400);
    let (flow, opts) = reg_start(&c, &s).await;
    let cd = same_origin_client_data("webauthn.create", &b64u(&[1u8; 32]), &s.url);
    let (st, _) = post(
        &c,
        &s,
        "/v1/passkeys/register/finish",
        json!({ "flow_id": flow, "credential": a.create(&opts, cd, "none") }),
    )
    .await;
    assert_eq!(st, 400);
}

// ---- one origin per app ---------------------------------------------------------------------

#[tokio::test]
async fn an_app_is_exactly_one_origin() {
    let s = spawn(|_| {}).await;
    let admin = Client::new();
    for origins in [json!([]), json!([APP_ORIGIN, "https://www.example.com"]), Value::Null] {
        let mut body = json!({ "name": "x" });
        if !origins.is_null() {
            body["allowed_origins"] = origins.clone();
        }
        let r = admin
            .post(s.u("/admin/v1/apps"))
            .bearer_auth(ADMIN)
            .json(&body)
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 400, "{origins}");
    }
    let app = s.create_app(&[APP_ORIGIN]).await;
    let id = app["id"].as_str().unwrap();
    let r = admin
        .patch(s.u(&format!("/admin/v1/apps/{id}")))
        .bearer_auth(ADMIN)
        .json(&json!({ "allowed_origins": [APP_ORIGIN, "https://www.example.com"] }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 400);
    // Moving the app to another origin is allowed (it is still one origin).
    let r = admin
        .patch(s.u(&format!("/admin/v1/apps/{id}")))
        .bearer_auth(ADMIN)
        .json(&json!({ "allowed_origins": ["https://www.example.com"] }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);

    // A legacy row with two origins (created before the rule) issues no tokens.
    sqlx::query("UPDATE apps SET allowed_origins = $1 WHERE id = $2")
        .bind(json!([APP_ORIGIN, "https://www.example.com"]).to_string())
        .bind(id)
        .execute(&s.state.db)
        .await
        .unwrap();
    lowki_server::apps::refresh_origin_cache(&s.state).await.unwrap();
    let c = s.browser();
    s.email_login(&c, "legacy@example.com").await;
    let r = s.app_token(&c, id, APP_ORIGIN).await;
    assert_eq!(r.status(), 409);
}
