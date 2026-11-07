//! The real backend: lowki-worker (keys, in a Web Worker), lowki-server (email codes,
//! passkeys, the wrapped blob, the server share, app tokens) and a chain provider
//! (balances and broadcasting).
//!
//! Private balance, Move in/out and Send privately are real (Privacy Pools v1 through
//! `private.rs`); delegation is not wired to real services yet and goes to the mock behind the
//! same traits. Chain reads and broadcasts go through Anon-RPC by default (`chain.rs`).
//!
//! Flows (see `docs/ARCHITECTURE.md`, "Sign-in and recovery"):
//! - **Sign-up:** email code -> passkey (PRF) -> worker `createWallet` -> PUT blob -> PUT
//!   share S -> show recovery code R.
//! - **Returning:** passkey login (PRF) -> blob from `login/finish` -> worker `unlock`.
//! - **Recovery:** email code -> release S -> worker `recover(S, R)` -> new passkey ->
//!   worker `rewrap` -> PUT blob.
//! - **Per app:** worker `appAccount(origin)` + lowki-server app token.

pub mod chain;
pub mod codec;
mod home;
mod js;
pub mod passkey;
pub mod private;
mod server;
mod worker;

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

use serde_json::{Value, json};
use wasm_bindgen::JsValue;

use self::chain::{ChainProvider, RoutedRpc};
use self::js::{bytes, get_bytes, get_str, obj, parse_json, wipe};
use self::passkey::{Ceremony, PasskeyError};
use self::server::{Server, ServerError};
use self::worker::{WorkerClient, WorkerFail};
use super::mock::MockBackend;
use super::*;

#[derive(Default)]
struct State {
    email: Option<String>,
    /// The passkey used in this session (base64url credential ID).
    credential_id: Option<String>,
    /// The worker holds the seed.
    unlocked: bool,
    /// Unlocked by `recover`: the next `create_passkey` rewraps instead of creating.
    recovered: bool,
    /// R, kept only between `create_passkey`/`recover_with_code` and the recovery screen.
    recovery: Option<RecoveryCode>,
    code: Option<CodeStatus>,
    /// origin -> (address, worker pairwise ID), for "Your apps".
    accounts: BTreeMap<String, (AppInfo, String)>,
}

pub struct RealBackend {
    server: Server,
    worker: Option<Rc<WorkerClient>>,
    chain: Box<dyn ChainProvider>,
    /// Privacy Pools (Kohaku + worker prover, through the glue).
    private: private::PrivatePool,
    /// Facts about the last private operation, for the done screen.
    report: RefCell<Vec<(String, String)>>,
    /// Delegation (not real yet).
    mock: Rc<MockBackend>,
    st: RefCell<State>,
}

// ---- error mapping -----------------------------------------------------------------------

fn server_err(e: ServerError) -> WalletError {
    match e {
        ServerError::Network => WalletError::Network,
        ServerError::Status { status: 400, code, .. } if code == "invalid_code" => WalletError::InvalidCode,
        ServerError::Status { status: 429, .. } => WalletError::Other("Too many attempts. Wait a few minutes and try again.".into()),
        ServerError::Status { status, message, .. } if status >= 500 => {
            log(&format!("lowki-server {status}: {message}"));
            WalletError::Network
        }
        ServerError::Status { message, .. } => WalletError::Other(capitalize(&message)),
    }
}

fn passkey_err(e: PasskeyError) -> WalletError {
    match e {
        PasskeyError::Cancelled => WalletError::PasskeyCancelled,
        PasskeyError::AlreadyRegistered => {
            WalletError::Other("This device already has a Lowki passkey for this account. Use it to sign in.".into())
        }
        PasskeyError::Unavailable(m) => {
            log(&format!("webauthn unavailable: {m}"));
            WalletError::PrfUnsupported
        }
    }
}

fn worker_err(e: WorkerFail) -> WalletError {
    log(&format!("lowki-worker {}: {}", e.code, e.message));
    match e.code.as_str() {
        "unwrapFailed" => WalletError::Other("This passkey can't open the wallet stored for it.".into()),
        "invalidShares" => WalletError::Other("That recovery code doesn't match this account.".into()),
        "locked" => WalletError::Other("Your wallet locked. Sign in again.".into()),
        _ => WalletError::Other("Something went wrong inside the wallet. Try again.".into()),
    }
}

fn chain_err(e: chain::ChainError) -> WalletError {
    log(&format!("rpc: {}", e.0));
    if e.0.contains("insufficient funds") {
        WalletError::InsufficientFunds
    } else {
        WalletError::Network
    }
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => format!("{}{}.", f.to_uppercase(), c.as_str().trim_end_matches('.')),
        None => "Something went wrong.".into(),
    }
}

fn log(s: &str) {
    web_sys::console::warn_1(&JsValue::from_str(&format!("[lowki] {s}")));
}

