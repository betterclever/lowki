// Drives headless Chrome (via chromedriver, W3C WebDriver over HTTP, no deps) on the page built
// by build.mjs, for each requested anon network, and prints window.__anon as JSON lines.
// Run on the macOS host:
//   mac sh -lc 'cd <lowki>/packages/lowki-kohaku-host && node browser-anon/run-mac.mjs nox nym tor-js'
// Env: CHROMEDRIVER, ANON_QUERY (extra query string, e.g. "gw=ip:port:hash&boot=180000"),
//      ANON_TIMEOUT_MS (per network, default 600000).
import { createServer } from "node:http";
import { readFile } from "node:fs/promises";
import { spawn } from "node:child_process";
import path from "node:path";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const CHROMEDRIVER = process.env.CHROMEDRIVER
  ?? path.resolve(here, "../../../target/tools/chromedriver/mac_arm-156.0.8078.80/chromedriver-mac-arm64/chromedriver");
const nets = process.argv.slice(2).length ? process.argv.slice(2) : ["nox", "nym", "tor-js"];
const TIMEOUT = Number(process.env.ANON_TIMEOUT_MS ?? 600_000);

const types = { ".html": "text/html", ".js": "text/javascript" };
const server = createServer(async (req, res) => {
  const p = new URL(req.url, "http://x").pathname;
  const file = p === "/" ? "/index.html" : p;
  try {
    const data = await readFile(path.join(here, file));
    res.writeHead(200, { "content-type": types[path.extname(file)] ?? "application/octet-stream" }).end(data);
  } catch { res.writeHead(404).end("not found"); }
});
await new Promise((r) => server.listen(0, "127.0.0.1", r));
const pageUrl = `http://127.0.0.1:${server.address().port}/`;

const port = 9500 + Math.floor(Math.random() * 400);
const drv = spawn(CHROMEDRIVER, [`--port=${port}`], { stdio: "ignore" });
const wd = async (method, p, body) => {
  const r = await fetch(`http://127.0.0.1:${port}${p}`, {
    method, headers: { "content-type": "application/json" }, body: body ? JSON.stringify(body) : undefined,
  });
  const j = await r.json();
  if (j.value?.error) throw new Error(`${j.value.error}: ${j.value.message}`);
  return j.value;
};
for (let i = 0; ; i++) {
  try { await wd("GET", "/status"); break; } catch { if (i > 50) throw new Error("chromedriver did not start"); await new Promise((r) => setTimeout(r, 200)); }
}

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
try {
  for (const net of nets) {
    const { sessionId } = await wd("POST", "/session", { capabilities: { alwaysMatch: { "goog:chromeOptions": { args: [
      "--headless=new", "--no-first-run", "--no-default-browser-check",
      // Same WebRTC flags as anon-rpc's own e2e / Kohaku's PoC (real KPS dials).
      "--disable-features=WebRtcHideLocalIpsWithMdns", "--force-webrtc-ip-handling-policy=default",
    ] } } } });
    const t0 = Date.now();
    try {
      await wd("POST", `/session/${sessionId}/url`, { url: `${pageUrl}?net=${net}${process.env.ANON_QUERY ? "&" + process.env.ANON_QUERY : ""}` });
      let result;
      while (!result && Date.now() - t0 < TIMEOUT) {
        await sleep(2000);
        result = await wd("POST", `/session/${sessionId}/execute/sync`, { script: "return window.__anon ?? null", args: [] });
      }
      const text = await wd("POST", `/session/${sessionId}/execute/sync`, { script: "return document.getElementById('out').textContent", args: [] });
      console.log(`=== ${net} (${Date.now() - t0} ms wall)\n${text}`);
      console.log(`ANON_RESULT ${JSON.stringify(result ?? { net, ok: false, error: `no result within ${TIMEOUT} ms` })}`);
    } finally {
      await wd("DELETE", `/session/${sessionId}`).catch(() => {});
    }
  }
} finally {
  drv.kill();
  server.close();
}
