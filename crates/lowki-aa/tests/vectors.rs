//! Differential tests against vectors generated from the pinned Kohaku sources + viem
//! (`vectors/lowki-aa/gen.ts`).

use std::str::FromStr;

use lowki_aa::eip7702::Authorization;
use lowki_aa::fee::{self, GasLimits, PaymasterGasLimits};
use lowki_aa::paymaster::{
    self, BuildSignedUserOpParams, Call, FeeData, WithdrawProof, Withdrawal,
};
use lowki_aa::shield;
use lowki_aa::signer::EphemeralSigner;
use lowki_aa::userop::{self, PaymasterFields, UserOperation};
use lowki_aa::{Address, B256, Bytes, U256};
use serde_json::Value;

const VECTORS: &str = include_str!("../../../vectors/lowki-aa/vectors.json");

fn vectors() -> Value {
    serde_json::from_str(VECTORS).unwrap()
}
fn s(v: &Value) -> &str {
    v.as_str().unwrap_or_else(|| panic!("not a string: {v}"))
}
fn u256(v: &Value) -> U256 {
    U256::from_str(s(v)).unwrap()
}
fn u128v(v: &Value) -> u128 {
    s(v).parse().unwrap()
}
fn addr(v: &Value) -> Address {
    Address::from_str(s(v)).unwrap()
}
fn bytes(v: &Value) -> Bytes {
    Bytes::from_str(s(v)).unwrap()
}
fn b256(v: &Value) -> B256 {
    B256::from_str(s(v)).unwrap()
}
fn hex(b: &[u8]) -> String {
    Bytes::copy_from_slice(b).to_string()
}
fn gas(v: &Value) -> GasLimits {
    GasLimits {
        call_gas_limit: u128v(&v["callGasLimit"]),
        verification_gas_limit: u128v(&v["verificationGasLimit"]),
        pre_verification_gas: u128v(&v["preVerificationGas"]),
        paymaster_verification_gas_limit: u128v(&v["paymasterVerificationGasLimit"]),
        paymaster_post_op_gas_limit: u128v(&v["paymasterPostOpGasLimit"]),
    }
}
fn key(v: &Value) -> EphemeralSigner {
    let b = b256(v);
    EphemeralSigner::from_bytes(&b.0).unwrap()
}

#[test]
fn erc4337_v08_user_op_hash_and_packing() {
    let v = vectors();
    let cases = v["userOpHash"].as_array().unwrap();
    assert!(cases.len() >= 60);
    for (i, c) in cases.iter().enumerate() {
        let paymaster = (!c["paymaster"].is_null()).then(|| PaymasterFields {
            paymaster: addr(&c["paymaster"]),
            verification_gas_limit: u128v(&c["paymasterVerificationGasLimit"]),
            post_op_gas_limit: u128v(&c["paymasterPostOpGasLimit"]),
            data: bytes(&c["paymasterData"]),
        });
        let op = UserOperation {
            sender: addr(&c["sender"]),
            nonce: u256(&c["nonce"]),
            factory: (!c["factory"].is_null()).then(|| bytes(&c["factory"])),
            factory_data: if c["factoryData"].is_null() {
                Bytes::new()
            } else {
                bytes(&c["factoryData"])
            },
            call_data: bytes(&c["callData"]),
            call_gas_limit: u128v(&c["callGasLimit"]),
            verification_gas_limit: u128v(&c["verificationGasLimit"]),
            pre_verification_gas: u256(&c["preVerificationGas"]),
            max_fee_per_gas: u128v(&c["maxFeePerGas"]),
            max_priority_fee_per_gas: u128v(&c["maxPriorityFeePerGas"]),
            paymaster,
            signature: bytes(&c["signature"]),
            eip7702_delegate: (!c["authorizationAddress"].is_null())
                .then(|| addr(&c["authorizationAddress"])),
        };
        let p = &c["packed"];
        let packed = op.pack();
        assert_eq!(
            hex(&packed.init_code),
            s(&p["initCode"]).to_lowercase(),
            "case {i} initCode"
        );
        assert_eq!(
            hex(&op.init_code_for_hash()),
            s(&p["initCodeForHash"]).to_lowercase(),
            "case {i} initCodeForHash"
        );
        assert_eq!(
            packed.account_gas_limits,
            b256(&p["accountGasLimits"]),
            "case {i}"
        );
        assert_eq!(packed.gas_fees, b256(&p["gasFees"]), "case {i}");
        assert_eq!(
            hex(&packed.paymaster_and_data),
            s(&p["paymasterAndData"]).to_lowercase(),
            "case {i}"
        );
        let chain_id = c["chainId"].as_u64().unwrap();
        assert_eq!(
            userop::user_op_hash(&op, chain_id, addr(&c["entryPoint"])),
            b256(&c["userOpHash"]),
            "case {i} userOpHash"
        );
    }
}

