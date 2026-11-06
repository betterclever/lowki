//! Shared test scenario: a real LeanIMT state tree containing the worker's own note
//! commitment, an ASP tree containing its label, and the withdraw request built from them
//! exactly as Kohaku's withdrawThunk would (proofFromTree, padded to 32).
#![allow(dead_code)]

use lowki_pp_core::{Fr, LeanImt, fr_from_be_bytes_mod_order, proof_from_tree};
use serde_json::{Value, json};

pub const ORIGIN: &str = "https://app.example";
pub const ENTRYPOINT: &str = "0x34A2068192b1297f2a7f85D7D8CdE66F8F0921cB";
pub const CHAIN_ID: &str = "11155111";
/// Sepolia ETH pool SCOPE() (read on-chain; see vectors/lowki-worker/onchain-verifier.ts).
pub const SCOPE: &str = "13541713702858359530363969798588891965037210808099002426745892519913535247342";
pub const EXISTING_VALUE: &str = "1000000000000000000";
pub const WITHDRAWN_VALUE: &str = "250000000000000000";

pub fn note(deposit_index: u64, secret_index: u64) -> Value {
    json!({"accountIndex": 0, "chainId": CHAIN_ID, "entrypoint": ENTRYPOINT,
           "depositIndex": deposit_index, "secretIndex": secret_index})
}

pub fn fr_dec(x: &Fr) -> String {
    lowki_worker::util::fr_to_dec(x)
}

pub fn parse_fr(s: &str) -> Fr {
    lowki_worker::util::fr(s, "test").unwrap()
}

fn filler(tag: &str, i: u64) -> Fr {
    use sha3::{Digest, Keccak256};
    let mut h = Keccak256::new();
    h.update(tag.as_bytes());
    h.update(i.to_be_bytes());
    fr_from_be_bytes_mod_order(&h.finalize().into())
}

/// The label of our (pretend) deposit.
pub fn label() -> String {
    fr_dec(&filler("label", 7))
}

/// Builds the ppProveWithdraw request given the note's commitment (from ppNote).
/// `n_state` filler leaves are placed around the commitment.
pub fn withdraw_request(commitment: &str, n_state: u64, processooor: &str) -> Value {
    let c = parse_fr(commitment);
    let mut state = LeanImt::new();
    for i in 0..n_state {
        state.insert(filler("state", i));
        if i == n_state / 2 {
            state.insert(c);
        }
    }
    if n_state == 0 {
        state.insert(c);
    }
    let sp = proof_from_tree(&state, &c).unwrap();
    let lbl = parse_fr(&label());
    let mut asp = LeanImt::new();
    for i in 0..5 {
        asp.insert(filler("asp", i));
    }
    asp.insert(lbl);
    let ap = proof_from_tree(&asp, &lbl).unwrap();
    let mp = |p: &lowki_pp_core::MerkleProof| {
        json!({"root": fr_dec(&p.root), "index": p.index.to_string(),
               "siblings": p.siblings.iter().map(fr_dec).collect::<Vec<_>>()})
    };
    json!({
        "type": "ppProveWithdraw",
        "note": note(0, 0),
        "label": label(),
        "existingValue": EXISTING_VALUE,
        "withdrawnValue": WITHDRAWN_VALUE,
        "context": {"withdrawal": {"processooor": processooor, "data": "0x1234"}, "scope": SCOPE},
        "stateProof": mp(&sp),
        "aspProof": mp(&ap),
    })
}

pub fn context_for(processooor: &str) -> String {
    let p = lowki_worker::util::address(processooor, "p").unwrap();
    let scope = lowki_worker::util::u256(SCOPE, "s").unwrap().to_be_bytes::<32>();
    fr_dec(&lowki_pp_core::calculate_context(&p.0.0, &[0x12, 0x34], &scope))
}

pub fn bytes_of(v: &Value) -> Vec<u8> {
    v.as_array().unwrap().iter().map(|x| x.as_u64().unwrap() as u8).collect()
}

pub fn bytes_json(b: &[u8]) -> Value {
    Value::Array(b.iter().map(|&x| json!(x)).collect())
}
