//! Kohaku `MnemonicKeystore.deriveAt` and `scopeKeystore` equivalents.

use core::fmt;

use hmac::{Hmac, Mac};
use k256::elliptic_curve::PrimeField;
use k256::elliptic_curve::sec1::ToEncodedPoint;
use k256::{Scalar, SecretKey};
use sha2::Sha512;
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

use crate::path::{HARDENED_OFFSET, parse_derivation_path};
use crate::{Error, mnemonic};

type HmacSha512 = Hmac<Sha512>;

/// RAILGUN registered coin type (SLIP-0044).
const RAILGUN_COIN_TYPE: u32 = 1984;
const MAX_DEPTH: usize = 255;

fn hmac512(key: &[u8], parts: &[&[u8]]) -> Zeroizing<[u8; 64]> {
    let mut mac = HmacSha512::new_from_slice(key).expect("HMAC accepts any key length");
    for p in parts {
        mac.update(p);
    }
    let mut out = Zeroizing::new([0u8; 64]);
    out.copy_from_slice(&mac.finalize().into_bytes());
    out
}

fn scalar_from(bytes: &[u8]) -> Option<Scalar> {
    let mut repr = k256::FieldBytes::default();
    repr.copy_from_slice(bytes);
    let s = Option::<Scalar>::from(Scalar::from_repr(repr));
    repr.zeroize();
    s
}

/// `@scure/bip32` `HDKey.fromMasterSeed(seed)` followed by `deriveChild` for each index.
fn bip32_derive(seed: &[u8], indices: &[u32]) -> Result<Zeroizing<[u8; 32]>, Error> {
    let i = hmac512(b"Bitcoin seed", &[seed]);
    let mut key = Zeroizing::new([0u8; 32]);
    let mut chain = Zeroizing::new([0u8; 32]);
    key.copy_from_slice(&i[..32]);
    chain.copy_from_slice(&i[32..]);
    // scure: master key must be a valid secret key.
    let mut k = scalar_from(&key[..])
        .filter(|s| !bool::from(s.is_zero()))
        .ok_or(Error::InvalidDerivedKey)?;

    for &index in indices {
        let mut ser = [0u8; 4];
        ser.copy_from_slice(&index.to_be_bytes());
        let i = if index >= HARDENED_OFFSET {
            hmac512(&chain[..], &[&[0u8], &key[..], &ser])
        } else {
            let sk = SecretKey::from_bytes(&k.to_bytes()).map_err(|_| Error::InvalidDerivedKey)?;
            let pubkey = sk.public_key().to_encoded_point(true);
            hmac512(&chain[..], &[pubkey.as_bytes(), &ser])
        };
        // scure throws if IL >= n or the child key is zero (instead of skipping the index).
        let tweak = scalar_from(&i[..32]).ok_or(Error::InvalidDerivedKey)?;
        let child = tweak + k;
        if bool::from(child.is_zero()) {
            return Err(Error::InvalidDerivedKey);
        }
        k.zeroize();
        k = child;
        key.copy_from_slice(&k.to_bytes());
        chain.copy_from_slice(&i[32..]);
    }
    k.zeroize();
    Ok(key)
}

/// RAILGUN engine "babyjubjub seed" hash chain (SLIP-0010 style, hardened only), as in
/// Kohaku's `deriveRailgunEngineKey`.
fn railgun_derive(seed: &[u8], indices: &[u32]) -> Result<Zeroizing<[u8; 32]>, Error> {
    let mut i = hmac512(b"babyjubjub seed", &[seed]);
    for &index in indices {
        if index < HARDENED_OFFSET {
            return Err(Error::RailgunNonHardened);
        }
        i = hmac512(&i[32..], &[&[0u8], &i[..32], &index.to_be_bytes()]);
    }
    let mut out = Zeroizing::new([0u8; 32]);
    out.copy_from_slice(&i[..32]);
    Ok(out)
}

