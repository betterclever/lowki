# @lowki/kohaku-host

Kohaku's `@kohaku-eth/plugins` `Host` (`network`, `storage`, `keystore`, `provider`,
optional `externalSyncProvider`) implemented on the Lowki Rust/wasm worker
(`crates/lowki-worker`). Kohaku's Privacy Pools v1 plugin (`PrivacyPoolsV1Protocol`) runs
unchanged on top of it. Kohaku is consumed as **source** from the pinned read-only clone
`vendor/kohaku` @ `8c83d64050040654082d70901a46a339bfa103b0`.

```
npm install
npm test                 # unit tests, real wasm worker if crates/lowki-worker/pkg/node exists
npm run test:stub        # same tests against the TEST-ONLY JS stub worker
npm run typecheck        # tsc --noEmit over src/test/scripts + the Kohaku sources they import
npx tsx --import ./resolve-hook.mjs scripts/sepolia-sync.ts   # read-only Sepolia sync (npm run sepolia-sync)
```

## Layout

| File | What |
|---|---|
| `src/protocol.ts` | Wire types for the worker requests used here; `LowkiWorkerError` |
| `src/transport.ts` | `WorkerTransport`; `InProcessTransport` (wraps a `LowkiWorker`, Node/tests); `WebWorkerTransport` (dedicated Web Worker, postMessage with request ids, timeouts) and `serveLowkiWorker` (the worker-side loop) |
| `src/keystore.ts` | `LowkiKeystore`: Kohaku `Keystore` over `kohakuDeriveAt` / `kohakuAllowPath`; modes `derive` and `signer-only` |
| `src/envelope-signer.ts` | `LowkiEnvelopeSigner` (Kohaku `EnvelopeSigner` over `ppSignNoteEnvelope`), `validateNoteEnvelope`, `lowkiSecretManager` factory |
| `src/storage.ts` | `EncryptedStorage` (AES-256-GCM via WebCrypto) over `MemoryBackend` / `IndexedDBBackend` / `LocalStorageBackend`; `StorageKeyProvider` hook |
| `src/network.ts` | `createNetwork`: fetch passthrough, optional host allowlist, https-only |
| `src/provider.ts` | `createRpcProvider`: Kohaku's viem adapter over a viem client whose fetch goes through the host `Network`; several URLs use a viem `fallback` transport |
| `src/host.ts` | `createLowkiHost({ transport, storage, network, provider, wiring })` → `{ host, keystore, secretManager }` |
| `src/kohaku-pp.ts` | The single import point for `@kohaku-eth/privacy-pools` (deep source imports, see Kohaku issues) |
| `resolve-hook.mjs`, `resolve-hook-impl.mjs` | Node resolve hook: `@kohaku-eth/*` → Kohaku sources, Kohaku's `~/` alias, Kohaku's bare deps → this package's `node_modules` |
| `scripts/load-worker.ts` | Picks the real wasm worker or the stub (`LOWKI_WORKER=real\|stub`) |
| `scripts/sepolia-sync.ts` | Read-only Sepolia run |
| `test/stub-worker.ts` | **TEST-ONLY** JS stub of the worker (keys in JS). Never ship. |
| `test/host.test.ts` | Unit tests |

