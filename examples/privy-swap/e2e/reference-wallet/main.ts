/**
 * Reference wallet for the SDK's end-to-end test: speaks the Lowki postMessage
 * protocol (v1, plus the proposed `capabilities` on `ready` and
 * `send_privately` / `private_sent`) and signs with a real secp256k1 key, so
 * the test can verify signatures. A fixed public dev seed: never use for funds.
 */
import { concat, keccak256, toBytes, toHex, type Hex, type TypedDataDefinition } from 'viem';
import { privateKeyToAccount } from 'viem/accounts';

const SEED = keccak256(toBytes('lowki/reference-wallet/dev-seed/DO-NOT-USE'));
const q = new URLSearchParams(location.search);
const appId = q.get('app') ?? '';
const appOrigin = q.get('origin') ?? '';
const target: Window | null = window.opener ?? (window.parent !== window ? window.parent : null);

// Pairwise, like Lowki: a different key and ID per app.
const account = privateKeyToAccount(keccak256(concat([SEED, toHex(toBytes('lowki/app/v1')), toHex(toBytes(appId))])));
const userId = 'did:lowki:' + keccak256(concat([SEED, toHex(toBytes('pairwise')), toHex(toBytes(appId))])).slice(2, 34);
let signedIn = false;
let open: string | null = null;

const b64 = (o: unknown) => btoa(JSON.stringify(o)).replace(/=+$/, '').replace(/\+/g, '-').replace(/\//g, '_');
const token = () => {
  const now = Math.floor(Date.now() / 1000);
  return `${b64({ alg: 'none', typ: 'JWT' })}.${b64({ iss: location.origin, aud: appId, sub: userId, iat: now, exp: now + 900 })}.`;
};

function post(msg: Record<string, unknown>) {
  if (!target || !appOrigin || appOrigin === '*') return;
  target.postMessage({ lowki: 1, ...msg }, appOrigin);
}

const el = (id: string) => document.getElementById(id)!;
function ask(title: string, detail: unknown): Promise<boolean> {
  el('title').textContent = title;
  el('detail').textContent = typeof detail === 'string' ? detail : JSON.stringify(detail, null, 2);
  el('sheet').hidden = false;
  return new Promise((res) => {
    const done = (ok: boolean) => {
      el('sheet').hidden = true;
      el('approve').onclick = el('reject').onclick = null;
      res(ok);
    };
    el('approve').onclick = () => done(true);
    el('reject').onclick = () => done(false);
  });
}

/** EIP-712 values arrive as JSON (numbers as strings); viem wants bigints for (u)int fields. */
function coerce(types: Record<string, { name: string; type: string }[]>, type: string, v: unknown): unknown {
  if (type.endsWith('[]')) return (v as unknown[]).map((x) => coerce(types, type.slice(0, -2), x));
  if (/^u?int\d*$/.test(type)) return BigInt(v as string);
  const fields = types[type];
  if (!fields) return v;
  const o = v as Record<string, unknown>;
  return Object.fromEntries(fields.map((f) => [f.name, coerce(types, f.type, o[f.name])]));
}

async function handle(m: any) {
  const reply = (d: Record<string, unknown>) => post({ id: m.id, ...d });
  if (m.type === 'sign_in') {
    if (signedIn) return reply({ type: 'authenticated', address: account.address, user_id: userId, access_token: token() });
    if (!(await ask('Continue with passkey', `Sign in to ${appId}`))) return reply({ type: 'rejected', reason: 'user_rejected', message: 'User closed sign-in' });
    signedIn = true;
    return reply({ type: 'authenticated', address: account.address, user_id: userId, access_token: token() });
  }
  if (m.type === 'sign_out') {
    signedIn = false;
    return reply({ type: 'signed_out' });
  }
  if (!signedIn) return reply({ type: 'rejected', reason: 'not_signed_in', message: 'Sign in first' });
  if (open) return reply({ type: 'rejected', reason: 'busy', message: 'Another request is open' });
  open = m.id;
  try {
    switch (m.type) {
      case 'sign_message': {
        if (!(await ask('Sign message', m.message))) return reply({ type: 'rejected', reason: 'user_rejected', message: 'Rejected' });
        return reply({ type: 'message_signed', signature: await account.signMessage({ message: m.message }) });
      }
      case 'sign_typed_data': {
        const t = m.typed_data;
        if (!(await ask('Sign typed data', t))) return reply({ type: 'rejected', reason: 'user_rejected', message: 'Rejected' });
        const types = { ...t.types };
        delete types.EIP712Domain;
        const def = { domain: t.domain, types, primaryType: t.primaryType, message: coerce(types, t.primaryType, t.message) } as TypedDataDefinition;
        return reply({ type: 'typed_data_signed', signature: await account.signTypedData(def) });
      }
      case 'send_transaction': {
        if (!(await ask('Send transaction', m.tx))) return reply({ type: 'rejected', reason: 'user_rejected', message: 'Rejected' });
        // Not broadcast: the hash commits to what was approved.
        return reply({ type: 'transaction_sent', hash: keccak256(toBytes(JSON.stringify(m.tx))), sponsored: false });
      }
      case 'send_privately': {
        if (!(await ask('Send privately', { to: m.to, amount: m.amount, asset: m.asset }))) return reply({ type: 'rejected', reason: 'user_rejected', message: 'Rejected' });
        await new Promise((r) => setTimeout(r, 300)); // "proof"
        return reply({ type: 'private_sent', hash: keccak256(toBytes(`private:${m.to}:${m.amount}:${m.asset}:${Date.now()}`)) as Hex });
      }
      default:
        return reply({ type: 'rejected', reason: 'policy_rejected', message: `Unsupported: ${m.type}` });
    }
  } catch (e) {
    return reply({ type: 'rejected', reason: 'network_error', message: e instanceof Error ? e.message : String(e) });
  } finally {
    open = null;
  }
}

window.addEventListener('message', (ev) => {
  if (ev.origin !== appOrigin || ev.source !== target) return; // strict, like the real wallet should be
  const m = ev.data;
  if (!m || m.lowki !== 1 || typeof m.type !== 'string' || typeof m.id !== 'string') return;
  void handle(m);
});

post({ type: 'ready', app_id: appId, capabilities: ['sign_in', 'sign_message', 'sign_typed_data', 'send_transaction', 'sign_out', 'send_privately'] });
