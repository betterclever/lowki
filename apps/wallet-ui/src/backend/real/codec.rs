//! Pure encodings used by the real backend: base64url (server JSON), hex, and the
//! human recovery code. No cryptography: the code's check digits only catch typos.

const B64: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";

/// Unpadded base64url, as lowki-server and WebAuthn JSON use it.
pub fn b64u(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk.iter().enumerate().fold(0u32, |acc, (i, b)| acc | (*b as u32) << (16 - 8 * i));
        let chars = chunk.len() + 1;
        for i in 0..chars {
            out.push(B64[((n >> (18 - 6 * i)) & 63) as usize] as char);
        }
    }
    out
}

/// Decodes base64url or standard base64, padded or not.
pub fn unb64u(s: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(s.len() * 3 / 4);
    let mut acc = 0u32;
    let mut bits = 0u32;
    for c in s.bytes() {
        let v = match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'-' | b'+' => 62,
            b'_' | b'/' => 63,
            b'=' => continue,
            _ => return None,
        } as u32;
        acc = (acc << 6) | v;
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    Some(out)
}

/// Parses a `0x` quantity (JSON-RPC) into u128. `None` if it does not fit.
pub fn parse_qty(s: &str) -> Option<u128> {
    let h = s.strip_prefix("0x").unwrap_or(s);
    if h.is_empty() {
        return Some(0);
    }
    let h = h.trim_start_matches('0');
    if h.len() > 32 {
        return None;
    }
    if h.is_empty() { Some(0) } else { u128::from_str_radix(h, 16).ok() }
}

pub fn is_address(s: &str) -> bool {
    s.len() == 42 && s.starts_with("0x") && s[2..].chars().all(|c| c.is_ascii_hexdigit())
}

// ---- recovery code ---------------------------------------------------------------------

/// Crockford base32 (no I, L, O, U).
const B32: &[u8; 32] = b"0123456789ABCDEFGHJKMNPQRSTVWXYZ";
/// Recovery share R is 33 bytes (lowki-shares, x coordinate included).
pub const SHARE_LEN: usize = 33;
const CHECK_LEN: usize = 2;
const GROUP: usize = 4;

fn check(bytes: &[u8]) -> [u8; CHECK_LEN] {
    // FNV-1a, folded to 16 bits: catches typos and swapped groups, nothing more.
    let mut h: u32 = 0x811c_9dc5;
    for b in b"lowki/recovery-code/v1".iter().chain(bytes) {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    let f = (h >> 16) ^ (h & 0xffff);
    [(f >> 8) as u8, f as u8]
}

/// 33-byte share -> 56 characters in 14 groups of 4 ("K7QM-9XHD-…").
pub fn encode_recovery(share: &[u8]) -> String {
    let mut data = share.to_vec();
    data.extend_from_slice(&check(share));
    let mut chars = String::new();
    let (mut acc, mut bits) = (0u32, 0u32);
    for b in &data {
        acc = (acc << 8) | *b as u32;
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            chars.push(B32[((acc >> bits) & 31) as usize] as char);
        }
        acc &= (1 << bits) - 1;
    }
    if bits > 0 {
        chars.push(B32[((acc << (5 - bits)) & 31) as usize] as char);
    }
    data.iter_mut().for_each(|b| *b = 0);
    chars.as_bytes().chunks(GROUP).map(|g| std::str::from_utf8(g).unwrap_or_default()).collect::<Vec<_>>().join("-")
}

/// Accepts any case, spaces and dashes, and Crockford's look-alikes (O=0, I/L=1).
/// Returns the 33-byte share, or `None` when the code is malformed or mistyped.
pub fn decode_recovery(code: &str) -> Option<Vec<u8>> {
    let mut out = Vec::with_capacity(SHARE_LEN + CHECK_LEN);
    let (mut acc, mut bits) = (0u32, 0u32);
    let mut n = 0;
    for c in code.chars() {
        let c = match c.to_ascii_uppercase() {
            ' ' | '-' | '\n' | '\t' => continue,
            'O' => '0',
            'I' | 'L' => '1',
            c => c,
        };
        let v = B32.iter().position(|x| *x as char == c)? as u32;
        n += 1;
        acc = (acc << 5) | v;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
            acc &= (1 << bits) - 1;
        }
    }
    if n != (SHARE_LEN + CHECK_LEN) * 8 / 5 || out.len() != SHARE_LEN + CHECK_LEN {
        return None;
    }
    let (share, sum) = out.split_at(SHARE_LEN);
    (check(share) == sum).then(|| share.to_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64url_roundtrip() {
        for n in 0..70u8 {
            let v: Vec<u8> = (0..n).map(|i| i.wrapping_mul(37).wrapping_add(n)).collect();
            assert_eq!(unb64u(&b64u(&v)).unwrap(), v);
        }
        assert_eq!(b64u(b"hello"), "aGVsbG8");
        assert_eq!(unb64u("aGVsbG8=").unwrap(), b"hello");
        assert!(unb64u("a b").is_none());
    }

    #[test]
    fn recovery_code_roundtrip_and_typos() {
        let share: Vec<u8> = (0..33u8).map(|i| i.wrapping_mul(91).wrapping_add(7)).collect();
        let code = encode_recovery(&share);
        assert_eq!(code.split('-').count(), 14);
        assert!(code.split('-').all(|g| g.len() == 4));
        assert_eq!(decode_recovery(&code).unwrap(), share);
        assert_eq!(decode_recovery(&code.to_lowercase().replace('-', " ")).unwrap(), share);
        // One wrong character is caught.
        let mut bad: Vec<char> = code.chars().collect();
        bad[5] = if bad[5] == 'A' { 'B' } else { 'A' };
        assert!(decode_recovery(&bad.into_iter().collect::<String>()).is_none());
        assert!(decode_recovery("K7QM-9XHD").is_none());
    }

    #[test]
    fn qty() {
        assert_eq!(parse_qty("0x0"), Some(0));
        assert_eq!(parse_qty("0x"), Some(0));
        assert_eq!(parse_qty("0xde0b6b3a7640000"), Some(1_000_000_000_000_000_000));
        assert!(is_address("0x1c7D4B196Cb0C7B01d743Fbc6116a902379C7238"));
    }
}
