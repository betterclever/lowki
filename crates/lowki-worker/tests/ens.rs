//! ENS name + stealth payment requests and the app-list key, through the worker's message API.
//! Checked against lowki-ens / lowki-stealth (themselves vector-tested against viem, @scure and
//! the ScopeLift SDK), and written to `vectors/lowki-worker/out/ens-worker.json` for the viem
//! check `vectors/lowki-worker/ens-check.ts` (verifyTypedData, recoverAuthorizationAddress,
//! recoverMessageAddress, parseTransaction).
#![cfg(not(target_arch = "wasm32"))]

mod common;

use common::bytes_json;
use lowki_ens::{
    identity::IdentityKey,
    lowki_registrar::{Registration, recover_registration},
    owner_account::{authorization_digest, execute_digest},
    sig::recover_digest,
    stealth::{recover_personal, records_auth_message},
};
use lowki_stealth::{SecretKey, derive_stealth_keys, generate_stealth_address};
use lowki_worker::protocol::Request;
use lowki_worker::{Core, handle_blocking};
use serde_json::{Value, json};

const M: &str = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon art";
const M2: &str = "zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo zoo vote";
const REGISTRAR: &str = "0xB5CbecBd2a52C76cdceb65DdCc3bf8f4d924c03e";
const OWNER_ACCOUNT: &str = "0x8Ed05C155ABB6aef00481Ca6aEc2Fc4815089228";
const STEALTH_RESOLVER: &str = "0xb78F3D4F40Eb9b28139cc116A94d79F976da1Dc6";
const REGISTRY: &str = "0xEEF136dcce9AE2130028990f28eD260f11A1BDB3";
const FACTORY: &str = "0xDa70306C98E97eCe36F997a21368e53298572991";
const PR_IMPL: &str = "0x115eb53F0c60696633855F90b138178Fb40b2b2C";
const RECORDS: &str = "0x00000000000000000000000000000000000000aa";
const GATEWAY: &str = "https://gw.example/v1/ens/gateway";

fn worker(m: &str) -> Core {
    let mut c = Core::new(1e15, Box::new(rand_core::OsRng));
    let r = call(&mut c, json!({"type": "importMnemonic", "mnemonic": m, "prfOutput": bytes_json(&[5; 32])}));
    assert_eq!(r["type"], "walletCreated");
    c
}

fn call(c: &mut Core, v: Value) -> Value {
    let req: Request = serde_json::from_value(v.clone()).unwrap_or_else(|e| panic!("{e}: {v}"));
    serde_json::to_value(handle_blocking(c, req, 0.0)).unwrap()
}

fn h(s: &Value) -> Vec<u8> {
    hex::decode(s.as_str().unwrap().trim_start_matches("0x")).unwrap()
}
fn a20(s: &str) -> [u8; 20] {
    hex::decode(s.trim_start_matches("0x")).unwrap().try_into().unwrap()
}
fn seed(m: &str) -> [u8; 64] {
    *lowki_keys::mnemonic_to_seed(m).unwrap()
}

fn setup_req(index: u32) -> Value {
    json!({"type": "ensSignOwnerSetup", "chainId": "11155111", "name": "test-abc.lowki.eth", "ownerAccount": OWNER_ACCOUNT,
        "authNonce": "0", "execNonce": "0", "deadline": 1_900_000_000u64, "factory": FACTORY,
        "permissionedResolverImpl": PR_IMPL, "stealthResolver": STEALTH_RESOLVER, "registry": REGISTRY,
        "gatewayUrl": GATEWAY, "recordsResolver": RECORDS, "index": index})
}

