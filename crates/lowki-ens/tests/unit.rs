//! Rust-only tests: malformed input, limits, error paths, redaction.

use lowki_ens::abi::{self, Reader, Token};
use lowki_ens::ccip::*;
use lowki_ens::identity::IdentityKey;
use lowki_ens::lowki_registrar::validate_label;
use lowki_ens::universal_resolver::{self as ur, UrError};
use lowki_ens::{Error, SecretKey, StealthMetaAddress, name, registry, resolver, sig, stealth};

#[test]
fn selector_constants() {
    assert_eq!(abi::selector(resolver::ADDR_SIG), resolver::ADDR_SELECTOR);
    assert_eq!(
        abi::selector(resolver::ADDR_COIN_SIG),
        resolver::ADDR_COIN_SELECTOR
    );
    assert_eq!(abi::selector(resolver::TEXT_SIG), resolver::TEXT_SELECTOR);
    assert_eq!(abi::selector(resolver::DATA_SIG), resolver::DATA_SELECTOR);
    assert_eq!(
        abi::selector(resolver::CONTENTHASH_SIG),
        resolver::CONTENTHASH_SELECTOR
    );
    assert_eq!(
        abi::selector(resolver::MULTICALL_SIG),
        resolver::MULTICALL_SELECTOR
    );
    assert_eq!(abi::selector(ur::RESOLVE_SIG), ur::RESOLVE_SELECTOR);
    assert_eq!(abi::selector(OFFCHAIN_LOOKUP_SIG), OFFCHAIN_LOOKUP_SELECTOR);
    // IBatchGateway interface id = query selector (single function), per ens-contracts.
    assert_eq!(abi::selector(BATCH_QUERY_SIG), [0xa7, 0x80, 0xba, 0xb6]);
    // IUniversalResolver.HttpError selector documented in IBatchGateway.sol
    assert_eq!(abi::selector(ur::HTTP_ERROR_SIG), [0x01, 0x80, 0x01, 0x52]);
}

/// Deterministic xorshift for fuzz-style tests.
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn bytes(&mut self, n: usize) -> Vec<u8> {
        (0..n).map(|_| self.next() as u8).collect()
    }
}

fn run_all_decoders(d: &[u8]) {
    let _ = resolver::decode_addr_result(d);
    let _ = resolver::decode_addr_coin_result(d);
    let _ = resolver::decode_text_result(d);
    let _ = resolver::decode_multicall_result(d);
    let _ = resolver::decode_multicall_calldata(d);
    let _ = ur::decode_resolve_result(d);
    let _ = ur::decode_ur_error(d);
    let _ = registry::decode_state(d);
    let _ = lowki_ens::eth_registrar::decode_register_price(d);
    let _ = OffchainLookup::decode(d);
    let _ = decode_batch_query(d);
    let _ = decode_batch_response(d);
    let _ = stealth::decode_gateway_response(d);
    let _ = stealth::decode_resolve_calldata(d);
    let _ = stealth::parse_addr_query(d);
    let _ = stealth::decode_stealth_record(d);
    let _ = abi::decode_string_array(d);
    let r = Reader::new(d);
    let _ = r.array(0, 2);
    let _ = r.u256_array(0);
    let _ = r.address_array(0);
}

#[test]
fn decoders_never_panic_on_truncation() {
    let lookup = OffchainLookup {
        sender: [7; 20],
        urls: vec![
            "https://a.example/{data}".into(),
            "x-batch-gateway:true".into(),
        ],
        call_data: encode_batch_query(&[BatchRequest {
            sender: [1; 20],
            urls: vec!["u".into()],
            data: vec![1, 2, 3],
        }]),
        callback_function: [1, 2, 3, 4],
        extra_data: vec![5; 70],
    };
    let full = lookup.encode();
    assert_eq!(OffchainLookup::decode(&full).unwrap(), lookup);
    for n in 0..full.len() {
        // (the trailing zero padding of `extraData` is not required, as in viem)
        if n < full.len() - 26 {
            assert!(
                OffchainLookup::decode(&full[..n]).is_err(),
                "prefix {n} decoded"
            );
        }
        run_all_decoders(&full[..n]);
        run_all_decoders(&full[4..n.max(4)]);
    }
    let bq = &lookup.call_data;
    for n in 0..bq.len() - 32 {
        assert!(decode_batch_query(&bq[..n]).is_err(), "{n}");
    }
}

