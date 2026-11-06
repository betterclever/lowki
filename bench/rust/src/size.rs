//! Payload-size probes (feature `size-only` + exactly one area feature). Each exports the
//! API surface an iframe would actually call for that area, so the linker keeps what a
//! production module would ship and drops the rest. Not used for timing.
#![allow(clippy::missing_safety_doc)]
use wasm_bindgen::prelude::*;

fn a32(b: &[u8]) -> [u8; 32] {
    b.try_into().unwrap()
}

#[cfg(feature = "keys")]
mod keys {
    use super::*;
    #[wasm_bindgen]
    pub fn mnemonic_to_key(mnemonic: &str, path: &str) -> Result<Vec<u8>, JsError> {
        let ks = lowki_keys::Keystore::from_mnemonic(mnemonic).map_err(|e| JsError::new(&e.to_string()))?;
        Ok(ks.derive_at(path).map_err(|e| JsError::new(&e.to_string()))?.to_vec())
    }
    #[wasm_bindgen]
    pub fn sign_digest(key: &[u8], digest: &[u8]) -> Result<Vec<u8>, JsError> {
        Ok(lowki_keys::sign_digest(&a32(key), &a32(digest)).map_err(|e| JsError::new(&e.to_string()))?.to_bytes().to_vec())
    }
    #[wasm_bindgen]
    pub fn app_address(seed: &[u8], origin: &str) -> Vec<u8> {
        lowki_keys::app_address(&seed.try_into().unwrap(), origin).to_vec()
    }
    #[wasm_bindgen]
    pub fn wrap(prf: &[u8], entropy: &[u8], nonce: &[u8]) -> Vec<u8> {
        lowki_keys::wrap_entropy(&a32(prf), &a32(entropy), &nonce.try_into().unwrap()).to_vec()
    }
    #[wasm_bindgen]
    pub fn unwrap(prf: &[u8], blob: &[u8]) -> Result<Vec<u8>, JsError> {
        Ok(lowki_keys::unwrap_entropy(&a32(prf), blob).map_err(|e| JsError::new(&e.to_string()))?.to_vec())
    }
}

#[cfg(feature = "pp")]
mod pp {
    use super::*;
    use lowki_pp_core::*;
    fn f(b: &[u8]) -> Fr {
        fr_from_be_bytes_mod_order(&a32(b))
    }
    #[wasm_bindgen]
    pub fn poseidon_t3(a: &[u8], b: &[u8]) -> Vec<u8> {
        fr_to_be_bytes(&poseidon2(f(a), f(b))).to_vec()
    }
    /// leaves: n×32 bytes; returns root || proof siblings of `index` (padded to 32).
    #[wasm_bindgen]
    pub fn limt_root_and_proof(leaves: &[u8], index: usize) -> Vec<u8> {
        let ls: Vec<Fr> = leaves.chunks(32).map(f).collect();
        let t = LeanImt::from_leaves(&ls);
        let p = proof_from_tree(&t, &ls[index]).unwrap();
        let mut out = fr_to_be_bytes(&p.root).to_vec();
        for s in p.siblings {
            out.extend(fr_to_be_bytes(&s));
        }
        out
    }
    #[wasm_bindgen]
    pub fn deposit_secrets(key: &[u8], chain_id: u64, entrypoint: &[u8], index: u64) -> Vec<u8> {
        let s = SignatureSecretManager::new(&a32(key)).unwrap().deposit_secrets(chain_id, &entrypoint.try_into().unwrap(), index).unwrap();
        [s.nullifier, s.salt, s.precommitment, s.nullifier_hash].iter().flat_map(fr_to_be_bytes).collect()
    }
    #[wasm_bindgen]
    pub fn ephemeral_signer(key: &[u8], chain_id: u64, entrypoint: &[u8], d: u64, w: u64) -> Vec<u8> {
        SignatureSecretManager::new(&a32(key)).unwrap().derive_ephemeral_signer(chain_id, &entrypoint.try_into().unwrap(), d, w).unwrap().to_vec()
    }
    #[wasm_bindgen]
    pub fn note_commitment(v: &[u8], l: &[u8], p: &[u8]) -> Vec<u8> {
        fr_to_be_bytes(&commitment(f(v), f(l), f(p))).to_vec()
    }
    #[wasm_bindgen]
    pub fn context(processooor: &[u8], data: &[u8], scope: &[u8]) -> Vec<u8> {
        fr_to_be_bytes(&calculate_context(&processooor.try_into().unwrap(), data, &a32(scope))).to_vec()
    }
}

