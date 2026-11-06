// The wallet home on LIVE Sepolia, real backend, headless Chrome with a CDP virtual
// authenticator (ctap2.1, UV, PRF):
//
//   1. sign up in app 1 (Game X, :8832) with email + passkey
//   2. use app 2 (:8833, no app_name: the wallet must show the server's registered name)
//   3. open the Lowki origin top level (:8831) -> "Open your wallet" -> passkey -> the home lists
//      both apps with the same addresses the apps got
//   4. claim test-<random>.lowki.eth live (sponsored registration + 7702 owner batch)
//   5. plain viem getEnsAddress on the public Sepolia RPC through the gateway: a fresh address
//      on every lookup
//   6. the operator key sends 0.0001 ETH to one resolved address
//   7. the home finds the payment
//
// lowki-server must run first: apps/wallet-ui/scripts/home-server.sh (:8830, behind the tunnel
// that is written on chain). This script serves dist/ on :8831 (with /v1 proxied to :8830 and a
// lowki-config.json) and the repo root (demo dapp) on :8832/:8833. Runs on the macOS host:
//
//   mac sh -lc 'cd <lowki>/apps/wallet-ui && node tests/e2e/home.mjs'
//
// Env: CHROME, LOWKI_SERVER, WALLET_DIST, SEPOLIA_RPC_URL, LOWKI_KEY_FILE, SHOTS=1 (screenshots
// into docs/ui-screens/), LABEL (default test-<random>), DRY=1 (stop before the live claim).
import http from "node:http";
import fs from "node:fs";
import path from "node:path";
import { spawn } from "node:child_process";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { randomBytes } from "node:crypto";
import { fileURLToPath, pathToFileURL } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const UI = path.resolve(here, "../..");
const REPO = path.resolve(UI, "../..");
const DIST = process.env.WALLET_DIST || path.join(UI, "dist");
const SERVER = process.env.LOWKI_SERVER || "http://localhost:8830";
const STATE = path.join(REPO, "target/wallet-home");
const CHROME = process.env.CHROME || "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";
const RPC = process.env.SEPOLIA_RPC_URL || "https://ethereum-sepolia-rpc.publicnode.com";
const KEY_FILE = process.env.LOWKI_KEY_FILE || "/Users/betterclever/.lowki/sepolia-test.env";
const WALLET = "http://localhost:8831";
const DAPP1 = "http://localhost:8832";
const DAPP2 = "http://localhost:8833";
const EMAIL = `home-${Date.now()}@lowki.test`;
const LABEL = process.env.LABEL || `test-${randomBytes(3).toString("hex")}`;
const SHOTS = process.env.SHOTS === "1";
const SHOT_DIR = path.join(REPO, "docs/ui-screens");

const viem = await import(pathToFileURL(path.join(REPO, "contracts/e2e/node_modules/viem/_esm/index.js")));
const { privateKeyToAccount } = await import(pathToFileURL(path.join(REPO, "contracts/e2e/node_modules/viem/_esm/accounts/index.js")));
const { sepolia } = await import(pathToFileURL(path.join(REPO, "contracts/e2e/node_modules/viem/_esm/chains/index.js")));

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const assert = (c, m) => { if (!c) throw new Error("assertion failed: " + m); };

// The wallet's runtime config: Sepolia over the public RPC (Anon-RPC from the browser), plus
// demo apps to suggest on the home.
const CONFIG = {
  chainId: 11155111,
  network: "Sepolia",
  rpcUrl: RPC,
  demoApps: [{ name: "Lowki Tips", url: "https://tips.lowki.money", blurb: "Tip creators by name, privately." }],
};

// ---- static servers ------------------------------------------------------------------------
const TYPES = { ".html": "text/html", ".js": "text/javascript", ".wasm": "application/wasm", ".css": "text/css",
  ".svg": "image/svg+xml", ".woff2": "font/woff2", ".png": "image/png", ".json": "application/json" };
