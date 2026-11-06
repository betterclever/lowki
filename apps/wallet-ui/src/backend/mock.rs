//! In-memory mock of the Lowki worker + server.
//!
//! Deterministic: the same user/app always gets the same fake address,
//! pairwise ID and balances. Delays are realistic and scale with
//! [`MockControls::set_speed`]. Errors can be forced per category so every
//! UI state is reachable (dev panel or `?fail=` URL flag).
//!
//! The "hashes" here are FNV-1a for stable fake data only. There is no
//! cryptography in this crate.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use gloo_timers::future::TimeoutFuture;

use super::*;

/// Which failure to force on the next relevant call(s).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FailMode {
    PasskeyCancelled,
    PrfUnsupported,
    Network,
    PolicyRejected,
}

impl FailMode {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "passkey_cancelled" | "passkey-cancelled" => FailMode::PasskeyCancelled,
            "prf_unsupported" | "prf-unsupported" | "prf" => FailMode::PrfUnsupported,
            "network" => FailMode::Network,
            "policy_rejected" | "policy-rejected" | "policy" => FailMode::PolicyRejected,
            _ => return None,
        })
    }
    pub fn key(self) -> &'static str {
        match self {
            FailMode::PasskeyCancelled => "passkey_cancelled",
            FailMode::PrfUnsupported => "prf_unsupported",
            FailMode::Network => "network",
            FailMode::PolicyRejected => "policy_rejected",
        }
    }
    pub fn label(self) -> &'static str {
        match self {
            FailMode::PasskeyCancelled => "Passkey cancelled",
            FailMode::PrfUnsupported => "PRF unsupported",
            FailMode::Network => "Network error",
            FailMode::PolicyRejected => "Policy rejected",
        }
    }
    pub fn all() -> [FailMode; 4] {
        [FailMode::PasskeyCancelled, FailMode::PrfUnsupported, FailMode::Network, FailMode::PolicyRejected]
    }
}

/// Dev-only knobs. The UI never sees these; only `dev.rs` does.
pub trait MockControls {
    fn set_fail(&self, f: Option<FailMode>);
    fn fail(&self) -> Option<FailMode>;
    fn set_speed(&self, s: f64);
    /// Fresh wallet: zero balance, no activity.
    fn set_empty(&self, empty: bool);
    /// Jump straight into a signed-in state (for deep links).
    fn seed_signed_in(&self, recovery_saved: bool);
    /// Next Google sign-in is a returning user.
    fn set_returning(&self, r: bool);
}

struct State {
    signed_in: bool,
    returning: bool,
    google: Option<String>,
    passkey: Option<String>,
    email: Option<String>,
    code: CodeStatus,
    empty: bool,
    public: HashMap<String, Vec<Amount>>,
    private: Vec<Amount>,
    activity: HashMap<String, Vec<Activity>>,
    delegations: Vec<Delegation>,
    seen_apps: Vec<String>,
    nonce: u64,
}

pub struct MockBackend {
    st: RefCell<State>,
    fail: Cell<Option<FailMode>>,
    speed: Cell<f64>,
}

const USER_SEED: &str = "mock-user-ada";
pub const MOCK_EMAIL: &str = "ada.l@gmail.com";

/// Apps the mock user has used before (for the "Your apps" list).
pub fn known_apps() -> Vec<AppInfo> {
    vec![
        AppInfo { id: "game-x".into(), name: "Game X".into(), origin: "https://play.gamex.gg".into(), hue: 205 },
        AppInfo { id: "pixel-market".into(), name: "Pixel Market".into(), origin: "https://pixel.market".into(), hue: 32 },
        AppInfo { id: "kingside".into(), name: "Kingside Chess".into(), origin: "https://kingside.club".into(), hue: 150 },
    ]
}

pub fn fnv(parts: &[&str]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for p in parts {
        for b in p.bytes().chain(std::iter::once(0xff)) {
            h ^= b as u64;
            h = h.wrapping_mul(0x0000_0100_0000_01b3);
        }
    }
    h
}

