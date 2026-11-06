//! Commitment, withdrawal context, and the on-chain scope / label derivations.

use sha3::{Digest, Keccak256};

use crate::Fr;
use crate::field::fr_from_be_bytes_mod_order;
use crate::poseidon::poseidon3;

pub(crate) fn keccak256(parts: &[&[u8]]) -> [u8; 32] {
    let mut h = Keccak256::new();
    for p in parts {
        h.update(p);
    }
    h.finalize().into()
}

/// Note commitment `poseidon([value, label, precommitment])` (Kohaku `utils.ts`
/// `commitment`; on-chain `PoseidonT4.hash([_value, _label, _precommitmentHash])`).
pub fn commitment(value: Fr, label: Fr, precommitment: Fr) -> Fr {
    poseidon3(value, label, precommitment)
}

/// Kohaku `calculateContext` (`utils/proof.util.ts`):
/// `keccak256(abi.encode((address processooor, bytes data) withdrawal, uint256 scope)) % SNARK_SCALAR_FIELD`.
///
/// `scope` is any uint256 (big-endian). Kohaku returns the result as unpadded hex
/// (`numberToHex`); here it is the field element.
pub fn calculate_context(processooor: &[u8; 20], data: &[u8], scope: &[u8; 32]) -> Fr {
    let padded_len = data.len().div_ceil(32) * 32;
    let mut enc = Vec::with_capacity(32 * 5 + padded_len);
    let word = |n: u64| {
        let mut w = [0u8; 32];
        w[24..].copy_from_slice(&n.to_be_bytes());
        w
    };
    enc.extend_from_slice(&word(0x40)); // offset of the dynamic tuple
    enc.extend_from_slice(scope);
    enc.extend_from_slice(&[0u8; 12]);
    enc.extend_from_slice(processooor);
    enc.extend_from_slice(&word(0x40)); // offset of `data` within the tuple
    enc.extend_from_slice(&word(data.len() as u64));
    enc.extend_from_slice(data);
    enc.resize(32 * 5 + padded_len, 0);
    fr_from_be_bytes_mod_order(&keccak256(&[&enc]))
}

/// Pool scope, as set in the privacy-pools-core `State` constructor:
/// `keccak256(abi.encodePacked(address(pool), block.chainid, asset)) % SNARK_SCALAR_FIELD`.
pub fn compute_scope(pool: &[u8; 20], chain_id: u64, asset: &[u8; 20]) -> Fr {
    let mut cid = [0u8; 32];
    cid[24..].copy_from_slice(&chain_id.to_be_bytes());
    fr_from_be_bytes_mod_order(&keccak256(&[pool, &cid, asset]))
}

/// Deposit label, as computed by privacy-pools-core `PrivacyPool.deposit`:
/// `keccak256(abi.encodePacked(SCOPE, ++nonce)) % SNARK_SCALAR_FIELD`, where `nonce` is
/// the pool's global deposit counter *after* incrementing (the first deposit uses 1).
/// The nonce is on-chain state, so wallets normally read `label` from the
/// `Deposited` event (as Kohaku does); this recomputes / predicts it.
pub fn compute_label(scope: &Fr, nonce: &[u8; 32]) -> Fr {
    let s = crate::field::fr_to_be_bytes(scope);
    fr_from_be_bytes_mod_order(&keccak256(&[&s, nonce]))
}
