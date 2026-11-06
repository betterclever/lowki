// In-browser probe of the anon-rpc provider mode (src/anon-provider.ts) for one network.
// Query: ?net=nox|nym|tor-js[&gw=ip:port:hash][&boot=ms][&rpc=url][&second=0][&logs=0]
// Result: window.__anon (polled by run-mac.mjs).
import { EthClient } from "@kohaku-eth/privacy-pools/src/data/eth-client";
import { createAnonProvider, pickAnonNetwork, type AnonNetworkId, type AnonProviderResult } from "../src/anon-provider";

const q = new URLSearchParams(location.search);
const net = (q.get("net") ?? pickAnonNetwork()) as AnonNetworkId;
const RPC = q.get("rpc") ?? "https://sepolia.gateway.tenderly.co";
const IP_ECHO = q.get("ip") ?? "https://api.ipify.org?format=json";
const ETH_POOL = "0x644d5A2554d36e27509254F32ccfeBe8cd58861f";
const ENTRYPOINT = "0x34A2068192b1297f2a7f85D7D8CdE66F8F0921cB";

declare global { interface Window { __anon?: Record<string, unknown> } }

const out = document.getElementById("out")!;
const log = (s: string) => { out.textContent += s + "\n"; console.log(`[anon] ${s}`); };
const R: Record<string, unknown> = { net, rpc: RPC, ua: navigator.userAgent, steps: {} };
const steps = R.steps as Record<string, unknown>;

async function timed<T>(name: string, fn: () => Promise<T>): Promise<T | undefined> {
  const t = performance.now();

  try {
    const v = await fn();
    const ms = Math.round(performance.now() - t);

    steps[name] = { ok: true, ms, value: typeof v === "bigint" ? v.toString() : v };
    log(`${name}: ${ms} ms -> ${JSON.stringify(steps[name] && (steps[name] as { value: unknown }).value)}`);

    return v;
  } catch (e) {
    const ms = Math.round(performance.now() - t);

    steps[name] = { ok: false, ms, error: e instanceof Error ? `${e.name}: ${e.message}` : String(e) };
    log(`${name}: FAILED after ${ms} ms: ${(steps[name] as { error: string }).error}`);

    return undefined;
  }
}

const ipOf = async (f: (u: string) => Promise<Response>) => {
  const r = await f(IP_ECHO);
  const t = await r.text();

  try { return (JSON.parse(t) as { ip: string }).ip; } catch { return t.trim(); }
};

async function session(key: string | undefined): Promise<AnonProviderResult> {
  return createAnonProvider({
    rpcUrl: RPC,
    network: net,
    bootTimeoutMs: Number(q.get("boot") ?? 120_000),
    callTimeoutMs: 120_000,
    ...(q.get("gw") ? { torGateways: [q.get("gw")!] } : {}),
    ...(key ? { isolationKey: key } : {}),
    onFallback: ({ reason }) => log(`FALLBACK to direct RPC (anonymous: false): ${reason}`),
  });
}

