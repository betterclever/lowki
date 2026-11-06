use core::fmt;

/// Errors returned by this crate. Variants never carry secret material.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// Not a valid BIP-39 English phrase (word count, unknown word, checksum, or formatting).
    InvalidMnemonic,
    /// Rejected by the derivation-path parser.
    InvalidPath,
    /// A scope prefix failed to parse when building a [`crate::ScopedKeystore`].
    InvalidScope,
    /// The path parses but is outside the keystore scope (`KeystorePathNotAllowedError`).
    PathNotAllowed,
    /// RAILGUN "babyjubjub seed" derivation only supports hardened indices.
    RailgunNonHardened,
    /// BIP-32 derivation produced an invalid key (probability ~2^-127).
    InvalidDerivedKey,
    /// The private key is zero or not below the curve order.
    InvalidPrivateKey,
    /// Wrapped blob has the wrong length.
    BadLength,
    /// Wrapped blob has an unknown version byte.
    BadVersion,
    /// AES-GCM authentication failed (wrong PRF output or tampered blob).
    Decrypt,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Error::InvalidMnemonic => "invalid mnemonic",
            Error::InvalidPath => "invalid derivation path",
            Error::InvalidScope => "invalid keystore scope prefix",
            Error::PathNotAllowed => "keystore path not allowed",
            Error::RailgunNonHardened => "RAILGUN engine derivation only supports hardened indices",
            Error::InvalidDerivedKey => "could not derive a valid private key",
            Error::InvalidPrivateKey => "invalid private key",
            Error::BadLength => "wrapped seed has the wrong length",
            Error::BadVersion => "wrapped seed has an unknown version",
            Error::Decrypt => "wrapped seed failed authentication",
        })
    }
}

impl core::error::Error for Error {}
