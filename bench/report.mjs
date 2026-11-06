// Joins results/*.json into markdown tables (results/tables.md) and checks output equivalence.
import { existsSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const HERE = dirname(fileURLToPath(import.meta.url));
const R = (f) => {
  const p = join(HERE, "results", f);
  if (!existsSync(p)) return null;
  const j = JSON.parse(readFileSync(p, "utf8"));
  return Array.isArray(j) ? j : j.results;
};
const byId = (rows) => Object.fromEntries((rows ?? []).map((r) => [r.id, r]));

function fmt(ms) {
  if (ms == null || !isFinite(ms)) return "–";
  if (ms >= 1000) return (ms / 1000).toFixed(2) + " s";
  if (ms >= 1) return ms.toFixed(ms >= 100 ? 0 : ms >= 10 ? 1 : 2) + " ms";
  if (ms >= 0.001) return (ms * 1000).toFixed(ms >= 0.1 ? 0 : 1) + " µs";
  return (ms * 1e6).toFixed(0) + " ns";
}
const cell = (r) => (r ? `${fmt(r.median_ms)} / ${fmt(r.p90_ms)}` : "–");
const ratio = (ts, rs) => {
  if (!ts || !rs) return "–";
  const x = ts.median_ms / rs.median_ms;
  return x >= 1 ? `**${x >= 10 ? x.toFixed(0) : x.toFixed(1)}× faster**` : `${(1 / x).toFixed(1)}× slower`;
};
const canon = (s) => {
  if (s == null) return s;
  try {
    const v = JSON.parse(s);
    if (typeof v !== "object") return String(s).toLowerCase();
    const norm = (v) => (Array.isArray(v) ? v.map(norm) : v && typeof v === "object" ? Object.fromEntries(Object.keys(v).sort().map((k) => [k, norm(v[k])])) : typeof v === "string" ? v.toLowerCase() : v);
    return JSON.stringify(norm(v));
  } catch {
    return String(s).toLowerCase();
  }
};
const rustIdOf = (id) => id.replace(/(_noble2|_maci|_parallel|_cached_account)$/, "").replace(/_(1k|10k|100k)_parallel$/, "_$1");
const eq = (ts, rs) => (!ts?.check || !rs?.check ? "–" : canon(ts.check) === canon(rs.check) ? "✓" : "✗ MISMATCH");

const LABEL = {
  "keys.mnemonic_to_seed": "BIP-39 mnemonic → seed (PBKDF2-SHA512 ×2048)",
  "keys.derive_pp_from_seed": "BIP-32 seed → key at m/28784'/1'/0'",
  "keys.derive_eoa10_from_seed": "BIP-32 seed → key at m/44'/60'/0'/0/i (per key, 10 keys)",
  "keys.mnemonic_to_key": "mnemonic → key (new keystore + deriveAt, end to end)",
  "keys.keystore_derive": "deriveAt on an existing keystore ¹",
  "keys.sign_digest": "secp256k1 sign 32-byte digest (viem `sign`)",
  "keys.sign_digest_noble2": "secp256k1 sign 32-byte digest (@noble/curves 2.0.1)",
  "pp.poseidon_t3": "Poseidon T3 hash (Kohaku poseidonT3)",
  "pp.poseidon_t3_maci": "Poseidon T3 hash (maci-crypto, used by Kohaku for secrets)",
  "pp.limt_build_1k": "LeanIMT build, 1k leaves",
  "pp.limt_build_10k": "LeanIMT build, 10k leaves",
  "pp.limt_build_100k": "LeanIMT build, 100k leaves",
  "pp.limt_build_1k_parallel": "LeanIMT build, 1k leaves (Kohaku fromLeavesParallel, workers) ²",
  "pp.limt_build_10k_parallel": "LeanIMT build, 10k leaves (Kohaku fromLeavesParallel, workers) ²",
  "pp.limt_build_100k_parallel": "LeanIMT build, 100k leaves (Kohaku fromLeavesParallel, workers) ²",
  "pp.limt_proof_100k": "Merkle proof generation (100k tree, per proof)",
  "pp.note_secrets": "Note secrets (SignatureSecretManager.getDepositSecrets, per note) ³",
  "pp.note_secrets_cached_account": "Note secrets, TS signer reusing one viem account ³",
  "pp.commitment": "Commitment poseidon(value, label, precommitment)",
  "stealth.generate": "Generate stealth address (per address)",
  "stealth.scan_1k": "Scan 1,000 announcements (view tag; 10 ours)",
  "stealth.scan_10k": "Scan 10,000 announcements (view tag; 100 ours)",
  "aa.build_signed_userop": "Build + sign paymaster-withdraw userOp (encoders + 7702 auth + EIP-712 sig)",
};

function microTable(tsRows, rsRows, tsName, rsName) {
  const ts = byId(tsRows), rs = byId(rsRows);
  const lines = [`| Operation | ${tsName} median / p90 | ${rsName} median / p90 | Lowki vs TS | Same output |`, "|---|---|---|---|---|"];
  for (const id of Object.keys(LABEL)) {
    const t = ts[id], r = rs[rustIdOf(id)];
    if (!t && !r) continue;
    const err = t?.error ? ` (TS error: ${t.error})` : "";
    lines.push(`| ${LABEL[id]}${err} | ${cell(t)} | ${cell(r)} | ${ratio(t, r)} | ${eq(t, r)} |`);
  }
  return lines.join("\n");
}

function g16Table(rowsets) {
  const all = Object.assign({}, ...rowsets.map(byId));
  const verify = byId(R("native-verify.json"));
  const CFG = [
    ["snarkjs_mt", "snarkjs fullProve, default (web workers / worker_threads)"],
    ["snarkjs_1t", "snarkjs fullProve, singleThread"],
    ["wasm_cpu", "lowki-groth16 wasm, CPU (1 thread)"],
    ["wasm_webgpu", "lowki-groth16 wasm, WebGPU"],
    ["native_1t.cpu", "lowki-groth16 native, 1 thread"],
    ["native_par.cpu", "lowki-groth16 native, rayon (`parallel`)"],
    ["native.metal", "lowki-groth16 native, Metal"],
  ];
  const out = [];
  for (const c of ["commitment", "withdraw"]) {
    out.push(`\n**${c}**\n`, "| Config | Full proof median / p90 | Witness median | Proof minus witness | One-time setup | Verifies (snarkjs) | Public signals = fixture |", "|---|---|---|---|---|---|---|");
    for (const [k, label] of CFG) {
      const r = all[`g16.${c}.${k}`];
      if (!r) continue;
      const w = r.extra?.witness?.median_ms ?? r.witness?.median_ms;
      const setup = [
        r.extra?.load_ms ?? r.load_ms, r.extra?.gpu_open_ms ?? r.gpu_open_ms, r.extra?.upload_ms ?? r.upload_ms,
      ];
      const setupStr = setup[0] != null || setup[1] != null
        ? [setup[1] != null ? `GPU open ${fmt(setup[1])}` : null, setup[0] != null ? `zkey load ${fmt(setup[0])}` : null, setup[2] != null ? `base upload ${fmt(setup[2])}` : null].filter(Boolean).join(", ")
        : "none (snarkjs re-reads the zkey on every proof)";
      const v = r.extra?.verified_snarkjs ?? verify[r.id]?.verified_snarkjs;
      const sig = r.extra?.signals_match_fixture ?? verify[r.id]?.signals_match_fixture;
      out.push(`| ${label} | ${cell(r)} | ${w != null ? fmt(w) : "–"} | ${w != null ? fmt(r.median_ms - w) : "–"} | ${setupStr} | ${v == null ? "–" : v ? "✓" : "✗"} | ${sig == null ? "–" : sig ? "✓" : "✗"} |`);
    }
  }
  return out.join("\n");
}

const md = [];
md.push("### Chrome: Lowki wasm vs TS in the same browser\n");
md.push(microTable(R("browser-ts-micro.json"), R("browser-wasm-micro.json"), "TS (Chrome)", "Lowki wasm (Chrome)"));
md.push("\n### Native Rust vs Node\n");
md.push(microTable(R("node-micro.json"), R("native-1t.json"), "TS (Node)", "Lowki native"));
md.push("\n### Groth16\n");
md.push(g16Table([
  ...["browser-g16-snarkjs-mt.json", "browser-g16-snarkjs-st.json", "browser-g16-wasm-cpu.json", "browser-g16-wasm-gpu.json"].map(R),
]));
md.push("\n#### Groth16, native / Node\n");
md.push(g16Table([R("node-g16-mt.json"), R("node-g16-st.json"), R("native-1t.json"), R("native-par.json")]));
// Before/after the host-WebAssembly witness change (results/before-*.json = the first run).
if (existsSync(join(HERE, "results/before-browser-g16-wasm-gpu.json"))) {
  const pair = (f) => [byId(R(`before-${f}`)), byId(R(f))];
  const rows = [
    ["browser-g16-wasm-cpu.json", "wasm_cpu", "lowki wasm, CPU (1 thread)"],
    ["browser-g16-wasm-gpu.json", "wasm_webgpu", "lowki wasm, WebGPU"],
    ["browser-g16-wasm-gpu-noiso.json", "wasm_webgpu", "lowki wasm, WebGPU, no COOP/COEP"],
    ["browser-g16-snarkjs-mt.json", "snarkjs_mt", "snarkjs, default workers (reference, unchanged code)"],
    ["browser-g16-snarkjs-mt-noiso.json", "snarkjs_mt", "snarkjs, default workers, no COOP/COEP"],
  ];
  md.push("\n### Groth16 in Chrome: before / after running the witness on the host WebAssembly engine\n");
  md.push("| Config | commitment before | commitment after | withdraw before | withdraw after | withdraw witness before → after |", "|---|---|---|---|---|---|");
  for (const [f, k, label] of rows) {
    const [b, a] = pair(f);
    const g = (o, c) => o[`g16.${c}.${k}`];
    const wit = (o) => o?.extra?.witness?.median_ms;
    const w = wit(g(b, "withdraw")) != null ? `${fmt(wit(g(b, "withdraw")))} → ${fmt(wit(g(a, "withdraw")))}` : "–";
    md.push(`| ${label} | ${cell(g(b, "commitment"))} | ${cell(g(a, "commitment"))} | ${cell(g(b, "withdraw"))} | ${cell(g(a, "withdraw"))} | ${w} |`);
  }
}
const size = existsSync(join(HERE, "results/size.md")) ? readFileSync(join(HERE, "results/size.md"), "utf8") : "";
md.push("\n### Payload size\n", size);
writeFileSync(join(HERE, "results/tables.md"), md.join("\n") + "\n");
console.log(md.join("\n"));
