//! # lowki-aa
//!
//! Account-abstraction primitives for Lowki's sponsored Privacy Pools unshields,
//! ported byte-for-byte from Kohaku's `packages/privacy-pools` (commit
//! `8c83d64050040654082d70901a46a339bfa103b0`) and viem 2.54.1:
//!
//! - [`userop`]: ERC-4337 EntryPoint v0.8 `PackedUserOperation`, packing helpers and
//!   the EIP-712 `userOpHash`.
//! - [`eip7702`]: EIP-7702 authorization signing hash, signing and the signed tuple
//!   (`eip7702Auth`).
//! - [`signer`]: the ephemeral secp256k1 sender key (zeroized on drop).
//! - [`paymaster`]: Kohaku's PrivacyPaymaster / PrivacyPoolsFeeAdapter encodings, the
//!   Simple7702 execution `callData`, and [`paymaster::build_signed_user_op`].
//! - [`shield`]: Entrypoint `deposit` calldata (ETH / ERC-20) and the ERC-20 approve.
//! - [`fee`]: gas baselines, minimum viable fee and the bundler gas-bound checks.
//! - [`config`]: protocol constants (EntryPoint, Simple7702, paymaster configs).
//!
//! No network access, no randomness: everything is a pure function of its inputs.

pub mod config;
pub mod eip7702;
mod error;
pub mod fee;
pub mod paymaster;
pub mod shield;
pub mod signer;
pub mod userop;

pub use alloy_primitives::{Address, B256, Bytes, U256};
pub use error::Error;

/// `0x`-prefixed minimal hex quantity, as viem's `toHex(bigint|number)`.
pub(crate) fn hex_quantity<T: core::fmt::LowerHex>(v: T) -> String {
    format!("{v:#x}")
}