fn s(v: &Value, k: &str) -> Option<String> {
    v.get(k).and_then(Value::as_str).map(str::to_string)
}

impl RealBackend {
    pub fn new(mock: Rc<MockBackend>) -> Self {
        let worker = match WorkerClient::spawn() {
            Ok(w) => Some(Rc::new(w)),
            Err(e) => {
                log(&format!("could not start the key worker: {e}"));
                None
            }
        };
        Self {
            server: Server::default(),
            worker,
            chain: Box::new(RoutedRpc::new()),
            private: private::PrivatePool::default(),
            report: RefCell::default(),
            mock,
            st: RefCell::default(),
        }
    }

    async fn w(&self, req: JsValue) -> Res<JsValue> {
        let w = self.worker.as_ref().ok_or_else(|| WalletError::Other("The key worker didn't start.".into()))?;
        w.call(req).await.map_err(worker_err)
    }

    /// Like [`Self::w`], but a locked worker (auto-lock) is unlocked with the passkey and
    /// the request retried once.
    async fn w_unlocked(&self, make: impl Fn() -> JsValue) -> Res<JsValue> {
        let w = self.worker.as_ref().ok_or_else(|| WalletError::Other("The key worker didn't start.".into()))?;
        match w.call(make()).await {
            Err(e) if e.code == "locked" => {
                self.st.borrow_mut().unlocked = false;
                self.passkey_unlock().await?;
                w.call(make()).await.map_err(worker_err)
            }
            r => r.map_err(worker_err),
        }
    }

    async fn lock_worker(&self) {
        if let Some(w) = &self.worker {
            let _ = w.call(obj(&[("type", "lock".into())])).await;
        }
        self.st.borrow_mut().unlocked = false;
        // The next unlock may be another seed: its notes are not this session's.
        self.private.reset();
    }

    fn worker_rc(&self) -> Res<Rc<WorkerClient>> {
        self.worker.clone().ok_or_else(|| WalletError::Other("The key worker didn't start.".into()))
    }

    /// Polls the receipt of `hash` (1 s steps, up to 3 min) over the routed provider.
    async fn wait_receipt(&self, hash: &str) -> Res<Value> {
        for _ in 0..180 {
            if let Some(r) = self.chain.receipt(hash).await.map_err(chain_err)? {
                if r.get("status").and_then(Value::as_str) != Some("0x1") {
                    return Err(WalletError::Other(format!("The transaction {} failed on chain.", &hash[..10.min(hash.len())])));
                }
                return Ok(r);
            }
            gloo_timers::future::TimeoutFuture::new(1_000).await;
        }
        Err(WalletError::Network)
    }

    /// Private withdraw (relayer) from the pool to `to`, with the stages reported.
    async fn private_withdraw(&self, to: &str, wei: u128, progress: ProgressFn) -> Res<SendReceipt> {
        let w = self.worker_rc()?;
        let u = self.private.unshield(&w, wei, to, progress).await.map_err(private_err)?;
        let mut rep = vec![
            ("Syncing pool".to_string(), secs(u.timings.sync_ms)),
            ("Building proof".to_string(), secs(u.timings.prove_ms)),
            ("Submitting".to_string(), secs(u.timings.submit_ms)),
        ];
        if let Some(p) = &u.proof {
            let backend = if p.backend == "webgpu" { "WebGPU" } else { "CPU" };
            rep.insert(0, ("Proof".to_string(), format!("{} ms on {backend}, in this browser", p.ms.round())));
        }
        rep.push(("Recipient gets".to_string(), Amount::new(Token::Eth, u.net.parse().unwrap_or(0)).fmt()));
        rep.push((format!("Relayer fee ({} bps)", u.relay_fee_bps), Amount::new(Token::Eth, u.fee.parse().unwrap_or(0)).fmt()));
        rep.push(("Nullifier".to_string(), short(&u.nullifier_hash)));
        rep.push(("Connection".to_string(), connection_line()));
        web_sys::console::info_1(&JsValue::from_str(&format!(
            "[lowki] private send: {wei} wei -> {to} tx {} nullifierHash={} proof={:?}",
            u.tx_hash,
            u.nullifier_hash,
            u.proof.as_ref().map(|p| (&p.backend, p.ms))
        )));
        *self.report.borrow_mut() = rep;
        Ok(SendReceipt { hash: u.tx_hash, private: true })
    }

    /// Passkey assertion through lowki-server. PRF is required: a real `get()` that returns
    /// no PRF output means this authenticator can't protect a Lowki wallet.
    async fn assertion(&self, allow: Option<&str>) -> Res<(Value, Ceremony)> {
        let start = self.server.post("/v1/passkeys/login/start", json!({})).await.map_err(server_err)?;
        let flow = s(&start, "flow_id").unwrap_or_default();
        let c = passkey::get_assertion(&start["options"], allow).await.map_err(passkey_err)?;
        let fin = self
            .server
            .post("/v1/passkeys/login/finish", json!({ "flow_id": flow, "credential": c.credential }))
            .await
            .map_err(server_err)?;
        if c.prf.is_none() {
            return Err(WalletError::PrfUnsupported);
        }
        Ok((fin, c))
    }

