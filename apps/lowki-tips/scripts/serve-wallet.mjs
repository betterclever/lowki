// Serves a wallet-ui build for the Lowki Tips demo, like a production reverse proxy would:
//   /v1/*               -> lowki-server (same origin: its cookie API has no CORS)
//   /rpc                -> the fork's JSON-RPC (same origin, so the wallet's CSP 'self' allows it)
//   /lowki-config.json  -> the wallet's runtime chain config (file given in env)
//   everything else     -> the static build
//   /circuits/*, /pp-snapshot.json -> ASSETS_UPSTREAM (the demo stack's wallet server), if set
// No dependencies. Env: PORT, DIST, SERVER (lowki-server base), FORK_RPC, WALLET_CONFIG, ASSETS_UPSTREAM.
import http from 'node:http';
import fs from 'node:fs';
import path from 'node:path';

const PORT = Number(process.env.PORT ?? 8822);
const DIST = path.resolve(process.env.DIST ?? '.');
const SERVER = process.env.SERVER ?? 'http://127.0.0.1:8821';
const FORK_RPC = process.env.FORK_RPC ?? 'http://127.0.0.1:8820';
const WALLET_CONFIG = process.env.WALLET_CONFIG;
const ASSETS_UPSTREAM = process.env.ASSETS_UPSTREAM;
const TYPES = {
  '.html': 'text/html', '.js': 'text/javascript', '.mjs': 'text/javascript', '.wasm': 'application/wasm',
  '.css': 'text/css', '.svg': 'image/svg+xml', '.woff2': 'font/woff2', '.png': 'image/png', '.json': 'application/json',
  '.zkey': 'application/octet-stream',
};

function proxy(req, res, target) {
  const up = new URL(target);
  const p = http.request(up, { method: req.method, headers: { ...req.headers, host: up.host } }, (r) => {
    res.writeHead(r.statusCode ?? 502, r.headers);
    r.pipe(res);
  });
  p.on('error', () => {
    res.writeHead(502, { 'content-type': 'application/json' });
    res.end('{"error":"upstream unavailable"}');
  });
  req.pipe(p);
}

http
  .createServer((req, res) => {
    const url = new URL(req.url ?? '/', 'http://x');
    if (url.pathname.startsWith('/v1/')) return proxy(req, res, new URL(req.url, SERVER).toString());
    if (url.pathname === '/rpc') return proxy(req, res, FORK_RPC);
    if (ASSETS_UPSTREAM && (url.pathname.startsWith('/circuits/') || url.pathname === '/pp-snapshot.json')) {
      return proxy(req, res, new URL(req.url, ASSETS_UPSTREAM).toString());
    }
    if (url.pathname === '/lowki-config.json' && WALLET_CONFIG) {
      res.writeHead(200, { 'content-type': 'application/json', 'cache-control': 'no-store' });
      return void res.end(fs.readFileSync(WALLET_CONFIG));
    }
    let p = decodeURIComponent(url.pathname);
    if (p.endsWith('/')) p += 'index.html';
    const file = path.join(DIST, p);
    if (!file.startsWith(DIST) || !fs.existsSync(file) || fs.statSync(file).isDirectory()) {
      res.writeHead(404);
      return void res.end('not found');
    }
    res.writeHead(200, { 'content-type': TYPES[path.extname(file)] ?? 'application/octet-stream', 'cache-control': 'no-store' });
    fs.createReadStream(file).pipe(res);
  })
  .listen(PORT, '127.0.0.1', () => console.log(`wallet on http://localhost:${PORT} (dist ${DIST})`));
