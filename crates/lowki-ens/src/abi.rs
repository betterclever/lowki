//! Minimal Solidity ABI encoder/decoder (enough for the ENS, registrar and gateway calls in
//! this crate).
//!
//! Encoding works on a [`Token`] tree (static words, `bytes`, `string`, dynamic arrays `T[]`,
//! tuples). Decoding is done with [`Reader`], a bounds-checked view over one ABI frame (the
//! encoding of a tuple, or of the element heads of an array). Every read is checked; no input
//! can make the decoder panic or allocate more than the input size.
//!
//! Strictness (deliberately stricter than viem, which ignores padding): addresses must have
//! zero upper 12 bytes, `bool` must be 0 or 1, `uintN` with N < 256 must fit, offsets and
//! lengths must fit in `usize`, and `string` must be valid UTF-8.

use alloc::string::String;
use alloc::vec::Vec;

use crate::{Address, Error, U256, keccak256};

/// One ABI value to encode.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Token {
    /// Any static 32-byte word (`uintN`, `intN`, `address`, `bool`, `bytesN`), already encoded.
    Word([u8; 32]),
    /// `bytes`
    Bytes(Vec<u8>),
    /// `string`
    String(String),
    /// Dynamic array `T[]`.
    Array(Vec<Token>),
    /// Tuple / struct (static if all members are static).
    Tuple(Vec<Token>),
}

impl Token {
    pub fn address(a: &Address) -> Token {
        let mut w = [0u8; 32];
        w[12..].copy_from_slice(a);
        Token::Word(w)
    }
    pub fn uint(v: u64) -> Token {
        Token::Word(u256_from_u64(v))
    }
    pub fn u256(v: &U256) -> Token {
        Token::Word(*v)
    }
    pub fn bool(b: bool) -> Token {
        Token::uint(b as u64)
    }
    pub fn b32(v: &[u8; 32]) -> Token {
        Token::Word(*v)
    }
    /// `bytes4` (left-aligned).
    pub fn bytes4(v: &[u8; 4]) -> Token {
        let mut w = [0u8; 32];
        w[..4].copy_from_slice(v);
        Token::Word(w)
    }
    pub fn bytes(v: &[u8]) -> Token {
        Token::Bytes(v.to_vec())
    }
    pub fn string(s: &str) -> Token {
        Token::String(s.into())
    }
    pub fn bytes_array<B: AsRef<[u8]>>(items: &[B]) -> Token {
        Token::Array(items.iter().map(|b| Token::bytes(b.as_ref())).collect())
    }
    pub fn string_array<S: AsRef<str>>(items: &[S]) -> Token {
        Token::Array(items.iter().map(|s| Token::string(s.as_ref())).collect())
    }

    fn is_dynamic(&self) -> bool {
        match self {
            Token::Word(_) => false,
            Token::Bytes(_) | Token::String(_) | Token::Array(_) => true,
            Token::Tuple(t) => t.iter().any(Token::is_dynamic),
        }
    }

    /// Size of the head slot(s) this token occupies inside its parent frame.
    fn head_len(&self) -> usize {
        match self {
            Token::Tuple(t) if !self.is_dynamic() => t.iter().map(Token::head_len).sum(),
            _ => 32,
        }
    }
}

/// Big-endian 32-byte word from a `u64`.
pub const fn u256_from_u64(v: u64) -> U256 {
    let b = v.to_be_bytes();
    let mut w = [0u8; 32];
    let mut i = 0;
    while i < 8 {
        w[24 + i] = b[i];
        i += 1;
    }
    w
}

/// The 4-byte selector of a canonical function / error signature, e.g. `"addr(bytes32)"`.
pub fn selector(signature: &str) -> [u8; 4] {
    let h = keccak256(signature.as_bytes());
    [h[0], h[1], h[2], h[3]]
}

/// `abi.encode(tokens...)`.
pub fn encode(tokens: &[Token]) -> Vec<u8> {
    let mut out = Vec::new();
    encode_seq(tokens, &mut out);
    out
}

