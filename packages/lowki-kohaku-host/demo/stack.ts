// The demo stack's Node side (started by scripts/demo-stack.sh, next to an anvil fork of
// Sepolia and lowki-server). One process, three listeners:
//
//   gateway  :GATEWAY_PORT   what the wallet reaches over Anon-RPC (through a public tunnel) or
//                            directly: /rpc (standard eth_* methods only, proxied to anvil, every
//                            call logged with the IP it came from), /ip (egress echo),
//                            /asp/leaves/:root (the fork ASP), /relayer/{quote,request,details}
//                            (the fork relayer: a fork account sends Entrypoint.relay)
//   wallet   :WALLET_PORT    apps/wallet-ui dist, /v1/ proxied to lowki-server (same origin),
//                            /lowki-config.json, /circuits/* (sha256-pinned artifacts),
//                            /pp-snapshot.json (Kohaku public pool state at the fork block)
//   dapp     :DAPP_PORT      the repo root, for apps/demo-dapp
//
// The ASP is a loop: it reads Deposited events on the ETH pool after the fork block and, after
// ASP_DELAY_MS, pushes a root over [0,1,2, ...labels] by impersonating Sepolia's test postman
// (exactly what scripts/fork-private-send.ts and Kohaku's e2e tests do), then serves the leaves.
//
//   node --import ./resolve-hook.mjs --import tsx demo/stack.ts
import http from "node:http";
import { createReadStream, existsSync, statSync, appendFileSync, writeFileSync, mkdirSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import {
  createPublicClient, createWalletClient, encodeAbiParameters, encodeFunctionData, getAddress, http as viemHttp, parseAbi,
  parseAbiItem, type Hex,
} from "viem";
import { generatePrivateKey, privateKeyToAccount } from "viem/accounts";
import { computeMerkleTreeRoot, encodeWithdrawalPayload, PrivacyPoolsV1_0xBow } from "../src/kohaku-pp";

const here = path.dirname(fileURLToPath(import.meta.url));
const REPO = path.resolve(here, "../../..");
const env = (k: string, d: string) => process.env[k] || d;
const ANVIL = env("ANVIL_RPC", "http://127.0.0.1:8814");
const GATEWAY_PORT = Number(env("GATEWAY_PORT", "8815"));
const WALLET_PORT = Number(env("WALLET_PORT", "8810"));
const DAPP_PORT = Number(env("DAPP_PORT", "8812"));
const LOWKI_SERVER = env("LOWKI_SERVER", "http://localhost:8813");
const WALLET_DIST = env("WALLET_DIST", path.join(REPO, "apps/wallet-ui/dist"));
const ARTIFACTS = env("PP_ARTIFACTS", path.resolve(REPO, "../vendor/npm/circuits/package/artifacts"));
const STATE = env("STATE", path.join(REPO, "target/demo-stack"));
const PUBLIC_URL = process.env.PUBLIC_URL?.replace(/\/$/, "") || "";
const ASP_DELAY_MS = Number(env("ASP_DELAY_MS", "3000"));
const RELAY_FEE_BPS = BigInt(env("RELAY_FEE_BPS", "50"));
const CHAIN_ID = 11155111;
const ENTRYPOINT = getAddress(PrivacyPoolsV1_0xBow[CHAIN_ID].entrypoint.entrypointAddress as Hex);
const ETH_POOL = getAddress("0x644d5A2554d36e27509254F32ccfeBe8cd58861f");
const POSTMAN = getAddress("0x696FE46495688fC9e99BAd2dAF2133B33de364eA");
const MOCK_IPFS_CID = "bafybeihrecrgyfkzyzli2oxnpfos5z2fgjt7zs52cbjyppigu64hva4z3i";
const ACCESS_LOG = path.join(STATE, "rpc-access.jsonl");

mkdirSync(STATE, { recursive: true });
const t0 = Date.now();
const log = (...a: unknown[]) => console.log(`[stack +${((Date.now() - t0) / 1000).toFixed(1)}s]`, ...a);

const pub = createPublicClient({ transport: viemHttp(ANVIL, { timeout: 120_000 }), cacheTime: 0, pollingInterval: 200 });
const anvil = (method: string, params: unknown[] = []) => pub.request({ method: method as never, params: params as never });
const epAbi = parseAbi([
  "function updateRoot(uint256 _root, string _ipfsCID) returns (uint256)",
  "function latestRoot() view returns (uint256)",
]);
const DEPOSITED = parseAbiItem("event Deposited(address indexed _depositor, uint256 _commitment, uint256 _label, uint256 _value, uint256 _precommitment)");

// ------------------------------------------------------------------ fork checks
const node = await anvil("anvil_nodeInfo").catch(() => undefined) as { forkConfig?: { forkBlockNumber?: number } } | undefined;

if (!node?.forkConfig?.forkBlockNumber) throw new Error(`${ANVIL} is not an anvil fork; refusing to run`);
const FORK_BLOCK = BigInt(node.forkConfig.forkBlockNumber);
const SNAPSHOT = env("PP_SNAPSHOT", path.join(REPO, `packages/lowki-kohaku-host/.cache/pp-state-aspblind-${CHAIN_ID}-${FORK_BLOCK}.json`));

if (!existsSync(SNAPSHOT)) throw new Error(`no Kohaku snapshot at ${SNAPSHOT}; run scripts/fork-private-send.sh once for FORK_BLOCK=${FORK_BLOCK}`);

// ------------------------------------------------------------------ ASP
const aspTrees = new Map<string, string[]>(); // root (decimal) -> leaves (decimal)
const BASE = [0n, 1n, 2n];
let approved: bigint[] = [];
const seen = new Map<string, number>(); // label -> first seen (ms)

async function pushRoot(leaves: bigint[]) {
  const root = await computeMerkleTreeRoot(leaves);

  aspTrees.set(root.toString(10), leaves.map(String));
  await anvil("anvil_impersonateAccount", [POSTMAN]);
  const hash = await pub.request({
    method: "eth_sendTransaction",
    params: [{ from: POSTMAN, to: ENTRYPOINT, data: encodeFunctionData({ abi: epAbi, functionName: "updateRoot", args: [root, MOCK_IPFS_CID] }) }],
  } as never) as Hex;
  const r = await pub.waitForTransactionReceipt({ hash });

  await anvil("anvil_stopImpersonatingAccount", [POSTMAN]);
  const latest = await pub.readContract({ address: ENTRYPOINT, abi: epAbi, functionName: "latestRoot" });

  if (latest !== root) throw new Error(`latestRoot ${latest} != pushed ${root}`);
  log(`ASP root ${root} (${leaves.length} labels) pushed as postman: tx ${hash} block ${r.blockNumber}`);
}

async function aspTick() {
  const head = await pub.getBlockNumber();

  if (head <= FORK_BLOCK) return;
  const logs = await pub.getLogs({ address: ETH_POOL, event: DEPOSITED, fromBlock: FORK_BLOCK + 1n, toBlock: head });
  const labels = logs.map((l) => l.args._label!);
  const now = Date.now();

  for (const l of labels) if (!seen.has(String(l))) { seen.set(String(l), now); log(`ASP: new deposit label ${l} (approve in ${ASP_DELAY_MS} ms)`); }
  const ready = labels.filter((l) => now - seen.get(String(l))! >= ASP_DELAY_MS);

  if (ready.length > approved.length) {
    approved = ready;
    await pushRoot([...BASE, ...approved]);
  }
}

await pushRoot(BASE);
let aspBusy = false;

setInterval(() => {
  if (aspBusy) return;
  aspBusy = true;
  aspTick().catch((e) => log("ASP tick failed:", e.message)).finally(() => { aspBusy = false; });
}, 1000);

// ------------------------------------------------------------------ relayer
const relayer = privateKeyToAccount(generatePrivateKey());

await anvil("anvil_setBalance", [relayer.address, `0x${(10n ** 19n).toString(16)}`]);
const relayerWallet = createWalletClient({ account: relayer, transport: viemHttp(ANVIL) });

log(`fork relayer ${relayer.address} (fee ${RELAY_FEE_BPS} bps)`);

const relayData = (recipient: Hex) => encodeAbiParameters(
  [{ type: "tuple", components: [{ name: "recipient", type: "address" }, { name: "feeRecipient", type: "address" }, { name: "relayFeeBPS", type: "uint256" }] }],
  [{ recipient: getAddress(recipient), feeRecipient: relayer.address, relayFeeBPS: RELAY_FEE_BPS }],
);

type Json = Record<string, any>;

const relayerApi = {
  details: () => ({
    feeBPS: String(RELAY_FEE_BPS), feeReceiverAddress: relayer.address, chainId: CHAIN_ID,
    assetAddress: "0xEeeeeEeeeEeEeeEeEeEeeEEEeeeeEeeeeeeeEEeE", minWithdrawAmount: "0", maxGasPrice: "0",
  }),
  quote: (b: Json) => {
    if (BigInt(b.chainId) !== BigInt(CHAIN_ID)) throw new Error(`unsupported chain ${b.chainId}`);

    return {
      baseFeeBPS: String(RELAY_FEE_BPS), feeBPS: String(RELAY_FEE_BPS), gasPrice: "1000000000",
      feeCommitment: { expiration: Date.now() + 600_000, withdrawalData: relayData(b.recipient as Hex), signedRelayerCommitment: "0x", extraGas: !!b.extraGas },
      detail: { relayTxCost: { gas: "0", eth: "0" } },
    };
  },
  // What a Privacy Pools relayer does on /relayer/request: simulate, then send
  // Entrypoint.relay(withdrawal, proof, scope) and pay the gas; the fee in the withdrawal repays it.
  request: async (b: Json) => {
    const data = encodeWithdrawalPayload(b.withdrawal, { proof: b.proof, publicSignals: b.publicSignals } as never, BigInt(b.scope));

    try {
      await pub.call({ account: relayer.address, to: ENTRYPOINT, data });
    } catch (e) {
      const m = (e as Error).message.split("\n").find((l) => /revert|reason|Error/i.test(l))?.trim() ?? "simulation reverted";

      log(`relayer: refused (simulation): ${m}`);

      return { success: false, error: m, timestamp: Date.now(), requestId: "sim" };
    }
    const hash = await relayerWallet.sendTransaction({ chain: null, to: ENTRYPOINT, data, value: 0n });
    const r = await pub.waitForTransactionReceipt({ hash });

    log(`relayer: Entrypoint.relay tx ${hash} block ${r.blockNumber} status ${r.status} gasUsed ${r.gasUsed}`);
    if (r.status !== "success") return { success: false, error: `relay reverted: ${hash}`, timestamp: Date.now(), requestId: hash };

    return { success: true, timestamp: Date.now(), requestId: hash, txHash: hash };
  },
};

// ------------------------------------------------------------------ HTTP helpers
const CORS = {
  "access-control-allow-origin": "*",
  "access-control-allow-methods": "GET, POST, OPTIONS",
  "access-control-allow-headers": "content-type",
  "access-control-max-age": "600",
};
const send = (res: http.ServerResponse, status: number, body: unknown, extra: Record<string, string> = {}) => {
  res.writeHead(status, { "content-type": "application/json", "cache-control": "no-store", ...CORS, ...extra });
  res.end(typeof body === "string" ? body : JSON.stringify(body));
};
const readBody = (req: http.IncomingMessage) => new Promise<string>((ok, fail) => {
  let s = "";

  req.on("data", (d) => { s += d; if (s.length > 4_000_000) req.destroy(); }).on("end", () => ok(s)).on("error", fail);
});
/** Who called: Cloudflare's header when the request came through the tunnel (an anon exit). */
const caller = (req: http.IncomingMessage) => {
  const cf = req.headers["cf-connecting-ip"];

  return cf ? { ip: String(cf), via: "tunnel" } : { ip: req.socket.remoteAddress ?? "?", via: "local" };
};

const ALLOWED = /^(eth_|net_version$|web3_clientVersion$)/;
const DENIED = new Set(["eth_sendTransaction", "eth_sign", "eth_signTransaction", "eth_accounts", "eth_signTypedData_v4"]);

// ------------------------------------------------------------------ gateway
const gateway = http.createServer(async (req, res) => {
  const url = new URL(req.url ?? "/", "http://x");
  const who = caller(req);

  try {
    if (req.method === "OPTIONS") { res.writeHead(204, CORS); res.end(); return; }
    if (url.pathname === "/rpc" && req.method === "POST") {
      const body = await readBody(req);
      const j = JSON.parse(body) as Json | Json[];
      const calls = Array.isArray(j) ? j : [j];
      const bad = calls.find((c) => !ALLOWED.test(String(c.method)) || DENIED.has(String(c.method)));

      appendFileSync(ACCESS_LOG, JSON.stringify({ t: new Date().toISOString(), ...who, calls: calls.map((c) => ({ method: c.method, params: c.params })) }) + "\n");
      if (bad) { send(res, 200, { jsonrpc: "2.0", id: bad.id ?? null, error: { code: -32601, message: `method ${bad.method} not served by the demo gateway` } }); return; }
      const up = await fetch(ANVIL, { method: "POST", headers: { "content-type": "application/json" }, body });

      send(res, up.status, await up.text());

      return;
    }
    if (url.pathname === "/ip") { send(res, 200, who); return; }
    if (url.pathname.startsWith("/asp/leaves/")) {
      const root = url.pathname.slice("/asp/leaves/".length);
      const leaves = aspTrees.get(BigInt(root).toString(10));

      appendFileSync(ACCESS_LOG, JSON.stringify({ t: new Date().toISOString(), ...who, asp: root }) + "\n");
      send(res, leaves ? 200 : 404, leaves ? { root, leaves } : { error: "unknown root" });

      return;
    }
    if (url.pathname === "/asp/status") { send(res, 200, { approved: approved.map(String), roots: [...aspTrees.keys()] }); return; }
    if (url.pathname.startsWith("/relayer/")) {
      const op = url.pathname.slice("/relayer/".length);

      appendFileSync(ACCESS_LOG, JSON.stringify({ t: new Date().toISOString(), ...who, relayer: op }) + "\n");
      if (op === "details") { send(res, 200, relayerApi.details()); return; }
      const b = JSON.parse(await readBody(req) || "{}");

      if (op === "quote") { send(res, 200, relayerApi.quote(b)); return; }
      if (op === "request") { send(res, 200, await relayerApi.request(b)); return; }
    }
    send(res, 404, { error: "not found" });
  } catch (e) {
    log(`gateway ${url.pathname}: ${(e as Error).message}`);
    send(res, 500, { error: (e as Error).message });
  }
});

// ------------------------------------------------------------------ static servers
const TYPES: Record<string, string> = {
  ".html": "text/html", ".js": "text/javascript", ".mjs": "text/javascript", ".wasm": "application/wasm", ".css": "text/css",
  ".svg": "image/svg+xml", ".woff2": "font/woff2", ".png": "image/png", ".json": "application/json", ".zkey": "application/octet-stream",
};
const file = (res: http.ServerResponse, f: string) => {
  if (!existsSync(f) || !statSync(f).isFile()) { res.writeHead(404); res.end("not found"); return; }
  res.writeHead(200, { "content-type": TYPES[path.extname(f)] ?? "application/octet-stream", "cache-control": "no-store", "content-length": statSync(f).size });
  createReadStream(f).pipe(res);
};
const under = (root: string, p: string) => {
  const f = path.join(root, decodeURIComponent(p));

  return f.startsWith(root) ? f : path.join(root, "__nope__");
};

const walletConfig = () => {
  const local = `http://localhost:${GATEWAY_PORT}`;

  return {
    chainId: CHAIN_ID,
    network: "Sepolia fork",
    rpcUrl: PUBLIC_URL ? `${PUBLIC_URL}/rpc` : null,
    directRpcUrl: `${local}/rpc`,
    anonUnavailable: "The demo fork runs on this machine and has no public URL (the tunnel is off), so the anon network can't reach it.",
    ipEchoUrl: PUBLIC_URL ? `${PUBLIC_URL}/ip` : null,
    private: {
      relayerUrl: PUBLIC_URL ? `${PUBLIC_URL}/relayer` : null,
      aspUrl: PUBLIC_URL ? `${PUBLIC_URL}/asp` : null,
      directRelayerUrl: `${local}/relayer`,
      directAspUrl: `${local}/asp`,
      snapshotUrl: "pp-snapshot.json",
      circuitsUrl: "circuits/",
      useGpu: process.env.USE_GPU !== "0",
    },
  };
};

const wallet = http.createServer((req, res) => {
  const url = new URL(req.url ?? "/", "http://x");

  if (url.pathname.startsWith("/v1/")) {
    const up = new URL(req.url!, LOWKI_SERVER);
    const p = http.request(up, { method: req.method, headers: { ...req.headers, host: up.host } }, (r) => {
      res.writeHead(r.statusCode ?? 502, r.headers);
      r.pipe(res);
    });

    p.on("error", () => { res.writeHead(502); res.end("{}"); });
    req.pipe(p);

    return;
  }
  if (url.pathname === "/lowki-config.json") { send(res, 200, walletConfig()); return; }
  if (url.pathname === "/pp-snapshot.json") { file(res, SNAPSHOT); return; }
  if (url.pathname.startsWith("/circuits/")) { file(res, under(ARTIFACTS, url.pathname.slice("/circuits".length))); return; }
  file(res, under(WALLET_DIST, url.pathname.endsWith("/") ? url.pathname + "index.html" : url.pathname));
});

const dapp = http.createServer((req, res) => {
  const p = new URL(req.url ?? "/", "http://x").pathname;

  file(res, under(REPO, p.endsWith("/") ? p + "index.html" : p));
});

const listen = (s: http.Server, port: number) => new Promise<void>((ok, fail) => s.once("error", fail).listen(port, "0.0.0.0", ok));

await listen(gateway, GATEWAY_PORT);
await listen(wallet, WALLET_PORT);
await listen(dapp, DAPP_PORT);
writeFileSync(path.join(STATE, "stack.json"), JSON.stringify({
  anvil: ANVIL, forkBlock: String(FORK_BLOCK), gateway: `http://localhost:${GATEWAY_PORT}`, publicUrl: PUBLIC_URL || null,
  wallet: `http://localhost:${WALLET_PORT}`, dapp: `http://localhost:${DAPP_PORT}`, relayer: relayer.address, accessLog: ACCESS_LOG,
}, null, 2));
log(`gateway http://localhost:${GATEWAY_PORT} (public: ${PUBLIC_URL || "none"}), wallet http://localhost:${WALLET_PORT}, dapp http://localhost:${DAPP_PORT}`);
log("READY");
