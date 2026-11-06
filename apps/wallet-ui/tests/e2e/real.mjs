// End-to-end run of the wallet UI on the REAL backend (lowki-worker + lowki-server), driven
// through the demo dapp in headless Chrome with a CDP virtual authenticator (ctap2.1, UV, PRF).
//
//   1. sign up with email (code read from the server log) -> passkey -> the dapp gets the
//      address, pairwise ID and a JWT, verified against the server's JWKS (jose)
//   2. reload -> passkey login -> same address and ID
//   3. second dapp origin -> different address and ID, same passkey
//   4. signMessage from the dapp -> the signature recovers to the app address (viem)
//   5. lose everything (storage cleared, passkey gone) -> email + recovery code -> same
//      address -> the new passkey unlocks after a reload
//
// Runs on the macOS host. lowki-server must already run (tests/e2e/real-server.sh, in the VM
// or here) on localhost:8787; this script serves the wallet dist on :8740 (proxying /v1/ to
// the server) and the repo root (demo dapp) on :8741 and :8742.
//
//   node tests/e2e/real.mjs        (after `trunk build --release`)
//
// Env: CHROME, LOWKI_SERVER (http://localhost:8787), WALLET_DIST. Needs Node >= 22.
import http from "node:http";
import fs from "node:fs";
import path from "node:path";
import { spawn } from "node:child_process";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { fileURLToPath, pathToFileURL } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const UI = path.resolve(here, "../..");
const REPO = path.resolve(UI, "../..");
const DIST = process.env.WALLET_DIST || path.join(UI, "dist");
const SERVER = process.env.LOWKI_SERVER || "http://localhost:8787";
const STATE = path.join(REPO, "target/wallet-ui-e2e");
const CHROME = process.env.CHROME || "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";
const WALLET = "http://localhost:8740";
const DAPP1 = "http://localhost:8741";
const DAPP2 = "http://localhost:8742";
const EMAIL = `e2e-${Date.now()}@lowki.test`;

const { createRemoteJWKSet, jwtVerify } = await import(pathToFileURL(path.join(REPO, "crates/lowki-server/tests/node/node_modules/jose/dist/webapi/index.js")));
const { recoverMessageAddress } = await import(pathToFileURL(path.join(REPO, "vectors/lowki-worker/node_modules/viem/_esm/index.js")));

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const assert = (c, m) => { if (!c) throw new Error("assertion failed: " + m); };

// ---- static servers ------------------------------------------------------------------------
const TYPES = { ".html": "text/html", ".js": "text/javascript", ".wasm": "application/wasm", ".css": "text/css",
  ".svg": "image/svg+xml", ".woff2": "font/woff2", ".png": "image/png", ".json": "application/json" };
