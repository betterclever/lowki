// Static server for dist/ with the headers the page wants:
// - COOP/COEP (cross-origin isolation): finer performance.now() for the race timers. snarkjs's
//   web workers don't need it (they copy buffers, no SharedArrayBuffer) and the benchmarks
//   showed it changes no timing, but it costs nothing here since everything is same-origin.
// - A CSP that allows only this origin, so the page provably makes no third-party request.
// - Content-Length on every file, so the 18 MB zkey download can show progress.
import { createServer } from "node:http";
import { createReadStream, existsSync, statSync } from "node:fs";
import { dirname, extname, join, normalize } from "node:path";
import { fileURLToPath } from "node:url";

const HERE = dirname(fileURLToPath(import.meta.url));
const DIST = join(HERE, "dist");
const PORT = Number(process.env.PORT ?? 8840);
const HOST = process.env.HOST ?? "127.0.0.1";
const MIME = {
  ".html": "text/html; charset=utf-8", ".js": "text/javascript", ".mjs": "text/javascript", ".css": "text/css",
  ".wasm": "application/wasm", ".json": "application/json", ".vkey": "application/json", ".zkey": "application/octet-stream",
  ".svg": "image/svg+xml", ".png": "image/png", ".woff2": "font/woff2", ".txt": "text/plain; charset=utf-8",
};
const CSP = [
  "default-src 'self'",
  "script-src 'self' 'wasm-unsafe-eval' blob:", // blob: = snarkjs's web workers
  "worker-src 'self' blob:",
  "style-src 'self' 'unsafe-inline'",
  "img-src 'self' data: blob:",
  "font-src 'self'",
  "connect-src 'self'",
  "object-src 'none'",
  "base-uri 'none'",
  "frame-ancestors 'self'",
].join("; ");

createServer((req, res) => {
  const url = new URL(req.url, "http://x");
  let file = normalize(join(DIST, decodeURIComponent(url.pathname)));
  if (!file.startsWith(DIST)) return res.writeHead(403).end();
  if (existsSync(file) && statSync(file).isDirectory()) file = join(file, "index.html");
  if (!existsSync(file)) return res.writeHead(404, { "content-type": "text/plain" }).end("not found");
  const size = statSync(file).size;
  res.writeHead(200, {
    "content-type": MIME[extname(file)] ?? "application/octet-stream",
    "content-length": size,
    "cache-control": extname(file) === ".zkey" || extname(file) === ".wasm" ? "public, max-age=3600" : "no-cache",
    "cross-origin-opener-policy": "same-origin",
    "cross-origin-embedder-policy": "require-corp",
    "cross-origin-resource-policy": "same-origin",
    "content-security-policy": CSP,
    "referrer-policy": "no-referrer",
    "x-content-type-options": "nosniff",
  });
  if (req.method === "HEAD") return res.end();
  createReadStream(file).pipe(res);
}).listen(PORT, HOST, () => console.log(`lowki site: http://localhost:${PORT}/ (serving ${DIST})`));
