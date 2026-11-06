//! ENSIP-30 stealth-address gateway (ERC-3668 / CCIP-Read) for `<label>.<parent>` names,
//! e.g. `alice.lowki.eth`, served through Fluidkey's `StealthResolver` in verified mode.
//!
//! For each address lookup the gateway:
//! 1. decodes the `resolve(name, addr(...))` calldata the resolver put in its `OffchainLookup`;
//! 2. reads the name's ENSIP-30 meta-address record **on chain** through the configured
//!    UniversalResolver (never from its own storage: verified mode rederives on chain from the
//!    same record, so the chain is the only source of truth);
//! 3. draws a fresh ephemeral key, derives the ERC-5564 scheme-1 stealth address (lowki-stealth),
//!    and builds Scheme1Verifier's `verificationData = e || S.x || S.y`;
//! 4. stores `(name, coin type, ephemeral public key, stealth address, view tag, time)` so the
//!    recipient can find the payment (ENSIP-30 Section 6: the gateway MUST make every derived
//!    address detectable), and only then answers `abi.encode(address, bytes)`.
//!
//! The ephemeral private key leaves the process only inside the response (the resolver's
//! verifier needs it); it is never stored or logged. The recipient fetches its records from
//! `GET /v1/ens/records`, authenticated by a signature from its **viewing key**: exactly the key
//! that can use the records, and nothing else (no session, no identity key).
//!
//! Nothing here logs names, addresses, keys or calldata: request logs are method + path only,
//! and the path of a GET lookup is not logged by the server's trace layer beyond the route.

use std::{
    collections::HashMap,
    fmt,
    sync::Mutex,
    time::{Duration, Instant},
};

use anyhow::{Context, bail};
use axum::{
    Json, Router,
    extract::{Path, Query, State},
    http::{HeaderMap, HeaderValue, Method, StatusCode, header},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use lowki_ens::{
    name::{dns_decode, dns_encode, namehash, normalize},
    resolver::{decode_data_result, encode_data},
    stealth::{
        StealthRecord, decode_resolve_calldata, decode_stealth_record, encode_gateway_response,
        gateway_derive, parse_addr_query, recover_personal, records_auth_message, stealth_data_key,
    },
    universal_resolver::{decode_resolve_result, encode_resolve},
};
use lowki_stealth::{SecretKey, address_from_pubkey};
use serde::Deserialize;
use serde_json::{Value, json};
use tower_http::cors::{Any, CorsLayer};

use crate::{
    AppState, ClientIp,
    config::Limit,
    crypto,
    db,
    error::{ApiError, ApiResult},
};

/// ERC-5564 scheme served. One `StealthResolver` deployment per scheme.
pub const SCHEME_ID: u64 = 1;

/// Gateway settings, from `LOWKI_ENS_*` environment variables.
#[derive(Clone)]
pub struct GatewayConfig {
    /// JSON-RPC endpoint of the chain ENS lives on. May embed an API key: never logged.
    pub rpc_url: String,
    /// ENS UniversalResolver (from `contracts/deployments/<chain>-ensv2.json`).
    pub universal_resolver: [u8; 20],
    /// If set, the looked-up name must resolve through this resolver (checked on chain).
    pub stealth_resolver: Option<[u8; 20]>,
    /// Normalized parent name; served names are exactly one label below it.
    pub parent: String,
    pub lookups_per_ip: Limit,
    pub lookups_per_name: Limit,
    pub records_per_ip: Limit,
    /// How long an on-chain meta-address read is reused.
    pub meta_cache: Duration,
    /// Accepted clock skew of a records-auth timestamp.
    pub auth_skew: Duration,
    /// Max records returned per call.
    pub records_page: u32,
}

impl fmt::Debug for GatewayConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GatewayConfig")
            .field("rpc_url", &"<redacted>")
            .field("universal_resolver", &hex::encode(self.universal_resolver))
            .field("stealth_resolver", &self.stealth_resolver.map(hex::encode))
            .field("parent", &self.parent)
            .finish_non_exhaustive()
    }
}

