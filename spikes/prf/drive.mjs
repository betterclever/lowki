// Drives headless Chrome over raw CDP (Node's built-in WebSocket, no npm deps) with a
// virtual CTAP2.1 authenticator that supports PRF, runs web/index.html, prints the result.
//
// Env knobs (for the option matrix in README.md):
//   CHROME            Chrome binary (default: /Applications/Google Chrome.app/...)
//   PRF_HEADLESS      headless flag (default "--headless=new"; "" = headful)
//   PRF_HOST          host used in the page URL (default "localhost")
//   PRF_QUERY         extra query string for the page, e.g. "rpId=" (omit rp.id)
//   PRF_AUTH          JSON merged into the virtual-authenticator options
//   PRF_NO_VA=1       don't add a virtual authenticator at all
//   PRF_NO_GESTURE=1  evaluate runSpike() without userGesture: true
import { spawn } from "node:child_process";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import { serve } from "./serve.mjs";

const CHROME = process.env.CHROME ?? "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";
const HEADLESS = process.env.PRF_HEADLESS ?? "--headless=new";
const HOST = process.env.PRF_HOST ?? "localhost";
const AUTH = {
  protocol: "ctap2",
  ctap2Version: "ctap2_1",
  transport: "internal",
  hasResidentKey: true,
  hasUserVerification: true,
  isUserVerified: true,
  hasPrf: true,
  automaticPresenceSimulation: true,
  ...JSON.parse(process.env.PRF_AUTH ?? "{}"),
};
for (const k of Object.keys(AUTH)) if (AUTH[k] === null) delete AUTH[k]; // PRF_AUTH='{"k":null}' drops k

const server = await serve(0);
const port = server.address().port;
const profile = fs.mkdtempSync(path.join(os.tmpdir(), "lowki-prf-"));
const args = [
  ...(HEADLESS ? [HEADLESS] : []),
  "--remote-debugging-port=0",
  `--user-data-dir=${profile}`,
  "--no-first-run",
  "--no-default-browser-check",
  "about:blank",
];
const chrome = spawn(CHROME, args, { stdio: ["ignore", "ignore", "pipe"] });
let done = false;
const cleanup = () => { if (done) return; done = true; try { chrome.kill(); } catch {} server.close(); try { fs.rmSync(profile, { recursive: true, force: true }); } catch {} };
const timer = setTimeout(() => { console.log(JSON.stringify({ ok: false, error: "timeout" })); cleanup(); process.exit(2); }, 60000);

const wsUrl = await new Promise((ok, bad) => {
  let buf = "";
  chrome.stderr.on("data", (d) => {
    buf += d;
    const m = buf.match(/DevTools listening on (ws:\/\/\S+)/);
    if (m) ok(m[1]);
  });
  chrome.on("exit", (c) => bad(new Error(`chrome exited ${c}: ${buf}`)));
});

const ws = new WebSocket(wsUrl);
await new Promise((ok, bad) => { ws.onopen = ok; ws.onerror = bad; });
let nextId = 1;
const pending = new Map();
const waiters = [];
ws.onmessage = (ev) => {
  const msg = JSON.parse(ev.data);
  if (msg.id && pending.has(msg.id)) {
    const { ok, bad, method } = pending.get(msg.id);
    pending.delete(msg.id);
    msg.error ? bad(new Error(`${method}: ${JSON.stringify(msg.error)}`)) : ok(msg.result);
  } else if (msg.method) {
    for (const w of waiters) if (w.method === msg.method && w.sessionId === msg.sessionId) w.ok(msg.params);
  }
};
const send = (method, params = {}, sessionId) => new Promise((ok, bad) => {
  const id = nextId++;
  pending.set(id, { ok, bad, method });
  ws.send(JSON.stringify({ id, method, params, ...(sessionId ? { sessionId } : {}) }));
});
const once = (method, sessionId) => new Promise((ok) => waiters.push({ method, sessionId, ok }));

let result;
try {
  const browserVersion = (await send("Browser.getVersion")).product;
  const { targetId } = await send("Target.createTarget", { url: "about:blank" });
  const { sessionId } = await send("Target.attachToTarget", { targetId, flatten: true });
  const s = (m, p) => send(m, p, sessionId);

  let authenticatorId = null;
  if (!process.env.PRF_NO_VA) {
    await s("WebAuthn.enable", { enableUI: false });
    ({ authenticatorId } = await s("WebAuthn.addVirtualAuthenticator", { options: AUTH }));
  }
  await s("Page.enable");
  const loaded = once("Page.loadEventFired", sessionId);
  const url = `http://${HOST}:${port}/${process.env.PRF_QUERY ? "?" + process.env.PRF_QUERY : ""}`;
  await s("Page.navigate", { url });
  await loaded;
  for (let i = 0; i < 100; i++) {
    const r = await s("Runtime.evaluate", { expression: "window.__PRF_READY === true", returnByValue: true });
    if (r.result.value) break;
    await new Promise((ok) => setTimeout(ok, 50));
  }
  const ev = await s("Runtime.evaluate", { expression: "window.runSpike()", awaitPromise: true, returnByValue: true, userGesture: !process.env.PRF_NO_GESTURE });
  result = ev.exceptionDetails ? { ok: false, error: ev.exceptionDetails.exception?.description ?? ev.exceptionDetails.text } : ev.result.value;
  if (authenticatorId) {
    const { credentials } = await s("WebAuthn.getCredentials", { authenticatorId });
    result.info.virtualAuthenticator = { options: AUTH, credentials: credentials.map((c) => ({ isResidentCredential: c.isResidentCredential, rpId: c.rpId, signCount: c.signCount })) };
  }
  result.info.browser = browserVersion;
  result.info.launch = args.filter((a) => !a.startsWith("--user-data-dir"));
  result.info.url = url;
} catch (e) {
  result = { ok: false, error: String(e.message ?? e) };
}
clearTimeout(timer);
console.log(JSON.stringify(result, null, 2));
cleanup();
process.exit(result.ok ? 0 : 1);
