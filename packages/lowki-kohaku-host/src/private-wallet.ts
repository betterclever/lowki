// Privacy Pools v1 flows for the Lowki wallet UI: shield (Move in), wait for the ASP, private
// withdraw through a relayer (Send privately / Move out). It is the browser counterpart of
// scripts/fork-private-send.ts, with the same pieces:
//
//   keys + signing   the Lowki worker (any `WorkerTransport`; in the wallet UI, the Rust
//                    WorkerClient's worker, bridged by browser-wallet/entry.ts)
//   sync             Kohaku PrivacyPoolsV1Protocol on the Lowki Host (keystore mode 2)
//   prove            lowki-worker ppProveWithdraw (WebGPU in the browser) via src/prover.ts
//   ASP              an HTTP IAspService: GET {aspUrl}/leaves/{root} -> { leaves: string[] }
//   submit           Kohaku's own RelayerClient + PrivacyPoolsBroadcaster over `network`
//
// The provider and `network` decide where traffic goes: pass the anon-rpc provider and
// `anonFetchFor(result)` to route reads, ASP fetches and the relayer through Anon-RPC.
// Shield transactions are returned unsigned: the caller signs them in the worker
// (`signTransaction`) and broadcasts them over the same provider.
import type { ERC20AssetId, AccountId, Network } from "@kohaku-eth/plugins";
import type { EthereumProvider } from "@kohaku-eth/provider";
import {
  E_ADDRESS, PrivacyPoolsBroadcaster, PrivacyPoolsV1Protocol, PrivacyPoolsV1_0xBow, lastUpdateRootEventSelector,
  type IAspService, type PPv1RelayerPrivateOperation,
} from "./kohaku-pp";
import { createLowkiHost } from "./host";
import { EncryptedStorage, MemoryBackend, randomStorageKey } from "./storage";
import { call, type WorkerTransport } from "./transport";
import { lowkiProverFactory } from "./prover";
import type { Hex } from "./protocol";

export type CircuitArtifacts = {
  withdrawWasm: Uint8Array; withdrawZkey: Uint8Array; commitmentWasm: Uint8Array; commitmentZkey: Uint8Array;
};

export type PrivateWalletOptions = {
  transport: WorkerTransport;
  provider: EthereumProvider;
  network: Network;
  chainId: number;
  /** Default: Kohaku's 0xbow config for `chainId`. */
  entrypoint?: { address: Hex; deploymentBlock: bigint };
  /** HTTP ASP: GET `${aspUrl}/leaves/${rootDecimal}` -> `{ leaves: string[] }`. */
  aspUrl: string;
  /** Privacy Pools relayer API base (Kohaku RelayerClient appends /quote, /request, /details). */
  relayerUrl: string;
  /** Kohaku state snapshot to start the sync from (public pool data only). */
  initialState?: () => Promise<unknown>;
  /** Circuit artifacts for `ppLoadCircuits` (sha256-pinned in the worker). */
  artifacts: () => Promise<CircuitArtifacts>;
  useGpu?: boolean;
  accountIndex?: number;
};

export type Stage = "sync" | "prove" | "submit" | "deposit" | "approve";
export type StageFn = (stage: Stage, fraction: number, detail?: Record<string, unknown>) => void;

export type ProofInfo = { circuit: string; backend: string; ms: number; depositIndex: number; secretIndex: number };

export type UnshieldResult = {
  txHash: Hex;
  recipient: Hex;
  gross: string;
  fee: string;
  net: string;
  relayFeeBps: string;
  nullifierHash: string;
  proof?: ProofInfo;
  timings: { syncMs: number; proveMs: number; submitMs: number };
};

const ETH: ERC20AssetId = { __type: "erc20", contract: E_ADDRESS as Hex };
const RELAYER_ID = "lowki";
const now = () => globalThis.performance?.now() ?? Date.now();
const toJson = (v: unknown) => JSON.parse(JSON.stringify(v, (_k, x) => (typeof x === "bigint" ? x.toString() : x)));

/** ASP leaves over HTTP (through `network`, so over Anon-RPC when that is the network's fetch). */
export function httpAspService(network: Network, aspUrl: string): IAspService {
  return {
    async getAspTree(state) {
      const last = lastUpdateRootEventSelector(state);

      if (!last) throw new Error("ASP: no RootUpdated event yet");
      const res = await network.fetch(`${aspUrl}/leaves/${BigInt(last.root).toString(10)}`);

      if (!res.ok) throw new Error(`ASP: no leaves for root ${last.root} (http ${res.status})`);
      const { leaves } = await res.json() as { leaves: string[] };

      return [leaves.map((l) => BigInt(l))];
    },
  };
}

