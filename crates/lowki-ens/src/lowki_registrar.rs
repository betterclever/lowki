//! `LowkiRegistrar`: gasless subname registration authorized by an EIP-712 signature of the
//! name-owner identity key.
//!
//! * Domain: `EIP712Domain(string name,string version,uint256 chainId,address verifyingContract)`
//!   with `name = "Lowki Registrar"`, `version = "1"`, `verifyingContract` = the registrar.
//! * Primary type:
//!   `Register(string label,address owner,address resolver,uint64 duration,uint256 nonce,uint256 deadline)`.
//! * Calls: `register(string,address,address,uint64,uint256,bytes)`, `nonces(address)`,
//!   `available(string)`, `renew(string,uint64)`.

use alloc::string::String;
use alloc::vec::Vec;

use crate::abi::{self, Token};
use crate::identity::IdentityKey;
use crate::{Address, Error, keccak256};

pub const DOMAIN_NAME: &str = "Lowki Registrar";
pub const DOMAIN_VERSION: &str = "1";
pub const EIP712_DOMAIN_TYPE: &str =
    "EIP712Domain(string name,string version,uint256 chainId,address verifyingContract)";
pub const REGISTER_TYPE: &str = "Register(string label,address owner,address resolver,uint64 duration,uint256 nonce,uint256 deadline)";

pub const REGISTER_SIG: &str = "register(string,address,address,uint64,uint256,bytes)";
pub const NONCES_SIG: &str = "nonces(address)";
pub const AVAILABLE_SIG: &str = "available(string)";
pub const RENEW_SIG: &str = "renew(string,uint64)";

/// The signed registration request. `nonce` and `deadline` are `uint256` onchain; `u64` is
/// enough for a nonce counter and a unix timestamp.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Registration {
    pub label: String,
    pub owner: Address,
    pub resolver: Address,
    pub duration: u64,
    pub nonce: u64,
    pub deadline: u64,
}

/// EIP-712 domain separator of a registrar.
pub fn domain_separator(chain_id: u64, registrar: &Address) -> [u8; 32] {
    keccak256(&abi::encode(&[
        Token::b32(&keccak256(EIP712_DOMAIN_TYPE.as_bytes())),
        Token::b32(&keccak256(DOMAIN_NAME.as_bytes())),
        Token::b32(&keccak256(DOMAIN_VERSION.as_bytes())),
        Token::uint(chain_id),
        Token::address(registrar),
    ]))
}

/// `hashStruct(Register)`.
pub fn registration_struct_hash(r: &Registration) -> [u8; 32] {
    keccak256(&abi::encode(&[
        Token::b32(&keccak256(REGISTER_TYPE.as_bytes())),
        Token::b32(&keccak256(r.label.as_bytes())),
        Token::address(&r.owner),
        Token::address(&r.resolver),
        Token::uint(r.duration),
        Token::uint(r.nonce),
        Token::uint(r.deadline),
    ]))
}

/// The EIP-712 digest the identity key signs (viem `hashTypedData`).
pub fn registration_digest(chain_id: u64, registrar: &Address, r: &Registration) -> [u8; 32] {
    crate::sig::eip712_digest(
        &domain_separator(chain_id, registrar),
        &registration_struct_hash(r),
    )
}

/// Signs a registration (viem `signTypedData`): `r || s || v`, v = 27/28, low-s.
pub fn sign_registration(
    key: &IdentityKey,
    chain_id: u64,
    registrar: &Address,
    r: &Registration,
) -> [u8; 65] {
    key.sign_digest(&registration_digest(chain_id, registrar, r))
}

/// Recovers the signer of a registration signature.
pub fn recover_registration(
    chain_id: u64,
    registrar: &Address,
    r: &Registration,
    sig: &[u8],
) -> Result<Address, Error> {
    crate::sig::recover_digest(&registration_digest(chain_id, registrar, r), sig)
}

/// `register(string label, address owner, address resolver, uint64 duration, uint256 deadline,
/// bytes signature)`
pub fn encode_register(r: &Registration, signature: &[u8]) -> Vec<u8> {
    abi::encode_sig(
        REGISTER_SIG,
        &[
            Token::string(&r.label),
            Token::address(&r.owner),
            Token::address(&r.resolver),
            Token::uint(r.duration),
            Token::uint(r.deadline),
            Token::bytes(signature),
        ],
    )
}

/// `nonces(address owner)`
pub fn encode_nonces(owner: &Address) -> Vec<u8> {
    abi::encode_sig(NONCES_SIG, &[Token::address(owner)])
}

/// Return of `nonces`: `uint256` (error if above `u64::MAX`).
pub fn decode_nonce(ret: &[u8]) -> Result<u64, Error> {
    abi::Reader::new(ret).u64(0)
}

/// `available(string label)`
pub fn encode_available(label: &str) -> Vec<u8> {
    abi::encode_sig(AVAILABLE_SIG, &[Token::string(label)])
}

/// `renew(string label, uint64 duration)`
pub fn encode_renew(label: &str, duration: u64) -> Vec<u8> {
    abi::encode_sig(RENEW_SIG, &[Token::string(label), Token::uint(duration)])
}

/// The contract's label rules: 3..=32 bytes of `a-z`, `0-9`, `-`; no leading or trailing
/// `-`; no `--` at byte positions 2..4 (ENSIP-15 CheckHyphen). Every label that passes is
/// already ENSIP-15 normalized.
pub fn validate_label(label: &str) -> Result<(), Error> {
    let b = label.as_bytes();
    if b.len() < 3 {
        return Err(Error::InvalidLabel("too short"));
    }
    if b.len() > 32 {
        return Err(Error::InvalidLabel("too long"));
    }
    if !b
        .iter()
        .all(|&c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-')
    {
        return Err(Error::InvalidLabel("invalid character"));
    }
    if b[0] == b'-' || b[b.len() - 1] == b'-' {
        return Err(Error::InvalidLabel("leading or trailing hyphen"));
    }
    if b[2] == b'-' && b[3] == b'-' {
        return Err(Error::InvalidLabel("hyphens at positions 3 and 4"));
    }
    Ok(())
}
