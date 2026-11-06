// Minimal W3C WebDriver client and static file server. No npm dependencies.
import http from 'node:http';
import fs from 'node:fs';
import path from 'node:path';
import { spawn } from 'node:child_process';

const EL = 'element-6066-11e4-a52e-4f735466cecf';
export const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const TYPES = { '.html': 'text/html', '.js': 'text/javascript', '.mjs': 'text/javascript', '.wasm': 'application/wasm', '.css': 'text/css',
  '.svg': 'image/svg+xml', '.woff2': 'font/woff2', '.png': 'image/png', '.json': 'application/json', '.map': 'application/json' };

export function serve(root, port) {
  root = path.resolve(root);
  const srv = http.createServer((req, res) => {
    let p = decodeURIComponent(new URL(req.url, 'http://x').pathname);
    if (p.endsWith('/')) p += 'index.html';
    const file = path.join(root, p);
    if (!file.startsWith(root) || !fs.existsSync(file) || fs.statSync(file).isDirectory()) { res.writeHead(404); res.end('not found'); return; }
    res.writeHead(200, { 'content-type': TYPES[path.extname(file)] || 'application/octet-stream', 'cache-control': 'no-store' });
    fs.createReadStream(file).pipe(res);
  });
  return new Promise((ok, bad) => { srv.once('error', bad); srv.listen(port, '127.0.0.1', () => ok(srv)); });
}

export async function startDriver(bin, port) {
  const proc = spawn(bin, [`--port=${port}`], { stdio: 'ignore' });
  const base = `http://127.0.0.1:${port}`;
  for (let i = 0; i < 100; i++) {
    try { const r = await fetch(base + '/status'); if (r.ok) break; } catch {}
    await sleep(100);
  }
  return { proc, base };
}

export class Browser {
  constructor(base) { this.base = base; }
  async cmd(method, p, body) {
    const r = await fetch(`${this.base}/session/${this.sid}${p}`, { method, headers: { 'content-type': 'application/json' }, body: body ? JSON.stringify(body) : undefined });
    const j = await r.json();
    if (j.value && j.value.error) throw new Error(`${p}: ${j.value.error}: ${j.value.message?.split('\n')[0]}`);
    return j.value;
  }
  async open() {
    const r = await fetch(`${this.base}/session`, { method: 'POST', headers: { 'content-type': 'application/json' },
      body: JSON.stringify({ capabilities: { alwaysMatch: { browserName: 'chrome', 'goog:loggingPrefs': { browser: 'ALL' }, 'goog:chromeOptions': {
        args: ['--headless=new', '--window-size=1280,900', '--disable-gpu', '--no-first-run', '--lang=en-US'] } } } }) });
    const j = await r.json();
    if (!j.value.sessionId) throw new Error('session: ' + JSON.stringify(j.value));
    this.sid = j.value.sessionId;
    return this;
  }
  close() { return fetch(`${this.base}/session/${this.sid}`, { method: 'DELETE' }).catch(() => {}); }
  go(url) { return this.cmd('POST', '/url', { url }); }
  exec(script, ...args) { return this.cmd('POST', '/execute/sync', { script, args }); }
  logs() { return this.cmd('POST', '/se/log', { type: 'browser' }).catch(() => []); }
  async find(css) { return (await this.cmd('POST', '/element', { using: 'css selector', value: css }))[EL]; }
  async click(css, timeout) { const e = await this.waitFor(css, { enabled: true, timeout }); await this.cmd('POST', `/element/${e}/click`, {}); }
  async type(css, text) { const e = await this.waitFor(css); await this.cmd('POST', `/element/${e}/value`, { text }); }
  async text(css) { return this.cmd('GET', `/element/${await this.find(css)}/text`); }
  async waitFor(css, { re, timeout = 15000, enabled = false } = {}) {
    const end = Date.now() + timeout;
    let last = '';
    while (Date.now() < end) {
      try {
        const e = await this.find(css);
        if (enabled && !((await this.cmd('GET', `/element/${e}/enabled`)) && (await this.cmd('GET', `/element/${e}/displayed`)))) { await sleep(60); continue; }
        if (!re) return e;
        last = await this.cmd('GET', `/element/${e}/text`);
        if (re.test(last)) return e;
      } catch {}
      await sleep(60);
    }
    throw new Error(`timeout waiting for ${css}${re ? ' ~ ' + re : ''} (last text: ${JSON.stringify(last)})`);
  }
  async until(fn, what, timeout = 15000) {
    const end = Date.now() + timeout;
    while (Date.now() < end) { if (await fn()) return; await sleep(60); }
    throw new Error('timeout: ' + what);
  }
  cdp(cmd, params = {}) { return this.cmd('POST', '/goog/cdp/execute', { cmd, params }); }
  handles() { return this.cmd('GET', '/window/handles'); }
  handle() { return this.cmd('GET', '/window'); }
  window(handle) { return this.cmd('POST', '/window', { handle }); }
  async frame(css) { const e = css ? await this.waitFor(css) : null; return this.cmd('POST', '/frame', { id: e ? { [EL]: e } : null }); }
}
