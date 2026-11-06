//! GPU differential tests: WGSL field ops and GPU MSMs vs arkworks, compared as affine points.
//! Without a usable GPU every test prints `SKIP` and passes (the CPU path is the fallback).
#![cfg(not(target_arch = "wasm32"))]

use std::sync::OnceLock;
use std::time::Instant;

use ark_bn254::{Fq, Fr, G1Affine, G1Projective, G2Affine, G2Projective};
use ark_ec::{AffineRepr, CurveGroup, VariableBaseMSM};
use ark_ff::{UniformRand, Zero};
use lowki_gpu::*;
use rand_chacha::ChaCha20Rng;
use rand_chacha::rand_core::SeedableRng;

fn gpu() -> Option<&'static Gpu> {
    static G: OnceLock<Option<Gpu>> = OnceLock::new();
    G.get_or_init(|| {
        let t = Instant::now();
        match pollster::block_on(Gpu::new()) {
            Ok(g) => {
                let i = g.adapter_info();
                eprintln!(
                    "GPU: {} ({:?}, {:?}, driver {} {}), maxStorageBufferBindingSize {} MiB, setup {:?}",
                    i.name,
                    i.backend,
                    i.device_type,
                    i.driver,
                    i.driver_info,
                    g.max_storage_binding() >> 20,
                    t.elapsed()
                );
                Some(g)
            }
            Err(e) => {
                eprintln!("SKIP: {e}");
                None
            }
        }
    })
    .as_ref()
}

macro_rules! need_gpu {
    () => {
        match gpu() {
            Some(g) => g,
            None => {
                eprintln!("SKIP (no GPU)");
                return;
            }
        }
    };
}

#[test]
fn field_ops_match_ark() {
    let g = need_gpu!();
    let mut rng = ChaCha20Rng::seed_from_u64(1);
    let m1 = -Fq::from(1u64);
    let mut pairs: Vec<(Fq, Fq)> = (0..4096).map(|_| (Fq::rand(&mut rng), Fq::rand(&mut rng))).collect();
    pairs.extend([
        (Fq::zero(), Fq::zero()),
        (m1, m1),
        (m1, Fq::from(1u64)),
        (Fq::zero(), m1),
        (Fq::from(1u64), m1),
        (Fq::from(2u64), m1),
    ]);
    let out = pollster::block_on(g.field_selftest(&pairs)).unwrap();
    for ((a, b), [m, s, d]) in pairs.iter().zip(&out) {
        assert_eq!(*m, a * b);
        assert_eq!(*s, a + b);
        assert_eq!(*d, a - b);
    }
}

fn point_case<P: GpuCurve>(g: &Gpu, rng: &mut ChaCha20Rng)
where
    P::BaseField: lowki_gpu::field::GpuField,
{
    use ark_ec::short_weierstrass::{Affine, Projective};
    let mut triples = Vec::new();
    let mut want = Vec::new();
    for i in 0..300 {
        let a = Projective::<P>::rand(rng).into_affine();
        let b = match i % 4 {
            0 => a,
            1 => -a,
            _ => Projective::<P>::rand(rng).into_affine(),
        };
        let c = Projective::<P>::rand(rng).into_affine();
        let (za, zb) = (P::BaseField::rand(rng), P::BaseField::rand(rng));
        let pa = if i % 7 == 3 {
            [P::BaseField::zero(), P::BaseField::from(1u64), P::BaseField::zero()]
        } else {
            [a.x * za, a.y * za, za]
        };
        let q_inf = i % 5 == 2;
        let pb = if q_inf {
            [P::BaseField::zero(), P::BaseField::from(1u64), P::BaseField::zero()]
        } else if i % 5 == 4 {
            [b.x, b.y, P::BaseField::from(1u64)]
        } else {
            [b.x * zb, b.y * zb, zb]
        };
        let bq: Projective<P> = if q_inf { Projective::zero() } else { b.into() };
        let ap: Projective<P> = if i % 7 == 3 { Projective::zero() } else { a.into() };
        want.push([ap + bq, ap + c, ap + ap, ap + ap]);
        triples.push((pa, pb, c));
    }
    let got = pollster::block_on(g.point_selftest::<P>(&triples)).unwrap();
    for (i, (g4, w4)) in got.iter().zip(&want).enumerate() {
        for k in 0..4 {
            assert_eq!(g4[k].into_affine(), w4[k].into_affine(), "{:?} case {i} op {k}", P::KIND);
        }
    }
    let _ = Affine::<P>::identity();
}