#[test]
fn decoders_never_panic_on_random_and_mutated_input() {
    let mut rng = Rng(0x1234_5678_9abc_def1);
    let base = OffchainLookup {
        sender: [9; 20],
        urls: vec!["https://x".into(); 3],
        call_data: vec![1; 40],
        callback_function: [0; 4],
        extra_data: vec![2; 33],
    }
    .encode();
    for _ in 0..3000 {
        let n = (rng.next() % 400) as usize;
        run_all_decoders(&rng.bytes(n));
        let mut m = base.clone();
        for _ in 0..3 {
            let i = (rng.next() as usize) % m.len();
            m[i] = rng.next() as u8;
        }
        run_all_decoders(&m);
        run_all_decoders(&m[4..]);
    }
}

#[test]
fn abi_rejects_bad_offsets_lengths_and_padding() {
    let good = abi::encode(&[Token::bytes(&[1, 2, 3]), Token::string("hi")]);
    let r = Reader::new(&good);
    assert_eq!(r.bytes(0).unwrap(), &[1, 2, 3]);
    // offset beyond the data
    let mut bad = good.clone();
    bad[31] = 0xff;
    assert_eq!(Reader::new(&bad).bytes(0), Err(Error::InvalidAbi));
    // offset that does not fit in u64
    let mut bad = good.clone();
    bad[0] = 1;
    assert_eq!(Reader::new(&bad).bytes(0), Err(Error::InvalidAbi));
    // huge length
    let mut bad = good.clone();
    bad[64..96].copy_from_slice(&[0xff; 32]);
    assert_eq!(Reader::new(&bad).bytes(0), Err(Error::InvalidAbi));
    let mut bad = good.clone();
    bad[88..96].copy_from_slice(&u64::MAX.to_be_bytes());
    assert_eq!(Reader::new(&bad).bytes(0), Err(Error::InvalidAbi));
    // array length overflowing usize multiplication
    let arr = abi::encode(&[Token::Array(vec![Token::uint(1)])]);
    let mut bad = arr.clone();
    bad[56..64].copy_from_slice(&u64::MAX.to_be_bytes());
    assert_eq!(Reader::new(&bad).u256_array(0), Err(Error::InvalidAbi));
    assert_eq!(
        Reader::new(&bad).array(0, 3).map(|x| x.0),
        Err(Error::InvalidAbi)
    );
    // dirty address / bool / uint64
    let mut w = abi::encode(&[Token::address(&[0xaa; 20])]);
    assert_eq!(abi::decode_address(&w).unwrap(), [0xaa; 20]);
    w[0] = 1;
    assert_eq!(abi::decode_address(&w), Err(Error::InvalidAbi));
    assert_eq!(
        abi::decode_bool(&abi::encode(&[Token::uint(2)])),
        Err(Error::InvalidAbi)
    );
    assert_eq!(
        Reader::new(&abi::encode(&[Token::u256(&[1; 32])])).u64(0),
        Err(Error::InvalidAbi)
    );
    // invalid UTF-8 string
    let s = abi::encode(&[Token::bytes(&[0xff, 0xfe])]);
    assert_eq!(abi::decode_string(&s), Err(Error::InvalidAbi));
    // bytes4 with dirty padding
    assert!(Reader::new(&[1u8; 32]).bytes4(0).is_err());
    // getState: status out of enum range
    let st = registry::State {
        status: registry::Status::Registered,
        expiry: 5,
        latest_owner: [3; 20],
        token_id: [4; 32],
        resource: [5; 32],
    };
    let mut enc = registry::encode_state(&st);
    assert_eq!(registry::decode_state(&enc).unwrap(), st);
    enc[31] = 3;
    assert_eq!(registry::decode_state(&enc), Err(Error::InvalidAbi));
    // wrong selector
    assert_eq!(
        OffchainLookup::decode(&[0, 0, 0, 0]),
        Err(Error::UnexpectedSelector)
    );
}

