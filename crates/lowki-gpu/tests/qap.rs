//! GPU QAP witness map vs the CPU reference (arkworks FFTs, the same steps as lowki-groth16's
//! `CircomReduction`), element by element. Prints `SKIP` without a GPU.
#![cfg(not(target_arch = "wasm32"))]

use std::time::Instant;

use ark_bn254::Fr;
use ark_ff::{PrimeField, UniformRand, Zero};
use ark_poly::{EvaluationDomain, Radix2EvaluationDomain};
use lowki_gpu::*;
use rand_chacha::ChaCha20Rng;
use rand_chacha::rand_core::SeedableRng;

/// snarkjs / ark-circom witness map for given constraint evaluations a, b (c = a ∘ b).
fn reference(a: &[Fr], b: &[Fr]) -> Vec<Fr> {
    let n = a.len();
    let d = Radix2EvaluationDomain::<Fr>::new(n).unwrap();
    let g = Radix2EvaluationDomain::<Fr>::new(2 * n).unwrap().element(1);
    let mut c: Vec<Fr> = a.iter().zip(b).map(|(x, y)| *x * y).collect();
    let (mut a, mut b) = (a.to_vec(), b.to_vec());
    for v in [&mut a, &mut b, &mut c] {
        d.ifft_in_place(v);
        Radix2EvaluationDomain::distribute_powers_and_mul_by_const(v, g, Fr::from(1u64));
        d.fft_in_place(v);
    }
    a.iter()
        .zip(&b)
        .zip(&c)
        .map(|((x, y), z)| *x * y - z)
        .collect()
}

#[test]
fn gpu_qap_equals_cpu() {
    let Ok(gpu) = pollster::block_on(Gpu::new()) else {
        eprintln!("SKIP (no GPU)");
        return;
    };
    let mut rng = ChaCha20Rng::seed_from_u64(42);
    for log_n in [1u32, 2, 3, 10, 12, 16] {
        let n = 1usize << log_n;
        let d = Radix2EvaluationDomain::<Fr>::new(n).unwrap();
        let g = Radix2EvaluationDomain::<Fr>::new(2 * n).unwrap().element(1);
        let t = gpu.qap_tables(n, d.group_gen(), g).unwrap();
        // like the prover: real rows, then input rows (b = 0), then zero padding; plus edge values
        let mut a: Vec<Fr> = (0..n).map(|_| Fr::rand(&mut rng)).collect();
        let mut b: Vec<Fr> = (0..n)
            .map(|i| {
                if i < 3 * n / 4 {
                    Fr::rand(&mut rng)
                } else {
                    Fr::zero()
                }
            })
            .collect();
        a[n - 1] = Fr::zero();
        a[0] = -Fr::from(1u64);
        b[0] = -Fr::from(1u64);
        let want = reference(&a, &b);
        let tm = Instant::now();
        let mut p = gpu.submit_qap(&t, &a, &b).unwrap();
        let h = pollster::block_on(p.read()).unwrap();
        let dt = tm.elapsed();
        pollster::block_on(p.finish()).unwrap();
        for (i, (got, want)) in h.0.iter().zip(&want).enumerate() {
            assert_eq!(*got, want.into_bigint(), "n=2^{log_n}: h[{i}]");
        }
        assert_eq!(h.0.len(), n);
        eprintln!("n=2^{log_n}: GPU QAP identical to CPU ({dt:?} incl. upload + readback)");
    }
    // bad inputs are errors, not wrong results
    let d = Radix2EvaluationDomain::<Fr>::new(8).unwrap();
    assert!(gpu.qap_tables(8, d.group_gen(), Fr::from(3u64)).is_err());
    let t = gpu
        .qap_tables(
            8,
            d.group_gen(),
            Radix2EvaluationDomain::<Fr>::new(16).unwrap().element(1),
        )
        .unwrap();
    assert!(
        gpu.submit_qap(&t, &[Fr::zero(); 4], &[Fr::zero(); 8])
            .is_err()
    );
}
