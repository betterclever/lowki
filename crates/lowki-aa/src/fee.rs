//! Gas baselines, minimum viable fee and bundler gas-bound checks
//! (Kohaku `paymaster/fee.ts`, `paymaster/gas-bounds.ts`).

use alloy_primitives::U256;

use crate::Error;

/// Kohaku `UserOpGasLimits`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct GasLimits {
    pub call_gas_limit: u128,
    pub verification_gas_limit: u128,
    pub pre_verification_gas: u128,
    pub paymaster_verification_gas_limit: u128,
    pub paymaster_post_op_gas_limit: u128,
}

/// Kohaku `PaymasterGasLimits` (optional absolute ceilings).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct PaymasterGasLimits {
    pub call_gas_limit: Option<u128>,
    pub verification_gas_limit: Option<u128>,
    pub pre_verification_gas: Option<u128>,
    pub paymaster_verification_gas_limit: Option<u128>,
    pub paymaster_post_op_gas_limit: Option<u128>,
}

const ERC20_TRANSFER_GAS: u128 = 100_000;
/// Default execution-phase budget for tail calls without an estimate.
pub const TAIL_CALLS_DEFAULT_GAS: u128 = 300_000;
const PER_EXTRA_WITHDRAW_GAS: u128 = 500_000;
const PER_EXTRA_WITHDRAW_GAS_ERC20: u128 = 600_000;
const FORWARD_GAS: u128 = 80_000;

const BASE_GAS_UNITS: GasLimits = GasLimits {
    pre_verification_gas: 100_000,
    verification_gas_limit: 50_000,
    call_gas_limit: 300_000,
    paymaster_verification_gas_limit: 1_200_000,
    paymaster_post_op_gas_limit: 50_000,
};

/// Kohaku `reasonableGasUnits(isERC20)`. Note the single-note flow overrides
/// `call_gas_limit` to 0 (`{ ...reasonableGasUnits(isERC20), callGasLimit: 0n }`).
pub fn reasonable_gas_units(is_erc20: bool) -> GasLimits {
    if !is_erc20 {
        return BASE_GAS_UNITS;
    }
    GasLimits {
        paymaster_verification_gas_limit: BASE_GAS_UNITS.paymaster_verification_gas_limit
            + ERC20_TRANSFER_GAS,
        ..BASE_GAS_UNITS
    }
}

/// Kohaku `reasonableGasUnitsForBatch`.
pub fn reasonable_gas_units_for_batch(
    is_erc20: bool,
    extra_withdrawals: u64,
    has_tail_calls: bool,
    tail_calls_gas_estimate: Option<u128>,
) -> GasLimits {
    let base = reasonable_gas_units(is_erc20);
    let per = if is_erc20 {
        PER_EXTRA_WITHDRAW_GAS_ERC20
    } else {
        PER_EXTRA_WITHDRAW_GAS
    };
    let tail = if has_tail_calls {
        tail_calls_gas_estimate.unwrap_or(TAIL_CALLS_DEFAULT_GAS)
    } else {
        FORWARD_GAS
    };
    GasLimits {
        call_gas_limit: u128::from(extra_withdrawals) * per + tail,
        ..base
    }
}

/// Kohaku `computeMinimumViableFee`: `sum(all gas limits) * maxFeePerGas * 12 / 10`
/// (1.2x multiplier, integer division), in wei.
pub fn compute_minimum_viable_fee(gas: &GasLimits, max_fee_per_gas: u128) -> U256 {
    let required = U256::from(gas.verification_gas_limit)
        + U256::from(gas.call_gas_limit)
        + U256::from(gas.paymaster_verification_gas_limit)
        + U256::from(gas.pre_verification_gas)
        + U256::from(gas.paymaster_post_op_gas_limit);
    required * U256::from(max_fee_per_gas) * U256::from(12u8) / U256::from(10u8)
}

/// Kohaku `assertGasLimitsWithinBounds`. `payload_bytes` is
/// `size(op.callData) + size(op.paymasterData)` of the built op (0 when no op yet; see
/// [`crate::paymaster::SignedUserOperation::payload_len`]). Fields are checked in
/// Kohaku's order: call, verification, preVerification, paymasterVerification,
/// paymasterPostOp.
pub fn assert_gas_limits_within_bounds(
    gas: &GasLimits,
    baseline: &GasLimits,
    payload_bytes: usize,
    limits: &PaymasterGasLimits,
) -> Result<(), Error> {
    let bytes = U256::from(payload_bytes);
    let two = U256::from(2u8);
    let defaults: [Option<U256>; 5] = [
        None,
        Some(U256::from(baseline.verification_gas_limit) * two),
        Some((U256::from(350_000u64) + U256::from(40u8) * bytes) * two),
        Some(U256::from(baseline.paymaster_verification_gas_limit) * two),
        Some(U256::from(baseline.paymaster_post_op_gas_limit) * two),
    ];
    let fields: [(&'static str, u128, Option<u128>); 5] = [
        ("callGasLimit", gas.call_gas_limit, limits.call_gas_limit),
        (
            "verificationGasLimit",
            gas.verification_gas_limit,
            limits.verification_gas_limit,
        ),
        (
            "preVerificationGas",
            gas.pre_verification_gas,
            limits.pre_verification_gas,
        ),
        (
            "paymasterVerificationGasLimit",
            gas.paymaster_verification_gas_limit,
            limits.paymaster_verification_gas_limit,
        ),
        (
            "paymasterPostOpGasLimit",
            gas.paymaster_post_op_gas_limit,
            limits.paymaster_post_op_gas_limit,
        ),
    ];
    for ((field, value, limit), default) in fields.into_iter().zip(defaults) {
        let cap = limit.map(U256::from).or(default);
        if let Some(cap) = cap
            && U256::from(value) > cap
        {
            return Err(Error::GasLimitExceedsCeiling {
                field,
                value,
                cap: cap.saturating_to::<u128>(),
            });
        }
    }
    Ok(())
}

/// Kohaku `assertGasPriceWithinBounds`: `tipCap = max(3 * refTip, 2 gwei)`,
/// `feeCap = 3 * baseFee + tipCap`.
pub fn assert_gas_price_within_bounds(
    max_fee_per_gas: u128,
    max_priority_fee_per_gas: u128,
    base_fee_per_gas: u128,
    reference_max_priority_fee_per_gas: u128,
) -> Result<(), Error> {
    let three = U256::from(3u8);
    let floor = U256::from(2_000_000_000u64);
    let tip3 = U256::from(reference_max_priority_fee_per_gas) * three;
    let tip_cap = if tip3 > floor { tip3 } else { floor };
    let fee_cap = U256::from(base_fee_per_gas) * three + tip_cap;

    if max_priority_fee_per_gas > max_fee_per_gas {
        return Err(Error::InvalidGasPrices);
    }
    if U256::from(max_priority_fee_per_gas) > tip_cap || U256::from(max_fee_per_gas) > fee_cap {
        return Err(Error::GasPriceExceedsBounds {
            fee_cap: fee_cap.to_string(),
            tip_cap: tip_cap.to_string(),
        });
    }
    Ok(())
}
