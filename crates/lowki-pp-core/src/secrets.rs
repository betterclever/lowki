//! Note secrets.
//!
//! * [`SignatureSecretManager`] — Kohaku's default `SignatureSecretManager`
//!   (`privacy-pools/src/account/signature-keys.ts`): one deterministic (RFC 6979)
//!   EIP-712 signature per note, hashed into the nullifier / salt / ephemeral signer.
//! * [`legacy`] — the deprecated BIP-32 HD `SecretManager` (`keys.ts`).

use core::fmt;

use k256::ecdsa::SigningKey;
use k256::elliptic_curve::ops::Reduce;
use k256::elliptic_curve::scalar::IsHigh;
use k256::{FieldBytes, Scalar, U256};
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

use crate::field::fr_from_be_bytes_mod_order;
use crate::note::keccak256;
use crate::poseidon::{poseidon1, poseidon2};
use crate::{Error, Fr};

/// Note secrets (Kohaku `Secret`).
///
/// `precommitment = poseidon([nullifier, salt])`, `nullifier_hash = poseidon([nullifier])`.
/// Zeroized on drop; `Debug` redacts `nullifier` and `salt`.
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct Secret {
    pub nullifier: Fr,
    pub salt: Fr,
    pub precommitment: Fr,
    pub nullifier_hash: Fr,
}

impl Secret {
    /// Completes a secret from its nullifier and salt.
    pub fn from_nullifier_and_salt(nullifier: Fr, salt: Fr) -> Self {
        Secret {
            precommitment: poseidon2(nullifier, salt),
            nullifier_hash: poseidon1(nullifier),
            nullifier,
            salt,
        }
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Secret")
            .field("nullifier", &"<redacted>")
            .field("salt", &"<redacted>")
            .field("precommitment", &self.precommitment)
            .field("nullifier_hash", &self.nullifier_hash)
            .finish()
    }
}

/// The message a note signature binds to (Kohaku `NoteMessage`).
/// `secret_index` 0 is the deposit secret, 1.. are withdrawal (change) secrets.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NoteMessage {
    pub chain_id: u64,
    /// The Privacy Pools entrypoint (EIP-712 `verifyingContract`).
    pub entrypoint: [u8; 20],
    pub deposit_index: u64,
    pub secret_index: u64,
}

const DOMAIN_TYPE: &[u8] =
    b"EIP712Domain(string name,string version,uint256 chainId,address verifyingContract,bytes32 salt)";
const NOTE_TYPE: &[u8] = b"PrivacyPoolNote(uint64 depositIndex,uint64 secretIndex)";
const DOMAIN_NAME: &[u8] = b"PrivacyPools Keychain";
const DOMAIN_VERSION: &[u8] = b"1";
const DOMAIN_SALT_PREIMAGE: &[u8] = b"privacypools";

/// secp256k1 group order n, big-endian.
const SECP256K1_N: [u8; 32] = [
    0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xfe,
    0xba, 0xae, 0xdc, 0xe6, 0xaf, 0x48, 0xa0, 0x3b, 0xbf, 0xd2, 0x5e, 0x8c, 0xd0, 0x36, 0x41, 0x41,
];

fn u256_word(n: u64) -> [u8; 32] {
    let mut w = [0u8; 32];
    w[24..].copy_from_slice(&n.to_be_bytes());
    w
}

/// EIP-712 digest of a note envelope:
/// domain `{ name: "PrivacyPools Keychain", version: "1", chainId, verifyingContract: entrypoint,
/// salt: keccak256("privacypools") }`, primary type
/// `PrivacyPoolNote(uint64 depositIndex,uint64 secretIndex)`.
pub fn note_typed_data_hash(msg: &NoteMessage) -> [u8; 32] {
    let mut vc = [0u8; 32];
    vc[12..].copy_from_slice(&msg.entrypoint);
    let domain_separator = keccak256(&[
        &keccak256(&[DOMAIN_TYPE]),
        &keccak256(&[DOMAIN_NAME]),
        &keccak256(&[DOMAIN_VERSION]),
        &u256_word(msg.chain_id),
        &vc,
        &keccak256(&[DOMAIN_SALT_PREIMAGE]),
    ]);
    let struct_hash = keccak256(&[
        &keccak256(&[NOTE_TYPE]),
        &u256_word(msg.deposit_index),
        &u256_word(msg.secret_index),
    ]);
    keccak256(&[b"\x19\x01", &domain_separator, &struct_hash])
}

