//! Differential tests against `vectors/lowki-worker/vectors.json` (viem 2.54.1, @scure 2.2.0,
//! @noble/hashes 2.2.0; see gen.ts). Every case goes through the worker's message API.
#![cfg(not(target_arch = "wasm32"))]

mod common;

use common::bytes_json;
use lowki_worker::protocol::Request;
use lowki_worker::{Core, handle_blocking};
use serde_json::{Value, json};

const VECTORS: &str = include_str!("../../../vectors/lowki-worker/vectors.json");

fn workers(v: &Value) -> Vec<Core> {
    v["mnemonics"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| {
            let mut c = Core::new(1e15, Box::new(rand_core::OsRng));
            let r = call(&mut c, json!({"type": "importMnemonic", "mnemonic": m, "prfOutput": bytes_json(&[5; 32])}));
            assert_eq!(r["type"], "walletCreated");
            c
        })
        .collect()
}

fn call(c: &mut Core, v: Value) -> Value {
    let req: Request = serde_json::from_value(v.clone()).unwrap_or_else(|e| panic!("{e}: {v}"));
    serde_json::to_value(handle_blocking(c, req, 0.0)).unwrap()
}

fn m(case: &Value) -> usize {
    case["mnemonic"].as_u64().unwrap() as usize
}

#[test]
fn accounts_and_pairwise_ids() {
    let v: Value = serde_json::from_str(VECTORS).unwrap();
    let mut w = workers(&v);
    let cases = v["accounts"].as_array().unwrap();
    for c in cases {
        let r = call(&mut w[m(c)], json!({"type": "appAccount", "origin": c["origin"]}));
        assert_eq!(r["address"], c["address"], "{c}");
        assert_eq!(r["pairwiseUserId"], c["pairwiseUserId"], "{c}");
        assert_eq!(r["appIndex"], c["appIndex"]);
    }
    assert_eq!(cases.len(), 18);
}

#[test]
fn personal_sign_equals_viem() {
    let v: Value = serde_json::from_str(VECTORS).unwrap();
    let mut w = workers(&v);
    for c in v["messages"].as_array().unwrap() {
        let mut req = json!({"type": "signMessage", "origin": c["origin"]});
        if let Some(t) = c.get("message") {
            req["message"] = t.clone();
        } else {
            req["messageHex"] = c["messageHex"].clone();
        }
        let r = call(&mut w[m(c)], req);
        assert_eq!(r["signature"], c["signature"], "{c}");
    }
}

#[test]
fn typed_data_equals_viem() {
    let v: Value = serde_json::from_str(VECTORS).unwrap();
    let mut w = workers(&v);
    for c in v["typed"].as_array().unwrap() {
        let r = call(&mut w[m(c)], json!({"type": "signTypedData", "origin": c["origin"], "typedData": c["typedData"]}));
        assert_eq!(r["signature"], c["signature"], "{c} -> {r}");
    }
}

#[test]
fn transactions_equal_viem() {
    let v: Value = serde_json::from_str(VECTORS).unwrap();
    let mut w = workers(&v);
    let mut n = [0; 2];
    for c in v["txs"].as_array().unwrap() {
        let r = call(&mut w[m(c)], json!({"type": "signTransaction", "origin": c["origin"], "tx": c["tx"]}));
        assert_eq!(r["rawTransaction"], c["raw"], "{c} -> {r}");
        assert_eq!(r["hash"], c["hash"]);
        n[(c["tx"]["type"] == "eip7702") as usize] += 1;
    }
    assert_eq!(n, [54, 54]);
}

#[test]
fn authorizations_equal_viem() {
    let v: Value = serde_json::from_str(VECTORS).unwrap();
    let mut w = workers(&v);
    for c in v["auths"].as_array().unwrap() {
        let r = call(&mut w[m(c)], json!({"type": "signAuthorization", "origin": c["origin"], "chainId": c["chainId"],
            "address": c["address"], "nonce": c["nonce"]}));
        let a = &r["authorization"];
        assert_eq!(a["yParity"], c["yParity"], "{c} -> {r}");
        assert_eq!(a["r"], c["r"]);
        assert_eq!(a["s"], c["s"]);
    }
}

#[test]
fn tx_rejections() {
    let v: Value = serde_json::from_str(VECTORS).unwrap();
    let mut w = workers(&v);
    let tx = v["txs"][1]["tx"].clone(); // an eip7702 tx
    assert_eq!(tx["type"], "eip7702");
    let mut no_to = tx.clone();
    no_to["to"] = Value::Null;
    let mut no_auth = tx.clone();
    no_auth["authorizationList"] = json!([]);
    let mut tip = tx.clone();
    tip["maxPriorityFeePerGas"] = json!("100000000000000000000");
    let mut kind = tx.clone();
    kind["type"] = json!("legacy");
    for bad in [no_to, no_auth, tip, kind] {
        let r = call(&mut w[0], json!({"type": "signTransaction", "origin": "https://app.example", "tx": bad}));
        assert_eq!(r["code"], "invalidRequest", "{r}");
    }
}
