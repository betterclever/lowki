//! Email one-time codes.
//!
//! * 6 digits, uniform, single use, short TTL (`LOWKI_OTP_TTL_SECS`, default 600 s).
//! * Stored as `HMAC(otp_key, email_key || code)`; compared in constant time.
//! * The email is stored only as `email_key = HMAC(identity_key, "email:" || normalized_email)`
//!   (plaintext only if `LOWKI_STORE_EMAIL=true`).
//! * Rate limits per email and per IP on sends, per email on verifies, and a per-code attempt cap.

use std::sync::Arc;

use async_trait::async_trait;
use axum::{Json, extract::State, response::Response};
use rand::Rng;
use serde::Deserialize;
use serde_json::json;

use crate::{
    AppState, ClientIp,
    config::EmailSenderConfig,
    crypto::{ct_eq, hmac, hmac_hex},
    db::now,
    error::{ApiError, ApiResult},
    login_response,
    session::{MaybeSession, establish},
    users,
};

#[async_trait]
pub trait OtpSender: Send + Sync {
    /// Whether email sign-in is available at all.
    fn enabled(&self) -> bool {
        true
    }
    async fn send(&self, to: &str, code: &str) -> anyhow::Result<()>;
}

/// Dev mode only: prints the code to stdout.
pub struct LogSender;

#[async_trait]
impl OtpSender for LogSender {
    async fn send(&self, to: &str, code: &str) -> anyhow::Result<()> {
        // Dev mode only (enforced in config). Codes are short-lived and single use.
        println!("[lowki dev] email sign-in code for {to}: {code}");
        Ok(())
    }
}

pub struct DisabledSender;

#[async_trait]
impl OtpSender for DisabledSender {
    fn enabled(&self) -> bool {
        false
    }
    async fn send(&self, _: &str, _: &str) -> anyhow::Result<()> {
        anyhow::bail!("email sign-in is disabled")
    }
}

pub struct SmtpSender {
    transport: lettre::AsyncSmtpTransport<lettre::Tokio1Executor>,
    from: lettre::message::Mailbox,
}

impl SmtpSender {
    pub fn new(url: &str, from: &str) -> anyhow::Result<Self> {
        let transport = lettre::AsyncSmtpTransport::<lettre::Tokio1Executor>::from_url(url)
            .map_err(|_| anyhow::anyhow!("LOWKI_SMTP_URL is invalid"))?
            .build();
        Ok(SmtpSender {
            transport,
            from: from
                .parse()
                .map_err(|_| anyhow::anyhow!("LOWKI_EMAIL_FROM is invalid"))?,
        })
    }
}

#[async_trait]
impl OtpSender for SmtpSender {
    async fn send(&self, to: &str, code: &str) -> anyhow::Result<()> {
        use lettre::AsyncTransport;
        let msg = lettre::Message::builder()
            .from(self.from.clone())
            .to(to.parse().map_err(|_| anyhow::anyhow!("invalid recipient"))?)
            .subject(format!("{code} is your Lowki sign-in code"))
            .body(format!(
                "Your Lowki sign-in code is {code}.\n\nIt expires in a few minutes. If you didn't ask for it, ignore this email.\n"
            ))?;
        self.transport
            .send(msg)
            .await
            .map_err(|e| anyhow::anyhow!("smtp send failed: {e}"))?;
        Ok(())
    }
}

pub fn sender_from_config(cfg: &EmailSenderConfig) -> anyhow::Result<Arc<dyn OtpSender>> {
    Ok(match cfg {
        EmailSenderConfig::Log => Arc::new(LogSender),
        EmailSenderConfig::Smtp { url, from } => Arc::new(SmtpSender::new(url.expose(), from)?),
        EmailSenderConfig::Disabled => Arc::new(DisabledSender),
    })
}

fn normalize(email: &str) -> ApiResult<String> {
    let e = email.trim().to_lowercase();
    let valid = e.len() <= 254
        && e.split_once('@')
            .is_some_and(|(l, d)| !l.is_empty() && d.contains('.') && !d.starts_with('.'))
        && !e.chars().any(|c| c.is_whitespace() || c.is_control());
    if !valid {
        return Err(ApiError::bad_request("invalid email address"));
    }
    Ok(e)
}

pub fn email_key(state: &AppState, normalized: &str) -> String {
    hmac_hex(&state.keys.identity, &[format!("email:{normalized}").as_bytes()])
}

fn code_mac(state: &AppState, email_key: &str, code: &str) -> [u8; 32] {
    hmac(&state.keys.otp, &[email_key.as_bytes(), code.as_bytes()])
}