    /// Returning user: passkey -> blob -> worker unlock.
    async fn passkey_unlock(&self) -> Res<()> {
        let (fin, c) = self.assertion(None).await?;
        let cred = s(&fin, "credential_id").unwrap_or(c.id);
        let Some(blob) = s(&fin, "wrapped_blob").and_then(|b| codec::unb64u(&b)) else {
            return Err(WalletError::Other(
                "This passkey isn't linked to a wallet yet. Continue with email to finish setting it up.".into(),
            ));
        };
        self.lock_worker().await;
        let prf = bytes(c.prf.as_deref().unwrap_or_default());
        let r = self.w(obj(&[("type", "unlock".into()), ("prfOutput", prf.clone()), ("wrappedBlob", bytes(&blob))])).await;
        wipe(&prf);
        r?;
        let mut st = self.st.borrow_mut();
        st.credential_id = Some(cred);
        st.unlocked = true;
        Ok(())
    }

    /// Registers a passkey for the signed-in user and returns its ID and PRF output.
    async fn register_passkey(&self) -> Res<(String, Vec<u8>)> {
        let name = self.st.borrow().email.clone().unwrap_or_else(|| "Lowki wallet".into());
        let start = self.server.post("/v1/passkeys/register/start", json!({ "name": name })).await.map_err(server_err)?;
        let flow = s(&start, "flow_id").unwrap_or_default();
        let c = passkey::create(&start["options"]).await.map_err(passkey_err)?;
        let fin = self
            .server
            .post("/v1/passkeys/register/finish", json!({ "flow_id": flow, "credential": c.credential }))
            .await
            .map_err(server_err)?;
        let cred = s(&fin, "credential_id").unwrap_or(c.id);
        // Many authenticators only evaluate PRF on get(): ask with a real assertion. This is
        // also the PRF-support check (`prf.enabled` at create time isn't reliable).
        let prf = match c.prf {
            Some(p) => p,
            None => self.assertion(Some(&cred)).await?.1.prf.unwrap_or_default(),
        };
        self.st.borrow_mut().credential_id = Some(cred.clone());
        Ok((cred, prf))
    }

    async fn put_blob(&self, cred: &str, blob: &[u8]) -> Res<()> {
        self.server
            .put(&format!("/v1/passkeys/{cred}/blob"), json!({ "blob": codec::b64u(blob) }))
            .await
            .map_err(server_err)?;
        Ok(())
    }

    async fn methods(&self) -> Res<Vec<Value>> {
        let v = self.server.get("/v1/me/methods").await.map_err(server_err)?;
        Ok(v.get("methods").and_then(Value::as_array).cloned().unwrap_or_default())
    }

    async fn account(&self, app: &AppInfo) -> Res<(String, String)> {
        let origin = app.origin.clone();
        let r = self.w_unlocked(|| obj(&[("type", "appAccount".into()), ("origin", JsValue::from_str(&origin))])).await?;
        let addr = get_str(&r, "address").unwrap_or_default().to_lowercase();
        let pairwise = get_str(&r, "pairwiseUserId").unwrap_or_default();
        self.st.borrow_mut().accounts.insert(app.origin.clone(), (app.clone(), addr.clone()));
        Ok((addr, pairwise))
    }

    async fn address(&self, app: &AppInfo) -> Res<String> {
        if let Some((_, a)) = self.st.borrow().accounts.get(&app.origin) {
            return Ok(a.clone());
        }
        Ok(self.account(app).await?.0)
    }

    async fn app_token(&self, app: &AppInfo) -> Res<Value> {
        let path = format!("/v1/apps/{}/token", app.id);
        let body = json!({ "origin": app.origin });
        match self.server.post(&path, body.clone()).await {
            Err(ServerError::Status { status: 401, .. }) => {
                // Session cookie expired: rotate it with the refresh cookie and retry once.
                self.server.post("/v1/session/refresh", json!({})).await.map_err(server_err)?;
                self.server.post(&path, body).await.map_err(server_err)
            }
            r => r.map_err(server_err),
        }
    }

