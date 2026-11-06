//! App access tokens (the Privy-compatible part): ES256 JWTs, a JWKS endpoint with rotation,
//! and pairwise per-app user IDs.

use std::{
    sync::RwLock,
    time::{Duration, Instant},
};

use axum::{
    Json,
    extract::{Path, State},
    http::{HeaderMap, header},
    response::{IntoResponse, Response},
};
use p256::ecdsa::{Signature, SigningKey, signature::Signer};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::{
    AppState, apps,
    crypto::{b64, hmac, open, random_bytes, seal, unb64},
    db::now,
    error::{ApiError, ApiResult},
    session::{MaybeSession, check_csrf, with_cookies},
};

/// `sub` for a user in an app: `did:lowki:` + hex(HMAC(user_key, "lowki/pairwise-sub/v1" || app_id)[..16])
/// where `user_key = HMAC(pairwise_root, "lowki/pairwise-user/v1" || user_salt)` and
/// `pairwise_root` is derived from `LOWKI_SERVER_SECRET`. Stable per (user, app); unlinkable
/// across apps without the server secret.
pub fn pairwise_id(state: &AppState, user_salt_hex: &str, app_id: &str) -> String {
    let salt = hex::decode(user_salt_hex).unwrap_or_default();
    let user_key = hmac(&state.keys.pairwise, &[b"lowki/pairwise-user/v1", &salt]);
    let id = hmac(&user_key, &[b"lowki/pairwise-sub/v1", app_id.as_bytes()]);
    format!("did:lowki:{}", hex::encode(&id[..16]))
}

struct Cache {
    loaded: Instant,
    kid: String,
    key: SigningKey,
    jwks: Value,
}

#[derive(Default)]
pub struct KeyStore {
    cache: RwLock<Option<Cache>>,
}

const CACHE_TTL: Duration = Duration::from_secs(60);

fn public_jwk(kid: &str, key: &SigningKey) -> Value {
    let point = key.verifying_key().to_encoded_point(false);
    json!({
        "kty": "EC",
        "crv": "P-256",
        "x": b64(point.x().expect("uncompressed point")),
        "y": b64(point.y().expect("uncompressed point")),
        "kid": kid,
        "use": "sig",
        "alg": "ES256",
    })
}

impl KeyStore {
    fn invalidate(&self) {
        *self.cache.write().expect("keystore poisoned") = None;
    }

    async fn load(&self, state: &AppState) -> ApiResult<()> {
        if let Some(c) = &*self.cache.read().expect("keystore poisoned")
            && c.loaded.elapsed() < CACHE_TTL
        {
            return Ok(());
        }
        // Retired keys stay published for one token lifetime plus a margin so tokens signed
        // just before a rotation keep verifying.
        let grace = state.config.access_token_ttl.as_secs() as i64 + 300;
        let mut rows: Vec<(String, String, String, Option<i64>)> = sqlx::query_as(
            "SELECT kid, private_sealed, public_jwk, retired_at FROM signing_keys \
             WHERE retired_at IS NULL OR retired_at > $1 ORDER BY created_at DESC",
        )
        .bind(now() - grace)
        .fetch_all(&state.db)
        .await?;
        if !rows.iter().any(|r| r.3.is_none()) {
            create_key(state).await?;
            rows = sqlx::query_as(
                "SELECT kid, private_sealed, public_jwk, retired_at FROM signing_keys \
                 WHERE retired_at IS NULL OR retired_at > $1 ORDER BY created_at DESC",
            )
            .bind(now() - grace)
            .fetch_all(&state.db)
            .await?;
        }
        let (kid, sealed, _, _) = rows.iter().find(|r| r.3.is_none()).ok_or_else(ApiError::internal)?;
        let raw = unb64(sealed)
            .and_then(|s| open(&state.keys.signing_kek, &s, kid.as_bytes()))
            .ok_or_else(|| anyhow::anyhow!("cannot unseal JWT signing key (wrong LOWKI_SERVER_SECRET?)"))?;
        let key = SigningKey::from_slice(&raw).map_err(|_| anyhow::anyhow!("bad signing key"))?;
        let keys: Vec<Value> = rows.iter().filter_map(|r| serde_json::from_str(&r.2).ok()).collect();
        *self.cache.write().expect("keystore poisoned") = Some(Cache {
            loaded: Instant::now(),
            kid: kid.clone(),
            key,
            jwks: json!({ "keys": keys }),
        });
        Ok(())
    }

    pub async fn jwks(&self, state: &AppState) -> ApiResult<Value> {
        self.load(state).await?;
        Ok(self
            .cache
            .read()
            .expect("keystore poisoned")
            .as_ref()
            .map(|c| c.jwks.clone())
            .unwrap_or_default())
    }

    pub async fn sign(&self, state: &AppState, claims: &Value) -> ApiResult<String> {
        self.load(state).await?;
        let guard = self.cache.read().expect("keystore poisoned");
        let c = guard.as_ref().ok_or_else(ApiError::internal)?;
        let header = json!({ "alg": "ES256", "typ": "JWT", "kid": c.kid });
        let signing_input = format!(
            "{}.{}",
            b64(header.to_string().as_bytes()),
            b64(claims.to_string().as_bytes())
        );
        let sig: Signature = c.key.sign(signing_input.as_bytes());
        Ok(format!("{signing_input}.{}", b64(&sig.to_bytes())))
    }