#[test]
fn eip7702_authorization_hash_and_signature() {
    let v = vectors();
    let cases = v["eip7702"].as_array().unwrap();
    assert!(cases.len() >= 40);
    for (i, c) in cases.iter().enumerate() {
        let signer = key(&c["privateKey"]);
        assert_eq!(signer.address(), addr(&c["signer"]));
        let auth = Authorization {
            chain_id: U256::from(c["chainId"].as_u64().unwrap()),
            address: addr(&c["address"]),
            nonce: s(&c["nonce"]).parse().unwrap(),
        };
        assert_eq!(auth.signing_hash(), b256(&c["hash"]), "case {i} hash");
        let signed = auth.sign(&signer).unwrap();
        assert_eq!(signed.r, b256(&c["r"]), "case {i} r");
        assert_eq!(signed.s, b256(&c["s"]), "case {i} s");
        assert_eq!(
            u64::from(signed.y_parity),
            c["yParity"].as_u64().unwrap(),
            "case {i} yParity"
        );
        assert_eq!(signed.recover_authority(), Some(signer.address()));
    }
}

fn proof_from_vector(c: &Value) -> WithdrawProof {
    let sj = &c["snarkjs"];
    let strs = |v: &Value| {
        v.as_array()
            .unwrap()
            .iter()
            .map(|x| s(x).to_string())
            .collect::<Vec<_>>()
    };
    let pi_b: Vec<Vec<String>> = sj["pi_b"].as_array().unwrap().iter().map(strs).collect();
    let p = WithdrawProof::from_snarkjs(
        &strs(&sj["pi_a"]),
        &pi_b,
        &strs(&sj["pi_c"]),
        &strs(&sj["publicSignals"]),
    )
    .unwrap();
    // Compare against Kohaku toWithdrawProof's struct.
    let q = &c["proof"];
    let arr = |v: &Value| v.as_array().unwrap().iter().map(u256).collect::<Vec<_>>();
    assert_eq!(p.p_a.to_vec(), arr(&q["pA"]));
    assert_eq!(p.p_b[0].to_vec(), arr(&q["pB"][0]));
    assert_eq!(p.p_b[1].to_vec(), arr(&q["pB"][1]));
    assert_eq!(p.p_c.to_vec(), arr(&q["pC"]));
    assert_eq!(p.pub_signals.to_vec(), arr(&q["pubSignals"]));
    p
}

#[test]
fn kohaku_paymaster_encodings() {
    let v = vectors();
    let cases = v["paymasterEncodings"].as_array().unwrap();
    assert!(cases.len() >= 40);
    for (i, c) in cases.iter().enumerate() {
        let proof = proof_from_vector(c);
        let fd = &c["feeData"];
        let fee_data = FeeData {
            recipient: addr(&fd["recipient"]),
            fee_recipient: addr(&fd["feeRecipient"]),
            fee: u256(&fd["fee"]),
            refund_recipient: addr(&fd["refundRecipient"]),
        };
        let fee_enc = paymaster::encode_fee_data(&fee_data);
        assert_eq!(fee_enc, bytes(&c["feeDataEncoded"]), "case {i} feeData");
        let adapter = addr(&c["adapter"]);
        let withdrawal = Withdrawal {
            processooor: adapter,
            data: fee_enc,
        };
        let adapter_data = paymaster::encode_privacy_pool_adapter_data(&withdrawal, &proof);
        assert_eq!(
            adapter_data,
            bytes(&c["adapterData"]),
            "case {i} adapterData"
        );
        assert_eq!(
            paymaster::encode_paymaster_data(adapter, &adapter_data),
            bytes(&c["paymasterData"]),
            "case {i} paymasterData"
        );
        let dw = &c["directWithdrawal"];
        let direct = Withdrawal {
            processooor: addr(&dw["processooor"]),
            data: bytes(&dw["data"]),
        };
        assert_eq!(
            paymaster::encode_pool_withdraw(&direct, &proof),
            bytes(&c["poolWithdraw"]),
            "case {i} withdraw"
        );
    }
}

