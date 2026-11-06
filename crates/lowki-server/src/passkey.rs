//! Passkeys (WebAuthn) for the Lowki RP ID, plus the passkey-wrapped seed blob.
//!
//! PRF: the server passes the `prf` extension through untouched. Registration options include
//! `extensions.prf = {}` so the authenticator enables PRF; the client adds `eval` (its salt)
//! itself. The client must delete `clientExtensionResults.prf.results` before posting the
//! credential; if it does not, the server drops the field on receipt and never stores or logs it.

use axum::{
    Json,
    extract::{Path, State},
    response::Response,
};
use serde::Deserialize;
use serde_json::{Value, json};
use webauthn_rs::prelude::{
    DiscoverableAuthentication, DiscoverableKey, Passkey, PasskeyRegistration, PublicKeyCredential,
    RegisterPublicKeyCredential, Uuid,
};

use crate::{
    AppState, ClientIp, audit,
    crypto::{b64, random_token, unb64},
    db::now,
    error::{ApiError, ApiResult},
    login_response,
    session::{AuthSession, MaybeSession, establish},
    users::{self, Resolved},
};

const FLOW_TTL: i64 = 300;

/// kind, state_json, user_id, user_handle, expires_at
type FlowRow = (String, String, Option<String>, Option<String>, i64);
const MAX_BLOB: usize = 4096;

fn webauthn_err(e: impl std::fmt::Display) -> ApiError {
    tracing::info!(error = %e, "webauthn ceremony rejected");
    ApiError::new(
        axum::http::StatusCode::BAD_REQUEST,
        "webauthn_failed",
        "passkey verification failed",
    )
}

/// Removes PRF outputs (secrets) from a client credential before anything else touches it.
fn strip_prf_results(cred: &mut Value) -> bool {
    let mut stripped = false;
    for key in ["clientExtensionResults", "extensions"] {
        if let Some(prf) = cred
            .get_mut(key)
            .and_then(|e| e.get_mut("prf"))
            .and_then(Value::as_object_mut)
        {
            stripped |= prf.remove("results").is_some();
        }
    }
    if stripped {
        tracing::warn!("client sent PRF results to the server; discarded");
    }
    stripped
}

