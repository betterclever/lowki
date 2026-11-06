//! Chain access for balances, nonces, gas, receipts and broadcasting, behind a small trait.
//!
//! The default provider is [`RoutedRpc`]: every call goes through Lowki's Anon-RPC glue
//! (`lowki-glue/lowki-glue.js`, built from `packages/lowki-kohaku-host/browser-wallet`), which
//! boots `@anon-rpc/browser-harness` via `createAnonProvider` (Nox on Chromium, Nym on
//! Safari/WebKit) and sends the JSON-RPC through it. The connection is direct only when:
//! - the URL carries `rpc=direct` (dev override);
//! - the config names no RPC the anon network can reach (e.g. a localhost fork without a tunnel);
//! - the anon network fails to boot (the glue's fallback, reported with its reason);
//! - the glue script didn't load.
//!
//! [`connection_status`] reports which, for the UI. Nothing falls back silently.

use std::cell::{Cell, RefCell};

use js_sys::{Function, Promise, Reflect};
use serde::Deserialize;
use serde_json::{Value, json};
use wasm_bindgen::{JsCast, JsValue};
use wasm_bindgen_futures::JsFuture;

use super::codec::parse_qty;
use crate::backend::BoxFut;

/// Default: Sepolia over a public RPC. Override at build time with `LOWKI_RPC_URL` and
/// `LOWKI_CHAIN_ID`, or at run time with a same-origin `lowki-config.json` (the demo stack
/// serves one). With Anon-RPC on, the RPC is reached from the anon network's exits.
pub const RPC_URL: &str = match option_env!("LOWKI_RPC_URL") {
    Some(u) => u,
    None => "https://ethereum-sepolia-rpc.publicnode.com",
};
pub const CHAIN_ID: u64 = 11_155_111;
pub const NETWORK: &str = "Sepolia";
/// Circle's USDC on Sepolia.
pub const USDC: &str = "0x1c7d4b196cb0c7b01d743fbc6116a902379c7238";
/// Same-origin runtime config read by the glue (optional).
pub const CONFIG_URL: &str = "lowki-config.json";

#[derive(Debug)]
pub struct ChainError(pub String);

/// Fees for an EIP-1559 transaction.
#[derive(Clone, Copy, Debug)]
pub struct Fees {
    pub max_fee_per_gas: u128,
    pub max_priority_fee_per_gas: u128,
}

pub trait ChainProvider {
    fn chain_id(&self) -> u64;
    fn network(&self) -> String;
    fn balance(&self, address: &str) -> BoxFut<'_, Result<u128, ChainError>>;
    /// `eth_call` returning raw hex.
    fn call(&self, to: &str, data: &str) -> BoxFut<'_, Result<String, ChainError>>;
    fn nonce(&self, address: &str) -> BoxFut<'_, Result<u128, ChainError>>;
    fn estimate_gas(&self, from: &str, to: &str, value: u128, data: &str) -> BoxFut<'_, Result<u128, ChainError>>;
    fn fees(&self) -> BoxFut<'_, Result<Fees, ChainError>>;
    /// Broadcasts a signed raw transaction. Returns the hash.
    fn send_raw(&self, raw: &str) -> BoxFut<'_, Result<String, ChainError>>;
    /// `eth_getTransactionReceipt`; `None` while pending.
    fn receipt(&self, hash: &str) -> BoxFut<'_, Result<Option<Value>, ChainError>>;
}

/// How the wallet reaches the chain right now (shown on Home).
#[derive(Clone, Debug, Default, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct ConnStatus {
    /// "connecting" or "ready".
    #[serde(default)]
    pub state: String,
    #[serde(default)]
    pub anonymous: bool,
    /// nox, nym or tor-js.
    #[serde(default)]
    pub anon_network: Option<String>,
    /// Why the connection is direct.
    #[serde(default)]
    pub reason: Option<String>,
    #[serde(default)]
    pub boot_ms: Option<f64>,
    #[serde(default)]
    pub chain_id: u64,
    #[serde(default)]
    pub network: String,
    #[serde(default)]
    pub rpc_host: Option<String>,
    /// The IP the RPC host saw, asked over the same path as the reads.
    #[serde(default)]
    pub egress_ip: Option<String>,
}

thread_local! {
    static STATUS: RefCell<ConnStatus> = RefCell::new(ConnStatus { state: "connecting".into(), ..Default::default() });
}