function serve(root, port, proxy) {
  const srv = http.createServer((req, res) => {
    const url = new URL(req.url, "http://x");
    if (proxy && url.pathname.startsWith("/v1/")) {
      // Same-origin proxy to lowki-server, like trunk's [[proxy]] or a production reverse proxy.
      const up = new URL(req.url, proxy);
      const p = http.request(up, { method: req.method, headers: { ...req.headers, host: up.host } }, (r) => {
        res.writeHead(r.statusCode, r.headers);
        r.pipe(res);
      });
      p.on("error", () => { res.writeHead(502); res.end("{}"); });
      req.pipe(p);
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
const profile = mkdtempSync(path.join(tmpdir(), "lowki-wallet-e2e-"));
const chrome = spawn(CHROME, ["--headless=new", "--remote-debugging-port=0", `--user-data-dir=${profile}`,
  "--no-first-run", "--no-default-browser-check", "--disable-gpu", "--window-size=1280,1000", "about:blank"],
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

const servers = [await serve(DIST, 8740, SERVER), await serve(REPO, 8741), await serve(REPO, 8742)];
let failed = false;
const results = [];
async function step(name, fn) {
  const t = Date.now();
  try { await fn(); results.push(name); console.log(`PASS  ${name} (${Date.now() - t} ms)`); }
  catch (e) { console.log(`FAIL  ${name}\n      ${e.message}`); throw e; }
}

const consoleLines = [];
let walletText = async () => "";
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
  let { authenticatorId } = await s("WebAuthn.addVirtualAuthenticator", { options: authOpts });

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
  // The wallet iframe (same site, so same process): an isolated world shares its DOM.
  const frameCtx = async () => {
    const { frameTree } = await s("Page.getFrameTree");
    const f = (frameTree.childFrames || []).find((c) => c.frame.url.startsWith(WALLET));
    if (!f) throw new Error("wallet frame not found");
    const { executionContextId } = await s("Page.createIsolatedWorld", { frameId: f.frame.id, worldName: "e2e" });
    return executionContextId;
  };
  const wallet = async (expr) => evalIn(await frameCtx(), expr);
  walletText = () => wallet(`document.querySelector('#main')?.innerText || ''`);
  const waitFor = async (fn, what, timeout = 20000) => {
    const end = Date.now() + timeout;
    let last;
    while (Date.now() < end) {
      try { const v = await fn(); if (v) return v; last = v; } catch (e) { last = e.message; }
      await sleep(100);
    }
    throw new Error(`timeout waiting for ${what} (last: ${JSON.stringify(last)})`);
  };
  const h1 = () => wallet(`document.querySelector('#main h1')?.textContent || ''`);
  const waitH1 = (re, timeout) => waitFor(async () => re.test(await h1()) && (await h1()), `wallet heading ~ ${re}`, timeout);
  const click = (test) => wallet(`(() => { const e = document.querySelector('[data-test="${test}"]'); if (!e) throw new Error('no ${test}'); e.click(); return true; })()`);
  const type = (test, value) => wallet(`(() => { const e = document.querySelector('[data-test="${test}"]'); e.value = ${JSON.stringify(value)}; e.dispatchEvent(new Event('input', { bubbles: true })); return true; })()`);
  const dappField = (id) => top(`document.getElementById(${JSON.stringify(id)}).textContent`);
  const waitDapp = (id, re) => waitFor(async () => { const v = await dappField(id); return re.test(v) && v; }, `dapp #${id} ~ ${re}`);
  const apps = JSON.parse(fs.readFileSync(path.join(STATE, "apps.json"), "utf8"));
  const dappUrl = (origin) => `${origin}/apps/demo-dapp/?wallet=${encodeURIComponent(WALLET + "/")}&app=${apps[origin].id}&app_name=${encodeURIComponent(apps[origin].name)}&walletflags=platform%3Dmac`;
  const emailCode = async (since) => waitFor(() => {
    const log = fs.readFileSync(path.join(STATE, "server.log"), "utf8").slice(since);
    const m = [...log.matchAll(new RegExp(`email sign-in code for ${EMAIL.replace(/[.+]/g, "\\$&")}: (\\d{6})`, "g"))];
    return m.length ? m[m.length - 1][1] : null;
  }, "email code in the server log");
  const logSize = () => fs.statSync(path.join(STATE, "server.log")).size;
  const jwks = createRemoteJWKSet(new URL(`${SERVER}/.well-known/jwks.json`));
  const verifyToken = async (token, aud) => (await jwtVerify(token, jwks, { issuer: SERVER, audience: aud, algorithms: ["ES256"] })).payload;
  const emailSignIn = async () => {
    await click("email-open");
    await type("email-input", EMAIL);
    const since = logSize();
    await click("email-continue");
    await waitH1(/Check your email/);
    await type("email-code", await emailCode(since));
    await click("email-verify");
  };

  let first = {};
  await step("1. sign up: email code -> passkey -> dapp gets address, pairwise ID and a JWT that verifies against the JWKS", async () => {
    await navigate(dappUrl(DAPP1));
    await waitH1(/Continue to Game X/);
    const google = await wallet(`!!document.querySelector('[data-test="google"]')`);
    assert(!google, "Google button hidden with the real backend");
    await top(`document.getElementById('signin').click()`);
    await emailSignIn();
    await waitH1(/Secure your wallet/);
    await click("create-passkey");
    await waitH1(/Save your recovery code/, 30000);
    const shot = await s("Page.captureScreenshot", { format: "png" });
    fs.writeFileSync(path.join(STATE, "recovery-code.png"), Buffer.from(shot.data, "base64"));
    first.code = await wallet(`[...document.querySelectorAll('[data-test="recovery-code"] li .mono')].map((e) => e.textContent).join('-')`);
    assert(/^([0-9A-Z]{4}-){13}[0-9A-Z]{4}$/.test(first.code), "recovery code is 14 groups of 4: " + first.code);
    await click("recovery-done");
    first.address = await waitDapp("address", /^0x[0-9a-f]{40}$/);
    first.userId = await dappField("userid");
    first.token = await dappField("token");
    const payload = await verifyToken(first.token, apps[DAPP1].id);
    assert(payload.sub === first.userId, `JWT sub ${payload.sub} == user_id ${first.userId}`);
    assert(/^did:lowki:[0-9a-f]{32}$/.test(first.userId), "pairwise ID format");
    await waitH1(/Your Game X wallet/);
    const chip = await wallet(`document.querySelector('.account .chip-addr')?.getAttribute('title')`);
    assert(chip === first.address, `wallet shows the same address (${chip})`);
    const { credentials } = await s("WebAuthn.getCredentials", { authenticatorId });
    assert(credentials.length === 1 && credentials[0].rpId === "localhost", "one resident passkey for RP ID localhost");
    console.log(`      address ${first.address}\n      user_id ${first.userId} (JWT verified: iss ${payload.iss}, aud ${payload.aud})`);
  });

  await step("2. reload -> passkey login -> same address and pairwise ID", async () => {
    await navigate(dappUrl(DAPP1));
    await waitH1(/Continue to Game X/);
    await click("passkey-signin");
    const addr = await waitDapp("address", /^0x[0-9a-f]{40}$/);
    assert(addr === first.address, `same address (${addr})`);
    assert((await dappField("userid")) === first.userId, "same pairwise ID");
    await verifyToken(await dappField("token"), apps[DAPP1].id);
    // Public balances come from the Sepolia RPC (publicnode by default).
    const bal = await waitFor(() => wallet(`(() => { const t = document.querySelector('[data-test="balance"]')?.textContent || ''; return /^[0-9]/.test(t) && t; })()`), "balance from the RPC");
    const meta = await wallet(`document.querySelector('.meta-line')?.textContent`);
    assert(/Sepolia/.test(meta), "balance line names Sepolia: " + meta);
    console.log(`      balance ${bal} (${meta})`);
  });

  await step("4. signMessage from the dapp -> confirm sheet -> signature recovers to the app address (viem)", async () => {
    const message = `Sign in to Game X\nnonce: ${Math.random().toString(16).slice(2)}`;
    await top(`(window.__replies = [], window.addEventListener('message', (e) => { if (e.origin === ${JSON.stringify(WALLET)}) window.__replies.push(e.data); }),
      document.getElementById('wallet').contentWindow.postMessage({ lowki: 1, type: 'sign_message', id: 'e2e-1', message: ${JSON.stringify(message)} }, ${JSON.stringify(WALLET)}), true)`);
    await waitH1(/Approve this message/);
    await click("approve");
    const reply = await waitFor(() => top(`window.__replies.find((r) => r.id === 'e2e-1')`), "message_signed");
    assert(reply.type === "message_signed", "reply " + JSON.stringify(reply));
    const signer = await recoverMessageAddress({ message, signature: reply.signature });
    assert(signer.toLowerCase() === first.address, `signature recovers to ${signer}`);
    console.log(`      signature ${reply.signature.slice(0, 20)}… recovers to ${signer}`);
  });

  let second = {};
  await step("3. second origin (:8742) -> same passkey -> different address and pairwise ID", async () => {
    await navigate(dappUrl(DAPP2));
    await waitH1(/Continue to Pixel Market/);
    await click("passkey-signin");
    second.address = await waitDapp("address", /^0x[0-9a-f]{40}$/);
    second.userId = await dappField("userid");
    assert(second.address !== first.address, "different address");
    assert(second.userId !== first.userId, "different pairwise ID");
    await verifyToken(await dappField("token"), apps[DAPP2].id);
    console.log(`      address ${second.address}\n      user_id ${second.userId}`);
  });

  await step("5. lose everything -> email + recovery code -> same address -> new passkey unlocks after reload", async () => {
    await s("Storage.clearDataForOrigin", { origin: WALLET, storageTypes: "all" });
    await s("Network.enable");
    await s("Network.clearBrowserCookies");
    await s("WebAuthn.removeVirtualAuthenticator", { authenticatorId });
    ({ authenticatorId } = await s("WebAuthn.addVirtualAuthenticator", { options: authOpts }));
    await navigate(dappUrl(DAPP1));
    await waitH1(/Continue to Game X/);
    await emailSignIn();
    await waitH1(/Welcome back/);
    await click("use-recovery-code");
    await waitH1(/Enter your recovery code/);
    fs.writeFileSync(path.join(STATE, "unlock-recover.png"), Buffer.from((await s("Page.captureScreenshot", { format: "png" })).data, "base64"));
    await type("recovery-input", first.code.toLowerCase().replace(/-/g, " "));
    await click("recover");
    await waitH1(/Your wallet is back/);
    await click("create-passkey");
    const addr = await waitDapp("address", /^0x[0-9a-f]{40}$/);
    assert(addr === first.address, `recovered the same address (${addr})`);
    assert((await dappField("userid")) === first.userId, "same pairwise ID");
    await navigate(dappUrl(DAPP1));
    await waitH1(/Continue to Game X/);
    await click("passkey-signin");
    const again = await waitDapp("address", /^0x[0-9a-f]{40}$/);
    assert(again === first.address, "the new passkey unlocks the same wallet");
    const { credentials } = await s("WebAuthn.getCredentials", { authenticatorId });
    assert(credentials.length === 1, "one new passkey");
  });
} catch (e) {
  failed = true;
  try { console.log("wallet screen:\n  " + (await walletText()).replace(/\n+/g, " | ")); } catch {}
  if (consoleLines.length) console.log("browser console:\n  " + consoleLines.slice(-25).join("\n  "));
} finally {
  ws.close();
  try { chrome.kill("SIGKILL"); } catch {}
  try { rmSync(profile, { recursive: true, force: true }); } catch {}
  servers.forEach((x) => x.close());
}
console.log(failed ? `FAIL (${results.length} steps passed)` : `PASS all ${results.length} steps`);
process.exit(failed ? 1 : 0);