#[test]
fn identity_registration_setup_and_records_auth() {
    let mut out = vec![];
    for m in [M, M2] {
        let mut c = worker(m);
        let s = seed(m);
        for index in [0u32, 1, 0x7fff_ffff] {
            let id = call(&mut c, json!({"type": "ensIdentity", "index": index}));
            let want = IdentityKey::from_seed(&s, index).unwrap();
            assert_eq!(id["address"], lowki_keys::to_checksum_address(&want.address()));
            assert_eq!(id["path"], format!("m/137'/0'/{index}'"));
            // Not an app account, not a stealth key.
            let app = call(&mut c, json!({"type": "appAccount", "origin": "https://app.example"}));
            assert_ne!(app["address"], id["address"]);

            let reg = call(&mut c, json!({"type": "ensSignRegistration", "chainId": "11155111", "registrar": REGISTRAR,
                "label": "test-abc", "duration": 31_536_000u64, "nonce": "0", "deadline": 1_900_000_000u64, "index": index}));
            assert_eq!(reg["owner"], id["address"]);
            let r = Registration { label: "test-abc".into(), owner: want.address(), resolver: [0; 20], duration: 31_536_000, nonce: 0, deadline: 1_900_000_000 };
            assert_eq!(recover_registration(11155111, &a20(REGISTRAR), &r, &h(&reg["signature"])).unwrap(), want.address());

            let set = call(&mut c, setup_req(index));
            assert_eq!(set["type"], "ensOwnerSetup", "{set}");
            assert_eq!(set["owner"], id["address"]);
            // Rebuild the calls the way lowki-server does and check the signature binds them.
            let keys = derive_stealth_keys(&s).unwrap();
            let meta = keys.meta_address();
            let p = lowki_worker::ens::SetupParams {
                name: "test-abc.lowki.eth",
                factory: a20(FACTORY),
                permissioned_resolver_impl: a20(PR_IMPL),
                stealth_resolver: a20(STEALTH_RESOLVER),
                registry: a20(REGISTRY),
                gateway_url: GATEWAY,
                records_resolver: a20(RECORDS),
            };
            let calls = lowki_worker::ens::owner_setup_calls(&want.address(), &meta, &p).unwrap();
            assert_eq!(calls.len(), 5);
            let d = execute_digest(11155111, &want.address(), &calls, 0, 1_900_000_000);
            assert_eq!(recover_digest(&d, &h(&set["executeSignature"])).unwrap(), want.address());
            let au = &set["authorization"];
            assert_eq!(au["address"], OWNER_ACCOUNT);
            let mut sig = h(&au["r"]);
            sig.extend(h(&au["s"]));
            sig.push(27 + au["yParity"].as_u64().unwrap() as u8);
            assert_eq!(recover_digest(&authorization_digest(11155111, &a20(OWNER_ACCOUNT), 0), &sig).unwrap(), want.address());
            assert_eq!(set["record"].as_str().unwrap().len(), 2 + 2 * 67);
            assert_eq!(set["metaAddress"], meta.to_uri());

            let ra = call(&mut c, json!({"type": "ensRecordsAuth", "name": "test-abc.lowki.eth", "timestamp": 1_800_000_000u64}));
            let signer = recover_personal(&records_auth_message("test-abc.lowki.eth", 1_800_000_000), &h(&ra["signature"])).unwrap();
            assert_eq!(signer, keys.viewing.address());
            assert_eq!(ra["viewingAddress"], lowki_keys::to_checksum_address(&keys.viewing.address()));

            out.push(json!({ "mnemonic": m, "index": index, "identity": id, "registration": reg, "setup": set,
                "setupCalls": calls.iter().map(|c| json!({"target": format!("0x{}", hex::encode(c.target)), "value": "0", "data": format!("0x{}", hex::encode(&c.data))})).collect::<Vec<_>>(),
                "recordsAuth": ra }));
        }
        // Label rules are enforced before signing.
        for bad in ["ab", "-abc", "abc-", "ab--c", "ABC", "a.b", "émile"] {
            let e = call(&mut c, json!({"type": "ensSignRegistration", "chainId": "11155111", "registrar": REGISTRAR,
                "label": bad, "duration": 1u64, "nonce": "0", "deadline": 1u64}));
            assert_eq!(e["code"], "invalidRequest", "{bad}");
        }
    }
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../vectors/lowki-worker/out");
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(format!("{dir}/ens-worker.json"), serde_json::to_string_pretty(&json!({
        "chainId": 11155111, "registrar": REGISTRAR, "ownerAccount": OWNER_ACCOUNT, "cases": out })).unwrap()).unwrap();
}

