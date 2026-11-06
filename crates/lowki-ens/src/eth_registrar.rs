//! ENSv2 `ETHRegistrar` (namechain `src/registrar/ETHRegistrar.sol`): `.eth` commit-reveal,
//! paid in an ERC-20 (on Sepolia, `MockUSDC`), plus the ERC-20 calls it needs.

use alloc::vec::Vec;

use crate::abi::{self, Token};
use crate::{Address, Error, U256, keccak256};

pub const COMMIT_SIG: &str = "commit(bytes32)";
pub const REGISTER_SIG: &str =
    "register(string,address,bytes32,address,address,uint64,address,bytes32)";
pub const IS_AVAILABLE_SIG: &str = "isAvailable(string)";
pub const GET_REGISTER_PRICE_SIG: &str = "getRegisterPrice(string,uint64,address)";
pub const MAKE_COMMITMENT_SIG: &str =
    "makeCommitment(string,address,bytes32,address,address,uint64,bytes32)";
pub const COMMITMENT_AT_SIG: &str = "commitmentAt(bytes32)";
pub const ERC20_APPROVE_SIG: &str = "approve(address,uint256)";
pub const ERC20_BALANCE_OF_SIG: &str = "balanceOf(address)";
pub const ERC20_ALLOWANCE_SIG: &str = "allowance(address,address)";
pub const MOCK_MINT_SIG: &str = "mint(address,uint256)";

/// `ETHRegistrar.makeCommitment`, computed locally:
/// `keccak256(abi.encode(label, owner, secret, subregistry, resolver, duration, referrer))`.
pub fn make_commitment(
    label: &str,
    owner: &Address,
    secret: &[u8; 32],
    subregistry: &Address,
    resolver: &Address,
    duration: u64,
    referrer: &[u8; 32],
) -> [u8; 32] {
    keccak256(&abi::encode(&[
        Token::string(label),
        Token::address(owner),
        Token::b32(secret),
        Token::address(subregistry),
        Token::address(resolver),
        Token::uint(duration),
        Token::b32(referrer),
    ]))
}

/// `makeCommitment(...)` calldata (to cross-check onchain).
pub fn encode_make_commitment(
    label: &str,
    owner: &Address,
    secret: &[u8; 32],
    subregistry: &Address,
    resolver: &Address,
    duration: u64,
    referrer: &[u8; 32],
) -> Vec<u8> {
    abi::encode_sig(
        MAKE_COMMITMENT_SIG,
        &[
            Token::string(label),
            Token::address(owner),
            Token::b32(secret),
            Token::address(subregistry),
            Token::address(resolver),
            Token::uint(duration),
            Token::b32(referrer),
        ],
    )
}

/// `commit(bytes32 commitment)`
pub fn encode_commit(commitment: &[u8; 32]) -> Vec<u8> {
    abi::encode_sig(COMMIT_SIG, &[Token::b32(commitment)])
}

/// `commitmentAt(bytes32)` (returns `uint64`).
pub fn encode_commitment_at(commitment: &[u8; 32]) -> Vec<u8> {
    abi::encode_sig(COMMITMENT_AT_SIG, &[Token::b32(commitment)])
}

/// `register(string label, address owner, bytes32 secret, IRegistry subregistry,
/// address resolver, uint64 duration, IERC20 paymentToken, bytes32 referrer)`
#[allow(clippy::too_many_arguments)]
pub fn encode_register(
    label: &str,
    owner: &Address,
    secret: &[u8; 32],
    subregistry: &Address,
    resolver: &Address,
    duration: u64,
    payment_token: &Address,
    referrer: &[u8; 32],
) -> Vec<u8> {
    abi::encode_sig(
        REGISTER_SIG,
        &[
            Token::string(label),
            Token::address(owner),
            Token::b32(secret),
            Token::address(subregistry),
            Token::address(resolver),
            Token::uint(duration),
            Token::address(payment_token),
            Token::b32(referrer),
        ],
    )
}

/// `isAvailable(string label)`
pub fn encode_is_available(label: &str) -> Vec<u8> {
    abi::encode_sig(IS_AVAILABLE_SIG, &[Token::string(label)])
}

/// `getRegisterPrice(string label, uint64 duration, IERC20 paymentToken)`
pub fn encode_get_register_price(label: &str, duration: u64, payment_token: &Address) -> Vec<u8> {
    abi::encode_sig(
        GET_REGISTER_PRICE_SIG,
        &[
            Token::string(label),
            Token::uint(duration),
            Token::address(payment_token),
        ],
    )
}

/// Return of `getRegisterPrice`: `(uint256 base, uint256 premium)`.
pub fn decode_register_price(ret: &[u8]) -> Result<(U256, U256), Error> {
    let r = abi::Reader::new(ret);
    Ok((r.u256(0)?, r.u256(1)?))
}

/// Return of `isAvailable` (and ERC-20 `approve`): `bool`.
pub fn decode_bool_result(ret: &[u8]) -> Result<bool, Error> {
    abi::decode_bool(ret)
}

/// Return of `commitmentAt`: `uint64`.
pub fn decode_commitment_at(ret: &[u8]) -> Result<u64, Error> {
    abi::Reader::new(ret).u64(0)
}

/// ERC-20 `approve(address spender, uint256 value)`
pub fn encode_erc20_approve(spender: &Address, value: &U256) -> Vec<u8> {
    abi::encode_sig(
        ERC20_APPROVE_SIG,
        &[Token::address(spender), Token::u256(value)],
    )
}

/// ERC-20 `balanceOf(address)`
pub fn encode_erc20_balance_of(account: &Address) -> Vec<u8> {
    abi::encode_sig(ERC20_BALANCE_OF_SIG, &[Token::address(account)])
}

/// ERC-20 `allowance(address owner, address spender)`
pub fn encode_erc20_allowance(owner: &Address, spender: &Address) -> Vec<u8> {
    abi::encode_sig(
        ERC20_ALLOWANCE_SIG,
        &[Token::address(owner), Token::address(spender)],
    )
}

/// `MockUSDC.mint(address to, uint256 amount)` (Sepolia test token; anyone can mint).
pub fn encode_mock_mint(to: &Address, amount: &U256) -> Vec<u8> {
    abi::encode_sig(MOCK_MINT_SIG, &[Token::address(to), Token::u256(amount)])
}
