//! ENSv2 `PermissionedRegistry` / `UserRegistry` (namechain `src/registry/*`,
//! `registry/libraries/RegistryRolesLib.sol`, `access-control/interfaces/*`).

use alloc::vec::Vec;

use crate::abi::{self, Token, admin, bit};
use crate::permissioned_resolver::{Grant, grants_token};
use crate::{Address, Error, U256, keccak256};

pub const ROLE_REGISTRAR: U256 = bit(0);
pub const ROLE_REGISTRAR_ADMIN: U256 = admin(ROLE_REGISTRAR);
pub const ROLE_REGISTER_RESERVED: U256 = bit(4);
pub const ROLE_REGISTER_RESERVED_ADMIN: U256 = admin(ROLE_REGISTER_RESERVED);
pub const ROLE_SET_PARENT: U256 = bit(8);
pub const ROLE_SET_PARENT_ADMIN: U256 = admin(ROLE_SET_PARENT);
pub const ROLE_UNREGISTER: U256 = bit(12);
pub const ROLE_UNREGISTER_ADMIN: U256 = admin(ROLE_UNREGISTER);
pub const ROLE_RENEW: U256 = bit(16);
pub const ROLE_RENEW_ADMIN: U256 = admin(ROLE_RENEW);
pub const ROLE_SET_SUBREGISTRY: U256 = bit(20);
pub const ROLE_SET_SUBREGISTRY_ADMIN: U256 = admin(ROLE_SET_SUBREGISTRY);
pub const ROLE_SET_RESOLVER: U256 = bit(24);
pub const ROLE_SET_RESOLVER_ADMIN: U256 = admin(ROLE_SET_RESOLVER);
/// `(1 << 28) << 128`
pub const ROLE_CAN_TRANSFER_ADMIN: U256 = admin(bit(28));
/// `1 << 32`
pub const ROLE_WAS_RESERVED: U256 = bit(32);
pub const ROLE_SET_URI: U256 = bit(36);
pub const ROLE_SET_URI_ADMIN: U256 = admin(ROLE_SET_URI);
pub const ROLE_CAN_NAME: U256 = bit(120);
pub const ROLE_CAN_NAME_ADMIN: U256 = admin(ROLE_CAN_NAME);
pub const ROLE_UPGRADE: U256 = bit(124);
pub const ROLE_UPGRADE_ADMIN: U256 = admin(ROLE_UPGRADE);

/// Root roles that violate token emancipation (excludes `ROLE_RENEW`/`ROLE_RENEW_ADMIN`).
pub const UNEMANCIPATED_ROLE_BITMAP: U256 = {
    let parts = [
        ROLE_CAN_TRANSFER_ADMIN,
        ROLE_SET_SUBREGISTRY,
        ROLE_SET_SUBREGISTRY_ADMIN,
        ROLE_SET_RESOLVER,
        ROLE_SET_RESOLVER_ADMIN,
        ROLE_UNREGISTER,
        ROLE_UNREGISTER_ADMIN,
        ROLE_UPGRADE,
        ROLE_UPGRADE_ADMIN,
    ];
    let mut w = [0u8; 32];
    let mut i = 0;
    while i < parts.len() {
        w = crate::abi::or(w, parts[i]);
        i += 1;
    }
    w
};

pub use crate::permissioned_resolver::{ADMIN_ROLES, ALL_ROLES};

pub const REGISTER_SIG: &str = "register(string,address,address,address,uint256,uint64)";
pub const SET_RESOLVER_SIG: &str = "setResolver(uint256,address)";
pub const SET_SUBREGISTRY_SIG: &str = "setSubregistry(uint256,address)";
pub const GET_STATE_SIG: &str = "getState(uint256)";
pub const GRANT_ROOT_ROLES_SIG: &str = "grantRootRoles(uint256,address)";
pub const INITIALIZE_SIG: &str = "initialize((address,uint256)[])";

/// `LibLabel.id(label)` = `uint256(keccak256(bytes(label)))`.
pub fn label_id(label: &str) -> U256 {
    keccak256(label.as_bytes())
}

/// `register(string label, address owner, IRegistry registry, address resolver,
/// uint256 roleBitmap, uint64 expiry)`
pub fn encode_register(
    label: &str,
    owner: &Address,
    subregistry: &Address,
    resolver: &Address,
    role_bitmap: &U256,
    expiry: u64,
) -> Vec<u8> {
    abi::encode_sig(
        REGISTER_SIG,
        &[
            Token::string(label),
            Token::address(owner),
            Token::address(subregistry),
            Token::address(resolver),
            Token::u256(role_bitmap),
            Token::uint(expiry),
        ],
    )
}

/// `setResolver(uint256 anyId, address resolver)`
pub fn encode_set_resolver(any_id: &U256, resolver: &Address) -> Vec<u8> {
    abi::encode_sig(
        SET_RESOLVER_SIG,
        &[Token::u256(any_id), Token::address(resolver)],
    )
}

/// `setSubregistry(uint256 anyId, IRegistry registry)`
pub fn encode_set_subregistry(any_id: &U256, registry: &Address) -> Vec<u8> {
    abi::encode_sig(
        SET_SUBREGISTRY_SIG,
        &[Token::u256(any_id), Token::address(registry)],
    )
}

/// `getState(uint256 anyId)`
pub fn encode_get_state(any_id: &U256) -> Vec<u8> {
    abi::encode_sig(GET_STATE_SIG, &[Token::u256(any_id)])
}

/// `grantRootRoles(uint256 roleBitmap, address account)`
pub fn encode_grant_root_roles(role_bitmap: &U256, account: &Address) -> Vec<u8> {
    abi::encode_sig(
        GRANT_ROOT_ROLES_SIG,
        &[Token::u256(role_bitmap), Token::address(account)],
    )
}

/// `UserRegistry.initialize((address account, uint256 roleBitmap)[] grants)`
pub fn encode_initialize(grants: &[Grant]) -> Vec<u8> {
    abi::encode_sig(INITIALIZE_SIG, &[grants_token(grants)])
}

/// `IPermissionedRegistry.Status`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Available = 0,
    Reserved = 1,
    Registered = 2,
}

/// `IPermissionedRegistry.State`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct State {
    pub status: Status,
    pub expiry: u64,
    pub latest_owner: Address,
    pub token_id: U256,
    pub resource: U256,
}

/// Return of `getState`: the static struct `(uint8 status, uint64 expiry, address latestOwner,
/// uint256 tokenId, uint256 resource)` (5 words). A status above 2 is an error (Solidity
/// would revert decoding the enum).
pub fn decode_state(ret: &[u8]) -> Result<State, Error> {
    let r = abi::Reader::new(ret);
    let status = match r.uint_max(0, 255)? {
        0 => Status::Available,
        1 => Status::Reserved,
        2 => Status::Registered,
        _ => return Err(Error::InvalidAbi),
    };
    Ok(State {
        status,
        expiry: r.u64(1)?,
        latest_owner: r.address(2)?,
        token_id: r.u256(3)?,
        resource: r.u256(4)?,
    })
}

/// Encodes a `State` (for mocks and tests).
pub fn encode_state(s: &State) -> Vec<u8> {
    abi::encode(&[Token::Tuple(alloc::vec![
        Token::uint(s.status as u64),
        Token::uint(s.expiry),
        Token::address(&s.latest_owner),
        Token::u256(&s.token_id),
        Token::u256(&s.resource),
    ])])
}