#[test]
fn ur_error_unknown_and_malformed() {
    assert_eq!(ur::decode_ur_error(&[]), UrError::Unknown(vec![]));
    let mut e = ur::encode_ur_error(&UrError::ResolverNotFound {
        name: vec![3, b'e', b't', b'h', 0],
    });
    assert!(matches!(
        ur::decode_ur_error(&e),
        UrError::ResolverNotFound { .. }
    ));
    e.truncate(40);
    assert!(matches!(ur::decode_ur_error(&e), UrError::Unknown(_)));
}

#[test]
fn dns_decoding_errors() {
    for bad in [
        &b""[..],
        &[1],
        &[1, b'a'],
        &[1, b'a', 0, 0],
        &[0, 0],
        &[2, b'a', 0],
        &[5, b'a', 0],
        &[3, b'a', b'.', b'b', 0],
    ] {
        assert!(name::dns_decode(bad).is_err(), "{bad:?}");
    }
    assert_eq!(
        name::dns_decode(&[2, 0xff, 0xfe, 0]),
        Err(Error::InvalidUtf8)
    );
    assert_eq!(name::dns_decode(&[0]).unwrap(), "");
    assert_eq!(name::namehash_dns(&[0]).unwrap(), [0; 32]);
    assert!(name::namehash_dns(&[1, b'a']).is_err());
    // a label containing '.' still hashes (NameCoder.namehash does not check)
    assert!(name::namehash_dns(&[3, b'a', b'.', b'b', 0]).is_ok());
    assert_eq!(name::packet_to_bytes(""), vec![0]);
    assert_eq!(name::dns_encode("."), Ok(vec![0]));
    assert_eq!(name::dns_encode("a..b"), Err(Error::EmptyLabel));
    assert_eq!(
        name::dns_labels_str(&name::dns_encode("a.bc.eth").unwrap()).unwrap(),
        ["a", "bc", "eth"]
    );
    assert_eq!(name::labelhash(""), [0; 32]);
    let h = [0xab; 32];
    assert_eq!(
        name::parse_label_hash(&name::encode_label_hash(&h)),
        Some(h)
    );
    assert_eq!(name::parse_label_hash("[ab]"), None);
}

fn lookup_revert(sender: [u8; 20], urls: &[&str], cb: [u8; 4]) -> Vec<u8> {
    OffchainLookup {
        sender,
        urls: urls.iter().map(|s| s.to_string()).collect(),
        call_data: vec![0xca, 0xfe],
        callback_function: cb,
        extra_data: vec![],
    }
    .encode()
}

fn ok_json(hex: &str) -> Result<HttpResponse, HttpFailure> {
    Ok(HttpResponse {
        status: 200,
        content_type: Some("application/json".into()),
        body: format!("{{\"data\":\"{hex}\"}}"),
    })
}

