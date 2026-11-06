// End-to-end: the Privy-docs example app (import swapped to @lowki/react-auth)
// in headless Chrome, against
//   A. apps/wallet-ui with its mock backend (built from git HEAD), and
//   B. the reference wallet (e2e/reference-wallet), which signs with a real key
//      and implements the proposed `send_privately`.
//
//   npx vite build && node e2e/run.mjs
//
// Env: CHROMEDRIVER, WALLET_DIST (wallet-ui `trunk build` output), APP_DIST.
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { recoverTypedDataAddress, verifyMessage, isAddress, isHex } from 'viem';
import { Browser, serve, sleep, startDriver } from './wd.mjs';

const here = path.dirname(fileURLToPath(import.meta.url));
const REPO = path.resolve(here, '../../..');
const APP_DIST = process.env.APP_DIST || path.resolve(here, '../dist');
const WALLET_DIST = process.env.WALLET_DIST || path.join(REPO, 'target/react-auth-e2e/wallet-dist');
const DRIVER = process.env.CHROMEDRIVER || path.join(REPO, 'target/tools/chromedriver/mac_arm-156.0.8078.80/chromedriver-mac-arm64/chromedriver');
const [P_WALLET, P_APP, P_REF, P_DRIVER] = [8720, 8721, 8722, 9619];

const SWAP_MESSAGE = 'Privy Swap wants you to sign in.\n\nNonce: 7f3a9c';
const QUOTE = {
  domain: { name: 'Privy Swap', version: '1', chainId: 11155111, verifyingContract: '0xeE567Fe1712Faf6149d80dA1E6934E354124CfE3' },
  types: { Quote: [{ name: 'sellToken', type: 'address' }, { name: 'buyToken', type: 'address' }, { name: 'sellAmount', type: 'uint256' }] },
  primaryType: 'Quote',
  message: { sellToken: '0xfFf9976782d46CC05630D1f6eBAb18b2324d6B14', buyToken: '0x1c7D4B196Cb0C7B01d743Fbc6116a902379C7238', sellAmount: 10000000000000000n },
};

const results = [];
const notes = [];
async function step(scenario, name, fn, { expectFail = false } = {}) {
  const t = Date.now();
  try {
    const info = await fn();
    results.push({ scenario, name, ok: true });
    console.log(`PASS  [${scenario}] ${name} (${Date.now() - t} ms)${info ? '  ' + info : ''}`);
  } catch (e) {
    results.push({ scenario, name, ok: false, expectFail, error: e.message });
    console.log(`${expectFail ? 'XFAIL' : 'FAIL '} [${scenario}] ${name}\n      ${e.message}`);
    if (!expectFail) throw e;
  }
}
const assert = (c, msg) => { if (!c) throw new Error('assertion failed: ' + msg); };

const overlayShown = (b) => b.exec(`const r = document.querySelector('[data-lowki="wallet"]'); return !!r && r.style.display !== 'none';`);
const top = (b) => b.frame(null);
const wallet = async (b) => {
  await b.until(() => overlayShown(b), 'wallet overlay shown');
  await b.frame('iframe[title="Lowki wallet"]');
};
const tid = (id) => `[data-testid="${id}"]`;