export class LowkiPrivateWallet {
  private circuits?: Promise<{ gpu: boolean; witnessBackend: string; ms: number; gpuError?: string }>;
  private lastProof?: ProofInfo;

  private constructor(
    private readonly opts: PrivateWalletOptions,
    readonly protocol: PrivacyPoolsV1Protocol,
    private readonly broadcaster: PrivacyPoolsBroadcaster,
    readonly entrypoint: Hex,
  ) {}

  static async open(opts: PrivateWalletOptions): Promise<LowkiPrivateWallet> {
    const cfg = PrivacyPoolsV1_0xBow[opts.chainId as keyof typeof PrivacyPoolsV1_0xBow];
    const entrypoint = opts.entrypoint
      ?? { address: cfg!.entrypoint.entrypointAddress as Hex, deploymentBlock: BigInt(cfg!.entrypoint.deploymentBlock) };
    // Kohaku stores only public pool state and history metadata; the key is per session
    // (the worker has no storage-key request yet), so each session syncs from the snapshot.
    const storage = new EncryptedStorage(new MemoryBackend(), await randomStorageKey());
    const lowki = createLowkiHost({
      transport: opts.transport, storage, network: opts.network, provider: opts.provider,
      wiring: "envelopeSigner", verifyDeterminism: false,
    });
    const relayersList = { [RELAYER_ID]: opts.relayerUrl };
    const initial = opts.initialState ? await opts.initialState() : undefined;
    let self: LowkiPrivateWallet | undefined;
    const protocol = new PrivacyPoolsV1Protocol(lowki.host, {
      accountIndex: opts.accountIndex ?? 0,
      entrypoint: { address: BigInt(entrypoint.address), deploymentBlock: entrypoint.deploymentBlock },
      secretManager: lowki.secretManager,
      ...(initial ? { initialState: async () => initial as never } : {}),
      aspServiceFactory: () => httpAspService(opts.network, opts.aspUrl),
      relayersList,
      proverFactory: lowkiProverFactory(opts.transport, {
        chainId: opts.chainId, entrypoint: entrypoint.address, accountIndex: opts.accountIndex ?? 0,
        onProof: ({ circuit, backend, ms, note }) => {
          if (self) self.lastProof = { circuit, backend, ms, depositIndex: note.depositIndex, secretIndex: note.secretIndex };
        },
      }),
    });
    const broadcaster = new PrivacyPoolsBroadcaster({ host: lowki.host, broadcasterUrl: relayersList });

    self = new LowkiPrivateWallet(opts, protocol, broadcaster, entrypoint.address);

    return self;
  }

  async sync(): Promise<number> {
    const t = now();

    await this.protocol.sync();

    return Math.round(now() - t);
  }

  /** ETH in the pool: `approved` can be sent privately; `pending` waits for the ASP. */
  async balance({ sync = true } = {}): Promise<{ approved: string; pending: string }> {
    const b = await this.protocol.balance([ETH], { sync });

    return {
      approved: String(b.find((x) => !x.tag)?.amount ?? 0n),
      pending: String(b.find((x) => x.tag === "pending")?.amount ?? 0n),
    };
  }

  async notes({ sync = true } = {}) {
    return toJson(await this.protocol.notes([ETH], false, { sync }));
  }

  /** Kohaku's deposit transaction, cross-checked byte for byte against the worker's ppShield. */
  async prepareShield(amountWei: bigint | string): Promise<{ to: Hex; data: Hex; value: string; precommitment: string; depositIndex: number }> {
    const amount = BigInt(amountWei);
    const k = (await this.protocol.prepareShield({ asset: ETH, amount })).txns[0] as { to: Hex; data: Hex; value: bigint };

    for (let depositIndex = 0; depositIndex < 256; depositIndex++) {
      const w = await call(this.opts.transport, {
        type: "ppShield",
        note: { accountIndex: this.opts.accountIndex ?? 0, chainId: String(this.opts.chainId), entrypoint: this.entrypoint, depositIndex, secretIndex: 0 },
        value: String(amount),
      });

      if (w.tx.data.toLowerCase() === k.data.toLowerCase()) {
        if (w.tx.to.toLowerCase() !== k.to.toLowerCase() || BigInt(w.tx.value) !== BigInt(k.value)) {
          throw new Error("worker ppShield differs from Kohaku prepareShield");
        }

        return { to: w.tx.to, data: w.tx.data, value: String(amount), precommitment: w.precommitment, depositIndex };
      }
    }
    throw new Error("worker ppShield: no deposit index reproduces Kohaku's deposit");
  }

