//! Shamir secret sharing over GF(2^8), byte-wise, format-compatible with Privy's
//! [`shamir-secret-sharing`](https://github.com/privy-io/shamir-secret-sharing) 0.0.4.
//!
//! * Field: GF(2^8) with the AES polynomial x^8 + x^4 + x^3 + x + 1 (0x11b). Privy's
//!   library uses log/exp tables with generator 0xe5; this crate computes the same field
//!   operations without tables (constant time), and the tests check every product and
//!   quotient against Privy's tables.
//! * Share layout: `y_0 .. y_{len-1} || x`, i.e. one y-byte per secret byte followed by one
//!   trailing x-coordinate byte (`x ∈ 1..=255`).
//! * [`split`] consumes randomness in exactly Privy's order (255 bytes for the x-coordinate
//!   shuffle, then `threshold - 1` coefficient bytes per secret byte), so the same random
//!   byte stream gives byte-identical shares.
//! * **Unlike Privy's `combine`,** [`combine`] takes the threshold and rejects fewer shares
//!   than that, and it rejects x = 0 as well as duplicate x-coordinates. With too few shares
//!   Privy's `combine` silently returns a wrong secret.
//!
//! Secrets, shares and every intermediate buffer are zeroized. `Debug` never prints share
//! bytes.

#![no_std]
#![forbid(unsafe_code)]

extern crate alloc;

use alloc::vec;
use alloc::vec::Vec;
use core::fmt;
use rand_core::{CryptoRng, RngCore};
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

/// Maximum number of shares (x-coordinates are the 255 non-zero bytes).
pub const MAX_SHARES: usize = 255;

/// Errors. Messages never contain share or secret bytes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Error {
    /// The secret is empty.
    EmptySecret,
    /// `n` is outside `2..=255`.
    InvalidShareCount,
    /// `t` is outside `2..=255`, or `t > n`.
    InvalidThreshold,
    /// `combine` got fewer shares than the threshold.
    TooFewShares,
    /// `combine` got more than 255 shares.
    TooManyShares,
    /// A share is shorter than 2 bytes.
    ShareTooShort,
    /// The shares do not all have the same length.
    LengthMismatch,
    /// Two shares have the same x-coordinate.
    DuplicateX,
    /// A share has x-coordinate 0 (never produced by `split`).
    ZeroX,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Error::EmptySecret => "secret cannot be empty",
            Error::InvalidShareCount => "shares must be at least 2 and at most 255",
            Error::InvalidThreshold => "threshold must be at least 2, at most 255 and at most the share count",
            Error::TooFewShares => "fewer shares than the threshold",
            Error::TooManyShares => "more than 255 shares",
            Error::ShareTooShort => "each share must be at least 2 bytes",
            Error::LengthMismatch => "all shares must have the same byte length",
            Error::DuplicateX => "duplicate share x-coordinate",
            Error::ZeroX => "share x-coordinate is zero",
        })
    }
}

impl core::error::Error for Error {}

/// One share: `y bytes || x`. Zeroized on drop; `Debug` shows only `x` and the length.
#[derive(Clone, PartialEq, Eq, Zeroize, ZeroizeOnDrop)]
pub struct Share(Vec<u8>);

impl Share {
    /// Wraps raw share bytes (Privy format). Fails if shorter than 2 bytes or if x = 0.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, Error> {
        if bytes.len() < 2 {
            return Err(Error::ShareTooShort);
        }
        if bytes[bytes.len() - 1] == 0 {
            return Err(Error::ZeroX);
        }
        Ok(Share(bytes.to_vec()))
    }

    /// Takes ownership of raw share bytes without copying (same checks as `from_bytes`).
    /// On error the input is zeroized.
    pub fn from_vec(mut bytes: Vec<u8>) -> Result<Self, Error> {
        let check = if bytes.len() < 2 {
            Err(Error::ShareTooShort)
        } else if bytes[bytes.len() - 1] == 0 {
            Err(Error::ZeroX)
        } else {
            Ok(())
        };
        match check {
            Ok(()) => Ok(Share(bytes)),
            Err(e) => {
                bytes.zeroize();
                Err(e)
            }
        }
    }

    /// The share bytes in Privy's format.
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }

    /// The x-coordinate (last byte).
    pub fn x(&self) -> u8 {
        self.0[self.0.len() - 1]
    }

    /// Length of the secret this share belongs to.
    pub fn secret_len(&self) -> usize {
        self.0.len() - 1
    }
}

impl fmt::Debug for Share {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Share(x={}, len={}, ..)", self.x(), self.0.len())
    }
}