async function scenarioMock(b) {
  const S = 'wallet-ui mock';
  const walletUrl = `http://127.0.0.1:${P_WALLET}/?speed=0.3&platform=mac`;
  await b.go(`http://127.0.0.1:${P_APP}/?wallet=${encodeURIComponent(walletUrl)}`);
  let address = '';
  let userId = '';

  await step(S, 'SDK ready, signed out; iframe hidden', async () => {
    await b.waitFor(tid('status'), { re: /^signed-out$/ });
    assert(!(await overlayShown(b)), 'iframe hidden while idle');
    const frames = await b.exec(`return document.querySelectorAll('[data-lowki="wallet"] iframe').length`);
    assert(frames === 1, `one wallet iframe after StrictMode double-mount, got ${frames}`);
  });

  await step(S, 'login: email code + passkey in the wallet iframe', async () => {
    await b.click(tid('login'));
    await wallet(b);
    await b.waitFor('#main h1', { re: /Continue to privy-swap/ });
    await b.click('[data-test="email-open"]');
    await b.type('[data-test="email-input"]', 'ada@example.com');
    await b.click('[data-test="email-continue"]');
    await b.type('[data-test="email-code"]', '424242');
    await b.click('[data-test="email-verify"]');
    await b.click('[data-test="create-passkey"]');
    await b.click('[data-test="recovery-done"]');
    await top(b);
  });

  await step(S, 'app shows the pairwise address and user ID; overlay hidden', async () => {
    await b.waitFor(tid('status'), { re: /^authenticated$/ });
    address = await b.text(tid('address'));
    userId = await b.text(tid('user-id'));
    assert(isAddress(address), 'address ' + address);
    assert(userId.length > 4, 'user id ' + userId);
    assert((await b.text(tid('linked'))) === 'wallet', 'linkedAccounts = wallet only (no email shared)');
    assert(!(await overlayShown(b)), 'overlay hidden after login');
    assert(/login complete: /.test(await b.text(tid('log'))), 'useLogin onComplete fired');
    return `${address} ${userId}`;
  });

  let signature = '';
  await step(S, 'signMessage: approve in wallet, app gets a signature', async () => {
    await b.click(tid('sign'));
    await wallet(b);
    await b.click('[data-test="approve"]');
    await top(b);
    signature = await (async () => { await b.waitFor(tid('signature'), { re: /^0x[0-9a-f]+$/i }); return b.text(tid('signature')); })();
    assert(isHex(signature) && signature.length === 132, '65-byte signature, got ' + signature.length);
    return signature.slice(0, 20) + '…';
  });

  await step(S, 'signature verifies for the address (viem verifyMessage)', async () => {
    let ok = false;
    try { ok = await verifyMessage({ address, message: SWAP_MESSAGE, signature }); } catch (e) { throw new Error(`viem could not even parse it: ${e.shortMessage ?? e.message}`); }
    assert(ok, 'mock signature does not verify (the wallet-ui mock returns FNV-hash hex, not an ECDSA signature)');
  }, { expectFail: true });

  await step(S, 'signTypedData: approve, app gets a signature', async () => {
    await b.click(tid('sign-typed'));
    await wallet(b);
    await b.click('[data-test="approve"]');
    await top(b);
    await b.waitFor(tid('typed-signature'), { re: /^0x[0-9a-f]{130}$/i });
  });

  await step(S, 'sendTransaction (swap): approve, app gets a hash', async () => {
    await b.click(tid('swap'));
    await wallet(b);
    await b.waitFor('[data-test="tx-preview"]');
    await b.click('[data-test="approve"]');
    await top(b);
    await b.waitFor(tid('tx-hash'), { re: /^0x[0-9a-f]{64}$/i });
    return await b.text(tid('tx-hash'));
  });

  await step(S, 'EIP-1193 provider via viem: switchChain + getChainId', async () => {
    await b.click(tid('viem'));
    await b.waitFor(tid('viem-chain'), { re: /^11155111$/ });
  });

  await step(S, 'getAccessToken (standalone export) returns the token', async () => {
    await b.click(tid('token'));
    await b.waitFor(tid('access-token'), { re: /\S{20,}/ });
  });

  await step(S, 'sendPrivately completes', async () => {
    await b.click(tid('private-send'));
    await b.waitFor(tid('private-hash'), { re: /^0x/, timeout: 4000 }).catch(async () => {
      throw new Error('no hash; app shows: ' + (await b.text(tid('private-error'))));
    });
  }, { expectFail: true });

  await step(S, 'logout: app and wallet signed out', async () => {
    await b.click(tid('logout'));
    await b.waitFor(tid('status'), { re: /^signed-out$/ });
  });
}

