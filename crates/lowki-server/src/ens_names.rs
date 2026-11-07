//! Sponsored `<label>.<parent>` names (e.g. `alice.lowki.eth`): one per user, claimed from the
//! wallet home. The operator pays; the owner (the wallet's identity key `m/137'/0'/0'`, which
//! never holds ETH) authorises everything with signatures made in lowki-worker.
//!
//! 1. `POST /v1/names/prepare {label, owner}`: label rules, one name per user, availability,
//!    and every public parameter the worker signs over (nonces, deadline, predicted records
//!    resolver, addresses).
//! 2. `POST /v1/names/register {label, owner, nonce, deadline, signature}`: the server rebuilds
//!    the EIP-712 `Register`, checks the signature recovers to `owner`, and submits
//!    `LowkiRegistrar.register` with the operator key.
//! 3. `POST /v1/names/setup {meta_address, exec_nonce, deadline, execute_signature,
//!    authorization}`: the server **rebuilds the owner batch itself** (the same five calls as
//!    `contracts/e2e`: own PermissionedResolver, StealthResolver config, chains, meta-address,
//!    setResolver), checks the `Execute` signature and the EIP-7702 authorisation recover to the
//!    owner, and sends one type-4 transaction. A client cannot make the operator relay anything
//!    else.
//! 4. `GET /v1/names/me`: the user's name and its status.
//!
//! `LOWKI_ENS_OPERATOR_KEY` is parsed once, held as a `SecretKey`, and never logged; neither are
//! names or addresses (logs carry route, status and transaction hashes only).

use std::{fmt, time::Duration};

use anyhow::{Context, bail};
use axum::{Json, extract::State, http::StatusCode};
use lowki_ens::{
    Address,
    abi::or,
    lowki_registrar::{Registration, encode_available, encode_nonces, encode_register, recover_registration, validate_label},
    name::{dns_encode, labelhash, namehash},
    owner_account::{Call, authorization_digest, encode_execute, encode_nonce, execute_digest},
    permissioned_resolver::{
        ADMIN_ROLES, ALL_ROLES, Grant, encode_deploy_proxy, encode_initialize, encode_predict_proxy_address, encode_set_text,
    },
    registry::encode_set_resolver,
    sig::{recover_digest, sign_digest},
    stealth::{encode_set_chains, encode_set_config, encode_set_meta_address, encode_stealth_record, stealth_text_key},
};
use lowki_stealth::{SecretKey, StealthMetaAddress, keccak256};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::sync::Mutex;

use crate::{
    AppState, ClientIp,
    config::Limit,
    db::now,
    ens_gateway::parse_address,
    error::{ApiError, ApiResult},
    session::AuthSession,
};

/// Settings, from `LOWKI_ENS_*` (on top of the gateway's RPC, parent and StealthResolver).
#[derive(Clone)]
pub struct NamesConfig {
    pub operator_key: [u8; 32],
    pub chain_id: u64,
    pub registrar: Address,
    pub registry: Address,
    pub owner_account: Address,
    pub factory: Address,
    pub permissioned_resolver_impl: Address,
    /// The gateway URL written on chain (`StealthResolver.setConfig`).
    pub gateway_url: String,
    pub duration_secs: u64,
    pub claims_per_ip: Limit,
    pub claims_per_user: Limit,
}

impl fmt::Debug for NamesConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NamesConfig")
            .field("operator_key", &"<redacted>")
            .field("chain_id", &self.chain_id)
            .field("registrar", &hex::encode(self.registrar))
            .field("gateway_url", &self.gateway_url)
            .finish_non_exhaustive()
    }
}

fn env(name: &str) -> Option<String> {
    std::env::var(name).ok().map(|v| v.trim().to_string()).filter(|v| !v.is_empty())
}

fn env_addr(name: &str) -> anyhow::Result<Address> {
    parse_address(&env(name).with_context(|| format!("{name} is required with LOWKI_ENS_OPERATOR_KEY"))?)
        .with_context(|| name.to_string())
}

