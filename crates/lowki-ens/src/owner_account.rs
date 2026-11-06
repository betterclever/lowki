//! `LowkiOwnerAccount` (`contracts/src/LowkiOwnerAccount.sol`): the EIP-7702 delegate that lets
//! a Lowki operator submit, and pay for, a batch of calls the name-owner key signed. With it the
//! owner key never needs ETH: after the sponsored registration (`lowki_registrar`), the owner's
//! setup (deploy its `PermissionedResolver`, configure `StealthResolver`, set the resolver) is one
//! sponsored type-4 transaction carrying
//!
//! 1. an EIP-7702 authorisation by the owner key to `LowkiOwnerAccount`
//!    ([`sign_authorization`]), and
//! 2. `execute(calls, deadline, signature)` on the owner's own address, where `signature` is the
//!    owner's EIP-712 `Execute` ([`sign_execute`]). The domain's `verifyingContract` is the
//!    owner's address (the delegate runs as the EOA), so a batch signed for one owner cannot run
//!    as another.

use alloc::vec::Vec;

use crate::abi::{self, Token};
use crate::identity::IdentityKey;
use crate::{Address, Error, U256, keccak256, sig};

pub const DOMAIN_NAME: &str = "Lowki Owner Account";
pub const DOMAIN_VERSION: &str = "1";
pub const EIP712_DOMAIN_TYPE: &str =
    "EIP712Domain(string name,string version,uint256 chainId,address verifyingContract)";
pub const CALL_TYPE: &str = "Call(address target,uint256 value,bytes data)";
/// The primary type with its referenced `Call` type appended, as EIP-712 encodes it.
pub const EXECUTE_TYPE: &str =
    "Execute(Call[] calls,uint256 nonce,uint256 deadline)Call(address target,uint256 value,bytes data)";
pub const EXECUTE_SIG: &str = "execute((address,uint256,bytes)[],uint256,bytes)";
pub const NONCE_SIG: &str = "nonce()";

/// EIP-7702 `MAGIC` prefix of an authorisation's signing payload.
pub const AUTHORIZATION_MAGIC: u8 = 0x05;

/// One call of a batch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Call {
    pub target: Address,
    /// Wei, as a big-endian word.
    pub value: U256,
    pub data: Vec<u8>,
}

/// EIP-712 domain separator for the delegated account `account` (the owner's EOA).
pub fn domain_separator(chain_id: u64, account: &Address) -> [u8; 32] {
    keccak256(&abi::encode(&[
        Token::b32(&keccak256(EIP712_DOMAIN_TYPE.as_bytes())),
        Token::b32(&keccak256(DOMAIN_NAME.as_bytes())),
        Token::b32(&keccak256(DOMAIN_VERSION.as_bytes())),
        Token::uint(chain_id),
        Token::address(account),
    ]))
}

/// `hashStruct(Call)`.
pub fn call_hash(c: &Call) -> [u8; 32] {
    keccak256(&abi::encode(&[
        Token::b32(&keccak256(CALL_TYPE.as_bytes())),
        Token::address(&c.target),
        Token::u256(&c.value),
        Token::b32(&keccak256(&c.data)),
    ]))
}

/// `hashStruct(Execute)`: the `Call[]` member is the keccak of the concatenated call hashes.
pub fn execute_struct_hash(calls: &[Call], nonce: u64, deadline: u64) -> [u8; 32] {
    let mut packed = Vec::with_capacity(32 * calls.len());
    for c in calls {
        packed.extend_from_slice(&call_hash(c));
    }
    keccak256(&abi::encode(&[
        Token::b32(&keccak256(EXECUTE_TYPE.as_bytes())),
        Token::b32(&keccak256(&packed)),
        Token::uint(nonce),
        Token::uint(deadline),
    ]))
}

/// The digest `LowkiOwnerAccount.executeDigest(calls, nonce, deadline)` returns on `account`.
pub fn execute_digest(chain_id: u64, account: &Address, calls: &[Call], nonce: u64, deadline: u64) -> [u8; 32] {
    sig::eip712_digest(&domain_separator(chain_id, account), &execute_struct_hash(calls, nonce, deadline))
}

