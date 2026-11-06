//! Storage for the server share S of the user's 2-of-3 Shamir split.
//!
//! S is opaque bytes. It is bound to the social/email sign-in method that stored it and is
//! released only to a session holding a recent sign-in with that method
//! (`LOWKI_SHARE_FRESH_AUTH_SECS`). Releases are rate limited and audited; no share bytes are
//! ever logged.
//!
//! "Balanced" security dial: with `LOWKI_RECOVERY_DELAY_SECS > 0` the first release request
//! only opens a recovery request; the share is released after the delay, and a passkey
//! session can cancel it in the meantime. TODO: notify the user (email / webhook) when a
//! recovery request opens.

use axum::{
    Json,
    extract::State,
    http::StatusCode,
    response::{IntoResponse, Response},
};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    AppState, audit,
    crypto::{b64, unb64},
    db::now,
    error::{ApiError, ApiResult},
    session::{AuthSession, Session},
};

const MAX_SHARE: usize = 1024;
const SOCIAL_KINDS: [&str; 2] = ["oidc", "email"];

/// The newest fresh social/email sign-in in the session.
fn fresh_social(state: &AppState, s: &Session) -> Option<String> {
    let cutoff = now() - state.config.share_fresh_auth.as_secs() as i64;
    s.auths
        .iter()
        .rev()
        .find(|a| SOCIAL_KINDS.contains(&a.kind.as_str()) && a.at >= cutoff)
        .map(|a| a.identity_id.clone())
}

#[derive(Deserialize)]
pub struct PutShare {
    /// base64url of the opaque share bytes.
    pub share: String,
}

/// `PUT /v1/me/share`. Binds S to the session's recent social/email method. Overwriting an
/// existing share requires a recent sign-in with the method it is bound to.
pub async fn put_share(
    State(state): State<AppState>,
    AuthSession(s): AuthSession,
    Json(req): Json<PutShare>,
) -> ApiResult<Json<Value>> {
    let bytes = unb64(&req.share).ok_or_else(|| ApiError::bad_request("share must be base64url"))?;
    if bytes.is_empty() || bytes.len() > MAX_SHARE {
        return Err(ApiError::bad_request("share size out of range"));
    }
    let existing: Option<(String,)> = sqlx::query_as("SELECT bound_identity_id FROM shares WHERE user_id = $1")
        .bind(&s.user_id)
        .fetch_optional(&state.db)
        .await?;
    let bound = match existing {
        Some((bound,)) => {
            if s.fresh_auth_with(&bound, state.config.share_fresh_auth).is_none() {
                return Err(ApiError::forbidden("sign in with the method that guards the share"));
            }
            bound
        }
        None => fresh_social(&state, &s)
            .ok_or_else(|| ApiError::forbidden("a recent Google or email sign-in is required to store the share"))?,
    };
    let t = now();
    sqlx::query(
        "INSERT INTO shares (user_id, share, bound_identity_id, created_at, updated_at) VALUES ($1, $2, $3, $4, $4) \
         ON CONFLICT (user_id) DO UPDATE SET share = excluded.share, bound_identity_id = excluded.bound_identity_id, \
         updated_at = excluded.updated_at",
    )
    .bind(&s.user_id)
    .bind(b64(&bytes))
    .bind(&bound)
    .bind(t)
    .execute(&state.db)
    .await?;
    audit::record(
        &state,
        Some(&s.user_id),
        "share.stored",
        json!({ "bound_method_id": bound, "len": bytes.len() }),
    )
    .await;
    Ok(Json(json!({ "ok": true, "bound_method_id": bound })))
}

