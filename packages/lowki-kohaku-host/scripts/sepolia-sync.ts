// Read-only Sepolia run of Kohaku's Privacy Pools v1 plugin on the Lowki Kohaku Host.
//
//   npx tsx --import ./resolve-hook.mjs scripts/sepolia-sync.ts      (or: npm run sepolia-sync)
//
// Env:
//   LOWKI_TEST_MNEMONIC  24-word phrase to use (never printed). Default: a fresh throwaway
//                        phrase is generated and printed, since it holds nothing.
//   LOWKI_WORKER         real | stub (default: real if crates/lowki-worker/pkg/node exists)
//   WIRING               envelopeSigner | deriveAt | both (default both: sync once in mode 2,
//                        then rerun mode 1 on the same encrypted storage)
//   RPC_URL              comma-separated; default https://sepolia.gateway.tenderly.co,https://rpc.sepolia.ethpandaops.io
//   ASP                  auto (default) | ipfs | 0xbow | none
//                        ipfs  = Kohaku's default IPFSAspService (reads the latest RootUpdated ipfsCID)
//                        0xbow = OxBowAspService (api.0xbow.io)
//                        none  = ASP-blind: RootUpdated logs are withheld from the plugin, so it
//                                skips the ASP sync and reports every note as pending (approved +
//                                pending is still the full balance). Needed on Sepolia today: every
//                                recent RootUpdated carries the placeholder CID
//                                "eEEeeee…EeEeee", and api.0xbow.io has no Sepolia leaves (404).
//                        auto  = preflight the latest RootUpdated CID against the IPFS gateway;
//                                ipfs if it serves a tree, otherwise none.
//
// Nothing is signed for or sent to the chain: the plugin only reads. The prover throws if called.
import { generateMnemonic } from "@scure/bip39";
import { decodeAbiParameters, toEventSelector, type Hex } from "viem";
import { wordlist } from "@scure/bip39/wordlists/english.js";
import type { ERC20AssetId } from "@kohaku-eth/plugins";
import {
  DataService, E_ADDRESS, EthClient, IPFSAspService, OxBowAspService, PrivacyPoolsV1Protocol, PrivacyPoolsV1_0xBow,
} from "../src/kohaku-pp";
import {
  EncryptedStorage, InProcessTransport, MemoryBackend, call, createLowkiHost, createNetwork, createRpcProvider,
  randomStorageKey, type KeystoreWiring, type LowkiEnvelopeSigner,
} from "../src/index";
import { loadWorker } from "./load-worker";

const CHAIN_ID = 11155111;
const ETH_POOL = "0x644d5A2554d36e27509254F32ccfeBe8cd58861f".toLowerCase();
const { entrypointAddress, deploymentBlock } = PrivacyPoolsV1_0xBow[CHAIN_ID].entrypoint;
// Comma-separated; a viem fallback transport tries them in order. publicnode now answers
// "pruned history unavailable" (code 4444) for logs near the deployment block, and 1rpc caps
// eth_getLogs at 50 blocks, so the default is Tenderly's public gateway (5000-block windows OK).
const RPC_URLS = (process.env.RPC_URL ?? "https://sepolia.gateway.tenderly.co,https://rpc.sepolia.ethpandaops.io").split(",");
let ASP = process.env.ASP ?? "auto";
const WIRING = process.env.WIRING ?? "both";

const envMnemonic = process.env.LOWKI_TEST_MNEMONIC;
const mnemonic = envMnemonic ?? generateMnemonic(wordlist, 256);

const log = (...a: unknown[]) => console.log(`[${new Date().toISOString().slice(11, 19)}]`, ...a);

// ---- worker ----
const { worker, kind, source } = await loadWorker(60);
const transport = new InProcessTransport(worker);
const prf = crypto.getRandomValues(new Uint8Array(32)); // stands in for the passkey PRF output

await call(transport, { type: "importMnemonic", mnemonic, prfOutput: prf });
log(`worker: ${kind} (${source})`);
log(envMnemonic
  ? "mnemonic: from LOWKI_TEST_MNEMONIC (not printed)"
  : `mnemonic: FRESH THROWAWAY TEST PHRASE, holds nothing, printed for reproducibility: "${mnemonic}"`);

