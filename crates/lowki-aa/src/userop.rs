//! ERC-4337 EntryPoint v0.8 user operations.
//!
//! v0.8 defines `userOpHash` as the EIP-712 typed-data hash of the packed userOp under
//! the domain `{ name: "ERC4337", version: "1", chainId, verifyingContract: entryPoint }`
//! (`EntryPoint.getUserOpHash` / viem `getUserOperationHash({ entryPointVersion: "0.8" })`).

use alloy_primitives::{Address, B256, Bytes, U256, address};
use alloy_sol_types::{Eip712Domain, SolStruct};

/// Canonical ERC-4337 EntryPoint v0.8.
pub const ENTRY_POINT_V08: Address = address!("0x4337084D9E255Ff0702461CF8895CE9E3b5Ff108");

/// EIP-7702 initCode marker (`0x7702` left-aligned in 20 bytes).
pub const INITCODE_EIP7702_MARKER: [u8; 20] = {
    let mut m = [0u8; 20];
    m[0] = 0x77;
    m[1] = 0x02;
    m
};

mod typed {
    alloy_sol_types::sol! {
    /// EIP-712 type of the v0.8 userOp hash (`signature` is not hashed). The struct name
    /// is part of the type hash, so it must be exactly `PackedUserOperation`.
    struct PackedUserOperation {
        address sender;
        uint256 nonce;
        bytes initCode;
        bytes callData;
        bytes32 accountGasLimits;
        uint256 preVerificationGas;
        bytes32 gasFees;
        bytes paymasterAndData;
    }
    }
}

/// On-chain `PackedUserOperation` (v0.7/v0.8 layout).
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct PackedUserOperation {
    pub sender: Address,
    pub nonce: U256,
    pub init_code: Bytes,
    pub call_data: Bytes,
    pub account_gas_limits: B256,
    pub pre_verification_gas: U256,
    pub gas_fees: B256,
    pub paymaster_and_data: Bytes,
    pub signature: Bytes,
}

/// `bytes32(uint128(hi) << 128 | uint128(lo))`.
fn pack_u128_pair(hi: u128, lo: u128) -> B256 {
    let mut out = [0u8; 32];
    out[..16].copy_from_slice(&hi.to_be_bytes());
    out[16..].copy_from_slice(&lo.to_be_bytes());
    B256::from(out)
}

/// `accountGasLimits = verificationGasLimit (16 bytes) || callGasLimit (16 bytes)`.
pub fn pack_account_gas_limits(verification_gas_limit: u128, call_gas_limit: u128) -> B256 {
    pack_u128_pair(verification_gas_limit, call_gas_limit)
}

/// `gasFees = maxPriorityFeePerGas (16 bytes) || maxFeePerGas (16 bytes)`.
pub fn pack_gas_fees(max_priority_fee_per_gas: u128, max_fee_per_gas: u128) -> B256 {
    pack_u128_pair(max_priority_fee_per_gas, max_fee_per_gas)
}

/// `paymasterAndData = paymaster (20) || paymasterVerificationGasLimit (16) ||
/// paymasterPostOpGasLimit (16) || paymasterData`.
pub fn pack_paymaster_and_data(
    paymaster: Address,
    verification_gas_limit: u128,
    post_op_gas_limit: u128,
    paymaster_data: &[u8],
) -> Bytes {
    let mut out = Vec::with_capacity(52 + paymaster_data.len());
    out.extend_from_slice(paymaster.as_slice());
    out.extend_from_slice(&verification_gas_limit.to_be_bytes());
    out.extend_from_slice(&post_op_gas_limit.to_be_bytes());
    out.extend_from_slice(paymaster_data);
    out.into()
}

/// The EIP-712 domain of the EntryPoint v0.8 userOp hash.
pub fn eip712_domain(chain_id: u64, entry_point: Address) -> Eip712Domain {
    Eip712Domain::new(
        Some("ERC4337".into()),
        Some("1".into()),
        Some(U256::from(chain_id)),
        Some(entry_point),
        None,
    )
}

