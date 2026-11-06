//! WebGPU prover inside wasm32 in a real browser (feature `webgpu`), vs the CPU prover in the
//! same wasm module: every vectors.json case must give a byte-identical proof for the same
//! RNG. Prints adapter info, setup costs and per-stage timings (`LOWKI_*` lines) and every
//! GPU proof as `LOWKI_PROOF {json}` for snarkjs. Run with `make verify-webgpu-chrome`.
//! Without WebGPU (e.g. Node, or a browser without an adapter) it prints `SKIP` and passes.
#![cfg(all(target_arch = "wasm32", feature = "webgpu"))]

use lowki_groth16::*;
use rand_chacha::ChaCha20Rng;
use rand_chacha::rand_core::SeedableRng;
use serde_json::Value;
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

macro_rules! artifact {
    ($f:literal) => {
        include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../../vendor/npm/circuits/package/artifacts/", $f))
    };
}

const VECTORS: &str = include_str!("../../../vectors/lowki-groth16/vectors.json");

fn now() -> f64 {
    js_sys::Date::now()
}

async fn adapter_info() -> String {
    let p = js_sys::eval(
        "navigator.gpu ? navigator.gpu.requestAdapter({powerPreference: 'high-performance'}).then(a => a ? JSON.stringify({vendor: a.info.vendor, architecture: a.info.architecture, device: a.info.device, description: a.info.description, maxStorageBufferBindingSize: a.limits.maxStorageBufferBindingSize, maxBufferSize: a.limits.maxBufferSize}) : 'null') : Promise.resolve('no navigator.gpu')",
    )
    .unwrap();
    let v = wasm_bindgen_futures::JsFuture::from(js_sys::Promise::from(p)).await.unwrap();
    v.as_string().unwrap_or_default()
}

fn stages(t: &[(&str, f64)]) -> String {
    t.iter().map(|(n, x)| format!("{n}={x:.0}")).collect::<Vec<_>>().join(" | ")
}

#[wasm_bindgen_test]
async fn webgpu_proofs_identical_to_cpu() {
    console_log!("LOWKI_GPU navigator adapter: {}", adapter_info().await);
    let t = now();
    let gpu = match open_gpu().await {
        Ok(g) => g,
        Err(e) => {
            console_log!("SKIP: {e}");
            return;
        }
    };
    console_log!("LOWKI_SETUP device+pipelines(shader compile)={:.0}ms", now() - t);
    let v: Value = serde_json::from_str(VECTORS).unwrap();
    let mut n = 0;
    for (circuit, name) in [(Circuit::Commitment, "commitment"), (Circuit::Withdraw, "withdraw")] {
        let t = now();
        let cpu = match circuit {
            Circuit::Commitment => {
                CircuitProver::load(circuit, artifact!("commitment.wasm"), artifact!("commitment.zkey")).unwrap()
            }
            Circuit::Withdraw => {
                CircuitProver::load(circuit, artifact!("withdraw.wasm"), artifact!("withdraw.zkey")).unwrap()
            }
        };
        let t_load = now() - t;
        let t = now();
        let gp = GpuProver::new(gpu.clone(), cpu).map_err(|e| e.1).unwrap();
        console_log!(
            "LOWKI_SETUP {name}: zkey load={t_load:.0}ms base upload={:.0}ms ({} MiB resident)",
            now() - t,
            gp.gpu_bytes() >> 20
        );
        for (k, case) in v["cases"].as_array().unwrap().iter().filter(|c| c["circuit"] == name).enumerate() {
            let json = case["input"].to_string();
            let seed = 1000 + k as u64;
            let wi;
            let ci;
            let sig: Vec<(&str, &[Fr])> = match circuit {
                Circuit::Withdraw => {
                    wi = WithdrawInputs::from_json(&json).unwrap();
                    wi.as_signals().to_vec()
                }
                Circuit::Commitment => {
                    ci = CommitmentInputs::from_json(&json).unwrap();
                    ci.as_signals().to_vec()
                }
            };
            // CPU path (default prove_* functions) for the reference proof and timing
            let t = now();
            let cpu_out = match circuit {
                Circuit::Withdraw => {
                    prove_withdraw_with_rng(gp.cpu(), &wi_ref(&json), &mut ChaCha20Rng::seed_from_u64(seed))
                }
                Circuit::Commitment => {
                    prove_commitment_with_rng(gp.cpu(), &ci_ref(&json), &mut ChaCha20Rng::seed_from_u64(seed))
                }
            }
            .unwrap();
            let t_cpu = now() - t;
            let t = now();
            let (gpu_out, st) =
                gp.profile_with_rng(&sig, &mut ChaCha20Rng::seed_from_u64(seed), &now).await.unwrap();
            let t_gpu = now() - t;
            assert_eq!(
                serde_json::to_vec(&cpu_out).unwrap(),
                serde_json::to_vec(&gpu_out).unwrap(),
                "{}: WebGPU proof differs from CPU proof",
                case["name"]
            );
            assert!(verify(gp.cpu().verifying_key(), &gpu_out.proof, &gpu_out.public_signals).unwrap());
            console_log!(
                "LOWKI_TIMING {} identical: cpu_wasm={t_cpu:.0}ms webgpu_wasm={t_gpu:.0}ms [{}]",
                case["name"].as_str().unwrap(),
                stages(&st)
            );
            let line = serde_json::json!({
                "name": format!("webgpu_{}", case["name"].as_str().unwrap()), "circuit": name,
                "proof": gpu_out.proof, "publicSignals": gpu_out.public_signals, "solidity": gpu_out.solidity,
            });
            console_log!("LOWKI_PROOF {}", line);
            n += 1;
        }
    }
    console_log!("LOWKI_RESULT {n} WebGPU proofs byte-identical to CPU proofs");
    console_log!("LOWKI_MEMORY wasm linear memory: {} MiB", core::arch::wasm32::memory_size(0) * 64 / 1024);
}

fn wi_ref(json: &str) -> WithdrawInputs {
    WithdrawInputs::from_json(json).unwrap()
}

fn ci_ref(json: &str) -> CommitmentInputs {
    CommitmentInputs::from_json(json).unwrap()
}
