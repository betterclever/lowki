//! Thin wasm-bindgen wrappers over `lowki_keys::{derive_kek, wrap_entropy, unwrap_entropy}`.
//!
//! All randomness (entropy, nonce) comes from the caller (`crypto.getRandomValues`), and
//! the PRF output comes from `getClientExtensionResults().prf.results.first`.

use lowki_keys::Error;
use wasm_bindgen::prelude::*;

fn arr<const N: usize>(name: &str, b: &[u8]) -> Result<[u8; N], JsError> {
    b.try_into()
        .map_err(|_| JsError::new(&format!("{name} must be {N} bytes, got {}", b.len())))
}

fn err(e: Error) -> JsError {
    JsError::new(&format!("{e:?}: {e}"))
}

/// HKDF-SHA256(prf, info = "lowki/kek/v1"). Exposed only so the spike can compare it with
/// WebCrypto; a real build should never hand the KEK to JS.
#[wasm_bindgen]
pub fn derive_kek(prf: &[u8]) -> Result<Vec<u8>, JsError> {
    let prf = arr::<32>("prf", prf)?;
    Ok(lowki_keys::derive_kek(&prf).to_vec())
}

/// Returns the 61-byte blob `0x01 || nonce || ct+tag`.
#[wasm_bindgen]
pub fn wrap_entropy(prf: &[u8], entropy: &[u8], nonce: &[u8]) -> Result<Vec<u8>, JsError> {
    let prf = arr::<32>("prf", prf)?;
    let entropy = arr::<32>("entropy", entropy)?;
    let nonce = arr::<12>("nonce", nonce)?;
    Ok(lowki_keys::wrap_entropy(&prf, &entropy, &nonce).to_vec())
}

/// Returns the 32-byte entropy, or throws `Decrypt` / `BadLength` / `BadVersion`.
#[wasm_bindgen]
pub fn unwrap_entropy(prf: &[u8], blob: &[u8]) -> Result<Vec<u8>, JsError> {
    let prf = arr::<32>("prf", prf)?;
    lowki_keys::unwrap_entropy(&prf, blob)
        .map(|e| e.to_vec())
        .map_err(err)
}
