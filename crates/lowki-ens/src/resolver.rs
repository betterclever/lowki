//! Resolver profile calldata and return decoding (ENSIP-1 `addr`, ENSIP-9 multicoin `addr`,
//! ENSIP-5 `text`, ENSIP-7 `contenthash`, ENSIP-24 `data`, `IMulticallable.multicall`).
//! Signatures from ens-contracts `resolvers/profiles/*.sol` and `resolvers/IMulticallable.sol`.

use alloc::string::String;
use alloc::vec::Vec;

use crate::abi::{self, Token};
use crate::{Address, Error};

pub const ADDR_SIG: &str = "addr(bytes32)";
pub const ADDR_COIN_SIG: &str = "addr(bytes32,uint256)";
pub const TEXT_SIG: &str = "text(bytes32,string)";
pub const DATA_SIG: &str = "data(bytes32,string)";
pub const CONTENTHASH_SIG: &str = "contenthash(bytes32)";
pub const MULTICALL_SIG: &str = "multicall(bytes[])";

/// `0x3b3b57de`
pub const ADDR_SELECTOR: [u8; 4] = [0x3b, 0x3b, 0x57, 0xde];
/// `0xf1cb7e06`
pub const ADDR_COIN_SELECTOR: [u8; 4] = [0xf1, 0xcb, 0x7e, 0x06];
/// `0x59d1d43c`
pub const TEXT_SELECTOR: [u8; 4] = [0x59, 0xd1, 0xd4, 0x3c];
/// `0xecbfada3`
pub const DATA_SELECTOR: [u8; 4] = [0xec, 0xbf, 0xad, 0xa3];
/// `0xbc1c58d1`
pub const CONTENTHASH_SELECTOR: [u8; 4] = [0xbc, 0x1c, 0x58, 0xd1];
/// `0xac9650d8`
pub const MULTICALL_SELECTOR: [u8; 4] = [0xac, 0x96, 0x50, 0xd8];

/// ENSIP-9 coin type of Ethereum mainnet.
pub const COIN_TYPE_ETH: u64 = 60;
/// ENSIP-19 default EVM coin type (`0x80000000`).
pub const COIN_TYPE_DEFAULT: u64 = 0x8000_0000;

/// ENSIP-11 coin type of an EVM chain: 60 for chain 1, else `0x80000000 | chain_id`.
pub fn evm_coin_type(chain_id: u64) -> u64 {
    if chain_id == 1 {
        COIN_TYPE_ETH
    } else {
        COIN_TYPE_DEFAULT | chain_id
    }
}

/// `addr(bytes32 node)`
pub fn encode_addr(node: &[u8; 32]) -> Vec<u8> {
    abi::encode_call(ADDR_SELECTOR, &[Token::b32(node)])
}

/// `addr(bytes32 node, uint256 coinType)`
pub fn encode_addr_coin(node: &[u8; 32], coin_type: u64) -> Vec<u8> {
    abi::encode_call(
        ADDR_COIN_SELECTOR,
        &[Token::b32(node), Token::uint(coin_type)],
    )
}

/// `text(bytes32 node, string key)`
pub fn encode_text(node: &[u8; 32], key: &str) -> Vec<u8> {
    abi::encode_call(TEXT_SELECTOR, &[Token::b32(node), Token::string(key)])
}

/// `data(bytes32 node, string key)` (ENSIP-24)
pub fn encode_data(node: &[u8; 32], key: &str) -> Vec<u8> {
    abi::encode_call(DATA_SELECTOR, &[Token::b32(node), Token::string(key)])
}

/// `contenthash(bytes32 node)`
pub fn encode_contenthash(node: &[u8; 32]) -> Vec<u8> {
    abi::encode_call(CONTENTHASH_SELECTOR, &[Token::b32(node)])
}

/// `multicall(bytes[] calls)`
pub fn encode_multicall<B: AsRef<[u8]>>(calls: &[B]) -> Vec<u8> {
    abi::encode_call(MULTICALL_SELECTOR, &[Token::bytes_array(calls)])
}

/// Return of `addr(bytes32)`: `address`.
pub fn decode_addr_result(ret: &[u8]) -> Result<Address, Error> {
    abi::decode_address(ret)
}

/// Return of `addr(bytes32,uint256)`: `bytes` (20 bytes for EVM chains, empty if unset).
pub fn decode_addr_coin_result(ret: &[u8]) -> Result<Vec<u8>, Error> {
    abi::decode_bytes(ret)
}

/// Return of `text(bytes32,string)`: `string`.
pub fn decode_text_result(ret: &[u8]) -> Result<String, Error> {
    abi::decode_string(ret)
}

/// Return of `data(bytes32,string)`: `bytes`.
pub fn decode_data_result(ret: &[u8]) -> Result<Vec<u8>, Error> {
    abi::decode_bytes(ret)
}

/// Return of `contenthash(bytes32)`: `bytes`.
pub fn decode_contenthash_result(ret: &[u8]) -> Result<Vec<u8>, Error> {
    abi::decode_bytes(ret)
}

/// Return of `multicall(bytes[])`: `bytes[]`.
pub fn decode_multicall_result(ret: &[u8]) -> Result<Vec<Vec<u8>>, Error> {
    abi::decode_bytes_array(ret)
}

/// Decodes multicall calldata `multicall(bytes[])` into its calls.
pub fn decode_multicall_calldata(data: &[u8]) -> Result<Vec<Vec<u8>>, Error> {
    abi::expect_selector(data, MULTICALL_SELECTOR)?.bytes_array(0)
}
