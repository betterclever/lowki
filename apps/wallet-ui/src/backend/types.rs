//! Plain data passed across the backend boundary. No crypto lives here.

use serde::{Deserialize, Serialize};

/// The app (dapp) the wallet is currently embedded in.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct AppInfo {
    pub id: String,
    pub name: String,
    pub origin: String,
    /// Hue (0-360) for the app's monogram tile when it has no icon.
    pub hue: u16,
}

impl AppInfo {
    pub fn monogram(&self) -> String {
        self.name
            .split_whitespace()
            .filter_map(|w| w.chars().next())
            .take(2)
            .collect::<String>()
            .to_uppercase()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AuthOutcome {
    /// First time: create a passkey, then show the recovery code.
    NewUser,
    /// Known user: their passkey unlocks the wallet.
    NeedsPasskey,
}

/// What the app is allowed to know after sign-in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Session {
    /// The pairwise account address for this app only.
    pub address: String,
    /// Pairwise user ID: stable for this app, unlinkable across apps.
    pub user_id: String,
    pub access_token: String,
    /// Shown to the user only (never sent to the app).
    pub account_label: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MethodKind {
    Google,
    Passkey,
    Email,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SignInMethod {
    pub kind: MethodKind,
    /// e.g. the Google account or "iCloud Keychain". `None` = not linked.
    pub detail: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RecoveryCode(pub String);

impl RecoveryCode {
    pub fn groups(&self) -> Vec<String> {
        self.0.split('-').map(str::to_string).collect()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackupTarget {
    ICloud,
    GoogleDrive,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CodeStatus {
    NotSaved,
    Saved,
    BackedUp(BackupTarget),
}

/// Shamir 2-of-3 share status.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RecoveryStatus {
    pub passkey_share: bool,
    pub server_share: bool,
    pub code: CodeStatus,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Token {
    Usdc,
    Mon,
    /// Native ether (the real backend's chain, Sepolia by default).
    Eth,
}

impl Token {
    pub fn symbol(self) -> &'static str {
        match self {
            Token::Usdc => "USDC",
            Token::Mon => "MON",
            Token::Eth => "ETH",
        }
    }
    pub fn name(self) -> &'static str {
        match self {
            Token::Usdc => "USD Coin",
            Token::Mon => "Monad",
            Token::Eth => "Ether",
        }
    }
    pub fn decimals(self) -> u32 {
        match self {
            Token::Usdc => 6,
            Token::Mon | Token::Eth => 18,
        }
    }
    /// Display precision.
    pub fn shown(self) -> u32 {
        match self {
            Token::Usdc => 2,
            Token::Mon | Token::Eth => 4,
        }
    }
    pub fn all() -> [Token; 2] {
        [Token::Usdc, Token::Mon]
    }
}

/// Token amount in base units.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Amount {
    pub token: Token,
    pub units: u128,
}

impl Amount {
    pub fn new(token: Token, units: u128) -> Self {
        Self { token, units }
    }
    pub fn zero(token: Token) -> Self {
        Self { token, units: 0 }
    }
    /// Parses a user-typed decimal ("12.5"). Returns `None` when malformed or
    /// more precise than the token allows.
    pub fn parse(token: Token, s: &str) -> Option<Self> {
        let s = s.trim().replace(',', "");
        if s.is_empty() {
            return None;
        }
        let (int, frac) = match s.split_once('.') {
            Some((i, f)) => (i, f),
            None => (s.as_str(), ""),
        };
        if (int.is_empty() && frac.is_empty())
            || !int.chars().all(|c| c.is_ascii_digit())
            || !frac.chars().all(|c| c.is_ascii_digit())
            || frac.len() as u32 > token.decimals()
        {
            return None;
        }
        let scale = 10u128.pow(token.decimals());
        let int: u128 = if int.is_empty() { 0 } else { int.parse().ok()? };
        let mut frac_s = frac.to_string();
        while (frac_s.len() as u32) < token.decimals() {
            frac_s.push('0');
        }
        let frac: u128 = if frac_s.is_empty() { 0 } else { frac_s.parse().ok()? };
        Some(Self { token, units: int.checked_mul(scale)?.checked_add(frac)? })
    }
    /// "1,234.50" (no symbol).
    pub fn fmt_plain(&self) -> String {
        let scale = 10u128.pow(self.token.decimals());
        let shown = self.token.shown();
        let step = 10u128.pow(self.token.decimals() - shown);
        let rounded = self.units / step;
        let int = rounded / 10u128.pow(shown);
        let frac = rounded % 10u128.pow(shown);
        let _ = scale;
        let int_s = group_thousands(int);
        format!("{int_s}.{frac:0width$}", width = shown as usize)
    }
    /// "1,234.50 USDC".
    pub fn fmt(&self) -> String {
        format!("{} {}", self.fmt_plain(), self.token.symbol())
    }
}

pub fn group_thousands(n: u128) -> String {
    let s = n.to_string();
    let mut out = String::new();
    for (i, c) in s.chars().enumerate() {
        if i > 0 && (s.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Balances {
    pub public: Vec<Amount>,
    /// The private (pool) balance. Only the user sees it.
    pub private: Vec<Amount>,
    /// Display value of the public balance in US cents.
    pub usd_cents: u64,
    /// Chain the public balance lives on, e.g. "Monad" or "Sepolia".
    pub network: String,
}

impl Balances {
    pub fn public_of(&self, t: Token) -> Amount {
        self.public.iter().copied().find(|a| a.token == t).unwrap_or(Amount::zero(t))
    }
    /// Tokens this account holds or can hold, in display order.
    pub fn tokens(&self) -> Vec<Token> {
        self.public.iter().map(|a| a.token).collect()
    }
    pub fn private_of(&self, t: Token) -> Amount {
        self.private.iter().copied().find(|a| a.token == t).unwrap_or(Amount::zero(t))
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActivityKind {
    Received,
    Sent,
    SentPrivately,
    MovedToPrivate,
    MovedFromPrivate,
    AppAction,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Activity {
    pub kind: ActivityKind,
    pub amount: Amount,
    /// Address or label of the other side, when there is one.
    pub counterparty: Option<String>,
    pub note: Option<String>,
    pub when: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SendRequest {
    pub to: String,
    pub amount: Amount,
    pub private: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VaultDirection {
    /// App account -> private balance (shield).
    In,
    /// Private balance -> app account (unshield).
    Out,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VaultMove {
    pub direction: VaultDirection,
    pub amount: Amount,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProofStage {
    SyncingPool,
    BuildingProof,
    Submitting,
}

impl ProofStage {
    pub fn all() -> [ProofStage; 3] {
        [ProofStage::SyncingPool, ProofStage::BuildingProof, ProofStage::Submitting]
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ProofProgress {
    pub stage: ProofStage,
    /// 0.0..=1.0 within the stage.
    pub fraction: f32,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SendReceipt {
    pub hash: String,
    pub private: bool,
}

/// A transaction as requested by the app.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct TxRequest {
    pub to: String,
    /// Value in wei as a decimal string.
    #[serde(default)]
    pub value: String,
    #[serde(default)]
    pub data: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DecodedCall {
    pub function: String,
    pub args: Vec<(String, String)>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum GasQuote {
    Sponsored { by: String },
    Paid { amount: Amount },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TxPreview {
    pub to: String,
    pub to_label: Option<String>,
    pub value: Amount,
    pub call: Option<DecodedCall>,
    pub gas: GasQuote,
    pub network: String,
}

/// A delegation request, as sent by the app (see `protocol.rs`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct DelegationPolicy {
    /// Chain name, e.g. "monad".
    pub chain: String,
    /// ISO date, e.g. "2026-12-31".
    pub expires: String,
    pub rules: Vec<PolicyRule>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PolicyRule {
    /// Swap through one router contract, capped per day.
    Swap {
        router: String,
        #[serde(default)]
        router_label: Option<String>,
        token: String,
        /// Decimal string in whole tokens, or "unlimited".
        max_per_day: String,
    },
    /// Call one function on one contract.
    Call {
        contract: String,
        #[serde(default)]
        contract_label: Option<String>,
        function: String,
        #[serde(default)]
        max_per_day: Option<u32>,
    },
    /// Transfer to an address. Lowki rejects open-ended recipients.
    Transfer {
        to: String,
        token: String,
        max_per_day: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Attestation {
    pub verified: bool,
    /// Short build hash of the signer enclave image.
    pub build: String,
    /// Full measurement, shown in details.
    pub measurement: String,
    pub checked: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DelegationReview {
    pub request_id: String,
    pub policy: DelegationPolicy,
    pub attestation: Attestation,
    /// The app server asking (shown as the actor).
    pub requester: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Delegation {
    pub id: String,
    pub app: AppInfo,
    pub policy: DelegationPolicy,
    pub attestation: Attestation,
    pub granted: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppAccount {
    pub app: AppInfo,
    pub address: String,
    pub last_used: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WalletError {
    /// The user dismissed the passkey prompt.
    PasskeyCancelled,
    /// WebAuthn PRF extension is not available in this browser.
    PrfUnsupported,
    /// Could not reach the Lowki server or RPC.
    Network,
    /// The delegation policy breaks a Lowki rule.
    PolicyRejected { reason: String },
    InvalidCode,
    InsufficientFunds,
    InvalidAddress,
    /// Anything else, with a user-facing sentence. Shown inline.
    Other(String),
}

impl WalletError {
    /// Short, user-facing explanation for inline display.
    pub fn message(&self) -> String {
        match self {
            WalletError::PasskeyCancelled => "The passkey prompt was closed.".into(),
            WalletError::PrfUnsupported => "This browser can't use passkeys to protect your wallet.".into(),
            WalletError::Network => "Can't reach Lowki right now.".into(),
            WalletError::PolicyRejected { reason } => reason.clone(),
            WalletError::InvalidCode => "That code doesn't match. Check the latest email and try again.".into(),
            WalletError::InsufficientFunds => "Not enough balance for this amount.".into(),
            WalletError::InvalidAddress => "That doesn't look like a valid address.".into(),
            WalletError::Other(m) => m.clone(),
        }
    }
}
