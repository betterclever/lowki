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
| `src/anon-provider.ts` | Anon-RPC provider mode (Nox/Nym/tor-js via `@anon-rpc/browser-harness` 0.4.0 + Kohaku `raw`), reported fallback, per-address session pool |
| `browser-anon/` | Headless-Chrome probe of the anon mode (`build.mjs`, `page.ts`, `run-mac.mjs`) |
| `test/anon-provider.test.ts` | Fallback/isolation unit tests (fake harness) |
| `src/host.ts` | `createLowkiHost({ transport, storage, network, provider, wiring })` → `{ host, keystore, secretManager }` |
| `src/kohaku-pp.ts` | The single import point for `@kohaku-eth/privacy-pools` (deep source imports, see Kohaku issues) |
| `resolve-hook.mjs`, `resolve-hook-impl.mjs` | Node resolve hook: `@kohaku-eth/*` → Kohaku sources, Kohaku's `~/` alias, Kohaku's bare deps → this package's `node_modules` |
| `scripts/load-worker.ts` | Picks the real wasm worker or the stub (`LOWKI_WORKER=real\|stub`) |
| `scripts/sepolia-sync.ts` | Read-only Sepolia run |
| `src/prover.ts` | `lowkiProverFactory`: Kohaku `proverFactory` on the worker's `ppProveWithdraw` / `ppProveCommitment` (lowki-groth16), with a public-signal cross-check |
| `scripts/fork-private-send.ts` | Shield → ASP → sync → prove → relay → checks → ragequit on an anvil Sepolia fork (run via `../../scripts/fork-private-send.sh`; see `docs/PRIVATE-SEND.md`) |
| `src/private-wallet.ts` | `LowkiPrivateWallet`: shield (Kohaku == worker `ppShield`), wait for ASP approval, private withdraw through Kohaku's relayer client, HTTP `IAspService`; used by the wallet UI |
| `browser-wallet/` | The wallet UI glue (`entry.ts`: Anon-RPC connect/rpc/status; lazy `private.ts`); `node browser-wallet/build.mjs` → `dist/`, copied to the wallet's `lowki-glue/` |
| `demo/stack.ts` | Demo stack gateway (fork RPC proxy with access log, ASP, relayer) + static servers; run by `../../scripts/demo-stack.sh` |
| `scripts/asp-from-chain-probe.ts` | Checks whether Sepolia ASP roots equal a LeanIMT of on-chain deposit labels (only the first 20 of 802 do) |
| `scripts/pp-sepolia.ts` | Live Sepolia runbook (`status`/`fund`/`shield`/`ragequit`/`withdraw`), dry run unless `BROADCAST=1`; also runs against a fork |
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
the plugin's reads; see "Anon-RPC provider" for the anonymous mode.
`externalSyncProvider` is passed through but Privacy Pools does not use it (only Tornado does, see
below).

## Anon-RPC provider

`src/anon-provider.ts` is the anonymous-read mode. It reimplements Kohaku's `anon` adapter
(`packages/provider/src/anon`) as a thin wrapper over `@anon-rpc/browser-harness` **0.4.0** and
Kohaku's `raw` adapter, for two reasons:
- Kohaku's adapter doesn't forward `anonOptions`, so it can't pass `isolationKey`.
- Kohaku's adapter throws on a failed boot; this wrapper reports it instead.

It uses Kohaku's anon receipt polling (4 s / 5 min). Browser-only: the harness runs the network client in a null-origin sandboxed iframe and dials over WebRTC (KPS).

```ts
const r = await createAnonProvider({
  rpcUrl: "https://sepolia.gateway.tenderly.co",  // destination, reached through the anon network
  network: "auto",              // nox on Chromium/Firefox, nym on Safari/WebKit + every iOS browser
  // torGateways: ["ip:port:hash"], // tor-js only; default = the EF dev's temporary 159.89.167.167 gateway
  bootTimeoutMs: 60_000,        // the harness has none of its own
  isolationKey: appAddress,     // sent as anonOptions.isolationKey on every call
  fallback: { rpcUrl },         // or `false`: reject with AnonUnavailableError instead
  onFallback: ({ network, reason }) => warnUser(reason),
});
// r: { provider, anonymous: true, network, bootMs, fetch, close }
//  | { provider, anonymous: false, network, bootMs, reason, error, fallbackRpcUrl, fetch, close }
createLowkiHost({ ..., provider: r.provider, network: createNetwork({ allowHosts, fetch: anonFetchFor(r) }) });
```

