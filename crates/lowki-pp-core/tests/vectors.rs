//! Differential tests against vectors generated from the pinned Kohaku sources
//! (`vectors/lowki-pp-core/gen.ts`).
#![cfg(not(target_arch = "wasm32"))]

use std::sync::OnceLock;

use lowki_pp_core::secrets::{
    canonical_rs, ephemeral_signer_from_signature, legacy, note_typed_data_hash,
    secrets_from_signature,
};
use lowki_pp_core::*;
use serde_json::Value;

const VECTORS: &str = include_str!("../../../vectors/lowki-pp-core/vectors.json");

fn v() -> &'static Value {
    static V: OnceLock<Value> = OnceLock::new();
    V.get_or_init(|| serde_json::from_str(VECTORS).expect("vectors.json"))
}

fn bytes(s: &Value) -> Vec<u8> {
    let s = s.as_str().expect("hex string");
    let s = s.strip_prefix("0x").unwrap_or(s);
    let s = if s.len() % 2 == 1 {
        format!("0{s}")
    } else {
        s.to_string()
    };
    hex::decode(s).expect("hex")
}
fn b32(s: &Value) -> [u8; 32] {
    let b = bytes(s);
    let mut out = [0u8; 32];
    out[32 - b.len()..].copy_from_slice(&b);
    out
}
fn b20(s: &Value) -> [u8; 20] {
    bytes(s).try_into().expect("20 bytes")
}
/// Canonical field element (panics if the vector value is >= p).
fn fr(s: &Value) -> Fr {
    fr_from_be_bytes(&b32(s)).expect("canonical")
}
fn hx(x: &Fr) -> String {
    format!("0x{}", hex::encode(fr_to_be_bytes(x)))
}
fn u64v(s: &Value) -> u64 {
    match s {
        Value::String(s) => s.parse().unwrap(),
        _ => s.as_u64().unwrap(),
    }
}
fn arr(key: &str) -> &'static Vec<Value> {
    v()[key].as_array().unwrap_or_else(|| panic!("{key}"))
}

#[test]
fn meta_pins_kohaku_commit() {
    assert_eq!(
        v()["meta"]["kohaku_commit"],
        "8c83d64050040654082d70901a46a339bfa103b0"
    );
    assert_eq!(b32(&v()["meta"]["snark_scalar_field"]), SNARK_SCALAR_FIELD);
}

#[test]
fn poseidon_vectors() {
    let cases = arr("poseidon");
    assert!(cases.len() >= 300);
    let mut reduced = 0;
    for c in cases {
        let raw: Vec<[u8; 32]> = c["inputs"].as_array().unwrap().iter().map(b32).collect();
        if raw.iter().any(|r| fr_from_be_bytes(r).is_err()) {
            reduced += 1;
        }
        // References reduce inputs >= p mod p.
        let inputs: Vec<Fr> = raw.iter().map(fr_from_be_bytes_mod_order).collect();
        let out = poseidon(&inputs).unwrap();
        assert_eq!(hx(&out), c["output"].as_str().unwrap(), "{c}");
        let want = fr(&c["output"]);
        match inputs.len() {
            1 => assert_eq!(poseidon1(inputs[0]), want),
            2 => assert_eq!(poseidon2(inputs[0], inputs[1]), want),
            3 => assert_eq!(poseidon3(inputs[0], inputs[1], inputs[2]), want),
            _ => unreachable!(),
        }
    }
    assert!(reduced >= 9, "edge cases >= p present");
    assert_eq!(poseidon(&[]), Err(Error::InvalidArity(0)));
    assert_eq!(poseidon(&[Fr::from(1u64); 4]), Err(Error::InvalidArity(4)));
}

fn derived_leaf(tree: u32, i: u32) -> Fr {
    use sha3::{Digest, Keccak256};
    let mut h = Keccak256::new();
    h.update(b"lowki-limt");
    h.update(tree.to_be_bytes());
    h.update(i.to_be_bytes());
    fr_from_be_bytes_mod_order(&h.finalize().into())
}

fn proof_of(c: &Value) -> MerkleProof {
    MerkleProof {
        root: fr(&c["root"]),
        leaf: fr(&c["leaf"]),
        index: c["path_index"].as_u64().unwrap(),
        siblings: c["siblings"].as_array().unwrap().iter().map(fr).collect(),
    }
}