    /// Signs and broadcasts an EIP-1559 transaction from the app account.
    async fn transact(&self, app: &AppInfo, to: &str, value: u128, data: &str) -> Res<String> {
        let from = self.address(app).await?;
        let nonce = self.chain.nonce(&from).await.map_err(chain_err)?;
        let gas = self.chain.estimate_gas(&from, to, value, data).await.map_err(chain_err)?;
        let fees = self.chain.fees().await.map_err(chain_err)?;
        let tx = json!({
            "type": "eip1559",
            "chainId": self.chain.chain_id().to_string(),
            "nonce": nonce.to_string(),
            "maxPriorityFeePerGas": fees.max_priority_fee_per_gas.to_string(),
            "maxFeePerGas": fees.max_fee_per_gas.to_string(),
            "gas": (gas + gas / 5).to_string(),
            "to": to,
            "value": value.to_string(),
            "data": if data.is_empty() { "0x" } else { data },
        })
        .to_string();
        let origin = app.origin.clone();
        let r = self
            .w_unlocked(|| obj(&[("type", "signTransaction".into()), ("origin", JsValue::from_str(&origin)), ("tx", parse_json(&tx))]))
            .await?;
        let raw = get_str(&r, "rawTransaction").unwrap_or_default();
        self.chain.send_raw(&raw).await.map_err(chain_err)
    }
}