#[test]
fn ccip_session_happy_path_and_url_fallback() {
    let to = [0x11; 20];
    let (mut s, step) = CcipReadSession::start(to, vec![1, 2]);
    assert_eq!(
        step,
        Step::EthCall {
            to,
            data: vec![1, 2]
        }
    );
    let urls = [
        "ftp://nope/{data}",
        "https://a.example/{sender}/{data}.json",
        "https://b.example/post",
    ];
    let step = s.on_eth_call(CallOutcome::Revert(lookup_revert(to, &urls, [9, 9, 9, 9])));
    // the ftp URL is skipped without a request
    let Step::Http(req) = step else {
        panic!("{step:?}")
    };
    assert_eq!(req.method, HttpMethod::Get);
    assert_eq!(
        req.url,
        format!("https://a.example/0x{}/0xcafe.json", hex::encode(to))
    );
    let Step::Http(req) = s.on_http(Err(HttpFailure::Timeout)) else {
        panic!()
    };
    assert_eq!(req.method, HttpMethod::Post);
    assert_eq!(req.content_type(), Some("application/json"));
    assert_eq!(
        req.body.as_deref(),
        Some("{\"data\":\"0xcafe\",\"sender\":\"0x1111111111111111111111111111111111111111\"}")
    );
    assert_eq!(s.remaining_queries(), MAX_QUERIES - 3);
    let step = s.on_http(ok_json("0xBEEF"));
    assert_eq!(
        step,
        Step::EthCall {
            to,
            data: callback_calldata([9, 9, 9, 9], &[0xbe, 0xef], &[])
        }
    );
    assert_eq!(
        s.on_eth_call(CallOutcome::Success(vec![7])),
        Step::Done(vec![7])
    );
    // finished: more input is a protocol error
    assert_eq!(
        s.on_eth_call(CallOutcome::Success(vec![])),
        Step::Failed(CcipError::UnexpectedInput)
    );
}

#[test]
fn ccip_session_failures() {
    let to = [0x22; 20];
    // non-OffchainLookup revert
    let (mut s, _) = CcipReadSession::start(to, vec![]);
    assert_eq!(
        s.on_eth_call(CallOutcome::Revert(vec![1, 2, 3, 4])),
        Step::Failed(CcipError::CallReverted(vec![1, 2, 3, 4]))
    );
    // sender mismatch
    let (mut s, _) = CcipReadSession::start(to, vec![]);
    let st = s.on_eth_call(CallOutcome::Revert(lookup_revert(
        [0x33; 20],
        &["https://a"],
        [0; 4],
    )));
    assert_eq!(
        st,
        Step::Failed(CcipError::SenderMismatch {
            sender: [0x33; 20],
            to
        })
    );
    // undecodable lookup
    let (mut s, _) = CcipReadSession::start(to, vec![]);
    assert_eq!(
        s.on_eth_call(CallOutcome::Revert(OFFCHAIN_LOOKUP_SELECTOR.to_vec())),
        Step::Failed(CcipError::InvalidOffchainLookup)
    );
    // no URLs
    let (mut s, _) = CcipReadSession::start(to, vec![]);
    assert_eq!(
        s.on_eth_call(CallOutcome::Revert(lookup_revert(to, &[], [0; 4]))),
        Step::Failed(CcipError::Gateway(GatewayError::NoUrls))
    );
    // only invalid URLs
    let (mut s, _) = CcipReadSession::start(to, vec![]);
    assert_eq!(
        s.on_eth_call(CallOutcome::Revert(lookup_revert(
            to,
            &["mailto:x", "nope"],
            [0; 4]
        ))),
        Step::Failed(CcipError::Gateway(GatewayError::Request))
    );
    // last error wins
    let (mut s, _) = CcipReadSession::start(to, vec![]);
    assert!(matches!(
        s.on_eth_call(CallOutcome::Revert(lookup_revert(
            to,
            &["https://a", "https://b"],
            [0; 4]
        ))),
        Step::Http(_)
    ));
    assert!(matches!(
        s.on_http(Err(HttpFailure::Network)),
        Step::Http(_)
    ));
    let r = HttpResponse {
        status: 502,
        content_type: None,
        body: String::new(),
    };
    assert_eq!(
        s.on_http(Ok(r)),
        Step::Failed(CcipError::Gateway(GatewayError::Http { status: 502 }))
    );
    // wrong input kind
    let (mut s, _) = CcipReadSession::start(to, vec![]);
    assert_eq!(
        s.on_http(Err(HttpFailure::Network)),
        Step::Failed(CcipError::UnexpectedInput)
    );
    // local batch with undecodable call data
    let (mut s, _) = CcipReadSession::start(to, vec![]);
    let st = s.on_eth_call(CallOutcome::Revert(lookup_revert(
        to,
        &[LOCAL_BATCH_GATEWAY_URL],
        [0; 4],
    )));
    assert_eq!(st, Step::Failed(CcipError::InvalidBatchCall));
}

