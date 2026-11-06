//! Cookie sessions on the Lowki origin.
//!
//! * The session cookie holds a random 256-bit token; the database stores only its SHA-256.
//! * A separate refresh cookie (also hashed at rest) rotates both tokens.
//! * Each session records every sign-in performed in it (`auths`), so endpoints can demand a
//!   *fresh* sign-in with a specific method (the server share needs the bound social/email
//!   method, the wrapped blob needs that passkey).
//! * CSRF: state-changing requests must carry an `Origin` that is a Lowki origin (enforced by
//!   `origin_guard`) *and* an `x-lowki-csrf` header equal to `HMAC(csrf_key, session_token)`.

use axum::{
    Json,
    extract::{FromRequestParts, Request, State},
    http::{HeaderMap, HeaderValue, Method, header, request::Parts},
    middleware::Next,
    response::{IntoResponse, Response},
};
use serde::{Deserialize, Serialize};
use serde_json::json;

use crate::{
    AppState,
    crypto::{b64, ct_eq_str, hmac, random_token, sha256_hex},
    db::now,
    error::{ApiError, ApiResult},
    users,
};

pub const CSRF_HEADER: &str = "x-lowki-csrf";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AuthEvent {
    pub identity_id: String,
    pub kind: String,
    pub at: i64,
}

#[derive(Clone, Debug)]
pub struct Session {
    pub id_hash: String,
    pub user_id: String,
    pub auths: Vec<AuthEvent>,
    pub expires_at: i64,
    token: String,
}

impl Session {
    pub fn csrf_token(&self, state: &AppState) -> String {
        csrf_for(state, &self.token)
    }

    /// The newest sign-in with `identity_id` if it happened within `max_age`.
    pub fn fresh_auth_with(&self, identity_id: &str, max_age: std::time::Duration) -> Option<&AuthEvent> {
        let cutoff = now() - max_age.as_secs() as i64;
        self.auths
            .iter()
            .rev()
            .find(|a| a.identity_id == identity_id && a.at >= cutoff)
    }

    pub fn any_fresh_auth(&self, max_age: std::time::Duration) -> bool {
        let cutoff = now() - max_age.as_secs() as i64;
        self.auths.iter().any(|a| a.at >= cutoff)
    }
}

fn csrf_for(state: &AppState, token: &str) -> String {
    b64(&hmac(&state.keys.csrf, &[token.as_bytes()])[..24])
}

pub fn cookie_name(state: &AppState, base: &str) -> String {
    if state.config.cookie_secure {
        format!("__Host-{base}")
    } else {
        base.to_string()
    }
}

pub fn read_cookie(headers: &HeaderMap, name: &str) -> Option<String> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .filter_map(|kv| kv.trim().split_once('='))
        .find(|(k, _)| *k == name)
        .map(|(_, v)| v.to_string())
}

/// Builds a Set-Cookie value. `same_site` overrides the configured policy (OAuth binding cookies
/// must be Lax so they survive the redirect back from the identity provider).
pub fn build_cookie(state: &AppState, base: &str, value: &str, max_age: i64, same_site: Option<&str>) -> String {
    let cfg = &state.config;
    let mut c = format!(
        "{}={}; Path=/; HttpOnly; SameSite={}; Max-Age={}",
        cookie_name(state, base),
        value,
        same_site.unwrap_or(cfg.cookie_same_site.as_str()),
        max_age.max(0)
    );
    if cfg.cookie_secure {
        c.push_str("; Secure");
        if cfg.cookie_partitioned {
            c.push_str("; Partitioned");
        }
    }
    c
}

pub fn with_cookies(resp: impl IntoResponse, cookies: Vec<String>) -> Response {
    let mut resp = resp.into_response();
    for c in cookies {
        if let Ok(v) = HeaderValue::from_str(&c) {
            resp.headers_mut().append(header::SET_COOKIE, v);
        }
    }
    resp.headers_mut()
        .insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    resp
}

const SID: &str = "lowki_sid";
const RT: &str = "lowki_rt";