Dependency versions are exact and match Kohaku's `pnpm-lock.yaml` (viem 2.54.1, ox 0.12.4,
@reduxjs/toolkit 2.12.0, @saga-sync/client 0.3.0, maci-crypto 2.5.0,
@fatsolutions/privacy-pools-core-circuits 1.0.5, @scure/bip32 2.2.0, @scure/bip39 2.2.0,
@noble/hashes 2.2.0, ethers 6.17.0 (needed by Kohaku's `0xbowAsp.service.ts`)).

## Usage

```ts
const transport = new WebWorkerTransport(new Worker(new URL("./lowki-worker.js", import.meta.url), { type: "module" }));
const network = createNetwork({ allowHosts: ["sepolia.gateway.tenderly.co", "ipfs.filebase.io"] });
const { host, secretManager } = createLowkiHost({
  transport,
  network,
  provider: createRpcProvider("https://sepolia.gateway.tenderly.co", network),
  storage: new EncryptedStorage(new IndexedDBBackend(), storageKeyFromWorker),
  wiring: "envelopeSigner",          // or "deriveAt"
});
const pp = new PrivacyPoolsV1Protocol(host, { entrypoint, secretManager /* mode 2 only */ });
```

Inside the dedicated worker script: `serveLowkiWorker(self, new LowkiWorker(15))`.

## Two ways to wire the keystore — what crosses the JS boundary

Kohaku's default secret scheme is `SignatureSecretManager`
(`packages/privacy-pools/src/account/signature-keys.ts`): every note's secrets come from one
deterministic EIP-712 signature σ over `PrivacyPoolNote(depositIndex, secretIndex)` with domain
`{name: "PrivacyPools Keychain", version: "1", chainId, verifyingContract: entrypoint, salt:
keccak256("privacypools")}`, signed by the account identity key at `m/28784'/1'/{account}'`.
From σ, **Kohaku computes in JS**: `nullifier = trunc31(keccak("nullifier" ‖ r ‖ lowS(s)))`,
`salt = trunc31(keccak("salt" ‖ …))`, `precommitment = poseidon(nullifier, salt)`,
`nullifierHash = poseidon(nullifier)` and, for paymaster withdrawals, the ephemeral 7702 sender
key `trunc31(keccak("signer" ‖ …))` (`signature-keys.ts:111-263`).

### Mode 1 — `wiring: "deriveAt"` (Kohaku's Host contract)

`keystore.deriveAt(path)` → worker `kohakuDeriveAt`. Kohaku's own `KeystoreEnvelopeSigner`
(`signature-keys.ts:152-163`) then signs with viem in JS.

| Direction | Data |
|---|---|
| JS → worker | `{type:"kohakuDeriveAt", path:"m/28784'/1'/{account}'"}` once per note signature (Kohaku re-derives on every `signEnvelope`; signatures are memoized per note) |
| worker → JS | **the account's Privacy Pools identity private key** (32 bytes, as a hex *string* in the JS heap: not zeroizable, may be retained by GC) |
| in JS | ECDSA signing of every note envelope; σ; all note secrets (see above) |
| JS → worker (delegation only) | `{type:"kohakuAllowPath", path}` then `kohakuDeriveAt(path)` for a paymaster `delegation: {mode:'deterministic', path}`; that wallet key crosses too. Call `host keystore.allowPath(path)` first: Kohaku adds the path to its own JS scope (`plugin/base.ts:428-430`), but Rust only learns it from `kohakuAllowPath` |

Anyone who reads the JS heap once gets the identity key, i.e. every past and future note
secret of that account on every chain/entrypoint, permanently.
Scope: Rust refuses anything not strictly below `m/28784'/1'` and not registered; `LowkiKeystore`
pre-checks the same in JS, and Kohaku wraps it again with `scopeKeystore` (`plugin/base.ts:128`).
The legacy HD `SecretManager` (`keys.ts`, deprecated) would pull deeper children
(`m/28784'/1'/{account}'/…`) through the same request.

### Mode 2 — `wiring: "envelopeSigner"` (default here)

`createLowkiHost` returns a `secretManager` factory to pass to `PrivacyPoolsV1Protocol`; it is
Kohaku's own `SignatureSecretManager` with `signer: LowkiEnvelopeSigner`. Before calling the
worker, `signEnvelope` checks the envelope is exactly Kohaku's (domain fields/name/version/salt,
`types` = only `PrivacyPoolNote` with exactly `depositIndex: uint64, secretIndex: uint64` in that
order, `primaryType`, message keys, indices in `[0, 2^32)`), so nothing else can be signed with the
identity key through this path. It also recovers the signer of each σ and requires it to stay the
same address (exposed as `identityAddress`, public). The keystore runs in `signer-only` mode and
refuses **every** path below `m/28784'/1'` in JS.

| Direction | Data |
|---|---|
| JS → worker | `{type:"ppSignNoteEnvelope", accountIndex, chainId (decimal), entrypoint, depositIndex, secretIndex}` — public parameters only; Rust rebuilds the envelope itself |
| worker → JS | the 65-byte signature σ for that one note |
| in JS | σ and the secrets Kohaku derives from it (nullifier, salt, ephemeral sender key) for the notes the session touches |
| never in JS | the identity key `m/28784'/1'/{account}'`, the seed, every other HD key |
| delegation only | as in mode 1 (`allowPath` + `kohakuDeriveAt` for that one wallet path) |

