//! Privacy Pools v1 note math, bit-exact with Kohaku's Privacy Pools plugin
//! (`vendor/kohaku` @ `8c83d64050040654082d70901a46a339bfa103b0`).
//!
//! * [`poseidon`] — circomlib-compatible Poseidon over BN254 for 1, 2 and 3 inputs.
//! * [`limt`] — zk-kit LeanIMT over Poseidon T3, plus Kohaku's `proofFromTree`.
//! * [`secrets`] — the default `SignatureSecretManager` and the legacy (deprecated)
//!   HD `SecretManager` derivation.
//! * [`note`] — `commitment`, `calculate_context`, and the on-chain `scope` / `label`
//!   derivations.
//!
//! Field elements are [`Fr`] (`ark_bn254::Fr`), always canonical (< p). Raw 256-bit
//! words (uint256 values, keys) are big-endian `[u8; 32]`; see [`field`] for the
//! conversions and their range-check behaviour.

pub mod field;
pub mod limt;
pub mod note;
pub mod poseidon;
pub mod secrets;

pub use ark_bn254::Fr;
pub use field::{SNARK_SCALAR_FIELD, fr_from_be_bytes, fr_from_be_bytes_mod_order, fr_to_be_bytes};
pub use limt::{LeanImt, MerkleProof, PADDED_PROOF_DEPTH, proof_from_tree, verify_proof};
pub use note::{calculate_context, commitment, compute_label, compute_scope};
pub use poseidon::{poseidon, poseidon1, poseidon2, poseidon3};
pub use secrets::{NoteMessage, Secret, SignatureSecretManager};

use core::fmt;

/// Errors returned by this crate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// Poseidon was called with an unsupported number of inputs (supported: 1..=3).
    InvalidArity(usize),
    /// A 32-byte value was not a canonical field element (>= p).
    NonCanonicalField,
    /// `insert_many` was called with no leaves.
    NoLeaves,
    /// The leaf index does not exist in the tree.
    IndexOutOfRange(usize),
    /// The leaf is not in the tree.
    LeafNotFound,
    /// The private key is not a valid secp256k1 scalar.
    InvalidPrivateKey,
    /// The signature is malformed (length, `v`, or `r`/`s` out of range).
    InvalidSignature,
    /// ECDSA signing failed.
    SigningFailed,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::InvalidArity(n) => {
                write!(f, "unsupported poseidon arity {n} (supported: 1..=3)")
            }
            Error::NonCanonicalField => f.write_str("value is not a canonical BN254 field element"),
            Error::NoLeaves => f.write_str("there are no leaves to add"),
            Error::IndexOutOfRange(i) => {
                write!(f, "the leaf at index {i} does not exist in this tree")
            }
            Error::LeafNotFound => f.write_str("leaf not found in the tree"),
            Error::InvalidPrivateKey => f.write_str("invalid secp256k1 private key"),
            Error::InvalidSignature => f.write_str("invalid signature"),
            Error::SigningFailed => f.write_str("ECDSA signing failed"),
        }
    }
}

impl std::error::Error for Error {}
