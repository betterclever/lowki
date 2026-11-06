//! ENS names: ENSIP-15 normalization (ENSIP-1 hashing) and DNS wire encoding.
//!
//! Two encoders exist on purpose:
//!
//! * [`packet_to_bytes`] is viem 2.57.16's `packetToBytes` byte for byte, including its quirks:
//!   it strips one leading and one trailing `.` (JS regex `/^\.|\.$/gm`, so also next to line
//!   terminators), replaces a label longer than 255 bytes by its encoded labelhash
//!   `[<64 hex>]`, and turns an empty label into a zero byte (an early terminator).
//! * [`dns_encode`] returns the same bytes but refuses names whose encoding would contain an
//!   empty label (viem's output for `a..b` is not a valid DNS name and `NameCoder` rejects it).
//!
//! Hashing on the string side ([`labelhash`], [`namehash`]) follows viem: a label of the form
//! `[<64 hex>]` is taken as its labelhash. Hashing on the DNS side ([`namehash_dns`]) follows
//! ens-contracts `NameCoder.namehash(name, 0)`, which hashes every label's raw bytes (so an
//! encoded-labelhash label is hashed as text there, exactly as the deployed contracts do).

use alloc::string::String;
use alloc::vec::Vec;

use crate::{Error, keccak256};

/// ENSIP-15 normalization (adraffy's `ens_normalize`), using the `ens-normalize` 0.1.1 crate
/// (TateB/ens-normalize-rs), whose bundled spec is the one of `@adraffy/ens-normalize` 1.11.1
/// (Unicode 17.0.0). Checked against all 38,613 of adraffy's official validation tests (see the
/// README). The error carries the normalizer's message (adraffy's wording).
#[cfg(feature = "normalize")]
pub fn normalize(name: &str) -> Result<String, Error> {
    ens_normalize::normalize(name).map_err(|e| Error::Normalize(alloc::format!("{e}")))
}

/// The labelhash encoded in a label of the form `[<64 hex digits>]` (viem's
/// `encodedLabelToLabelhash`; either hex case), or `None`.
pub fn parse_label_hash(label: &str) -> Option<[u8; 32]> {
    let b = label.as_bytes();
    if b.len() != 66 || b[0] != b'[' || b[65] != b']' {
        return None;
    }
    let v = crate::hex::decode_unprefixed(&label[1..65]).ok()?;
    v.try_into().ok()
}

/// `[<64 lowercase hex>]` (viem's `encodeLabelhash`).
pub fn encode_label_hash(hash: &[u8; 32]) -> String {
    let mut s = String::with_capacity(66);
    s.push('[');
    crate::hex::push_hex(&mut s, hash);
    s.push(']');
    s
}

/// viem `labelhash`: zero for the empty label, the embedded hash for `[<hex>]`, otherwise
/// `keccak256(utf8(label))`. The label is assumed normalized.
pub fn labelhash(label: &str) -> [u8; 32] {
    if label.is_empty() {
        return [0u8; 32];
    }
    parse_label_hash(label).unwrap_or_else(|| keccak256(label.as_bytes()))
}

/// ENSIP-1 namehash (viem `namehash`): zero for `""`; labels split on `.`; an empty label
/// hashes as `keccak256("")`; `[<hex>]` labels are taken as labelhashes. The name is assumed
/// normalized.
pub fn namehash(name: &str) -> [u8; 32] {
    let mut node = [0u8; 32];
    if name.is_empty() {
        return node;
    }
    for label in name.rsplit('.') {
        let lh = parse_label_hash(label).unwrap_or_else(|| keccak256(label.as_bytes()));
        node = child_node(&node, &lh);
    }
    node
}

/// `keccak256(parent || labelhash)`.
pub fn child_node(parent: &[u8; 32], labelhash: &[u8; 32]) -> [u8; 32] {
    let mut buf = [0u8; 64];
    buf[..32].copy_from_slice(parent);
    buf[32..].copy_from_slice(labelhash);
    keccak256(&buf)
}

fn is_line_terminator(c: char) -> bool {
    matches!(c, '\n' | '\r' | '\u{2028}' | '\u{2029}')
}

