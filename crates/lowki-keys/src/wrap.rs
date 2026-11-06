//! Wrapping the 32-byte BIP-39 entropy with a passkey PRF output.
//!
//! ```text
//! kek  = HKDF-SHA256(ikm = prf, salt = "", info = "lowki/kek/v1", L = 32)
//! blob = 0x01 || nonce(12) || AES-256-GCM(kek, nonce, entropy, aad = "lowki/seed/v1")   // 61 bytes
//! ```
//!
//! Byte-compatible with WebCrypto `HKDF` + `AES-GCM` (128-bit tag).

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use hkdf::Hkdf;
use sha2::Sha256;
use zeroize::{Zeroize, Zeroizing};

use crate::Error;

const VERSION: u8 = 0x01;
const KEK_INFO: &[u8] = b"lowki/kek/v1";
const AAD: &[u8] = b"lowki/seed/v1";
const NONCE_LEN: usize = 12;
const TAG_LEN: usize = 16;
/// `1 + 12 + 32 + 16`
pub const WRAPPED_LEN: usize = 1 + NONCE_LEN + 32 + TAG_LEN;

/// The key-encryption key derived from the PRF output.
pub fn derive_kek(prf: &[u8; 32]) -> Zeroizing<[u8; 32]> {
    let mut kek = Zeroizing::new([0u8; 32]);
    Hkdf::<Sha256>::new(Some(&[]), prf)
        .expand(KEK_INFO, &mut kek[..])
        .expect("32 bytes is a valid HKDF-SHA256 output length");
    kek
}

/// Wraps `entropy` under `prf`. `nonce` must be fresh random bytes for every wrap
/// (callers on wasm get them from `crypto.getRandomValues`).
pub fn wrap_entropy(prf: &[u8; 32], entropy: &[u8; 32], nonce: &[u8; 12]) -> [u8; WRAPPED_LEN] {
    let kek = derive_kek(prf);
    let cipher = Aes256Gcm::new((&*kek).into());
    let ct = cipher
        .encrypt(
            Nonce::from_slice(nonce),
            Payload {
                msg: entropy,
                aad: AAD,
            },
        )
        .expect("AES-GCM encryption of 32 bytes cannot fail");
    let mut out = [0u8; WRAPPED_LEN];
    out[0] = VERSION;
    out[1..1 + NONCE_LEN].copy_from_slice(nonce);
    out[1 + NONCE_LEN..].copy_from_slice(&ct);
    out
}

/// Unwraps a blob produced by [`wrap_entropy`] (or by the WebCrypto implementation).
/// Checks length first ([`Error::BadLength`]), then the version byte
/// ([`Error::BadVersion`]), then the GCM tag ([`Error::Decrypt`]).
pub fn unwrap_entropy(prf: &[u8; 32], blob: &[u8]) -> Result<Zeroizing<[u8; 32]>, Error> {
    if blob.len() != WRAPPED_LEN {
        return Err(Error::BadLength);
    }
    if blob[0] != VERSION {
        return Err(Error::BadVersion);
    }
    let kek = derive_kek(prf);
    let cipher = Aes256Gcm::new((&*kek).into());
    let mut pt = cipher
        .decrypt(
            Nonce::from_slice(&blob[1..1 + NONCE_LEN]),
            Payload {
                msg: &blob[1 + NONCE_LEN..],
                aad: AAD,
            },
        )
        .map_err(|_| Error::Decrypt)?;
    let mut out = Zeroizing::new([0u8; 32]);
    out.copy_from_slice(&pt);
    pt.zeroize();
    Ok(out)
}
