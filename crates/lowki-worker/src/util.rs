//! Parsing helpers for public request fields.

use alloy_primitives::{Address, B256, Bytes, U256};
use lowki_pp_core::Fr;

use crate::error::{Result, WorkerError};

pub fn address(s: &str, field: &str) -> Result<Address> {
    let h = s.strip_prefix("0x").or_else(|| s.strip_prefix("0X")).unwrap_or(s);
    if h.len() != 40 {
        return Err(WorkerError::invalid(format!("{field}: expected a 20-byte hex address")));
    }
    let mut out = [0u8; 20];
    hex::decode_to_slice(h, &mut out).map_err(|_| WorkerError::invalid(format!("{field}: bad hex")))?;
    Ok(Address::from(out))
}

pub fn bytes(s: &str, field: &str) -> Result<Bytes> {
    let h = s.strip_prefix("0x").unwrap_or(s);
    hex::decode(h).map(Bytes::from).map_err(|_| WorkerError::invalid(format!("{field}: bad hex")))
}

pub fn b256(s: &str, field: &str) -> Result<B256> {
    let b = bytes(s, field)?;
    if b.len() != 32 {
        return Err(WorkerError::invalid(format!("{field}: expected 32 bytes")));
    }
    Ok(B256::from_slice(&b))
}

/// Decimal or 0x-hex uint256 (JS `BigInt()` semantics).
pub fn u256(s: &str, field: &str) -> Result<U256> {
    let t = s.trim();
    let r = if let Some(h) = t.strip_prefix("0x").or_else(|| t.strip_prefix("0X")) {
        if h.is_empty() { Ok(U256::ZERO) } else { U256::from_str_radix(h, 16) }
    } else {
        U256::from_str_radix(t, 10)
    };
    r.map_err(|_| WorkerError::invalid(format!("{field}: not a uint256")))
}

pub fn u64q(s: &str, field: &str) -> Result<u64> {
    let v = u256(s, field)?;
    if v > U256::from(u64::MAX) {
        return Err(WorkerError::invalid(format!("{field}: exceeds u64")));
    }
    Ok(v.to::<u64>())
}

pub fn u128q(s: &str, field: &str) -> Result<u128> {
    let v = u256(s, field)?;
    if v > U256::from(u128::MAX) {
        return Err(WorkerError::invalid(format!("{field}: exceeds u128")));
    }
    Ok(v.to::<u128>())
}

/// A field element from a decimal/hex string; must be < p (canonical).
pub fn fr(s: &str, field: &str) -> Result<Fr> {
    let v = u256(s, field)?;
    lowki_pp_core::fr_from_be_bytes(&v.to_be_bytes::<32>())
        .map_err(|_| WorkerError::invalid(format!("{field}: not a canonical BN254 field element")))
}

pub fn fr_to_dec(x: &Fr) -> String {
    U256::from_be_bytes(lowki_pp_core::fr_to_be_bytes(x)).to_string()
}

pub fn fr_to_hex(x: &Fr) -> String {
    format!("0x{}", hex::encode(lowki_pp_core::fr_to_be_bytes(x)))
}

pub fn hex0x(b: &[u8]) -> String {
    format!("0x{}", hex::encode(b))
}

/// Validates a serialized web origin as `event.origin` gives it: `scheme://host[:port]`,
/// lowercase, no path/query/fragment/trailing slash. The per-app key and pairwise ID are
/// derived from these exact bytes, so one origin must have exactly one spelling.
pub fn origin(s: &str) -> Result<&str> {
    let bad = || WorkerError::invalid("origin must be a serialized origin like https://app.example");
    let (scheme, rest) = s.split_once("://").ok_or_else(bad)?;
    if !(scheme == "https" || scheme == "http") || rest.is_empty() {
        return Err(bad());
    }
    if rest.contains(['/', '?', '#', '@']) || rest.chars().any(|c| c.is_ascii_uppercase() || c.is_whitespace()) {
        return Err(bad());
    }
    if rest.ends_with(":443") && scheme == "https" || rest.ends_with(":80") && scheme == "http" {
        return Err(WorkerError::invalid("origin must omit the default port"));
    }
    Ok(s)
}