σ is a per-note secret: whoever has it can spend/ragequit that note (with a proof). What mode 2
removes is the *permanent, account-wide* exfiltration: after lock, nothing more can be signed,
and a heap dump only holds the notes the session derived. While unlocked, compromised JS could
still ask the worker for σ of any (chain, entrypoint, index): the worker is a signing oracle for
this one envelope type.

Kohaku's determinism check (`verifyDeterminism`, default on) signs the first envelope of a
session twice; the worker is RFC 6979, so `verifyDeterminism: false` is safe and saves one call.

### Proving

Kohaku proves in JS with its own `Prover` (`@fatsolutions/privacy-pools-core-circuits`, snarkjs):
`withdrawThunk.ts:82-91` passes `existingNullifier`, `existingSecret`, `newNullifier`, `newSecret`
as witness inputs, and `ragequitThunk.ts:51` does the same for commitment proofs. So in **both**
modes the note secrets are in JS whenever the plugin proves. The worker already has
`ppProveWithdraw` / `ppProveCommitment` that take a note reference and derive the secrets in
Rust. To use them without secrets in JS, Kohaku would need (a) an `ISecretManager` that may return
only the public hashes (`precommitment`, `nullifierHash`) plus an opaque note reference, with the
`userSecrets` slice storing that instead of `nullifier`/`salt`, and (b) a prover seam taking
`(noteRef, label, values, context, state/ASP Merkle proofs)` instead of the witness. A
`proverFactory` adapter mapping Kohaku's witness onto `ppProveWithdraw` is possible today, but
the secrets would still have passed through JS (Kohaku computed them), so it only buys speed
(WebGPU), not isolation. Likewise `deriveEphemeralSigner` puts the paymaster sender key in JS;
the worker's `ppBuildPaymasterWithdraw` signs that userOp in Rust but is not reachable through
Kohaku's interfaces.

## Storage

