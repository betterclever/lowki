//! Per-stage timing of the WebGPU prover (native):
//! `cargo run --release --features webgpu --example gpu_profile [-- iterations [window_bits]]`.
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
    let c: Option<u32> = std::env::args().nth(2).and_then(|s| s.parse().ok());
    let v: Value = serde_json::from_str(VECTORS).unwrap();
    let t0 = Instant::now();
    let now = || t0.elapsed().as_secs_f64() * 1e3;
    let t = Instant::now();
    let gpu = pollster::block_on(open_gpu()).expect("GPU");
    println!("GPU {} ({:?}): device + pipelines {:?}", gpu.adapter_info().name, gpu.adapter_info().backend, t.elapsed());
    for (circuit, name) in [(Circuit::Commitment, "commitment"), (Circuit::Withdraw, "withdraw")] {
        let wasm = std::fs::read(format!("{dir}/{name}.wasm")).unwrap();
        let zkey = std::fs::read(format!("{dir}/{name}.zkey")).unwrap();
        let p = CircuitProver::load(circuit, &wasm, &zkey).unwrap();
        let t = Instant::now();
        let mut gp = GpuProver::new(gpu.clone(), p).map_err(|e| e.1).unwrap();
        println!("{name}: base upload {:?}", t.elapsed());
        if let Some(c) = c {
            gp.set_window_bits(c);
        }
        let case = v["cases"].as_array().unwrap().iter().find(|c| c["circuit"] == name).unwrap();
        let json = case["input"].to_string();
        let wi = WithdrawInputs::from_json(if circuit == Circuit::Withdraw { &json } else { "{}" });
        let ci = CommitmentInputs::from_json(if circuit == Circuit::Commitment { &json } else { "{}" });
        let sig: Vec<(&str, &[Fr])> = match circuit {
            Circuit::Withdraw => wi.as_ref().unwrap().as_signals().to_vec(),
            Circuit::Commitment => ci.as_ref().unwrap().as_signals().to_vec(),
        };
        let sig = &sig[..];
        let mut sums: Vec<(&str, f64)> = Vec::new();
        for i in 0..iters {
            let (_, t) =
                pollster::block_on(gp.profile_with_rng(sig, &mut ChaCha20Rng::seed_from_u64(i as u64), &now)).unwrap();
            if i == 0 {
                let total: f64 = t.iter().map(|x| x.1).sum();
                println!("  first run total {total:.1} ms");
                sums = t.iter().map(|(n, _)| (*n, 0.0)).collect();
                continue;
            }
            for (s, (_, x)) in sums.iter_mut().zip(&t) {
                s.1 += x;
            }
        }
        let k = (iters - 1).max(1) as f64;
        let total: f64 = sums.iter().map(|x| x.1).sum::<f64>() / k;
        println!("{name} (avg of {} warm runs), total {total:.1} ms", iters - 1);
        for (n, x) in &sums {
            println!("  {n:<32} {:8.1} ms", x / k);
        }
    }
}
