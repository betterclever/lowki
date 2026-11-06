//! Wallet home endpoints: the encrypted app list, the public app name, and sponsored name
//! claims (src/ens_names.rs) against a mock JSON-RPC chain that records the operator's raw
//! transactions. The live run is apps/wallet-ui/tests/e2e/home.mjs.

mod common;

use std::{
    net::SocketAddr,
    sync::{Arc, Mutex},
    time::Duration,
};

use axum::{Json, Router, extract::State, routing::post};
use lowki_ens::{
    identity::IdentityKey,
    lowki_registrar::{Registration, encode_register, sign_registration},
    owner_account::{encode_execute, sign_authorization, sign_execute},
    sig::recover_digest,
    stealth::encode_stealth_record,
};
use lowki_server::{
    config::Limit,
    ens_gateway::GatewayConfig,
    ens_names::{NamesConfig, owner_setup_calls},
};
use lowki_stealth::{SecretKey, StealthMetaAddress, keccak256};
use serde_json::{Value, json};

const REGISTRAR: [u8; 20] = [0xa1; 20];
const REGISTRY: [u8; 20] = [0xa2; 20];
const OWNER_ACCOUNT: [u8; 20] = [0xa3; 20];
const FACTORY: [u8; 20] = [0xa4; 20];
const PR_IMPL: [u8; 20] = [0xa5; 20];
const STEALTH: [u8; 20] = [0x5e; 20];
const RECORDS: [u8; 20] = [0xbb; 20];
const OPERATOR: [u8; 32] = [0x42; 32];

#[derive(Default)]
struct Chain {
    sent: Mutex<Vec<Vec<u8>>>,
    taken: Mutex<Vec<String>>,
}

fn word(v: u64) -> String {
    format!("0x{:064x}", v)
}

async fn rpc(State(c): State<Arc<Chain>>, Json(req): Json<Value>) -> Json<Value> {
    let m = req["method"].as_str().unwrap();
    let p = &req["params"];
    let result = match m {
        "eth_call" => {
            let to = p[0]["to"].as_str().unwrap().to_string();
            let data = p[0]["data"].as_str().unwrap().to_string();
            if to == format!("0x{}", hex::encode(REGISTRAR)) && data.starts_with(&format!("0x{}", hex::encode(lowki_ens::abi::selector("available(string)")))) {
                let label_taken = c.taken.lock().unwrap().iter().any(|l| data.contains(&hex::encode(l.as_bytes())));
                json!(word(u64::from(!label_taken)))
            } else if to == format!("0x{}", hex::encode(FACTORY)) {
                json!(format!("0x{:0>64}", hex::encode(RECORDS)))
            } else if to == format!("0x{}", hex::encode(REGISTRAR)) {
                json!(word(0)) // nonces(owner)
            } else {
                json!("0x") // owner nonce() before delegation
            }
        }
        "eth_getTransactionCount" => json!("0x0"),
        "eth_getBlockByNumber" => json!({ "baseFeePerGas": "0x10" }),
        "eth_maxPriorityFeePerGas" => json!("0x1"),
        "eth_estimateGas" => json!("0x30000"),
        "eth_sendRawTransaction" => {
            let raw = hex::decode(p[0].as_str().unwrap().trim_start_matches("0x")).unwrap();
            let h = keccak256(&raw);
            c.sent.lock().unwrap().push(raw);
            json!(format!("0x{}", hex::encode(h)))
        }
        "eth_getTransactionReceipt" => json!({ "status": "0x1" }),
        other => panic!("unexpected {other}"),
    };
    Json(json!({ "jsonrpc": "2.0", "id": req["id"], "result": result }))
}

async fn setup() -> (common::TestServer, Arc<Chain>) {
    let chain = Arc::new(Chain::default());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr: SocketAddr = listener.local_addr().unwrap();
    let app = Router::new().route("/", post(rpc)).with_state(chain.clone());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let server = common::spawn(|c| {
        c.ens_gateway = Some(GatewayConfig {
            rpc_url: format!("http://{addr}/"),
            universal_resolver: [0xee; 20],
            stealth_resolver: Some(STEALTH),
            parent: "lowki.eth".into(),
            lookups_per_ip: Limit::new(100, 60),
            lookups_per_name: Limit::new(100, 3600),
            records_per_ip: Limit::new(100, 60),
            meta_cache: Duration::from_secs(30),
            auth_skew: Duration::from_secs(300),
            records_page: 500,
        });
        c.ens_names = Some(names_config());
    })
    .await;
    (server, chain)
}

