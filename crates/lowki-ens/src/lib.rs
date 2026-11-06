//! ENS client primitives for Lowki (ENSv2 on Sepolia, namechain tag `v2-mainnet-deployment-1`).
//!
//! * [`name`]: ENSIP-15 normalization, labelhash/namehash, DNS wire encoding (viem- and
//!   `NameCoder`-compatible).
//! * [`abi`]: a small, bounds-checked Solidity ABI encoder/decoder.
//! * [`resolver`], [`universal_resolver`]: resolver profile and UniversalResolver calldata,
//!   results and errors.
//! * [`ccip`]: a sans-IO ERC-3668 (CCIP-Read) client that matches viem 2.57.16, including the
//!   local batch gateway (`x-batch-gateway:true`, ENSIP-21).
//! * [`permissioned_resolver`], [`registry`], [`eth_registrar`]: ENSv2 contract calldata.
//! * [`lowki_registrar`]: EIP-712 registration signatures and calldata for `LowkiRegistrar`.
//! * [`stealth`]: ENSIP-30 (draft) stealth meta-address records, `StealthResolver` calldata,
//!   gateway response / scheme-1 verification data, records-endpoint auth.
//! * [`identity`]: the ENS name-owner identity key `m/137'/0'/{index}'`.
//!
//! The crate does no IO and needs no randomness: the caller supplies keys, nonces and the
//! transport. `no_std` + `alloc`; the default `normalize` feature pulls in `std` through the
//! `ens-normalize` dependency.

#![forbid(unsafe_code)]
#![no_std]

extern crate alloc;

pub mod abi;
pub mod ccip;
pub mod eth_registrar;
pub mod hex;
pub mod identity;
pub mod lowki_registrar;
pub mod name;
pub mod owner_account;
pub mod permissioned_resolver;
pub mod registry;
pub mod resolver;
pub mod sig;
pub mod stealth;
pub mod universal_resolver;

pub use lowki_stealth::{
    Address, SecretKey, StealthMetaAddress, bip32, generate_stealth_address, keccak256,
    to_checksum_address,
};

/// A `uint256` as a 32-byte big-endian word.
pub type U256 = [u8; 32];

pub use abi::u256_from_u64;

/// Errors returned by this crate. Never carries secret material.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    /// ENSIP-15 normalization failed (message from the normalizer).
    Normalize(alloc::string::String),
    /// A name has an empty label where one is not allowed (`a..b`, `.a`).
    EmptyLabel,
    /// A DNS-encoded name is malformed (`NameCoder` `DNSDecodingFailed`).
    InvalidDnsName,
    /// Bytes are not valid UTF-8.
    InvalidUtf8,
    /// Malformed ABI data.
    InvalidAbi,
    /// Calldata / revert data has a different selector than expected.
    UnexpectedSelector,
    /// Malformed hex string.
    InvalidHex,
    /// A `LowkiRegistrar` label breaks the contract's label rules.
    InvalidLabel(&'static str),
    /// An ENSIP-30 record is malformed (length, reserved flag bits, keys).
    InvalidStealthRecord,
    /// Scheme-1 meta-address is malformed (`Scheme1Verifier.MalformedMetaAddress`).
    MalformedMetaAddress,
    /// Scheme-1 verification data is malformed (`InvalidVerificationData`).
    InvalidVerificationData,
    /// The shared secret point does not match `e·P_view` (`InvalidSharedSecret`).
    InvalidSharedSecret,
    /// Invalid private key.
    InvalidPrivateKey,
    /// Invalid or unrecoverable signature.
    InvalidSignature,
    /// BIP-32 path index out of range (must be < 2^31) or derivation failed.
    InvalidDerivation,
    /// Error from `lowki-stealth`.
    Stealth(lowki_stealth::Error),
    /// CCIP-Read failed.
    Ccip(ccip::CcipError),
}

impl From<lowki_stealth::Error> for Error {
    fn from(e: lowki_stealth::Error) -> Self {
        Error::Stealth(e)
    }
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        core::fmt::Debug::fmt(self, f)
    }
}

impl core::error::Error for Error {}

pub type Result<T, E = Error> = core::result::Result<T, E>;