/// `MnemonicKeystore.deriveAt` on already-parsed indices.
pub fn derive_indices(seed: &[u8; 64], indices: &[u32]) -> Result<Zeroizing<[u8; 32]>, Error> {
    if indices.is_empty() || indices.len() > MAX_DEPTH {
        return Err(Error::InvalidPath);
    }
    if indices.len() >= 2 && indices[1] == RAILGUN_COIN_TYPE + HARDENED_OFFSET {
        railgun_derive(seed, indices)
    } else {
        bip32_derive(seed, indices)
    }
}

/// `MnemonicKeystore.deriveAt(path)`: 32-byte private key at `path` for a BIP-39 seed.
pub fn derive_at(seed: &[u8; 64], path: &str) -> Result<Zeroizing<[u8; 32]>, Error> {
    derive_indices(seed, &parse_derivation_path(path)?)
}

/// Holds a 64-byte BIP-39 seed (zeroized on drop) and derives keys from it.
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct Keystore {
    seed: [u8; 64],
}

impl fmt::Debug for Keystore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Keystore(..)")
    }
}

impl Keystore {
    pub fn from_seed(seed: &[u8; 64]) -> Self {
        Self { seed: *seed }
    }
    pub fn from_entropy(entropy: &[u8; 32]) -> Self {
        Self::from_seed(&mnemonic::entropy_to_seed(entropy))
    }
    /// Validates the phrase like `new MnemonicKeystore(mnemonic)`.
    pub fn from_mnemonic(phrase: &str) -> Result<Self, Error> {
        Ok(Self::from_seed(&*mnemonic::mnemonic_to_seed(phrase)?))
    }
    pub fn derive_at(&self, path: &str) -> Result<Zeroizing<[u8; 32]>, Error> {
        derive_at(&self.seed, path)
    }
    /// `scopeKeystore(this, { prefixes, paths })`.
    pub fn scope(&self, prefixes: &[&str], paths: &[&str]) -> Result<ScopedKeystore, Error> {
        ScopedKeystore::new(self.clone(), prefixes, paths)
    }
}

/// `scopeKeystore`: derives only strictly below one of `prefixes` or exactly at one of
/// `paths` (compared after parsing, so `'` and `h` are interchangeable; unparsable entries
/// in `paths` never match).
pub struct ScopedKeystore {
    inner: Keystore,
    prefixes: Vec<Vec<u32>>,
    paths: Vec<String>,
}

impl fmt::Debug for ScopedKeystore {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ScopedKeystore")
            .field("prefixes", &self.prefixes)
            .field("paths", &self.paths)
            .finish_non_exhaustive()
    }
}

impl ScopedKeystore {
    /// Fails with [`Error::InvalidScope`] if any prefix does not parse (Kohaku throws).
    pub fn new(inner: Keystore, prefixes: &[&str], paths: &[&str]) -> Result<Self, Error> {
        let prefixes = prefixes
            .iter()
            .map(|p| parse_derivation_path(p).map_err(|_| Error::InvalidScope))
            .collect::<Result<_, _>>()?;
        Ok(Self {
            inner,
            prefixes,
            paths: paths.iter().map(|p| (*p).to_owned()).collect(),
        })
    }

    /// Adds a single allowed path (Kohaku's `paths` set may grow after scoping).
    pub fn allow_path(&mut self, path: &str) {
        self.paths.push(path.to_owned());
    }

    /// `Err(InvalidPath)` if `path` does not parse, otherwise whether it is in scope.
    pub fn is_allowed(&self, path: &str) -> Result<bool, Error> {
        let idx = parse_derivation_path(path)?;
        Ok(self
            .prefixes
            .iter()
            .any(|p| idx.len() > p.len() && idx.starts_with(p))
            || self
                .paths
                .iter()
                .any(|p| parse_derivation_path(p).is_ok_and(|q| q == idx)))
    }

    pub fn derive_at(&self, path: &str) -> Result<Zeroizing<[u8; 32]>, Error> {
        if !self.is_allowed(path)? {
            return Err(Error::PathNotAllowed);
        }
        self.inner.derive_at(path)
    }
}