/// Deterministic hex of `n` chars.
pub fn fake_hex(parts: &[&str], n: usize) -> String {
    let mut x = fnv(parts) | 1;
    let mut s = String::with_capacity(n);
    while s.len() < n {
        // xorshift64*
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        let v = x.wrapping_mul(0x2545_F491_4F6C_DD1D);
        s.push_str(&format!("{v:016x}"));
    }
    s.truncate(n);
    s
}

/// The mock USDC contract.
pub fn usdc_address() -> String {
    fake_address(&["usdc"])
}

pub fn fake_address(parts: &[&str]) -> String {
    format!("0x{}", fake_hex(parts, 40))
}

fn usdc(whole: u128, cents: u128) -> Amount {
    Amount::new(Token::Usdc, whole * 1_000_000 + cents * 10_000)
}
fn mon(milli: u128) -> Amount {
    Amount::new(Token::Mon, milli * 1_000_000_000_000_000)
}

impl Default for MockBackend {
    fn default() -> Self {
        Self::new()
    }
}

impl MockBackend {
    pub fn new() -> Self {
        Self {
            st: RefCell::new(State {
                signed_in: false,
                returning: false,
                google: None,
                passkey: None,
                email: None,
                code: CodeStatus::NotSaved,
                empty: false,
                public: HashMap::new(),
                private: vec![usdc(250, 0)],
                activity: HashMap::new(),
                delegations: vec![],
                seen_apps: vec![],
                nonce: 0,
            }),
            fail: Cell::new(None),
            speed: Cell::new(1.0),
        }
    }

    async fn wait(&self, ms: u32) {
        let ms = (ms as f64 * self.speed.get()).round() as u32;
        if ms > 0 {
            TimeoutFuture::new(ms).await;
        }
    }

    fn check(&self, applies: &[FailMode]) -> Res<()> {
        match self.fail.get() {
            Some(f) if applies.contains(&f) => Err(match f {
                FailMode::PasskeyCancelled => WalletError::PasskeyCancelled,
                FailMode::PrfUnsupported => WalletError::PrfUnsupported,
                FailMode::Network => WalletError::Network,
                FailMode::PolicyRejected => WalletError::PolicyRejected {
                    reason: "This request has no daily spending limit. Lowki only approves signers with a cap.".into(),
                },
            }),
            _ => Ok(()),
        }
    }

    fn next_hash(&self, kind: &str) -> String {
        let mut st = self.st.borrow_mut();
        st.nonce += 1;
        format!("0x{}", fake_hex(&[kind, &st.nonce.to_string()], 64))
    }

    fn ensure_app(&self, app: &AppInfo) {
        let mut st = self.st.borrow_mut();
        if !st.seen_apps.contains(&app.id) {
            st.seen_apps.push(app.id.clone());
        }
        if st.public.contains_key(&app.id) {
            return;
        }
        let (bal, act) = if st.empty {
            (vec![Amount::zero(Token::Usdc), Amount::zero(Token::Mon)], vec![])
        } else {
            let seed = fnv(&[USER_SEED, &app.id]);
            let extra = (seed % 9000) as u128; // stable per app
            let bal = vec![usdc(120 + extra / 100, extra % 100), mon(1520 + (seed % 500) as u128)];
            let act = vec![
                Activity {
                    kind: ActivityKind::Received,
                    amount: usdc(40, 0),
                    counterparty: Some(fake_address(&["friend", &app.id])),
                    note: None,
                    when: "Today, 09:12".into(),
                },
                Activity {
                    kind: ActivityKind::AppAction,
                    amount: usdc(12, 50),
                    counterparty: Some(format!("{} Market", app.name)),
                    note: Some("Bought Iron Sword #2041".into()),
                    when: "Yesterday".into(),
                },
                Activity {
                    kind: ActivityKind::MovedToPrivate,
                    amount: usdc(100, 0),
                    counterparty: None,
                    note: None,
                    when: "3 Nov".into(),
                },
                Activity {
                    kind: ActivityKind::Received,
                    amount: mon(2000),
                    counterparty: Some("Monad faucet".into()),
                    note: None,
                    when: "1 Nov".into(),
                },
            ];
            (bal, act)
        };
        st.public.insert(app.id.clone(), bal);
        st.activity.insert(app.id.clone(), act);
    }

