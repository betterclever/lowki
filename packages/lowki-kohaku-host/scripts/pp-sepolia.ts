// Live Sepolia Privacy Pools runbook (also runs against an anvil fork as a rehearsal).
//
//   node --import ./resolve-hook.mjs --import tsx scripts/pp-sepolia.ts <command>
//
// Commands
//   status    sync Kohaku on the Lowki Host; print the app account, its ETH and the notes
//   fund      LOWKI_SEPOLIA_KEY sends AMOUNT_WEI + gas buffer to the Lowki app account
//   shield    worker ppShield (checked against Kohaku prepareShield), signed by the worker
//   ragequit  Kohaku ragequit for LABEL (default: every pending note), commitment proof by the
//             worker, Pool.ragequit signed by the worker (the depositor)
//   withdraw  AMOUNT_WEI gross to RECIPIENT through the Privacy Pools relayer (RELAYER_URL);
//             needs a live ASP root that contains the note's label (ASP=ipfs|0xbow)
//
// Env
//   LOWKI_TEST_MNEMONIC  REQUIRED, 24 words. Notes are derived from it: lose it, lose the funds.
//                        Never printed.
//   LOWKI_SEPOLIA_KEY    funded EOA private key, used by `fund` only. Never printed (only its address).
//   BROADCAST=1          actually send. Without it every command stops after simulating
//                        (eth_estimateGas / eth_call) and prints what it would send.
//   RPC_URL              default https://sepolia.gateway.tenderly.co (needs 5000-block eth_getLogs)
//   LOWKI_ORIGIN         app origin whose Lowki account deposits (default https://shop.example)
//   AMOUNT_WEI           shield/fund/withdraw amount (default 0.01 ETH; pool minimum is 0.001)
//   GAS_BUFFER_WEI       extra ETH `fund` sends for gas (default 0.03 ETH)
//   RECIPIENT, LABEL     withdraw recipient; ragequit label (decimal)
//   ASP                  auto (default) | none | ipfs | 0xbow. none = ASP-blind (RootUpdated logs are
//                        withheld, so sync works on Sepolia today; withdraw is impossible)
//   RELAYER_URL          default https://testnet-relayer.privacypools.com/relayer
import { readFileSync, existsSync, writeFileSync, mkdirSync, readdirSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import {
  createPublicClient, createWalletClient, decodeAbiParameters, formatEther, getAddress, http, toEventSelector,
  type Hex,
} from "viem";
import { privateKeyToAccount } from "viem/accounts";
import type { AccountId, ERC20AssetId } from "@kohaku-eth/plugins";
import {
  DataService, E_ADDRESS, EthClient, IPFSAspService, OxBowAspService, PrivacyPoolsBroadcaster, PrivacyPoolsV1Protocol,
  PrivacyPoolsV1_0xBow, RelayerClient, type PPv1RelayerPrivateOperation,
} from "../src/kohaku-pp";
import {
  EncryptedStorage, InProcessTransport, MemoryBackend, call, createLowkiHost, createNetwork, createRpcProvider,
  randomStorageKey,
} from "../src/index";
import { lowkiProverFactory } from "../src/prover";
import { loadWorker } from "./load-worker";

const here = path.dirname(fileURLToPath(import.meta.url));
const VENDOR = path.resolve(here, "../../../..", "vendor");
const ARTIFACTS = process.env.PP_ARTIFACTS ?? path.join(VENDOR, "npm/circuits/package/artifacts");
const CHAIN_ID = 11155111;
const { entrypointAddress, deploymentBlock } = PrivacyPoolsV1_0xBow[CHAIN_ID].entrypoint;
const ENTRYPOINT = getAddress(entrypointAddress as Hex);
const RPC = process.env.RPC_URL ?? "https://sepolia.gateway.tenderly.co";
const ORIGIN = process.env.LOWKI_ORIGIN ?? "https://shop.example";
const AMOUNT = BigInt(process.env.AMOUNT_WEI ?? 10n ** 16n);
const RELAYER_URL = process.env.RELAYER_URL ?? "https://testnet-relayer.privacypools.com/relayer";
const BROADCAST = process.env.BROADCAST === "1";
const cmd = process.argv[2] ?? "status";
const ROOT_UPDATED = toEventSelector("event RootUpdated(uint256 _root, string _ipfsCID, uint256 _timestamp)");

const log = (...a: unknown[]) => console.log(`[${new Date().toISOString().slice(11, 19)}]`, ...a);
const eth = (w: bigint) => `${formatEther(w)} ETH`;
const die = (m: string): never => {
  console.error(`ERROR: ${m}`);
  process.exit(1);
};

if (!["status", "fund", "shield", "ragequit", "withdraw"].includes(cmd)) die(`unknown command ${cmd}`);
const mnemonic = process.env.LOWKI_TEST_MNEMONIC ?? die("LOWKI_TEST_MNEMONIC is required (notes are derived from it)");

const pub = createPublicClient({ transport: http(RPC, { timeout: 60_000 }), cacheTime: 0 });
const chainId = await pub.getChainId();

if (chainId !== CHAIN_ID) die(`RPC chainId ${chainId} is not Sepolia`);
const fork = await pub.request({ method: "anvil_nodeInfo" as never }).then(() => true, () => false);

log(`${fork ? "FORK REHEARSAL (anvil)" : "LIVE SEPOLIA"} rpc=${new URL(RPC).host} command=${cmd} broadcast=${BROADCAST ? "YES" : "no (dry run)"}`);

// ---- worker ----
const { worker, kind } = await loadWorker(30);

if (kind !== "real") die("needs the real wasm worker (crates/lowki-worker/pkg/node)");
const transport = new InProcessTransport(worker);

await call(transport, { type: "importMnemonic", mnemonic, prfOutput: crypto.getRandomValues(new Uint8Array(32)) });
const app = await call(transport, { type: "appAccount", origin: ORIGIN });
const APP = getAddress(app.address);

log(`Lowki app account (${ORIGIN}): ${APP}, ${eth(await pub.getBalance({ address: APP }))}`);

async function sendFromApp(label: string, tx: { to: Hex; data: Hex; value: bigint }) {
  const gas = (await pub.estimateGas({ account: APP, ...tx })) * 12n / 10n;
  const [nonce, block] = await Promise.all([pub.getTransactionCount({ address: APP, blockTag: "pending" }), pub.getBlock()]);
  const prio = 10n ** 9n;
  const maxFee = (block.baseFeePerGas ?? 0n) * 2n + prio;

  log(`${label}: to ${tx.to} value ${eth(tx.value)} gas ${gas} maxFee ${maxFee} (simulation OK)`);
  if (!BROADCAST) return log(`${label}: dry run, not sent (set BROADCAST=1)`);
  const signed = await call(transport, {
    type: "signTransaction", origin: ORIGIN,
    tx: {
      type: "eip1559", chainId: String(CHAIN_ID), nonce: String(nonce), gas: String(gas),
      maxPriorityFeePerGas: String(prio), maxFeePerGas: String(maxFee), to: tx.to, data: tx.data, value: String(tx.value),
    },
  });
  const hash = await pub.sendRawTransaction({ serializedTransaction: signed.rawTransaction });

  log(`${label}: sent ${hash}`);
  const r = await pub.waitForTransactionReceipt({ hash, timeout: 300_000 });

  log(`${label}: block ${r.blockNumber} status ${r.status} gasUsed ${r.gasUsed}`);
  if (r.status !== "success") die(`${label} reverted`);
}

if (cmd === "fund") {
  const key = (process.env.LOWKI_SEPOLIA_KEY ?? die("LOWKI_SEPOLIA_KEY is required for fund")) as Hex;
  const funder = privateKeyToAccount(key.startsWith("0x") ? key : `0x${key}` as Hex);
  // + gas for shield (~0.9M gas) and ragequit (~0.45M gas) at live base fees.
  const value = AMOUNT + BigInt(process.env.GAS_BUFFER_WEI ?? 3n * 10n ** 16n);

  log(`funder ${funder.address} (${eth(await pub.getBalance({ address: funder.address }))}) -> ${APP}: ${eth(value)}`);
  await pub.estimateGas({ account: funder.address, to: APP, value });
  if (!BROADCAST) {
    log("fund: dry run, not sent (set BROADCAST=1)");
    process.exit(0);
  }
  const hash = await createWalletClient({ account: funder, transport: http(RPC) }).sendTransaction({ chain: null, to: APP, value });
  const r = await pub.waitForTransactionReceipt({ hash, timeout: 300_000 });

  log(`fund: ${hash} status ${r.status}`);
  process.exit(r.status === "success" ? 0 : 1);
}

// ---- Kohaku on the Lowki Host ----
let ASP = process.env.ASP ?? "auto";
const network = createNetwork({
  allowHosts: [new URL(RPC).hostname, "ipfs.filebase.io", "api.0xbow.io", new URL(RELAYER_URL).hostname],
  httpsOnly: !fork,
});
const provider = createRpcProvider(RPC, network);

if (ASP === "auto") {
  const head = await provider.getBlockNumber();
  let cid: string | undefined;

  for (let to = head; to > deploymentBlock && cid === undefined; to -= 5000n) {
    const logs = await provider.getLogs({ address: ENTRYPOINT, fromBlock: to - 4999n, toBlock: to });
    const last = logs.filter((l) => l.topics[0] === ROOT_UPDATED).at(-1);

    if (last) cid = decodeAbiParameters([{ type: "uint256" }, { type: "string" }, { type: "uint256" }], last.data as Hex)[1];
  }
  const ok = cid !== undefined && await network.fetch(`https://ipfs.filebase.io/ipfs/${cid}`).then((r) => r.ok, () => false);

  ASP = ok ? "ipfs" : "none";
  log(`ASP preflight: latest RootUpdated ipfsCID=${cid} -> ${ok ? "served" : "not served"} -> ASP=${ASP}`);
}
const ethClient = new EthClient(provider);
const dataService = new DataService({
  provider,
  getLogs: async (p) => {
    const logs = await ethClient.getLogs(p);

    return ASP === "none" ? logs.filter((l) => l.topics[0] !== ROOT_UPDATED) : logs;
  },
});

// Public pool state only (deposits/withdrawals/leaves of everyone); no user secrets. The cache is
// ASP-blind (no RootUpdated events), so it is only used when ASP=none; with a real ASP start from
// Kohaku's snapshot (or PP_INITIAL_STATE) so the latest RootUpdated is seen.
const cacheDir = path.join(here, "..", ".cache");
const cached = ASP === "none" && existsSync(cacheDir)
  ? readdirSync(cacheDir).filter((f) => /^pp-state-aspblind-11155111-\d+\.json$/.test(f))
    .sort((a, b) => Number(a.match(/(\d+)\.json$/)![1]) - Number(b.match(/(\d+)\.json$/)![1])).at(-1)
  : undefined;
const statePath = process.env.PP_INITIAL_STATE
  ?? (cached ? path.join(cacheDir, cached) : path.join(VENDOR, "kohaku/packages/privacy-pools/tests/state.11155111.json"));
const initialState = JSON.parse(readFileSync(statePath, "utf8"));

// Kohaku's tests/state.11155111.json carries the [0,1,2] ASP root its fork tests pushed, which is
// not on the real chain. Real RootUpdated events after it supersede it with a real ASP; ASP-blind
// must drop it or root verification fails.
if (ASP === "none") {
  for (const st of Object.values(initialState as Record<string, any>)) {
    delete st.asp;
    delete st.updateRootEvents;
  }
}
const rd = (f: string) => new Uint8Array(readFileSync(path.join(ARTIFACTS, f)));

await call(transport, {
  type: "ppLoadCircuits", useGpu: false,
  withdrawWasm: rd("withdraw.wasm"), withdrawZkey: rd("withdraw.zkey"),
  commitmentWasm: rd("commitment.wasm"), commitmentZkey: rd("commitment.zkey"),
});
const lowki = createLowkiHost({
  transport, network, provider, wiring: "envelopeSigner", verifyDeterminism: false,
  storage: new EncryptedStorage(new MemoryBackend(), await randomStorageKey()),
});
const relayers = { "privacypools-testnet": RELAYER_URL };
const protocol = new PrivacyPoolsV1Protocol(lowki.host, {
  entrypoint: { address: BigInt(ENTRYPOINT), deploymentBlock },
  secretManager: lowki.secretManager,
  initialState: async () => initialState,
  dataService,
  relayersList: relayers,
  aspServiceFactory: ASP === "0xbow" ? () => new OxBowAspService({ network }) : () => new IPFSAspService({ network }),
  proverFactory: lowkiProverFactory(transport, {
    chainId: CHAIN_ID, entrypoint: ENTRYPOINT,
    onProof: ({ circuit, backend, ms }) => log(`lowki-worker ${circuit} proof: ${backend} ${ms.toFixed(0)} ms`),
  }),
});
const ETH: ERC20AssetId = { __type: "erc20", contract: E_ADDRESS as Hex };

log(`sync from ${path.basename(statePath)} ...`);
const t0 = Date.now();

await protocol.sync();
log(`synced in ${((Date.now() - t0) / 1000).toFixed(1)} s`);
if (!fork && ASP === "none") {
  const head = BigInt((Object.values(protocol.dumpState() as Record<string, any>)[0]).sync.lastSyncedBlock);

  mkdirSync(cacheDir, { recursive: true });
  writeFileSync(path.join(cacheDir, `pp-state-aspblind-11155111-${head}.json`),
    JSON.stringify(protocol.dumpState(), (_k, v) => (typeof v === "bigint" ? `0x${v.toString(16)}` : v)));
}
const printNotes = async () => {
  const notes = await protocol.notes([ETH], false, { sync: false });

  for (const n of notes) {
    log(`  note deposit#${n.deposit} withdraw#${n.withdraw} label ${n.label} balance ${eth(n.balance)} ${n.approved ? "approved" : "pending"}`);
  }
  if (notes.length === 0) log("  (no notes)");

  return notes;
};

log(`notes${ASP === "none" ? " (ASP-blind: everything shows as pending)" : ""}:`);
const notes = await printNotes();

if (cmd === "shield") {
  const k = (await protocol.prepareShield({ asset: ETH, amount: AMOUNT })).txns[0]! as { to: Hex; data: Hex; value: bigint };
  // Kohaku chose the next free deposit index (it counts spent notes too); the worker must
  // produce the identical deposit for that index.
  const all = await protocol.notes([ETH], true, { sync: false });
  const depositIndex = all.length === 0 ? 0 : Math.max(...all.map((n) => n.deposit)) + 1;
  const w = await call(transport, {
    type: "ppShield", value: String(AMOUNT),
    note: { accountIndex: 0, chainId: String(CHAIN_ID), entrypoint: ENTRYPOINT, depositIndex, secretIndex: 0 },
  });

  if (w.tx.data.toLowerCase() !== k.data.toLowerCase()) {
    die(`worker deposit (index ${depositIndex}) differs from Kohaku's prepareShield, refusing`);
  }
  await sendFromApp(`shield ${eth(AMOUNT)} (deposit #${depositIndex})`, { to: w.tx.to, data: w.tx.data, value: BigInt(w.tx.value) });
}

if (cmd === "ragequit") {
  const labels = process.env.LABEL ? [BigInt(process.env.LABEL)] : notes.filter((n) => !n.approved).map((n) => n.label);

  if (labels.length === 0) die("nothing to ragequit");
  const op = await protocol.ragequit(labels);

  for (const [i, tx] of (op.txns as { to: Hex; data: Hex; value: bigint }[]).entries()) {
    await sendFromApp(`ragequit label ${labels[i]}`, tx);
  }
}

if (cmd === "withdraw") {
  if (ASP === "none") die("withdraw needs an ASP root containing the label; this RPC's latest RootUpdated is not servable (ASP=none)");
  const recipient = getAddress(process.env.RECIPIENT ?? die("RECIPIENT is required"));
  const relayerClient = new RelayerClient({ network });
  const fees = await relayerClient.getFees({ relayerUrl: RELAYER_URL, chainId: BigInt(CHAIN_ID), assetAddress: BigInt(E_ADDRESS) });

  log(`relayer ${RELAYER_URL}: fee ${fees.feeBPS} bps, receiver ${fees.feeReceiverAddress}`);
  const op = await protocol.prepareUnshield({ asset: ETH, amount: AMOUNT }, recipient as AccountId);

  if (op.mode !== "relayer") die("expected a relayer operation");
  const rop = op as unknown as PPv1RelayerPrivateOperation;

  log(`relayData: recipient ${rop.rawData.relayData.recipient} feeRecipient ${rop.rawData.relayData.feeRecipient} fee ${rop.rawData.relayData.relayFeeBps} bps`);
  await pub.call({ to: rop.txData.to as Hex, data: rop.txData.data as Hex });
  log("Entrypoint.relay simulation (eth_call) OK");
  if (!BROADCAST) {
    log("withdraw: dry run, not sent to the relayer (set BROADCAST=1)");
  } else {
    if (fork) die("refusing to send a fork proof to the live relayer");
    const res = await new PrivacyPoolsBroadcaster({ host: lowki.host, broadcasterUrl: relayers }).broadcast(op);

    log(`relayer accepted: ${res.txHash}`);
    await pub.waitForTransactionReceipt({ hash: res.txHash, timeout: 300_000 });
  }
}

if (BROADCAST && cmd !== "status") {
  log("after:");
  await protocol.sync();
  await printNotes();
}
process.exit(0);
