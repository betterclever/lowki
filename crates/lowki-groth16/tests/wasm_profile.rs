//! Per-stage timing of the CPU prover inside wasm32 (Node by default; Chrome with
//! `WASM_BINDGEN_USE_BROWSER=1`). Prints `LOWKI_PROFILE` lines.
//!
//!   CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUNNER=wasm-bindgen-test-runner \
//!     cargo test --release --target wasm32-unknown-unknown --test wasm_profile -- --nocapture
#![cfg(target_arch = "wasm32")]

use lowki_groth16::*;
use rand_chacha::ChaCha20Rng;
use rand_chacha::rand_core::SeedableRng;
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

fn input(circuit: &str) -> String {
    let v: Value = serde_json::from_str(VECTORS).unwrap();
    v["cases"].as_array().unwrap().iter().find(|c| c["circuit"] == circuit).unwrap()["input"].to_string()
}

fn run(name: &str, p: &CircuitProver, signals: &[(&str, &[Fr])]) {
    // run twice: the first call includes wasmi's lazy translation of the witness wasm
    for iter in 0..2 {
        let mut rng = ChaCha20Rng::seed_from_u64(1);
        let (_, t) = p.profile_with_rng(signals, &mut rng, &now).unwrap();
        let total: f64 = t.iter().map(|x| x.1).sum();
        let parts: Vec<String> = t.iter().map(|(n, x)| format!("{n}={x:.0}")).collect();
        console_log!("LOWKI_PROFILE {name} run={iter} total={total:.0}ms | {}", parts.join(" | "));
    }
}

#[wasm_bindgen_test]
fn profile_commitment() {
    let p = CircuitProver::load(Circuit::Commitment, artifact!("commitment.wasm"), artifact!("commitment.zkey")).unwrap();
    let i = CommitmentInputs::from_json(&input("commitment")).unwrap();
    run("commitment", &p, &i.as_signals());
}

#[wasm_bindgen_test]
fn profile_withdraw() {
    let p = CircuitProver::load(Circuit::Withdraw, artifact!("withdraw.wasm"), artifact!("withdraw.zkey")).unwrap();
    let i = WithdrawInputs::from_json(&input("withdraw")).unwrap();
    run("withdraw", &p, &i.as_signals());
}