impl NamesConfig {
    /// `None` unless `LOWKI_ENS_OPERATOR_KEY` is set.
    pub fn from_env() -> anyhow::Result<Option<Self>> {
        let Some(raw) = env("LOWKI_ENS_OPERATOR_KEY") else { return Ok(None) };
        // Never format `raw` or the decode error (it could echo input).
        let bytes = hex::decode(raw.trim_start_matches("0x")).map_err(|_| anyhow::anyhow!("LOWKI_ENS_OPERATOR_KEY is not hex"))?;
        let operator_key: [u8; 32] =
            bytes.try_into().map_err(|_| anyhow::anyhow!("LOWKI_ENS_OPERATOR_KEY must be 32 bytes"))?;
        SecretKey::from_bytes(&operator_key).map_err(|_| anyhow::anyhow!("LOWKI_ENS_OPERATOR_KEY is not a valid key"))?;
        let gateway_url = env("LOWKI_ENS_GATEWAY_URL").context("LOWKI_ENS_GATEWAY_URL is required (the URL written on chain)")?;
        if !gateway_url.starts_with("https://") && !gateway_url.starts_with("http://") {
            bail!("LOWKI_ENS_GATEWAY_URL must be an http(s) URL");
        }
        let days: u64 = env("LOWKI_ENS_NAME_DAYS").map_or(Ok(365), |v| v.parse()).context("LOWKI_ENS_NAME_DAYS")?;
        if !(28..=366).contains(&days) {
            bail!("LOWKI_ENS_NAME_DAYS must be 28..=366 (LowkiRegistrar's range)");
        }
        let per_ip: u32 = env("LOWKI_ENS_CLAIMS_PER_HOUR_IP").map_or(Ok(10), |v| v.parse()).context("LOWKI_ENS_CLAIMS_PER_HOUR_IP")?;
        Ok(Some(NamesConfig {
            operator_key,
            chain_id: env("LOWKI_ENS_CHAIN_ID").map_or(Ok(11_155_111), |v| v.parse()).context("LOWKI_ENS_CHAIN_ID")?,
            registrar: env_addr("LOWKI_ENS_REGISTRAR")?,
            registry: env_addr("LOWKI_ENS_REGISTRY")?,
            owner_account: env_addr("LOWKI_ENS_OWNER_ACCOUNT")?,
            factory: env_addr("LOWKI_ENS_VERIFIABLE_FACTORY")?,
            permissioned_resolver_impl: env_addr("LOWKI_ENS_PERMISSIONED_RESOLVER_IMPL")?,
            gateway_url,
            duration_secs: days * 86_400,
            claims_per_ip: Limit::new(per_ip, 3600),
            claims_per_user: Limit::new(12, 3600),
        }))
    }
}

/// Runtime state: config, the operator key and a lock that serialises operator transactions
/// (one nonce sequence).
pub struct Names {
    pub config: NamesConfig,
    operator: SecretKey,
    tx_lock: Mutex<()>,
}

impl Names {
    pub fn new(config: NamesConfig) -> Self {
        let operator = SecretKey::from_bytes(&config.operator_key).expect("checked in from_env");
        Names { config, operator, tx_lock: Mutex::new(()) }
    }
    pub fn operator_address(&self) -> Address {
        self.operator.address()
    }
}

fn names(state: &AppState) -> ApiResult<(&Names, &crate::ens_gateway::GatewayConfig)> {
    match (state.names.as_ref(), state.ens.as_ref()) {
        (Some(n), Some(g)) => Ok((n, &g.config)),
        _ => Err(ApiError::not_found("name claims are not enabled on this server")),
    }
}

fn h0x(b: &[u8]) -> String {
    format!("0x{}", hex::encode(b))
}

fn addr_param(s: &str, what: &str) -> ApiResult<Address> {
    parse_address(s).map_err(|_| ApiError::bad_request(format!("{what} must be a 20-byte hex address")))
}

fn bytes_param(s: &str, what: &str) -> ApiResult<Vec<u8>> {
    hex::decode(s.trim_start_matches("0x")).map_err(|_| ApiError::bad_request(format!("{what} must be hex")))
}

fn upstream(e: anyhow::Error) -> ApiError {
    tracing::warn!(error = %e, "ens names: chain call failed");
    ApiError::new(StatusCode::BAD_GATEWAY, "upstream", "the chain could not be reached; try again")
}

// ------------------------------------------------------------------------------ the batch

