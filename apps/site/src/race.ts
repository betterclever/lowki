// "Prove it on your GPU": a real Privacy Pools withdraw proof (circuits 1.0.5, inputs from
// vectors/lowki-groth16/vectors.json) made by Lowki's Rust/wasm prover on WebGPU, then by
// snarkjs 0.7.5 with its web workers, on the same bytes. Both proofs are verified by both
// verifiers. Each prover runs alone, so neither slows the other down.
import * as snarkjs from "snarkjs";
// @ts-ignore: wasm-bindgen output, copied next to app.js at build time
import init, { load_withdraw, open_webgpu, verify as lowkiVerify } from "./pkg/lowki_site_prover.js";
import type { Vis } from "./vis";
import { adapterLabel, noGpu } from "./gpu";

const $ = <T extends HTMLElement = HTMLElement>(id: string) => document.getElementById(id) as T;
const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));
const fmtMs = (ms: number) => (ms < 1000 ? `${ms.toFixed(0)} ms` : `${(ms / 1000).toFixed(2)} s`);
const fmtS = (ms: number) => `${(ms / 1000).toFixed(ms < 10000 ? 2 : 1)} s`;
const MB = (b: number) => (b / 1e6).toFixed(1);

interface Artifacts {
  wasm: Uint8Array;
  zkey: Uint8Array;
  vkey: any;
  vkeyText: string;
  input: any;
  expected: string[];
}

/** The replay of the GPU stages in the visualisation runs this many times slower than real. */
const SLOW = 12;

let art: Artifacts | null = null;
let prover: any = null;
let gpuNote = "";
let running = false;
let wasmReady: Promise<unknown> | null = null;
/** WebGPU is opened ahead of the click (it compiles the MSM/NTT shaders, which can take seconds
 * the first time on a machine), as soon as the proof section comes near the viewport. */
let gpuOpen: Promise<{ gpu: any; ms: number }> | null = null;
function openGpuEarly() {
  if (noGpu || !("gpu" in navigator)) return null;
  gpuOpen ??= (wasmReady ?? Promise.resolve()).then(async () => {
    const t = performance.now();
    const gpu = await open_webgpu();
    return { gpu, ms: performance.now() - t };
  });
  gpuOpen.catch(() => {});
  return gpuOpen;
}

async function fetchWithProgress(url: string, onBytes: (n: number) => void): Promise<Uint8Array> {
  const res = await fetch(url);
  if (!res.ok || !res.body) throw new Error(`${url}: HTTP ${res.status}`);
  const total = Number(res.headers.get("content-length")) || 0;
  const out = total ? new Uint8Array(total) : null;
  const chunks: Uint8Array[] = [];
  let n = 0;
  const rd = res.body.getReader();
  for (;;) {
    const { done, value } = await rd.read();
    if (done) break;
    if (out && n + value.length <= total) out.set(value, n);
    else chunks.push(value);
    n += value.length;
    onBytes(value.length);
  }
  if (out && n === total) return out;
  const all = new Uint8Array(n);
  let o = 0;
  for (const c of out ? [out.subarray(0, Math.min(n, total)), ...chunks] : chunks) all.set(c, o), (o += c.length);
  return all;
}

async function download(onProgress: (got: number, total: number) => void): Promise<Artifacts> {
  const sizes = await Promise.all(
    ["withdraw.zkey", "withdraw.wasm"].map((f) => fetch(`artifacts/${f}`, { method: "HEAD" }).then((r) => Number(r.headers.get("content-length")) || 0)),
  );
  const total = sizes[0] + sizes[1];
  let got = 0;
  const tick = (b: number) => onProgress((got += b), total);
  const [zkey, wasm, vkeyText, inp] = await Promise.all([
    fetchWithProgress("artifacts/withdraw.zkey", tick),
    fetchWithProgress("artifacts/withdraw.wasm", tick),
    fetch("artifacts/withdraw.vkey").then((r) => r.text()),
    fetch("artifacts/withdraw-input.json").then((r) => r.json()),
  ]);
  return { zkey, wasm, vkeyText, vkey: JSON.parse(vkeyText), input: inp.input, expected: inp.publicSignals };
}

