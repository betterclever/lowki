// Thin browser glue between the Rust wallet UI (apps/wallet-ui) and this package. Exposes
// `globalThis.lowkiGlue`; Rust calls it from src/backend/real/chain.rs and private.rs.
//
//   connect(opts)        reads lowki-config.json (if served), boots the anon-rpc session
//                        (Nox on Chromium, Nym on Safari/WebKit) unless `mode: "direct"`, and
//                        resolves to the connection status (never silent about a fallback)
//   status()             the same status, synchronously
//   rpc(method, params)  one JSON-RPC call over that connection (params/result as JSON text)
//   openPrivate(request) Privacy Pools flows (src/private-wallet.ts) on the Rust worker client;
//                        loaded lazily (Kohaku + circuits code is a separate chunk)
//
// Logic lives in src/anon-provider.ts and src/private-wallet.ts; nothing here holds keys.
import type { EthereumProvider } from "@kohaku-eth/provider";
import { anonFetchFor, createAnonProvider, pickAnonNetwork, type AnonProviderResult } from "../src/anon-provider";
import { createNetwork } from "../src/network";
import { createRpcProvider } from "../src/provider";

export type GlueConfig = {
  chainId: number;
  network: string;
  /** Destination RPC. With anon on, the anon network's exits call it, so it must be public. */
  rpcUrl?: string | null;
  /** RPC the browser calls itself (direct mode or fallback). Default `rpcUrl`. */
  directRpcUrl?: string;
  /** Why `rpcUrl` is missing (shown as the reason for a direct connection). */
  anonUnavailable?: string;
  /** Echo of the caller's IP, reached the same way as the RPC (egress check). */
  ipEchoUrl?: string | null;
  private?: {
    relayerUrl?: string | null; aspUrl?: string | null;
    directRelayerUrl?: string; directAspUrl?: string;
    snapshotUrl?: string; circuitsUrl?: string; useGpu?: boolean;
  };
};

export type ConnectOptions = {
  /** "anon" (default) or "direct" (the `?rpc=direct` dev override). */
  mode?: "anon" | "direct";
  /** Same-origin config file; missing = `defaults`. */
  configUrl?: string;
  defaults: GlueConfig;
  bootTimeoutMs?: number;
};

export type GlueStatus = {
  state: "connecting" | "ready";
  anonymous: boolean;
  /** anon network that carries the traffic (or was tried). */
  anonNetwork?: string;
  /** Why the connection is direct. */
  reason?: string;
  bootMs?: number;
  chainId: number;
  network: string;
  rpcHost?: string;
  /** IP the RPC host saw (egress check through the same path as the reads). */
  egressIp?: string;
  calls: { anon: number; direct: number };
};

type Conn = { provider: EthereumProvider; fetch: typeof fetch; anon?: AnonProviderResult; cfg: GlueConfig; mode: "anon" | "direct" };

let status: GlueStatus = { state: "connecting", anonymous: false, chainId: 0, network: "", calls: { anon: 0, direct: 0 } };
let conn: Promise<Conn> | undefined;

const host = (u?: string | null) => { try { return u ? new URL(u).host : undefined; } catch { return undefined; } };
const log = (...a: unknown[]) => console.info("[lowki-glue]", ...a);

async function loadConfig(o: ConnectOptions): Promise<GlueConfig> {
  if (!o.configUrl) return o.defaults;
  try {
    const r = await fetch(o.configUrl, { cache: "no-store" });

    if (!r.ok) return o.defaults;

    return { ...o.defaults, ...(await r.json() as Partial<GlueConfig>) };
  } catch {
    return o.defaults;
  }
}