#[test]
fn limt_vectors() {
    let cases = arr("limt");
    assert!(cases.len() >= 40);
    let (mut n_proofs, mut n_padded, mut n_verify) = (0, 0, 0);
    for c in cases {
        let size = c["size"].as_u64().unwrap() as usize;
        let leaves: Vec<Fr> = if c["leaves"].is_array() {
            c["leaves"].as_array().unwrap().iter().map(fr).collect()
        } else {
            let tree = c["derived"]["tree"].as_u64().unwrap() as u32;
            let distinct = c["derived"]["distinct"].as_u64().unwrap() as u32;
            (0..size as u32)
                .map(|i| derived_leaf(tree, i % distinct))
                .collect()
        };
        assert_eq!(leaves.len(), size);

        // Replay the exact build sequence (insert / insert_many chunks) and check
        // the root after every recorded op.
        let mut t = LeanImt::new();
        let mut pos = 0;
        for op in c["ops"].as_array().unwrap() {
            let n = op["count"].as_u64().unwrap() as usize;
            match op["kind"].as_str().unwrap() {
                "insert" => leaves[pos..pos + n].iter().for_each(|l| t.insert(*l)),
                _ => t.insert_many(&leaves[pos..pos + n]).unwrap(),
            }
            pos += n;
            assert_eq!(
                t.root().map(|r| hx(&r)).as_deref(),
                op["root"].as_str(),
                "tree {}",
                c["tree_id"]
            );
        }
        assert_eq!(pos, size);
        assert_eq!(hx(&t.root().unwrap()), c["root"].as_str().unwrap());
        assert_eq!(t.depth() as u64, c["depth"].as_u64().unwrap());
        assert_eq!(t.size(), size);
        // Any build order gives the same tree.
        let t2 = LeanImt::from_leaves(&leaves);
        assert_eq!(t2.root(), t.root());

        for p in c["proofs"].as_array().unwrap() {
            let got = t
                .generate_proof(p["index"].as_u64().unwrap() as usize)
                .unwrap();
            assert_eq!(got, proof_of(p));
            assert!(verify_proof(&got));
            n_proofs += 1;
        }
        for p in c["proofs_from_tree"].as_array().unwrap() {
            let got = proof_from_tree(&t, &fr(&p["leaf"])).unwrap();
            assert_eq!(got.siblings.len(), PADDED_PROOF_DEPTH);
            assert_eq!(got, proof_of(p));
            n_padded += 1;
        }
        for p in c["index_of"].as_array().unwrap() {
            let want = p["index"].as_i64().unwrap();
            let got = t.index_of(&fr(&p["leaf"])).map(|i| i as i64).unwrap_or(-1);
            assert_eq!(got, want);
        }
        for p in c["verify_cases"].as_array().unwrap() {
            assert_eq!(
                verify_proof(&proof_of(p)),
                p["valid"].as_bool().unwrap(),
                "{p}"
            );
            n_verify += 1;
        }
    }
    assert!(
        n_proofs > 200 && n_padded > 80 && n_verify > 150,
        "{n_proofs} {n_padded} {n_verify}"
    );
}

fn account_key(index: u64) -> [u8; 32] {
    let a = arr("accounts")
        .iter()
        .find(|a| a["account_index"].as_u64() == Some(index))
        .unwrap();
    b32(&a["private_key"])
}

#[test]
fn signature_secret_manager_vectors() {
    let cases = arr("signature_secrets");
    assert!(cases.len() >= 400);
    let managers: Vec<SignatureSecretManager> = (0..3)
        .map(|i| SignatureSecretManager::new(&account_key(i)).unwrap())
        .collect();
    for c in cases {
        let m = &managers[c["account_index"].as_u64().unwrap() as usize];
        let msg = NoteMessage {
            chain_id: u64v(&c["chain_id"]),
            entrypoint: b20(&c["entrypoint"]),
            deposit_index: u64v(&c["deposit_index"]),
            secret_index: u64v(&c["secret_index"]),
        };
        assert_eq!(
            note_typed_data_hash(&msg),
            b32(&c["typed_data_hash"]),
            "{c}"
        );
        let sig = m.sign_note(&msg).unwrap();
        assert_eq!(
            sig.as_slice(),
            bytes(&c["signature"]).as_slice(),
            "signature {c}"
        );

        let s = if msg.secret_index == 0 {
            m.deposit_secrets(msg.chain_id, &msg.entrypoint, msg.deposit_index)
                .unwrap()
        } else {
            m.withdrawal_secrets(
                msg.chain_id,
                &msg.entrypoint,
                msg.deposit_index,
                msg.secret_index,
            )
            .unwrap()
        };
        assert_eq!(hx(&s.nullifier), c["nullifier"].as_str().unwrap());
        assert_eq!(hx(&s.salt), c["salt"].as_str().unwrap());
        assert_eq!(hx(&s.precommitment), c["precommitment"].as_str().unwrap());
        assert_eq!(hx(&s.nullifier_hash), c["nullifier_hash"].as_str().unwrap());
        let eph = m
            .derive_ephemeral_signer(
                msg.chain_id,
                &msg.entrypoint,
                msg.deposit_index,
                msg.secret_index,
            )
            .unwrap();
        assert_eq!(eph.as_slice(), b32(&c["ephemeral_signer"]).as_slice());
    }
}