function unstringify(o: any): any {
  if (typeof o === "string" && /^[0-9]+$|^0x[0-9a-fA-F]+$/.test(o)) return BigInt(o);
  if (Array.isArray(o)) return o.map(unstringify);
  if (o && typeof o === "object") return Object.fromEntries(Object.entries(o).map(([k, v]) => [k, unstringify(v)]));
  return o;
}

/** snarkjs 0.7.5 fullProve, exactly as groth16.fullProve does it: witness, then prove. */
async function snarkjsProve(a: Artifacts) {
  const w: any = { type: "mem" };
  await snarkjs.wtns.calculate(unstringify(a.input), a.wasm, w);
  return snarkjs.groth16.prove(a.zkey, w);
}

// ---- UI ----------------------------------------------------------------------------------------

type StepState = "todo" | "run" | "ok" | "warn" | "err";
function step(id: string, state: StepState, value?: string) {
  const el = $(`step-${id}`);
  if (!el) return;
  el.dataset.state = state;
  if (value !== undefined) el.querySelector<HTMLElement>(".v")!.textContent = value;
}

class Lane {
  t0 = 0;
  raf = 0;
  ms = 0;
  constructor(readonly key: string) {}
  get timeEl() {
    return $(`${this.key}-time`);
  }
  reset() {
    cancelAnimationFrame(this.raf);
    this.ms = 0;
    this.timeEl.textContent = "–";
    $(`${this.key}-bar`).style.setProperty("--w", "0");
    $(`lane-${this.key}`).dataset.state = "idle";
    $(`${this.key}-checks`).innerHTML = "";
  }
  start() {
    $(`lane-${this.key}`).dataset.state = "run";
    this.t0 = performance.now();
    const tick = () => {
      const ms = performance.now() - this.t0;
      this.timeEl.textContent = fmtS(ms);
      $(`${this.key}-bar`).style.setProperty("--w", String(Math.min(1, ms / 2500)));
      this.raf = requestAnimationFrame(tick);
    };
    tick();
  }
  stop(ms: number) {
    cancelAnimationFrame(this.raf);
    this.ms = ms;
    this.timeEl.textContent = fmtS(ms);
    $(`lane-${this.key}`).dataset.state = "done";
  }
  check(label: string, ok: boolean) {
    const li = document.createElement("li");
    li.dataset.ok = String(ok);
    li.innerHTML = `<span class="tick" aria-hidden="true">${ok ? "✓" : "✕"}</span>`;
    li.append(document.createTextNode(label));
    $(`${this.key}-checks`).append(li);
  }
}

function scaleBars(lanes: Lane[]) {
  const max = Math.max(...lanes.map((l) => l.ms));
  for (const l of lanes) $(`${l.key}-bar`).style.setProperty("--w", String(max ? l.ms / max : 0));
}

function showStages(stages: [string, number][], total: number) {
  const box = $("stages");
  const groups: [string, string, number][] = [
    ["witness", "Witness (browser WebAssembly)", 0],
    ["qap", "Constraints + NTTs on the GPU", 0],
    ["msm", "MSMs on the GPU (5, incl. G2)", 0],
    ["fin", "Assemble + pairing self-check", 0],
  ];
  for (const [name, ms] of stages) {
    const g = name === "witness" ? 0 : name.startsWith("assemble") ? 3 : /MSM|A,B,L|submit H/.test(name) ? 2 : 1;
    groups[g][2] += ms;
  }
  const sum = groups.reduce((s, g) => s + g[2], 0) || total;
  box.innerHTML = "";
  const bar = document.createElement("div");
  bar.className = "stage-bar";
  const legend = document.createElement("ul");
  legend.className = "stage-legend";
  for (const [k, label, ms] of groups) {
    const seg = document.createElement("i");
    seg.className = `seg seg-${k}`;
    seg.style.flexGrow = String(Math.max(ms, 0.5));
    bar.append(seg);
    const li = document.createElement("li");
    li.innerHTML = `<span class="sw seg-${k}"></span><span>${label}</span><span class="ms">${fmtMs(ms)}</span>`;
    legend.append(li);
  }
  box.append(bar, legend);
  box.hidden = false;
}

