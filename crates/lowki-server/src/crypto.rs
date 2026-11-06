//! Server-side key derivation and small crypto helpers.
//!
//! Every purpose gets its own subkey derived from `LOWKI_SERVER_SECRET` with HKDF-SHA256 and a
//! distinct label, so a value computed for one purpose can never be replayed for another.

use aes_gcm::{
    Aes256Gcm, Nonce,
    aead::{Aead, KeyInit},
};
use base64::{Engine, engine::general_purpose::URL_SAFE_NO_PAD};
use hkdf::Hkdf;
use hmac::{Hmac, Mac};
use rand::RngCore;
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

type HmacSha256 = Hmac<Sha256>;

pub struct Keys {
    /// Identity lookup keys: `HMAC(k, "google:" || sub)`, `HMAC(k, "email:" || email)`.
    pub identity: [u8; 32],
    /// OTP code MACs.
    pub otp: [u8; 32],
    /// Root for pairwise app user IDs.
    pub pairwise: [u8; 32],
    /// CSRF tokens bound to a session token.
    pub csrf: [u8; 32],
    /// Wraps JWT signing keys at rest.
    pub signing_kek: [u8; 32],
    /// Root for per-app webhook secrets.
    pub webhook: [u8; 32],
}

fn derive(secret: &[u8], label: &str) -> [u8; 32] {
    let hk = Hkdf::<Sha256>::new(Some(b"lowki-server/v1"), secret);
    let mut out = [0u8; 32];
    hk.expand(label.as_bytes(), &mut out)
        .expect("32 bytes is a valid HKDF length");
    out
}

impl Keys {
    pub fn derive(secret: &[u8]) -> Self {
        Keys {
            identity: derive(secret, "lowki/identity/v1"),
            otp: derive(secret, "lowki/otp/v1"),
            pairwise: derive(secret, "lowki/pairwise/v1"),
            csrf: derive(secret, "lowki/csrf/v1"),
            signing_kek: derive(secret, "lowki/jwt-kek/v1"),
            webhook: derive(secret, "lowki/webhook/v1"),
        }
    }
}

pub fn hmac(key: &[u8], parts: &[&[u8]]) -> [u8; 32] {
    let mut mac = <HmacSha256 as Mac>::new_from_slice(key).expect("hmac accepts any key length");
    for p in parts {
        // Length-prefix every part so concatenations are unambiguous.
        mac.update(&(p.len() as u32).to_be_bytes());
        mac.update(p);
    }
    mac.finalize().into_bytes().into()
}

pub fn hmac_hex(key: &[u8], parts: &[&[u8]]) -> String {
    hex::encode(hmac(key, parts))
}

pub fn sha256_hex(data: &[u8]) -> String {
    hex::encode(Sha256::digest(data))
}

pub fn random_bytes<const N: usize>() -> [u8; N] {
    let mut b = [0u8; N];
    rand::thread_rng().fill_bytes(&mut b);
    b
}

/// 256-bit random token, base64url.
pub fn random_token() -> String {
    b64(&random_bytes::<32>())
}

pub fn b64(data: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(data)
}

pub fn unb64(data: &str) -> Option<Vec<u8>> {
    URL_SAFE_NO_PAD.decode(data.trim_end_matches('=')).ok()
}

pub fn ct_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && bool::from(a.ct_eq(b))
}

/// Constant-time comparison of two strings of possibly different lengths (compares digests).
pub fn ct_eq_str(a: &str, b: &str) -> bool {
    let da = Sha256::digest(a.as_bytes());
    let db = Sha256::digest(b.as_bytes());
    bool::from(da.ct_eq(&db))
}

pub fn seal(key: &[u8; 32], plaintext: &[u8], aad: &[u8]) -> Vec<u8> {
    let cipher = Aes256Gcm::new_from_slice(key).expect("32-byte key");
    let nonce = random_bytes::<12>();
    let ct = cipher
        .encrypt(&Nonce::from(nonce), aes_gcm::aead::Payload { msg: plaintext, aad })
        .expect("aes-gcm encrypt");
    let mut out = Vec::with_capacity(13 + ct.len());
    out.push(1);
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&ct);
    out
}

pub fn open(key: &[u8; 32], sealed: &[u8], aad: &[u8]) -> Option<Vec<u8>> {
    if sealed.len() < 13 + 16 || sealed[0] != 1 {
        return None;
    }
    let cipher = Aes256Gcm::new_from_slice(key).ok()?;
    let nonce: [u8; 12] = sealed[1..13].try_into().ok()?;
    cipher
        .decrypt(
            &Nonce::from(nonce),
            aes_gcm::aead::Payload {
                msg: &sealed[13..],
                aad,
            },
        )
        .ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hmac_parts_are_unambiguous() {
        let k = [7u8; 32];
        assert_ne!(hmac(&k, &[b"ab", b"c"]), hmac(&k, &[b"a", b"bc"]));
    }

    #[test]
    fn seal_roundtrip() {
        let k = [9u8; 32];
        let s = seal(&k, b"hello", b"aad");
        assert_eq!(open(&k, &s, b"aad").unwrap(), b"hello");
        assert!(open(&k, &s, b"other").is_none());
    }
}
