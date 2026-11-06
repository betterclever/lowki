//! Differential tests against `vectors/lowki-ens/vectors.json`, produced by
//! `vectors/lowki-ens/gen.ts` (viem 2.57.16, @adraffy/ens-normalize 1.11.1 and its official
//! ENSIP-15 validation tests, @scure/bip32 2.4.0, @noble/curves 2.4.0).

use std::collections::HashMap;
use std::sync::OnceLock;

use lowki_ens::abi::{self, Reader, Token};
use lowki_ens::ccip::{self, *};
use lowki_ens::identity::{IdentityKey, identity_path};
use lowki_ens::lowki_registrar::{self as lr, Registration};
use lowki_ens::permissioned_resolver::{self as pr, Grant};
use lowki_ens::universal_resolver::{self as ur, UrError};
use lowki_ens::{
    Address, Error, SecretKey, StealthMetaAddress, U256, eth_registrar as er, name, registry,
    resolver, sig, stealth, to_checksum_address,
};
use serde_json::Value;

const VECTORS: &str = include_str!("../../../vectors/lowki-ens/vectors.json");

fn v() -> &'static Value {
    static V: OnceLock<Value> = OnceLock::new();
    V.get_or_init(|| serde_json::from_str(VECTORS).unwrap())
}
fn s(v: &Value) -> &str {
    v.as_str().unwrap_or_else(|| panic!("not a string: {v}"))
}
fn hx(v: &Value) -> Vec<u8> {
    hex::decode(s(v).strip_prefix("0x").unwrap()).unwrap()
}
fn h32(v: &Value) -> [u8; 32] {
    hx(v).try_into().unwrap()
}
fn addr(v: &Value) -> Address {
    hx(v).try_into().unwrap()
}
fn sk(v: &Value) -> SecretKey {
    SecretKey::from_slice(&hx(v)).unwrap()
}
fn dec_u64(v: &Value) -> u64 {
    s(v).parse().unwrap()
}
fn strs(v: &Value) -> Vec<String> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|x| s(x).to_string())
        .collect()
}
fn hexes(v: &Value) -> Vec<Vec<u8>> {
    v.as_array().unwrap().iter().map(hx).collect()
}
fn grants(v: &Value) -> Vec<Grant> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|g| Grant {
            account: addr(&g["account"]),
            role_bitmap: h32(&g["role_bitmap"]),
        })
        .collect()
}
fn arr(key: &str) -> &'static Vec<Value> {
    v()[key].as_array().unwrap()
}

#[cfg(feature = "normalize")]
#[test]
fn ensip15_official_validation_tests() {
    let cases = arr("normalize");
    assert!(cases.len() > 38_000);
    let mut errors = 0;
    for c in cases {
        let got = name::normalize(s(&c["name"]));
        if c.get("error").is_some() {
            assert!(got.is_err(), "expected error for {:?}", c["name"]);
            errors += 1;
        } else {
            assert_eq!(got.as_deref(), Ok(s(&c["norm"])), "{:?}", c["name"]);
        }
    }
    assert!(errors > 1000);
}

#[test]
fn names_hashing_and_dns() {
    for c in arr("names") {
        let n = s(&c["name"]);
        assert_eq!(name::namehash(n), h32(&c["namehash"]), "namehash {n:?}");
        let pkt = hx(&c["packet"]);
        assert_eq!(name::packet_to_bytes(n), pkt, "packet {n:?}");
        if c["dns_encode_ok"].as_bool().unwrap() {
            assert_eq!(name::dns_encode(n).unwrap(), pkt);
        } else {
            assert_eq!(name::dns_encode(n), Err(Error::EmptyLabel), "{n:?}");
        }
        let lhs: Vec<_> = c["labelhashes"]
            .as_array()
            .unwrap()
            .iter()
            .map(h32)
            .collect();
        let labels: Vec<&str> = n.split('.').collect();
        assert_eq!(labels.len(), lhs.len());
        for (l, e) in labels.iter().zip(&lhs) {
            assert_eq!(name::labelhash(l), *e, "labelhash {l:?}");
        }
        if let Some(nh) = c.get("namehash_dns") {
            assert_eq!(name::namehash_dns(&pkt).unwrap(), h32(nh), "{n:?}");
            assert_eq!(name::dns_decode(&pkt).unwrap(), s(&c["dns_decode"]));
        }
    }
}

