// Headless Chrome WebAuthn end-to-end test for lowki-server.
//
// Uses a CDP virtual authenticator (ctap2, resident keys, user verification on, PRF on) and
// drives the real endpoints from a page on the server's origin:
//   register (no session) -> passkey assertion with PRF -> wrap a seed with HKDF(PRF) -> PUT blob
//   -> logout -> assertion with PRF -> blob returned by login/finish -> unwrap -> GET blob.
//
// Usage: node webauthn-e2e.mjs [server_url]   (default http://localhost:18080)
// Env:   CHROME=/path/to/chrome   (default: macOS Google Chrome)
// Needs Node >= 22 (global WebSocket). No npm dependencies.

import { spawn } from 'node:child_process';
import { mkdtempSync, rmSync } from 'node:fs';
import { tmpdir } from 'node:os';
import { join } from 'node:path';

const SERVER = process.argv[2] || 'http://localhost:18080';
const CHROME = process.env.CHROME || '/Applications/Google Chrome.app/Contents/MacOS/Google Chrome';

const profile = mkdtempSync(join(tmpdir(), 'lowki-e2e-'));
const chrome = spawn(CHROME, [
  '--headless=new', '--remote-debugging-port=0', `--user-data-dir=${profile}`,
  '--no-first-run', '--no-default-browser-check', '--disable-gpu', 'about:blank',
], { stdio: ['ignore', 'ignore', 'pipe'] });

function cleanup() {
  try { chrome.kill('SIGKILL'); } catch {}
  try { rmSync(profile, { recursive: true, force: true }); } catch {}
}

const wsUrl = await new Promise((resolve, reject) => {
  let buf = '';
  const t = setTimeout(() => reject(new Error('Chrome did not start')), 20000);
  chrome.stderr.on('data', (d) => {
    buf += d;
    const m = buf.match(/DevTools listening on (ws:\/\/\S+)/);
    if (m) { clearTimeout(t); resolve(m[1]); }
  });
  chrome.on('exit', (c) => reject(new Error(`Chrome exited ${c}`)));
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
  } else if (msg.method) {
    listeners.forEach((l) => l(msg));
  }
};
const send = (method, params = {}, sessionId) => new Promise((resolve, reject) => {
  const id = nextId++;
  pending.set(id, { resolve, reject });
  ws.send(JSON.stringify({ id, method, params, ...(sessionId ? { sessionId } : {}) }));
});

let failed = false;
try {
  const browser = await send('Browser.getVersion');
  console.log(`browser: ${browser.product}`);
  const { targetId } = await send('Target.createTarget', { url: 'about:blank' });
  const { sessionId } = await send('Target.attachToTarget', { targetId, flatten: true });
  const s = (m, p) => send(m, p, sessionId);

  await s('Page.enable');
  await s('WebAuthn.enable', { enableUI: false });
  const { authenticatorId } = await s('WebAuthn.addVirtualAuthenticator', {
    options: {
      protocol: 'ctap2', ctap2Version: 'ctap2_1', transport: 'internal',
      hasResidentKey: true, hasUserVerification: true, isUserVerified: true,
      hasPrf: true, automaticPresenceSimulation: true,
    },
  });
  console.log(`virtual authenticator: ${authenticatorId} (ctap2.1, rk, uv, prf)`);

  const loaded = new Promise((r) => listeners.push((m) => m.method === 'Page.loadEventFired' && m.sessionId === sessionId && r()));
  await s('Page.navigate', { url: `${SERVER}/healthz` });
  await loaded;

  const result = await s('Runtime.evaluate', {
    expression: `(${pageFlow.toString()})()`,
    awaitPromise: true, returnByValue: true, userGesture: true,
  });
  if (result.exceptionDetails) {
    throw new Error(result.exceptionDetails.exception?.description || JSON.stringify(result.exceptionDetails));
  }
  const out = result.result.value;
  for (const step of out.steps) console.log(`  ok  ${step}`);
  const { credentials } = await s('WebAuthn.getCredentials', { authenticatorId });
  console.log(`authenticator holds ${credentials.length} resident credential(s), signCount=${credentials[0]?.signCount}`);
  if (credentials.length !== 1 || !credentials[0].isResidentCredential) throw new Error('expected one resident credential');
  console.log('PASS webauthn e2e (register -> authenticate with PRF -> store/fetch wrapped blob)');
} catch (e) {
  failed = true;
  console.error(`FAIL ${e.message}`);
} finally {
  ws.close();
  cleanup();
}
process.exit(failed ? 1 : 0);