#[test]
fn ccip_depth_limit() {
    let to = [0x44; 20];
    let (mut s, _) = CcipReadSession::start(to, vec![]);
    let revert = lookup_revert(to, &["https://a/{data}"], [1, 1, 1, 1]);
    let mut step = s.on_eth_call(CallOutcome::Revert(revert.clone()));
    for _ in 0..MAX_LOOKUP_DEPTH {
        assert!(matches!(step, Step::Http(_)), "{step:?}");
        let cb = s.on_http(ok_json("0x01"));
        assert!(matches!(cb, Step::EthCall { .. }));
        step = s.on_eth_call(CallOutcome::Revert(revert.clone()));
    }
    assert_eq!(step, Step::Failed(CcipError::DepthExceeded));
}

#[test]
fn gateway_response_edge_cases() {
    let resp = |status, ct: Option<&str>, body: &str| HttpResponse {
        status,
        content_type: ct.map(String::from),
        body: body.into(),
    };
    assert_eq!(
        parse_gateway_response(&resp(200, None, "0x123")),
        Err(GatewayError::Malformed)
    );
    assert_eq!(parse_gateway_response(&resp(200, None, "0x")), Ok(vec![]));
    assert_eq!(
        parse_gateway_response(&resp(200, Some("application/json"), "{\"data\":\"0x0\"}")),
        Err(GatewayError::Malformed)
    );
    let huge = "0".repeat(MAX_RESPONSE_BYTES + 1);
    assert_eq!(
        parse_gateway_response(&resp(200, None, &huge)),
        Err(GatewayError::BodyTooLarge)
    );
    assert_eq!(
        GatewayError::BodyTooLarge.encode_batch_failure(),
        abi::encode_sig(
            "Error(string)",
            &[Token::string("HTTP response body exceeded the size limit.")]
        )
    );
    assert_eq!(
        GatewayError::Http { status: 0 }.encode_batch_failure(),
        abi::encode_sig("Error(string)", &[Token::string("HTTP request failed.")])
    );
    assert!(is_allowed_url("https://x"));
    assert!(is_allowed_url("\t http://x"));
    assert!(!is_allowed_url("x-batch-gateway:true"));
    assert!(!is_allowed_url("//x"));
}

fn key(b: u8) -> SecretKey {
    SecretKey::from_bytes(&[b; 32]).unwrap()
}

fn meta() -> StealthMetaAddress {
    StealthMetaAddress::new(&key(1).public_key(), &key(2).public_key()).unwrap()
}