#[test]
fn role_constants_match_solidity() {
    let r = &v()["roles"];
    let reg = &r["registry"];
    let pairs: &[(&str, U256)] = &[
        ("ROLE_REGISTRAR", registry::ROLE_REGISTRAR),
        ("ROLE_REGISTRAR_ADMIN", registry::ROLE_REGISTRAR_ADMIN),
        ("ROLE_REGISTER_RESERVED", registry::ROLE_REGISTER_RESERVED),
        (
            "ROLE_REGISTER_RESERVED_ADMIN",
            registry::ROLE_REGISTER_RESERVED_ADMIN,
        ),
        ("ROLE_SET_PARENT", registry::ROLE_SET_PARENT),
        ("ROLE_SET_PARENT_ADMIN", registry::ROLE_SET_PARENT_ADMIN),
        ("ROLE_UNREGISTER", registry::ROLE_UNREGISTER),
        ("ROLE_UNREGISTER_ADMIN", registry::ROLE_UNREGISTER_ADMIN),
        ("ROLE_RENEW", registry::ROLE_RENEW),
        ("ROLE_RENEW_ADMIN", registry::ROLE_RENEW_ADMIN),
        ("ROLE_SET_SUBREGISTRY", registry::ROLE_SET_SUBREGISTRY),
        (
            "ROLE_SET_SUBREGISTRY_ADMIN",
            registry::ROLE_SET_SUBREGISTRY_ADMIN,
        ),
        ("ROLE_SET_RESOLVER", registry::ROLE_SET_RESOLVER),
        ("ROLE_SET_RESOLVER_ADMIN", registry::ROLE_SET_RESOLVER_ADMIN),
        ("ROLE_CAN_TRANSFER_ADMIN", registry::ROLE_CAN_TRANSFER_ADMIN),
        ("ROLE_WAS_RESERVED", registry::ROLE_WAS_RESERVED),
        ("ROLE_SET_URI", registry::ROLE_SET_URI),
        ("ROLE_SET_URI_ADMIN", registry::ROLE_SET_URI_ADMIN),
        ("ROLE_CAN_NAME", registry::ROLE_CAN_NAME),
        ("ROLE_CAN_NAME_ADMIN", registry::ROLE_CAN_NAME_ADMIN),
        ("ROLE_UPGRADE", registry::ROLE_UPGRADE),
        ("ROLE_UPGRADE_ADMIN", registry::ROLE_UPGRADE_ADMIN),
        (
            "UNEMANCIPATED_ROLE_BITMAP",
            registry::UNEMANCIPATED_ROLE_BITMAP,
        ),
    ];
    for (k, val) in pairs {
        assert_eq!(h32(&reg[*k]), *val, "{k}");
    }
    assert_eq!(reg.as_object().unwrap().len(), pairs.len());
    let res = &r["resolver"];
    let pairs: &[(&str, U256)] = &[
        ("ROLE_SET_ADDRESS", pr::ROLE_SET_ADDRESS),
        ("ROLE_SET_ADDRESS_ADMIN", pr::ROLE_SET_ADDRESS_ADMIN),
        ("ROLE_SET_TEXT", pr::ROLE_SET_TEXT),
        ("ROLE_SET_TEXT_ADMIN", pr::ROLE_SET_TEXT_ADMIN),
        ("ROLE_SET_CONTENTHASH", pr::ROLE_SET_CONTENTHASH),
        ("ROLE_SET_CONTENTHASH_ADMIN", pr::ROLE_SET_CONTENTHASH_ADMIN),
        ("ROLE_SET_ABI", pr::ROLE_SET_ABI),
        ("ROLE_SET_ABI_ADMIN", pr::ROLE_SET_ABI_ADMIN),
        ("ROLE_SET_INTERFACE", pr::ROLE_SET_INTERFACE),
        ("ROLE_SET_INTERFACE_ADMIN", pr::ROLE_SET_INTERFACE_ADMIN),
        ("ROLE_SET_NAME", pr::ROLE_SET_NAME),
        ("ROLE_SET_NAME_ADMIN", pr::ROLE_SET_NAME_ADMIN),
        ("ROLE_SET_DATA", pr::ROLE_SET_DATA),
        ("ROLE_SET_DATA_ADMIN", pr::ROLE_SET_DATA_ADMIN),
        ("ROLE_LINK", pr::ROLE_LINK),
        ("ROLE_LINK_ADMIN", pr::ROLE_LINK_ADMIN),
        ("ROLE_CAN_NAME", pr::ROLE_CAN_NAME),
        ("ROLE_CAN_NAME_ADMIN", pr::ROLE_CAN_NAME_ADMIN),
        ("ROLE_UPGRADE", pr::ROLE_UPGRADE),
        ("ROLE_UPGRADE_ADMIN", pr::ROLE_UPGRADE_ADMIN),
    ];
    for (k, val) in pairs {
        assert_eq!(h32(&res[*k]), *val, "{k}");
    }
    assert_eq!(res.as_object().unwrap().len(), pairs.len());
    assert_eq!(h32(&r["eac"]["ALL_ROLES"]), pr::ALL_ROLES);
    assert_eq!(h32(&r["eac"]["ADMIN_ROLES"]), pr::ADMIN_ROLES);
}

