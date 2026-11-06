# Private send (Privacy Pools v1) on Sepolia

Lowki's private send is a Privacy Pools v1 withdrawal:

1. Kohaku's privacy-pools plugin syncs the pool on the Lowki Host.
2. The Lowki worker proves the withdrawal.
3. A relayer submits `Entrypoint.relay`.

This page covers:
- the fork run that exercises the whole flow;
- what in that run is real and what is forked;
- the runbook for live Sepolia.

Pool constants (Sepolia, read from chain):

| | |
|---|---|
| Entrypoint | `0x34A2068192b1297f2a7f85D7D8CdE66F8F0921cB` |
| ETH pool | `0x644d5A2554d36e27509254F32ccfeBe8cd58861f` |
| Withdrawal verifier | `0x822f33Ed5Ac1d33ceed4EEC60A99b06e5053A00a` |
| Minimum deposit | 0.001 ETH |
| Vetting fee | 100 bps |
| `maxRelayFeeBPS` | 100 |
| Test postman (holds `ASP_POSTMAN` on the Entrypoint) | `0x696FE46495688fC9e99BAd2dAF2133B33de364eA` |
| Testnet relayer | `https://testnet-relayer.privacypools.com/relayer` (10 bps, fee receiver `0x3497…84d8`) |

## Fork run (one command)

Prerequisites:
- **Foundry:** `curl -L https://foundry.paradigm.xyz | bash && foundryup`. anvil 1.9.0 was used.
- **The worker's Node build:** `crates/lowki-worker/pkg/node`, made with `make pkg`.
- **`npm ci`** in `packages/lowki-kohaku-host`.
- **Circuit artifacts** at `../vendor/npm/circuits/package/artifacts`.

```sh
scripts/fork-private-send.sh
# env: FORK_URL (default https://sepolia.gateway.tenderly.co; must serve 5000-block eth_getLogs),
#      FORK_BLOCK (default 12078500), PORT (8547), ANVIL (binary), DEPOSIT_WEI (0.1 ETH),
#      WITHDRAW_WEI (0.05 ETH), RELAY_FEE_BPS (50), LOWKI_TEST_MNEMONIC (else a fresh throwaway)
```

The script starts `anvil --fork-url $FORK_URL --fork-block-number $FORK_BLOCK --chain-id 11155111`, runs `packages/lowki-kohaku-host/scripts/fork-private-send.ts` against it, and stops anvil when it exits. It exits 0 only if every check passes. The TS script refuses any RPC that is not an anvil fork (`anvil_nodeInfo`).

**If another process runs `pkill -x anvil`** (other agents in this workspace do), it also kills this fork. Copy the binary to a different name and pass it as `ANVIL=/path/to/ppfork-anvil`.

### What it does, and with which code

