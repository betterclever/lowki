//! CPU side of the GPU field representation.
//!
//! WGSL has no 64-bit integers, so an Fq element is held as [`LIMBS`] = 20 limbs of
//! [`LIMB_BITS`] = 13 bits, each in a `u32` (260 bits). Products of two limbs are 26 bits, so a
//! CIOS Montgomery step `s + x·y + q·p + carry` stays below 2^28 and never overflows a `u32`
//! (the same trick as the ZPrize 2023 WebGPU MSM entries). Elements are in Montgomery form with
//! `R = 2^260` (arkworks uses `2^256`), always fully reduced (`< p`).

use ark_bn254::{Fq, Fq2, Fr};
use ark_ff::{BigInteger256, Field, PrimeField, Zero};

/// Limbs per base-field element.
pub const LIMBS: usize = 20;
/// Bits per limb.
pub const LIMB_BITS: u32 = 13;
pub(crate) const MASK: u64 = (1 << LIMB_BITS) - 1;

/// Split a 256-bit integer into 20 little-endian 13-bit limbs.
pub(crate) fn to_limbs(b: &BigInteger256) -> [u32; LIMBS] {
    let mut out = [0u32; LIMBS];
    for (i, o) in out.iter_mut().enumerate() {
        let bit = i as u32 * LIMB_BITS;
        let (w, s) = ((bit / 64) as usize, bit % 64);
        if w >= 4 {
            break;
        }
        let mut v = b.0[w] >> s;
        if s + LIMB_BITS > 64 && w + 1 < 4 {
            v |= b.0[w + 1] << (64 - s);
        }
        *o = (v & MASK) as u32;
    }
    out
}

/// Inverse of [`to_limbs`] (limbs must be normalized, value < 2^256).
pub(crate) fn from_limbs(l: &[u32]) -> BigInteger256 {
    let mut out = [0u64; 4];
    for (i, &x) in l.iter().enumerate().take(LIMBS) {
        let bit = i as u32 * LIMB_BITS;
        let (w, s) = ((bit / 64) as usize, bit % 64);
        let x = u64::from(x);
        if w < 4 {
            out[w] |= x << s;
        }
        if s + LIMB_BITS > 64 && w + 1 < 4 {
            out[w + 1] |= x >> (64 - s);
        }
    }
    BigInteger256::new(out)
}

/// `2^260 mod p` as an `Fq` (the GPU Montgomery factor) and its inverse.
fn r260() -> (Fq, Fq) {
    let r = Fq::from(2u64).pow([260u64]);
    (r, r.inverse().expect("2^260 is invertible mod p"))
}

/// Field constants baked into the WGSL source.
pub(crate) struct FieldConsts {
    pub p: [u32; LIMBS],
    /// `-p^{-1} mod 2^13`.
    pub n0: u32,
    /// `R mod p` (Montgomery one).
    pub one: [u32; LIMBS],
}

fn consts<F: PrimeField<BigInt = BigInteger256>>() -> FieldConsts {
    let p = F::MODULUS;
    // -p^{-1} mod 2^13 via Newton iteration on the low limb.
    let p0 = p.0[0];
    let mut inv: u64 = 1;
    for _ in 0..6 {
        inv = inv.wrapping_mul(2u64.wrapping_sub(p0.wrapping_mul(inv)));
    }
    let n0 = (inv.wrapping_neg() & MASK) as u32;
    FieldConsts {
        p: to_limbs(&p),
        n0,
        one: to_limbs(&F::from(2u64).pow([260u64]).into_bigint()),
    }
}

pub(crate) fn fq_consts() -> FieldConsts {
    consts::<Fq>()
}

/// The same 20 × 13-bit Montgomery representation (R = 2^260) for the scalar field Fr,
/// used by the QAP / NTT kernels ([`crate::qap`]).
pub(crate) fn fr_consts() -> FieldConsts {
    consts::<Fr>()
}

/// GPU encoding of an Fr element (`x · 2^260 mod r`, 13-bit limbs).
pub(crate) fn fr_words(x: &Fr, r260: &Fr) -> [u32; LIMBS] {
    to_limbs(&(*x * r260).into_bigint())
}

/// A base field the GPU kernels understand (Fq for G1, Fq2 for G2).
pub trait GpuField: Field {
    /// `u32` words per element on the GPU.
    const WORDS: usize;
    /// Append the GPU encoding of `self`, using precomputed `(2^260, 2^-260)`.
    fn push_words(&self, conv: &Conv, out: &mut Vec<u32>);
    /// Decode (the inverse of [`GpuField::push_words`]).
    fn from_words(conv: &Conv, w: &[u32]) -> Option<Self>;
}

