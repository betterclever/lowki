//! Backend boundary for the wallet UI.
//!
//! The UI only talks to [`AuthBackend`] and [`WalletBackend`]. Today they are
//! implemented by [`mock::MockBackend`]; the real implementation (the Rust
//! worker running the passkey/PRF unwrap, Kohaku prover and the Lowki server
//! client) plugs in behind the same traits. Nothing in `screens/` knows which
//! implementation is live.

pub mod mock;
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
