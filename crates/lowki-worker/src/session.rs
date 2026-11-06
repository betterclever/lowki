//! Unlock state. The seed exists only inside an unlocked `Session`; `lock()` (explicit,
//! or automatic after the timeout) drops it, and every field zeroizes on drop.

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use lowki_keys::{Keystore, ScopedKeystore, Zeroizing};
use rand_core::{CryptoRng, RngCore};
use zeroize::Zeroize;

use crate::error::{ErrorCode, Result, WorkerError};
use crate::protocol::{SecretBytes, ShareXs};

/// Kohaku's Privacy Pools keystore prefix (`PRIVACY_POOLS_KEYSTORE_PREFIX`).
pub const KOHAKU_PP_PREFIX: &str = "m/28784'/1'";

/// Version byte of the passkey-wrapped P share.
pub const WRAPPED_SHARE_VERSION: u8 = 0x02;
/// AAD of the passkey-wrapped P share (the entropy blob uses "lowki/seed/v1").
pub const WRAPPED_SHARE_AAD: &[u8] = b"lowki/share-p/v1";
/// Shamir parameters: 2-of-3 over the 32-byte entropy.
pub const SHARES_N: usize = 3;
pub const SHARES_T: usize = 2;
pub const ENTROPY_LEN: usize = 32;
/// Share length: 32 y-bytes + 1 x-byte.
pub const SHARE_LEN: usize = ENTROPY_LEN + 1;
pub const WRAPPED_SHARE_LEN: usize = 1 + 12 + SHARE_LEN + 16;

pub struct Session {
    entropy: Zeroizing<[u8; 32]>,
    seed: Zeroizing<[u8; 64]>,
    /// Kohaku Host keystore, scoped in Rust to m/28784'/1'/… plus allowed paths.
    kohaku: ScopedKeystore,
    pub expires_at_ms: f64,
}

impl Session {
    pub fn from_entropy(entropy: &[u8; 32], expires_at_ms: f64) -> Result<Self> {
        let seed = lowki_keys::entropy_to_seed(entropy);
        let kohaku = ScopedKeystore::new(Keystore::from_seed(&seed), &[KOHAKU_PP_PREFIX], &[])?;
        Ok(Self { entropy: Zeroizing::new(*entropy), seed, kohaku, expires_at_ms })
    }

    pub fn seed(&self) -> &[u8; 64] {
        &self.seed
    }

    pub fn entropy(&self) -> &[u8; 32] {
        &self.entropy
    }

    pub fn kohaku(&self) -> &ScopedKeystore {
        &self.kohaku
    }

    pub fn kohaku_mut(&mut self) -> &mut ScopedKeystore {
        &mut self.kohaku
    }
}

impl std::fmt::Debug for Session {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Session(expires_at_ms={}, ..)", self.expires_at_ms)
    }
}

/// Output of create/import: everything the iframe must persist or hand out.
pub struct NewWallet {
    pub wrapped_blob: SecretBytes,
    pub wrapped_passkey_share: SecretBytes,
    pub server_share: SecretBytes,
    pub recovery_share: SecretBytes,
    pub share_xs: ShareXs,
}

fn prf32(prf: &SecretBytes) -> Result<Zeroizing<[u8; 32]>> {
    prf.to_array::<32>("prfOutput")
}

/// `0x02 || nonce12 || AES-256-GCM(derive_kek(prf), nonce, share, aad="lowki/share-p/v1")`.
pub fn wrap_share<R: RngCore + CryptoRng>(prf: &[u8; 32], share: &[u8], rng: &mut R) -> Result<SecretBytes> {
    if share.len() != SHARE_LEN {
        return Err(WorkerError::new(ErrorCode::Internal, "unexpected share length"));
    }
    let kek = lowki_keys::derive_kek(prf);
    let cipher = Aes256Gcm::new_from_slice(kek.as_slice()).map_err(|_| WorkerError::new(ErrorCode::Internal, "kek"))?;
    let mut nonce = [0u8; 12];
    rng.fill_bytes(&mut nonce);
    let ct = cipher
        .encrypt(Nonce::from_slice(&nonce), Payload { msg: share, aad: WRAPPED_SHARE_AAD })
        .map_err(|_| WorkerError::new(ErrorCode::Internal, "share wrap failed"))?;
    let mut out = Vec::with_capacity(WRAPPED_SHARE_LEN);
    out.push(WRAPPED_SHARE_VERSION);
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&ct);
    Ok(SecretBytes::new(out))
}

