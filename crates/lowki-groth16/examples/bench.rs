//! Native timings: `cargo run --release --example bench [-- iterations]`
//! (add `--features parallel` for multi-threaded proving). Reads artifacts from
//! `LOWKI_PP_ARTIFACTS` (default: unpacked circuits package) and inputs from vectors.json.
use std::time::Instant;

use lowki_groth16::*;
use serde_json::Value;

const VECTORS: &str = include_str!("../../../vectors/lowki-groth16/vectors.json");

fn main() {
    let dir = std::env::var("LOWKI_PP_ARTIFACTS")
        .unwrap_or_else(|_| "/Users/betterclever/projects/ethglobal/vendor/npm/circuits/package/artifacts".into());
    let iters: u32 = std::env::args().nth(1).and_then(|s| s.parse().ok()).unwrap_or(3);
    let v: Value = serde_json::from_str(VECTORS).unwrap();
    for (circuit, name) in [(Circuit::Commitment, "commitment"), (Circuit::Withdraw, "withdraw")] {
        let wasm = std::fs::read(format!("{dir}/{name}.wasm")).unwrap();
        let zkey = std::fs::read(format!("{dir}/{name}.zkey")).unwrap();
        let t = Instant::now();
        let p = CircuitProver::load(circuit, &wasm, &zkey).unwrap();
        let load = t.elapsed();
        let case = v["cases"]
            .as_array()
            .unwrap()
            .iter()
            .find(|c| c["circuit"] == name)
            .unwrap();
        let json = case["input"].to_string();
        let (mut wit, mut prove) = (0f64, 0f64);
        for _ in 0..iters {
            let t = Instant::now();
            let out = match circuit {
                Circuit::Commitment => {
                    let i = CommitmentInputs::from_json(&json).unwrap();
                    p.witness(&i.as_signals()).unwrap();
                    wit += t.elapsed().as_secs_f64();
                    let t = Instant::now();
                    let o = prove_commitment(&p, &i).unwrap();
                    prove += t.elapsed().as_secs_f64();
                    o
                }
                Circuit::Withdraw => {
                    let i = WithdrawInputs::from_json(&json).unwrap();
                    p.witness(&i.as_signals()).unwrap();
                    wit += t.elapsed().as_secs_f64();
                    let t = Instant::now();
                    let o = prove_withdraw(&p, &i).unwrap();
                    prove += t.elapsed().as_secs_f64();
                    o
                }
            };
            assert!(verify(p.verifying_key(), &out.proof, &out.public_signals).unwrap());
        }
        let n = f64::from(iters);
        println!(
            "{name}: load {:.0}ms | witness {:.0}ms | prove (witness+groth16+selfcheck) {:.0}ms  (avg of {iters})",
            load.as_secs_f64() * 1e3,
            wit / n * 1e3,
            prove / n * 1e3
        );
    }
}