#[test]
fn scheme1_verifier_rejections() {
    let m = meta();
    let raw = m.to_raw();
    let vd = stealth::scheme1_verification_data(&key(3), &m).unwrap();
    let ok = stealth::scheme1_verify(&raw, &vd[..], &[0; 20]).unwrap();
    assert_eq!(
        ok,
        lowki_ens::generate_stealth_address(&m, &key(3))
            .unwrap()
            .stealth_address
    );
    // unverified mode returns `claimed`
    assert_eq!(
        stealth::scheme1_verify(&[], &[], &[5; 20]).unwrap(),
        [5; 20]
    );
    // verified mode ignores `claimed`
    assert_eq!(
        stealth::scheme1_verify(&raw, &vd[..], &[5; 20]).unwrap(),
        ok
    );
    // meta-address
    assert_eq!(
        stealth::scheme1_verify(&raw[..65], &vd[..], &[0; 20]),
        Err(Error::MalformedMetaAddress)
    );
    let mut bad = raw;
    bad[0] = 4;
    assert_eq!(
        stealth::scheme1_verify(&bad, &vd[..], &[0; 20]),
        Err(Error::MalformedMetaAddress)
    );
    let mut bad = raw;
    bad[34..66].copy_from_slice(&[0; 32]);
    assert_eq!(
        stealth::scheme1_verify(&bad, &vd[..], &[0; 20]),
        Err(Error::MalformedMetaAddress)
    );
    let mut bad = raw;
    bad[1..33].copy_from_slice(&[0xff; 32]); // x >= n
    assert_eq!(
        stealth::scheme1_verify(&bad, &vd[..], &[0; 20]),
        Err(Error::MalformedMetaAddress)
    );
    let mut bad = raw;
    bad[1..33].copy_from_slice(&{
        let mut x = [0u8; 32];
        x[31] = 5; // x = 5 is not on secp256k1
        x
    });
    assert_eq!(
        stealth::scheme1_verify(&bad, &vd[..], &[0; 20]),
        Err(Error::MalformedMetaAddress)
    );
    // verification data
    assert_eq!(
        stealth::scheme1_verify(&raw, &vd[..95], &[0; 20]),
        Err(Error::InvalidVerificationData)
    );
    let mut e0 = *vd;
    e0[..32].copy_from_slice(&[0; 32]);
    assert_eq!(
        stealth::scheme1_verify(&raw, &e0, &[0; 20]),
        Err(Error::InvalidVerificationData)
    );
    let mut en = *vd;
    en[..32].copy_from_slice(&[0xff; 32]);
    assert_eq!(
        stealth::scheme1_verify(&raw, &en, &[0; 20]),
        Err(Error::InvalidVerificationData)
    );
    let mut ts = *vd;
    ts[40] ^= 1;
    assert_eq!(
        stealth::scheme1_verify(&raw, &ts, &[0; 20]),
        Err(Error::InvalidSharedSecret)
    );
    // the wrong viewing key in the meta-address
    let other = StealthMetaAddress::new(&key(1).public_key(), &key(9).public_key()).unwrap();
    assert_eq!(
        stealth::scheme1_verify(&other.to_raw(), &vd[..], &[0; 20]),
        Err(Error::InvalidSharedSecret)
    );
}

#[test]
fn stealth_records_and_keys() {
    let m = meta();
    let rec = stealth::encode_stealth_record(true, &m);
    assert_eq!(rec.len(), 67);
    assert_eq!(stealth::decode_stealth_record(&rec).unwrap().meta, m);
    assert_eq!(
        stealth::decode_stealth_record(&[]),
        Err(Error::InvalidStealthRecord)
    );
    for bad in [
        "stealth-meta-address[01][60]",
        "stealth-meta-address[1][0x3c]",
        "stealth-meta-address[1][]",
        "stealth-meta-address[1][60",
        "stealth-meta-address[1]",
        "stealth-meta-address[1][+60]",
        "stealth-meta-address[1][18446744073709551616]",
        "x[1][60]",
    ] {
        assert_eq!(stealth::parse_stealth_data_key(bad), None, "{bad}");
    }
    assert_eq!(
        stealth::parse_stealth_data_key("stealth-meta-address[1][0]"),
        Some((1, 0))
    );
    assert_eq!(
        stealth::stealth_data_key(1, resolver::evm_coin_type(11155111)),
        "stealth-meta-address[1][2158638759]"
    );
    assert_eq!(resolver::evm_coin_type(1), 60);
}