/// The owner batch (byte-identical to lowki-worker `ens::owner_setup_calls`).
pub fn owner_setup_calls(
    cfg: &NamesConfig,
    stealth_resolver: &Address,
    name: &str,
    owner: &Address,
    meta: &StealthMetaAddress,
    records_resolver: &Address,
) -> anyhow::Result<Vec<Call>> {
    let label = name.split('.').next().unwrap_or_default();
    let dns = dns_encode(name).map_err(|_| anyhow::anyhow!("name"))?;
    let node = namehash(name);
    let init = encode_initialize(
        &[Grant { account: *owner, role_bitmap: or(ALL_ROLES, ADMIN_ROLES) }],
        &[encode_set_text(&dns, &stealth_text_key(1), &meta.to_uri())],
    );
    let zero = [0u8; 32];
    Ok(vec![
        Call { target: cfg.factory, value: zero, data: encode_deploy_proxy(&cfg.permissioned_resolver_impl, &node, &init) },
        Call { target: *stealth_resolver, value: zero, data: encode_set_config(&dns, records_resolver, &[cfg.gateway_url.as_str()]) },
        Call { target: *stealth_resolver, value: zero, data: encode_set_chains(&dns, &[60], true) },
        Call { target: *stealth_resolver, value: zero, data: encode_set_meta_address(&dns, &encode_stealth_record(true, meta)) },
        Call { target: cfg.registry, value: zero, data: encode_set_resolver(&labelhash(label), stealth_resolver) },
    ])
}

// ------------------------------------------------------------------------------ JSON-RPC

async fn rpc(state: &AppState, url: &str, method: &str, params: Value) -> anyhow::Result<Value> {
    let body = json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params });
    // reqwest errors can include the URL (and an API key in it): never format them.
    let resp = state.http.post(url).json(&body).send().await.map_err(|_| anyhow::anyhow!("RPC {method} failed"))?;
    let v: Value = resp.json().await.map_err(|_| anyhow::anyhow!("RPC {method}: not JSON"))?;
    if let Some(e) = v.get("error") {
        let msg = e.get("message").and_then(Value::as_str).unwrap_or("error");
        bail!("RPC {method}: {}", msg.chars().take(200).collect::<String>());
    }
    v.get("result").cloned().context("RPC result missing")
}

fn qty(v: &Value) -> anyhow::Result<u128> {
    let s = v.as_str().context("quantity")?;
    u128::from_str_radix(s.trim_start_matches("0x"), 16).context("quantity")
}

async fn eth_call(state: &AppState, url: &str, to: &Address, data: &[u8]) -> anyhow::Result<Vec<u8>> {
    let r = rpc(state, url, "eth_call", json!([{ "to": h0x(to), "data": h0x(data) }, "latest"])).await?;
    hex::decode(r.as_str().context("eth_call result")?.trim_start_matches("0x")).context("eth_call hex")
}

fn word_u64(ret: &[u8]) -> u64 {
    if ret.len() < 32 {
        return 0;
    }
    u64::from_be_bytes(ret[24..32].try_into().unwrap_or_default())
}

// ------------------------------------------------------------------------------ transactions

/// Minimal RLP (EIP-1559 / EIP-7702 envelopes only).
pub mod rlp {
    pub fn bytes(out: &mut Vec<u8>, b: &[u8]) {
        if b.len() == 1 && b[0] < 0x80 {
            out.push(b[0]);
        } else {
            header(out, 0x80, b.len());
            out.extend_from_slice(b);
        }
    }
    pub fn uint(out: &mut Vec<u8>, v: u128) {
        let be = v.to_be_bytes();
        let skip = be.iter().take_while(|&&x| x == 0).count();
        bytes(out, &be[skip..]);
    }
    /// A big-endian word without leading zeros (r, s).
    pub fn word(out: &mut Vec<u8>, w: &[u8; 32]) {
        let skip = w.iter().take_while(|&&x| x == 0).count();
        bytes(out, &w[skip..]);
    }
    pub fn list(out: &mut Vec<u8>, payload: &[u8]) {
        header(out, 0xc0, payload.len());
        out.extend_from_slice(payload);
    }
    fn header(out: &mut Vec<u8>, base: u8, len: usize) {
        if len < 56 {
            out.push(base + len as u8);
        } else {
            let be = (len as u64).to_be_bytes();
            let skip = be.iter().take_while(|&&x| x == 0).count();
            out.push(base + 55 + (8 - skip) as u8);
            out.extend_from_slice(&be[skip..]);
        }
    }
}

/// A signed EIP-7702 authorisation tuple.
#[derive(Clone, Debug)]
pub struct Authorization {
    pub chain_id: u64,
    pub address: Address,
    pub nonce: u64,
    pub y_parity: u8,
    pub r: [u8; 32],
    pub s: [u8; 32],
}

pub struct TxFields<'a> {
    pub chain_id: u64,
    pub nonce: u64,
    pub max_priority_fee_per_gas: u128,
    pub max_fee_per_gas: u128,
    pub gas: u64,
    pub to: &'a Address,
    pub data: &'a [u8],
    /// Non-empty: a type-4 (EIP-7702) transaction.
    pub authorizations: &'a [Authorization],
}