pub fn unwrap_share(prf: &[u8; 32], blob: &[u8]) -> Result<SecretBytes> {
    let fail = |m: &str| WorkerError::new(ErrorCode::UnwrapFailed, m.to_string());
    if blob.len() != WRAPPED_SHARE_LEN {
        return Err(fail("wrapped share has the wrong length"));
    }
    if blob[0] != WRAPPED_SHARE_VERSION {
        return Err(fail("wrapped share has an unknown version"));
    }
    let kek = lowki_keys::derive_kek(prf);
    let cipher = Aes256Gcm::new_from_slice(kek.as_slice()).map_err(|_| fail("kek"))?;
    let pt = cipher
        .decrypt(Nonce::from_slice(&blob[1..13]), Payload { msg: &blob[13..], aad: WRAPPED_SHARE_AAD })
        .map_err(|_| fail("passkey did not open the wrapped share"))?;
    Ok(SecretBytes::new(pt))
}

/// Wraps fresh/imported entropy and splits it 2-of-3. Shares are assigned in split order:
/// P (passkey), S (server), R (recovery).
pub fn new_wallet<R: RngCore + CryptoRng>(prf: &SecretBytes, entropy: &[u8; 32], rng: &mut R) -> Result<NewWallet> {
    let prf = prf32(prf)?;
    let mut nonce = [0u8; 12];
    rng.fill_bytes(&mut nonce);
    let blob = lowki_keys::wrap_entropy(&prf, entropy, &nonce);
    let mut shares = lowki_shares::split(entropy, SHARES_N, SHARES_T, rng)?;
    let (p, s, r) = (shares.remove(0), shares.remove(0), shares.remove(0));
    let share_xs = ShareXs { passkey: p.x(), server: s.x(), recovery: r.x() };
    let wrapped_passkey_share = wrap_share(&prf, p.as_bytes(), rng)?;
    Ok(NewWallet {
        wrapped_blob: SecretBytes::from_slice(&blob),
        wrapped_passkey_share,
        server_share: SecretBytes::from_slice(s.as_bytes()),
        recovery_share: SecretBytes::from_slice(r.as_bytes()),
        share_xs,
    })
}

pub fn unlock_entropy(prf: &SecretBytes, blob: &SecretBytes) -> Result<Zeroizing<[u8; 32]>> {
    let prf = prf32(prf)?;
    lowki_keys::unwrap_entropy(&prf, blob.as_slice()).map_err(|e| match e {
        lowki_keys::Error::Decrypt => WorkerError::new(ErrorCode::UnwrapFailed, "passkey did not open the wrapped blob"),
        other => other.into(),
    })
}

/// Combines two shares (2-of-3) into the 32-byte entropy.
pub fn combine_entropy(a: &[u8], b: &[u8]) -> Result<Zeroizing<[u8; 32]>> {
    let mut secret = lowki_shares::combine(&[a, b], SHARES_T)?;
    if secret.len() != ENTROPY_LEN {
        secret.zeroize();
        return Err(WorkerError::new(ErrorCode::InvalidShares, "shares are not for a 32-byte seed"));
    }
    let mut out = Zeroizing::new([0u8; 32]);
    out.copy_from_slice(&secret);
    Ok(out)
}

pub fn unwrap_passkey_share(prf: &SecretBytes, wrapped: &SecretBytes) -> Result<SecretBytes> {
    let prf = prf32(prf)?;
    unwrap_share(&prf, wrapped.as_slice())
}

/// Re-derives the share at `x` from two shares of the same split (for `rewrap`).
pub fn share_at(a: &[u8], b: &[u8], x: u8) -> Result<SecretBytes> {
    let s = lowki_shares::interpolate_share(&[a, b], SHARES_T, x)?;
    Ok(SecretBytes::from_slice(s.as_bytes()))
}

pub fn rewrap<R: RngCore + CryptoRng>(prf: &SecretBytes, entropy: &[u8; 32], rng: &mut R) -> Result<SecretBytes> {
    let prf = prf32(prf)?;
    let mut nonce = [0u8; 12];
    rng.fill_bytes(&mut nonce);
    Ok(SecretBytes::from_slice(&lowki_keys::wrap_entropy(&prf, entropy, &nonce)))
}

pub fn wrap_passkey_share<R: RngCore + CryptoRng>(prf: &SecretBytes, share: &[u8], rng: &mut R) -> Result<SecretBytes> {
    let prf = prf32(prf)?;
    wrap_share(&prf, share, rng)
}