function serve(root, port, proxy) {
  const srv = http.createServer((req, res) => {
    const url = new URL(req.url, "http://x");
    if (proxy && url.pathname.startsWith("/v1/")) {
      const up = new URL(req.url, proxy);
      const p = http.request(up, { method: req.method, headers: { ...req.headers, host: up.host } }, (r) => {
        res.writeHead(r.statusCode, r.headers);
        r.pipe(res);
      });
      p.on("error", () => { res.writeHead(502); res.end("{}"); });
      req.pipe(p);
      return;
    }
    if (proxy && url.pathname === "/lowki-config.json") {
      res.writeHead(200, { "content-type": "application/json", "cache-control": "no-store" });
      res.end(JSON.stringify(CONFIG));
      return;
    }
    let p = decodeURIComponent(url.pathname);
    if (p.endsWith("/")) p += "index.html";
    const file = path.join(root, p);
    if (!file.startsWith(root) || !fs.existsSync(file)) { res.writeHead(404); res.end("not found"); return; }
    res.writeHead(200, { "content-type": TYPES[path.extname(file)] || "application/octet-stream", "cache-control": "no-store" });
    fs.createReadStream(file).pipe(res);
  });
  return new Promise((ok) => srv.listen(port, "127.0.0.1", () => ok(srv)));
}

// ---- CDP ------------------------------------------------------------------------------------
const profile = mkdtempSync(path.join(tmpdir(), "lowki-home-e2e-"));
const chrome = spawn(CHROME, ["--headless=new", "--remote-debugging-port=0", `--user-data-dir=${profile}`,
  "--no-first-run", "--no-default-browser-check", "--window-size=1280,1100", "about:blank"],
  { stdio: ["ignore", "ignore", "pipe"] });
const wsUrl = await new Promise((resolve, reject) => {
  let buf = "";
  const t = setTimeout(() => reject(new Error("Chrome did not start")), 20000);
  chrome.stderr.on("data", (d) => { buf += d; const m = buf.match(/DevTools listening on (ws:\/\/\S+)/); if (m) { clearTimeout(t); resolve(m[1]); } });
});
const ws = new WebSocket(wsUrl);
await new Promise((r, j) => { ws.onopen = r; ws.onerror = j; });
let nextId = 1;
const pending = new Map();
const listeners = [];
ws.onmessage = (ev) => {
  const msg = JSON.parse(ev.data);
  if (msg.id && pending.has(msg.id)) {
    const { resolve, reject } = pending.get(msg.id);
    pending.delete(msg.id);
    msg.error ? reject(new Error(JSON.stringify(msg.error))) : resolve(msg.result);
  } else if (msg.method) listeners.forEach((l) => l(msg));
};
const send = (method, params = {}, sessionId) => new Promise((resolve, reject) => {
  const id = nextId++;
  pending.set(id, { resolve, reject });
  ws.send(JSON.stringify({ id, method, params, ...(sessionId ? { sessionId } : {}) }));
});

const servers = [await serve(DIST, 8831, SERVER), await serve(REPO, 8832), await serve(REPO, 8833)];
let failed = false;
const results = [];
const facts = { name: `${LABEL}.lowki.eth` };
async function step(name, fn) {
  const t = Date.now();
  try { await fn(); results.push(name); console.log(`PASS  ${name} (${Date.now() - t} ms)`); }
  catch (e) { console.log(`FAIL  ${name}\n      ${e.message}`); throw e; }
}