#[cfg(feature = "stealth")]
mod stealth {
    use super::*;
    use lowki_stealth::*;
    #[wasm_bindgen]
    pub fn generate(meta_uri: &str, eph: &[u8]) -> Vec<u8> {
        let g = generate_stealth_address(&StealthMetaAddress::parse_uri(meta_uri).unwrap(), &SecretKey::from_bytes(&a32(eph)).unwrap()).unwrap();
        let mut v = g.stealth_address.to_vec();
        v.extend(g.ephemeral_pubkey);
        v.push(g.view_tag);
        v
    }
    /// anns: n × (33 eph || 1 tag || 20 addr); returns matching indices.
    #[wasm_bindgen]
    pub fn scan(view: &[u8], spend_pub: &[u8], anns: &[u8]) -> Vec<u32> {
        let v = SecretKey::from_bytes(&a32(view)).unwrap();
        let sp: [u8; 33] = spend_pub.try_into().unwrap();
        anns.chunks(54).enumerate().filter(|(_, a)| check_announcement(&v, &sp, &a[..33], a[33]).map(|x| x[..] == a[34..]) == Some(true)).map(|(i, _)| i as u32).collect()
    }
    #[wasm_bindgen]
    pub fn stealth_key(spend: &[u8], view: &[u8], eph: &[u8]) -> Vec<u8> {
        compute_stealth_private_key(&SecretKey::from_bytes(&a32(spend)).unwrap(), &SecretKey::from_bytes(&a32(view)).unwrap(), eph).unwrap().to_bytes().to_vec()
    }
    #[wasm_bindgen]
    pub fn announce_calldata(stealth: &[u8], eph: &[u8], metadata: &[u8]) -> Vec<u8> {
        encode_announce_calldata(1, &stealth.try_into().unwrap(), eph, metadata)
    }
    #[wasm_bindgen]
    pub fn derive_keys(seed: &[u8]) -> String {
        derive_stealth_keys(&seed.try_into().unwrap()).unwrap().meta_address().to_uri()
    }
}

#[cfg(feature = "aa")]
mod aa {
    use super::*;
    /// Takes the same JSON the benchmark uses; returns the eth_sendUserOperation JSON.
    #[wasm_bindgen]
    pub fn build_signed_userop(params_json: &str) -> String {
        crate::aa::aa_build(&serde_json::from_str(params_json).unwrap())
    }
}

#[cfg(feature = "g16")]
mod g16 {
    use super::*;
    use lowki_groth16::*;
    #[wasm_bindgen]
    pub struct Prover(CircuitProver);
    #[wasm_bindgen]
    impl Prover {
        #[wasm_bindgen(constructor)]
        pub fn new(withdraw: bool, wasm: &[u8], zkey: &[u8]) -> Prover {
            Prover(CircuitProver::load(if withdraw { Circuit::Withdraw } else { Circuit::Commitment }, wasm, zkey).unwrap())
        }
        pub fn prove(&self, input_json: &str) -> String {
            let o = match self.0.circuit() {
                Circuit::Withdraw => prove_withdraw(&self.0, &WithdrawInputs::from_json(input_json).unwrap()),
                Circuit::Commitment => prove_commitment(&self.0, &CommitmentInputs::from_json(input_json).unwrap()),
            }
            .unwrap();
            serde_json::to_string(&o).unwrap()
        }
    }
    #[cfg(feature = "webgpu")]
    #[wasm_bindgen]
    pub async fn prove_gpu(withdraw: bool, wasm: Vec<u8>, zkey: Vec<u8>, input_json: String) -> String {
        let gpu = open_gpu().await.unwrap();
        let p = GpuProver::new(gpu, CircuitProver::load(if withdraw { Circuit::Withdraw } else { Circuit::Commitment }, &wasm, &zkey).unwrap()).map_err(|e| e.1).unwrap();
        let o = if withdraw {
            prove_withdraw_gpu(&p, &WithdrawInputs::from_json(&input_json).unwrap()).await
        } else {
            prove_commitment_gpu(&p, &CommitmentInputs::from_json(&input_json).unwrap()).await
        };
        serde_json::to_string(&o.unwrap()).unwrap()
    }
}
