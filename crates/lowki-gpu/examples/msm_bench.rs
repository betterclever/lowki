//! GPU MSM timing + check vs ark: `cargo run --release --example msm_bench -- <n> [window_bits]`
//! (set `G2=1` for a G2 MSM).
use std::time::Instant;
use ark_bn254::{Fr, G1Projective};
use ark_ec::{CurveGroup, VariableBaseMSM};
use ark_ff::UniformRand;
use lowki_gpu::*;
fn main() {
    let n: usize = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(1);
    let c: Option<u32> = std::env::args().nth(2).and_then(|s| s.parse().ok());
    let t = Instant::now();
    let g = pollster::block_on(Gpu::new()).unwrap();
    println!("setup {:?}", t.elapsed());
    let mut rng = ark_std::test_rng();
    let p = G1Projective::rand(&mut rng);
    let bases = G1Projective::normalize_batch(&(0..n).map(|i| p * Fr::from(i as u64 + 1)).collect::<Vec<_>>());
    let sc: Vec<Fr> = (0..n).map(|_| Fr::rand(&mut rng)).collect();
    let mut gb = g.upload_bases(&bases).unwrap();
    if let Some(c) = c { gb.set_window_bits(c); }
    if std::env::var("G2").is_ok() {
        let q = ark_bn254::G2Projective::rand(&mut rng);
        let b2 = ark_bn254::G2Projective::normalize_batch(&(0..n).map(|i| q * Fr::from(i as u64 + 1)).collect::<Vec<_>>());
        let gb = g.upload_bases(&b2).unwrap();
        let r = pollster::block_on(g.msm_g2(&gb, &sc));
        println!("g2 ok={:?}", r.map(|r| r.into_affine() == ark_bn254::G2Projective::msm(&b2, &sc).unwrap().into_affine()));
        return;
    }
    for _ in 0..3 {
        let t = Instant::now();
        let r = pollster::block_on(g.msm_g1(&gb, &sc));
        println!("gpu {:?} c={} ok={:?}", t.elapsed(), gb.window_bits(), r.as_ref().map(|r| r.into_affine() == G1Projective::msm(&bases, &sc).unwrap().into_affine()));
    }
}
