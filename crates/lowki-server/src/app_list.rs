//! The wallet home's "Your apps": an opaque, device-encrypted blob per user, plus the public
//! display name of a registered app.
//!
//! The wallet adds an app's origin to its list each time it issues an app token, encrypts the
//! list in lowki-worker under a key derived from the seed (`lowki/app-list/key/v1`), and stores
//! the ciphertext here. The server never learns which apps a user uses from this endpoint: it
//! stores bytes and a version counter. (It does see token issuance per app, as it always has.)

use axum::{
    Json,
    extract::{Path, State},
};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    AppState,
    apps,
    config::Limit,
    crypto::{b64, unb64},
    db::now,
    error::{ApiError, ApiResult},
    session::AuthSession,
};

/// Ciphertext size cap (the plaintext cap in the worker is 32 KiB).
pub const MAX_BLOB: usize = 33 * 1024;
const WRITES_PER_HOUR: Limit = Limit::new(240, 3600);

/// `GET /v1/me/app-list` -> `{blob: base64url | null, version}`.
pub async fn get_app_list(State(state): State<AppState>, AuthSession(s): AuthSession) -> ApiResult<Json<Value>> {
    let row: Option<(String, i64)> = sqlx::query_as("SELECT blob, version FROM app_lists WHERE user_id = $1")
        .bind(&s.user_id)
        .fetch_optional(&state.db)
        .await?;
    Ok(Json(match row {
        Some((blob, version)) => json!({ "blob": blob, "version": version }),
        None => json!({ "blob": null, "version": 0 }),
    }))
}

#[derive(Deserialize)]
pub struct PutAppList {
    /// base64url ciphertext.
    pub blob: String,
    /// The version the client read (0 = none yet). A stale version gets 409 and the client
    /// re-reads, merges and retries.
    pub version: i64,
}

/// `PUT /v1/me/app-list` (session + CSRF) -> `{version}` or `409 version_conflict`.
pub async fn put_app_list(
    State(state): State<AppState>,
    AuthSession(s): AuthSession,
    Json(req): Json<PutAppList>,
) -> ApiResult<Json<Value>> {
    state.limiter.check("app-list", &s.user_id, WRITES_PER_HOUR)?;
    let bytes = unb64(&req.blob).ok_or_else(|| ApiError::bad_request("blob must be base64url"))?;
    if bytes.is_empty() || bytes.len() > MAX_BLOB {
        return Err(ApiError::bad_request("blob size out of range"));
    }
    let t = now();
    let next = req.version + 1;
    let done = if req.version == 0 {
        sqlx::query(
            "INSERT INTO app_lists (user_id, blob, version, updated_at) VALUES ($1, $2, 1, $3) ON CONFLICT (user_id) DO NOTHING",
        )
        .bind(&s.user_id)
        .bind(b64(&bytes))
        .bind(t)
        .execute(&state.db)
        .await?
    } else {
        sqlx::query("UPDATE app_lists SET blob = $1, version = $2, updated_at = $3 WHERE user_id = $4 AND version = $5")
            .bind(b64(&bytes))
            .bind(next)
            .bind(t)
            .bind(&s.user_id)
            .bind(req.version)
            .execute(&state.db)
            .await?
    };
    if done.rows_affected() != 1 {
        return Err(ApiError::new(
            axum::http::StatusCode::CONFLICT,
            "version_conflict",
            "the app list changed; read it again",
        ));
    }
    Ok(Json(json!({ "version": next })))
}

/// `GET /v1/apps/{app_id}` (public): the registered display name and its one origin, so the
/// wallet shows "Lowki Tips" (and the origin under it) rather than an app ID or a name the
/// embedding page made up.
pub async fn public_app(State(state): State<AppState>, Path(app_id): Path<String>) -> ApiResult<Json<Value>> {
    let app = apps::get_app(&state, &app_id).await?.ok_or_else(|| ApiError::not_found("unknown app"))?;
    Ok(Json(json!({
        "id": app.id,
        "name": app.name,
        "origin": app.allowed_origins.first(),
    })))
}
