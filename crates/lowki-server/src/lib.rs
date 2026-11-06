//! lowki-server: self-hostable auth, user and share-storage service for Lowki.
//!
//! The server never sees keys. It stores credential public keys, HMAC'd identity subjects,
//! opaque passkey-wrapped blobs and one Shamir share that is useless on its own.

pub mod apps;
pub mod audit;
pub mod config;
pub mod crypto;
pub mod db;
pub mod email;
pub mod error;
pub mod oidc;
pub mod passkey;
pub mod ratelimit;
pub mod session;
pub mod share;
pub mod tokens;
pub mod users;

use std::{
    collections::HashSet,
    net::SocketAddr,
    sync::{Arc, RwLock},
    time::Duration,
};

use axum::{
    Json, Router,
    extract::{ConnectInfo, DefaultBodyLimit, FromRequestParts},
    http::{HeaderName, HeaderValue, Method, Request, header, request::Parts},
    middleware,
    response::Response,
    routing::{delete, get, post},
};
use serde_json::{Value, json};
use tower_http::{
    cors::{AllowOrigin, Any, CorsLayer},
    trace::TraceLayer,
};
use webauthn_rs::{Webauthn, WebauthnBuilder};

use crate::{
    config::Config,
    crypto::Keys,
    email::OtpSender,
    ratelimit::RateLimiter,
    session::{NewSession, with_cookies},
    tokens::KeyStore,
    users::Resolved,
};

pub struct Inner {
    pub config: Config,
    pub db: db::Db,
    pub keys: Keys,
    pub webauthn: Webauthn,
    pub limiter: RateLimiter,
    pub otp_sender: Arc<dyn OtpSender>,
    pub oidc: Option<oidc::Provider>,
    pub jwt: KeyStore,
    pub app_origins: RwLock<HashSet<String>>,
    pub http: reqwest::Client,
}

pub type AppState = Arc<Inner>;

/// Client IP for rate limiting. Uses `X-Forwarded-For` only with `LOWKI_TRUST_PROXY_HEADERS=true`.
pub struct ClientIp(pub String);

impl FromRequestParts<AppState> for ClientIp {
    type Rejection = std::convert::Infallible;
    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        if state.config.trust_proxy_headers
            && let Some(ip) = parts
                .headers
                .get("x-forwarded-for")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| v.split(',').next())
                .map(|v| v.trim().to_string())
                .filter(|v| !v.is_empty())
        {
            return Ok(ClientIp(ip));
        }
        let ip = parts
            .extensions
            .get::<ConnectInfo<SocketAddr>>()
            .map(|c| c.0.ip().to_string())
            .unwrap_or_else(|| "unknown".into());
        Ok(ClientIp(ip))
    }
}

/// Common JSON body for every successful sign-in.
pub fn login_response(ns: NewSession, r: &Resolved, extra: Value) -> Response {
    let mut body = json!({
        "ok": true,
        "new_user": r.new_user,
        "linked": r.linked,
        "method_id": r.identity_id,
        "csrf_token": ns.csrf_token,
        "expires_at": ns.expires_at,
    });
    if let (Some(b), Some(e)) = (body.as_object_mut(), extra.as_object()) {
        for (k, v) in e {
            b.insert(k.clone(), v.clone());
        }
    }
    with_cookies(Json(body), ns.cookies)
}

/// Builds shared state. `otp_sender` overrides the sender from config (tests capture codes).
pub async fn build_state(config: Config, otp_sender: Option<Arc<dyn OtpSender>>) -> anyhow::Result<AppState> {
    let db = db::connect(config.database_url.expose()).await?;
    let keys = Keys::derive(config.server_secret.expose());
    let primary = config
        .lowki_origins
        .first()
        .cloned()
        .unwrap_or_else(|| config.public_url.clone());
    let mut wb = WebauthnBuilder::new(&config.rp_id, &primary)?.rp_name(&config.rp_name);
    for o in config.lowki_origins.iter().skip(1) {
        wb = wb.append_allowed_origin(o);
    }
    if config.dev_mode {
        wb = wb.allow_any_port(true);
    }
    let webauthn = wb.build()?;
    let otp_sender = match otp_sender {
        Some(s) => s,
        None => email::sender_from_config(&config.email_sender)?,
    };
    let oidc = config.oidc.clone().map(|c| oidc::Provider::new(c, &config.public_url));
    let http = reqwest::Client::builder()
        .timeout(Duration::from_secs(10))
        .redirect(reqwest::redirect::Policy::none())
        .build()?;
    let state = Arc::new(Inner {
        config,
        db,
        keys,
        webauthn,
        limiter: RateLimiter::default(),
        otp_sender,
        oidc,
        jwt: KeyStore::default(),
        app_origins: RwLock::new(HashSet::new()),
        http,
    });
    apps::refresh_origin_cache(&state)
        .await
        .map_err(|e| anyhow::anyhow!("loading apps: {}", e.message))?;
    state
        .jwt
        .jwks(&state)
        .await
        .map_err(|e| anyhow::anyhow!("loading signing keys: {}", e.message))?;
    Ok(state)
}