#[test]
fn kohaku_build_signed_user_op_full_json() {
    let v = vectors();
    let cases = v["signedUserOps"].as_array().unwrap();
    assert!(cases.len() >= 40);
    let (mut none, mut one, mut many) = (0, 0, 0);
    for (i, c) in cases.iter().enumerate() {
        let signer = key(&c["privateKey"]);
        let tail_calls = c["tailCalls"].as_array().map(|calls| {
            calls
                .iter()
                .map(|t| Call {
                    to: addr(&t["to"]),
                    value: u256(&t["value"]),
                    data: bytes(&t["data"]),
                })
                .collect::<Vec<_>>()
        });
        match tail_calls.as_ref().map(Vec::len).unwrap_or(0) {
            0 => none += 1,
            1 => one += 1,
            _ => many += 1,
        }
        let params = BuildSignedUserOpParams {
            chain_id: c["chainId"].as_u64().unwrap(),
            paymaster_address: addr(&c["paymasterAddress"]),
            paymaster_data: bytes(&c["paymasterData"]),
            gas: gas(&c["gas"]),
            max_fee_per_gas: u128v(&c["maxFeePerGas"]),
            max_priority_fee_per_gas: u128v(&c["maxPriorityFeePerGas"]),
            nonce: if c["nonce"].is_null() {
                U256::ZERO
            } else {
                u256(&c["nonce"])
            },
            authorization_nonce: (!c["authorizationNonce"].is_null())
                .then(|| s(&c["authorizationNonce"]).parse().unwrap()),
            tail_calls,
        };
        let signed = paymaster::build_signed_user_op(&signer, &params).unwrap();
        assert_eq!(
            signed.user_op_hash,
            b256(&c["userOpHash"]),
            "case {i} userOpHash"
        );
        let json = serde_json::to_value(signed.to_serialized()).unwrap();
        assert_eq!(json, c["userOperation"], "case {i} signed userOp JSON");
        // Round-trip through the JSON type.
        let back: paymaster::SerializedUserOperation =
            serde_json::from_value(c["userOperation"].clone()).unwrap();
        assert_eq!(back, signed.to_serialized());
        assert_eq!(
            signed.authorization.recover_authority(),
            Some(signer.address())
        );
    }
    assert!(none > 0 && one > 0 && many > 0);
}

#[test]
fn kohaku_shield_calldata() {
    let v = vectors();
    let cases = v["shield"].as_array().unwrap();
    assert!(cases.len() >= 30);
    for (i, c) in cases.iter().enumerate() {
        let pre = u256(&c["precommitment"]);
        let amount = u256(&c["amount"]);
        let ep = addr(&c["entrypointAddress"]);
        let tx = &c["tx"];
        match s(&c["kind"]) {
            "native" => {
                let t = shield::prepare_native_shield(pre, amount, ep);
                assert_eq!(
                    (t.to, t.data.clone(), t.value),
                    (addr(&tx["to"]), bytes(&tx["data"]), u256(&tx["value"])),
                    "case {i}"
                );
                assert_eq!(
                    shield::shield_transactions(pre, lowki_aa::config::E_ADDRESS, amount, ep),
                    vec![shield::prepare_native_shield(pre, amount, ep)]
                );
            }
            "erc20" => {
                let token = addr(&c["tokenAddress"]);
                let t = shield::prepare_erc20_shield(pre, amount, token, ep);
                assert_eq!(
                    (t.to, t.data.clone(), t.value),
                    (addr(&tx["to"]), bytes(&tx["data"]), u256(&tx["value"])),
                    "case {i}"
                );
                let a = shield::erc20_approve(token, ep, amount);
                let ap = &c["approve"];
                assert_eq!(
                    (a.to, a.data.clone(), a.value),
                    (addr(&ap["to"]), bytes(&ap["data"]), u256(&ap["value"])),
                    "case {i}"
                );
                let both = shield::shield_transactions(pre, token, amount, ep);
                assert_eq!(both, vec![a, t]);
            }
            k => panic!("unknown kind {k}"),
        }
    }
}