    fn adjust(list: &mut Vec<Amount>, a: Amount, add: bool) -> Res<()> {
        if let Some(x) = list.iter_mut().find(|x| x.token == a.token) {
            if add {
                x.units += a.units;
            } else if x.units >= a.units {
                x.units -= a.units;
            } else {
                return Err(WalletError::InsufficientFunds);
            }
            Ok(())
        } else if add {
            list.push(a);
            Ok(())
        } else {
            Err(WalletError::InsufficientFunds)
        }
    }

    fn methods(&self) -> Vec<SignInMethod> {
        let st = self.st.borrow();
        vec![
            SignInMethod { kind: MethodKind::Passkey, detail: st.passkey.clone() },
            SignInMethod { kind: MethodKind::Google, detail: st.google.clone() },
            SignInMethod { kind: MethodKind::Email, detail: st.email.clone() },
        ]
    }

    fn attestation(&self) -> Attestation {
        Attestation {
            verified: true,
            build: "3f2a9c1".into(),
            measurement: format!("sha384:3f2a9c1{}", fake_hex(&["mrenclave"], 41)),
            checked: "just now".into(),
        }
    }

    async fn run_proof(&self, progress: &ProgressFn) {
        // Stage durations (ms) at speed 1.0: the proof itself is ~1 s.
        let plan = [(ProofStage::SyncingPool, 600u32, 6u32), (ProofStage::BuildingProof, 1100, 22), (ProofStage::Submitting, 600, 6)];
        for (stage, total, steps) in plan {
            for i in 0..=steps {
                progress(ProofProgress { stage, fraction: i as f32 / steps as f32 });
                if i < steps {
                    self.wait(total / steps).await;
                }
            }
        }
    }
}

impl MockControls for MockBackend {
    fn set_fail(&self, f: Option<FailMode>) {
        self.fail.set(f);
    }
    fn fail(&self) -> Option<FailMode> {
        self.fail.get()
    }
    fn set_speed(&self, s: f64) {
        self.speed.set(s.max(0.0));
    }
    fn set_empty(&self, empty: bool) {
        let mut st = self.st.borrow_mut();
        st.empty = empty;
        st.public.clear();
        st.activity.clear();
        if empty {
            st.private = vec![];
        }
    }
    fn seed_signed_in(&self, recovery_saved: bool) {
        let mut st = self.st.borrow_mut();
        st.signed_in = true;
        st.google = Some(MOCK_EMAIL.into());
        st.passkey = Some("iCloud Keychain".into());
        st.code = if recovery_saved { CodeStatus::BackedUp(BackupTarget::ICloud) } else { CodeStatus::NotSaved };
        if st.delegations.is_empty() {
            let pm = known_apps()[1].clone();
            st.delegations.push(Delegation {
                id: "dlg-pixel-1".into(),
                app: pm,
                policy: DelegationPolicy {
                    chain: "monad".into(),
                    expires: "2027-03-01".into(),
                    rules: vec![PolicyRule::Call {
                        contract: fake_address(&["pixel-market-listings"]),
                        contract_label: Some("Pixel Market listings".into()),
                        function: "relist(uint256,uint256)".into(),
                        max_per_day: Some(20),
                    }],
                },
                attestation: self.attestation(),
                granted: "12 Oct 2026".into(),
            });
        }
    }
    fn set_returning(&self, r: bool) {
        self.st.borrow_mut().returning = r;
    }
}