async fn load_by_token(state: &AppState, token: &str) -> ApiResult<Option<Session>> {
    let row: Option<(String, String, String, i64)> = sqlx::query_as(
        "SELECT id_hash, user_id, auths, expires_at FROM sessions WHERE id_hash = $1 AND expires_at > $2",
    )
    .bind(sha256_hex(token.as_bytes()))
    .bind(now())
    .fetch_optional(&state.db)
    .await?;
    Ok(row.map(|(id_hash, user_id, auths, expires_at)| Session {
        id_hash,
        user_id,
        auths: serde_json::from_str(&auths).unwrap_or_default(),
        expires_at,
        token: token.to_string(),
    }))
}

/// Optional session (no CSRF check). Use for GETs and for sign-in endpoints that may link.
pub struct MaybeSession(pub Option<Session>);

impl FromRequestParts<AppState> for MaybeSession {
    type Rejection = ApiError;
    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        let Some(token) = read_cookie(&parts.headers, &cookie_name(state, SID)) else {
            return Ok(MaybeSession(None));
        };
        Ok(MaybeSession(load_by_token(state, &token).await?))
    }
}

/// Required session; for non-safe methods the CSRF header must match.
pub struct AuthSession(pub Session);

impl FromRequestParts<AppState> for AuthSession {
    type Rejection = ApiError;
    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        let MaybeSession(s) = MaybeSession::from_request_parts(parts, state).await?;
        let s = s.ok_or_else(|| ApiError::unauthorized("no session"))?;
        if !is_safe(&parts.method) {
            check_csrf(state, &parts.headers, &s)?;
        }
        Ok(AuthSession(s))
    }
}

pub fn check_csrf(state: &AppState, headers: &HeaderMap, s: &Session) -> ApiResult<()> {
    let got = headers.get(CSRF_HEADER).and_then(|v| v.to_str().ok()).unwrap_or("");
    if got.is_empty() || !ct_eq_str(got, &s.csrf_token(state)) {
        return Err(ApiError::forbidden("missing or invalid CSRF token"));
    }
    Ok(())
}

fn is_safe(m: &Method) -> bool {
    matches!(*m, Method::GET | Method::HEAD | Method::OPTIONS)
}

/// Middleware for the cookie API: browsers always send `Origin` on non-GET fetches, so a
/// missing or foreign `Origin` on a state-changing request is rejected. Cross-origin reads are
/// already blocked because this router has no CORS layer.
pub async fn origin_guard(State(state): State<AppState>, req: Request, next: Next) -> Response {
    let origin = req.headers().get(header::ORIGIN).and_then(|v| v.to_str().ok());
    let ok = match origin {
        Some(o) => state.config.is_lowki_origin(o),
        None => is_safe(req.method()),
    };
    if !ok {
        return ApiError::forbidden("request origin is not allowed").into_response();
    }
    next.run(req).await
}

pub struct NewSession {
    pub csrf_token: String,
    pub cookies: Vec<String>,
    pub expires_at: i64,
}

/// Records a sign-in. If the browser already has a session for the same user, the sign-in is
/// added to it (step-up) and the tokens rotate; otherwise a fresh session replaces it.
pub async fn establish(
    state: &AppState,
    current: Option<&Session>,
    user_id: &str,
    identity_id: &str,
    kind: &str,
) -> ApiResult<NewSession> {
    let t = now();
    let mut auths = match current {
        Some(s) if s.user_id == user_id => s.auths.clone(),
        _ => Vec::new(),
    };
    auths.push(AuthEvent {
        identity_id: identity_id.into(),
        kind: kind.into(),
        at: t,
    });
    let keep_after = t - state.config.refresh_ttl.as_secs() as i64;
    auths.retain(|a| a.at >= keep_after);
    if auths.len() > 16 {
        auths.drain(..auths.len() - 16);
    }
    if let Some(s) = current {
        sqlx::query("DELETE FROM sessions WHERE id_hash = $1")
            .bind(&s.id_hash)
            .execute(&state.db)
            .await?;
    }
    issue(state, user_id, &auths, t).await
}