#[test]
fn calldata_builders_match_viem() {
    let mut seen = HashMap::<String, usize>::new();
    for c in arr("calldata") {
        let f = s(&c["fn"]);
        let a = &c["args"];
        let got: Vec<u8> = match f {
            "addr" => resolver::encode_addr(&h32(&a["node"])),
            "addr_coin" => resolver::encode_addr_coin(&h32(&a["node"]), dec_u64(&a["coin_type"])),
            "text" => resolver::encode_text(&h32(&a["node"]), s(&a["key"])),
            "data" => resolver::encode_data(&h32(&a["node"]), s(&a["key"])),
            "contenthash" => resolver::encode_contenthash(&h32(&a["node"])),
            "multicall" => resolver::encode_multicall(&hexes(&a["calls"])),
            "ur_resolve" => ur::encode_resolve(&hx(&a["dns"]), &hx(&a["data"])),
            "ur_resolve_with_gateways" => ur::encode_resolve_with_gateways(
                &hx(&a["dns"]),
                &hx(&a["data"]),
                &strs(&a["gateways"]),
            ),
            "pr_set_text" => pr::encode_set_text(&hx(&a["dns"]), s(&a["key"]), s(&a["value"])),
            "pr_set_data" => pr::encode_set_data(&hx(&a["dns"]), s(&a["key"]), &hx(&a["value"])),
            "pr_set_address" => {
                pr::encode_set_address(&hx(&a["dns"]), dec_u64(&a["coin_type"]), &hx(&a["value"]))
            }
            "pr_set_contenthash" => pr::encode_set_contenthash(&hx(&a["dns"]), &hx(&a["value"])),
            "pr_initialize" => pr::encode_initialize(&grants(&a["grants"]), &hexes(&a["calls"])),
            "factory_deploy_proxy" => pr::encode_deploy_proxy(
                &addr(&a["implementation"]),
                &h32(&a["salt"]),
                &hx(&a["data"]),
            ),
            "factory_predict" => {
                pr::encode_predict_proxy_address(&addr(&a["deployer"]), &h32(&a["salt"]))
            }
            "reg_register" => registry::encode_register(
                s(&a["label"]),
                &addr(&a["owner"]),
                &addr(&a["subregistry"]),
                &addr(&a["resolver"]),
                &h32(&a["role_bitmap"]),
                dec_u64(&a["expiry"]),
            ),
            "reg_set_resolver" => {
                registry::encode_set_resolver(&h32(&a["any_id"]), &addr(&a["address"]))
            }
            "reg_set_subregistry" => {
                registry::encode_set_subregistry(&h32(&a["any_id"]), &addr(&a["address"]))
            }
            "reg_get_state" => registry::encode_get_state(&h32(&a["any_id"])),
            "reg_grant_root_roles" => {
                registry::encode_grant_root_roles(&h32(&a["role_bitmap"]), &addr(&a["account"]))
            }
            "reg_initialize" => registry::encode_initialize(&grants(&a["grants"])),
            "eth_commit" => er::encode_commit(&h32(&a["commitment"])),
            "eth_register" => er::encode_register(
                s(&a["label"]),
                &addr(&a["owner"]),
                &h32(&a["secret"]),
                &addr(&a["subregistry"]),
                &addr(&a["resolver"]),
                dec_u64(&a["duration"]),
                &addr(&a["payment_token"]),
                &h32(&a["referrer"]),
            ),
            "eth_make_commitment" => {
                let args = (
                    s(&a["label"]),
                    addr(&a["owner"]),
                    h32(&a["secret"]),
                    addr(&a["subregistry"]),
                    addr(&a["resolver"]),
                    dec_u64(&a["duration"]),
                    h32(&a["referrer"]),
                );
                let commitment = er::make_commitment(
                    args.0, &args.1, &args.2, &args.3, &args.4, args.5, &args.6,
                );
                assert_eq!(commitment, h32(&a["commitment"]), "make_commitment");
                er::encode_make_commitment(
                    args.0, &args.1, &args.2, &args.3, &args.4, args.5, &args.6,
                )
            }
            "eth_is_available" => er::encode_is_available(s(&a["label"])),
            "eth_get_register_price" => er::encode_get_register_price(
                s(&a["label"]),
                dec_u64(&a["duration"]),
                &addr(&a["payment_token"]),
            ),
            "eth_commitment_at" => er::encode_commitment_at(&h32(&a["commitment"])),
            "erc20_approve" => er::encode_erc20_approve(&addr(&a["spender"]), &h32(&a["value"])),
            "mock_mint" => er::encode_mock_mint(&addr(&a["to"]), &h32(&a["value"])),
            "lowki_register" => {
                let r = Registration {
                    label: s(&a["label"]).into(),
                    owner: addr(&a["owner"]),
                    resolver: addr(&a["resolver"]),
                    duration: dec_u64(&a["duration"]),
                    nonce: 0,
                    deadline: dec_u64(&a["deadline"]),
                };
                lr::encode_register(&r, &hx(&a["signature"]))
            }
            "lowki_nonces" => lr::encode_nonces(&addr(&a["owner"])),
            "lowki_available" => lr::encode_available(s(&a["label"])),
            "lowki_renew" => lr::encode_renew(s(&a["label"]), dec_u64(&a["duration"])),
            "st_set_config" => {
                stealth::encode_set_config(&hx(&a["dns"]), &addr(&a["address"]), &strs(&a["urls"]))
            }
            "st_set_chains" => {
                let coins: Vec<u64> = a["coin_types"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(dec_u64)
                    .collect();
                stealth::encode_set_chains(&hx(&a["dns"]), &coins, a["enabled"].as_bool().unwrap())
            }
            "st_set_meta_address" => {
                stealth::encode_set_meta_address(&hx(&a["dns"]), &hx(&a["value"]))
            }
            "st_clear_records" => stealth::encode_clear_records(&hx(&a["dns"])),
            "st_can_set_meta_address" => {
                stealth::encode_can_set_meta_address(&hx(&a["dns"]), &addr(&a["address"]))
            }
            "st_can_set_config" => {
                stealth::encode_can_set_config(&hx(&a["dns"]), &addr(&a["address"]))
            }
            other => panic!("unknown calldata fn {other}"),
        };
        assert_eq!(hex::encode(&got), hex::encode(hx(&c["data"])), "{f} {a}");
        *seen.entry(f.to_string()).or_default() += 1;
    }
    assert_eq!(seen.len(), 39, "{seen:?}");
}

#[test]
fn decoders_match_viem() {
    for c in arr("decoders") {
        let d = hx(&c["data"]);
        match s(&c["fn"]) {
            "addr" => assert_eq!(
                resolver::decode_addr_result(&d).unwrap(),
                addr(&c["address"])
            ),
            "addr_coin" => assert_eq!(
                resolver::decode_addr_coin_result(&d).unwrap(),
                hx(&c["bytes"])
            ),
            "text" => assert_eq!(resolver::decode_text_result(&d).unwrap(), s(&c["string"])),
            "data" => assert_eq!(resolver::decode_data_result(&d).unwrap(), hx(&c["bytes"])),
            "contenthash" => assert_eq!(
                resolver::decode_contenthash_result(&d).unwrap(),
                hx(&c["bytes"])
            ),
            "multicall" => assert_eq!(
                resolver::decode_multicall_result(&d).unwrap(),
                hexes(&c["list"])
            ),
            "ur_resolve" => {
                let (r, a) = ur::decode_resolve_result(&d).unwrap();
                assert_eq!(r, hx(&c["bytes"]));
                assert_eq!(a, addr(&c["address"]));
                assert_eq!(ur::encode_resolve_result(&r, &a), d);
            }
            "get_state" => {
                let st = registry::decode_state(&d).unwrap();
                assert_eq!(st.status as u64, c["status"].as_u64().unwrap());
                assert_eq!(st.expiry, dec_u64(&c["expiry"]));
                assert_eq!(st.latest_owner, addr(&c["latest_owner"]));
                assert_eq!(st.token_id, h32(&c["token_id"]));
                assert_eq!(st.resource, h32(&c["resource"]));
                assert_eq!(registry::encode_state(&st), d);
            }
            "register_price" => {
                assert_eq!(
                    er::decode_register_price(&d).unwrap(),
                    (h32(&c["base"]), h32(&c["premium"]))
                )
            }
            "bool" => assert_eq!(
                er::decode_bool_result(&d).unwrap(),
                c["bool"].as_bool().unwrap()
            ),
            "nonce" => assert_eq!(lr::decode_nonce(&d).unwrap(), dec_u64(&c["nonce"])),
            "commitment_at" => {
                assert_eq!(er::decode_commitment_at(&d).unwrap(), dec_u64(&c["nonce"]))
            }
            "address_result" => {
                assert_eq!(pr::decode_address_result(&d).unwrap(), addr(&c["address"]))
            }
            "ur_error" => {
                let e = &c["expect"];
                let got = ur::decode_ur_error(&d);
                let want = match s(&e["kind"]) {
                    "ResolverNotFound" => UrError::ResolverNotFound {
                        name: hx(&e["name"]),
                    },
                    "ResolverNotContract" => UrError::ResolverNotContract {
                        name: hx(&e["name"]),
                        resolver: addr(&e["address"]),
                    },
                    "UnsupportedResolverProfile" => UrError::UnsupportedResolverProfile {
                        selector: hx(&e["selector"]).try_into().unwrap(),
                    },
                    "ResolverError" => UrError::ResolverError {
                        error_data: hx(&e["bytes"]),
                    },
                    "ReverseAddressMismatch" => UrError::ReverseAddressMismatch {
                        primary: s(&e["string"]).into(),
                        primary_address: hx(&e["bytes"]),
                    },
                    "HttpError" => UrError::HttpError {
                        status: e["status"].as_u64().unwrap() as u16,
                        message: s(&e["string"]).into(),
                    },
                    "Error" => UrError::Error(s(&e["string"]).into()),
                    "Panic" => UrError::Panic(h32(&e["code"])),
                    "Unknown" => UrError::Unknown(d.clone()),
                    k => panic!("{k}"),
                };
                assert_eq!(got, want);
                assert_eq!(ur::encode_ur_error(&got), d);
            }
            other => panic!("unknown decoder {other}"),
        }
    }
}

#[test]
fn abi_roundtrip_matches_viem() {
    for c in arr("abi_roundtrip") {
        let enc = hx(&c["encoded"]);
        let r = Reader::new(&enc);
        assert_eq!(r.bytes(0).unwrap(), hx(&c["bytes"]));
        assert_eq!(r.string(1).unwrap(), s(&c["string"]));
        assert_eq!(r.bytes_array(2).unwrap(), hexes(&c["bytes_array"]));
        assert_eq!(r.string_array(3).unwrap(), strs(&c["string_array"]));
        let addrs: Vec<Address> = c["address_array"]
            .as_array()
            .unwrap()
            .iter()
            .map(addr)
            .collect();
        assert_eq!(r.address_array(4).unwrap(), addrs);
        let u: Vec<U256> = c["u256_array"]
            .as_array()
            .unwrap()
            .iter()
            .map(h32)
            .collect();
        assert_eq!(r.u256_array(5).unwrap(), u);
        let gs = grants(&c["grants"]);
        let (n, el) = r.array(6, 2).unwrap();
        assert_eq!(n, gs.len());
        for (i, g) in gs.iter().enumerate() {
            let t = el.inline(2 * i).unwrap();
            assert_eq!(t.address(0).unwrap(), g.account);
            assert_eq!(t.u256(1).unwrap(), g.role_bitmap);
        }
        assert_eq!(r.bool(7).unwrap(), c["bool"].as_bool().unwrap());
        assert_eq!(r.b32(8).unwrap(), h32(&c["b32"]));
        assert_eq!(r.u64(9).unwrap(), dec_u64(&c["u64"]));
        let tokens = vec![
            Token::bytes(&hx(&c["bytes"])),
            Token::string(s(&c["string"])),
            Token::bytes_array(&hexes(&c["bytes_array"])),
            Token::string_array(&strs(&c["string_array"])),
            Token::Array(addrs.iter().map(Token::address).collect()),
            Token::Array(u.iter().map(Token::u256).collect()),
            Token::Array(
                gs.iter()
                    .map(|g| {
                        Token::Tuple(vec![
                            Token::address(&g.account),
                            Token::u256(&g.role_bitmap),
                        ])
                    })
                    .collect(),
            ),
            Token::bool(c["bool"].as_bool().unwrap()),
            Token::b32(&h32(&c["b32"])),
            Token::uint(dec_u64(&c["u64"])),
        ];
        assert_eq!(abi::encode(&tokens), enc);
    }
}

#[test]
fn offchain_lookup_and_callback() {
    for c in arr("offchain_lookup") {
        let revert = hx(&c["revert"]);
        let l = OffchainLookup::decode(&revert).unwrap();
        assert_eq!(l.sender, addr(&c["sender"]));
        assert_eq!(l.urls, strs(&c["urls"]));
        assert_eq!(l.call_data, hx(&c["call_data"]));
        assert_eq!(l.callback_function.to_vec(), hx(&c["callback_function"]));
        assert_eq!(l.extra_data, hx(&c["extra_data"]));
        assert_eq!(l.encode(), revert);
        assert_eq!(l.callback_calldata(&hx(&c["response"])), hx(&c["callback"]));
        assert!(is_offchain_lookup(&revert));
    }
}

#[test]
fn ccip_request_construction_matches_viem() {
    for c in arr("ccip_requests") {
        let sender = addr(&c["sender"]);
        let req = ccip_request(s(&c["template"]), &sender, &hx(&c["data"]));
        if c["error"].is_null() {
            let r = &c["request"];
            assert!(is_allowed_url(&req.url), "{}", req.url);
            assert_eq!(req.url, s(&r["url"]));
            assert_eq!(req.method.as_str(), s(&r["method"]));
            assert_eq!(req.body.as_deref(), r["body"].as_str());
            assert_eq!(req.content_type(), r["content_type"].as_str());
        } else {
            assert_eq!(s(&c["error"]["kind"]), "Request");
            assert!(c["request"].is_null());
            assert!(!is_allowed_url(&req.url), "{}", req.url);
        }
    }
}

fn canned(r: &Value) -> Result<HttpResponse, HttpFailure> {
    match r.get("failure").and_then(Value::as_str) {
        Some("Network") => Err(HttpFailure::Network),
        Some("Timeout") => Err(HttpFailure::Timeout),
        Some(f) => panic!("{f}"),
        None => Ok(HttpResponse {
            status: r["status"].as_u64().unwrap() as u16,
            content_type: r["ct"].as_str().map(String::from),
            body: s(&r["body"]).into(),
        }),
    }
}

fn gateway_error(e: &Value) -> GatewayError {
    match s(&e["kind"]) {
        "Http" => GatewayError::Http {
            status: e["status"].as_u64().unwrap() as u16,
        },
        "Request" => GatewayError::Request,
        "Malformed" => GatewayError::Malformed,
        "Timeout" => GatewayError::Timeout,
        "NoUrls" => GatewayError::NoUrls,
        k => panic!("{k}"),
    }
}

#[test]
fn gateway_response_parsing_matches_viem() {
    for c in arr("ccip_responses") {
        let parsed = match canned(&c["response"]) {
            Ok(r) => parse_gateway_response(&r),
            Err(f) => Err(f.into()),
        };
        let item = if c["error"].is_null() {
            assert_eq!(
                parsed.clone().unwrap(),
                hx(&c["result"]),
                "{}",
                c["response"]
            );
            (false, parsed.unwrap())
        } else {
            let want = gateway_error(&c["error"]);
            assert_eq!(parsed, Err(want), "{}", c["response"]);
            (true, want.encode_batch_failure())
        };
        assert_eq!(
            encode_batch_response(&[item.0], &[item.1]),
            hx(&c["batch"]),
            "{}",
            c["response"]
        );
        let (f, r) = decode_batch_response(&hx(&c["batch"])).unwrap();
        assert_eq!(f, vec![item.0]);
        assert_eq!(r.len(), 1);
    }
}

/// Mock transport over the generator's tables.
struct Mock<'a> {
    to: Address,
    calls: HashMap<Vec<u8>, CallOutcome>,
    responses: &'a Value,
    requests: Vec<(String, String, Option<String>)>,
}

