//! BIP-39 (English wordlist, empty passphrase), matching `@scure/bip39`.

use bip39::{Language, Mnemonic};
use zeroize::{Zeroize, Zeroizing};

use crate::Error;

/// `@scure/bip39` splits the NFKD-normalised phrase on single `' '` characters and looks
/// each word up exactly. For the English wordlist that means: only `a-z` and single
/// spaces, no leading/trailing/double spaces. (`bip39::Mnemonic::parse_in_normalized`
/// splits on any whitespace, so this check comes first.)
///
/// Deviation: non-ASCII input is always rejected; scure would NFKD-normalise it first
/// (e.g. full-width letters). See README.
fn well_formed(s: &str) -> bool {
    !s.is_empty()
        && s.bytes().all(|b| b.is_ascii_lowercase() || b == b' ')
        && s.split(' ').all(|w| !w.is_empty())
}

fn parse(s: &str) -> Result<Mnemonic, Error> {
    if !well_formed(s) {
        return Err(Error::InvalidMnemonic);
    }
    Mnemonic::parse_in_normalized(Language::English, s).map_err(|_| Error::InvalidMnemonic)
}

/// 32 bytes of entropy -> 24-word English mnemonic (single-space separated).
pub fn entropy_to_mnemonic(entropy: &[u8; 32]) -> Zeroizing<String> {
    let m = Mnemonic::from_entropy_in(Language::English, entropy)
        .expect("32 bytes is a valid BIP-39 entropy length");
    let mut out = Zeroizing::new(String::with_capacity(24 * 9));
    for (i, w) in m.words().enumerate() {
        if i > 0 {
            out.push(' ');
        }
        out.push_str(w);
    }
    out
}

/// Same result as `@scure/bip39` `validateMnemonic(m, english)` (12/15/18/21/24 words).
pub fn validate_mnemonic(mnemonic: &str) -> bool {
    parse(mnemonic).is_ok()
}

/// 24-word mnemonic -> its 32 bytes of entropy. Shorter (valid) phrases are rejected,
/// since Lowki wraps exactly 32 bytes.
pub fn mnemonic_to_entropy(mnemonic: &str) -> Result<Zeroizing<[u8; 32]>, Error> {
    let m = parse(mnemonic)?;
    let (mut buf, len) = m.to_entropy_array();
    let res = if len == 32 {
        let mut out = Zeroizing::new([0u8; 32]);
        out.copy_from_slice(&buf[..32]);
        Ok(out)
    } else {
        Err(Error::InvalidMnemonic)
    };
    buf.zeroize();
    res
}

/// Mnemonic (any valid length) -> 64-byte BIP-39 seed with empty passphrase.
/// Like `mnemonicToSeedSync`, but the phrase must validate first.
pub fn mnemonic_to_seed(mnemonic: &str) -> Result<Zeroizing<[u8; 64]>, Error> {
    let m = parse(mnemonic)?;
    Ok(Zeroizing::new(m.to_seed_normalized("")))
}

/// 32 bytes of entropy -> 64-byte seed of the corresponding 24-word mnemonic.
pub fn entropy_to_seed(entropy: &[u8; 32]) -> Zeroizing<[u8; 64]> {
    let m = Mnemonic::from_entropy_in(Language::English, entropy)
        .expect("32 bytes is a valid BIP-39 entropy length");
    Zeroizing::new(m.to_seed_normalized(""))
}
