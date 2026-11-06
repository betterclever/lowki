use core::fmt;

/// Errors. `Display` strings that mirror a Kohaku `Error` message reproduce it exactly.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// The 32 bytes are not a valid secp256k1 scalar (zero or >= n).
    InvalidPrivateKey,
    /// ECDSA signing failed (practically unreachable).
    Signing,
    /// Kohaku passes the 7702 authorization nonce through JS `Number(...)`, which viem
    /// rejects above `Number.MAX_SAFE_INTEGER`.
    UnsafeAuthorizationNonce,
    /// `toWithdrawProof`: the prove output does not carry exactly 8 public signals, or a
    /// coordinate is missing / not an integer.
    InvalidProof,
    /// `assertGasLimitsWithinBounds`.
    GasLimitExceedsCeiling {
        field: &'static str,
        value: u128,
        cap: u128,
    },
    /// `assertGasPriceWithinBounds`: priority fee above max fee.
    InvalidGasPrices,
    /// `assertGasPriceWithinBounds`: price above the host-RPC derived caps.
    GasPriceExceedsBounds { fee_cap: String, tip_cap: String },
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::InvalidPrivateKey => f.write_str("Invalid secp256k1 private key"),
            Error::Signing => f.write_str("ECDSA signing failed"),
            Error::UnsafeAuthorizationNonce => f.write_str(
                "Authorization nonce is not in safe integer range (0 to 9007199254740991)",
            ),
            Error::InvalidProof => f.write_str("Invalid proof"),
            Error::GasLimitExceedsCeiling { field, value, cap } => {
                write!(f, "Bundler {field} {value} exceeds the gas ceiling {cap}")
            }
            Error::InvalidGasPrices => f.write_str("Bundler returned invalid gas prices"),
            Error::GasPriceExceedsBounds { fee_cap, tip_cap } => write!(
                f,
                "Bundler gas price exceeds the host RPC bounds (maxFeePerGas={fee_cap}, maxPriorityFeePerGas={tip_cap})"
            ),
        }
    }
}

impl std::error::Error for Error {}
