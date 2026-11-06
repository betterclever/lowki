// Deterministic inputs shared by every benchmark side (TS in Node/Chrome, Rust native/wasm).
// Run: node build.mjs && node dist/gen-fixtures.mjs  -> fixtures/*.json
import { createHash } from "node:crypto";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { generateStealthAddress } from "@scopelift/stealth-address-sdk";
import { getPublicKey } from "@noble/secp256k1";

const HERE = dirname(fileURLToPath(import.meta.url));
const ROOT = join(HERE, "..");
const OUT = join(ROOT, "fixtures");
mkdirSync(OUT, { recursive: true });

const P = 21888242871839275222246405745257275088548364400416034343698204186575808495617n;
const N = 0xfffffffffffffffffffffffffffffffebaaedce6af48a03bbfd25e8cd0364141n;

let ctr = 0;
function rand32(): Buffer {
  return createHash("sha256").update(`lowki-bench-v1/${ctr++}`).digest();
}
const hex = (b: Uint8Array) => Buffer.from(b).toString("hex");
const big = (b: Uint8Array) => BigInt("0x" + hex(b));
const field = () => big(rand32()) % P;
const scalar = (): string => {
  for (;;) {
    const k = big(rand32());
    if (k > 0n && k < N) return k.toString(16).padStart(64, "0");
  }
};

// ---- keys ----
const mnemonic = "test test test test test test test test test test test junk";
const keys = {
  mnemonic,
  ppPath: "m/28784'/1'/0'",
  eoaPaths: Array.from({ length: 10 }, (_, i) => `m/44'/60'/0'/0/${i}`),
  signKey: scalar(),
  digests: Array.from({ length: 64 }, () => hex(rand32())),
};

// ---- pp-core ----
const poseidonPairs = Array.from({ length: 10_000 }, () => [field().toString(), field().toString()]);
const leaves = Array.from({ length: 100_000 }, () => field().toString());
const proofIndices = Array.from({ length: 1000 }, () => Number(big(rand32()) % 100_000n));
const commitments = Array.from({ length: 1000 }, () => ({
  value: (big(rand32()) % (1n << 128n)).toString(),
  label: field().toString(),
  precommitment: field().toString(),
}));
const pp = {
  poseidonPairs,
  leaves,
  proofIndices,
  commitments,
  accountKey: scalar(), // stands in for the m/28784'/1'/0' key (see keys bench for that derivation)
  chainId: 11155111,
  entrypoint: "0x34A2068192b1297f2a7f85D7D8CdE66F8F0921cB",
  depositIndices: Array.from({ length: 16 }, (_, i) => i),
};

// ---- stealth ----
const spend = scalar();
const view = scalar();
const otherSpend = scalar();
const otherView = scalar();
const metaUri = (s: string, v: string) =>
  `st:eth:0x${hex(getPublicKey(Buffer.from(s, "hex"), true))}${hex(getPublicKey(Buffer.from(v, "hex"), true))}`;
const myMeta = metaUri(spend, view);
const otherMeta = metaUri(otherSpend, otherView);
const announcements: { eph: string; tag: number; addr: string; mine: boolean }[] = [];
for (let i = 0; i < 10_000; i++) {
  const mine = i % 100 === 37; // 1 % of the announcements are ours
  const g = generateStealthAddress({
    stealthMetaAddressURI: mine ? myMeta : otherMeta,
    ephemeralPrivateKey: Buffer.from(scalar(), "hex") as unknown as `0x${string}`,
  });
  announcements.push({ eph: g.ephemeralPublicKey.slice(2), tag: parseInt(g.viewTag.slice(2), 16), addr: g.stealthAddress, mine });
}
const stealth = {
  spendingKey: spend,
  viewingKey: view,
  spendingPub: hex(getPublicKey(Buffer.from(spend, "hex"), true)),
  metaUri: myMeta,
  ephKeys: Array.from({ length: 64 }, scalar),
  announcements,
};

// ---- aa ----
const proofs = JSON.parse(readFileSync(join(ROOT, "../vectors/lowki-groth16/out/rust-proofs.json"), "utf8"));
const wp = proofs.find((p: any) => p.name === "withdraw_1500_leaves");
const aa = {
  ephemeralKey: scalar(),
  chainId: 1,
  paymaster: "0xe06CB96C57D2442f8F60F5017354BC08F7e91308",
  adapter: "0x521F578Ac6E894939364Dc27c7e6978Ecf7b7926",
  recipient: "0x" + hex(rand32()).slice(0, 40),
  refundRecipient: "0x" + hex(rand32()).slice(0, 40),
  fee: "123456789012345",
  maxFeePerGas: "30000000000",
  maxPriorityFeePerGas: "1500000000",
  gas: {
    callGasLimit: "0",
    verificationGasLimit: "120000",
    preVerificationGas: "60000",
    paymasterVerificationGasLimit: "750000",
    paymasterPostOpGasLimit: "60000",
  },
  proof: { pi_a: wp.proof.pi_a, pi_b: wp.proof.pi_b, pi_c: wp.proof.pi_c, publicSignals: wp.publicSignals },
};

// ---- groth16 ----
const g16v = JSON.parse(readFileSync(join(ROOT, "../vectors/lowki-groth16/vectors.json"), "utf8"));
const pick = (n: string) => g16v.cases.find((c: any) => c.name === n);
const g16 = {
  commitment: { name: "commitment_rand_0", input: pick("commitment_rand_0").input, publicSignals: pick("commitment_rand_0").publicSignals },
  withdraw: { name: "withdraw_1500_leaves", input: pick("withdraw_1500_leaves").input, publicSignals: pick("withdraw_1500_leaves").publicSignals },
};

for (const [n, v] of Object.entries({ keys, pp, stealth, aa, g16 })) {
  writeFileSync(join(OUT, `${n}.json`), JSON.stringify(v));
}
console.log("fixtures written:", Object.keys({ keys, pp, stealth, aa, g16 }).join(", "),
  "| my announcements:", announcements.filter((a) => a.mine).length, "| withdraw proof from", wp.name);
