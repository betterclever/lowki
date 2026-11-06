use alloc::string::String;
use k256::elliptic_curve::sec1::ToEncodedPoint;
use k256::{NonZeroScalar, ProjectivePoint, PublicKey};
use sha3::{Digest, Keccak256};
use zeroize::Zeroizing;

use crate::Error;

/// 20-byte Ethereum address.
pub type Address = [u8; 20];

/// keccak256 (Ethereum's pre-standard Keccak).
pub fn keccak256(data: &[u8]) -> [u8; 32] {
    Keccak256::digest(data).into()
}

/// A secp256k1 private key in `[1, n)`. Zeroized on drop; `Debug` does not print it.
#[derive(Clone)]
pub struct SecretKey(k256::SecretKey);

impl SecretKey {
    /// Parse a 32-byte big-endian private key; rejects 0 and values >= n.
    pub fn from_bytes(bytes: &[u8; 32]) -> Result<Self, Error> {
        k256::SecretKey::from_bytes(bytes.into())
            .map(SecretKey)
            .map_err(|_| Error::InvalidPrivateKey)
    }

    /// Parse from a slice (must be exactly 32 bytes).
    pub fn from_slice(bytes: &[u8]) -> Result<Self, Error> {
        let arr: &[u8; 32] = bytes.try_into().map_err(|_| Error::InvalidPrivateKey)?;
        Self::from_bytes(arr)
    }

    pub(crate) fn from_scalar(s: NonZeroScalar) -> Self {
        SecretKey(k256::SecretKey::from(s))
    }

    /// The 32-byte big-endian private key (zeroized when dropped).
    pub fn to_bytes(&self) -> Zeroizing<[u8; 32]> {
        let fb = Zeroizing::new(self.0.to_bytes());
        let mut out = Zeroizing::new([0u8; 32]);
        out.copy_from_slice(&fb);
        out
    }

    pub(crate) fn scalar(&self) -> NonZeroScalar {
        self.0.to_nonzero_scalar()
    }

    /// 33-byte SEC1 compressed public key.
    pub fn public_key(&self) -> [u8; 33] {
        compress(&self.0.public_key())
    }

    /// Ethereum address of this key.
    pub fn address(&self) -> Address {
        address_from_point(&self.0.public_key().to_projective())
    }
}

impl core::fmt::Debug for SecretKey {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str("SecretKey(<redacted>)")
    }
}

pub(crate) fn compress(pk: &PublicKey) -> [u8; 33] {
    let ep = pk.to_encoded_point(true);
    let mut out = [0u8; 33];
    out.copy_from_slice(ep.as_bytes());
    out
}

/// Parse a SEC1 public key (33-byte compressed or 65-byte uncompressed).
pub(crate) fn parse_pubkey(bytes: &[u8]) -> Result<PublicKey, Error> {
    if bytes.len() != 33 && bytes.len() != 65 {
        return Err(Error::InvalidPublicKey);
    }
    PublicKey::from_sec1_bytes(bytes).map_err(|_| Error::InvalidPublicKey)
}

/// Parse a 33-byte compressed public key strictly (prefix 0x02/0x03, on curve).
pub(crate) fn parse_compressed(bytes: &[u8]) -> Result<PublicKey, Error> {
    if bytes.len() != 33 || (bytes[0] != 2 && bytes[0] != 3) {
        return Err(Error::InvalidPublicKey);
    }
    PublicKey::from_sec1_bytes(bytes).map_err(|_| Error::InvalidPublicKey)
}

pub(crate) fn address_from_point(p: &ProjectivePoint) -> Address {
    let ep = p.to_affine().to_encoded_point(false);
    let h = keccak256(&ep.as_bytes()[1..]);
    let mut out = [0u8; 20];
    out.copy_from_slice(&h[12..]);
    out
}

/// Ethereum address of a SEC1 public key (33 or 65 bytes).
pub fn address_from_pubkey(pubkey: &[u8]) -> Result<Address, Error> {
    Ok(address_from_point(&parse_pubkey(pubkey)?.to_projective()))
}

/// EIP-55 mixed-case checksum encoding (`0x`-prefixed).
pub fn to_checksum_address(addr: &Address) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut lower = [0u8; 40];
    for (i, b) in addr.iter().enumerate() {
        lower[2 * i] = HEX[(b >> 4) as usize];
        lower[2 * i + 1] = HEX[(b & 0xf) as usize];
    }
    let h = keccak256(&lower);
    let mut s = String::with_capacity(42);
    s.push_str("0x");
    for (i, &c) in lower.iter().enumerate() {
        let nibble = (h[i / 2] >> (if i % 2 == 0 { 4 } else { 0 })) & 0xf;
        if c.is_ascii_alphabetic() && nibble >= 8 {
            s.push(c.to_ascii_uppercase() as char);
        } else {
            s.push(c as char);
        }
    }
    s
}
