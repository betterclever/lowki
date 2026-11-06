//! `<label>.lowki.eth` names and stealth payments (lowki-ens + lowki-stealth), and the
//! encrypted app list.
//!
//! - **Identity key** `m/137'/0'/{i}'` (lowki-ens `IdentityKey`): owns the name. It is never an
//!   app account or a stealth key, so the public name links to nothing else the user does.
//! - **Registration**: the owner's EIP-712 `Register` authorisation for `LowkiRegistrar`.
//! - **Owner batch**: one EIP-7702 authorisation (delegate `LowkiOwnerAccount`) and one
//!   EIP-712 `Execute` over [`owner_setup_calls`]. lowki-server rebuilds the same calls itself and
//!   only relays them if this signature recovers to the owner.
//! - **Records auth**: `personal_sign` by the stealth **viewing** key of
//!   `records_auth_message(name, ts)` (lowki-server `GET /v1/ens/records`).
//! - **Scan / spend**: ERC-5564 checks with the viewing key; a found address's key is derived
//!   (spending + viewing key) and used only inside the worker to sign a transaction.
//! - **App list**: `0x01 || nonce12 || AES-256-GCM(K, json, aad "lowki/app-list/v1")`,
//!   `K = HKDF-SHA256(ikm = seed, salt = "", info = "lowki/app-list/key/v1")`.

use aes_gcm::aead::{Aead, KeyInit, Payload};
use aes_gcm::{Aes256Gcm, Nonce};
use lowki_ens::{
    Address,
    abi::or,
    identity::IdentityKey,
    lowki_registrar::{Registration, sign_registration, validate_label},
    name::{dns_encode, labelhash, namehash},
    owner_account::{Call, sign_authorization, sign_execute},
    permissioned_resolver::{ADMIN_ROLES, ALL_ROLES, Grant, encode_deploy_proxy, encode_initialize, encode_set_text},
    registry::encode_set_resolver,
    stealth::{encode_set_chains, encode_set_config, encode_set_meta_address, encode_stealth_record, stealth_text_key},
};
use lowki_stealth::{StealthKeys, StealthMetaAddress};
use rand_core::{CryptoRng, RngCore};
use sha2::Sha256;
use zeroize::Zeroizing;

use crate::error::{ErrorCode, Result, WorkerError};

pub const APP_LIST_KEY_INFO: &[u8] = b"lowki/app-list/key/v1";
pub const APP_LIST_AAD: &[u8] = b"lowki/app-list/v1";
pub const APP_LIST_VERSION: u8 = 0x01;
/// The app list is small; anything bigger is a bug or abuse.
pub const APP_LIST_MAX: usize = 32 * 1024;

pub fn ens_err(e: lowki_ens::Error) -> WorkerError {
    // lowki-ens errors are enum names with public data only.
    WorkerError::new(ErrorCode::Signing, format!("ens: {e:?}"))
}

pub fn identity(seed: &[u8; 64], index: u32) -> Result<IdentityKey> {
    if index >= 0x8000_0000 {
        return Err(WorkerError::invalid("index must be below 2^31"));
    }
    IdentityKey::from_seed(seed, index).map_err(ens_err)
}

/// Public parameters of the owner batch (all addresses from the deployment).
pub struct SetupParams<'a> {
    pub name: &'a str,
    pub factory: Address,
    pub permissioned_resolver_impl: Address,
    pub stealth_resolver: Address,
    pub registry: Address,
    pub gateway_url: &'a str,
    /// `VerifiableFactory.predictProxyAddress(owner, namehash(name))`.
    pub records_resolver: Address,
}