pub fn router(state: AppState) -> Router {
    let lowki_api = Router::new()
        .route("/v1/session", get(session::get_session))
        .route("/v1/session/refresh", post(session::refresh))
        .route("/v1/session/logout", post(session::logout))
        .route("/v1/passkeys/register/start", post(passkey::register_start))
        .route("/v1/passkeys/register/finish", post(passkey::register_finish))
        .route("/v1/passkeys/login/start", post(passkey::login_start))
        .route("/v1/passkeys/login/finish", post(passkey::login_finish))
        .route(
            "/v1/passkeys/{cred_id}/blob",
            get(passkey::get_blob).put(passkey::put_blob),
        )
        .route("/v1/oauth/{provider}/start", get(oidc::start))
        .route("/v1/oauth/{provider}/callback", get(oidc::callback))
        .route("/v1/email/start", post(email::start))
        .route("/v1/email/verify", post(email::verify))
        .route("/v1/me/methods", get(users::get_methods))
        .route("/v1/me/methods/{method_id}", delete(users::unlink_method))
        .route("/v1/me/share", get(share::share_status).put(share::put_share))
        .route("/v1/me/share/release", post(share::release_share))
        .route("/v1/me/share/recovery", delete(share::cancel_recovery))
        .layer(middleware::from_fn_with_state(state.clone(), session::origin_guard));

    let lowki_origins: Vec<String> = state.config.lowki_origins.iter().map(config::origin_of).collect();
    let cors_state = state.clone();
    let token_cors = CorsLayer::new()
        .allow_origin(AllowOrigin::predicate(move |origin: &HeaderValue, _| {
            let Ok(o) = origin.to_str() else { return false };
            lowki_origins.iter().any(|l| l == o)
                || cors_state.app_origins.read().map(|s| s.contains(o)).unwrap_or(false)
        }))
        .allow_credentials(true)
        .allow_methods([Method::POST])
        .allow_headers([header::CONTENT_TYPE, HeaderName::from_static(session::CSRF_HEADER)])
        .max_age(Duration::from_secs(600));
    let token_api = Router::new()
        .route("/v1/apps/{app_id}/token", post(tokens::issue_token))
        .layer(token_cors);

    let public = Router::new()
        .route("/.well-known/jwks.json", get(tokens::jwks))
        .route("/.well-known/openid-configuration", get(tokens::openid_configuration))
        .layer(CorsLayer::new().allow_origin(Any).allow_methods([Method::GET]));

    let admin = Router::new()
        .route("/admin/v1/apps", post(apps::create_app).get(apps::list_apps))
        .route(
            "/admin/v1/apps/{app_id}",
            get(apps::admin_get_app).patch(apps::update_app),
        )
        .route("/admin/v1/apps/{app_id}/users", get(apps::list_app_users))
        .route("/admin/v1/keys/rotate", post(apps::rotate_keys))
        .route("/admin/v1/audit", get(apps::list_audit))
        .route("/api/v1/users/{user_id}", get(apps::app_get_user));

    Router::new()
        .route("/healthz", get(|| async { Json(json!({ "ok": true })) }))
        .merge(lowki_api)
        .merge(token_api)
        .merge(public)
        .merge(admin)
        .layer(DefaultBodyLimit::max(64 * 1024))
        .layer(middleware::from_fn(security_headers))
        // Log method and path only: query strings can carry OAuth codes and state.
        .layer(TraceLayer::new_for_http().make_span_with(
            |req: &Request<_>| tracing::info_span!("http", method = %req.method(), path = %req.uri().path()),
        ))
        .with_state(state)
}

async fn security_headers(req: axum::extract::Request, next: middleware::Next) -> Response {
    let mut resp = next.run(req).await;
    let h = resp.headers_mut();
    h.insert(header::X_CONTENT_TYPE_OPTIONS, HeaderValue::from_static("nosniff"));
    h.entry(header::REFERRER_POLICY)
        .or_insert(HeaderValue::from_static("no-referrer"));
    resp
}

/// Deletes expired sessions, codes and ceremonies every few minutes.
pub fn spawn_janitor(state: AppState) {
    tokio::spawn(async move {
        let mut tick = tokio::time::interval(Duration::from_secs(300));
        loop {
            tick.tick().await;
            let t = db::now();
            for sql in [
                "DELETE FROM sessions WHERE refresh_expires_at < $1",
                "DELETE FROM otp_codes WHERE expires_at < $1",
                "DELETE FROM webauthn_flows WHERE expires_at < $1",
                "DELETE FROM oauth_flows WHERE expires_at < $1",
            ] {
                if let Err(e) = sqlx::query(sql).bind(t).execute(&state.db).await {
                    tracing::warn!(error = %e, "janitor query failed");
                }
            }
        }
    });
}

/// Binds and serves until ctrl-c / SIGTERM.
pub async fn serve(state: AppState, listener: tokio::net::TcpListener) -> anyhow::Result<()> {
    let app = router(state);
    axum::serve(listener, app.into_make_service_with_connect_info::<SocketAddr>())
        .with_graceful_shutdown(async {
            let ctrl_c = tokio::signal::ctrl_c();
            #[cfg(unix)]
            {
                let mut term =
                    tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate()).expect("signal handler");
                tokio::select! { _ = ctrl_c => {}, _ = term.recv() => {} }
            }
            #[cfg(not(unix))]
            let _ = ctrl_c.await;
        })
        .await?;
    Ok(())
}
