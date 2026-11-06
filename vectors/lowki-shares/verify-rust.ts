// Rust split -> Privy combine. Reads out/rust-shares.json (written by
// `cargo test --test vectors rust_split_export`), runs shamir-secret-sharing 0.0.4 combine()
// on each recorded subset and checks it returns the secret. Uses the library's real csprng
// (no hook): combine() draws no randomness.
import { readFileSync } from 'node:fs';
import { combine } from 'shamir-secret-sharing';

type C = { secret: string; t: number; shares: string[]; subsets: number[][] };
const file = process.argv[2] ?? 'out/rust-shares.json';
const cases: C[] = JSON.parse(readFileSync(new URL(file, import.meta.url + '/../'), 'utf8'));
const u8 = (h: string) => Uint8Array.from(Buffer.from(h, 'hex'));
let ok = 0;
let checks = 0;
for (const c of cases) {
  for (const idx of c.subsets) {
    checks++;
    const s = await combine(idx.map((i) => u8(c.shares[i])));
    if (Buffer.from(s).toString('hex') !== c.secret) {
      console.error('MISMATCH', { t: c.t, n: c.shares.length, idx });
      process.exit(1);
    }
    ok++;
  }
}
console.log(`Privy combine() reconstructed ${ok}/${checks} Rust-split subsets across ${cases.length} cases`);