/// Signs a type-2 (or type-4 with authorisations) transaction. Returns `(raw, hash)`.
pub fn sign_tx(key: &SecretKey, t: &TxFields) -> anyhow::Result<(Vec<u8>, [u8; 32])> {
    let ty: u8 = if t.authorizations.is_empty() { 2 } else { 4 };
    let mut f = Vec::new();
    rlp::uint(&mut f, t.chain_id.into());
    rlp::uint(&mut f, t.nonce.into());
    rlp::uint(&mut f, t.max_priority_fee_per_gas);
    rlp::uint(&mut f, t.max_fee_per_gas);
    rlp::uint(&mut f, t.gas.into());
    rlp::bytes(&mut f, t.to);
    rlp::uint(&mut f, 0);
    rlp::bytes(&mut f, t.data);
    rlp::list(&mut f, &[]); // access list
    if ty == 4 {
        let mut auths = Vec::new();
        for a in t.authorizations {
            let mut x = Vec::new();
            rlp::uint(&mut x, a.chain_id.into());
            rlp::bytes(&mut x, &a.address);
            rlp::uint(&mut x, a.nonce.into());
            rlp::uint(&mut x, a.y_parity.into());
            rlp::word(&mut x, &a.r);
            rlp::word(&mut x, &a.s);
            rlp::list(&mut auths, &x);
        }
        let mut l = Vec::new();
        rlp::list(&mut l, &auths);
        f.extend_from_slice(&l);
    }
    let mut pre = vec![ty];
    rlp::list(&mut pre, &f);
    let sig = sign_digest(key, &keccak256(&pre)).map_err(|_| anyhow::anyhow!("signing failed"))?;
    rlp::uint(&mut f, u128::from(sig[64] - 27));
    rlp::word(&mut f, sig[..32].try_into().expect("32"));
    rlp::word(&mut f, sig[32..64].try_into().expect("32"));
    let mut raw = vec![ty];
    rlp::list(&mut raw, &f);
    let hash = keccak256(&raw);
    Ok((raw, hash))
}

/// Sends one operator transaction and waits for its receipt (serialised by `tx_lock`).
async fn send_and_wait(
    state: &AppState,
    n: &Names,
    url: &str,
    to: &Address,
    data: &[u8],
    auths: &[Authorization],
    fallback_gas: u64,
) -> anyhow::Result<String> {
    let _guard = n.tx_lock.lock().await;
    let from = h0x(&n.operator_address());
    let nonce = qty(&rpc(state, url, "eth_getTransactionCount", json!([from, "pending"])).await?)? as u64;
    let block = rpc(state, url, "eth_getBlockByNumber", json!(["latest", false])).await?;
    let base = block.get("baseFeePerGas").map(qty).transpose()?.unwrap_or(1_000_000_000);
    let tip = rpc(state, url, "eth_maxPriorityFeePerGas", json!([])).await.ok().and_then(|v| qty(&v).ok()).unwrap_or(1_000_000);
    let tip = tip.max(1_000_000); // Sepolia quotes near-zero tips; 0.001 gwei is still cheap.
    let mut call = json!({ "from": from, "to": h0x(to), "data": h0x(data) });
    if !auths.is_empty() {
        call["authorizationList"] = json!(auths.iter().map(|a| json!({
            "chainId": format!("0x{:x}", a.chain_id), "address": h0x(&a.address), "nonce": format!("0x{:x}", a.nonce),
            "yParity": format!("0x{:x}", a.y_parity), "r": h0x(&a.r), "s": h0x(&a.s),
        })).collect::<Vec<_>>());
    }
    // Pad estimates: on Sepolia's schedule, estimates for 7702 batches that deploy contracts
    // come out short (contracts/e2e pads 30 %).
    let gas = match rpc(state, url, "eth_estimateGas", json!([call])).await {
        Ok(v) => (qty(&v)? as u64) * 13 / 10,
        Err(e) => {
            // A revert in simulation would revert on chain too: don't spend gas on it.
            if e.to_string().contains("revert") {
                return Err(e);
            }
            fallback_gas
        }
    };
    let (raw, hash) = sign_tx(
        &n.operator,
        &TxFields {
            chain_id: n.config.chain_id,
            nonce,
            max_priority_fee_per_gas: tip,
            max_fee_per_gas: base * 2 + tip,
            gas,
            to,
            data,
            authorizations: auths,
        },
    )?;
    let hash_hex = h0x(&hash);
    rpc(state, url, "eth_sendRawTransaction", json!([h0x(&raw)])).await?;
    tracing::info!(tx = %hash_hex, "ens names: operator transaction sent");
    for _ in 0..120 {
        tokio::time::sleep(Duration::from_secs(2)).await;
        let r = rpc(state, url, "eth_getTransactionReceipt", json!([hash_hex])).await.unwrap_or(Value::Null);
        if r.is_null() {
            continue;
        }
        if r.get("status").and_then(Value::as_str) != Some("0x1") {
            bail!("transaction {hash_hex} reverted");
        }
        return Ok(hash_hex);
    }
    bail!("transaction {hash_hex} not mined in time")
}

