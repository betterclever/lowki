//! Differential tests against snarkjs 0.7.5 (vectors from vectors/lowki-groth16/gen.ts).
//!
//! Needs the circuit artifacts (not committed): set `LOWKI_PP_ARTIFACTS` or use the default
//! path of the unpacked `@fatsolutions/privacy-pools-core-circuits@1.0.5` package.
#![cfg(not(target_arch = "wasm32"))]

use std::path::PathBuf;
use std::sync::OnceLock;
use std::time::Instant;

use ark_ff::PrimeField;
use lowki_groth16::*;
use rand_chacha::ChaCha20Rng;
use rand_chacha::rand_core::SeedableRng;
use serde_json::Value;
use sha2::{Digest, Sha256};

const VECTORS: &str = include_str!("../../../vectors/lowki-groth16/vectors.json");

fn vectors_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../vectors/lowki-groth16")
}

fn artifacts() -> PathBuf {
    std::env::var("LOWKI_PP_ARTIFACTS")
        .map(PathBuf::from)
        .unwrap_or_else(|_| {
            PathBuf::from("/Users/betterclever/projects/ethglobal/vendor/npm/circuits/package/artifacts")
        })
}

fn read(name: &str) -> Vec<u8> {
    let p = artifacts().join(name);
    std::fs::read(&p)
        .unwrap_or_else(|e| panic!("missing artifact {}: {e} (see README: fetching artifacts)", p.display()))
}

fn vectors() -> &'static Value {
    static V: OnceLock<Value> = OnceLock::new();
    V.get_or_init(|| serde_json::from_str(VECTORS).unwrap())
}

fn prover(c: Circuit) -> &'static CircuitProver {
    static P: OnceLock<(CircuitProver, CircuitProver)> = OnceLock::new();
    let (cm, wd) = P.get_or_init(|| {
        let t = Instant::now();
        let cm = CircuitProver::load(Circuit::Commitment, &read("commitment.wasm"), &read("commitment.zkey")).unwrap();
        eprintln!("load commitment: {:?}", t.elapsed());
        let t = Instant::now();
        let wd = CircuitProver::load(Circuit::Withdraw, &read("withdraw.wasm"), &read("withdraw.zkey")).unwrap();
        eprintln!("load withdraw: {:?}", t.elapsed());
        (cm, wd)
    });
    match c {
        Circuit::Commitment => cm,
        Circuit::Withdraw => wd,
    }
}

fn circuit_of(case: &Value) -> Circuit {
    match case["circuit"].as_str().unwrap() {
        "commitment" => Circuit::Commitment,
        "withdraw" => Circuit::Withdraw,
        c => panic!("unknown circuit {c}"),
    }
}

fn cases() -> &'static Vec<Value> {
    vectors()["cases"].as_array().unwrap()
}

fn vkey(c: Circuit) -> VerifyingKey<Bn254> {
    let name = match c {
        Circuit::Commitment => "commitment.vkey",
        Circuit::Withdraw => "withdraw.vkey",
    };
    parse_vkey_json(std::str::from_utf8(&read(name)).unwrap()).unwrap()
}

fn witness_for(case: &Value) -> Vec<Fr> {
    let json = case["input"].to_string();
    let p = prover(circuit_of(case));
    let w = match circuit_of(case) {
        Circuit::Commitment => p.witness(&CommitmentInputs::from_json(&json).unwrap().as_signals()),
        Circuit::Withdraw => p.witness(&WithdrawInputs::from_json(&json).unwrap().as_signals()),
    };
    w.unwrap().to_vec()
}

fn le_bytes(w: &[Fr]) -> Vec<u8> {
    let mut out = Vec::with_capacity(32 * w.len());
    for f in w {
        for limb in f.into_bigint().0 {
            out.extend_from_slice(&limb.to_le_bytes());
        }
    }
    out
}