    /// Retires the active key (it stays in the JWKS for the grace period) and creates a new one.
    pub async fn rotate(&self, state: &AppState) -> ApiResult<String> {
        sqlx::query("UPDATE signing_keys SET retired_at = $1 WHERE retired_at IS NULL")
            .bind(now())
            .execute(&state.db)
            .await?;
        let kid = create_key(state).await?;
        self.invalidate();
        Ok(kid)
    }
}

async fn create_key(state: &AppState) -> ApiResult<String> {
    let key = SigningKey::random(&mut rand::rngs::OsRng);
    let kid = b64(&random_bytes::<12>());
    let sealed = seal(&state.keys.signing_kek, &key.to_bytes(), kid.as_bytes());
    sqlx::query("INSERT INTO signing_keys (kid, private_sealed, public_jwk, created_at) VALUES ($1, $2, $3, $4)")
        .bind(&kid)
        .bind(b64(&sealed))
        .bind(public_jwk(&kid, &key).to_string())
        .bind(now())
        .execute(&state.db)
        .await?;
    tracing::info!(kid, "created JWT signing key");
    Ok(kid)
}

// ---- handlers ------------------------------------------------------------------------------

pub async fn jwks(State(state): State<AppState>) -> ApiResult<Response> {
    let body = state.jwt.jwks(&state).await?;
    let mut resp = Json(body).into_response();
    resp.headers_mut().insert(
        header::CACHE_CONTROL,
        "public, max-age=300".parse().expect("static header"),
    );
    Ok(resp)
}

pub async fn openid_configuration(State(state): State<AppState>) -> Json<Value> {
    let iss = state.config.issuer();
    Json(json!({
        "issuer": iss,
        "jwks_uri": format!("{iss}/.well-known/jwks.json"),
        "id_token_signing_alg_values_supported": ["ES256"],
        "subject_types_supported": ["pairwise"],
    }))
}

#[derive(Deserialize, Default)]
pub struct TokenRequest {
    /// The embedding app page's origin, when the request comes from the Lowki iframe/popup.
    pub origin: Option<String>,
}

/// `POST /v1/apps/{app_id}/token`: issues an access token for the signed-in user.
///
/// Callable either from the Lowki origin (iframe; CSRF header required; `origin` names the
/// parent page and must be registered for the app) or directly from a registered app origin
/// with credentials (CORS; the browser-enforced `Origin` header is the CSRF defence).
pub async fn issue_token(
    State(state): State<AppState>,
    Path(app_id): Path<String>,
    headers: HeaderMap,
    MaybeSession(session): MaybeSession,
    body: Option<Json<TokenRequest>>,
) -> ApiResult<Response> {
    let app = apps::get_app(&state, &app_id)
        .await?
        .ok_or_else(|| ApiError::not_found("unknown app"))?;
    let s = session.ok_or_else(|| ApiError::unauthorized("no session"))?;
    let req_origin = headers.get(header::ORIGIN).and_then(|v| v.to_str().ok()).unwrap_or("");
    let body = body.map(|Json(b)| b).unwrap_or_default();
    if state.config.is_lowki_origin(req_origin) {
        check_csrf(&state, &headers, &s)?;
        let parent = body
            .origin
            .as_deref()
            .ok_or_else(|| ApiError::bad_request("origin is required"))?;
        if !app.allowed_origins.iter().any(|o| o == parent) {
            return Err(ApiError::forbidden("origin is not registered for this app"));
        }
    } else if !app.allowed_origins.iter().any(|o| o == req_origin) {
        return Err(ApiError::forbidden("origin is not registered for this app"));
    }
    state
        .limiter
        .check("token", &s.user_id, state.config.limits.token_per_user)?;

    let (salt,): (String,) = sqlx::query_as("SELECT pairwise_salt FROM users WHERE id = $1")
        .bind(&s.user_id)
        .fetch_one(&state.db)
        .await?;
    let sub = pairwise_id(&state, &salt, &app_id);
    let sid = hex::encode(
        &hmac(
            &state.keys.pairwise,
            &[b"lowki/sid/v1", s.id_hash.as_bytes(), app_id.as_bytes()],
        )[..16],
    );
    let iat = now();
    let exp = iat + state.config.access_token_ttl.as_secs() as i64;
    let claims = json!({
        "iss": state.config.issuer(),
        "aud": app_id,
        "sub": sub,
        "sid": sid,
        "iat": iat,
        "exp": exp,
        "jti": b64(&random_bytes::<16>()),
    });
    let token = state.jwt.sign(&state, &claims).await?;

    let inserted = sqlx::query(
        "INSERT INTO app_users (app_id, pairwise_id, user_id, created_at, last_seen_at) VALUES ($1, $2, $3, $4, $4) \
         ON CONFLICT (app_id, pairwise_id) DO NOTHING",
    )
    .bind(&app_id)
    .bind(&sub)
    .bind(&s.user_id)
    .bind(iat)
    .execute(&state.db)
    .await?;
    if inserted.rows_affected() == 1 {
        apps::emit_webhook(&state, &app_id, "user.created", json!({ "user_id": sub })).await;
    } else {
        sqlx::query("UPDATE app_users SET last_seen_at = $1 WHERE app_id = $2 AND pairwise_id = $3")
            .bind(iat)
            .bind(&app_id)
            .bind(&sub)
            .execute(&state.db)
            .await?;
    }

    Ok(with_cookies(
        Json(json!({
            "access_token": token,
            "token_type": "Bearer",
            "expires_in": state.config.access_token_ttl.as_secs(),
            "user_id": sub,
            "app_id": app_id,
        })),
        vec![],
    ))
}