async function scenarioReference(b) {
  const S = 'reference wallet';
  const walletUrl = `http://127.0.0.1:${P_REF}/e2e/reference-wallet/index.html`;
  await b.go(`http://127.0.0.1:${P_APP}/?wallet=${encodeURIComponent(walletUrl)}`);
  let address = '';
  let userId = '';
  const approve = async () => { await wallet(b); await b.click('[data-test="approve"]'); await top(b); };

  await step(S, 'login', async () => {
    await b.waitFor(tid('status'), { re: /^signed-out$/ });
    await b.click(tid('login'));
    await approve();
    await b.waitFor(tid('status'), { re: /^authenticated$/ });
    address = await b.text(tid('address'));
    userId = await b.text(tid('user-id'));
    assert(isAddress(address) && /^did:lowki:[0-9a-f]{32}$/.test(userId), `${address} ${userId}`);
    return `${address} ${userId}`;
  });

  await step(S, 'signMessage returns a valid signature for that address (viem)', async () => {
    await b.click(tid('sign'));
    await approve();
    await b.waitFor(tid('signature'), { re: /^0x[0-9a-f]{130}$/i });
    const signature = await b.text(tid('signature'));
    assert(await verifyMessage({ address, message: SWAP_MESSAGE, signature }), 'verifyMessage');
  });

  await step(S, 'signTypedData recovers to that address (viem)', async () => {
    await b.click(tid('sign-typed'));
    await approve();
    await b.waitFor(tid('typed-signature'), { re: /^0x[0-9a-f]{130}$/i });
    const signature = await b.text(tid('typed-signature'));
    const who = await recoverTypedDataAddress({ ...QUOTE, signature });
    assert(who.toLowerCase() === address.toLowerCase(), `recovered ${who}`);
  });

  await step(S, 'sendTransaction returns a hash', async () => {
    await b.click(tid('swap'));
    await approve();
    await b.waitFor(tid('tx-hash'), { re: /^0x[0-9a-f]{64}$/i });
  });

  await step(S, 'sendPrivately completes', async () => {
    await b.click(tid('private-send'));
    await approve();
    await b.waitFor(tid('private-hash'), { re: /^0x[0-9a-f]{64}$/i });
    return await b.text(tid('private-hash'));
  });

  await step(S, 'access token sub = user.id, aud = app ID', async () => {
    await b.click(tid('token'));
    await b.waitFor(tid('access-token'), { re: /\./ });
    const claims = JSON.parse(Buffer.from((await b.text(tid('access-token'))).split('.')[1], 'base64url').toString());
    assert(claims.sub === userId && claims.aud === 'privy-swap', JSON.stringify(claims));
  });

  await step(S, 'rejecting in the wallet surfaces a typed error', async () => {
    await b.click(tid('sign'));
    await wallet(b);
    await b.click('[data-test="reject"]');
    await top(b);
    await b.waitFor(tid('log'), { re: /sign error: unable_to_sign/ });
  });
}

const SAFARI_UA = 'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/19.0 Safari/605.1.15';

