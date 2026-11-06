//! Registered apps, the admin API, the app (server-to-server) API and webhook stubs.

use axum::{
    Json,
    extract::{FromRequestParts, Path, Query, State},
    http::{header, request::Parts},
};
use base64::Engine;
use hmac::{Hmac, Mac};
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::Sha256;

use crate::{
    AppState, audit,
    config::origin_of,
    crypto::{b64, ct_eq_str, hmac, random_bytes, random_token, sha256_hex},
    db::now,
    error::{ApiError, ApiResult},
};

#[derive(Clone, Debug)]
pub struct App {
    pub id: String,
    pub name: String,
    pub allowed_origins: Vec<String>,
    pub webhook_url: Option<String>,
    pub created_at: i64,
}

impl App {
    fn to_json(&self) -> Value {
        json!({
            "id": self.id,
            "name": self.name,
            "allowed_origins": self.allowed_origins,
            "webhook_url": self.webhook_url,
            "created_at": self.created_at,
        })
    }
}

type AppRow = (String, String, String, Option<String>, i64);

fn from_row((id, name, origins, webhook_url, created_at): AppRow) -> App {
    App {
        id,
        name,
        allowed_origins: serde_json::from_str(&origins).unwrap_or_default(),
        webhook_url,
        created_at,
    }
}

pub async fn get_app(state: &AppState, app_id: &str) -> ApiResult<Option<App>> {
    let row: Option<AppRow> =
        sqlx::query_as("SELECT id, name, allowed_origins, webhook_url, created_at FROM apps WHERE id = $1")
            .bind(app_id)
            .fetch_optional(&state.db)
            .await?;
    Ok(row.map(from_row))
}

/// Reloads the set of registered app origins used by the CORS layer of the token endpoint.
pub async fn refresh_origin_cache(state: &AppState) -> ApiResult<()> {
    let rows: Vec<(String,)> = sqlx::query_as("SELECT allowed_origins FROM apps")
        .fetch_all(&state.db)
        .await?;
    let set = rows
        .into_iter()
        .flat_map(|(o,)| serde_json::from_str::<Vec<String>>(&o).unwrap_or_default())
        .collect();
    *state.app_origins.write().expect("origin cache poisoned") = set;
    Ok(())
}

/// An app is exactly one web origin.
///
/// The wallet derives a separate account (address) for every origin it is embedded under, and
/// the access token's `sub` is pairwise per app. With one origin per app the two line up: one
/// app, one origin, one address, one `sub`. An app that serves several origins (prod, staging,
/// a second domain) registers one app per origin, and gets a different wallet and `sub` on each,
/// which is what the user's wallet does anyway.
fn normalize_origins(list: &[String]) -> ApiResult<Vec<String>> {
    if list.len() != 1 {
        return Err(ApiError::bad_request(
            "allowed_origins must contain exactly one origin: register one app per origin",
        ));
    }
    list.iter()
        .map(|o| {
            let u = url::Url::parse(o).map_err(|_| ApiError::bad_request(format!("invalid origin: {o}")))?;
            if !matches!(u.scheme(), "http" | "https")
                || u.host_str().is_none()
                || u.path() != "/"
                || u.query().is_some()
            {
                return Err(ApiError::bad_request(format!(
                    "origin must be scheme://host[:port]: {o}"
                )));
            }
            Ok(origin_of(&u))
        })
        .collect()
}

fn webhook_secret(state: &AppState, app_id: &str) -> String {
    format!("whsec_{}", b64(&hmac(&state.keys.webhook, &[app_id.as_bytes()])))
}

// ---- admin auth ----------------------------------------------------------------------------

/// `Authorization: Bearer $LOWKI_ADMIN_TOKEN`. The admin API is disabled when the token is unset.
pub struct Admin;

impl FromRequestParts<AppState> for Admin {
    type Rejection = ApiError;
    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        let Some(expected) = &state.config.admin_token else {
            return Err(ApiError::not_found("admin API disabled"));
        };
        let got = parts
            .headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Bearer "))
            .unwrap_or("");
        if got.is_empty() || !ct_eq_str(got, expected.expose()) {
            return Err(ApiError::unauthorized("invalid admin token"));
        }
        Ok(Admin)
    }
}

// ---- admin handlers ------------------------------------------------------------------------

#[derive(Deserialize)]
pub struct CreateApp {
    pub name: String,
    #[serde(default)]
    pub allowed_origins: Vec<String>,
    pub webhook_url: Option<String>,
}

