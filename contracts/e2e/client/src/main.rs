//! Test-only CLI used by `contracts/e2e/e2e.ts`. One subcommand per step; JSON on stdin, JSON on
//! stdout. All key handling, calldata, signatures, record parsing, stealth derivation, CCIP-Read
//! and scanning happen here with lowki-ens and lowki-stealth; the TypeScript side only sends
//! transactions and plays an unmodified wallet.
//!
//! The wallet seed arrives on stdin (it is random per e2e run, on a fork). Private keys are never
//! printed: outputs are addresses, public keys, calldata and signatures.

use std::io::Read;

use lowki_ens::{
    Address, SecretKey, StealthMetaAddress,
    abi,
    ccip::{CallOutcome, HttpFailure, HttpMethod, HttpRequest, HttpResponse, Transport, ccip_read},
    hex,
    identity::IdentityKey,
    lowki_registrar::{Registration, encode_register, sign_registration},
    name::{dns_encode, labelhash, namehash},
    owner_account::{Call, encode_execute, sign_authorization, sign_execute},
    permissioned_resolver::{ADMIN_ROLES, ALL_ROLES, Grant, encode_deploy_proxy, encode_initialize, encode_set_text},
    registry::encode_set_resolver,
    resolver::{decode_addr_result, decode_data_result, encode_addr, encode_data},
    stealth::{
        decode_stealth_record, encode_set_chains, encode_set_config, encode_set_meta_address,
        encode_stealth_record, records_auth_message, sign_personal, stealth_data_key, stealth_text_key,
    },
    universal_resolver::{decode_resolve_result, encode_resolve, encode_resolve_with_gateways},
};
use lowki_stealth::{check_announcement, compute_stealth_private_key, derive_stealth_keys, generate_stealth_address};
use serde_json::{Value, json};
use zeroize::Zeroizing;

type R<T> = Result<T, String>;

fn main() {
    let cmd = std::env::args().nth(1).unwrap_or_default();
    let mut input = String::new();
    std::io::stdin().read_to_string(&mut input).expect("stdin");
    let v: Value = serde_json::from_str(&input).expect("JSON on stdin");
    let out = match cmd.as_str() {
        "keys" => keys(&v),
        "sign-registration" => sign_reg(&v),
        "owner-setup" => owner_setup(&v),
        "read-derive" => read_derive(&v),
        "resolve-addr" => resolve_addr(&v),
        "scan" => scan(&v),
        _ => Err(format!("unknown command {cmd:?}")),
    };
    match out {
        Ok(o) => println!("{o}"),
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    }
}

// ------------------------------------------------------------------------------- helpers

fn s<'a>(v: &'a Value, k: &str) -> R<&'a str> {
    v[k].as_str().ok_or_else(|| format!("missing string {k}"))
}
fn u(v: &Value, k: &str) -> R<u64> {
    v[k].as_u64().ok_or_else(|| format!("missing number {k}"))
}
fn addr(v: &Value, k: &str) -> R<Address> {
    let b = hex::decode(s(v, k)?).map_err(|_| format!("{k} not hex"))?;
    b.try_into().map_err(|_| format!("{k} not 20 bytes"))
}
fn h(b: &[u8]) -> String {
    hex::encode(b)
}
fn cs(a: &Address) -> String {
    lowki_ens::to_checksum_address(a)
}

fn seed(v: &Value) -> R<Zeroizing<[u8; 64]>> {
    let b = Zeroizing::new(hex::decode(s(v, "seed")?).map_err(|_| "seed not hex")?);
    let mut out = Zeroizing::new([0u8; 64]);
    if b.len() != 64 {
        return Err("seed must be 64 bytes".into());
    }
    out.copy_from_slice(&b);
    Ok(out)
}

/// The name-owner identity key (`m/137'/0'/0'`) and the stealth keys (`m/5564'/0'/{0,1}'`).
fn wallet(v: &Value) -> R<(IdentityKey, lowki_stealth::StealthKeys)> {
    let seed = seed(v)?;
    let id = IdentityKey::from_seed(&seed, 0).map_err(|e| format!("identity: {e:?}"))?;
    let sk = derive_stealth_keys(&seed).map_err(|e| format!("stealth keys: {e:?}"))?;
    Ok((id, sk))
}