#[test]
fn signature_framing_vectors() {
    let cases = arr("signature_framing");
    assert!(cases.len() >= 60);
    let (mut ok, mut rejected) = (0, 0);
    for c in cases {
        let sig = bytes(&c["signature"]);
        if c["accepted"].as_bool().unwrap() {
            let s = secrets_from_signature(&sig).unwrap();
            assert_eq!(hx(&s.nullifier), c["nullifier"].as_str().unwrap());
            assert_eq!(hx(&s.salt), c["salt"].as_str().unwrap());
            assert_eq!(hx(&s.precommitment), c["precommitment"].as_str().unwrap());
            assert_eq!(hx(&s.nullifier_hash), c["nullifier_hash"].as_str().unwrap());
            let eph = ephemeral_signer_from_signature(&sig).unwrap();
            assert_eq!(eph.as_slice(), b32(&c["ephemeral_signer"]).as_slice());
            ok += 1;
        } else {
            assert_eq!(
                canonical_rs(&sig).err(),
                Some(Error::InvalidSignature),
                "{c}"
            );
            rejected += 1;
        }
    }
    assert!(ok > 0 && rejected > 0);
}

#[test]
fn legacy_secret_manager_vectors() {
    let cases = arr("legacy_secrets");
    assert!(cases.len() >= 100);
    for c in cases {
        let chain = u64v(&c["chain_id"]);
        let ep = b20(&c["entrypoint"]);
        let s = legacy::legacy_secrets(
            chain,
            &ep,
            &b32(&c["nullifier_secret"]),
            &b32(&c["salt_secret"]),
        );
        assert_eq!(hx(&s.nullifier), c["nullifier"].as_str().unwrap());
        assert_eq!(hx(&s.salt), c["salt"].as_str().unwrap());
        assert_eq!(hx(&s.precommitment), c["precommitment"].as_str().unwrap());
        assert_eq!(hx(&s.nullifier_hash), c["nullifier_hash"].as_str().unwrap());
        let eph = legacy::legacy_ephemeral_signer(chain, &ep, &b32(&c["signer_secret"]));
        assert_eq!(eph.as_slice(), b32(&c["ephemeral_signer"]).as_slice());
    }
}

#[test]
fn commitment_vectors() {
    let cases = arr("commitment");
    assert!(cases.len() >= 80);
    for c in cases {
        let got = commitment(fr(&c["value"]), fr(&c["label"]), fr(&c["precommitment"]));
        assert_eq!(hx(&got), c["commitment"].as_str().unwrap());
    }
}

#[test]
fn context_vectors() {
    let cases = arr("context");
    assert!(cases.len() >= 80);
    for c in cases {
        let got = calculate_context(
            &b20(&c["processooor"]),
            &bytes(&c["data"]),
            &b32(&c["scope"]),
        );
        assert_eq!(hx(&got), c["context_padded"].as_str().unwrap(), "{c}");
        // Kohaku returns unpadded hex (numberToHex).
        assert_eq!(fr_to_be_bytes(&got), b32(&c["context"]));
    }
}

#[test]
fn scope_and_label_vectors() {
    let cases = arr("label");
    assert!(cases.len() >= 40);
    for c in cases {
        let scope = compute_scope(&b20(&c["pool"]), u64v(&c["chain_id"]), &b20(&c["asset"]));
        assert_eq!(hx(&scope), c["scope"].as_str().unwrap());
        assert_eq!(
            hx(&compute_label(&scope, &b32(&c["nonce"]))),
            c["label"].as_str().unwrap()
        );
    }
}

/// Real mainnet deposits (0xbow ETH pool): scope, label and commitment recomputed
/// from chain data.
#[test]
fn onchain_deposits() {
    let o = &v()["onchain"];
    let dec = |s: &Value| -> Fr { s.as_str().unwrap().parse::<Fr>().unwrap() };
    let scope = compute_scope(&b20(&o["pool"]), u64v(&o["chainId"]), &b20(&o["asset"]));
    assert_eq!(scope, dec(&o["scope"]));
    let deps = o["deposits"].as_array().unwrap();
    assert!(deps.len() >= 4);
    for d in deps {
        let mut nonce = [0u8; 32];
        nonce[24..].copy_from_slice(&u64v(&d["nonce"]).to_be_bytes());
        let label = compute_label(&scope, &nonce);
        assert_eq!(label, dec(&d["label"]));
        assert_eq!(
            commitment(dec(&d["value"]), label, dec(&d["precommitment"])),
            dec(&d["commitment"])
        );
    }
}

#[test]
fn secret_debug_is_redacted() {
    let m = SignatureSecretManager::new(&account_key(0)).unwrap();
    let s = m.deposit_secrets(1, &[1u8; 20], 0).unwrap();
    let dbg = format!("{s:?} {m:?}");
    assert!(dbg.contains("<redacted>"));
    assert!(!dbg.contains(&format!("{}", s.nullifier)));
    assert!(!dbg.contains(&hex::encode(account_key(0))));
    assert_eq!(
        SignatureSecretManager::new(&[0u8; 32]).err(),
        Some(Error::InvalidPrivateKey)
    );
}