fn names_config() -> NamesConfig {
    NamesConfig {
        operator_key: OPERATOR,
        chain_id: 11155111,
        registrar: REGISTRAR,
        registry: REGISTRY,
        owner_account: OWNER_ACCOUNT,
        factory: FACTORY,
        permissioned_resolver_impl: PR_IMPL,
        gateway_url: "https://gw.example/v1/ens/gateway".into(),
        duration_secs: 365 * 86400,
        claims_per_ip: Limit::new(100, 3600),
        claims_per_user: Limit::new(100, 3600),
    }
}

fn h(b: &[u8]) -> String {
    format!("0x{}", hex::encode(b))
}

/// Recovers the sender of a raw type-2/4 transaction and returns `(type, sender, to, data)`.
fn decode_tx(raw: &[u8]) -> (u8, [u8; 20], Vec<u8>, Vec<u8>) {
    // Tiny RLP reader for the test.
    fn item(b: &[u8]) -> (usize, usize, bool) {
        let p = b[0];
        match p {
            0..=0x7f => (0, 1, false),
            0x80..=0xb7 => (1, (p - 0x80) as usize, false),
            0xb8..=0xbf => {
                let n = (p - 0xb7) as usize;
                (1 + n, b[1..1 + n].iter().fold(0usize, |a, &x| a * 256 + x as usize), false)
            }
            0xc0..=0xf7 => (1, (p - 0xc0) as usize, true),
            _ => {
                let n = (p - 0xf7) as usize;
                (1 + n, b[1..1 + n].iter().fold(0usize, |a, &x| a * 256 + x as usize), true)
            }
        }
    }
    fn items(b: &[u8]) -> Vec<(usize, usize)> {
        let mut out = vec![];
        let mut i = 0;
        while i < b.len() {
            let (h, l, _) = item(&b[i..]);
            out.push((i, h + l));
            i += h + l;
        }
        out
    }
    let ty = raw[0];
    let (hl, l, _) = item(&raw[1..]);
    let body = &raw[1 + hl..1 + hl + l];
    let parts = items(body);
    let payload = |k: usize| {
        let (s, len) = parts[k];
        let (hh, _, _) = item(&body[s..]);
        body[s + hh..s + len].to_vec()
    };
    let n = parts.len();
    let unsigned_len: usize = parts[..n - 3].iter().map(|p| p.1).sum();
    let mut pre = vec![ty];
    lowki_server::ens_names::rlp::list(&mut pre, &body[..unsigned_len]);
    let mut sig = vec![0u8; 65];
    let r = payload(n - 2);
    let s = payload(n - 1);
    sig[32 - r.len()..32].copy_from_slice(&r);
    sig[64 - s.len()..64].copy_from_slice(&s);
    sig[64] = 27 + payload(n - 3).first().copied().unwrap_or(0);
    let from = recover_digest(&keccak256(&pre), &sig).unwrap();
    (ty, from, payload(5), payload(7))
}

#[tokio::test]
async fn app_list_blob_is_opaque_versioned_and_per_user() {
    let (s, _) = setup().await;
    let c = s.browser();
    let csrf = s.email_login(&c, "a@example.com").await;
    let r: Value = c.get(s.u("/v1/me/app-list")).send().await.unwrap().json().await.unwrap();
    assert_eq!(r, json!({ "blob": null, "version": 0 }));
    let put = |blob: &str, version: i64, csrf: Option<&str>| {
        let mut rq = c.put(s.u("/v1/me/app-list")).json(&json!({ "blob": blob, "version": version }));
        if let Some(t) = csrf {
            rq = rq.header("x-lowki-csrf", t);
        }
        rq.send()
    };
    assert_eq!(put("AQID", 0, None).await.unwrap().status(), 403, "CSRF required");
    let r = put("AQID", 0, Some(&csrf)).await.unwrap();
    assert_eq!(r.status(), 200);
    assert_eq!(r.json::<Value>().await.unwrap()["version"], 1);
    assert_eq!(put("BBBB", 0, Some(&csrf)).await.unwrap().status(), 409, "stale version");
    assert_eq!(put("BBBB", 1, Some(&csrf)).await.unwrap().status(), 200);
    let r: Value = c.get(s.u("/v1/me/app-list")).send().await.unwrap().json().await.unwrap();
    assert_eq!(r, json!({ "blob": "BBBB", "version": 2 }));
    assert_eq!(put("not base64!", 2, Some(&csrf)).await.unwrap().status(), 400);
    let big = "A".repeat(50 * 1024);
    assert_eq!(put(&big, 2, Some(&csrf)).await.unwrap().status(), 400);
    // Another user sees their own (empty) list; no session sees nothing.
    let c2 = s.browser();
    s.email_login(&c2, "b@example.com").await;
    let r: Value = c2.get(s.u("/v1/me/app-list")).send().await.unwrap().json().await.unwrap();
    assert_eq!(r["blob"], Value::Null);
    assert_eq!(reqwest::get(s.u("/v1/me/app-list")).await.unwrap().status(), 401);
}

