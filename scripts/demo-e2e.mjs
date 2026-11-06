// End-to-end run of the wallet UI's REAL private send against the demo stack
// (scripts/demo-stack.sh), in Chrome with a CDP virtual authenticator (ctap2.1, UV, PRF) and
// WebGPU on. Runs on the macOS host:
//
//   mac sh -lc 'cd <lowki> && node scripts/demo-e2e.mjs'
//
//   1. sign up (email code from the server log + passkey) -> the dapp gets the app address
//   2. fund the app account on the fork (anvil_setBalance 1 ETH) -> passkey login -> Home
//      shows the balance and the chain connection status
//   3. Move in 0.1 ETH (deposit -> ASP approval -> sync), private balance 0.099 ETH
//   4. Send privately 0.05 ETH to a fresh address: stages + proof time on screen; then on the
//      fork: recipient balance 0.04975 ETH, Pool.nullifierHashes(h) == true
//   5. Anon-RPC: the provider status the wallet showed, the gateway's access log (which path
//      each fork read took), and, in the same wallet origin and CSP, the glue against live
//      Sepolia: anonymous == true and the egress IP differs from this machine's (reads only)
//
// Env: CHROME, STACK (target/demo-stack/stack.json), SEPOLIA_RPC (live read-only check,
//      default https://sepolia.gateway.tenderly.co), SKIP_LIVE_ANON=1.
import fs from "node:fs";
import path from "node:path";
import { spawn } from "node:child_process";
import { mkdtempSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { fileURLToPath, pathToFileURL } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const REPO = path.resolve(here, "..");
const STACK = JSON.parse(fs.readFileSync(process.env.STACK || path.join(REPO, "target/demo-stack/stack.json"), "utf8"));
const CHROME = process.env.CHROME || "/Applications/Google Chrome.app/Contents/MacOS/Google Chrome";
const SEPOLIA_RPC = process.env.SEPOLIA_RPC || "https://sepolia.gateway.tenderly.co";
const WALLET = STACK.wallet;
const EMAIL = `demo-${Date.now()}@lowki.test`;
const OUT = path.join(REPO, "target/demo-stack/e2e");
fs.mkdirSync(OUT, { recursive: true });

const viem = await import(pathToFileURL(path.join(REPO, "vectors/lowki-worker/node_modules/viem/_esm/index.js")));
const { privateKeyToAccount, generatePrivateKey } = await import(pathToFileURL(path.join(REPO, "vectors/lowki-worker/node_modules/viem/_esm/accounts/index.js")));
const { formatEther, encodeFunctionData, decodeFunctionResult, parseAbi } = viem;

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const assert = (c, m) => { if (!c) throw new Error("assertion failed: " + m); };
const anvil = async (method, params = []) => {
  const r = await fetch(STACK.anvilRpc, { method: "POST", headers: { "content-type": "application/json" }, body: JSON.stringify({ jsonrpc: "2.0", id: 1, method, params }) });
  const j = await r.json();
  if (j.error) throw new Error(`${method}: ${j.error.message}`);
  return j.result;
};
const POOL = "0x644d5A2554d36e27509254F32ccfeBe8cd58861f";
const poolAbi = parseAbi(["function nullifierHashes(uint256) view returns (bool)"]);

// ---- Chrome over CDP (WebGPU + the anon-rpc WebRTC flags)
const profile = mkdtempSync(path.join(tmpdir(), "lowki-demo-e2e-"));
const chrome = spawn(CHROME, ["--headless=new", "--remote-debugging-port=0", `--user-data-dir=${profile}`,
  "--no-first-run", "--no-default-browser-check", "--window-size=1280,1000",
  "--enable-unsafe-webgpu", "--enable-features=WebGPU", "--ignore-gpu-blocklist",
  "--disable-features=WebRtcHideLocalIpsWithMdns", "--force-webrtc-ip-handling-policy=default", "about:blank"],
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

const results = [];
const facts = {};
let failed = false;
async function step(name, fn) {
  const t = Date.now();
  try { await fn(); results.push(name); console.log(`PASS  ${name} (${((Date.now() - t) / 1000).toFixed(1)} s)`); }
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
  // The wallet iframe is same-site, so its console arrives on this session too (Runtime per target).
  await s("Page.enable");
  await s("Runtime.enable");
  await s("WebAuthn.enable", { enableUI: false });
  const authOpts = { protocol: "ctap2", ctap2Version: "ctap2_1", transport: "internal", hasResidentKey: true,
    hasUserVerification: true, isUserVerified: true, hasPrf: true, automaticPresenceSimulation: true };
  await s("WebAuthn.addVirtualAuthenticator", { options: authOpts });

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
  const frameCtx = async () => {
    const { frameTree } = await s("Page.getFrameTree");
    const f = (frameTree.childFrames || []).find((c) => c.frame.url.startsWith(WALLET));
    if (!f) throw new Error("wallet frame not found");
    const { executionContextId } = await s("Page.createIsolatedWorld", { frameId: f.frame.id, worldName: "e2e" });
    return executionContextId;
  };
  const wallet = async (expr) => evalIn(await frameCtx(), expr);
  walletText = () => wallet(`document.querySelector('#main')?.innerText || ''`);
  const waitFor = async (fn, what, timeout = 20000, every = 150) => {
    const end = Date.now() + timeout;
    let last;
    while (Date.now() < end) {
      try { const v = await fn(); if (v) return v; last = v; } catch (e) { last = e.message; }
      await sleep(every);
    }
    throw new Error(`timeout waiting for ${what} (last: ${JSON.stringify(last)})`);
  };
  const h1 = () => wallet(`document.querySelector('#main h1')?.textContent || ''`);
  const waitH1 = (re, timeout) => waitFor(async () => re.test(await h1()) && (await h1()), `wallet heading ~ ${re}`, timeout);
  const click = (test) => wallet(`(() => { const e = document.querySelector('[data-test="${test}"]'); if (!e) throw new Error('no ${test}'); e.click(); return true; })()`);
  const type = (test, value) => wallet(`(() => { const e = document.querySelector('[data-test="${test}"]'); e.value = ${JSON.stringify(value)}; e.dispatchEvent(new Event('input', { bubbles: true })); return true; })()`);
  const text = (sel) => wallet(`document.querySelector(${JSON.stringify(sel)})?.innerText || ''`);
  const dappField = (id) => top(`document.getElementById(${JSON.stringify(id)}).textContent`);
  const waitDapp = (id, re) => waitFor(async () => { const v = await dappField(id); return re.test(v) && v; }, `dapp #${id} ~ ${re}`);
  const shot = async (name) => fs.writeFileSync(path.join(OUT, name), Buffer.from((await s("Page.captureScreenshot", { format: "png" })).data, "base64"));
  const logSize = () => fs.statSync(STACK.serverLog).size;
  const emailCode = async (since) => waitFor(() => {
    const log = fs.readFileSync(STACK.serverLog, "utf8").slice(since);
    const m = [...log.matchAll(new RegExp(`email sign-in code for ${EMAIL.replace(/[.+]/g, "\\$&")}: (\\d{6})`, "g"))];
    return m.length ? m[m.length - 1][1] : null;
  }, "email code in the server log");

  let address;
  await step("1. sign up: email code -> passkey -> dapp gets the app address", async () => {
    await navigate(STACK.dappUrl + "&walletflags=platform%3Dmac");
    await waitH1(/Continue to Game X/, 30000);
    await top(`document.getElementById('signin').click()`);
    await click("email-open");
    await type("email-input", EMAIL);
    const since = logSize();
    await click("email-continue");
    await waitH1(/Check your email/);
    await type("email-code", await emailCode(since));
    await click("email-verify");
    await waitH1(/Secure your wallet/);
    await click("create-passkey");
    await waitH1(/Save your recovery code/, 30000);
    await click("recovery-done");
    address = await waitDapp("address", /^0x[0-9a-f]{40}$/);
    console.log(`      app account ${address}`);
  });

  await step("2. fund the app account on the fork (anvil_setBalance 1 ETH) -> passkey login -> balance + connection status", async () => {
    await anvil("anvil_setBalance", [address, "0xDE0B6B3A7640000"]);
    await navigate(STACK.dappUrl + "&walletflags=platform%3Dmac");
    await waitH1(/Continue to Game X/, 30000);
    await click("passkey-signin");
    await waitH1(/Your Game X wallet/, 60000);
    const status = await waitFor(() => text('[data-test="rpc-status"]'), "connection status on Home", 90000);
    facts.homeStatus = status.replace(/\n+/g, " | ");
    facts.homeStatusPrivate = await wallet(`document.querySelector('[data-test="rpc-status"]')?.dataset.private`);
    const tokens = await waitFor(async () => { const t = await text('#tok-h + .list'); return /ETH[\s\S]*1\.0000/.test(t) && t; }, "1 ETH in the account", 90000);
    console.log(`      connection: ${facts.homeStatus} (private=${facts.homeStatusPrivate})`);
    console.log(`      tokens: ${tokens.replace(/\n+/g, " ")}`);
    await shot("home.png");
  });

  await step("3. Move in 0.1 ETH: deposit -> ASP approval -> sync -> private balance 0.099 ETH", async () => {
    await waitFor(() => wallet(`!document.querySelector('[data-test="move-in"]').disabled`), "Move in enabled", 60000);
    await click("move-in");
    await waitH1(/Move to private balance/);
    await type("vault-amount", "0.1");
    await click("vault-submit");
    const stages = [];
    await waitFor(async () => {
      const st = await text('[data-test="proof-steps"]').catch(() => "");
      if (st) stages.push(st.replace(/\n+/g, " | "));
      return /Now private/.test(await h1());
    }, "Now private", 240000, 500);
    facts.moveInStages = stages.at(-1);
    facts.moveIn = (await text('[data-test="private-report"]')).replace(/\n+/g, " | ");
    facts.moveInTx = await wallet(`document.querySelector('.kvs .chip-addr')?.getAttribute('title') || ''`);
    console.log(`      last progress: ${facts.moveInStages}`);
    console.log(`      report: ${facts.moveIn}`);
    console.log(`      deposit tx ${facts.moveInTx}`);
    await shot("move-in-done.png");
    await click("done");
    const pb = await waitFor(async () => { const t = await text('[data-test="private-balance"]'); return /0\.0990/.test(t) && t; }, "private balance 0.0990 ETH", 120000, 500);
    console.log(`      private balance ${pb.replace(/\n/g, " ")}`);
  });

  const recipient = privateKeyToAccount(generatePrivateKey()).address;
  await step("4. Send privately 0.05 ETH to a fresh address: stages + proof time on screen", async () => {
    assert(BigInt(await anvil("eth_getBalance", [recipient, "latest"])) === 0n, "recipient starts at 0");
    await click("send");
    await waitH1(/^Send$/);
    const priv = await wallet(`document.querySelector('[data-test="private-toggle"]').getAttribute('aria-checked')`);
    assert(priv === "true", "Send privately is on");
    // The token buttons come from the balances (public + Kohaku private sync).
    const tok = await waitFor(() => wallet(`[...document.querySelectorAll('.seg-btn')].find((b) => b.getAttribute('aria-checked') === 'true')?.textContent`), "a selected token", 60000);
    assert(tok === "ETH", `token ETH (got ${tok})`);
    const avail = await waitFor(async () => { const t = await text('.label-aside'); return /0\.0990 ETH/.test(t) && t; }, "0.0990 ETH available", 60000);
    console.log(`      ${avail}`);
    await type("send-to", recipient);
    await type("send-amount", "0.05");
    await click("send-submit");
    const stages = [];
    let shotTaken = false;
    await waitFor(async () => {
      const st = await text('[data-test="proof-steps"]').catch(() => "");
      if (st) {
        stages.push(st.replace(/\n+/g, " | "));
        if (!shotTaken && /Building proof/.test(st)) { shotTaken = true; await shot("send-proving.png").catch(() => {}); }
      }
      return /Sent privately/.test(await h1());
    }, "Sent privately", 300000, 250);
    facts.sendStages = [...new Set(stages)].slice(-4);
    facts.send = (await text('[data-test="private-report"]')).replace(/\n+/g, " | ");
    facts.relayTx = await wallet(`document.querySelector('.kvs .chip-addr')?.getAttribute('title') || ''`);
    console.log(`      progress samples: ${facts.sendStages.join("  //  ")}`);
    console.log(`      report: ${facts.send}`);
    console.log(`      relay tx ${facts.relayTx}`);
    assert(/Proof/.test(facts.send) && /ms on (WebGPU|CPU)/.test(facts.send), "proof time + backend shown");
    assert(/Syncing pool/.test(facts.send) && /Building proof/.test(facts.send) && /Submitting/.test(facts.send), "stage times shown");
    await shot("send-done.png");
  });

  await step("4b. on the fork: recipient got 0.05 - 0.5% fee, nullifier spent", async () => {
    const bal = BigInt(await anvil("eth_getBalance", [recipient, "latest"]));
    facts.recipient = recipient;
    facts.recipientBalance = formatEther(bal);
    console.log(`      recipient ${recipient}: 0 -> ${formatEther(bal)} ETH`);
    assert(bal === 49_750_000_000_000_000n, `recipient balance 0.04975 ETH (got ${formatEther(bal)})`);
    const line = consoleLines.find((l) => /private send:.*nullifierHash=/.test(l));
    assert(line, "nullifier hash logged by the wallet");
    const h = BigInt(line.match(/nullifierHash=(\d+)/)[1]);
    const data = encodeFunctionData({ abi: poolAbi, functionName: "nullifierHashes", args: [h] });
    const spent = decodeFunctionResult({ abi: poolAbi, functionName: "nullifierHashes", data: await anvil("eth_call", [{ to: POOL, data }, "latest"]) });
    facts.nullifier = h.toString();
    console.log(`      Pool.nullifierHashes(${h}) = ${spent}`);
    assert(spent === true, "nullifier spent");
    const rc = await anvil("eth_getTransactionReceipt", [facts.relayTx]);
    console.log(`      relay receipt: block ${parseInt(rc.blockNumber, 16)}, status ${rc.status}, gasUsed ${parseInt(rc.gasUsed, 16)}, from ${rc.from} (relayer), to ${rc.to} (Entrypoint)`);
  });

  await step("5. Anon-RPC: status shown, which path the fork reads took, live Sepolia through the anon network", async () => {
    const log = fs.readFileSync(STACK.accessLog, "utf8").trim().split("\n").filter(Boolean).map((l) => JSON.parse(l));
    const rpc = log.filter((e) => e.calls);
    const by = (via) => rpc.filter((e) => e.via === via).length;
    const mine = rpc.filter((e) => JSON.stringify(e.calls).toLowerCase().includes(address.slice(2)));
    facts.gateway = { rpcRequests: rpc.length, viaTunnel: by("tunnel"), viaLocal: by("local"), aboutAppAccount: mine.length,
      relayer: log.filter((e) => e.relayer).map((e) => `${e.relayer}:${e.via}`), asp: log.filter((e) => e.asp).length };
    console.log(`      wallet status: ${facts.homeStatus}`);
    console.log(`      gateway: ${JSON.stringify(facts.gateway)}`);
    if (facts.homeStatusPrivate === "true") assert(by("local") === 0 || mine.every((e) => e.via === "tunnel"), "app-account reads came through the anon exits");
    if (process.env.SKIP_LIVE_ANON === "1") return;
    // Same glue, same wallet origin and CSP (mock backend, so Rust doesn't connect first):
    // live Sepolia over the anon network, read-only.
    await navigate(`${WALLET}/?backend=mock&screen=home`);
    await waitFor(() => top(`!!globalThis.lowkiGlue`), "lowkiGlue on the wallet page");
    const r = await top(`(async () => {
      const st = await lowkiGlue.connect({ mode: "anon", defaults: { chainId: 11155111, network: "Sepolia", rpcUrl: ${JSON.stringify(SEPOLIA_RPC)}, ipEchoUrl: "https://api.ipify.org?format=json" } });
      const t = performance.now();
      const bal = JSON.parse(await lowkiGlue.rpc("eth_getBalance", JSON.stringify([${JSON.stringify(address)}, "latest"])));
      const ms = Math.round(performance.now() - t);
      const block = JSON.parse(await lowkiGlue.rpc("eth_blockNumber", "[]"));
      for (let i = 0; i < 60 && !lowkiGlue.status().egressIp; i++) await new Promise((r) => setTimeout(r, 500));
      return { connect: st, status: lowkiGlue.status(), balance: bal, block: parseInt(block, 16), callMs: ms };
    })()`);
    const direct = (await (await fetch("https://api.ipify.org?format=json")).json()).ip;
    facts.liveAnon = { ...r, directIp: direct };
    console.log(`      live Sepolia via glue: anonymous=${r.status.anonymous} network=${r.status.anonNetwork} boot=${r.status.bootMs} ms block=${r.block} eth_getBalance=${r.callMs} ms`);
    console.log(`      egress IP seen by ipify through the session: ${r.status.egressIp}; this machine directly: ${direct}`);
    assert(r.status.anonymous === true, `anon session booted (reason: ${r.status.reason})`);
    assert(r.status.egressIp && r.status.egressIp !== direct, "egress IP differs from the direct IP");
  });
} catch (e) {
  failed = true;
  try { console.log("wallet screen:\n  " + (await walletText()).replace(/\n+/g, " | ")); } catch {}
} finally {
  if (consoleLines.length) fs.writeFileSync(path.join(OUT, "console.log"), consoleLines.join("\n"));
  fs.writeFileSync(path.join(OUT, "facts.json"), JSON.stringify(facts, null, 2));
  if (failed && consoleLines.length) console.log("browser console (last 30):\n  " + consoleLines.slice(-30).join("\n  "));
  ws.close();
  try { chrome.kill("SIGKILL"); } catch {}
  try { rmSync(profile, { recursive: true, force: true }); } catch {}
}
console.log(failed ? `FAIL (${results.length} steps passed)` : `PASS all ${results.length} steps`);
console.log(`facts: ${path.join(OUT, "facts.json")}`);
process.exit(failed ? 1 : 0);
