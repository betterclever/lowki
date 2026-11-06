//! App-account signing: EIP-191 personal_sign, EIP-712 typed data, EIP-1559 (type 2) and
//! EIP-7702 (type 4) transactions, and EIP-7702 authorizations.

use alloy_dyn_abi::TypedData;
use alloy_primitives::{Address, B256, Bytes, U256, keccak256};
use alloy_rlp::{Encodable, Header};
use lowki_keys::Zeroizing;

use crate::error::{ErrorCode, Result, WorkerError};
use crate::protocol::{SignedAuthorizationIn, TxIn};
use crate::util;

/// `keccak256("\x19Ethereum Signed Message:\n" || len || message)`.
pub fn eip191_hash(message: &[u8]) -> B256 {
    let mut buf = Vec::with_capacity(32 + message.len());
    buf.extend_from_slice(b"\x19Ethereum Signed Message:\n");
    buf.extend_from_slice(message.len().to_string().as_bytes());
    buf.extend_from_slice(message);
    keccak256(&buf)
}

/// EIP-712 signing hash of a typed-data JSON object (`{types, primaryType, domain, message}`).
/// `EIP712Domain` may be omitted from `types` (viem infers it from `domain`).
pub fn eip712_hash(typed_data: &serde_json::Value) -> Result<B256> {
    let td: TypedData = serde_json::from_value(typed_data.clone())
        .map_err(|e| WorkerError::invalid(format!("typedData: {e}")))?;
    td.eip712_signing_hash()
        .map_err(|e| WorkerError::invalid(format!("typedData: {e}")))
}

pub struct Sig {
    pub r: [u8; 32],
    pub s: [u8; 32],
    pub y_parity: u8,
}

impl Sig {
    /// `r || s || v` with `v = 27 + yParity`.
    pub fn to_bytes65(&self) -> [u8; 65] {
        let mut o = [0u8; 65];
        o[..32].copy_from_slice(&self.r);
        o[32..64].copy_from_slice(&self.s);
        o[64] = 27 + self.y_parity;
        o
    }
}

pub fn sign_hash(key: &Zeroizing<[u8; 32]>, digest: &B256) -> Result<Sig> {
    let s = lowki_keys::sign_digest(key, &digest.0)?;
    Ok(Sig { r: s.r, s: s.s, y_parity: s.y_parity() })
}

// ---- RLP helpers ----------------------------------------------------------------------

fn rlp_list(items: &[Vec<u8>]) -> Vec<u8> {
    let payload_length: usize = items.iter().map(Vec::len).sum();
    let mut out = Vec::with_capacity(payload_length + 9);
    Header { list: true, payload_length }.encode(&mut out);
    for i in items {
        out.extend_from_slice(i);
    }
    out
}

fn enc<T: Encodable + ?Sized>(v: &T) -> Vec<u8> {
    let mut out = Vec::new();
    v.encode(&mut out);
    out
}

fn enc_u256_be(b: &[u8; 32]) -> Vec<u8> {
    enc(&U256::from_be_bytes(*b))
}

struct AccessItem {
    address: Address,
    keys: Vec<B256>,
}

fn enc_access_list(list: &[AccessItem]) -> Vec<u8> {
    let items: Vec<Vec<u8>> = list
        .iter()
        .map(|a| {
            let keys: Vec<Vec<u8>> = a.keys.iter().map(enc).collect();
            rlp_list(&[enc(&a.address), rlp_list(&keys)])
        })
        .collect();
    rlp_list(&items)
}

struct Auth {
    chain_id: U256,
    address: Address,
    nonce: u64,
    y_parity: u8,
    r: U256,
    s: U256,
}

fn enc_auth_list(list: &[Auth]) -> Vec<u8> {
    let items: Vec<Vec<u8>> = list
        .iter()
        .map(|a| rlp_list(&[enc(&a.chain_id), enc(&a.address), enc(&a.nonce), enc(&a.y_parity), enc(&a.r), enc(&a.s)]))
        .collect();
    rlp_list(&items)
}

pub struct SignedTx {
    pub raw: Vec<u8>,
    pub hash: B256,
}

