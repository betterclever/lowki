# lowki-worker

The single wasm-bindgen module that holds Lowki keys. It runs in a dedicated Web Worker
inside the Lowki iframe, and it is the only place where the seed exists. It combines
`lowki-keys`, `lowki-shares`, `lowki-stealth`, `lowki-pp-core`, `lowki-groth16` (with
`lowki-gpu`) and `lowki-aa` behind one typed message API.

```js
// in the iframe
const w = new Worker(new URL('./js/worker.js', import.meta.url), { type: 'module' });
w.postMessage({ id: 1, request: { type: 'unlock', prfOutput, wrappedBlob } });
// -> { id: 1, response: { type: 'unlocked', expiresAtMs } }
```

`js/worker.js` is bindings only: it queues messages and calls `LowkiWorker.handle`.
`pkg/web` holds the ES module (`make pkg`), and `pkg/node` holds the CommonJS build for
Node and tests. The Rust API is `LowkiWorker::new(autoLockMinutes?)`,
`async handle(request) -> response`, `lock()` and `isUnlocked()`. Natively, use
`Core::prepare` + `run_proof` / `load_circuits`, or `handle_blocking`.

## Message protocol

Every request is a plain object with a `type` and camelCase fields, and so is every response.
`handle` always resolves; failures come back as `{type: "error", code, message}`.

Field types:
- **bytes**: a `Uint8Array`. Used for everything secret-bearing (PRF output, shares, wrapped
  blobs) so the caller can `.fill(0)` it.
- **qty**: a decimal or `0x` hex string.
- **addr**: a 20-byte hex string.

`note` = `{accountIndex = 0, chainId: qty, entrypoint: addr, depositIndex, secretIndex = 0}`.

