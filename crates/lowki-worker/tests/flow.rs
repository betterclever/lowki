//! Native end-to-end: create → lock → unlock → recover (S+R, P+S) → same app address →
//! sign → prove withdraw/commitment (real artifacts) → verify with lowki-groth16 →
//! paymaster userOp. Plus auto-lock, scoping, rejection and redaction checks.
#![cfg(not(target_arch = "wasm32"))]

mod common;

use common::*;
use lowki_worker::protocol::Request;
use lowki_worker::{Core, handle_blocking};
use rand_core::{CryptoRng, RngCore};
use serde_json::{Value, json};

struct TestRng(u64);
impl RngCore for TestRng {
    fn next_u32(&mut self) -> u32 {
        self.next_u64() as u32
    }
    fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^ (z >> 31)
    }
    fn fill_bytes(&mut self, d: &mut [u8]) {
        for c in d.chunks_mut(8) {
            let v = self.next_u64().to_le_bytes();
            c.copy_from_slice(&v[..c.len()]);
        }
    }
    fn try_fill_bytes(&mut self, d: &mut [u8]) -> Result<(), rand_core::Error> {
        self.fill_bytes(d);
        Ok(())
    }
}
impl CryptoRng for TestRng {}

const MIN: f64 = 60_000.0;

fn core() -> Core {
    Core::new(15.0 * MIN, Box::new(TestRng(1)))
}

fn call(c: &mut Core, v: Value, now: f64) -> Value {
    let req: Request = serde_json::from_value(v).expect("request parses");
    serde_json::to_value(handle_blocking(c, req, now)).unwrap()
}

fn ok(r: &Value, ty: &str) {
    assert_eq!(r["type"], ty, "{r}");
}

fn artifacts() -> std::path::PathBuf {
    std::env::var("LOWKI_PP_ARTIFACTS")
        .map(Into::into)
        .unwrap_or_else(|_| concat!(env!("CARGO_MANIFEST_DIR"), "/../../../vendor/npm/circuits/package/artifacts").into())
}