fn meta_of(sk: &lowki_stealth::StealthKeys) -> StealthMetaAddress {
    sk.meta_address()
}

// ------------------------------------------------------------------------------- JSON-RPC / HTTP

struct Rpc {
    url: String,
}

impl Rpc {
    fn call(&self, method: &str, params: Value) -> R<Value> {
        let resp: Value = ureq::post(&self.url)
            .send_json(json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params }))
            .map_err(|e| format!("rpc {method}: {e}"))?
            .into_json()
            .map_err(|e| format!("rpc {method}: {e}"))?;
        Ok(resp)
    }

    fn eth_call(&self, to: &Address, data: &[u8]) -> R<CallOutcome> {
        let r = self.call("eth_call", json!([{ "to": h(to), "data": h(data) }, "latest"]))?;
        if let Some(res) = r["result"].as_str() {
            return Ok(CallOutcome::Success(hex::decode(res).map_err(|_| "result not hex")?));
        }
        // anvil/geth put revert data in error.data.
        let data = r["error"]["data"].as_str().unwrap_or("0x");
        Ok(CallOutcome::Revert(hex::decode(data).map_err(|_| "revert data not hex")?))
    }

    fn balance(&self, a: &Address) -> R<u128> {
        let r = self.call("eth_getBalance", json!([h(a), "latest"]))?;
        let x = r["result"].as_str().ok_or("no balance")?;
        u128::from_str_radix(x.trim_start_matches("0x"), 16).map_err(|_| "bad balance".into())
    }
}

impl Transport for Rpc {
    fn eth_call(&mut self, to: &Address, data: &[u8]) -> Result<CallOutcome, lowki_ens::Error> {
        Rpc::eth_call(self, to, data).map_err(|_| lowki_ens::Error::InvalidAbi)
    }
    fn http(&mut self, req: &HttpRequest) -> Result<HttpResponse, HttpFailure> {
        let r = match req.method {
            HttpMethod::Get => ureq::get(&req.url).call(),
            HttpMethod::Post => ureq::post(&req.url)
                .set("content-type", "application/json")
                .send_string(req.body.as_deref().unwrap_or("")),
        };
        let resp = match r {
            Ok(r) => r,
            Err(ureq::Error::Status(_, r)) => r,
            Err(_) => return Err(HttpFailure::Network),
        };
        let status = resp.status();
        let content_type = resp.header("content-type").map(str::to_string);
        let body = resp.into_string().map_err(|_| HttpFailure::Network)?;
        Ok(HttpResponse { status, content_type, body })
    }
}

/// `UniversalResolver.resolve(name, data(node, "stealth-meta-address[1][60]"))`: on chain,
/// no CCIP-Read (ENSIP-30 requires the record on chain).
fn read_record(rpc: &Rpc, ur: &Address, name: &str) -> R<Vec<u8>> {
    let dns = dns_encode(name).map_err(|e| format!("{e:?}"))?;
    let call = encode_resolve(&dns, &encode_data(&namehash(name), &stealth_data_key(1, 60)));
    match rpc.eth_call(ur, &call)? {
        CallOutcome::Success(ret) => {
            let (result, _resolver) = decode_resolve_result(&ret).map_err(|e| format!("{e:?}"))?;
            decode_data_result(&result).map_err(|e| format!("{e:?}"))
        }
        CallOutcome::Revert(d) => Err(format!("UniversalResolver reverted: {}", format!("{:?}", lowki_ens::universal_resolver::decode_ur_error(&d)))),
    }
}

// ------------------------------------------------------------------------------- commands

fn keys(v: &Value) -> R<Value> {
    let (id, sk) = wallet(v)?;
    let meta = meta_of(&sk);
    Ok(json!({
        "owner": cs(&id.address()),
        "identity_path": lowki_ens::identity::identity_path(0),
        "meta_uri": meta.to_uri(),
        "record": h(&encode_stealth_record(true, &meta)),
        "viewing_address": cs(&sk.viewing.address()),
    }))
}

