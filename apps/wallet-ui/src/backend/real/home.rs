//! The wallet home (top-level page on the Lowki origin) for the real backend:
//!
//! - **Your apps.** Each time the wallet issues an app token it adds that app to the user's app
//!   list. The list is JSON encrypted in the key worker (`appListSeal`, a key derived from the
//!   seed) and stored as an opaque blob at `PUT /v1/me/app-list`; the server can't read it.
//!   The home opens it (`appListOpen`) and derives each address with `appAccount(origin)`.
//! - **Your name.** `ensIdentity` -> `POST /v1/names/prepare` -> `ensSignRegistration` +
//!   `ensSignOwnerSetup` -> `POST /v1/names/register` -> `POST /v1/names/setup`. The operator
//!   pays; the identity key (`m/137'/0'/0'`) only signs.
//! - **Payments to the name.** `ensRecordsAuth` (viewing key) -> `GET /v1/ens/records` ->
//!   `stealthScan` in the worker -> balances over the routed (Anon-RPC) provider.
//! - **Move to private.** A pool deposit from the stealth address, signed in the worker
//!   (`stealthSignTransaction`; the stealth key never leaves it).

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use wasm_bindgen::JsValue;

use super::js::{get, get_str, obj};
use super::*;

/// Pool minimum deposit on Sepolia (docs/PRIVATE-SEND.md).
pub const POOL_MIN_DEPOSIT: u128 = 1_000_000_000_000_000;
/// Gas budget for a pool deposit from a stealth address (measured ~180k; padded).
const SWEEP_GAS: u128 = 300_000;

#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct AppList {
    #[serde(default)]
    pub v: u32,
    #[serde(default)]
    pub apps: Vec<AppListEntry>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct AppListEntry {
    pub origin: String,
    #[serde(default)]
    pub app_id: String,
    #[serde(default)]
    pub name: String,
    /// Unix seconds the wallet first issued a token for it.
    #[serde(default)]
    pub added: i64,
}

impl AppList {
    /// Adds or refreshes an entry. Returns true if anything changed.
    pub fn remember(&mut self, e: AppListEntry) -> bool {
        self.v = 1;
        match self.apps.iter_mut().find(|a| a.origin == e.origin) {
            Some(a) if a.app_id == e.app_id && a.name == e.name => false,
            Some(a) => {
                a.app_id = e.app_id;
                a.name = e.name;
                true
            }
            None => {
                self.apps.push(e);
                true
            }
        }
    }
}

fn hex_to_b64u(h: &str) -> Option<String> {
    let s = h.trim_start_matches("0x");
    if !s.len().is_multiple_of(2) {
        return None;
    }
    let bytes: Option<Vec<u8>> = (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok()).collect();
    Some(codec::b64u(&bytes?))
}

fn b64u_to_hex(b: &str) -> Option<String> {
    let bytes = codec::unb64u(b)?;
    Some(format!("0x{}", bytes.iter().map(|x| format!("{x:02x}")).collect::<String>()))
}

fn now_secs() -> i64 {
    (js_sys::Date::now() / 1000.0) as i64
}

fn hue_of(origin: &str) -> u16 {
    (origin.bytes().fold(7u32, |h, b| h.wrapping_mul(31).wrapping_add(b as u32)) % 360) as u16
}

impl RealBackend {
    async fn read_app_list(&self) -> Res<(AppList, i64)> {
        let v = self.server.get("/v1/me/app-list").await.map_err(server_err)?;
        let version = v.get("version").and_then(Value::as_i64).unwrap_or(0);
        let Some(blob) = v.get("blob").and_then(Value::as_str) else { return Ok((AppList::default(), version)) };
        let hex = b64u_to_hex(blob).ok_or(WalletError::Network)?;
        let r = self.w_unlocked(|| obj(&[("type", "appListOpen".into()), ("blob", JsValue::from_str(&hex))])).await?;
        let data = get_str(&r, "data").unwrap_or_default();
        Ok((serde_json::from_str(&data).unwrap_or_default(), version))
    }

    async fn write_app_list(&self, list: &AppList, version: i64) -> Result<(), ServerError> {
        let data = serde_json::to_string(list).unwrap_or_default();
        let r = self
            .w_unlocked(|| obj(&[("type", "appListSeal".into()), ("data", JsValue::from_str(&data))]))
            .await
            .map_err(|_| ServerError::Network)?;
        let blob = get_str(&r, "blob").and_then(|h| hex_to_b64u(&h)).ok_or(ServerError::Network)?;
        self.server.put("/v1/me/app-list", json!({ "blob": blob, "version": version })).await.map(|_| ())
    }