/// `POST /v1/me/share/release`: returns S to a session with a recent sign-in with the bound method.
pub async fn release_share(State(state): State<AppState>, AuthSession(s): AuthSession) -> ApiResult<Response> {
    if let Err(e) = state
        .limiter
        .check("share", &s.user_id, state.config.limits.share_release_per_user)
    {
        audit::record(
            &state,
            Some(&s.user_id),
            "share.release_denied",
            json!({ "reason": "rate_limited" }),
        )
        .await;
        return Err(e);
    }
    let row: Option<(String, String)> =
        sqlx::query_as("SELECT share, bound_identity_id FROM shares WHERE user_id = $1")
            .bind(&s.user_id)
            .fetch_optional(&state.db)
            .await?;
    let Some((share, bound)) = row else {
        return Err(ApiError::not_found("no share stored"));
    };
    if s.fresh_auth_with(&bound, state.config.share_fresh_auth).is_none() {
        audit::record(
            &state,
            Some(&s.user_id),
            "share.release_denied",
            json!({ "reason": "bound_method_not_fresh" }),
        )
        .await;
        return Err(ApiError::forbidden("sign in with the method that guards the share"));
    }

    let delay = state.config.recovery_delay.as_secs() as i64;
    if delay > 0 {
        let t = now();
        let req: Option<(i64,)> = sqlx::query_as("SELECT available_at FROM recovery_requests WHERE user_id = $1")
            .bind(&s.user_id)
            .fetch_optional(&state.db)
            .await?;
        match req {
            None => {
                sqlx::query("INSERT INTO recovery_requests (user_id, requested_at, available_at) VALUES ($1, $2, $3)")
                    .bind(&s.user_id)
                    .bind(t)
                    .bind(t + delay)
                    .execute(&state.db)
                    .await?;
                audit::record(
                    &state,
                    Some(&s.user_id),
                    "recovery.requested",
                    json!({ "available_at": t + delay }),
                )
                .await;
                return Ok((
                    StatusCode::ACCEPTED,
                    Json(json!({ "status": "pending", "available_at": t + delay })),
                )
                    .into_response());
            }
            Some((available_at,)) if available_at > t => {
                return Ok((
                    StatusCode::ACCEPTED,
                    Json(json!({ "status": "pending", "available_at": available_at })),
                )
                    .into_response());
            }
            Some(_) => {
                sqlx::query("DELETE FROM recovery_requests WHERE user_id = $1")
                    .bind(&s.user_id)
                    .execute(&state.db)
                    .await?;
            }
        }
    }

    audit::record(
        &state,
        Some(&s.user_id),
        "share.released",
        json!({ "bound_method_id": bound }),
    )
    .await;
    Ok(Json(json!({ "status": "released", "share": share })).into_response())
}

/// `DELETE /v1/me/share/recovery`: cancels a pending recovery. Needs a recent passkey sign-in.
pub async fn cancel_recovery(State(state): State<AppState>, AuthSession(s): AuthSession) -> ApiResult<Json<Value>> {
    let cutoff = now() - state.config.share_fresh_auth.as_secs() as i64;
    if !s.auths.iter().any(|a| a.kind == "passkey" && a.at >= cutoff) {
        return Err(ApiError::forbidden("a recent passkey sign-in is required"));
    }
    let res = sqlx::query("DELETE FROM recovery_requests WHERE user_id = $1")
        .bind(&s.user_id)
        .execute(&state.db)
        .await?;
    if res.rows_affected() > 0 {
        audit::record(&state, Some(&s.user_id), "recovery.cancelled", json!({})).await;
    }
    Ok(Json(json!({ "ok": true, "cancelled": res.rows_affected() > 0 })))
}

/// `GET /v1/me/share`: metadata only, never the bytes.
pub async fn share_status(State(state): State<AppState>, AuthSession(s): AuthSession) -> ApiResult<Json<Value>> {
    let row: Option<(String, i64)> =
        sqlx::query_as("SELECT bound_identity_id, updated_at FROM shares WHERE user_id = $1")
            .bind(&s.user_id)
            .fetch_optional(&state.db)
            .await?;
    let rec: Option<(i64, i64)> =
        sqlx::query_as("SELECT requested_at, available_at FROM recovery_requests WHERE user_id = $1")
            .bind(&s.user_id)
            .fetch_optional(&state.db)
            .await?;
    Ok(Json(json!({
        "exists": row.is_some(),
        "bound_method_id": row.as_ref().map(|r| &r.0),
        "updated_at": row.map(|r| r.1),
        "recovery_request": rec.map(|(r, a)| json!({ "requested_at": r, "available_at": a })),
        "recovery_delay_secs": state.config.recovery_delay.as_secs(),
    })))
}
