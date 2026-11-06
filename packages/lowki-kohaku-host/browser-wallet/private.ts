// Lazy chunk of the wallet glue: Kohaku Privacy Pools + the worker prover adapter. Every method
// returns JSON text so the Rust side parses it with serde (no JS object walking in Rust).
import type { EthereumProvider } from "@kohaku-eth/provider";
import { LowkiPrivateWallet, type CircuitArtifacts, type Stage } from "../src/private-wallet";
import { createNetwork } from "../src/network";
import type { Hex, WorkerRequest, WorkerResponse } from "../src/protocol";

export type OpenOptions = {
  request: (req: unknown) => Promise<unknown>;
  provider: EthereumProvider;
  fetch: typeof fetch;
  chainId: number;
  relayerUrl: string;
  aspUrl: string;
  snapshotUrl?: string;
  circuitsUrl: string;
  useGpu: boolean;
};

const J = (v: unknown) => JSON.stringify(v, (_k, x) => (typeof x === "bigint" ? x.toString() : x));
// serde-wasm-bindgen may hand back Maps.
const plain = (v: unknown): unknown => {
  if (v instanceof Map) return Object.fromEntries([...v].map(([k, x]) => [k, plain(x)]));
  if (Array.isArray(v)) return v.map(plain);

  return v;
};

async function bytes(url: string): Promise<Uint8Array> {
  const r = await fetch(url);

  if (!r.ok) throw new Error(`circuit artifact ${url}: http ${r.status}`);

  return new Uint8Array(await r.arrayBuffer());
}

export async function openPrivateWallet(o: OpenOptions) {
  const transport = { request: async (req: WorkerRequest) => plain(await o.request(req)) as WorkerResponse };
  const network = createNetwork({ httpsOnly: false, fetch: o.fetch });
  const base = o.circuitsUrl.endsWith("/") ? o.circuitsUrl : `${o.circuitsUrl}/`;
  const artifacts = async (): Promise<CircuitArtifacts> => {
    const [withdrawWasm, withdrawZkey, commitmentWasm, commitmentZkey] = await Promise.all(
      ["withdraw.wasm", "withdraw.zkey", "commitment.wasm", "commitment.zkey"].map((f) => bytes(base + f)),
    );

    return { withdrawWasm: withdrawWasm!, withdrawZkey: withdrawZkey!, commitmentWasm: commitmentWasm!, commitmentZkey: commitmentZkey! };
  };
  // The cold pool scan is user-neutral: it comes from a same-origin snapshot (public pool state
  // at a fixed block) instead of ~3,000 RPC calls; only the increment after it goes over `provider`.
  const initialState = o.snapshotUrl
    ? async () => { const r = await fetch(o.snapshotUrl!); if (!r.ok) throw new Error(`snapshot: http ${r.status}`); return r.json(); }
    : undefined;
  const t = performance.now();
  const w = await LowkiPrivateWallet.open({
    transport, provider: o.provider, network, chainId: o.chainId,
    aspUrl: o.aspUrl, relayerUrl: o.relayerUrl, artifacts, useGpu: o.useGpu,
    ...(initialState ? { initialState } : {}),
  });

  console.info(`[lowki-glue] private wallet opened in ${Math.round(performance.now() - t)} ms`);

  return {
    sync: async () => J({ ms: await w.sync() }),
    balance: async () => J(await w.balance()),
    notes: async () => J(await w.notes()),
    loadCircuits: async () => J(await w.loadCircuits()),
    prepareShield: async (amountWei: string) => J(await w.prepareShield(amountWei)),
    waitApproved: async (precommitment: string, onPoll?: (state: string, ms: number) => void) =>
      J(await w.waitApproved(precommitment, { onPoll })),
    unshield: async (amountWei: string, recipient: string, onStage?: (stage: Stage, fraction: number, detail: string) => void) =>
      J(await w.unshield(amountWei, recipient as Hex, (s, f, d) => onStage?.(s, f, J(d ?? {})))),
  };
}
