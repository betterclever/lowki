//! Differential tests against vectors produced by `vectors/lowki-stealth/gen.ts`
//! (@scopelift/stealth-address-sdk 1.0.0-beta.5, viem 2.57.16, @scure/bip32 2.0.1).

use lowki_stealth::*;
use serde_json::Value;

const VECTORS: &str = include_str!("../../../vectors/lowki-stealth/vectors.json");

fn v() -> Value {
    serde_json::from_str(VECTORS).unwrap()
}

fn hx(v: &Value) -> Vec<u8> {
    let s = v.as_str().unwrap();
    hex::decode(s.strip_prefix("0x").unwrap()).unwrap()
}
fn h32(v: &Value) -> [u8; 32] {
    hx(v).try_into().unwrap()
}
fn h33(v: &Value) -> [u8; 33] {
    hx(v).try_into().unwrap()
}
fn addr(v: &Value) -> Address {
    hx(v).try_into().unwrap()
}
fn sk(v: &Value) -> SecretKey {
    SecretKey::from_slice(&hx(v)).unwrap()
}
fn u256_dec(s: &str) -> [u8; 32] {
    // decimal string -> big-endian 32 bytes
    let mut out = [0u8; 32];
    for c in s.bytes() {
        let mut carry = (c - b'0') as u32;
        for b in out.iter_mut().rev() {
            let x = (*b as u32) * 10 + carry;
            *b = x as u8;
            carry = x >> 8;
        }
        assert_eq!(carry, 0);
    }
    out
}

#[test]
fn constants() {
    let v = v();
    assert_eq!(
        to_checksum_address(&ERC5564_ANNOUNCER),
        v["constants"]["erc5564_announcer"].as_str().unwrap()
    );
    assert_eq!(
        to_checksum_address(&ERC6538_REGISTRY),
        v["constants"]["erc6538_registry"].as_str().unwrap()
    );
    assert_eq!(
        announcement_topic0(),
        h32(&v["constants"]["announcement_topic0"])
    );
}

#[test]
fn meta_address_round_trips() {
    let v = v();
    let cases = v["meta_address"].as_array().unwrap();
    assert!(cases.len() >= 64);
    for c in cases {
        let s = h33(&c["spending_pubkey"]);
        let w = h33(&c["viewing_pubkey"]);
        let m = StealthMetaAddress::new(&s, &w).unwrap();
        let uri = c["uri"].as_str().unwrap();
        assert_eq!(m.to_uri(), uri);
        assert_eq!(m.to_raw().to_vec(), hx(&c["raw"]));
        let p = StealthMetaAddress::parse_uri(uri).unwrap();
        assert_eq!(p, m);
        assert_eq!(
            StealthMetaAddress::parse_uri(&uri.to_uppercase().replace("ST:ETH:0X", "st:eth:0x"))
                .unwrap(),
            m
        );
        assert_eq!(StealthMetaAddress::from_raw(&hx(&c["raw"])).unwrap(), m);
        assert_eq!(p.spending_pubkey(), &h33(&c["parsed_spending"]));
        assert_eq!(p.viewing_pubkey(), &h33(&c["parsed_viewing"]));
    }
    for c in v["single_key_meta_address"].as_array().unwrap() {
        let m = StealthMetaAddress::from_raw(&hx(&c["raw"])).unwrap();
        assert_eq!(m.spending_pubkey(), &h33(&c["parsed_spending"]));
        assert_eq!(m.viewing_pubkey(), &h33(&c["parsed_viewing"]));
    }
}

#[test]
fn meta_address_rejects_garbage() {
    let v = v();
    let uri = v["meta_address"][0]["uri"].as_str().unwrap().to_string();
    let raw = hx(&v["meta_address"][0]["raw"]);
    let bad = [
        uri.replace("st:eth:", "st:btc:"),
        uri.replace("st:eth:0x", "st:eth:"),
        uri.replace("st:eth:", ""),
        format!("{uri}00"),
        uri[..uri.len() - 2].to_string(),
        format!("{}zz", &uri[..uri.len() - 2]),
    ];
    for b in &bad {
        assert!(StealthMetaAddress::parse_uri(b).is_err(), "{b}");
    }
    // bad prefix byte, off-curve x, wrong lengths
    let mut r = raw.clone();
    r[0] = 4;
    assert_eq!(
        StealthMetaAddress::from_raw(&r),
        Err(Error::InvalidPublicKey)
    );
    let mut r = raw.clone();
    r[33] = 0x05;
    assert!(StealthMetaAddress::from_raw(&r).is_err());
    let mut off = [0u8; 66];
    off[0] = 2;
    off[33] = 2; // x = 0 is not on secp256k1 (0^3+7 = 7 is a non-residue mod p)
    assert!(StealthMetaAddress::from_raw(&off).is_err());
    for len in [0, 32, 34, 65, 67, 132] {
        assert!(StealthMetaAddress::from_raw(&vec![2u8; len]).is_err());
    }
}