pub async fn create_app(State(state): State<AppState>, _: Admin, Json(req): Json<CreateApp>) -> ApiResult<Json<Value>> {
    if req.name.trim().is_empty() || req.name.len() > 100 {
        return Err(ApiError::bad_request("name must be 1-100 characters"));
    }
    let origins = normalize_origins(&req.allowed_origins)?;
    let id = format!("app_{}", hex::encode(random_bytes::<10>()));
    let secret = format!("lsk_{}", random_token());
    sqlx::query(
        "INSERT INTO apps (id, name, allowed_origins, secret_hash, webhook_url, created_at) VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(&id)
    .bind(req.name.trim())
    .bind(serde_json::to_string(&origins).map_err(anyhow::Error::from)?)
    .bind(sha256_hex(secret.as_bytes()))
    .bind(&req.webhook_url)
    .bind(now())
    .execute(&state.db)
    .await?;
    refresh_origin_cache(&state).await?;
    audit::record(&state, None, "app.created", json!({ "app_id": id })).await;
    let app = get_app(&state, &id).await?.ok_or_else(ApiError::internal)?;
    let mut body = app.to_json();
    // Shown once. Only its hash is stored.
    body["app_secret"] = json!(secret);
    body["webhook_secret"] = json!(webhook_secret(&state, &id));
    Ok(Json(body))
}

pub async fn list_apps(State(state): State<AppState>, _: Admin) -> ApiResult<Json<Value>> {
    let rows: Vec<AppRow> =
        sqlx::query_as("SELECT id, name, allowed_origins, webhook_url, created_at FROM apps ORDER BY created_at")
            .fetch_all(&state.db)
            .await?;
    Ok(Json(
        json!({ "apps": rows.into_iter().map(|r| from_row(r).to_json()).collect::<Vec<_>>() }),
    ))
}

pub async fn admin_get_app(State(state): State<AppState>, _: Admin, Path(id): Path<String>) -> ApiResult<Json<Value>> {
    let app = get_app(&state, &id)
        .await?
        .ok_or_else(|| ApiError::not_found("unknown app"))?;
    Ok(Json(app.to_json()))
}

#[derive(Deserialize)]
pub struct UpdateApp {
    pub name: Option<String>,
    pub allowed_origins: Option<Vec<String>>,
    pub webhook_url: Option<Option<String>>,
}

pub async fn update_app(
    State(state): State<AppState>,
    _: Admin,
    Path(id): Path<String>,
    Json(req): Json<UpdateApp>,
) -> ApiResult<Json<Value>> {
    let mut app = get_app(&state, &id)
        .await?
        .ok_or_else(|| ApiError::not_found("unknown app"))?;
    if let Some(n) = req.name {
        app.name = n;
    }
    if let Some(o) = req.allowed_origins {
        app.allowed_origins = normalize_origins(&o)?;
    }
    if let Some(w) = req.webhook_url {
        app.webhook_url = w;
    }
    sqlx::query("UPDATE apps SET name = $1, allowed_origins = $2, webhook_url = $3 WHERE id = $4")
        .bind(&app.name)
        .bind(serde_json::to_string(&app.allowed_origins).map_err(anyhow::Error::from)?)
        .bind(&app.webhook_url)
        .bind(&id)
        .execute(&state.db)
        .await?;
    refresh_origin_cache(&state).await?;
    audit::record(&state, None, "app.updated", json!({ "app_id": id })).await;
    Ok(Json(app.to_json()))
}

#[derive(Deserialize)]
pub struct Page {
    pub limit: Option<i64>,
    pub offset: Option<i64>,
    pub event: Option<String>,
}

/// Lists an app's users by pairwise ID only.
pub async fn list_app_users(
    State(state): State<AppState>,
    _: Admin,
    Path(id): Path<String>,
    Query(p): Query<Page>,
) -> ApiResult<Json<Value>> {
    let limit = p.limit.unwrap_or(100).clamp(1, 1000);
    let rows: Vec<(String, i64, i64)> = sqlx::query_as(
        "SELECT pairwise_id, created_at, last_seen_at FROM app_users WHERE app_id = $1 \
         ORDER BY created_at, pairwise_id LIMIT $2 OFFSET $3",
    )
    .bind(&id)
    .bind(limit)
    .bind(p.offset.unwrap_or(0).max(0))
    .fetch_all(&state.db)
    .await?;
    Ok(Json(json!({
        "users": rows.into_iter().map(|(pid, c, l)| json!({"id": pid, "created_at": c, "last_seen_at": l})).collect::<Vec<_>>()
    })))
}

pub async fn rotate_keys(State(state): State<AppState>, _: Admin) -> ApiResult<Json<Value>> {
    let kid = state.jwt.rotate(&state).await?;
    audit::record(&state, None, "jwt.key_rotated", json!({ "kid": kid })).await;
    Ok(Json(json!({ "kid": kid })))
}

pub async fn list_audit(State(state): State<AppState>, _: Admin, Query(p): Query<Page>) -> ApiResult<Json<Value>> {
    let limit = p.limit.unwrap_or(100).clamp(1, 1000);
    let rows: Vec<(String, i64, Option<String>, String, String)> =
        match &p.event {
            Some(ev) => sqlx::query_as(
                "SELECT id, at, user_id, event, detail FROM audit_log WHERE event = $1 ORDER BY at DESC, id LIMIT $2",
            )
            .bind(ev)
            .bind(limit)
            .fetch_all(&state.db)
            .await?,
            None => {
                sqlx::query_as("SELECT id, at, user_id, event, detail FROM audit_log ORDER BY at DESC, id LIMIT $1")
                    .bind(limit)
                    .fetch_all(&state.db)
                    .await?
            }
        };
    Ok(Json(json!({
        "events": rows.into_iter().map(|(id, at, user, event, detail)| json!({
            "id": id, "at": at, "user_id": user, "event": event,
            "detail": serde_json::from_str::<Value>(&detail).unwrap_or(Value::Null),
        })).collect::<Vec<_>>()
    })))
}

// ---- app API (server to server) ------------------------------------------------------------

/// `Authorization: Basic base64(app_id:app_secret)` (the same shape Privy's server API uses).
pub struct AppAuth(pub String);

impl FromRequestParts<AppState> for AppAuth {
    type Rejection = ApiError;
    async fn from_request_parts(parts: &mut Parts, state: &AppState) -> Result<Self, Self::Rejection> {
        let creds = parts
            .headers
            .get(header::AUTHORIZATION)
            .and_then(|v| v.to_str().ok())
            .and_then(|v| v.strip_prefix("Basic "))
            .and_then(|b| base64::engine::general_purpose::STANDARD.decode(b).ok())
            .and_then(|b| String::from_utf8(b).ok())
            .ok_or_else(|| ApiError::unauthorized("app credentials required"))?;
        let (app_id, secret) = creds
            .split_once(':')
            .ok_or_else(|| ApiError::unauthorized("bad credentials"))?;
        let row: Option<(String,)> = sqlx::query_as("SELECT secret_hash FROM apps WHERE id = $1")
            .bind(app_id)
            .fetch_optional(&state.db)
            .await?;
        let ok = row.is_some_and(|(h,)| ct_eq_str(&h, &sha256_hex(secret.as_bytes())));
        if !ok {
            return Err(ApiError::unauthorized("bad credentials"));
        }
        Ok(AppAuth(app_id.to_string()))
    }
}

pub async fn app_get_user(
    State(state): State<AppState>,
    AppAuth(app_id): AppAuth,
    Path(pairwise_id): Path<String>,
) -> ApiResult<Json<Value>> {
    let row: Option<(String, i64, i64)> = sqlx::query_as(
        "SELECT user_id, created_at, last_seen_at FROM app_users WHERE app_id = $1 AND pairwise_id = $2",
    )
    .bind(&app_id)
    .bind(&pairwise_id)
    .fetch_optional(&state.db)
    .await?;
    let (user_id, created_at, last_seen_at) = row.ok_or_else(|| ApiError::not_found("unknown user"))?;
    let kinds: Vec<(String,)> = sqlx::query_as("SELECT DISTINCT kind FROM identities WHERE user_id = $1")
        .bind(&user_id)
        .fetch_all(&state.db)
        .await?;
    Ok(Json(json!({
        "id": pairwise_id,
        "created_at": created_at,
        "last_seen_at": last_seen_at,
        "linked_account_types": kinds.into_iter().map(|(k,)| k).collect::<Vec<_>>(),
    })))
}

// ---- webhooks (stub) -----------------------------------------------------------------------

/// Records an event in the outbox and makes one best-effort delivery attempt.
///
/// Signature: `lowki-webhook-signature: v1=hex(HMAC-SHA256(webhook_secret, id + "." + timestamp + "." + body))`.
/// TODO: retries with backoff from the outbox, and a delivery log in the admin API.
pub async fn emit_webhook(state: &AppState, app_id: &str, event: &str, data: Value) {
    let id = format!("evt_{}", hex::encode(random_bytes::<12>()));
    let ts = now();
    let payload = json!({ "id": id, "type": event, "created_at": ts, "data": data }).to_string();
    let res = sqlx::query(
        "INSERT INTO webhook_events (id, app_id, event, payload, created_at, attempts) VALUES ($1, $2, $3, $4, $5, 0)",
    )
    .bind(&id)
    .bind(app_id)
    .bind(event)
    .bind(&payload)
    .bind(ts)
    .execute(&state.db)
    .await;
    if let Err(e) = res {
        tracing::error!(error = %e, "failed to record webhook event");
        return;
    }
    let Ok(Some(app)) = get_app(state, app_id).await else {
        return;
    };
    let Some(url) = app.webhook_url else { return };
    let state = state.clone();
    let secret = webhook_secret(&state, app_id);
    tokio::spawn(async move {
        let mut mac = Hmac::<Sha256>::new_from_slice(secret.as_bytes()).expect("hmac key");
        mac.update(format!("{id}.{ts}.{payload}").as_bytes());
        let sig = hex::encode(mac.finalize().into_bytes());
        let resp = state
            .http
            .post(&url)
            .header("content-type", "application/json")
            .header("lowki-webhook-id", &id)
            .header("lowki-webhook-timestamp", ts.to_string())
            .header("lowki-webhook-signature", format!("v1={sig}"))
            .body(payload)
            .send()
            .await;
        let delivered = matches!(&resp, Ok(r) if r.status().is_success());
        let _ = sqlx::query("UPDATE webhook_events SET attempts = attempts + 1, delivered_at = $1 WHERE id = $2")
            .bind(if delivered { Some(now()) } else { None })
            .bind(&id)
            .execute(&state.db)
            .await;
        if !delivered {
            tracing::warn!(event_id = %id, "webhook delivery failed");
        }
    });
}