/// Kohaku `canonicalRS`: `r(32) ‖ lowS(s)(32)` of a 65-byte `r ‖ s ‖ v` signature.
///
/// Accepts exactly what Kohaku (viem 2.54.1 `parseSignature`) accepts: 65 bytes,
/// `v ∈ {0, 1, 27, 28}`, `1 <= r, s < n`. A high `s` is folded to `n - s`.
/// 64-byte ERC-2098 compact signatures are rejected (as they are by the reference,
/// despite its doc comment).
pub fn canonical_rs(sig: &[u8]) -> Result<Zeroizing<[u8; 64]>, Error> {
    if sig.len() != 65 || !matches!(sig[64], 0 | 1 | 27 | 28) {
        return Err(Error::InvalidSignature);
    }
    let r: [u8; 32] = sig[..32].try_into().expect("32");
    let s: [u8; 32] = sig[32..64].try_into().expect("32");
    let in_range =
        |x: &[u8; 32]| x.iter().any(|b| *b != 0) && x.as_slice() < SECP256K1_N.as_slice();
    if !in_range(&r) || !in_range(&s) {
        return Err(Error::InvalidSignature);
    }
    let s_scalar = <k256::Scalar as k256::elliptic_curve::PrimeField>::from_repr(s.into());
    let s_scalar = Option::<k256::Scalar>::from(s_scalar).ok_or(Error::InvalidSignature)?;
    let s_low = if bool::from(s_scalar.is_high()) {
        -s_scalar
    } else {
        s_scalar
    };
    let mut out = Zeroizing::new([0u8; 64]);
    out[..32].copy_from_slice(&r);
    out[32..].copy_from_slice(&s_low.to_bytes());
    Ok(out)
}

/// Kohaku `fieldFromSig`: the first 31 bytes of `keccak256(tag ‖ rs)` as a field element
/// (always < 2^248 < p, no reduction).
pub fn field_from_rs(tag: &str, rs: &[u8; 64]) -> Fr {
    let mut digest = Zeroizing::new(keccak256(&[tag.as_bytes(), rs]));
    digest.copy_within(0..31, 1);
    digest[0] = 0;
    fr_from_be_bytes_mod_order(&digest)
}

/// Note secrets from a note signature (65-byte `r ‖ s ‖ v`), for external /
/// hardware signers that implement Kohaku's `EnvelopeSigner` seam.
pub fn secrets_from_signature(sig: &[u8]) -> Result<Secret, Error> {
    let rs = canonical_rs(sig)?;
    Ok(Secret::from_nullifier_and_salt(
        field_from_rs("nullifier", &rs),
        field_from_rs("salt", &rs),
    ))
}

/// Ephemeral (paymaster sender) private key from a note signature: `fieldFromSig("signer")`
/// as 32 big-endian bytes.
pub fn ephemeral_signer_from_signature(sig: &[u8]) -> Result<Zeroizing<[u8; 32]>, Error> {
    let rs = canonical_rs(sig)?;
    let mut digest = Zeroizing::new(keccak256(&[b"signer", rs.as_slice()]));
    digest.copy_within(0..31, 1);
    digest[0] = 0;
    Ok(digest)
}

/// Kohaku's default `SignatureSecretManager`, given the account identity key
/// (the secp256k1 key Kohaku derives at `m/28784'/1'/{accountIndex}'`).
///
/// Signing is RFC 6979 (k256), hence deterministic by construction; Kohaku's runtime
/// "sign twice" determinism check is therefore unnecessary here.
pub struct SignatureSecretManager {
    key: SigningKey,
}

impl fmt::Debug for SignatureSecretManager {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("SignatureSecretManager { key: <redacted> }")
    }
}

impl SignatureSecretManager {
    /// From the 32-byte account identity private key.
    pub fn new(account_private_key: &[u8; 32]) -> Result<Self, Error> {
        let key =
            SigningKey::from_slice(account_private_key).map_err(|_| Error::InvalidPrivateKey)?;
        Ok(Self { key })
    }

    /// RFC 6979 signature of the note envelope as 65 bytes `r ‖ s ‖ v` (low-s,
    /// `v ∈ {27, 28}`), identical to viem's `signTypedData`.
    pub fn sign_note(&self, msg: &NoteMessage) -> Result<Zeroizing<[u8; 65]>, Error> {
        let digest = note_typed_data_hash(msg);
        // RFC 6979 / @noble/curves (viem) seed the nonce with `digest mod n`; k256 uses the
        // raw digest, so they diverge for digests >= n. Note secrets come from this
        // signature, so reduce first to stay bit-exact with Kohaku.
        let z: FieldBytes = <Scalar as Reduce<U256>>::reduce_bytes(&digest.into()).to_bytes();
        let (sig, recid) = self
            .key
            .sign_prehash_recoverable(&z)
            .map_err(|_| Error::SigningFailed)?;
        let mut out = Zeroizing::new([0u8; 65]);
        out[..64].copy_from_slice(&sig.to_bytes());
        out[64] = 27 + recid.is_y_odd() as u8;
        Ok(out)
    }

