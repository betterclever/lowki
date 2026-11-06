// Chrome entry point: ?suite=ts-micro | wasm-micro | g16-snarkjs-mt | g16-snarkjs-st | g16-wasm-cpu | g16-wasm-gpu
// Results are POSTed to the runner (run-browser.mjs).
import { runCase, stats, type Stat } from "./harness.ts";
import { tsCases } from "./ts-ops.ts";
import { benchSnarkjs, verifyWithSnarkjs } from "./g16-snarkjs.ts";

const suite = new URLSearchParams(location.search).get("suite")!;
const G16_ITERS = Number(new URLSearchParams(location.search).get("iters") ?? 10);
const log = (s: string) => {
  console.log(s);
  fetch("/log", { method: "POST", body: `[${suite}] ${s}` });
};
const getJson = (p: string) => fetch(p).then((r) => r.json());
const getBytes = async (p: string) => new Uint8Array(await (await fetch(p)).arrayBuffer());
const fixtures = async (names: string[]) => Object.fromEntries(await Promise.all(names.map(async (n) => [n, await getJson(`/fixtures/${n}.json`)])));
const artifacts = async (name: string) => ({ wasm: await getBytes(`/artifacts/${name}.wasm`), zkey: await getBytes(`/artifacts/${name}.zkey`), vkey: await getJson(`/artifacts/${name}.vkey`) });

async function gpuInfo() {
  const gpu = (navigator as any).gpu;
  if (!gpu) return "no navigator.gpu";
  const a = await gpu.requestAdapter({ powerPreference: "high-performance" });
  return a ? `${a.info.vendor} / ${a.info.architecture} ${a.info.description ?? ""}` : "no adapter";
}

async function timeN(f: () => unknown, iters: number, warm = 2) {
  for (let i = 0; i < warm; i++) await f();
  const s: number[] = [];
  for (let i = 0; i < iters; i++) {
    const t = performance.now();
    await f();
    s.push(performance.now() - t);
  }
  return s;
}

async function main() {
  const results: Stat[] = [];
  const env = { ua: navigator.userAgent, cores: navigator.hardwareConcurrency, crossOriginIsolated, gpu: await gpuInfo() };
  log(JSON.stringify(env));

  if (suite === "ts-micro") {
    const F = await fixtures(["keys", "pp", "stealth", "aa"]);
    for (const c of tsCases(F, { parallel: true, workerUrl: new URL("./limt-worker.js", location.href) })) results.push(await runCase(c, log));
  } else if (suite === "wasm-micro") {
    const F = await fixtures(["keys", "pp", "stealth", "aa"]);
    const m: any = await import("./pkg/lowki_bench.js");
    await m.default();
    const mb = new m.MicroBench(JSON.stringify(F));
    for (let i = 0; i < mb.len(); i++) {
      const iters = mb.iters(i) || undefined;
      results.push(await runCase({ id: mb.id(i), batch: mb.batch(i), iters, warmup: iters ? 2 : undefined, run: () => mb.run(i), check: () => mb.check(i) }, log));
    }
  } else if (suite === "fieldbench") {
    const m: any = await import("./pkg/lowki_bench.js");
    await m.default();
    const ok = m.fb_check();
    const N = 2_000_000;
    const out: Record<string, unknown> = { mont32_equals_ark: ok };
    const t = (f: () => unknown) => { f(); const s: number[] = []; for (let i = 0; i < 7; i++) { const t0 = performance.now(); f(); s.push(performance.now() - t0); } return stats(s, 1).median_ms; };
    out.ark_mul_ns = (t(() => m.fb_mul_ark(N)) / N) * 1e6;
    out.mont32_mul_ns = (t(() => m.fb_mul_32(N)) / N) * 1e6;
    out.same_result = m.fb_mul_ark(1000) === m.fb_mul_32(1000);
    out.ark_fft_2_16_ms = t(() => m.fb_fft_ark(16));
    log(JSON.stringify(out));
    results.push({ id: "fieldbench", batch: 1, n: 7, warmup: 1, median_ms: 0, p90_ms: 0, min_ms: 0, mean_ms: 0, extra: out });
  } else if (suite.startsWith("g16-")) {
    const F = await fixtures(["g16"]);
    for (const name of ["commitment", "withdraw"]) {
      const art = await artifacts(name);
      const input = F.g16[name].input;
      const expected: string[] = F.g16[name].publicSignals;
      if (suite === "g16-snarkjs-mt" || suite === "g16-snarkjs-st") {
        const st = suite.endsWith("st");
        results.push(await benchSnarkjs(`g16.${name}.snarkjs_${st ? "1t" : "mt"}`, art, input, expected, st, G16_ITERS, log));
      } else {
        const m: any = await import("./pkg/lowki_bench.js");
        await m.default();
        const inputJson = JSON.stringify(input);
        let id: string, extra: Record<string, unknown>, prove: () => any, wit: number[] | undefined;
        if (suite === "g16-wasm-cpu") {
          const t = performance.now();
          const p = new m.G16(name, art.wasm, art.zkey, inputJson);
          const load = performance.now() - t;
          wit = await timeN(() => p.witness(), G16_ITERS);
          id = `g16.${name}.wasm_cpu`;
          extra = { load_ms: load, witness: stats(wit, 1), witness_backend: p.backend() };
          prove = () => p.prove();
        } else {
          const c = Number(new URLSearchParams(location.search).get("c") ?? 0);
          const [p, open, load, upload] = await m.g16_gpu(name, art.wasm, art.zkey, inputJson, c);
          id = `g16.${name}.wasm_webgpu`;
          extra = { gpu_open_ms: open, load_ms: load, upload_ms: upload };
          prove = () => p.prove();
        }
        const s = await timeN(prove, G16_ITERS);
        const last = await prove();
        const sig = JSON.parse(last).publicSignals;
        extra.verified_snarkjs = await verifyWithSnarkjs(art.vkey, last);
        extra.signals_match_fixture = JSON.stringify(sig) === JSON.stringify(expected);
        const st: Stat = { id, batch: 1, n: s.length, warmup: 2, ...stats(s, 1), check: sig.join(","), extra };
        log(`${id}: prove median ${st.median_ms.toFixed(1)} ms p90 ${st.p90_ms.toFixed(1)} ${JSON.stringify(extra)}`);
        results.push(st);
      }
    }
  }
  await fetch("/result", { method: "POST", body: JSON.stringify({ suite, env, results }) });
}

main().catch(async (e) => {
  log("FATAL " + (e?.stack ?? e));
  await fetch("/result", { method: "POST", body: JSON.stringify({ suite, fatal: String(e?.stack ?? e) }) });
});