/** With a Safari UA the SDK signs in through a popup (Safari can't create passkeys in cross-origin iframes). */
async function scenarioSafariPopup(b) {
  await b.cdp('Emulation.setUserAgentOverride', { userAgent: SAFARI_UA });
  const main = await b.handle();
  const popupOpens = async () => {
    await b.waitFor(tid('status'), { re: /^signed-out$/ });
    await b.click(tid('login'));
    await b.until(async () => (await b.handles()).length === 2, 'popup window');
    assert(!(await overlayShown(b)), 'iframe modal stays hidden');
    await b.window((await b.handles()).find((h) => h !== main));
  };
  const closeExtra = async () => {
    for (const h of await b.handles()) if (h !== main) { await b.window(h); await b.cmd('DELETE', '/window').catch(() => {}); }
    await b.window(main);
  };

  // C1: apps/wallet-ui in a popup.
  let S = 'wallet-ui mock, Safari UA (popup)';
  await b.go(`http://127.0.0.1:${P_APP}/?wallet=${encodeURIComponent(`http://127.0.0.1:${P_WALLET}/?speed=0.3&platform=mac`)}`);
  await step(S, 'login opens the wallet in a popup', async () => {
    await popupOpens();
    await b.waitFor('#main h1', { re: /Continue to privy-swap/, timeout: 3000 }); // its own UI, no `sign_in` needed
  });
  await step(S, 'popup sign-in reaches the app', async () => {
    await b.click('[data-test="email-open"]');
    await b.type('[data-test="email-input"]', 'ada@example.com');
    await b.click('[data-test="email-continue"]');
    await b.type('[data-test="email-code"]', '424242');
    await b.click('[data-test="email-verify"]');
    await b.click('[data-test="create-passkey"]');
    await b.click('[data-test="recovery-done"]');
    const opener = await b.exec('return window.opener instanceof Window');
    await b.window(main);
    await b.waitFor(tid('status'), { re: /^authenticated$/, timeout: 4000 }).catch(() => {
      throw new Error(`the popup never posted to its opener (not even \`ready\`). In the popup, \`window.opener instanceof Window\` is ${opener}, so wallet-ui protocol.rs post() (opener.dyn_into::<Window>) falls through to "standalone"`);
    });
  }, { expectFail: true });
  await closeExtra();

  // C2: the reference wallet in a popup: the SDK side of the popup flow.
  S = 'reference wallet, Safari UA (popup)';
  await b.go(`http://127.0.0.1:${P_APP}/?wallet=${encodeURIComponent(`http://127.0.0.1:${P_REF}/e2e/reference-wallet/index.html`)}`);
  await step(S, 'login through the popup; the SDK closes it', async () => {
    await popupOpens();
    await b.click('[data-test="approve"]');
    await b.window(main);
    await b.until(async () => (await b.handles()).length === 1, 'popup closed by the SDK');
    await b.waitFor(tid('status'), { re: /^authenticated$/ });
    return await b.text(tid('address'));
  });
  await step(S, 'after popup sign-in, the iframe can sign', async () => {
    await b.click(tid('sign'));
    await b.waitFor(tid('log'), { re: /sign error: \w+/, timeout: 3000 }).then(async () => {
      throw new Error('the iframe has no session after a popup sign-in (' + (await b.text(tid('log'))).split('\n')[0] + '): protocol has no session hand-off');
    }, () => undefined);
  }, { expectFail: true });
  await closeExtra();
  await b.cdp('Emulation.setUserAgentOverride', { userAgent: '' }).catch(() => {});
}

const servers = [await serve(WALLET_DIST, P_WALLET), await serve(APP_DIST, P_APP), await serve(APP_DIST, P_REF)];
const { proc } = await startDriver(DRIVER, P_DRIVER);
const b = await new Browser(`http://127.0.0.1:${P_DRIVER}`).open();
let crashed = null;
try {
  for (const run of [scenarioMock, scenarioReference, scenarioSafariPopup]) {
    try { await run(b); } catch (e) { crashed ??= e; await top(b).catch(() => {}); }
  }
} finally {
  const logs = await b.logs();
  const errors = (logs || []).filter((l) => l.level === 'SEVERE').map((l) => l.message);
  if (errors.length) notes.push('browser console errors:\n  ' + errors.join('\n  '));
  await b.close();
  proc.kill();
  for (const s of servers) s.close();
}
const failed = results.filter((r) => !r.ok && !r.expectFail);
const xfail = results.filter((r) => !r.ok && r.expectFail);
console.log(`\n${results.filter((r) => r.ok).length} passed, ${failed.length} failed, ${xfail.length} expected failures (protocol/mock gaps)`);
for (const n of notes) console.log(n);
await sleep(50);
process.exit(failed.length || crashed ? 1 : 0);
