use serde_json::Value;

fn h32(s: &str) -> [u8; 32] {
    hex::decode(s.trim_start_matches("0x")).unwrap().try_into().unwrap()
}
fn s(v: &Value) -> &str {
    v.as_str().unwrap()
}

/// The whole paymaster-withdraw pipeline, from fixture strings to the `eth_sendUserOperation`
/// JSON (returned serialized, so the check can compare it with Kohaku's object).
pub fn aa_build(a: &Value) -> String {
    use lowki_aa::fee::GasLimits;
    use lowki_aa::paymaster::*;
    use lowki_aa::signer::EphemeralSigner;
    use lowki_aa::{Address, U256};
    let addr = |k: &str| s(&a[k]).parse::<Address>().unwrap();
    let u = |v: &Value| s(v).parse::<u128>().unwrap();
    let signer = EphemeralSigner::from_bytes(&h32(s(&a["ephemeralKey"]))).unwrap();
    let paymaster = addr("paymaster");
    let adapter = addr("adapter");
    let data = encode_fee_data(&FeeData {
        recipient: addr("recipient"),
        fee_recipient: paymaster,
        fee: s(&a["fee"]).parse::<U256>().unwrap(),
        refund_recipient: addr("refundRecipient"),
    });
    let p = &a["proof"];
    let strs = |v: &Value| v.as_array().unwrap().iter().map(|x| s(x).to_string()).collect::<Vec<_>>();
    let pi_b: Vec<Vec<String>> = p["pi_b"].as_array().unwrap().iter().map(strs).collect();
    let proof = WithdrawProof::from_snarkjs(&strs(&p["pi_a"]), &pi_b, &strs(&p["pi_c"]), &strs(&p["publicSignals"])).unwrap();
    let adapter_data = encode_privacy_pool_adapter_data(&Withdrawal { processooor: adapter, data }, &proof);
    let paymaster_data = encode_paymaster_data(adapter, &adapter_data);
    let g = &a["gas"];
    let op = build_signed_user_op(
        &signer,
        &BuildSignedUserOpParams {
            chain_id: a["chainId"].as_u64().unwrap(),
            paymaster_address: paymaster,
            paymaster_data,
            gas: GasLimits {
                call_gas_limit: u(&g["callGasLimit"]),
                verification_gas_limit: u(&g["verificationGasLimit"]),
                pre_verification_gas: u(&g["preVerificationGas"]),
                paymaster_verification_gas_limit: u(&g["paymasterVerificationGasLimit"]),
                paymaster_post_op_gas_limit: u(&g["paymasterPostOpGasLimit"]),
            },
            max_fee_per_gas: u(&a["maxFeePerGas"]),
            max_priority_fee_per_gas: u(&a["maxPriorityFeePerGas"]),
            nonce: U256::ZERO,
            authorization_nonce: None,
            tail_calls: None,
        },
    )
    .unwrap();
    serde_json::to_string(&op.to_serialized()).unwrap()
}
