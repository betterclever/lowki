//! UniversalResolver (ens-contracts `universalResolver/IUniversalResolver.sol`,
//! `AbstractUniversalResolver.sol`; ENSv2 `UniversalResolverV2` inherits both).

use alloc::string::String;
use alloc::vec::Vec;

use crate::abi::{self, Token};
use crate::{Address, Error};

pub const RESOLVE_SIG: &str = "resolve(bytes,bytes)";
pub const RESOLVE_WITH_GATEWAYS_SIG: &str = "resolveWithGateways(bytes,bytes,string[])";

/// `resolve(bytes,bytes)` = `0x9061b923` (also ENSIP-10 `IExtendedResolver.resolve`).
pub const RESOLVE_SELECTOR: [u8; 4] = [0x90, 0x61, 0xb9, 0x23];
/// Selector of `resolveWithGateways(bytes,bytes,string[])`.
pub fn resolve_with_gateways_selector() -> [u8; 4] {
    abi::selector(RESOLVE_WITH_GATEWAYS_SIG)
}

pub const RESOLVER_NOT_FOUND_SIG: &str = "ResolverNotFound(bytes)";
pub const RESOLVER_NOT_CONTRACT_SIG: &str = "ResolverNotContract(bytes,address)";
pub const UNSUPPORTED_RESOLVER_PROFILE_SIG: &str = "UnsupportedResolverProfile(bytes4)";
pub const RESOLVER_ERROR_SIG: &str = "ResolverError(bytes)";
pub const REVERSE_ADDRESS_MISMATCH_SIG: &str = "ReverseAddressMismatch(string,bytes)";
pub const HTTP_ERROR_SIG: &str = "HttpError(uint16,string)";
pub const ERROR_STRING_SIG: &str = "Error(string)";
pub const PANIC_SIG: &str = "Panic(uint256)";

/// `resolve(bytes name, bytes data)`; `name` is DNS-encoded.
pub fn encode_resolve(dns_name: &[u8], data: &[u8]) -> Vec<u8> {
    abi::encode_call(
        RESOLVE_SELECTOR,
        &[Token::bytes(dns_name), Token::bytes(data)],
    )
}

/// `resolveWithGateways(bytes name, bytes data, string[] gateways)` (what viem's
/// `getEnsAddress`/`getEnsText` call, with `gateways = ["x-batch-gateway:true"]`).
pub fn encode_resolve_with_gateways<S: AsRef<str>>(
    dns_name: &[u8],
    data: &[u8],
    gateways: &[S],
) -> Vec<u8> {
    abi::encode_call(
        resolve_with_gateways_selector(),
        &[
            Token::bytes(dns_name),
            Token::bytes(data),
            Token::string_array(gateways),
        ],
    )
}

/// Return of `resolve` / `resolveWithGateways`: `(bytes result, address resolver)`.
pub fn decode_resolve_result(ret: &[u8]) -> Result<(Vec<u8>, Address), Error> {
    let r = abi::Reader::new(ret);
    Ok((r.bytes(0)?.to_vec(), r.address(1)?))
}

/// Encodes `(bytes result, address resolver)` (for gateways and tests).
pub fn encode_resolve_result(result: &[u8], resolver: &Address) -> Vec<u8> {
    abi::encode(&[Token::bytes(result), Token::address(resolver)])
}

/// A decoded UniversalResolver revert.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UrError {
    ResolverNotFound {
        name: Vec<u8>,
    },
    ResolverNotContract {
        name: Vec<u8>,
        resolver: Address,
    },
    UnsupportedResolverProfile {
        selector: [u8; 4],
    },
    /// The resolver reverted; `error_data` is its raw revert data.
    ResolverError {
        error_data: Vec<u8>,
    },
    ReverseAddressMismatch {
        primary: String,
        primary_address: Vec<u8>,
    },
    HttpError {
        status: u16,
        message: String,
    },
    /// Solidity `Error(string)`.
    Error(String),
    /// Solidity `Panic(uint256)`.
    Panic([u8; 32]),
    /// Unknown selector or malformed arguments (raw revert data).
    Unknown(Vec<u8>),
}

/// Decodes UniversalResolver revert data. Never fails: unknown or malformed data is
/// [`UrError::Unknown`].
pub fn decode_ur_error(revert: &[u8]) -> UrError {
    try_decode(revert).unwrap_or_else(|_| UrError::Unknown(revert.to_vec()))
}

fn try_decode(revert: &[u8]) -> Result<UrError, Error> {
    let (sel, r) = abi::split_selector(revert)?;
    let is = |sig: &str| sel == abi::selector(sig);
    Ok(if is(RESOLVER_NOT_FOUND_SIG) {
        UrError::ResolverNotFound {
            name: r.bytes(0)?.to_vec(),
        }
    } else if is(RESOLVER_NOT_CONTRACT_SIG) {
        UrError::ResolverNotContract {
            name: r.bytes(0)?.to_vec(),
            resolver: r.address(1)?,
        }
    } else if is(UNSUPPORTED_RESOLVER_PROFILE_SIG) {
        UrError::UnsupportedResolverProfile {
            selector: r.bytes4(0)?,
        }
    } else if is(RESOLVER_ERROR_SIG) {
        UrError::ResolverError {
            error_data: r.bytes(0)?.to_vec(),
        }
    } else if is(REVERSE_ADDRESS_MISMATCH_SIG) {
        UrError::ReverseAddressMismatch {
            primary: r.string(0)?.into(),
            primary_address: r.bytes(1)?.to_vec(),
        }
    } else if is(HTTP_ERROR_SIG) {
        UrError::HttpError {
            status: r.uint_max(0, u16::MAX as u64)? as u16,
            message: r.string(1)?.into(),
        }
    } else if is(ERROR_STRING_SIG) {
        UrError::Error(r.string(0)?.into())
    } else if is(PANIC_SIG) {
        UrError::Panic(r.u256(0)?)
    } else {
        return Err(Error::UnexpectedSelector);
    })
}

/// Encodes a UniversalResolver error (for gateways, mocks and tests).
pub fn encode_ur_error(e: &UrError) -> Vec<u8> {
    match e {
        UrError::ResolverNotFound { name } => {
            abi::encode_sig(RESOLVER_NOT_FOUND_SIG, &[Token::bytes(name)])
        }
        UrError::ResolverNotContract { name, resolver } => abi::encode_sig(
            RESOLVER_NOT_CONTRACT_SIG,
            &[Token::bytes(name), Token::address(resolver)],
        ),
        UrError::UnsupportedResolverProfile { selector } => {
            abi::encode_sig(UNSUPPORTED_RESOLVER_PROFILE_SIG, &[Token::bytes4(selector)])
        }
        UrError::ResolverError { error_data } => {
            abi::encode_sig(RESOLVER_ERROR_SIG, &[Token::bytes(error_data)])
        }
        UrError::ReverseAddressMismatch {
            primary,
            primary_address,
        } => abi::encode_sig(
            REVERSE_ADDRESS_MISMATCH_SIG,
            &[Token::string(primary), Token::bytes(primary_address)],
        ),
        UrError::HttpError { status, message } => abi::encode_sig(
            HTTP_ERROR_SIG,
            &[Token::uint(*status as u64), Token::string(message)],
        ),
        UrError::Error(m) => abi::encode_sig(ERROR_STRING_SIG, &[Token::string(m)]),
        UrError::Panic(c) => abi::encode_sig(PANIC_SIG, &[Token::u256(c)]),
        UrError::Unknown(d) => d.clone(),
    }
}
