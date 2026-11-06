//! Kohaku Privacy Pools paymaster encodings and the signed sponsored-withdrawal userOp.
//!
//! Ports `packages/privacy-pools/src/paymaster/{adapter-data,utils}.ts` and
//! `utils/encoding.utils.ts#toWithdrawProof`.

use alloy_primitives::{Address, B256, Bytes, U256};
use alloy_sol_types::{SolCall, SolValue, sol};
use serde::{Deserialize, Serialize};

use crate::Error;
use crate::eip7702::{Authorization, SerializedAuth, SignedAuthorization};
use crate::fee::GasLimits;
use crate::hex_quantity;
use crate::signer::EphemeralSigner;
use crate::userop::{ENTRY_POINT_V08, PaymasterFields, UserOperation};

pub use crate::config::SIMPLE_7702_IMPLEMENTATION;

mod abi {
    alloy_sol_types::sol! {
        struct FeeData { address recipient; address feeRecipient; uint256 fee; address refundRecipient; }
        struct Withdrawal { address processooor; bytes data; }
        struct WithdrawProof { uint256[2] pA; uint256[2][2] pB; uint256[2] pC; uint256[8] pubSignals; }
        struct AdapterData { Withdrawal withdrawal; WithdrawProof proof; }
        struct PaymasterData { address adapter; bytes adapterData; }
        function withdraw(Withdrawal _withdrawal, WithdrawProof _proof);
    }
}

sol! {
    /// Simple7702Account (EntryPoint v0.8) batch call item.
    struct Call7702 { address target; uint256 value; bytes data; }
    function execute(address target, uint256 value, bytes data);
    function executeBatch(Call7702[] calls);
}

/// The PrivacyPoolsFeeAdapter's `FeeData` (`withdrawal.data` of the sponsored note).
/// `fee` is an absolute amount in the pool asset; `fee_recipient` must be the paymaster;
/// `refund_recipient = 0x0` disables the postOp refund.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FeeData {
    pub recipient: Address,
    pub fee_recipient: Address,
    pub fee: U256,
    pub refund_recipient: Address,
}

/// Privacy Pools `IPrivacyPool.Withdrawal`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Withdrawal {
    pub processooor: Address,
    pub data: Bytes,
}

/// On-chain `ProofLib.WithdrawProof` (`pB` already in the Solidity verifier's swapped
/// coordinate order). 8 public signals for the withdraw circuit.
#[derive(Clone, Debug, PartialEq, Eq, Default)]
pub struct WithdrawProof {
    pub p_a: [U256; 2],
    pub p_b: [[U256; 2]; 2],
    pub p_c: [U256; 2],
    pub pub_signals: [U256; 8],
}

fn parse_big(s: &str) -> Result<U256, Error> {
    // JS BigInt(): decimal or 0x-hex.
    s.trim().parse::<U256>().map_err(|_| Error::InvalidProof)
}

impl WithdrawProof {
    /// Kohaku `toWithdrawProof`: from snarkjs `{ pi_a, pi_b, pi_c }` + `publicSignals`
    /// (decimal strings), swapping each `pi_b` row (`[x1, x0]`).
    pub fn from_snarkjs<S: AsRef<str>>(
        pi_a: &[S],
        pi_b: &[Vec<S>],
        pi_c: &[S],
        public_signals: &[S],
    ) -> Result<Self, Error> {
        if public_signals.len() != 8 {
            return Err(Error::InvalidProof);
        }
        let at = |v: &[S], i: usize| {
            v.get(i)
                .ok_or(Error::InvalidProof)
                .and_then(|x| parse_big(x.as_ref()))
        };
        let row = |i: usize| -> Result<[U256; 2], Error> {
            let r = pi_b.get(i).ok_or(Error::InvalidProof)?;
            Ok([at(r, 1)?, at(r, 0)?])
        };
        let mut pub_signals = [U256::ZERO; 8];
        for (i, s) in public_signals.iter().enumerate() {
            pub_signals[i] = parse_big(s.as_ref())?;
        }
        Ok(Self {
            p_a: [at(pi_a, 0)?, at(pi_a, 1)?],
            p_b: [row(0)?, row(1)?],
            p_c: [at(pi_c, 0)?, at(pi_c, 1)?],
            pub_signals,
        })
    }

