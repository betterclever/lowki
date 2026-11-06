//! Witness differential test on wasm32: the host-`WebAssembly` witness path (what `load` /
//! `load_async` select on wasm32) vs the wasmi interpreter vs snarkjs 0.7.5, element-wise, for
//! every vectors.json case. Runs in headless Chrome (main thread):
//!
//!   cargo test --release --target wasm32-unknown-unknown --test wasm_witness --no-run
//!   cd ../lowki-gpu/browser && CHROMEDRIVER=<chromedriver> wasm-bindgen-test-runner <wasm>
//!
//! (or under Node with `WASM_BINDGEN_USE_NODE=1`). The snarkjs witnesses are
//! `vectors/lowki-groth16/witness/*.bin` (32-byte little-endian elements, from `make vectors`),
//! and each is also pinned by sha256 in vectors.json.
#![cfg(target_arch = "wasm32")]

use ark_ff::{BigInteger, PrimeField};
use lowki_groth16::*;
use serde_json::Value;
use sha2::{Digest, Sha256};
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_browser);

macro_rules! artifact {
    ($f:literal) => {
        include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../../vendor/npm/circuits/package/artifacts/",
            $f
        ))
    };
}
macro_rules! witness_bin {
    ($n:literal) => {
        &include_bytes!(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../vectors/lowki-groth16/witness/",
            $n,
            ".bin"
        ))[..]
    };
}

const VECTORS: &str = include_str!("../../../vectors/lowki-groth16/vectors.json");

fn snarkjs_witness(name: &str) -> &'static [u8] {
    match name {
        "commitment_rand_0" => witness_bin!("commitment_rand_0"),
        "commitment_rand_1" => witness_bin!("commitment_rand_1"),
        "commitment_rand_2" => witness_bin!("commitment_rand_2"),
        "commitment_rand_3" => witness_bin!("commitment_rand_3"),
        "commitment_rand_4" => witness_bin!("commitment_rand_4"),
        "commitment_rand_5" => witness_bin!("commitment_rand_5"),
        "commitment_zero_value" => witness_bin!("commitment_zero_value"),
        "commitment_max_field" => witness_bin!("commitment_max_field"),
        "commitment_small" => witness_bin!("commitment_small"),
        "commitment_1eth" => witness_bin!("commitment_1eth"),
        "withdraw_single_leaf" => witness_bin!("withdraw_single_leaf"),
        "withdraw_two_leaves_last" => witness_bin!("withdraw_two_leaves_last"),
        "withdraw_five_leaves_lifted" => witness_bin!("withdraw_five_leaves_lifted"),
        "withdraw_full_amount" => witness_bin!("withdraw_full_amount"),
        "withdraw_zero_amount" => witness_bin!("withdraw_zero_amount"),
        "withdraw_200_leaves" => witness_bin!("withdraw_200_leaves"),
        "withdraw_1500_leaves" => witness_bin!("withdraw_1500_leaves"),
        n => panic!("no witness file for {n}"),
    }
}

fn now() -> f64 {
    js_sys::Date::now()
}

fn le_bytes(w: &[Fr]) -> Vec<u8> {
    w.iter().flat_map(|x| x.into_bigint().to_bytes_le()).collect()
}

fn calc(c: Circuit) -> (&'static [u8], &'static [u8]) {
    match c {
        Circuit::Commitment => (artifact!("commitment.wasm"), artifact!("commitment.zkey")),
        Circuit::Withdraw => (artifact!("withdraw.wasm"), artifact!("withdraw.zkey")),
    }
}