/// Signs an `Execute` with the owner key; `account` must be the key's own address.
pub fn sign_execute(
    key: &IdentityKey,
    chain_id: u64,
    account: &Address,
    calls: &[Call],
    nonce: u64,
    deadline: u64,
) -> Result<[u8; 65], Error> {
    if key.address() != *account {
        return Err(Error::InvalidSignature);
    }
    Ok(key.sign_digest(&execute_digest(chain_id, account, calls, nonce, deadline)))
}

fn calls_token(calls: &[Call]) -> Token {
    Token::Array(
        calls
            .iter()
            .map(|c| Token::Tuple(alloc::vec![Token::address(&c.target), Token::u256(&c.value), Token::bytes(&c.data)]))
            .collect(),
    )
}

/// `execute((address,uint256,bytes)[] calls, uint256 deadline, bytes signature)`
pub fn encode_execute(calls: &[Call], deadline: u64, signature: &[u8]) -> Vec<u8> {
    abi::encode_sig(EXECUTE_SIG, &[calls_token(calls), Token::uint(deadline), Token::bytes(signature)])
}

/// `nonce()`
pub fn encode_nonce() -> Vec<u8> {
    abi::encode_sig(NONCE_SIG, &[])
}

// ------------------------------------------------------------------------------ EIP-7702

/// An EIP-7702 authorisation signed by the owner key.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SignedAuthorization {
    pub chain_id: u64,
    /// The delegate (`LowkiOwnerAccount`).
    pub address: Address,
    /// The authority's account nonce at inclusion time.
    pub nonce: u64,
    pub y_parity: u8,
    pub r: [u8; 32],
    pub s: [u8; 32],
}

fn rlp_uint(out: &mut Vec<u8>, v: u64) {
    if v == 0 {
        out.push(0x80);
    } else if v < 0x80 {
        out.push(v as u8);
    } else {
        let b = v.to_be_bytes();
        let skip = b.iter().take_while(|&&x| x == 0).count();
        out.push(0x80 + (8 - skip) as u8);
        out.extend_from_slice(&b[skip..]);
    }
}

/// `rlp([chain_id, address, nonce])`.
pub fn authorization_rlp(chain_id: u64, address: &Address, nonce: u64) -> Vec<u8> {
    let mut payload = Vec::with_capacity(40);
    rlp_uint(&mut payload, chain_id);
    payload.push(0x80 + 20);
    payload.extend_from_slice(address);
    rlp_uint(&mut payload, nonce);
    // At most 9 + 21 + 9 = 39 bytes: always the short-list form.
    let mut out = Vec::with_capacity(payload.len() + 1);
    out.push(0xc0 + payload.len() as u8);
    out.extend_from_slice(&payload);
    out
}

/// `keccak256(0x05 || rlp([chain_id, address, nonce]))` (viem `hashAuthorization`).
pub fn authorization_digest(chain_id: u64, address: &Address, nonce: u64) -> [u8; 32] {
    let rlp = authorization_rlp(chain_id, address, nonce);
    let mut pre = Vec::with_capacity(rlp.len() + 1);
    pre.push(AUTHORIZATION_MAGIC);
    pre.extend_from_slice(&rlp);
    keccak256(&pre)
}

/// Signs an EIP-7702 authorisation delegating the owner's EOA to `delegate`.
pub fn sign_authorization(key: &IdentityKey, chain_id: u64, delegate: &Address, nonce: u64) -> SignedAuthorization {
    let sig = key.sign_digest(&authorization_digest(chain_id, delegate, nonce));
    let mut r = [0u8; 32];
    let mut s = [0u8; 32];
    r.copy_from_slice(&sig[..32]);
    s.copy_from_slice(&sig[32..64]);
    SignedAuthorization { chain_id, address: *delegate, nonce, y_parity: sig[64] - 27, r, s }
}
