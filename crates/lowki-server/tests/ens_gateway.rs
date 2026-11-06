//! ENSIP-30 gateway (src/ens_gateway.rs) over HTTP, against a mock JSON-RPC chain whose
//! UniversalResolver serves `alice.lowki.eth`'s meta-address record from a fake
//! `StealthResolver`. The full flow against real contracts is `contracts/e2e/run.sh`.

mod common;

use std::{
    net::SocketAddr,
    sync::Arc,
    time::Duration,
};

use axum::{Json, Router, extract::State, routing::post};
use lowki_ens::{
    abi::{self, Token},
    name::{dns_encode, namehash},
    resolver::{encode_addr, encode_addr_coin, encode_data},
    stealth::{
        decode_gateway_response, decode_resolve_calldata, encode_stealth_record, records_auth_message,
        scheme1_verify, sign_personal, stealth_data_key,
    },
    universal_resolver::{encode_resolve, encode_resolve_result},
};
use lowki_server::{config::Limit, ens_gateway::GatewayConfig};
use lowki_stealth::{SecretKey, StealthMetaAddress, check_announcement, compute_stealth_private_key};
use serde_json::{Value, json};

const UR: [u8; 20] = [0xee; 20];
const STEALTH: [u8; 20] = [0x5e; 20];

struct Chain {
    /// `name -> record` (absent: empty `data()` result).
    records: Vec<(String, Vec<u8>)>,
}

/// Answers `eth_call(UR, resolve(name, data(node, key)))` with `(abi.encode(bytes), STEALTH)`.
async fn rpc(State(chain): State<Arc<Chain>>, Json(req): Json<Value>) -> Json<Value> {
    let to = req["params"][0]["to"].as_str().unwrap().to_lowercase();
    assert_eq!(to, format!("0x{}", hex::encode(UR)));
    let data = hex::decode(req["params"][0]["data"].as_str().unwrap().trim_start_matches("0x")).unwrap();
    let (dns, inner) = decode_resolve_calldata(&data).expect("UR resolve(bytes,bytes)");
    let name = lowki_ens::name::dns_decode(&dns).unwrap();
    assert_eq!(inner, encode_data(&namehash(&name), &stealth_data_key(1, 60)), "gateway asks for the ENSIP-30 record");
    let value = chain.records.iter().find(|(n, _)| *n == name).map(|(_, v)| v.clone()).unwrap_or_default();
    let ret = encode_resolve_result(&abi::encode(&[Token::bytes(&value)]), &STEALTH);
    Json(json!({ "jsonrpc": "2.0", "id": req["id"], "result": format!("0x{}", hex::encode(ret)) }))
}

struct Keys {
    spending: SecretKey,
    viewing: SecretKey,
    meta: StealthMetaAddress,
}

fn keys(seed: u8) -> Keys {
    let spending = SecretKey::from_bytes(&[seed; 32]).unwrap();
    let viewing = SecretKey::from_bytes(&[seed.wrapping_add(1); 32]).unwrap();
    let meta = StealthMetaAddress::new(&spending.public_key(), &viewing.public_key()).unwrap();
    Keys { spending, viewing, meta }
}

async fn setup(lookups_per_ip: u32) -> (common::TestServer, Keys) {
    let alice = keys(7);
    let chain = Arc::new(Chain { records: vec![("alice.lowki.eth".into(), encode_stealth_record(true, &alice.meta))] });
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr: SocketAddr = listener.local_addr().unwrap();
    let app = Router::new().route("/", post(rpc)).with_state(chain);
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    let server = common::spawn(|c| {
        c.ens_gateway = Some(GatewayConfig {
            rpc_url: format!("http://{addr}/"),
            universal_resolver: UR,
            stealth_resolver: Some(STEALTH),
            parent: "lowki.eth".into(),
            lookups_per_ip: Limit { max: lookups_per_ip, window: Duration::from_secs(60) },
            lookups_per_name: Limit { max: 1000, window: Duration::from_secs(3600) },
            records_per_ip: Limit { max: 1000, window: Duration::from_secs(60) },
            meta_cache: Duration::from_secs(30),
            auth_skew: Duration::from_secs(300),
            records_page: 500,
        });
    })
    .await;
    (server, alice)
}

fn resolve_call(name: &str, inner: &[u8]) -> String {
    format!("0x{}", hex::encode(encode_resolve(&dns_encode(name).unwrap(), inner)))
}