#[test]
fn stealth_generate_check_compute() {
    let v = v();
    let cases = v["stealth"].as_array().unwrap();
    assert!(cases.len() >= 160);
    for (idx, c) in cases.iter().enumerate() {
        let label = c["label"].as_str().unwrap();
        let spend = sk(&c["spending_priv"]);
        let view = sk(&c["viewing_priv"]);
        let eph = sk(&c["ephemeral_priv"]);
        assert_eq!(spend.public_key(), h33(&c["spending_pubkey"]), "{label}");
        assert_eq!(view.public_key(), h33(&c["viewing_pubkey"]), "{label}");
        let meta = StealthMetaAddress::parse_uri(c["meta_uri"].as_str().unwrap()).unwrap();

        // hashing convention: keccak256 over 33-byte compressed shared point
        assert_eq!(
            keccak256(&hx(&c["shared_secret_compressed"])),
            h32(&c["hashed_shared_secret"]),
            "{label}"
        );

        let g = generate_stealth_address(&meta, &eph).unwrap();
        let expected_addr = addr(&c["stealth_address"]);
        assert_eq!(g.stealth_address, expected_addr, "{label}");
        assert_eq!(
            to_checksum_address(&g.stealth_address),
            c["stealth_address"].as_str().unwrap()
        );
        assert_eq!(
            g.ephemeral_pubkey.to_vec(),
            hx(&c["ephemeral_pubkey"]),
            "{label}"
        );
        assert_eq!(g.view_tag, hx(&c["view_tag"])[0], "{label}");
        assert_eq!(g.view_tag, h32(&c["hashed_shared_secret"])[0], "{label}");

        // recipient check
        let found = check_announcement(
            &view,
            meta.spending_pubkey(),
            &g.ephemeral_pubkey,
            g.view_tag,
        );
        assert_eq!(found, Some(expected_addr), "{label}");

        // stealth private key
        let k = compute_stealth_private_key(&spend, &view, &g.ephemeral_pubkey).unwrap();
        assert_eq!(k.to_bytes().to_vec(), hx(&c["stealth_priv"]), "{label}");
        assert_eq!(
            k.address(),
            addr(&c["address_from_stealth_priv"]),
            "{label}"
        );
        assert_eq!(k.address(), expected_addr, "{label}");

        // uncompressed ephemeral key works too
        let unc = uncompress(&g.ephemeral_pubkey);
        assert_eq!(
            check_announcement(&view, meta.spending_pubkey(), &unc, g.view_tag),
            Some(expected_addr)
        );
        assert_eq!(
            compute_stealth_private_key(&spend, &view, &unc)
                .unwrap()
                .address(),
            expected_addr
        );

        // wrong view tag rejected (SDK rejected it too)
        assert!(!c["wrong_view_tag_accepted"].as_bool().unwrap());
        let wt = hx(&c["wrong_view_tag"])[0];
        assert_ne!(wt, g.view_tag);
        assert_eq!(
            check_announcement(&view, meta.spending_pubkey(), &g.ephemeral_pubkey, wt),
            None,
            "{label}"
        );
        // exhaustive view-tag sweep on a subset (slow in debug builds)
        for t in 0..=255u8 {
            if idx < 12 && t != g.view_tag {
                assert_eq!(
                    check_announcement(&view, meta.spending_pubkey(), &g.ephemeral_pubkey, t),
                    None
                );
            }
        }

        // wrong viewing key rejected (SDK rejected it too)
        assert!(!c["wrong_viewing_priv_accepted"].as_bool().unwrap());
        let wrong = sk(&c["wrong_viewing_priv"]);
        let wrong_tag = hx(&c["wrong_viewing_priv_view_tag"])[0];
        let r = check_announcement(
            &wrong,
            meta.spending_pubkey(),
            &g.ephemeral_pubkey,
            g.view_tag,
        );
        if wrong_tag == g.view_tag {
            // 1/256 view-tag collision: passes the filter but yields a different address.
            assert_ne!(r, Some(expected_addr));
        } else {
            assert_eq!(r, None, "{label}");
        }
        // even if we ignore the filter, the wrong key derives a different address
        let r2 = check_announcement(
            &wrong,
            meta.spending_pubkey(),
            &g.ephemeral_pubkey,
            wrong_tag,
        );
        assert!(r2.is_some());
        assert_ne!(r2, Some(expected_addr));
        // wrong spending key → different address
        let other_spend = view.public_key();
        if other_spend != *meta.spending_pubkey() {
            assert_ne!(
                check_announcement(&view, &other_spend, &g.ephemeral_pubkey, g.view_tag),
                Some(expected_addr)
            );
        }
    }
}