const consoleLines = [];
let screenText = async () => "";
try {
  const { targetId } = await send("Target.createTarget", { url: "about:blank" });
  const { sessionId } = await send("Target.attachToTarget", { targetId, flatten: true });
  const s = (m, p) => send(m, p, sessionId);
  listeners.push((m) => {
    if (m.method === "Runtime.consoleAPICalled") consoleLines.push(m.params.args.map((a) => a.value ?? a.description).join(" "));
    if (m.method === "Runtime.exceptionThrown") consoleLines.push("exception: " + JSON.stringify(m.params.exceptionDetails.exception?.description));
  });
  await s("Page.enable");
  await s("Runtime.enable");
  await s("WebAuthn.enable", { enableUI: false });
  const authOpts = { protocol: "ctap2", ctap2Version: "ctap2_1", transport: "internal", hasResidentKey: true,
    hasUserVerification: true, isUserVerified: true, hasPrf: true, automaticPresenceSimulation: true };
  const { authenticatorId } = await s("WebAuthn.addVirtualAuthenticator", { options: authOpts });

  const navigate = async (url) => {
    const loaded = new Promise((r) => { const l = (m) => { if (m.method === "Page.loadEventFired" && m.sessionId === sessionId) { listeners.splice(listeners.indexOf(l), 1); r(); } }; listeners.push(l); });
    await s("Page.navigate", { url });
    await loaded;
  };
  const evalIn = async (contextId, expression) => {
    const r = await s("Runtime.evaluate", { expression, awaitPromise: true, returnByValue: true, userGesture: true, ...(contextId ? { contextId } : {}) });
    if (r.exceptionDetails) throw new Error(r.exceptionDetails.exception?.description || JSON.stringify(r.exceptionDetails));
    return r.result.value;
  };
  const top = (expr) => evalIn(null, expr);
  let inFrame = true;
  const frameCtx = async () => {
    const { frameTree } = await s("Page.getFrameTree");
    const f = (frameTree.childFrames || []).find((c) => c.frame.url.startsWith(WALLET));
    if (!f) throw new Error("wallet frame not found");
    const { executionContextId } = await s("Page.createIsolatedWorld", { frameId: f.frame.id, worldName: "e2e" });
    return executionContextId;
  };
  const wallet = async (expr) => (inFrame ? evalIn(await frameCtx(), expr) : top(expr));
  screenText = () => wallet(`document.querySelector('#main')?.innerText || ''`);
  const waitFor = async (fn, what, timeout = 20000) => {
    const end = Date.now() + timeout;
    let last;
    while (Date.now() < end) {
      try { const v = await fn(); if (v) return v; last = v; } catch (e) { last = e.message; }
      await sleep(150);
    }
    throw new Error(`timeout waiting for ${what} (last: ${JSON.stringify(last)})`);
  };
  const h1 = () => wallet(`document.querySelector('#main h1')?.textContent || ''`);
  const waitH1 = (re, timeout) => waitFor(async () => re.test(await h1()) && (await h1()), `wallet heading ~ ${re}`, timeout);
  const click = (test) => wallet(`(() => { const e = document.querySelector('[data-test="${test}"]'); if (!e) throw new Error('no ${test}'); e.click(); return true; })()`);
  const type = (test, value) => wallet(`(() => { const e = document.querySelector('[data-test="${test}"]'); e.value = ${JSON.stringify(value)}; e.dispatchEvent(new Event('input', { bubbles: true })); return true; })()`);
  const dappField = (id) => top(`document.getElementById(${JSON.stringify(id)}).textContent`);
  const waitDapp = (id, re) => waitFor(async () => { const v = await dappField(id); return re.test(v) && v; }, `dapp #${id} ~ ${re}`, 30000);
  const apps = JSON.parse(fs.readFileSync(path.join(STATE, "apps.json"), "utf8"));
  const dappUrl = (origin, withName) => `${origin}/apps/demo-dapp/?wallet=${encodeURIComponent(WALLET + "/")}&app=${apps[origin].id}` +
    (withName ? `&app_name=${encodeURIComponent(apps[origin].name)}` : "") + "&walletflags=platform%3Dmac";
  const LOG = path.join(STATE, "server.log");
  const emailCode = async (since) => waitFor(() => {
    const log = fs.readFileSync(LOG, "utf8").slice(since);
    const m = [...log.matchAll(new RegExp(`email sign-in code for ${EMAIL.replace(/[.+]/g, "\\$&")}: (\\d{6})`, "g"))];
    return m.length ? m[m.length - 1][1] : null;
  }, "email code in the server log");
  const shot = async (name, theme) => {
    if (!SHOTS) return;
    await s("Emulation.setEmulatedMedia", { features: [{ name: "prefers-color-scheme", value: theme }] });
    await sleep(400);
    for (const [w, h0] of [[390, 844], [1280, 900]]) {
      await s("Emulation.setDeviceMetricsOverride", { width: w, height: h0, deviceScaleFactor: w < 500 ? 2 : 1, mobile: w < 500 });
      await sleep(300);
      const h = Math.max(h0, await top("Math.ceil(document.documentElement.scrollHeight)"));
      await s("Emulation.setDeviceMetricsOverride", { width: w, height: h, deviceScaleFactor: w < 500 ? 2 : 1, mobile: w < 500 });
      await sleep(300);
      const { data } = await s("Page.captureScreenshot", { format: "png", captureBeyondViewport: false });
      fs.writeFileSync(path.join(SHOT_DIR, `${name}--${theme}--${w}.png`), Buffer.from(data, "base64"));
    }
    await s("Emulation.clearDeviceMetricsOverride");
    await s("Emulation.setEmulatedMedia", { features: [] });
  };

  await step("1. sign up in Game X (:8832): email code -> passkey -> the app gets its address", async () => {
    await navigate(dappUrl(DAPP1, true));
    await waitH1(/Continue to Game X/);
    await top(`document.getElementById('signin').click()`);
    await click("email-open");
    await type("email-input", EMAIL);
    const since = fs.statSync(LOG).size;
    await click("email-continue");
    await waitH1(/Check your email/);
    await type("email-code", await emailCode(since));
    await click("email-verify");
    await waitH1(/Secure your wallet/);
    await click("create-passkey");
    await waitH1(/Save your recovery code/, 30000);
    await click("recovery-done");
    facts.app1 = await waitDapp("address", /^0x[0-9a-f]{40}$/);
    console.log(`      Game X address ${facts.app1}`);
  });

  await step("2. Pixel Market (:8833, no app_name): the wallet shows the registered name; passkey -> its own address", async () => {
    await navigate(dappUrl(DAPP2, false));
    await waitH1(/Continue to Pixel Market/);
    const origin = await wallet(`document.querySelector('.hero .origin')?.textContent`);
    assert(origin === "localhost:8833" || origin === "http://localhost:8833", "origin under the name: " + origin);
    await click("passkey-signin");
    facts.app2 = await waitDapp("address", /^0x[0-9a-f]{40}$/);
    assert(facts.app2 !== facts.app1, "a different address per app");
    console.log(`      Pixel Market address ${facts.app2}`);
  });

  await step("3. open the Lowki origin top level -> passkey -> the home lists both apps with their addresses", async () => {
    inFrame = false;
    await navigate(WALLET + "/");
    await waitH1(/Open your wallet/);
    await click("passkey-signin");
    await waitH1(/Your wallet/, 30000);
    const rows = await waitFor(async () => {
      const r = await wallet(`[...document.querySelectorAll('[data-test="home-app"]')].map((e) => ({ origin: e.dataset.origin, address: e.dataset.address, text: e.innerText }))`);
      return r.length >= 2 && r;
    }, "two apps on the home", 60000);
    const g = rows.find((r) => r.origin === DAPP1);
    const p = rows.find((r) => r.origin === DAPP2);
    assert(g && g.address === facts.app1, `Game X listed with ${facts.app1}: ${JSON.stringify(g)}`);
    assert(p && p.address === facts.app2, `Pixel Market listed with ${facts.app2}: ${JSON.stringify(p)}`);
    assert(/Game X/.test(g.text) && /Pixel Market/.test(p.text), "names shown");
    await waitFor(async () => !(await wallet(`document.body.innerText`)).includes("Loading"), "balances", 30000).catch(() => {});
    console.log(`      home: ${rows.map((r) => `${r.text.split("\n")[0]} ${r.address}`).join(", ")}`);
    await shot("36-wallet-home-live", "light");
    await shot("36-wallet-home-live", "dark");
  });

  if (process.env.DRY === "1") throw new Error("DRY=1: stopping before the live claim");
  await step("0. the gateway behind the on-chain URL answers", async () => {
    const gw = JSON.parse(fs.readFileSync(path.join(REPO, "contracts/deployments/sepolia-lowki.json"), "utf8")).gatewayUrl;
    const r = await fetch(new URL("/healthz", gw));
    assert(r.ok, "tunnel -> lowki-server :8830");
  });
  await step(`4. claim ${facts.name} live (sponsored registration + owner batch)`, async () => {
    await waitFor(() => wallet(`!!document.querySelector('[data-test="claim-label"]')`), "claim form", 30000);
    await type("claim-label", LABEL);
    const btn = await wallet(`document.querySelector('[data-test="claim"]').textContent`);
    assert(btn.includes(`Claim ${LABEL}.lowki.eth`), "button names the claim: " + btn);
    await click("claim");
    await waitFor(() => wallet(`!!document.querySelector('[data-test="claim-progress"]')`), "claim progress");
    const name = await waitFor(() => wallet(`document.querySelector('[data-test="my-name"]')?.textContent`), "the claimed name", 180000);
    assert(name === facts.name, "claimed " + name);
    const me = await top(`fetch('/v1/names/me').then((r) => r.json())`);
    facts.register_tx = me.register_tx;
    facts.setup_tx = me.setup_tx;
    assert(me.status === "ready", "server status ready");
    console.log(`      register tx ${me.register_tx}\n      setup tx    ${me.setup_tx}`);
  });

  const client = viem.createPublicClient({ chain: sepolia, transport: viem.http(RPC) });
  await step("5. plain viem getEnsAddress on public Sepolia: a fresh address per lookup", async () => {
    facts.lookups = [];
    await sleep(10000); // let public RPC nodes see the setup block
    // Public infrastructure (the RPC node catching up with the setup block, the quick tunnel)
    // can fail a lookup transiently: retry each one a few times and say so.
    const lookup = async () => {
      for (let t = 1; ; t++) {
        try { return await client.getEnsAddress({ name: facts.name }); }
        catch (e) {
          if (t >= 6) throw e;
          console.log(`      lookup attempt ${t} failed (${e.shortMessage || e.message}); retrying in 5 s`);
          await sleep(5000);
        }
      }
    };
    for (let i = 0; i < 3; i++) {
      const a = await lookup();
      assert(a && a !== viem.zeroAddress, "resolved");
      facts.lookups.push(a);
      console.log(`      lookup ${i + 1}: ${a}`);
    }
    assert(new Set(facts.lookups.map((a) => a.toLowerCase())).size === 3, "three different addresses");
  });

  await step("6. the operator sends 0.0001 ETH to the first resolved address", async () => {
    const env = Object.fromEntries(fs.readFileSync(KEY_FILE, "utf8").split("\n").map((l) => l.trim()).filter((l) => l && !l.startsWith("#") && l.includes("="))
      .map((l) => { const i = l.indexOf("="); return [l.slice(0, i).replace(/^export\s+/, ""), l.slice(i + 1).replace(/^["']|["']$/g, "")]; }));
    const key = env.LOWKI_SEPOLIA_KEY;
    assert(/^(0x)?[0-9a-fA-F]{64}$/.test(key || ""), "operator key present");
    const account = privateKeyToAccount(key.startsWith("0x") ? key : `0x${key}`);
    const w = viem.createWalletClient({ account, chain: sepolia, transport: viem.http(RPC) });
    const hash = await w.sendTransaction({ to: facts.lookups[0], value: viem.parseEther("0.0001") });
    const r = await client.waitForTransactionReceipt({ hash, timeout: 180000 });
    assert(r.status === "success", "payment mined");
    facts.payment_tx = hash;
    console.log(`      payment tx ${hash} -> ${facts.lookups[0]}`);
  });

  await step("7. the home finds the payment at its stealth address", async () => {
    const want = facts.lookups[0].toLowerCase();
    const row = await waitFor(async () => {
      const r = await wallet(`(() => { const e = document.querySelector('[data-test="payment"][data-address="${want}"]'); return e ? e.innerText : null; })()`);
      if (r) return r;
      await wallet(`(() => { const b = document.querySelector('[data-test="payments-refresh"]'); if (b && !b.disabled) b.click(); return true; })()`);
      await sleep(4000);
      return null;
    }, "the payment row", 120000);
    assert(/0\.0001 ETH/.test(row), "shows 0.0001 ETH: " + row);
    assert(/Found/.test(row), "below the pool minimum: shown as found");
    const others = await wallet(`[...document.querySelectorAll('[data-test="payment"]')].map((e) => e.dataset.address)`);
    assert(others.length === 1, "unpaid lookups are not listed: " + others);
    console.log(`      found: ${row.replace(/\n+/g, " | ")}`);
    await shot("37-wallet-home-live-name", "light");
    await shot("37-wallet-home-live-name", "dark");
  });
} catch (e) {
  failed = true;
  try { console.log("screen:\n  " + (await screenText()).replace(/\n+/g, " | ")); } catch {}
  if (consoleLines.length) console.log("browser console:\n  " + consoleLines.slice(-30).join("\n  "));
} finally {
  ws.close();
  try { chrome.kill("SIGKILL"); } catch {}
  try { rmSync(profile, { recursive: true, force: true }); } catch {}
  servers.forEach((x) => x.close());
}
console.log(JSON.stringify(facts, null, 2));
console.log(failed ? `FAIL (${results.length} steps passed)` : `PASS all ${results.length} steps`);
process.exit(failed ? 1 : 0);