impl AuthBackend for RealBackend {
    fn sign_in_google(&self, _app: &AppInfo) -> BoxFut<'_, Res<AuthOutcome>> {
        Box::pin(async { Err(WalletError::Other("Google sign-in isn't set up here yet. Continue with email.".into())) })
    }

    fn send_email_code(&self, email: &str) -> BoxFut<'_, Res<()>> {
        let email = email.trim().to_lowercase();
        Box::pin(async move {
            self.server.post("/v1/email/start", json!({ "email": email })).await.map_err(server_err)?;
            self.st.borrow_mut().email = Some(email);
            Ok(())
        })
    }

    fn verify_email_code(&self, email: &str, code: &str) -> BoxFut<'_, Res<AuthOutcome>> {
        let (email, code) = (email.trim().to_lowercase(), code.to_string());
        Box::pin(async move {
            let v = self.server.post("/v1/email/verify", json!({ "email": email, "code": code })).await.map_err(server_err)?;
            self.st.borrow_mut().email = Some(email);
            if v.get("new_user").and_then(Value::as_bool) == Some(true) {
                return Ok(AuthOutcome::NewUser);
            }
            // Known email: it has a wallet only if a passkey and the server share exist
            // (an earlier sign-up may have stopped halfway).
            let sess = self.server.get("/v1/session").await.map_err(server_err)?;
            let has_passkey = sess["methods"].as_array().is_some_and(|m| m.iter().any(|m| m["kind"] == "passkey"));
            let has_share = sess.get("share").is_some_and(|x| !x.is_null());
            Ok(if has_passkey && has_share { AuthOutcome::NeedsPasskey } else { AuthOutcome::NewUser })
        })
    }

    fn sign_in_passkey(&self, app: &AppInfo) -> BoxFut<'_, Res<Session>> {
        let app = app.clone();
        Box::pin(async move {
            self.passkey_unlock().await?;
            self.session(&app).await
        })
    }

    fn create_passkey(&self) -> BoxFut<'_, Res<RecoveryCode>> {
        Box::pin(async move {
            let recovered = self.st.borrow().recovered;
            let (cred, prf) = self.register_passkey().await?;
            let prf_js = bytes(&prf);
            if recovered {
                // Same seed (from S + R), wrapped for the new passkey.
                let r = self.w(obj(&[("type", "rewrap".into()), ("prfOutput", prf_js.clone())])).await;
                wipe(&prf_js);
                let r = r?;
                let blob = get_bytes(&r, "wrappedBlob").unwrap_or_default();
                self.put_blob(&cred, &blob).await?;
                let mut st = self.st.borrow_mut();
                st.recovered = false;
                st.unlocked = true;
                st.code = Some(CodeStatus::Saved);
                return Ok(st.recovery.take().unwrap_or(RecoveryCode(String::new())));
            }
            self.lock_worker().await;
            let r = self.w(obj(&[("type", "createWallet".into()), ("prfOutput", prf_js.clone())])).await;
            wipe(&prf_js);
            let r = r?;
            self.st.borrow_mut().unlocked = true;
            let blob = get_bytes(&r, "wrappedBlob").unwrap_or_default();
            let server_share = get_bytes(&r, "serverShare").unwrap_or_default();
            let recovery_share = get_bytes(&r, "recoveryShare").unwrap_or_default();
            for k in ["serverShare", "recoveryShare", "wrappedPasskeyShare"] {
                wipe(&js::get(&r, k));
            }
            self.put_blob(&cred, &blob).await?;
            // Passkey-only accounts have no email or Google sign-in to guard the server share,
            // so there is nothing to recover with yet: the passkey (synced by iCloud Keychain or
            // Google Password Manager) is the way in. Recovery starts once an email is linked.
            if self.st.borrow().email.is_none() {
                let mut st = self.st.borrow_mut();
                st.code = None;
                st.recovery = None;
                return Ok(RecoveryCode(String::new()));
            }
            self.server
                .put("/v1/me/share", json!({ "share": codec::b64u(&server_share) }))
                .await
                .map_err(server_err)?;
            let code = RecoveryCode(codec::encode_recovery(&recovery_share));
            let mut st = self.st.borrow_mut();
            st.code = Some(CodeStatus::NotSaved);
            st.recovery = Some(code.clone());
            Ok(code)
        })
    }

    fn continue_without_prf(&self) -> BoxFut<'_, Res<RecoveryCode>> {
        Box::pin(async {
            Err(WalletError::Other("Lowki needs a passkey with PRF on this server. Open the link in Chrome, Edge or Safari.".into()))
        })
    }

    fn recover_with_code(&self, code: &str) -> BoxFut<'_, Res<()>> {
        let code = code.to_string();
        Box::pin(async move {
            let Some(r) = codec::decode_recovery(&code) else {
                return Err(WalletError::Other("That recovery code isn't right. Check each group and try again.".into()));
            };
            let v = self.server.post("/v1/me/share/release", json!({})).await.map_err(|e| match e {
                ServerError::Status { status: 401 | 403, .. } => {
                    WalletError::Other("Confirm your email first, then enter the recovery code.".into())
                }
                e => server_err(e),
            })?;
            if s(&v, "status").as_deref() != Some("released") {
                return Err(WalletError::Other("Recovery is waiting for its safety delay. Try again later.".into()));
            }
            let server_share = s(&v, "share").and_then(|x| codec::unb64u(&x)).ok_or(WalletError::Network)?;
            self.lock_worker().await;
            let (sj, rj) = (bytes(&server_share), bytes(&r));
            let res = self.w(obj(&[("type", "recover".into()), ("serverShare", sj.clone()), ("recoveryShare", rj.clone())])).await;
            wipe(&sj);
            wipe(&rj);
            res?;
            let mut st = self.st.borrow_mut();
            st.unlocked = true;
            st.recovered = true;
            st.recovery = Some(RecoveryCode(codec::encode_recovery(&r)));
            Ok(())
        })
    }

    fn backup_recovery(&self, _target: BackupTarget) -> BoxFut<'_, Res<()>> {
        Box::pin(async { Err(WalletError::Other("Cloud backup isn't available yet. Copy the code and keep it somewhere safe.".into())) })
    }

    fn acknowledge_recovery(&self, saved: bool) -> BoxFut<'_, Res<()>> {
        Box::pin(async move {
            let mut st = self.st.borrow_mut();
            st.recovery = None;
            st.code = Some(if saved { CodeStatus::Saved } else { CodeStatus::NotSaved });
            Ok(())
        })
    }

    fn session(&self, app: &AppInfo) -> BoxFut<'_, Res<Session>> {
        let app = app.clone();
        Box::pin(async move {
            let (address, _pairwise) = self.account(&app).await?;
            if app.id.is_empty() {
                // The wallet home (top level, no app): nothing is issued and nothing is shared.
                let label = self.st.borrow().email.clone().unwrap_or_else(|| "Your passkey".into());
                return Ok(Session { address, user_id: String::new(), access_token: String::new(), account_label: label });
            }
            let t = self.app_token(&app).await?;
            // Remember the app in the user's encrypted app list (for the wallet home).
            if let Err(e) = self.remember_app(&app).await {
                log(&format!("app list: {}", e.message()));
            }
            let label = self.st.borrow().email.clone().unwrap_or_else(|| "Your passkey".into());
            Ok(Session {
                address,
                // The token's `sub`: what the app verifies against Lowki's JWKS.
                user_id: s(&t, "user_id").unwrap_or_default(),
                access_token: s(&t, "access_token").unwrap_or_default(),
                account_label: label,
            })
        })
    }

    fn sign_in_methods(&self) -> BoxFut<'_, Res<Vec<SignInMethod>>> {
        Box::pin(async move {
            let ms = self.methods().await?;
            let find = |kind: &str| ms.iter().find(|m| m["kind"] == kind);
            Ok(vec![
                SignInMethod { kind: MethodKind::Passkey, detail: find("passkey").map(|_| "Passkey".into()) },
                SignInMethod {
                    kind: MethodKind::Email,
                    detail: find("email").map(|m| s(m, "email").or_else(|| self.st.borrow().email.clone()).unwrap_or_else(|| "Verified email".into())),
                },
            ])
        })
    }

    fn link_method(&self, _kind: MethodKind) -> BoxFut<'_, Res<Vec<SignInMethod>>> {
        Box::pin(async { Err(WalletError::Other("Adding sign-in methods isn't available yet.".into())) })
    }

    fn unlink_method(&self, _kind: MethodKind) -> BoxFut<'_, Res<Vec<SignInMethod>>> {
        Box::pin(async { Err(WalletError::Other("Removing sign-in methods isn't available yet.".into())) })
    }

    fn recovery_status(&self) -> BoxFut<'_, Res<RecoveryStatus>> {
        Box::pin(async move {
            let share = self.server.get("/v1/me/share").await.map_err(server_err)?;
            let ms = self.methods().await.unwrap_or_default();
            Ok(RecoveryStatus {
                passkey_share: ms.iter().any(|m| m["kind"] == "passkey"),
                server_share: share.get("exists").and_then(Value::as_bool).unwrap_or(false),
                code: self.st.borrow().code.unwrap_or(CodeStatus::Saved),
            })
        })
    }

    fn sign_out(&self) -> BoxFut<'_, Res<()>> {
        Box::pin(async move {
            let _ = self.server.post("/v1/session/logout", json!({})).await;
            self.server.set_csrf(None);
            self.lock_worker().await;
            *self.st.borrow_mut() = State::default();
            self.report.borrow_mut().clear();
            Ok(())
        })
    }
}

