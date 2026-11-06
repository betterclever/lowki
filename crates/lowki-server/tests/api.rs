mod common;

use std::time::Duration;

use common::*;
use jsonwebtoken::{DecodingKey, Validation, jwk::JwkSet};
use lowki_server::{config::Limit, crypto::hmac_hex};
use reqwest::{Client, header};
use serde_json::{Value, json};

const APP_ORIGIN: &str = "https://app.example.com";

async fn oidc_server(m: &MockIssuer, customize: impl FnOnce(&mut lowki_server::config::Config)) -> TestServer {
    let cfg = oidc_config(m);
    spawn(move |c| {
        c.oidc = Some(cfg);
        customize(c);
    })
    .await
}

// ---- OIDC ----------------------------------------------------------------------------------

#[tokio::test]
async fn oidc_flow_end_to_end_with_mock_issuer() {
    let m = spawn_mock_issuer().await;
    m.behaviour.lock().unwrap().email = Some("alice@example.com".into());
    let s = oidc_server(&m, |_| {}).await;
    let c = s.browser();

    let html = google_login(&s, &c, false).await;
    assert!(html.contains("\"ok\":true"), "{html}");
    assert!(html.contains("\"new_user\":true"));
    assert!(html.contains("window.opener.postMessage"));
    assert!(
        html.contains(&format!("{:?}", s.url)),
        "postMessage targets the Lowki origin"
    );

    let sess: Value = c.get(s.u("/v1/session")).send().await.unwrap().json().await.unwrap();
    assert_eq!(sess["authenticated"], true);
    let methods = sess["methods"].as_array().unwrap();
    assert_eq!(methods.len(), 1);
    assert_eq!(methods[0]["kind"], "oidc");
    assert_eq!(methods[0]["provider"], "google");
    assert!(methods[0].get("email").is_none(), "email is not stored by default");

    // Only HMAC("google:" || sub) is stored, never the raw sub or the email.
    let expected = hmac_hex(&s.state.keys.identity, &[b"google:1234567890"]);
    let rows: Vec<(String, Option<String>)> = sqlx::query_as("SELECT subject_key, email FROM identities")
        .fetch_all(&s.state.db)
        .await
        .unwrap();
    assert_eq!(rows, vec![(expected, None)]);

    // Second sign-in with the same Google account maps to the same user.
    let c2 = s.browser();
    let html = google_login(&s, &c2, false).await;
    assert!(html.contains("\"new_user\":false"));
    let (n,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM users")
        .fetch_one(&s.state.db)
        .await
        .unwrap();
    assert_eq!(n, 1);
}

#[tokio::test]
async fn oidc_stores_email_only_when_enabled() {
    let m = spawn_mock_issuer().await;
    m.behaviour.lock().unwrap().email = Some("bob@example.com".into());
    let s = oidc_server(&m, |c| c.store_email = true).await;
    google_login(&s, &s.browser(), false).await;
    let (email,): (Option<String>,) = sqlx::query_as("SELECT email FROM identities")
        .fetch_one(&s.state.db)
        .await
        .unwrap();
    assert_eq!(email.as_deref(), Some("bob@example.com"));
}

#[tokio::test]
async fn oidc_rejects_bad_id_tokens() {
    let m = spawn_mock_issuer().await;
    let s = oidc_server(&m, |_| {}).await;
    type Tweak = fn(&mut MockBehaviour);
    let cases: [(&str, Tweak); 5] = [
        ("nonce", |b| b.wrong_nonce = true),
        ("aud", |b| b.wrong_aud = true),
        ("iss", |b| b.wrong_iss = true),
        ("exp", |b| b.expired = true),
        ("kid", |b| b.unknown_kid = true),
    ];
    for (name, tweak) in cases {
        {
            let mut b = m.behaviour.lock().unwrap();
            *b = MockBehaviour {
                sub: "x".into(),
                ..Default::default()
            };
            tweak(&mut b);
        }
        let html = google_login(&s, &s.browser(), false).await;
        assert!(html.contains("\"error\":\"invalid_id_token\""), "{name}: {html}");
    }
    let (n,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM users")
        .fetch_one(&s.state.db)
        .await
        .unwrap();
    assert_eq!(n, 0);
}

#[tokio::test]
async fn oidc_state_is_bound_to_browser_and_single_use() {
    let m = spawn_mock_issuer().await;
    let s = oidc_server(&m, |_| {}).await;
    let victim = s.browser();
    let r = victim.get(s.u("/v1/oauth/google/start")).send().await.unwrap();
    let to_idp = r.headers()[header::LOCATION].to_str().unwrap().to_string();
    let r = victim.get(&to_idp).send().await.unwrap();
    let back = r.headers()[header::LOCATION].to_str().unwrap().to_string();

    // The callback URL replayed in another browser (no binding cookie) fails...
    let attacker = s.browser();
    let html = attacker.get(&back).send().await.unwrap().text().await.unwrap();
    assert!(html.contains("invalid_state"), "{html}");
    // ...and the state was consumed, so it can't be used again either.
    let html = victim.get(&back).send().await.unwrap().text().await.unwrap();
    assert!(html.contains("invalid_state"), "{html}");
    // The code was never exchanged.
    assert_eq!(*m.token_requests.lock().unwrap(), 0);
}

#[tokio::test]
async fn oidc_start_uses_pkce_and_nonce() {
    let m = spawn_mock_issuer().await;
    let s = oidc_server(&m, |_| {}).await;
    let r = s.browser().get(s.u("/v1/oauth/google/start")).send().await.unwrap();
    let loc = url::Url::parse(r.headers()[header::LOCATION].to_str().unwrap()).unwrap();
    let q: std::collections::HashMap<_, _> = loc.query_pairs().into_owned().collect();
    assert_eq!(q["code_challenge_method"], "S256");
    assert_eq!(q["scope"], "openid");
    assert!(q["state"].len() >= 43 && q["nonce"].len() >= 43 && q["code_challenge"].len() == 43);
    assert_eq!(q["redirect_uri"], format!("{}/v1/oauth/google/callback", s.url));
    let cookie = r.headers()[header::SET_COOKIE].to_str().unwrap();
    assert!(cookie.contains("HttpOnly") && cookie.contains("SameSite=Lax"));
}

// ---- email OTP -----------------------------------------------------------------------------

#[tokio::test]
async fn email_otp_flow() {
    let s = spawn(|_| {}).await;
    let c = s.browser();
    let r = c
        .post(s.u("/v1/email/start"))
        .json(&json!({ "email": " Carol@Example.com " }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    let code = s.otp.code_for("carol@example.com");
    assert_eq!(code.len(), 6);
    assert!(code.chars().all(|ch| ch.is_ascii_digit()));

    let wrong = if code == "000000" { "000001" } else { "000000" };
    let r = c
        .post(s.u("/v1/email/verify"))
        .json(&json!({ "email": "carol@example.com", "code": wrong }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 400);

    let r = c
        .post(s.u("/v1/email/verify"))
        .json(&json!({ "email": "carol@example.com", "code": code }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    let body: Value = r.json().await.unwrap();
    assert_eq!(body["new_user"], true);

    // Single use.
    let r = s
        .browser()
        .post(s.u("/v1/email/verify"))
        .json(&json!({ "email": "carol@example.com", "code": code }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 400);

    // Stored as an HMAC only.
    let rows: Vec<(String, Option<String>)> = sqlx::query_as("SELECT subject_key, email FROM identities")
        .fetch_all(&s.state.db)
        .await
        .unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(
        rows[0].0,
        hmac_hex(&s.state.keys.identity, &[b"email:carol@example.com"])
    );
    assert_eq!(rows[0].1, None);
    let (otp_rows,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM otp_codes")
        .fetch_one(&s.state.db)
        .await
        .unwrap();
    assert_eq!(otp_rows, 0);
}

#[tokio::test]
async fn email_otp_expires_and_burns_after_max_attempts() {
    let s = spawn(|c| c.otp_ttl = Duration::from_secs(1)).await;
    let c = s.browser();
    c.post(s.u("/v1/email/start"))
        .json(&json!({ "email": "dave@example.com" }))
        .send()
        .await
        .unwrap();
    let code = s.otp.code_for("dave@example.com");
    tokio::time::sleep(Duration::from_millis(2100)).await;
    let r = c
        .post(s.u("/v1/email/verify"))
        .json(&json!({ "email": "dave@example.com", "code": code }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 400, "expired");

    let s = spawn(|_| {}).await;
    let c = s.browser();
    c.post(s.u("/v1/email/start"))
        .json(&json!({ "email": "erin@example.com" }))
        .send()
        .await
        .unwrap();
    let code = s.otp.code_for("erin@example.com");
    let wrong = if code == "999999" { "999998" } else { "999999" };
    for _ in 0..5 {
        let r = c
            .post(s.u("/v1/email/verify"))
            .json(&json!({ "email": "erin@example.com", "code": wrong }))
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 400);
    }
    let r = c
        .post(s.u("/v1/email/verify"))
        .json(&json!({ "email": "erin@example.com", "code": code }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 400, "code is burned after 5 wrong attempts");
}

// ---- rate limits ---------------------------------------------------------------------------

#[tokio::test]
async fn rate_limits() {
    let s = spawn(|c| {
        c.limits.otp_start_per_email = Limit::new(2, 900);
        c.limits.otp_verify_per_email = Limit::new(3, 900);
        c.limits.share_release_per_user = Limit::new(2, 3600);
    })
    .await;
    let c = s.browser();
    for _ in 0..2 {
        let r = c
            .post(s.u("/v1/email/start"))
            .json(&json!({ "email": "frank@example.com" }))
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 200);
    }
    let r = c
        .post(s.u("/v1/email/start"))
        .json(&json!({ "email": "frank@example.com" }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 429);
    assert!(r.headers().contains_key(header::RETRY_AFTER));
    // Normalized: case changes don't dodge the limit.
    let r = c
        .post(s.u("/v1/email/start"))
        .json(&json!({ "email": "FRANK@example.com" }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 429);

    for _ in 0..3 {
        let r = c
            .post(s.u("/v1/email/verify"))
            .json(&json!({ "email": "gina@example.com", "code": "123456" }))
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 400);
    }
    let r = c
        .post(s.u("/v1/email/verify"))
        .json(&json!({ "email": "gina@example.com", "code": "123456" }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 429);

    // Share release.
    let c = s.browser();
    let csrf = s.email_login(&c, "hank@example.com").await;
    let r = c
        .put(s.u("/v1/me/share"))
        .header("x-lowki-csrf", &csrf)
        .json(&json!({ "share": "AQIDBA" }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    for _ in 0..2 {
        let r = c
            .post(s.u("/v1/me/share/release"))
            .header("x-lowki-csrf", &csrf)
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 200);
    }
    let r = c
        .post(s.u("/v1/me/share/release"))
        .header("x-lowki-csrf", &csrf)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 429);
    let denied: Vec<(String,)> = sqlx::query_as("SELECT detail FROM audit_log WHERE event = 'share.release_denied'")
        .fetch_all(&s.state.db)
        .await
        .unwrap();
    assert!(denied.iter().any(|(d,)| d.contains("rate_limited")));
}

// ---- share storage -------------------------------------------------------------------------

#[tokio::test]
async fn share_release_requires_the_bound_method() {
    let m = spawn_mock_issuer().await;
    let s = oidc_server(&m, |_| {}).await;

    // No session.
    let r = s.browser().post(s.u("/v1/me/share/release")).send().await.unwrap();
    assert_eq!(r.status(), 401);

    // Google sign-in, store S (bound to Google).
    let c = s.browser();
    google_login(&s, &c, false).await;
    let csrf = s.csrf(&c).await;
    let r = c
        .put(s.u("/v1/me/share"))
        .json(&json!({ "share": "c2hhcmUtYnl0ZXM" }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 403, "CSRF header required");
    let r = c
        .put(s.u("/v1/me/share"))
        .header("x-lowki-csrf", &csrf)
        .json(&json!({ "share": "c2hhcmUtYnl0ZXM" }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    let r = c
        .post(s.u("/v1/me/share/release"))
        .header("x-lowki-csrf", "wrong")
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 403);
    let r: Value = c
        .post(s.u("/v1/me/share/release"))
        .header("x-lowki-csrf", &csrf)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(r["share"], "c2hhcmUtYnl0ZXM");

    // Link an email to the same user.
    c.post(s.u("/v1/email/start"))
        .json(&json!({ "email": "ivy@example.com" }))
        .send()
        .await
        .unwrap();
    let code = s.otp.code_for("ivy@example.com");
    let r = c
        .post(s.u("/v1/email/verify"))
        .header("x-lowki-csrf", &csrf)
        .json(&json!({ "email": "ivy@example.com", "code": code, "link": true }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    assert_eq!(r.json::<Value>().await.unwrap()["linked"], true);

    // A new browser signed in with the linked email only: same user, wrong method.
    let other = s.browser();
    let csrf2 = s.email_login(&other, "ivy@example.com").await;
    let r = other
        .post(s.u("/v1/me/share/release"))
        .header("x-lowki-csrf", &csrf2)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 403);
    // It also can't overwrite S.
    let r = other
        .put(s.u("/v1/me/share"))
        .header("x-lowki-csrf", &csrf2)
        .json(&json!({ "share": "ZXZpbA" }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 403);
    // After a Google sign-in in that browser, it can.
    google_login(&s, &other, false).await;
    let csrf2 = s.csrf(&other).await;
    let r: Value = other
        .post(s.u("/v1/me/share/release"))
        .header("x-lowki-csrf", &csrf2)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(r["share"], "c2hhcmUtYnl0ZXM");

    // A different user never sees it.
    let mallory = s.browser();
    let csrf3 = s.email_login(&mallory, "mallory@example.com").await;
    let r = mallory
        .post(s.u("/v1/me/share/release"))
        .header("x-lowki-csrf", &csrf3)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 404);

    // Audit log has releases and denials, never the share bytes.
    let rows: Vec<(String, String)> = sqlx::query_as("SELECT event, detail FROM audit_log")
        .fetch_all(&s.state.db)
        .await
        .unwrap();
    assert!(rows.iter().any(|(e, _)| e == "share.released"));
    assert!(
        rows.iter()
            .any(|(e, d)| e == "share.release_denied" && d.contains("bound_method_not_fresh"))
    );
    assert!(rows.iter().all(|(_, d)| !d.contains("c2hhcmUtYnl0ZXM")));
}

#[tokio::test]
async fn share_release_requires_fresh_sign_in() {
    let s = spawn(|c| c.share_fresh_auth = Duration::from_secs(1)).await;
    let c = s.browser();
    let csrf = s.email_login(&c, "jack@example.com").await;
    let r = c
        .put(s.u("/v1/me/share"))
        .header("x-lowki-csrf", &csrf)
        .json(&json!({ "share": "AQID" }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    tokio::time::sleep(Duration::from_millis(2100)).await;
    let r = c
        .post(s.u("/v1/me/share/release"))
        .header("x-lowki-csrf", &csrf)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 403);
}

#[tokio::test]
async fn recovery_delay_hook() {
    let s = spawn(|c| c.recovery_delay = Duration::from_secs(2)).await;
    let c = s.browser();
    let csrf = s.email_login(&c, "kate@example.com").await;
    c.put(s.u("/v1/me/share"))
        .header("x-lowki-csrf", &csrf)
        .json(&json!({ "share": "AQID" }))
        .send()
        .await
        .unwrap();
    let r = c
        .post(s.u("/v1/me/share/release"))
        .header("x-lowki-csrf", &csrf)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 202);
    assert_eq!(r.json::<Value>().await.unwrap()["status"], "pending");
    let r = c
        .post(s.u("/v1/me/share/release"))
        .header("x-lowki-csrf", &csrf)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 202);
    // Only a passkey session may cancel.
    let r = c
        .delete(s.u("/v1/me/share/recovery"))
        .header("x-lowki-csrf", &csrf)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 403);
    tokio::time::sleep(Duration::from_millis(3100)).await;
    let r: Value = c
        .post(s.u("/v1/me/share/release"))
        .header("x-lowki-csrf", &csrf)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(r["share"], "AQID");
}

// ---- JWT / JWKS / pairwise -----------------------------------------------------------------

async fn fetch_jwks(s: &TestServer) -> JwkSet {
    Client::new()
        .get(s.u("/.well-known/jwks.json"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap()
}

fn verify(token: &str, jwks: &JwkSet, iss: &str, aud: &str) -> Value {
    let header = jsonwebtoken::decode_header(token).unwrap();
    assert_eq!(header.alg, jsonwebtoken::Algorithm::ES256);
    let jwk = jwks.find(header.kid.as_deref().unwrap()).expect("kid in JWKS");
    let mut v = Validation::new(jsonwebtoken::Algorithm::ES256);
    v.set_issuer(&[iss]);
    v.set_audience(&[aud]);
    jsonwebtoken::decode::<Value>(token, &DecodingKey::from_jwk(jwk).unwrap(), &v)
        .unwrap()
        .claims
}

#[tokio::test]
async fn jwt_issuance_jwks_and_rotation() {
    let s = spawn(|_| {}).await;
    let app = s.create_app(&[APP_ORIGIN]).await;
    let app_id = app["id"].as_str().unwrap();
    let c = s.browser();
    s.email_login(&c, "liam@example.com").await;

    let r = s.app_token(&c, app_id, APP_ORIGIN).await;
    assert_eq!(r.status(), 200);
    assert_eq!(r.headers()[header::ACCESS_CONTROL_ALLOW_ORIGIN], APP_ORIGIN);
    assert_eq!(r.headers()[header::ACCESS_CONTROL_ALLOW_CREDENTIALS], "true");
    let body: Value = r.json().await.unwrap();
    let token = body["access_token"].as_str().unwrap().to_string();

    let jwks = fetch_jwks(&s).await;
    let claims = verify(&token, &jwks, &s.url, app_id);
    assert_eq!(claims["iss"], s.url.as_str());
    assert_eq!(claims["aud"], app_id);
    assert_eq!(claims["sub"], body["user_id"]);
    assert!(claims["sub"].as_str().unwrap().starts_with("did:lowki:"));
    let ttl = claims["exp"].as_i64().unwrap() - claims["iat"].as_i64().unwrap();
    assert_eq!(ttl, 900);
    assert!(claims["sid"].is_string() && claims["jti"].is_string());

    // Discovery doc points at the JWKS.
    let d: Value = Client::new()
        .get(s.u("/.well-known/openid-configuration"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(d["jwks_uri"], format!("{}/.well-known/jwks.json", s.url));

    // Node `jose` (the library Privy's docs use) accepts the token, if installed.
    verify_with_node_jose(&s, &token, app_id).await;

    // Rotation: new tokens use a new kid; old tokens still verify during the grace period.
    let r: Value = Client::new()
        .post(s.u("/admin/v1/keys/rotate"))
        .bearer_auth(ADMIN)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let new_kid = r["kid"].as_str().unwrap();
    let token2: Value = s.app_token(&c, app_id, APP_ORIGIN).await.json().await.unwrap();
    let token2 = token2["access_token"].as_str().unwrap();
    assert_eq!(
        jsonwebtoken::decode_header(token2).unwrap().kid.as_deref(),
        Some(new_kid)
    );
    let jwks = fetch_jwks(&s).await;
    assert_eq!(jwks.keys.len(), 2);
    verify(&token, &jwks, &s.url, app_id);
    verify(token2, &jwks, &s.url, app_id);

    // A token for one app does not validate for another audience.
    let header = jsonwebtoken::decode_header(&token).unwrap();
    let mut v = Validation::new(jsonwebtoken::Algorithm::ES256);
    v.set_audience(&["app_other"]);
    let k = DecodingKey::from_jwk(jwks.find(header.kid.as_deref().unwrap()).unwrap()).unwrap();
    assert!(jsonwebtoken::decode::<Value>(&token, &k, &v).is_err());
}

async fn verify_with_node_jose(s: &TestServer, token: &str, app_id: &str) {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/node");
    if !dir.join("node_modules/jose").exists() {
        eprintln!("SKIP node jose check: run `npm ci` in tests/node to enable it");
        return;
    }
    let jwks = Client::new()
        .get(s.u("/.well-known/jwks.json"))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    let out = tokio::process::Command::new("node")
        .arg("verify-jose.mjs")
        .current_dir(&dir)
        .env("LOWKI_JWKS", jwks)
        .env("LOWKI_TOKEN", token)
        .env("LOWKI_ISS", &s.url)
        .env("LOWKI_AUD", app_id)
        .output()
        .await
        .expect("node runs");
    let stdout = String::from_utf8_lossy(&out.stdout);
    assert!(
        out.status.success(),
        "jose rejected token: {stdout} {}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(stdout.contains("\"sub\":\"did:lowki:"), "{stdout}");
    eprintln!("node jose verified: {}", stdout.trim());
}

#[tokio::test]
async fn token_endpoint_origin_rules() {
    let s = spawn(|_| {}).await;
    let app = s.create_app(&[APP_ORIGIN]).await;
    let app_id = app["id"].as_str().unwrap();
    let c = s.browser();
    let csrf = s.email_login(&c, "mia@example.com").await;

    // Unregistered origin.
    let r = s.app_token(&c, app_id, "https://evil.example").await;
    assert_eq!(r.status(), 403);
    assert!(!r.headers().contains_key(header::ACCESS_CONTROL_ALLOW_ORIGIN));

    // CORS preflight only succeeds for registered origins.
    let pre = |origin: &'static str| {
        Client::new()
            .request(reqwest::Method::OPTIONS, s.u(&format!("/v1/apps/{app_id}/token")))
            .header(header::ORIGIN, origin)
            .header(header::ACCESS_CONTROL_REQUEST_METHOD, "POST")
            .send()
    };
    let r = pre(APP_ORIGIN).await.unwrap();
    assert_eq!(r.headers()[header::ACCESS_CONTROL_ALLOW_ORIGIN], APP_ORIGIN);
    let r = pre("https://evil.example").await.unwrap();
    assert!(!r.headers().contains_key(header::ACCESS_CONTROL_ALLOW_ORIGIN));

    // From the Lowki iframe: CSRF header and a registered parent origin are required.
    let url = s.u(&format!("/v1/apps/{app_id}/token"));
    let r = c
        .post(&url)
        .json(&json!({ "origin": APP_ORIGIN }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 403);
    let r = c
        .post(&url)
        .header("x-lowki-csrf", &csrf)
        .json(&json!({ "origin": "https://evil.example" }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 403);
    let r = c
        .post(&url)
        .header("x-lowki-csrf", &csrf)
        .json(&json!({ "origin": APP_ORIGIN }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);

    // No session.
    let r = s.app_token(&s.browser(), app_id, APP_ORIGIN).await;
    assert_eq!(r.status(), 401);
}

#[tokio::test]
async fn pairwise_ids_differ_across_apps_and_are_stable() {
    let s = spawn(|_| {}).await;
    let a = s.create_app(&[APP_ORIGIN]).await;
    let b = s.create_app(&["https://other.example.org"]).await;
    let (a_id, b_id) = (a["id"].as_str().unwrap(), b["id"].as_str().unwrap());

    let c = s.browser();
    s.email_login(&c, "noah@example.com").await;
    let sub = |r: Value| r["user_id"].as_str().unwrap().to_string();
    let a1 = sub(s.app_token(&c, a_id, APP_ORIGIN).await.json().await.unwrap());
    let a2 = sub(s.app_token(&c, a_id, APP_ORIGIN).await.json().await.unwrap());
    let b1 = sub(s
        .app_token(&c, b_id, "https://other.example.org")
        .await
        .json()
        .await
        .unwrap());
    assert_eq!(a1, a2, "stable within an app");
    assert_ne!(a1, b1, "different across apps");

    // Stable across sessions and devices.
    let c2 = s.browser();
    s.email_login(&c2, "noah@example.com").await;
    let a3 = sub(s.app_token(&c2, a_id, APP_ORIGIN).await.json().await.unwrap());
    assert_eq!(a1, a3);

    // Another user gets another ID.
    let c3 = s.browser();
    s.email_login(&c3, "olivia@example.com").await;
    let other = sub(s.app_token(&c3, a_id, APP_ORIGIN).await.json().await.unwrap());
    assert_ne!(a1, other);

    // Admin lists users by pairwise ID only.
    let list: Value = Client::new()
        .get(s.u(&format!("/admin/v1/apps/{a_id}/users")))
        .bearer_auth(ADMIN)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let ids: Vec<&str> = list["users"]
        .as_array()
        .unwrap()
        .iter()
        .map(|u| u["id"].as_str().unwrap())
        .collect();
    assert_eq!(ids.len(), 2);
    assert!(ids.contains(&a1.as_str()) && ids.contains(&other.as_str()));
    assert!(!list.to_string().contains("@"), "no emails");

    // App server API with app credentials.
    let secret = a["app_secret"].as_str().unwrap();
    let r: Value = Client::new()
        .get(s.u(&format!("/api/v1/users/{a1}")))
        .basic_auth(a_id, Some(secret))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(r["id"], a1.as_str());
    assert_eq!(r["linked_account_types"], json!(["email"]));
    // App B can't look up app A's user.
    let r = Client::new()
        .get(s.u(&format!("/api/v1/users/{a1}")))
        .basic_auth(b_id, Some(b["app_secret"].as_str().unwrap()))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 404);
    let r = Client::new()
        .get(s.u(&format!("/api/v1/users/{a1}")))
        .basic_auth(a_id, Some("nope"))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 401);

    // A user.created webhook event was recorded per app user.
    let (n,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM webhook_events WHERE event = 'user.created'")
        .fetch_one(&s.state.db)
        .await
        .unwrap();
    assert_eq!(n, 3);
}

// ---- sessions, methods, CSRF ---------------------------------------------------------------

#[tokio::test]
async fn session_refresh_and_logout() {
    let s = spawn(|_| {}).await;
    let c = s.browser();
    s.email_login(&c, "paul@example.com").await;

    let r = c.post(s.u("/v1/session/refresh")).send().await.unwrap();
    assert_eq!(r.status(), 200);
    let cookies: Vec<String> = r
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .map(|v| v.to_str().unwrap().to_string())
        .collect();
    assert_eq!(cookies.len(), 2);
    assert!(
        cookies
            .iter()
            .all(|c| c.contains("HttpOnly") && c.contains("SameSite=Lax") && c.contains("Path=/"))
    );
    let sess: Value = c.get(s.u("/v1/session")).send().await.unwrap().json().await.unwrap();
    assert_eq!(sess["authenticated"], true);

    let r = c.post(s.u("/v1/session/logout")).send().await.unwrap();
    assert_eq!(r.status(), 200);
    let sess: Value = c.get(s.u("/v1/session")).send().await.unwrap().json().await.unwrap();
    assert_eq!(sess["authenticated"], false);
    let r = c.post(s.u("/v1/session/refresh")).send().await.unwrap();
    assert_eq!(r.status(), 401);
}

#[tokio::test]
async fn secure_cookies_use_host_prefix() {
    let s = spawn(|c| c.cookie_secure = true).await;
    let r = s
        .browser()
        .post(s.u("/v1/email/start"))
        .json(&json!({ "email": "q@example.com" }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);
    let code = s.otp.code_for("q@example.com");
    let r = s
        .browser()
        .post(s.u("/v1/email/verify"))
        .json(&json!({ "email": "q@example.com", "code": code }))
        .send()
        .await
        .unwrap();
    let cookies: Vec<String> = r
        .headers()
        .get_all(header::SET_COOKIE)
        .iter()
        .map(|v| v.to_str().unwrap().to_string())
        .collect();
    assert!(
        cookies
            .iter()
            .all(|c| c.starts_with("__Host-") && c.contains("; Secure")),
        "{cookies:?}"
    );
}

#[tokio::test]
async fn origin_guard_blocks_cross_site_posts() {
    let s = spawn(|_| {}).await;
    let no_origin = Client::new();
    let r = no_origin
        .post(s.u("/v1/email/start"))
        .json(&json!({ "email": "r@example.com" }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 403);
    let r = no_origin
        .post(s.u("/v1/email/start"))
        .header(header::ORIGIN, "https://evil.example")
        .json(&json!({ "email": "r@example.com" }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 403);
    assert!(!r.headers().contains_key(header::ACCESS_CONTROL_ALLOW_ORIGIN));
}

#[tokio::test]
async fn methods_link_and_unlink() {
    let m = spawn_mock_issuer().await;
    let s = oidc_server(&m, |_| {}).await;
    let c = s.browser();
    let csrf = s.email_login(&c, "sam@example.com").await;
    let methods: Value = c.get(s.u("/v1/me/methods")).send().await.unwrap().json().await.unwrap();
    let email_id = methods["methods"][0]["id"].as_str().unwrap().to_string();

    // Can't remove the last method.
    let r = c
        .delete(s.u(&format!("/v1/me/methods/{email_id}")))
        .header("x-lowki-csrf", &csrf)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 409);

    // Link Google through the popup flow.
    let html = google_login(&s, &c, true).await;
    assert!(html.contains("\"linked\":true"), "{html}");
    let methods: Value = c.get(s.u("/v1/me/methods")).send().await.unwrap().json().await.unwrap();
    assert_eq!(methods["methods"].as_array().unwrap().len(), 2);

    // Bind the share to email (the newest social sign-in is Google, so sign in by email again).
    c.post(s.u("/v1/email/start"))
        .json(&json!({ "email": "sam@example.com" }))
        .send()
        .await
        .unwrap();
    let code = s.otp.code_for("sam@example.com");
    c.post(s.u("/v1/email/verify"))
        .json(&json!({ "email": "sam@example.com", "code": code }))
        .send()
        .await
        .unwrap();
    let csrf = s.csrf(&c).await;
    let r: Value = c
        .put(s.u("/v1/me/share"))
        .header("x-lowki-csrf", &csrf)
        .json(&json!({ "share": "AQID" }))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(r["bound_method_id"], email_id.as_str());
    // The bound method can't be removed; the other one can.
    let r = c
        .delete(s.u(&format!("/v1/me/methods/{email_id}")))
        .header("x-lowki-csrf", &csrf)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 409);
    let google_id = methods["methods"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["kind"] == "oidc")
        .unwrap()["id"]
        .as_str()
        .unwrap()
        .to_string();
    let r = c
        .delete(s.u(&format!("/v1/me/methods/{google_id}")))
        .header("x-lowki-csrf", &csrf)
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 200);

    // Linking a Google account that belongs to someone else fails.
    let other = s.browser();
    google_login(&s, &other, false).await; // creates a new user with that Google sub
    let html = google_login(&s, &c, true).await;
    assert!(html.contains("already_linked"), "{html}");
}

#[tokio::test]
async fn admin_api_requires_token() {
    let s = spawn(|_| {}).await;
    let r = Client::new().get(s.u("/admin/v1/apps")).send().await.unwrap();
    assert_eq!(r.status(), 401);
    let r = Client::new()
        .get(s.u("/admin/v1/apps"))
        .bearer_auth("wrong-token-wrong-token")
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 401);
    let r = Client::new()
        .post(s.u("/admin/v1/apps"))
        .bearer_auth(ADMIN)
        .json(&json!({ "name": "x", "allowed_origins": ["https://a.example/path"] }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 400);
    let app = s.create_app(&["https://A.example:8443"]).await;
    assert_eq!(app["allowed_origins"], json!(["https://a.example:8443"]));
    assert!(app["app_secret"].as_str().unwrap().starts_with("lsk_"));
    let list: Value = Client::new()
        .get(s.u("/admin/v1/apps"))
        .bearer_auth(ADMIN)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(!list.to_string().contains("lsk_"), "secrets are never listed");
    let audit: Value = Client::new()
        .get(s.u("/admin/v1/audit"))
        .bearer_auth(ADMIN)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(
        audit["events"]
            .as_array()
            .unwrap()
            .iter()
            .any(|e| e["event"] == "app.created")
    );
}

#[tokio::test]
async fn passkey_endpoints_include_prf_and_guard_blob() {
    let s = spawn(|_| {}).await;
    let c = s.browser();
    let r: Value = c
        .post(s.u("/v1/passkeys/register/start"))
        .json(&json!({}))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let pk = &r["options"]["publicKey"];
    assert_eq!(pk["extensions"]["prf"], json!({}));
    assert_eq!(pk["authenticatorSelection"]["residentKey"], "required");
    assert_eq!(pk["authenticatorSelection"]["userVerification"], "required");
    assert_eq!(pk["rp"]["id"], "localhost");

    let r: Value = c
        .post(s.u("/v1/passkeys/login/start"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert!(r["options"].get("mediation").is_none());
    assert_eq!(r["options"]["publicKey"]["userVerification"], "required");

    // Blob access without a passkey sign-in is refused.
    let csrf = s.email_login(&c, "tom@example.com").await;
    let r = c.get(s.u("/v1/passkeys/abc/blob")).send().await.unwrap();
    assert_eq!(r.status(), 404);
    let r = c
        .put(s.u("/v1/passkeys/abc/blob"))
        .header("x-lowki-csrf", &csrf)
        .json(&json!({ "blob": "AQ" }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 404);

    // A bogus assertion is rejected.
    let r = c
        .post(s.u("/v1/passkeys/login/finish"))
        .json(&json!({ "flow_id": "nope", "credential": {} }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 400);
}
