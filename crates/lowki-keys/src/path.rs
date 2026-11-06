//! BIP-32 path parser with the exact accept/reject semantics of Kohaku's
//! `parseDerivationPath` (`packages/plugins/src/host/derivation-path.ts`).

use crate::Error;

pub const HARDENED_OFFSET: u32 = 0x8000_0000;
const MAX_DEPTH: usize = 255;

/// Parses `"m/44'/60'/0'/0/0"` into raw indices (hardened offset applied).
///
/// * must start with `m/`; the master path `m` is rejected;
/// * at most 255 segments;
/// * each segment is `0` or `[1-9][0-9]*` (ASCII), optionally followed by one `'` or `h`;
/// * the numeric value must be `< 2^31`.
pub fn parse_derivation_path(path: &str) -> Result<Vec<u32>, Error> {
    let mut segments = path.split('/');
    if segments.next() != Some("m") {
        return Err(Error::InvalidPath);
    }
    let mut out = Vec::new();
    for seg in segments {
        if out.len() == MAX_DEPTH {
            return Err(Error::InvalidPath);
        }
        out.push(parse_segment(seg)?);
    }
    if out.is_empty() {
        return Err(Error::InvalidPath);
    }
    Ok(out)
}

fn parse_segment(seg: &str) -> Result<u32, Error> {
    let bytes = seg.as_bytes();
    let (digits, hardened) = match bytes.last() {
        Some(b'\'') | Some(b'h') => (&bytes[..bytes.len() - 1], true),
        _ => (bytes, false),
    };
    if digits.is_empty() || !digits.iter().all(u8::is_ascii_digit) {
        return Err(Error::InvalidPath);
    }
    if digits.len() > 1 && digits[0] == b'0' {
        return Err(Error::InvalidPath);
    }
    // 2^31 has 10 digits; anything longer is out of range (and cannot overflow u64 below).
    if digits.len() > 10 {
        return Err(Error::InvalidPath);
    }
    let value = digits
        .iter()
        .fold(0u64, |acc, d| acc * 10 + u64::from(d - b'0'));
    if value >= u64::from(HARDENED_OFFSET) {
        return Err(Error::InvalidPath);
    }
    let index = value as u32;
    Ok(if hardened {
        index + HARDENED_OFFSET
    } else {
        index
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basics() {
        assert_eq!(
            parse_derivation_path("m/44'/60h/0'/0/0").unwrap(),
            vec![
                HARDENED_OFFSET + 44,
                HARDENED_OFFSET + 60,
                HARDENED_OFFSET,
                0,
                0
            ]
        );
        for bad in ["m", "m/", "m/01", "m/1''", "m/2147483648", "M/0", "m/0/"] {
            assert_eq!(parse_derivation_path(bad), Err(Error::InvalidPath), "{bad}");
        }
    }
}
