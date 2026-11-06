//! Runs real proofs inside `wasm32-unknown-unknown` (wasm-bindgen-test under Node).
//!
//!   CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUNNER=wasm-bindgen-test-runner \
//!     cargo test --release --target wasm32-unknown-unknown --test wasm
//!
//! Artifacts are embedded with `include_bytes!` from the unpacked circuits package
//! (`../../../vendor/npm/circuits/package/artifacts`, i.e. next to the lowki checkout).
//! Each proof is printed as a `LOWKI_PROOF {json}` line so `make verify-wasm` can hand it
//! to snarkjs.
#![cfg(target_arch = "wasm32")]

use lowki_groth16::*;
use serde_json::Value;
use wasm_bindgen_test::*;

macro_rules! artifact {
    ($f:literal) => {
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../../vendor/npm/circuits/package/artifacts/",
            $f
        ))
    };
}

const VECTORS: &str = include_str!("../../../vectors/lowki-groth16/vectors.json");

fn now() -> f64 {
    js_sys::Date::now()
}

fn case(circuit: &str) -> Value {
    let v: Value = serde_json::from_str(VECTORS).unwrap();
    v["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["circuit"] == circuit)
        .unwrap()
        .clone()
}

fn report(name: &str, circuit: &str, out: &ProofOutput) {
    let line = serde_json::json!({
        "name": format!("wasm_{name}"), "circuit": circuit,
        "proof": out.proof, "publicSignals": out.public_signals, "solidity": out.solidity,
    });
    console_log!("LOWKI_PROOF {}", line);
}

#[wasm_bindgen_test]
fn wasm_commitment_proof() {
    let c = case("commitment");
    let t0 = now();
    let p = CircuitProver::load(
        Circuit::Commitment,
        artifact!("commitment.wasm"),
        artifact!("commitment.zkey"),
    )
    .unwrap();
    let t1 = now();
    let inputs = CommitmentInputs::from_json(&c["input"].to_string()).unwrap();
    let w = p.witness(&inputs.as_signals()).unwrap();
    let t2 = now();
    let out = prove_commitment(&p, &inputs).unwrap();
    let t3 = now();
    assert_eq!(w.len() as u64, c["witnessLen"].as_u64().unwrap());
    assert_eq!(serde_json::to_value(&out.public_signals).unwrap(), c["publicSignals"]);
    assert!(verify(p.verifying_key(), &out.proof, &out.public_signals).unwrap());
    console_log!(
        "LOWKI_TIMING commitment load={:.0}ms witness={:.0}ms prove(witness+groth16+selfcheck)={:.0}ms",
        t1 - t0,
        t2 - t1,
        t3 - t2
    );
    report(c["name"].as_str().unwrap(), "commitment", &out);
    console_log!(
        "LOWKI_MEMORY wasm linear memory after commitment: {} MiB",
        core::arch::wasm32::memory_size(0) * 64 / 1024
    );
}

#[wasm_bindgen_test]
fn wasm_withdraw_proof() {
    let c = case("withdraw");
    let t0 = now();
    let p = CircuitProver::load(
        Circuit::Withdraw,
        artifact!("withdraw.wasm"),
        artifact!("withdraw.zkey"),
    )
    .unwrap();
    let t1 = now();
    let inputs = WithdrawInputs::from_json(&c["input"].to_string()).unwrap();
    let w = p.witness(&inputs.as_signals()).unwrap();
    let t2 = now();
    let out = prove_withdraw(&p, &inputs).unwrap();
    let t3 = now();
    assert_eq!(w.len() as u64, c["witnessLen"].as_u64().unwrap());
    assert_eq!(serde_json::to_value(&out.public_signals).unwrap(), c["publicSignals"]);
    assert!(verify(p.verifying_key(), &out.proof, &out.public_signals).unwrap());
    console_log!(
        "LOWKI_TIMING withdraw load={:.0}ms witness={:.0}ms prove(witness+groth16+selfcheck)={:.0}ms",
        t1 - t0,
        t2 - t1,
        t3 - t2
    );
    report(c["name"].as_str().unwrap(), "withdraw", &out);
    console_log!(
        "LOWKI_MEMORY wasm linear memory after withdraw: {} MiB",
        core::arch::wasm32::memory_size(0) * 64 / 1024
    );
}
