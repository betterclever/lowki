//! Lowercase `0x` hex helpers (public data only; never used on secrets).

use alloc::string::String;
use alloc::vec::Vec;

use crate::Error;

const HEX: &[u8; 16] = b"0123456789abcdef";

/// `0x`-prefixed lowercase hex.
pub fn encode(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(2 + bytes.len() * 2);
    s.push_str("0x");
    push_hex(&mut s, bytes);
    s
}

pub(crate) fn push_hex(s: &mut String, bytes: &[u8]) {
    for b in bytes {
        s.push(HEX[(b >> 4) as usize] as char);
        s.push(HEX[(b & 15) as usize] as char);
    }
}

/// viem's `isHex(value, { strict: true })`: `^0x[0-9a-fA-F]*$`.
pub fn is_hex(s: &str) -> bool {
    s.strip_prefix("0x")
        .is_some_and(|h| h.bytes().all(|c| c.is_ascii_hexdigit()))
}

/// Decodes `0x`-prefixed hex (either case). Odd length is an error.
pub fn decode(s: &str) -> Result<Vec<u8>, Error> {
    let h = s.strip_prefix("0x").ok_or(Error::InvalidHex)?;
    decode_unprefixed(h)
}

/// Decodes bare hex (either case). Odd length is an error.
pub fn decode_unprefixed(h: &str) -> Result<Vec<u8>, Error> {
    let b = h.as_bytes();
    if !b.len().is_multiple_of(2) {
        return Err(Error::InvalidHex);
    }
    b.chunks(2)
        .map(|c| Ok((nib(c[0])? << 4) | nib(c[1])?))
        .collect()
}

fn nib(c: u8) -> Result<u8, Error> {
    match c {
        b'0'..=b'9' => Ok(c - b'0'),
        b'a'..=b'f' => Ok(c - b'a' + 10),
        b'A'..=b'F' => Ok(c - b'A' + 10),
        _ => Err(Error::InvalidHex),
    }
}