async fn save_flow(
    state: &AppState,
    kind: &str,
    st: String,
    user_id: Option<&str>,
    handle: Option<&str>,
) -> ApiResult<String> {
    let id = random_token();
    sqlx::query("DELETE FROM webauthn_flows WHERE expires_at < $1")
        .bind(now())
        .execute(&state.db)
        .await?;
    sqlx::query(
        "INSERT INTO webauthn_flows (id, kind, state_json, user_id, user_handle, expires_at) VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(&id)
    .bind(kind)
    .bind(st)
    .bind(user_id)
    .bind(handle)
    .bind(now() + FLOW_TTL)
    .execute(&state.db)
    .await?;
    Ok(id)
}

async fn take_flow(state: &AppState, id: &str, kind: &str) -> ApiResult<(String, Option<String>, Option<String>)> {
    let row: Option<FlowRow> =
        sqlx::query_as("SELECT kind, state_json, user_id, user_handle, expires_at FROM webauthn_flows WHERE id = $1")
            .bind(id)
            .fetch_optional(&state.db)
            .await?;
    let del = sqlx::query("DELETE FROM webauthn_flows WHERE id = $1")
        .bind(id)
        .execute(&state.db)
        .await?;
    match row {
        Some((k, st, uid, handle, exp)) if k == kind && exp > now() && del.rows_affected() == 1 => {
            Ok((st, uid, handle))
        }
        _ => Err(ApiError::bad_request("unknown or expired ceremony")),
    }
}

// ---- registration --------------------------------------------------------------------------

#[derive(Deserialize, Default)]
pub struct RegisterStart {
    /// Shown by the authenticator. Defaults to a generic label so no PII is required.
    pub name: Option<String>,
}

/// `POST /v1/passkeys/register/start`. With a session the passkey is added to that user;
/// without one a new user is created on finish.
pub async fn register_start(
    State(state): State<AppState>,
    MaybeSession(session): MaybeSession,
    ClientIp(ip): ClientIp,
    headers: axum::http::HeaderMap,
    body: Option<Json<RegisterStart>>,
) -> ApiResult<Json<Value>> {
    state.limiter.check("login-ip", &ip, state.config.limits.login_per_ip)?;
    if let Some(s) = &session {
        crate::session::check_csrf(&state, &headers, s)?;
    }
    let name = body
        .and_then(|Json(b)| b.name)
        .map(|n| n.trim().chars().take(64).collect::<String>())
        .filter(|n| !n.is_empty())
        .unwrap_or_else(|| "Lowki wallet".to_string());

    let (user_id, handle, exclude) = match &session {
        Some(s) => {
            let (handle,): (String,) = sqlx::query_as("SELECT webauthn_handle FROM users WHERE id = $1")
                .bind(&s.user_id)
                .fetch_one(&state.db)
                .await?;
            let creds: Vec<(String,)> = sqlx::query_as("SELECT cred_id FROM passkeys WHERE user_id = $1")
                .bind(&s.user_id)
                .fetch_all(&state.db)
                .await?;
            let exclude = creds
                .into_iter()
                .filter_map(|(c,)| unb64(&c))
                .map(Into::into)
                .collect::<Vec<_>>();
            (Some(s.user_id.clone()), handle, exclude)
        }
        None => (None, Uuid::new_v4().to_string(), vec![]),
    };
    let uuid = Uuid::parse_str(&handle).map_err(|_| ApiError::internal())?;
    let (ccr, reg) = state
        .webauthn
        .start_passkey_registration(uuid, &name, &name, Some(exclude))
        .map_err(webauthn_err)?;

    let mut options = serde_json::to_value(&ccr).map_err(anyhow::Error::from)?;
    if let Some(pk) = options.get_mut("publicKey") {
        // Lowki's passkey must be discoverable so a new device can sign in with no username.
        pk["authenticatorSelection"]["residentKey"] = json!("required");
        pk["authenticatorSelection"]["requireResidentKey"] = json!(true);
        // Ask the authenticator to enable PRF. The client supplies `eval` if it wants output now.
        if !pk["extensions"].is_object() {
            pk["extensions"] = json!({});
        }
        pk["extensions"]["prf"] = json!({});
    }
    let st = serde_json::to_string(&reg).map_err(anyhow::Error::from)?;
    let flow_id = save_flow(&state, "reg", st, user_id.as_deref(), Some(&handle)).await?;
    Ok(Json(json!({ "flow_id": flow_id, "options": options })))
}

#[derive(Deserialize)]
pub struct RegisterFinish {
    pub flow_id: String,
    pub credential: Value,
}

pub async fn register_finish(
    State(state): State<AppState>,
    MaybeSession(session): MaybeSession,
    headers: axum::http::HeaderMap,
    Json(mut req): Json<RegisterFinish>,
) -> ApiResult<Response> {
    if let Some(s) = &session {
        crate::session::check_csrf(&state, &headers, s)?;
    }
    strip_prf_results(&mut req.credential);
    let (st, flow_user, handle) = take_flow(&state, &req.flow_id, "reg").await?;
    if flow_user.is_some() && flow_user.as_deref() != session.as_ref().map(|s| s.user_id.as_str()) {
        return Err(ApiError::forbidden("session changed during registration"));
    }
    let reg: PasskeyRegistration = serde_json::from_str(&st).map_err(anyhow::Error::from)?;
    let cred: RegisterPublicKeyCredential =
        serde_json::from_value(req.credential).map_err(|_| ApiError::bad_request("malformed credential"))?;
    let passkey = state
        .webauthn
        .finish_passkey_registration(&cred, &reg)
        .map_err(webauthn_err)?;
    let cred_id = b64(passkey.cred_id().as_ref());

    let exists: Option<(String,)> = sqlx::query_as("SELECT cred_id FROM passkeys WHERE cred_id = $1")
        .bind(&cred_id)
        .fetch_optional(&state.db)
        .await?;
    if exists.is_some() {
        return Err(ApiError::conflict("credential already registered"));
    }
    let (user_id, new_user) = match flow_user {
        Some(u) => (u, false),
        None => (users::create_user(&state, handle).await?, true),
    };
    let identity_id = users::insert_identity(
        &state,
        &user_id,
        "passkey",
        "webauthn",
        &format!("passkey:{cred_id}"),
        None,
    )
    .await?;
    let t = now();
    sqlx::query(
        "INSERT INTO passkeys (cred_id, identity_id, user_id, passkey_json, created_at, last_used_at) VALUES ($1, $2, $3, $4, $5, $5)",
    )
    .bind(&cred_id)
    .bind(&identity_id)
    .bind(&user_id)
    .bind(serde_json::to_string(&passkey).map_err(anyhow::Error::from)?)
    .bind(t)
    .execute(&state.db)
    .await?;

    // A UV registration proves possession, so it counts as a passkey sign-in for this session.
    let ns = establish(&state, session.as_ref(), &user_id, &identity_id, "passkey").await?;
    let resolved = Resolved {
        user_id,
        identity_id,
        new_user,
        linked: !new_user,
    };
    Ok(login_response(ns, &resolved, json!({ "credential_id": cred_id })))
}

// ---- authentication ------------------------------------------------------------------------

/// `POST /v1/passkeys/login/start`: discoverable (username-less) assertion options.
pub async fn login_start(State(state): State<AppState>, ClientIp(ip): ClientIp) -> ApiResult<Json<Value>> {
    state.limiter.check("login-ip", &ip, state.config.limits.login_per_ip)?;
    let (rcr, auth) = state
        .webauthn
        .start_discoverable_authentication()
        .map_err(webauthn_err)?;
    let mut options = serde_json::to_value(&rcr).map_err(anyhow::Error::from)?;
    // webauthn-rs forces conditional mediation; Lowki uses a modal prompt, so let the client choose.
    if let Some(o) = options.as_object_mut() {
        o.remove("mediation");
    }
    let st = serde_json::to_string(&auth).map_err(anyhow::Error::from)?;
    let flow_id = save_flow(&state, "auth", st, None, None).await?;
    Ok(Json(json!({ "flow_id": flow_id, "options": options })))
}

#[derive(Deserialize)]
pub struct LoginFinish {
    pub flow_id: String,
    pub credential: Value,
}

/// `POST /v1/passkeys/login/finish`: verifies the assertion, starts a session, and returns the
/// wrapped blob stored for this credential (if any).
pub async fn login_finish(
    State(state): State<AppState>,
    MaybeSession(session): MaybeSession,
    Json(mut req): Json<LoginFinish>,
) -> ApiResult<Response> {
    strip_prf_results(&mut req.credential);
    let (st, _, _) = take_flow(&state, &req.flow_id, "auth").await?;
    let auth: DiscoverableAuthentication = serde_json::from_str(&st).map_err(anyhow::Error::from)?;
    let cred: PublicKeyCredential =
        serde_json::from_value(req.credential).map_err(|_| ApiError::bad_request("malformed credential"))?;
    let (handle, raw_id) = state
        .webauthn
        .identify_discoverable_authentication(&cred)
        .map_err(webauthn_err)?;
    let cred_id = b64(raw_id);

    let row: Option<(String, String, String, Option<String>, String)> = sqlx::query_as(
        "SELECT p.identity_id, p.user_id, p.passkey_json, p.wrapped_blob, u.webauthn_handle \
         FROM passkeys p JOIN users u ON u.id = p.user_id WHERE p.cred_id = $1",
    )
    .bind(&cred_id)
    .fetch_optional(&state.db)
    .await?;
    let (identity_id, user_id, pk_json, blob, user_handle) =
        row.ok_or_else(|| ApiError::unauthorized("unknown credential"))?;
    if Uuid::parse_str(&user_handle).ok() != Some(handle) {
        return Err(ApiError::unauthorized("credential does not match user"));
    }
    let mut passkey: Passkey = serde_json::from_str(&pk_json).map_err(anyhow::Error::from)?;
    let result = state
        .webauthn
        .finish_discoverable_authentication(&cred, auth, &[DiscoverableKey::from(&passkey)])
        .map_err(webauthn_err)?;
    if !result.user_verified() {
        return Err(ApiError::unauthorized("user verification required"));
    }
    if passkey.update_credential(&result) == Some(true) {
        sqlx::query("UPDATE passkeys SET passkey_json = $1 WHERE cred_id = $2")
            .bind(serde_json::to_string(&passkey).map_err(anyhow::Error::from)?)
            .bind(&cred_id)
            .execute(&state.db)
            .await?;
    }
    let t = now();
    sqlx::query("UPDATE passkeys SET last_used_at = $1 WHERE cred_id = $2")
        .bind(t)
        .bind(&cred_id)
        .execute(&state.db)
        .await?;
    sqlx::query("UPDATE identities SET last_used_at = $1 WHERE id = $2")
        .bind(t)
        .bind(&identity_id)
        .execute(&state.db)
        .await?;

    let ns = establish(&state, session.as_ref(), &user_id, &identity_id, "passkey").await?;
    if blob.is_some() {
        audit::record(
            &state,
            Some(&user_id),
            "blob.released",
            json!({ "credential_id": cred_id, "via": "assertion" }),
        )
        .await;
    }
    let resolved = Resolved {
        user_id,
        identity_id,
        new_user: false,
        linked: false,
    };
    Ok(login_response(
        ns,
        &resolved,
        json!({ "credential_id": cred_id, "wrapped_blob": blob }),
    ))
}

// ---- wrapped blob --------------------------------------------------------------------------

async fn owned_passkey(state: &AppState, user_id: &str, cred_id: &str) -> ApiResult<String> {
    let row: Option<(String,)> = sqlx::query_as("SELECT identity_id FROM passkeys WHERE cred_id = $1 AND user_id = $2")
        .bind(cred_id)
        .bind(user_id)
        .fetch_optional(&state.db)
        .await?;
    Ok(row.ok_or_else(|| ApiError::not_found("no such credential"))?.0)
}

#[derive(Deserialize)]
pub struct PutBlob {
    /// base64url of the opaque wrapped seed (`0x01 || nonce12 || ct+tag`).
    pub blob: String,
}

/// `PUT /v1/passkeys/{cred_id}/blob`: needs a recent assertion (or registration) with that passkey.
pub async fn put_blob(
    State(state): State<AppState>,
    AuthSession(s): AuthSession,
    Path(cred_id): Path<String>,
    Json(req): Json<PutBlob>,
) -> ApiResult<Json<Value>> {
    let identity_id = owned_passkey(&state, &s.user_id, &cred_id).await?;
    if s.fresh_auth_with(&identity_id, state.config.blob_fresh_auth).is_none() {
        return Err(ApiError::forbidden("a recent assertion with this passkey is required"));
    }
    let bytes = unb64(&req.blob).ok_or_else(|| ApiError::bad_request("blob must be base64url"))?;
    if bytes.is_empty() || bytes.len() > MAX_BLOB {
        return Err(ApiError::bad_request("blob size out of range"));
    }
    sqlx::query("UPDATE passkeys SET wrapped_blob = $1 WHERE cred_id = $2")
        .bind(b64(&bytes))
        .bind(&cred_id)
        .execute(&state.db)
        .await?;
    audit::record(
        &state,
        Some(&s.user_id),
        "blob.stored",
        json!({ "credential_id": cred_id, "len": bytes.len() }),
    )
    .await;
    Ok(Json(json!({ "ok": true })))
}

/// `GET /v1/passkeys/{cred_id}/blob`: same requirement as storing.
pub async fn get_blob(
    State(state): State<AppState>,
    AuthSession(s): AuthSession,
    Path(cred_id): Path<String>,
) -> ApiResult<Json<Value>> {
    let identity_id = owned_passkey(&state, &s.user_id, &cred_id).await?;
    state
        .limiter
        .check("blob", &s.user_id, state.config.limits.blob_fetch_per_user)?;
    if s.fresh_auth_with(&identity_id, state.config.blob_fresh_auth).is_none() {
        audit::record(
            &state,
            Some(&s.user_id),
            "blob.release_denied",
            json!({ "credential_id": cred_id, "reason": "no_fresh_assertion" }),
        )
        .await;
        return Err(ApiError::forbidden("a recent assertion with this passkey is required"));
    }
    let (blob,): (Option<String>,) = sqlx::query_as("SELECT wrapped_blob FROM passkeys WHERE cred_id = $1")
        .bind(&cred_id)
        .fetch_one(&state.db)
        .await?;
    let blob = blob.ok_or_else(|| ApiError::not_found("no blob stored for this credential"))?;
    audit::record(
        &state,
        Some(&s.user_id),
        "blob.released",
        json!({ "credential_id": cred_id, "via": "session" }),
    )
    .await;
    Ok(Json(json!({ "credential_id": cred_id, "wrapped_blob": blob })))
}
