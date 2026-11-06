//! ERC-5564 stealth addresses (scheme id 1: secp256k1 with view tags), ERC-6538
//! registry formats, Announcer calldata/event helpers, and Lowki's BIP-32 stealth
//! key derivation.
//!
//! Hashing conventions (scheme 1, matched byte-for-byte against
//! `@scopelift/stealth-address-sdk` 1.0.0-beta.5 / `@noble/secp256k1` 2.3.0):
//!
//! * shared secret point `S = eph_priv · P_view = view_priv · P_eph`;
//! * `s_h = keccak256(SEC1_compressed(S))` — the **33-byte compressed** encoding
//!   (`0x02|0x03 || x`), not the x-coordinate alone and not the 65-byte form;
//! * view tag = `s_h[0]` (most significant byte);
//! * `s_h` is interpreted as a big-endian integer and used **as-is** as a scalar:
//!   it is **not reduced mod n**. The reference (`Point.fromPrivateKey(s_h)`)
//!   throws if `s_h == 0` or `s_h >= n`; this crate returns
//!   [`Error::InvalidHashScalar`] in that case (probability ≈ 2⁻¹²⁸);
//! * `P_stealth = P_spend + s_h·G`, address = last 20 bytes of
//!   `keccak256(uncompressed(P_stealth)[1..])`;
//! * `p_stealth = (p_spend + s_h) mod n`.
//!
//! Secrets are held in [`SecretKey`] (zeroized on drop, no secret-printing `Debug`).

#![forbid(unsafe_code)]
#![no_std]

extern crate alloc;

mod abi;
pub mod announce;
pub mod bip32;
mod keys;
mod meta;
mod stealth;

pub use announce::{
    ANNOUNCE_SELECTOR_SIGNATURE, ANNOUNCEMENT_EVENT_SIGNATURE, Announcement,
    ERC20_TRANSFER_SELECTOR, ERC721_TRANSFER_FROM_SELECTOR, ETH_TOKEN_ADDRESS,
    ETH_TOKEN_IDENTIFIER, MetadataFields, announcement_topic0, build_metadata,
    decode_announcement_log, decode_stealth_meta_address_of_return, encode_announce_calldata,
    encode_register_keys_calldata, encode_stealth_meta_address_of_calldata, metadata_erc20,
    metadata_erc721, metadata_eth, parse_metadata, view_tag_from_metadata,
};
pub use bip32::{SPENDING_PATH, StealthKeys, VIEWING_PATH, derive_stealth_keys};
pub use keys::{Address, SecretKey, address_from_pubkey, keccak256, to_checksum_address};
pub use meta::StealthMetaAddress;
pub use stealth::{
    GeneratedStealthAddress, check_announcement, compute_stealth_private_key,
    generate_stealth_address,
};

/// ERC-5564 scheme id implemented by this crate (secp256k1 with view tags).
pub const SCHEME_ID: u64 = 1;

/// Canonical ERC-5564 Announcer singleton (same on all chains; CREATE2).
/// Source: EIP-5564 "Deployment Method" section, and
/// `@scopelift/stealth-address-sdk` 1.0.0-beta.5 `dist/config/contractAddresses.js`.
pub const ERC5564_ANNOUNCER: Address = hex20("55649E01B5Df198D18D95b5cc5051630cfD45564");

/// Canonical ERC-6538 Registry singleton (same on all chains; CREATE2).
/// Source: EIP-6538 "Deployment Method" section, and
/// `@scopelift/stealth-address-sdk` 1.0.0-beta.5 `dist/config/contractAddresses.js`.
pub const ERC6538_REGISTRY: Address = hex20("6538E6bf4B0eBd30A8Ea093027Ac2422ce5d6538");

/// Errors returned by this crate. Never carries secret material.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// Meta-address has the wrong format/length or a bad `st:eth:0x` prefix.
    InvalidMetaAddress,
    /// Bytes are not a valid secp256k1 public key (or not compressed where required).
    InvalidPublicKey,
    /// Private key is zero or >= n, or wrong length.
    InvalidPrivateKey,
    /// keccak256(shared secret) is zero or >= n (reference implementation throws too).
    InvalidHashScalar,
    /// Resulting stealth point/key is the identity / zero.
    PointAtInfinity,
    /// Malformed ABI data (calldata, log data, return data).
    InvalidAbi,
    /// Log topics do not match the `Announcement` event.
    InvalidLog,
    /// Metadata does not have the expected layout.
    InvalidMetadata,
    /// BIP-32 derivation produced an invalid key (IL >= n or zero child key).
    Bip32InvalidKey,
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        core::fmt::Debug::fmt(self, f)
    }
}

impl core::error::Error for Error {}

const fn hex20(s: &str) -> [u8; 20] {
    let b = s.as_bytes();
    let mut out = [0u8; 20];
    let mut i = 0;
    while i < 20 {
        out[i] = (nib(b[2 * i]) << 4) | nib(b[2 * i + 1]);
        i += 1;
    }
    out
}

const fn nib(c: u8) -> u8 {
    match c {
        b'0'..=b'9' => c - b'0',
        b'a'..=b'f' => c - b'a' + 10,
        b'A'..=b'F' => c - b'A' + 10,
        _ => panic!("bad hex"),
    }
}