    /// Adds `app` to the encrypted app list (called whenever an app token is issued).
    pub(super) async fn remember_app(&self, app: &AppInfo) -> Res<()> {
        if app.id.is_empty() {
            return Ok(());
        }
        let entry = AppListEntry { origin: app.origin.clone(), app_id: app.id.clone(), name: app.name.clone(), added: now_secs() };
        // Two app frames may add at once: re-read and merge on a version conflict.
        for _ in 0..3 {
            let (mut list, version) = self.read_app_list().await?;
            if !list.remember(entry.clone()) {
                return Ok(());
            }
            match self.write_app_list(&list, version).await {
                Ok(()) => return Ok(()),
                Err(ServerError::Status { status: 409, .. }) => continue,
                Err(e) => return Err(server_err(e)),
            }
        }
        Err(WalletError::Network)
    }

    pub(super) async fn home_apps_impl(&self) -> Res<Vec<HomeApp>> {
        let (list, _) = self.read_app_list().await?;
        let mut out = Vec::with_capacity(list.apps.len());
        for e in list.apps {
            let origin = e.origin.clone();
            let r = self.w_unlocked(|| obj(&[("type", "appAccount".into()), ("origin", JsValue::from_str(&origin))])).await?;
            let address = get_str(&r, "address").unwrap_or_default();
            let name = if e.name.is_empty() { origin.trim_start_matches("https://").to_string() } else { e.name.clone() };
            out.push(HomeApp {
                app: AppInfo { id: e.app_id.clone(), name, origin: e.origin.clone(), hue: hue_of(&e.origin) },
                address,
                balance: None,
            });
        }
        // Balances over the routed provider (Anon-RPC): one read per app.
        for a in out.iter_mut() {
            a.balance = self.chain.balance(&a.address).await.ok().map(|w| Amount::new(Token::Eth, w));
        }
        Ok(out)
    }

    pub(super) async fn suggestions_impl(&self) -> Vec<Suggestion> {
        #[derive(Deserialize)]
        struct Cfg {
            #[serde(default, rename = "demoApps")]
            demo_apps: Vec<Suggestion>,
        }
        let Some(win) = web_sys::window() else { return vec![] };
        let Ok(resp) = wasm_bindgen_futures::JsFuture::from(win.fetch_with_str(chain::CONFIG_URL)).await else { return vec![] };
        let resp: web_sys::Response = match wasm_bindgen::JsCast::dyn_into(resp) {
            Ok(r) => r,
            Err(_) => return vec![],
        };
        if !resp.ok() {
            return vec![];
        }
        let Ok(t) = resp.text() else { return vec![] };
        let text = wasm_bindgen_futures::JsFuture::from(t).await.ok().and_then(|v| v.as_string()).unwrap_or_default();
        serde_json::from_str::<Cfg>(&text)
            .map(|c| c.demo_apps.into_iter().filter(|s| s.url.starts_with("https://") || s.url.starts_with("http://")).collect())
            .unwrap_or_default()
    }

    pub(super) async fn name_status_impl(&self) -> Res<NameStatus> {
        let v = self.server.get("/v1/names/me").await.map_err(server_err)?;
        Ok(NameStatus {
            enabled: v.get("enabled").and_then(Value::as_bool).unwrap_or(false),
            parent: s(&v, "parent").unwrap_or_else(|| {
                s(&v, "name").and_then(|n| n.split_once('.').map(|(_, p)| p.to_string())).unwrap_or_else(|| "lowki.eth".into())
            }),
            name: s(&v, "name"),
            ready: s(&v, "status").as_deref() == Some("ready"),
            register_tx: s(&v, "register_tx"),
            setup_tx: s(&v, "setup_tx"),
        })
    }

