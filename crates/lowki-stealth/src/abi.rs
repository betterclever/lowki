//! Minimal Solidity ABI encoding/decoding for the handful of signatures we need.

use alloc::vec::Vec;

use crate::Error;
use crate::keys::{Address, keccak256};

pub(crate) fn selector(signature: &str) -> [u8; 4] {
    let h = keccak256(signature.as_bytes());
    [h[0], h[1], h[2], h[3]]
}

pub(crate) fn word_u64(v: u64) -> [u8; 32] {
    let mut w = [0u8; 32];
    w[24..].copy_from_slice(&v.to_be_bytes());
    w
}

pub(crate) fn word_address(a: &Address) -> [u8; 32] {
    let mut w = [0u8; 32];
    w[12..].copy_from_slice(a);
    w
}

/// Static head words (`Static`) and dynamic `bytes` values (`Bytes`).
pub(crate) enum Token<'a> {
    Static([u8; 32]),
    Bytes(&'a [u8]),
}

/// ABI-encode a tuple of static words and `bytes` values (standard head/tail layout).
pub(crate) fn encode(tokens: &[Token<'_>]) -> Vec<u8> {
    let head_len = 32 * tokens.len();
    let mut head = Vec::with_capacity(head_len);
    let mut tail = Vec::new();
    for t in tokens {
        match t {
            Token::Static(w) => head.extend_from_slice(w),
            Token::Bytes(b) => {
                head.extend_from_slice(&word_u64((head_len + tail.len()) as u64));
                tail.extend_from_slice(&word_u64(b.len() as u64));
                tail.extend_from_slice(b);
                let pad = (32 - b.len() % 32) % 32;
                tail.resize(tail.len() + pad, 0);
            }
        }
    }
    head.extend_from_slice(&tail);
    head
}

pub(crate) fn encode_call(sel: [u8; 4], tokens: &[Token<'_>]) -> Vec<u8> {
    let mut out = Vec::from(sel);
    out.extend_from_slice(&encode(tokens));
    out
}

fn read_word(data: &[u8], at: usize) -> Result<&[u8; 32], Error> {
    let end = at.checked_add(32).ok_or(Error::InvalidAbi)?;
    data.get(at..end)
        .and_then(|s| s.try_into().ok())
        .ok_or(Error::InvalidAbi)
}

/// Read a word as a `usize`, rejecting values that don't fit in a u32 (all real offsets/lengths do).
fn read_usize(data: &[u8], at: usize) -> Result<usize, Error> {
    let w = read_word(data, at)?;
    if w[..28].iter().any(|&b| b != 0) {
        return Err(Error::InvalidAbi);
    }
    Ok(u32::from_be_bytes([w[28], w[29], w[30], w[31]]) as usize)
}

/// Decode the dynamic `bytes` whose offset sits in head slot `slot`.
pub(crate) fn decode_bytes(data: &[u8], slot: usize) -> Result<Vec<u8>, Error> {
    let off = read_usize(data, slot * 32)?;
    let len = read_usize(data, off)?;
    let start = off.checked_add(32).ok_or(Error::InvalidAbi)?;
    let end = start.checked_add(len).ok_or(Error::InvalidAbi)?;
    data.get(start..end)
        .map(<[u8]>::to_vec)
        .ok_or(Error::InvalidAbi)
}

/// Strict address decoding from a 32-byte word (upper 12 bytes must be zero).
pub(crate) fn address_from_word(w: &[u8; 32]) -> Result<Address, Error> {
    if w[..12].iter().any(|&b| b != 0) {
        return Err(Error::InvalidAbi);
    }
    let mut a = [0u8; 20];
    a.copy_from_slice(&w[12..]);
    Ok(a)
}
