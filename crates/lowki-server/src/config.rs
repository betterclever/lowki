//! Configuration. Everything comes from environment variables (see README.md).
//!
//! Secrets are wrapped in [`Secret`] so they never show up in `Debug` output or logs.

use std::{fmt, net::SocketAddr, time::Duration};

use anyhow::{Context, bail};
use base64::Engine;
use url::Url;

/// A value that must never be printed.
#[derive(Clone)]
pub struct Secret<T>(pub T);

impl<T> fmt::Debug for Secret<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(***)")
    }
}

impl<T> Secret<T> {
    pub fn expose(&self) -> &T {
        &self.0
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SameSite {
    Strict,
    Lax,
    None,
}

impl SameSite {
    pub fn as_str(self) -> &'static str {
        match self {
            SameSite::Strict => "Strict",
            SameSite::Lax => "Lax",
            SameSite::None => "None",
        }
    }
}

#[derive(Clone, Debug)]
pub struct OidcConfig {
    /// Short provider id used in URLs and in the identity HMAC (`HMAC(secret, "<id>:" || sub)`).
    pub provider_id: String,
    pub issuer: String,
    /// Extra accepted `iss` values (Google also issues `accounts.google.com` without a scheme).
    pub extra_issuers: Vec<String>,
    pub client_id: String,
    pub client_secret: Secret<String>,
    pub scopes: String,
}

#[derive(Clone, Debug)]
pub enum EmailSenderConfig {
    /// Print codes to the log. Only allowed with `LOWKI_DEV_MODE=true`.
    Log,
    Smtp {
        url: Secret<String>,
        from: String,
    },
    Disabled,
}

/// `(max events, window)` pairs for the fixed-window limiter.
#[derive(Clone, Copy, Debug)]
pub struct Limit {
    pub max: u32,
    pub window: Duration,
}

impl Limit {
    pub const fn new(max: u32, window_secs: u64) -> Self {
        Limit {
            max,
            window: Duration::from_secs(window_secs),
        }
    }
}

#[derive(Clone, Debug)]
pub struct RateLimits {
    pub otp_start_per_email: Limit,
    pub otp_start_per_ip: Limit,
    pub otp_verify_per_email: Limit,
    /// Wrong guesses allowed against a single issued code before it is burned.
    pub otp_max_attempts: u32,
    pub login_per_ip: Limit,
    pub share_release_per_user: Limit,
    pub blob_fetch_per_user: Limit,
    pub token_per_user: Limit,
}

impl Default for RateLimits {
    fn default() -> Self {
        RateLimits {
            otp_start_per_email: Limit::new(3, 15 * 60),
            otp_start_per_ip: Limit::new(30, 15 * 60),
            otp_verify_per_email: Limit::new(10, 15 * 60),
            otp_max_attempts: 5,
            login_per_ip: Limit::new(60, 60),
            share_release_per_user: Limit::new(5, 60 * 60),
            blob_fetch_per_user: Limit::new(30, 60 * 60),
            token_per_user: Limit::new(120, 60),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Config {
    pub bind: SocketAddr,
    /// Public URL of this server. Used as JWT `iss` and as the default WebAuthn origin.
    pub public_url: Url,
    pub database_url: Secret<String>,
    pub server_secret: Secret<Vec<u8>>,
    pub admin_token: Option<Secret<String>>,
    pub dev_mode: bool,

    pub rp_id: String,
    pub rp_name: String,
    /// Origins allowed to run WebAuthn ceremonies and to call the cookie API.
    pub lowki_origins: Vec<Url>,

    pub oidc: Option<OidcConfig>,
    pub store_email: bool,
    pub email_sender: EmailSenderConfig,
    pub otp_ttl: Duration,

    pub session_ttl: Duration,
    pub refresh_ttl: Duration,
    pub access_token_ttl: Duration,
    /// How recent a sign-in must be to release the server share.
    pub share_fresh_auth: Duration,
    /// How recent a passkey assertion must be to read or write the wrapped blob.
    pub blob_fresh_auth: Duration,
    /// "Balanced" security dial: delay between a recovery request and the share release.
    /// Zero disables the delay.
    pub recovery_delay: Duration,

    pub cookie_secure: bool,
    pub cookie_same_site: SameSite,
    pub cookie_partitioned: bool,
    pub trust_proxy_headers: bool,
    pub limits: RateLimits,
    /// ENSIP-30 stealth gateway; `None` unless `LOWKI_ENS_GATEWAY=true`.
    pub ens_gateway: Option<crate::ens_gateway::GatewayConfig>,
    /// Sponsored name claims (src/ens_names.rs).
    pub ens_names: Option<crate::ens_names::NamesConfig>,
}

fn env(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty())
}

fn env_bool(name: &str, default: bool) -> anyhow::Result<bool> {
    match env(name).as_deref() {
        None => Ok(default),
        Some("1" | "true" | "yes" | "on") => Ok(true),
        Some("0" | "false" | "no" | "off") => Ok(false),
        Some(_) => bail!("{name} must be true or false"),
    }
}

fn env_secs(name: &str, default: u64) -> anyhow::Result<Duration> {
    Ok(Duration::from_secs(match env(name) {
        None => default,
        Some(v) => v
            .parse()
            .with_context(|| format!("{name} must be a number of seconds"))?,
    }))
}

/// Accepts hex or base64/base64url. Never echoes the value on error.
pub fn decode_secret(raw: &str) -> anyhow::Result<Vec<u8>> {
    if raw.len().is_multiple_of(2) && raw.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Ok(hex::decode(raw)?);
    }
    let cleaned = raw.trim_end_matches('=');
    base64::engine::general_purpose::URL_SAFE_NO_PAD
        .decode(cleaned.replace('+', "-").replace('/', "_"))
        .map_err(|_| anyhow::anyhow!("secret is neither hex nor base64"))
}

pub fn origin_of(url: &Url) -> String {
    url.origin().ascii_serialization()
}

impl Config {
    /// A config with sensible defaults for a given public URL and secret; used by tests.
    pub fn for_tests(public_url: &str, database_url: &str) -> Self {
        let public_url = Url::parse(public_url).expect("url");
        let mut secret = vec![0u8; 32];
        rand::Rng::fill(&mut rand::thread_rng(), &mut secret[..]);
        Config {
            bind: "127.0.0.1:0".parse().unwrap(),
            rp_id: public_url.host_str().unwrap_or("localhost").to_string(),
            rp_name: "Lowki".into(),
            lowki_origins: vec![public_url.clone()],
            public_url,
            database_url: Secret(database_url.into()),
            server_secret: Secret(secret),
            admin_token: Some(Secret("test-admin-token".into())),
            dev_mode: true,
            oidc: None,
            store_email: false,
            email_sender: EmailSenderConfig::Log,
            otp_ttl: Duration::from_secs(600),
            session_ttl: Duration::from_secs(3600),
            refresh_ttl: Duration::from_secs(30 * 86400),
            access_token_ttl: Duration::from_secs(900),
            share_fresh_auth: Duration::from_secs(600),
            blob_fresh_auth: Duration::from_secs(300),
            recovery_delay: Duration::ZERO,
            cookie_secure: false,
            cookie_same_site: SameSite::Lax,
            cookie_partitioned: false,
            trust_proxy_headers: false,
            limits: RateLimits::default(),
            ens_gateway: None,
            ens_names: None,
        }
    }