#[test]
fn point_formulas_match_ark() {
    let g = need_gpu!();
    let mut rng = ChaCha20Rng::seed_from_u64(12);
    point_case::<G1>(g, &mut rng);
    point_case::<G2>(g, &mut rng);
}

fn random_g1(n: usize, rng: &mut ChaCha20Rng) -> Vec<G1Affine> {
    // n random points cheaply: multiples of a random point plus a few independent ones
    let p = G1Projective::rand(rng);
    let mut acc = p;
    let mut v = Vec::with_capacity(n);
    for i in 0..n {
        v.push(acc);
        acc += if i % 97 == 0 { G1Projective::rand(rng) } else { p };
    }
    G1Projective::normalize_batch(&v)
}

fn random_g2(n: usize, rng: &mut ChaCha20Rng) -> Vec<G2Affine> {
    let p = G2Projective::rand(rng);
    let mut acc = p;
    let mut v = Vec::with_capacity(n);
    for i in 0..n {
        v.push(acc);
        acc += if i % 97 == 0 { G2Projective::rand(rng) } else { p };
    }
    G2Projective::normalize_batch(&v)
}

#[derive(Clone, Copy, Debug)]
enum Scalars {
    Random,
    Small,
    Zero,
    MinusOne,
    Mixed,
}

fn scalars(n: usize, kind: Scalars, rng: &mut ChaCha20Rng) -> Vec<Fr> {
    (0..n)
        .map(|i| match kind {
            Scalars::Random => Fr::rand(rng),
            Scalars::Small => Fr::from((i % 7) as u64),
            Scalars::Zero => Fr::zero(),
            Scalars::MinusOne => -Fr::from(1u64),
            Scalars::Mixed => match i % 5 {
                0 => Fr::zero(),
                1 => Fr::from(1u64),
                2 => -Fr::from(1u64),
                3 => Fr::from(u64::MAX),
                _ => Fr::rand(rng),
            },
        })
        .collect()
}

fn check_g1(g: &Gpu, bases: &[G1Affine], sc: &[Fr], what: &str) {
    let gb = g.upload_bases(bases).unwrap();
    let t = Instant::now();
    let got = pollster::block_on(g.msm_g1(&gb, sc)).unwrap();
    let dt = t.elapsed();
    let want = G1Projective::msm(&bases[..sc.len()], sc).unwrap();
    assert_eq!(got.into_affine(), want.into_affine(), "G1 {what}");
    eprintln!("G1 ok: {what} (c={}, gpu {dt:?})", gb.window_bits());
}

fn check_g2(g: &Gpu, bases: &[G2Affine], sc: &[Fr], what: &str) {
    let gb = g.upload_bases(bases).unwrap();
    let t = Instant::now();
    let got = pollster::block_on(g.msm_g2(&gb, sc)).unwrap();
    let dt = t.elapsed();
    let want = G2Projective::msm(&bases[..sc.len()], sc).unwrap();
    assert_eq!(got.into_affine(), want.into_affine(), "G2 {what}");
    eprintln!("G2 ok: {what} (c={}, gpu {dt:?})", gb.window_bits());
}

#[test]
fn g1_msm_matches_ark() {
    let g = need_gpu!();
    let mut rng = ChaCha20Rng::seed_from_u64(2);
    for &n in &[1usize, 2, 3, 255, 256, 1000, 1 << 16] {
        let mut bases = random_g1(n, &mut rng);
        for kind in [Scalars::Random, Scalars::Mixed, Scalars::Small, Scalars::Zero, Scalars::MinusOne] {
            let sc = scalars(n, kind, &mut rng);
            check_g1(g, &bases, &sc, &format!("n={n} {kind:?}"));
        }
        if n >= 3 {
            // edge cases: infinity among bases, repeated points, P and -P, r-1 and 0 scalars
            bases[0] = G1Affine::identity();
            bases[1] = bases[2];
            if n > 4 {
                bases[3] = -bases[4];
            }
            let mut sc = scalars(n, Scalars::Random, &mut rng);
            sc[1] = -Fr::from(1u64);
            sc[2] = Fr::from(1u64); // (r-1)·P + 1·P = 0
            if n > 4 {
                sc[3] = Fr::from(5u64);
                sc[4] = Fr::from(5u64);
            }
            check_g1(g, &bases, &sc, &format!("n={n} edge (inf, repeated, cancelling)"));
            // all bases the same point
            let same = vec![bases[2]; n];
            check_g1(g, &same, &scalars(n, Scalars::Random, &mut rng), &format!("n={n} all-equal bases"));
        }
    }
    // the generator, scalar 1 and r-1
    let g0 = G1Affine::generator();
    check_g1(g, &[g0], &[Fr::from(1u64)], "1·G");
    check_g1(g, &[g0], &[-Fr::from(1u64)], "(r-1)·G");
    check_g1(g, &[g0, g0], &[-Fr::from(1u64), Fr::from(1u64)], "(r-1)·G + G = 0");
    check_g1(g, &[G1Affine::identity()], &[Fr::from(3u64)], "3·inf");
    // fewer scalars than bases
    let b = random_g1(100, &mut rng);
    check_g1(g, &b, &scalars(60, Scalars::Random, &mut rng), "60 scalars, 100 bases");
}