// ---- host pieces shared by both wiring modes ----
const rpcHosts = RPC_URLS.map((u) => new URL(u).hostname);
const counters = { rpcHttp: 0, getLogsCalls: 0, withheldRootUpdated: 0, logs: new Map<string, number>() };
const baseNetwork = createNetwork({ allowHosts: [...rpcHosts, "ipfs.filebase.io", "api.0xbow.io"] });
const network = {
  fetch: (input: RequestInfo | URL, init?: RequestInit) => {
    const url = String(input instanceof Request ? input.url : input);

    if (rpcHosts.some((h) => url.includes(h))) counters.rpcHttp++;

    return baseNetwork.fetch(input, init);
  },
};
const provider = createRpcProvider(RPC_URLS, network);

const ROOT_UPDATED = toEventSelector("event RootUpdated(uint256 _root, string _ipfsCID, uint256 _timestamp)");

if (ASP === "auto") {
  // Find the latest RootUpdated by scanning back from head, then try its CID on the gateway.
  const head = await provider.getBlockNumber();
  let cid: string | undefined;

  for (let to = head; to > deploymentBlock && cid === undefined; to -= 5000n) {
    const logs = await provider.getLogs({ address: entrypointAddress as Hex, fromBlock: to - 4999n, toBlock: to });
    const last = logs.filter((l) => l.topics[0] === ROOT_UPDATED).at(-1);

    if (last) cid = (decodeAbiParameters([{ type: "uint256" }, { type: "string" }, { type: "uint256" }], last.data as Hex)[1]);
  }

  const ok = cid !== undefined && await baseNetwork.fetch(`https://ipfs.filebase.io/ipfs/${cid}`).then((r) => r.ok, () => false);

  log(`ASP preflight: latest RootUpdated ipfsCID=${cid} -> gateway ${ok ? "OK" : "FAILS"}`);
  ASP = ok ? "ipfs" : "none";
}

log(`rpc: ${RPC_URLS.join(" -> ")}; asp: ${ASP}${ASP === "none" ? " (ASP-blind: RootUpdated withheld, approved/pending split unknown)" : ""}`);
const backend = new MemoryBackend();
const storage = new EncryptedStorage(backend, await randomStorageKey()); // dev key; Lowki: worker-derived

// Kohaku's own batched log scan (EthClient.getLogs, 5000-block windows), instrumented.
const ethClient = new EthClient(provider);
const dataService = new DataService({
  provider,
  getLogs: async (params) => {
    const windows = Math.ceil(Number((params.toBlock ?? 0n) - params.fromBlock + 1n) / 5000);

    counters.getLogsCalls += Math.max(windows, 1);
    const t0 = Date.now();
    const fetched = await ethClient.getLogs(params);
    const logs = ASP === "none" ? fetched.filter((l) => l.topics[0] !== ROOT_UPDATED) : fetched;

    if (logs.length !== fetched.length) counters.withheldRootUpdated += fetched.length - logs.length;
    const a = params.address.toLowerCase();

    counters.logs.set(a, (counters.logs.get(a) ?? 0) + logs.length);
    log(`  getLogs ${a} [${params.fromBlock}..${params.toBlock}] -> ${logs.length} logs (${((Date.now() - t0) / 1000).toFixed(1)}s)`);

    return logs;
  },
});

const noProver = () => {
  throw new Error("read-only run: proving is disabled");
};

