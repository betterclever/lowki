//! Lowki key primitives.
//!
//! * [`mnemonic`]: BIP-39 (English, 24-word generation, validation, seed) matching `@scure/bip39`.
//! * [`path`]: derivation-path parser with Kohaku's `parseDerivationPath` semantics.
//! * [`keystore`]: Kohaku `MnemonicKeystore.deriveAt` (BIP-32 secp256k1 + RAILGUN hash-chain branch)
//!   and `scopeKeystore`.
//! * [`app`]: Lowki per-origin EOA derivation, Ethereum addresses and EIP-55.
//! * [`wrap`]: wrapping the BIP-39 entropy with a passkey PRF output (HKDF-SHA256 + AES-256-GCM),
//!   interoperable with browser WebCrypto.
//! * [`sign`]: RFC-6979 low-s secp256k1 digest signing.
//!
//! All secret outputs are returned as [`zeroize::Zeroizing`] values; no type in this crate
//! prints secret material through `Debug`.

#![forbid(unsafe_code)]

pub mod app;
mod error;
pub mod keystore;
pub mod mnemonic;
pub mod path;
pub mod sign;
pub mod wrap;

pub use app::{
    address_from_private_key, app_address, app_index, app_path, app_private_key,
    to_checksum_address,
};
pub use error::Error;
pub use keystore::{Keystore, ScopedKeystore, derive_at, derive_indices};
pub use mnemonic::{
    entropy_to_mnemonic, entropy_to_seed, mnemonic_to_entropy, mnemonic_to_seed, validate_mnemonic,
};
pub use path::{HARDENED_OFFSET, parse_derivation_path};
pub use sign::{Signature, sign_digest};
pub use wrap::{WRAPPED_LEN, derive_kek, unwrap_entropy, wrap_entropy};

pub use zeroize::Zeroizing;