  /** Syncs until the note with `precommitment` is in the pool and approved by the ASP. */
  async waitApproved(precommitment: string, { timeoutMs = 180_000, pollMs = 2_000, onPoll }: {
    timeoutMs?: number; pollMs?: number; onPoll?: (state: "missing" | "pending" | "approved", elapsedMs: number) => void;
  } = {}): Promise<{ label: string; balance: string; ms: number }> {
    const t = now();
    const want = BigInt(precommitment);

    for (;;) {
      const notes = await this.protocol.notes([ETH], false);
      const n = notes.find((x) => BigInt(x.precommitment) === want);
      const state = !n ? "missing" : n.approved ? "approved" : "pending";

      onPoll?.(state, Math.round(now() - t));
      if (n && n.approved) return { label: String(n.label), balance: String(n.balance), ms: Math.round(now() - t) };
      if (now() - t > timeoutMs) throw new Error(`ASP did not approve the deposit within ${timeoutMs / 1000} s (${state})`);
      await new Promise((r) => setTimeout(r, pollMs));
    }
  }

  /** Loads the circuits into the worker once (WebGPU when `useGpu` and available). */
  loadCircuits() {
    this.circuits ??= (async () => {
      const t = now();
      const a = await this.opts.artifacts();
      const r = await call(this.opts.transport, { type: "ppLoadCircuits", ...a, useGpu: this.opts.useGpu ?? true });

      return { gpu: r.gpu, witnessBackend: r.witnessBackend, ms: Math.round(now() - t), ...(r.gpuError ? { gpuError: r.gpuError } : {}) };
    })();
    this.circuits.catch(() => { this.circuits = undefined; });

    return this.circuits;
  }

  /**
   * Private withdraw of `amountWei` (gross, from approved notes) to `recipient` through the
   * relayer. Stages: sync (pool + ASP), prove (quote, Merkle proofs, worker proof), submit
   * (relayer sends Entrypoint.relay, then the receipt is read back over the provider).
   */
  async unshield(amountWei: bigint | string, recipient: Hex, onStage: StageFn = () => {}): Promise<UnshieldResult> {
    const amount = BigInt(amountWei);
    const circuits = this.loadCircuits();

    onStage("sync", 0);
    let t = now();

    await this.protocol.sync();
    const { approved } = await this.balance({ sync: false });

    if (BigInt(approved) < amount) throw new Error(`insufficient private balance: ${approved} < ${amount}`);
    const syncMs = Math.round(now() - t);

    onStage("sync", 1, { ms: syncMs });

    onStage("prove", 0, { circuits: await circuits });
    t = now();
    this.lastProof = undefined;
    const op = await this.protocol.prepareUnshield({ asset: ETH, amount }, recipient as AccountId);

    if (op.mode !== "relayer") throw new Error(`expected a relayer operation, got ${op.mode}`);
    const rop = op as unknown as PPv1RelayerPrivateOperation;
    const proveMs = Math.round(now() - t);

    onStage("prove", 1, { ms: proveMs, proof: this.lastProof });

    onStage("submit", 0);
    t = now();
    const sent = await this.broadcaster.broadcast(op);
    const receipt = await this.opts.provider.request({ method: "eth_getTransactionReceipt", params: [sent.txHash] } as never) as { status?: string } | null;

    if (!receipt || receipt.status !== "0x1") throw new Error(`relay transaction ${sent.txHash} failed (${receipt?.status ?? "no receipt"})`);
    const submitMs = Math.round(now() - t);

    onStage("submit", 1, { ms: submitMs, txHash: sent.txHash });
    const feeBps = BigInt(rop.rawData.relayData.relayFeeBps);
    const fee = amount * feeBps / 10_000n;

    return {
      txHash: sent.txHash, recipient, gross: String(amount), fee: String(fee), net: String(amount - fee),
      relayFeeBps: String(feeBps), nullifierHash: String(rop.rawData.proof.mappedSignals.existingNullifierHash),
      ...(this.lastProof ? { proof: this.lastProof } : {}),
      timings: { syncMs, proveMs, submitMs },
    };
  }
}
