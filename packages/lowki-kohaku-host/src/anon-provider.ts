import type { EthereumProvider } from "@kohaku-eth/provider";
import { raw } from "@kohaku-eth/provider/raw";
import type { Provider } from "ox/Provider";

/**
 * Anon-RPC provider mode: Kohaku's `anon` adapter (vendor/kohaku/packages/provider/src/anon),
 * re-done as a thin wrapper over `@anon-rpc/browser-harness` 0.4.0 + Kohaku's `raw` adapter so
 * that every call can carry `anonOptions.isolationKey` (Kohaku's adapter does not forward it) and
 * so that a failed boot falls back to direct RPC *visibly* (`anonymous: false` + `reason`).
 *
 * Browser-only for the real harness (null-origin sandboxed iframe + WebRTC KPS). Anonymized
 * traffic leaves through the worker's own transport, so the host `Network` allowlist does NOT
 * cover it; only the bootstrap read and the direct fallback go through `fetchFn`.
 */

export type AnonNetworkId = "nox" | "nym" | "tor-js";

export type AnonNetworkInfo = {
  /** IWorkerSpecifier contract (anon-rpc adopters.json5). */
  specifier: string;
  /** Chain the specifier contract lives on: the bootstrap RPC must read this chain. */
  specifierChainId: 1 | 11155111;
  defaultConfig?: unknown;
};

/** Temporary tor-js gateway posted by an EF dev (kept up "a day or so"); the adopters.json5 demo
 * gateway was down. Pass `torGateways` to use your own. */
export const TOR_JS_DEFAULT_GATEWAYS = ["159.89.167.167:12298:uEiA-2gShJvf4zebUOQElPY_y46cFeECnyn1TqOfnIQMeTg"];

export const ANON_NETWORKS: Record<AnonNetworkId, AnonNetworkInfo> = {
  nox: { specifier: "0x35133267A8F7EC35046a83DCA0f4Ca8A6e1b7A9A", specifierChainId: 11155111 },
  nym: { specifier: "0x3aCD9742C68F6d993066Ae25533e09b9e1f60eD0", specifierChainId: 1 },
  "tor-js": {
    specifier: "0x700dA3193D35fA54Cd3fBf29B66f2a2A0385659e",
    specifierChainId: 1,
    defaultConfig: { gateways: TOR_JS_DEFAULT_GATEWAYS },
  },
};

/** Default bootstrap RPCs (one non-anonymous `eth_call` pair to read the specifier). */
export const DEFAULT_BOOTSTRAP_URLS: Record<1 | 11155111, string> = {
  1: "https://ethereum-rpc.publicnode.com",
  11155111: "https://sepolia.gateway.tenderly.co",
};

/** Kohaku's anon receipt polling (packages/provider/src/anon/wait.ts): 4 s / 5 min. */
export const ANON_WAIT_FOR_TRANSACTION = { pollIntervalMs: 4_000, timeoutMs: 300_000 } as const;

/**
 * Nox works on desktop Chromium but fails on iPhone Safari; Nym works on Safari/WebKit (slow).
 * Every iOS browser is WebKit, so any iPhone/iPad UA picks Nym.
 */
export function pickAnonNetwork(userAgent: string = globalThis.navigator?.userAgent ?? ""): AnonNetworkId {
  if (/iPhone|iPad|iPod/.test(userAgent)) return "nym";
  const webkit = /AppleWebKit/.test(userAgent);
  const blink = /Chrome\/|Chromium\/|Edg\/|OPR\//.test(userAgent);

  return webkit && !blink ? "nym" : "nox";
}

type RpcProvider = { request(args: { method: string; params?: unknown[] }): Promise<unknown> };

/** Structural subset of `AnonRpcWorker` (injectable for tests). */
export type AnonHarness = {
  ready: Promise<void>;
  fetch(input: string, init?: RequestInit & { anonOptions?: { isolationKey?: string } }): Promise<Response>;
  close(): void;
};
export type AnonHarnessInit = { address: string; config?: unknown; preExisting: { rpcProvider: RpcProvider } };
export type AnonHarnessFactory = (init: AnonHarnessInit) => AnonHarness | Promise<AnonHarness>;

const defaultHarnessFactory: AnonHarnessFactory = async (init) => {
  const { AnonRpcWorker } = await import("@anon-rpc/browser-harness");

  return new AnonRpcWorker(init as ConstructorParameters<typeof AnonRpcWorker>[0]);
};