    fn to_abi(&self) -> abi::WithdrawProof {
        abi::WithdrawProof {
            pA: self.p_a,
            pB: self.p_b,
            pC: self.p_c,
            pubSignals: self.pub_signals,
        }
    }
}

impl Withdrawal {
    fn to_abi(&self) -> abi::Withdrawal {
        abi::Withdrawal {
            processooor: self.processooor,
            data: self.data.clone(),
        }
    }
}

/// `abi.encode(FeeData{recipient, feeRecipient, fee, refundRecipient})`.
pub fn encode_fee_data(fee_data: &FeeData) -> Bytes {
    abi::FeeData {
        recipient: fee_data.recipient,
        feeRecipient: fee_data.fee_recipient,
        fee: fee_data.fee,
        refundRecipient: fee_data.refund_recipient,
    }
    .abi_encode()
    .into()
}

/// `abi.encode(AdapterData{Withdrawal withdrawal; WithdrawProof proof})`.
pub fn encode_privacy_pool_adapter_data(withdrawal: &Withdrawal, proof: &WithdrawProof) -> Bytes {
    abi::AdapterData {
        withdrawal: withdrawal.to_abi(),
        proof: proof.to_abi(),
    }
    .abi_encode()
    .into()
}

/// userOp `paymasterData`: `abi.encode(PaymasterData{address adapter; bytes adapterData})`.
pub fn encode_paymaster_data(adapter: Address, adapter_data: &[u8]) -> Bytes {
    abi::PaymasterData {
        adapter,
        adapterData: Bytes::copy_from_slice(adapter_data),
    }
    .abi_encode()
    .into()
}

/// `pool.withdraw(withdrawal, proof)` calldata (batch extra notes, processooor = sender).
pub fn encode_pool_withdraw(withdrawal: &Withdrawal, proof: &WithdrawProof) -> Bytes {
    abi::withdrawCall {
        _withdrawal: withdrawal.to_abi(),
        _proof: proof.to_abi(),
    }
    .abi_encode()
    .into()
}

/// One execution-phase call (Kohaku `TxData`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Call {
    pub to: Address,
    pub value: U256,
    pub data: Bytes,
}

/// The Simple7702 userOp `callData`: empty for no calls, `execute(target,value,data)` for
/// one, `executeBatch((target,value,data)[])` for several.
pub fn encode_execution_call_data(calls: &[Call]) -> Bytes {
    match calls {
        [] => Bytes::new(),
        [c] => executeCall {
            target: c.to,
            value: c.value,
            data: c.data.clone(),
        }
        .abi_encode()
        .into(),
        many => executeBatchCall {
            calls: many
                .iter()
                .map(|c| Call7702 {
                    target: c.to,
                    value: c.value,
                    data: c.data.clone(),
                })
                .collect(),
        }
        .abi_encode()
        .into(),
    }
}

/// Inputs of Kohaku `buildSignedUserOp`.
#[derive(Clone, Debug)]
pub struct BuildSignedUserOpParams {
    pub chain_id: u64,
    pub paymaster_address: Address,
    pub paymaster_data: Bytes,
    pub gas: GasLimits,
    pub max_fee_per_gas: u128,
    pub max_priority_fee_per_gas: u128,
    /// EntryPoint nonce (Kohaku default 0 for a fresh sender).
    pub nonce: U256,
    /// EOA transaction count the 7702 authorization binds to; `None` = use `nonce`
    /// (Kohaku: `Number(authorizationNonce ?? nonce)`).
    pub authorization_nonce: Option<u64>,
    /// Execution-phase calls. `None` and `Some(vec![])` both give callData `0x`.
    pub tail_calls: Option<Vec<Call>>,
}

/// A fully built, signed, sponsored userOp (+ its 7702 authorization).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SignedUserOperation {
    pub user_operation: UserOperation,
    pub authorization: SignedAuthorization,
    /// The v0.8 hash that was signed (EntryPoint v0.8 at `ENTRY_POINT_V08`).
    pub user_op_hash: B256,
}

/// `eth_sendUserOperation` JSON (Kohaku `SerializedUserOperation`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SerializedUserOperation {
    pub sender: String,
    pub nonce: String,
    pub call_data: String,
    pub call_gas_limit: String,
    pub verification_gas_limit: String,
    pub pre_verification_gas: String,
    pub max_fee_per_gas: String,
    pub max_priority_fee_per_gas: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub paymaster: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub paymaster_verification_gas_limit: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub paymaster_post_op_gas_limit: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub paymaster_data: Option<String>,
    pub signature: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub eip7702_auth: Option<SerializedAuth>,
}

