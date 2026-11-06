//! Field / FFT micro-benchmarks (wasm32 vs native), to decide how to speed up the prover's
//! H witness map. `mont32_mul` is a candidate 8×32-bit-limb Montgomery multiply; with
//! R = 2^256 it uses the same Montgomery representation as ark-ff's 4×64-bit `Fr`.
use ark_bn254::Fr;
use ark_ff::{BigInt, Field, UniformRand};
use ark_poly::{EvaluationDomain, Radix2EvaluationDomain};
use std::hint::black_box;

const P: [u32; 8] = [0xf0000001, 0x43e1f593, 0x79b97091, 0x2833e848, 0x8181585d, 0xb85045b6, 0xe131a029, 0x30644e72];
const INV: u32 = 0xefffffff; // -p^-1 mod 2^32

#[inline(always)]
fn to32(a: &Fr) -> [u32; 8] {
    let l = a.0.0;
    [l[0] as u32, (l[0] >> 32) as u32, l[1] as u32, (l[1] >> 32) as u32, l[2] as u32, (l[2] >> 32) as u32, l[3] as u32, (l[3] >> 32) as u32]
}
#[inline(always)]
fn from32(t: &[u32; 8]) -> Fr {
    let l = |i: usize| t[2 * i] as u64 | (t[2 * i + 1] as u64) << 32;
    Fr::new_unchecked(BigInt([l(0), l(1), l(2), l(3)]))
}

/// CIOS Montgomery multiply, 8×u32 limbs; p < 2^254, so the "no-carry" variant applies.
#[inline(always)]
pub fn mont32_mul(a: &[u32; 8], b: &[u32; 8]) -> [u32; 8] {
    let mut t = [0u32; 8];
    for i in 0..8 {
        let bi = b[i] as u64;
        let mut s = t[0] as u64 + a[0] as u64 * bi;
        let mut c = s >> 32;
        let m = (s as u32).wrapping_mul(INV) as u64;
        let mut c2 = (s as u32 as u64 + m * P[0] as u64) >> 32;
        for j in 1..8 {
            s = t[j] as u64 + a[j] as u64 * bi + c;
            c = s >> 32;
            let s2 = (s as u32) as u64 + m * P[j] as u64 + c2;
            c2 = s2 >> 32;
            t[j - 1] = s2 as u32;
        }
        t[7] = (c + c2) as u32;
    }
    // t < 2p: subtract p once if needed
    let mut ge = true;
    for j in (0..8).rev() {
        if t[j] != P[j] {
            ge = t[j] > P[j];
            break;
        }
    }
    if ge {
        let mut borrow = 0i64;
        for j in 0..8 {
            let d = t[j] as i64 - P[j] as i64 - borrow;
            t[j] = d as u32;
            borrow = (d < 0) as i64;
        }
    }
    t
}

pub fn check_mont32() -> bool {
    let mut rng = ark_std::test_rng();
    (0..10_000).all(|_| {
        let (a, b) = (Fr::rand(&mut rng), Fr::rand(&mut rng));
        from32(&mont32_mul(&to32(&a), &to32(&b))) == a * b
    }) && from32(&mont32_mul(&to32(&-Fr::ONE), &to32(&-Fr::ONE))) == Fr::ONE
}

pub fn mul_ark(n: u32) -> Fr {
    let mut x = Fr::from(3u64);
    let y = Fr::from(7u64).inverse().unwrap();
    for _ in 0..n {
        x *= y;
    }
    black_box(x)
}

pub fn mul_32(n: u32) -> Fr {
    let mut x = to32(&Fr::from(3u64));
    let y = to32(&Fr::from(7u64).inverse().unwrap());
    for _ in 0..n {
        x = mont32_mul(&x, &y);
    }
    black_box(from32(&x))
}

pub fn fft_ark(log_n: u32) -> Fr {
    let d = Radix2EvaluationDomain::<Fr>::new(1 << log_n).unwrap();
    let mut v: Vec<Fr> = (0..1u64 << log_n).map(Fr::from).collect();
    d.fft_in_place(&mut v);
    black_box(v[1])
}
