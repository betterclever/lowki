// Static file server with same-origin proxies, for running the example against the real
// wallet: the wallet's dist with `/v1/` -> lowki-server (its cookie API has no CORS, so the
// wallet calls it same-origin, like trunk's [[proxy]] or a production reverse proxy) and
// `/rpc` -> a JSON-RPC node (the wallet's CSP allows 'self', so a wallet built with
// LOWKI_RPC_URL=http://localhost:8800/rpc can use a local anvil).
//
//   node e2e/serve.mjs <root> <port> [--v1 http://localhost:8801] [--rpc http://localhost:8803] [--spa]
//
// Also imported by e2e/real.mjs. Binds 127.0.0.1 only. Needs Node >= 22.
import http from 'node:http';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const TYPES = {
  '.html': 'text/html', '.js': 'text/javascript', '.mjs': 'text/javascript', '.wasm': 'application/wasm', '.css': 'text/css',
  '.svg': 'image/svg+xml', '.woff2': 'font/woff2', '.png': 'image/png', '.json': 'application/json', '.ico': 'image/x-icon',
};

function proxy(req, res, base, rewrite = (p) => p) {
  const up = new URL(rewrite(req.url), base);
  const p = http.request(up, { method: req.method, headers: { ...req.headers, host: up.host } }, (r) => {
    res.writeHead(r.statusCode, r.headers);
    r.pipe(res);
  });
  p.on('error', () => { res.writeHead(502, { 'content-type': 'application/json' }); res.end('{"error":"upstream down"}'); });
  req.pipe(p);
}

/** Serves `root` on 127.0.0.1:`port`. `v1`/`rpc`: proxy targets. `spa`: unknown paths get index.html. */
export function serve(root, port, { v1, rpc, spa = false } = {}) {
  root = path.resolve(root);
  const srv = http.createServer((req, res) => {
    const url = new URL(req.url, 'http://x');
    if (v1 && url.pathname.startsWith('/v1/')) return proxy(req, res, v1);
    if (rpc && url.pathname === '/rpc') return proxy(req, res, rpc, () => '/');
    let p = decodeURIComponent(url.pathname);
    if (p.endsWith('/')) p += 'index.html';
    let file = path.join(root, p);
    if (!file.startsWith(root)) { res.writeHead(403); res.end(); return; }
    if (!fs.existsSync(file) || fs.statSync(file).isDirectory()) {
      if (!spa) { res.writeHead(404); res.end('not found'); return; }
      file = path.join(root, 'index.html');
    }
    res.writeHead(200, { 'content-type': TYPES[path.extname(file)] || 'application/octet-stream', 'cache-control': 'no-store' });
    fs.createReadStream(file).pipe(res);
  });
  return new Promise((ok, fail) => { srv.once('error', fail); srv.listen(port, '127.0.0.1', () => ok(srv)); });
}

if (process.argv[1] === fileURLToPath(import.meta.url)) {
  const [root, port, ...rest] = process.argv.slice(2);
  const opt = (k) => { const i = rest.indexOf(k); return i >= 0 ? rest[i + 1] : undefined; };
  if (!root || !port) { console.error('usage: node e2e/serve.mjs <root> <port> [--v1 url] [--rpc url] [--spa]'); process.exit(2); }
  await serve(root, Number(port), { v1: opt('--v1'), rpc: opt('--rpc'), spa: rest.includes('--spa') });
  console.log(`serving ${root} on http://localhost:${port}${opt('--v1') ? ` (/v1/ -> ${opt('--v1')})` : ''}${opt('--rpc') ? ` (/rpc -> ${opt('--rpc')})` : ''}`);
}
