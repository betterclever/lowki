//! Headless Chrome, inside a dedicated Web Worker (`run_in_dedicated_worker`), through the
//! real JS boundary (`LowkiWorker.handle` with JS objects and `Uint8Array`s):
//! create → lock → unlock → recover → same app address → sign → load circuits (WebGPU when
//! available) → prove withdraw → verify with lowki-groth16. Prints `LOWKI_PROOF_JSON` lines
//! for snarkjs and the on-chain verifier check. Run with `make test-chrome`.
#![cfg(target_arch = "wasm32")]

mod common;

use common::*;
use js_sys::{Function, Object, Reflect, Uint8Array};
use lowki_worker::wasm_api::LowkiWorker;
use serde_json::{Value, json};
use wasm_bindgen::JsValue;
use wasm_bindgen_test::*;

wasm_bindgen_test_configure!(run_in_dedicated_worker);

const W_WASM: &[u8] = include_bytes!("../../../../vendor/npm/circuits/package/artifacts/withdraw.wasm");
const W_ZKEY: &[u8] = include_bytes!("../../../../vendor/npm/circuits/package/artifacts/withdraw.zkey");
const W_VKEY: &str = include_str!("../../../../vendor/npm/circuits/package/artifacts/withdraw.vkey");
const C_WASM: &[u8] = include_bytes!("../../../../vendor/npm/circuits/package/artifacts/commitment.wasm");
const C_ZKEY: &[u8] = include_bytes!("../../../../vendor/npm/circuits/package/artifacts/commitment.zkey");

/// Fields that cross as Uint8Array.
const BYTE_FIELDS: [&str; 10] = [
    "prfOutput", "wrappedBlob", "wrappedPasskeyShare", "serverShare", "recoveryShare", "otherShare",
    "withdrawWasm", "withdrawZkey", "commitmentWasm", "commitmentZkey",
];

fn to_js(v: &Value) -> JsValue {
    let obj = js_sys::JSON::parse(&v.to_string()).unwrap();
    for f in BYTE_FIELDS {
        if let Some(arr) = v.get(f).and_then(|x| x.as_array()) {
            let b: Vec<u8> = arr.iter().map(|x| x.as_u64().unwrap() as u8).collect();
            Reflect::set(&obj, &f.into(), &Uint8Array::from(&b[..])).unwrap();
        }
    }
    obj
}

fn from_js(v: &JsValue) -> Value {
    // Uint8Array -> plain arrays, then JSON
    let f = Function::new_with_args("o", "return JSON.stringify(o, (k, v) => v instanceof Uint8Array ? Array.from(v) : v)");
    let s = f.call1(&JsValue::NULL, v).unwrap().as_string().unwrap();
    serde_json::from_str(&s).unwrap()
}

async fn call(w: &LowkiWorker, v: Value) -> Value {
    from_js(&w.handle(to_js(&v)).await)
}

fn now() -> f64 {
    js_sys::Date::now()
}

fn log(s: &str) {
    web_sys_log(s);
}

#[wasm_bindgen::prelude::wasm_bindgen]
extern "C" {
    #[wasm_bindgen(js_namespace = console, js_name = log)]
    fn web_sys_log(s: &str);
}