impl WalletBackend for RealBackend {
    fn balances(&self, app: &AppInfo) -> BoxFut<'_, Res<Balances>> {
        let app = app.clone();
        Box::pin(async move {
            let addr = self.address(&app).await?;
            let eth = self.chain.balance(&addr).await.map_err(chain_err)?;
            let usdc = self
                .chain
                .call(chain::USDC, &chain::balance_of_data(&addr))
                .await
                .ok()
                .and_then(|h| codec::parse_qty(&h))
                .unwrap_or(0);
            // The private balance is the user's approved ETH in the pool (Kohaku sync). A pool
            // that can't be read shows as empty rather than failing Home.
            let private = match self.worker_rc() {
                Ok(w) => match self.private.balance(&w).await {
                    Ok(b) => vec![Amount::new(Token::Eth, b.approved.parse().unwrap_or(0))],
                    Err(e) => {
                        log(&format!("private balance: {e}"));
                        vec![]
                    }
                },
                Err(_) => vec![],
            };
            Ok(Balances {
                public: vec![Amount::new(Token::Usdc, usdc), Amount::new(Token::Eth, eth)],
                private,
                usd_cents: (usdc / 10_000) as u64,
                network: self.chain.network().to_string(),
            })
        })
    }

    fn activity(&self, _app: &AppInfo) -> BoxFut<'_, Res<Vec<Activity>>> {
        // No indexer yet: activity needs one that doesn't learn the user's addresses.
        Box::pin(async { Ok(vec![]) })
    }

    fn send(&self, app: &AppInfo, req: SendRequest, progress: ProgressFn) -> BoxFut<'_, Res<SendReceipt>> {
        let app = app.clone();
        Box::pin(async move {
            if !codec::is_address(&req.to) {
                return Err(WalletError::InvalidAddress);
            }
            if req.private {
                if req.amount.token != Token::Eth {
                    return Err(WalletError::Other("The private balance holds ETH. Switch the token to ETH.".into()));
                }
                return self.private_withdraw(&req.to, req.amount.units, progress).await;
            }
            let hash = match req.amount.token {
                Token::Usdc => self.transact(&app, chain::USDC, 0, &chain::transfer_data(&req.to, req.amount.units)).await?,
                _ => self.transact(&app, &req.to, req.amount.units, "").await?,
            };
            Ok(SendReceipt { hash, private: false })
        })
    }

    fn move_private(&self, app: &AppInfo, req: VaultMove, progress: ProgressFn) -> BoxFut<'_, Res<SendReceipt>> {
        let app = app.clone();
        Box::pin(async move {
            if req.amount.token != Token::Eth {
                return Err(WalletError::Other("The private balance holds ETH.".into()));
            }
            let wei = req.amount.units;
            if req.direction == VaultDirection::Out {
                // Withdraw to this app account through the relayer: a fresh proof, so the
                // account can't be linked to the deposit it came from.
                let to = self.address(&app).await?;
                return self.private_withdraw(&to, wei, progress).await;
            }
            // Move in: Kohaku deposit (== worker ppShield) signed by the worker from the app
            // account and broadcast over the routed provider; then the ASP; then a sync.
            let w = self.worker_rc()?;
            let t0 = js_sys::Date::now();
            progress(ProofProgress { stage: ProofStage::SyncingPool, fraction: 0.05 });
            let tx = self.private.prepare_shield(&w, wei).await.map_err(private_err)?;
            progress(ProofProgress { stage: ProofStage::SyncingPool, fraction: 0.4 });
            let value: u128 = tx.value.parse().unwrap_or(wei);
            let hash = self.transact(&app, &tx.to, value, &tx.data).await?;
            self.wait_receipt(&hash).await?;
            let t1 = js_sys::Date::now();
            progress(ProofProgress { stage: ProofStage::SyncingPool, fraction: 1.0 });
            let p2 = progress.clone();
            let on_poll: Rc<dyn Fn(f32)> = Rc::new(move |f| p2(ProofProgress { stage: ProofStage::BuildingProof, fraction: f }));
            on_poll(0.0);
            let ok = self.private.wait_approved(&w, &tx.precommitment, on_poll).await.map_err(private_err)?;
            let t2 = js_sys::Date::now();
            progress(ProofProgress { stage: ProofStage::Submitting, fraction: 0.2 });
            let bal = self.private.balance(&w).await.map_err(private_err)?;
            progress(ProofProgress { stage: ProofStage::Submitting, fraction: 1.0 });
            *self.report.borrow_mut() = vec![
                ("Deposit".to_string(), format!("#{} · {}", tx.deposit_index, secs(t1 - t0))),
                ("ASP approval".to_string(), secs(ok.ms)),
                ("Syncing pool".to_string(), secs(js_sys::Date::now() - t2)),
                ("In the pool".to_string(), Amount::new(Token::Eth, ok.balance.parse().unwrap_or(0)).fmt()),
                ("Private balance".to_string(), Amount::new(Token::Eth, bal.approved.parse().unwrap_or(0)).fmt()),
                ("Connection".to_string(), connection_line()),
            ];
            let pending: u128 = bal.pending.parse().unwrap_or(0);
            if pending > 0 {
                self.report.borrow_mut().insert(5, ("Awaiting the ASP".to_string(), Amount::new(Token::Eth, pending).fmt()));
            }
            log(&format!("move in: deposit {hash} label {} approved in {} ms", ok.label, ok.ms));
            Ok(SendReceipt { hash, private: true })
        })
    }

    fn sign_message(&self, app: &AppInfo, message: &str) -> BoxFut<'_, Res<String>> {
        let (origin, message) = (app.origin.clone(), message.to_string());
        Box::pin(async move {
            let r = self
                .w_unlocked(|| {
                    obj(&[("type", "signMessage".into()), ("origin", JsValue::from_str(&origin)), ("message", JsValue::from_str(&message))])
                })
                .await?;
            get_str(&r, "signature").ok_or(WalletError::Other("No signature returned.".into()))
        })
    }

    fn sign_typed_data(&self, app: &AppInfo, typed: &serde_json::Value) -> BoxFut<'_, Res<String>> {
        let (origin, typed) = (app.origin.clone(), typed.to_string());
        Box::pin(async move {
            let r = self
                .w_unlocked(|| {
                    obj(&[("type", "signTypedData".into()), ("origin", JsValue::from_str(&origin)), ("typedData", parse_json(&typed))])
                })
                .await?;
            get_str(&r, "signature").ok_or(WalletError::Other("No signature returned.".into()))
        })
    }

    fn preview_transaction(&self, app: &AppInfo, tx: &TxRequest) -> BoxFut<'_, Res<TxPreview>> {
        let app = app.clone();
        let tx = tx.clone();
        Box::pin(async move {
            if !codec::is_address(&tx.to) {
                return Err(WalletError::InvalidAddress);
            }
            let wei: u128 = tx.value.parse().unwrap_or(0);
            let from = self.address(&app).await?;
            let fees = self.chain.fees().await.map_err(chain_err)?;
            // An estimate fails when the account can't pay; quote a typical call instead.
            let gas = self.chain.estimate_gas(&from, &tx.to, wei, &tx.data).await.unwrap_or(100_000);
            let call = (tx.data.len() >= 10).then(|| DecodedCall {
                function: format!("Function {}", &tx.data[..10]),
                args: vec![("Call data".into(), format!("{} bytes", (tx.data.len() - 2) / 2))],
            });
            Ok(TxPreview {
                to: tx.to.clone(),
                to_label: None,
                value: Amount::new(Token::Eth, wei),
                call,
                gas: GasQuote::Paid { amount: Amount::new(Token::Eth, gas * fees.max_fee_per_gas) },
                network: self.chain.network().to_string(),
            })
        })
    }

    fn send_transaction(&self, app: &AppInfo, tx: &TxRequest) -> BoxFut<'_, Res<SendReceipt>> {
        let app = app.clone();
        let tx = tx.clone();
        Box::pin(async move {
            let wei: u128 = tx.value.parse().unwrap_or(0);
            let hash = self.transact(&app, &tx.to, wei, &tx.data).await?;
            Ok(SendReceipt { hash, private: false })
        })
    }

    fn review_delegation(&self, app: &AppInfo, policy: &DelegationPolicy) -> BoxFut<'_, Res<DelegationReview>> {
        self.mock.review_delegation(app, policy)
    }

    fn approve_delegation(&self, app: &AppInfo, review: &DelegationReview) -> BoxFut<'_, Res<Delegation>> {
        self.mock.approve_delegation(app, review)
    }

    fn delegations(&self) -> BoxFut<'_, Res<Vec<Delegation>>> {
        self.mock.delegations()
    }

    fn revoke_delegation(&self, id: &str) -> BoxFut<'_, Res<()>> {
        self.mock.revoke_delegation(id)
    }

    fn app_accounts(&self) -> BoxFut<'_, Res<Vec<AppAccount>>> {
        Box::pin(async move {
            Ok(self
                .st
                .borrow()
                .accounts
                .values()
                .map(|(app, address)| AppAccount { app: app.clone(), address: address.clone(), last_used: "Now".into() })
                .collect())
        })
    }

    fn token_at(&self, address: &str) -> Option<Token> {
        address.eq_ignore_ascii_case(chain::USDC).then_some(Token::Usdc)
    }

    fn chain_connection(&self) -> BoxFut<'_, Option<ChainConnection>> {
        Box::pin(async move {
            // Decides the route (boots the anon network) if nothing has yet.
            let _ = self.chain.fees().await;
            let s = chain::connection_status();
            Some(if s.anonymous {
                let net = s.anon_network.as_deref().map(net_name).unwrap_or("the anon network");
                let exit = s.egress_ip.as_deref().map(|ip| format!(", exit {ip}")).unwrap_or_default();
                ChainConnection {
                    private: true,
                    title: "Private connection".into(),
                    detail: format!("Reads go through {net}{exit}. {} can't see your IP.", s.rpc_host.unwrap_or_else(|| "The RPC".into())),
                }
            } else {
                ChainConnection {
                    private: false,
                    title: "Direct connection".into(),
                    detail: s.reason.unwrap_or_else(|| "The RPC sees your IP.".into()),
                }
            })
        })
    }

    fn private_token(&self) -> Token {
        Token::Eth
    }

    fn private_report(&self) -> Vec<(String, String)> {
        self.report.borrow().clone()
    }

    fn app_info(&self, app_id: &str) -> BoxFut<'_, Option<(String, String)>> {
        let path = format!("/v1/apps/{}", js_sys::encode_uri_component(app_id));
        Box::pin(async move {
            let v = self.server.get(&path).await.ok()?;
            Some((s(&v, "name")?, s(&v, "origin").unwrap_or_default()))
        })
    }

    fn home_apps(&self) -> BoxFut<'_, Res<Vec<HomeApp>>> {
        Box::pin(self.home_apps_impl())
    }

    fn app_suggestions(&self) -> BoxFut<'_, Vec<Suggestion>> {
        Box::pin(self.suggestions_impl())
    }

    fn private_balance(&self) -> BoxFut<'_, Option<Amount>> {
        Box::pin(self.private_balance_impl())
    }

    fn name_status(&self) -> BoxFut<'_, Res<NameStatus>> {
        Box::pin(self.name_status_impl())
    }

    fn claim_name(&self, label: &str, progress: Rc<dyn Fn(ClaimStage)>) -> BoxFut<'_, Res<NameStatus>> {
        let label = label.to_string();
        Box::pin(async move { self.claim_impl(&label, progress).await })
    }

    fn name_payments(&self, name: &str) -> BoxFut<'_, Res<Vec<StealthPayment>>> {
        let name = name.to_string();
        Box::pin(async move { self.payments_impl(&name).await })
    }

    fn sweep_payment(&self, p: &StealthPayment, progress: ProgressFn) -> BoxFut<'_, Res<SendReceipt>> {
        let p = p.clone();
        Box::pin(async move { self.sweep_impl(&p, progress).await })
    }
}