    pub(super) async fn claim_impl(&self, label: &str, progress: Rc<dyn Fn(ClaimStage)>) -> Res<NameStatus> {
        if let Some(p) = label_problem(label) {
            return Err(WalletError::Other(p.into()));
        }
        progress(ClaimStage::Signing);
        let id = self.w_unlocked(|| obj(&[("type", "ensIdentity".into())])).await?;
        let owner = get_str(&id, "address").unwrap_or_default();
        let p = self
            .server
            .post("/v1/names/prepare", json!({ "label": label, "owner": owner }))
            .await
            .map_err(claim_err)?;
        let st = s(&p, "status");
        let qs = |k: &str| p.get(k).map(|v| v.as_str().map(str::to_string).unwrap_or_else(|| v.to_string())).unwrap_or_default();
        let num = |k: &str| p.get(k).and_then(Value::as_f64).unwrap_or(0.0);
        if st.as_deref() != Some("registered") {
            let reg = self
                .w_unlocked(|| {
                    obj(&[
                        ("type", "ensSignRegistration".into()),
                        ("chainId", JsValue::from_str(&qs("chain_id"))),
                        ("registrar", JsValue::from_str(&qs("registrar"))),
                        ("label", JsValue::from_str(label)),
                        ("duration", JsValue::from_f64(num("duration"))),
                        ("nonce", JsValue::from_str(&qs("registration_nonce"))),
                        ("deadline", JsValue::from_f64(num("deadline"))),
                    ])
                })
                .await?;
            progress(ClaimStage::Registering);
            self.server
                .post(
                    "/v1/names/register",
                    json!({ "label": label, "owner": owner, "nonce": p["registration_nonce"], "deadline": p["deadline"],
                            "signature": get_str(&reg, "signature") }),
                )
                .await
                .map_err(claim_err)?;
        }
        let set = self
            .w_unlocked(|| {
                obj(&[
                    ("type", "ensSignOwnerSetup".into()),
                    ("chainId", JsValue::from_str(&qs("chain_id"))),
                    ("name", JsValue::from_str(&qs("name"))),
                    ("ownerAccount", JsValue::from_str(&qs("owner_account"))),
                    ("authNonce", JsValue::from_str(&qs("auth_nonce"))),
                    ("execNonce", JsValue::from_str(&qs("exec_nonce"))),
                    ("deadline", JsValue::from_f64(num("deadline"))),
                    ("factory", JsValue::from_str(&qs("factory"))),
                    ("permissionedResolverImpl", JsValue::from_str(&qs("permissioned_resolver_impl"))),
                    ("stealthResolver", JsValue::from_str(&qs("stealth_resolver"))),
                    ("registry", JsValue::from_str(&qs("registry"))),
                    ("gatewayUrl", JsValue::from_str(&qs("gateway_url"))),
                    ("recordsResolver", JsValue::from_str(&qs("records_resolver"))),
                ])
            })
            .await?;
        progress(ClaimStage::SettingUp);
        let auth: Value = serde_json::from_str(&super::js::stringify(&get(&set, "authorization"))).unwrap_or(Value::Null);
        self.server
            .post(
                "/v1/names/setup",
                json!({ "record": get_str(&set, "record"), "exec_nonce": p["exec_nonce"], "deadline": p["deadline"],
                        "execute_signature": get_str(&set, "executeSignature"), "authorization": auth }),
            )
            .await
            .map_err(claim_err)?;
        self.name_status_impl().await
    }

    pub(super) async fn payments_impl(&self, name: &str) -> Res<Vec<StealthPayment>> {
        let ts = now_secs();
        let n = name.to_string();
        let a = self
            .w_unlocked(|| obj(&[("type", "ensRecordsAuth".into()), ("name", JsValue::from_str(&n)), ("timestamp", JsValue::from_f64(ts as f64))]))
            .await?;
        let sig = get_str(&a, "signature").unwrap_or_default();
        let path = format!("/v1/ens/records?name={}", js_sys::encode_uri_component(name));
        let v = self
            .server
            .get_with_headers(&path, &[("x-lowki-auth-timestamp", &ts.to_string()), ("x-lowki-auth-signature", &sig)])
            .await
            .map_err(server_err)?;
        let records = v.get("records").and_then(Value::as_array).cloned().unwrap_or_default();
        let created: Vec<(String, i64)> = records
            .iter()
            .map(|r| (s(r, "stealth_address").unwrap_or_default().to_lowercase(), r.get("created_at").and_then(Value::as_i64).unwrap_or(0)))
            .collect();
        let list: Vec<Value> = records
            .iter()
            .map(|r| json!({ "stealthAddress": r["stealth_address"], "ephemeralPubkey": r["ephemeral_pubkey"], "viewTag": r["view_tag"] }))
            .collect();
        let list_js = super::js::parse_json(&Value::Array(list).to_string());
        let scan = self.w_unlocked(|| obj(&[("type", "stealthScan".into()), ("records", list_js.clone())])).await?;
        let found: Value = serde_json::from_str(&super::js::stringify(&get(&scan, "found"))).unwrap_or(Value::Null);
        let pool_ok = self.private_balance_impl().await.is_some();
        let fees = self.chain.fees().await.ok();
        let mut out = vec![];
        for f in found.as_array().cloned().unwrap_or_default() {
            let address = s(&f, "stealthAddress").unwrap_or_default();
            let Ok(bal) = self.chain.balance(&address).await else { continue };
            if bal == 0 {
                continue; // handed out, never paid
            }
            let gas_cost = fees.map(|f| f.max_fee_per_gas * SWEEP_GAS).unwrap_or(u128::MAX / 4);
            let created_at = created.iter().find(|(a, _)| *a == address.to_lowercase()).map(|x| x.1).unwrap_or(0);
            out.push(StealthPayment {
                ephemeral: s(&f, "ephemeralPubkey").unwrap_or_default(),
                balance: Amount::new(Token::Eth, bal),
                created_at,
                can_sweep: pool_ok && bal.saturating_sub(gas_cost) >= POOL_MIN_DEPOSIT,
                address,
            });
        }
        out.sort_by_key(|p| std::cmp::Reverse(p.created_at));
        Ok(out)
    }