#[wasm_bindgen_test]
async fn webassembly_witness_equals_wasmi_and_snarkjs() {
    let v: Value = serde_json::from_str(VECTORS).unwrap();
    let mut n = 0;
    for (circuit, name) in [(Circuit::Commitment, "commitment"), (Circuit::Withdraw, "withdraw")] {
        let (wasm, zkey) = calc(circuit);
        let t = now();
        let p = CircuitProver::load_async(circuit, wasm, zkey).await.unwrap();
        let t_async = now() - t;
        let t = now();
        let p_sync = CircuitProver::load(circuit, wasm, zkey).unwrap();
        let t_sync = now() - t;
        assert_eq!(p.witness_backend(), "webassembly");
        assert_eq!(
            p_sync.witness_backend(),
            "webassembly",
            "sync compile refused on the main thread"
        );
        let wasmi = WitnessCalculator::new_wasmi(wasm).unwrap();
        assert_eq!(wasmi.backend(), "wasmi");
        console_log!("LOWKI_SETUP {name}: load_async={t_async:.0}ms load(sync)={t_sync:.0}ms");

        for case in v["cases"].as_array().unwrap().iter().filter(|c| c["circuit"] == name) {
            let json = case["input"].to_string();
            let (wi, ci);
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
            let t = now();
            let w_js = p.witness(&sig).unwrap();
            let t_js = now() - t;
            let w_sync = p_sync.witness(&sig).unwrap();
            let t = now();
            let w_wasmi = wasmi.calculate(&sig).unwrap();
            let t_wasmi = now() - t;
            let case_name = case["name"].as_str().unwrap();
            assert_eq!(w_js.len(), case["witnessLen"].as_u64().unwrap() as usize);
            for (i, (a, b)) in w_js.iter().zip(w_wasmi.iter()).enumerate() {
                assert_eq!(a, b, "{case_name}: witness[{i}] webassembly != wasmi");
            }
            assert_eq!(w_js.len(), w_wasmi.len());
            assert!(w_js[..] == w_sync[..], "{case_name}: load_async vs load witness differ");
            let bytes = le_bytes(&w_js);
            let snark = snarkjs_witness(case_name);
            assert_eq!(bytes.len(), snark.len(), "{case_name}: witness length vs snarkjs");
            for (i, (a, b)) in bytes.chunks(32).zip(snark.chunks(32)).enumerate() {
                assert_eq!(a, b, "{case_name}: witness[{i}] webassembly != snarkjs");
            }
            assert_eq!(
                hex(&Sha256::digest(&bytes)),
                case["witnessSha256"].as_str().unwrap(),
                "{case_name}: sha256"
            );
            console_log!(
                "LOWKI_WITNESS {case_name}: {} signals identical (webassembly == wasmi == snarkjs) webassembly={t_js:.1}ms wasmi={t_wasmi:.1}ms",
                w_js.len()
            );
            n += 1;
        }
    }
    assert_eq!(n, 17);
    // The escape hatch: no pin, every proving-key G2 point subgroup-checked (cost shown).
    for (circuit, name) in [(Circuit::Commitment, "commitment"), (Circuit::Withdraw, "withdraw")] {
        let (wasm, zkey) = calc(circuit);
        let t = now();
        let p = CircuitProver::load_async_unpinned(circuit, wasm, zkey).await.unwrap();
        console_log!("LOWKI_SETUP {name}: load_async_unpinned (full G2 subgroup checks)={:.0}ms", now() - t);
        assert_eq!(p.witness_backend(), "webassembly");
    }
    console_log!("LOWKI_RESULT {n}/17 witnesses identical");
}

/// A failing circuit assert must surface as `Error::Witness` with circom's message on both engines.
#[wasm_bindgen_test]
async fn webassembly_reports_circuit_errors_like_wasmi() {
    let v: Value = serde_json::from_str(VECTORS).unwrap();
    let case = v["cases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|c| c["circuit"] == "withdraw")
        .unwrap();
    let mut input = case["input"].clone();
    // withdrawnValue > existingValue violates the circuit's range check.
    let existing: u128 = input["existingValue"].as_str().unwrap().parse().unwrap();
    input["withdrawnValue"] = Value::String((existing + 1).to_string());
    let wi = WithdrawInputs::from_json(&input.to_string()).unwrap();
    let (wasm, zkey) = calc(Circuit::Withdraw);
    let p = CircuitProver::load_async(Circuit::Withdraw, wasm, zkey).await.unwrap();
    let e_js = p.witness(&wi.as_signals()).unwrap_err();
    let e_wasmi = WitnessCalculator::new_wasmi(wasm)
        .unwrap()
        .calculate(&wi.as_signals())
        .unwrap_err();
    console_log!("LOWKI_ERROR webassembly: {e_js}");
    console_log!("LOWKI_ERROR wasmi:       {e_wasmi}");
    assert!(matches!(e_js, Error::Witness(_)) && matches!(e_wasmi, Error::Witness(_)));
    assert!(e_js.to_string().contains("Assert Failed") && e_wasmi.to_string().contains("Assert Failed"));
    // The prover is still usable after a trapped instance.
    let ok = WithdrawInputs::from_json(&case["input"].to_string()).unwrap();
    assert!(p.witness(&ok.as_signals()).is_ok());
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}