| Step | Code | Detail |
|---|---|---|
| Keys | lowki-worker (wasm, Node build) | `importMnemonic`, then `appAccount("https://shop.example")`. The fork funds that address with `anvil_setBalance` (1 ETH). |
| Catch-up sync, first run only | Kohaku `PrivacyPoolsV1Protocol` on the Lowki Host | Starts from Kohaku's `tests/state.11155111.json` (block 11,627,888) and syncs to the fork block ASP-blind, before any fork transaction. Caches the public pool state as `packages/lowki-kohaku-host/.cache/pp-state-aspblind-11155111-<block>.json`. About 74–78 s; 0.2–0.8 s after that. |
| ASP | `anvil_impersonateAccount(postman)` then `Entrypoint.updateRoot(root, cid)` | Same as Kohaku's `pushNewAspRoot`. A local `IAspService` serves the leaves for each root. Kohaku rebuilds the LeanIMT root from those leaves and checks it against the on-chain `RootUpdated` and `latestRoot()`. The first root is `[0,1,2]`, as in Kohaku's tests. |
| Shield ×2 | worker `ppShield`, then worker `signTransaction`, then `eth_sendRawTransaction` | Each deposit is checked to be **byte-identical** to Kohaku's `prepareShield` for the same deposit index. The deposits come from the app account, so `Pool.depositors(label)` is the app account. |
| Approve | ASP root `[0,1,2,label(deposit #0)]` | Deposit #1 stays unapproved, for the ragequit. |
| Sync | Kohaku on the Lowki Host | Keystore mode 2 (`envelopeSigner`): every note envelope is signed in Rust. RPC is the fork, through `createNetwork` and `createRpcProvider`. |
| Prove | `src/prover.ts` (`lowkiProverFactory`), then worker `ppProveWithdraw` | Kohaku's `prepareUnshield` builds the witness and the Merkle proofs. The adapter finds the note reference, proves in the worker (lowki-groth16; CPU wasm in Node), and checks every public signal against Kohaku's witness. |
| Submit | Kohaku `PrivacyPoolsBroadcaster`, then the relayer client's `relay()` | The fork relayer is a fresh fork account: (1) its quote embeds `RelayData{recipient, feeRecipient = relayer, relayFeeBPS = 50}`; (2) Kohaku validates the quote (`validateWithdrawalData`); (3) Kohaku sets `processooor = Entrypoint` and binds `context = keccak(withdrawal, scope)` into the proof; (4) `relay()` sends `Entrypoint.relay(withdrawal, proof, scope)` from the relayer, which pays the gas and is repaid by the fee. |
| Check | viem reads | Recipient balance, relayer balance, `Pool.nullifierHashes(h)`, replay (both `eth_call` and a mined transaction), and Kohaku's view of the change note. |
| Ragequit | Kohaku `ragequit([label])` → worker `ppProveCommitment` → worker-signed `Pool.ragequit` | Sent by the depositor (the app account). Then a replay. |

### Result (2026-11-06, fork of Sepolia block 12,078,500, cold cache)

Throwaway mnemonic. Actors:
- app account `0xD755692a28Bf342BE823AC750A779E7c9BD6C93A`;
- relayer `0xABf05Dd8169687B8fdea56291D7Ef56fc5007DDf`;
- recipient `0x68160dABb5c2175FDE57fE6Fc6A326D415413435`, fresh with 0 ETH.

| Step | Tx (fork) | Outcome |
|---|---|---|
| ASP root `[0,1,2]` (postman) | `0xa4ec2b6c…8096469` | `latestRoot()` = our root |
| Deposit #0, 0.1 ETH | `0x78ac4a70…c01ad3e` (block 12078502, 721,289 gas) | Note 0.099 ETH (1% vetting fee); worker tx == Kohaku tx |
| Deposit #1, 0.1 ETH | `0xb13caacf…c634cc49` (block 12078503) | Note 0.099 ETH |
| ASP root with label #0 (postman) | `0xd236805b…3ad8b110` | Kohaku balance: approved 0.099, pending 0.099 |
| Withdraw proof | n/a | lowki-worker, CPU, 2,178 ms (2.2–2.3 s across runs); `prepareUnshield` 2.3 s total |
| `Entrypoint.relay`, 0.05 ETH gross | `0x6aab0f07…cb39d26a` (block 12078505, 999,534 gas) | Recipient 0 → **0.04975 ETH** (0.05 minus the 0.00025 fee) |
| | | Relayer 1 → 0.99925 ETH (+0.00025 fee, −0.0009995 gas) |
| | | `nullifierHashes(...)` = true |
| Replay of the same withdrawal | `0x…` mined with status **reverted** | `eth_call` reverts with `0xb115d857` = `NullifierAlreadySpent()`; recipient unchanged |
| Change note | n/a | Kohaku sees 0.049 ETH at secretIndex 1, so the worker-derived change commitment matches Kohaku's derivation |
| Commitment proof | n/a | lowki-worker, CPU, 147–165 ms |
| `Pool.ragequit` deposit #1 | `0x8422bedb…4221067` (block 12078507, 364,440 gas) | App account +0.099 ETH (minus 0.000364 ETH gas); replay reverts; Kohaku drops the note |
| End state | n/a | Kohaku balance: approved 0.049 ETH, pending 0 |

