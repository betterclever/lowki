// Differential test-vector generator for crates/lowki-keys.
//
// Ground truth:
//   - Kohaku @ 8c83d64050040654082d70901a46a339bfa103b0: packages/plugins/src/host/
//     {derivation-path,mnemonic-keystore,scoped-keystore}.ts, copied verbatim into
//     ./kohaku-src/ by this script (so their bare imports resolve to this package's
//     node_modules) and imported from there.
//   - @scure/bip39, @scure/bip32, @noble/hashes, @noble/curves, viem, ox (pinned in package.json).
//   - Node's WebCrypto (crypto.subtle) for the PRF seed-wrapping scheme.
//
// Run: npm install && npx tsx gen.ts   (writes ./vectors.json)

import { execFileSync } from "node:child_process";
import { copyFileSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { webcrypto } from "node:crypto";

import { sha256 } from "@noble/hashes/sha2.js";
import { keccak_256 } from "@noble/hashes/sha3.js";
import { bytesToHex, hexToBytes, utf8ToBytes, concatBytes } from "@noble/hashes/utils.js";
import { secp256k1 } from "@noble/curves/secp256k1.js";
import { HDKey } from "@scure/bip32";
import * as bip39 from "@scure/bip39";
import { wordlist as english } from "@scure/bip39/wordlists/english.js";
import { privateKeyToAddress, sign as viemSign } from "viem/accounts";

const HERE = dirname(fileURLToPath(import.meta.url));
const KOHAKU = join(HERE, "../../../vendor/kohaku");
const KOHAKU_COMMIT = "8c83d64050040654082d70901a46a339bfa103b0";

// ---------------------------------------------------------------------------
// Copy the Kohaku sources (verbatim) and import them.
// ---------------------------------------------------------------------------
const head = execFileSync("git", ["-C", KOHAKU, "rev-parse", "HEAD"]).toString().trim();
if (head !== KOHAKU_COMMIT) throw new Error(`Kohaku clone at ${head}, expected ${KOHAKU_COMMIT}`);
const SRC_DIR = join(HERE, "kohaku-src");
mkdirSync(SRC_DIR, { recursive: true });
const copied: Record<string, string> = {};
for (const f of ["derivation-path.ts", "mnemonic-keystore.ts", "scoped-keystore.ts"]) {
  const from = join(KOHAKU, "packages/plugins/src/host", f);
  copyFileSync(from, join(SRC_DIR, f));
  copied[f] = bytesToHex(sha256(readFileSync(from)));
}
// `./index` is only used for the `Keystore` type.
writeFileSync(
  join(SRC_DIR, "index.ts"),
  "// Stub: Kohaku's host/index.ts, reduced to the Keystore type the copied files reference.\n" +
    "export interface Keystore { readonly _brand?: 'Keystore'; deriveAt(path: string): Promise<`0x${string}`>; }\n",
);

const { parseDerivationPath } = await import("./kohaku-src/derivation-path.ts");
const { MnemonicKeystore } = await import("./kohaku-src/mnemonic-keystore.ts");
const { scopeKeystore, KeystorePathNotAllowedError } = await import("./kohaku-src/scoped-keystore.ts");

// ---------------------------------------------------------------------------
// Deterministic randomness (SHA-256 counter DRBG, fixed seed).
// ---------------------------------------------------------------------------
let ctr = 0;
function rand(n: number, label = "lowki-keys-vectors"): Uint8Array {
  const out = new Uint8Array(n);
  let off = 0;
  while (off < n) {
    const block = sha256(utf8ToBytes(`${label}/${ctr++}`));
    out.set(block.subarray(0, Math.min(32, n - off)), off);
    off += 32;
  }
  return out;
}
function randInt(max: number): number {
  const b = rand(4);
  return (((b[0] << 24) | (b[1] << 16) | (b[2] << 8) | b[3]) >>> 0) % max;
}
const hex = (b: Uint8Array) => bytesToHex(b);

// ---------------------------------------------------------------------------
// 1. BIP-39
// ---------------------------------------------------------------------------
const entropies: Uint8Array[] = [
  new Uint8Array(32),
  new Uint8Array(32).fill(0xff),
  new Uint8Array(32).fill(0x7f),
  new Uint8Array(32).fill(0x80),
  Uint8Array.from({ length: 32 }, (_, i) => i),
];
for (let i = 0; i < 59; i++) entropies.push(rand(32));
const bip39Generate = entropies.map((e) => {
  const mnemonic = bip39.entropyToMnemonic(e, english);
  return { entropy: hex(e), mnemonic, seed: hex(bip39.mnemonicToSeedSync(mnemonic)) };
});

// Validation (accept/reject must equal scure's validateMnemonic). Also record seeds for
// valid non-24-word phrases.
const validateCases: { mnemonic: string; valid: boolean; seed?: string; note?: string }[] = [];
function addValidate(m: string, note?: string) {
  const valid = bip39.validateMnemonic(m, english);
  validateCases.push({ mnemonic: m, valid, ...(valid ? { seed: hex(bip39.mnemonicToSeedSync(m)) } : {}), ...(note ? { note } : {}) });
}
for (const len of [16, 20, 24, 28, 32]) for (let i = 0; i < 4; i++) addValidate(bip39.entropyToMnemonic(rand(len), english), `valid ${len * 8}-bit`);
addValidate(bip39.entropyToMnemonic(new Uint8Array(16), english), "trezor 'abandon...about'");
const base = bip39Generate[7].mnemonic;
const words = base.split(" ");
// checksum failures: replace the last word with other words until scure rejects
for (let i = 0, made = 0; made < 8 && i < 2048; i++) {
  const w = [...words];
  w[23] = english[(english.indexOf(words[23]) + 1 + i) % 2048];
  const m = w.join(" ");
  if (!bip39.validateMnemonic(m, english)) { addValidate(m, "bad checksum"); made++; }
}
for (let k = 0; k < 6; k++) {
  const w = [...words];
  const j = randInt(23);
  [w[j], w[j + 1]] = [w[j + 1], w[j]];
  addValidate(w.join(" "), "swapped adjacent words");
}
addValidate(words.slice(0, 23).join(" "), "23 words");
addValidate([...words, "abandon"].join(" "), "25 words");
addValidate(words.slice(0, 12).join(" "), "first 12 words of a 24-word phrase");
addValidate("", "empty");
addValidate(" " + base, "leading space");
addValidate(base + " ", "trailing space");
addValidate(base.replace(" ", "  "), "double space");
addValidate(base.replace(" ", "\t"), "tab separator");
addValidate(base.replace(" ", "\n"), "newline separator");
addValidate(base.toUpperCase(), "uppercase");
addValidate(base.replace(/^(\S)/, (c) => c.toUpperCase()), "capitalised first word");
addValidate([...words.slice(0, 23), "notaword"].join(" "), "unknown word");
addValidate([...words.slice(0, 23), "abandonx"].join(" "), "word with suffix");
addValidate([...words.slice(0, 23), words[23].slice(0, 4)].join(" "), "4-letter prefix of last word");
addValidate(Array(24).fill("abandon").join(" "), "24x abandon (bad checksum)");
addValidate(Array(23).fill("abandon").concat("art").join(" "), "23x abandon + art (valid)");
addValidate(Array(12).fill("zoo").join(" "), "12x zoo");
addValidate(Array(11).fill("zoo").concat("wrong").join(" "), "11x zoo + wrong (valid)");

// ---------------------------------------------------------------------------
// 2. Derivation-path parser
// ---------------------------------------------------------------------------
const pathCases: { path: string; indices?: number[]; error?: true }[] = [];
function addPath(p: string) {
  try {
    pathCases.push({ path: p, indices: parseDerivationPath(p) });
  } catch {
    pathCases.push({ path: p, error: true });
  }
}
const validPaths = [
  "m/0", "m/0'", "m/0h", "m/1", "m/44'/60'/0'/0/0", "m/44h/60h/0h/0/0", "m/44'/60h/0'/0/0",
  "m/28784'/1'/0'", "m/28784'/1'/0'/0'", "m/29795'/1'", "m/44'/1984'/0'/0'/0'", "m/420'/1984'/0'/0'/0'",
  "m/2147483647", "m/2147483647'", "m/2147483647h", "m/1/2/3/4/5/6/7/8/9/10",
  "m/" + Array(255).fill("0").join("/"), "m/" + Array(255).fill("7'").join("/"),
  "m/10", "m/100'", "m/999999999",
];
const invalidPaths = [
  "", "m", "M", "M/0", "m/", "/0", "0", "0/1", "m//0", "m/0/", "m/0//1", "m/01", "m/00", "m/007'",
  "m/-1", "m/+1", "m/1.0", "m/1e3", "m/0x10", "m/ 1", "m/1 ", "m/1\n", "m/1'\n", "m/1''", "m/1h'",
  "m/1H", "m/1'h", "m/'", "m/h", "m/a", "m/1a", "m/１", "m/٣", "m/2147483648", "m/2147483648'",
  "m/4294967295", "m/4294967296", "m/99999999999999999999", "m/99999999999999999999999999999999999999'",
  "m/" + Array(256).fill("0").join("/"), "m/" + Array(300).fill("1'").join("/"), "mm/0", "m0", "m/0/m",
  " m/0", "m/0 ", "m\\0", "m/0\\1", "m/0'/1/", "m/44'/60'/0'/0/0/", "n/0", "m/0'/-0", "m/-0",
  "m/1_000", "m/1,0", "m/∞", "m/NaN", "m/Infinity", "m/0b1", "m/0o1", "m/ 0'", "m/0''",
];
for (const p of [...validPaths, ...invalidPaths]) addPath(p);
for (let i = 0; i < 40; i++) {
  const depth = 1 + randInt(8);
  const segs: string[] = [];
  for (let j = 0; j < depth; j++) {
    const r = randInt(10);
    const idx = r < 3 ? randInt(100) : r < 9 ? randInt(0x80000000) : 0x80000000 + randInt(0x7fffffff);
    const suf = ["", "'", "h"][randInt(3)];
    segs.push(`${idx}${suf}`);
  }
  addPath("m/" + segs.join("/"));
}
const nValid = pathCases.filter((c) => !c.error).length;

// ---------------------------------------------------------------------------
// 3. MnemonicKeystore.deriveAt
// ---------------------------------------------------------------------------
const derivePaths = [
  "m/0", "m/1", "m/0'", "m/0h", "m/2147483647", "m/2147483647'",
  "m/44'/60'/0'/0/0", "m/44'/60'/0'/0/1", "m/44'/60'/1'/0/0", "m/44'/1'/0'/0/0",
  "m/28784'/1'/0'", "m/28784'/1'/1'", "m/28784'/1'/0'/0'", "m/28784'/1'/0'/1'", "m/28784'/11155111'/0'",
  "m/29795'/1'/0'", "m/0/1/2/3/4", "m/0'/1/2'/2/1000000000", "m/1/2'/3/4'/5/6'/7/8'",
  // RAILGUN "babyjubjub seed" branch (index 1 == 1984')
  "m/44'/1984'/0'/0'/0'", "m/44'/1984'/0'/0'/1'", "m/420'/1984'/0'/0'/0'", "m/0'/1984'", "m/0/1984'",
  "m/44'/1984'", "m/44'/1984'/0'",
  // 1984 not at index 1, or not hardened -> plain BIP-32
  "m/1984'", "m/1984'/1984'/0'".replace(/^m\/1984'/, "m/1984'"), "m/44'/1984", "m/44'/1984/0'",
  // RAILGUN branch with a non-hardened index -> error
  "m/44'/1984'/0", "m/0/1984'/0'/5", "m/44'/1984'/0'/0'/0",
  // invalid paths -> error
  "m", "m/01", "m/2147483648",
  "m/" + Array(255).fill("1").join("/"),
];
for (let i = 0; i < 20; i++) {
  const depth = 1 + randInt(6);
  const segs: string[] = [];
  for (let j = 0; j < depth; j++) segs.push(`${randInt(0x80000000)}${randInt(2) ? "'" : ""}`);
  derivePaths.push("m/" + segs.join("/"));
}
const deriveMnemonics = [
  bip39.entropyToMnemonic(new Uint8Array(16), english),
  bip39Generate[0].mnemonic,
  bip39Generate[5].mnemonic,
  bip39Generate[6].mnemonic,
];
const deriveCases: { mnemonic: string; seed: string; path: string; key?: string; error?: true }[] = [];
for (const mnemonic of deriveMnemonics) {
  const ks = new MnemonicKeystore(mnemonic);
  const seed = hex(bip39.mnemonicToSeedSync(mnemonic));
  for (const path of derivePaths) {
    try {
      const key = await ks.deriveAt(path);
      deriveCases.push({ mnemonic, seed, path, key: key.slice(2) });
    } catch {
      deriveCases.push({ mnemonic, seed, path, error: true });
    }
  }
}

// ---------------------------------------------------------------------------
// 4. scopeKeystore
// ---------------------------------------------------------------------------
type ScopeResult = "ok" | "not_allowed" | "invalid_path" | "invalid_scope";
const fake = { _brand: "Keystore" as const, async deriveAt() { return "0x00" as const; } };
const scopeDefs: { prefixes: string[]; paths: string[] }[] = [
  { prefixes: ["m/28784'/1'"], paths: [] },
  { prefixes: ["m/28784'/1'", "m/29795'/1'"], paths: ["m/44'/60'/0'/0/0"] },
  { prefixes: [], paths: ["m/44'/60'/0'/0/0", "m/44h/60h/1h/0/0", "not-a-path", "m"] },
  { prefixes: ["m/0"], paths: [] },
  { prefixes: ["m/44'/60'/0'/0"], paths: ["m/44'/60'/0'/0"] },
  { prefixes: [], paths: [] },
  { prefixes: ["m"], paths: [] },
  { prefixes: ["m/01'"], paths: [] },
  { prefixes: ["m/28784'/1'", "m/x"], paths: [] },
];
const scopeQueries = [
  "m/28784'/1'", "m/28784'/1'/0'", "m/28784h/1h/0h", "m/28784'/1'/0'/5'", "m/28784'/1", "m/28784'/1/0'",
  "m/28784'/2'/0'", "m/28784'", "m/29795'/1'/7'", "m/29795'/1'", "m/44'/60'/0'/0/0", "m/44h/60h/0h/0/0",
  "m/44'/60'/0'/0/1", "m/44'/60'/0'/0", "m/44'/60'/0'", "m/44'/60'/1'/0/0", "m/0", "m/0/0", "m/0'/0",
  "m/0/0/0/0", "m", "m/", "m/01", "m/28784'/1'/2147483648'", "garbage",
];
const scopeCases: { prefixes: string[]; paths: string[]; path: string; result: ScopeResult }[] = [];
for (const def of scopeDefs) {
  let scoped: { deriveAt(p: string): Promise<unknown> } | null = null;
  try {
    scoped = scopeKeystore(fake, def);
  } catch {
    scoped = null;
  }
  for (const q of scopeQueries) {
    let result: ScopeResult;
    if (!scoped) result = "invalid_scope";
    else {
      try {
        await scoped.deriveAt(q);
        result = "ok";
      } catch (e) {
        result = e instanceof KeystorePathNotAllowedError ? "not_allowed" : "invalid_path";
      }
    }
    scopeCases.push({ ...def, path: q, result });
  }
}

// ---------------------------------------------------------------------------
// 5. Lowki per-app EOA
// ---------------------------------------------------------------------------
function appIndex(origin: string): number {
  const h = keccak_256(concatBytes(utf8ToBytes("lowki/app/v1"), utf8ToBytes(origin)));
  return (((h[0] << 24) | (h[1] << 16) | (h[2] << 8) | h[3]) >>> 0) & 0x7fffffff;
}
const origins = [
  "https://app.uniswap.org", "https://uniswap.org", "http://localhost:3000", "http://localhost:5173",
  "https://example.com", "https://example.com:8443", "https://sub.example.com", "http://127.0.0.1:8080",
  "https://opensea.io", "https://app.aave.com", "https://lowki.xyz", "https://xn--bcher-kva.example",
  "https://bücher.example", "https://例子.测试", "https://🦊.eth.limo", "", "null", "chrome-extension://abcdefghijklmnop",
  "https://a", "https://" + "a".repeat(200) + ".com",
];
for (let i = 0; i < 40; i++) origins.push(`https://app${i}-${hex(rand(3))}.example.org`);
const appSeeds = [bip39Generate[0], bip39Generate[1], bip39Generate[9]];
const appCases: { seed: string; origin: string; index: number; path: string; private_key: string; address: string }[] = [];
for (const s of appSeeds) {
  const root = HDKey.fromMasterSeed(hexToBytes(s.seed));
  const ks = new MnemonicKeystore(s.mnemonic);
  for (const origin of origins) {
    const index = appIndex(origin);
    const path = `m/44'/60'/${index}'/0/0`;
    const pk = root.derive(path).privateKey!;
    const viaKohaku = (await ks.deriveAt(path)).slice(2);
    if (viaKohaku !== hex(pk)) throw new Error("scure/kohaku mismatch");
    appCases.push({ seed: s.seed, origin, index, path, private_key: hex(pk), address: privateKeyToAddress(`0x${hex(pk)}`) });
  }
}
// EIP-55 checksum cases (lowercase -> viem getAddress)
const { getAddress } = await import("viem");
const eip55Cases = [
  "0x5aaeb6053f3e94c9b9a09f33669435e7ef1beaed", "0xfb6916095ca1df60bb79ce92ce3ea74c37c5d359",
  "0xdbf03b407c01e7cd3cbea99509d93f8dddc8c6fb", "0xd1220a0cf47c7b9be7a2e6ba89f429762e7b9adb",
  "0x0000000000000000000000000000000000000000", "0xffffffffffffffffffffffffffffffffffffffff",
].concat(Array.from({ length: 30 }, () => "0x" + hex(rand(20))))
  .map((a) => ({ address: a.slice(2), checksummed: getAddress(a) }));

// ---------------------------------------------------------------------------
// 6. Seed wrapping with WebCrypto
// ---------------------------------------------------------------------------
const subtle = webcrypto.subtle;
const KEK_INFO = utf8ToBytes("lowki/kek/v1");
const AAD = utf8ToBytes("lowki/seed/v1");
async function kekKey(prf: Uint8Array) {
  const ikm = await subtle.importKey("raw", prf, "HKDF", false, ["deriveKey", "deriveBits"]);
  const bits = new Uint8Array(await subtle.deriveBits({ name: "HKDF", hash: "SHA-256", salt: new Uint8Array(0), info: KEK_INFO }, ikm, 256));
  const key = await subtle.deriveKey(
    { name: "HKDF", hash: "SHA-256", salt: new Uint8Array(0), info: KEK_INFO }, ikm,
    { name: "AES-GCM", length: 256 }, false, ["encrypt", "decrypt"],
  );
  return { bits, key };
}
async function wcWrap(prf: Uint8Array, entropy: Uint8Array, nonce: Uint8Array) {
  const { bits, key } = await kekKey(prf);
  const ct = new Uint8Array(await subtle.encrypt({ name: "AES-GCM", iv: nonce, additionalData: AAD, tagLength: 128 }, key, entropy));
  return { kek: bits, blob: concatBytes(new Uint8Array([1]), nonce, ct) };
}
async function wcUnwrap(prf: Uint8Array, blob: Uint8Array): Promise<Uint8Array | null> {
  const { key } = await kekKey(prf);
  try {
    return new Uint8Array(await subtle.decrypt({ name: "AES-GCM", iv: blob.slice(1, 13), additionalData: AAD, tagLength: 128 }, key, blob.slice(13)));
  } catch {
    return null;
  }
}
const wrapCases: { prf: string; entropy: string; nonce: string; kek: string; blob: string }[] = [];
const wrapInputs: [Uint8Array, Uint8Array, Uint8Array][] = [
  [new Uint8Array(32), new Uint8Array(32), new Uint8Array(12)],
  [new Uint8Array(32).fill(0xff), new Uint8Array(32).fill(0xff), new Uint8Array(12).fill(0xff)],
];
for (let i = 0; i < 38; i++) wrapInputs.push([rand(32), rand(32), rand(12)]);
for (const [prf, entropy, nonce] of wrapInputs) {
  const { kek, blob } = await wcWrap(prf, entropy, nonce);
  const back = await wcUnwrap(prf, blob);
  if (!back || hex(back) !== hex(entropy)) throw new Error("webcrypto roundtrip failed");
  wrapCases.push({ prf: hex(prf), entropy: hex(entropy), nonce: hex(nonce), kek: hex(kek), blob: hex(blob) });
}
type UnwrapErr = "bad_version" | "bad_length" | "tag";
const tamperCases: { prf: string; blob: string; error: UnwrapErr; note: string }[] = [];
async function addTamper(prf: Uint8Array, blob: Uint8Array, error: UnwrapErr, note: string) {
  // Everything that reaches AES-GCM (length 61, version 1) must fail in WebCrypto too.
  if (error === "tag" && (await wcUnwrap(prf, blob)) !== null) throw new Error(`tamper case decrypted: ${note}`);
  tamperCases.push({ prf: hex(prf), blob: hex(blob), error, note });
}
for (let i = 0; i < 8; i++) {
  const c = wrapCases[i];
  const prf = hexToBytes(c.prf);
  const blob = hexToBytes(c.blob);
  const flip = (pos: number, bit = 1) => { const b = blob.slice(); b[pos] ^= bit; return b; };
  await addTamper(prf, flip(1 + randInt(12)), "tag", "nonce bit flip");
  await addTamper(prf, flip(13 + randInt(32)), "tag", "ciphertext bit flip");
  await addTamper(prf, flip(45 + randInt(16), 0x80), "tag", "tag bit flip");
  await addTamper(prf, flip(60), "tag", "last byte flip");
  const wrongPrf = prf.slice(); wrongPrf[randInt(32)] ^= 1;
  await addTamper(wrongPrf, blob, "tag", "wrong PRF output");
  await addTamper(prf, flip(0, 0x01), "bad_version", "version 0x00");
  { const b = blob.slice(); b[0] = 0x02; await addTamper(prf, b, "bad_version", "version 0x02"); }
  await addTamper(prf, blob.slice(0, 60), "bad_length", "truncated by 1");
  await addTamper(prf, concatBytes(blob, new Uint8Array([0])), "bad_length", "extended by 1");
  await addTamper(prf, blob.slice(0, 13 + 16), "bad_length", "no ciphertext, only tag");
}
await addTamper(new Uint8Array(32), new Uint8Array(0), "bad_length", "empty");
await addTamper(new Uint8Array(32), new Uint8Array([1]), "bad_length", "version only");
// a valid blob for a *different* AAD must fail
{
  const prf = rand(32), entropy = rand(32), nonce = rand(12);
  const { key } = await kekKey(prf);
  const ct = new Uint8Array(await subtle.encrypt({ name: "AES-GCM", iv: nonce, additionalData: utf8ToBytes("lowki/seed/v2"), tagLength: 128 }, key, entropy));
  await addTamper(prf, concatBytes(new Uint8Array([1]), nonce, ct), "tag", "encrypted under different AAD");
  const ct2 = new Uint8Array(await subtle.encrypt({ name: "AES-GCM", iv: nonce, tagLength: 128 }, key, entropy));
  await addTamper(prf, concatBytes(new Uint8Array([1]), nonce, ct2), "tag", "encrypted without AAD");
}

// ---------------------------------------------------------------------------
// 7. sign_digest
// ---------------------------------------------------------------------------
const N = secp256k1.Point.CURVE().n;
const signKeys: Uint8Array[] = [
  hexToBytes("0000000000000000000000000000000000000000000000000000000000000001"),
  hexToBytes((N - 1n).toString(16).padStart(64, "0")),
  hexToBytes("ac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80"),
];
for (let i = 0; i < 9; i++) signKeys.push(hexToBytes(appCases[i * 5].private_key));
const digests: Uint8Array[] = [new Uint8Array(32), new Uint8Array(32).fill(0xff), keccak_256(utf8ToBytes("hello"))];
for (let i = 0; i < 5; i++) digests.push(rand(32));
const signCases: { private_key: string; digest: string; r: string; s: string; v: number }[] = [];
for (const pk of signKeys) {
  for (const d of digests) {
    const sig = await viemSign({ hash: `0x${hex(d)}`, privateKey: `0x${hex(pk)}` });
    const rec = secp256k1.sign(d, pk, { prehash: false, lowS: true, format: "recovered" });
    // noble v2 'recovered' format: recovery(1) || r(32) || s(32)
    if (hex(rec.slice(1, 33)) !== sig.r.slice(2) || hex(rec.slice(33)) !== sig.s.slice(2) || rec[0] !== sig.yParity)
      throw new Error("viem / noble v2 disagree");
    if (BigInt(sig.s) > N / 2n) throw new Error("high s");
    signCases.push({ private_key: hex(pk), digest: hex(d), r: sig.r.slice(2), s: sig.s.slice(2), v: 27 + sig.yParity! });
  }
}

// ---------------------------------------------------------------------------
const pkgVersion = (name: string) => JSON.parse(readFileSync(join(HERE, "node_modules", name, "package.json"), "utf8")).version;
const out = {
  meta: {
    kohaku_commit: KOHAKU_COMMIT,
    kohaku_files_sha256: copied,
    packages: Object.fromEntries(["@scure/bip39", "@scure/bip32", "@noble/hashes", "@noble/curves", "viem", "ox"].map((p) => [p, pkgVersion(p)])),
    node: process.version,
    webcrypto: "node:crypto webcrypto.subtle (HKDF + AES-GCM)",
  },
  bip39_generate: bip39Generate,
  bip39_validate: validateCases,
  paths: pathCases,
  derive: deriveCases,
  scope: scopeCases,
  app: appCases,
  eip55: eip55Cases,
  wrap: wrapCases,
  unwrap_tamper: tamperCases,
  sign: signCases,
};
writeFileSync(join(HERE, "vectors.json"), JSON.stringify(out, null, 1) + "\n");
console.log(
  Object.entries(out).filter(([k]) => k !== "meta").map(([k, v]) => `${k}=${(v as unknown[]).length}`).join(" "),
  `(paths valid=${nValid})`,
);
