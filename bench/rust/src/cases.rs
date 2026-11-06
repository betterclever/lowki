use serde_json::Value;
use std::hint::black_box;

pub struct Case {
    pub id: &'static str,
    pub batch: usize,
    pub iters: Option<usize>,
    pub warmup: Option<usize>,
    pub run: Box<dyn FnMut()>,
    pub check: Box<dyn FnMut() -> String>,
}

fn case(
    id: &'static str,
    batch: usize,
    iters: Option<usize>,
    run: impl FnMut() + 'static,
    check: impl FnMut() -> String + 'static,
) -> Case {
    Case { id, batch, iters, warmup: iters.map(|_| 2), run: Box::new(run), check: Box::new(check) }
}

#[allow(dead_code)]
fn h32(s: &str) -> [u8; 32] {
    hex::decode(s.trim_start_matches("0x")).unwrap().try_into().unwrap()
}
#[allow(dead_code)]
fn s(v: &Value) -> &str {
    v.as_str().unwrap()
}

/// `fx` maps fixture name (keys, pp, stealth, aa) to its parsed JSON.
#[allow(unused_variables, unused_mut)]
pub fn cases(fx: &Value) -> Vec<Case> {
    let mut out = Vec::new();

    #[cfg(feature = "keys")]
    {
        use lowki_keys::*;
        let k = &fx["keys"];
        let m = s(&k["mnemonic"]).to_string();
        let pp_path = s(&k["ppPath"]).to_string();
        let eoa: Vec<String> = k["eoaPaths"].as_array().unwrap().iter().map(|p| s(p).to_string()).collect();
        let seed = *mnemonic_to_seed(&m).unwrap();
        {
            let m2 = m.clone();
            let m = m.clone();
            out.push(case(
                "keys.mnemonic_to_seed", 1, None,
                move || { black_box(mnemonic_to_seed(black_box(&m)).unwrap()); },
                move || hex::encode(*mnemonic_to_seed(&m2).unwrap()),
            ));
        }
        {
            let (p, p2) = (pp_path.clone(), pp_path.clone());
            out.push(case(
                "keys.derive_pp_from_seed", 1, None,
                move || { black_box(derive_at(black_box(&seed), &p).unwrap()); },
                move || hex::encode(*derive_at(&seed, &p2).unwrap()),
            ));
        }
        {
            let (e, e2) = (eoa.clone(), eoa.clone());
            out.push(case(
                "keys.derive_eoa10_from_seed", 10, None,
                move || { for p in &e { black_box(derive_at(black_box(&seed), p).unwrap()); } },
                move || e2.iter().map(|p| hex::encode(*derive_at(&seed, p).unwrap())).collect::<Vec<_>>().join(","),
            ));
        }
        {
            let (m1, p1, m2, p2) = (m.clone(), pp_path.clone(), m.clone(), pp_path.clone());
            out.push(case(
                "keys.mnemonic_to_key", 1, None,
                move || { black_box(Keystore::from_mnemonic(black_box(&m1)).unwrap().derive_at(&p1).unwrap()); },
                move || hex::encode(*Keystore::from_mnemonic(&m2).unwrap().derive_at(&p2).unwrap()),
            ));
        }
        {
            let ks = std::rc::Rc::new(Keystore::from_mnemonic(&m).unwrap());
            let ks2 = ks.clone();
            let (p1, p2) = (pp_path.clone(), pp_path.clone());
            out.push(case(
                "keys.keystore_derive", 1, None,
                move || { black_box(ks.derive_at(black_box(&p1)).unwrap()); },
                move || hex::encode(*ks2.derive_at(&p2).unwrap()),
            ));
        }
        {
            let sk = h32(s(&k["signKey"]));
            let ds: Vec<[u8; 32]> = k["digests"].as_array().unwrap().iter().map(|d| h32(s(d))).collect();
            let ds2 = ds.clone();
            out.push(case(
                "keys.sign_digest", ds.len(), None,
                move || { for d in &ds { black_box(sign_digest(&sk, black_box(d)).unwrap()); } },
                move || ds2[..8].iter().map(|d| hex::encode(sign_digest(&sk, d).unwrap().to_bytes())).collect::<Vec<_>>().join(","),
            ));
        }
    }

    #[cfg(feature = "pp")]
    {
        use ark_ff::PrimeField;
        use lowki_pp_core::*;
        use std::str::FromStr;
        let p = &fx["pp"];
        let fr = |v: &Value| Fr::from_str(s(v)).unwrap();
        let dec = |x: &Fr| x.into_bigint().to_string();
        let pairs: Vec<(Fr, Fr)> = p["poseidonPairs"].as_array().unwrap().iter().map(|x| (fr(&x[0]), fr(&x[1]))).collect();
        {
            let pairs2 = pairs.clone();
            out.push(case(
                "pp.poseidon_t3", pairs.len(), None,
                move || { for (a, b) in &pairs { black_box(poseidon2(black_box(*a), *b)); } },
                move || pairs2[..4].iter().map(|(a, b)| dec(&poseidon2(*a, *b))).collect::<Vec<_>>().join(","),
            ));
        }
        let leaves: Vec<Fr> = p["leaves"].as_array().unwrap().iter().map(fr).collect();
        for (n, id, iters) in [(1000, "pp.limt_build_1k", None), (10_000, "pp.limt_build_10k", None), (100_000, "pp.limt_build_100k", Some(10))] {
            let ls: Vec<Fr> = leaves[..n].to_vec();
            let ls2 = ls.clone();
            out.push(case(
                id, 1, iters,
                move || { black_box(LeanImt::from_leaves(black_box(&ls))); },
                move || dec(&LeanImt::from_leaves(&ls2).root().unwrap()),
            ));
        }
        {
            let tree = std::rc::Rc::new(LeanImt::from_leaves(&leaves));
            let t2 = tree.clone();
            let idx: Vec<usize> = p["proofIndices"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap() as usize).collect();
            let i0 = idx[0];
            out.push(case(
                "pp.limt_proof_100k", idx.len(), None,
                move || { for &i in &idx { black_box(tree.generate_proof(black_box(i)).unwrap()); } },
                move || {
                    let pr = t2.generate_proof(i0).unwrap();
                    let mut v = vec![dec(&pr.root), dec(&pr.leaf), pr.index.to_string()];
                    v.extend(pr.siblings.iter().map(dec));
                    v.join(",")
                },
            ));
        }
        {
            let key = h32(s(&p["accountKey"]));
            let chain = p["chainId"].as_u64().unwrap();
            let ep: [u8; 20] = hex::decode(&s(&p["entrypoint"])[2..]).unwrap().try_into().unwrap();
            let idx: Vec<u64> = p["depositIndices"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap()).collect();
            let sec = move |x: &Secret| [&x.nullifier, &x.salt, &x.precommitment, &x.nullifier_hash].map(|f| dec(f)).join(",");
            out.push(case(
                "pp.note_secrets", idx.len(), None,
                move || {
                    let sm = SignatureSecretManager::new(black_box(&key)).unwrap();
                    for &i in &idx { black_box(sm.deposit_secrets(chain, &ep, i).unwrap()); }
                },
                move || sec(&SignatureSecretManager::new(&key).unwrap().deposit_secrets(chain, &ep, 3).unwrap()),
            ));
        }
        {
            let cs: Vec<(Fr, Fr, Fr)> = p["commitments"].as_array().unwrap().iter()
                .map(|c| (fr(&c["value"]), fr(&c["label"]), fr(&c["precommitment"]))).collect();
            let cs2 = cs.clone();
            out.push(case(
                "pp.commitment", cs.len(), None,
                move || { for (v, l, pc) in &cs { black_box(commitment(black_box(*v), *l, *pc)); } },
                move || cs2[..4].iter().map(|(v, l, pc)| dec(&commitment(*v, *l, *pc))).collect::<Vec<_>>().join(","),
            ));
        }
    }

    #[cfg(feature = "stealth")]
    {
        use lowki_stealth::*;
        let st = &fx["stealth"];
        let uri = s(&st["metaUri"]).to_string();
        let ephs: Vec<[u8; 32]> = st["ephKeys"].as_array().unwrap().iter().map(|k| h32(s(k))).collect();
        {
            let (u1, u2, e2) = (uri.clone(), uri.clone(), ephs.clone());
            out.push(case(
                "stealth.generate", ephs.len(), None,
                move || {
                    for e in &ephs {
                        let meta = StealthMetaAddress::parse_uri(black_box(&u1)).unwrap();
                        let sk = SecretKey::from_bytes(e).unwrap();
                        black_box(generate_stealth_address(&meta, &sk).unwrap());
                    }
                },
                move || {
                    let meta = StealthMetaAddress::parse_uri(&u2).unwrap();
                    e2[..4].iter().map(|e| {
                        let g = generate_stealth_address(&meta, &SecretKey::from_bytes(e).unwrap()).unwrap();
                        format!("0x{}:{}:{}", hex::encode(g.stealth_address), hex::encode(g.ephemeral_pubkey), g.view_tag)
                    }).collect::<Vec<_>>().join(",")
                },
            ));
        }
        let view = SecretKey::from_bytes(&h32(s(&st["viewingKey"]))).unwrap();
        let spend_pub: [u8; 33] = hex::decode(s(&st["spendingPub"])).unwrap().try_into().unwrap();
        // Announcements pre-decoded from hex (the SDK decodes hex inside each call; ~1 µs, vs ~100 µs of ECDH).
        let anns: std::rc::Rc<Vec<([u8; 33], u8, [u8; 20])>> = std::rc::Rc::new(
            st["announcements"].as_array().unwrap().iter().map(|a| {
                (
                    hex::decode(s(&a["eph"])).unwrap().try_into().unwrap(),
                    a["tag"].as_u64().unwrap() as u8,
                    hex::decode(&s(&a["addr"])[2..]).unwrap().try_into().unwrap(),
                )
            }).collect(),
        );
        let view = std::rc::Rc::new(view);
        let scan = move |anns: &[([u8; 33], u8, [u8; 20])], view: &SecretKey| -> Vec<usize> {
            let mut hits = Vec::new();
            for (i, (eph, tag, addr)) in anns.iter().enumerate() {
                if check_announcement(view, &spend_pub, eph, *tag) == Some(*addr) {
                    hits.push(i);
                }
            }
            hits
        };
        for (n, id, iters) in [(1000usize, "stealth.scan_1k", None), (10_000, "stealth.scan_10k", Some(10))] {
            let (a1, a2, v1, v2) = (anns.clone(), anns.clone(), view.clone(), view.clone());
            out.push(case(
                id, 1, iters,
                move || { black_box(scan(black_box(&a1[..n]), &v1)); },
                move || scan(&a2[..n], &v2).iter().map(|i| i.to_string()).collect::<Vec<_>>().join(","),
            ));
        }
    }

    #[cfg(feature = "aa")]
    {
        let a = fx["aa"].clone();
        let build = move || -> String { crate::aa::aa_build(&a) };
        let mut b2 = build.clone();
        out.push(case("aa.build_signed_userop", 1, None, move || { black_box(build()); }, move || b2()));
    }

    out
}