impl PackedUserOperation {
    /// EIP-712 hash of this packed op exactly as given (no 7702 initCode overlay: pass
    /// the hash-form initCode; see [`UserOperation::init_code_for_hash`]).
    pub fn hash(&self, chain_id: u64, entry_point: Address) -> B256 {
        let s = typed::PackedUserOperation {
            sender: self.sender,
            nonce: self.nonce,
            initCode: self.init_code.clone(),
            callData: self.call_data.clone(),
            accountGasLimits: self.account_gas_limits,
            preVerificationGas: self.pre_verification_gas,
            gasFees: self.gas_fees,
            paymasterAndData: self.paymaster_and_data.clone(),
        };
        s.eip712_signing_hash(&eip712_domain(chain_id, entry_point))
    }
}

/// Paymaster section of an unpacked userOp.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PaymasterFields {
    pub paymaster: Address,
    pub verification_gas_limit: u128,
    pub post_op_gas_limit: u128,
    pub data: Bytes,
}

/// Unpacked v0.8 user operation (the `eth_sendUserOperation` field set).
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct UserOperation {
    pub sender: Address,
    pub nonce: U256,
    /// `factory`: a 20-byte address, or the 7702 marker in viem's short (`0x7702`) or
    /// long (`0x7702` + 18 zero bytes) form. `None` = no initCode.
    pub factory: Option<Bytes>,
    pub factory_data: Bytes,
    pub call_data: Bytes,
    pub call_gas_limit: u128,
    pub verification_gas_limit: u128,
    pub pre_verification_gas: U256,
    pub max_fee_per_gas: u128,
    pub max_priority_fee_per_gas: u128,
    pub paymaster: Option<PaymasterFields>,
    pub signature: Bytes,
    /// The EIP-7702 delegate (authorization `address`), used to overlay the 7702 marker
    /// in the hashed initCode.
    pub eip7702_delegate: Option<Address>,
}

impl UserOperation {
    fn is_7702_marker(f: &[u8]) -> bool {
        f == [0x77, 0x02] || f == INITCODE_EIP7702_MARKER
    }

    /// `initCode` as sent on-chain (`factory || factoryData`, or empty).
    pub fn init_code(&self) -> Bytes {
        match &self.factory {
            None => Bytes::new(),
            Some(f) => [f.as_ref(), self.factory_data.as_ref()].concat().into(),
        }
    }

    /// `initCode` as hashed: with the 7702 marker, the delegate address replaces the
    /// marker (`delegate || factoryData`; just the 20-byte marker if no delegate is
    /// known) — viem `getInitCode(..., { forHash: true })`.
    pub fn init_code_for_hash(&self) -> Bytes {
        match &self.factory {
            Some(f) if Self::is_7702_marker(f) => match self.eip7702_delegate {
                Some(d) => [d.as_slice(), self.factory_data.as_ref()].concat().into(),
                None => Bytes::copy_from_slice(&INITCODE_EIP7702_MARKER),
            },
            _ => self.init_code(),
        }
    }

    pub fn account_gas_limits(&self) -> B256 {
        pack_account_gas_limits(self.verification_gas_limit, self.call_gas_limit)
    }

    pub fn gas_fees(&self) -> B256 {
        pack_gas_fees(self.max_priority_fee_per_gas, self.max_fee_per_gas)
    }

    pub fn paymaster_and_data(&self) -> Bytes {
        match &self.paymaster {
            None => Bytes::new(),
            Some(p) => pack_paymaster_and_data(
                p.paymaster,
                p.verification_gas_limit,
                p.post_op_gas_limit,
                &p.data,
            ),
        }
    }

    /// The on-chain `PackedUserOperation` (viem `toPackedUserOperation`).
    pub fn pack(&self) -> PackedUserOperation {
        PackedUserOperation {
            sender: self.sender,
            nonce: self.nonce,
            init_code: self.init_code(),
            call_data: self.call_data.clone(),
            account_gas_limits: self.account_gas_limits(),
            pre_verification_gas: self.pre_verification_gas,
            gas_fees: self.gas_fees(),
            paymaster_and_data: self.paymaster_and_data(),
            signature: self.signature.clone(),
        }
    }

    /// The v0.8 `userOpHash`.
    pub fn hash(&self, chain_id: u64, entry_point: Address) -> B256 {
        let mut packed = self.pack();
        packed.init_code = self.init_code_for_hash();
        packed.hash(chain_id, entry_point)
    }
}

/// Free-function form of [`UserOperation::hash`].
pub fn user_op_hash(op: &UserOperation, chain_id: u64, entry_point: Address) -> B256 {
    op.hash(chain_id, entry_point)
}