#[test]
fn addr_query_parsing() {
    assert_eq!(stealth::parse_addr_query(&[]), None);
    assert_eq!(
        stealth::parse_addr_query(&resolver::encode_text(&[1; 32], "x")),
        None
    );
    let q = stealth::parse_addr_query(&resolver::encode_addr_coin(&[1; 32], 2147483658)).unwrap();
    assert_eq!(q.coin_type_u64(), Some(2147483658));
    assert!(!q.legacy);
    // truncated addr calldata
    assert_eq!(
        stealth::parse_addr_query(&resolver::encode_addr(&[1; 32])[..20]),
        None
    );
}

#[test]
fn redaction_and_identity_limits() {
    let m = meta();
    let g = stealth::gateway_derive(&m, &key(7)).unwrap();
    let dbg = format!("{g:?}");
    assert!(dbg.contains("redacted"));
    assert!(!dbg.contains(&hex::encode(&g.verification_data[..32])));
    let id = IdentityKey::from_seed(&[1; 64], 0).unwrap();
    assert!(!format!("{id:?}").contains(&hex::encode(*id.secret_key().to_bytes())));
    assert_eq!(
        IdentityKey::from_seed(&[1; 64], 1 << 31).unwrap_err(),
        Error::InvalidDerivation
    );
    let r: Result<IdentityKey, Error> = IdentityKey::from_derive_at(1 << 31, |_| unreachable!());
    assert_eq!(r.unwrap_err(), Error::InvalidDerivation);
    let r: Result<IdentityKey, Error> =
        IdentityKey::from_derive_at(0, |_| Ok(zeroize::Zeroizing::new([0; 32])));
    assert_eq!(r.unwrap_err(), Error::InvalidPrivateKey);
    assert_ne!(
        id.address(),
        IdentityKey::from_seed(&[1; 64], 1).unwrap().address()
    );
    // identity key differs from the stealth keys of the same seed
    let sk = lowki_stealth::derive_stealth_keys(&[1; 64]).unwrap();
    assert_ne!(id.address(), sk.spending.address());
    assert_ne!(id.address(), sk.viewing.address());
}

#[test]
fn signatures_reject_high_s_and_bad_v() {
    let k = key(5);
    let d = [0x42; 32];
    let s = sig::sign_digest(&k, &d).unwrap();
    assert_eq!(sig::recover_digest(&d, &s).unwrap(), k.address());
    let mut s01 = s;
    s01[64] -= 27;
    assert_eq!(sig::recover_digest(&d, &s01).unwrap(), k.address());
    let mut bad_v = s;
    bad_v[64] = 29;
    assert_eq!(
        sig::recover_digest(&d, &bad_v),
        Err(Error::InvalidSignature)
    );
    assert_eq!(
        sig::recover_digest(&d, &s[..64]),
        Err(Error::InvalidSignature)
    );
    // malleated (high-s) signature: s' = n - s, parity flipped
    let n: [u8; 32] =
        hex::decode("fffffffffffffffffffffffffffffffebaaedce6af48a03bbfd25e8cd0364141")
            .unwrap()
            .try_into()
            .unwrap();
    let mut hs = s;
    let mut borrow = 0i16;
    for i in (0..32).rev() {
        let v = n[i] as i16 - s[32 + i] as i16 - borrow;
        hs[32 + i] = v.rem_euclid(256) as u8;
        borrow = (v < 0) as i16;
    }
    hs[64] = if s[64] == 27 { 28 } else { 27 };
    assert_eq!(sig::recover_digest(&d, &hs), Err(Error::InvalidSignature));
}

#[test]
fn lowki_label_rules() {
    for ok in [
        "abc",
        "a-b",
        "a1-b2-c3",
        "000",
        &"z".repeat(32),
        "ab-cd",
        "abc--d",
    ] {
        assert_eq!(validate_label(ok), Ok(()), "{ok}");
    }
    for bad in [
        "",
        "ab",
        &"z".repeat(33),
        "-ab",
        "ab-",
        "ab--c",
        "xn--ab",
        "aBc",
        "a_b",
        "a.b",
        "añb",
        "a b",
    ] {
        assert!(validate_label(bad).is_err(), "{bad}");
    }
}
