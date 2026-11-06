//! Test harness: an in-process lowki-server, a capturing OTP sender and a mock OIDC issuer.
#![allow(dead_code)]

use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{Arc, Mutex},
};

use async_trait::async_trait;
use axum::{
    Form, Json, Router,
    extract::{Query, State},
    http::StatusCode,
    response::{IntoResponse, Redirect, Response},
    routing::{get, post},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use lowki_server::{AppState, build_state, config::Config, email::OtpSender, serve};
use reqwest::{Client, header};
use rsa::{
    RsaPrivateKey,
    pkcs1::{EncodeRsaPrivateKey, LineEnding},
    traits::PublicKeyParts,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

pub const ADMIN: &str = "test-admin-token";

#[derive(Default)]
pub struct CaptureSender {
    pub codes: Mutex<HashMap<String, String>>,
}

#[async_trait]
impl OtpSender for CaptureSender {
    async fn send(&self, to: &str, code: &str) -> anyhow::Result<()> {
        self.codes.lock().unwrap().insert(to.to_string(), code.to_string());
        Ok(())
    }
}

impl CaptureSender {
    pub fn code_for(&self, email: &str) -> String {
        self.codes.lock().unwrap().get(email).cloned().expect("a code was sent")
    }
}

pub struct TestServer {
    pub url: String,
    pub state: AppState,
    pub otp: Arc<CaptureSender>,
}

/// SQLite temp file by default. With `LOWKI_TEST_DATABASE_URL=postgres://.../db`, each test
/// gets its own fresh Postgres database.
async fn db_url() -> String {
    match std::env::var("LOWKI_TEST_DATABASE_URL") {
        Ok(u) if !u.is_empty() => {
            use sqlx::Connection;
            sqlx::any::install_default_drivers();
            let mut admin = sqlx::AnyConnection::connect(&u).await.expect("postgres");
            let name = format!("lowki_t_{}", uuid::Uuid::new_v4().simple());
            sqlx::query(&format!("CREATE DATABASE {name}"))
                .execute(&mut admin)
                .await
                .unwrap();
            let mut url = url::Url::parse(&u).unwrap();
            url.set_path(&name);
            url.to_string()
        }
        _ => {
            let p = std::env::temp_dir().join(format!("lowki-test-{}.db", uuid::Uuid::new_v4()));
            format!("sqlite://{}?mode=rwc", p.display())
        }
    }
}

pub async fn spawn(customize: impl FnOnce(&mut Config)) -> TestServer {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let url = format!("http://localhost:{}", addr.port());
    let mut config = Config::for_tests(&url, &db_url().await);
    customize(&mut config);
    let otp = Arc::new(CaptureSender::default());
    let state = build_state(config, Some(otp.clone())).await.expect("server state");
    tokio::spawn(serve(state.clone(), listener));
    TestServer { url, state, otp }
}

impl TestServer {
    /// A browser-like client: keeps cookies, sends the Lowki Origin, doesn't follow redirects.
    pub fn browser(&self) -> Client {
        let mut h = header::HeaderMap::new();
        h.insert(header::ORIGIN, self.url.parse().unwrap());
        Client::builder()
            .cookie_store(true)
            .default_headers(h)
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .unwrap()
    }

    pub fn u(&self, path: &str) -> String {
        format!("{}{}", self.url, path)
    }

    /// Email OTP sign-in; returns the CSRF token.
    pub async fn email_login(&self, c: &Client, email: &str) -> String {
        let r = c
            .post(self.u("/v1/email/start"))
            .json(&json!({ "email": email }))
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 200, "{}", r.text().await.unwrap());
        let code = self.otp.code_for(email);
        let r = c
            .post(self.u("/v1/email/verify"))
            .json(&json!({ "email": email, "code": code }))
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 200);
        let body: Value = r.json().await.unwrap();
        body["csrf_token"].as_str().unwrap().to_string()
    }

    pub async fn csrf(&self, c: &Client) -> String {
        let s: Value = c.get(self.u("/v1/session")).send().await.unwrap().json().await.unwrap();
        s["csrf_token"].as_str().unwrap().to_string()
    }

    pub async fn create_app(&self, origins: &[&str]) -> Value {
        let r = Client::new()
            .post(self.u("/admin/v1/apps"))
            .bearer_auth(ADMIN)
            .json(&json!({ "name": "Test app", "allowed_origins": origins }))
            .send()
            .await
            .unwrap();
        assert_eq!(r.status(), 200);
        r.json().await.unwrap()
    }

    /// Requests an app token directly from the app origin (CORS path).
    pub async fn app_token(&self, c: &Client, app_id: &str, app_origin: &str) -> reqwest::Response {
        c.post(self.u(&format!("/v1/apps/{app_id}/token")))
            .header(header::ORIGIN, app_origin)
            .json(&json!({}))
            .send()
            .await
            .unwrap()
    }
}

