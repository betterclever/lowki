// Node entry point: node dist/node-bench.mjs <micro|g16-mt|g16-st|verify> <out.json> [proofs.json...]
import { readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";
import { availableParallelism, cpus } from "node:os";
import { runCase, type Stat } from "./harness.ts";
import { tsCases } from "./ts-ops.ts";
import { benchSnarkjs, snarkjsCurveTerminate, verifyWithSnarkjs } from "./g16-snarkjs.ts";

const HERE = dirname(fileURLToPath(import.meta.url));
const ROOT = join(HERE, "..");
const ART = process.env.LOWKI_PP_ARTIFACTS ?? join(ROOT, "../../vendor/npm/circuits/package/artifacts");
const fx = (n: string) => JSON.parse(readFileSync(join(ROOT, "fixtures", `${n}.json`), "utf8"));
const [mode, out, ...rest] = process.argv.slice(2);
const log = (s: string) => console.error(`[node ${mode}] ${s}`);
const iters = Number(process.env.G16_ITERS ?? 10);

const env = { node: process.version, cpu: cpus()[0]?.model, parallelism: availableParallelism() };
const results: Stat[] = [];
if (mode === "micro") {
  const F = { keys: fx("keys"), pp: fx("pp"), stealth: fx("stealth"), aa: fx("aa") };
  for (const c of tsCases(F, { parallel: true, workerUrl: pathToFileURL(join(HERE, "limt-worker.mjs")) })) results.push(await runCase(c, log));
} else if (mode === "g16-mt" || mode === "g16-st") {
  const g16 = fx("g16");
  for (const name of ["commitment", "withdraw"]) {
    const art = {
      wasm: new Uint8Array(readFileSync(join(ART, `${name}.wasm`))),
      zkey: new Uint8Array(readFileSync(join(ART, `${name}.zkey`))),
      vkey: JSON.parse(readFileSync(join(ART, `${name}.vkey`), "utf8")),
    };
    const st = mode === "g16-st";
    results.push(await benchSnarkjs(`g16.${name}.snarkjs_${st ? "1t" : "mt"}`, art, g16[name].input, g16[name].publicSignals, st, iters, log));
  }
} else if (mode === "verify") {
  // snarkjs-verify proofs written by the native Rust binary.
  const vk = (c: string) => JSON.parse(readFileSync(join(ART, `${c}.vkey`), "utf8"));
  const g16 = fx("g16");
  const rows: any[] = [];
  for (const f of rest) {
    for (const p of JSON.parse(readFileSync(f, "utf8"))) {
      const ok = await verifyWithSnarkjs(vk(p.circuit), JSON.stringify(p.out));
      const sig = JSON.stringify(p.out.publicSignals) === JSON.stringify(g16[p.circuit].publicSignals);
      rows.push({ id: p.id, verified_snarkjs: ok, signals_match_fixture: sig });
      log(`${p.id}: snarkjs verify=${ok} signals=${sig}`);
    }
  }
  writeFileSync(out, JSON.stringify(rows, null, 1));
  await snarkjsCurveTerminate();
  process.exit(0);
}
writeFileSync(out, JSON.stringify({ suite: `node-${mode}`, env, results }, null, 1));
await snarkjsCurveTerminate();
process.exit(0);