fn net_name(n: &str) -> &'static str {
    match n {
        "nox" => "Nox",
        "nym" => "Nym",
        "tor-js" => "Tor",
        _ => "the anon network",
    }
}

fn connection_line() -> String {
    let s = chain::connection_status();
    if s.anonymous {
        format!("Anon-RPC ({})", s.anon_network.as_deref().map(net_name).unwrap_or("?"))
    } else {
        format!("Direct: {}", s.reason.unwrap_or_default())
    }
}

fn secs(ms: f64) -> String {
    format!("{:.1} s", (ms / 1000.0).max(0.0))
}

fn short(h: &str) -> String {
    if h.len() > 14 { format!("{}…{}", &h[..8], &h[h.len() - 6..]) } else { h.to_string() }
}

fn private_err(e: String) -> WalletError {
    log(&format!("private: {e}"));
    if e.contains("insufficient") {
        WalletError::InsufficientFunds
    } else if e.contains("Failed to fetch") || e.contains("timed out") || e.contains("http 5") {
        WalletError::Network
    } else {
        WalletError::Other(format!("Private transfer failed: {e}"))
    }
}

/// Builds the real backend. The mock carries delegation for now.
pub fn new_shared(mock: Rc<MockBackend>) -> Rc<RealBackend> {
    Rc::new(RealBackend::new(mock))
}