impl Transport for Mock<'_> {
    fn eth_call(&mut self, to: &Address, data: &[u8]) -> Result<CallOutcome, Error> {
        assert_eq!(*to, self.to);
        self.calls.get(data).cloned().ok_or(Error::InvalidAbi)
    }
    fn http(&mut self, req: &HttpRequest) -> Result<HttpResponse, HttpFailure> {
        self.requests.push((
            req.url.clone(),
            req.method.as_str().into(),
            req.body.clone(),
        ));
        let r = self
            .responses
            .get(&req.url)
            .unwrap_or_else(|| panic!("viem never fetched {}", req.url));
        canned(r)
    }
}

fn sorted_requests(
    reqs: &[(String, String, Option<String>)],
) -> Vec<(String, String, Option<String>)> {
    let mut v = reqs.to_vec();
    v.sort_by_key(|(u, _, b)| format!("{u}{}", b.as_deref().unwrap_or("null")));
    v
}

fn viem_requests(v: &Value) -> Vec<(String, String, Option<String>)> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|r| {
            (
                s(&r["url"]).into(),
                s(&r["method"]).into(),
                r["body"].as_str().map(String::from),
            )
        })
        .collect()
}

#[test]
fn local_batch_gateway_matches_viem() {
    for c in arr("batch_gateway") {
        let to = [0x42u8; 20];
        let lookup = OffchainLookup {
            sender: to,
            urls: vec![LOCAL_BATCH_GATEWAY_URL.into()],
            call_data: hx(&c["call_data"]),
            callback_function: [0x11, 0x22, 0x33, 0x44],
            extra_data: vec![9, 9],
        };
        let expected_cb = lookup.callback_calldata(&hx(&c["result"]));
        let mut calls = HashMap::new();
        calls.insert(vec![1u8], CallOutcome::Revert(lookup.encode()));
        calls.insert(expected_cb, CallOutcome::Success(vec![0xaa]));
        let mut m = Mock {
            to,
            calls,
            responses: &c["responses"],
            requests: vec![],
        };
        assert_eq!(ccip_read(&mut m, to, &[1]).unwrap(), vec![0xaa]);
        assert_eq!(sorted_requests(&m.requests), viem_requests(&c["requests"]));
        // decode the batch response for sanity
        let (f, r) = decode_batch_response(&hx(&c["result"])).unwrap();
        assert_eq!(f.len(), r.len());
    }
}