/// viem `name.replace(/^\.|\.$/gm, '')`.
fn strip_dots(name: &str) -> String {
    let chars: Vec<(usize, char)> = name.char_indices().collect();
    let mut out = String::with_capacity(name.len());
    for (k, &(_, c)) in chars.iter().enumerate() {
        if c == '.' {
            let at_line_start = k == 0 || is_line_terminator(chars[k - 1].1);
            let at_line_end = k + 1 == chars.len() || is_line_terminator(chars[k + 1].1);
            if at_line_start || at_line_end {
                continue;
            }
        }
        out.push(c);
    }
    out
}

/// viem `packetToBytes(name)`: DNS wire encoding with viem's exact behavior (see module docs).
pub fn packet_to_bytes(name: &str) -> Vec<u8> {
    encode_labels(name).0
}

fn encode_labels(name: &str) -> (Vec<u8>, bool) {
    let value = strip_dots(name);
    if value.is_empty() {
        return (alloc::vec![0u8], false);
    }
    let mut out = Vec::with_capacity(value.len() + 2);
    let mut has_empty = false;
    for label in value.split('.') {
        let hashed;
        let enc: &[u8] = if label.len() > 255 {
            hashed = encode_label_hash(&keccak256(label.as_bytes()));
            hashed.as_bytes()
        } else {
            label.as_bytes()
        };
        has_empty |= enc.is_empty();
        out.push(enc.len() as u8);
        out.extend_from_slice(enc);
    }
    out.push(0);
    (out, has_empty)
}

/// DNS wire encoding: [`packet_to_bytes`], but an error if any label would be empty
/// (`"a..b"`, `".."`). `""` (and `"."`) encode the root as `[0]`.
pub fn dns_encode(name: &str) -> Result<Vec<u8>, Error> {
    let (out, has_empty) = encode_labels(name);
    if has_empty {
        return Err(Error::EmptyLabel);
    }
    Ok(out)
}

/// Iterates over the labels of a DNS-encoded name with `NameCoder.nextLabel`'s checks: every
/// length byte must be in range, and the terminator must be the last byte.
fn dns_labels(dns: &[u8]) -> Result<Vec<&[u8]>, Error> {
    let mut labels = Vec::new();
    let mut off = 0usize;
    loop {
        let size = *dns.get(off).ok_or(Error::InvalidDnsName)? as usize;
        let next = off + 1 + size;
        if size == 0 {
            if next != dns.len() {
                return Err(Error::InvalidDnsName);
            }
            return Ok(labels);
        }
        if next >= dns.len() {
            return Err(Error::InvalidDnsName);
        }
        labels.push(&dns[off + 1..next]);
        off = next;
    }
}

/// `NameCoder.decode`: DNS-encoded name to dotted name. Errors on malformed encodings, on a
/// label containing `.`, and on invalid UTF-8. `[0]` decodes to `""`.
pub fn dns_decode(dns: &[u8]) -> Result<String, Error> {
    let labels = dns_labels(dns)?;
    let mut out = Vec::with_capacity(dns.len());
    for (i, l) in labels.iter().enumerate() {
        if l.contains(&b'.') {
            return Err(Error::InvalidDnsName);
        }
        if i > 0 {
            out.push(b'.');
        }
        out.extend_from_slice(l);
    }
    String::from_utf8(out).map_err(|_| Error::InvalidUtf8)
}

/// `NameCoder.namehash(name, 0)`: namehash of a DNS-encoded name, hashing each label's raw
/// bytes. Errors like `NameCoder.nextLabel` on malformed encodings.
pub fn namehash_dns(dns: &[u8]) -> Result<[u8; 32], Error> {
    let labels = dns_labels(dns)?;
    let mut node = [0u8; 32];
    for l in labels.iter().rev() {
        node = child_node(&node, &keccak256(l));
    }
    Ok(node)
}

/// The labels of a DNS-encoded name, as UTF-8 strings (leaf first).
pub fn dns_labels_str(dns: &[u8]) -> Result<Vec<String>, Error> {
    dns_labels(dns)?
        .into_iter()
        .map(|l| String::from_utf8(l.to_vec()).map_err(|_| Error::InvalidUtf8))
        .collect()
}