#[test]
fn g1_window_sizes() {
    let g = need_gpu!();
    let mut rng = ChaCha20Rng::seed_from_u64(3);
    let bases = random_g1(3000, &mut rng);
    let sc = scalars(3000, Scalars::Mixed, &mut rng);
    let want = G1Projective::msm(&bases, &sc).unwrap().into_affine();
    let mut gb = g.upload_bases(&bases).unwrap();
    for c in 2..=16 {
        gb.set_window_bits(c);
        let got = pollster::block_on(g.msm_g1(&gb, &sc)).unwrap();
        assert_eq!(got.into_affine(), want, "c={c}");
    }
}

#[test]
fn g2_msm_matches_ark() {
    let g = need_gpu!();
    let mut rng = ChaCha20Rng::seed_from_u64(4);
    for &n in &[1usize, 2, 3, 255, 256, 1000, 1 << 15] {
        let mut bases = random_g2(n, &mut rng);
        for kind in [Scalars::Random, Scalars::Mixed, Scalars::Zero, Scalars::MinusOne] {
            let sc = scalars(n, kind, &mut rng);
            check_g2(g, &bases, &sc, &format!("n={n} {kind:?}"));
        }
        if n >= 5 {
            bases[0] = G2Affine::identity();
            bases[1] = bases[2];
            bases[3] = -bases[4];
            let mut sc = scalars(n, Scalars::Random, &mut rng);
            sc[1] = -Fr::from(1u64);
            sc[2] = Fr::from(1u64);
            sc[3] = Fr::from(9u64);
            sc[4] = Fr::from(9u64);
            check_g2(g, &bases, &sc, &format!("n={n} edge"));
        }
    }
}

#[test]
fn batch_of_jobs() {
    let g = need_gpu!();
    let mut rng = ChaCha20Rng::seed_from_u64(5);
    let b1 = random_g1(5000, &mut rng);
    let b2 = random_g2(700, &mut rng);
    let (g1b, g2b) = (g.upload_bases(&b1).unwrap(), g.upload_bases(&b2).unwrap());
    let s1 = scalars(5000, Scalars::Mixed, &mut rng);
    let s2 = scalars(700, Scalars::Random, &mut rng);
    let (k1, k2) = (BigInts::from_fr(&s1), BigInts::from_fr(&s2));
    let out = pollster::block_on(
        g.submit(&[MsmJob::G1(&g1b, &k1.0), MsmJob::G2(&g2b, &k2.0), MsmJob::G1(&g1b, &k1.0[..10])])
            .unwrap()
            .wait(),
    )
    .unwrap();
    assert_eq!(out[0].g1().unwrap().into_affine(), G1Projective::msm(&b1, &s1).unwrap().into_affine());
    assert_eq!(out[1].g2().unwrap().into_affine(), G2Projective::msm(&b2, &s2).unwrap().into_affine());
    assert_eq!(
        out[2].g1().unwrap().into_affine(),
        G1Projective::msm(&b1[..10], &s1[..10]).unwrap().into_affine()
    );
}

#[test]
fn rejects_bad_input() {
    let g = need_gpu!();
    let mut rng = ChaCha20Rng::seed_from_u64(6);
    let b = g.upload_bases(&random_g1(3, &mut rng)).unwrap();
    let four = BigInts::from_fr(&[Fr::from(1u64); 4]);
    assert!(matches!(g.submit(&[MsmJob::G1(&b, &four.0)]), Err(GpuError::Input(_))));
    let unreduced = [ark_ff::BigInteger256::new([u64::MAX; 4])];
    assert!(matches!(g.submit(&[MsmJob::G1(&b, &unreduced)]), Err(GpuError::Input(_))));
}