/// The latest connection status (the glue's, or the Rust fallback's).
pub fn connection_status() -> ConnStatus {
    if let Some(g) = glue()
        && let Ok(f) = Reflect::get(&g, &"status".into()).and_then(|f| f.dyn_into::<Function>())
        && let Ok(v) = f.call0(&g)
        && let Some(s) = js_sys::JSON::stringify(&v).ok().and_then(|s| s.as_string())
        && let Ok(st) = serde_json::from_str::<ConnStatus>(&s)
        && st.state == "ready"
    {
        return st;
    }
    STATUS.with(|s| s.borrow().clone())
}

fn set_status(s: ConnStatus) {
    STATUS.with(|c| *c.borrow_mut() = s);
}

/// `globalThis.lowkiGlue`, once its module script has run.
pub fn glue() -> Option<JsValue> {
    let g = Reflect::get(&js_sys::global(), &"lowkiGlue".into()).ok()?;
    (!g.is_undefined() && !g.is_null()).then_some(g)
}

/// Waits up to `ms` for the glue module (a module script, so it may run after the wasm starts).
pub async fn wait_glue(ms: u32) -> Option<JsValue> {
    let mut waited = 0;
    loop {
        if let Some(g) = glue() {
            return Some(g);
        }
        if waited >= ms {
            return None;
        }
        gloo_timers::future::TimeoutFuture::new(50).await;
        waited += 50;
    }
}

/// Calls `obj[name](...args)` and awaits the returned promise.
pub async fn call_method(obj: &JsValue, name: &str, args: &[JsValue]) -> Result<JsValue, String> {
    let f: Function = Reflect::get(obj, &name.into())
        .ok()
        .and_then(|f| f.dyn_into().ok())
        .ok_or_else(|| format!("glue has no {name}()"))?;
    let arr = js_sys::Array::new();
    for a in args {
        arr.push(a);
    }
    let r = f.apply(obj, &arr).map_err(js_err)?;
    match r.dyn_into::<Promise>() {
        Ok(p) => JsFuture::from(p).await.map_err(js_err),
        Err(v) => Ok(v),
    }
}

/// The message of a thrown JS error.
pub fn js_err(e: JsValue) -> String {
    Reflect::get(&e, &"message".into())
        .ok()
        .and_then(|m| m.as_string())
        .or_else(|| e.as_string())
        .unwrap_or_else(|| format!("{e:?}"))
}

/// `?rpc=direct` (also accepted inside `walletflags` by the demo dapp, which forwards them).
fn direct_requested() -> bool {
    let search = web_sys::window().and_then(|w| w.location().search().ok()).unwrap_or_default();
    web_sys::UrlSearchParams::new_with_str(&search).ok().and_then(|q| q.get("rpc")).as_deref() == Some("direct")
}

/// Plain JSON-RPC over `fetch` (the fallback when the glue is missing).
pub struct JsonRpc {
    url: String,
    id: Cell<u32>,
}

impl JsonRpc {
    pub fn new(url: &str) -> Self {
        Self { url: url.into(), id: Cell::new(1) }
    }

    async fn rpc(&self, method: &str, params: Value) -> Result<Value, ChainError> {
        let id = self.id.get();
        self.id.set(id + 1);
        let body = json!({ "jsonrpc": "2.0", "id": id, "method": method, "params": params });
        let init = web_sys::RequestInit::new();
        init.set_method("POST");
        init.set_credentials(web_sys::RequestCredentials::Omit);
        init.set_referrer_policy(web_sys::ReferrerPolicy::NoReferrer);
        let headers = web_sys::Headers::new().map_err(|_| ChainError("headers".into()))?;
        let _ = headers.set("content-type", "application/json");
        init.set_headers(&headers);
        init.set_body(&JsValue::from_str(&body.to_string()));
        let req = web_sys::Request::new_with_str_and_init(&self.url, &init).map_err(|_| ChainError("request".into()))?;
        let win = web_sys::window().ok_or_else(|| ChainError("no window".into()))?;
        let resp = JsFuture::from(win.fetch_with_request(&req)).await.map_err(|_| ChainError("unreachable".into()))?;
        let resp: web_sys::Response = resp.dyn_into().map_err(|_| ChainError("response".into()))?;
        let text = JsFuture::from(resp.text().map_err(|_| ChainError("body".into()))?)
            .await
            .ok()
            .and_then(|t| t.as_string())
            .unwrap_or_default();
        let v: Value = serde_json::from_str(&text).map_err(|_| ChainError("not json".into()))?;
        if let Some(e) = v.get("error") {
            return Err(ChainError(e.get("message").and_then(Value::as_str).unwrap_or("rpc error").to_string()));
        }
        v.get("result").cloned().ok_or_else(|| ChainError("no result".into()))
    }
}