// ------------------------------------------------------------------------------ endpoints

struct Row {
    label: String,
    owner: String,
    status: String,
    register_tx: Option<String>,
    setup_tx: Option<String>,
}

async fn row(state: &AppState, user_id: &str) -> ApiResult<Option<Row>> {
    type R = (String, String, String, Option<String>, Option<String>);
    let r: Option<R> =
        sqlx::query_as("SELECT label, owner, status, register_tx, setup_tx FROM ens_names WHERE user_id = $1")
            .bind(user_id)
            .fetch_optional(&state.db)
            .await?;
    Ok(r.map(|(label, owner, status, register_tx, setup_tx)| Row { label, owner, status, register_tx, setup_tx }))
}

async fn set_status(state: &AppState, user_id: &str, status: &str, col: Option<(&str, &str)>) -> ApiResult<()> {
    let t = now();
    match col {
        Some(("register_tx", tx)) => {
            sqlx::query("UPDATE ens_names SET status = $1, register_tx = $2, updated_at = $3 WHERE user_id = $4")
                .bind(status)
                .bind(tx)
                .bind(t)
                .bind(user_id)
                .execute(&state.db)
                .await?;
        }
        Some((_, tx)) => {
            sqlx::query("UPDATE ens_names SET status = $1, setup_tx = $2, updated_at = $3 WHERE user_id = $4")
                .bind(status)
                .bind(tx)
                .bind(t)
                .bind(user_id)
                .execute(&state.db)
                .await?;
        }
        None => {
            sqlx::query("UPDATE ens_names SET status = $1, updated_at = $2 WHERE user_id = $3")
                .bind(status)
                .bind(t)
                .bind(user_id)
                .execute(&state.db)
                .await?;
        }
    }
    Ok(())
}

fn row_json(r: &Row, parent: &str) -> Value {
    json!({
        "label": r.label,
        "name": format!("{}.{parent}", r.label),
        "owner": r.owner,
        "status": r.status,
        "register_tx": r.register_tx,
        "setup_tx": r.setup_tx,
    })
}

/// `GET /v1/names/me` -> `{name: null}` or the user's name and status.
pub async fn me(State(state): State<AppState>, AuthSession(s): AuthSession) -> ApiResult<Json<Value>> {
    let parent = state.ens.as_ref().map(|g| g.config.parent.clone()).unwrap_or_default();
    let enabled = state.names.is_some() && state.ens.is_some();
    Ok(Json(match row(&state, &s.user_id).await? {
        Some(r) => {
            let mut v = row_json(&r, &parent);
            v["enabled"] = json!(enabled);
            v
        }
        None => json!({ "name": null, "parent": parent, "enabled": enabled }),
    }))
}

fn check_label(label: &str) -> ApiResult<()> {
    validate_label(label).map_err(|_| {
        ApiError::new(
            StatusCode::BAD_REQUEST,
            "invalid_label",
            "use 3 to 32 lowercase letters, digits or hyphens, not starting or ending with a hyphen",
        )
    })
}

#[derive(Deserialize)]
pub struct PrepareReq {
    pub label: String,
    pub owner: String,
}