fn prove_case(case: &Value, rng: &mut ChaCha20Rng) -> ProofOutput {
    let json = case["input"].to_string();
    let c = circuit_of(case);
    match c {
        Circuit::Commitment => prove_commitment_with_rng(prover(c), &CommitmentInputs::from_json(&json).unwrap(), rng),
        Circuit::Withdraw => prove_withdraw_with_rng(prover(c), &WithdrawInputs::from_json(&json).unwrap(), rng),
    }
    .unwrap()
}

/// (i) Full witness equality with snarkjs for every case.
#[test]
fn i_witness_equals_snarkjs() {
    assert!(cases().len() >= 17);
    for case in cases() {
        let name = case["name"].as_str().unwrap();
        let t = Instant::now();
        let w = witness_for(case);
        let dt = t.elapsed();
        assert_eq!(
            w.len() as u64,
            case["witnessLen"].as_u64().unwrap(),
            "{name}: witness length"
        );
        let bytes = le_bytes(&w);
        // Element-wise diff against the snarkjs dump when available (diagnostics).
        let bin = vectors_dir().join(format!("witness/{name}.bin"));
        if let Ok(expected) = std::fs::read(&bin) {
            assert_eq!(expected.len(), bytes.len(), "{name}: bin length");
            if let Some(i) = (0..w.len()).find(|&i| expected[32 * i..32 * i + 32] != bytes[32 * i..32 * i + 32]) {
                panic!("{name}: witness differs from snarkjs at signal {i}");
            }
        }
        let digest = hex(&Sha256::digest(&bytes));
        assert_eq!(
            digest,
            case["witnessSha256"].as_str().unwrap(),
            "{name}: witness sha256"
        );
        eprintln!("(i) {name}: {} signals equal (rust witness {dt:?})", w.len());
    }
}

/// (ii) Public signals equal snarkjs' for every case.
#[test]
fn ii_public_signals_equal_snarkjs() {
    for case in cases() {
        let c = circuit_of(case);
        let w = witness_for(case);
        let ours: Vec<String> = w[1..=c.n_public()].iter().map(|f| f.to_string()).collect();
        let theirs: Vec<String> = case["publicSignals"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_string())
            .collect();
        assert_eq!(ours, theirs, "{}", case["name"]);
    }
}

/// (iii) Rust proofs verify with ark-groth16 against the snarkjs vkey JSON, for every case;
/// tampered signals are rejected. Also exports all proofs for check (iv).
#[test]
fn iii_rust_proofs_verify_and_export() {
    let mut rng = ChaCha20Rng::seed_from_u64(7);
    let vk_c = vkey(Circuit::Commitment);
    let vk_w = vkey(Circuit::Withdraw);
    // vkey JSON and zkey agree
    assert!(vk_c == *prover(Circuit::Commitment).verifying_key());
    assert!(vk_w == *prover(Circuit::Withdraw).verifying_key());

    let mut exported = Vec::new();
    for case in cases() {
        let name = case["name"].as_str().unwrap();
        let c = circuit_of(case);
        let t = Instant::now();
        let out = prove_case(case, &mut rng);
        let dt = t.elapsed();
        let vk = if c == Circuit::Commitment { &vk_c } else { &vk_w };
        assert_eq!(
            out.public_signals,
            serde_json::from_value::<Vec<String>>(case["publicSignals"].clone()).unwrap()
        );
        assert!(
            verify(vk, &out.proof, &out.public_signals).unwrap(),
            "{name}: proof must verify"
        );
        let mut bad = out.public_signals.clone();
        bad[0] = (bad[0].parse::<Fr>().unwrap() + Fr::from(1u64)).to_string();
        assert!(
            !verify(vk, &out.proof, &bad).unwrap(),
            "{name}: tampered signals must fail"
        );
        // Solidity form is the format conversion of this very proof.
        assert_eq!(out.solidity, out.proof.to_solidity(&out.public_signals).unwrap());
        eprintln!("(iii) {name}: prove {dt:?}, verifies");
        exported.push(serde_json::json!({
            "name": name,
            "circuit": case["circuit"],
            "proof": out.proof,
            "publicSignals": out.public_signals,
            "solidity": out.solidity,
        }));
    }
    let dir = vectors_dir().join("out");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        dir.join("rust-proofs.json"),
        serde_json::to_string_pretty(&exported).unwrap(),
    )
    .unwrap();
}