    pub(super) async fn private_balance_impl(&self) -> Option<Amount> {
        let w = self.worker_rc().ok()?;
        // The pool session rides on the routed connection: decide it first (boots Anon-RPC).
        let _ = self.chain.fees().await;
        match self.private.balance(&w).await {
            Ok(b) => Some(Amount::new(Token::Eth, b.approved.parse().unwrap_or(0))),
            Err(e) => {
                log(&format!("private balance: {e}"));
                None
            }
        }
    }

    /// Pool deposit from a stealth address: Kohaku `prepareShield` (== worker `ppShield`), signed
    /// by the stealth key inside the worker, broadcast over the routed provider, then the ASP.
    pub(super) async fn sweep_impl(&self, p: &StealthPayment, progress: ProgressFn) -> Res<SendReceipt> {
        let w = self.worker_rc()?;
        progress(ProofProgress { stage: ProofStage::SyncingPool, fraction: 0.05 });
        let bal = self.chain.balance(&p.address).await.map_err(chain_err)?;
        let fees = self.chain.fees().await.map_err(chain_err)?;
        let value = bal.saturating_sub(fees.max_fee_per_gas * SWEEP_GAS);
        if value < POOL_MIN_DEPOSIT {
            return Err(WalletError::Other("This payment is below the pool's minimum deposit (0.001 ETH).".into()));
        }
        let tx = self.private.prepare_shield(&w, value).await.map_err(private_err)?;
        progress(ProofProgress { stage: ProofStage::SyncingPool, fraction: 0.5 });
        let nonce = self.chain.nonce(&p.address).await.map_err(chain_err)?;
        let gas = self.chain.estimate_gas(&p.address, &tx.to, value, &tx.data).await.map_err(chain_err)?;
        let gas = (gas + gas / 5).min(SWEEP_GAS);
        let txj = json!({
            "type": "eip1559", "chainId": self.chain.chain_id().to_string(), "nonce": nonce.to_string(),
            "maxPriorityFeePerGas": fees.max_priority_fee_per_gas.to_string(), "maxFeePerGas": fees.max_fee_per_gas.to_string(),
            "gas": gas.to_string(), "to": tx.to, "value": value.to_string(), "data": tx.data,
        })
        .to_string();
        let (eph, addr) = (p.ephemeral.clone(), p.address.clone());
        let r = self
            .w_unlocked(|| {
                obj(&[
                    ("type", "stealthSignTransaction".into()),
                    ("ephemeralPubkey", JsValue::from_str(&eph)),
                    ("stealthAddress", JsValue::from_str(&addr)),
                    ("tx", super::js::parse_json(&txj)),
                ])
            })
            .await?;
        let hash = self.chain.send_raw(&get_str(&r, "rawTransaction").unwrap_or_default()).await.map_err(chain_err)?;
        self.wait_receipt(&hash).await?;
        progress(ProofProgress { stage: ProofStage::SyncingPool, fraction: 1.0 });
        let p2 = progress.clone();
        let on_poll: Rc<dyn Fn(f32)> = Rc::new(move |f| p2(ProofProgress { stage: ProofStage::BuildingProof, fraction: f }));
        self.private.wait_approved(&w, &tx.precommitment, on_poll).await.map_err(private_err)?;
        progress(ProofProgress { stage: ProofStage::Submitting, fraction: 1.0 });
        Ok(SendReceipt { hash, private: true })
    }
}

fn claim_err(e: ServerError) -> WalletError {
    match e {
        ServerError::Status { status: 409, message, .. } => WalletError::Other(capitalize(&message)),
        ServerError::Status { status: 400, message, .. } => WalletError::Other(capitalize(&message)),
        ServerError::Status { status: 404, .. } => WalletError::Other("Names aren't available on this server.".into()),
        ServerError::Status { status: 502, .. } => WalletError::Other("The network didn't confirm in time. Try again.".into()),
        e => server_err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn app_list_remember_dedups_by_origin() {
        let mut l = AppList::default();
        let e = |o: &str, n: &str| AppListEntry { origin: o.into(), app_id: "app_1".into(), name: n.into(), added: 1 };
        assert!(l.remember(e("https://a.example", "A")));
        assert!(!l.remember(e("https://a.example", "A")));
        assert!(l.remember(e("https://a.example", "A2")));
        assert!(l.remember(e("https://b.example", "B")));
        assert_eq!(l.apps.len(), 2);
        let j = serde_json::to_string(&l).unwrap();
        assert_eq!(serde_json::from_str::<AppList>(&j).unwrap(), l);
    }

    #[test]
    fn hex_b64_round_trip() {
        let h = "0x01ff00aa";
        assert_eq!(b64u_to_hex(&hex_to_b64u(h).unwrap()).unwrap(), h);
    }
}
