# lowki-ens

ENS client primitives for Lowki, targeting ENSv2 on Sepolia (namechain tag
`v2-mainnet-deployment-1`). The crate covers ENSIP-15 names, a bounds-checked ABI codec, resolver,
UniversalResolver and ENSv2 registry/registrar calldata, a sans-IO ERC-3668 (CCIP-Read) client
that behaves like viem's, EIP-712 signatures for the new `LowkiRegistrar`, ENSIP-30 stealth
records and gateway formats, and the ENS name-owner identity key.

- `#![forbid(unsafe_code)]`. The crate is `no_std` + `alloc`. The default `normalize` feature
  brings in `std` through its dependency.
- Builds for `wasm32-unknown-unknown`, with and without default features.
- Does no IO and uses no randomness: the caller supplies keys, nonces, `secret`s and the
  transport. `getrandom` is not in the dependency tree.
- Dependencies:
  - `lowki-stealth` (path dependency). Used for `keccak256`, `SecretKey`, `StealthMetaAddress`,
    `generate_stealth_address`, `bip32` and EIP-55.
  - `k256` 0.13.4 (`arithmetic`, `ecdsa`), `zeroize` 1.9.1, `serde_json` 1.0.154 (alloc), and
    the optional `ens-normalize` 0.1.1.

## Public API