// ---- runs inside the page (Lowki origin) ------------------------------------------------------
async function pageFlow() {
  const steps = [];
  const assert = (c, m) => { if (!c) throw new Error(`assertion failed: ${m}`); };
  const enc = new TextEncoder();
  const b64u = (buf) => btoa(String.fromCharCode(...new Uint8Array(buf))).replace(/\+/g, '-').replace(/\//g, '_').replace(/=+$/, '');
  const unb64u = (s) => Uint8Array.from(atob(s.replace(/-/g, '+').replace(/_/g, '/') + '==='.slice((s.length + 3) % 4)), (c) => c.charCodeAt(0));
  const call = async (method, path, body, csrf) => {
    const r = await fetch(path, {
      method, credentials: 'same-origin',
      headers: { 'content-type': 'application/json', ...(csrf ? { 'x-lowki-csrf': csrf } : {}) },
      body: body === undefined ? undefined : JSON.stringify(body),
    });
    const j = await r.json();
    return { status: r.status, body: j };
  };
  const ok = async (...a) => { const r = await call(...a); if (r.status !== 200) throw new Error(`${a[0]} ${a[1]} -> ${r.status} ${JSON.stringify(r.body)}`); return r.body; };
  const salt = new Uint8Array(await crypto.subtle.digest('SHA-256', enc.encode('lowki/prf-salt/test')));
  const dropNulls = (o) => JSON.parse(JSON.stringify(o, (_, v) => (v === null ? undefined : v)));
  // What the Lowki worker must do: never send PRF output to the server.
  const credJson = (cred, strip = true) => {
    const j = cred.toJSON();
    if (strip && j.clientExtensionResults?.prf) delete j.clientExtensionResults.prf.results;
    return j;
  };
  const kek = async (prfFirst) => {
    const ikm = await crypto.subtle.importKey('raw', prfFirst, 'HKDF', false, ['deriveKey']);
    return crypto.subtle.deriveKey({ name: 'HKDF', hash: 'SHA-256', salt: new Uint8Array(), info: enc.encode('lowki/kek/v1') },
      ikm, { name: 'AES-GCM', length: 256 }, false, ['encrypt', 'decrypt']);
  };
  const assertWithPrf = async (stripPrf = true) => {
    const start = await ok('POST', '/v1/passkeys/login/start', {});
    const pk = dropNulls(start.options.publicKey);
    assert(!('mediation' in start.options), 'server leaves mediation to the client');
    const cred = await navigator.credentials.get({
      publicKey: {
        ...pk,
        challenge: unb64u(pk.challenge),
        allowCredentials: (pk.allowCredentials || []).map((c) => ({ ...c, id: unb64u(c.id) })),
        extensions: { ...(pk.extensions || {}), prf: { eval: { first: salt } } },
      },
    });
    const prf = cred.getClientExtensionResults().prf?.results?.first;
    assert(prf && prf.byteLength === 32, 'assertion returned a 32-byte PRF output');
    const fin = await ok('POST', '/v1/passkeys/login/finish', { flow_id: start.flow_id, credential: credJson(cred, stripPrf) });
    return { fin, prf };
  };

  // 1. Registration ceremony (no session -> new user), PRF requested.
  const rs = await ok('POST', '/v1/passkeys/register/start', { name: 'Lowki e2e' });
  const cpk = dropNulls(rs.options.publicKey);
  assert(cpk.extensions?.prf && typeof cpk.extensions.prf === 'object', 'server options carry extensions.prf');
  assert(cpk.authenticatorSelection.residentKey === 'required', 'discoverable credential required');
  assert(cpk.authenticatorSelection.userVerification === 'required', 'UV required');
  const created = await navigator.credentials.create({
    publicKey: {
      ...cpk,
      challenge: unb64u(cpk.challenge),
      user: { ...cpk.user, id: unb64u(cpk.user.id) },
      excludeCredentials: (cpk.excludeCredentials || []).map((c) => ({ ...c, id: unb64u(c.id) })),
      extensions: { ...cpk.extensions, prf: { eval: { first: salt } } },
    },
  });
  const ext = created.getClientExtensionResults();
  assert(ext.prf?.enabled === true, 'authenticator reports prf.enabled');
  const reg = await ok('POST', '/v1/passkeys/register/finish', { flow_id: rs.flow_id, credential: credJson(created) });
  assert(reg.new_user === true && reg.credential_id === created.id, 'registration created a user and stored the credential');
  steps.push(`register: credential ${reg.credential_id.slice(0, 12)}..., prf.enabled=true, new user`);

  // 2. Authenticate with PRF; wrap a 16-byte seed under HKDF(PRF, "lowki/kek/v1"); store the blob.
  const a1 = await assertWithPrf();
  assert(a1.fin.credential_id === reg.credential_id && a1.fin.wrapped_blob === null, 'first assertion: no blob yet');
  const entropy = crypto.getRandomValues(new Uint8Array(16));
  const nonce = crypto.getRandomValues(new Uint8Array(12));
  const ct = new Uint8Array(await crypto.subtle.encrypt({ name: 'AES-GCM', iv: nonce }, await kek(a1.prf), entropy));
  const blob = new Uint8Array([1, ...nonce, ...ct]);
  await ok('PUT', `/v1/passkeys/${reg.credential_id}/blob`, { blob: b64u(blob) }, a1.fin.csrf_token);
  steps.push(`authenticate (UV + PRF) and store wrapped blob (${blob.length} bytes)`);

  // 3. Sign out; the blob is not readable without a session.
  await ok('POST', '/v1/session/logout', {});
  const anon = await call('GET', `/v1/passkeys/${reg.credential_id}/blob`);
  assert(anon.status === 401, `blob fetch without assertion is refused (${anon.status})`);
  steps.push('logout: blob fetch without a session -> 401');

  // 4. New assertion: login/finish returns the blob, and the same PRF output unwraps it.
  //    This time the PRF results are left in the payload: the server must accept and drop them.
  const a2 = await assertWithPrf(false);
  assert(b64u(a2.prf) === b64u(a1.prf), 'PRF output is stable across assertions');
  assert(a2.fin.wrapped_blob === b64u(blob), 'login/finish returns the stored blob');
  const got = unb64u(a2.fin.wrapped_blob);
  const pt = new Uint8Array(await crypto.subtle.decrypt({ name: 'AES-GCM', iv: got.slice(1, 13) }, await kek(a2.prf), got.slice(13)));
  assert(b64u(pt) === b64u(entropy), 'unwrapped seed matches');
  steps.push('authenticate again: blob returned, PRF stable, seed unwrapped client-side');

  // 5. Explicit fetch with the fresh passkey session.
  const fetched = await ok('GET', `/v1/passkeys/${reg.credential_id}/blob`);
  assert(fetched.wrapped_blob === b64u(blob), 'GET blob returns the same bytes');
  const sess = await ok('GET', '/v1/session');
  assert(sess.methods.length === 1 && sess.methods[0].kind === 'passkey', 'one passkey method');
  steps.push('GET blob with fresh assertion -> same bytes; session lists 1 passkey method');

  // 6. Adding a passkey to an existing account needs the CSRF header, and excludeCredentials
  //    stops the same authenticator from registering twice.
  const noCsrf = await call('POST', '/v1/passkeys/register/start', {});
  assert(noCsrf.status === 403, `register/start with a session but no CSRF header -> 403 (${noCsrf.status})`);
  const rs2 = await ok('POST', '/v1/passkeys/register/start', {}, sess.csrf_token);
  const p2 = dropNulls(rs2.options.publicKey);
  assert(p2.excludeCredentials?.length === 1 && p2.excludeCredentials[0].id === reg.credential_id, 'existing credential excluded');
  let excluded = false;
  try {
    await navigator.credentials.create({ publicKey: { ...p2, challenge: unb64u(p2.challenge), user: { ...p2.user, id: unb64u(p2.user.id) },
      excludeCredentials: p2.excludeCredentials.map((c) => ({ ...c, id: unb64u(c.id) })) } });
  } catch (e) { excluded = e.name === 'InvalidStateError'; }
  assert(excluded, 'authenticator refuses a duplicate registration');
  steps.push('link flow: CSRF enforced; excludeCredentials prevents duplicate passkey');
  return { steps };
}