/// `POST /v1/names/prepare`.
pub async fn prepare(
    State(state): State<AppState>,
    ip: ClientIp,
    AuthSession(s): AuthSession,
    Json(req): Json<PrepareReq>,
) -> ApiResult<Json<Value>> {
    let (n, gw) = names(&state)?;
    state.limiter.check("names-prepare", &s.user_id, Limit::new(60, 3600))?;
    state.limiter.check("names-prepare-ip", &ip.0, Limit::new(120, 3600))?;
    check_label(&req.label)?;
    let owner = addr_param(&req.owner, "owner")?;
    let stealth_resolver = gw.stealth_resolver.ok_or_else(|| ApiError::not_found("no StealthResolver configured"))?;
    let existing = row(&state, &s.user_id).await?;
    if let Some(r) = &existing
        && (r.label != req.label || !r.owner.eq_ignore_ascii_case(&h0x(&owner)))
        && r.status != "failed"
    {
        return Err(ApiError::new(StatusCode::CONFLICT, "has_name", "you already have a name"));
    }
    let already_ours = existing.as_ref().is_some_and(|r| r.label == req.label && r.status != "failed" && r.status != "registering");
    if !already_ours {
        let taken: Option<(String,)> = sqlx::query_as("SELECT user_id FROM ens_names WHERE label = $1")
            .bind(&req.label)
            .fetch_optional(&state.db)
            .await?;
        if taken.is_some_and(|(u,)| u != s.user_id) {
            return Err(ApiError::new(StatusCode::CONFLICT, "taken", "that name is taken"));
        }
        let avail = eth_call(&state, &gw.rpc_url, &n.config.registrar, &encode_available(&req.label)).await.map_err(upstream)?;
        if word_u64(&avail) != 1 {
            return Err(ApiError::new(StatusCode::CONFLICT, "taken", "that name is taken"));
        }
    }
    let name = format!("{}.{}", req.label, gw.parent);
    let reg_nonce = word_u64(&eth_call(&state, &gw.rpc_url, &n.config.registrar, &encode_nonces(&owner)).await.map_err(upstream)?);
    let auth_nonce = qty(&rpc(&state, &gw.rpc_url, "eth_getTransactionCount", json!([h0x(&owner), "pending"])).await.map_err(upstream)?)
        .map_err(upstream)?;
    // LowkiOwnerAccount.nonce() on the owner: empty before the first delegation (= 0).
    let exec_nonce = eth_call(&state, &gw.rpc_url, &owner, &encode_nonce()).await.map(|r| word_u64(&r)).unwrap_or(0);
    let predicted = eth_call(&state, &gw.rpc_url, &n.config.factory, &encode_predict_proxy_address(&owner, &namehash(&name)))
        .await
        .map_err(upstream)?;
    let records_resolver: Address = predicted.get(12..32).and_then(|b| b.try_into().ok()).ok_or_else(ApiError::internal)?;
    let deadline = u64::try_from(now()).unwrap_or(0) + 1800;
    Ok(Json(json!({
        "label": req.label,
        "name": name,
        "owner": h0x(&owner),
        "chain_id": n.config.chain_id,
        "registrar": h0x(&n.config.registrar),
        "registry": h0x(&n.config.registry),
        "owner_account": h0x(&n.config.owner_account),
        "stealth_resolver": h0x(&stealth_resolver),
        "factory": h0x(&n.config.factory),
        "permissioned_resolver_impl": h0x(&n.config.permissioned_resolver_impl),
        "gateway_url": n.config.gateway_url,
        "records_resolver": h0x(&records_resolver),
        "duration": n.config.duration_secs,
        "registration_nonce": reg_nonce,
        "auth_nonce": auth_nonce as u64,
        "exec_nonce": exec_nonce,
        "deadline": deadline,
        "status": existing.map(|r| r.status),
    })))
}

#[derive(Deserialize)]
pub struct RegisterReq {
    pub label: String,
    pub owner: String,
    pub nonce: u64,
    pub deadline: u64,
    pub signature: String,
}