    pub fn from_env() -> anyhow::Result<Self> {
        let dev_mode = env_bool("LOWKI_DEV_MODE", false)?;
        let bind: SocketAddr = env("LOWKI_BIND")
            .unwrap_or_else(|| "0.0.0.0:8080".into())
            .parse()
            .context("LOWKI_BIND must be host:port")?;
        let public_url = Url::parse(
            &env("LOWKI_PUBLIC_URL").context("LOWKI_PUBLIC_URL is required (e.g. https://auth.example.com)")?,
        )
        .context("LOWKI_PUBLIC_URL is not a URL")?;
        let database_url = env("LOWKI_DATABASE_URL").unwrap_or_else(|| "sqlite://lowki.db?mode=rwc".into());

        let secret_raw = env("LOWKI_SERVER_SECRET").context(
            "LOWKI_SERVER_SECRET is required (32+ random bytes, hex or base64; e.g. `openssl rand -hex 32`)",
        )?;
        let server_secret = decode_secret(&secret_raw).context("LOWKI_SERVER_SECRET")?;
        if server_secret.len() < 32 {
            bail!("LOWKI_SERVER_SECRET must decode to at least 32 bytes");
        }

        let admin_token = env("LOWKI_ADMIN_TOKEN").map(Secret);
        if let Some(t) = &admin_token
            && t.0.len() < 24
        {
            bail!("LOWKI_ADMIN_TOKEN must be at least 24 characters");
        }

        let rp_id = env("LOWKI_RP_ID")
            .or_else(|| public_url.host_str().map(str::to_string))
            .context("LOWKI_RP_ID")?;
        let rp_name = env("LOWKI_RP_NAME").unwrap_or_else(|| "Lowki".into());
        let lowki_origins = match env("LOWKI_ORIGINS") {
            Some(list) => list
                .split(',')
                .map(|s| Url::parse(s.trim()).with_context(|| "LOWKI_ORIGINS entry is not a URL"))
                .collect::<anyhow::Result<Vec<_>>>()?,
            None => vec![public_url.clone()],
        };

        let oidc = match env("LOWKI_OIDC_CLIENT_ID") {
            None => None,
            Some(client_id) => {
                let issuer = env("LOWKI_OIDC_ISSUER").unwrap_or_else(|| "https://accounts.google.com".into());
                let mut extra_issuers = Vec::new();
                if issuer == "https://accounts.google.com" {
                    extra_issuers.push("accounts.google.com".to_string());
                }
                Some(OidcConfig {
                    provider_id: env("LOWKI_OIDC_PROVIDER").unwrap_or_else(|| "google".into()),
                    issuer,
                    extra_issuers,
                    client_id,
                    client_secret: Secret(
                        env("LOWKI_OIDC_CLIENT_SECRET")
                            .context("LOWKI_OIDC_CLIENT_SECRET is required with LOWKI_OIDC_CLIENT_ID")?,
                    ),
                    scopes: env("LOWKI_OIDC_SCOPES").unwrap_or_else(|| "openid".into()),
                })
            }
        };

        let store_email = env_bool("LOWKI_STORE_EMAIL", false)?;
        let email_sender = match env("LOWKI_EMAIL_SENDER").as_deref() {
            Some("smtp") => EmailSenderConfig::Smtp {
                url: Secret(env("LOWKI_SMTP_URL").context("LOWKI_SMTP_URL is required for smtp")?),
                from: env("LOWKI_EMAIL_FROM").context("LOWKI_EMAIL_FROM is required for smtp")?,
            },
            Some("log") => {
                if !dev_mode {
                    bail!("LOWKI_EMAIL_SENDER=log prints codes and requires LOWKI_DEV_MODE=true");
                }
                EmailSenderConfig::Log
            }
            Some("disabled") => EmailSenderConfig::Disabled,
            None if dev_mode => EmailSenderConfig::Log,
            None => EmailSenderConfig::Disabled,
            Some(_) => bail!("LOWKI_EMAIL_SENDER must be log, smtp or disabled"),
        };

        let cookie_secure = env_bool("LOWKI_COOKIE_SECURE", public_url.scheme() == "https")?;
        let cookie_same_site = match env("LOWKI_COOKIE_SAMESITE")
            .as_deref()
            .map(str::to_ascii_lowercase)
            .as_deref()
        {
            None | Some("lax") => SameSite::Lax,
            Some("strict") => SameSite::Strict,
            Some("none") => SameSite::None,
            Some(_) => bail!("LOWKI_COOKIE_SAMESITE must be strict, lax or none"),
        };
        if cookie_same_site == SameSite::None && !cookie_secure {
            bail!("LOWKI_COOKIE_SAMESITE=none requires secure cookies (https)");
        }
        if !cookie_secure && !dev_mode {
            tracing::warn!("cookies are not Secure: set LOWKI_PUBLIC_URL to https or LOWKI_COOKIE_SECURE=true");
        }

        let mut limits = RateLimits::default();
        if let Some(n) = env("LOWKI_SHARE_RELEASES_PER_HOUR") {
            limits.share_release_per_user = Limit::new(n.parse().context("LOWKI_SHARE_RELEASES_PER_HOUR")?, 3600);
        }
        if let Some(n) = env("LOWKI_OTP_SENDS_PER_15MIN") {
            limits.otp_start_per_email = Limit::new(n.parse().context("LOWKI_OTP_SENDS_PER_15MIN")?, 900);
        }

        Ok(Config {
            bind,
            public_url,
            database_url: Secret(database_url),
            server_secret: Secret(server_secret),
            admin_token,
            dev_mode,
            rp_id,
            rp_name,
            lowki_origins,
            oidc,
            store_email,
            email_sender,
            otp_ttl: env_secs("LOWKI_OTP_TTL_SECS", 600)?,
            session_ttl: env_secs("LOWKI_SESSION_TTL_SECS", 3600)?,
            refresh_ttl: env_secs("LOWKI_REFRESH_TTL_SECS", 30 * 86400)?,
            access_token_ttl: env_secs("LOWKI_ACCESS_TOKEN_TTL_SECS", 900)?,
            share_fresh_auth: env_secs("LOWKI_SHARE_FRESH_AUTH_SECS", 600)?,
            blob_fresh_auth: env_secs("LOWKI_BLOB_FRESH_AUTH_SECS", 300)?,
            recovery_delay: env_secs("LOWKI_RECOVERY_DELAY_SECS", 0)?,
            cookie_secure,
            cookie_same_site,
            cookie_partitioned: env_bool("LOWKI_COOKIE_PARTITIONED", false)?,
            trust_proxy_headers: env_bool("LOWKI_TRUST_PROXY_HEADERS", false)?,
            limits,
            ens_gateway: crate::ens_gateway::GatewayConfig::from_env()?,
            ens_names: crate::ens_names::NamesConfig::from_env()?,
        })
    }

    /// `iss` claim: the public URL without a trailing slash.
    pub fn issuer(&self) -> String {
        self.public_url.as_str().trim_end_matches('/').to_string()
    }

    pub fn is_lowki_origin(&self, origin: &str) -> bool {
        self.lowki_origins.iter().any(|u| origin_of(u) == origin)
    }
}
