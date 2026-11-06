//! Chain access for balances and broadcasting, behind a small trait so another provider
//! (e.g. Kohaku's anon RPC) can replace plain JSON-RPC without touching the backend.

use std::cell::Cell;

use serde_json::{Value, json};
use wasm_bindgen::JsCast;
use wasm_bindgen_futures::JsFuture;

use super::codec::parse_qty;
use crate::backend::BoxFut;

/// Default: Sepolia over a public RPC. Override at build time with `LOWKI_RPC_URL` and
/// `LOWKI_CHAIN_ID` (and allow the RPC origin in `index.html`'s CSP `connect-src`).
pub const RPC_URL: &str = match option_env!("LOWKI_RPC_URL") {
    Some(u) => u,
    None => "https://ethereum-sepolia-rpc.publicnode.com",
};
pub const CHAIN_ID: u64 = 11_155_111;
pub const NETWORK: &str = "Sepolia";
/// Circle's USDC on Sepolia.
pub const USDC: &str = "0x1c7d4b196cb0c7b01d743fbc6116a902379c7238";

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
    fn network(&self) -> &str;
    fn balance(&self, address: &str) -> BoxFut<'_, Result<u128, ChainError>>;
    /// `eth_call` returning raw hex.
    fn call(&self, to: &str, data: &str) -> BoxFut<'_, Result<String, ChainError>>;
    fn nonce(&self, address: &str) -> BoxFut<'_, Result<u128, ChainError>>;
    fn estimate_gas(&self, from: &str, to: &str, value: u128, data: &str) -> BoxFut<'_, Result<u128, ChainError>>;
    fn fees(&self) -> BoxFut<'_, Result<Fees, ChainError>>;
    /// Broadcasts a signed raw transaction. Returns the hash.
    fn send_raw(&self, raw: &str) -> BoxFut<'_, Result<String, ChainError>>;
}

pub struct JsonRpc {
    url: String,
    chain_id: u64,
    network: String,
    id: Cell<u32>,
}

impl JsonRpc {
    pub fn new(url: &str, chain_id: u64, network: &str) -> Self {
        Self { url: url.into(), chain_id, network: network.into(), id: Cell::new(1) }
    }

    pub fn default_sepolia() -> Self {
        let chain_id = option_env!("LOWKI_CHAIN_ID").and_then(|s| s.parse().ok()).unwrap_or(CHAIN_ID);
        Self::new(RPC_URL, chain_id, option_env!("LOWKI_NETWORK").unwrap_or(NETWORK))
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
        init.set_body(&wasm_bindgen::JsValue::from_str(&body.to_string()));
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

    async fn qty(&self, method: &str, params: Value) -> Result<u128, ChainError> {
        let r = self.rpc(method, params).await?;
        r.as_str().and_then(parse_qty).ok_or_else(|| ChainError("bad quantity".into()))
    }
}

fn q(n: u128) -> String {
    format!("0x{n:x}")
}

impl ChainProvider for JsonRpc {
    fn chain_id(&self) -> u64 {
        self.chain_id
    }
    fn network(&self) -> &str {
        &self.network
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
}