async function open(o: ConnectOptions): Promise<Conn> {
  const cfg = await loadConfig(o);
  const directUrl = cfg.directRpcUrl ?? cfg.rpcUrl ?? "";
  const directNet = createNetwork({ httpsOnly: false });
  const direct = (reason: string, anonNetwork?: string, bootMs?: number): Conn => {
    status = {
      ...status, state: "ready", anonymous: false, reason, chainId: cfg.chainId, network: cfg.network,
      rpcHost: host(directUrl), ...(anonNetwork ? { anonNetwork } : {}), ...(bootMs !== undefined ? { bootMs } : {}),
    };
    log(`direct connection to ${host(directUrl)}: ${reason}`);

    return { provider: createRpcProvider(directUrl, directNet, { retryCount: 1 }), fetch: globalThis.fetch.bind(globalThis), cfg, mode: "direct" };
  };

  status = { ...status, chainId: cfg.chainId, network: cfg.network };
  if (o.mode === "direct") return direct("Direct mode was requested (rpc=direct).");
  if (!cfg.rpcUrl) return direct(cfg.anonUnavailable ?? "No public RPC is configured for the anon network.");
  const anonNetwork = pickAnonNetwork();

  status = { ...status, anonNetwork, rpcHost: host(cfg.rpcUrl) };
  const r = await createAnonProvider({
    rpcUrl: cfg.rpcUrl,
    network: anonNetwork,
    bootTimeoutMs: o.bootTimeoutMs ?? 60_000,
    fallback: { rpcUrl: directUrl },
  });

  if (!r.anonymous) return direct(`The ${anonNetwork} network did not start: ${r.reason}`, anonNetwork, r.bootMs);
  status = { ...status, state: "ready", anonymous: true, bootMs: r.bootMs };
  delete status.reason;
  log(`anon-rpc ${anonNetwork} ready in ${r.bootMs} ms; destination ${host(cfg.rpcUrl)}`);
  const c: Conn = { provider: r.provider, fetch: anonFetchFor(r), anon: r, cfg, mode: "anon" };

  // Egress check: ask the RPC's host which IP it sees, over the same session.
  if (cfg.ipEchoUrl) {
    c.fetch(cfg.ipEchoUrl).then((x) => x.json()).then((j: { ip?: string }) => {
      if (j.ip) { status = { ...status, egressIp: j.ip }; log(`egress IP seen by ${host(cfg.ipEchoUrl)}: ${j.ip}`); }
    }).catch((e) => log("egress check failed", e));
  }

  return c;
}

export function connect(o: ConnectOptions): Promise<GlueStatus> {
  conn ??= open(o);

  return conn.then(() => ({ ...status }));
}

export function getStatus(): GlueStatus {
  return { ...status, calls: { ...status.calls } };
}

function ready(): Promise<Conn> {
  if (!conn) throw new Error("lowkiGlue.connect() was not called");

  return conn;
}

export async function rpc(method: string, paramsJson: string): Promise<string> {
  const c = await ready();
  const result = await c.provider.request({ method, params: JSON.parse(paramsJson) } as never);

  status.calls[c.mode === "anon" ? "anon" : "direct"]++;

  return JSON.stringify(result ?? null);
}

type RequestFn = (req: unknown) => Promise<unknown>;

/** Opens the Privacy Pools session for the unlocked wallet behind `request` (the Rust worker client). */
export async function openPrivate(request: RequestFn) {
  const c = await ready();
  const p = c.cfg.private ?? {};
  const anon = c.mode === "anon";
  const relayerUrl = (anon ? p.relayerUrl : p.directRelayerUrl ?? p.relayerUrl) ?? undefined;
  const aspUrl = (anon ? p.aspUrl : p.directAspUrl ?? p.aspUrl) ?? undefined;

  if (!relayerUrl || !aspUrl) throw new Error("private send is not configured (no relayer or ASP URL in lowki-config.json)");
  const { openPrivateWallet } = await import("./private");

  return openPrivateWallet({
    request,
    provider: c.provider,
    fetch: c.fetch,
    chainId: c.cfg.chainId,
    relayerUrl,
    aspUrl,
    snapshotUrl: p.snapshotUrl,
    circuitsUrl: p.circuitsUrl ?? "circuits/",
    useGpu: p.useGpu ?? true,
  });
}

const glue = { connect, status: getStatus, rpc, openPrivate };

declare global { var lowkiGlue: typeof glue | undefined }
globalThis.lowkiGlue = glue;
globalThis.dispatchEvent?.(new Event("lowki-glue-ready"));
