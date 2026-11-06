// The race: a real Privacy Pools withdraw proof (circuits 1.0.5, inputs from
// vectors/lowki-groth16/vectors.json) by Lowki's Rust/wasm prover on WebGPU, then by snarkjs
// 0.7.5 with its web workers, from the same bytes. Each proof is checked by both verifiers.
// Each prover runs alone, so neither slows the other down.
import * as snarkjs from "snarkjs";
// @ts-ignore: wasm-bindgen output, copied next to app.js at build time
import init, { load_withdraw, open_webgpu, verify as lowkiVerify } from "./pkg/lowki_site_prover.js";
import { noGpu } from "./gpu";
import type { Stage } from "./stage";

const $ = <T extends HTMLElement = HTMLElement>(id: string) => document.getElementById(id) as T;
const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));
const fmtS = (ms: number) => (ms / 1000).toFixed(2);

interface Artifacts {
  wasm: Uint8Array;
  zkey: Uint8Array;
  vkey: any;
  vkeyText: string;
  input: any;
  expected: string[];
}

/** The stage replays the GPU stages this many times slower than they ran. */
const SLOW = 12;

let art: Artifacts | null = null;
let prover: any = null;
let running = false;
let wasmReady: Promise<unknown> | null = null;
let gpuOpen: Promise<{ gpu: any; ms: number }> | null = null;
/** Opening WebGPU compiles the prover's shaders (seconds, the first time on a machine), so it
 * starts as soon as the race comes near the viewport. */
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

class Timer {
  raf = 0;
  constructor(readonly el: HTMLElement) {}
  start() {
    const t0 = performance.now();
    const tick = () => {
      this.el.textContent = fmtS(performance.now() - t0);
      this.raf = requestAnimationFrame(tick);
    };
    tick();
  }
  stop(ms: number) {
    cancelAnimationFrame(this.raf);
    this.el.textContent = fmtS(ms);
  }
}

export interface RaceInfo { setup: Record<string, number | string>; }
export const raceInfo: Record<string, any> = {};

export function setupRace(stage: Stage | null) {
  const btn = $<HTMLButtonElement>("race-btn");
  const label = btn.querySelector<HTMLElement>(".label")!;
  wasmReady = init();
  const io = new IntersectionObserver((es) => {
    if (es.some((e) => e.isIntersecting)) {
      io.disconnect();
      openGpuEarly();
    }
  }, { rootMargin: "1200px 0px" });
  io.observe($("prove"));
  if (noGpu || !("gpu" in navigator)) $("lowki-tag").textContent = "1 CPU thread";

  btn.addEventListener("click", async () => {
    if (running) return;
    running = true;
    btn.disabled = true;
    const race = $("race");
    race.dataset.state = "run";
    for (const id of ["lowki-time", "snark-time"]) $(id).textContent = "0.00";
    for (const id of ["lowki-ok", "snark-ok"]) ($(id).textContent = ""), ($(id).dataset.ok = "");
    $("result-x").textContent = "";
    stage?.setRace({ r1: 0, burst: false });
    try {
      await run(stage, label);
      label.textContent = "Again";
    } catch (e: any) {
      console.warn(e);
      $("race-note").textContent = `That didn't work: ${e?.message ?? e}. Reload and try again.`;
      race.dataset.state = "error";
      label.textContent = "Try again";
    } finally {
      running = false;
      btn.disabled = false;
    }
  });
}

