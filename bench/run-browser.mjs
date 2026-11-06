// Serves bench/web, launches headless Chrome (WebGPU on) on ?suite=<suite>, collects the
// result the page POSTs back, kills Chrome. One fresh Chrome process per suite.
// Usage (host): node run-browser.mjs <suite> [iters]   -> results/browser-<suite>.json
import { createServer } from "node:http";
import { spawn } from "node:child_process";
import { mkdtempSync, readFileSync, writeFileSync, existsSync, statSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, extname, join, normalize } from "node:path";
import { fileURLToPath } from "node:url";

const HERE = dirname(fileURLToPath(import.meta.url));
const ART = process.env.LOWKI_PP_ARTIFACTS ?? join(HERE, "../../vendor/npm/circuits/package/artifacts");
const CHROME = process.env.CHROME ?? "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";
const [suite, iters = "10"] = process.argv.slice(2);
const NO_ISO = process.env.NO_ISOLATION === "1";
const MIME = { ".html": "text/html", ".js": "text/javascript", ".mjs": "text/javascript", ".wasm": "application/wasm", ".json": "application/json", ".zkey": "application/octet-stream", ".vkey": "application/json" };
const ROUTES = [["/fixtures/", join(HERE, "fixtures")], ["/artifacts/", ART], ["/", join(HERE, "web")]];

let done;
const finished = new Promise((r) => (done = r));
const server = createServer((req, res) => {
  const url = new URL(req.url, "http://x");
  if (req.method === "POST") {
    let body = "";
    req.on("data", (c) => (body += c));
    req.on("end", () => {
      res.end("ok");
      if (url.pathname === "/log") console.error(body);
      else if (url.pathname === "/result") done(body);
    });
    return;
  }
  const [prefix, dir] = ROUTES.find(([p]) => url.pathname.startsWith(p));
  let file = normalize(join(dir, url.pathname.slice(prefix.length) || "index.html"));
  if (!file.startsWith(dir) || !existsSync(file) || statSync(file).isDirectory()) {
    res.writeHead(404).end();
    return;
  }
  // Cross-origin isolation only so performance.now() has 5 µs (not 100 µs) resolution.
  // Nothing here uses SharedArrayBuffer (snarkjs/ffjavascript workers copy buffers);
  // NO_ISOLATION=1 drops the headers to confirm that (an embedded iframe can't rely on them).
  const iso = NO_ISO ? {} : { "cross-origin-opener-policy": "same-origin", "cross-origin-embedder-policy": "require-corp" };
  res.writeHead(200, { "content-type": MIME[extname(file)] ?? "application/octet-stream", "cache-control": "no-store", ...iso });
  res.end(readFileSync(file));
});
await new Promise((r) => server.listen(0, "127.0.0.1", r));
const port = server.address().port;
const profile = mkdtempSync(join(tmpdir(), "lowki-bench-chrome-"));
const chrome = spawn(CHROME, [
  "--headless=new", "--enable-unsafe-webgpu", "--enable-features=WebGPU", "--ignore-gpu-blocklist",
  "--no-first-run", "--no-default-browser-check", "--disable-background-timer-throttling",
  "--disable-renderer-backgrounding", "--disable-backgrounding-occluded-windows",
  `--user-data-dir=${profile}`, `http://127.0.0.1:${port}/${process.env.PAGE ?? "index.html"}?suite=${suite}&iters=${iters}`,
], { stdio: ["ignore", "ignore", "pipe"] });
chrome.stderr.on("data", () => {});
const timeout = setTimeout(() => done(JSON.stringify({ suite, fatal: "timeout" })), 45 * 60 * 1000);
const body = await finished;
clearTimeout(timeout);
chrome.kill("SIGTERM");
server.close();
const parsed = JSON.parse(body);
parsed.chrome = process.env.CHROME_VERSION;
const outName = `browser-${suite}${NO_ISO ? "-noiso" : ""}.json`;
writeFileSync(join(HERE, "results", outName), JSON.stringify(parsed, null, 1));
console.error(`wrote results/${outName}${parsed.fatal ? " (FATAL: " + parsed.fatal + ")" : ""}`);
process.exit(parsed.fatal ? 1 : 0);