#[derive(Deserialize)]
pub struct StartReq {
    pub email: String,
}

pub async fn start(
    State(state): State<AppState>,
    ClientIp(ip): ClientIp,
    Json(req): Json<StartReq>,
) -> ApiResult<Json<serde_json::Value>> {
    if !state.otp_sender.enabled() {
        return Err(ApiError::not_found("email sign-in is disabled"));
    }
    let email = normalize(&req.email)?;
    let key = email_key(&state, &email);
    let limits = &state.config.limits;
    state.limiter.check("otp-start-ip", &ip, limits.otp_start_per_ip)?;
    state
        .limiter
        .check("otp-start-email", &key, limits.otp_start_per_email)?;

    let code = format!("{:06}", rand::thread_rng().gen_range(0..1_000_000u32));
    let mac = hex::encode(code_mac(&state, &key, &code));
    let expires_at = now() + state.config.otp_ttl.as_secs() as i64;
    sqlx::query(
        "INSERT INTO otp_codes (email_key, code_mac, attempts, expires_at) VALUES ($1, $2, 0, $3) \
         ON CONFLICT (email_key) DO UPDATE SET code_mac = excluded.code_mac, attempts = 0, expires_at = excluded.expires_at",
    )
    .bind(&key)
    .bind(&mac)
    .bind(expires_at)
    .execute(&state.db)
    .await?;
    if let Err(e) = state.otp_sender.send(&email, &code).await {
        tracing::error!(error = %e, "failed to send sign-in code");
        return Err(ApiError::new(
            axum::http::StatusCode::BAD_GATEWAY,
            "send_failed",
            "could not send the code",
        ));
    }
    Ok(Json(
        json!({ "ok": true, "expires_in": state.config.otp_ttl.as_secs() }),
    ))
}

#[derive(Deserialize)]
pub struct VerifyReq {
    pub email: String,
    pub code: String,
    #[serde(default)]
    pub link: bool,
}

pub async fn verify(
    State(state): State<AppState>,
    MaybeSession(session): MaybeSession,
    headers: axum::http::HeaderMap,
    Json(req): Json<VerifyReq>,
) -> ApiResult<Response> {
    if req.link {
        let s = session
            .as_ref()
            .ok_or_else(|| ApiError::unauthorized("linking requires a session"))?;
        crate::session::check_csrf(&state, &headers, s)?;
    }
    let email = normalize(&req.email)?;
    let key = email_key(&state, &email);
    state
        .limiter
        .check("otp-verify-email", &key, state.config.limits.otp_verify_per_email)?;

    let invalid = || {
        ApiError::new(
            axum::http::StatusCode::BAD_REQUEST,
            "invalid_code",
            "invalid or expired code",
        )
    };
    let row: Option<(String, i64, i64)> =
        sqlx::query_as("SELECT code_mac, attempts, expires_at FROM otp_codes WHERE email_key = $1")
            .bind(&key)
            .fetch_optional(&state.db)
            .await?;
    let Some((stored, attempts, expires_at)) = row else {
        return Err(invalid());
    };
    if expires_at <= now() || attempts >= state.config.limits.otp_max_attempts as i64 {
        sqlx::query("DELETE FROM otp_codes WHERE email_key = $1")
            .bind(&key)
            .execute(&state.db)
            .await?;
        return Err(invalid());
    }
    let code = req.code.trim();
    let candidate = code_mac(&state, &key, code);
    let stored = hex::decode(&stored).unwrap_or_default();
    if code.len() != 6 || !ct_eq(&candidate, &stored) {
        sqlx::query("UPDATE otp_codes SET attempts = attempts + 1 WHERE email_key = $1")
            .bind(&key)
            .execute(&state.db)
            .await?;
        return Err(invalid());
    }
    // Single use: only the request that deletes the row wins.
    let del = sqlx::query("DELETE FROM otp_codes WHERE email_key = $1 AND code_mac = $2")
        .bind(&key)
        .bind(hex::encode(&stored))
        .execute(&state.db)
        .await?;
    if del.rows_affected() != 1 {
        return Err(invalid());
    }

    let resolved =
        users::resolve_external(&state, session.as_ref(), req.link, "email", "email", &key, Some(&email)).await?;
    let ns = establish(
        &state,
        session.as_ref(),
        &resolved.user_id,
        &resolved.identity_id,
        "email",
    )
    .await?;
    Ok(login_response(ns, &resolved, json!({})))
}