export function setupRace(vis: Vis | null, gpuAvailable: boolean) {
  const btn = $<HTMLButtonElement>("race-btn");
  const status = $("race-status");
  const lowki = new Lane("lowki");
  const snark = new Lane("snark");
  $("snark-sub").textContent = `snarkjs 0.7.5 · ${navigator.hardwareConcurrency || "?"} workers`;
  if (!gpuAvailable) {
    $("lowki-sub").textContent = "Rust/wasm · one CPU thread";
    $("race-gpu-note").hidden = false;
  }
  wasmReady = init();
  const io = new IntersectionObserver((es) => {
    if (es.some((e) => e.isIntersecting)) {
      io.disconnect();
      openGpuEarly();
    }
  }, { rootMargin: "800px 0px" });
  io.observe($("prove"));

  btn.addEventListener("click", async () => {
    if (running) return;
    running = true;
    btn.disabled = true;
    btn.textContent = "Proving…";
    $("result").hidden = true;
    $("stages").hidden = true;
    $("race").dataset.state = "run";
    lowki.reset();
    snark.reset();
    vis?.reset();
    try {
      await run(vis, lowki, snark, status);
      btn.textContent = "Run it again";
    } catch (e: any) {
      console.warn(e);
      status.textContent = `Something went wrong: ${e?.message ?? e}. Reload the page and try again.`;
      $("race").dataset.state = "error";
      btn.textContent = "Try again";
    } finally {
      running = false;
      btn.disabled = false;
    }
  });
}