// ---- mock OIDC issuer ----------------------------------------------------------------------

#[derive(Clone, Default)]
pub struct MockBehaviour {
    pub sub: String,
    pub email: Option<String>,
    pub wrong_nonce: bool,
    pub wrong_aud: bool,
    pub wrong_iss: bool,
    pub expired: bool,
    pub unknown_kid: bool,
}

struct Pending {
    redirect_uri: String,
    nonce: String,
    challenge: String,
    client_id: String,
}

pub struct MockIssuerInner {
    pub issuer: String,
    pub client_id: String,
    pub client_secret: String,
    key_pem: String,
    n: String,
    e: String,
    pub behaviour: Mutex<MockBehaviour>,
    codes: Mutex<HashMap<String, Pending>>,
    pub token_requests: Mutex<u32>,
}

pub type MockIssuer = Arc<MockIssuerInner>;

pub async fn spawn_mock_issuer() -> MockIssuer {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr: SocketAddr = listener.local_addr().unwrap();
    let key = RsaPrivateKey::new(&mut rand::rngs::OsRng, 2048).unwrap();
    let inner = Arc::new(MockIssuerInner {
        issuer: format!("http://127.0.0.1:{}", addr.port()),
        client_id: "lowki-test-client.apps.example".into(),
        client_secret: format!("mock-secret-{}", uuid::Uuid::new_v4()),
        key_pem: key.to_pkcs1_pem(LineEnding::LF).unwrap().to_string(),
        n: URL_SAFE_NO_PAD.encode(key.n().to_bytes_be()),
        e: URL_SAFE_NO_PAD.encode(key.e().to_bytes_be()),
        behaviour: Mutex::new(MockBehaviour {
            sub: "1234567890".into(),
            ..Default::default()
        }),
        codes: Mutex::new(HashMap::new()),
        token_requests: Mutex::new(0),
    });
    let app = Router::new()
        .route("/.well-known/openid-configuration", get(discovery))
        .route("/authorize", get(authorize))
        .route("/token", post(token))
        .route("/jwks", get(jwks))
        .with_state(inner.clone());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    inner
}

async fn discovery(State(m): State<MockIssuer>) -> Json<Value> {
    Json(json!({
        "issuer": m.issuer,
        "authorization_endpoint": format!("{}/authorize", m.issuer),
        "token_endpoint": format!("{}/token", m.issuer),
        "jwks_uri": format!("{}/jwks", m.issuer),
        "response_types_supported": ["code"],
        "id_token_signing_alg_values_supported": ["RS256"],
    }))
}

async fn jwks(State(m): State<MockIssuer>) -> Json<Value> {
    Json(json!({ "keys": [{ "kty": "RSA", "alg": "RS256", "use": "sig", "kid": "mock-1", "n": m.n, "e": m.e }] }))
}

/// Simulates the user consenting: immediately redirects back with a code.
async fn authorize(State(m): State<MockIssuer>, Query(q): Query<HashMap<String, String>>) -> Response {
    assert_eq!(q.get("response_type").map(String::as_str), Some("code"));
    assert_eq!(q.get("code_challenge_method").map(String::as_str), Some("S256"));
    assert!(q.get("scope").unwrap().contains("openid"));
    let code = uuid::Uuid::new_v4().to_string();
    m.codes.lock().unwrap().insert(
        code.clone(),
        Pending {
            redirect_uri: q["redirect_uri"].clone(),
            nonce: q["nonce"].clone(),
            challenge: q["code_challenge"].clone(),
            client_id: q["client_id"].clone(),
        },
    );
    let mut u = url::Url::parse(&q["redirect_uri"]).unwrap();
    u.query_pairs_mut()
        .append_pair("code", &code)
        .append_pair("state", &q["state"]);
    Redirect::to(u.as_str()).into_response()
}