fn uncompress(c: &[u8; 33]) -> Vec<u8> {
    use k256::elliptic_curve::sec1::ToEncodedPoint;
    k256::PublicKey::from_sec1_bytes(c)
        .unwrap()
        .to_encoded_point(false)
        .as_bytes()
        .to_vec()
}

#[test]
fn metadata_layouts() {
    let v = v();
    let cases = v["metadata"].as_array().unwrap();
    for c in cases {
        let md = hx(&c["metadata"]);
        let vt = hx(&c["view_tag"])[0];
        assert_eq!(view_tag_from_metadata(&md), Some(vt));
        let kind = c["kind"].as_str().unwrap();
        let amount = c["amount"].as_str().map(u256_dec);
        let token = if c["token"].is_null() {
            None
        } else {
            Some(addr(&c["token"]))
        };
        let built: Option<[u8; 57]> = match kind {
            "eth" => Some(metadata_eth(vt, &amount.unwrap())),
            "erc20" => Some(metadata_erc20(vt, &token.unwrap(), &amount.unwrap())),
            "erc721" => Some(metadata_erc721(vt, &token.unwrap(), &amount.unwrap())),
            "custom" => {
                let sel: [u8; 4] = hx(&c["selector"]).try_into().unwrap();
                Some(build_metadata(vt, sel, &token.unwrap(), &amount.unwrap()))
            }
            _ => None,
        };
        if let Some(b) = built {
            assert_eq!(b.to_vec(), md, "{kind}");
            let p = parse_metadata(&md).unwrap();
            assert_eq!(p.view_tag, vt);
            assert_eq!(p.amount, amount.unwrap());
            assert_eq!(
                build_metadata(
                    p.view_tag,
                    p.function_identifier,
                    &p.contract_address,
                    &p.amount
                ),
                b
            );
        } else if md.len() != 57 {
            assert!(parse_metadata(&md).is_err());
        }
    }
    assert_eq!(view_tag_from_metadata(&[]), None);
}

#[test]
fn announce_calldata_matches_viem() {
    let v = v();
    for c in v["announce"].as_array().unwrap() {
        let cd = encode_announce_calldata(
            1,
            &addr(&c["stealth_address"]),
            &hx(&c["ephemeral_pubkey"]),
            &hx(&c["metadata"]),
        );
        assert_eq!(cd, hx(&c["calldata"]));
    }
}