    /// Secrets for `secret_index` of a deposit lineage (0 = deposit, 1.. = withdrawals).
    pub fn note_secrets(&self, msg: &NoteMessage) -> Result<Secret, Error> {
        secrets_from_signature(self.sign_note(msg)?.as_slice())
    }

    /// Kohaku `getDepositSecrets` (secretIndex 0).
    pub fn deposit_secrets(
        &self,
        chain_id: u64,
        entrypoint: &[u8; 20],
        deposit_index: u64,
    ) -> Result<Secret, Error> {
        self.note_secrets(&NoteMessage {
            chain_id,
            entrypoint: *entrypoint,
            deposit_index,
            secret_index: 0,
        })
    }

    /// Kohaku `getSecrets` (secretIndex = withdrawIndex).
    pub fn withdrawal_secrets(
        &self,
        chain_id: u64,
        entrypoint: &[u8; 20],
        deposit_index: u64,
        withdraw_index: u64,
    ) -> Result<Secret, Error> {
        self.note_secrets(&NoteMessage {
            chain_id,
            entrypoint: *entrypoint,
            deposit_index,
            secret_index: withdraw_index,
        })
    }

    /// Kohaku `deriveEphemeralSigner`: 32-byte private key of the single-use
    /// paymaster sender for `(deposit_index, withdraw_index)`.
    pub fn derive_ephemeral_signer(
        &self,
        chain_id: u64,
        entrypoint: &[u8; 20],
        deposit_index: u64,
        withdraw_index: u64,
    ) -> Result<Zeroizing<[u8; 32]>, Error> {
        let msg = NoteMessage {
            chain_id,
            entrypoint: *entrypoint,
            deposit_index,
            secret_index: withdraw_index,
        };
        ephemeral_signer_from_signature(self.sign_note(&msg)?.as_slice())
    }
}

/// **Legacy / deprecated.** Kohaku's BIP-32 HD `SecretManager` (`keys.ts`), superseded
/// by [`SignatureSecretManager`]. The two schemes derive disjoint notes.
///
/// Callers derive the HD keys themselves (paths
/// `m/28784'/1'/{account}'/{0=nullifier,1=salt,2=signer}'/{depositIndex}'/{secretIndex}'`)
/// and pass the raw 32-byte keys here. `hashToSnarkField([chainId, entrypoint, key])`
/// is `poseidon3` with the key reduced mod p (keys can exceed p).
pub mod legacy {
    use zeroize::Zeroizing;

    use super::Secret;
    use crate::Fr;
    use crate::field::{fr_from_be_bytes_mod_order, fr_to_be_bytes};
    use crate::poseidon::poseidon3;

    fn hash_to_snark_field(chain_id: u64, entrypoint: &[u8; 20], key: &[u8; 32]) -> Fr {
        let mut ep = [0u8; 32];
        ep[12..].copy_from_slice(entrypoint);
        poseidon3(
            Fr::from(chain_id),
            fr_from_be_bytes_mod_order(&ep),
            fr_from_be_bytes_mod_order(key),
        )
    }

    /// Legacy note secrets from the HD-derived nullifier and salt keys.
    pub fn legacy_secrets(
        chain_id: u64,
        entrypoint: &[u8; 20],
        nullifier_key: &[u8; 32],
        salt_key: &[u8; 32],
    ) -> Secret {
        Secret::from_nullifier_and_salt(
            hash_to_snark_field(chain_id, entrypoint, nullifier_key),
            hash_to_snark_field(chain_id, entrypoint, salt_key),
        )
    }

    /// Legacy ephemeral signer from the HD-derived signer key (secretType 2').
    pub fn legacy_ephemeral_signer(
        chain_id: u64,
        entrypoint: &[u8; 20],
        signer_key: &[u8; 32],
    ) -> Zeroizing<[u8; 32]> {
        let mut f = hash_to_snark_field(chain_id, entrypoint, signer_key);
        let out = Zeroizing::new(fr_to_be_bytes(&f));
        zeroize::Zeroize::zeroize(&mut f);
        out
    }
}
