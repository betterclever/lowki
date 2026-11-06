//! Ephemeral secp256k1 signer (the EIP-7702-delegated userOp sender).

use alloy_primitives::{Address, B256, keccak256};
use k256::ecdsa::{RecoveryId, Signature, SigningKey, VerifyingKey};
use zeroize::Zeroizing;

use crate::Error;

/// A secp256k1 private key. The inner `SigningKey` is zeroized on drop; there is no
/// `Debug`/`Display` and no way to read the key back out.
pub struct EphemeralSigner {
    key: SigningKey,
    address: Address,
}

/// A recoverable ECDSA signature (low-s, RFC 6979 deterministic, as viem/noble).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RecoverableSignature {
    pub r: B256,
    pub s: B256,
    /// 0 or 1.
    pub y_parity: u8,
}

impl RecoverableSignature {
    /// 65-byte `r || s || v` with `v = 27 + yParity` (viem `serializeSignature`).
    pub fn to_bytes65(&self) -> [u8; 65] {
        let mut out = [0u8; 65];
        out[..32].copy_from_slice(self.r.as_slice());
        out[32..64].copy_from_slice(self.s.as_slice());
        out[64] = 27 + self.y_parity;
        out
    }

    /// Recover the signer address of a 32-byte prehash.
    pub fn recover(&self, prehash: &B256) -> Option<Address> {
        let mut rs = [0u8; 64];
        rs[..32].copy_from_slice(self.r.as_slice());
        rs[32..].copy_from_slice(self.s.as_slice());
        let sig = Signature::from_slice(&rs).ok()?;
        let recid = RecoveryId::from_byte(self.y_parity)?;
        let vk = VerifyingKey::recover_from_prehash(prehash.as_slice(), &sig, recid).ok()?;
        Some(address_of(&vk))
    }
}

fn address_of(vk: &VerifyingKey) -> Address {
    let point = vk.to_encoded_point(false);
    let hash = keccak256(&point.as_bytes()[1..]);
    Address::from_slice(&hash[12..])
}

// `SigningKey` zeroizes its scalar on drop; the address is public. Checked at compile time.
const _: fn() = || {
    fn assert_zeroize_on_drop<T: zeroize::ZeroizeOnDrop>() {}
    assert_zeroize_on_drop::<SigningKey>();
};
impl zeroize::ZeroizeOnDrop for EphemeralSigner {}

impl EphemeralSigner {
    /// Build from a 32-byte big-endian private key. The caller keeps ownership of (and
    /// should zeroize) its own copy.
    pub fn from_bytes(secret: &[u8; 32]) -> Result<Self, Error> {
        let copy = Zeroizing::new(*secret);
        let key = SigningKey::from_bytes((&*copy).into()).map_err(|_| Error::InvalidPrivateKey)?;
        let address = address_of(key.verifying_key());
        Ok(Self { key, address })
    }

    /// The EOA address (also the 7702 userOp `sender`).
    pub fn address(&self) -> Address {
        self.address
    }

    /// Sign a 32-byte prehash (no EIP-191 prefix).
    pub fn sign_hash(&self, hash: &B256) -> Result<RecoverableSignature, Error> {
        let (sig, recid) = self
            .key
            .sign_prehash_recoverable(hash.as_slice())
            .map_err(|_| Error::Signing)?;
        // k256 already returns low-s; normalise defensively (flipping parity).
        let (sig, recid) = match sig.normalize_s() {
            Some(n) => (n, RecoveryId::new(!recid.is_y_odd(), recid.is_x_reduced())),
            None => (sig, recid),
        };
        let (r, s) = sig.split_bytes();
        Ok(RecoverableSignature {
            r: B256::from_slice(&r),
            s: B256::from_slice(&s),
            y_parity: recid.is_y_odd() as u8,
        })
    }
}
