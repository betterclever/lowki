//! Worker errors. A `WorkerError` crosses to JS as `{type: "error", code, message}`.
//! Messages are fixed strings or contain only public data (field names, lengths, paths
//! the caller supplied); they never contain key material, shares, PRF outputs or seeds.

use serde::Serialize;
use std::fmt;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum ErrorCode {
    /// No unlocked session (never unlocked, locked, or auto-locked).
    Locked,
    /// A wallet operation that needs a locked worker was called while unlocked.
    AlreadyUnlocked,
    /// The request could not be parsed or a field is malformed.
    InvalidRequest,
    /// The PRF output did not open the wrapped blob (wrong passkey or tampered blob).
    UnwrapFailed,
    /// Shares did not combine (too few, duplicates, mismatched lengths, wrong secret length).
    InvalidShares,
    /// A Kohaku keystore path outside the allowed scope.
    PathNotAllowed,
    /// Circuits not loaded.
    CircuitsNotLoaded,
    /// Witness/proving/verification failure.
    Prover,
    /// Signing or encoding failure.
    Signing,
    /// HPKE sealing failure.
    Seal,
    /// Unexpected internal error.
    Internal,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct WorkerError {
    pub code: ErrorCode,
    pub message: String,
}

impl WorkerError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self { code, message: message.into() }
    }
    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::InvalidRequest, message)
    }
    pub fn locked() -> Self {
        Self::new(ErrorCode::Locked, "wallet is locked")
    }
}

impl fmt::Display for WorkerError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}: {}", self.code, self.message)
    }
}

impl std::error::Error for WorkerError {}

pub type Result<T> = std::result::Result<T, WorkerError>;

// The underlying crates' error Display impls carry no secret data (checked: lowki-keys,
// lowki-shares, lowki-pp-core, lowki-aa and lowki-groth16 errors are enum names, lengths,
// indices or circuit messages). They are still mapped to a category code here.
impl From<lowki_keys::Error> for WorkerError {
    fn from(e: lowki_keys::Error) -> Self {
        use lowki_keys::Error as E;
        let code = match e {
            E::PathNotAllowed | E::InvalidScope => ErrorCode::PathNotAllowed,
            E::InvalidPath | E::InvalidMnemonic | E::RailgunNonHardened => ErrorCode::InvalidRequest,
            E::BadLength | E::BadVersion | E::Decrypt => ErrorCode::UnwrapFailed,
            _ => ErrorCode::Internal,
        };
        Self::new(code, format!("keys: {e:?}"))
    }
}

impl From<lowki_shares::Error> for WorkerError {
    fn from(e: lowki_shares::Error) -> Self {
        Self::new(ErrorCode::InvalidShares, format!("shares: {e}"))
    }
}

impl From<lowki_pp_core::Error> for WorkerError {
    fn from(e: lowki_pp_core::Error) -> Self {
        Self::new(ErrorCode::Internal, format!("privacy pools: {e}"))
    }
}

impl From<lowki_aa::Error> for WorkerError {
    fn from(e: lowki_aa::Error) -> Self {
        Self::new(ErrorCode::Signing, format!("aa: {e}"))
    }
}

impl From<lowki_groth16::Error> for WorkerError {
    fn from(e: lowki_groth16::Error) -> Self {
        Self::new(ErrorCode::Prover, format!("prover: {e}"))
    }
}

impl From<lowki_stealth::Error> for WorkerError {
    fn from(e: lowki_stealth::Error) -> Self {
        Self::new(ErrorCode::Internal, format!("stealth: {e:?}"))
    }
}