/// `selector || abi.encode(tokens...)`.
pub fn encode_call(selector: [u8; 4], tokens: &[Token]) -> Vec<u8> {
    let mut out = Vec::from(selector);
    encode_seq(tokens, &mut out);
    out
}

/// Calldata for a canonical signature: `selector(sig) || abi.encode(tokens...)`.
pub fn encode_sig(signature: &str, tokens: &[Token]) -> Vec<u8> {
    encode_call(selector(signature), tokens)
}

fn encode_seq(tokens: &[Token], out: &mut Vec<u8>) {
    let head_size: usize = tokens.iter().map(Token::head_len).sum();
    let mut tail = Vec::new();
    for t in tokens {
        if t.is_dynamic() {
            out.extend_from_slice(&usize_word(head_size + tail.len()));
            encode_tail(t, &mut tail);
        } else {
            encode_static(t, out);
        }
    }
    out.extend_from_slice(&tail);
}

fn encode_static(t: &Token, out: &mut Vec<u8>) {
    match t {
        Token::Word(w) => out.extend_from_slice(w),
        Token::Tuple(ts) => {
            for x in ts {
                encode_static(x, out)
            }
        }
        _ => unreachable!("dynamic token in static position"),
    }
}

fn encode_tail(t: &Token, out: &mut Vec<u8>) {
    match t {
        Token::Bytes(b) => encode_packed_bytes(b, out),
        Token::String(s) => encode_packed_bytes(s.as_bytes(), out),
        Token::Array(items) => {
            out.extend_from_slice(&usize_word(items.len()));
            encode_seq(items, out);
        }
        Token::Tuple(ts) => encode_seq(ts, out),
        Token::Word(_) => unreachable!("static token in tail"),
    }
}

fn encode_packed_bytes(b: &[u8], out: &mut Vec<u8>) {
    out.extend_from_slice(&usize_word(b.len()));
    out.extend_from_slice(b);
    let pad = (32 - b.len() % 32) % 32;
    out.resize(out.len() + pad, 0);
}

fn usize_word(v: usize) -> [u8; 32] {
    u256_from_u64(v as u64)
}

/// Splits calldata / revert data into its 4-byte selector and the argument frame.
pub fn split_selector(data: &[u8]) -> Result<([u8; 4], Reader<'_>), Error> {
    if data.len() < 4 {
        return Err(Error::InvalidAbi);
    }
    let sel = [data[0], data[1], data[2], data[3]];
    Ok((sel, Reader::new(&data[4..])))
}

/// Checks the selector and returns the argument frame.
pub fn expect_selector(data: &[u8], expected: [u8; 4]) -> Result<Reader<'_>, Error> {
    let (sel, r) = split_selector(data)?;
    if sel != expected {
        return Err(Error::UnexpectedSelector);
    }
    Ok(r)
}

/// Bounds-checked view of one ABI frame. Head slot `i` is at byte offset `32 * i`; dynamic
/// members are located through offsets relative to the start of the frame.
#[derive(Clone, Copy, Debug)]
pub struct Reader<'a> {
    buf: &'a [u8],
}

impl<'a> Reader<'a> {
    pub fn new(buf: &'a [u8]) -> Self {
        Reader { buf }
    }

