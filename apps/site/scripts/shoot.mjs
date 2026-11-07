// Headless Chrome check of the site (run on the macOS host, with `./serve.sh` running):
//   node scripts/shoot.mjs [url]
// For 1440 and 390 px, light and dark: loads the page, screenshots the hero, runs the race,
// screenshots it in progress and with results, then a full page. Records the race times, every
// console message and exception, and every network request (fails if any leaves the origin).
// Writes screens/*.png and screens/report.json.
import { spawn } from "node:child_process";
import { mkdirSync, mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const HERE = dirname(fileURLToPath(import.meta.url));
const OUT = join(HERE, "../screens");
const URL0 = process.argv[2] ?? `http://localhost:8840/${process.env.NO_WEBGPU === "1" ? "?nogpu" : ""}`;
const CHROME = process.env.CHROME ?? "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";
const PORT = Number(process.env.CDP_PORT ?? 8849);
const ONLY = process.env.ONLY; // e.g. "1440-dark"
const NO_RACE = process.env.NO_RACE === "1";
mkdirSync(OUT, { recursive: true });

const profile = mkdtempSync(join(tmpdir(), "lowki-site-chrome-"));
const chrome = spawn(CHROME, [
  "--headless=new", "--enable-unsafe-webgpu", "--enable-features=WebGPU", "--ignore-gpu-blocklist",
  "--no-first-run", "--no-default-browser-check", "--disable-background-timer-throttling",
  "--disable-renderer-backgrounding", "--disable-backgrounding-occluded-windows", "--hide-scrollbars",
  `--remote-debugging-port=${PORT}`, `--user-data-dir=${profile}`, "about:blank",
], { stdio: ["ignore", "ignore", "pipe"] });
chrome.stderr.on("data", () => {});
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));

let wsUrl;
for (let i = 0; i < 100 && !wsUrl; i++) {
  await sleep(100);
  try {
    const list = await (await fetch(`http://127.0.0.1:${PORT}/json/list`)).json();
    wsUrl = list.find((t) => t.type === "page")?.webSocketDebuggerUrl;
  } catch {}
}
if (!wsUrl) throw new Error("no Chrome page target");
const ws = new WebSocket(wsUrl);
await new Promise((r) => ws.addEventListener("open", r));
let id = 0;
const pending = new Map();
const listeners = [];
ws.addEventListener("message", (ev) => {
  const m = JSON.parse(ev.data);
  if (m.id && pending.has(m.id)) {
    const { res, rej } = pending.get(m.id);
    pending.delete(m.id);
    m.error ? rej(new Error(JSON.stringify(m.error))) : res(m.result);
  } else if (m.method) listeners.forEach((l) => l(m));
});
const send = (method, params = {}) => new Promise((res, rej) => { const i = ++id; pending.set(i, { res, rej }); ws.send(JSON.stringify({ id: i, method, params })); });
const evaluate = async (expr) => (await send("Runtime.evaluate", { expression: expr, awaitPromise: true, returnByValue: true })).result.value;

const report = { url: URL0, chrome: (await (await fetch(`http://127.0.0.1:${PORT}/json/version`)).json()).Browser, runs: [], console: [], exceptions: [], requests: [], external: [] };
const origin = new URL(URL0).origin;
listeners.push((m) => {
  if (m.method === "Network.requestWillBeSent") {
    const u = m.params.request.url;
    report.requests.push(u);
    if (/^https?:/.test(u) && new URL(u).origin !== origin) report.external.push(u);
  }
  if (m.method === "Runtime.consoleAPICalled") report.console.push({ type: m.params.type, text: m.params.args.map((a) => a.value ?? a.description ?? "").join(" ") });
  if (m.method === "Runtime.exceptionThrown") report.exceptions.push(m.params.exceptionDetails.exception?.description ?? m.params.exceptionDetails.text);
  if (m.method === "Log.entryAdded") report.console.push({ type: `log-${m.params.entry.level}`, text: m.params.entry.text + (m.params.entry.url ? ` (${m.params.entry.url})` : "") });
});
await send("Page.enable");
await send("Runtime.enable");
await send("Network.enable");
await send("Log.enable");

async function shot(name, full = false) {
  let params = { format: "png" };
  if (full) {
    const h = await evaluate("document.documentElement.scrollHeight");
    const w = await evaluate("document.documentElement.clientWidth");
    params = { ...params, captureBeyondViewport: true, clip: { x: 0, y: 0, width: w, height: h, scale: 1 } };
  }
  const { data } = await send("Page.captureScreenshot", params);
  writeFileSync(join(OUT, `${name}.png`), Buffer.from(data, "base64"));
  console.log("  shot", name);
}
async function waitFor(expr, ms = 60000) {
  const t = Date.now();
  while (Date.now() - t < ms) {
    if (await evaluate(expr)) return true;
    await sleep(50);
  }
  return false;
}