```rust
type Address = [u8; 20];  type U256 = [u8; 32];   // big-endian uint256
enum Error { Normalize(String), EmptyLabel, InvalidDnsName, InvalidUtf8, InvalidAbi,
             UnexpectedSelector, InvalidHex, InvalidLabel(&'static str), InvalidStealthRecord,
             MalformedMetaAddress, InvalidVerificationData, InvalidSharedSecret,
             InvalidPrivateKey, InvalidSignature, InvalidDerivation,
             Stealth(lowki_stealth::Error), Ccip(ccip::CcipError) }
// re-exported from lowki-stealth: Address, SecretKey, StealthMetaAddress, keccak256,
// to_checksum_address, generate_stealth_address, bip32

// name (ENSIP-1 / ENSIP-15 / DNS wire format)
normalize(&str) -> Result<String>                 // feature "normalize" (default)
labelhash(&str) -> [u8;32]                        // viem: "" -> 0, "[<64hex>]" -> that hash
namehash(&str) -> [u8;32]                         // viem semantics
child_node(&[u8;32], &[u8;32]) -> [u8;32]
parse_label_hash(&str) -> Option<[u8;32]>;  encode_label_hash(&[u8;32]) -> String
packet_to_bytes(&str) -> Vec<u8>                  // viem packetToBytes, byte for byte
dns_encode(&str) -> Result<Vec<u8>>               // same bytes, Err(EmptyLabel) on empty labels
dns_decode(&[u8]) -> Result<String>               // NameCoder.decode
namehash_dns(&[u8]) -> Result<[u8;32]>            // NameCoder.namehash(name, 0)
dns_labels_str(&[u8]) -> Result<Vec<String>>

// abi
enum Token { Word([u8;32]), Bytes(Vec<u8>), String(String), Array(Vec<Token>), Tuple(Vec<Token>) }
Token::{address, uint(u64), u256, bool, b32, bytes4, bytes, string, bytes_array, string_array}
encode(&[Token]) / encode_call([u8;4], &[Token]) / encode_sig(&str, &[Token]) -> Vec<u8>
selector(&str) -> [u8;4];  split_selector / expect_selector -> Reader
Reader::{new, word, u256, b32, u64, uint_max, address, bool, bytes4, frame, inline, bytes, string,
         array(i, elem_words), bytes_array, string_array, bool_array, address_array, u256_array}
decode_{bytes,string,address,bool,u256,bytes_array,string_array}(&[u8])
u256_from_u64, word_to_u64, bit(n), or(a,b), admin(role)

// resolver (profiles)
encode_addr(&node), encode_addr_coin(&node, coin: u64), encode_text(&node, key),
encode_data(&node, key) /*ENSIP-24*/, encode_contenthash(&node), encode_multicall(&[calls])
decode_addr_result -> Address, decode_addr_coin_result -> Vec<u8>, decode_text_result -> String,
decode_data_result / decode_contenthash_result -> Vec<u8>, decode_multicall_result -> Vec<Vec<u8>>,
decode_multicall_calldata;  evm_coin_type(chain_id), COIN_TYPE_ETH, COIN_TYPE_DEFAULT, *_SELECTOR

// universal_resolver
encode_resolve(dns, data), encode_resolve_with_gateways(dns, data, &[gateways])
decode_resolve_result(&[u8]) -> Result<(Vec<u8>, Address)>;  encode_resolve_result
enum UrError { ResolverNotFound{name}, ResolverNotContract{name,resolver},
  UnsupportedResolverProfile{selector}, ResolverError{error_data},
  ReverseAddressMismatch{primary,primary_address}, HttpError{status,message}, Error(String),
  Panic([u8;32]), Unknown(Vec<u8>) }
decode_ur_error(&[u8]) -> UrError  (never fails);  encode_ur_error(&UrError) -> Vec<u8>

// ccip (ERC-3668, sans-IO)
OffchainLookup { sender, urls, call_data, callback_function: [u8;4], extra_data }
OffchainLookup::{decode(&[u8]) -> Result<Self>, encode(), callback_calldata(response)}
callback_calldata(cb, response, extra) = cb || abi.encode(bytes response, bytes extra)
ccip_request(url_template, &sender, data) -> HttpRequest { method: Get|Post, url, body: Option<String> }
HttpRequest::content_type() -> Option<&str>;  is_allowed_url(&str)
HttpResponse { status: u16, content_type: Option<String>, body: String };  HttpFailure { Network, Timeout, BodyTooLarge }
parse_gateway_response(&HttpResponse) -> Result<Vec<u8>, GatewayError>
GatewayError { Http{status}, Request, Timeout, BodyTooLarge, Malformed, NoUrls,
               UnknownBatchSelector{selector,len}, InvalidBatch }::{short_message, encode_batch_failure}
BatchRequest { sender, urls, data };  encode_batch_query / decode_batch_query
encode_batch_response(&[bool], &[bytes]) / decode_batch_response
CcipReadSession::start(to, data) -> (Self, Step);  .on_eth_call(CallOutcome) -> Step;
    .on_http(Result<HttpResponse, HttpFailure>) -> Step;  .remaining_queries()
Step { EthCall{to,data}, Http(HttpRequest), Done(Vec<u8>), Failed(CcipError) }
CallOutcome { Success(Vec<u8>), Revert(Vec<u8>) }
CcipError { CallReverted(data), InvalidOffchainLookup, SenderMismatch{sender,to}, DepthExceeded,
            QueryBudgetExceeded, Gateway(GatewayError), InvalidBatchCall, UnexpectedInput }
trait Transport { fn eth_call(&mut self, &Address, &[u8]) -> Result<CallOutcome>;
                  fn http(&mut self, &HttpRequest) -> Result<HttpResponse, HttpFailure>; }
ccip_read(&mut impl Transport, to, &data) -> Result<Vec<u8>>
LOCAL_BATCH_GATEWAY_URL = "x-batch-gateway:true", MAX_LOOKUP_DEPTH = 4, MAX_QUERIES = 256

// permissioned_resolver (setters take the DNS-encoded name)
encode_set_text(dns, key, value), encode_set_data(dns, key, &bytes), encode_set_address(dns, coin, &bytes),
encode_set_contenthash(dns, &hash), encode_multicall(&[calls]),
encode_initialize(&[Grant { account, role_bitmap }], &[calls]),
encode_deploy_proxy(&impl, &salt, &data), encode_predict_proxy_address(&deployer, &salt), decode_address_result
ALL_ROLES, ADMIN_ROLES, ALL_SETTER_ROLES, ROLE_SET_{ADDRESS,TEXT,CONTENTHASH,ABI,INTERFACE,NAME,DATA}[_ADMIN],
ROLE_LINK[_ADMIN], ROLE_CAN_NAME[_ADMIN], ROLE_UPGRADE[_ADMIN]

// registry (PermissionedRegistry / UserRegistry)
ROLE_* (all RegistryRolesLib constants, incl. UNEMANCIPATED_ROLE_BITMAP), label_id(label)
encode_register(label, &owner, &subregistry, &resolver, &role_bitmap, expiry: u64),
encode_set_resolver(&any_id, &resolver), encode_set_subregistry(&any_id, &registry),
encode_get_state(&any_id), encode_grant_root_roles(&role_bitmap, &account), encode_initialize(&[Grant])
decode_state(&[u8]) -> Result<State { status: Available|Reserved|Registered, expiry, latest_owner, token_id, resource }>

// eth_registrar (ETHRegistrar + ERC-20)
make_commitment(label, &owner, &secret, &subregistry, &resolver, duration: u64, &referrer) -> [u8;32]
encode_commit, encode_register(label, owner, secret, subregistry, resolver, duration, payment_token, referrer),
encode_is_available, encode_get_register_price(label, duration, &token), decode_register_price -> (base, premium),
encode_make_commitment, encode_commitment_at, decode_commitment_at, decode_bool_result,
encode_erc20_approve(&spender, &value), encode_erc20_balance_of, encode_erc20_allowance, encode_mock_mint(&to, &amount)

// lowki_registrar
Registration { label: String, owner, resolver, duration: u64, nonce: u64, deadline: u64 }
domain_separator(chain_id, &registrar), registration_struct_hash(&r),
registration_digest(chain_id: u64, &registrar, &r) -> [u8;32]
sign_registration(&IdentityKey, chain_id, &registrar, &r) -> [u8;65]; recover_registration(...) -> Result<Address>
encode_register(&r, &signature), encode_nonces(&owner), decode_nonce -> u64, encode_available(label),
encode_renew(label, duration), validate_label(&str) -> Result<()>

// stealth (ENSIP-30 draft)
stealth_data_key(scheme, coin) -> "stealth-meta-address[<s>][<c>]", stealth_text_key(scheme),
parse_stealth_data_key(&str) -> Option<(u64,u64)>
encode_stealth_record(client_side_allowed, &meta) -> Vec<u8>;  decode_stealth_record(&[u8]) -> Result<StealthRecord>
encode_set_config(dns, &records_resolver, &[urls]), encode_set_chains(dns, &[coin: u64], enabled),
encode_set_meta_address(dns, &value), encode_clear_records(dns), encode_can_set_meta_address(dns, &account),
encode_can_set_config(dns, &account)
encode_gateway_response(&stealth, &vd) / decode_gateway_response -> (Address, Zeroizing<Vec<u8>>)
scheme1_verification_data(&ephemeral, &meta) -> Result<Zeroizing<[u8;96]>>      // e || S.x || S.y
gateway_derive(&meta, &ephemeral) -> Result<GatewayDerivation { stealth_address, ephemeral_pubkey,
                                    view_tag, verification_data: Zeroizing<[u8;96]> }>  (+ .gateway_response())
scheme1_verify(meta_address: &[u8], verification_data: &[u8], claimed: &Address) -> Result<Address>
decode_resolve_calldata(&[u8]) -> Result<(dns_name, inner_data)>
parse_addr_query(&[u8]) -> Option<AddrQuery { node, coin_type: U256, legacy }>
records_auth_message(name, timestamp) -> "Lowki stealth records\nname: {name}\ntimestamp: {timestamp}"
sign_personal(&SecretKey, &str) -> Result<[u8;65]>;  recover_personal(&str, &sig) -> Result<Address>

// sig
sign_digest(&SecretKey, &[u8;32]) -> Result<[u8;65]>, recover_digest, personal_message_hash,
sign_personal(&SecretKey, &[u8]), recover_personal(&[u8], &sig), eip712_digest(&ds, &sh)

// identity
identity_path(index) -> "m/137'/0'/{index}'"
IdentityKey::from_seed(&[u8;64], index) -> Result<Self>
IdentityKey::from_derive_at(index, |path: &str| -> Result<Zeroizing<[u8;32]>, E>) -> Result<Self, E>  (E: From<Error>)
IdentityKey::{from_secret_key, index, path, address, public_key, secret_key, sign_digest, sign_personal}
// owner_account (contracts/src/LowkiOwnerAccount.sol: EIP-7702 delegate for sponsored owner batches)
Call { target: Address, value: U256, data: Vec<u8> }
domain_separator(chain_id, &account) / call_hash(&Call) / execute_struct_hash(&[Call], nonce, deadline)
execute_digest(chain_id, &account, &[Call], nonce: u64, deadline: u64) -> [u8;32]   // == LowkiOwnerAccount.executeDigest on `account`
sign_execute(&IdentityKey, chain_id, &account, &[Call], nonce, deadline) -> Result<[u8;65]>  // Err if account != key.address()
encode_execute(&[Call], deadline, &sig) -> Vec<u8>;  encode_nonce()
authorization_rlp(chain_id, &delegate, nonce) / authorization_digest(...)            // keccak(0x05 || rlp([chainId, address, nonce]))
sign_authorization(&IdentityKey, chain_id, &delegate, nonce) -> SignedAuthorization { chain_id, address, nonce, y_parity, r, s }
```