enum Route {
    Glue(JsValue),
    Direct(JsonRpc),
}

/// The default provider: Anon-RPC through the glue, direct only for a reported reason.
pub struct RoutedRpc {
    /// Resolves once the glue connected (or Rust fell back to direct).
    route: RefCell<Option<std::rc::Rc<Route>>>,
    connecting: Cell<bool>,
    chain_id: Cell<u64>,
    network: RefCell<String>,
}

impl Default for RoutedRpc {
    fn default() -> Self {
        Self::new()
    }
}

impl RoutedRpc {
    pub fn new() -> Self {
        let chain_id = option_env!("LOWKI_CHAIN_ID").and_then(|s| s.parse().ok()).unwrap_or(CHAIN_ID);
        Self {
            route: RefCell::new(None),
            connecting: Cell::new(false),
            chain_id: Cell::new(chain_id),
            network: RefCell::new(option_env!("LOWKI_NETWORK").unwrap_or(NETWORK).to_string()),
        }
    }

    /// Connects once; concurrent callers wait for the first.
    async fn ready(&self) -> std::rc::Rc<Route> {
        loop {
            if let Some(r) = self.route.borrow().clone() {
                return r;
            }
            if !self.connecting.get() {
                break;
            }
            gloo_timers::future::TimeoutFuture::new(50).await;
        }
        self.connecting.set(true);
        let route = self.connect().await;
        let route = std::rc::Rc::new(route);
        *self.route.borrow_mut() = Some(route.clone());
        self.connecting.set(false);
        route
    }

    async fn connect(&self) -> Route {
        let defaults = json!({
            "chainId": self.chain_id.get(),
            "network": *self.network.borrow(),
            "rpcUrl": RPC_URL,
        });
        let Some(g) = wait_glue(8_000).await else {
            set_status(ConnStatus {
                state: "ready".into(),
                reason: Some("The Anon-RPC module (lowki-glue) didn't load.".into()),
                chain_id: self.chain_id.get(),
                network: self.network.borrow().clone(),
                rpc_host: RPC_URL.split('/').nth(2).map(str::to_string),
                ..Default::default()
            });
            web_sys::console::warn_1(&"[lowki] lowki-glue missing: reading the chain directly".into());
            return Route::Direct(JsonRpc::new(RPC_URL));
        };
        let opts = js_sys::JSON::parse(
            &json!({
                "mode": if direct_requested() { "direct" } else { "anon" },
                "configUrl": CONFIG_URL,
                "defaults": defaults,
            })
            .to_string(),
        )
        .unwrap_or(JsValue::UNDEFINED);
        match call_method(&g, "connect", &[opts]).await {
            Ok(v) => {
                let s = js_sys::JSON::stringify(&v).ok().and_then(|s| s.as_string()).unwrap_or_default();
                if let Ok(st) = serde_json::from_str::<ConnStatus>(&s) {
                    if st.chain_id != 0 {
                        self.chain_id.set(st.chain_id);
                    }
                    if !st.network.is_empty() {
                        *self.network.borrow_mut() = st.network.clone();
                    }
                    web_sys::console::info_1(
                        &format!(
                            "[lowki] chain connection: {} ({})",
                            if st.anonymous { "anon" } else { "direct" },
                            st.anon_network.clone().or(st.reason.clone()).unwrap_or_default()
                        )
                        .into(),
                    );
                    set_status(st);
                }
                Route::Glue(g)
            }
            Err(e) => {
                set_status(ConnStatus {
                    state: "ready".into(),
                    reason: Some(format!("The Anon-RPC module failed: {e}")),
                    chain_id: self.chain_id.get(),
                    network: self.network.borrow().clone(),
                    ..Default::default()
                });
                Route::Direct(JsonRpc::new(RPC_URL))
            }
        }
    }

    async fn rpc(&self, method: &str, params: Value) -> Result<Value, ChainError> {
        match &*self.ready().await {
            Route::Direct(d) => d.rpc(method, params).await,
            Route::Glue(g) => {
                let r = call_method(g, "rpc", &[JsValue::from_str(method), JsValue::from_str(&params.to_string())])
                    .await
                    .map_err(ChainError)?;
                serde_json::from_str(&r.as_string().unwrap_or_default()).map_err(|_| ChainError("not json".into()))
            }
        }
    }