/// Signs an EIP-1559 or EIP-7702 transaction with `key`.
pub fn sign_transaction(key: &Zeroizing<[u8; 32]>, tx: &TxIn) -> Result<SignedTx> {
    let ty: u8 = match tx.kind.as_str() {
        "eip1559" | "0x2" | "2" => 2,
        "eip7702" | "0x4" | "4" => 4,
        _ => return Err(WorkerError::invalid("tx.type must be eip1559 or eip7702")),
    };
    let chain_id = util::u64q(&tx.chain_id, "tx.chainId")?;
    let nonce = util::u64q(&tx.nonce, "tx.nonce")?;
    let tip = util::u128q(&tx.max_priority_fee_per_gas, "tx.maxPriorityFeePerGas")?;
    let max_fee = util::u128q(&tx.max_fee_per_gas, "tx.maxFeePerGas")?;
    if tip > max_fee {
        return Err(WorkerError::invalid("tx.maxPriorityFeePerGas exceeds tx.maxFeePerGas"));
    }
    let gas = util::u64q(&tx.gas, "tx.gas")?;
    let to = match &tx.to {
        Some(t) => Some(util::address(t, "tx.to")?),
        None => None,
    };
    if ty == 4 && to.is_none() {
        return Err(WorkerError::invalid("eip7702 transactions cannot create contracts (tx.to required)"));
    }
    let value = match &tx.value {
        Some(v) => util::u256(v, "tx.value")?,
        None => U256::ZERO,
    };
    let data: Bytes = match &tx.data {
        Some(d) => util::bytes(d, "tx.data")?,
        None => Bytes::new(),
    };
    let access: Vec<AccessItem> = tx
        .access_list
        .iter()
        .map(|a| {
            Ok(AccessItem {
                address: util::address(&a.address, "accessList.address")?,
                keys: a.storage_keys.iter().map(|k| util::b256(k, "accessList.storageKeys")).collect::<Result<_>>()?,
            })
        })
        .collect::<Result<_>>()?;
    let auths: Vec<Auth> = tx
        .authorization_list
        .iter()
        .map(|a| {
            Ok(Auth {
                chain_id: util::u256(&a.chain_id, "authorization.chainId")?,
                address: util::address(&a.address, "authorization.address")?,
                nonce: util::u64q(&a.nonce, "authorization.nonce")?,
                y_parity: a.y_parity,
                r: util::u256(&a.r, "authorization.r")?,
                s: util::u256(&a.s, "authorization.s")?,
            })
        })
        .collect::<Result<_>>()?;
    if ty == 2 && !auths.is_empty() {
        return Err(WorkerError::invalid("authorizationList is only valid for eip7702"));
    }
    if ty == 4 && auths.is_empty() {
        return Err(WorkerError::invalid("eip7702 transactions need a non-empty authorizationList"));
    }

    let mut fields = vec![
        enc(&chain_id),
        enc(&nonce),
        enc(&tip),
        enc(&max_fee),
        enc(&gas),
        match to {
            Some(a) => enc(&a),
            None => enc(&Bytes::new()),
        },
        enc(&value),
        enc(&data),
        enc_access_list(&access),
    ];
    if ty == 4 {
        fields.push(enc_auth_list(&auths));
    }
    let mut pre = vec![ty];
    pre.extend_from_slice(&rlp_list(&fields));
    let sighash = keccak256(&pre);
    let sig = sign_hash(key, &sighash)?;
    fields.push(enc(&sig.y_parity));
    fields.push(enc_u256_be(&sig.r));
    fields.push(enc_u256_be(&sig.s));
    let mut raw = vec![ty];
    raw.extend_from_slice(&rlp_list(&fields));
    let hash = keccak256(&raw);
    Ok(SignedTx { raw, hash })
}

/// EIP-7702 authorization `{chainId, address, nonce}` signed by `key`.
pub fn sign_authorization(key: &Zeroizing<[u8; 32]>, chain_id: U256, address: Address, nonce: u64) -> Result<SignedAuthorizationIn> {
    let signer = lowki_aa::signer::EphemeralSigner::from_bytes(key)
        .map_err(|_| WorkerError::new(ErrorCode::Signing, "invalid app key"))?;
    let signed = lowki_aa::eip7702::Authorization { chain_id, address, nonce }.sign(&signer)?;
    Ok(SignedAuthorizationIn {
        chain_id: chain_id.to_string(),
        address: address.to_checksum(None),
        nonce: nonce.to_string(),
        y_parity: signed.y_parity,
        r: format!("{}", signed.r),
        s: format!("{}", signed.s),
    })
}