| Request | Fields | Response |
|---|---|---|
| `createWallet` | `prfOutput` bytes(32) | `walletCreated {wrappedBlob, wrappedPasskeyShare, serverShare, recoveryShare, shareXs{passkey,server,recovery}, expiresAtMs}`; unlocks |
| `importMnemonic` | `mnemonic` (24 words), `prfOutput` | same as `createWallet` |
| `unlock` | `prfOutput`, `wrappedBlob` | `unlocked {expiresAtMs}` |
| `recover` | `serverShare`, `recoveryShare` | `unlocked` |
| `recoverWithPasskeyShare` | `prfOutput`, `wrappedPasskeyShare`, `otherShare` (S or R) | `unlocked` |
| `rewrap` (after recover, for a new passkey) | `prfOutput`, optional `passkeyShareX` + `serverShare` + `recoveryShare` | `rewrapped {wrappedBlob, wrappedPasskeyShare?}` |
| `lock` | – | `locked` |
| `status` | – | `status {unlocked, expiresAtMs, circuits, gpu}` |
| `appAccount` | `origin` | `appAccount {origin, address, pairwiseUserId, appIndex}` |
| `stealthMetaAddress` | – | `stealthMetaAddress {uri, spendingPublicKey, viewingPublicKey}` |
| `signMessage` | `origin`, `message` (UTF-8) or `messageHex` | `signature {address, signature, hash}` (EIP-191) |
| `signTypedData` | `origin`, `typedData` `{types, primaryType, domain, message}` | `signature` (EIP-712; `EIP712Domain` may be omitted) |
| `signTransaction` | `origin`, `tx {type: "eip1559"\|"eip7702", chainId, nonce, maxPriorityFeePerGas, maxFeePerGas, gas, to?, value?, data?, accessList?, authorizationList?}` | `signedTransaction {address, rawTransaction, hash}` |
| `signAuthorization` | `origin`, `chainId`, `address`, `nonce` | `signedAuthorization {address, authorization{chainId,address,nonce,yParity,r,s}}` |
| `ppNote` | `note`, optional `value` + `label` | `ppNote {precommitment, nullifierHash, commitment?}` (public hashes, decimal) |
| `ppShield` | `note` (secretIndex 0), `value` | `ppShield {precommitment, tx{to,data,value}}` (Entrypoint `deposit`) |
| `ppLoadCircuits` | `withdrawWasm`, `withdrawZkey`, `commitmentWasm`, `commitmentZkey` (bytes, pinned sha256 enforced by lowki-groth16), `useGpu` | `circuitsLoaded {circuits, witnessBackend, gpu, gpuError?}` |
| `ppProveWithdraw` | `note`, `label`, `existingValue`, `withdrawnValue`, `context` (`{context}` or `{withdrawal{processooor,data}, scope}`), `stateProof`, `aspProof` (`{root, index, siblings[32]}`, Kohaku `proofFromTree`) | `proof {circuit, proof, publicSignals, solidity, backend: "webgpu"\|"cpu", ms}` |
| `ppProveCommitment` | `note`, `value`, `label` | `proof` (ragequit) |
| `ppBuildPaymasterWithdraw` | `note`, `withdrawal`, `proof` (the `solidity` object), `pool`, `paymaster`, `adapter`, `gas{…5 limits}`, `maxFeePerGas`, `maxPriorityFeePerGas`, `nonce`, `authorizationNonce?` | `paymasterUserOp {sender, userOpHash, userOperation}` (`eth_sendUserOperation` JSON incl. `eip7702Auth`) |
| `kohakuDeriveAt` | `path` | `kohakuKey {key}` (only below `m/28784'/1'` or allowed paths) |
| `kohakuAllowPath` | `path` | `ok` |
| `ppSignNoteEnvelope` | `accountIndex`, `chainId`, `entrypoint`, `depositIndex`, `secretIndex` | `noteSignature {signature}` (Kohaku `EnvelopeSigner`) |
| `sealAppKey` | `origin`, `enclavePublicKey` (32-byte X25519 hex), `grant {signerId, policyIds, expiry, nonce?}` | `sealedAppKey {suite, address, enc, ciphertext, grant, grantHash, grantSignature}` |
| `ensIdentity` | `index = 0` | `ensIdentity {index, path, address}`: the name-owner key `m/137'/0'/{index}'` (address only) |
| `ensSignRegistration` | `chainId`, `registrar`, `label`, `resolver?` (zero), `duration`, `nonce`, `deadline`, `index = 0` | `ensRegistration {owner, signature}`: EIP-712 `Register` for `LowkiRegistrar`; the label rules are checked first |
| `ensSignOwnerSetup` | `chainId`, `name`, `ownerAccount`, `authNonce`, `execNonce`, `deadline`, `factory`, `permissionedResolverImpl`, `stealthResolver`, `registry`, `gatewayUrl`, `recordsResolver`, `index = 0` | `ensOwnerSetup {owner, executeSignature, authorization{chainId,address,nonce,yParity,r,s}, record, metaAddress}`: the EIP-7702 authorisation (delegate `LowkiOwnerAccount`) and the EIP-712 `Execute` over the five setup calls (`ens::owner_setup_calls`) |
| `ensRecordsAuth` | `name`, `timestamp` | `recordsAuth {name, timestamp, signature, viewingAddress}`: `personal_sign` by the stealth **viewing** key of `records_auth_message(name, timestamp)` (lowki-server `GET /v1/ens/records`) |
| `stealthScan` | `records: [{stealthAddress, ephemeralPubkey, viewTag}]` | `stealthScan {scanned, found: [{stealthAddress, ephemeralPubkey}]}`: view tag, then the derived address must equal the record's |
| `stealthSignTransaction` | `ephemeralPubkey`, `stealthAddress`, `tx` (as `signTransaction`) | `signedTransaction`: signed by the stealth key of a found payment, derived here and never returned; refused if it doesn't control `stealthAddress` |
| `appListSeal` | `data` (UTF-8, ≤ 32 KiB) | `sealedAppList {blob}` (hex) |
| `appListOpen` | `blob` (hex) | `appList {data}`; `unwrapFailed` for another wallet's or a tampered blob |

Error `code`s: `locked`, `alreadyUnlocked`, `invalidRequest`, `unwrapFailed`,
`invalidShares`, `pathNotAllowed`, `circuitsNotLoaded`, `prover`, `signing`, `seal`, `internal`.

## Key material and formats

- **Seed:** 32 bytes of entropy (a 24-word BIP-39 phrase), then the BIP-39 seed with an
  empty passphrase. lowki-keys only wraps 32 bytes, so this is **not** the 16 bytes that
  ARCHITECTURE.md describes; see "Needs changing".
- **wrappedBlob:** lowki-keys `wrap_entropy`, which is
  `0x01 || nonce12 || AES-256-GCM(HKDF(prf,"lowki/kek/v1"), entropy, aad "lowki/seed/v1")`
  (61 bytes). The nonce comes from WebCrypto.