#[test]
fn scan_finds_only_own_payments_and_signs_from_them() {
    let mut c = worker(M);
    let keys = derive_stealth_keys(&seed(M)).unwrap();
    let other = derive_stealth_keys(&seed(M2)).unwrap();
    let mut records = vec![];
    let mut mine = vec![];
    for i in 1..=6u8 {
        let eph = SecretKey::from_bytes(&[i; 32]).unwrap();
        let meta = if i % 2 == 0 { other.meta_address() } else { keys.meta_address() };
        let g = generate_stealth_address(&meta, &eph).unwrap();
        let addr = lowki_keys::to_checksum_address(&g.stealth_address);
        if i % 2 == 1 {
            mine.push((addr.clone(), format!("0x{}", hex::encode(g.ephemeral_pubkey))));
        }
        records.push(json!({"stealthAddress": addr, "ephemeralPubkey": format!("0x{}", hex::encode(g.ephemeral_pubkey)), "viewTag": g.view_tag}));
    }
    // A forged record: our ephemeral key and view tag, someone else's address.
    let mut forged = records[0].clone();
    forged["stealthAddress"] = json!("0x00000000000000000000000000000000000000ff");
    records.push(forged);
    let r = call(&mut c, json!({"type": "stealthScan", "records": records}));
    assert_eq!(r["scanned"], 7);
    let found: Vec<String> = r["found"].as_array().unwrap().iter().map(|f| f["stealthAddress"].as_str().unwrap().to_string()).collect();
    assert_eq!(found, mine.iter().map(|m| m.0.clone()).collect::<Vec<_>>());

    let tx = json!({"type": "eip1559", "chainId": "11155111", "nonce": "0", "maxPriorityFeePerGas": "1000", "maxFeePerGas": "2000",
        "gas": "21000", "to": "0x00000000000000000000000000000000000000aa", "value": "12345"});
    let signed = call(&mut c, json!({"type": "stealthSignTransaction", "ephemeralPubkey": mine[0].1, "stealthAddress": mine[0].0, "tx": tx}));
    assert_eq!(signed["type"], "signedTransaction", "{signed}");
    assert_eq!(signed["address"], mine[0].0);
    // Wrong address for that ephemeral key: refused.
    let bad = call(&mut c, json!({"type": "stealthSignTransaction", "ephemeralPubkey": mine[0].1, "stealthAddress": mine[1].0, "tx": tx}));
    assert_eq!(bad["code"], "invalidRequest");
    let dir = concat!(env!("CARGO_MANIFEST_DIR"), "/../../vectors/lowki-worker/out");
    std::fs::create_dir_all(dir).unwrap();
    std::fs::write(format!("{dir}/stealth-tx.json"), serde_json::to_string_pretty(&json!({"from": mine[0].0, "tx": tx, "signed": signed})).unwrap()).unwrap();
}

#[test]
fn app_list_round_trip_and_key_separation() {
    let mut a = worker(M);
    let mut b = worker(M2);
    let data = r#"{"v":1,"apps":[{"origin":"https://app.example","name":"App"}]}"#;
    let s1 = call(&mut a, json!({"type": "appListSeal", "data": data}));
    let s2 = call(&mut a, json!({"type": "appListSeal", "data": data}));
    assert_ne!(s1["blob"], s2["blob"], "fresh nonce per seal");
    assert!(!s1["blob"].as_str().unwrap().contains("app.example"));
    assert_eq!(call(&mut a, json!({"type": "appListOpen", "blob": s1["blob"]}))["data"], data);
    assert_eq!(call(&mut b, json!({"type": "appListOpen", "blob": s1["blob"]}))["code"], "unwrapFailed");
    let mut t = h(&s1["blob"]);
    *t.last_mut().unwrap() ^= 1;
    assert_eq!(call(&mut a, json!({"type": "appListOpen", "blob": format!("0x{}", hex::encode(t))}))["code"], "unwrapFailed");
    // The key is domain separated: not the pairwise-ID key, not the KEK.
    let k = {
        let hk = hkdf::Hkdf::<sha2::Sha256>::new(None, &seed(M));
        let mut k = [0u8; 32];
        hk.expand(b"lowki/app-list/key/v1", &mut k).unwrap();
        k
    };
    let pairwise = {
        let hk = hkdf::Hkdf::<sha2::Sha256>::new(None, &seed(M));
        let mut k = [0u8; 32];
        hk.expand(lowki_worker::PAIRWISE_KEY_INFO, &mut k).unwrap();
        k
    };
    assert_ne!(k, pairwise);
    let big = "x".repeat(40 * 1024);
    assert_eq!(call(&mut a, json!({"type": "appListSeal", "data": big}))["code"], "invalidRequest");
    // Locked: nothing works.
    call(&mut a, json!({"type": "lock"}));
    assert_eq!(call(&mut a, json!({"type": "appListOpen", "blob": s1["blob"]}))["code"], "locked");
    assert_eq!(call(&mut a, json!({"type": "ensIdentity"}))["code"], "locked");
}