- **Fallback:** it applies only to the boot (harness error, worker declined, or timeout). It is never silent: the result carries `anonymous: false` and `reason`, and `onFallback` fires. Once a session has booted, a failed call rejects; it is never re-sent directly.
- **What anonymous traffic bypasses:** it leaves through the worker's own transport, so the host `Network` allowlist does not apply to it. The bootstrap read and a direct fallback still go through `fetchFn`.
- **Bootstrap read:** each session reads the network's specifier contract once (`workerHash`/`workerResolvers`) over a plain RPC, which is not anonymous. Nox's specifier is on **Sepolia**; Nym's and tor-js's are on mainnet. The defaults are in `DEFAULT_BOOTSTRAP_URLS`.
- **`anonFetchFor(r)`:** routes Kohaku's other HTTP through the same session (relayer `RelayerClient`, paymaster bundler client, IPFS/0xbow ASP), since all of it goes through `host.network`.
- **`createAnonSessionPool({ ..., isolation })`:** `forAddress(addr)` gives one session per app address (lazy, cached). Each result reports `anonymous` separately. There are two modes:
  - `isolation: "worker"` (default): a separate `AnonRpcWorker` per address, plus `isolationKey = address`.
  - `isolation: "isolationKey"`: one shared worker, with a different `isolationKey` per address.

### Measured (2026-11-06, headless Chrome 156, macOS host; direct egress IP 45.127.45.75)

```sh
npm install
node browser-anon/build.mjs                          # esbuild bundle of browser-anon/page.ts (Linux or mac)
mac sh -lc 'cd <lowki>/packages/lowki-kohaku-host && ANON_QUERY="ipn=4" node browser-anon/run-mac.mjs nox nym tor-js'
#   ANON_QUERY extras: rpc=<sepolia rpc>  gw=<tor gateway>  boot=<ms>  second=0  kohaku=0  kblocks=<n>
node --import ./resolve-hook.mjs --import tsx --test test/anon-provider.test.ts   # fallback/isolation logic, fake harness (8/8)
```

`run-mac.mjs` starts chromedriver 156 with `--headless=new` and the anon-rpc e2e WebRTC flags (`--disable-features=WebRtcHideLocalIpsWithMdns --force-webrtc-ip-handling-policy=default`). It serves the page and prints `window.__anon`. The IP echo is `api.ipify.org`. Every anon run returned Sepolia chainId 11155111, the head block (about 12,078,5xx, the same as direct) and the ETH pool balance `31986556177510679061` wei.

| | Nox | Nym | tor-js (gateway 159.89.167.167) |
|---|---|---|---|
| Boot (`ready`) | 3.1 s, 2.8 s, 3.1 s, 3.4 s | 11.0 s, 10.3 s, 10.8 s, 10.9 s | 2.7–3.7 s |
| First call after boot | 1.3–2.1 s | 9.6–13.4 s; first call on a 2nd worker 8–20 s | 1.8–3.9 s |
| Steady-state `eth_blockNumber`/`eth_getBalance`/`eth_getTransactionCount` | 0.4–1.4 s | 0.5–5 s; outliers 7–22 s | 0.7–1.2 s (publicnode), 0.8–2.9 s (ethpandaops) |
| `eth_getLogs`, 5,000 blocks of the ETH pool (direct: 0.3–2.4 s) | 0.6–0.8 s | 0.8–2.3 s | 0.8–3.8 s |
| Kohaku `EthClient` incremental step: blockNumber, getLogs of entrypoint and ETH pool over 15,000 blocks (3 windows each), `currentRoot`, `latestRoot` | **8.6 s** | **12.8 s** | **21.9 s** (ethpandaops) |
| Egress IP (direct 45.127.45.75) | 98.92.70.228, 3.226.251.110, 13.223.188.90 (AWS) | 45.9.168.34, 185.191.239.7, 185.246.188.210, 193.200.238.106 | Tor exits: 192.42.116.x, 171.25.193.x, 185.220.10x.x, 109.70.100.x, 204.8.96.x, … |
| Different `isolationKey`, same worker | **No effect.** The exit varies per request among about 3 IPs regardless of key (key A got .228 and .110; key B got .110 and .90) | **No effect.** One exit (IP Packet Router) per worker for every key (10/10 same IP) | **Honoured.** A separate circuit per key: key A 4/4 on one exit, key B 4/4 on another |
| Second worker | Same 3-IP exit pool | **New exit** (different IPR) | New circuit / exit |
| Failures seen | none | one boot failed in 0.6 s with `Failed to fetch` (bundle or bootstrap fetch). The fallback reported `anonymous: false` and the retry worked | `sepolia.gateway.tenderly.co` returns **HTTP 403 to Tor exits**. `ethereum-sepolia-rpc.publicnode.com` over Tor refuses 15,000-block log ranges (`pruned history unavailable`). `rpc.sepolia.ethpandaops.io` works for everything |