#[test]
fn ccip_read_flows_match_viem() {
    let (mut ok, mut failed) = (0, 0);
    for c in arr("ccip_flows") {
        let to = addr(&c["to"]);
        let mut calls = HashMap::new();
        for k in c["calls"].as_array().unwrap() {
            let o = if let Some(d) = k.get("ok") {
                CallOutcome::Success(hx(d))
            } else {
                CallOutcome::Revert(hx(&k["revert"]))
            };
            calls.insert(hx(&k["data"]), o);
        }
        let mut m = Mock {
            to,
            calls,
            responses: &c["responses"],
            requests: vec![],
        };
        let got = ccip_read(&mut m, to, &hx(&c["data"]));
        if c["result"].is_null() {
            assert!(got.is_err(), "viem failed but we got {got:?}");
            failed += 1;
        } else {
            assert_eq!(got.unwrap(), hx(&c["result"]));
            ok += 1;
        }
        assert_eq!(
            sorted_requests(&m.requests),
            viem_requests(&c["requests_sorted"])
        );
    }
    assert!(ok >= 10 && failed >= 10, "{ok} {failed}");
}

#[test]
fn eip712_registration_matches_viem() {
    for c in arr("eip712") {
        let r = Registration {
            label: s(&c["label"]).into(),
            owner: addr(&c["owner"]),
            resolver: addr(&c["resolver"]),
            duration: dec_u64(&c["duration"]),
            nonce: dec_u64(&c["nonce"]),
            deadline: dec_u64(&c["deadline"]),
        };
        let chain = c["chain_id"].as_u64().unwrap();
        let reg = addr(&c["registrar"]);
        assert_eq!(lr::registration_digest(chain, &reg, &r), h32(&c["digest"]));
        let key = IdentityKey::from_secret_key(0, sk(&c["private_key"]));
        assert_eq!(key.address(), addr(&c["signer"]));
        let sig = lr::sign_registration(&key, chain, &reg, &r);
        assert_eq!(sig.to_vec(), hx(&c["signature"]));
        assert_eq!(
            lr::recover_registration(chain, &reg, &r, &sig).unwrap(),
            key.address()
        );
    }
}