/// `POST /v1/names/register`.
pub async fn register(
    State(state): State<AppState>,
    ip: ClientIp,
    AuthSession(s): AuthSession,
    Json(req): Json<RegisterReq>,
) -> ApiResult<Json<Value>> {
    let (n, gw) = names(&state)?;
    state.limiter.check("names-claim", &s.user_id, n.config.claims_per_user)?;
    state.limiter.check("names-claim-ip", &ip.0, n.config.claims_per_ip)?;
    check_label(&req.label)?;
    let owner = addr_param(&req.owner, "owner")?;
    let r = Registration {
        label: req.label.clone(),
        owner,
        resolver: [0u8; 20],
        duration: n.config.duration_secs,
        nonce: req.nonce,
        deadline: req.deadline,
    };
    let sig = bytes_param(&req.signature, "signature")?;
    match recover_registration(n.config.chain_id, &n.config.registrar, &r, &sig) {
        Ok(a) if a == owner => {}
        _ => return Err(ApiError::bad_request("the registration signature is not by the owner")),
    }
    if let Some(existing) = row(&state, &s.user_id).await? {
        if existing.label != req.label || !existing.owner.eq_ignore_ascii_case(&h0x(&owner)) {
            if existing.status != "failed" {
                return Err(ApiError::new(StatusCode::CONFLICT, "has_name", "you already have a name"));
            }
            sqlx::query("DELETE FROM ens_names WHERE user_id = $1 AND status = 'failed'")
                .bind(&s.user_id)
                .execute(&state.db)
                .await?;
        } else if existing.status == "registered" || existing.status == "ready" {
            return Ok(Json(row_json(&existing, &gw.parent)));
        } else if existing.status == "registering" {
            return Err(ApiError::conflict("this name is being registered"));
        }
    }
    let t = now();
    let ins = sqlx::query(
        "INSERT INTO ens_names (user_id, label, owner, status, created_at, updated_at) VALUES ($1, $2, $3, 'registering', $4, $4) \
         ON CONFLICT (user_id) DO UPDATE SET status = 'registering', updated_at = excluded.updated_at WHERE ens_names.status = 'failed'",
    )
    .bind(&s.user_id)
    .bind(&req.label)
    .bind(h0x(&owner))
    .bind(t)
    .execute(&state.db)
    .await;
    match ins {
        Ok(d) if d.rows_affected() == 1 => {}
        Ok(_) => return Err(ApiError::new(StatusCode::CONFLICT, "has_name", "you already have a name")),
        Err(_) => return Err(ApiError::new(StatusCode::CONFLICT, "taken", "that name is taken")),
    }
    match send_and_wait(&state, n, &gw.rpc_url, &n.config.registrar, &encode_register(&r, &sig), &[], 1_000_000).await {
        Ok(tx) => {
            set_status(&state, &s.user_id, "registered", Some(("register_tx", &tx))).await?;
            crate::audit::record(&state, Some(&s.user_id), "name.registered", json!({ "tx": tx })).await;
            Ok(Json(row_json(&row(&state, &s.user_id).await?.ok_or_else(ApiError::internal)?, &gw.parent)))
        }
        Err(e) => {
            set_status(&state, &s.user_id, "failed", None).await?;
            Err(upstream(e))
        }
    }
}

#[derive(Deserialize)]
pub struct AuthorizationIn {
    #[serde(rename = "chainId")]
    pub chain_id: String,
    pub address: String,
    pub nonce: String,
    #[serde(rename = "yParity")]
    pub y_parity: u8,
    pub r: String,
    pub s: String,
}

#[derive(Deserialize)]
pub struct SetupReq {
    /// The ENSIP-30 record `0x01 || spendPub || viewPub` (or the raw 66-byte meta-address).
    pub record: String,
    pub exec_nonce: u64,
    pub deadline: u64,
    pub execute_signature: String,
    pub authorization: AuthorizationIn,
}

fn word32(s: &str, what: &str) -> ApiResult<[u8; 32]> {
    let b = bytes_param(s, what)?;
    if b.len() > 32 {
        return Err(ApiError::bad_request(format!("{what} too long")));
    }
    let mut w = [0u8; 32];
    w[32 - b.len()..].copy_from_slice(&b);
    Ok(w)
}