The forced-failure check used tor-js with an unreachable gateway and `boot=20000`. It returned `anonymous: false` with reason `anon-rpc tor-js boot timed out after 20000 ms`, after 20,007 ms.

### Per-address isolation: what you actually get

- **tor-js:** `isolationKey` alone gives each address its own Tor circuit and exit inside one worker. A worker per address is not needed for IP unlinkability.
- **Nym:** `isolationKey` is ignored. Each worker is one Nym client with one exit IPR, and every address on that worker shares the egress IP. Only `isolation: "worker"` separates addresses. Its cost is roughly 10 s of boot plus a slow first call, and an 18 MB bundle per worker.
- **Nox:** `isolationKey` is ignored. Every request already leaves from one of a small shared pool of exits, chosen per request. Requests from different addresses (and from other Nox users) share those exit IPs, so the RPC can't separate users by IP. That is also true within one address, and it is the network's property, not per-address isolation. A separate worker per address gives separate Sphinx sessions; we can't observe whether that adds anything beyond the per-request mixing.
- **Shared state across workers:** separate workers on one host origin share the harness storage. It is IndexedDB, namespaced per specifier, not per worker, so any persistent client state the network keeps there (for example cached directory data or client keys) is the same for every per-address worker. Nothing at the harness level changes this; it would need a per-session namespace in the spec.
- **What the RPC still sees:** request timing and content. Reading address A and then address B 200 ms later still correlates them, so stagger per-address reads.

### Which calls should go through anon

- **Through anon:** everything that names the user. That means balances, nonces and code of the user's app addresses, receipts and `waitForTransaction` for the user's own transactions, `eth_sendRawTransaction` (deposit and ragequit), `eth_estimateGas`/`eth_call` with the user's data, relayer quote and relay, bundler and paymaster calls, and ASP fetches (wire these through `anonFetchFor`). Each is about one call, at 0.5–2 s on Nox and tor-js and up to about 20 s on Nym.
- **Incremental pool sync through anon:** this is practical too, at 9–22 s for 15,000 blocks (about 2 days of Sepolia), and it hides *that* this IP follows Privacy Pools.
- **Cold sync over direct RPC:** a cold sync from block 8,461,453 is about 3,076 RPC requests (the Sepolia run below). Serially that is about 45 min on Nox, 1–3 h on tor-js and several hours on Nym. These are user-neutral scans that fetch the same data for everyone. Ship a snapshot, or do the cold scan direct and only the increments anonymously.
- **Pick a destination RPC that serves Tor:** Tenderly's public gateway 403s Tor exits.

### Anon-RPC gaps

- Kohaku's `Host` has one `provider`, so the Privacy Pools plugin can't send its cold scan direct and its increments through anon on its own. Switch the provider between the two phases, or use two hosts on the same storage.
- **Only desktop Chrome was tested.** The Safari/iOS → Nym choice rests on the reports in the chat, not on a run here.
- The tor-js gateway is temporary; production needs our own KPS gateway (`torGateways`).
- The harness bundles come from on-chain specifiers. Whoever controls each specifier controls the code; the harness checks the hash, nothing more.

## Proving with the worker (`src/prover.ts`)

`lowkiProverFactory(transport, { chainId, entrypoint })` is passed as
`PrivacyPoolsV1Protocol({ proverFactory })`. For each witness Kohaku builds, it finds the note
reference whose `poseidon(nullifier, salt)` equals the worker's `ppNote` precommitment (walk over
`(depositIndex, secretIndex)`, cached), calls `ppProveWithdraw` / `ppProveCommitment` with
Kohaku's Merkle proofs, and refuses the proof unless every public signal (new commitment,
nullifier hash, value, both roots, context) equals what Kohaku's witness implies. Used end to end
on a Sepolia fork (`docs/PRIVATE-SEND.md`): the on-chain verifier accepted the worker proofs and
Kohaku recognised the worker-derived change note. It does not keep note secrets out of JS (Kohaku
computed them already); see "Proving".

`src/protocol.ts` now also types `ppNote`, `ppShield`, `ppLoadCircuits`, `ppProveWithdraw`,
`ppProveCommitment`, `appAccount` and `signTransaction`.

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
6. Kohaku's `tests/state.11155111.json` snapshot (lastSyncedBlock 11,627,888) contains the
   `[0,1,2]` ASP root that Kohaku's own fork tests pushed (block 0xb16d71, CID `bafybeihrec…`),
   which never happened on the real chain. Starting an ASP-blind sync from it fails
   `verifyRootsThunk`; the scripts drop its `asp`/`updateRootEvents` slices first.
7. Kohaku's `tests/utils/mock-host.ts` keystore returns a non-promise from `deriveAt` and uses
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
