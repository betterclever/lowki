//! Privacy Pools v1: note secrets (Kohaku `SignatureSecretManager`), commitments, shield
//! calldata, Groth16 proving (WebGPU with CPU fallback), and the paymaster withdraw userOp.
//! Note secrets (nullifier, salt) and the account identity key never leave this module;
//! only public hashes, proofs and signed operations are returned.

use alloy_primitives::{Address, Bytes, U256};
use lowki_groth16::{Circuit, CircuitProver, CommitmentInputs, ProofOutput, WithdrawInputs};
use lowki_keys::Zeroizing;
use lowki_pp_core::{Fr, NoteMessage, Secret, SignatureSecretManager};

use crate::error::{ErrorCode, Result, WorkerError};
use crate::protocol::{ContextIn, GasIn, MerkleProofIn, NoteRef, SolidityProofIn, WithdrawalIn};
use crate::util;

/// Kohaku signature-scheme identity path: `m/28784'/1'/{accountIndex}'`.
pub fn account_path(account_index: u32) -> Result<String> {
    if account_index >= lowki_keys::HARDENED_OFFSET {
        return Err(WorkerError::invalid("accountIndex must be < 2^31"));
    }
    Ok(format!("m/28784'/1'/{account_index}'"))
}

pub fn manager(seed: &[u8; 64], account_index: u32) -> Result<SignatureSecretManager> {
    let key = lowki_keys::derive_at(seed, &account_path(account_index)?)?;
    Ok(SignatureSecretManager::new(&key)?)
}

pub struct NoteCoords {
    pub account_index: u32,
    pub chain_id: u64,
    pub entrypoint: [u8; 20],
    pub deposit_index: u64,
    pub secret_index: u64,
}

impl NoteCoords {
    pub fn parse(n: &NoteRef) -> Result<Self> {
        Ok(Self {
            account_index: n.account_index,
            chain_id: util::u64q(&n.chain_id, "note.chainId")?,
            entrypoint: util::address(&n.entrypoint, "note.entrypoint")?.0.0,
            deposit_index: n.deposit_index,
            secret_index: n.secret_index,
        })
    }

    pub fn message(&self, secret_index: u64) -> NoteMessage {
        NoteMessage {
            chain_id: self.chain_id,
            entrypoint: self.entrypoint,
            deposit_index: self.deposit_index,
            secret_index,
        }
    }
}

pub fn note_secret(seed: &[u8; 64], c: &NoteCoords, secret_index: u64) -> Result<Secret> {
    Ok(manager(seed, c.account_index)?.note_secrets(&c.message(secret_index))?)
}

/// Kohaku `EnvelopeSigner.signEnvelope` output for a PrivacyPoolNote envelope.
pub fn sign_note_envelope(seed: &[u8; 64], c: &NoteCoords) -> Result<Zeroizing<[u8; 65]>> {
    Ok(manager(seed, c.account_index)?.sign_note(&c.message(c.secret_index))?)
}

pub fn fr_u256(v: &U256) -> Result<Fr> {
    lowki_pp_core::fr_from_be_bytes(&v.to_be_bytes::<32>())
        .map_err(|_| WorkerError::invalid("value is not a canonical field element"))
}

pub fn context(ctx: &ContextIn) -> Result<Fr> {
    match (&ctx.context, &ctx.withdrawal, &ctx.scope) {
        (Some(c), None, _) => util::fr(c, "context"),
        (None, Some(w), Some(scope)) => {
            let processooor = util::address(&w.processooor, "withdrawal.processooor")?;
            let data = util::bytes(&w.data, "withdrawal.data")?;
            let scope = util::u256(scope, "scope")?.to_be_bytes::<32>();
            Ok(lowki_pp_core::calculate_context(&processooor.0.0, &data, &scope))
        }
        _ => Err(WorkerError::invalid("give either context, or withdrawal + scope")),
    }
}

fn siblings(p: &MerkleProofIn, what: &str) -> Result<Vec<Fr>> {
    if p.siblings.len() != lowki_pp_core::PADDED_PROOF_DEPTH {
        return Err(WorkerError::invalid(format!(
            "{what}.siblings must have {} entries (Kohaku proofFromTree pads to 32)",
            lowki_pp_core::PADDED_PROOF_DEPTH
        )));
    }
    p.siblings.iter().map(|s| util::fr(s, what)).collect()
}

pub struct WithdrawRequest<'a> {
    pub note: &'a NoteRef,
    pub label: &'a str,
    pub existing_value: &'a str,
    pub withdrawn_value: &'a str,
    pub context: &'a ContextIn,
    pub state_proof: &'a MerkleProofIn,
    pub asp_proof: &'a MerkleProofIn,
}

