//! Groth16 helpers shared by the native binary and the wasm module.
use lowki_groth16::*;

pub enum Inputs {
    Commitment(CommitmentInputs),
    Withdraw(WithdrawInputs),
}

pub fn circuit(name: &str) -> Circuit {
    match name {
        "commitment" => Circuit::Commitment,
        "withdraw" => Circuit::Withdraw,
        _ => panic!("unknown circuit {name}"),
    }
}

/// Parses the snarkjs-style input JSON (done inside the timed region, like snarkjs fullProve's
/// unstringifyBigInts).
pub fn parse(c: Circuit, json: &str) -> Inputs {
    match c {
        Circuit::Commitment => Inputs::Commitment(CommitmentInputs::from_json(json).unwrap()),
        Circuit::Withdraw => Inputs::Withdraw(WithdrawInputs::from_json(json).unwrap()),
    }
}

/// Witness only (wasmi running the circom witness calculator). Returns the witness length.
pub fn witness(p: &CircuitProver, json: &str) -> usize {
    match parse(p.circuit(), json) {
        Inputs::Commitment(i) => p.witness(&i.as_signals()).unwrap().len(),
        Inputs::Withdraw(i) => p.witness(&i.as_signals()).unwrap().len(),
    }
}

/// Full proof (witness + QAP/FFTs + MSMs + 3-pairing self-check), CPU.
pub fn prove(p: &CircuitProver, json: &str) -> ProofOutput {
    match parse(p.circuit(), json) {
        Inputs::Commitment(i) => prove_commitment(p, &i).unwrap(),
        Inputs::Withdraw(i) => prove_withdraw(p, &i).unwrap(),
    }
}

#[cfg(feature = "webgpu")]
pub async fn prove_gpu(p: &GpuProver, json: &str) -> ProofOutput {
    match parse(p.cpu().circuit(), json) {
        Inputs::Commitment(i) => prove_commitment_gpu(p, &i).await.unwrap(),
        Inputs::Withdraw(i) => prove_withdraw_gpu(p, &i).await.unwrap(),
    }
}

/// `{proof, publicSignals}` as snarkjs prints them, for snarkjs.groth16.verify.
pub fn to_json(o: &ProofOutput) -> String {
    serde_json::json!({ "proof": o.proof, "publicSignals": o.public_signals }).to_string()
}