/// The owner's sponsored setup batch (contracts/README.md "Per-name setup"):
/// 1. deploy the owner's PermissionedResolver (all roles to the owner, legacy text record);
/// 2. `StealthResolver.setConfig(name, recordsResolver, [gateway])`;
/// 3. `setChains(name, [60], true)`;
/// 4. `setMetaAddress(name, 0x01 || spendPub || viewPub)` (client-side derivation allowed);
/// 5. `registry.setResolver(labelhash, StealthResolver)`, last.
///
/// lowki-server has a byte-identical copy (`crates/lowki-server/src/ens_names.rs`).
pub fn owner_setup_calls(owner: &Address, meta: &StealthMetaAddress, p: &SetupParams) -> Result<Vec<Call>> {
    let label = p.name.split('.').next().unwrap_or_default();
    validate_label(label).map_err(|_| WorkerError::invalid("name: label breaks the Lowki label rules"))?;
    let dns = dns_encode(p.name).map_err(ens_err)?;
    let node = namehash(p.name);
    let init = encode_initialize(
        &[Grant { account: *owner, role_bitmap: or(ALL_ROLES, ADMIN_ROLES) }],
        &[encode_set_text(&dns, &stealth_text_key(1), &meta.to_uri())],
    );
    let zero = [0u8; 32];
    Ok(vec![
        Call { target: p.factory, value: zero, data: encode_deploy_proxy(&p.permissioned_resolver_impl, &node, &init) },
        Call { target: p.stealth_resolver, value: zero, data: encode_set_config(&dns, &p.records_resolver, &[p.gateway_url]) },
        Call { target: p.stealth_resolver, value: zero, data: encode_set_chains(&dns, &[60], true) },
        Call { target: p.stealth_resolver, value: zero, data: encode_set_meta_address(&dns, &encode_stealth_record(true, meta)) },
        Call { target: p.registry, value: zero, data: encode_set_resolver(&labelhash(label), &p.stealth_resolver) },
    ])
}

pub struct SignedSetup {
    pub owner: Address,
    pub execute_signature: [u8; 65],
    pub authorization: lowki_ens::owner_account::SignedAuthorization,
    pub record: Vec<u8>,
}

#[allow(clippy::too_many_arguments)]
pub fn sign_owner_setup(
    seed: &[u8; 64],
    index: u32,
    stealth: &StealthKeys,
    chain_id: u64,
    owner_account: &Address,
    auth_nonce: u64,
    exec_nonce: u64,
    deadline: u64,
    p: &SetupParams,
) -> Result<SignedSetup> {
    let id = identity(seed, index)?;
    let owner = id.address();
    let meta = stealth.meta_address();
    let calls = owner_setup_calls(&owner, &meta, p)?;
    let execute_signature = sign_execute(&id, chain_id, &owner, &calls, exec_nonce, deadline).map_err(ens_err)?;
    let authorization = sign_authorization(&id, chain_id, owner_account, auth_nonce);
    Ok(SignedSetup { owner, execute_signature, authorization, record: encode_stealth_record(true, &meta) })
}

pub fn sign_register(seed: &[u8; 64], index: u32, chain_id: u64, registrar: &Address, mut r: Registration) -> Result<(Address, [u8; 65])> {
    validate_label(&r.label).map_err(|_| WorkerError::invalid("label: 3-32 of a-z, 0-9 and -, no leading or trailing -"))?;
    let id = identity(seed, index)?;
    r.owner = id.address();
    Ok((r.owner, sign_registration(&id, chain_id, registrar, &r)))
}

fn app_list_key(seed: &[u8; 64]) -> Zeroizing<[u8; 32]> {
    let hk = hkdf::Hkdf::<Sha256>::new(None, seed);
    let mut k = Zeroizing::new([0u8; 32]);
    hk.expand(APP_LIST_KEY_INFO, k.as_mut_slice()).expect("32 bytes is a valid HKDF length");
    k
}

pub fn seal_app_list<R: RngCore + CryptoRng>(seed: &[u8; 64], data: &[u8], rng: &mut R) -> Result<Vec<u8>> {
    if data.len() > APP_LIST_MAX {
        return Err(WorkerError::invalid("app list too large"));
    }
    let k = app_list_key(seed);
    let cipher = Aes256Gcm::new_from_slice(k.as_slice()).map_err(|_| WorkerError::new(ErrorCode::Internal, "key"))?;
    let mut nonce = [0u8; 12];
    rng.fill_bytes(&mut nonce);
    let ct = cipher
        .encrypt(Nonce::from_slice(&nonce), Payload { msg: data, aad: APP_LIST_AAD })
        .map_err(|_| WorkerError::new(ErrorCode::Internal, "seal failed"))?;
    let mut out = Vec::with_capacity(13 + ct.len());
    out.push(APP_LIST_VERSION);
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&ct);
    Ok(out)
}

pub fn open_app_list(seed: &[u8; 64], blob: &[u8]) -> Result<Vec<u8>> {
    let fail = || WorkerError::new(ErrorCode::UnwrapFailed, "the app list does not open with this wallet");
    if blob.len() < 13 + 16 || blob[0] != APP_LIST_VERSION {
        return Err(fail());
    }
    let k = app_list_key(seed);
    let cipher = Aes256Gcm::new_from_slice(k.as_slice()).map_err(|_| fail())?;
    cipher.decrypt(Nonce::from_slice(&blob[1..13]), Payload { msg: &blob[13..], aad: APP_LIST_AAD }).map_err(|_| fail())
}
