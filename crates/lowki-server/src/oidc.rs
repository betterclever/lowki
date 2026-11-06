//! Google (or any OIDC provider) sign-in: authorization code flow with PKCE, `state` and
//! `nonce`, run from a popup on the Lowki origin.
//!
//! 1. `GET /v1/oauth/{provider}/start` stores `{sha256(state), verifier, nonce}` and a
//!    browser-binding cookie, then redirects to the provider.
//! 2. `GET /v1/oauth/{provider}/callback` checks the state and the binding cookie, exchanges the
//!    code with the client secret, verifies the ID token (JWKS signature, `iss`, `aud`, `exp`,
//!    `nonce`) and stores only `HMAC(server_secret, "<provider>:" || sub)`.
//! 3. The callback page `postMessage`s the result to `window.opener` on the Lowki origin.

use std::{
    sync::RwLock,
    time::{Duration, Instant},
};

use anyhow::{Context, anyhow, bail};
use axum::{
    extract::{Path, Query, State},
    http::{HeaderMap, HeaderValue, StatusCode, header},
    response::{Html, IntoResponse, Redirect, Response},
};
use jsonwebtoken::{Algorithm, DecodingKey, Validation, jwk::JwkSet};
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};

use crate::{
    AppState,
    config::{OidcConfig, origin_of},
    crypto::{b64, ct_eq_str, hmac_hex, random_token, sha256_hex},
    db::now,
    error::{ApiError, ApiResult},
    session::{MaybeSession, build_cookie, cookie_name, establish, read_cookie, with_cookies},
    users,
};

#[derive(Deserialize, Clone, Debug)]
pub struct Metadata {
    pub issuer: String,
    pub authorization_endpoint: String,
    pub token_endpoint: String,
    pub jwks_uri: String,
}

pub struct Provider {
    pub cfg: OidcConfig,
    pub redirect_uri: String,
    meta: RwLock<Option<(Instant, Metadata)>>,
    jwks: RwLock<Option<(Instant, JwkSet)>>,
}

const META_TTL: Duration = Duration::from_secs(3600);
const JWKS_TTL: Duration = Duration::from_secs(3600);
const JWKS_MIN_REFRESH: Duration = Duration::from_secs(30);
const FLOW_TTL: i64 = 600;
const BINDING_COOKIE: &str = "lowki_oauth";

#[derive(Deserialize)]
pub struct IdClaims {
    pub sub: String,
    pub nonce: Option<String>,
    pub email: Option<String>,
    pub email_verified: Option<bool>,
}

impl Provider {
    pub fn new(cfg: OidcConfig, public_url: &url::Url) -> Self {
        let redirect_uri = format!(
            "{}/v1/oauth/{}/callback",
            public_url.as_str().trim_end_matches('/'),
            cfg.provider_id
        );
        Provider {
            cfg,
            redirect_uri,
            meta: RwLock::new(None),
            jwks: RwLock::new(None),
        }
    }

    pub async fn metadata(&self, http: &reqwest::Client) -> anyhow::Result<Metadata> {
        if let Some((at, m)) = &*self.meta.read().expect("poisoned")
            && at.elapsed() < META_TTL
        {
            return Ok(m.clone());
        }
        let url = format!(
            "{}/.well-known/openid-configuration",
            self.cfg.issuer.trim_end_matches('/')
        );
        let m: Metadata = http
            .get(&url)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await
            .context("discovery")?;
        if m.issuer.trim_end_matches('/') != self.cfg.issuer.trim_end_matches('/') {
            bail!("discovery document issuer does not match LOWKI_OIDC_ISSUER");
        }
        *self.meta.write().expect("poisoned") = Some((Instant::now(), m.clone()));
        Ok(m)
    }

    async fn jwks(&self, http: &reqwest::Client, force: bool) -> anyhow::Result<JwkSet> {
        if let Some((at, set)) = &*self.jwks.read().expect("poisoned") {
            let fresh = at.elapsed() < JWKS_TTL;
            let too_soon = at.elapsed() < JWKS_MIN_REFRESH;
            if (fresh && !force) || too_soon {
                return Ok(set.clone());
            }
        }
        let meta = self.metadata(http).await?;
        let set: JwkSet = http
            .get(&meta.jwks_uri)
            .send()
            .await?
            .error_for_status()?
            .json()
            .await
            .context("jwks")?;
        *self.jwks.write().expect("poisoned") = Some((Instant::now(), set.clone()));
        Ok(set)
    }