async fn post_lookup(s: &common::TestServer, sender: [u8; 20], data: &str) -> reqwest::Response {
    reqwest::Client::new()
        .post(s.u("/v1/ens/gateway"))
        .json(&json!({ "sender": format!("0x{}", hex::encode(sender)), "data": data }))
        .send()
        .await
        .unwrap()
}

fn now() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs()
}

async fn fetch_records(s: &common::TestServer, name: &str, key: &SecretKey, ts: u64) -> reqwest::Response {
    let sig = sign_personal(key, &records_auth_message(name, ts)).unwrap();
    reqwest::Client::new()
        .get(s.u(&format!("/v1/ens/records?name={name}")))
        .header("x-lowki-auth-timestamp", ts.to_string())
        .header("x-lowki-auth-signature", format!("0x{}", hex::encode(sig)))
        .send()
        .await
        .unwrap()
}

#[tokio::test]
async fn verified_answers_fresh_addresses_and_records() {
    let (s, alice) = setup(100).await;
    let node = namehash("alice.lowki.eth");
    let mut seen = Vec::new();
    for inner in [encode_addr(&node), encode_addr_coin(&node, 60), encode_addr(&node)] {
        let r = post_lookup(&s, STEALTH, &resolve_call("alice.lowki.eth", &inner)).await;
        assert_eq!(r.status(), 200);
        assert_eq!(r.headers()["cache-control"], "no-store");
        assert_eq!(r.headers()["access-control-allow-origin"], "*");
        let body: Value = r.json().await.unwrap();
        let data = hex::decode(body["data"].as_str().unwrap().trim_start_matches("0x")).unwrap();
        let (addr, vd) = decode_gateway_response(&data).unwrap();
        // What Scheme1Verifier does on chain, re-implemented in lowki-ens.
        assert_eq!(scheme1_verify(&alice.meta.to_raw(), &vd, &addr).unwrap(), addr);
        assert!(!seen.contains(&addr), "a fresh address per lookup");
        seen.push(addr);
    }

    // GET form of the EIP-3668 template.
    let r = reqwest::get(s.u(&format!(
        "/v1/ens/gateway/0x{}/{}.json",
        hex::encode(STEALTH),
        resolve_call("alice.lowki.eth", &encode_addr(&node))
    )))
    .await
    .unwrap();
    assert_eq!(r.status(), 200);
    let body: Value = r.json().await.unwrap();
    let data = hex::decode(body["data"].as_str().unwrap().trim_start_matches("0x")).unwrap();
    seen.push(decode_gateway_response(&data).unwrap().0);

    // The recipient fetches its records with the viewing key and finds every address.
    let r = fetch_records(&s, "alice.lowki.eth", &alice.viewing, now()).await;
    assert_eq!(r.status(), 200);
    let body: Value = r.json().await.unwrap();
    let records = body["records"].as_array().unwrap();
    assert_eq!(records.len(), seen.len());
    assert_eq!(body["complete"], true);
    for rec in records {
        let eph = hex::decode(rec["ephemeral_pubkey"].as_str().unwrap().trim_start_matches("0x")).unwrap();
        let tag = rec["view_tag"].as_u64().unwrap() as u8;
        let found = check_announcement(&alice.viewing, alice.meta.spending_pubkey(), &eph, tag).expect("view tag matches");
        assert_eq!(format!("0x{}", hex::encode(found)), rec["stealth_address"].as_str().unwrap());
        assert!(seen.contains(&found));
        let sk = compute_stealth_private_key(&alice.spending, &alice.viewing, &eph).unwrap();
        assert_eq!(sk.address(), found, "spending key derivable");
    }

    // Names are stored keyed, never in clear.
    let (n,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM ens_stealth_records WHERE name_key = 'alice.lowki.eth'")
        .fetch_one(&s.state.db)
        .await
        .unwrap();
    assert_eq!(n, 0);
}

#[tokio::test]
async fn records_need_the_viewing_key() {
    let (s, alice) = setup(100).await;
    let node = namehash("alice.lowki.eth");
    assert_eq!(post_lookup(&s, STEALTH, &resolve_call("alice.lowki.eth", &encode_addr(&node))).await.status(), 200);

    // Spending key, someone else's key, a stale or future timestamp, a signature for another
    // name, and no signature are all refused.
    assert_eq!(fetch_records(&s, "alice.lowki.eth", &alice.spending, now()).await.status(), 401);
    assert_eq!(fetch_records(&s, "alice.lowki.eth", &keys(40).viewing, now()).await.status(), 401);
    assert_eq!(fetch_records(&s, "alice.lowki.eth", &alice.viewing, now() - 3600).await.status(), 401);
    assert_eq!(fetch_records(&s, "alice.lowki.eth", &alice.viewing, now() + 3600).await.status(), 401);
    let sig = sign_personal(&alice.viewing, &records_auth_message("bob.lowki.eth", now())).unwrap();
    let r = reqwest::Client::new()
        .get(s.u("/v1/ens/records?name=alice.lowki.eth"))
        .header("x-lowki-auth-timestamp", now().to_string())
        .header("x-lowki-auth-signature", format!("0x{}", hex::encode(sig)))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 401);
    let r = reqwest::get(s.u("/v1/ens/records?name=alice.lowki.eth")).await.unwrap();
    assert_eq!(r.status(), 401);
    assert_eq!(fetch_records(&s, "alice.lowki.eth", &alice.viewing, now()).await.status(), 200);
}