#[tokio::test]
async fn public_app_name_and_token_name() {
    let (s, _) = setup().await;
    let app = s.create_app(&["https://tips.example"]).await;
    let id = app["id"].as_str().unwrap();
    let r: Value = reqwest::get(s.u(&format!("/v1/apps/{id}"))).await.unwrap().json().await.unwrap();
    assert_eq!(r, json!({ "id": id, "name": "Test app", "origin": "https://tips.example" }));
    assert_eq!(reqwest::get(s.u("/v1/apps/app_nope")).await.unwrap().status(), 404);
    let c = s.browser();
    s.email_login(&c, "t@example.com").await;
    let t: Value = s.app_token(&c, id, "https://tips.example").await.json().await.unwrap();
    assert_eq!(t["app_name"], "Test app");
    assert_eq!(t["app_origin"], "https://tips.example");
}

struct Wallet {
    id: IdentityKey,
    meta: StealthMetaAddress,
}

fn wallet(n: u8) -> Wallet {
    let seed = [n; 64];
    let spending = SecretKey::from_bytes(&[n.wrapping_add(1); 32]).unwrap();
    let viewing = SecretKey::from_bytes(&[n.wrapping_add(2); 32]).unwrap();
    Wallet { id: IdentityKey::from_seed(&seed, 0).unwrap(), meta: StealthMetaAddress::new(&spending.public_key(), &viewing.public_key()).unwrap() }
}