    pub async fn verify_id_token(&self, http: &reqwest::Client, token: &str, nonce: &str) -> anyhow::Result<IdClaims> {
        let header = jsonwebtoken::decode_header(token).context("malformed id_token")?;
        if !matches!(
            header.alg,
            Algorithm::RS256
                | Algorithm::RS384
                | Algorithm::RS512
                | Algorithm::PS256
                | Algorithm::ES256
                | Algorithm::ES384
                | Algorithm::EdDSA
        ) {
            bail!("id_token algorithm not allowed");
        }
        let kid = header.kid.clone().ok_or_else(|| anyhow!("id_token has no kid"))?;
        let mut set = self.jwks(http, false).await?;
        if set.find(&kid).is_none() {
            set = self.jwks(http, true).await?;
        }
        let jwk = set.find(&kid).ok_or_else(|| anyhow!("id_token kid not in JWKS"))?;
        let key = DecodingKey::from_jwk(jwk).context("unusable JWK")?;
        let mut v = Validation::new(header.alg);
        v.set_audience(&[&self.cfg.client_id]);
        let mut issuers = vec![self.cfg.issuer.clone()];
        issuers.extend(self.cfg.extra_issuers.iter().cloned());
        v.set_issuer(&issuers);
        v.set_required_spec_claims(&["exp", "iss", "aud", "sub"]);
        v.leeway = 60;
        let data = jsonwebtoken::decode::<IdClaims>(token, &key, &v).context("id_token rejected")?;
        let claims = data.claims;
        match &claims.nonce {
            Some(n) if ct_eq_str(n, nonce) => {}
            _ => bail!("id_token nonce mismatch"),
        }
        if claims.sub.is_empty() {
            bail!("id_token has empty sub");
        }
        Ok(claims)
    }
}

fn provider<'a>(state: &'a AppState, name: &str) -> ApiResult<&'a Provider> {
    state
        .oidc
        .as_ref()
        .filter(|p| p.cfg.provider_id == name)
        .ok_or_else(|| ApiError::not_found("unknown sign-in provider"))
}

#[derive(Deserialize)]
pub struct StartQuery {
    #[serde(default)]
    pub link: Option<String>,
}

pub async fn start(
    State(state): State<AppState>,
    Path(name): Path<String>,
    MaybeSession(session): MaybeSession,
    Query(q): Query<StartQuery>,
) -> ApiResult<Response> {
    let p = provider(&state, &name)?;
    let link = matches!(q.link.as_deref(), Some("1" | "true"));
    let link_user = if link {
        Some(
            session
                .as_ref()
                .ok_or_else(|| ApiError::unauthorized("linking requires a session"))?
                .user_id
                .clone(),
        )
    } else {
        None
    };
    let meta = p.metadata(&state.http).await.map_err(|e| {
        tracing::error!(error = %e, "OIDC discovery failed");
        ApiError::new(
            StatusCode::BAD_GATEWAY,
            "provider_unavailable",
            "sign-in provider unavailable",
        )
    })?;

    let st = random_token();
    let nonce = random_token();
    let verifier = random_token();
    let challenge = b64(&Sha256::digest(verifier.as_bytes()));
    let binding = random_token();
    sqlx::query(
        "INSERT INTO oauth_flows (state_hash, provider, binding_hash, code_verifier, nonce, link_user_id, expires_at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7)",
    )
    .bind(sha256_hex(st.as_bytes()))
    .bind(&name)
    .bind(sha256_hex(binding.as_bytes()))
    .bind(&verifier)
    .bind(&nonce)
    .bind(&link_user)
    .bind(now() + FLOW_TTL)
    .execute(&state.db)
    .await?;
    sqlx::query("DELETE FROM oauth_flows WHERE expires_at < $1")
        .bind(now())
        .execute(&state.db)
        .await?;

    let mut url = url::Url::parse(&meta.authorization_endpoint).map_err(|_| ApiError::internal())?;
    url.query_pairs_mut()
        .append_pair("response_type", "code")
        .append_pair("client_id", &p.cfg.client_id)
        .append_pair("redirect_uri", &p.redirect_uri)
        .append_pair("scope", &p.cfg.scopes)
        .append_pair("state", &st)
        .append_pair("nonce", &nonce)
        .append_pair("code_challenge", &challenge)
        .append_pair("code_challenge_method", "S256");
    let cookie = build_cookie(&state, BINDING_COOKIE, &binding, FLOW_TTL, Some("Lax"));
    Ok(with_cookies(Redirect::to(url.as_str()), vec![cookie]))
}

#[derive(Deserialize)]
pub struct CallbackQuery {
    pub code: Option<String>,
    pub state: Option<String>,
    pub error: Option<String>,
}

#[derive(Deserialize)]
struct TokenResponse {
    id_token: String,
}

/// A tiny page that reports the outcome to the opener on the Lowki origin and closes.
fn popup_page(state: &AppState, payload: serde_json::Value, cookies: Vec<String>) -> Response {
    let nonce = &random_token()[..22];
    let target = serde_json::to_string(&origin_of(&state.config.public_url)).unwrap_or_default();
    // `</` cannot appear in serde_json output of our payload except inside strings; escape anyway.
    let data = payload.to_string().replace("</", "<\\/");
    let html = format!(
        "<!doctype html><meta charset=\"utf-8\"><title>Lowki</title><p id=m>You can close this window.</p>\
         <script nonce=\"{nonce}\">(function(){{var r={data};if(window.opener){{window.opener.postMessage(r,{target});window.close();}}}})();</script>"
    );
    let mut resp = Html(html).into_response();
    if let Ok(v) = HeaderValue::from_str(&format!(
        "default-src 'none'; script-src 'nonce-{nonce}'; frame-ancestors 'none'"
    )) {
        resp.headers_mut().insert(header::CONTENT_SECURITY_POLICY, v);
    }
    resp.headers_mut()
        .insert(header::REFERRER_POLICY, HeaderValue::from_static("no-referrer"));
    with_cookies(resp, cookies)
}