async function run(vis: Vis | null, lowki: Lane, snark: Lane, status: HTMLElement) {
  // 1. Download the circuit (once per page).
  if (!art) {
    for (const s of ["dl", "load", "gpu", "upload"]) step(s, "todo", "");
    step("dl", "run", "starting");
    status.textContent = "Downloading the withdraw circuit…";
    vis?.go(1, 500);
    art = await download((got, total) => {
      step("dl", "run", `${MB(got)} / ${MB(total)} MB`);
      $("dl-bar").style.setProperty("--w", String(got / total));
      vis?.download(got / total);
    });
    step("dl", "ok", `${MB(art.zkey.length + art.wasm.length)} MB`);
    vis?.download(1);
  } else {
    vis?.download(1);
    vis?.go(1, 300);
  }
  const a = art;
  const inputJson = JSON.stringify(a.input);

  // 2. Lowki: load + pin the circuit, open WebGPU, upload the proving key (once per page).
  if (!prover) {
    await wasmReady;
    step("load", "run", "");
    status.textContent = "Loading the circuit into Lowki's prover (sha256-pinned)…";
    await sleep(30);
    let t = performance.now();
    prover = await load_withdraw(a.wasm, a.zkey);
    step("load", "ok", fmtMs(performance.now() - t));
    step("gpu", "run", "");
    try {
      t = performance.now();
      if (noGpu) throw new Error("WebGPU disabled with ?nogpu");
      const opened = await openGpuEarly();
      if (!opened) throw new Error("no WebGPU");
      const gpu = opened.gpu;
      const tOpen = opened.ms;
      step("gpu", "ok", `${adapterLabel || gpu.adapter()} · ${fmtMs(tOpen)}`);
      if (tOpen > 1500) $("step-gpu").title = "The first run on a machine compiles the GPU shaders; the browser caches them after that.";
      step("upload", "run", "");
      t = performance.now();
      prover.use_gpu(gpu);
      step("upload", "ok", fmtMs(performance.now() - t));
    } catch (e: any) {
      gpuNote = String(e?.message ?? e);
      step("gpu", "warn", "not available");
      step("upload", "warn", "skipped");
      $("lowki-sub").textContent = "Rust/wasm · one CPU thread";
      $("race-gpu-note").hidden = false;
      console.info("WebGPU unavailable for proving:", gpuNote);
    }
  }
  const backend = prover.backend();
  await (vis ? Promise.race([vis.go(2, 900), sleep(1100)]) : sleep(50));

  // 3. Lowki proves. The render pauses so it doesn't share the GPU with the prover.
  status.textContent = backend === "webgpu" ? "Lowki is proving on your GPU…" : "Lowki is proving on one CPU thread (about 2 s)…";
  vis?.pause(true);
  await sleep(40);
  lowki.start();
  const t1 = performance.now();
  const lowkiOut = JSON.parse(await prover.prove(inputJson));
  const lowkiMs = performance.now() - t1;
  lowki.stop(lowkiMs);
  vis?.pause(false);
  const lowkiProof = JSON.stringify({ proof: lowkiOut.proof, publicSignals: lowkiOut.publicSignals });
  const lowkiSelf = lowkiVerify(a.vkeyText, lowkiProof);
  const lowkiSignals = JSON.stringify(lowkiOut.publicSignals) === JSON.stringify(a.expected);

  // Replay the measured stages, slowed down, while snarkjs runs.
  const stages: [string, number][] = lowkiOut.stages;
  const slow = lowkiOut.backend === "webgpu" ? SLOW : 1;
  const fin = stages.filter(([n]) => n.startsWith("assemble")).reduce((s, [, m]) => s + m, 0);
  const visDone = vis
    ? (async () => {
        await vis.go(3, Math.max(1400, (lowkiMs - fin) * slow));
        await vis.go(4, Math.max(1100, fin * slow * 8));
        await vis.go(5, 900);
      })()
    : Promise.resolve();
  if (lowkiOut.backend === "webgpu") showStages(stages, lowkiOut.total_ms);

  // 4. snarkjs proves the same input from the same bytes.
  status.textContent = "snarkjs is proving with its web workers…";
  await sleep(30);
  snark.start();
  const t2 = performance.now();
  const s = await snarkjsProve(a);
  const snarkMs = performance.now() - t2;
  snark.stop(snarkMs);
  scaleBars([lowki, snark]);
  const snarkProof = JSON.stringify({ proof: s.proof, publicSignals: s.publicSignals });

  // 5. Cross-verify: each proof with both verifiers.
  status.textContent = "Verifying both proofs with both verifiers…";
  const lowkiBySnark = await snarkjs.groth16.verify(a.vkey, lowkiOut.publicSignals, lowkiOut.proof);
  const snarkBySnark = await snarkjs.groth16.verify(a.vkey, s.publicSignals, s.proof);
  const snarkByLowki = lowkiVerify(a.vkeyText, snarkProof);
  const snarkSignals = JSON.stringify(s.publicSignals) === JSON.stringify(a.expected);
  lowki.check("valid (Lowki verifier)", lowkiSelf);
  lowki.check("valid (snarkjs verify)", lowkiBySnark);
  lowki.check("public signals match", lowkiSignals);
  snark.check("valid (snarkjs verify)", snarkBySnark);
  snark.check("valid (Lowki verifier)", snarkByLowki);
  snark.check("public signals match", snarkSignals);
  const allOk = lowkiSelf && lowkiBySnark && snarkBySnark && snarkByLowki && lowkiSignals && snarkSignals;

  await Promise.race([visDone, sleep(6000)]);

  // 6. The result.
  const x = snarkMs / lowkiMs;
  $("result-x").textContent = x >= 1 ? `${x.toFixed(1)}×` : `${(1 / x).toFixed(1)}×`;
  $("result-word").textContent = x >= 1 ? "faster than snarkjs" : "slower than snarkjs";
  $("result-detail").textContent =
    `Lowki ${fmtS(lowkiMs)} (${lowkiOut.backend === "webgpu" ? "WebGPU" : "one CPU thread"}) vs snarkjs ${fmtS(snarkMs)} ` +
    `(${navigator.hardwareConcurrency || "?"} workers), same withdraw, same bytes. ` +
    (allOk ? "Both proofs are valid under both verifiers." : "A check failed: see the lanes.") +
    (lowkiOut.backend === "webgpu" ? "" : " Without a GPU Lowki uses one thread against snarkjs's many; thread for thread it is 2.1× faster (see the benchmarks below).");
  $("result").hidden = false;
  $("race").dataset.state = allOk ? "done" : "error";
  status.textContent = allOk ? "Done. Both proofs verified." : "Done, but a check failed.";
  (window as any).__lowkiRace = { lowkiMs, snarkMs, x, backend: lowkiOut.backend, stages, allOk, lowkiSelf, lowkiBySnark, snarkBySnark, snarkByLowki, lowkiSignals, snarkSignals, gpuNote };
}