fn env(name: &str) -> Option<String> {
    std::env::var(name).ok().map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
}

fn env_u32(name: &str, default: u32) -> anyhow::Result<u32> {
    env(name).map_or(Ok(default), |v| v.parse().with_context(|| format!("{name} must be a number")))
}

pub fn parse_address(s: &str) -> anyhow::Result<[u8; 20]> {
    let h = s.strip_prefix("0x").unwrap_or(s);
    let v = hex::decode(h).context("address is not hex")?;
    v.try_into().map_err(|_| anyhow::anyhow!("address must be 20 bytes"))
}

impl GatewayConfig {
    /// `None` unless `LOWKI_ENS_GATEWAY=true`.
    pub fn from_env() -> anyhow::Result<Option<Self>> {
        match env("LOWKI_ENS_GATEWAY").as_deref() {
            None | Some("0" | "false" | "no" | "off") => return Ok(None),
            Some("1" | "true" | "yes" | "on") => {}
            Some(_) => bail!("LOWKI_ENS_GATEWAY must be true or false"),
        }
        let rpc_url = env("LOWKI_ENS_RPC_URL").context("LOWKI_ENS_RPC_URL is required with LOWKI_ENS_GATEWAY")?;
        let universal_resolver = parse_address(
            &env("LOWKI_ENS_UNIVERSAL_RESOLVER").context("LOWKI_ENS_UNIVERSAL_RESOLVER is required")?,
        )
        .context("LOWKI_ENS_UNIVERSAL_RESOLVER")?;
        let stealth_resolver = env("LOWKI_ENS_STEALTH_RESOLVER")
            .map(|s| parse_address(&s))
            .transpose()
            .context("LOWKI_ENS_STEALTH_RESOLVER")?;
        let parent_raw = env("LOWKI_ENS_PARENT").context("LOWKI_ENS_PARENT is required (e.g. lowki.eth)")?;
        let parent = normalize(&parent_raw).map_err(|_| anyhow::anyhow!("LOWKI_ENS_PARENT is not a valid ENS name"))?;
        if parent != parent_raw {
            bail!("LOWKI_ENS_PARENT must be normalized");
        }
        let minute = Duration::from_secs(60);
        Ok(Some(GatewayConfig {
            rpc_url,
            universal_resolver,
            stealth_resolver,
            parent,
            lookups_per_ip: Limit { max: env_u32("LOWKI_ENS_LOOKUPS_PER_MIN_IP", 30)?, window: minute },
            lookups_per_name: Limit {
                max: env_u32("LOWKI_ENS_LOOKUPS_PER_HOUR_NAME", 600)?,
                window: Duration::from_secs(3600),
            },
            records_per_ip: Limit { max: env_u32("LOWKI_ENS_RECORDS_PER_MIN_IP", 30)?, window: minute },
            meta_cache: Duration::from_secs(env_u32("LOWKI_ENS_META_CACHE_SECS", 30)?.into()),
            auth_skew: Duration::from_secs(env_u32("LOWKI_ENS_AUTH_SKEW_SECS", 300)?.into()),
            records_page: env_u32("LOWKI_ENS_RECORDS_PAGE", 500)?.clamp(1, 5000),
        }))
    }
}

/// Runtime state: config and a short cache of on-chain meta-address reads.
pub struct Gateway {
    pub config: GatewayConfig,
    cache: Mutex<HashMap<String, (Instant, OnChain)>>,
}

#[derive(Clone)]
struct OnChain {
    /// The decoded ENSIP-30 record, or `None` if absent/malformed (never derive from those).
    record: Option<StealthRecord>,
}

impl Gateway {
    pub fn new(config: GatewayConfig) -> Self {
        Gateway { config, cache: Mutex::new(HashMap::new()) }
    }
}