    async fn qty(&self, method: &str, params: Value) -> Result<u128, ChainError> {
        let r = self.rpc(method, params).await?;
        r.as_str().and_then(parse_qty).ok_or_else(|| ChainError("bad quantity".into()))
    }
}

fn q(n: u128) -> String {
    format!("0x{n:x}")
}

impl ChainProvider for RoutedRpc {
    fn chain_id(&self) -> u64 {
        self.chain_id.get()
    }
    fn network(&self) -> String {
        self.network.borrow().clone()
    }
    fn balance(&self, address: &str) -> BoxFut<'_, Result<u128, ChainError>> {
        let a = address.to_string();
        Box::pin(async move { self.qty("eth_getBalance", json!([a, "latest"])).await })
    }
    fn call(&self, to: &str, data: &str) -> BoxFut<'_, Result<String, ChainError>> {
        let (to, data) = (to.to_string(), data.to_string());
        Box::pin(async move {
            let r = self.rpc("eth_call", json!([{ "to": to, "data": data }, "latest"])).await?;
            r.as_str().map(str::to_string).ok_or_else(|| ChainError("bad call result".into()))
        })
    }
    fn nonce(&self, address: &str) -> BoxFut<'_, Result<u128, ChainError>> {
        let a = address.to_string();
        Box::pin(async move { self.qty("eth_getTransactionCount", json!([a, "pending"])).await })
    }
    fn estimate_gas(&self, from: &str, to: &str, value: u128, data: &str) -> BoxFut<'_, Result<u128, ChainError>> {
        let p = json!([{ "from": from, "to": to, "value": q(value), "data": if data.is_empty() { "0x" } else { data } }]);
        Box::pin(async move { self.qty("eth_estimateGas", p).await })
    }
    fn fees(&self) -> BoxFut<'_, Result<Fees, ChainError>> {
        Box::pin(async move {
            let block = self.rpc("eth_getBlockByNumber", json!(["latest", false])).await?;
            let base = block.get("baseFeePerGas").and_then(Value::as_str).and_then(parse_qty).unwrap_or(0);
            let tip = self.qty("eth_maxPriorityFeePerGas", json!([])).await.unwrap_or(1_000_000_000);
            Ok(Fees { max_fee_per_gas: base * 2 + tip, max_priority_fee_per_gas: tip })
        })
    }
    fn send_raw(&self, raw: &str) -> BoxFut<'_, Result<String, ChainError>> {
        let raw = raw.to_string();
        Box::pin(async move {
            let r = self.rpc("eth_sendRawTransaction", json!([raw])).await?;
            r.as_str().map(str::to_string).ok_or_else(|| ChainError("bad hash".into()))
        })
    }
    fn receipt(&self, hash: &str) -> BoxFut<'_, Result<Option<Value>, ChainError>> {
        let h = hash.to_string();
        Box::pin(async move {
            let r = self.rpc("eth_getTransactionReceipt", json!([h])).await?;
            Ok((!r.is_null()).then_some(r))
        })
    }
}

/// `balanceOf(address)` calldata.
pub fn balance_of_data(owner: &str) -> String {
    format!("0x70a08231{:0>64}", owner.trim_start_matches("0x").to_lowercase())
}

/// `transfer(address,uint256)` calldata.
pub fn transfer_data(to: &str, amount: u128) -> String {
    format!("0xa9059cbb{:0>64}{:064x}", to.trim_start_matches("0x").to_lowercase(), amount)
}

#[cfg(test)]
mod tests {
    #[test]
    fn calldata() {
        assert_eq!(
            super::transfer_data("0x00000000000000000000000000000000000000aA", 1_000_000),
            "0xa9059cbb00000000000000000000000000000000000000000000000000000000000000aa00000000000000000000000000000000000000000000000000000000000f4240"
        );
        assert_eq!(super::balance_of_data("0xAB").len(), 2 + 8 + 64);
    }

    #[test]
    fn status_parses_glue_json() {
        let s: super::ConnStatus = serde_json::from_str(
            r#"{"state":"ready","anonymous":true,"anonNetwork":"nox","bootMs":3100,"chainId":11155111,"network":"Sepolia fork","rpcHost":"x.trycloudflare.com","egressIp":"98.92.70.228","calls":{"anon":3,"direct":0}}"#,
        )
        .unwrap();
        assert!(s.anonymous);
        assert_eq!(s.anon_network.as_deref(), Some("nox"));
        assert_eq!(s.egress_ip.as_deref(), Some("98.92.70.228"));
    }
}
