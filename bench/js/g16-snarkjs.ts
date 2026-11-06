// snarkjs 0.7.5 groth16: witness (wtns.calculate) and fullProve (= wtns.calculate +
// groth16.prove, exactly what groth16.fullProve does), timed separately; every proof's
// public signals are checked against the fixture and the last proof is snarkjs-verified.
import * as snarkjs from "snarkjs";
import { stats, type Stat } from "./harness.ts";

export interface Artifacts {
  wasm: Uint8Array;
  zkey: Uint8Array;
  vkey: any;
}

function unstringify(o: any): any {
  if (typeof o === "string" && /^[0-9]+$|^0x[0-9a-fA-F]+$/.test(o)) return BigInt(o);
  if (Array.isArray(o)) return o.map(unstringify);
  if (o && typeof o === "object") return Object.fromEntries(Object.entries(o).map(([k, v]) => [k, unstringify(v)]));
  return o;
}

export async function benchSnarkjs(
  id: string, art: Artifacts, input: any, expected: string[], singleThread: boolean, iters: number, log: (s: string) => void,
): Promise<Stat> {
  const opts = singleThread ? { singleThread: true } : undefined;
  const witness = async () => {
    const w: any = { type: "mem" };
    await snarkjs.wtns.calculate(unstringify(input), art.wasm, w);
    return w;
  };
  const full = async () => {
    const w = await witness();
    return snarkjs.groth16.prove(art.zkey, w, undefined, opts);
  };
  const time = async (f: () => Promise<unknown>) => {
    for (let i = 0; i < 2; i++) await f();
    const s: number[] = [];
    for (let i = 0; i < iters; i++) {
      const t = performance.now();
      await f();
      s.push(performance.now() - t);
    }
    return s;
  };
  const ws = await time(witness);
  const fs = await time(full);
  const last: any = await full();
  const sigOk = JSON.stringify(last.publicSignals) === JSON.stringify(expected);
  const verified = await snarkjs.groth16.verify(art.vkey, last.publicSignals, last.proof);
  const st: Stat = { id, batch: 1, n: fs.length, warmup: 2, ...stats(fs, 1), check: last.publicSignals.join(","), extra: { witness: stats(ws, 1), verified_snarkjs: verified, signals_match_fixture: sigOk } };
  log(`${id}: full median ${st.median_ms.toFixed(1)} ms p90 ${st.p90_ms.toFixed(1)}, witness ${(st.extra!.witness as any).median_ms.toFixed(1)} ms, verified=${verified}, signals=${sigOk}`);
  return st;
}

export async function verifyWithSnarkjs(vkey: any, proofJson: string): Promise<boolean> {
  const p = JSON.parse(proofJson);
  return snarkjs.groth16.verify(vkey, p.publicSignals, p.proof);
}

export function snarkjsCurveTerminate() {
  // Let the process exit in Node (ffjavascript keeps its worker threads alive otherwise).
  const c = (globalThis as any).curve_bn128;
  if (c) return c.terminate();
}