#[test]
fn announcement_event_decoding_matches_viem() {
    let v = v();
    let cases = v["events"].as_array().unwrap();
    for c in cases {
        let topics: Vec<[u8; 32]> = c["topics"].as_array().unwrap().iter().map(h32).collect();
        let data = hx(&c["data"]);
        let a = decode_announcement_log(&topics, &data).unwrap();
        let d = &c["decoded"];
        assert_eq!(a.scheme_id, u256_dec(d["scheme_id"].as_str().unwrap()));
        assert_eq!(a.is_scheme_1(), d["scheme_id"].as_str().unwrap() == "1");
        assert_eq!(a.stealth_address, addr(&d["stealth_address"]));
        assert_eq!(a.caller, addr(&d["caller"]));
        assert_eq!(a.ephemeral_pubkey, hx(&d["ephemeral_pubkey"]));
        assert_eq!(a.metadata, hx(&d["metadata"]));
        assert_eq!(a.view_tag(), Some(hx(&d["metadata"])[0]));

        // malformed inputs rejected
        assert_eq!(
            decode_announcement_log(&topics[..3], &data),
            Err(Error::InvalidLog)
        );
        let mut t = topics.clone();
        t[0][0] ^= 1;
        assert_eq!(decode_announcement_log(&t, &data), Err(Error::InvalidLog));
        let mut t = topics.clone();
        t[2][0] = 1;
        assert_eq!(decode_announcement_log(&t, &data), Err(Error::InvalidLog));
        assert_eq!(
            decode_announcement_log(&topics, &data[..data.len() - 32]),
            Err(Error::InvalidAbi)
        );
        assert_eq!(
            decode_announcement_log(&topics, &data[..40]),
            Err(Error::InvalidAbi)
        );
        let mut bad = data.clone();
        bad[31] = 0xff; // offset out of range
        bad[30] = 0xff;
        assert_eq!(
            decode_announcement_log(&topics, &bad),
            Err(Error::InvalidAbi)
        );
    }
}

#[test]
fn end_to_end_announcement_scan() {
    // Sender builds calldata/log from vectors; recipient scans the decoded log.
    let v = v();
    for (i, c) in v["stealth"].as_array().unwrap().iter().enumerate().take(64) {
        let spend = sk(&c["spending_priv"]);
        let view = sk(&c["viewing_priv"]);
        let meta = StealthMetaAddress::parse_uri(c["meta_uri"].as_str().unwrap()).unwrap();
        let g = generate_stealth_address(&meta, &sk(&c["ephemeral_priv"])).unwrap();
        let md = metadata_eth(g.view_tag, &[0u8; 32]);
        let ev = &v["events"][i % v["events"].as_array().unwrap().len()];
        let topics: Vec<[u8; 32]> = ev["topics"].as_array().unwrap().iter().map(h32).collect();
        let a = decode_announcement_log(&topics, &hx(&ev["data"])).unwrap();
        // events[i] announces stealth[i] (see gen.ts)
        assert_eq!(a.stealth_address, g.stealth_address);
        {
            let tag = a.view_tag().unwrap();
            let found = check_announcement(&view, meta.spending_pubkey(), &a.ephemeral_pubkey, tag);
            assert_eq!(found, Some(a.stealth_address));
            let k = compute_stealth_private_key(&spend, &view, &a.ephemeral_pubkey).unwrap();
            assert_eq!(k.address(), a.stealth_address);
        }
        assert_eq!(view_tag_from_metadata(&md), Some(g.view_tag));
    }
}

#[test]
fn registry_calldata_matches_viem() {
    let v = v();
    for c in v["registry"].as_array().unwrap() {
        let m = StealthMetaAddress::from_raw(&hx(&c["meta_raw"])).unwrap();
        assert_eq!(
            encode_register_keys_calldata(1, &m),
            hx(&c["register_keys_calldata"])
        );
        assert_eq!(
            encode_stealth_meta_address_of_calldata(&addr(&c["registrant"]), 1),
            hx(&c["stealth_meta_address_of_calldata"])
        );
        let r = decode_stealth_meta_address_of_return(&hx(&c["stealth_meta_address_of_return"]))
            .unwrap();
        assert_eq!(r, Some(m));
    }
    // empty bytes → not registered
    let mut empty = vec![0u8; 64];
    empty[31] = 0x20;
    assert_eq!(decode_stealth_meta_address_of_return(&empty), Ok(None));
}

#[test]
fn bip32_matches_scure() {
    let v = v();
    assert_eq!(v["bip32"]["spending_path"], "m/5564'/0'/0'");
    assert_eq!(v["bip32"]["viewing_path"], "m/5564'/0'/1'");
    let cases = v["bip32"]["cases"].as_array().unwrap();
    assert!(cases.len() >= 64);
    for c in cases {
        let seed: [u8; 64] = hx(&c["seed"]).try_into().unwrap();
        let m = bip32::master_key(&seed).unwrap();
        assert_eq!(m.key.to_bytes().to_vec(), hx(&c["master_priv"]));
        assert_eq!(m.chain_code.to_vec(), hx(&c["master_chain_code"]));
        let keys = derive_stealth_keys(&seed).unwrap();
        assert_eq!(keys.spending.to_bytes().to_vec(), hx(&c["spending_priv"]));
        assert_eq!(keys.viewing.to_bytes().to_vec(), hx(&c["viewing_priv"]));
        assert_eq!(keys.spending.public_key(), h33(&c["spending_pubkey"]));
        assert_eq!(keys.viewing.public_key(), h33(&c["viewing_pubkey"]));
        assert_eq!(
            keys.meta_address().to_uri(),
            c["meta_uri"].as_str().unwrap()
        );
    }
}

