// Lowki Tips end to end, in headless Chrome over raw CDP with a virtual authenticator
// (ctap2.1, UV, PRF), against the stack from scripts/demo-tips.sh. Runs on the macOS host.
//
//   1. sign in: email code (read from the server log) -> passkey -> the app gets address + pairwise ID
//   2. tip chai.lowki.eth twice: two different resolved addresses, both funded on the fork
//   3. tip privately: useSendPrivately -> the wallet's proof sheet -> "Sent privately"
//   4. creator inbox (viewing-key signature -> gateway records) finds both tips with balances
//   Screenshots of every screen go to apps/lowki-tips/screens/.
//
//   node apps/lowki-tips/e2e/run.mjs
//
// Env: CHROME, TIPS_URL (http://localhost:8823), WALLET_URL (http://localhost:8822),
//      TIPS_STATE (target/lowki-tips/state), CREATOR (chai). Node >= 22, no npm dependencies.
import fs from 'node:fs';
import path from 'node:path';
import { spawn } from 'node:child_process';
import { mkdtempSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { fileURLToPath } from 'node:url';

const here = path.dirname(fileURLToPath(import.meta.url));
const APP = path.resolve(here, '..');
const REPO = path.resolve(APP, '../..');
const CHROME = process.env.CHROME || '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome';
const TIPS = process.env.TIPS_URL || 'http://localhost:8823';
const WALLET = process.env.WALLET_URL || 'http://localhost:8822';
const STATE = process.env.TIPS_STATE || path.join(REPO, 'target/lowki-tips/state');
const SHOTS = path.join(APP, 'screens');
const CREATOR = process.env.CREATOR || 'chai';
const EMAIL = `tipper-${Date.now()}@lowki.test`;
const config = JSON.parse(fs.readFileSync(path.join(APP, 'config/fork.json'), 'utf8'));
const NAME = `${CREATOR}.${config.ens.parentName}`;
const TIP_WEI = BigInt(Math.round(Number(config.tip.amountEth) * 1e6)) * 10n ** 12n;

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const assert = (c, m) => { if (!c) throw new Error('assertion failed: ' + m); };
fs.mkdirSync(SHOTS, { recursive: true });

async function rpc(method, params) {
  const r = await fetch(config.chain.rpcUrl, { method: 'POST', headers: { 'content-type': 'application/json' }, body: JSON.stringify({ jsonrpc: '2.0', id: 1, method, params }) });
  const j = await r.json();
  if (j.error) throw new Error(JSON.stringify(j.error));
  return j.result;
}

// ---- Chrome + CDP -----------------------------------------------------------------------------
const profile = mkdtempSync(path.join(tmpdir(), 'lowki-tips-e2e-'));
const chrome = spawn(CHROME, ['--headless=new', '--remote-debugging-port=0', `--user-data-dir=${profile}`, '--no-first-run',
  '--no-default-browser-check', '--hide-scrollbars', '--window-size=1280,900', 'about:blank'], { stdio: ['ignore', 'ignore', 'pipe'] });
const wsUrl = await new Promise((resolve, reject) => {
  let buf = '';
  const t = setTimeout(() => reject(new Error('Chrome did not start')), 20000);
  chrome.stderr.on('data', (d) => { buf += d; const m = buf.match(/DevTools listening on (ws:\/\/\S+)/); if (m) { clearTimeout(t); resolve(m[1]); } });
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

const consoleLines = [];
async function openTab(url) {
  const { targetId } = await send('Target.createTarget', { url: 'about:blank' });
  const { sessionId } = await send('Target.attachToTarget', { targetId, flatten: true });
  const s = (m, p) => send(m, p, sessionId);
  await s('Page.enable');
  await s('Runtime.enable');
  await s('Emulation.setDeviceMetricsOverride', { width: 1280, height: 900, deviceScaleFactor: 2, mobile: false });
  await s('Emulation.setEmulatedMedia', { features: [{ name: 'prefers-color-scheme', value: process.env.THEME || 'light' }] });
  listeners.push((m) => {
    if (m.sessionId !== sessionId) return;
    if (m.method === 'Runtime.consoleAPICalled') consoleLines.push(m.params.args.map((a) => a.value ?? a.description).join(' '));
    if (m.method === 'Runtime.exceptionThrown') consoleLines.push('exception: ' + m.params.exceptionDetails.exception?.description);
  });
  const t = {
    s,
    async navigate(u) {
      const loaded = new Promise((r) => { const l = (m) => { if (m.method === 'Page.loadEventFired' && m.sessionId === sessionId) { listeners.splice(listeners.indexOf(l), 1); r(); } }; listeners.push(l); });
      await s('Page.navigate', { url: u });
      await loaded;
    },
    async evalIn(contextId, expression) {
      const r = await s('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true, userGesture: true, ...(contextId ? { contextId } : {}) });
      if (r.exceptionDetails) throw new Error(r.exceptionDetails.exception?.description || JSON.stringify(r.exceptionDetails));
      return r.result.value;
    },
    top: (expr) => t.evalIn(null, expr),
    async frameCtx() {
      const { frameTree } = await s('Page.getFrameTree');
      const f = (frameTree.childFrames || []).find((c) => c.frame.url.startsWith(WALLET));
      if (!f) throw new Error('wallet frame not found');
      const { executionContextId } = await s('Page.createIsolatedWorld', { frameId: f.frame.id, worldName: 'e2e' });
      return executionContextId;
    },
    wallet: async (expr) => t.evalIn(await t.frameCtx(), expr),
    async shot(name) {
      await sleep(350);
      const { data } = await s('Page.captureScreenshot', { format: 'png' });
      fs.writeFileSync(path.join(SHOTS, name), Buffer.from(data, 'base64'));
      console.log(`      screenshot screens/${name}`);
    },
  };
  await t.navigate(url);
  return t;
}

const waitFor = async (fn, what, timeout = 30000) => {
  const end = Date.now() + timeout;
  let last;
  while (Date.now() < end) {
    try { const v = await fn(); if (v) return v; last = v; } catch (e) { last = e.message; }
    await sleep(150);
  }
  throw new Error(`timeout waiting for ${what} (last: ${JSON.stringify(last)})`);
};

const results = {};
async function step(name, fn) {
  const t = Date.now();
  try { await fn(); console.log(`PASS  ${name} (${Date.now() - t} ms)`); }
  catch (e) { console.log(`FAIL  ${name}\n      ${e.message}`); throw e; }
}

let exitCode = 0;
try {
  await send('WebAuthn.enable', {}).catch(() => {});
  const tab = await openTab(TIPS + '/');
  await tab.s('WebAuthn.enable', { enableUI: false });
  await tab.s('WebAuthn.addVirtualAuthenticator', { options: { protocol: 'ctap2', ctap2Version: 'ctap2_1', transport: 'internal',
    hasResidentKey: true, hasUserVerification: true, isUserVerified: true, hasPrf: true, automaticPresenceSimulation: true } });

  const q = (testid) => `document.querySelector('[data-testid="${testid}"]')`;
  const text = (testid) => tab.top(`${q(testid)}?.textContent ?? ''`);
  const clickApp = (testid) => tab.top(`(() => { const e = ${q(testid)}; if (!e) throw new Error('no ${testid}'); if (e.disabled) throw new Error('${testid} disabled'); e.click(); return true; })()`);
  const waitApp = (testid, timeout) => waitFor(() => tab.top(`!!${q(testid)}`), `app [data-testid=${testid}]`, timeout);
  const h1 = () => tab.wallet(`document.querySelector('#main h1')?.textContent || ''`);
  const waitH1 = (re, timeout) => waitFor(async () => re.test(await h1()) && (await h1()), `wallet heading ~ ${re}`, timeout);
  const clickW = (test) => tab.wallet(`(() => { const e = document.querySelector('[data-test="${test}"]'); if (!e) throw new Error('no ${test}'); e.click(); return true; })()`);
  const typeW = (test, value) => tab.wallet(`(() => { const e = document.querySelector('[data-test="${test}"]'); e.value = ${JSON.stringify(value)}; e.dispatchEvent(new Event('input', { bubbles: true })); return true; })()`);
  const waitApprove = () => waitFor(() => tab.wallet(`(() => { const b = document.querySelector('[data-test="approve"]'); return !!b && !b.disabled; })()`), 'wallet approve button enabled', 30000);
  const logPath = path.join(STATE, 'server.log');
  const emailCode = (since) => waitFor(() => {
    const log = fs.readFileSync(logPath, 'utf8').slice(since);
    const m = [...log.matchAll(new RegExp(`email sign-in code for ${EMAIL.replace(/[.+]/g, '\\$&')}: (\\d{6})`, 'g'))];
    return m.length ? m[m.length - 1][1] : null;
  }, 'email code in the server log');

  await step('0. landing: "Tip your favourite creators" + Sign in; footer names the one-import change', async () => {
    await waitApp('landing');
    assert(/Tip your favourite/.test(await tab.top('document.querySelector("h1").textContent')), 'hero headline');
    assert(/We changed one import/.test(await text('one-import')), 'one-import note');
    await tab.shot('01-landing.png');
  });

  await step('1. sign in: email code (server log) -> passkey (virtual authenticator, PRF) -> the app gets address + pairwise ID', async () => {
    await clickApp('signin');
    await waitH1(/Continue to|Sign in/i);
    await tab.shot('02-wallet-signin.png');
    await clickW('email-open');
    await typeW('email-input', EMAIL);
    const since = fs.statSync(logPath).size;
    await clickW('email-continue');
    await waitH1(/Check your email/);
    await typeW('email-code', await emailCode(since));
    await clickW('email-verify');
    await waitH1(/Secure your wallet/);
    await clickW('create-passkey');
    await waitH1(/recovery code/i, 40000);
    await clickW('recovery-done');
    await waitApp('creators', 30000);
    results.address = await waitFor(() => tab.top(`${q('user-address')}?.dataset.full`), 'header address');
    results.userId = await tab.top(`${q('user-id')}?.dataset.full`);
    assert(/^0x[0-9a-fA-F]{40}$/.test(results.address), 'address ' + results.address);
    assert(/^did:lowki:/.test(results.userId), 'pairwise ID ' + results.userId);
    console.log(`      address ${results.address}, user ${results.userId}`);
    await tab.shot('03-creators.png');
    await tab.top(`${q('who')}.classList.add('show-tip')`);
    await tab.shot('04-header-only-this-app.png');
    await tab.top(`${q('who')}.classList.remove('show-tip')`);
  });

  await step('   fork faucet: +1 test ETH to the app account (anvil_setBalance)', async () => {
    await clickApp('faucet');
    await waitFor(async () => BigInt(await rpc('eth_getBalance', [results.address, 'latest'])) > 0n, 'funded app account');
  });

  // Top-level wallet page (same origin as the iframe, so the same session and passkey).
  const topH1 = () => tab.top(`document.querySelector('#main h1')?.textContent || ''`);
  const waitTopH1 = (re, timeout) => waitFor(async () => re.test(await topH1()) && (await topH1()), `wallet page heading ~ ${re}`, timeout);
  const clickTop = (test) => tab.top(`(() => { const e = document.querySelector('[data-test="${test}"]'); if (!e) throw new Error('no ${test}'); e.click(); return true; })()`);

  if (process.env.SKIP_PRIVATE !== '1') await step('   private balance: open the Lowki wallet for this app (link on the private screen) -> Move in 0.1 ETH', async () => {
    await tab.navigate(`${WALLET}/?app=${config.lowki.appId}&origin=${encodeURIComponent(TIPS)}&app_name=Lowki%20Tips&platform=mac`);
    await waitTopH1(/Continue to Lowki Tips/, 30000);
    await clickTop('passkey-signin');
    await waitFor(() => tab.top(`(() => { const b = document.querySelector('[data-test="move-in"]'); return !!b && !b.disabled; })()`), 'Move in enabled', 90000);
    await clickTop('move-in');
    await waitTopH1(/Move to private balance/);
    await tab.top(`(() => { const e = document.querySelector('[data-test="vault-amount"]'); e.value = '0.1'; e.dispatchEvent(new Event('input', { bubbles: true })); })()`);
    await clickTop('vault-submit');
    await waitTopH1(/Now private/, 240000);
    await tab.shot('00-wallet-move-in.png');
    await clickTop('done');
    const pb = await waitFor(async () => { const t = await tab.top(`document.querySelector('[data-test="private-balance"]')?.textContent || ''`); return /0\.0990/.test(t) && t; }, 'private balance 0.099 ETH', 120000);
    console.log(`      private balance ${pb.replace(/\s+/g, ' ')}`);
    // Back to the app: it finds the session again (passkey unlock if the wallet asks).
    await tab.navigate(TIPS + '/');
    await waitFor(() => tab.top(`!!(${q('creators')} || ${q('landing')})`), 'app loaded', 30000);
    if (await tab.top(`!!${q('landing')}`)) {
      await clickApp('signin');
      await waitH1(/Continue to|Unlock|Sign in/i);
      await clickW('passkey-signin');
    }
    await waitApp('creators', 60000);
  });

  const tipOnce = async (i) => {
    await waitApp('resolved-address', 30000);
    const addr = (await text('resolved-address')).trim();
    assert(/^0x[0-9a-fA-F]{40}$/.test(addr), 'resolved ' + addr);
    assert(await tab.top(`!!${q('fresh-badge')}`), '"Never seen before" badge');
    if (i === 1) await tab.shot('05-tip-resolved.png');
    await clickApp('send-tip');
    await waitApprove();
    if (i === 1) await tab.shot('06-wallet-confirm-tx.png');
    await clickW('approve');
    await waitApp('tip-sent', 60000);
    return addr;
  };

  await step(`2. tip ${NAME} twice: plain viem getEnsAddress -> two different fresh addresses, both funded`, async () => {
    await clickApp(`tip-${CREATOR}`);
    const a1 = await tipOnce(1);
    await clickApp('tip-again');
    await waitFor(async () => (await text('resolved-address')).trim() && (await text('resolved-address')).trim() !== a1, 'second resolution', 30000);
    const a2 = await tipOnce(2);
    assert(a1.toLowerCase() !== a2.toLowerCase(), 'two different addresses');
    await tab.shot('07-tip-twice.png');
    for (const a of [a1, a2]) {
      const bal = BigInt(await rpc('eth_getBalance', [a, 'latest']));
      assert(bal === TIP_WEI, `${a} holds ${bal}`);
      console.log(`      ${a}: ${bal} wei`);
    }
    results.tips = [a1, a2];
  });

  if (process.env.SKIP_PRIVATE !== '1') await step('3. tip privately: useSendPrivately -> wallet proof sheet -> "Sent privately" + hash', async () => {
    await tab.top(`document.querySelector('.back').click()`);
    await clickApp(`private-${CREATOR}`);
    await waitFor(async () => /^0x/.test((await text('private-to')).trim()), 'private recipient resolved', 30000);
    await tab.shot('08-private.png');
    await clickApp('send-private');
    await waitApprove();
    await tab.shot('09-wallet-confirm-private.png');
    await clickW('approve');
    let proofShot = false;
    const end = Date.now() + 180000;
    for (;;) {
      if (await tab.top(`!!${q('private-sent')}`)) break;
      const err = await tab.top(`${q('private-error')}?.textContent ?? ''`);
      if (err) throw new Error('private send failed: ' + err + ' / wallet: ' + (await tab.wallet(`document.querySelector('#main')?.innerText || ''`).catch(() => '')));
      if (!proofShot) { try { await tab.shot('10-wallet-proof-progress.png'); proofShot = true; } catch { /* frame gone */ } }
      if (Date.now() > end) throw new Error('private send timed out');
      await sleep(300);
    }
    results.privateHash = (await tab.top(`${q('tx-hash')}?.title || ${q('tx-hash')}?.textContent`)).trim();
    console.log(`      hash ${results.privateHash}`);
    await tab.shot('11-private-sent.png');
  });

  await step(`4. creator inbox: viewing-key signature -> gateway records -> both tips found with balances`, async () => {
    const inbox = await openTab(`${TIPS}/creator.html?name=${NAME}`);
    await waitFor(() => inbox.top(`!!document.querySelector('[data-testid="scan"]')`), 'inbox');
    await inbox.top(`document.querySelector('[data-testid="scan"]').click()`);
    await waitFor(() => inbox.top(`!!document.querySelector('[data-testid="inbox-results"]')`), 'scan results', 30000);
    const rows = await inbox.top(`[...document.querySelectorAll('[data-testid="inbox-row"]')].map((r) => ({ a: r.dataset.address.toLowerCase(), t: r.textContent }))`);
    for (const a of results.tips) {
      const row = rows.find((r) => r.a === a.toLowerCase());
      assert(row && /ETH/.test(row.t) && /spend key/.test(row.t), `inbox found ${a}`);
    }
    console.log(`      ${rows.length} records match ${NAME}'s keys; total ${await inbox.top(`document.querySelector('[data-testid="inbox-total"]').textContent`)}`);
    await inbox.shot('12-creator-inbox.png');
  });

  console.log('\nALL LOWKI TIPS E2E CHECKS PASSED');
  console.log(JSON.stringify(results, null, 2));
} catch (e) {
  exitCode = 1;
  console.log('\nconsole (last 25):\n' + consoleLines.slice(-25).join('\n'));
} finally {
  ws.close();
  chrome.kill();
  process.exit(exitCode);
}
