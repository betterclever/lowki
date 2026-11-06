//! Delegation export (stub of ARCHITECTURE.md "Delegation" step 5): seal one app-account
//! key to a signer enclave with HPKE, together with a grant signed by that account key.
//!
//! * HPKE (RFC 9180), mode Base, suite DHKEM(X25519, HKDF-SHA256) / HKDF-SHA256 /
//!   ChaCha20-Poly1305 (KEM 0x0020, KDF 0x0001, AEAD 0x0003), `hpke` crate 0.13.
//! * `info = "lowki/delegation/v1"`, `aad = grantHash` (the EIP-712 hash of the grant), so
//!   the ciphertext only opens together with the exact grant that was signed.
//! * plaintext = the 32-byte secp256k1 private key of `m/44'/60'/{app_index(origin)}'/0/0`.
//! * The grant is EIP-712 typed data signed by the same account key:
//!   domain `{name: "Lowki Delegation", version: "1"}`,
//!   `LowkiDelegationGrant(address account,string signerId,string[] policyIds,uint64 expiry,bytes32 nonce,bytes32 enclaveKey)`.
//!   `enclaveKey` is the enclave's raw X25519 public key, binding the grant to the enclave.
//!
//! Attestation verification of the enclave key (step 4) is NOT done here: the caller
//! must only pass keys from an enclave whose attestation it verified. That is the stub.

use hpke::aead::ChaCha20Poly1305;
use hpke::kdf::HkdfSha256;
use hpke::kem::X25519HkdfSha256;
use hpke::{Deserializable, Kem as KemTrait, OpModeS, Serializable};
use rand_core::{CryptoRng, RngCore};

use crate::error::{ErrorCode, Result, WorkerError};
use crate::protocol::GrantIn;
use crate::sign;
use crate::util;

pub const SUITE: &str = "HPKE RFC9180 base mode; KEM DHKEM(X25519, HKDF-SHA256) 0x0020; KDF HKDF-SHA256 0x0001; AEAD ChaCha20-Poly1305 0x0003; info=\"lowki/delegation/v1\"; aad=grantHash";
pub const INFO: &[u8] = b"lowki/delegation/v1";

/// rand_core 0.9 view of a rand_core 0.6 CSPRNG (hpke 0.13 uses rand_core 0.9).
struct Rng09<'a, R>(&'a mut R);

impl<R: RngCore> rand_core_09::RngCore for Rng09<'_, R> {
    fn next_u32(&mut self) -> u32 {
        self.0.next_u32()
    }
    fn next_u64(&mut self) -> u64 {
        self.0.next_u64()
    }
    fn fill_bytes(&mut self, dst: &mut [u8]) {
        self.0.fill_bytes(dst)
    }
}
impl<R: RngCore + CryptoRng> rand_core_09::CryptoRng for Rng09<'_, R> {}

pub fn grant_typed_data(account: &str, g: &GrantIn, nonce_hex: &str, enclave_key_hex: &str) -> serde_json::Value {
    serde_json::json!({
        "types": {
            "EIP712Domain": [
                {"name": "name", "type": "string"},
                {"name": "version", "type": "string"}
            ],
            "LowkiDelegationGrant": [
                {"name": "account", "type": "address"},
                {"name": "signerId", "type": "string"},
                {"name": "policyIds", "type": "string[]"},
                {"name": "expiry", "type": "uint64"},
                {"name": "nonce", "type": "bytes32"},
                {"name": "enclaveKey", "type": "bytes32"}
            ]
        },
        "primaryType": "LowkiDelegationGrant",
        "domain": {"name": "Lowki Delegation", "version": "1"},
        "message": {
            "account": account,
            "signerId": g.signer_id,
            "policyIds": g.policy_ids,
            "expiry": g.expiry,
            "nonce": nonce_hex,
            "enclaveKey": enclave_key_hex
        }
    })
}

pub struct Sealed {
    pub address: String,
    pub enc: Vec<u8>,
    pub ciphertext: Vec<u8>,
    pub grant: serde_json::Value,
    pub grant_hash: [u8; 32],
    pub grant_signature: [u8; 65],
}

pub fn seal_app_key<R: RngCore + CryptoRng>(
    seed: &[u8; 64],
    origin: &str,
    enclave_public_key: &str,
    grant: &GrantIn,
    rng: &mut R,
) -> Result<Sealed> {
    let pk_bytes = util::bytes(enclave_public_key, "enclavePublicKey")?;
    if pk_bytes.len() != 32 {
        return Err(WorkerError::invalid("enclavePublicKey must be a 32-byte X25519 public key"));
    }
    let pk = <X25519HkdfSha256 as KemTrait>::PublicKey::from_bytes(&pk_bytes)
        .map_err(|_| WorkerError::invalid("enclavePublicKey is not a valid X25519 key"))?;
    if grant.signer_id.is_empty() {
        return Err(WorkerError::invalid("grant.signerId is empty"));
    }
    let nonce = match &grant.nonce {
        Some(n) => util::b256(n, "grant.nonce")?.0,
        None => {
            let mut n = [0u8; 32];
            rng.fill_bytes(&mut n);
            n
        }
    };
    let key = lowki_keys::app_private_key(seed, origin);
    let address = lowki_keys::to_checksum_address(&lowki_keys::address_from_private_key(&key)?);
    let typed = grant_typed_data(&address, grant, &util::hex0x(&nonce), &util::hex0x(&pk_bytes));
    let grant_hash = sign::eip712_hash(&typed)?;
    let sig = sign::sign_hash(&key, &grant_hash)?;
    let (enc, ciphertext) = hpke::single_shot_seal::<ChaCha20Poly1305, HkdfSha256, X25519HkdfSha256, _>(
        &OpModeS::Base,
        &pk,
        INFO,
        key.as_slice(),
        grant_hash.as_slice(),
        &mut Rng09(rng),
    )
    .map_err(|_| WorkerError::new(ErrorCode::Seal, "HPKE seal failed"))?;
    Ok(Sealed {
        address,
        enc: enc.to_bytes().to_vec(),
        ciphertext,
        grant: typed,
        grant_hash: grant_hash.0,
        grant_signature: sig.to_bytes65(),
    })
}