The fork transaction hashes change from run to run (fresh keys). The two postman pushes are deterministic.

### Real versus forked

**Real** (live Sepolia state at block 12,078,500, or unmodified code):
- the Entrypoint, the pool, the withdrawal and commitment verifiers, the scope, fees and limits;
- every deposit, leaf and root already in the pool;
- Kohaku's plugin source (pinned `8c83d64`);
- the Lowki worker (wasm) and its keys, signatures and proofs;
- the circuit artifacts (sha256-pinned).

**Forked or simulated:**
- **ETH balances:** set with `anvil_setBalance`.
- **ASP:** the ASP root is pushed by **impersonating** the test postman, and a local `IAspService` serves our leaf list. On live Sepolia the 0xbow ASP publishes placeholder CIDs and returns 404, so no withdrawal can prove ASP membership there today.
- **Relayer:** a fork account plays it, with Kohaku's relayer-client interface and a self-made quote. The real testnet relayer would also sign a fee commitment.
- **Prover:** native-Node wasm on the CPU, not browser WebGPU. The browser path produces the same proof format, and the deployed verifier already accepted WebGPU proofs via `eth_call` (`crates/lowki-worker/README.md`).

### Why relay and not the paymaster

The relay path is what works on live Sepolia: the testnet relayer is up, and Kohaku has no Privacy Pools paymaster configured for Sepolia. The paymaster path would need:
- `privacy-paymaster` deployed and staked on the fork;
- a bundler.

The worker already builds paymaster userOps (`ppBuildPaymasterWithdraw`). They are not exercised here.

## Live Sepolia runbook

Everything below goes through `packages/lowki-kohaku-host/scripts/pp-sepolia.ts`. **Nothing is sent unless `BROADCAST=1`.** Without it, each command simulates (`eth_estimateGas` / `eth_call`) and prints what it would send. The whole runbook was rehearsed on a fork (`RPC_URL=http://127.0.0.1:8548`) with these exact commands: fund, shield, ragequit dry run, ragequit, and the withdraw refusal. It has **not** been run on live Sepolia.

```sh
cd packages/lowki-kohaku-host
export LOWKI_TEST_MNEMONIC="..."   # 24 words, KEEP IT: every note is derived from it. Never printed.
export LOWKI_SEPOLIA_KEY="0x..."   # funded Sepolia EOA, used by `fund` only. Never printed (address only).
# optional: RPC_URL (default Tenderly), LOWKI_ORIGIN (default https://shop.example),
#           AMOUNT_WEI (default 0.01 ETH), GAS_BUFFER_WEI (default 0.03 ETH)
S="node --import ./resolve-hook.mjs --import tsx scripts/pp-sepolia.ts"

$S status                       # app account address and ETH, notes (ASP preflight picks ASP=none today)
BROADCAST=1 $S fund             # LOWKI_SEPOLIA_KEY -> app account: AMOUNT_WEI + GAS_BUFFER_WEI
BROADCAST=1 $S shield           # worker ppShield == Kohaku prepareShield, signed by the worker
$S status                       # the note shows as pending

# Ragequit: works on live Sepolia today (no ASP needed)
$S ragequit                     # commitment proof + simulation
BROADCAST=1 $S ragequit         # Pool.ragequit from the app account (the depositor)

# Private withdraw: only once a live RootUpdated root that contains the note's label is servable
ASP=ipfs  RECIPIENT=0x... $S withdraw      # or ASP=0xbow; quote, proof, eth_call of Entrypoint.relay
ASP=ipfs  RECIPIENT=0x... BROADCAST=1 $S withdraw   # POST /relayer/request to the testnet relayer
```

Notes:
- **Sync time.** With `ASP=none`, the first live sync uses the ASP-blind cache from the fork run (block 12,078,500) if present, else Kohaku's snapshot (block 11,627,888, about 75 s). Then it writes a fresh cache. With `ASP=ipfs`/`0xbow` it starts from Kohaku's snapshot, so the real `RootUpdated` events are seen.
- **Funding is public.** `fund` is an ordinary transfer, so it links the funding key to the Lowki app account on chain. Live users would fund the app account some other way. The deposit and the ragequit are visible from the app account by design; the withdrawal is the unlinkable step.
- **Ragequit leaves the pool.** It returns the full note to the depositor and is not private.
- **When withdraw is refused.** `withdraw` refuses with `ASP=none`, and Kohaku refuses to prove against an ASP root it could not match to `latestRoot()` (`withdrawThunk`).

