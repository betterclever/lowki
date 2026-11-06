//! Backend boundary for the wallet UI.
//!
//! The UI only talks to [`AuthBackend`] and [`WalletBackend`]. Two implementations:
//! [`real::RealBackend`] (lowki-worker in a Web Worker, lowki-server, a chain RPC) and
//! [`mock::MockBackend`] (screenshots, deep links, the dev panel). `main.rs` picks one at
//! runtime (`?backend=mock|real`, default real). Nothing in `screens/` knows which
//! implementation is live.

pub mod mock;
pub mod real;
pub mod types;

use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;

pub use types::*;

/// Futures in the browser are single threaded, so the traits return
/// non-`Send` boxed futures.
pub type BoxFut<'a, T> = Pin<Box<dyn Future<Output = T> + 'a>>;
pub type Res<T> = Result<T, WalletError>;

/// Progress callback for the private send pipeline.
pub type ProgressFn = Rc<dyn Fn(ProofProgress)>;

/// Identity, keys and recovery.
pub trait AuthBackend {
    /// Opens the Google popup on the Lowki origin. Google alone never unlocks
    /// the seed: the outcome says what is still needed.
    fn sign_in_google(&self, app: &AppInfo) -> BoxFut<'_, Res<AuthOutcome>>;
    /// Sends a one-time code to `email`.
    fn send_email_code(&self, email: &str) -> BoxFut<'_, Res<()>>;
    fn verify_email_code(&self, email: &str, code: &str) -> BoxFut<'_, Res<AuthOutcome>>;
    /// Signs in with an existing passkey (PRF unwraps the seed).
    fn sign_in_passkey(&self, app: &AppInfo) -> BoxFut<'_, Res<Session>>;
    /// Creates the passkey, wraps the seed with its PRF output and splits the
    /// seed 2-of-3 (passkey share, server share, recovery code).
    fn create_passkey(&self) -> BoxFut<'_, Res<RecoveryCode>>;
    /// Fallback when PRF is missing: unlock with Google + recovery code.
    fn continue_without_prf(&self) -> BoxFut<'_, Res<RecoveryCode>>;
    /// Lost passkey: after an email (or Google) sign-in, the server releases share S and
    /// the worker rebuilds the seed from S + the recovery code R. The next
    /// [`AuthBackend::create_passkey`] wraps that same seed for a new passkey.
    fn recover_with_code(&self, code: &str) -> BoxFut<'_, Res<()>>;
    fn backup_recovery(&self, target: BackupTarget) -> BoxFut<'_, Res<()>>;
    /// Marks the recovery code as shown-and-acknowledged (or postponed).
    fn acknowledge_recovery(&self, saved: bool) -> BoxFut<'_, Res<()>>;
    /// Finishes sign-in for `app`, deriving the pairwise account.
    fn session(&self, app: &AppInfo) -> BoxFut<'_, Res<Session>>;
    fn sign_in_methods(&self) -> BoxFut<'_, Res<Vec<SignInMethod>>>;
    fn link_method(&self, kind: MethodKind) -> BoxFut<'_, Res<Vec<SignInMethod>>>;
    fn unlink_method(&self, kind: MethodKind) -> BoxFut<'_, Res<Vec<SignInMethod>>>;
    fn recovery_status(&self) -> BoxFut<'_, Res<RecoveryStatus>>;
    fn sign_out(&self) -> BoxFut<'_, Res<()>>;
}