/// `POST /v1/names/setup`.
pub async fn setup(
    State(state): State<AppState>,
    ip: ClientIp,
    AuthSession(s): AuthSession,
    Json(req): Json<SetupReq>,
) -> ApiResult<Json<Value>> {
    let (n, gw) = names(&state)?;
    state.limiter.check("names-claim", &s.user_id, n.config.claims_per_user)?;
    state.limiter.check("names-claim-ip", &ip.0, n.config.claims_per_ip)?;
    let r = row(&state, &s.user_id).await?.ok_or_else(|| ApiError::not_found("claim a name first"))?;
    if r.status == "ready" {
        return Ok(Json(row_json(&r, &gw.parent)));
    }
    if r.status != "registered" {
        return Err(ApiError::conflict("the name is not registered yet"));
    }
    let owner = addr_param(&r.owner, "owner")?;
    let stealth_resolver = gw.stealth_resolver.ok_or_else(|| ApiError::not_found("no StealthResolver configured"))?;
    let raw = bytes_param(&req.record, "record")?;
    let meta_raw = match raw.len() {
        67 if raw[0] & !1 == 0 => &raw[1..],
        66 => &raw[..],
        _ => return Err(ApiError::bad_request("record must be 0x01 || spendPub || viewPub")),
    };
    let meta = StealthMetaAddress::from_raw(meta_raw).map_err(|_| ApiError::bad_request("record is not a valid meta-address"))?;
    let name = format!("{}.{}", r.label, gw.parent);
    let predicted = eth_call(&state, &gw.rpc_url, &n.config.factory, &encode_predict_proxy_address(&owner, &namehash(&name)))
        .await
        .map_err(upstream)?;
    let records_resolver: Address = predicted.get(12..32).and_then(|b| b.try_into().ok()).ok_or_else(ApiError::internal)?;
    // Idempotent: if an earlier setup landed but its result was never recorded (a restart
    // mid-request), the owner's records resolver already exists. Don't resend stale nonces.
    let deployed = rpc(&state, &gw.rpc_url, "eth_getCode", json!([h0x(&records_resolver), "latest"]))
        .await
        .ok()
        .and_then(|v| v.as_str().map(|c| c.len() > 2))
        .unwrap_or(false);
    if deployed {
        set_status(&state, &s.user_id, "ready", None).await?;
        crate::audit::record(&state, Some(&s.user_id), "name.ready", json!({ "tx": null, "recovered": true })).await;
        return Ok(Json(row_json(&row(&state, &s.user_id).await?.ok_or_else(ApiError::internal)?, &gw.parent)));
    }
    let calls = owner_setup_calls(&n.config, &stealth_resolver, &name, &owner, &meta, &records_resolver)
        .map_err(|_| ApiError::internal())?;
    let sig = bytes_param(&req.execute_signature, "execute_signature")?;
    let digest = execute_digest(n.config.chain_id, &owner, &calls, req.exec_nonce, req.deadline);
    if recover_digest(&digest, &sig).ok() != Some(owner) {
        return Err(ApiError::bad_request("the batch signature is not by the owner over this name's setup"));
    }
    let a = &req.authorization;
    let auth = Authorization {
        chain_id: a.chain_id.parse().map_err(|_| ApiError::bad_request("authorization.chainId"))?,
        address: addr_param(&a.address, "authorization.address")?,
        nonce: a.nonce.parse().map_err(|_| ApiError::bad_request("authorization.nonce"))?,
        y_parity: a.y_parity,
        r: word32(&a.r, "authorization.r")?,
        s: word32(&a.s, "authorization.s")?,
    };
    if auth.chain_id != n.config.chain_id || auth.address != n.config.owner_account || auth.y_parity > 1 {
        return Err(ApiError::bad_request("the authorisation must delegate to LowkiOwnerAccount on this chain"));
    }
    let mut asig = auth.r.to_vec();
    asig.extend_from_slice(&auth.s);
    asig.push(27 + auth.y_parity);
    if recover_digest(&authorization_digest(auth.chain_id, &auth.address, auth.nonce), &asig).ok() != Some(owner) {
        return Err(ApiError::bad_request("the authorisation is not signed by the owner"));
    }
    let data = encode_execute(&calls, req.deadline, &sig);
    match send_and_wait(&state, n, &gw.rpc_url, &owner, &data, std::slice::from_ref(&auth), 4_500_000).await {
        Ok(tx) => {
            set_status(&state, &s.user_id, "ready", Some(("setup_tx", &tx))).await?;
            crate::audit::record(&state, Some(&s.user_id), "name.ready", json!({ "tx": tx })).await;
            Ok(Json(row_json(&row(&state, &s.user_id).await?.ok_or_else(ApiError::internal)?, &gw.parent)))
        }
        Err(e) => Err(upstream(e)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn config_debug_redacts_the_key() {
        let c = NamesConfig {
            operator_key: [0x42; 32],
            chain_id: 1,
            registrar: [1; 20],
            registry: [2; 20],
            owner_account: [3; 20],
            factory: [4; 20],
            permissioned_resolver_impl: [5; 20],
            gateway_url: "https://gw/v1/ens/gateway".into(),
            duration_secs: 1,
            claims_per_ip: Limit::new(1, 1),
            claims_per_user: Limit::new(1, 1),
        };
        let d = format!("{c:?}");
        assert!(d.contains("redacted") && !d.contains("4242"), "{d}");
    }

    #[test]
    fn rlp_basics() {
        let mut o = vec![];
        rlp::uint(&mut o, 0);
        rlp::uint(&mut o, 127);
        rlp::uint(&mut o, 128);
        rlp::bytes(&mut o, &[]);
        assert_eq!(o, vec![0x80, 0x7f, 0x81, 0x80, 0x80]);
        let mut l = vec![];
        rlp::list(&mut l, &[0u8; 60]);
        assert_eq!(&l[..2], &[0xf8, 60]);
    }
}
