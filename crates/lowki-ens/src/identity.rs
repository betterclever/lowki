//! The ENS name-owner identity key.
//!
//! Path: `m/137'/0'/{index}'` (hardened BIP-32 from the 64-byte wallet seed). 137 is the
//! EIP-137 (ENS) number. It is deliberately **not** a BIP-44 path, so the address that owns a
//! public ENS name never coincides with a Lowki per-app account (`m/44'/60'/{app}'/0/0`) or
//! with the stealth keys (`m/5564'/0'/{0,1}'`): owning a name must not link it to app activity
//! or to stealth payments.

use alloc::string::String;
use zeroize::Zeroizing;

use crate::bip32::{HARDENED, master_key};
use crate::{Address, Error, SecretKey};

/// `137'` (EIP-137 / ENS).
pub const IDENTITY_PURPOSE: u32 = 137;

/// `m/137'/0'/{index}'`
pub fn identity_path(index: u32) -> String {
    alloc::format!("m/{IDENTITY_PURPOSE}'/0'/{index}'")
}

/// A name-owner key. Zeroized on drop; `Debug` prints only the address.
#[derive(Clone)]
pub struct IdentityKey {
    index: u32,
    key: SecretKey,
}

impl core::fmt::Debug for IdentityKey {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.debug_struct("IdentityKey")
            .field("index", &self.index)
            .field("address", &crate::to_checksum_address(&self.address()))
            .finish_non_exhaustive()
    }
}

impl IdentityKey {
    /// Derives `m/137'/0'/{index}'` from a 64-byte BIP-39 seed. `index` must be < 2^31.
    pub fn from_seed(seed: &[u8; 64], index: u32) -> Result<Self, Error> {
        if index >= HARDENED {
            return Err(Error::InvalidDerivation);
        }
        let m = master_key(seed)?;
        let k = m.derive_path(&[IDENTITY_PURPOSE | HARDENED, HARDENED, index | HARDENED])?;
        Ok(IdentityKey { index, key: k.key })
    }

    /// Derives through a keystore callback, e.g. `|p| keystore.derive_at(p)` (the worker's
    /// `Keystore::derive_at`), which receives [`identity_path`]`(index)`.
    pub fn from_derive_at<E, F>(index: u32, derive_at: F) -> Result<Self, E>
    where
        F: FnOnce(&str) -> Result<Zeroizing<[u8; 32]>, E>,
        E: From<Error>,
    {
        if index >= HARDENED {
            return Err(Error::InvalidDerivation.into());
        }
        let bytes = derive_at(&identity_path(index))?;
        let key = SecretKey::from_bytes(&bytes).map_err(|_| Error::InvalidPrivateKey)?;
        Ok(IdentityKey { index, key })
    }

    /// Wraps an existing key (e.g. for tests).
    pub fn from_secret_key(index: u32, key: SecretKey) -> Self {
        IdentityKey { index, key }
    }

    pub fn index(&self) -> u32 {
        self.index
    }

    pub fn path(&self) -> String {
        identity_path(self.index)
    }

    pub fn address(&self) -> Address {
        self.key.address()
    }

    /// 33-byte compressed public key.
    pub fn public_key(&self) -> [u8; 33] {
        self.key.public_key()
    }

    pub fn secret_key(&self) -> &SecretKey {
        &self.key
    }

    /// RFC 6979, low-s, `r || s || v` (v = 27/28).
    pub fn sign_digest(&self, digest: &[u8; 32]) -> [u8; 65] {
        crate::sig::sign_digest(&self.key, digest).expect("valid key")
    }

    /// EIP-191 `personal_sign`.
    pub fn sign_personal(&self, msg: &[u8]) -> [u8; 65] {
        crate::sig::sign_personal(&self.key, msg).expect("valid key")
    }
}