#[wasm_bindgen_test]
async fn worker_end_to_end_in_chrome() {
    let in_worker = Reflect::get(&js_sys::global(), &"WorkerGlobalScope".into()).map(|v| !v.is_undefined()).unwrap_or(false);
    log(&format!("LOWKI_ENV dedicated_worker={in_worker} ua={:?}",
        Reflect::get(&Reflect::get(&js_sys::global(), &"navigator".into()).unwrap(), &"userAgent".into()).unwrap().as_string()));
    let w = LowkiWorker::new(Some(15.0));
    let prf = [0x5a_u8; 32];

    let created = call(&w, json!({"type": "createWallet", "prfOutput": bytes_json(&prf)})).await;
    assert_eq!(created["type"], "walletCreated", "{created}");
    let a0 = call(&w, json!({"type": "appAccount", "origin": ORIGIN})).await;
    let addr = a0["address"].clone();
    assert!(w.is_unlocked());

    assert_eq!(call(&w, json!({"type": "lock"})).await["type"], "locked");
    assert!(!w.is_unlocked());
    assert_eq!(call(&w, json!({"type": "appAccount", "origin": ORIGIN})).await["code"], "locked");

    let u = call(&w, json!({"type": "unlock", "prfOutput": bytes_json(&prf), "wrappedBlob": created["wrappedBlob"]})).await;
    assert_eq!(u["type"], "unlocked", "{u}");
    assert_eq!(call(&w, json!({"type": "appAccount", "origin": ORIGIN})).await["address"], addr);
    w.lock();

    let r = call(&w, json!({"type": "recover", "serverShare": created["serverShare"], "recoveryShare": created["recoveryShare"]})).await;
    assert_eq!(r["type"], "unlocked", "{r}");
    let a1 = call(&w, json!({"type": "appAccount", "origin": ORIGIN})).await;
    assert_eq!(a1["address"], addr);
    assert_eq!(a1["pairwiseUserId"], a0["pairwiseUserId"]);
    log(&format!("LOWKI_FLOW create->lock->unlock->recover ok, address {}", addr));

    let sig = call(&w, json!({"type": "signMessage", "origin": ORIGIN, "message": "hello from the worker"})).await;
    assert_eq!(sig["address"], addr);
    let td = json!({"types": {"Mail": [{"name": "contents", "type": "string"}]}, "primaryType": "Mail",
                    "domain": {"name": "Ether Mail", "version": "1", "chainId": 1}, "message": {"contents": "hi"}});
    let sig2 = call(&w, json!({"type": "signTypedData", "origin": ORIGIN, "typedData": td})).await;
    assert_eq!(sig2["type"], "signature", "{sig2}");
    log(&format!("LOWKI_SIGN personal_sign {} typed {}", sig["signature"], sig2["signature"]));

    // circuits: request WebGPU, fall back to CPU
    let t = now();
    // artifacts go straight in as Uint8Array (no JSON round trip for 20+ MB)
    let req = js_sys::JSON::parse(r#"{"type":"ppLoadCircuits","useGpu":true}"#).unwrap();
    for (k, b) in [("withdrawWasm", W_WASM), ("withdrawZkey", W_ZKEY), ("commitmentWasm", C_WASM), ("commitmentZkey", C_ZKEY)] {
        Reflect::set(&req, &k.into(), &Uint8Array::from(b)).unwrap();
    }
    let loaded = from_js(&w.handle(req).await);
    assert_eq!(loaded["type"], "circuitsLoaded", "{loaded}");
    log(&format!("LOWKI_LOAD {} in {:.0} ms", loaded, now() - t));

    let n0 = call(&w, json!({"type": "ppNote", "note": note(0, 0), "value": EXISTING_VALUE, "label": label()})).await;
    let adapter = "0x521F578Ac6E894939364Dc27c7e6978Ecf7b7926";
    let mut proofs = vec![];
    for i in 0..2 {
        let p = call(&w, withdraw_request(n0["commitment"].as_str().unwrap(), 100, adapter)).await;
        assert_eq!(p["type"], "proof", "{p}");
        log(&format!("LOWKI_TIMING withdraw#{i} backend={} {:.0} ms", p["backend"], p["ms"].as_f64().unwrap()));
        proofs.push(p);
    }
    let p = &proofs[1];
    let sigs: Vec<String> = serde_json::from_value(p["publicSignals"].clone()).unwrap();
    assert_eq!(sigs[1], n0["nullifierHash"].as_str().unwrap());
    assert_eq!(sigs[7], context_for(adapter));
    let vk = lowki_groth16::parse_vkey_json(W_VKEY).unwrap();
    let sp: lowki_groth16::SnarkjsProof = serde_json::from_value(p["proof"].clone()).unwrap();
    assert!(lowki_groth16::verify(&vk, &sp, &sigs).unwrap());
    log("LOWKI_VERIFY lowki-groth16 verify(withdraw) = true");
    for (i, p) in proofs.iter().enumerate() {
        let line = json!({"circuit": "withdraw", "name": format!("chrome_worker_withdraw_{i}_{}", p["backend"].as_str().unwrap()),
            "proof": p["proof"], "publicSignals": p["publicSignals"], "solidity": p["solidity"]});
        log(&format!("LOWKI_PROOF_JSON {line}"));
    }
    let cp = call(&w, json!({"type": "ppProveCommitment", "note": note(0, 0), "value": EXISTING_VALUE, "label": label()})).await;
    assert_eq!(cp["type"], "proof", "{cp}");
    log(&format!("LOWKI_TIMING commitment backend={} {:.0} ms", cp["backend"], cp["ms"].as_f64().unwrap()));
    let line = json!({"circuit": "commitment", "name": format!("chrome_worker_commitment_{}", cp["backend"].as_str().unwrap()),
        "proof": cp["proof"], "publicSignals": cp["publicSignals"], "solidity": cp["solidity"]});
    log(&format!("LOWKI_PROOF_JSON {line}"));

    // CPU path (no WebGPU requested): same API, backend "cpu"
    let cpu = LowkiWorker::new(Some(15.0));
    call(&cpu, json!({"type": "recover", "serverShare": created["serverShare"], "recoveryShare": created["recoveryShare"]})).await;
    let req = js_sys::JSON::parse(r#"{"type":"ppLoadCircuits","useGpu":false}"#).unwrap();
    Reflect::set(&req, &"withdrawWasm".into(), &Uint8Array::from(W_WASM)).unwrap();
    Reflect::set(&req, &"withdrawZkey".into(), &Uint8Array::from(W_ZKEY)).unwrap();
    let l = from_js(&cpu.handle(req).await);
    assert_eq!(l["gpu"], false, "{l}");
    let pc = call(&cpu, withdraw_request(n0["commitment"].as_str().unwrap(), 100, adapter)).await;
    assert_eq!(pc["backend"], "cpu", "{pc}");
    let sigs: Vec<String> = serde_json::from_value(pc["publicSignals"].clone()).unwrap();
    let sp: lowki_groth16::SnarkjsProof = serde_json::from_value(pc["proof"].clone()).unwrap();
    assert!(lowki_groth16::verify(&vk, &sp, &sigs).unwrap());
    log(&format!("LOWKI_TIMING withdraw backend=\"cpu\" {:.0} ms (verified)", pc["ms"].as_f64().unwrap()));
    let line = json!({"circuit": "withdraw", "name": "chrome_worker_withdraw_cpu",
        "proof": pc["proof"], "publicSignals": pc["publicSignals"], "solidity": pc["solidity"]});
    log(&format!("LOWKI_PROOF_JSON {line}"));

    // auto-lock timer: a 0.002-minute (120 ms) worker locks itself without any message
    let w2 = LowkiWorker::new(Some(0.002));
    call(&w2, json!({"type": "createWallet", "prfOutput": bytes_json(&prf)})).await;
    assert!(w2.is_unlocked());
    sleep_ms(400).await;
    assert!(!w2.is_unlocked(), "timer did not lock");
    log("LOWKI_AUTOLOCK timer locked the session after 120 ms with no message");
}

async fn sleep_ms(ms: i32) {
    let p = js_sys::Promise::new(&mut |res, _| {
        let st = Reflect::get(&js_sys::global(), &"setTimeout".into()).unwrap();
        let st: Function = st.into();
        st.call2(&JsValue::NULL, &res, &JsValue::from(ms)).unwrap();
    });
    wasm_bindgen_futures::JsFuture::from(p).await.unwrap();
}

#[allow(dead_code)]
fn _unused(_: Object) {}
