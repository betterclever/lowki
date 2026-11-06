//! Per-stage timing of one proof (single-threaded unless `--features parallel`):
//! `cargo run --release --example profile [-- iterations]`.
use std::time::Instant;

use lowki_groth16::*;
use rand_chacha::ChaCha20Rng;
use rand_chacha::rand_core::SeedableRng;
use serde_json::Value;

const VECTORS: &str = include_str!("../../../vectors/lowki-groth16/vectors.json");

fn main() {
    let dir = std::env::var("LOWKI_PP_ARTIFACTS")
        .unwrap_or_else(|_| "/Users/betterclever/projects/ethglobal/vendor/npm/circuits/package/artifacts".into());
    let iters: usize = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(3);
    let v: Value = serde_json::from_str(VECTORS).unwrap();
    let t0 = Instant::now();
    let now = || t0.elapsed().as_secs_f64() * 1e3;
    for (circuit, name) in [(Circuit::Commitment, "commitment"), (Circuit::Withdraw, "withdraw")] {
        let wasm = std::fs::read(format!("{dir}/{name}.wasm")).unwrap();
        let zkey = std::fs::read(format!("{dir}/{name}.zkey")).unwrap();
        let p = CircuitProver::load(circuit, &wasm, &zkey).unwrap();
        let case = v["cases"].as_array().unwrap().iter().find(|c| c["circuit"] == name).unwrap();
        let json = case["input"].to_string();
        let mut sums: Vec<(&str, f64)> = Vec::new();
        for i in 0..iters {
            let mut rng = ChaCha20Rng::seed_from_u64(i as u64);
            let (out, t) = match circuit {
                Circuit::Commitment => {
                    let inp = CommitmentInputs::from_json(&json).unwrap();
                    p.profile_with_rng(&inp.as_signals(), &mut rng, &now).unwrap()
                }
                Circuit::Withdraw => {
                    let inp = WithdrawInputs::from_json(&json).unwrap();
                    p.profile_with_rng(&inp.as_signals(), &mut rng, &now).unwrap()
                }
            };
            // same proof as the default path with the same RNG
            let mut rng = ChaCha20Rng::seed_from_u64(i as u64);
            let reference = match circuit {
                Circuit::Commitment => {
                    prove_commitment_with_rng(&p, &CommitmentInputs::from_json(&json).unwrap(), &mut rng)
                }
                Circuit::Withdraw => prove_withdraw_with_rng(&p, &WithdrawInputs::from_json(&json).unwrap(), &mut rng),
            }
            .unwrap();
            assert_eq!(out, reference, "profiled proof must equal the default proof");
            if sums.is_empty() {
                sums = t.iter().map(|(n, _)| (*n, 0.0)).collect();
            }
            for (s, (_, x)) in sums.iter_mut().zip(&t) {
                s.1 += x;
            }
        }
        let total: f64 = sums.iter().map(|x| x.1).sum::<f64>() / iters as f64;
        println!("{name} (avg of {iters}), total {total:.1} ms");
        for (n, x) in &sums {
            let x = x / iters as f64;
            println!("  {n:<24} {x:8.1} ms  {:5.1}%", 100.0 * x / total);
        }
    }
}
