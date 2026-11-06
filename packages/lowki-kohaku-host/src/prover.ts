// Kohaku `proverFactory` backed by the Lowki worker's Groth16 prover (lowki-groth16: WebGPU in the
// browser, CPU wasm elsewhere) instead of snarkjs.
//
// Kohaku's withdraw/ragequit thunks hand the prover a full witness (note secrets included,
// computed in JS by Kohaku's SignatureSecretManager). The worker's `ppProveWithdraw` /
// `ppProveCommitment` take a note *reference* and re-derive the secrets in Rust. This adapter
// maps one onto the other:
//   1. it finds the note reference whose precommitment `poseidon(nullifier, salt)` matches the
//      witness, by asking the worker for `ppNote` hashes (public) over (depositIndex, secretIndex);
//   2. it calls the worker with the reference, the label/values/context and Kohaku's Merkle proofs;
//   3. it checks that the worker's public signals are exactly the ones Kohaku's witness implies
//      (same value, roots, context, nullifier hash and change commitment), so a divergence between
//      the Rust and the Kohaku secret derivations fails here, not on chain.
// What this buys is speed (WebGPU) and one prover implementation; it does NOT keep secrets out of
// JS, because Kohaku already derived them (see README "Proving").
import { CommitmentSignalMapper, WithdrawSignalMapper } from "@fatsolutions/privacy-pools-core-circuits";
import { poseidon } from "maci-crypto/build/ts/hashing.js";
import type { Hex, PpNoteRef, ProofResponse } from "./protocol";
import { call, type WorkerTransport } from "./transport";

export type LowkiProverOptions = {
  accountIndex?: number;
  chainId: bigint | number;
  entrypoint: Hex;
  /** Search bounds for the note reference (default 256 deposits x 64 withdrawals per deposit). */
  maxDepositIndex?: number;
  maxSecretIndex?: number;
  /** Called after every proof (backend, time, which note). */
  onProof?: (info: { circuit: string; backend: string; ms: number; note: PpNoteRef }) => void;
};

type WithdrawInputs = {
  withdrawnValue: bigint; stateRoot: bigint; ASPRoot: bigint; context: bigint; label: bigint;
  existingValue: bigint; existingNullifier: bigint; existingSecret: bigint; newNullifier: bigint; newSecret: bigint;
  stateSiblings: bigint[]; stateIndex: bigint; ASPSiblings: bigint[]; ASPIndex: bigint;
};
type CommitmentInputs = { value: bigint; label: bigint; nullifier: bigint; secret: bigint };

const pad32 = (s: bigint[]) => {
  if (s.length > 32) throw new Error(`Merkle proof deeper than 32 (${s.length})`);

  return [...s, ...Array<bigint>(32 - s.length).fill(0n)].map(String);
};

// serde-wasm-bindgen may hand back nested Maps.
const plain = (v: unknown): unknown => {
  if (v instanceof Map) return Object.fromEntries([...v].map(([k, x]) => [k, plain(x)]));
  if (Array.isArray(v)) return v.map(plain);

  return v;
};

/** Returns a factory to pass as `PrivacyPoolsV1Protocol({ proverFactory })`. */
export function lowkiProverFactory(transport: WorkerTransport, opts: LowkiProverOptions) {
  const accountIndex = opts.accountIndex ?? 0;
  const chainId = String(opts.chainId);
  const entrypoint = opts.entrypoint;
  const maxD = opts.maxDepositIndex ?? 256;
  const maxS = opts.maxSecretIndex ?? 64;
  const known = new Map<string, PpNoteRef>(); // precommitment -> ref

  const ref = (depositIndex: number, secretIndex: number): PpNoteRef =>
    ({ accountIndex, chainId, entrypoint, depositIndex, secretIndex });

  async function findNote(nullifier: bigint, salt: bigint): Promise<PpNoteRef> {
    const pre = String(poseidon([nullifier, salt]));
    const hit = known.get(pre);

    if (hit) return hit;

    // Diagonal walk: small (depositIndex + secretIndex) first.
    for (let sum = 0; sum <= maxD + maxS; sum++) {
      for (let s = Math.max(0, sum - maxD); s <= Math.min(sum, maxS); s++) {
        const r = ref(sum - s, s);
        const { precommitment } = await call(transport, { type: "ppNote", note: r });

        known.set(precommitment, r);
        if (precommitment === pre) return r;
      }
    }

    throw new Error("lowki prover: no worker note matches this witness (different seed, chain or entrypoint?)");
  }

  const prove = async (circuit: "withdraw" | "commitment", inputs: WithdrawInputs | CommitmentInputs) => {
    let out: ProofResponse;
    let note: PpNoteRef;

    if (circuit === "withdraw") {
      const w = inputs as WithdrawInputs;

      note = await findNote(w.existingNullifier, w.existingSecret);
      out = plain(await call(transport, {
        type: "ppProveWithdraw",
        note,
        label: String(w.label),
        existingValue: String(w.existingValue),
        withdrawnValue: String(w.withdrawnValue),
        context: { context: String(w.context) },
        stateProof: { root: String(w.stateRoot), index: String(w.stateIndex), siblings: pad32(w.stateSiblings) },
        aspProof: { root: String(w.ASPRoot), index: String(w.ASPIndex), siblings: pad32(w.ASPSiblings) },
      })) as ProofResponse;
      const m = WithdrawSignalMapper(out.publicSignals);
      const newCommitment = poseidon([w.existingValue - w.withdrawnValue, w.label, poseidon([w.newNullifier, w.newSecret])]);
      const want = {
        newCommitmentHash: newCommitment, existingNullifierHash: poseidon([w.existingNullifier]),
        withdrawnValue: w.withdrawnValue, stateRoot: w.stateRoot, ASPRoot: w.ASPRoot, context: w.context,
      };

      for (const [k, v] of Object.entries(want)) {
        if ((m as Record<string, bigint>)[k] !== v) {
          throw new Error(`lowki prover: public signal ${k} differs from Kohaku's witness (worker ${(m as Record<string, bigint>)[k]}, Kohaku ${v})`);
        }
      }
    } else {
      const c = inputs as CommitmentInputs;

      note = await findNote(c.nullifier, c.secret);
      out = plain(await call(transport, {
        type: "ppProveCommitment", note, value: String(c.value), label: String(c.label),
      })) as ProofResponse;
      const m = CommitmentSignalMapper(out.publicSignals);

      if (m.value !== c.value || m.label !== c.label || m.nullifierHash !== poseidon([c.nullifier])) {
        throw new Error("lowki prover: commitment public signals differ from Kohaku's witness");
      }
    }

    opts.onProof?.({ circuit, backend: out.backend, ms: out.ms, note });
    const mappedSignals = circuit === "withdraw" ? WithdrawSignalMapper(out.publicSignals) : CommitmentSignalMapper(out.publicSignals);

    return { proof: out.proof, publicSignals: out.publicSignals, mappedSignals };
  };

  // Kohaku types the factory as `typeof Prover` (snarkjs); only `prove` is used by the thunks.
  return (() => Promise.resolve({ circuits: undefined, prove })) as never;
}
