// Differential vectors for crates/lowki-shares against Privy's shamir-secret-sharing 0.0.4.
//
// Run with `npm run gen` (node --import ./rng-hook.mjs --import tsx gen.ts). The hook
// replaces the package's csprng with a replay source, so every random byte that split()
// consumes comes from a fixed-seed SHA-256 counter DRBG and is recorded in the vectors.
// The Rust test feeds the same bytes to `split` and must produce identical shares.
import { createHash } from 'node:crypto';
import { readFileSync, writeFileSync } from 'node:fs';
import { createRequire } from 'node:module';
import { split, combine } from 'shamir-secret-sharing';

const g = globalThis as any;
const hex = (b: Uint8Array) => Buffer.from(b).toString('hex');

// SHA-256(seed || u64be counter) byte stream.
function drbg(seed: string) {
  let counter = 0n;
  let buf = new Uint8Array(0);
  return (n: number): Uint8Array => {
    while (buf.length < n) {
      const c = Buffer.alloc(8);
      c.writeBigUInt64BE(counter++);
      const block = createHash('sha256').update(seed).update(c).digest();
      const nb = new Uint8Array(buf.length + block.length);
      nb.set(buf);
      nb.set(block, buf.length);
      buf = nb;
    }
    const out = buf.slice(0, n);
    buf = buf.slice(n);
    return out;
  };
}

const params = drbg('lowki-shares-vectors-v1/params');
const rint = (lo: number, hi: number) => {
  // inclusive, uniform enough for test parameters
  const b = params(4);
  return lo + (((b[0] << 24) | (b[1] << 16) | (b[2] << 8) | b[3]) >>> 0) % (hi - lo + 1);
};

// Deterministic subset selection (indices), `k` out of `n`.
function pick(n: number, k: number): number[] {
  const idx = [...Array(n).keys()];
  for (let i = n - 1; i > 0; i--) {
    const j = rint(0, i);
    [idx[i], idx[j]] = [idx[j], idx[i]];
  }
  return idx.slice(0, k);
}

type Case = {
  secret: string;
  n: number;
  t: number;
  random: string; // exact bytes split() consumed, in order
  shares: string[];
  // JS combine() results on subsets of the JS shares
  subsets: { idx: number[]; secret: string }[];
};

const cases: Case[] = [];
const lens = [1, 2, 3, 16, 17, 31, 32, 33, 64, 65, 100, 255, 256];
const nt: [number, number][] = [
  [2, 2], [3, 2], [3, 3], [5, 3], [5, 5], [10, 2], [10, 7], [16, 16], [255, 2], [255, 255], [100, 50],
];

async function addCase(secret: Uint8Array, n: number, t: number, seed: string) {
  g.__lowkiRng = drbg(seed);
  g.__lowkiRngLog = [];
  const shares = await split(secret, n, t);
  const consumed: Uint8Array[] = g.__lowkiRngLog;
  const random = Buffer.concat(consumed.map((b) => Buffer.from(b)));
  if (random.length !== 255 + secret.length * (t - 1)) throw new Error('unexpected RNG consumption');
  const subsets: Case['subsets'] = [];
  // exact threshold, a random superset, all shares
  const sizes = Array.from(new Set([t, rint(t, n), n]));
  for (const k of sizes) {
    const idx = pick(n, k);
    const s = await combine(idx.map((i) => shares[i]));
    if (hex(s) !== hex(secret)) throw new Error('JS combine mismatch');
    subsets.push({ idx, secret: hex(s) });
  }
  cases.push({ secret: hex(secret), n, t, random: random.toString('hex'), shares: shares.map(hex), subsets });
}

let caseNo = 0;
// edge secrets
const edge = [new Uint8Array(32), new Uint8Array(32).fill(0xff), Uint8Array.from({ length: 32 }, (_, i) => i), new Uint8Array([0]), new Uint8Array([0xff])];
for (const s of edge) for (const [n, t] of [[3, 2], [5, 3]] as const) await addCase(s, n, t, `case-${caseNo++}`);
// grid over lengths × (n, t); keep the largest combos to short secrets to bound file size
for (const len of lens) {
  for (const [n, t] of nt) {
    if (n * len > 20000 || (t - 1) * len > 8000) continue;
    const secret = params(len);
    await addCase(secret, n, t, `case-${caseNo++}`);
  }
}
// random parameters
for (let i = 0; i < 60; i++) {
  const n = rint(2, 40);
  const t = rint(2, n);
  const secret = params(rint(1, 80));
  await addCase(secret, n, t, `case-${caseNo++}`);
}
// Lowki's shape: 2-of-3 over 32-byte BIP-39 entropy (33-byte shares) and 16-byte (17-byte shares)
for (let i = 0; i < 16; i++) await addCase(params(i % 2 ? 16 : 32), 3, 2, `case-${caseNo++}`);

// Privy's combine does not take a threshold: with t-1 shares it returns a wrong secret
// instead of failing. Recorded so the Rust test can assert it rejects the same input.
const tooFew: { shares: string[]; t: number; secret: string; privy_result: string }[] = [];
for (const c of cases.filter((c) => c.t >= 3).slice(0, 12)) {
  const shares = c.shares.slice(0, c.t - 1).map((h) => Uint8Array.from(Buffer.from(h, 'hex')));
  const r = await combine(shares);
  tooFew.push({ shares: c.shares.slice(0, c.t - 1), t: c.t, secret: c.secret, privy_result: hex(r) });
}

// Privy's own errors, for parity of the inputs both reject.
const privyErrors: { shares: string[]; error: string }[] = [];
const dup = cases[0].shares.slice(0, 2);
for (const shares of [[dup[0], dup[0]], [dup[0], dup[1].slice(0, -2)], [dup[0]]]) {
  try {
    await combine(shares.map((h) => Uint8Array.from(Buffer.from(h, 'hex'))));
    privyErrors.push({ shares, error: '' });
  } catch (e: any) {
    privyErrors.push({ shares, error: e.message });
  }
}

// Privy's log/exp tables (generator 0xe5), parsed from the installed source, so the Rust
// test can check its table-free field arithmetic against every entry.
// (the package's `exports` map hides package.json, so resolve the directory from the entry point)
const require = createRequire(import.meta.url);
const pkgDir = require.resolve('shamir-secret-sharing').replace(/index\.js$/, '');
const src = readFileSync(pkgDir + 'src/index.ts', 'utf8');
const table = (name: string) => {
  const m = src.match(new RegExp(`const ${name}[^\\[]*\\[([^\\]]*)\\]`));
  if (!m) throw new Error('table not found: ' + name);
  const v = m[1].split(',').map((s) => s.trim()).filter(Boolean).map((s) => parseInt(s, 16));
  if (v.length !== 256) throw new Error('bad table');
  return Buffer.from(v).toString('hex');
};
const pkg = JSON.parse(readFileSync(pkgDir + 'package.json', 'utf8'));

writeFileSync(
  new URL('./vectors.json', import.meta.url),
  JSON.stringify(
    {
      meta: {
        reference: `shamir-secret-sharing@${pkg.version}`,
        source_sha256: createHash('sha256').update(src).digest('hex'),
        rng: 'SHA-256(seed || u64be counter), seeds "case-<i>"; bytes split() consumed are in `random`',
        node: process.version,
      },
      log_table: table('LOG_TABLE'),
      exp_table: table('EXP_TABLE'),
      cases,
      too_few: tooFew,
      privy_errors: privyErrors,
    },
    null,
    1,
  ) + '\n',
);
console.log(`wrote ${cases.length} split cases, ${tooFew.length} too-few, ${privyErrors.length} error cases`);
