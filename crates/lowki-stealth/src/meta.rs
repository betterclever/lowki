use alloc::string::String;

use crate::Error;
use crate::keys::parse_compressed;

const URI_PREFIX: &str = "st:eth:";

/// ERC-5564 scheme-1 stealth meta-address: two 33-byte compressed secp256k1 keys.
///
/// Text form: `st:eth:0x<spendingPubKey(33)><viewingPubKey(33)>`.
/// Raw form (ERC-6538 registry value): the 66 bytes `spending || viewing`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct StealthMetaAddress {
    spending: [u8; 33],
    viewing: [u8; 33],
}

impl StealthMetaAddress {
    /// Build from two compressed public keys (validated: 0x02/0x03 prefix, on curve).
    pub fn new(spending_pubkey: &[u8; 33], viewing_pubkey: &[u8; 33]) -> Result<Self, Error> {
        parse_compressed(spending_pubkey)?;
        parse_compressed(viewing_pubkey)?;
        Ok(Self {
            spending: *spending_pubkey,
            viewing: *viewing_pubkey,
        })
    }

    /// Parse the raw registry form: 66 bytes (`spending || viewing`), or the 33-byte
    /// single-key form (same key for spending and viewing), as the ScopeLift SDK accepts.
    pub fn from_raw(raw: &[u8]) -> Result<Self, Error> {
        let (s, v) = match raw.len() {
            66 => (&raw[..33], &raw[33..]),
            33 => (raw, raw),
            _ => return Err(Error::InvalidMetaAddress),
        };
        let s: &[u8; 33] = s.try_into().map_err(|_| Error::InvalidMetaAddress)?;
        let v: &[u8; 33] = v.try_into().map_err(|_| Error::InvalidMetaAddress)?;
        Self::new(s, v)
    }

    /// 66-byte raw form (`spending || viewing`), as stored by the ERC-6538 registry.
    pub fn to_raw(&self) -> [u8; 66] {
        let mut out = [0u8; 66];
        out[..33].copy_from_slice(&self.spending);
        out[33..].copy_from_slice(&self.viewing);
        out
    }

    /// Parse `st:eth:0x<132 hex>` (hex is case-insensitive; the 66-hex single-key
    /// form is also accepted). Any other chain tag or missing `0x` is rejected.
    pub fn parse_uri(uri: &str) -> Result<Self, Error> {
        let rest = uri
            .strip_prefix(URI_PREFIX)
            .ok_or(Error::InvalidMetaAddress)?;
        let hex = rest.strip_prefix("0x").ok_or(Error::InvalidMetaAddress)?;
        let mut buf = [0u8; 66];
        let n = decode_hex(hex, &mut buf)?;
        Self::from_raw(&buf[..n])
    }

    /// `st:eth:0x...` with lowercase hex.
    pub fn to_uri(&self) -> String {
        let mut s = String::with_capacity(URI_PREFIX.len() + 2 + 132);
        s.push_str(URI_PREFIX);
        s.push_str("0x");
        for b in self.to_raw() {
            s.push(char::from_digit((b >> 4) as u32, 16).unwrap_or('0'));
            s.push(char::from_digit((b & 0xf) as u32, 16).unwrap_or('0'));
        }
        s
    }

    /// Spending public key (33 bytes, compressed).
    pub fn spending_pubkey(&self) -> &[u8; 33] {
        &self.spending
    }

    /// Viewing public key (33 bytes, compressed).
    pub fn viewing_pubkey(&self) -> &[u8; 33] {
        &self.viewing
    }
}

impl core::str::FromStr for StealthMetaAddress {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self, Error> {
        Self::parse_uri(s)
    }
}

fn decode_hex(hex: &str, out: &mut [u8; 66]) -> Result<usize, Error> {
    let b = hex.as_bytes();
    if !b.len().is_multiple_of(2) || b.len() / 2 > out.len() {
        return Err(Error::InvalidMetaAddress);
    }
    for (i, pair) in b.as_chunks::<2>().0.iter().enumerate() {
        let hi = (pair[0] as char)
            .to_digit(16)
            .ok_or(Error::InvalidMetaAddress)?;
        let lo = (pair[1] as char)
            .to_digit(16)
            .ok_or(Error::InvalidMetaAddress)?;
        out[i] = ((hi << 4) | lo) as u8;
    }
    Ok(b.len() / 2)
}
