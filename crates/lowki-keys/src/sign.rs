//! RFC-6979 deterministic secp256k1 signing of a 32-byte digest, low-s normalised.

use k256::ecdsa::SigningKey;
use k256::ecdsa::signature::hazmat::PrehashSigner;
use k256::elliptic_curve::ops::Reduce;
use k256::{FieldBytes, Scalar, U256};

use crate::Error;

/// `v` is `27 + y_parity` (Ethereum `personal_sign` / legacy convention).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Signature {
    pub r: [u8; 32],
    pub s: [u8; 32],
    pub v: u8,
}

impl Signature {
    pub fn y_parity(&self) -> u8 {
        self.v - 27
    }
    /// `r || s || v` (65 bytes).
    pub fn to_bytes(&self) -> [u8; 65] {
        let mut out = [0u8; 65];
        out[..32].copy_from_slice(&self.r);
        out[32..64].copy_from_slice(&self.s);
        out[64] = self.v;
        out
    }
}

/// Signs `digest` (already hashed, e.g. keccak256) with `private_key`. No extra entropy,
/// so the output equals viem `sign({ hash, privateKey })` / `@noble/curves` with `lowS`.
pub fn sign_digest(private_key: &[u8; 32], digest: &[u8; 32]) -> Result<Signature, Error> {
    let sk = SigningKey::from_bytes(private_key.into()).map_err(|_| Error::InvalidPrivateKey)?;
    // RFC 6979 (and @noble/curves) feed `bits2int(h) mod n` into the nonce DRBG; the
    // `ecdsa` crate passes the unreduced digest, which differs for digests >= n. ECDSA
    // itself only uses `z mod n`, so reducing first changes nothing else.
    let z: FieldBytes = <Scalar as Reduce<U256>>::reduce_bytes(digest.into()).to_bytes();
    let (sig, recid): (k256::ecdsa::Signature, k256::ecdsa::RecoveryId) =
        sk.sign_prehash(&z).map_err(|_| Error::InvalidPrivateKey)?;
    // k256 already normalises to low-s and adjusts the recovery id; keep this defensive.
    let (sig, recid) = match sig.normalize_s() {
        Some(n) => (
            n,
            k256::ecdsa::RecoveryId::new(!recid.is_y_odd(), recid.is_x_reduced()),
        ),
        None => (sig, recid),
    };
    let (r, s) = sig.split_bytes();
    Ok(Signature {
        r: r.into(),
        s: s.into(),
        v: 27 + u8::from(recid.is_y_odd()),
    })
}