/// Builds the withdraw circuit inputs exactly as Kohaku's `withdrawThunk`: existing note =
/// `secretIndex`, change note = `secretIndex + 1`, both depths 32.
pub fn withdraw_inputs(seed: &[u8; 64], r: &WithdrawRequest) -> Result<WithdrawInputs> {
    let c = NoteCoords::parse(r.note)?;
    let existing_value = util::u256(r.existing_value, "existingValue")?;
    let withdrawn_value = util::u256(r.withdrawn_value, "withdrawnValue")?;
    if withdrawn_value > existing_value {
        return Err(WorkerError::invalid("withdrawnValue exceeds existingValue"));
    }
    let existing = note_secret(seed, &c, c.secret_index)?;
    let change = note_secret(seed, &c, c.secret_index + 1)?;
    Ok(WithdrawInputs {
        withdrawn_value: fr_u256(&withdrawn_value)?,
        state_root: util::fr(&r.state_proof.root, "stateProof.root")?,
        state_tree_depth: Fr::from(32u64),
        asp_root: util::fr(&r.asp_proof.root, "aspProof.root")?,
        asp_tree_depth: Fr::from(32u64),
        context: context(r.context)?,
        label: util::fr(r.label, "label")?,
        existing_value: fr_u256(&existing_value)?,
        existing_nullifier: existing.nullifier,
        existing_secret: existing.salt,
        new_nullifier: change.nullifier,
        new_secret: change.salt,
        state_siblings: siblings(r.state_proof, "stateProof")?,
        state_index: util::fr(&r.state_proof.index, "stateProof.index")?,
        asp_siblings: siblings(r.asp_proof, "aspProof")?,
        asp_index: util::fr(&r.asp_proof.index, "aspProof.index")?,
    })
}

pub fn commitment_inputs(seed: &[u8; 64], note: &NoteRef, value: &str, label: &str) -> Result<CommitmentInputs> {
    let c = NoteCoords::parse(note)?;
    let s = note_secret(seed, &c, c.secret_index)?;
    Ok(CommitmentInputs {
        value: util::fr(value, "value")?,
        label: util::fr(label, "label")?,
        nullifier: s.nullifier,
        secret: s.salt,
    })
}

// ---- provers ----------------------------------------------------------------------------

#[allow(clippy::large_enum_variant)] // one long-lived instance per circuit, behind an Rc
pub enum Prover {
    Cpu(CircuitProver),
    #[cfg(feature = "webgpu")]
    Gpu(lowki_groth16::GpuProver),
}

impl Prover {
    pub fn cpu(&self) -> &CircuitProver {
        match self {
            Prover::Cpu(p) => p,
            #[cfg(feature = "webgpu")]
            Prover::Gpu(g) => g.cpu(),
        }
    }
    pub fn is_gpu(&self) -> bool {
        !matches!(self, Prover::Cpu(_))
    }
}

pub async fn load_prover(circuit: Circuit, wasm: &[u8], zkey: &[u8]) -> Result<CircuitProver> {
    #[cfg(target_arch = "wasm32")]
    {
        Ok(CircuitProver::load_async(circuit, wasm, zkey).await?)
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        Ok(CircuitProver::load(circuit, wasm, zkey)?)
    }
}

/// Proves; on a GPU failure falls back to the CPU and reports which backend produced it.
pub async fn prove_withdraw(p: &Prover, inputs: &WithdrawInputs) -> Result<(ProofOutput, &'static str)> {
    #[cfg(feature = "webgpu")]
    if let Prover::Gpu(g) = p {
        match lowki_groth16::prove_withdraw_gpu(g, inputs).await {
            Ok(out) => return Ok((out, "webgpu")),
            Err(lowki_groth16::Error::Gpu(_)) => {}
            Err(e) => return Err(e.into()),
        }
    }
    Ok((lowki_groth16::prove_withdraw(p.cpu(), inputs)?, "cpu"))
}

pub async fn prove_commitment(p: &Prover, inputs: &CommitmentInputs) -> Result<(ProofOutput, &'static str)> {
    #[cfg(feature = "webgpu")]
    if let Prover::Gpu(g) = p {
        match lowki_groth16::prove_commitment_gpu(g, inputs).await {
            Ok(out) => return Ok((out, "webgpu")),
            Err(lowki_groth16::Error::Gpu(_)) => {}
            Err(e) => return Err(e.into()),
        }
    }
    Ok((lowki_groth16::prove_commitment(p.cpu(), inputs)?, "cpu"))
}

// ---- paymaster userOp -------------------------------------------------------------------