// ---------------------------------------------------------------------------------------
// GF(2^8), AES polynomial, constant time.

/// a * b mod x^8 + x^4 + x^3 + x + 1, without branches on secret data.
#[inline]
pub(crate) fn gf_mul(a: u8, b: u8) -> u8 {
    let mut a = a;
    let mut b = b;
    let mut p = 0u8;
    for _ in 0..8 {
        p ^= a & 0u8.wrapping_sub(b & 1);
        let hi = 0u8.wrapping_sub(a >> 7);
        a = (a << 1) ^ (0x1b & hi);
        b >>= 1;
    }
    p
}

/// a^-1 = a^254 (0 maps to 0).
#[inline]
pub(crate) fn gf_inv(a: u8) -> u8 {
    // 254 = 0b11111110
    let a2 = gf_mul(a, a);
    let a4 = gf_mul(a2, a2);
    let a8 = gf_mul(a4, a4);
    let a16 = gf_mul(a8, a8);
    let a32 = gf_mul(a16, a16);
    let a64 = gf_mul(a32, a32);
    let a128 = gf_mul(a64, a64);
    let r = gf_mul(a128, a64);
    let r = gf_mul(r, a32);
    let r = gf_mul(r, a16);
    let r = gf_mul(r, a8);
    let r = gf_mul(r, a4);
    gf_mul(r, a2)
}

/// a / b. Callers guarantee b != 0 (x-coordinates are distinct).
#[inline]
pub(crate) fn gf_div(a: u8, b: u8) -> u8 {
    gf_mul(a, gf_inv(b))
}

/// Exposed for the tests that compare against Privy's tables.
#[doc(hidden)]
pub mod field {
    /// GF(2^8) product.
    pub fn mul(a: u8, b: u8) -> u8 {
        super::gf_mul(a, b)
    }
    /// GF(2^8) quotient (b != 0).
    pub fn div(a: u8, b: u8) -> u8 {
        super::gf_div(a, b)
    }
}

// ---------------------------------------------------------------------------------------

/// Privy's x-coordinate shuffle: [1..=255] permuted with 255 random bytes (`j = r[i] % 255`).
/// Biased, but Shamir's security does not depend on the choice of x-coordinates; it is kept
/// for byte compatibility.
fn new_coordinates<R: RngCore + CryptoRng + ?Sized>(rng: &mut R) -> Zeroizing<[u8; 255]> {
    let mut coords = Zeroizing::new([0u8; 255]);
    for (i, c) in coords.iter_mut().enumerate() {
        *c = (i + 1) as u8;
    }
    let mut idx = Zeroizing::new([0u8; 255]);
    rng.fill_bytes(&mut idx[..]);
    for i in 0..255 {
        let j = (idx[i] % 255) as usize;
        coords.swap(i, j);
    }
    coords
}

/// Horner evaluation at x != 0, highest coefficient first.
fn evaluate(coeffs: &[u8], x: u8) -> u8 {
    let degree = coeffs.len() - 1;
    let mut r = coeffs[degree];
    for i in (0..degree).rev() {
        r = gf_mul(r, x) ^ coeffs[i];
    }
    r
}

/// Splits `secret` into `n` shares, any `t` of which reconstruct it.
///
/// `2 <= t <= n <= 255`. Randomness is drawn from `rng` in Privy's order, so a replayed byte
/// stream reproduces Privy's shares exactly.
pub fn split<R: RngCore + CryptoRng + ?Sized>(
    secret: &[u8],
    n: usize,
    t: usize,
    rng: &mut R,
) -> Result<Vec<Share>, Error> {
    if secret.is_empty() {
        return Err(Error::EmptySecret);
    }
    if !(2..=MAX_SHARES).contains(&n) {
        return Err(Error::InvalidShareCount);
    }
    if !(2..=MAX_SHARES).contains(&t) || t > n {
        return Err(Error::InvalidThreshold);
    }
    let len = secret.len();
    let coords = new_coordinates(rng);
    let mut shares: Vec<Share> = (0..n)
        .map(|j| {
            let mut v = vec![0u8; len + 1];
            v[len] = coords[j];
            Share(v)
        })
        .collect();
    let mut coeffs = Zeroizing::new(vec![0u8; t]);
    for (i, &byte) in secret.iter().enumerate() {
        coeffs[0] = byte;
        rng.fill_bytes(&mut coeffs[1..]);
        for share in shares.iter_mut() {
            let x = share.0[len];
            share.0[i] = evaluate(&coeffs, x);
        }
    }
    Ok(shares)
}

