//! ENSv2 `PermissionedResolver` (namechain `src/resolver/PermissionedResolver.sol`,
//! `libraries/PermissionedResolverLib.sol`) and `VerifiableFactory`
//! (`lib/verifiable-factory/src/IVerifiableFactory.sol`).
//!
//! The setters take the **DNS-encoded name**, not the node.

use alloc::vec::Vec;

use crate::abi::{self, Token, admin, bit, or};
use crate::{Address, Error, U256};

// EACBaseRolesLib
/// `EACBaseRolesLib.ALL_ROLES` (`0x1111…1111`).
pub const ALL_ROLES: U256 = [0x11; 32];
/// `EACBaseRolesLib.ADMIN_ROLES` (`0x1111…` in the upper 128 bits).
pub const ADMIN_ROLES: U256 = {
    let mut w = [0u8; 32];
    let mut i = 0;
    while i < 16 {
        w[i] = 0x11;
        i += 1;
    }
    w
};

// PermissionedResolverLib
pub const ROLE_SET_ADDRESS: U256 = bit(0);
pub const ROLE_SET_ADDRESS_ADMIN: U256 = admin(ROLE_SET_ADDRESS);
pub const ROLE_SET_TEXT: U256 = bit(4);
pub const ROLE_SET_TEXT_ADMIN: U256 = admin(ROLE_SET_TEXT);
pub const ROLE_SET_CONTENTHASH: U256 = bit(8);
pub const ROLE_SET_CONTENTHASH_ADMIN: U256 = admin(ROLE_SET_CONTENTHASH);
pub const ROLE_SET_ABI: U256 = bit(12);
pub const ROLE_SET_ABI_ADMIN: U256 = admin(ROLE_SET_ABI);
pub const ROLE_SET_INTERFACE: U256 = bit(16);
pub const ROLE_SET_INTERFACE_ADMIN: U256 = admin(ROLE_SET_INTERFACE);
pub const ROLE_SET_NAME: U256 = bit(20);
pub const ROLE_SET_NAME_ADMIN: U256 = admin(ROLE_SET_NAME);
pub const ROLE_SET_DATA: U256 = bit(24);
pub const ROLE_SET_DATA_ADMIN: U256 = admin(ROLE_SET_DATA);
pub const ROLE_LINK: U256 = bit(28);
pub const ROLE_LINK_ADMIN: U256 = admin(ROLE_LINK);
pub const ROLE_CAN_NAME: U256 = bit(120);
pub const ROLE_CAN_NAME_ADMIN: U256 = admin(ROLE_CAN_NAME);
pub const ROLE_UPGRADE: U256 = bit(124);
pub const ROLE_UPGRADE_ADMIN: U256 = admin(ROLE_UPGRADE);

/// Every setter role and its admin (address, text, contenthash, ABI, interface, name, data,
/// link).
pub const ALL_SETTER_ROLES: U256 = {
    let mut w = [0u8; 32];
    let roles = [
        ROLE_SET_ADDRESS,
        ROLE_SET_TEXT,
        ROLE_SET_CONTENTHASH,
        ROLE_SET_ABI,
        ROLE_SET_INTERFACE,
        ROLE_SET_NAME,
        ROLE_SET_DATA,
        ROLE_LINK,
    ];
    let mut i = 0;
    while i < roles.len() {
        w = or(w, or(roles[i], admin(roles[i])));
        i += 1;
    }
    w
};

/// `Grant { address account; uint256 roleBitmap; }` (`IEACGrantInitializable.sol`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Grant {
    pub account: Address,
    pub role_bitmap: U256,
}

pub(crate) fn grants_token(grants: &[Grant]) -> Token {
    Token::Array(
        grants
            .iter()
            .map(|g| {
                Token::Tuple(alloc::vec![
                    Token::address(&g.account),
                    Token::u256(&g.role_bitmap)
                ])
            })
            .collect(),
    )
}

pub const SET_TEXT_SIG: &str = "setText(bytes,string,string)";
pub const SET_DATA_SIG: &str = "setData(bytes,string,bytes)";
pub const SET_ADDRESS_SIG: &str = "setAddress(bytes,uint256,bytes)";
pub const SET_CONTENTHASH_SIG: &str = "setContenthash(bytes,bytes)";
pub const MULTICALL_SIG: &str = "multicall(bytes[])";
pub const INITIALIZE_SIG: &str = "initialize((address,uint256)[],bytes[])";
pub const DEPLOY_PROXY_SIG: &str = "deployProxy(address,uint256,bytes)";
pub const PREDICT_PROXY_ADDRESS_SIG: &str = "predictProxyAddress(address,uint256)";

/// `setText(bytes name, string key, string value)`
pub fn encode_set_text(dns_name: &[u8], key: &str, value: &str) -> Vec<u8> {
    abi::encode_sig(
        SET_TEXT_SIG,
        &[
            Token::bytes(dns_name),
            Token::string(key),
            Token::string(value),
        ],
    )
}

/// `setData(bytes name, string key, bytes value)` (ENSIP-24)
pub fn encode_set_data(dns_name: &[u8], key: &str, value: &[u8]) -> Vec<u8> {
    abi::encode_sig(
        SET_DATA_SIG,
        &[
            Token::bytes(dns_name),
            Token::string(key),
            Token::bytes(value),
        ],
    )
}

/// `setAddress(bytes name, uint256 coinType, bytes addressBytes)`
pub fn encode_set_address(dns_name: &[u8], coin_type: u64, address_bytes: &[u8]) -> Vec<u8> {
    abi::encode_sig(
        SET_ADDRESS_SIG,
        &[
            Token::bytes(dns_name),
            Token::uint(coin_type),
            Token::bytes(address_bytes),
        ],
    )
}

/// `setContenthash(bytes name, bytes hash)`
pub fn encode_set_contenthash(dns_name: &[u8], hash: &[u8]) -> Vec<u8> {
    abi::encode_sig(
        SET_CONTENTHASH_SIG,
        &[Token::bytes(dns_name), Token::bytes(hash)],
    )
}

/// `multicall(bytes[] calls)`
pub fn encode_multicall<B: AsRef<[u8]>>(calls: &[B]) -> Vec<u8> {
    crate::resolver::encode_multicall(calls)
}

/// `initialize((address account, uint256 roleBitmap)[] grants, bytes[] calls)`: the proxy
/// initializer, passed as `data` to `VerifiableFactory.deployProxy`.
pub fn encode_initialize<B: AsRef<[u8]>>(grants: &[Grant], calls: &[B]) -> Vec<u8> {
    abi::encode_sig(
        INITIALIZE_SIG,
        &[grants_token(grants), Token::bytes_array(calls)],
    )
}

/// `VerifiableFactory.deployProxy(address implementation, uint256 salt, bytes data)`
pub fn encode_deploy_proxy(implementation: &Address, salt: &U256, data: &[u8]) -> Vec<u8> {
    abi::encode_sig(
        DEPLOY_PROXY_SIG,
        &[
            Token::address(implementation),
            Token::u256(salt),
            Token::bytes(data),
        ],
    )
}

/// `VerifiableFactory.predictProxyAddress(address deployer, uint256 salt)`
pub fn encode_predict_proxy_address(deployer: &Address, salt: &U256) -> Vec<u8> {
    abi::encode_sig(
        PREDICT_PROXY_ADDRESS_SIG,
        &[Token::address(deployer), Token::u256(salt)],
    )
}

/// Return of `deployProxy` / `predictProxyAddress`: `address`.
pub fn decode_address_result(ret: &[u8]) -> Result<Address, Error> {
    abi::decode_address(ret)
}