pub fn router() -> Router<AppState> {
    // ERC-3668 clients include browsers: the gateway must answer cross-origin requests. Nothing
    // here uses cookies, so a wildcard origin is safe.
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods([Method::GET, Method::POST])
        .allow_headers([
            header::CONTENT_TYPE,
            header::HeaderName::from_static(AUTH_TS_HEADER),
            header::HeaderName::from_static(AUTH_SIG_HEADER),
        ])
        .max_age(Duration::from_secs(600));
    Router::new()
        .route("/v1/ens/gateway/{sender}/{data}", get(lookup_get))
        .route("/v1/ens/gateway", post(lookup_post))
        .route("/v1/ens/records", get(records))
        .layer(cors)
}

fn gateway(state: &AppState) -> ApiResult<&Gateway> {
    state.ens.as_ref().ok_or_else(|| ApiError::not_found("ENS gateway is not enabled"))
}

/// ERC-3668 error body: `{"message": ...}` with a 4xx/5xx status.
fn ccip_error(status: StatusCode, message: &str) -> Response {
    (status, Json(json!({ "message": message }))).into_response()
}

fn no_store(mut resp: Response) -> Response {
    resp.headers_mut().insert(header::CACHE_CONTROL, HeaderValue::from_static("no-store"));
    resp
}

/// `GET /v1/ens/gateway/{sender}/{data}` (the URL template `.../{sender}/{data}.json`).
async fn lookup_get(
    State(state): State<AppState>,
    ip: ClientIp,
    Path((sender, data)): Path<(String, String)>,
) -> Response {
    let data = data.strip_suffix(".json").unwrap_or(&data).to_string();
    no_store(lookup(&state, &ip.0, &sender, &data).await)
}

#[derive(Deserialize)]
struct LookupBody {
    sender: String,
    data: String,
}

/// `POST /v1/ens/gateway` with `{"sender","data"}` (URL template without `{data}`).
async fn lookup_post(State(state): State<AppState>, ip: ClientIp, Json(body): Json<LookupBody>) -> Response {
    no_store(lookup(&state, &ip.0, &body.sender, &body.data).await)
}

fn decode_hex(s: &str) -> Option<Vec<u8>> {
    hex::decode(s.strip_prefix("0x")?).ok()
}

async fn lookup(state: &AppState, ip: &str, sender: &str, data: &str) -> Response {
    let gw = match gateway(state) {
        Ok(g) => g,
        Err(_) => return ccip_error(StatusCode::NOT_FOUND, "ENS gateway is not enabled"),
    };
    if let Err(e) = state.limiter.check("ens-lookup-ip", ip, gw.config.lookups_per_ip) {
        return e.into_response();
    }
    // `sender` is informational only: through the UniversalResolver's batching it is the UR,
    // not the name's resolver, and anyone can POST anything. What binds the answer to the name
    // is the on-chain check below (the name resolves through the configured StealthResolver)
    // and, in the end, the resolver's own on-chain verification of the answer.
    let (Ok(_sender), Some(calldata)) = (parse_address(sender), decode_hex(data)) else {
        return ccip_error(StatusCode::BAD_REQUEST, "malformed sender or data");
    };
    let Ok((dns_name, inner)) = decode_resolve_calldata(&calldata) else {
        return ccip_error(StatusCode::BAD_REQUEST, "expected resolve(bytes,bytes)");
    };
    let Ok(name) = dns_decode(&dns_name) else {
        return ccip_error(StatusCode::BAD_REQUEST, "malformed name");
    };
    if !is_served_name(&name, &gw.config.parent) {
        return ccip_error(StatusCode::NOT_FOUND, "name not served by this gateway");
    }
    let Some(query) = parse_addr_query(&inner) else {
        return ccip_error(StatusCode::BAD_REQUEST, "only address records are served");
    };
    if query.node != namehash(&name) {
        return ccip_error(StatusCode::BAD_REQUEST, "node does not match name");
    }
    let Some(coin_type) = query.coin_type_u64().filter(|c| is_evm_coin_type(*c)) else {
        return ccip_error(StatusCode::BAD_REQUEST, "unsupported coin type");
    };
    // The per-name limit bounds how fast anyone can grow a recipient's record list.
    let name_key = name_key(state, &name);
    if let Err(e) = state.limiter.check("ens-lookup-name", &name_key, gw.config.lookups_per_name) {
        return e.into_response();
    }

    let onchain = match meta_address(state, gw, &name, coin_type).await {
        Ok(m) => m,
        Err(e) => {
            tracing::warn!(error = %e, "ens gateway: reading the meta-address failed");
            return ccip_error(StatusCode::BAD_GATEWAY, "could not read the name's record");
        }
    };
    // Lowki serves verified mode only: with no record the resolver would accept any address.
    let Some(record) = onchain.record else {
        return ccip_error(StatusCode::NOT_FOUND, "name has no stealth meta-address record");
    };

    let ephemeral = fresh_ephemeral_key();
    let derived = match gateway_derive(&record.meta, &ephemeral) {
        Ok(d) => d,
        Err(_) => return ccip_error(StatusCode::INTERNAL_SERVER_ERROR, "derivation failed"),
    };
    drop(ephemeral);

    // Store before answering: an address the recipient cannot find must never be handed out.
    if let Err(e) = store(state, &name_key, coin_type as i64, &derived.ephemeral_pubkey, &derived.stealth_address, derived.view_tag).await {
        tracing::error!(code = e.code, "ens gateway: storing the record failed");
        return ccip_error(StatusCode::INTERNAL_SERVER_ERROR, "could not record the lookup");
    }
    let response = encode_gateway_response(&derived.stealth_address, derived.verification_data.as_ref());
    Json(json!({ "data": format!("0x{}", hex::encode(response)) })).into_response()
}