/// Reconstructs the secret from at least `threshold` shares.
///
/// Rejects: `threshold` outside `2..=255`, fewer than `threshold` shares, more than 255
/// shares, shares shorter than 2 bytes, mixed lengths, x = 0 and duplicate x-coordinates.
/// Like Privy's `combine`, all given shares are interpolated (extra shares from the same
/// polynomial give the same result). Shares from different splits are not detected.
pub fn combine<S: AsRef<[u8]>>(shares: &[S], threshold: usize) -> Result<Zeroizing<Vec<u8>>, Error> {
    if !(2..=MAX_SHARES).contains(&threshold) {
        return Err(Error::InvalidThreshold);
    }
    if shares.len() < threshold {
        return Err(Error::TooFewShares);
    }
    if shares.len() > MAX_SHARES {
        return Err(Error::TooManyShares);
    }
    let share_len = shares[0].as_ref().len();
    for s in shares {
        let s = s.as_ref();
        if s.len() < 2 {
            return Err(Error::ShareTooShort);
        }
        if s.len() != share_len {
            return Err(Error::LengthMismatch);
        }
    }
    let k = shares.len();
    let mut xs = Zeroizing::new(vec![0u8; k]);
    let mut seen = [false; 256];
    for (i, s) in shares.iter().enumerate() {
        let x = s.as_ref()[share_len - 1];
        if x == 0 {
            return Err(Error::ZeroX);
        }
        if seen[x as usize] {
            return Err(Error::DuplicateX);
        }
        seen[x as usize] = true;
        xs[i] = x;
    }
    seen.zeroize();

    // Lagrange basis at 0: l_i = Π_{j≠i} x_j / (x_i ⊕ x_j). Same for every byte position.
    let mut basis = Zeroizing::new(vec![0u8; k]);
    for i in 0..k {
        let mut b = 1u8;
        for j in 0..k {
            if i != j {
                b = gf_mul(b, gf_div(xs[j], xs[i] ^ xs[j]));
            }
        }
        basis[i] = b;
    }

    let secret_len = share_len - 1;
    let mut secret = Zeroizing::new(vec![0u8; secret_len]);
    for (pos, out) in secret.iter_mut().enumerate() {
        let mut acc = 0u8;
        for (i, s) in shares.iter().enumerate() {
            acc ^= gf_mul(s.as_ref()[pos], basis[i]);
        }
        *out = acc;
    }
    Ok(secret)
}

/// Re-derives the share at x-coordinate `x` (1..=255) of the same polynomial from at least
/// `threshold` shares, with the same checks as [`combine`]. Lowki uses it to re-create the
/// passkey share P from S and R after recovery, so a new passkey can wrap the same P.
pub fn interpolate_share<S: AsRef<[u8]>>(shares: &[S], threshold: usize, x: u8) -> Result<Share, Error> {
    if x == 0 {
        return Err(Error::ZeroX);
    }
    // Validate exactly like combine (threshold, counts, lengths, x-coordinates).
    combine(shares, threshold)?;
    let k = shares.len();
    let share_len = shares[0].as_ref().len();
    let xs: Zeroizing<Vec<u8>> = Zeroizing::new(shares.iter().map(|s| s.as_ref()[share_len - 1]).collect());
    // l_i(x) = Π_{j≠i} (x ⊕ x_j) / (x_i ⊕ x_j). If x equals some x_i this yields that share.
    let mut basis = Zeroizing::new(vec![0u8; k]);
    for i in 0..k {
        let mut b = 1u8;
        for j in 0..k {
            if i != j {
                b = gf_mul(b, gf_div(x ^ xs[j], xs[i] ^ xs[j]));
            }
        }
        basis[i] = b;
    }
    let mut out = vec![0u8; share_len];
    out[share_len - 1] = x;
    for (pos, o) in out[..share_len - 1].iter_mut().enumerate() {
        let mut acc = 0u8;
        for (i, s) in shares.iter().enumerate() {
            acc ^= gf_mul(s.as_ref()[pos], basis[i]);
        }
        *o = acc;
    }
    Ok(Share(out))
}

/// Convenience: [`combine`] over [`Share`]s.
pub fn combine_shares(shares: &[Share], threshold: usize) -> Result<Zeroizing<Vec<u8>>, Error> {
    let refs: Vec<&[u8]> = shares.iter().map(Share::as_bytes).collect();
    combine(&refs, threshold)
}

impl AsRef<[u8]> for Share {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}
