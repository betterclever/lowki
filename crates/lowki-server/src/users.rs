//! Users and their sign-in methods ("identities").

use axum::{
    Json,
    extract::{Path, State},
};
use serde::Serialize;
use serde_json::{Value, json};

use crate::{
    AppState, audit,
    crypto::{random_bytes, random_token},
    db::now,
    error::{ApiError, ApiResult},
    session::{AuthSession, Session},
};

pub struct Resolved {
    pub user_id: String,
    pub identity_id: String,
    pub new_user: bool,
    pub linked: bool,
}

/// Creates a user row. `handle` is the WebAuthn user handle (random UUID).
pub async fn create_user(state: &AppState, handle: Option<String>) -> ApiResult<String> {
    let id = uuid::Uuid::new_v4().to_string();
    let handle = handle.unwrap_or_else(|| uuid::Uuid::new_v4().to_string());
    sqlx::query("INSERT INTO users (id, webauthn_handle, pairwise_salt, created_at) VALUES ($1, $2, $3, $4)")
        .bind(&id)
        .bind(&handle)
        .bind(hex::encode(random_bytes::<32>()))
        .bind(now())
        .execute(&state.db)
        .await?;
    audit::record(state, Some(&id), "user.created", json!({})).await;
    Ok(id)
}

pub async fn insert_identity(
    state: &AppState,
    user_id: &str,
    kind: &str,
    provider: &str,
    subject_key: &str,
    email: Option<&str>,
) -> ApiResult<String> {
    let id = format!("m_{}", &random_token()[..22]);
    let t = now();
    sqlx::query(
        "INSERT INTO identities (id, user_id, kind, provider, subject_key, email, created_at, last_used_at) \
         VALUES ($1, $2, $3, $4, $5, $6, $7, $7)",
    )
    .bind(&id)
    .bind(user_id)
    .bind(kind)
    .bind(provider)
    .bind(subject_key)
    .bind(if state.config.store_email { email } else { None })
    .bind(t)
    .execute(&state.db)
    .await?;
    audit::record(
        state,
        Some(user_id),
        "method.linked",
        json!({ "method_id": id, "kind": kind, "provider": provider }),
    )
    .await;
    Ok(id)
}

/// Maps an external identity (OIDC subject or email, already HMAC'd into `subject_key`) to a user.
///
/// * Known identity: signs in as its user. With `link`, it must already belong to the session user.
/// * Unknown identity with `link` and a session: attaches it to the session user.
/// * Unknown identity otherwise: creates a new user.
pub async fn resolve_external(
    state: &AppState,
    session: Option<&Session>,
    link: bool,
    kind: &str,
    provider: &str,
    subject_key: &str,
    email: Option<&str>,
) -> ApiResult<Resolved> {
    let found: Option<(String, String)> = sqlx::query_as("SELECT id, user_id FROM identities WHERE subject_key = $1")
        .bind(subject_key)
        .fetch_optional(&state.db)
        .await?;
    if let Some((identity_id, user_id)) = found {
        if link {
            match session {
                Some(s) if s.user_id == user_id => {}
                Some(_) => return Err(ApiError::conflict("this sign-in method belongs to another account")),
                None => return Err(ApiError::unauthorized("linking requires a session")),
            }
        }
        sqlx::query("UPDATE identities SET last_used_at = $1 WHERE id = $2")
            .bind(now())
            .bind(&identity_id)
            .execute(&state.db)
            .await?;
        return Ok(Resolved {
            user_id,
            identity_id,
            new_user: false,
            linked: false,
        });
    }
    if link {
        let s = session.ok_or_else(|| ApiError::unauthorized("linking requires a session"))?;
        let identity_id = insert_identity(state, &s.user_id, kind, provider, subject_key, email).await?;
        return Ok(Resolved {
            user_id: s.user_id.clone(),
            identity_id,
            new_user: false,
            linked: true,
        });
    }
    let user_id = create_user(state, None).await?;
    let identity_id = insert_identity(state, &user_id, kind, provider, subject_key, email).await?;
    Ok(Resolved {
        user_id,
        identity_id,
        new_user: true,
        linked: false,
    })
}

#[derive(Serialize)]
pub struct Method {
    pub id: String,
    pub kind: String,
    pub provider: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub email: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub credential_id: Option<String>,
    pub created_at: i64,
    pub last_used_at: i64,
}

/// id, kind, provider, subject_key, email, created_at, last_used_at
type MethodRow = (String, String, String, String, Option<String>, i64, i64);

pub async fn list_methods(state: &AppState, user_id: &str) -> ApiResult<Vec<Method>> {
    let rows: Vec<MethodRow> = sqlx::query_as(
        "SELECT id, kind, provider, subject_key, email, created_at, last_used_at FROM identities \
         WHERE user_id = $1 ORDER BY created_at",
    )
    .bind(user_id)
    .fetch_all(&state.db)
    .await?;
    Ok(rows
        .into_iter()
        .map(
            |(id, kind, provider, subject, email, created_at, last_used_at)| Method {
                credential_id: subject.strip_prefix("passkey:").map(str::to_string),
                id,
                kind,
                provider,
                email,
                created_at,
                last_used_at,
            },
        )
        .collect())
}

pub async fn get_methods(State(state): State<AppState>, AuthSession(s): AuthSession) -> ApiResult<Json<Value>> {
    Ok(Json(json!({ "methods": list_methods(&state, &s.user_id).await? })))
}

/// Unlinks a sign-in method. A user always keeps at least one, the method bound to the server
/// share cannot be removed, and the session must contain a recent sign-in.
pub async fn unlink_method(
    State(state): State<AppState>,
    AuthSession(s): AuthSession,
    Path(method_id): Path<String>,
) -> ApiResult<Json<Value>> {
    if !s.any_fresh_auth(state.config.share_fresh_auth) {
        return Err(ApiError::forbidden("sign in again to change sign-in methods"));
    }
    let mut tx = state.db.begin().await?;
    let owned: Option<(String,)> = sqlx::query_as("SELECT kind FROM identities WHERE id = $1 AND user_id = $2")
        .bind(&method_id)
        .bind(&s.user_id)
        .fetch_optional(&mut *tx)
        .await?;
    let (kind,) = owned.ok_or_else(|| ApiError::not_found("no such method"))?;
    let (count,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM identities WHERE user_id = $1")
        .bind(&s.user_id)
        .fetch_one(&mut *tx)
        .await?;
    if count <= 1 {
        return Err(ApiError::conflict("cannot remove the last sign-in method"));
    }
    let bound: Option<(String,)> = sqlx::query_as("SELECT bound_identity_id FROM shares WHERE user_id = $1")
        .bind(&s.user_id)
        .fetch_optional(&mut *tx)
        .await?;
    if bound.is_some_and(|(b,)| b == method_id) {
        return Err(ApiError::conflict(
            "this method guards the recovery share; it cannot be removed",
        ));
    }
    sqlx::query("DELETE FROM passkeys WHERE identity_id = $1")
        .bind(&method_id)
        .execute(&mut *tx)
        .await?;
    sqlx::query("DELETE FROM identities WHERE id = $1")
        .bind(&method_id)
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    audit::record(
        &state,
        Some(&s.user_id),
        "method.unlinked",
        json!({ "method_id": method_id, "kind": kind }),
    )
    .await;
    Ok(Json(json!({ "ok": true })))
}
