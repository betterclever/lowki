//! Minimal BIP-32 (private, hardened-only) and Lowki stealth key derivation.
//!
//! Lowki spec: from the 64-byte wallet seed,
//! spending key = `m/5564'/0'/0'`, viewing key = `m/5564'/0'/1'`.

use hmac::{Hmac, Mac};
use k256::elliptic_curve::PrimeField;
use k256::{NonZeroScalar, Scalar};
use sha2::Sha512;
use zeroize::Zeroizing;

use crate::Error;
use crate::keys::SecretKey;
use crate::meta::StealthMetaAddress;

/// Hardened index offset.
pub const HARDENED: u32 = 0x8000_0000;
/// `m/5564'/0'/0'`
pub const SPENDING_PATH: [u32; 3] = [5564 | HARDENED, HARDENED, HARDENED];
/// `m/5564'/0'/1'`
pub const VIEWING_PATH: [u32; 3] = [5564 | HARDENED, HARDENED, 1 | HARDENED];

/// Extended private key (key + chain code). Zeroized on drop; no `Debug` output of secrets.
#[derive(Clone)]
pub struct ExtendedPrivKey {
    pub key: SecretKey,
    pub chain_code: Zeroizing<[u8; 32]>,
}

impl core::fmt::Debug for ExtendedPrivKey {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("ExtendedPrivKey(<redacted>)")
    }
}

fn split(i: &[u8]) -> (Zeroizing<[u8; 32]>, Zeroizing<[u8; 32]>) {
    let mut l = Zeroizing::new([0u8; 32]);
    let mut r = Zeroizing::new([0u8; 32]);
    l.copy_from_slice(&i[..32]);
    r.copy_from_slice(&i[32..]);
    (l, r)
}

fn hmac512(key: &[u8], parts: &[&[u8]]) -> Zeroizing<[u8; 64]> {
    // HMAC accepts keys of any length.
    let mut mac = <Hmac<Sha512> as Mac>::new_from_slice(key).unwrap_or_else(|_| unreachable!());
    for p in parts {
        mac.update(p);
    }
    let mut out = Zeroizing::new([0u8; 64]);
    out.copy_from_slice(&mac.finalize().into_bytes());
    out
}

/// BIP-32 master key from a seed (`HMAC-SHA512("Bitcoin seed", seed)`).
pub fn master_key(seed: &[u8]) -> Result<ExtendedPrivKey, Error> {
    let i = hmac512(b"Bitcoin seed", &[seed]);
    let (il, ir) = split(&*i);
    let key = SecretKey::from_bytes(&il).map_err(|_| Error::Bip32InvalidKey)?;
    Ok(ExtendedPrivKey {
        key,
        chain_code: ir,
    })
}

impl ExtendedPrivKey {
    /// Hardened child derivation `CKDpriv` (index must have the hardened bit set).
    pub fn derive_hardened(&self, index: u32) -> Result<ExtendedPrivKey, Error> {
        if index & HARDENED == 0 {
            return Err(Error::Bip32InvalidKey);
        }
        let k = self.key.to_bytes();
        let i = hmac512(&*self.chain_code, &[&[0u8], &*k, &index.to_be_bytes()]);
        let (il, ir) = split(&*i);
        let il: Option<Scalar> = Scalar::from_repr((*il).into()).into();
        let il = il.ok_or(Error::Bip32InvalidKey)?;
        let child = il + *self.key.scalar();
        let child = Option::<NonZeroScalar>::from(NonZeroScalar::new(child))
            .ok_or(Error::Bip32InvalidKey)?;
        Ok(ExtendedPrivKey {
            key: SecretKey::from_scalar(child),
            chain_code: ir,
        })
    }

    /// Derive along a path of (hardened) indices.
    pub fn derive_path(&self, path: &[u32]) -> Result<ExtendedPrivKey, Error> {
        let mut cur = self.clone();
        for &i in path {
            cur = cur.derive_hardened(i)?;
        }
        Ok(cur)
    }
}

/// Lowki stealth key pair.
#[derive(Clone, Debug)]
pub struct StealthKeys {
    pub spending: SecretKey,
    pub viewing: SecretKey,
}

impl StealthKeys {
    /// The stealth meta-address to publish (ENS record / ERC-6538 registry).
    pub fn meta_address(&self) -> StealthMetaAddress {
        // Public keys derived from valid secret keys are always valid compressed points.
        StealthMetaAddress::new(&self.spending.public_key(), &self.viewing.public_key())
            .unwrap_or_else(|_| unreachable!())
    }
}

/// Derive Lowki's stealth spending (`m/5564'/0'/0'`) and viewing (`m/5564'/0'/1'`) keys.
pub fn derive_stealth_keys(seed: &[u8; 64]) -> Result<StealthKeys, Error> {
    let m = master_key(seed)?;
    Ok(StealthKeys {
        spending: m.derive_path(&SPENDING_PATH)?.key,
        viewing: m.derive_path(&VIEWING_PATH)?.key,
    })
}