/// Cached Montgomery conversion factors.
pub struct Conv {
    r: Fq,
    r_inv: Fq,
}

impl Conv {
    pub fn new() -> Self {
        let (r, r_inv) = r260();
        Self { r, r_inv }
    }
}

impl Default for Conv {
    fn default() -> Self {
        Self::new()
    }
}

impl GpuField for Fq {
    const WORDS: usize = LIMBS;
    fn push_words(&self, conv: &Conv, out: &mut Vec<u32>) {
        out.extend_from_slice(&to_limbs(&(*self * conv.r).into_bigint()));
    }
    fn from_words(conv: &Conv, w: &[u32]) -> Option<Self> {
        if w.len() != LIMBS || w.iter().any(|&x| u64::from(x) > MASK) {
            return None;
        }
        let b = from_limbs(w);
        let f = Fq::from_bigint(b)?;
        if f.is_zero() {
            return Some(f);
        }
        Some(f * conv.r_inv)
    }
}

impl GpuField for Fq2 {
    const WORDS: usize = 2 * LIMBS;
    fn push_words(&self, conv: &Conv, out: &mut Vec<u32>) {
        self.c0.push_words(conv, out);
        self.c1.push_words(conv, out);
    }
    fn from_words(conv: &Conv, w: &[u32]) -> Option<Self> {
        if w.len() != 2 * LIMBS {
            return None;
        }
        Some(Fq2::new(
            Fq::from_words(conv, &w[..LIMBS])?,
            Fq::from_words(conv, &w[LIMBS..])?,
        ))
    }
}

/// Encode an Fq2 constant's limbs for WGSL (`c0` then `c1`).
pub(crate) fn fq2_const_words(x: &Fq2) -> Vec<u32> {
    let conv = Conv::new();
    let mut v = Vec::new();
    x.push_words(&conv, &mut v);
    v
}

#[cfg(test)]
mod tests {
    use super::*;
    use ark_ff::UniformRand;

    /// CPU model of the WGSL Montgomery product (same algorithm, same limb bounds).
    pub(crate) fn mont_mul(x: &[u32; LIMBS], y: &[u32; LIMBS], c: &FieldConsts) -> [u32; LIMBS] {
        let mut s = [0u32; LIMBS];
        for &xi in x.iter() {
            let mut t = s[0] + xi * y[0];
            let q = ((t & MASK as u32) * c.n0) & MASK as u32;
            t += q * c.p[0];
            let mut carry = t >> LIMB_BITS;
            for j in 1..LIMBS {
                let t = s[j]
                    .checked_add(xi * y[j])
                    .and_then(|t| t.checked_add(q * c.p[j]))
                    .and_then(|t| t.checked_add(carry))
                    .expect("no u32 overflow");
                s[j - 1] = t & MASK as u32;
                carry = t >> LIMB_BITS;
            }
            s[LIMBS - 1] = carry;
        }
        // conditional subtract
        let mut d = [0u32; LIMBS];
        let mut br = 0u32;
        for j in 0..LIMBS {
            let t = s[j].wrapping_sub(c.p[j]).wrapping_sub(br);
            d[j] = t & MASK as u32;
            br = t >> 31;
        }
        if br == 0 { d } else { s }
    }

    #[test]
    fn limbs_roundtrip_and_mont_mul_model() {
        let c = fq_consts();
        let conv = Conv::new();
        let mut rng = ark_std::test_rng();
        let mut cases: Vec<(Fq, Fq)> = (0..2000).map(|_| (Fq::rand(&mut rng), Fq::rand(&mut rng))).collect();
        let m1 = -Fq::from(1u64);
        cases.extend([(m1, m1), (Fq::zero(), m1), (Fq::from(1u64), m1)]);
        for (a, b) in cases {
            let mut wa = Vec::new();
            a.push_words(&conv, &mut wa);
            assert_eq!(Fq::from_words(&conv, &wa), Some(a));
            let mut wb = Vec::new();
            b.push_words(&conv, &mut wb);
            let prod = mont_mul(&wa.clone().try_into().unwrap(), &wb.try_into().unwrap(), &c);
            assert_eq!(Fq::from_words(&conv, &prod), Some(a * b));
        }
        assert_eq!(Fq::from_words(&conv, &c.one), Some(Fq::from(1u64)));
    }
}