async function run(wiring: KeystoreWiring) {
  let signer: LowkiEnvelopeSigner | undefined;
  const lowki = createLowkiHost({
    transport, storage, network, provider, wiring,
    onSigner: (s) => { signer = s; },
  });
  const protocol = new PrivacyPoolsV1Protocol(lowki.host, {
    entrypoint: { address: BigInt(entrypointAddress), deploymentBlock },
    ...(lowki.secretManager ? { secretManager: lowki.secretManager } : {}),
    aspServiceFactory: ASP === "0xbow"
      ? () => new OxBowAspService({ network })
      : () => new IPFSAspService({ network }),
    proverFactory: noProver as never,
    dataService,
  });

  const head0 = await provider.getBlockNumber();
  const t0 = Date.now();
  const rpc0 = counters.rpcHttp;

  log(`--- wiring=${wiring}: sync from ${deploymentBlock} (head ${head0}) ---`);
  await protocol.sync();
  const syncMs = Date.now() - t0;

  const eth: ERC20AssetId = { __type: "erc20", contract: E_ADDRESS };
  const balances = await protocol.balance([eth], { sync: false });
  const all = await protocol.balance([], { sync: false });
  const notes = await protocol.notes([], true, { sync: false });

  const dump = protocol.dumpState() as Record<string, any>;
  const st = Object.values(dump)[0]!;
  const pools = (st.pools.poolsTuples as [string, any][]).map(([a, p]) => ({ pool: a, asset: p.asset, registeredBlock: BigInt(p.registeredBlock) }));
  const leaves = new Map<string, number>((st.poolsLeaves.poolLeavesTuples as [string, unknown[]][]).map(([a, l]) => [a, l.length]));

  log(`synced in ${(syncMs / 1000).toFixed(1)}s, ${counters.rpcHttp - rpc0} RPC HTTP requests this run`);
  log(`lastSyncedBlock=${BigInt(st.sync.lastSyncedBlock)} (blocks covered: ${BigInt(st.sync.lastSyncedBlock) - deploymentBlock + 1n})`);
  log(`pools registered on entrypoint: ${pools.length}`);
  for (const p of pools) log(`  pool ${p.pool} asset ${p.asset} registered@${p.registeredBlock} leaves=${leaves.get(p.pool) ?? 0}`);
  log(`ETH pool ${ETH_POOL}: leaves=${leaves.get(ETH_POOL) ?? 0}`);
  log(`totals: deposits=${st.deposits.depositsTuples.length} withdrawals=${st.withdrawals.withdrawalsTuples.length} ragequits=${st.ragequits.ragequitsTuples.length} entrypointDeposits=${st.entrypointDeposits.entrypointDepositsTuples.length}`);
  log(`ASP: leaves=${st.asp.leaves.length} root=${st.asp.aspTreeRoot} (latest RootUpdated root=${st.updateRootEvents.lastUpdateRootEvent?.root}, ipfs=${st.updateRootEvents.lastUpdateRootEvent?.ipfsCID})`);
  log(`ETH balance: ${balances.map((b) => `${b.tag ?? "approved"}=${b.amount}`).join(" ")}`);
  log(`all assets: ${all.map((b) => `${b.asset.contract}:${b.tag ?? "approved"}=${b.amount}`).join(" ") || "(none)"}`);
  log(`notes (incl. spent): ${notes.length}`);

  if (signer?.identityAddress) log(`PP identity address (m/28784'/1'/0', public): ${signer.identityAddress}`);

  return { balances, syncMs };
}

const modes: KeystoreWiring[] = WIRING === "both" ? ["envelopeSigner", "deriveAt"] : [WIRING as KeystoreWiring];
const results = [];

for (const m of modes) results.push(await run(m));

log(`per-address logs: ${[...counters.logs].map(([a, n]) => `${a}=${n}`).join(" ")}`);
if (counters.withheldRootUpdated) log(`ASP-blind: withheld ${counters.withheldRootUpdated} RootUpdated logs`);
log(`getLogs windows (5000 blocks): ${counters.getLogsCalls}; RPC HTTP requests total: ${counters.rpcHttp}`);
log(`encrypted storage: ${[...backend.raw()].map(([k, v]) => `${k} (${v.length} chars ciphertext)`).join(", ")}`);
const zero = results.every((r) => r.balances.every((b) => b.amount === 0n));

log(zero ? "RESULT: balance 0 in every mode, as expected for a fresh mnemonic" : "RESULT: NON-ZERO balance");
process.exit(zero ? 0 : 1);
