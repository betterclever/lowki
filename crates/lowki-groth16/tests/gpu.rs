//! WebGPU prover (feature `webgpu`) vs the CPU prover, natively (Metal/Vulkan).
//! Without a usable GPU every test prints `SKIP` and passes.
//!
//!   cargo test --release --features webgpu --test gpu -- --nocapture --test-threads=1
#![cfg(all(feature = "webgpu", not(target_arch = "wasm32")))]

use std::path::PathBuf;
use std::rc::Rc;
use std::time::Instant;

use ark_bn254::{G1Projective, G2Projective};
use ark_ec::{CurveGroup, VariableBaseMSM};
use ark_ff::UniformRand;
use lowki_groth16::*;
use rand_chacha::ChaCha20Rng;
use rand_chacha::rand_core::SeedableRng;
use serde_json::Value;

const VECTORS: &str = include_str!("../../../vectors/lowki-groth16/vectors.json");

fn read(name: &str) -> Vec<u8> {
    let dir = std::env::var("LOWKI_PP_ARTIFACTS").map(PathBuf::from).unwrap_or_else(|_| {
        PathBuf::from("/Users/betterclever/projects/ethglobal/vendor/npm/circuits/package/artifacts")
    });
    std::fs::read(dir.join(name)).unwrap()
}

fn load(c: Circuit) -> CircuitProver {
    let n = if c == Circuit::Withdraw { "withdraw" } else { "commitment" };
    CircuitProver::load(c, &read(&format!("{n}.wasm")), &read(&format!("{n}.zkey"))).unwrap()
}

fn gpu() -> Option<Rc<lowki_gpu::Gpu>> {
    let t = Instant::now();
    match pollster::block_on(open_gpu()) {
        Ok(g) => {
            let i = g.adapter_info();
            eprintln!("GPU: {} ({:?}), device + pipelines {:?}", i.name, i.backend, t.elapsed());
            Some(g)
        }
        Err(e) => {
            eprintln!("SKIP: {e}");
            None
        }
    }
}

/// Every vectors.json case: GPU proof == CPU proof byte for byte (same RNG), and both verify.
/// Exports the GPU proofs to vectors/lowki-groth16/out/gpu-proofs.json for snarkjs.
#[test]
fn gpu_proofs_identical_to_cpu() {
    let Some(g) = gpu() else { return };
    let v: Value = serde_json::from_str(VECTORS).unwrap();
    let mut exported = Vec::new();
    for c in [Circuit::Commitment, Circuit::Withdraw] {
        let name = if c == Circuit::Withdraw { "withdraw" } else { "commitment" };
        let t = Instant::now();
        let gp = GpuProver::new(g.clone(), load(c)).map_err(|e| e.1).unwrap();
        eprintln!("{name}: load + base upload {:?} ({} MiB on GPU)", t.elapsed(), gp.gpu_bytes() >> 20);
        let mut n = 0;
        for (k, case) in v["cases"].as_array().unwrap().iter().filter(|x| x["circuit"] == name).enumerate() {
            let json = case["input"].to_string();
            let seed = 1000 + k as u64;
            let (t_cpu, t_gpu);
            let (cpu, gpu) = match c {
                Circuit::Withdraw => {
                    let i = WithdrawInputs::from_json(&json).unwrap();
                    let t = Instant::now();
                    let a = prove_withdraw_with_rng(gp.cpu(), &i, &mut ChaCha20Rng::seed_from_u64(seed)).unwrap();
                    t_cpu = t.elapsed();
                    let t = Instant::now();
                    let b = pollster::block_on(gp.prove_withdraw_with_rng(&i, &mut ChaCha20Rng::seed_from_u64(seed)))
                        .unwrap();
                    t_gpu = t.elapsed();
                    (a, b)
                }
                Circuit::Commitment => {
                    let i = CommitmentInputs::from_json(&json).unwrap();
                    let t = Instant::now();
                    let a = prove_commitment_with_rng(gp.cpu(), &i, &mut ChaCha20Rng::seed_from_u64(seed)).unwrap();
                    t_cpu = t.elapsed();
                    let t = Instant::now();
                    let b =
                        pollster::block_on(gp.prove_commitment_with_rng(&i, &mut ChaCha20Rng::seed_from_u64(seed)))
                            .unwrap();
                    t_gpu = t.elapsed();
                    (a, b)
                }
            };
            assert_eq!(
                serde_json::to_vec(&cpu).unwrap(),
                serde_json::to_vec(&gpu).unwrap(),
                "{}: GPU proof differs from CPU proof",
                case["name"]
            );
            assert!(verify(gp.cpu().verifying_key(), &gpu.proof, &gpu.public_signals).unwrap());
            eprintln!("  {}: identical (cpu {t_cpu:?}, gpu {t_gpu:?})", case["name"].as_str().unwrap());
            exported.push(serde_json::json!({
                "name": format!("gpu_{}", case["name"].as_str().unwrap()), "circuit": name,
                "proof": gpu.proof, "publicSignals": gpu.public_signals, "solidity": gpu.solidity,
            }));
            n += 1;
        }
        assert!(n > 0);
    }
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../vectors/lowki-groth16/out");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("gpu-proofs.json"), serde_json::to_string_pretty(&exported).unwrap()).unwrap();
    eprintln!("{} GPU proofs identical to CPU proofs", exported.len());
}