    /// The raw bytes of the frame.
    pub fn raw(&self) -> &'a [u8] {
        self.buf
    }

    /// Head word `i`.
    pub fn word(&self, i: usize) -> Result<&'a [u8; 32], Error> {
        let start = i.checked_mul(32).ok_or(Error::InvalidAbi)?;
        let end = start.checked_add(32).ok_or(Error::InvalidAbi)?;
        let s = self.buf.get(start..end).ok_or(Error::InvalidAbi)?;
        Ok(s.try_into().expect("32-byte slice"))
    }

    pub fn u256(&self, i: usize) -> Result<U256, Error> {
        Ok(*self.word(i)?)
    }

    pub fn b32(&self, i: usize) -> Result<[u8; 32], Error> {
        Ok(*self.word(i)?)
    }

    /// `uintN` for N <= 64 (upper bytes must be zero).
    pub fn u64(&self, i: usize) -> Result<u64, Error> {
        word_to_u64(self.word(i)?)
    }

    /// `uint8`..`uint16` style small integer that must fit `max`.
    pub fn uint_max(&self, i: usize, max: u64) -> Result<u64, Error> {
        let v = self.u64(i)?;
        if v > max {
            return Err(Error::InvalidAbi);
        }
        Ok(v)
    }

    pub fn address(&self, i: usize) -> Result<Address, Error> {
        let w = self.word(i)?;
        if w[..12].iter().any(|&b| b != 0) {
            return Err(Error::InvalidAbi);
        }
        let mut a = [0u8; 20];
        a.copy_from_slice(&w[12..]);
        Ok(a)
    }

    pub fn bool(&self, i: usize) -> Result<bool, Error> {
        match self.u64(i)? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(Error::InvalidAbi),
        }
    }

    /// `bytes4` (must be left-aligned with zero padding).
    pub fn bytes4(&self, i: usize) -> Result<[u8; 4], Error> {
        let w = self.word(i)?;
        if w[4..].iter().any(|&b| b != 0) {
            return Err(Error::InvalidAbi);
        }
        Ok([w[0], w[1], w[2], w[3]])
    }

    /// The frame of a dynamic member whose offset is in head slot `i`.
    pub fn frame(&self, i: usize) -> Result<Reader<'a>, Error> {
        let off = word_to_usize(self.word(i)?)?;
        let sub = self.buf.get(off..).ok_or(Error::InvalidAbi)?;
        Ok(Reader { buf: sub })
    }

    /// A static tuple embedded in the head starting at slot `i`.
    pub fn inline(&self, i: usize) -> Result<Reader<'a>, Error> {
        let start = i.checked_mul(32).ok_or(Error::InvalidAbi)?;
        let sub = self.buf.get(start..).ok_or(Error::InvalidAbi)?;
        Ok(Reader { buf: sub })
    }

    /// `bytes` member at head slot `i`.
    pub fn bytes(&self, i: usize) -> Result<&'a [u8], Error> {
        self.frame(i)?.as_bytes()
    }

    /// `string` member at head slot `i` (must be UTF-8).
    pub fn string(&self, i: usize) -> Result<&'a str, Error> {
        core::str::from_utf8(self.bytes(i)?).map_err(|_| Error::InvalidAbi)
    }

    /// Interprets this frame as a `bytes` encoding (length word + data).
    pub fn as_bytes(&self) -> Result<&'a [u8], Error> {
        let len = word_to_usize(self.word(0)?)?;
        let end = len.checked_add(32).ok_or(Error::InvalidAbi)?;
        self.buf.get(32..end).ok_or(Error::InvalidAbi)
    }

    /// Dynamic array member at head slot `i`, whose elements take `elem_words` head words each
    /// (1 for any dynamic element or single word; k for a static k-word tuple).
    /// Returns the element count and the element frame.
    pub fn array(&self, i: usize, elem_words: usize) -> Result<(usize, Reader<'a>), Error> {
        self.frame(i)?.as_array(elem_words)
    }

    /// Interprets this frame as an array encoding (length word + element heads).
    pub fn as_array(&self, elem_words: usize) -> Result<(usize, Reader<'a>), Error> {
        let len = word_to_usize(self.word(0)?)?;
        let elems = Reader {
            buf: &self.buf[32..],
        };
        let need = len
            .checked_mul(elem_words)
            .and_then(|w| w.checked_mul(32))
            .ok_or(Error::InvalidAbi)?;
        if need > elems.buf.len() {
            return Err(Error::InvalidAbi);
        }
        Ok((len, elems))
    }

    /// `bytes[]` at head slot `i`.
    pub fn bytes_array(&self, i: usize) -> Result<Vec<Vec<u8>>, Error> {
        let (n, el) = self.array(i, 1)?;
        (0..n).map(|k| el.bytes(k).map(<[u8]>::to_vec)).collect()
    }

    /// `string[]` at head slot `i`.
    pub fn string_array(&self, i: usize) -> Result<Vec<String>, Error> {
        let (n, el) = self.array(i, 1)?;
        (0..n).map(|k| el.string(k).map(String::from)).collect()
    }

    /// `bool[]` at head slot `i`.
    pub fn bool_array(&self, i: usize) -> Result<Vec<bool>, Error> {
        let (n, el) = self.array(i, 1)?;
        (0..n).map(|k| el.bool(k)).collect()
    }

    /// `address[]` at head slot `i`.
    pub fn address_array(&self, i: usize) -> Result<Vec<Address>, Error> {
        let (n, el) = self.array(i, 1)?;
        (0..n).map(|k| el.address(k)).collect()
    }

    /// `uint256[]` at head slot `i`.
    pub fn u256_array(&self, i: usize) -> Result<Vec<U256>, Error> {
        let (n, el) = self.array(i, 1)?;
        (0..n).map(|k| el.u256(k)).collect()
    }
}