/// (v) Solidity calldata format == Kohaku's toWithdrawProof / encodeRagequitPayload and
/// snarkjs exportSolidityCallData, applied to the same fixed snarkjs proof. Also checks
/// that our verifier accepts snarkjs-produced proofs.
#[test]
fn v_solidity_format_matches_reference() {
    for fp in vectors()["fixedProofs"].as_array().unwrap() {
        let proof: SnarkjsProof = serde_json::from_value(fp["proof"].clone()).unwrap();
        let pubs: Vec<String> = serde_json::from_value(fp["publicSignals"].clone()).unwrap();
        let sol = proof.to_solidity(&pubs).unwrap();
        let kohaku: SolidityProof = serde_json::from_value(fp["solidity"].clone()).unwrap();
        assert_eq!(sol, kohaku, "{}: vs Kohaku encoding.utils", fp["circuit"]);
        let cd = fp["snarkjsSolidityCallData"].as_array().unwrap();
        let snark = SolidityProof {
            p_a: serde_json::from_value(cd[0].clone()).unwrap(),
            p_b: serde_json::from_value(cd[1].clone()).unwrap(),
            p_c: serde_json::from_value(cd[2].clone()).unwrap(),
            pub_signals: serde_json::from_value(cd[3].clone()).unwrap(),
        };
        assert_eq!(sol, snark, "{}: vs snarkjs exportSolidityCallData", fp["circuit"]);
        let c = if fp["circuit"] == "commitment" {
            Circuit::Commitment
        } else {
            Circuit::Withdraw
        };
        assert!(
            verify(&vkey(c), &proof, &pubs).unwrap(),
            "snarkjs proof verifies in Rust"
        );
        // and our snarkjs JSON round-trips exactly
        assert_eq!(SnarkjsProof::from_ark(&proof.to_ark().unwrap()), proof);
    }
}

#[test]
fn rejects_invalid_witness_inputs() {
    let case = cases().iter().find(|c| c["circuit"] == "withdraw").unwrap();
    let mut input = case["input"].clone();
    // withdraw more than the note holds
    let existing: u128 = input["existingValue"].as_str().unwrap().parse().unwrap();
    input["withdrawnValue"] = Value::String((existing + 1).to_string());
    let err = prove_withdraw(
        prover(Circuit::Withdraw),
        &WithdrawInputs::from_json(&input.to_string()).unwrap(),
    )
    .unwrap_err();
    assert!(matches!(err, Error::Witness(_)), "{err}");
    // wrong sibling count
    let mut input = case["input"].clone();
    input["stateSiblings"].as_array_mut().unwrap().pop();
    let err = prove_withdraw(
        prover(Circuit::Withdraw),
        &WithdrawInputs::from_json(&input.to_string()).unwrap(),
    )
    .unwrap_err();
    assert!(matches!(err, Error::Input(_)), "{err}");
    // wrong circuit
    let c = cases().iter().find(|c| c["circuit"] == "commitment").unwrap();
    let err = prove_commitment(
        prover(Circuit::Withdraw),
        &CommitmentInputs::from_json(&c["input"].to_string()).unwrap(),
    )
    .unwrap_err();
    assert!(matches!(err, Error::Circuit(_)));
    // mismatched artifacts
    assert!(CircuitProver::load(Circuit::Commitment, &read("withdraw.wasm"), &read("commitment.zkey")).is_err());
    assert!(CircuitProver::load(Circuit::Withdraw, &read("commitment.wasm"), &read("commitment.zkey")).is_err());
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}