/// GPU MSMs on the real withdraw proving-key bases (all five queries) with the real witness
/// and with random scalars, vs ark's VariableBaseMSM.
#[test]
fn gpu_msm_on_proving_key_bases() {
    let Some(g) = gpu() else { return };
    let p = load(Circuit::Withdraw);
    let pk = p.proving_key().clone();
    let v: Value = serde_json::from_str(VECTORS).unwrap();
    let case = v["cases"].as_array().unwrap().iter().find(|c| c["circuit"] == "withdraw").unwrap();
    let w = p.witness(&WithdrawInputs::from_json(&case["input"].to_string()).unwrap().as_signals()).unwrap();
    let mut rng = ChaCha20Rng::seed_from_u64(9);
    let rand_h: Vec<Fr> = (0..pk.h_query.len()).map(|_| Fr::rand(&mut rng)).collect();
    let rand_w: Vec<Fr> = (0..w.len()).map(|_| Fr::rand(&mut rng)).collect();
    for (label, sc) in [("witness", &w[..]), ("random", &rand_w[..])] {
        for (q, name) in [(&pk.a_query, "a_query"), (&pk.b_g1_query, "b_g1_query")] {
            let b = g.upload_bases(q).unwrap();
            let got = pollster::block_on(g.msm_g1(&b, sc)).unwrap().into_affine();
            assert_eq!(got, G1Projective::msm(q, sc).unwrap().into_affine(), "{name} {label}");
        }
        let ni = Circuit::Withdraw.n_public() + 1;
        let b = g.upload_bases(&pk.l_query).unwrap();
        let got = pollster::block_on(g.msm_g1(&b, &sc[ni..])).unwrap().into_affine();
        assert_eq!(got, G1Projective::msm(&pk.l_query, &sc[ni..]).unwrap().into_affine(), "l_query {label}");
        let b = g.upload_bases(&pk.b_g2_query).unwrap();
        let got = pollster::block_on(g.msm_g2(&b, sc)).unwrap().into_affine();
        assert_eq!(got, G2Projective::msm(&pk.b_g2_query, sc).unwrap().into_affine(), "b_g2_query {label}");
        eprintln!("proving-key bases, {label} scalars: a, b_g1, l, b_g2 equal");
    }
    let b = g.upload_bases(&pk.h_query).unwrap();
    let got = pollster::block_on(g.msm_g1(&b, &rand_h)).unwrap().into_affine();
    assert_eq!(got, G1Projective::msm(&pk.h_query, &rand_h).unwrap().into_affine(), "h_query random");
    let inf = pk.a_query.iter().filter(|p| p.infinity).count() + pk.b_g2_query.iter().filter(|p| p.infinity).count();
    eprintln!("h_query (65536) random scalars equal; {inf} points at infinity among a/b_g2 bases");
}