#[test]
fn personal_sign_matches_viem() {
    for c in arr("personal_sign") {
        let msg = stealth::records_auth_message(s(&c["name"]), dec_u64(&c["timestamp"]));
        assert_eq!(msg, s(&c["message"]));
        assert_eq!(sig::personal_message_hash(msg.as_bytes()), h32(&c["hash"]));
        let k = sk(&c["private_key"]);
        let sg = stealth::sign_personal(&k, &msg).unwrap();
        assert_eq!(sg.to_vec(), hx(&c["signature"]));
        assert_eq!(
            stealth::recover_personal(&msg, &sg).unwrap(),
            addr(&c["address"])
        );
        assert_eq!(k.address(), addr(&c["address"]));
    }
    for c in arr("digest_signatures") {
        let k = sk(&c["private_key"]);
        let sg = sig::sign_digest(&k, &h32(&c["digest"])).unwrap();
        assert_eq!(sg.to_vec(), hx(&c["signature"]));
        assert_eq!(
            sig::recover_digest(&h32(&c["digest"]), &sg).unwrap(),
            k.address()
        );
        let id = IdentityKey::from_secret_key(3, k);
        assert_eq!(id.sign_digest(&h32(&c["digest"])), sg);
    }
}

#[test]
fn stealth_records() {
    let (mut good, mut bad) = (0, 0);
    for c in arr("stealth_records") {
        let val = hx(&c["value"]);
        let r = stealth::decode_stealth_record(&val);
        if c["ok"].as_bool().unwrap() {
            let r = r.unwrap();
            assert_eq!(
                r.client_side_allowed,
                c["client_side_allowed"].as_bool().unwrap()
            );
            assert_eq!(r.meta.spending_pubkey().to_vec(), hx(&c["spend"]));
            assert_eq!(r.meta.viewing_pubkey().to_vec(), hx(&c["view"]));
            assert_eq!(
                stealth::encode_stealth_record(r.client_side_allowed, &r.meta),
                val
            );
            good += 1;
        } else {
            assert_eq!(r, Err(Error::InvalidStealthRecord), "{}", c["value"]);
            bad += 1;
        }
    }
    assert!(good >= 64 && bad >= 16);
    for c in arr("stealth_keys") {
        let (sc, co) = (dec_u64(&c["scheme"]), dec_u64(&c["coin"]));
        assert_eq!(stealth::stealth_data_key(sc, co), s(&c["data_key"]));
        assert_eq!(stealth::stealth_text_key(sc), s(&c["text_key"]));
        assert_eq!(
            stealth::parse_stealth_data_key(s(&c["data_key"])),
            Some((sc, co))
        );
    }
}