const configs = [{ w: 1440, h: 900, dpr: 1, mobile: false }, { w: 390, h: 844, dpr: 2, mobile: true }];
const SCREENCAST = process.env.SCREENCAST !== "0";
for (const c of configs) {
  const tag = (process.env.PREFIX ?? "") + c.w;
  if (ONLY && ONLY !== String(c.w)) continue;
  console.log(tag);
  await send("Emulation.setDeviceMetricsOverride", { width: c.w, height: c.h, deviceScaleFactor: c.dpr, mobile: c.mobile });
  await send("Emulation.setEmulatedMedia", { features: [{ name: "prefers-reduced-motion", value: process.env.REDUCED === "1" ? "reduce" : "no-preference" }] });
  await send("Page.navigate", { url: URL0 });
  await waitFor("document.readyState === 'complete' && document.documentElement.classList.contains('ready')", 20000);
  await evaluate("document.documentElement.style.scrollBehavior = 'auto'");
  await sleep(1500);
  const n = await evaluate("document.querySelectorAll('.scene').length");
  const tops = await evaluate("[...document.querySelectorAll('.scene')].map(s => s.offsetTop)");
  for (let k = 0; k < n; k++) {
    await evaluate(`window.scrollTo(0, ${tops[k]})`);
    await sleep(1400);
    await shot(`${tag}-scene${k}`);
    if (k === 0 && !c.mobile) {
      // the cursor parts the lauki
      const r = await evaluate("(() => { const b = document.querySelector('.s0 .art').getBoundingClientRect(); return [b.left + b.width / 2, b.top + b.height / 2]; })()");
      for (let j = 0; j <= 10; j++) { await send("Input.dispatchMouseEvent", { type: "mouseMoved", x: r[0] - 60 + j * 8, y: r[1] - 40 + j * 6 }); await sleep(40); }
      await sleep(500);
      await shot(`${tag}-scene0-cursor`);
      await send("Input.dispatchMouseEvent", { type: "mouseMoved", x: 5, y: c.h - 5 });
    }
    if (k < n - 1 && (k % 2 === 0 || c.w === 1440)) {
      await evaluate(`window.scrollTo(0, ${Math.round((tops[k] + tops[k + 1]) / 2)})`);
      await sleep(250);
      await shot(`${tag}-scene${k}-to-${k + 1}`);
    }
  }
  report.stage = report.stage ?? {};
  report.stage[tag] = await evaluate("JSON.stringify(window.__stage)");
  if (SCREENCAST) {
    // a scroll-through, recorded with the CDP screencast
    const dir = join(OUT, `scroll-${tag}`);
    mkdirSync(dir, { recursive: true });
    let fi = 0;
    const onFrame = async (m) => {
      if (m.method !== "Page.screencastFrame") return;
      send("Page.screencastFrameAck", { sessionId: m.params.sessionId }).catch(() => {});
      if (fi < 400) writeFileSync(join(dir, `f${String(fi++).padStart(3, "0")}.jpg`), Buffer.from(m.params.data, "base64"));
    };
    listeners.push(onFrame);
    await evaluate("window.scrollTo(0, 0)");
    await sleep(500);
    await send("Page.startScreencast", { format: "jpeg", quality: 70, maxWidth: 960, maxHeight: 600, everyNthFrame: 2 });
    const H = await evaluate("document.documentElement.scrollHeight - innerHeight");
    for (let y = 0; y <= H; y += 18) { await evaluate(`window.scrollTo(0, ${y})`); await sleep(16); }
    await sleep(500);
    await send("Page.stopScreencast");
    listeners.splice(listeners.indexOf(onFrame), 1);
    console.log("  screencast frames", fi);
  }
  if (NO_RACE) continue;
  await evaluate("window.scrollTo(0, document.getElementById('prove').offsetTop)");
  await sleep(1500);
  await evaluate("document.getElementById('race-btn').click()");
  await waitFor("document.getElementById('lowki-ok') && document.getElementById('lowki-time').textContent !== '0.00' && document.getElementById('race-btn').querySelector('.label').textContent === 'Proving'", 120000);
  await waitFor("document.getElementById('snark-time').textContent !== '0.00' && document.getElementById('snark-time').textContent !== '–'", 120000);
  await sleep(200);
  await shot(`${tag}-race-progress`);
  await sleep(1300);
  await shot(`${tag}-race-msm`);
  await waitFor("!!window.__lowkiRace", 120000);
  await sleep(350);
  await shot(`${tag}-race-burst`);
  await sleep(1500);
  await shot(`${tag}-race-results`);
  const r1 = await evaluate("JSON.stringify(window.__lowkiRace)");
  await evaluate("window.__lowkiRace = null; document.getElementById('race-btn').click()");
  await waitFor("!!window.__lowkiRace", 120000);
  await sleep(1000);
  const r2 = await evaluate("JSON.stringify(window.__lowkiRace)");
  report.runs.push({ tag, cold: JSON.parse(r1), warm: JSON.parse(r2) });
  const p = (r) => `${r.lowkiMs.toFixed(1)} / ${r.snarkMs.toFixed(1)} ms ok=${r.allOk} ${r.backend}`;
  console.log("  cold", p(JSON.parse(r1)), " warm", p(JSON.parse(r2)), JSON.stringify(JSON.parse(r1).setup));
}
writeFileSync(join(OUT, "report.json"), JSON.stringify(report, null, 1));
console.log("external requests:", report.external.length, report.external.slice(0, 5));
console.log("exceptions:", report.exceptions.length, report.exceptions.slice(0, 5));
console.log("console:", report.console.filter((m) => /error|warn/.test(m.type)).slice(0, 10));
ws.close();
chrome.kill("SIGTERM");
process.exit(0);
