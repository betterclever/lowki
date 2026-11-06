//! Shield (deposit) transactions, as Kohaku's `prepareNativeShield` /
//! `prepareErc20Shield` (`account/tx/shield.ts`) build them.

use alloy_primitives::{Address, Bytes, U256};
use alloy_sol_types::SolCall;

mod abi {
    alloy_sol_types::sol! {
        interface NativeEntrypoint { function deposit(uint256 _precommitment) payable returns (uint256 _commitment); }
        interface Erc20Entrypoint { function deposit(address _asset, uint256 _value, uint256 _precommitment) returns (uint256 _commitment); }
        interface IERC20 { function approve(address spender, uint256 amount) returns (bool); }
    }
}

/// A plain transaction request (Kohaku `TxData`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TxData {
    pub to: Address,
    pub data: Bytes,
    pub value: U256,
}

/// `Entrypoint.deposit(uint256 precommitment)` with `value = amount` (ETH pool).
pub fn prepare_native_shield(precommitment: U256, amount: U256, entrypoint: Address) -> TxData {
    let data = abi::NativeEntrypoint::depositCall {
        _precommitment: precommitment,
    }
    .abi_encode();
    TxData {
        to: entrypoint,
        data: data.into(),
        value: amount,
    }
}

/// `Entrypoint.deposit(address asset, uint256 value, uint256 precommitment)`, value 0.
/// Requires a prior [`erc20_approve`] of `amount` to the entrypoint (Kohaku's PP plugin
/// does not emit the approve itself).
pub fn prepare_erc20_shield(
    precommitment: U256,
    amount: U256,
    token: Address,
    entrypoint: Address,
) -> TxData {
    let data = abi::Erc20Entrypoint::depositCall {
        _asset: token,
        _value: amount,
        _precommitment: precommitment,
    }
    .abi_encode();
    TxData {
        to: entrypoint,
        data: data.into(),
        value: U256::ZERO,
    }
}

/// `token.approve(spender, amount)`.
pub fn erc20_approve(token: Address, spender: Address, amount: U256) -> TxData {
    let data = abi::IERC20::approveCall { spender, amount }.abi_encode();
    TxData {
        to: token,
        data: data.into(),
        value: U256::ZERO,
    }
}

/// Kohaku `buildDepositPayload`: native when `asset == E_ADDRESS`, else ERC-20.
/// For ERC-20 returns `[approve, deposit]`; for ETH `[deposit]`.
pub fn shield_transactions(
    precommitment: U256,
    asset: Address,
    amount: U256,
    entrypoint: Address,
) -> Vec<TxData> {
    if asset == crate::config::E_ADDRESS {
        vec![prepare_native_shield(precommitment, amount, entrypoint)]
    } else {
        vec![
            erc20_approve(asset, entrypoint, amount),
            prepare_erc20_shield(precommitment, amount, asset, entrypoint),
        ]
    }
}
