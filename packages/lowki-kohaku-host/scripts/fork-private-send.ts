// End-to-end private send on an anvil fork of Sepolia, with Lowki's own code at every step:
//
//   shield    lowki-worker ppShield (cross-checked against Kohaku's prepareShield) -> worker
//             signTransaction from the user's app account -> eth_sendRawTransaction
//   ASP       impersonate Sepolia's test postman and push an ASP root containing our label
//             (exactly as Kohaku's e2e tests do); a local IAspService serves the leaves
//   sync      Kohaku PrivacyPoolsV1Protocol on the Lowki Host (keystore mode 2: note envelopes
//             signed in Rust), RPC = the fork
//   prove     lowki-worker ppProveWithdraw / ppProveCommitment (lowki-groth16) via src/prover.ts
//   submit    Kohaku PrivacyPoolsBroadcaster -> a fork relayer (relayer client whose relay()
//             signs and sends Entrypoint.relay from a fork account; fee = relayFeeBPS of the
//             quote, processooor = Entrypoint, both bound into the proof context)
//   check     recipient balance, nullifier spent, replay reverts, change note visible to Kohaku
//   ragequit  second deposit, commitment proof, Pool.ragequit signed by the worker (depositor)
//
//   bash scripts/fork-private-send.sh         (from the repo root; starts anvil and runs this)
//
// Env: FORK_RPC (default http://127.0.0.1:8547), LOWKI_TEST_MNEMONIC (else a fresh throwaway
// phrase), DEPOSIT_WEI (default 0.1 ETH), WITHDRAW_WEI (default 0.05 ETH), RELAY_FEE_BPS (50),
// PP_INITIAL_STATE (Kohaku state snapshot; default the fork-block cache, else Kohaku's
// tests/state.11155111.json), LOWKI_USE_GPU=1 (only meaningful in a browser).
//
// This script refuses to run against anything that is not an anvil fork (anvil_nodeInfo).
import { readFileSync, writeFileSync, existsSync, mkdirSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { generateMnemonic } from "@scure/bip39";
import { wordlist } from "@scure/bip39/wordlists/english.js";
import {
  createPublicClient, createWalletClient, encodeAbiParameters, encodeFunctionData, formatEther, getAddress, http,
  parseAbi, toEventSelector, type Hex,
} from "viem";
import { generatePrivateKey, privateKeyToAccount } from "viem/accounts";
import type { ERC20AssetId, AccountId } from "@kohaku-eth/plugins";
import {
  DataService, E_ADDRESS, EthClient, PrivacyPoolsBroadcaster, PrivacyPoolsV1Protocol, PrivacyPoolsV1_0xBow, computeMerkleTreeRoot,
  encodeWithdrawalPayload, lastUpdateRootEventSelector,
  type IAspService, type IRelayerClient, type PPv1RelayerPrivateOperation,
} from "../src/kohaku-pp";
import {
  EncryptedStorage, InProcessTransport, MemoryBackend, call, createLowkiHost, createNetwork, createRpcProvider,
  randomStorageKey,
} from "../src/index";
import { lowkiProverFactory } from "../src/prover";
import { loadWorker } from "./load-worker";

const here = path.dirname(fileURLToPath(import.meta.url));
const ROOT = path.resolve(here, "../../..");
const VENDOR = path.resolve(ROOT, "..", "vendor");
const ARTIFACTS = process.env.PP_ARTIFACTS ?? path.join(VENDOR, "npm/circuits/package/artifacts");

const RPC = process.env.FORK_RPC ?? "http://127.0.0.1:8547";
const CHAIN_ID = 11155111;
const { entrypointAddress, deploymentBlock } = PrivacyPoolsV1_0xBow[CHAIN_ID].entrypoint;
const ENTRYPOINT = getAddress(entrypointAddress as Hex);
const ETH_POOL = getAddress("0x644d5A2554d36e27509254F32ccfeBe8cd58861f");
const POSTMAN = getAddress("0x696FE46495688fC9e99BAd2dAF2133B33de364eA"); // Kohaku tests/constants.ts
const ORIGIN = "https://shop.example"; // the app whose per-app Lowki account deposits
const DEPOSIT = BigInt(process.env.DEPOSIT_WEI ?? 10n ** 17n);
const WITHDRAW = BigInt(process.env.WITHDRAW_WEI ?? 5n * 10n ** 16n);
const RELAY_FEE_BPS = BigInt(process.env.RELAY_FEE_BPS ?? 50);
const MOCK_IPFS_CID = "bafybeihrecrgyfkzyzli2oxnpfos5z2fgjt7zs52cbjyppigu64hva4z3i"; // Kohaku test-helpers.ts

const t00 = Date.now();
const log = (...a: unknown[]) => console.log(`[+${((Date.now() - t00) / 1000).toFixed(1).padStart(6)}s]`, ...a);
const eth = (w: bigint) => `${formatEther(w)} ETH`;
const fail = (m: string): never => {
  console.error(`FAIL: ${m}`);
  process.exit(1);
};
const check = (ok: boolean, m: string) => (ok ? log(`  ok: ${m}`) : fail(m));

const pub = createPublicClient({ transport: http(RPC, { timeout: 120_000 }), cacheTime: 0 });
const anvil = (method: string, params: unknown[] = []) => pub.request({ method: method as never, params: params as never });
const balanceOf = (a: Hex) => pub.getBalance({ address: a });

const poolAbi = parseAbi([
  "function nullifierHashes(uint256) view returns (bool)",
  "function depositors(uint256) view returns (address)",
  "function currentRoot() view returns (uint256)",
]);
const epAbi = parseAbi([
  "function updateRoot(uint256 _root, string _ipfsCID) returns (uint256)",
  "function latestRoot() view returns (uint256)",
  "function assetConfig(address) view returns (address pool, uint256 minimumDepositAmount, uint256 vettingFeeBPS, uint256 maxRelayFeeBPS)",
]);

// ---------------------------------------------------------------- 0. environment
const node = await anvil("anvil_nodeInfo").catch(() => undefined) as { forkConfig?: { forkUrl?: string; forkBlockNumber?: number } } | undefined;

if (!node?.forkConfig?.forkBlockNumber) fail(`${RPC} is not an anvil fork (anvil_nodeInfo); refusing to send transactions`);
const forkBlock = BigInt(node!.forkConfig!.forkBlockNumber!);
const chainId = await pub.getChainId();

check(chainId === CHAIN_ID, `fork chainId ${chainId}`);
log(`anvil fork of ${node!.forkConfig!.forkUrl} at block ${forkBlock}; rpc ${RPC}`);
const [, , vettingFeeBPS, maxRelayFeeBPS] = await pub.readContract({ address: ENTRYPOINT, abi: epAbi, functionName: "assetConfig", args: [E_ADDRESS as Hex] });

log(`ETH pool ${ETH_POOL}: vettingFeeBPS=${vettingFeeBPS} maxRelayFeeBPS=${maxRelayFeeBPS}`);

// ---------------------------------------------------------------- 1. Lowki worker
const envMnemonic = process.env.LOWKI_TEST_MNEMONIC;
const mnemonic = envMnemonic ?? generateMnemonic(wordlist, 256);
const { worker, kind, source } = await loadWorker(60);

if (kind !== "real") fail(`needs the real wasm worker (got ${kind})`);
const transport = new InProcessTransport(worker);

await call(transport, { type: "importMnemonic", mnemonic, prfOutput: crypto.getRandomValues(new Uint8Array(32)) });
log(`worker: ${source}`);
log(envMnemonic ? "mnemonic: LOWKI_TEST_MNEMONIC (not printed)" : `mnemonic: fresh throwaway (fork only): "${mnemonic}"`);
const rd = (f: string) => new Uint8Array(readFileSync(path.join(ARTIFACTS, f)));
const loaded = await call(transport, {
  type: "ppLoadCircuits",
  withdrawWasm: rd("withdraw.wasm"), withdrawZkey: rd("withdraw.zkey"),
  commitmentWasm: rd("commitment.wasm"), commitmentZkey: rd("commitment.zkey"),
  useGpu: process.env.LOWKI_USE_GPU === "1",
});

log(`circuits: ${loaded.circuits.join(",")} witness=${loaded.witnessBackend} gpu=${loaded.gpu}${loaded.gpuError ? ` (${loaded.gpuError})` : ""}`);
const app = await call(transport, { type: "appAccount", origin: ORIGIN });
const APP = getAddress(app.address);

log(`app account for ${ORIGIN}: ${APP} (m/44'/60'/${app.appIndex}'/0/0)`);

// Fork-only funding and actors.
await anvil("anvil_setBalance", [APP, `0x${(10n ** 18n).toString(16)}`]);
const relayerKey = generatePrivateKey();
const relayer = privateKeyToAccount(relayerKey);

await anvil("anvil_setBalance", [relayer.address, `0x${(10n ** 18n).toString(16)}`]);
const recipient = privateKeyToAccount(generatePrivateKey()).address;

log(`relayer (fork account) ${relayer.address}; recipient (fresh) ${recipient}`);

/** Signs `tx` in the worker with the app account and sends it raw. */
async function sendFromApp(label: string, tx: { to: Hex; data: Hex; value: bigint }, gasOverride?: bigint) {
  const [nonce, block] = await Promise.all([
    pub.getTransactionCount({ address: APP, blockTag: "pending" }), pub.getBlock(),
  ]);
  const gas = gasOverride ?? (await pub.estimateGas({ account: APP, to: tx.to, data: tx.data, value: tx.value })) * 12n / 10n;
  const prio = 10n ** 9n;
  const signed = await call(transport, {
    type: "signTransaction",
    origin: ORIGIN,
    tx: {
      type: "eip1559", chainId: String(CHAIN_ID), nonce: String(nonce), gas: String(gas),
      maxPriorityFeePerGas: String(prio), maxFeePerGas: String((block.baseFeePerGas ?? 0n) * 2n + prio),
      to: tx.to, data: tx.data, value: String(tx.value),
    },
  });
  const hash = await pub.sendRawTransaction({ serializedTransaction: signed.rawTransaction });
  const receipt = await pub.waitForTransactionReceipt({ hash });

  log(`  ${label}: tx ${hash} block ${receipt.blockNumber} status ${receipt.status} gasUsed ${receipt.gasUsed}`);

  return receipt;
}

// ---------------------------------------------------------------- 2. local ASP + postman
/** The fork's ASP: approved label sets by root, served to Kohaku as IAspService. Kohaku rebuilds
 * the root from the leaves and checks it against the Entrypoint's RootUpdated event. */
const aspTrees = new Map<bigint, bigint[]>();
const aspService: IAspService = {
  async getAspTree(state) {
    const last = lastUpdateRootEventSelector(state);
    const leaves = last ? aspTrees.get(BigInt(last.root)) : undefined;

    if (!leaves) throw new Error(`fork ASP: no tree for on-chain root ${last?.root}`);

    return [leaves];
  },
};

async function pushAspRoot(leaves: bigint[]) {
  const root = await computeMerkleTreeRoot(leaves);

  aspTrees.set(root, leaves);
  await anvil("anvil_impersonateAccount", [POSTMAN]);
  const hash = await pub.request({
    method: "eth_sendTransaction",
    params: [{ from: POSTMAN, to: ENTRYPOINT, data: encodeFunctionData({ abi: epAbi, functionName: "updateRoot", args: [root, MOCK_IPFS_CID] }) }],
  } as never) as Hex;
  const r = await pub.waitForTransactionReceipt({ hash });

  await anvil("anvil_stopImpersonatingAccount", [POSTMAN]);
  const latest = await pub.readContract({ address: ENTRYPOINT, abi: epAbi, functionName: "latestRoot" });

  log(`  ASP updateRoot as postman ${POSTMAN}: tx ${hash} status ${r.status}; ${leaves.length} labels, root ${root}`);
  check(latest === root, "Entrypoint.latestRoot() == our ASP root");

  return root;
}


// ---------------------------------------------------------------- 3. Kohaku on the Lowki Host
const network = createNetwork({ allowHosts: [new URL(RPC).hostname], httpsOnly: false });
const provider = createRpcProvider(RPC, network);
const storage = new EncryptedStorage(new MemoryBackend(), await randomStorageKey());
const lowki = createLowkiHost({ transport, storage, network, provider, wiring: "envelopeSigner", verifyDeterminism: false });
const proofs: { circuit: string; backend: string; ms: number; deposit: number; secret: number }[] = [];

const relayerUrl = "fork://lowki-relayer";
const forkRelayer: IRelayerClient = {
  async getQuote(body) {
    const relayData = { recipient: getAddress(`0x${body.recipient.toString(16).padStart(40, "0")}`), feeRecipient: relayer.address, relayFeeBPS: RELAY_FEE_BPS };
    const withdrawalData = encodeAbiParameters(
      [{ type: "tuple", components: [{ name: "recipient", type: "address" }, { name: "feeRecipient", type: "address" }, { name: "relayFeeBPS", type: "uint256" }] }],
      [relayData],
    );

    return {
      baseFeeBPS: String(RELAY_FEE_BPS), feeBPS: String(RELAY_FEE_BPS), gasPrice: "1000000000",
      feeCommitment: { expiration: Date.now() + 600_000, withdrawalData, signedRelayerCommitment: "0x", extraGas: body.extraGas },
      detail: { relayTxCost: { gas: "0", eth: "0" } },
    };
  },
  // What a Privacy Pools relayer does on /relayer/request: Entrypoint.relay(withdrawal, proof, scope),
  // paid for by the relayer, refunded by the fee in the withdrawal data.
  async relay(body) {
    const data = encodeWithdrawalPayload(body.withdrawal, { proof: body.proof, publicSignals: body.publicSignals } as never, body.scope);
    const wallet = createWalletClient({ account: relayer, transport: http(RPC) });
    const hash = await wallet.sendTransaction({ chain: null, to: ENTRYPOINT, data, value: 0n });
    const r = await pub.waitForTransactionReceipt({ hash });

    if (r.status !== "success") throw new Error(`relay reverted: ${hash}`);

    return { success: true, timestamp: Date.now(), requestId: hash, txHash: hash };
  },
  async getFees(body) {
    return { feeBPS: String(RELAY_FEE_BPS), feeReceiverAddress: relayer.address, chainId: Number(body.chainId), assetAddress: String(body.assetAddress), minWithdrawAmount: "0", maxGasPrice: "0" };
  },
};

// Public pool state at the fork block, cached so later runs skip the catch-up. It is built
// ASP-blind (RootUpdated logs withheld: every live Sepolia root carries a placeholder CID no ASP
// service can serve) and BEFORE any fork transaction, so it only holds real Sepolia data.
const cacheDir = path.join(here, "..", ".cache");
const cachePath = path.join(cacheDir, `pp-state-aspblind-${CHAIN_ID}-${forkBlock}.json`);
const kohakuSnapshot = path.join(VENDOR, "kohaku/packages/privacy-pools/tests/state.11155111.json");
const lastSynced = (st: unknown) => BigInt(Object.values(st as Record<string, any>)[0].sync.lastSyncedBlock);
const toJson = (st: unknown) => JSON.stringify(st, (_k, v) => (typeof v === "bigint" ? `0x${v.toString(16)}` : v));

if (!process.env.PP_INITIAL_STATE && !existsSync(cachePath)) {
  const snap = JSON.parse(readFileSync(kohakuSnapshot, "utf8"));

  // Kohaku's snapshot carries the [0,1,2] test root its own fork tests pushed (block 0xb16d71);
  // that root is not on the real chain, so an ASP-blind sync must forget it.
  for (const st of Object.values(snap as Record<string, any>)) {
    delete st.asp;
    delete st.updateRootEvents;
  }
  const ROOT_UPDATED = toEventSelector("event RootUpdated(uint256 _root, string _ipfsCID, uint256 _timestamp)");
  const ethClient = new EthClient(provider);
  const blindHost = { ...lowki.host, storage: new EncryptedStorage(new MemoryBackend(), await randomStorageKey()) };
  const blind = new PrivacyPoolsV1Protocol(blindHost, {
    entrypoint: { address: BigInt(ENTRYPOINT), deploymentBlock },
    secretManager: lowki.secretManager,
    initialState: async () => snap,
    dataService: new DataService({
      provider, getLogs: async (p) => (await ethClient.getLogs(p)).filter((l) => l.topics[0] !== ROOT_UPDATED),
    }),
  });
  const t1 = Date.now();

  log(`catch-up sync (ASP-blind) from Kohaku's snapshot @${lastSynced(snap)} to fork block ${forkBlock} ...`);
  await blind.sync();
  mkdirSync(cacheDir, { recursive: true });
  writeFileSync(cachePath, toJson(blind.dumpState()));
  log(`  ${((Date.now() - t1) / 1000).toFixed(1)} s; cached -> ${path.relative(ROOT, cachePath)}`);
}
const statePath = process.env.PP_INITIAL_STATE ?? cachePath;
const initialState = JSON.parse(readFileSync(statePath, "utf8"));

log(`Kohaku initial state: ${path.relative(ROOT, statePath)} (lastSyncedBlock ${lastSynced(initialState)})`);

// Kohaku's e2e tests push a root before the first sync (test-helpers.ts getProtocolWithState).
log("ASP: initial root (Kohaku test labels [0,1,2])");
const baseLabels = [0n, 1n, 2n];

await pushAspRoot(baseLabels);

const protocol = new PrivacyPoolsV1Protocol(lowki.host, {
  entrypoint: { address: BigInt(ENTRYPOINT), deploymentBlock },
  secretManager: lowki.secretManager,
  initialState: async () => initialState,
  aspServiceFactory: () => aspService,
  relayersList: { "lowki-fork-relayer": relayerUrl },
  relayerClientFactory: () => forkRelayer,
  proverFactory: lowkiProverFactory(transport, {
    chainId: CHAIN_ID, entrypoint: ENTRYPOINT,
    onProof: ({ circuit, backend, ms, note }) => {
      proofs.push({ circuit, backend, ms, deposit: note.depositIndex, secret: note.secretIndex });
      log(`  lowki-worker ${circuit} proof: backend=${backend} ${ms.toFixed(0)} ms (note deposit ${note.depositIndex}, secret ${note.secretIndex})`);
    },
  }),
});
const broadcaster = new PrivacyPoolsBroadcaster({ host: lowki.host, broadcasterUrl: { "lowki-fork-relayer": relayerUrl }, relayerClientFactory: () => forkRelayer });
const ETH: ERC20AssetId = { __type: "erc20", contract: E_ADDRESS as Hex };
const balances = async () => {
  const b = await protocol.balance([ETH]);

  return { approved: b.find((x) => !x.tag)!.amount, pending: b.find((x) => x.tag === "pending")!.amount };
};

log("sync: Kohaku PrivacyPoolsV1Protocol on the Lowki Host, against the fork");
let t = Date.now();

await protocol.sync();
log(`  synced to fork head in ${((Date.now() - t) / 1000).toFixed(1)} s`);
check((await balances()).approved === 0n && (await balances()).pending === 0n, "fresh account has no notes");

// ---------------------------------------------------------------- 4. shield x2
const shieldPre: string[] = [];

for (const depositIndex of [0, 1]) {
  log(`shield #${depositIndex}: ${eth(DEPOSIT)} from ${APP}`);
  const k = (await protocol.prepareShield({ asset: ETH, amount: DEPOSIT })).txns[0]! as { to: Hex; data: Hex; value: bigint };
  const w = await call(transport, {
    type: "ppShield",
    note: { accountIndex: 0, chainId: String(CHAIN_ID), entrypoint: ENTRYPOINT, depositIndex, secretIndex: 0 },
    value: String(DEPOSIT),
  });

  check(w.tx.data.toLowerCase() === k.data.toLowerCase() && getAddress(w.tx.to) === getAddress(k.to) && BigInt(w.tx.value) === BigInt(k.value),
    `worker ppShield(depositIndex ${depositIndex}) == Kohaku prepareShield (precommitment ${w.precommitment})`);
  shieldPre.push(w.precommitment);
  const r = await sendFromApp(`Entrypoint.deposit #${depositIndex}`, { to: w.tx.to, data: w.tx.data, value: BigInt(w.tx.value) });

  check(r.status === "success", "deposit mined");
}

const notes0 = await protocol.notes([ETH]);
const credited = DEPOSIT - DEPOSIT * vettingFeeBPS / 10_000n;

check(notes0.length === 2 && notes0.every((n) => n.balance === credited),
  `Kohaku sees 2 notes of ${eth(credited)} (deposit minus ${vettingFeeBPS} bps vetting fee)`);
const noteW = notes0.find((n) => n.deposit === 0)!;
const noteR = notes0.find((n) => n.deposit === 1)!;

check(BigInt(noteW.precommitment) === BigInt(shieldPre[0]!) && BigInt(noteR.precommitment) === BigInt(shieldPre[1]!), "notes match the worker's precommitments");
for (const n of [noteW, noteR]) {
  const dep = await pub.readContract({ address: ETH_POOL, abi: poolAbi, functionName: "depositors", args: [n.label] });

  check(getAddress(dep) === APP, `Pool.depositors(label ${n.label}) == app account`);
}

log(`balance before ASP: ${JSON.stringify(await balances(), (_k, v) => typeof v === "bigint" ? eth(v) : v)}`);

// ---------------------------------------------------------------- 5. ASP approves deposit #0
log(`ASP: approve label of deposit #0 (${noteW.label})`);
await pushAspRoot([...baseLabels, noteW.label]);
const b1 = await balances();

check(b1.approved === credited && b1.pending === credited, `approved ${eth(b1.approved)}, pending ${eth(b1.pending)}`);

// ---------------------------------------------------------------- 6. private withdraw via relay
log(`withdraw: ${eth(WITHDRAW)} gross from deposit #0 to ${recipient} via relayer (fee ${RELAY_FEE_BPS} bps)`);
const recipient0 = await balanceOf(recipient);
const relayer0 = await balanceOf(relayer.address);

check(recipient0 === 0n, "recipient starts at 0 ETH");
t = Date.now();
const op = await protocol.prepareUnshield({ asset: ETH, amount: WITHDRAW }, recipient as AccountId);

log(`  prepareUnshield (sync + quote + Merkle proofs + lowki proof) ${((Date.now() - t) / 1000).toFixed(1)} s`);
if (op.mode !== "relayer") fail("expected a relayer operation");
const rop = op as unknown as PPv1RelayerPrivateOperation;
const nullifierHash = rop.rawData.proof.mappedSignals.existingNullifierHash;

check(getAddress(rop.rawData.withdrawalPayload.processooor) === ENTRYPOINT, "withdrawal.processooor == Entrypoint (relay path)");
check(BigInt(rop.rawData.relayData.relayFeeBps) === RELAY_FEE_BPS && getAddress(rop.rawData.relayData.feeRecipient) === relayer.address,
  `relayData: fee ${rop.rawData.relayData.relayFeeBps} bps to ${rop.rawData.relayData.feeRecipient}`);
check(!(await pub.readContract({ address: ETH_POOL, abi: poolAbi, functionName: "nullifierHashes", args: [nullifierHash] })), "nullifier unspent before");
const sent = await broadcaster.broadcast(op);
const wr = await pub.getTransactionReceipt({ hash: sent.txHash });

log(`  Entrypoint.relay: tx ${sent.txHash} block ${wr.blockNumber} status ${wr.status} gasUsed ${wr.gasUsed} (sent by relayer ${relayer.address})`);
const fee = WITHDRAW * RELAY_FEE_BPS / 10_000n;
const recipient1 = await balanceOf(recipient);
const relayer1 = await balanceOf(relayer.address);
const relayGas = wr.gasUsed * wr.effectiveGasPrice;

check(recipient1 === WITHDRAW - fee, `recipient: ${eth(recipient0)} -> ${eth(recipient1)} (= ${eth(WITHDRAW)} - ${eth(fee)} fee)`);
check(relayer1 === relayer0 + fee - relayGas, `relayer: ${eth(relayer0)} -> ${eth(relayer1)} (+fee ${eth(fee)} - gas ${eth(relayGas)})`);
check(await pub.readContract({ address: ETH_POOL, abi: poolAbi, functionName: "nullifierHashes", args: [nullifierHash] }), `Pool.nullifierHashes(${nullifierHash}) == true`);

// Replay: the same proof again (what a second withdraw of the same note amounts to).
log("replay the same withdrawal (same note, same proof)");
const replayData = encodeWithdrawalPayload(rop.rawData.withdrawalPayload, rop.rawData.proof as never, rop.rawData.scope);
const replayErr = await pub.call({ account: relayer.address, to: ENTRYPOINT, data: replayData }).then(() => undefined, (e: Error) => e);

check(replayErr !== undefined, `eth_call reverts: ${replayErr?.message.split("\n").find((l) => /revert|Error|reason/i.test(l))?.trim()}`);
const replayHash = await createWalletClient({ account: relayer, transport: http(RPC) })
  .sendTransaction({ chain: null, to: ENTRYPOINT, data: replayData, value: 0n, gas: 1_000_000n });
const replayR = await pub.waitForTransactionReceipt({ hash: replayHash });

check(replayR.status === "reverted", `replay tx ${replayHash} mined with status ${replayR.status}`);
check(await balanceOf(recipient) === recipient1, "recipient unchanged after replay");

const notes1 = await protocol.notes([ETH]);
const change = notes1.find((n) => n.label === noteW.label);

check(change?.balance === credited - WITHDRAW && change.withdraw === 1,
  `Kohaku sees the change note: ${eth(change?.balance ?? 0n)} at secretIndex ${change?.withdraw} (worker-derived change commitment matches Kohaku)`);

// ---------------------------------------------------------------- 7. ragequit deposit #1
log(`ragequit: deposit #1 (label ${noteR.label}, not ASP-approved) back to its depositor ${APP}`);
const rq = await protocol.ragequit([noteR.label]);
const rqTx = rq.txns[0]! as { to: Hex; data: Hex; value: bigint };
const app0 = await balanceOf(APP);
const rr = await sendFromApp("Pool.ragequit", rqTx);
const app1 = await balanceOf(APP);
const rqGas = rr.gasUsed * rr.effectiveGasPrice;

check(rr.status === "success", "ragequit mined");
check(app1 === app0 + credited - rqGas, `app account: ${eth(app0)} -> ${eth(app1)} (+${eth(credited)} - gas ${eth(rqGas)})`);
const rqReplay = await pub.call({ account: APP, to: rqTx.to, data: rqTx.data }).then(() => undefined, (e: Error) => e);

check(rqReplay !== undefined, "ragequit replay reverts");
const notes2 = await protocol.notes([ETH]);

check(!notes2.some((n) => n.label === noteR.label), "Kohaku drops the ragequit note");
const bEnd = await balances();

log(`final Kohaku balance: approved ${eth(bEnd.approved)}, pending ${eth(bEnd.pending)}`);
log(`proofs: ${proofs.map((p) => `${p.circuit}/${p.backend}/${p.ms.toFixed(0)}ms`).join(", ")}`);
log("RESULT: PASS (shield -> ASP -> sync -> lowki proof -> relay -> nullifier spent -> replay rejected; ragequit)");
process.exit(0);