/// Balances, signing, private pool and delegation for the current app account.
pub trait WalletBackend {
    fn balances(&self, app: &AppInfo) -> BoxFut<'_, Res<Balances>>;
    fn activity(&self, app: &AppInfo) -> BoxFut<'_, Res<Vec<Activity>>>;
    /// Public send, or a private send through the pool when `req.private`.
    /// Private sends report [`ProofProgress`] while the proof is built.
    fn send(&self, app: &AppInfo, req: SendRequest, progress: ProgressFn) -> BoxFut<'_, Res<SendReceipt>>;
    /// Move funds between the app account and the private balance.
    fn move_private(&self, app: &AppInfo, req: VaultMove, progress: ProgressFn) -> BoxFut<'_, Res<SendReceipt>>;
    fn sign_message(&self, app: &AppInfo, message: &str) -> BoxFut<'_, Res<String>>;
    fn sign_typed_data(&self, app: &AppInfo, typed: &serde_json::Value) -> BoxFut<'_, Res<String>>;
    /// Decodes calldata and quotes gas for a transaction preview.
    fn preview_transaction(&self, app: &AppInfo, tx: &TxRequest) -> BoxFut<'_, Res<TxPreview>>;
    fn send_transaction(&self, app: &AppInfo, tx: &TxRequest) -> BoxFut<'_, Res<SendReceipt>>;
    /// Validates a delegation request and fetches the signer attestation.
    fn review_delegation(&self, app: &AppInfo, policy: &DelegationPolicy) -> BoxFut<'_, Res<DelegationReview>>;
    /// Seals the app account key into the attested signer. Nothing on chain.
    fn approve_delegation(&self, app: &AppInfo, review: &DelegationReview) -> BoxFut<'_, Res<Delegation>>;
    fn delegations(&self) -> BoxFut<'_, Res<Vec<Delegation>>>;
    fn revoke_delegation(&self, id: &str) -> BoxFut<'_, Res<()>>;
    /// Every app account of this user. Never shared with any app.
    fn app_accounts(&self) -> BoxFut<'_, Res<Vec<AppAccount>>>;
    /// Token registry lookup, used to format amounts in typed data.
    fn token_at(&self, address: &str) -> Option<Token>;
    /// How chain reads and broadcasts travel. `None` when there is no chain (the mock).
    /// Resolves once the connection is decided (the anon network may take seconds to boot).
    fn chain_connection(&self) -> BoxFut<'_, Option<ChainConnection>> {
        Box::pin(async { None })
    }
    /// The token the private balance holds (the real pool is ETH).
    fn private_token(&self) -> Token {
        Token::Usdc
    }
    /// Label/value facts about the last private operation (stages, proof backend and time,
    /// relayer fee), for the done screen. Empty when there is nothing real to report.
    fn private_report(&self) -> Vec<(String, String)> {
        Vec::new()
    }

    // ---- wallet home (top-level; never reachable from inside an app) ---------------------
    /// The registered display name and origin of an app ID (`GET /v1/apps/{id}`).
    fn app_info(&self, _app_id: &str) -> BoxFut<'_, Option<(String, String)>> {
        Box::pin(async { None })
    }
    /// Every app the user has used (the encrypted app list), with addresses and balances.
    fn home_apps(&self) -> BoxFut<'_, Res<Vec<HomeApp>>> {
        Box::pin(async { Ok(vec![]) })
    }
    /// Demo apps to try (from `lowki-config.json`).
    fn app_suggestions(&self) -> BoxFut<'_, Vec<Suggestion>> {
        Box::pin(async { vec![] })
    }
    /// The private balance (one per wallet, not per app). `None` when the pool can't be read.
    fn private_balance(&self) -> BoxFut<'_, Option<Amount>> {
        Box::pin(async { None })
    }
    fn name_status(&self) -> BoxFut<'_, Res<NameStatus>> {
        Box::pin(async { Ok(NameStatus::default()) })
    }
    /// Claims `<label>.<parent>`: the key worker signs, lowki-server submits (sponsored).
    fn claim_name(&self, _label: &str, _progress: Rc<dyn Fn(ClaimStage)>) -> BoxFut<'_, Res<NameStatus>> {
        Box::pin(async { Err(WalletError::Other("Names aren't available here.".into())) })
    }
    /// Payments to the name's stealth addresses (gateway records, scanned with the viewing key).
    fn name_payments(&self, _name: &str) -> BoxFut<'_, Res<Vec<StealthPayment>>> {
        Box::pin(async { Ok(vec![]) })
    }
    /// Moves a found payment into the private balance (a pool deposit from the stealth address).
    fn sweep_payment(&self, _p: &StealthPayment, _progress: ProgressFn) -> BoxFut<'_, Res<SendReceipt>> {
        Box::pin(async { Err(WalletError::Other("Moving payments isn't available here.".into())) })
    }
}

pub trait Backend: AuthBackend + WalletBackend {}
impl<T: AuthBackend + WalletBackend> Backend for T {}

thread_local! {
    static BACKEND: std::cell::RefCell<Option<Rc<dyn Backend>>> = const { std::cell::RefCell::new(None) };
}

/// Installs the live backend. Called once from `main`.
pub fn install(b: Rc<dyn Backend>) {
    BACKEND.with(|cell| *cell.borrow_mut() = Some(b));
}

/// The live backend.
pub fn backend() -> Rc<dyn Backend> {
    BACKEND.with(|cell| cell.borrow().clone().expect("backend installed in main"))
}