## Design notes

**Identity key.** The identity key lives at `m/137'/0'/{index}'`, where 137 is the EIP number of
ENS. This path is deliberately not BIP-44. The address that owns a public ENS name must never
equal a Lowki per-app account (`m/44'/60'/{app}'/0/0`) or a stealth key (`m/5564'/0'/{0,1}'`),
because that would link the name to app activity. `Debug` prints only the index and the address.

**Signatures.** Signing uses RFC 6979 with low-s, and the output is `r || s || v` with v = 27/28.
It is byte-equal to viem's `sign`, `signMessage` and `signTypedData`, including digests ≥ n.
Recovery accepts v ∈ {0, 1, 27, 28}. It rejects high-s signatures, which viem accepts.

**CCIP-Read.** The client copies viem 2.57.16 `utils/ccip.ts` and
`utils/ens/localBatchGatewayRequest.ts`:

- At most 4 `OffchainLookup`s per read.
- `sender` must equal the called contract.
- URLs are tried in order, and the last error wins.
- One budget of 256 requests applies across callbacks and nested batches.
- In URL templates, only the first `{sender}` (lowercase) and the first `{data}` are replaced.
- A template with `{data}` is sent as GET. Anything else is sent as POST, with the exact body
  `{"data":"0x…","sender":"0x<EIP-55>"}` and `Content-Type: application/json`.