#[test]
fn scheme1_gateway_derivation_and_verifier() {
    for c in arr("scheme1") {
        let meta = StealthMetaAddress::from_raw(&hx(&c["meta"])).unwrap();
        let e = sk(&c["ephemeral"]);
        let vd = stealth::scheme1_verification_data(&e, &meta).unwrap();
        assert_eq!(vd.to_vec(), hx(&c["verification_data"]));
        let g = stealth::gateway_derive(&meta, &e).unwrap();
        let stealth_addr = addr(&c["stealth_address"]);
        assert_eq!(g.stealth_address, stealth_addr);
        assert_eq!(g.ephemeral_pubkey.to_vec(), hx(&c["ephemeral_pubkey"]));
        assert_eq!(g.view_tag as u64, c["view_tag"].as_u64().unwrap());
        assert_eq!(*g.verification_data, *vd);
        assert_eq!(g.gateway_response(), hx(&c["gateway_response"]));
        let (a, vd2) = stealth::decode_gateway_response(&g.gateway_response()).unwrap();
        assert_eq!(a, stealth_addr);
        assert_eq!(vd2.as_slice(), &vd[..]);
        // the verifier re-derives the same address
        assert_eq!(
            stealth::scheme1_verify(&meta.to_raw(), &vd[..], &[0u8; 20]).unwrap(),
            stealth_addr
        );
        // and the recipient finds it with lowki-stealth
        let view = sk(&c["view_priv"]);
        let spend = sk(&c["spend_priv"]);
        assert_eq!(
            lowki_stealth::check_announcement(
                &view,
                meta.spending_pubkey(),
                &g.ephemeral_pubkey,
                g.view_tag
            ),
            Some(stealth_addr)
        );
        let p =
            lowki_stealth::compute_stealth_private_key(&spend, &view, &g.ephemeral_pubkey).unwrap();
        assert_eq!(p.address(), stealth_addr);
    }
}