impl AuthBackend for MockBackend {
    fn sign_in_google(&self, _app: &AppInfo) -> BoxFut<'_, Res<AuthOutcome>> {
        Box::pin(async move {
            self.wait(1400).await; // popup round trip
            self.check(&[FailMode::Network])?;
            let mut st = self.st.borrow_mut();
            st.google = Some(MOCK_EMAIL.into());
            Ok(if st.returning { AuthOutcome::NeedsPasskey } else { AuthOutcome::NewUser })
        })
    }

    fn send_email_code(&self, email: &str) -> BoxFut<'_, Res<()>> {
        let email = email.to_string();
        Box::pin(async move {
            self.wait(700).await;
            self.check(&[FailMode::Network])?;
            self.st.borrow_mut().email = Some(email);
            Ok(())
        })
    }

    fn verify_email_code(&self, _email: &str, code: &str) -> BoxFut<'_, Res<AuthOutcome>> {
        let ok = code.len() == 6 && code.chars().all(|c| c.is_ascii_digit()) && code != "000000";
        Box::pin(async move {
            self.wait(800).await;
            self.check(&[FailMode::Network])?;
            if !ok {
                return Err(WalletError::InvalidCode);
            }
            Ok(if self.st.borrow().returning { AuthOutcome::NeedsPasskey } else { AuthOutcome::NewUser })
        })
    }

    fn sign_in_passkey(&self, app: &AppInfo) -> BoxFut<'_, Res<Session>> {
        let app = app.clone();
        Box::pin(async move {
            self.wait(1100).await;
            self.check(&[FailMode::PasskeyCancelled, FailMode::PrfUnsupported, FailMode::Network])?;
            self.seed_signed_in(true);
            self.session(&app).await
        })
    }

    fn create_passkey(&self) -> BoxFut<'_, Res<RecoveryCode>> {
        Box::pin(async move {
            self.wait(1300).await; // OS sheet + PRF + share split
            self.check(&[FailMode::PasskeyCancelled, FailMode::PrfUnsupported, FailMode::Network])?;
            let mut st = self.st.borrow_mut();
            st.passkey = Some("iCloud Keychain".into());
            st.signed_in = true;
            Ok(mock_recovery_code())
        })
    }

    fn continue_without_prf(&self) -> BoxFut<'_, Res<RecoveryCode>> {
        Box::pin(async move {
            self.wait(900).await;
            self.check(&[FailMode::Network])?;
            self.st.borrow_mut().signed_in = true;
            Ok(mock_recovery_code())
        })
    }

    fn recover_with_code(&self, code: &str) -> BoxFut<'_, Res<()>> {
        let ok = code.chars().filter(|c| c.is_ascii_alphanumeric()).count() >= 16;
        Box::pin(async move {
            self.wait(900).await;
            self.check(&[FailMode::Network])?;
            if !ok {
                return Err(WalletError::Other("That recovery code isn't right. Check each group and try again.".into()));
            }
            self.st.borrow_mut().signed_in = true;
            Ok(())
        })
    }

    fn backup_recovery(&self, target: BackupTarget) -> BoxFut<'_, Res<()>> {
        Box::pin(async move {
            self.wait(900).await;
            self.check(&[FailMode::Network])?;
            self.st.borrow_mut().code = CodeStatus::BackedUp(target);
            Ok(())
        })
    }

    fn acknowledge_recovery(&self, saved: bool) -> BoxFut<'_, Res<()>> {
        Box::pin(async move {
            let mut st = self.st.borrow_mut();
            if saved && st.code == CodeStatus::NotSaved {
                st.code = CodeStatus::Saved;
            }
            Ok(())
        })
    }

    fn session(&self, app: &AppInfo) -> BoxFut<'_, Res<Session>> {
        let app = app.clone();
        Box::pin(async move {
            self.wait(300).await;
            self.check(&[FailMode::Network])?;
            self.ensure_app(&app);
            let st = self.st.borrow();
            Ok(Session {
                address: fake_address(&[USER_SEED, "account", &app.id]),
                user_id: format!("lk_{}", fake_hex(&[USER_SEED, "pairwise", &app.id], 24)),
                access_token: format!("eyJhbGciOiJFZERTQSJ9.{}.{}", fake_hex(&["tok", &app.id], 32), fake_hex(&["sig", &app.id], 22)),
                account_label: st.google.clone().or(st.email.clone()).unwrap_or_else(|| MOCK_EMAIL.into()),
            })
        })
    }

    fn sign_in_methods(&self) -> BoxFut<'_, Res<Vec<SignInMethod>>> {
        Box::pin(async move {
            self.wait(200).await;
            Ok(self.methods())
        })
    }

    fn link_method(&self, kind: MethodKind) -> BoxFut<'_, Res<Vec<SignInMethod>>> {
        Box::pin(async move {
            self.wait(900).await;
            self.check(&[FailMode::Network])?;
            {
                let mut st = self.st.borrow_mut();
                match kind {
                    MethodKind::Google => st.google = Some(MOCK_EMAIL.into()),
                    MethodKind::Passkey => st.passkey = Some("iCloud Keychain".into()),
                    MethodKind::Email => st.email = Some("ada@lovelace.dev".into()),
                }
            }
            Ok(self.methods())
        })
    }

    fn unlink_method(&self, kind: MethodKind) -> BoxFut<'_, Res<Vec<SignInMethod>>> {
        Box::pin(async move {
            self.wait(600).await;
            self.check(&[FailMode::Network])?;
            {
                let mut st = self.st.borrow_mut();
                match kind {
                    MethodKind::Google => st.google = None,
                    MethodKind::Passkey => st.passkey = None,
                    MethodKind::Email => st.email = None,
                }
            }
            Ok(self.methods())
        })
    }

    fn recovery_status(&self) -> BoxFut<'_, Res<RecoveryStatus>> {
        Box::pin(async move {
            self.wait(200).await;
            let st = self.st.borrow();
            Ok(RecoveryStatus { passkey_share: st.passkey.is_some(), server_share: st.google.is_some(), code: st.code })
        })
    }

    fn sign_out(&self) -> BoxFut<'_, Res<()>> {
        Box::pin(async move {
            self.wait(300).await;
            self.st.borrow_mut().signed_in = false;
            Ok(())
        })
    }
}

