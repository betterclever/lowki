// End to end: the Privy-docs example app (`@privy-io/react-auth` swapped for `@lowki/react-auth`)
// against the REAL wallet: apps/wallet-ui with its real backend (lowki-worker in a Web Worker,
// lowki-server), in headless Chrome over raw CDP with a virtual authenticator (ctap2.1, UV, PRF).
//
//   1. login: email code (read from the server log) -> passkey -> the app shows the address
//      and a did:lowki: user ID -> getAccessToken's JWT verifies against the server's JWKS
//   2. signMessage verifies (viem)
//   3. signTypedData recovers to the address (viem)
//   4. sendTransaction: signed by the address, broadcast to a local anvil posing as Sepolia
//   5. sendPrivately round-trips through the protocol: confirm sheet, then `private_sent` or the
//      wallet's own rejection (Privacy Pools needs a relayer/ASP in lowki-config.json)
//   6. Safari UA: login goes through a popup (here: email + recovery code + a new passkey,
//      created in the popup), then the next request goes through the iframe, which picks up
//      the session with a passkey `get` (the hand-off)
//
// Ports (8800-8809 are this example's): 8800 wallet dist (+ /v1/ -> lowki-server, /rpc -> anvil),
// 8801 lowki-server, 8802 the example app, 8803 anvil. The wallet gets `rpc=direct`: its
// Anon-RPC tunnels need https RPC hosts, and anvil is plain http on localhost.
//
// Run on the macOS host:
//   e2e/real-server.sh                     (VM or host; LOWKI_SERVER_BIN=... to skip cargo run)
//   (cd apps/wallet-ui && LOWKI_RPC_URL=http://localhost:8800/rpc trunk build --release \
//       --dist ../../target/privy-swap-e2e/wallet-dist)
//   npm run build && node e2e/real.mjs
//
// Env: CHROME, LOWKI_SERVER (http://localhost:8801), WALLET_DIST, APP_DIST, ANVIL (path).
import fs from 'node:fs';
import path from 'node:path';
import { spawn } from 'node:child_process';
import { mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { fileURLToPath, pathToFileURL } from 'node:url';
import { createPublicClient, http as viemHttp, parseEther, recoverTypedDataAddress, verifyMessage, isAddress } from 'viem';
import { sepolia } from 'viem/chains';
import { serve } from './serve.mjs';

const here = path.dirname(fileURLToPath(import.meta.url));
const EX = path.resolve(here, '..');
const REPO = path.resolve(EX, '../..');
const STATE = path.join(REPO, 'target/privy-swap-e2e');
const WALLET_DIST = process.env.WALLET_DIST || path.join(STATE, 'wallet-dist');
const APP_DIST = process.env.APP_DIST || path.join(EX, 'dist');
const SERVER = process.env.LOWKI_SERVER || 'http://localhost:8801';
const CHROME = process.env.CHROME || '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome';
const ANVIL = process.env.ANVIL || 'anvil';
const WALLET = 'http://localhost:8800';
const APP = 'http://localhost:8802';
const RPC = 'http://localhost:8803';
const EMAIL = `privy-swap-${Date.now()}@lowki.test`;
const SAFARI_UA = 'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/19.0 Safari/605.1.15';

// The example's own constants (src/app/App.tsx, src/app/swap.ts).
const SWAP_MESSAGE = 'Privy Swap wants you to sign in.\n\nNonce: 7f3a9c';
const QUOTE = {
  domain: { name: 'Privy Swap', version: '1', chainId: 11155111, verifyingContract: '0xeE567Fe1712Faf6149d80dA1E6934E354124CfE3' },
  types: { Quote: [{ name: 'sellToken', type: 'address' }, { name: 'buyToken', type: 'address' }, { name: 'sellAmount', type: 'uint256' }] },
  primaryType: 'Quote',
  message: { sellToken: '0xfFf9976782d46CC05630D1f6eBAb18b2324d6B14', buyToken: '0x1c7D4B196Cb0C7B01d743Fbc6116a902379C7238', sellAmount: 10000000000000000n },
};

const { createRemoteJWKSet, jwtVerify } = await import(pathToFileURL(path.join(REPO, 'crates/lowki-server/tests/node/node_modules/jose/dist/webapi/index.js')));

const sleep = (ms) => new Promise((r) => setTimeout(r, ms));
const assert = (c, m) => { if (!c) throw new Error('assertion failed: ' + m); };

// ---- processes: anvil, static servers, Chrome ---------------------------------------------
const procs = [];
const anvil = spawn(ANVIL, ['--port', '8803', '--chain-id', '11155111', '--silent'], { stdio: 'ignore' });
procs.push(anvil);
const chain = createPublicClient({ chain: sepolia, transport: viemHttp(RPC) });
for (let i = 0; ; i++) {
  try { await chain.getChainId(); break; } catch { if (i > 100) throw new Error('anvil did not start on :8803'); await sleep(100); }
}
const servers = [
  await serve(WALLET_DIST, 8800, { v1: SERVER, rpc: RPC }),
  await serve(APP_DIST, 8802, { spa: true }),
];
const profile = mkdtempSync(path.join(tmpdir(), 'lowki-privy-swap-e2e-'));
const chrome = spawn(CHROME, ['--headless=new', '--remote-debugging-port=0', `--user-data-dir=${profile}`,
  '--no-first-run', '--no-default-browser-check', '--disable-gpu', '--window-size=1280,1000', 'about:blank'],
  { stdio: ['ignore', 'ignore', 'pipe'] });
procs.push(chrome);
const wsUrl = await new Promise((resolve, reject) => {
  let buf = '';
  const t = setTimeout(() => reject(new Error('Chrome did not start')), 20000);
  chrome.stderr.on('data', (d) => { buf += d; const m = buf.match(/DevTools listening on (ws:\/\/\S+)/); if (m) { clearTimeout(t); resolve(m[1]); } });
});

// ---- CDP ------------------------------------------------------------------------------------
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

const AUTH = { protocol: 'ctap2', ctap2Version: 'ctap2_1', transport: 'internal', hasResidentKey: true,
  hasUserVerification: true, isUserVerified: true, hasPrf: true, automaticPresenceSimulation: true };
const consoleLines = [];

/** A page target: evaluation in the top frame or the wallet frame, clicks, waits. */
function pageApi(sessionId, name) {
  const s = (m, p) => send(m, p, sessionId);
  listeners.push((m) => {
    if (m.sessionId !== sessionId) return;
    if (m.method === 'Runtime.consoleAPICalled') consoleLines.push(`[${name}] ` + m.params.args.map((a) => a.value ?? a.description).join(' '));
    if (m.method === 'Log.entryAdded') consoleLines.push(`[${name}] ${m.params.entry.level}: ${m.params.entry.text}`);
    if (m.method === 'Runtime.exceptionThrown') consoleLines.push(`[${name}] exception: ` + JSON.stringify(m.params.exceptionDetails.exception?.description));
  });
  const evalIn = async (contextId, expression) => {
    const r = await s('Runtime.evaluate', { expression, awaitPromise: true, returnByValue: true, userGesture: true, ...(contextId ? { contextId } : {}) });
    if (r.exceptionDetails) throw new Error(r.exceptionDetails.exception?.description || JSON.stringify(r.exceptionDetails));
    return r.result.value;
  };
  const waitFor = async (fn, what, timeout = 20000) => {
    const end = Date.now() + timeout;
    let last;
    while (Date.now() < end) {
      try { const v = await fn(); if (v) return v; last = v; } catch (e) { last = e.message; }
      await sleep(100);
    }
    throw new Error(`timeout waiting for ${what} (last: ${JSON.stringify(last)})`);
  };
  // The wallet iframe is same-site (localhost), so it runs in-process: an isolated world shares its DOM.
  const walletCtx = async () => {
    const { frameTree } = await s('Page.getFrameTree');
    const f = (frameTree.childFrames || []).find((c) => c.frame.url.startsWith(WALLET));
    if (!f) throw new Error('wallet frame not found');
    const { executionContextId } = await s('Page.createIsolatedWorld', { frameId: f.frame.id, worldName: 'e2e' });
    return executionContextId;
  };
  const api = {
    s,
    sessionId,
    top: (expr) => evalIn(null, expr),
    waitFor,
    /** In the wallet: the iframe on the app page, or the page itself (popup). */
    wallet: async (expr) => evalIn(name === 'popup' ? null : await walletCtx(), expr),
    async navigate(url) {
      const loaded = new Promise((r) => { const l = (m) => { if (m.method === 'Page.loadEventFired' && m.sessionId === sessionId) { listeners.splice(listeners.indexOf(l), 1); r(); } }; listeners.push(l); });
      await s('Page.navigate', { url });
      await loaded;
    },
    h1: () => api.wallet(`document.querySelector('#main h1')?.textContent || ''`),
    waitH1: (re, timeout) => waitFor(async () => { const t = await api.h1(); return re.test(t) && t; }, `wallet heading ~ ${re}`, timeout),
    click: (test) => api.wallet(`(() => { const e = document.querySelector('[data-test="${test}"]'); if (!e) throw new Error('no ${test}'); e.click(); return true; })()`),
    type: (test, value) => api.wallet(`(() => { const e = document.querySelector('[data-test="${test}"]'); e.value = ${JSON.stringify(value)}; e.dispatchEvent(new Event('input', { bubbles: true })); return true; })()`),
    // The example app (React): data-testid.
    appClick: (id) => api.top(`(() => { const e = document.querySelector('[data-testid="${id}"]'); if (!e) throw new Error('no ${id}'); e.click(); return true; })()`),
    appText: (id) => api.top(`document.querySelector('[data-testid="${id}"]')?.textContent ?? ''`),
    waitApp: (id, re, timeout) => waitFor(async () => { const v = await api.appText(id); return re.test(v) && v; }, `app [${id}] ~ ${re}`, timeout),
    overlay: () => api.top(`(() => { const r = document.querySelector('[data-lowki="wallet"]'); return !!r && r.style.display !== 'none'; })()`),
    walletText: () => api.wallet(`document.querySelector('#main')?.innerText || ''`),
  };
  return api;
}

let failed = false;
const results = [];
async function step(name, fn) {
  const t = Date.now();
  try { const info = await fn(); results.push(name); console.log(`PASS  ${name} (${Date.now() - t} ms)${info ? '\n      ' + info : ''}`); }
  catch (e) { console.log(`FAIL  ${name}\n      ${e.message}`); throw e; }
}

const apps = JSON.parse(fs.readFileSync(path.join(STATE, 'server/apps.json'), 'utf8'));
const APP_ID = apps[APP].id;
const appUrl = () => `${APP}/?app=${APP_ID}&wallet=${encodeURIComponent(`${WALLET}/?app_name=Privy%20Swap&platform=mac&rpc=direct`)}`;
const serverLog = path.join(STATE, 'server/server.log');
const logSize = () => fs.statSync(serverLog).size;
const jwks = createRemoteJWKSet(new URL(`${SERVER}/.well-known/jwks.json`));
const verifyToken = async (token) => (await jwtVerify(token, jwks, { issuer: SERVER, audience: APP_ID, algorithms: ['ES256'] })).payload;

let page;
try {
  const { targetId } = await send('Target.createTarget', { url: 'about:blank' });
  const { sessionId } = await send('Target.attachToTarget', { targetId, flatten: true });
  page = pageApi(sessionId, 'app');
  await page.s('Page.enable');
  await page.s('Runtime.enable');
  await page.s('Log.enable');
  await page.s('WebAuthn.enable', { enableUI: false });
  let { authenticatorId } = await page.s('WebAuthn.addVirtualAuthenticator', { options: AUTH });

  const emailCode = (since) => page.waitFor(() => {
    const log = fs.readFileSync(serverLog, 'utf8').slice(since);
    const m = [...log.matchAll(new RegExp(`email sign-in code for ${EMAIL.replace(/[.+]/g, '\\$&')}: (\\d{6})`, 'g'))];
    return m.length ? m[m.length - 1][1] : null;
  }, 'email code in the server log');
  /** Email sign-in in a wallet (iframe or popup). `share`: tick "Share this email". */
  const emailSignIn = async (w, { share = false } = {}) => {
    await w.click('email-open');
    await w.type('email-input', EMAIL);
    const since = logSize();
    await w.click('email-continue');
    await w.waitH1(/Check your email/);
    await w.type('email-code', await emailCode(since));
    if (share) await w.click('share-email');
    await w.click('email-verify');
  };

  let address = '';
  let userId = '';
  let recoveryCode = '';

  await step('1. login: email code -> passkey in the wallet iframe -> address + did:lowki: ID -> getAccessToken JWT verifies (JWKS)', async () => {
    await page.navigate(appUrl());
    await page.waitApp('status', /^signed-out$/);
    assert(!(await page.overlay()), 'wallet hidden while idle');
    await page.appClick('login');
    await page.waitH1(/Continue to Privy Swap/);
    assert(await page.overlay(), 'wallet shown as a modal');
    await emailSignIn(page, { share: true });
    await page.waitH1(/Secure your wallet/);
    await page.click('create-passkey');
    await page.waitH1(/Save your recovery code/, 30000);
    recoveryCode = await page.wallet(`[...document.querySelectorAll('[data-test="recovery-code"] li .mono')].map((e) => e.textContent).join('-')`);
    assert(/^([0-9A-Z]{4}-){13}[0-9A-Z]{4}$/.test(recoveryCode), 'recovery code ' + recoveryCode);
    await page.click('recovery-done');
    await page.waitApp('status', /^authenticated$/);
    address = await page.appText('address');
    userId = await page.appText('user-id');
    assert(isAddress(address), 'address ' + address);
    assert(/^did:lowki:[0-9a-f]{32}$/.test(userId), 'pairwise user ID ' + userId);
    await page.waitFor(async () => !(await page.overlay()), 'wallet hidden after login');
    const log = await page.appText('log');
    assert(/login complete: did:lowki:\w+ \(new: true, already: false\)/.test(log), 'onComplete with isNewUser: true: ' + log);
    // The user shared the email: Privy's linkedAccounts carries it.
    assert((await page.appText('linked')) === 'wallet, email', 'linked accounts: ' + (await page.appText('linked')));
    await page.appClick('token');
    const token = await page.waitApp('access-token', /^ey[\w-]+\.[\w-]+\.[\w-]+$/);
    const claims = await verifyToken(token);
    assert(claims.sub === userId, `JWT sub ${claims.sub} = user.id`);
    return `${address} ${userId}\n      JWT verified: iss ${claims.iss}, aud ${claims.aud}, sub = user.id`;
  });

  await step('2. signMessage -> wallet confirm sheet -> signature verifies for the address (viem)', async () => {
    await page.appClick('sign');
    await page.waitH1(/Approve this message/);
    await page.click('approve');
    const sig = await page.waitApp('signature', /^0x[0-9a-f]{130}$/i);
    assert(await verifyMessage({ address, message: SWAP_MESSAGE, signature: sig }), 'verifyMessage');
    return sig.slice(0, 24) + '…';
  });

  await step('3. signTypedData (EIP-712 quote) -> recovers to the address (viem)', async () => {
    await page.appClick('sign-typed');
    await page.waitH1(/Approve this signature/);
    await page.click('approve');
    const sig = await page.waitApp('typed-signature', /^0x[0-9a-f]{130}$/i);
    const who = await recoverTypedDataAddress({ ...QUOTE, signature: sig });
    assert(who.toLowerCase() === address.toLowerCase(), `recovered ${who}`);
    return `recovers to ${who}`;
  });

  await step('4. sendTransaction (swap, 0.01 ETH + calldata) -> hash on anvil (chain 11155111), sent from the address', async () => {
    await chain.request({ method: 'anvil_setBalance', params: [address, '0x' + parseEther('1').toString(16)] });
    await page.appClick('swap');
    await page.waitH1(/Approve this transaction/);
    await page.waitFor(() => page.wallet(`!!document.querySelector('[data-test="tx-preview"]')`), 'transaction preview');
    await page.click('approve');
    const hash = await page.waitApp('tx-hash', /^0x[0-9a-f]{64}$/i, 30000);
    const tx = await chain.getTransaction({ hash });
    assert(tx.from.toLowerCase() === address.toLowerCase(), `tx.from ${tx.from}`);
    assert(tx.chainId === 11155111 && tx.type === 'eip1559', `chain ${tx.chainId}, type ${tx.type}`);
    assert(tx.value === parseEther('0.01'), 'value 0.01 ETH');
    const receipt = await chain.waitForTransactionReceipt({ hash });
    assert(receipt.status === 'success', 'mined: ' + receipt.status);
    return `${hash} (from ${tx.from}, block ${receipt.blockNumber})`;
  });

  await step('5. sendPrivately -> "Send privately?" sheet -> the wallet answers through the protocol', async () => {
    await page.appClick('private-send');
    await page.waitH1(/Send privately/);
    const preview = await page.wallet(`document.querySelector('[data-test="private-preview"]').innerText`);
    assert(/0\.0010 ETH/.test(preview), 'sheet shows the amount: ' + preview);
    await page.click('approve');
    // The wallet's private send (Privacy Pools, real backend) either sends (`private_sent`) or
    // refuses with a reason (`rejected`), which the SDK surfaces as a typed LowkiError. Both are
    // a full protocol round trip; a timeout or `not_supported` is not.
    const out = await page.waitFor(async () => {
      const h = await page.appText('private-hash');
      if (/^0x[0-9a-f]+$/i.test(h)) return { hash: h };
      const err = await page.appText('private-error');
      return err && { err };
    }, 'private_sent or a wallet rejection', 120000);
    if (out.err) {
      assert(/^(transaction_failure|unknown_embedded_wallet_error|invalid_data): /.test(out.err) && !/not support|did not answer/.test(out.err), 'a wallet answer, not an SDK failure: ' + out.err);
      // Close the sheet that shows the reason.
      await page.wallet(`document.querySelector('[aria-label="Close"], [data-test="reject"]').click()`);
      return `wallet answered rejected: ${out.err}`;
    }
    return `private_sent ${out.hash}`;
  });

  await step('   getAccessToken again: no wallet UI', async () => {
    await page.appClick('token');
    await page.waitApp('access-token', /^ey/);
    await verifyToken(await page.appText('access-token'));
    assert(!(await page.overlay()), 'no UI for the token');
  });

  await step('6. Safari UA: login through a popup (email + recovery code + new passkey there), then the next request goes through the iframe', async () => {
    // A fresh page load: the iframe's key worker is locked, the app is signed out.
    await page.s('Emulation.setUserAgentOverride', { userAgent: SAFARI_UA });
    await page.navigate(appUrl());
    await page.waitApp('status', /^signed-out$/);
    // Follow the popup: a new page target whose opener is the app.
    await send('Target.setDiscoverTargets', { discover: true });
    const popupTarget = new Promise((r) => listeners.push((m) => {
      if (m.method === 'Target.targetCreated' && m.params.targetInfo.type === 'page' && m.params.targetInfo.openerId === targetId) r(m.params.targetInfo.targetId);
    }));
    await page.appClick('login');
    const popupId = await Promise.race([popupTarget, sleep(10000).then(() => { throw new Error('no popup opened'); })]);
    assert(!(await page.overlay()), 'the iframe modal stays hidden: sign-in is in the popup');
    const { sessionId: ps } = await send('Target.attachToTarget', { targetId: popupId, flatten: true });
    const popup = pageApi(ps, 'popup');
    await popup.s('Runtime.enable');
    await popup.s('WebAuthn.enable', { enableUI: false });
    // A virtual authenticator belongs to one page, and its PRF secrets can't be copied: the
    // popup has its own authenticator, so the popup creates a new passkey for the same wallet
    // (email + recovery code), exactly the cross-device path. The app page's passkey stays.
    await popup.s('WebAuthn.addVirtualAuthenticator', { options: AUTH });
    await popup.waitH1(/Continue to Privy Swap/);
    assert(await popup.top(`!!window.opener`), 'popup has its opener');
    await emailSignIn(popup);
    await popup.waitH1(/Welcome back/);
    await popup.click('use-recovery-code');
    await popup.waitH1(/Enter your recovery code/);
    await popup.type('recovery-input', recoveryCode);
    await popup.click('recover');
    await popup.waitH1(/Your wallet is back/);
    await popup.click('create-passkey');
    // The popup posts `authenticated` to its opener (the protocol.rs opener fix), and the SDK closes it.
    await page.waitApp('status', /^authenticated$/, 30000);
    assert((await page.appText('address')) === address, 'same address after the popup sign-in');
    assert((await page.appText('user-id')) === userId, 'same pairwise ID');
    await page.waitFor(async () => !(await send('Target.getTargets')).targetInfos.some((t) => t.targetId === popupId), 'the SDK closed the popup');

    // Next request: the iframe has no session (own document, own key worker). The SDK signs
    // it in first: the wallet's sign-in sheet in the iframe, passkey `get` (the app page's
    // passkey, created in step 1).
    await page.appClick('sign');
    await page.waitH1(/Continue to Privy Swap/);
    assert(await page.overlay(), 'iframe shown');
    await page.click('passkey-signin');
    await page.waitH1(/Approve this message/, 30000);
    await page.click('approve');
    const sig = await page.waitFor(async () => {
      const v = await page.appText('signature');
      return /^0x[0-9a-f]{130}$/i.test(v) && v;
    }, 'signature from the iframe');
    assert(await verifyMessage({ address, message: SWAP_MESSAGE, signature: sig }), 'verifyMessage');
    const { credentials } = await page.s('WebAuthn.getCredentials', { authenticatorId });
    assert(credentials.length === 1, 'the app page used its own passkey');
    return `popup login -> ${address}; the iframe then signed (verifies)`;
  });
} catch (e) {
  failed = true;
  try { console.log('wallet screen:\n  ' + (await page.walletText()).replace(/\n+/g, ' | ')); } catch {}
  if (consoleLines.length) console.log('browser console:\n  ' + consoleLines.slice(-30).join('\n  '));
} finally {
  ws.close();
  for (const p of procs) try { p.kill('SIGKILL'); } catch {}
  try { rmSync(profile, { recursive: true, force: true }); } catch {}
  servers.forEach((x) => x.close());
}
const autofocus = consoleLines.filter((l) => /autofocus/i.test(l));
if (autofocus.length) console.log('autofocus warnings:\n  ' + autofocus.join('\n  '));
console.log(failed ? `FAIL (${results.length} steps passed)` : `PASS all ${results.length} steps`);
process.exit(failed ? 1 : 0);