impl SignedUserOperation {
    /// Serialize to the bundler JSON shape, identical to Kohaku's output.
    pub fn to_serialized(&self) -> SerializedUserOperation {
        let op = &self.user_operation;
        let pm = op.paymaster.as_ref();
        SerializedUserOperation {
            sender: op.sender.to_checksum(None),
            nonce: hex_quantity(op.nonce),
            call_data: op.call_data.to_string(),
            call_gas_limit: hex_quantity(op.call_gas_limit),
            verification_gas_limit: hex_quantity(op.verification_gas_limit),
            pre_verification_gas: hex_quantity(op.pre_verification_gas),
            max_fee_per_gas: hex_quantity(op.max_fee_per_gas),
            max_priority_fee_per_gas: hex_quantity(op.max_priority_fee_per_gas),
            paymaster: pm.map(|p| p.paymaster.to_checksum(None)),
            paymaster_verification_gas_limit: pm.map(|p| hex_quantity(p.verification_gas_limit)),
            paymaster_post_op_gas_limit: pm.map(|p| hex_quantity(p.post_op_gas_limit)),
            paymaster_data: pm.map(|p| p.data.to_string()),
            signature: op.signature.to_string(),
            eip7702_auth: Some(self.authorization.to_serialized()),
        }
    }

    /// `callData.len() + paymasterData.len()`, the payload size used by
    /// [`crate::fee::assert_gas_limits_within_bounds`].
    pub fn payload_len(&self) -> usize {
        let op = &self.user_operation;
        op.call_data.len() + op.paymaster.as_ref().map_or(0, |p| p.data.len())
    }
}

/// `Number.MAX_SAFE_INTEGER`.
const MAX_SAFE_INTEGER: u64 = (1 << 53) - 1;

/// Kohaku `buildSignedUserOp`: sender = the ephemeral EOA, 7702-delegated to
/// [`SIMPLE_7702_IMPLEMENTATION`]; no initCode/factory; the owner signs the EntryPoint
/// v0.8 EIP-712 userOp hash (what `Simple7702Account.validateUserOp` checks via
/// `ECDSA.recover(userOpHash, signature) == address(this)`).
pub fn build_signed_user_op(
    signer: &EphemeralSigner,
    params: &BuildSignedUserOpParams,
) -> Result<SignedUserOperation, Error> {
    let call_data = encode_execution_call_data(params.tail_calls.as_deref().unwrap_or(&[]));

    let auth_nonce = match params.authorization_nonce {
        Some(n) => n,
        None => {
            if params.nonce > U256::from(MAX_SAFE_INTEGER) {
                return Err(Error::UnsafeAuthorizationNonce);
            }
            params.nonce.to::<u64>()
        }
    };
    if auth_nonce > MAX_SAFE_INTEGER {
        return Err(Error::UnsafeAuthorizationNonce);
    }
    let authorization = Authorization {
        chain_id: U256::from(params.chain_id),
        address: SIMPLE_7702_IMPLEMENTATION,
        nonce: auth_nonce,
    }
    .sign(signer)?;

    let mut user_operation = UserOperation {
        sender: signer.address(),
        nonce: params.nonce,
        factory: None,
        factory_data: Bytes::new(),
        call_data,
        call_gas_limit: params.gas.call_gas_limit,
        verification_gas_limit: params.gas.verification_gas_limit,
        pre_verification_gas: U256::from(params.gas.pre_verification_gas),
        max_fee_per_gas: params.max_fee_per_gas,
        max_priority_fee_per_gas: params.max_priority_fee_per_gas,
        paymaster: Some(PaymasterFields {
            paymaster: params.paymaster_address,
            verification_gas_limit: params.gas.paymaster_verification_gas_limit,
            post_op_gas_limit: params.gas.paymaster_post_op_gas_limit,
            data: params.paymaster_data.clone(),
        }),
        signature: Bytes::new(),
        eip7702_delegate: None,
    };
    let user_op_hash = user_operation.hash(params.chain_id, ENTRY_POINT_V08);
    let sig = signer.sign_hash(&user_op_hash)?;
    user_operation.signature = Bytes::copy_from_slice(&sig.to_bytes65());

    Ok(SignedUserOperation {
        user_operation,
        authorization,
        user_op_hash,
    })
}
