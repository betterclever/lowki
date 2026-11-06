// results/size-{wasm,js}.csv -> results/size.md
import { readFileSync, writeFileSync } from "node:fs";
const csv = (f) => {
  const [h, ...rows] = readFileSync(f, "utf8").trim().split("\n").map((l) => l.split(","));
  return rows.map((r) => Object.fromEntries(h.map((k, i) => [k, isNaN(+r[i]) ? r[i] : +r[i]])));
};
const w = csv("results/size-wasm.csv"), j = csv("results/size-js.csv");
const kb = (b) => (b / 1024).toFixed(1) + " KiB";
const W = (a, v) => w.find((r) => r.area === a && r.variant === v);
const J = (a) => j.find((r) => r.area === a);
const AREAS = [
  ["keys", "keys", "lowki-keys", "Kohaku MnemonicKeystore + @scure/bip32/39 + viem sign/address"],
  ["pp", "pp", "lowki-pp-core", "Kohaku poseidon + LeanIMT + SignatureSecretManager + commitment/context (maci-crypto, viem)"],
  ["stealth", "stealth", "lowki-stealth", "ScopeLift SDK (noble/secp256k1) + viem encode/decode"],
  ["aa", "aa", "lowki-aa", "Kohaku paymaster encoders + buildSignedUserOp (viem)"],
  ["g16", "g16", "lowki-groth16 (CPU)", "snarkjs groth16 (fullProve + verify)"],
  ["webgpu", "g16", "lowki-groth16 + lowki-gpu (WebGPU)", "snarkjs groth16 (fullProve + verify)"],
  ["all", "all", "all six crates in one module", "all of the above in one bundle"],
];
const lines = [
  "| Area | Lowki module | wasm+glue gzip (opt-level 3, wasm-opt -O3) | wasm+glue gzip (opt-level z, wasm-opt -Oz) | TS equivalent | JS minified | JS gzip | gzip ratio (speed build) |",
  "|---|---|---|---|---|---|---|---|",
];
for (const [a, ja, name, tsName] of AREAS) {
  const s = W(a, "speed"), z = W(a, "size"), t = J(ja);
  if (!s || !t) continue;
  const sg = s.wasm_gz + s.glue_gz, zg = z.wasm_gz + z.glue_gz;
  const r = t.js_gz / sg;
  lines.push(`| ${a} | ${name} | ${kb(sg)} (wasm ${kb(s.wasm_raw)} raw) | ${kb(zg)} (wasm ${kb(z.wasm_raw)} raw) | ${tsName} | ${kb(t.js_raw)} | ${kb(t.js_gz)} | ${r >= 1 ? `**${r.toFixed(1)}× smaller**` : `${(1 / r).toFixed(1)}× larger`} |`);
}
writeFileSync("results/size.md", lines.join("\n") + "\n");
console.log(lines.join("\n"));