export type AnonProviderOptions = {
  /** Destination JSON-RPC endpoint, reached through the anon network. */
  rpcUrl: string;
  /** "auto" (default) = `pickAnonNetwork()`. */
  network?: AnonNetworkId | "auto";
  /** tor-js only: KPS gateways (`ip:port:multihash`). Default `TOR_JS_DEFAULT_GATEWAYS`. */
  torGateways?: string[];
  /** Overrides the network's worker config entirely. */
  workerConfig?: unknown;
  /** Default 60 s. The harness has none; a silent worker would hang forever. */
  bootTimeoutMs?: number;
  /** Per-call timeout through the anon network (default 120 s). */
  callTimeoutMs?: number;
  /** Bootstrap RPC per chain (not anonymized; used once to read the specifier). */
  bootstrapUrls?: Partial<Record<1 | 11155111, string>>;
  /** `anonOptions.isolationKey` attached to every call of this provider. */
  isolationKey?: string;
  /**
   * Direct RPC used if the anon network fails to boot. `false` = no fallback: the call rejects
   * with `AnonUnavailableError`. Default: `rpcUrl` fetched directly.
   */
  fallback?: false | { rpcUrl?: string };
  /** Called (in addition to the returned `anonymous: false`) when falling back. */
  onFallback?: (info: { network: AnonNetworkId; reason: string; error: unknown }) => void;
  /** fetch for the bootstrap read and the direct fallback (e.g. the host `Network.fetch`). */
  fetchFn?: (input: string, init?: RequestInit) => Promise<Response>;
  /** Test seam; default constructs `AnonRpcWorker`. */
  harnessFactory?: AnonHarnessFactory;
  waitForTransaction?: { pollIntervalMs?: number; timeoutMs?: number };
};

type Common = {
  provider: EthereumProvider;
  /** The network that was asked for (the one that ran, if `anonymous`). */
  network: AnonNetworkId;
  /** Time spent booting (or failing to boot) the anon worker. */
  bootMs: number;
  isolationKey?: string;
  /** Plain fetch over the same session (HTTP to ASP/relayer APIs); direct when not anonymous. */
  fetch: (url: string, init?: RequestInit & { anonOptions?: { isolationKey?: string } }) => Promise<Response>;
  close(): void;
};
export type AnonProviderResult =
  | (Common & { anonymous: true })
  | (Common & { anonymous: false; reason: string; error: unknown; fallbackRpcUrl: string });

export class AnonUnavailableError extends Error {
  constructor(readonly network: AnonNetworkId, readonly reason: string, override readonly cause: unknown) {
    super(`anon-rpc (${network}) unavailable: ${reason}`);
    this.name = "AnonUnavailableError";
  }
}

type RpcErrorBody = { code?: number; message?: string; data?: unknown };

function jsonRpcClient(
  doFetch: (url: string, init: RequestInit) => Promise<Response>,
  url: string,
  tag: string,
): Provider {
  let id = 0;

  return {
    async request({ method, params }: { method: string; params?: unknown }): Promise<unknown> {
      const res = await doFetch(url, {
        method: "POST",
        headers: { "content-type": "application/json" },
        body: JSON.stringify({ jsonrpc: "2.0", id: ++id, method, params: params ?? [] }),
      });

      if (!res.ok) throw new Error(`${tag}: endpoint returned http ${res.status}`);
      const body = await res.json() as { result?: unknown; error?: RpcErrorBody };

      if (body.error) {
        throw Object.assign(new Error(body.error.message ?? `${tag}: rpc error`), {
          code: body.error.code, data: body.error.data,
        });
      }

      return body.result;
    },
  } as unknown as Provider;
}

function withTimeout<T>(p: Promise<T>, ms: number, what: string, onTimeout?: () => void): Promise<T> {
  let timer: ReturnType<typeof setTimeout> | undefined;
  const t = new Promise<never>((_, reject) => {
    timer = setTimeout(() => { reject(new Error(`${what} timed out after ${ms} ms`)); onTimeout?.(); }, ms);
  });

  return Promise.race([p, t]).finally(() => clearTimeout(timer));
}

const now = () => globalThis.performance?.now() ?? Date.now();

/**
 * Boots one anon-rpc session and returns a Kohaku `EthereumProvider` over it. If the boot fails
 * or times out, returns a direct provider with `anonymous: false` and the reason (never silent),
 * or rejects with `AnonUnavailableError` when `fallback: false`.
 * Calls made after a successful boot never fall back: their errors propagate.
 */