`EncryptedStorage` encrypts each value with AES-256-GCM: `lk1:` + base64(iv12 ‖ ct ‖ tag),
AAD = the storage key name (a ciphertext can't be moved to another key). Key *names* are in clear
(Kohaku uses `privacy-pool-state-<chainId>-<entrypoint>` and `privacy-pool-history-…`). The key is
supplied by a `StorageKeyProvider = () => Promise<CryptoKey>`, called on every access. In Lowki it
should come from the worker (derived from the unlocked seed, non-extractable), so storage is
unreadable while locked; the worker has no such request yet, so the hook is left typed and open.
`randomStorageKey()` is for tests only. Kohaku stores the whole pool state as one JSON value
(the Sepolia snapshot is ~1.7 MB), so prefer `IndexedDBBackend` to `LocalStorageBackend`.
Kohaku keeps `userSecrets` out of storage (only the public state and the user's history metadata
are written, `state-manager.ts`).

## Network and provider

`createNetwork({ allowHosts, httpsOnly })` wraps `fetch`; `"*.example.com"` matches subdomains.
The viem RPC client uses the same `Network`, so the allowlist covers RPC too. The plain RPC sees
the plugin's reads; for privacy, swap in Kohaku's anon/Helios/Colibri providers later.
`externalSyncProvider` is passed through but Privacy Pools does not use it (only Tornado does, see
below).

## Kohaku issues hit (pinned commit 8c83d64)

1. `packages/privacy-pools/src/v2/index.ts:3` imports `Address` from Node's `'cluster'`, and
   `v2/index.ts:50` (`PPv2Broadcaster = Broadcaster<PPv2BroadcasterParameters>`) violates
   `Broadcaster`'s `{ __type: "privateOperation" }` constraint, so the package index
   (`src/index.ts:2`, `export * from './v2'`) does not typecheck. `src/kohaku-pp.ts` deep-imports the
   v1 modules to avoid it.
2. Kohaku packages use different compiler settings (`poseidon-lean-imt` lacks
   `noUncheckedIndexedAccess`, which `tsconfig.base.json` enables), so one combined program cannot
   use the base config as-is; `tsconfig.typecheck.json` drops that flag.
3. `PrivacyPoolsV1Protocol` ignores `host.externalSyncProvider` (only
   `packages/tornado-cash/src/plugin/base.ts:86` reads it). Privacy Pools' fast path is a
   separate saga-sync `DataService` (`src/data/saga-log-source.ts`), and the public saga manifest
   (`https://saga.fatsolutions.xyz`, also `/v2`) has **no Sepolia streams** for the entrypoint or the
   ETH pool, so Sepolia syncs fully over RPC.
4. `EthClient.getLogs` (`src/data/eth-client.ts:37-56`) scans 5000-block windows sequentially with
   no range back-off, so it needs an RPC that serves 5000-block `eth_getLogs` from block 8,461,453:
   publicnode now returns `4444 pruned history unavailable` there; 1rpc caps at 50 blocks;
   thirdweb at 1000; Tenderly's public gateway works.
5. ASP on Sepolia: every recent `RootUpdated` on the Sepolia entrypoint (802 events; the last six
   checked, blocks 11,936,304 to 12,071,890) carries the placeholder ipfsCID
   `eEEeeeeEEEeeeEEEeeeeeeEEEeEEEeeeeeEEEeEEEEEeEeee`, so Kohaku's default `IPFSAspService`
   (`src/data/ipfsAsp.service.ts:29-36`) gets a 400 from the gateway, and `api.0xbow.io` returns
   404 "No leaves found" for the ETH pool scope. `syncAspThunk` (`src/state/thunks/syncAspThunk.ts:58`)
   runs on every sync once a RootUpdated is known, and its failure fails the whole `sync()`, so no
   balance can be read on Sepolia with a real ASP. Labels-of-all-deposits per pool do not
   rebuild the on-chain root either. `scripts/sepolia-sync.ts` therefore has an ASP-blind mode
   (`ASP=none`, picked automatically by a CID preflight): the instrumented `DataService.getLogs`
   withholds RootUpdated logs, Kohaku skips the ASP step, and every note is reported as
   pending. Approved + pending is still the full balance; only the approval split is unknown.
6. Kohaku's `tests/utils/mock-host.ts` keystore returns a non-promise from `deriveAt` and uses
   ethers' HD wallet; fine for tests, just not the `Keystore` type.

## Gaps

- Mode 2 is enforced in JS only (`signer-only` keystore). The worker still answers
  `kohakuDeriveAt` for `m/28784'/1'/{n}'`; a worker-side session flag that disables it once a host
  chooses envelope signing would make mode 2 hold against compromised JS.
- No worker request yet for the storage key (`StorageKeyProvider` hook).
- Proving and the paymaster sender key stay in Kohaku's JS (see Proving).
- Kohaku is consumed from source via a Node resolve hook; a browser build needs a bundler alias
  for the same mapping (or the published `@kohaku-eth/*` builds of this commit).
- The envelope signer caps indices at `2^32 - 1` (the worker takes `u64`); Kohaku's indices are
  JS numbers starting at 0, so this is not reached in practice.

## Sepolia run (2026-11-06, real wasm worker)

`npx tsx --import ./resolve-hook.mjs scripts/sepolia-sync.ts` with a fresh throwaway 24-word
mnemonic, RPC `sepolia.gateway.tenderly.co` (fallback `rpc.sepolia.ethpandaops.io`), ASP-blind
(preflight found the placeholder CID):

- mode 2 (`envelopeSigner`), cold sync blocks 8,461,453 → 12,078,305 (3,616,853 blocks) in 628.7 s,
  3,076 RPC HTTP requests; entrypoint 1,720 logs (+802 RootUpdated withheld), 3 pools: ETH
  `0x644d…861f` 1,481 leaves (3,132 logs), `0x6709…ff54` 117 leaves, `0x0b06…4c0f` 311 leaves;
  1,157 deposits, 752 withdrawals, 190 ragequits; Kohaku's `verifyRootsThunk` matched every pool's
  rebuilt Merkle root to `currentRoot` on chain.
- mode 1 (`deriveAt`), same encrypted storage: incremental sync of 53 blocks in 1.7 s.
- Balance in both modes: ETH approved 0 / pending 0 (also 0 for the two ERC-20 pools), 0 notes.