pub async fn callback(
    State(state): State<AppState>,
    Path(name): Path<String>,
    MaybeSession(session): MaybeSession,
    headers: HeaderMap,
    Query(q): Query<CallbackQuery>,
) -> Response {
    let clear = build_cookie(&state, BINDING_COOKIE, "", 0, Some("Lax"));
    match callback_inner(&state, &name, session, &headers, q).await {
        Ok((payload, mut cookies)) => {
            cookies.push(clear);
            popup_page(&state, payload, cookies)
        }
        Err(code) => popup_page(
            &state,
            json!({ "type": "lowki:oauth", "ok": false, "error": code }),
            vec![clear],
        ),
    }
}

async fn callback_inner(
    state: &AppState,
    name: &str,
    session: Option<crate::session::Session>,
    headers: &HeaderMap,
    q: CallbackQuery,
) -> Result<(serde_json::Value, Vec<String>), &'static str> {
    let p = provider(state, name).map_err(|_| "unknown_provider")?;
    if q.error.is_some() {
        return Err("provider_error");
    }
    let (Some(code), Some(st)) = (q.code, q.state) else {
        return Err("invalid_request");
    };
    let row: Option<(String, String, String, String, Option<String>, i64)> = sqlx::query_as(
        "SELECT provider, binding_hash, code_verifier, nonce, link_user_id, expires_at FROM oauth_flows WHERE state_hash = $1",
    )
    .bind(sha256_hex(st.as_bytes()))
    .fetch_optional(&state.db)
    .await
    .map_err(|_| "internal")?;
    let (flow_provider, binding_hash, verifier, nonce, link_user, expires_at) = row.ok_or("invalid_state")?;
    // Single use.
    sqlx::query("DELETE FROM oauth_flows WHERE state_hash = $1")
        .bind(sha256_hex(st.as_bytes()))
        .execute(&state.db)
        .await
        .map_err(|_| "internal")?;
    if flow_provider != name || expires_at < now() {
        return Err("invalid_state");
    }
    let binding = read_cookie(headers, &cookie_name(state, BINDING_COOKIE)).ok_or("invalid_state")?;
    if !ct_eq_str(&sha256_hex(binding.as_bytes()), &binding_hash) {
        return Err("invalid_state");
    }

    let meta = p.metadata(&state.http).await.map_err(|_| "provider_unavailable")?;
    let resp = state
        .http
        .post(&meta.token_endpoint)
        .form(&[
            ("grant_type", "authorization_code"),
            ("code", code.as_str()),
            ("redirect_uri", p.redirect_uri.as_str()),
            ("client_id", p.cfg.client_id.as_str()),
            ("client_secret", p.cfg.client_secret.expose().as_str()),
            ("code_verifier", verifier.as_str()),
        ])
        .send()
        .await
        .map_err(|_| "provider_unavailable")?;
    if !resp.status().is_success() {
        tracing::warn!(status = %resp.status(), "OIDC code exchange failed");
        return Err("exchange_failed");
    }
    let tokens: TokenResponse = resp.json().await.map_err(|_| "exchange_failed")?;
    let claims = p
        .verify_id_token(&state.http, &tokens.id_token, &nonce)
        .await
        .map_err(|e| {
            tracing::warn!(error = %e, "id_token verification failed");
            "invalid_id_token"
        })?;

    let subject_key = hmac_hex(&state.keys.identity, &[format!("{name}:{}", claims.sub).as_bytes()]);
    let email = if claims.email_verified == Some(true) {
        claims.email.as_deref()
    } else {
        None
    };
    let link = link_user.is_some();
    if let Some(uid) = &link_user
        && session.as_ref().map(|s| &s.user_id) != Some(uid)
    {
        return Err("session_changed");
    }
    let resolved = users::resolve_external(state, session.as_ref(), link, "oidc", name, &subject_key, email)
        .await
        .map_err(|e| {
            if e.status == StatusCode::CONFLICT {
                "already_linked"
            } else {
                "internal"
            }
        })?;
    let ns = establish(
        state,
        session.as_ref(),
        &resolved.user_id,
        &resolved.identity_id,
        "oidc",
    )
    .await
    .map_err(|_| "internal")?;
    Ok((
        json!({
            "type": "lowki:oauth",
            "ok": true,
            "provider": name,
            "new_user": resolved.new_user,
            "linked": resolved.linked,
            "method_id": resolved.identity_id,
        }),
        ns.cookies,
    ))
}