- **Shares:** lowki-shares 2-of-3 over the entropy, 33 bytes each (Privy format). They are
  assigned in split order: P, S, R.
  - **P** never leaves in clear: `wrappedPasskeyShare = 0x02 || nonce12 || AES-256-GCM(same KEK, P, aad "lowki/share-p/v1")` (62 bytes).
  - **S** goes to lowki-server.
  - **R** is the recovery code.
  - The x-coordinates are public (`shareXs`). `rewrap` uses them to re-derive the same P from
    S + R for a new passkey, after checking that S + R reproduce the unlocked seed.
- **App account:** lowki-keys `app_private_key(seed, origin)`, at `m/44'/60'/{app_index}'/0/0`.
  `origin` must be a serialized origin (`event.origin`): lowercase, no path, no default port.
  Anything else is rejected, so each app has exactly one spelling.
- **Pairwise user ID:** `HMAC-SHA256(K, "lowki/pairwise-id/v1" || 0x00 || origin)`, with
  `K = HKDF-SHA256(ikm = seed, salt = "", info = "lowki/pairwise-id/key/v1")`. It is
  domain-separated from the app index (`keccak("lowki/app/v1" || origin)`) and from the
  stealth and Privacy Pools trees. It is returned as 32-byte hex.
- **Stealth:** lowki-stealth `m/5564'/0'/{0,1}'`, as an ERC-5564 `st:eth:` URI.
- **Privacy Pools:** Kohaku's `SignatureSecretManager`, with the identity key at
  `m/28784'/1'/{accountIndex}'`.
  - Existing note = `secretIndex`; change note = `secretIndex + 1`; both tree depths are 32,
    exactly as in Kohaku's `withdrawThunk`.
  - The ephemeral userOp sender is `deriveEphemeralSigner(depositIndex, withdrawIndex = secretIndex)`.
  - Nullifiers, salts, the identity key and the ephemeral key are never returned (except
    through the Kohaku requests below).
- **Delegation (`sealAppKey`, a stub of ARCHITECTURE.md step 5):**
  - Encryption is HPKE RFC 9180, base mode, with **DHKEM(X25519, HKDF-SHA256) 0x0020 /
    HKDF-SHA256 0x0001 / ChaCha20-Poly1305 0x0003** (`hpke` 0.13).
  - `info = "lowki/delegation/v1"`, `aad = grantHash`, and the plaintext is the 32-byte app key.
  - The grant is EIP-712 `LowkiDelegationGrant(address account,string signerId,string[] policyIds,uint64 expiry,bytes32 nonce,bytes32 enclaveKey)`
    in domain `{name: "Lowki Delegation", version: "1"}`, signed by the app key.
    `enclaveKey` binds the grant to the recipient.
  - **Enclave attestation is not checked here.** The caller must pass only an attested key.

- **ENS name owner (`src/ens.rs`):** lowki-ens `IdentityKey` at `m/137'/0'/{i}'`, never an
  app account or a stealth key, so the public name links to nothing else. It only signs: the
  operator (lowki-server) pays for registration and for the owner batch, and rebuilds the batch
  calls itself before relaying (a byte-identical copy of `ens::owner_setup_calls`).
- **App list:** `0x01 || nonce12 || AES-256-GCM(K, json, aad "lowki/app-list/v1")` with
  `K = HKDF-SHA256(ikm = seed, salt = "", info = "lowki/app-list/key/v1")`, domain-separated
  from the pairwise-ID key and the KEK. The key never leaves the worker; lowki-server stores only
  the ciphertext (`PUT /v1/me/app-list`).
- lowki-ens is built **without** its normalizer (`default-features = false`): Lowki labels are
  ASCII by rule, which keeps the wasm at 4.2 MB.

## Session and auto-lock

- `autoLockMinutes` defaults to 15. The deadline is set when the session unlocks; activity
  does not extend it.