- The response is JSON `.data` when the Content-Type starts with `application/json`. Otherwise
  the raw body is used.
- Local batch failures are encoded as `HttpError(status, "HTTP request failed.")` or as
  `Error(<viem shortMessage>)`.
- The callback is `cb || abi.encode(bytes, bytes)`, sent to the original contract.

viem runs batch queries concurrently. The session runs them depth-first and one at a time. The
results and the budget accounting are the same.

**Names.** On the string side (`namehash`, `labelhash`), hashing follows viem: a `[<hex>]` label
is treated as an encoded labelhash. On the DNS side (`namehash_dns`), hashing follows the deployed
`NameCoder`, which hashes raw label bytes. The two differ only for `[hex]` labels and for labels
longer than 255 bytes. `dns_encode` returns viem's bytes but rejects empty labels: viem would emit
an early terminator, which `NameCoder` rejects.

**Scheme-1 verifier.** `scheme1_verify` re-implements `Scheme1Verifier.verify`. It reduces
`s_h mod n`. lowki-stealth and ScopeLift instead reject `s_h ≥ n`. The two disagree with
probability about 2⁻¹²⁸. The verifier also rejects keys whose `x ≥ n` (it cannot pass them to
`ecrecover`). `decode_stealth_record` accepts any valid compressed point, so it would accept such
a key (also about 2⁻¹²⁸).