#[tokio::test]
async fn claim_registers_then_sets_up_only_what_the_owner_signed() {
    let (s, chain) = setup().await;
    let c = s.browser();
    let csrf = s.email_login(&c, "n@example.com").await;
    let w = wallet(3);
    let owner = w.id.address();
    let post = |path: &'static str, body: Value| c.post(s.u(path)).header("x-lowki-csrf", &csrf).json(&body).send();

    let me: Value = c.get(s.u("/v1/names/me")).send().await.unwrap().json().await.unwrap();
    assert_eq!(me["name"], Value::Null);
    assert_eq!(me["enabled"], true);

    for bad in ["ab", "-abc", "Abc", "a_b", "ab--cd", "émile"] {
        let r = post("/v1/names/prepare", json!({ "label": bad, "owner": h(&owner) })).await.unwrap();
        assert_eq!(r.status(), 400, "{bad}");
    }
    let p: Value = post("/v1/names/prepare", json!({ "label": "test-one", "owner": h(&owner) })).await.unwrap().json().await.unwrap();
    assert_eq!(p["name"], "test-one.lowki.eth");
    assert_eq!(p["records_resolver"], h(&RECORDS));
    assert_eq!(p["exec_nonce"], 0);
    let deadline = p["deadline"].as_u64().unwrap();

    // A registration signed by someone else is refused before anything is sent.
    let other = wallet(9);
    let reg = Registration { label: "test-one".into(), owner, resolver: [0; 20], duration: 365 * 86400, nonce: 0, deadline };
    let forged = sign_registration(&other.id, 11155111, &REGISTRAR, &reg);
    let r = post("/v1/names/register", json!({ "label": "test-one", "owner": h(&owner), "nonce": 0, "deadline": deadline, "signature": h(&forged) })).await.unwrap();
    assert_eq!(r.status(), 400);
    assert!(chain.sent.lock().unwrap().is_empty());

    let sig = sign_registration(&w.id, 11155111, &REGISTRAR, &reg);
    let r = post("/v1/names/register", json!({ "label": "test-one", "owner": h(&owner), "nonce": 0, "deadline": deadline, "signature": h(&sig) })).await.unwrap();
    assert_eq!(r.status(), 200, "{}", r.text().await.unwrap());
    {
        let sent = chain.sent.lock().unwrap();
        assert_eq!(sent.len(), 1);
        let (ty, from, to, data) = decode_tx(&sent[0]);
        assert_eq!(ty, 2);
        assert_eq!(from, SecretKey::from_bytes(&OPERATOR).unwrap().address(), "sent by the operator");
        assert_eq!(to, REGISTRAR);
        assert_eq!(data, encode_register(&reg, &sig));
    }

    // One name per user.
    let r = post("/v1/names/prepare", json!({ "label": "test-two", "owner": h(&owner) })).await.unwrap();
    assert_eq!(r.status(), 409);
    // Someone else can't take it.
    let c2 = s.browser();
    let csrf2 = s.email_login(&c2, "m@example.com").await;
    let r = c2.post(s.u("/v1/names/prepare")).header("x-lowki-csrf", &csrf2).json(&json!({ "label": "test-one", "owner": h(&other.id.address()) })).send().await.unwrap();
    assert_eq!(r.status(), 409);

    // Setup: a batch signature over different calls (another gateway URL) is refused.
    let cfg = names_config();
    let calls = owner_setup_calls(&cfg, &STEALTH, "test-one.lowki.eth", &owner, &w.meta, &RECORDS).unwrap();
    let mut evil_cfg = names_config();
    evil_cfg.gateway_url = "https://evil.example".into();
    let evil = owner_setup_calls(&evil_cfg, &STEALTH, "test-one.lowki.eth", &owner, &w.meta, &RECORDS).unwrap();
    let auth = sign_authorization(&w.id, 11155111, &OWNER_ACCOUNT, 0);
    let auth_json = json!({ "chainId": "11155111", "address": h(&OWNER_ACCOUNT), "nonce": "0", "yParity": auth.y_parity, "r": h(&auth.r), "s": h(&auth.s) });
    let record = h(&encode_stealth_record(true, &w.meta));
    let bad_sig = sign_execute(&w.id, 11155111, &owner, &evil, 0, deadline).unwrap();
    let r = post("/v1/names/setup", json!({ "record": record, "exec_nonce": 0, "deadline": deadline, "execute_signature": h(&bad_sig), "authorization": auth_json })).await.unwrap();
    assert_eq!(r.status(), 400);
    let exec_sig = sign_execute(&w.id, 11155111, &owner, &calls, 0, deadline).unwrap();
    let r = post("/v1/names/setup", json!({ "record": record, "exec_nonce": 0, "deadline": deadline, "execute_signature": h(&exec_sig), "authorization": auth_json })).await.unwrap();
    assert_eq!(r.status(), 200, "{}", r.text().await.unwrap());
    let body: Value = r.json().await.unwrap();
    assert_eq!(body["status"], "ready");
    assert_eq!(body["name"], "test-one.lowki.eth");
    {
        let sent = chain.sent.lock().unwrap();
        assert_eq!(sent.len(), 2);
        let (ty, from, to, data) = decode_tx(&sent[1]);
        assert_eq!(ty, 4, "EIP-7702 transaction");
        assert_eq!(from, SecretKey::from_bytes(&OPERATOR).unwrap().address());
        assert_eq!(to, owner, "sent to the owner's delegated EOA");
        assert_eq!(data, encode_execute(&calls, deadline, &exec_sig));
    }
    let me: Value = c.get(s.u("/v1/names/me")).send().await.unwrap().json().await.unwrap();
    assert_eq!(me["status"], "ready");
    assert!(me["register_tx"].as_str().unwrap().starts_with("0x"));
}

#[tokio::test]
async fn claims_are_off_without_the_operator_key() {
    let s = common::spawn(|_| {}).await;
    let c = s.browser();
    let csrf = s.email_login(&c, "x@example.com").await;
    let me: Value = c.get(s.u("/v1/names/me")).send().await.unwrap().json().await.unwrap();
    assert_eq!(me["enabled"], false);
    let r = c.post(s.u("/v1/names/prepare")).header("x-lowki-csrf", &csrf).json(&json!({ "label": "abc", "owner": h(&[1; 20]) })).send().await.unwrap();
    assert_eq!(r.status(), 404);
}