/// A 32-byte big-endian word as `u64` (error if it does not fit).
pub fn word_to_u64(w: &[u8; 32]) -> Result<u64, Error> {
    if w[..24].iter().any(|&b| b != 0) {
        return Err(Error::InvalidAbi);
    }
    Ok(u64::from_be_bytes(w[24..].try_into().expect("8 bytes")))
}

fn word_to_usize(w: &[u8; 32]) -> Result<usize, Error> {
    usize::try_from(word_to_u64(w)?).map_err(|_| Error::InvalidAbi)
}

/// `abi.decode(data, (bytes))`.
pub fn decode_bytes(data: &[u8]) -> Result<Vec<u8>, Error> {
    Reader::new(data).bytes(0).map(<[u8]>::to_vec)
}

/// `abi.decode(data, (string))`.
pub fn decode_string(data: &[u8]) -> Result<String, Error> {
    Reader::new(data).string(0).map(String::from)
}

/// `abi.decode(data, (address))`.
pub fn decode_address(data: &[u8]) -> Result<Address, Error> {
    Reader::new(data).address(0)
}

/// `abi.decode(data, (bool))`.
pub fn decode_bool(data: &[u8]) -> Result<bool, Error> {
    Reader::new(data).bool(0)
}

/// `abi.decode(data, (uint256))`.
pub fn decode_u256(data: &[u8]) -> Result<U256, Error> {
    Reader::new(data).u256(0)
}

/// `abi.decode(data, (bytes[]))`.
pub fn decode_bytes_array(data: &[u8]) -> Result<Vec<Vec<u8>>, Error> {
    Reader::new(data).bytes_array(0)
}

/// `abi.decode(data, (string[]))`.
pub fn decode_string_array(data: &[u8]) -> Result<Vec<String>, Error> {
    Reader::new(data).string_array(0)
}

/// `1 << n` as a big-endian word (n < 256).
pub const fn bit(n: u32) -> U256 {
    let mut w = [0u8; 32];
    w[31 - (n / 8) as usize] = 1 << (n % 8);
    w
}

/// Bitwise OR of two words.
pub const fn or(a: U256, b: U256) -> U256 {
    let mut w = [0u8; 32];
    let mut i = 0;
    while i < 32 {
        w[i] = a[i] | b[i];
        i += 1;
    }
    w
}

/// `x << 128` for a role in the low 128 bits (its admin counterpart in EAC).
pub const fn admin(role: U256) -> U256 {
    let mut w = [0u8; 32];
    let mut i = 0;
    while i < 16 {
        w[i] = role[i + 16];
        i += 1;
    }
    w
}