fn sign_reg(v: &Value) -> R<Value> {
    let (id, _) = wallet(v)?;
    let reg = Registration {
        label: s(v, "label")?.to_string(),
        owner: id.address(),
        resolver: addr(v, "resolver")?,
        duration: u(v, "duration")?,
        nonce: u(v, "nonce")?,
        deadline: u(v, "deadline")?,
    };
    lowki_ens::lowki_registrar::validate_label(&reg.label).map_err(|e| format!("label: {e:?}"))?;
    let sig = sign_registration(&id, u(v, "chain_id")?, &addr(v, "registrar")?, &reg);
    Ok(json!({ "owner": cs(&reg.owner), "signature": h(&sig), "calldata": h(&encode_register(&reg, &sig)) }))
}

/// One EIP-7702 + EIP-712 sponsored batch: deploy the owner's PermissionedResolver (with the
/// legacy text record), configure StealthResolver (records resolver, gateway, chain 60,
/// meta-address), then point the name at StealthResolver.
fn owner_setup(v: &Value) -> R<Value> {
    let (id, sk) = wallet(v)?;
    let chain_id = u(v, "chain_id")?;
    let owner = id.address();
    let name = s(v, "name")?;
    let label = name.split('.').next().ok_or("name")?;
    let dns = dns_encode(name).map_err(|e| format!("{e:?}"))?;
    let node = namehash(name);
    let meta = meta_of(&sk);
    let factory = addr(v, "factory")?;
    let stealth = addr(v, "stealth_resolver")?;

    // The proxy address the factory will deploy for (deployer = owner, salt = node).
    let rpc_less_predict = predict_proxy(v, &owner, &node)?;
    let init = encode_initialize(
        &[Grant { account: owner, role_bitmap: or(ALL_ROLES, ADMIN_ROLES) }],
        &[encode_set_text(&dns, &stealth_text_key(1), &meta.to_uri())],
    );
    let calls = vec![
        Call { target: factory, value: [0; 32], data: encode_deploy_proxy(&addr(v, "permissioned_resolver_impl")?, &node, &init) },
        Call { target: stealth, value: [0; 32], data: encode_set_config(&dns, &rpc_less_predict, &[s(v, "gateway_url")?]) },
        Call { target: stealth, value: [0; 32], data: encode_set_chains(&dns, &[60], true) },
        Call { target: stealth, value: [0; 32], data: encode_set_meta_address(&dns, &encode_stealth_record(true, &meta)) },
        Call { target: addr(v, "registry")?, value: [0; 32], data: encode_set_resolver(&labelhash(label), &stealth) },
    ];
    let deadline = u(v, "deadline")?;
    let sig = sign_execute(&id, chain_id, &owner, &calls, u(v, "exec_nonce")?, deadline).map_err(|e| format!("{e:?}"))?;
    let auth = sign_authorization(&id, chain_id, &addr(v, "owner_account")?, u(v, "auth_nonce")?);
    Ok(json!({
        "calldata": h(&encode_execute(&calls, deadline, &sig)),
        "records_resolver": cs(&rpc_less_predict),
        "authorization": {
            "address": cs(&auth.address),
            "chainId": auth.chain_id,
            "nonce": auth.nonce,
            "r": h(&auth.r),
            "s": h(&auth.s),
            "yParity": auth.y_parity,
        },
    }))
}

fn or(a: lowki_ens::U256, b: lowki_ens::U256) -> lowki_ens::U256 {
    let mut o = [0u8; 32];
    for i in 0..32 {
        o[i] = a[i] | b[i];
    }
    o
}

/// `VerifiableFactory.predictProxyAddress(owner, salt)` (read through the fork's RPC).
fn predict_proxy(v: &Value, deployer: &Address, salt: &[u8; 32]) -> R<Address> {
    let rpc = Rpc { url: v["rpc"].as_str().map(str::to_string).unwrap_or_else(|| std::env::var("E2E_RPC").unwrap_or_default()) };
    let call = lowki_ens::permissioned_resolver::encode_predict_proxy_address(deployer, salt);
    match rpc.eth_call(&addr(v, "factory")?, &call)? {
        CallOutcome::Success(r) => abi::Reader::new(&r).address(0).map_err(|e| format!("{e:?}")),
        CallOutcome::Revert(_) => Err("predictProxyAddress reverted".into()),
    }
}