/// Exactly one label directly below the parent, and already normalized.
fn is_served_name(name: &str, parent: &str) -> bool {
    let Some(label) = name.strip_suffix(parent).and_then(|l| l.strip_suffix('.')) else {
        return false;
    };
    !label.is_empty() && !label.contains('.') && normalize(name).is_ok_and(|n| n == name)
}

/// ENSIP-11 EVM coin types: 60 (Ethereum), `0x80000000 | chainId`, and the ENSIP-19 default.
fn is_evm_coin_type(coin_type: u64) -> bool {
    coin_type == 60 || (coin_type & 0x8000_0000 != 0 && coin_type < 0x1_0000_0000)
}

/// A uniformly random valid secp256k1 private key from the OS RNG.
fn fresh_ephemeral_key() -> SecretKey {
    loop {
        let bytes = zeroize::Zeroizing::new(crypto::random_bytes::<32>());
        if let Ok(k) = SecretKey::from_bytes(&bytes) {
            return k;
        }
    }
}

/// Names are stored and rate-limited under a keyed hash: a database dump does not map names
/// to stealth addresses without `LOWKI_SERVER_SECRET`.
fn name_key(state: &AppState, name: &str) -> String {
    crypto::hmac_hex(&state.keys.identity, &[b"lowki/ens-name/v1", name.as_bytes()])
}

async fn store(
    state: &AppState,
    name_key: &str,
    coin_type: i64,
    ephemeral_pubkey: &[u8; 33],
    stealth_address: &[u8; 20],
    view_tag: u8,
) -> ApiResult<()> {
    sqlx::query(
        "INSERT INTO ens_stealth_records (stealth_address, name_key, coin_type, ephemeral_pubkey, view_tag, created_at) \
         VALUES ($1, $2, $3, $4, $5, $6)",
    )
    .bind(format!("0x{}", hex::encode(stealth_address)))
    .bind(name_key)
    .bind(coin_type)
    .bind(format!("0x{}", hex::encode(ephemeral_pubkey)))
    .bind(i64::from(view_tag))
    .bind(db::now())
    .execute(&state.db)
    .await?;
    Ok(())
}