- Two mechanisms lock the session:
  1. every request checks the deadline first;
  2. a `setTimeout` (unref'd under Node) locks at the deadline even if no message arrives.
- Locking drops the `Session`. Its entropy, seed and scoped keystore are all
  `Zeroizing`/`ZeroizeOnDrop`.
- Circuits stay loaded across locks; they contain no secrets.
- `createWallet`/`importMnemonic`/`unlock`/`recover` refuse to run while unlocked
  (`alreadyUnlocked`), so a session is never silently replaced.

## Secrets hygiene

- **Debug output:** `SecretBytes`, `SecretString` and `SecretHex` print only a length or `..`.
  `Request`/`Response` `Debug` output is therefore safe to log; a test checks this.
- **Error messages:** these are fixed strings or contain only public data. serde parse errors
  are passed through only when they name a field or variant ("missing field", "unknown
  variant", …). Any other parse error becomes a generic message, because serde would quote the
  offending value (for example a PRF output sent as a string).
- **What crosses into JS by design:**
  - the PRF output (WebAuthn returns it to JS);
  - the shares and wrapped blobs (they must be stored or sent);
  - `kohakuDeriveAt` keys and `ppSignNoteEnvelope` signatures (Kohaku's Host contract; see
    `packages/lowki-kohaku-host`);
  - the `sealAppKey` ciphertext.
- JS strings and engine temporaries cannot be wiped by any library.

## Verification

All commands run from `crates/lowki-worker` with
`CARGO_TARGET_DIR=/Users/betterclever/projects/ethglobal/lowki/target/lowki-worker`.

| Check | Command | Result |
|---|---|---|
| Native lifecycle: create → lock → unlock (and wrong PRF refused) → recover S+R → same address and pairwise ID → rewrap with a new passkey → unlock → P+S and new-P+R recovery → duplicate share rejected → auto-lock at the deadline → signature recovers to the address → origin validation | `cargo test --release --test flow` | pass |
| Kohaku scope: `m/28784'/1'/0'` == lowki-keys `derive_at`. `m/28784'/1'` itself, other trees and unparsable paths are refused. `kohakuAllowPath` works. The note envelope signature == lowki-pp-core `sign_note` (which is itself vector-equal to Kohaku/viem) | same | pass |
| No secrets in `Debug` or error messages | same | pass |
| Native proofs: withdraw against a real 41-leaf LeanIMT state tree + ASP tree with the real Sepolia pool `SCOPE`. Public signals checked (nullifier hash, value, context, new commitment == `ppNote(secretIndex+1)`). `lowki-groth16::verify` true. Over-withdraw rejected. Commitment proof. Paymaster userOp sender == the note's ephemeral signer | same | pass |
| **Differential vs viem 2.54.1 / @scure 2.2.0 / @noble 2.2.0** (`vectors/lowki-worker/gen.ts`): 18 app addresses + pairwise IDs, 63 personal_sign, 27 EIP-712 (spec Mail, nested arrays/bytes/int/bool/unicode with inferred domain, salt-only domain), 108 transactions (54 EIP-1559 incl. contract creation and access lists, 54 EIP-7702 with 1–2 authorizations), 27 7702 authorizations | `cargo test --release --test vectors` | all byte-identical |
| HPKE + grant interop: Rust seals; **@hpke/core 1.10.0 + @hpke/chacha20poly1305 1.8.0** open it. The plaintext is the account key. `grantHash` == viem `hashTypedData`. The signature recovers to the account. A tampered aad fails | `cargo test --release --test flow seal_app_key_writes_js_check && (cd ../../vectors/lowki-worker && npx tsx open-sealed.ts)` | 2/2 |
| **Headless Chrome 156, inside a dedicated Web Worker, through the JS API:** create → lock → unlock → recover → same address and pairwise ID → personal_sign + EIP-712 → `ppLoadCircuits(useGpu)` → 2 withdraw proofs on **WebGPU** + commitment → `lowki-groth16::verify` → a CPU-only worker proves and verifies a withdraw → the auto-lock timer fires with no message | `make test-chrome` on the host (see below) | pass |
| snarkjs 0.7.5 verifies the browser proofs (on Linux) and the native proofs | `cd vectors/lowki-worker && npx tsx verify-proofs.ts out/worker-proofs.jsonl out/native-proofs.jsonl` | 4/4 and 2/2 |
| **Deployed Sepolia withdrawal verifier** (`0x822f…A00a`, read from the pool's `WITHDRAWAL_VERIFIER()`): `eth_call verifyProof` | `npx tsx onchain-verifier.ts out/worker-proofs.jsonl out/native-proofs.jsonl` | true for every real proof, false for tampered pA, un-swapped pB and +1 public signal |
| ENS + stealth + app list (`tests/ens.rs`): identity = lowki-ens `IdentityKey` for 3 indices × 2 seeds; `Register` recovers to the owner; the `Execute` signature binds the rebuilt calls and the 7702 authorisation recovers to the owner; records auth recovers to the viewing key; label rules refused before signing; scan finds exactly the wallet's addresses (a forged record with our ephemeral key is rejected); stealth-signed tx; app list round trip, fresh nonces, other wallet / tampered blob refused, lock | `cargo test --release --test ens` | pass |
| **Differential vs viem 2.54.1** for the above (`vectors/lowki-worker/ens-check.ts` on `out/ens-worker.json`): identity addresses (`mnemonicToAccount` at `m/137'/0'/i'`), `verifyTypedData` for `Register` and `Execute(Call[])`, `recoverAuthorizationAddress`, `recoverMessageAddress` (records auth), `recoverTransactionAddress` (stealth tx) | `cargo test --release --test ens && (cd ../../vectors/lowki-worker && npx tsx ens-check.ts)` | 32/32 |
| Live Sepolia: these requests (Node build) claim a name through lowki-server and the name resolves with plain viem | `node apps/wallet-ui/scripts/claim-live.mjs test-<x>` | `test-c8d16c.lowki.eth` (see wallet-ui README) |
| clippy `-D warnings`: native, `--no-default-features`, wasm32, wasm32 `--no-default-features` | `make clippy` (+ wasm32 no-default) | clean |
| wasm32 build | `make wasm-build` | 5.3 MB release (4.0 MB after wasm-bindgen) |

Browser timings (Chrome 156 headless, Apple M5 Max, inside a dedicated worker):

| Step | Time |
|---|---|
| `ppLoadCircuits` (both circuits, sha256 pin, zkey parse, GPU upload) | 205–215 ms |
| withdraw on WebGPU | 179–233 ms |
| commitment on WebGPU | 61–86 ms |
| withdraw on the CPU | 2.12 s |

The first load took 10.5 s, because serde-wasm-bindgen copies a `Uint8Array` into Rust one
element at a time. `wasm_api` now bulk-copies the four artifact buffers.

Run the browser test from the VM. Build in the VM, then run on the host:

```sh
cargo test --release --target wasm32-unknown-unknown --test browser --no-run
mac sh -lc 'cd <lowki>/crates/lowki-worker/browser && CHROMEDRIVER=<lowki>/target/tools/chromedriver/mac_arm-156.0.8078.80/chromedriver-mac-arm64/chromedriver \
  <lowki>/target/tools/bin/wasm-bindgen-test-runner <lowki>/target/lowki-worker/wasm32-unknown-unknown/release/deps/browser-*.wasm --nocapture' \
  > ../../vectors/lowki-worker/out/chrome-worker.log
grep -o 'LOWKI_PROOF_JSON .*' ../../vectors/lowki-worker/out/chrome-worker.log | sed 's/^LOWKI_PROOF_JSON //' > ../../vectors/lowki-worker/out/worker-proofs.jsonl
```

`browser/webdriver.json` holds the Chrome flags (`--headless=new --enable-unsafe-webgpu …`).
The tests embed the circuit artifacts from `vendor/npm/circuits/package/artifacts`.

## Known gaps

- **Attestation is stubbed:** `sealAppKey` trusts the enclave key it is given.
- **Delegated-grant format:** the format is Lowki's own, and lowki-signer must verify exactly
  this EIP-712 type. No RFC 8785 canonical form is used.
- **Recovery code R is 33 raw bytes.** Its human encoding (words, base32, …) is up to the UI.
- **Wrong shares:** a wrong S or R that still combines opens a different wallet. Shares carry
  no MAC. The UI should compare the address.
- **`rewrap` needs `passkeyShareX`** (returned by `walletCreated`; it is public) to re-derive P.
- **`ppProveWithdraw` takes the Merkle proofs from the caller** (Kohaku state sync).
  - Bad proofs fail in the circuit; there is no early root check.
  - Depths are fixed at 32, as in Kohaku.
- **Native ETH only for shielding:** `ppShield` covers native ETH. ERC-20 shields
  (lowki-aa `prepare_erc20_shield`) are not wired up.
- **Only Chrome was tested** (host Metal WebGPU). Safari, Firefox and other GPUs are untested.
- **Concurrency:** requests are serialized by `js/worker.js`. A re-entrant `handle` while
  another is awaiting a proof returns `internal: worker busy`.