#[test]
fn bip32_official_test_vector_1() {
    // BIP-32 test vector 1 (seed 000102...0f): m/0'
    let seed = hex::decode("000102030405060708090a0b0c0d0e0f").unwrap();
    let m = bip32::master_key(&seed).unwrap();
    assert_eq!(
        hex::encode(*m.key.to_bytes()),
        "e8f32e723decf4051aefac8e2c93c9c5b214313817cdb01a1494b917c8436b35"
    );
    let c = m.derive_hardened(bip32::HARDENED).unwrap();
    assert_eq!(
        hex::encode(*c.key.to_bytes()),
        "edb2e14f9ee77d26dd93b4ecede8d16ed408ce149b6cd80b0715a2d911a0afea"
    );
    assert!(m.derive_hardened(0).is_err());
}

#[test]
fn invalid_inputs() {
    let n_minus_1 =
        hex::decode("fffffffffffffffffffffffffffffffebaaedce6af48a03bbfd25e8cd0364140").unwrap();
    assert!(SecretKey::from_slice(&n_minus_1).is_ok());
    let n =
        hex::decode("fffffffffffffffffffffffffffffffebaaedce6af48a03bbfd25e8cd0364141").unwrap();
    assert_eq!(
        SecretKey::from_slice(&n).unwrap_err(),
        Error::InvalidPrivateKey
    );
    assert_eq!(
        SecretKey::from_slice(&[0u8; 32]).unwrap_err(),
        Error::InvalidPrivateKey
    );
    assert_eq!(
        SecretKey::from_slice(&[1u8; 31]).unwrap_err(),
        Error::InvalidPrivateKey
    );
    let k = SecretKey::from_slice(&[7u8; 32]).unwrap();
    assert_eq!(format!("{k:?}"), "SecretKey(<redacted>)");
    let spend = k.public_key();
    let mut x0 = [0u8; 33];
    x0[0] = 2; // x = 0 is not on the curve
    for bad in [
        &[][..],
        &x0[..],
        &[4u8; 65][..],
        &[2u8; 32][..],
        &[5u8; 33][..],
    ] {
        assert_eq!(check_announcement(&k, &spend, bad, 0), None);
        assert!(compute_stealth_private_key(&k, &k, bad).is_err());
    }
}

#[test]
fn stealth_point_at_infinity_is_an_error() {
    // Choose spending key = n - s_h so that P_spend + s_h·G = O.
    use k256::elliptic_curve::PrimeField;
    let view = SecretKey::from_slice(&[3u8; 32]).unwrap();
    let eph = SecretKey::from_slice(&[5u8; 32]).unwrap();
    let shared = {
        use k256::elliptic_curve::sec1::ToEncodedPoint;
        let vp = k256::PublicKey::from_sec1_bytes(&view.public_key()).unwrap();
        let e = k256::NonZeroScalar::try_from(&eph.to_bytes()[..]).unwrap();
        (vp.to_projective() * *e)
            .to_affine()
            .to_encoded_point(true)
            .as_bytes()
            .to_vec()
    };
    let h = keccak256(&shared);
    let s_h = k256::Scalar::from_repr(h.into()).unwrap();
    let spend_scalar = -s_h;
    let spend = SecretKey::from_slice(&spend_scalar.to_repr()).unwrap();
    let meta = StealthMetaAddress::new(&spend.public_key(), &view.public_key()).unwrap();
    assert_eq!(
        generate_stealth_address(&meta, &eph),
        Err(Error::PointAtInfinity)
    );
    assert_eq!(
        check_announcement(&view, &spend.public_key(), &eph.public_key(), h[0]),
        None
    );
    assert_eq!(
        compute_stealth_private_key(&spend, &view, &eph.public_key()).unwrap_err(),
        Error::PointAtInfinity
    );
}