#[test]
fn kohaku_fee_helpers() {
    let v = vectors();
    let f = &v["fees"];
    assert_eq!(
        fee::TAIL_CALLS_DEFAULT_GAS,
        u128v(&f["tailCallsDefaultGas"])
    );
    for c in f["reasonableGasUnits"].as_array().unwrap() {
        assert_eq!(
            fee::reasonable_gas_units(c["isERC20"].as_bool().unwrap()),
            gas(&c["gas"])
        );
    }
    let batch = f["reasonableGasUnitsForBatch"].as_array().unwrap();
    for (i, c) in batch.iter().enumerate() {
        let est = (!c["tailCallsGasEstimate"].is_null()).then(|| u128v(&c["tailCallsGasEstimate"]));
        let got = fee::reasonable_gas_units_for_batch(
            c["isERC20"].as_bool().unwrap(),
            c["extraWithdrawals"].as_u64().unwrap(),
            c["hasTailCalls"].as_bool().unwrap(),
            est,
        );
        assert_eq!(got, gas(&c["gas"]), "batch case {i}");
    }
    let mvf = f["computeMinimumViableFee"].as_array().unwrap();
    for (i, c) in mvf.iter().enumerate() {
        let got = fee::compute_minimum_viable_fee(&gas(&c["gas"]), u128v(&c["maxFeePerGas"]));
        assert_eq!(got, u256(&c["fee"]), "minFee case {i}");
    }
    let limits = f["gasLimitsBounds"].as_array().unwrap();
    let (mut ok, mut err) = (0, 0);
    for (i, c) in limits.iter().enumerate() {
        let l = &c["limits"];
        let opt = |k: &str| (!l[k].is_null()).then(|| u128v(&l[k]));
        let caps = PaymasterGasLimits {
            call_gas_limit: opt("callGasLimit"),
            verification_gas_limit: opt("verificationGasLimit"),
            pre_verification_gas: opt("preVerificationGas"),
            paymaster_verification_gas_limit: opt("paymasterVerificationGasLimit"),
            paymaster_post_op_gas_limit: opt("paymasterPostOpGasLimit"),
        };
        let payload =
            c["callDataLen"].as_u64().unwrap_or(0) + c["paymasterDataLen"].as_u64().unwrap_or(0);
        let res = fee::assert_gas_limits_within_bounds(
            &gas(&c["gas"]),
            &gas(&c["baseline"]),
            payload as usize,
            &caps,
        );
        match c["error"].as_str() {
            None => {
                ok += 1;
                assert_eq!(res, Ok(()), "limits case {i}")
            }
            Some(msg) => {
                err += 1;
                assert_eq!(res.unwrap_err().to_string(), msg, "limits case {i}")
            }
        }
    }
    assert!(ok > 5 && err > 5);
    let prices = f["gasPriceBounds"].as_array().unwrap();
    let (mut ok, mut err) = (0, 0);
    for (i, c) in prices.iter().enumerate() {
        let res = fee::assert_gas_price_within_bounds(
            u128v(&c["maxFeePerGas"]),
            u128v(&c["maxPriorityFeePerGas"]),
            u128v(&c["baseFeePerGas"]),
            u128v(&c["referenceMaxPriorityFeePerGas"]),
        );
        match c["error"].as_str() {
            None => {
                ok += 1;
                assert_eq!(res, Ok(()), "price case {i}")
            }
            Some(msg) => {
                err += 1;
                assert_eq!(res.unwrap_err().to_string(), msg, "price case {i}")
            }
        }
    }
    assert!(ok > 5 && err > 5);
}

#[test]
fn config_matches_kohaku() {
    let v = vectors();
    let m = &v["meta"];
    assert_eq!(userop::ENTRY_POINT_V08, addr(&m["entryPoint08"]));
    assert_eq!(
        lowki_aa::config::SIMPLE_7702_IMPLEMENTATION,
        addr(&m["simple7702Implementation"])
    );
    assert_eq!(lowki_aa::config::E_ADDRESS, addr(&m["eAddress"]));
    let main = &m["paymasterConfigs"]["1"];
    let cfg = lowki_aa::config::PAYMASTER_MAINNET;
    assert_eq!(cfg.paymaster, addr(&main["paymasterAddress"]));
    assert_eq!(cfg.bundler_url, s(&main["bundlerUrl"]));
    let map = main["poolsAccountsMap"].as_object().unwrap();
    assert_eq!(map.len(), cfg.pool_adapters.len());
    for (pool, adapter) in map {
        assert_eq!(
            cfg.adapter_for_pool(Address::from_str(pool).unwrap()),
            Some(addr(adapter))
        );
    }
    let sep = &m["paymasterConfigs"]["11155111"];
    assert_eq!(addr(&sep["paymasterAddress"]), Address::ZERO);
    assert!(sep["poolsAccountsMap"].as_object().unwrap().is_empty());
}

#[test]
fn unsafe_authorization_nonce_is_rejected() {
    let signer = EphemeralSigner::from_bytes(&[0x11; 32]).unwrap();
    let mut params = BuildSignedUserOpParams {
        chain_id: 11155111,
        paymaster_address: Address::ZERO,
        paymaster_data: Bytes::new(),
        gas: fee::reasonable_gas_units(false),
        max_fee_per_gas: 1,
        max_priority_fee_per_gas: 1,
        nonce: U256::from(1u64 << 53),
        authorization_nonce: None,
        tail_calls: None,
    };
    assert_eq!(
        paymaster::build_signed_user_op(&signer, &params).unwrap_err(),
        lowki_aa::Error::UnsafeAuthorizationNonce
    );
    params.authorization_nonce = Some(0);
    assert!(paymaster::build_signed_user_op(&signer, &params).is_ok());
    assert!(EphemeralSigner::from_bytes(&[0u8; 32]).is_err());
}