### Can the ASP root be rebuilt from chain data? (tested 2026-11-06: no)

**Hypothesis:** the testnet ASP approves every deposit, so its root would be the LeanIMT of all deposit labels.

**Method:**
- `packages/lowki-kohaku-host/scripts/asp-from-chain-probe.ts`, plus bounded subset searches.
- 802 `RootUpdated` events from the Entrypoint (all with the placeholder CID; 800 distinct roots; `latestRoot()` is the last one, block 12,071,890), compared with deposit data from 1,157 deposits (902 in the ETH pool) and 190 ragequits.
- Kohaku's Poseidon LeanIMT.

| Variant | Roots matched |
|---|---|
| Labels in deposit order, all pools, every prefix | **20**, all at or before block 8,719,849, prefixes up to 36 labels; none later |
| Labels in deposit order, ETH pool only | the same 20; other pools register later |
| The previous rows evaluated at each root's block or block − 1 | 18 of 46 sampled roots, the same early ones |
| Labels minus those ragequit by the root's block (all pools / ETH pool) | 7–8, early only |
| Up to each root's block, with any 1 or 2 labels excluded (roots #17–#45) | 3, all early, all equal to a shorter prefix (deposits not yet approved) |
| Subset search: append any subset of the next 12 pending labels per root | stops at root #16 |
| Commitments; precommitments; `poseidon(label)`; labels sorted ascending | 0 |

**Conclusion:**
- For roughly the first 20 roots (mid 2025), the testnet ASP root was exactly the LeanIMT of all deposit labels in deposit order, with a short approval delay.
- No later root matches any variant tried. From about block 8,719,875 the ASP's leaf set differs from "every deposit so far" by more than one or two labels, or uses an order or leaf encoding we could not reproduce.
- So no "ASP from chain" provider was added. The fork run, with an impersonated postman, remains the private-withdraw demo.

### What a live private withdraw needs

1. **An ASP root on the Sepolia Entrypoint whose leaf set includes our deposit label, and a service that serves those leaves.** That means either:
   - 0xbow fixes the Sepolia ASP: real IPFS CIDs, or `api.0xbow.io` leaves for the ETH pool scope; or
   - the holder of `ASP_POSTMAN` (the test postman `0x696F…364eA`) pushes a root for us and publishes the leaves. Then we use a custom `IAspService` (as in the fork script) or `ASP=ipfs` with a real CID.
2. **The testnet relayer accepting the request** (`/relayer/request`). It is up and quotes 10 bps. We have not called `/request`, because that would be a live transaction.
3. **A live ETH balance** for the funding key: 0.01 ETH plus about 0.03 ETH of gas headroom with the defaults.

## Gaps and follow-ups

- **Note secrets pass through JS.** Kohaku derives them for the witness. The adapter adds speed (WebGPU in the browser) and a public-signal cross-check, not isolation. Isolation needs the Kohaku seams described in `packages/lowki-kohaku-host/README.md` ("Proving").
- **Note-reference lookup.** The prover adapter finds the note by walking `(depositIndex, secretIndex)` with `ppNote`. A `noteRef` from Kohaku, or a worker request that matches a precommitment, would remove the walk. That is a possible worker change; nothing was edited in `crates/`.
- **Browser runs.** The fork flow runs in Node. The same host code runs in the browser with `WebWorkerTransport`, but that browser run on a fork has not been done.
- **Paymaster path.** Not exercised; see above.
- **Anonymity.** Fork reads go to a local anvil. On live Sepolia, per-user calls (the relayer, receipts, balances) should go through the anon provider (`src/anon-provider.ts`, see the host README). The cold log scan can use direct RPC or the cache.