#[test]
fn lifecycle_recovery_and_signing() {
    let mut c = core();
    let prf = [7u8; 32];
    let prf2 = [9u8; 32];
    let created = call(&mut c, json!({"type": "createWallet", "prfOutput": bytes_json(&prf)}), 0.0);
    ok(&created, "walletCreated");
    assert_eq!(bytes_of(&created["wrappedBlob"]).len(), 61);
    assert_eq!(bytes_of(&created["wrappedPasskeyShare"]).len(), 62);
    let s = bytes_of(&created["serverShare"]);
    let r = bytes_of(&created["recoveryShare"]);
    assert_eq!((s.len(), r.len()), (33, 33));
    let px = created["shareXs"]["passkey"].as_u64().unwrap();
    let acct = call(&mut c, json!({"type": "appAccount", "origin": ORIGIN}), 1.0);
    ok(&acct, "appAccount");
    let addr = acct["address"].clone();
    let uid = acct["pairwiseUserId"].clone();
    let other = call(&mut c, json!({"type": "appAccount", "origin": "https://other.example"}), 1.0);
    assert_ne!(other["address"], addr);
    assert_ne!(other["pairwiseUserId"], uid);
    // create while unlocked is refused
    ok(&call(&mut c, json!({"type": "createWallet", "prfOutput": bytes_json(&prf)}), 2.0), "error");

    // lock -> everything needing keys fails
    ok(&call(&mut c, json!({"type": "lock"}), 3.0), "locked");
    let e = call(&mut c, json!({"type": "appAccount", "origin": ORIGIN}), 3.0);
    assert_eq!(e["code"], "locked");

    // unlock with the passkey (and a wrong PRF fails)
    let bad = call(&mut c, json!({"type": "unlock", "prfOutput": bytes_json(&prf2), "wrappedBlob": created["wrappedBlob"]}), 4.0);
    assert_eq!(bad["code"], "unwrapFailed");
    ok(&call(&mut c, json!({"type": "unlock", "prfOutput": bytes_json(&prf), "wrappedBlob": created["wrappedBlob"]}), 4.0), "unlocked");
    assert_eq!(call(&mut c, json!({"type": "appAccount", "origin": ORIGIN}), 5.0)["address"], addr);
    call(&mut c, json!({"type": "lock"}), 5.0);

    // recover with S + R (new device, no passkey)
    ok(&call(&mut c, json!({"type": "recover", "serverShare": bytes_json(&s), "recoveryShare": bytes_json(&r)}), 6.0), "unlocked");
    let rec = call(&mut c, json!({"type": "appAccount", "origin": ORIGIN}), 6.0);
    assert_eq!(rec["address"], addr);
    assert_eq!(rec["pairwiseUserId"], uid);
    // new passkey: rewrap the seed and re-derive P
    let rw = call(&mut c, json!({"type": "rewrap", "prfOutput": bytes_json(&prf2), "passkeyShareX": px,
        "serverShare": bytes_json(&s), "recoveryShare": bytes_json(&r)}), 6.0);
    ok(&rw, "rewrapped");
    call(&mut c, json!({"type": "lock"}), 6.0);
    ok(&call(&mut c, json!({"type": "unlock", "prfOutput": bytes_json(&prf2), "wrappedBlob": rw["wrappedBlob"]}), 7.0), "unlocked");
    assert_eq!(call(&mut c, json!({"type": "appAccount", "origin": ORIGIN}), 7.0)["address"], addr);
    call(&mut c, json!({"type": "lock"}), 7.0);
    // P (old passkey) + S, and new-passkey P + R
    ok(&call(&mut c, json!({"type": "recoverWithPasskeyShare", "prfOutput": bytes_json(&prf),
        "wrappedPasskeyShare": created["wrappedPasskeyShare"], "otherShare": bytes_json(&s)}), 8.0), "unlocked");
    assert_eq!(call(&mut c, json!({"type": "appAccount", "origin": ORIGIN}), 8.0)["address"], addr);
    call(&mut c, json!({"type": "lock"}), 8.0);
    ok(&call(&mut c, json!({"type": "recoverWithPasskeyShare", "prfOutput": bytes_json(&prf2),
        "wrappedPasskeyShare": rw["wrappedPasskeyShare"], "otherShare": bytes_json(&r)}), 8.0), "unlocked");
    assert_eq!(call(&mut c, json!({"type": "appAccount", "origin": ORIGIN}), 8.0)["address"], addr);
    call(&mut c, json!({"type": "lock"}), 8.0);
    // one share alone / duplicated share is rejected
    let one = call(&mut c, json!({"type": "recover", "serverShare": bytes_json(&s), "recoveryShare": bytes_json(&s)}), 9.0);
    assert_eq!(one["code"], "invalidShares");

    // auto-lock: unlocked at t=10 min, still unlocked at 24 min, locked at 25 min
    ok(&call(&mut c, json!({"type": "recover", "serverShare": bytes_json(&s), "recoveryShare": bytes_json(&r)}), 10.0 * MIN), "unlocked");
    ok(&call(&mut c, json!({"type": "appAccount", "origin": ORIGIN}), 24.9 * MIN), "appAccount");
    let late = call(&mut c, json!({"type": "appAccount", "origin": ORIGIN}), 25.0 * MIN);
    assert_eq!(late["code"], "locked");
    assert!(!c.is_unlocked());

    // signing (checked byte-for-byte against viem in tests/vectors.rs; here: recovers)
    call(&mut c, json!({"type": "recover", "serverShare": bytes_json(&s), "recoveryShare": bytes_json(&r)}), 30.0 * MIN);
    let sig = call(&mut c, json!({"type": "signMessage", "origin": ORIGIN, "message": "hi"}), 30.0 * MIN);
    ok(&sig, "signature");
    assert_eq!(sig["address"], addr);
    let sig_bytes = hex::decode(sig["signature"].as_str().unwrap().trim_start_matches("0x")).unwrap();
    let hash: [u8; 32] = hex::decode(sig["hash"].as_str().unwrap().trim_start_matches("0x")).unwrap().try_into().unwrap();
    let rs = k256::ecdsa::Signature::from_slice(&sig_bytes[..64]).unwrap();
    let rid = k256::ecdsa::RecoveryId::from_byte(sig_bytes[64] - 27).unwrap();
    let vk = k256::ecdsa::VerifyingKey::recover_from_prehash(&hash, &rs, rid).unwrap();
    let pt = vk.to_encoded_point(false);
    let a = alloy_primitives::keccak256(&pt.as_bytes()[1..]);
    assert_eq!(alloy_primitives::Address::from_slice(&a[12..]).to_checksum(None), addr.as_str().unwrap());

    // origin validation
    for bad in ["https://app.example/", "https://App.example", "app.example", "https://app.example:443", "ftp://x"] {
        let e = call(&mut c, json!({"type": "appAccount", "origin": bad}), 30.0 * MIN);
        assert_eq!(e["code"], "invalidRequest", "{bad}");
    }
}

