//! Lowki per-app EOAs: one BIP-44 account per web origin.
//!
//! `app_index(origin) = u32_be(keccak256("lowki/app/v1" || utf8(origin))[0..4]) & 0x7fff_ffff`
//! and the key lives at `m/44'/60'/{app_index}'/0/0`.

use k256::SecretKey;
use k256::elliptic_curve::sec1::ToEncodedPoint;
use sha3::{Digest, Keccak256};
use zeroize::Zeroizing;

use crate::Error;
use crate::keystore::derive_indices;
use crate::path::HARDENED_OFFSET;

const APP_DOMAIN: &[u8] = b"lowki/app/v1";

fn keccak(parts: &[&[u8]]) -> [u8; 32] {
    let mut h = Keccak256::new();
    for p in parts {
        h.update(p);
    }
    h.finalize().into()
}

pub fn app_index(origin: &str) -> u32 {
    let h = keccak(&[APP_DOMAIN, origin.as_bytes()]);
    u32::from_be_bytes([h[0], h[1], h[2], h[3]]) & 0x7fff_ffff
}

/// `m/44'/60'/{app_index}'/0/0`
pub fn app_path(origin: &str) -> String {
    format!("m/44'/60'/{}'/0/0", app_index(origin))
}

pub fn app_private_key(seed: &[u8; 64], origin: &str) -> Zeroizing<[u8; 32]> {
    let indices = [
        HARDENED_OFFSET + 44,
        HARDENED_OFFSET + 60,
        HARDENED_OFFSET + app_index(origin),
        0,
        0,
    ];
    // Can only fail with probability ~2^-127 per level; scure would throw too.
    derive_indices(seed, &indices).expect("BIP-32 derivation produced an invalid key")
}

pub fn app_address(seed: &[u8; 64], origin: &str) -> [u8; 20] {
    address_from_private_key(&app_private_key(seed, origin))
        .expect("derived keys are valid secret keys")
}

/// Ethereum address: last 20 bytes of keccak256 of the uncompressed public key (sans 0x04).
pub fn address_from_private_key(private_key: &[u8; 32]) -> Result<[u8; 20], Error> {
    let sk = SecretKey::from_bytes(private_key.into()).map_err(|_| Error::InvalidPrivateKey)?;
    let point = sk.public_key().to_encoded_point(false);
    let h = keccak(&[&point.as_bytes()[1..]]);
    let mut out = [0u8; 20];
    out.copy_from_slice(&h[12..]);
    Ok(out)
}

/// EIP-55 mixed-case checksum encoding, `0x`-prefixed.
pub fn to_checksum_address(address: &[u8; 20]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut lower = [0u8; 40];
    for (i, b) in address.iter().enumerate() {
        lower[2 * i] = HEX[(b >> 4) as usize];
        lower[2 * i + 1] = HEX[(b & 0xf) as usize];
    }
    let h = keccak(&[&lower]);
    let mut out = String::with_capacity(42);
    out.push_str("0x");
    for (i, &c) in lower.iter().enumerate() {
        let nibble = (h[i / 2] >> (if i % 2 == 0 { 4 } else { 0 })) & 0xf;
        out.push(if c.is_ascii_alphabetic() && nibble >= 8 {
            c.to_ascii_uppercase() as char
        } else {
            c as char
        });
    }
    out
}