## Verification

The vectors are in `vectors/lowki-ens/vectors.json` (5.3 MB, sha256
`a1c089afbb73586184534bc1a8096395a7a33536972dbc0a7c604f9d6ed8a18d`). `gen.ts` produces them from
a fixed-seed sha256 counter RNG (seed `lowki-ens-vectors-v1`). Regeneration is deterministic: two
runs give identical bytes.

References, pinned in `package.json`:

- viem **2.57.16** (latest at the time of writing)
- @adraffy/ens-normalize **1.11.1** (the version viem/ox use)
- @scure/bip32 **2.4.0**
- @noble/curves **2.4.0**
- tsx 4.23.15, on Node 24.22.1

The ABIs in `gen.ts` were written from the Solidity sources:

- namechain `v2-mainnet-deployment-1`: `PermissionedResolver`, `PermissionedRegistry`/`UserRegistry`,
  `ETHRegistrar`, `IEACGrantInitializable.Grant`, `IPermissionedRegistry.State`
- ens-contracts: `IUniversalResolver`, `AbstractUniversalResolver.resolveWithGateways`,
  `IBatchGateway`, `EIP3668`, resolver profiles
- `lib/verifiable-factory`: `IVerifiableFactory`
- stealth-ens-resolver: `StealthResolver`, `Scheme1Verifier`
- MockUSDC: the ABI in `deployments/sepolia/MockUSDC.json`

`gen.ts` evaluates the role constants from the `.sol` files themselves (`RegistryRolesLib`,
`PermissionedResolverLib`, `EACBaseRolesLib`).