async fn token(State(m): State<MockIssuer>, Form(f): Form<HashMap<String, String>>) -> Response {
    *m.token_requests.lock().unwrap() += 1;
    let bad = |msg: &str| {
        (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "invalid_grant", "error_description": msg })),
        )
            .into_response()
    };
    if f.get("grant_type").map(String::as_str) != Some("authorization_code") {
        return bad("grant_type");
    }
    if f.get("client_id") != Some(&m.client_id) || f.get("client_secret") != Some(&m.client_secret) {
        return (StatusCode::UNAUTHORIZED, Json(json!({ "error": "invalid_client" }))).into_response();
    }
    let Some(p) = m
        .codes
        .lock()
        .unwrap()
        .remove(f.get("code").map(String::as_str).unwrap_or(""))
    else {
        return bad("unknown code");
    };
    if f.get("redirect_uri") != Some(&p.redirect_uri) || p.client_id != m.client_id {
        return bad("redirect_uri");
    }
    let verifier = f.get("code_verifier").cloned().unwrap_or_default();
    if URL_SAFE_NO_PAD.encode(Sha256::digest(verifier.as_bytes())) != p.challenge {
        return bad("pkce");
    }
    let b = m.behaviour.lock().unwrap().clone();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    let claims = json!({
        "iss": if b.wrong_iss { "https://evil.example".to_string() } else { m.issuer.clone() },
        "aud": if b.wrong_aud { "someone-else".to_string() } else { m.client_id.clone() },
        "sub": b.sub,
        "nonce": if b.wrong_nonce { "not-the-nonce".to_string() } else { p.nonce },
        "iat": now,
        "exp": if b.expired { now - 3600 } else { now + 600 },
        "email": b.email,
        "email_verified": true,
    });
    let mut header = jsonwebtoken::Header::new(jsonwebtoken::Algorithm::RS256);
    header.kid = Some(if b.unknown_kid {
        "rotated-away".into()
    } else {
        "mock-1".into()
    });
    let key = jsonwebtoken::EncodingKey::from_rsa_pem(m.key_pem.as_bytes()).unwrap();
    let id_token = jsonwebtoken::encode(&header, &claims, &key).unwrap();
    Json(json!({ "access_token": "mock-access", "token_type": "Bearer", "expires_in": 3600, "id_token": id_token }))
        .into_response()
}

pub fn oidc_config(m: &MockIssuer) -> lowki_server::config::OidcConfig {
    lowki_server::config::OidcConfig {
        provider_id: "google".into(),
        issuer: m.issuer.clone(),
        extra_issuers: vec![],
        client_id: m.client_id.clone(),
        client_secret: lowki_server::config::Secret(m.client_secret.clone()),
        scopes: "openid".into(),
    }
}

/// Runs the popup flow with the given browser client. Returns the callback page HTML.
pub async fn google_login(server: &TestServer, c: &Client, link: bool) -> String {
    let start = if link {
        "/v1/oauth/google/start?link=1"
    } else {
        "/v1/oauth/google/start"
    };
    let r = c.get(server.u(start)).send().await.unwrap();
    assert_eq!(r.status(), 303, "start should redirect");
    let to_idp = r.headers()[header::LOCATION].to_str().unwrap().to_string();
    let r = c.get(&to_idp).send().await.unwrap();
    assert!(r.status().is_redirection());
    let back = r.headers()[header::LOCATION].to_str().unwrap().to_string();
    assert!(back.starts_with(&server.url));
    let r = c.get(&back).send().await.unwrap();
    assert_eq!(r.status(), 200);
    r.text().await.unwrap()
}