#[test]
fn kohaku_scope_and_note_envelope() {
    let mut c = core();
    let m = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon art";
    ok(&call(&mut c, json!({"type": "importMnemonic", "mnemonic": m, "prfOutput": bytes_json(&[1; 32])}), 0.0), "walletCreated");
    let k = call(&mut c, json!({"type": "kohakuDeriveAt", "path": "m/28784'/1'/0'"}), 0.0);
    let seed = lowki_keys::mnemonic_to_seed(m).unwrap();
    let want = lowki_keys::derive_at(&seed, "m/28784'/1'/0'").unwrap();
    assert_eq!(k["key"], format!("0x{}", hex::encode(want.as_slice())));
    for p in ["m/28784'/1'", "m/28784'/2'/0'", "m/44'/60'/0'/0/0", "m/5564'/0'/0'"] {
        assert_eq!(call(&mut c, json!({"type": "kohakuDeriveAt", "path": p}), 0.0)["code"], "pathNotAllowed", "{p}");
    }
    assert_eq!(call(&mut c, json!({"type": "kohakuDeriveAt", "path": "bogus"}), 0.0)["code"], "invalidRequest");
    ok(&call(&mut c, json!({"type": "kohakuAllowPath", "path": "m/44'/60'/0'/0/0"}), 0.0), "ok");
    ok(&call(&mut c, json!({"type": "kohakuDeriveAt", "path": "m/44'/60'/0'/0/0"}), 0.0), "kohakuKey");
    // note envelope signature == lowki-pp-core SignatureSecretManager (itself == Kohaku/viem)
    let sig = call(&mut c, json!({"type": "ppSignNoteEnvelope", "accountIndex": 0, "chainId": CHAIN_ID,
        "entrypoint": ENTRYPOINT, "depositIndex": 3, "secretIndex": 1}), 0.0);
    let mgr = lowki_pp_core::SignatureSecretManager::new(&want).unwrap();
    let ep: [u8; 20] = hex::decode(&ENTRYPOINT[2..]).unwrap().try_into().unwrap();
    let w = mgr.sign_note(&lowki_pp_core::NoteMessage { chain_id: 11155111, entrypoint: ep, deposit_index: 3, secret_index: 1 }).unwrap();
    assert_eq!(sig["signature"], format!("0x{}", hex::encode(w.as_slice())));
}

#[test]
fn no_secrets_in_debug_or_errors() {
    let prf = [0xab_u8; 32];
    let req: Request = serde_json::from_value(json!({"type": "unlock", "prfOutput": bytes_json(&prf), "wrappedBlob": bytes_json(&[0xcd; 61])})).unwrap();
    let dbg = format!("{req:?}");
    assert!(!dbg.contains("171") && !dbg.contains("ab") && !dbg.contains("205"), "{dbg}");
    let m = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon art";
    let req: Request = serde_json::from_value(json!({"type": "importMnemonic", "mnemonic": m, "prfOutput": bytes_json(&prf)})).unwrap();
    assert!(!format!("{req:?}").contains("abandon"));
    let mut c = core();
    let r = call(&mut c, json!({"type": "unlock", "prfOutput": bytes_json(&prf), "wrappedBlob": bytes_json(&[1; 61])}), 0.0);
    let msg = r["message"].as_str().unwrap();
    assert!(!msg.contains("171") && !msg.contains("ab,"), "{msg}");
    let r = call(&mut c, json!({"type": "importMnemonic", "mnemonic": "abandon zoo", "prfOutput": bytes_json(&prf)}), 0.0);
    assert!(!r["message"].as_str().unwrap().contains("zoo"));
    // Debug of a response with secrets
    let created = handle_blocking(&mut c, serde_json::from_value(json!({"type": "createWallet", "prfOutput": bytes_json(&prf)})).unwrap(), 0.0);
    let d = format!("{created:?}");
    assert!(d.contains("SecretBytes(len=33)") && d.contains("SecretBytes(len=61)"), "{d}");
    let k = handle_blocking(&mut c, serde_json::from_value(json!({"type": "kohakuDeriveAt", "path": "m/28784'/1'/0'"})).unwrap(), 0.0);
    assert!(format!("{k:?}").contains("SecretHex(..)"));
}