| Area | Cases | Checked against |
|---|---|---|
| ENSIP-15 `normalize` | **38,613**: all of adraffy's official `validate/tests.json` @ v1.11.1 (21,466 pass, 17,147 must fail) | expected `norm`/`error` in tests.json, also re-checked against `ens_normalize` and viem `normalize` |
| namehash / labelhash / packetToBytes / dns_encode / dns_decode / namehash_dns | 138 names: empty, dots, `..`, newline cases, 255/256/300-byte labels, emoji, `[hash]` labels (either case), random normalized names | viem `namehash`, `labelhash`, `packetToBytes` |
| Calldata builders | 936 (39 functions × 24 random) | viem `encodeFunctionData` |
| Return / error decoders | 528 (14 kinds, including 216 UR errors: all 6 UR errors, `Error`, `Panic`, unknown) | viem `encodeFunctionResult` / `encodeErrorResult` |
| Generic ABI codec (decode **and** encode) | 64 tuples of `bytes, string, bytes[], string[], address[], uint256[], (address,uint256)[], bool, bytes32, uint64` | viem `encodeAbiParameters` |
| `OffchainLookup` decode + callback calldata | 48 | viem `encodeErrorResult` + `concat(cb, encodeAbiParameters)` |
| EIP-3668 request construction | 40 (13 URL templates: GET/POST, repeated placeholders, invalid schemes, padded URL) | viem `ccipRequest` with `fetch` stubbed and requests captured |
| Gateway response parsing + batch failure encoding | 52 (26 response shapes × 2: JSON/raw, content types, non-2xx, `null`, non-hex, network error, timeout) | viem `ccipRequest` result/error class, and viem `localBatchGatewayRequest` bytes |
| Local batch gateway | 50 (40 random nested batches with mixed OK/500/404/malformed/network/ftp URLs; nesting past the depth limit; unknown nested selectors `0x`, 1–3 bytes, wrong 4 bytes; empty batch; 360 URLs → budget) | viem `localBatchGatewayRequest` output bytes and the set of HTTP requests it sent |
| Full CCIP-Read flows | 49 (1–5 nested lookups, direct and batch, URL fallback, sender mismatch, depth 5, final revert, 301-URL budget case); 20 succeed, 29 fail | viem `client.call` with a table-driven `eth_call` mock and stubbed `fetch` (result, plus the set of HTTP requests) |
| EIP-712 `Register` digest + signature | 64 | viem `hashTypedData` / `signTypedData` |
| `personal_sign` records auth | 48 | viem `hashMessage` / `signMessage` / `recoverMessageAddress` |
| Raw digest signatures | 32 (including digests n−1, n, n+1, n+2) | viem `sign` |
| ENSIP-30 records | 82 (64 valid; 18 malformed: lengths 0/66/68, reserved flag bits, prefix 04/05, x not on the curve, x ≥ p, x = 0) | `@noble/curves` `Point.fromBytes` validity |
| Scheme-1 verification data / gateway derivation / verifier | 64 (including e = 1 and e = n−1) | `@noble/curves` point multiplication, with the derivation done mod n; Rust also checks lowki-stealth `check_announcement` and `compute_stealth_private_key` give the same address |
| Gateway `resolve()` calldata parsing | 32 | viem `encodeFunctionData` |
| Identity keys `m/137'/0'/{i}'` | 32 seeds (zero, all-ff, random; index 0, 2³¹−1, random) | `@scure/bip32` + viem `privateKeyToAddress` |
| Lowki label rules ⇒ normalized | 222 labels | adraffy `ens_normalize`: every label `validate_label` accepts equals its normalization |
| `owner_account` (LowkiOwnerAccount, EIP-7702) | 48 (0–4 calls, random values incl. 2²⁵⁶-range, random calldata; 4 chain ids + random; auth nonces 0/1/127/128/255/256/65535 + random) | viem `hashTypedData` (`Execute` with `Call[]`), `sign`, `encodeFunctionData(execute)`, `hashAuthorization`, `signAuthorization`; the contract's `executeDigest` is separately checked against forge's `eip712HashTypedData` in `contracts/test` |

Rust-only tests (`tests/unit.rs`):

- Selector constants.
- Every truncation of `OffchainLookup` / batch encodings.
- 3,000 random and mutated inputs into every decoder (no panics).
- Out-of-range and overflowing offsets and lengths, dirty padding, invalid UTF-8, enum range.
- Malformed DNS names.
- Session protocol errors: depth limit, sender mismatch, no URLs, invalid schemes, last error
  wins, wrong input kind, undecodable batch.
- Scheme-1 verifier rejections: lengths, prefixes, x = 0, x ≥ n, off-curve, e = 0, e ≥ n,
  tampered S, wrong viewing key.
- High-s and bad-v rejection.
- Redacted `Debug`.
- Identity index ≥ 2³¹.
- Label rules.

```sh
# regenerate vectors (Node 24). Needs network once: tests.json is fetched from
# raw.githubusercontent.com (adraffy/ens-normalize.js tag v1.11.1), checked against
# sha256 7b20fb69…3d089d and cached in vectors/lowki-ens/.cache/. Role constants are read from
# /Users/betterclever/projects/ethglobal/vendor/stealth-ens-resolver (VENDOR in gen.ts).
cd vectors/lowki-ens && npm install && npx tsx gen.ts

# test / lint / wasm
cd crates/lowki-ens
export CARGO_TARGET_DIR=/Users/betterclever/projects/ethglobal/lowki/target/lowki-ens
cargo test && cargo clippy --all-targets -- -D warnings
cargo build --release --target wasm32-unknown-unknown
cargo build --release --target wasm32-unknown-unknown --no-default-features   # without the normalizer
```

## Normalizer decision