/// Reads `data(node, "stealth-meta-address[1][<coinType>]")` through the UniversalResolver.
async fn meta_address(state: &AppState, gw: &Gateway, name: &str, coin_type: u64) -> anyhow::Result<OnChain> {
    let cache_key = format!("{name}|{coin_type}");
    if let Some((at, v)) = gw.cache.lock().expect("cache poisoned").get(&cache_key)
        && at.elapsed() < gw.config.meta_cache
    {
        return Ok(v.clone());
    }
    let dns = dns_encode(name).map_err(|_| anyhow::anyhow!("dns encode"))?;
    let inner = encode_data(&namehash(name), &stealth_data_key(SCHEME_ID, coin_type));
    let call = encode_resolve(&dns, &inner);
    let ret = eth_call(state, &gw.config, &gw.config.universal_resolver, &call)
        .await?
        .context("UniversalResolver reverted (no resolver, or the record is not on chain)")?;
    let (result, resolver) = decode_resolve_result(&ret).map_err(|_| anyhow::anyhow!("decoding UR return"))?;
    let value = decode_data_result(&result).map_err(|_| anyhow::anyhow!("decoding data() return"))?;
    let record = if value.is_empty() { None } else { decode_stealth_record(&value).ok() };
    if let Some(expected) = gw.config.stealth_resolver
        && expected != resolver
    {
        bail!("name is not served by the configured StealthResolver");
    }
    let v = OnChain { record };
    let mut cache = gw.cache.lock().expect("cache poisoned");
    if cache.len() > 10_000 {
        cache.clear();
    }
    cache.insert(cache_key, (Instant::now(), v.clone()));
    Ok(v)
}

/// `eth_call` at `latest`. `Ok(None)` on revert.
async fn eth_call(state: &AppState, cfg: &GatewayConfig, to: &[u8; 20], data: &[u8]) -> anyhow::Result<Option<Vec<u8>>> {
    let body = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "eth_call",
        "params": [{ "to": format!("0x{}", hex::encode(to)), "data": format!("0x{}", hex::encode(data)) }, "latest"],
    });
    // reqwest errors can include the URL (and an API key in it): never format them.
    let resp = state
        .http
        .post(&cfg.rpc_url)
        .json(&body)
        .send()
        .await
        .map_err(|_| anyhow::anyhow!("RPC request failed"))?;
    let v: Value = resp.json().await.map_err(|_| anyhow::anyhow!("RPC response is not JSON"))?;
    if v.get("error").is_some() {
        return Ok(None);
    }
    let hex_str = v.get("result").and_then(Value::as_str).context("RPC result missing")?;
    Ok(Some(decode_hex(hex_str).context("RPC result is not hex")?))
}

// ---------------------------------------------------------------------------------------------
// Records endpoint
// ---------------------------------------------------------------------------------------------

pub const AUTH_TS_HEADER: &str = "x-lowki-auth-timestamp";
pub const AUTH_SIG_HEADER: &str = "x-lowki-auth-signature";

#[derive(Deserialize)]
struct RecordsQuery {
    name: String,
    #[serde(default)]
    since: i64,
}