#[test]
fn gateway_resolve_calldata() {
    for c in arr("resolve_calldata") {
        let (dns, inner) = stealth::decode_resolve_calldata(&hx(&c["calldata"])).unwrap();
        assert_eq!(dns, hx(&c["dns"]));
        assert_eq!(inner, hx(&c["inner"]));
        let q = stealth::parse_addr_query(&inner);
        match c["addr"].as_object() {
            None => assert_eq!(q, None),
            Some(a) => {
                let q = q.unwrap();
                assert_eq!(q.node, h32(&a["node"]));
                assert_eq!(q.coin_type, h32(&a["coin_type"]));
                assert_eq!(q.legacy, a["legacy"].as_bool().unwrap());
                assert_eq!(q.node, name::namehash_dns(&dns).unwrap());
            }
        }
    }
}

#[test]
fn identity_keys_match_scure_bip32() {
    for c in arr("identity") {
        let seed: [u8; 64] = hx(&c["seed"]).try_into().unwrap();
        let index = c["index"].as_u64().unwrap() as u32;
        assert_eq!(identity_path(index), s(&c["path"]));
        let k = IdentityKey::from_seed(&seed, index).unwrap();
        assert_eq!(k.secret_key().to_bytes().to_vec(), hx(&c["private_key"]));
        assert_eq!(k.address(), addr(&c["address"]));
        assert_eq!(k.path(), s(&c["path"]));
        let k2 = IdentityKey::from_derive_at(index, |p: &str| -> Result<_, Error> {
            assert_eq!(p, s(&c["path"]));
            Ok(k.secret_key().to_bytes())
        })
        .unwrap();
        assert_eq!(k2.address(), k.address());
        let dbg = format!("{k:?}");
        assert!(dbg.contains(&to_checksum_address(&k.address())));
        assert!(!dbg.contains(&hex::encode(*k.secret_key().to_bytes())));
    }
}

#[cfg(feature = "normalize")]
#[test]
fn lowki_labels_are_normalized() {
    let mut accepted = 0;
    for c in arr("lowki_labels") {
        let l = s(&c["label"]);
        let norm = c["normalized"].as_str();
        assert_eq!(name::normalize(l).ok().as_deref(), norm, "{l:?}");
        if lr::validate_label(l).is_ok() {
            assert_eq!(
                norm,
                Some(l),
                "{l:?} passes the registrar rules but is not normalized"
            );
            accepted += 1;
        }
    }
    assert!(accepted > 50, "{accepted}");
}

#[test]
fn batch_query_codec_roundtrip() {
    for c in arr("batch_gateway") {
        let cd = hx(&c["call_data"]);
        let qs = decode_batch_query(&cd).unwrap();
        assert_eq!(encode_batch_query(&qs), cd);
    }
    let _ = ccip::MAX_QUERIES;
}

/// `LowkiOwnerAccount` (EIP-7702 delegate): EIP-712 `Execute` digest and signature, `execute`
/// calldata, EIP-7702 authorisation hash and signature, against viem `hashTypedData`, `sign`,
/// `encodeFunctionData`, `hashAuthorization`, `signAuthorization`.
#[test]
fn owner_account_matches_viem() {
    use lowki_ens::owner_account as oa;
    let cases = arr("owner_account");
    assert!(cases.len() >= 48);
    for c in cases {
        let raw = hx(&c["key"]);
        let key = IdentityKey::from_derive_at(0, |_| -> Result<_, Error> {
            let mut z = zeroize::Zeroizing::new([0u8; 32]);
            z.copy_from_slice(&raw);
            Ok(z)
        })
        .unwrap();
        let account = addr(&c["account"]);
        assert_eq!(key.address(), account);
        let chain_id = c["chain_id"].as_u64().unwrap();
        let calls: Vec<oa::Call> = c["calls"]
            .as_array()
            .unwrap()
            .iter()
            .map(|x| oa::Call { target: addr(&x["target"]), value: h32(&x["value"]), data: hx(&x["data"]) })
            .collect();
        let (nonce, deadline) = (dec_u64(&c["nonce"]), dec_u64(&c["deadline"]));
        assert_eq!(oa::execute_digest(chain_id, &account, &calls, nonce, deadline), h32(&c["digest"]));
        let sig = oa::sign_execute(&key, chain_id, &account, &calls, nonce, deadline).unwrap();
        assert_eq!(sig.to_vec(), hx(&c["signature"]));
        assert_eq!(oa::encode_execute(&calls, deadline, &sig), hx(&c["calldata"]));
        // A batch signed for another account is refused.
        assert!(oa::sign_execute(&key, chain_id, &[1; 20], &calls, nonce, deadline).is_err());

        let delegate = addr(&c["delegate"]);
        let n = c["auth_nonce"].as_u64().unwrap();
        assert_eq!(oa::authorization_digest(chain_id, &delegate, n), h32(&c["auth_hash"]));
        let a = oa::sign_authorization(&key, chain_id, &delegate, n);
        assert_eq!((a.r, a.s, a.y_parity as u64), (h32(&c["auth_r"]), h32(&c["auth_s"]), c["auth_y_parity"].as_u64().unwrap()));
    }
}