Two existing crates were evaluated against all 38,613 official tests:

- **ens-normalize 0.1.1** (TateB/ens-normalize-rs)
- **ens-normalize-rs 0.2.0** (sevenzing)

Both bundle the same spec as @adraffy/ens-normalize 1.11.1 (created 2025-09-14, Unicode 17.0.0).
Both pass all 38,613 tests, and both build for wasm32. This crate uses **ens-normalize 0.1.1**:

- It has the smaller dependency tree (`serde`, `serde_json`, versus `anyhow`, `itertools`,
  `lazy_static`, `serde_with`, `thiserror` and `unicode-normalization`).
- It loads its data lazily (`LazyLock`).
- Its error messages follow adraffy's.

No subset implementation was needed.

Limits:

- The spec data (`spec.json` 3.3 MB and `nf.json` 49 KB) is embedded in the binary and parsed on
  first use. That adds megabytes to the wasm. Build with `--no-default-features` to leave out
  `name::normalize`, for example in the worker if names are normalized in JS.
- The normalizer follows the 1.11.1 spec. Future Unicode or spec updates need a crate bump and
  regenerated vectors.

## Known gaps and deviations from the brief

- **`scheme1_verification_data`** returns `Zeroizing<[u8; 96]>`, not `[u8; 96]`. The data holds
  the ephemeral private key.
- **`scheme1_verify`** takes the raw meta-address bytes and `claimed`, mirroring the contract.
  Empty bytes mean unverified mode, which returns `claimed`. Call it as
  `scheme1_verify(&meta.to_raw(), vd, &claimed)`.
- **`Transport::http`** returns `Result<HttpResponse, HttpFailure>`, not the crate `Result`. The
  failure kind (network or timeout) changes the batch-gateway error bytes.
- **`Registration.nonce` and `deadline`** are `u64`. They are `uint256` onchain and encode
  identically.
- **`decode_nonce`** errors above `u64::MAX`.
- **LowkiRegistrar.** The contract is being written in parallel. The digest is checked against
  viem's `hashTypedData` for the spec given here, not against the contract. Re-check once its
  `REGISTER_TYPEHASH` exists.
- **CCIP: transport-level behavior.**
  - Timeouts and the 10 MiB limit are the transport's job. `parse_gateway_response` checks the
    body size after the fact.
  - URL validation approximates WHATWG `new URL()`: the scheme must be `http` or `https` after
    trimming C0 controls and spaces.
  - JSON parsing uses `serde_json`, which rejects lone surrogate escapes that `JSON.parse`
    accepts.
- **CCIP: odd-length hex.** A gateway answer of odd-length hex (`0x123`) is rejected as
  malformed. viem would pass it on.
- **CCIP: nested batch error text.** For a nested batch query whose selector matches `query` but
  whose arguments are malformed, the failure text is `Invalid batch gateway request.`. viem uses
  its ABI decoder's message there. Unknown selectors and short data do match viem byte for byte.
- **ABI decoding** is stricter than viem:
  - non-canonical address, bool and `uintN` padding is rejected;
  - `string` must be UTF-8;
  - the trailing zero padding of `bytes` is not required, as in viem.
- **`decode_stealth_record`** treats an empty value (no record) as an error. Callers should map
  that to "not stealth" per ENSIP-30.
- **Gateway / `StealthResolver` details.** `extraData`
  (`abi.encode(name, coinType, legacy, binding)`) is not decoded: the gateway does not need it.
  Neither the batch `multicall` callbacks of `StealthResolver` nor `verifyStealthResolution` are
  wrapped.
- **`UniversalResolver.reverse`** is not implemented.
- **`ETHRegistrar.renew`** (`RenewData` struct) is not implemented.
- **Zeroization.** k256 `Scalar`/`ProjectivePoint` temporaries are `Copy` and are not wiped (same
  as lowki-stealth). Secrets are held in `SecretKey`, `Zeroizing` buffers and `IdentityKey`.