/// `GET /v1/ens/records?name=alice.lowki.eth&since=<unix>`
///
/// Headers: `x-lowki-auth-timestamp: <unix seconds>` and `x-lowki-auth-signature: 0x<65 bytes>`,
/// an EIP-191 `personal_sign` by the name's current **viewing key** over
/// `lowki_ens::stealth::records_auth_message(name, timestamp)`. The viewing public key is taken
/// from the name's on-chain meta-address (coin type 60).
async fn records(
    State(state): State<AppState>,
    ip: ClientIp,
    headers: HeaderMap,
    Query(q): Query<RecordsQuery>,
) -> ApiResult<Response> {
    let gw = gateway(&state)?;
    state.limiter.check("ens-records-ip", &ip.0, gw.config.records_per_ip)?;
    if !is_served_name(&q.name, &gw.config.parent) {
        return Err(ApiError::not_found("name not served by this gateway"));
    }
    let ts: u64 = headers
        .get(AUTH_TS_HEADER)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.parse().ok())
        .ok_or_else(|| ApiError::unauthorized("missing auth timestamp"))?;
    let sig: [u8; 65] = headers
        .get(AUTH_SIG_HEADER)
        .and_then(|v| v.to_str().ok())
        .and_then(decode_hex)
        .and_then(|v| v.try_into().ok())
        .ok_or_else(|| ApiError::unauthorized("missing or malformed auth signature"))?;
    let now = u64::try_from(db::now()).unwrap_or(0);
    if now.abs_diff(ts) > gw.config.auth_skew.as_secs() {
        return Err(ApiError::unauthorized("stale auth timestamp"));
    }
    let signer = recover_personal(&records_auth_message(&q.name, ts), &sig)
        .map_err(|_| ApiError::unauthorized("bad signature"))?;
    let onchain = meta_address(&state, gw, &q.name, 60).await.map_err(|e| {
        tracing::warn!(error = %e, "ens records: reading the meta-address failed");
        ApiError::new(StatusCode::BAD_GATEWAY, "upstream", "could not read the name's record")
    })?;
    let record = onchain.record.ok_or_else(|| ApiError::not_found("name has no stealth record"))?;
    let viewing = address_from_pubkey(record.meta.viewing_pubkey()).map_err(|_| ApiError::internal())?;
    if !crypto::ct_eq(&viewing, &signer) {
        return Err(ApiError::unauthorized("signature is not by the name's viewing key"));
    }

    let rows: Vec<(String, i64, String, i64, i64)> = sqlx::query_as(
        "SELECT stealth_address, coin_type, ephemeral_pubkey, view_tag, created_at FROM ens_stealth_records \
         WHERE name_key = $1 AND created_at >= $2 ORDER BY created_at, stealth_address LIMIT $3",
    )
    .bind(name_key(&state, &q.name))
    .bind(q.since)
    .bind(i64::from(gw.config.records_page))
    .fetch_all(&state.db)
    .await?;
    let records: Vec<Value> = rows
        .into_iter()
        .map(|(addr, coin, eph, tag, at)| {
            json!({ "stealth_address": addr, "coin_type": coin, "ephemeral_pubkey": eph, "view_tag": tag, "created_at": at })
        })
        .collect();
    let complete = records.len() < gw.config.records_page as usize;
    Ok(no_store(
        Json(json!({ "name": q.name, "scheme_id": SCHEME_ID, "records": records, "complete": complete })).into_response(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn served_names() {
        assert!(is_served_name("alice.lowki.eth", "lowki.eth"));
        assert!(!is_served_name("lowki.eth", "lowki.eth"));
        assert!(!is_served_name("a.alice.lowki.eth", "lowki.eth"));
        assert!(!is_served_name("alicelowki.eth", "lowki.eth"));
        assert!(!is_served_name("Alice.lowki.eth", "lowki.eth"));
        assert!(!is_served_name(".lowki.eth", "lowki.eth"));
        assert!(!is_served_name("alice.lowki.eth.evil", "lowki.eth"));
    }

    #[test]
    fn coin_types() {
        assert!(is_evm_coin_type(60));
        assert!(is_evm_coin_type(0x8000_0000));
        assert!(is_evm_coin_type(0x8000_0000 | 11_155_111));
        assert!(!is_evm_coin_type(0));
        assert!(!is_evm_coin_type(501));
        assert!(!is_evm_coin_type(0x1_0000_0000));
    }

    #[test]
    fn config_debug_redacts_rpc() {
        let c = GatewayConfig {
            rpc_url: "https://rpc.example/secret-api-key".into(),
            universal_resolver: [1; 20],
            stealth_resolver: None,
            parent: "lowki.eth".into(),
            lookups_per_ip: Limit { max: 1, window: Duration::from_secs(1) },
            lookups_per_name: Limit { max: 1, window: Duration::from_secs(1) },
            records_per_ip: Limit { max: 1, window: Duration::from_secs(1) },
            meta_cache: Duration::from_secs(1),
            auth_skew: Duration::from_secs(1),
            records_page: 1,
        };
        assert!(!format!("{c:?}").contains("secret-api-key"));
    }
}
