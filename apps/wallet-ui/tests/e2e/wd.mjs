// Minimal W3C WebDriver client + static file server. No npm dependencies.
import http from "node:http";
import fs from "node:fs";
import path from "node:path";
import { spawn } from "node:child_process";

const EL = "element-6066-11e4-a52e-4f735466cecf";
const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
export { sleep };

const TYPES = { ".html": "text/html", ".js": "text/javascript", ".wasm": "application/wasm", ".css": "text/css",
  ".svg": "image/svg+xml", ".woff2": "font/woff2", ".png": "image/png", ".txt": "text/plain" };

/** Serve `root` on `port`. Returns the server. */
export function serve(root, port) {
  const srv = http.createServer((req, res) => {
    let p = decodeURIComponent(new URL(req.url, "http://x").pathname);
    if (p.endsWith("/")) p += "index.html";
    const file = path.join(root, p);
    if (!file.startsWith(root) || !fs.existsSync(file)) { res.writeHead(404); res.end("not found"); return; }
    res.writeHead(200, { "content-type": TYPES[path.extname(file)] || "application/octet-stream", "cache-control": "no-store" });
    fs.createReadStream(file).pipe(res);
  });
  return new Promise((ok) => srv.listen(port, "127.0.0.1", () => ok(srv)));
}

export async function startDriver(bin, port = 9617) {
  const proc = spawn(bin, [`--port=${port}`], { stdio: "ignore" });
  const base = `http://127.0.0.1:${port}`;
  for (let i = 0; i < 50; i++) {
    try { const r = await fetch(base + "/status"); if (r.ok) break; } catch {}
    await sleep(100);
  }
  return { proc, base };
}

export class Browser {
  constructor(base) { this.base = base; }
  async cmd(method, p, body) {
    const r = await fetch(`${this.base}/session/${this.sid}${p}`, {
      method, headers: { "content-type": "application/json" }, body: body ? JSON.stringify(body) : undefined,
    });
    const j = await r.json();
    if (j.value && j.value.error) throw new Error(`${p}: ${j.value.error}: ${j.value.message?.split("\n")[0]}`);
    return j.value;
  }
  async open() {
    const r = await fetch(`${this.base}/session`, { method: "POST", headers: { "content-type": "application/json" },
      body: JSON.stringify({ capabilities: { alwaysMatch: { browserName: "chrome", "goog:chromeOptions": {
        args: ["--headless=new", "--hide-scrollbars", "--window-size=1280,900", "--disable-gpu", "--no-first-run",
               "--disable-features=Translate,OptimizationHints", "--lang=en-US"] } } } }) });
    const j = await r.json();
    if (!j.value.sessionId) throw new Error("session: " + JSON.stringify(j.value));
    this.sid = j.value.sessionId;
    return this;
  }
  close() { return fetch(`${this.base}/session/${this.sid}`, { method: "DELETE" }).catch(() => {}); }
  cdp(cmd, params = {}) { return this.cmd("POST", "/goog/cdp/execute", { cmd, params }); }
  go(url) { return this.cmd("POST", "/url", { url }); }
  exec(script, ...args) { return this.cmd("POST", "/execute/sync", { script, args }); }
  async find(css) { const v = await this.cmd("POST", "/element", { using: "css selector", value: css }); return v[EL]; }
  async findAll(css) { return (await this.cmd("POST", "/elements", { using: "css selector", value: css })).map((v) => v[EL]); }
  async click(css) { const e = await this.waitFor(css); await this.cmd("POST", `/element/${e}/click`, {}); }
  async type(css, text) { const e = await this.waitFor(css); await this.cmd("POST", `/element/${e}/value`, { text }); }
  async text(css) { const e = await this.find(css); return this.cmd("GET", `/element/${e}/text`); }
  async attr(css, name) { const e = await this.find(css); return this.cmd("GET", `/element/${e}/attribute/${name}`); }
  /** Wait until `css` exists (and, when given, its text matches `re`). */
  async waitFor(css, { re, timeout = 10000, enabled = false } = {}) {
    const end = Date.now() + timeout;
    let last = "";
    while (Date.now() < end) {
      try {
        const e = await this.find(css);
        if (enabled && !(await this.cmd("GET", `/element/${e}/enabled`))) { await sleep(60); continue; }
        if (!re) return e;
        last = await this.cmd("GET", `/element/${e}/text`);
        if (re.test(last)) return e;
      } catch {}
      await sleep(60);
    }
    throw new Error(`timeout waiting for ${css}${re ? " ~ " + re : ""} (last text: ${JSON.stringify(last)})`);
  }
  async waitGone(css, timeout = 10000) {
    const end = Date.now() + timeout;
    while (Date.now() < end) { if ((await this.findAll(css)).length === 0) return; await sleep(60); }
    throw new Error("still present: " + css);
  }
  async frame(css) { const e = css ? await this.waitFor(css) : null; return this.cmd("POST", "/frame", { id: e ? { [EL]: e } : null }); }
  async viewport(width, height, mobile) {
    await this.cdp("Emulation.setDeviceMetricsOverride", { width, height, deviceScaleFactor: mobile ? 2 : 1, mobile });
  }
  theme(dark) { return this.cdp("Emulation.setEmulatedMedia", { features: [{ name: "prefers-color-scheme", value: dark ? "dark" : "light" }] }); }
  /** Full-page PNG (base64). */
  async shot(width, minHeight, mobile) {
    const h = await this.exec("return Math.ceil(document.documentElement.scrollHeight)");
    const height = Math.max(minHeight, h);
    await this.viewport(width, height, mobile);
    await sleep(120);
    const r = await this.cdp("Page.captureScreenshot", { format: "png", captureBeyondViewport: false });
    await this.viewport(width, minHeight, mobile);
    return r.data;
  }
}