export async function createAnonProvider(opts: AnonProviderOptions): Promise<AnonProviderResult> {
  const network = !opts.network || opts.network === "auto" ? pickAnonNetwork() : opts.network;
  const info = ANON_NETWORKS[network];
  const fetchFn = opts.fetchFn ?? ((u: string, i?: RequestInit) => globalThis.fetch(u, i));
  const wait = { ...ANON_WAIT_FOR_TRANSACTION, ...opts.waitForTransaction };
  const bootTimeoutMs = opts.bootTimeoutMs ?? 60_000;
  const callTimeoutMs = opts.callTimeoutMs ?? 120_000;
  const bootstrapUrl = opts.bootstrapUrls?.[info.specifierChainId] ?? DEFAULT_BOOTSTRAP_URLS[info.specifierChainId];
  const config = opts.workerConfig
    ?? (network === "tor-js" ? { gateways: opts.torGateways ?? TOR_JS_DEFAULT_GATEWAYS } : info.defaultConfig);
  const t0 = now();
  let harness: AnonHarness | undefined;

  try {
    harness = await (opts.harnessFactory ?? defaultHarnessFactory)({
      address: info.specifier,
      ...(config === undefined ? {} : { config }),
      preExisting: { rpcProvider: jsonRpcClient(fetchFn as never, bootstrapUrl, "bootstrap") as unknown as RpcProvider },
    });
    const h = harness;

    await withTimeout(h.ready, bootTimeoutMs, `anon-rpc ${network} boot`, () => h.close());
  } catch (error) {
    harness?.close();
    const bootMs = Math.round(now() - t0);
    const reason = error instanceof Error ? error.message : String(error);

    if (opts.fallback === false) throw new AnonUnavailableError(network, reason, error);
    const fallbackRpcUrl = opts.fallback?.rpcUrl ?? opts.rpcUrl;

    opts.onFallback?.({ network, reason, error });

    return {
      anonymous: false, network, reason, error, fallbackRpcUrl, bootMs, isolationKey: opts.isolationKey,
      provider: raw(jsonRpcClient(fetchFn as never, fallbackRpcUrl, "direct-rpc"), { waitForTransaction: wait }),
      fetch: (u, i) => fetchFn(u, i),
      close: () => {},
    };
  }

  const h = harness;
  const bootMs = Math.round(now() - t0);
  // A caller-supplied `anonOptions` (per call) wins over the session's isolationKey.
  const anonFetch = (url: string, init: RequestInit & { anonOptions?: { isolationKey?: string } } = {}) => {
    const signal = typeof AbortSignal.timeout === "function" ? AbortSignal.timeout(callTimeoutMs) : undefined;

    return h.fetch(url, {
      ...init,
      ...(signal && !init.signal ? { signal } : {}),
      ...(!init.anonOptions && opts.isolationKey ? { anonOptions: { isolationKey: opts.isolationKey } } : {}),
    });
  };

  return {
    anonymous: true, network, bootMs, isolationKey: opts.isolationKey,
    provider: raw(jsonRpcClient(anonFetch, opts.rpcUrl, `anon-rpc ${network}`), { waitForTransaction: wait }),
    fetch: anonFetch,
    close: () => h.close(),
  };
}

export type AnonIsolation =
  /** One anon worker (own network client, own circuits/mixnet identity) per address + isolationKey. */
  | "worker"
  /** One shared worker; addresses separated only by `anonOptions.isolationKey` (network-dependent). */
  | "isolationKey";

/**
 * Per-address sessions, so reading several of a user's app addresses is not done over one
 * shared session. `forAddress` boots lazily and caches one session per (lowercased) address.
 * Each session's result reports `anonymous` on its own; one address may fall back while
 * another is anonymous.
 */
export function createAnonSessionPool(
  opts: Omit<AnonProviderOptions, "isolationKey"> & { isolation?: AnonIsolation },
) {
  const isolation = opts.isolation ?? "worker";
  const sessions = new Map<string, Promise<AnonProviderResult>>();
  let shared: Promise<AnonProviderResult> | undefined;

  async function forAddress(address: string): Promise<AnonProviderResult> {
    const key = address.toLowerCase();
    const hit = sessions.get(key);

    if (hit) return hit;
    let p: Promise<AnonProviderResult>;

    if (isolation === "worker") {
      p = createAnonProvider({ ...opts, isolationKey: key });
    } else {
      shared ??= createAnonProvider(opts);
      p = shared.then((base) => {
        if (!base.anonymous) return { ...base, isolationKey: key };
        // Same harness, per-address isolationKey on every call.
        return createAnonProvider({
          ...opts,
          isolationKey: key,
          harnessFactory: () => ({ ready: Promise.resolve(), fetch: (u, i) => base.fetch(u, i) /* keeps this key's anonOptions */, close: () => {} }),
        }).then((r) => ({ ...r, bootMs: base.bootMs, close: () => {} }));
      });
    }
    sessions.set(key, p);

    return p;
  }

  return {
    isolation,
    forAddress,
    async closeAll() {
      const all = [...sessions.values(), ...(shared ? [shared] : [])];

      sessions.clear();
      shared = undefined;
      for (const s of await Promise.allSettled(all)) if (s.status === "fulfilled") s.value.close();
    },
  };
}

/**
 * A `fetch` over the session, for `createNetwork({ fetch })`: Kohaku's relayer client, paymaster
 * bundler client and ASP services all fetch through `host.network`, so this routes them through
 * the same anon session as the provider (or directly, if the session fell back: check
 * `result.anonymous`). Request bodies must be strings/bytes (the harness rejects streams).
 */
export function anonFetchFor(result: AnonProviderResult): typeof fetch {
  return async (input, init) => {
    if (input instanceof Request) {
      const body = init?.body ?? (input.method === "GET" || input.method === "HEAD" ? undefined : await input.arrayBuffer());

      return result.fetch(input.url, { method: input.method, headers: input.headers, ...init, ...(body ? { body } : {}) });
    }

    return result.fetch(input instanceof URL ? input.href : input, init);
  };
}