#[tokio::test]
async fn refuses_what_it_does_not_serve() {
    let (s, _) = setup(100).await;
    let node = namehash("alice.lowki.eth");
    let ok = resolve_call("alice.lowki.eth", &encode_addr(&node));
    // `sender` is informational (through the UniversalResolver it is the UR): any address is
    // accepted, the name's resolver is checked on chain instead.
    assert_eq!(post_lookup(&s, [1; 20], &ok).await.status(), 200);
    // Not under the parent, the parent itself, two labels deep, not normalized.
    for name in ["alice.other.eth", "lowki.eth", "a.alice.lowki.eth", "Alice.lowki.eth"] {
        let r = post_lookup(&s, STEALTH, &resolve_call(name, &encode_addr(&namehash(name)))).await;
        assert_eq!(r.status(), 404, "{name}");
        let body: Value = r.json().await.unwrap();
        assert!(body["message"].is_string(), "ERC-3668 error body");
    }
    // Node does not match the name.
    let r = post_lookup(&s, STEALTH, &resolve_call("alice.lowki.eth", &encode_addr(&namehash("bob.lowki.eth")))).await;
    assert_eq!(r.status(), 400);
    // Not an address query.
    let r = post_lookup(&s, STEALTH, &resolve_call("alice.lowki.eth", &encode_data(&node, "x"))).await;
    assert_eq!(r.status(), 400);
    // Non-EVM coin type (Solana 501).
    let r = post_lookup(&s, STEALTH, &resolve_call("alice.lowki.eth", &encode_addr_coin(&node, 501))).await;
    assert_eq!(r.status(), 400);
    // Malformed hex / calldata.
    assert_eq!(post_lookup(&s, STEALTH, "0xzz").await.status(), 400);
    assert_eq!(post_lookup(&s, STEALTH, "0x12345678").await.status(), 400);
    // A served name without a meta-address record: unverified mode is not served.
    let r = post_lookup(&s, STEALTH, &resolve_call("bob.lowki.eth", &encode_addr(&namehash("bob.lowki.eth")))).await;
    assert_eq!(r.status(), 404);
    let (n,): (i64,) = sqlx::query_as("SELECT COUNT(*) FROM ens_stealth_records").fetch_one(&s.state.db).await.unwrap();
    assert_eq!(n, 1, "nothing stored for refused lookups (one accepted lookup above)");
}

#[tokio::test]
async fn rate_limited_per_ip() {
    let (s, _) = setup(3).await;
    let ok = resolve_call("alice.lowki.eth", &encode_addr(&namehash("alice.lowki.eth")));
    for _ in 0..3 {
        assert_eq!(post_lookup(&s, STEALTH, &ok).await.status(), 200);
    }
    let r = post_lookup(&s, STEALTH, &ok).await;
    assert_eq!(r.status(), 429);
    assert!(r.headers().contains_key("retry-after"));
}

#[tokio::test]
async fn disabled_by_default() {
    let s = common::spawn(|_| {}).await;
    let r = reqwest::Client::new()
        .post(s.u("/v1/ens/gateway"))
        .json(&json!({ "sender": "0x00", "data": "0x" }))
        .send()
        .await
        .unwrap();
    assert_eq!(r.status(), 404);
}