/// Client-side path: read the ENSIP-30 record on chain, honour its flag, derive locally.
fn read_derive(v: &Value) -> R<Value> {
    let rpc = Rpc { url: s(v, "rpc")?.into() };
    let record = read_record(&rpc, &addr(v, "universal_resolver")?, s(v, "name")?)?;
    let rec = decode_stealth_record(&record).map_err(|e| format!("malformed record: {e:?}"))?;
    if !rec.client_side_allowed {
        return Err("the recipient does not permit client-side derivation".into());
    }
    let eph_bytes = Zeroizing::new(hex::decode(s(v, "ephemeral")?).map_err(|_| "ephemeral not hex")?);
    let eph = SecretKey::from_slice(&eph_bytes).map_err(|e| format!("{e:?}"))?;
    let g = generate_stealth_address(&rec.meta, &eph).map_err(|e| format!("{e:?}"))?;
    Ok(json!({
        "record": h(&record),
        "client_side_allowed": rec.client_side_allowed,
        "meta_uri": rec.meta.to_uri(),
        "stealth_address": cs(&g.stealth_address),
        "ephemeral_pubkey": h(&g.ephemeral_pubkey),
        "view_tag": g.view_tag,
    }))
}

/// Gateway path through lowki-ens's own ERC-3668 client:
/// `UR.resolveWithGateways(name, addr(node), ["x-batch-gateway:true"])`, local batch gateway,
/// our gateway, `StealthResolver` callback (verified on chain), UR callback.
fn resolve_addr(v: &Value) -> R<Value> {
    let mut rpc = Rpc { url: s(v, "rpc")?.into() };
    let name = s(v, "name")?;
    let dns = dns_encode(name).map_err(|e| format!("{e:?}"))?;
    let call = encode_resolve_with_gateways(&dns, &encode_addr(&namehash(name)), &[lowki_ens::ccip::LOCAL_BATCH_GATEWAY_URL]);
    let ret = ccip_read(&mut rpc, addr(v, "universal_resolver")?, &call).map_err(|e| format!("ccip-read: {e:?}"))?;
    let (result, resolver) = decode_resolve_result(&ret).map_err(|e| format!("{e:?}"))?;
    let a = decode_addr_result(&result).map_err(|e| format!("{e:?}"))?;
    Ok(json!({ "address": cs(&a), "resolver": cs(&resolver) }))
}

/// Recipient: fetch the gateway records (viewing-key auth), keep those whose view tag and
/// derived address match, check balances on chain, and prove control of each by signing with the
/// derived stealth private key.
fn scan(v: &Value) -> R<Value> {
    let (_, sk) = wallet(v)?;
    let rpc = Rpc { url: s(v, "rpc")?.into() };
    let name = s(v, "name")?;
    let ts = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_err(|_| "clock")?.as_secs();
    let sig = sign_personal(&sk.viewing, &records_auth_message(name, ts)).map_err(|e| format!("{e:?}"))?;
    let body: Value = ureq::get(&format!("{}/v1/ens/records", s(v, "gateway_base")?))
        .query("name", name)
        .set("x-lowki-auth-timestamp", &ts.to_string())
        .set("x-lowki-auth-signature", &h(&sig))
        .call()
        .map_err(|e| format!("records: {e}"))?
        .into_json()
        .map_err(|e| format!("records: {e}"))?;
    let records = body["records"].as_array().ok_or("no records")?;
    let spend_pub = sk.spending.public_key();
    let proof_message = s(v, "proof_message")?;
    let mut found = Vec::new();
    for r in records {
        let eph = hex::decode(r["ephemeral_pubkey"].as_str().ok_or("eph")?).map_err(|_| "eph hex")?;
        let tag = r["view_tag"].as_u64().ok_or("tag")? as u8;
        let claimed = r["stealth_address"].as_str().ok_or("addr")?;
        let Some(a) = check_announcement(&sk.viewing, &spend_pub, &eph, tag) else { continue };
        if h(&a) != claimed.to_lowercase() {
            continue; // view tag collision (1/256): not ours
        }
        let key = compute_stealth_private_key(&sk.spending, &sk.viewing, &eph).map_err(|e| format!("{e:?}"))?;
        if key.address() != a {
            return Err("derived spending key does not match".into());
        }
        let proof = sign_personal(&key, proof_message).map_err(|e| format!("{e:?}"))?;
        found.push(json!({
            "stealth_address": cs(&a),
            "balance": rpc.balance(&a)?.to_string(),
            "proof_signature": h(&proof),
        }));
    }
    Ok(json!({ "records": records.len(), "matched": found.len(), "found": found }))
}