#[test]
fn prove_withdraw_and_commitment_native() {
    let dir = artifacts();
    if !dir.join("withdraw.zkey").exists() {
        eprintln!("SKIP: no artifacts at {}", dir.display());
        return;
    }
    let rd = |f: &str| std::fs::read(dir.join(f)).unwrap();
    let mut c = core();
    ok(&call(&mut c, json!({"type": "createWallet", "prfOutput": bytes_json(&[3; 32])}), 0.0), "walletCreated");
    let loaded = call(&mut c, json!({"type": "ppLoadCircuits",
        "withdrawWasm": bytes_json(&rd("withdraw.wasm")), "withdrawZkey": bytes_json(&rd("withdraw.zkey")),
        "commitmentWasm": bytes_json(&rd("commitment.wasm")), "commitmentZkey": bytes_json(&rd("commitment.zkey")),
        "useGpu": false}), 0.0);
    ok(&loaded, "circuitsLoaded");

    let lbl = label();
    let n0 = call(&mut c, json!({"type": "ppNote", "note": note(0, 0), "value": EXISTING_VALUE, "label": lbl}), 0.0);
    let n1 = call(&mut c, json!({"type": "ppNote", "note": note(0, 1)}), 0.0);
    let shield = call(&mut c, json!({"type": "ppShield", "note": note(0, 0), "value": EXISTING_VALUE}), 0.0);
    assert_eq!(shield["precommitment"], n0["precommitment"]);
    assert_eq!(shield["tx"]["to"], ENTRYPOINT);

    let adapter = "0x521F578Ac6E894939364Dc27c7e6978Ecf7b7926";
    let req = withdraw_request(n0["commitment"].as_str().unwrap(), 40, adapter);
    let proof = call(&mut c, req, 0.0);
    ok(&proof, "proof");
    assert_eq!(proof["backend"], "cpu");
    let sig: Vec<String> = serde_json::from_value(proof["publicSignals"].clone()).unwrap();
    // [newCommitmentHash, existingNullifierHash, withdrawnValue, stateRoot, stateTreeDepth, ASPRoot, ASPTreeDepth, context]
    assert_eq!(sig[1], n0["nullifierHash"].as_str().unwrap());
    assert_eq!(sig[2], WITHDRAWN_VALUE);
    assert_eq!(sig[7], context_for(adapter));
    let remaining = (1_000_000_000_000_000_000u128 - 250_000_000_000_000_000u128).to_string();
    let n1c = call(&mut c, json!({"type": "ppNote", "note": note(0, 1), "value": remaining, "label": lbl}), 0.0);
    assert_eq!(sig[0], n1c["commitment"].as_str().unwrap());
    assert_ne!(n1["precommitment"], n0["precommitment"]);
    let vk = lowki_groth16::parse_vkey_json(&String::from_utf8(rd("withdraw.vkey")).unwrap()).unwrap();
    let p: lowki_groth16::SnarkjsProof = serde_json::from_value(proof["proof"].clone()).unwrap();
    assert!(lowki_groth16::verify(&vk, &p, &sig).unwrap());

    // over-withdraw rejected before proving
    let mut bad = withdraw_request(n0["commitment"].as_str().unwrap(), 4, adapter);
    bad["withdrawnValue"] = json!("2000000000000000000");
    assert_eq!(call(&mut c, bad, 0.0)["code"], "invalidRequest");

    // commitment (ragequit) proof
    let cp = call(&mut c, json!({"type": "ppProveCommitment", "note": note(0, 0), "value": EXISTING_VALUE, "label": lbl}), 0.0);
    ok(&cp, "proof");
    let csig: Vec<String> = serde_json::from_value(cp["publicSignals"].clone()).unwrap();
    assert_eq!(csig[0], n0["commitment"].as_str().unwrap());
    let cvk = lowki_groth16::parse_vkey_json(&String::from_utf8(rd("commitment.vkey")).unwrap()).unwrap();
    let cpp: lowki_groth16::SnarkjsProof = serde_json::from_value(cp["proof"].clone()).unwrap();
    assert!(lowki_groth16::verify(&cvk, &cpp, &csig).unwrap());

    // paymaster userOp from the withdraw proof (Kohaku mainnet adapter/paymaster addresses)
    let op = call(&mut c, json!({"type": "ppBuildPaymasterWithdraw", "note": note(0, 0),
        "withdrawal": {"processooor": adapter, "data": "0x1234"}, "proof": proof["solidity"],
        "pool": "0xF241d57C6DebAE225c0F2e6eA1529373C9A9C9fB", "paymaster": "0xe06CB96C57D2442f8F60F5017354BC08F7e91308",
        "adapter": adapter,
        "gas": {"callGasLimit": "0", "verificationGasLimit": "120000", "preVerificationGas": "60000",
                "paymasterVerificationGasLimit": "700000", "paymasterPostOpGasLimit": "60000"},
        "maxFeePerGas": "2000000000", "maxPriorityFeePerGas": "1000000", "nonce": "0"}), 0.0);
    ok(&op, "paymasterUserOp");
    let uo = &op["userOperation"];
    assert_eq!(uo["sender"], op["sender"]);
    assert!(uo["paymasterData"].as_str().unwrap().len() > 1000);
    assert!(uo["eip7702Auth"].is_object(), "{uo}");
    // the sender is the note's ephemeral signer (Kohaku deriveEphemeralSigner)
    let s = lowki_keys::to_checksum_address(&lowki_keys::address_from_private_key(&ephemeral(&mut c)).unwrap());
    assert_eq!(op["sender"], s);

    // write the proof for snarkjs + the on-chain verifier check
    let dirout = concat!(env!("CARGO_MANIFEST_DIR"), "/../../vectors/lowki-worker/out");
    std::fs::create_dir_all(dirout).unwrap();
    let line = json!({"circuit": "withdraw", "name": "native_worker_withdraw", "proof": proof["proof"],
        "publicSignals": proof["publicSignals"], "solidity": proof["solidity"]});
    let cline = json!({"circuit": "commitment", "name": "native_worker_commitment", "proof": cp["proof"],
        "publicSignals": cp["publicSignals"], "solidity": cp["solidity"]});
    std::fs::write(format!("{dirout}/native-proofs.jsonl"), format!("{line}\n{cline}\n")).unwrap();
}