async function run(stage: Stage | null, label: HTMLElement) {
  const note = $("race-note");
  if (!art) {
    stage?.racePhase(1, 400);
    art = await download((got, total) => {
      label.textContent = `Loading ${Math.floor((got / total) * 100)}%`;
      $("race-btn").style.setProperty("--p", String(got / total));
      stage?.setRace({ download: got / total });
    });
  } else {
    stage?.setRace({ download: 1 });
    stage?.racePhase(1, 300);
  }
  $("race-btn").style.setProperty("--p", "1");
  const a = art;
  const inputJson = JSON.stringify(a.input);
  label.textContent = "Proving";

  const setup: Record<string, any> = raceInfo.setup ?? {};
  if (!prover) {
    await wasmReady;
    let t = performance.now();
    prover = await load_withdraw(a.wasm, a.zkey);
    setup.load_ms = performance.now() - t;
    try {
      if (noGpu) throw new Error("WebGPU disabled with ?nogpu");
      const opened = await openGpuEarly();
      if (!opened) throw new Error("no WebGPU");
      setup.gpu_open_ms = opened.ms;
      setup.adapter = opened.gpu.adapter();
      t = performance.now();
      prover.use_gpu(opened.gpu);
      setup.upload_ms = performance.now() - t;
    } catch (e: any) {
      setup.gpu = String(e?.message ?? e);
      $("lowki-tag").textContent = "1 CPU thread";
    }
    raceInfo.setup = setup;
  }
  if (stage) await Promise.race([stage.racePhase(2, 700), sleep(900)]);

  // Lowki proves; the stage pauses so it doesn't share the GPU.
  const lowki = new Timer($("lowki-time"));
  stage?.pause(true);
  await sleep(30);
  lowki.start();
  const t1 = performance.now();
  const lowkiOut = JSON.parse(await prover.prove(inputJson));
  const lowkiMs = performance.now() - t1;
  lowki.stop(lowkiMs);
  stage?.pause(false);
  const lowkiProof = JSON.stringify({ proof: lowkiOut.proof, publicSignals: lowkiOut.publicSignals });
  const lowkiSelf = lowkiVerify(a.vkeyText, lowkiProof);
  const lowkiSignals = JSON.stringify(lowkiOut.publicSignals) === JSON.stringify(a.expected);

  const stages: [string, number][] = lowkiOut.stages;
  const slow = lowkiOut.backend === "webgpu" ? SLOW : 1;
  const fin = stages.filter(([n]) => n.startsWith("assemble")).reduce((s, [, m]) => s + m, 0);
  const show = stage
    ? (async () => {
        await stage.racePhase(3, Math.max(1400, (lowkiMs - fin) * slow));
        await stage.racePhase(4, Math.max(1000, fin * slow * 8));
      })()
    : Promise.resolve();

  // snarkjs, same input, same bytes
  const snark = new Timer($("snark-time"));
  await sleep(30);
  snark.start();
  const t2 = performance.now();
  const s = await snarkjsProve(a);
  const snarkMs = performance.now() - t2;
  snark.stop(snarkMs);
  const snarkProof = JSON.stringify({ proof: s.proof, publicSignals: s.publicSignals });

  const lowkiBySnark = await snarkjs.groth16.verify(a.vkey, lowkiOut.publicSignals, lowkiOut.proof);
  const snarkBySnark = await snarkjs.groth16.verify(a.vkey, s.publicSignals, s.proof);
  const snarkByLowki = lowkiVerify(a.vkeyText, snarkProof);
  const snarkSignals = JSON.stringify(s.publicSignals) === JSON.stringify(a.expected);
  const lowkiOk = lowkiSelf && lowkiBySnark && lowkiSignals;
  const snarkOk = snarkBySnark && snarkByLowki && snarkSignals;
  const okText = (ok: boolean) => (ok ? "✓ verified" : "✕ failed");
  $("lowki-ok").textContent = okText(lowkiOk);
  $("lowki-ok").dataset.ok = String(lowkiOk);
  $("lowki-ok").title = "Checked by Lowki's verifier and by snarkjs groth16.verify; public signals match.";
  $("snark-ok").textContent = okText(snarkOk);
  $("snark-ok").dataset.ok = String(snarkOk);
  $("snark-ok").title = "Checked by snarkjs groth16.verify and by Lowki's verifier; public signals match.";

  await Promise.race([show, sleep(5000)]);
  const x = snarkMs / lowkiMs;
  $("result-x").textContent = x >= 1 ? `${x.toFixed(1)}× faster` : `${(1 / x).toFixed(1)}× slower`;
  $("race").dataset.state = lowkiOk && snarkOk ? "done" : "error";
  if (x >= 1) stage?.setRace({ burst: true });
  note.textContent = lowkiOut.backend === "webgpu" ? "" : "No GPU here: one CPU thread against every core. Try a desktop browser.";
  Object.assign(raceInfo, { lowkiMs, snarkMs, x, backend: lowkiOut.backend, stages, lowkiSelf, lowkiBySnark, snarkBySnark, snarkByLowki, lowkiSignals, snarkSignals, allOk: lowkiOk && snarkOk });
  (window as any).__lowkiRace = { ...raceInfo };
}