pub struct PaymasterParams<'a> {
    pub note: &'a NoteRef,
    pub withdrawal: &'a WithdrawalIn,
    pub proof: &'a SolidityProofIn,
    pub pool: &'a str,
    pub paymaster: &'a str,
    pub adapter: &'a str,
    pub gas: &'a GasIn,
    pub max_fee_per_gas: &'a str,
    pub max_priority_fee_per_gas: &'a str,
    pub nonce: &'a str,
    pub authorization_nonce: Option<u64>,
}

pub struct PaymasterOut {
    pub sender: Address,
    pub user_op_hash: String,
    pub user_operation: serde_json::Value,
}

/// Kohaku `paymasterWithdrawThunk` + `buildSignedUserOp`: the sender is the ephemeral
/// signer for `(depositIndex, withdrawIndex = note.secretIndex)`, 7702-delegated to
/// Simple7702Account; `paymasterData = encodePaymasterData(adapter, adapterData)`.
pub fn build_paymaster_withdraw(seed: &[u8; 64], p: &PaymasterParams) -> Result<PaymasterOut> {
    use lowki_aa::fee::GasLimits;
    use lowki_aa::paymaster::{
        BuildSignedUserOpParams, Withdrawal, WithdrawProof, build_signed_user_op, encode_paymaster_data,
        encode_privacy_pool_adapter_data,
    };
    let c = NoteCoords::parse(p.note)?;
    let _pool = util::address(p.pool, "pool")?;
    let adapter = util::address(p.adapter, "adapter")?;
    let processooor = util::address(&p.withdrawal.processooor, "withdrawal.processooor")?;
    if processooor != adapter {
        return Err(WorkerError::invalid("withdrawal.processooor must be the paymaster adapter"));
    }
    let withdrawal = Withdrawal { processooor, data: util::bytes(&p.withdrawal.data, "withdrawal.data")? };
    let pb: Vec<Vec<String>> = vec![
        vec![p.proof.p_b[0][1].clone(), p.proof.p_b[0][0].clone()],
        vec![p.proof.p_b[1][1].clone(), p.proof.p_b[1][0].clone()],
    ];
    // SolidityProof already has pB swapped; from_snarkjs swaps again, so feed it un-swapped.
    let proof = WithdrawProof::from_snarkjs(&p.proof.p_a[..], &pb, &p.proof.p_c[..], &p.proof.pub_signals)?;
    let adapter_data = encode_privacy_pool_adapter_data(&withdrawal, &proof);
    let paymaster_data: Bytes = encode_paymaster_data(adapter, &adapter_data);
    let gas = GasLimits {
        call_gas_limit: util::u128q(&p.gas.call_gas_limit, "gas.callGasLimit")?,
        verification_gas_limit: util::u128q(&p.gas.verification_gas_limit, "gas.verificationGasLimit")?,
        pre_verification_gas: util::u128q(&p.gas.pre_verification_gas, "gas.preVerificationGas")?,
        paymaster_verification_gas_limit: util::u128q(&p.gas.paymaster_verification_gas_limit, "gas.paymasterVerificationGasLimit")?,
        paymaster_post_op_gas_limit: util::u128q(&p.gas.paymaster_post_op_gas_limit, "gas.paymasterPostOpGasLimit")?,
    };
    let params = BuildSignedUserOpParams {
        chain_id: c.chain_id,
        paymaster_address: util::address(p.paymaster, "paymaster")?,
        paymaster_data,
        gas,
        max_fee_per_gas: util::u128q(p.max_fee_per_gas, "maxFeePerGas")?,
        max_priority_fee_per_gas: util::u128q(p.max_priority_fee_per_gas, "maxPriorityFeePerGas")?,
        nonce: util::u256(p.nonce, "nonce")?,
        authorization_nonce: p.authorization_nonce,
        tail_calls: None,
    };
    let key = manager(seed, c.account_index)?.derive_ephemeral_signer(
        c.chain_id,
        &c.entrypoint,
        c.deposit_index,
        c.secret_index,
    )?;
    let signer = lowki_aa::signer::EphemeralSigner::from_bytes(&key)
        .map_err(|_| WorkerError::new(ErrorCode::Signing, "invalid ephemeral key"))?;
    let signed = build_signed_user_op(&signer, &params)?;
    let user_operation = serde_json::to_value(signed.to_serialized())
        .map_err(|_| WorkerError::new(ErrorCode::Internal, "userOp serialization"))?;
    Ok(PaymasterOut {
        sender: signer.address(),
        user_op_hash: format!("{}", signed.user_op_hash),
        user_operation,
    })
}