/// The ephemeral signer key for note (0,0), via the Kohaku-scoped keystore + pp-core.
fn ephemeral(c: &mut Core) -> [u8; 32] {
    let k = call(c, json!({"type": "kohakuDeriveAt", "path": "m/28784'/1'/0'"}), 0.0);
    let key: [u8; 32] = hex::decode(&k["key"].as_str().unwrap()[2..]).unwrap().try_into().unwrap();
    let mgr = lowki_pp_core::SignatureSecretManager::new(&key).unwrap();
    let ep: [u8; 20] = hex::decode(&ENTRYPOINT[2..]).unwrap().try_into().unwrap();
    *mgr.derive_ephemeral_signer(11155111, &ep, 0, 0).unwrap()
}

#[test]
fn seal_app_key_writes_js_check() {
    let mut c = core();
    let m = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon art";
    call(&mut c, json!({"type": "importMnemonic", "mnemonic": m, "prfOutput": bytes_json(&[1; 32])}), 0.0);
    // enclave key pair: X25519 secret = 0x11.. (JS opens with it)
    let sk = [0x11u8; 32];
    let pk = x25519_public(&sk);
    let mut out = vec![];
    for (i, origin) in [ORIGIN, "https://uniswap.org"].iter().enumerate() {
        let r = call(&mut c, json!({"type": "sealAppKey", "origin": origin, "enclavePublicKey": format!("0x{}", hex::encode(pk)),
            "grant": {"signerId": "signer-1", "policyIds": ["pol-a", "pol-b"], "expiry": 1_800_000_000 + i as u64,
                       "nonce": if i == 0 { Some(format!("0x{}", "22".repeat(32))) } else { None }}}), 0.0);
        ok(&r, "sealedAppKey");
        let acct = call(&mut c, json!({"type": "appAccount", "origin": origin}), 0.0);
        assert_eq!(r["address"], acct["address"]);
        out.push(json!({"origin": origin, "sealed": r}));
    }
    assert_eq!(call(&mut c, json!({"type": "sealAppKey", "origin": ORIGIN, "enclavePublicKey": "0x1234",
        "grant": {"signerId": "s", "policyIds": [], "expiry": 1}}), 0.0)["code"], "invalidRequest");
    let dirout = concat!(env!("CARGO_MANIFEST_DIR"), "/../../vectors/lowki-worker/out");
    std::fs::create_dir_all(dirout).unwrap();
    std::fs::write(
        format!("{dirout}/sealed.json"),
        serde_json::to_string_pretty(&json!({"enclaveSecretKey": format!("0x{}", hex::encode(sk)), "mnemonic": m, "cases": out})).unwrap(),
    )
    .unwrap();
}

fn x25519_public(sk: &[u8; 32]) -> [u8; 32] {
    use hpke::{Kem, Serializable, Deserializable};
    let sk = <hpke::kem::X25519HkdfSha256 as Kem>::PrivateKey::from_bytes(sk).unwrap();
    <hpke::kem::X25519HkdfSha256 as Kem>::sk_to_pk(&sk).to_bytes().into()
}