fn mock_recovery_code() -> RecoveryCode {
    RecoveryCode("K7QM-9XHD-4FPA-T3RW-B6NE-2LCY".into())
}

impl WalletBackend for MockBackend {
    fn balances(&self, app: &AppInfo) -> BoxFut<'_, Res<Balances>> {
        let app = app.clone();
        Box::pin(async move {
            self.wait(450).await;
            self.check(&[FailMode::Network])?;
            self.ensure_app(&app);
            let st = self.st.borrow();
            let public = st.public.get(&app.id).cloned().unwrap_or_default();
            let usd_cents = public
                .iter()
                .map(|a| match a.token {
                    Token::Usdc => (a.units / 10_000) as u64,
                    // 1 MON ~ $0.42 in the mock
                    Token::Mon => (a.units / 1_000_000_000_000_000 * 42 / 1000) as u64,
                    Token::Eth => 0,
                })
                .sum();
            Ok(Balances { public, private: st.private.clone(), usd_cents, network: "Monad".into() })
        })
    }

    fn activity(&self, app: &AppInfo) -> BoxFut<'_, Res<Vec<Activity>>> {
        let app = app.clone();
        Box::pin(async move {
            self.wait(500).await;
            self.check(&[FailMode::Network])?;
            self.ensure_app(&app);
            Ok(self.st.borrow().activity.get(&app.id).cloned().unwrap_or_default())
        })
    }

    fn send(&self, app: &AppInfo, req: SendRequest, progress: ProgressFn) -> BoxFut<'_, Res<SendReceipt>> {
        let app = app.clone();
        Box::pin(async move {
            if !(req.to.starts_with("0x") && req.to.len() == 42 && req.to[2..].chars().all(|c| c.is_ascii_hexdigit())) {
                return Err(WalletError::InvalidAddress);
            }
            self.ensure_app(&app);
            {
                let st = self.st.borrow();
                let have = if req.private {
                    st.private.iter().find(|a| a.token == req.amount.token).map_or(0, |a| a.units)
                } else {
                    st.public.get(&app.id).and_then(|l| l.iter().find(|a| a.token == req.amount.token)).map_or(0, |a| a.units)
                };
                if have < req.amount.units {
                    return Err(WalletError::InsufficientFunds);
                }
            }
            if req.private {
                self.check(&[FailMode::Network])?;
                self.run_proof(&progress).await;
            } else {
                self.wait(1200).await;
                self.check(&[FailMode::Network])?;
            }
            {
                let mut st = self.st.borrow_mut();
                if req.private {
                    Self::adjust(&mut st.private, req.amount, false)?;
                } else if let Some(l) = st.public.get_mut(&app.id) {
                    Self::adjust(l, req.amount, false)?;
                }
                let kind = if req.private { ActivityKind::SentPrivately } else { ActivityKind::Sent };
                st.activity.entry(app.id.clone()).or_default().insert(
                    0,
                    Activity { kind, amount: req.amount, counterparty: Some(req.to.clone()), note: None, when: "Just now".into() },
                );
            }
            Ok(SendReceipt { hash: self.next_hash("send"), private: req.private })
        })
    }

    fn move_private(&self, app: &AppInfo, req: VaultMove, progress: ProgressFn) -> BoxFut<'_, Res<SendReceipt>> {
        let app = app.clone();
        Box::pin(async move {
            self.ensure_app(&app);
            self.check(&[FailMode::Network])?;
            match req.direction {
                VaultDirection::In => {
                    // Deposit is a plain transaction: no proof.
                    progress(ProofProgress { stage: ProofStage::Submitting, fraction: 0.2 });
                    self.wait(1000).await;
                }
                VaultDirection::Out => self.run_proof(&progress).await,
            }
            let mut st = self.st.borrow_mut();
            let st = &mut *st;
            let public = st.public.entry(app.id.clone()).or_default();
            match req.direction {
                VaultDirection::In => {
                    Self::adjust(public, req.amount, false)?;
                    Self::adjust(&mut st.private, req.amount, true)?;
                }
                VaultDirection::Out => {
                    Self::adjust(&mut st.private, req.amount, false)?;
                    Self::adjust(public, req.amount, true)?;
                }
            }
            let kind = match req.direction {
                VaultDirection::In => ActivityKind::MovedToPrivate,
                VaultDirection::Out => ActivityKind::MovedFromPrivate,
            };
            st.activity
                .entry(app.id.clone())
                .or_default()
                .insert(0, Activity { kind, amount: req.amount, counterparty: None, note: None, when: "Just now".into() });
            st.nonce += 1;
            Ok(SendReceipt { hash: format!("0x{}", fake_hex(&["vault", &st.nonce.to_string()], 64)), private: true })
        })
    }

    fn sign_message(&self, _app: &AppInfo, message: &str) -> BoxFut<'_, Res<String>> {
        let m = message.to_string();
        Box::pin(async move {
            self.wait(500).await;
            self.check(&[FailMode::Network])?;
            Ok(format!("0x{}", fake_hex(&["sig", &m], 130)))
        })
    }

    fn sign_typed_data(&self, _app: &AppInfo, typed: &serde_json::Value) -> BoxFut<'_, Res<String>> {
        let t = typed.to_string();
        Box::pin(async move {
            self.wait(500).await;
            self.check(&[FailMode::Network])?;
            Ok(format!("0x{}", fake_hex(&["sig712", &t], 130)))
        })
    }

    fn preview_transaction(&self, app: &AppInfo, tx: &TxRequest) -> BoxFut<'_, Res<TxPreview>> {
        let app = app.clone();
        let tx = tx.clone();
        Box::pin(async move {
            self.wait(400).await;
            self.check(&[FailMode::Network])?;
            let wei: u128 = tx.value.parse().unwrap_or(0);
            let call = if tx.data.starts_with("0x6a2c4e1b") {
                Some(DecodedCall {
                    function: "buyItem".into(),
                    args: vec![("Item".into(), "Iron Sword #2041".into()), ("Max price".into(), "12.50 USDC".into())],
                })
            } else if tx.data.len() > 2 {
                Some(DecodedCall { function: format!("Unknown function {}", &tx.data[..10.min(tx.data.len())]), args: vec![] })
            } else {
                None
            };
            let sponsored = app.id == "game-x";
            Ok(TxPreview {
                to: tx.to.clone(),
                to_label: call.as_ref().filter(|c| c.function == "buyItem").map(|_| format!("{} Market", app.name)),
                value: Amount::new(Token::Mon, wei),
                call,
                gas: if sponsored {
                    GasQuote::Sponsored { by: app.name.clone() }
                } else {
                    GasQuote::Paid { amount: Amount::new(Token::Mon, 2_100_000_000_000_000) }
                },
                network: "Monad".into(),
            })
        })
    }

    fn send_transaction(&self, app: &AppInfo, tx: &TxRequest) -> BoxFut<'_, Res<SendReceipt>> {
        let app = app.clone();
        let _ = tx;
        Box::pin(async move {
            self.wait(1100).await;
            self.check(&[FailMode::Network])?;
            self.ensure_app(&app);
            {
                let mut st = self.st.borrow_mut();
                st.activity.entry(app.id.clone()).or_default().insert(
                    0,
                    Activity {
                        kind: ActivityKind::AppAction,
                        amount: Amount::new(Token::Usdc, 12_500_000),
                        counterparty: Some(format!("{} Market", app.name)),
                        note: Some("Bought Iron Sword #2041".into()),
                        when: "Just now".into(),
                    },
                );
            }
            Ok(SendReceipt { hash: self.next_hash("tx"), private: false })
        })
    }

    fn review_delegation(&self, app: &AppInfo, policy: &DelegationPolicy) -> BoxFut<'_, Res<DelegationReview>> {
        let app = app.clone();
        let policy = policy.clone();
        Box::pin(async move {
            self.wait(700).await; // fetch + verify attestation quote
            self.check(&[FailMode::Network, FailMode::PolicyRejected])?;
            for r in &policy.rules {
                match r {
                    PolicyRule::Swap { max_per_day, .. } | PolicyRule::Transfer { max_per_day, .. } if max_per_day == "unlimited" => {
                        return Err(WalletError::PolicyRejected {
                            reason: "This request has no daily spending limit. Lowki only approves signers with a cap.".into(),
                        });
                    }
                    PolicyRule::Transfer { to, .. } if to == "*" || to == "any" => {
                        return Err(WalletError::PolicyRejected {
                            reason: "This request lets the signer send to any address. Lowki only allows fixed recipients.".into(),
                        });
                    }
                    _ => {}
                }
            }
            Ok(DelegationReview {
                request_id: self.next_hash("dlg")[2..14].to_string(),
                policy,
                attestation: self.attestation(),
                requester: format!("{} server", app.name),
            })
        })
    }

    fn approve_delegation(&self, app: &AppInfo, review: &DelegationReview) -> BoxFut<'_, Res<Delegation>> {
        let app = app.clone();
        let review = review.clone();
        Box::pin(async move {
            self.wait(1000).await; // seal key into the enclave
            self.check(&[FailMode::Network])?;
            let d = Delegation {
                id: format!("dlg-{}", review.request_id),
                app,
                policy: review.policy,
                attestation: review.attestation,
                granted: "Today".into(),
            };
            self.st.borrow_mut().delegations.insert(0, d.clone());
            Ok(d)
        })
    }

    fn delegations(&self) -> BoxFut<'_, Res<Vec<Delegation>>> {
        Box::pin(async move {
            self.wait(250).await;
            self.check(&[FailMode::Network])?;
            Ok(self.st.borrow().delegations.clone())
        })
    }

    fn revoke_delegation(&self, id: &str) -> BoxFut<'_, Res<()>> {
        let id = id.to_string();
        Box::pin(async move {
            self.wait(800).await;
            self.check(&[FailMode::Network])?;
            self.st.borrow_mut().delegations.retain(|d| d.id != id);
            Ok(())
        })
    }

    fn token_at(&self, address: &str) -> Option<Token> {
        address.eq_ignore_ascii_case(&usdc_address()).then_some(Token::Usdc)
    }

    fn app_accounts(&self) -> BoxFut<'_, Res<Vec<AppAccount>>> {
        Box::pin(async move {
            self.wait(250).await;
            self.check(&[FailMode::Network])?;
            let used = ["Now", "2 days ago", "18 Oct"];
            Ok(known_apps()
                .into_iter()
                .zip(used)
                .map(|(app, when)| AppAccount {
                    address: fake_address(&[USER_SEED, "account", &app.id]),
                    app,
                    last_used: when.into(),
                })
                .collect())
        })
    }
}

/// Convenience for `main`: one mock behind both views.
pub fn new_shared() -> Rc<MockBackend> {
    Rc::new(MockBackend::new())
}

#[cfg(test)]
mod tests {
    #[test]
    fn deterministic_addresses() {
        assert_eq!(super::usdc_address(), super::usdc_address());
        assert_eq!(super::usdc_address().len(), 42);
        println!("usdc={} market={}", super::usdc_address(), super::fake_address(&["market", "game-x"]));
    }
}