async fn issue(state: &AppState, user_id: &str, auths: &[AuthEvent], created_at: i64) -> ApiResult<NewSession> {
    let token = random_token();
    let refresh = random_token();
    let t = now();
    let expires_at = t + state.config.session_ttl.as_secs() as i64;
    let refresh_expires_at = t + state.config.refresh_ttl.as_secs() as i64;
    sqlx::query(
        "INSERT INTO sessions (id_hash, refresh_hash, user_id, auths, created_at, expires_at, refresh_expires_at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7)",
    )
    .bind(sha256_hex(token.as_bytes()))
    .bind(sha256_hex(refresh.as_bytes()))
    .bind(user_id)
    .bind(serde_json::to_string(auths).map_err(anyhow::Error::from)?)
    .bind(created_at)
    .bind(expires_at)
    .bind(refresh_expires_at)
    .execute(&state.db)
    .await?;
    Ok(NewSession {
        csrf_token: csrf_for(state, &token),
        cookies: vec![
            build_cookie(state, SID, &token, state.config.session_ttl.as_secs() as i64, None),
            build_cookie(state, RT, &refresh, state.config.refresh_ttl.as_secs() as i64, None),
        ],
        expires_at,
    })
}

fn clear_cookies(state: &AppState) -> Vec<String> {
    vec![
        build_cookie(state, SID, "", 0, None),
        build_cookie(state, RT, "", 0, None),
    ]
}

// ---- handlers ------------------------------------------------------------------------------

pub async fn get_session(State(state): State<AppState>, MaybeSession(s): MaybeSession) -> ApiResult<Response> {
    let Some(s) = s else {
        return Ok(with_cookies(Json(json!({ "authenticated": false })), vec![]));
    };
    let methods = users::list_methods(&state, &s.user_id).await?;
    let has_share: Option<(String,)> = sqlx::query_as("SELECT bound_identity_id FROM shares WHERE user_id = $1")
        .bind(&s.user_id)
        .fetch_optional(&state.db)
        .await?;
    Ok(with_cookies(
        Json(json!({
            "authenticated": true,
            "csrf_token": s.csrf_token(&state),
            "expires_at": s.expires_at,
            "methods": methods,
            "auths": s.auths.iter().map(|a| json!({"method_id": a.identity_id, "kind": a.kind, "at": a.at})).collect::<Vec<_>>(),
            "share": has_share.map(|(b,)| json!({"bound_method_id": b})),
        })),
        vec![],
    ))
}

pub async fn refresh(State(state): State<AppState>, headers: HeaderMap) -> ApiResult<Response> {
    let rt =
        read_cookie(&headers, &cookie_name(&state, RT)).ok_or_else(|| ApiError::unauthorized("no refresh token"))?;
    let row: Option<(String, String, String, i64)> = sqlx::query_as(
        "SELECT id_hash, user_id, auths, created_at FROM sessions WHERE refresh_hash = $1 AND refresh_expires_at > $2",
    )
    .bind(sha256_hex(rt.as_bytes()))
    .bind(now())
    .fetch_optional(&state.db)
    .await?;
    let Some((id_hash, user_id, auths, created_at)) = row else {
        return Ok(with_cookies(
            ApiError::unauthorized("refresh token expired"),
            clear_cookies(&state),
        ));
    };
    // Single use: delete before issuing so a replayed refresh token fails.
    let deleted = sqlx::query("DELETE FROM sessions WHERE id_hash = $1")
        .bind(&id_hash)
        .execute(&state.db)
        .await?;
    if deleted.rows_affected() != 1 {
        return Err(ApiError::unauthorized("refresh token already used"));
    }
    let auths: Vec<AuthEvent> = serde_json::from_str(&auths).unwrap_or_default();
    let ns = issue(&state, &user_id, &auths, created_at).await?;
    Ok(with_cookies(
        Json(json!({ "csrf_token": ns.csrf_token, "expires_at": ns.expires_at })),
        ns.cookies,
    ))
}

pub async fn logout(State(state): State<AppState>, headers: HeaderMap) -> ApiResult<Response> {
    if let Some(t) = read_cookie(&headers, &cookie_name(&state, SID)) {
        sqlx::query("DELETE FROM sessions WHERE id_hash = $1")
            .bind(sha256_hex(t.as_bytes()))
            .execute(&state.db)
            .await?;
    }
    if let Some(t) = read_cookie(&headers, &cookie_name(&state, RT)) {
        sqlx::query("DELETE FROM sessions WHERE refresh_hash = $1")
            .bind(sha256_hex(t.as_bytes()))
            .execute(&state.db)
            .await?;
    }
    Ok(with_cookies(Json(json!({ "ok": true })), clear_cookies(&state)))
}