async function main() {
  log(`network ${net}, rpc ${RPC}`);
  R.directIp = await timed("direct.ipEcho", () => ipOf((u) => fetch(u)));
  await timed("direct.eth_blockNumber", () => fetch(RPC, {
    method: "POST", headers: { "content-type": "application/json" },
    body: JSON.stringify({ jsonrpc: "2.0", id: 1, method: "eth_blockNumber", params: [] }),
  }).then((r) => r.json()).then((j) => j.result));

  const a = await session("addr-A");

  R.anonymous = a.anonymous;
  R.bootMs = a.bootMs;
  if (!a.anonymous) R.fallbackReason = a.reason;
  log(`boot ${net}: ${a.bootMs} ms, anonymous=${a.anonymous}`);
  if (!a.anonymous) return;

  R.anonIpA1 = await timed("anon.ipEcho[keyA]#1", () => ipOf(a.fetch));
  R.anonIpA2 = await timed("anon.ipEcho[keyA]#2", () => ipOf(a.fetch));
  await timed("anon.eth_chainId", () => a.provider.getChainId());
  const bn = await timed("anon.eth_blockNumber#1", () => a.provider.getBlockNumber());

  await timed("anon.eth_blockNumber#2", () => a.provider.getBlockNumber());
  await timed("anon.eth_blockNumber#3", () => a.provider.getBlockNumber());
  await timed("anon.eth_getBalance(pool)", () => a.provider.getBalance(ETH_POOL));
  await timed("anon.eth_getTransactionCount(entrypoint)", () => a.provider.request({ method: "eth_getTransactionCount", params: [ENTRYPOINT, "latest"] }));

  // isolationKey within the SAME worker: different key -> different egress?
  R.anonIpSameWorkerKeyB = await timed("anon.ipEcho[sameWorker,keyB]", () => ipOf((u) => a.fetch(u, { anonOptions: { isolationKey: "addr-B" } })));
  R.anonIpSameWorkerKeyA3 = await timed("anon.ipEcho[keyA]#3", () => ipOf(a.fetch));

  // Egress sample: N echoes under each key, to see whether the exit depends on the key.
  const n = Number(q.get("ipn") ?? 0);

  if (n > 0) {
    const sample: Record<string, string[]> = {};

    for (const key of ["addr-A", "addr-B"]) {
      sample[key] = [];
      for (let i = 0; i < n; i++) {
        const ip = await timed(`anon.ipEcho.sample[${key}]#${i + 1}`, () => ipOf((u) => a.fetch(u, { anonOptions: { isolationKey: key } })));

        if (ip) sample[key]!.push(ip);
      }
    }
    R.egressSample = sample;
  }

  // Kohaku sync step: EthClient.getLogs issues provider.getLogs over 5000-block windows.
  if (q.get("logs") !== "0" && bn) {
    for (const [i, from] of [bn - 10_000n, bn - 5_000n].entries()) {
      await timed(`anon.eth_getLogs(pool, 5000 blocks)#${i + 1}`, async () =>
        (await a.provider.getLogs({ address: ETH_POOL, fromBlock: from, toBlock: from + 4_999n })).length);
    }
    await timed("direct.eth_getLogs(pool, 5000 blocks)", () => fetch(RPC, {
      method: "POST", headers: { "content-type": "application/json" },
      body: JSON.stringify({ jsonrpc: "2.0", id: 1, method: "eth_getLogs", params: [{ address: ETH_POOL, fromBlock: "0x" + (bn - 5_000n).toString(16), toBlock: "0x" + (bn - 1n).toString(16) }] }),
    }).then((r) => r.json()).then((j) => j.result?.length ?? j.error));
  }

  // Kohaku's own sync primitives (privacy-pools EthClient, the class syncThunk/DataService use)
  // over the anon provider: an incremental sync step of the last `kblocks` blocks.
  if (q.get("kohaku") !== "0") {
    const eth = new EthClient(a.provider);
    const kb = BigInt(q.get("kblocks") ?? 15_000);
    const t = performance.now();
    const to = await timed("kohaku.getBlockNumber", () => eth.getBlockNumber());

    if (to) {
      await timed(`kohaku.getLogs(entrypoint, ${kb} blocks)`, async () =>
        (await eth.getLogs({ address: ENTRYPOINT, fromBlock: to - kb, toBlock: to })).length);
      await timed(`kohaku.getLogs(ETH pool, ${kb} blocks)`, async () =>
        (await eth.getLogs({ address: ETH_POOL, fromBlock: to - kb, toBlock: to })).length);
      await timed("kohaku.pool.currentRoot", async () => String(await eth.makeContractRequest(BigInt(ETH_POOL), "pool", "currentRoot")));
      await timed("kohaku.entrypoint.latestRoot", async () => String(await eth.makeContractRequest(BigInt(ENTRYPOINT), "entrypoint", "latestRoot")));
    }
    R.kohakuStepMs = Math.round(performance.now() - t);
    log(`kohaku incremental step (${kb} blocks): ${R.kohakuStepMs} ms`);
  }

  // Per-address isolation by a SEPARATE worker (createAnonSessionPool isolation="worker").
  if (q.get("second") !== "0") {
    const b = await session("addr-B");

    R.secondBootMs = b.bootMs;
    R.secondAnonymous = b.anonymous;
    log(`second worker boot: ${b.bootMs} ms, anonymous=${b.anonymous}`);
    if (b.anonymous) {
      R.anonIpSecondWorker = await timed("anon.ipEcho[secondWorker]", () => ipOf(b.fetch));
      await timed("anon.eth_getBalance[secondWorker]", () => b.provider.getBalance(ETH_POOL));
    }
    b.close();
  }
  a.close();
}

main()
  .then(() => { R.ok = true; })
  .catch((e) => { R.ok = false; R.error = e instanceof Error ? e.stack ?? e.message : String(e); log(`FAILED ${R.error}`); })
  .finally(() => { window.__anon = R; });
