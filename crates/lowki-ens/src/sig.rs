//! secp256k1 ECDSA over 32-byte digests: RFC 6979 deterministic, low-s, `r || s || v` with
//! `v = 27 + y_parity` (byte-equal to viem `sign` / `signMessage` / `signTypedData`), and
//! address recovery. Same construction as `lowki-keys::sign_digest`.

use alloc::vec::Vec;

use k256::ecdsa::signature::hazmat::PrehashSigner;
use k256::ecdsa::{RecoveryId, Signature, SigningKey, VerifyingKey};
use k256::elliptic_curve::ops::Reduce;
use k256::{FieldBytes, Scalar, U256 as KU256};

use crate::{Address, Error, SecretKey, keccak256};

/// Signs `digest` with `key`: 65 bytes `r || s || v`, `v` in {27, 28}, low-s.
pub fn sign_digest(key: &SecretKey, digest: &[u8; 32]) -> Result<[u8; 65], Error> {
    let kb = key.to_bytes();
    let sk = SigningKey::from_bytes((&*kb).into()).map_err(|_| Error::InvalidPrivateKey)?;
    // RFC 6979 / @noble/curves feed `bits2int(h) mod n` into the nonce DRBG; the `ecdsa`
    // crate passes the unreduced digest, which differs for digests >= n.
    let z: FieldBytes = <Scalar as Reduce<KU256>>::reduce_bytes(digest.into()).to_bytes();
    let (sig, recid): (Signature, RecoveryId) =
        sk.sign_prehash(&z).map_err(|_| Error::InvalidPrivateKey)?;
    let (sig, recid) = match sig.normalize_s() {
        Some(n) => (n, RecoveryId::new(!recid.is_y_odd(), recid.is_x_reduced())),
        None => (sig, recid),
    };
    let mut out = [0u8; 65];
    out[..64].copy_from_slice(&sig.to_bytes());
    out[64] = 27 + u8::from(recid.is_y_odd());
    Ok(out)
}

/// Recovers the signer address of a 65-byte `r || s || v` signature over `digest`.
/// `v` may be 0/1 or 27/28. High-s signatures are rejected (viem accepts them).
pub fn recover_digest(digest: &[u8; 32], sig: &[u8]) -> Result<Address, Error> {
    if sig.len() != 65 {
        return Err(Error::InvalidSignature);
    }
    let v = match sig[64] {
        0 | 27 => 0u8,
        1 | 28 => 1u8,
        _ => return Err(Error::InvalidSignature),
    };
    let s = Signature::from_slice(&sig[..64]).map_err(|_| Error::InvalidSignature)?;
    if s.normalize_s().is_some() {
        return Err(Error::InvalidSignature);
    }
    let recid = RecoveryId::from_byte(v).ok_or(Error::InvalidSignature)?;
    let z: FieldBytes = <Scalar as Reduce<KU256>>::reduce_bytes(digest.into()).to_bytes();
    let vk =
        VerifyingKey::recover_from_prehash(&z, &s, recid).map_err(|_| Error::InvalidSignature)?;
    let ep = vk.to_encoded_point(false);
    let h = keccak256(&ep.as_bytes()[1..]);
    let mut a = [0u8; 20];
    a.copy_from_slice(&h[12..]);
    Ok(a)
}

/// EIP-191 `personal_sign` digest: `keccak256("\x19Ethereum Signed Message:\n" || len || msg)`
/// (viem `hashMessage`).
pub fn personal_message_hash(msg: &[u8]) -> [u8; 32] {
    let mut buf: Vec<u8> = Vec::with_capacity(msg.len() + 40);
    buf.extend_from_slice(b"\x19Ethereum Signed Message:\n");
    buf.extend_from_slice(alloc::format!("{}", msg.len()).as_bytes());
    buf.extend_from_slice(msg);
    keccak256(&buf)
}

/// EIP-191 `personal_sign` (viem `signMessage({ message })` for a string message).
pub fn sign_personal(key: &SecretKey, msg: &[u8]) -> Result<[u8; 65], Error> {
    sign_digest(key, &personal_message_hash(msg))
}

/// Recovers the `personal_sign` signer (viem `recoverMessageAddress`, low-s only).
pub fn recover_personal(msg: &[u8], sig: &[u8]) -> Result<Address, Error> {
    recover_digest(&personal_message_hash(msg), sig)
}

/// EIP-712 digest `keccak256(0x19 0x01 || domainSeparator || structHash)`.
pub fn eip712_digest(domain_separator: &[u8; 32], struct_hash: &[u8; 32]) -> [u8; 32] {
    let mut buf = [0u8; 66];
    buf[0] = 0x19;
    buf[1] = 0x01;
    buf[2..34].copy_from_slice(domain_separator);
    buf[34..].copy_from_slice(struct_hash);
    keccak256(&buf)
}
