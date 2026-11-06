// Differential vector generator for crates/lowki-groth16.
//
// Builds *valid* inputs for the Privacy Pools `commitment` and `withdraw` circuits
// (mirroring Kohaku's withdrawThunk input construction), runs snarkjs 0.7.5 against the
// published artifacts to get the full witness + public signals, and produces one fixed
// snarkjs proof per circuit together with its Solidity-format conversion (Kohaku's
// `toWithdrawProof` / `encodeRagequitPayload`, plus snarkjs `exportSolidityCallData`).
//
// Output:
//   vectors.json           (committed)  inputs, public signals, witness length + sha256
//   witness/<case>.bin     (generated)  raw witness, 32-byte little-endian per element
//
// Run: npm run gen   (or: npx tsx gen.ts)
import { createHash } from "node:crypto";
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import * as snarkjs from "snarkjs";
import { LeanIMT } from "@zk-kit/lean-imt";
import { poseidon1, poseidon2, poseidon3 } from "poseidon-lite";
import { decodeFunctionData, getAddress } from "viem";
import { KOHAKU_COMMIT, kohakuEncoding, kohakuLimt, kohakuPoseidon, kohakuProofUtil } from "./kohaku.ts";

export const ARTIFACTS =
  process.env.LOWKI_PP_ARTIFACTS ??
  "/Users/betterclever/projects/ethglobal/vendor/npm/circuits/package/artifacts";
const HERE = new URL(".", import.meta.url).pathname;
const P = 21888242871839275222246405745257275088548364400416034343698204186575808495617n;

// ---------- deterministic RNG (sha256 counter mode, fixed seed) ----------
let ctr = 0;
const SEED = "lowki-groth16/vectors/v1";
function rand256(): bigint {
  const h = createHash("sha256").update(`${SEED}/${ctr++}`).digest("hex");
  return BigInt("0x" + h);
}
const randField = () => rand256() % P;
const randBelow = (n: bigint) => rand256() % n;
const randInt = (n: number) => Number(randBelow(BigInt(n)));

// ---------- helpers ----------
const dec = (x: bigint) => x.toString(10);
function witnessBytes(w: bigint[]): Buffer {
  const buf = Buffer.alloc(32 * w.length);
  w.forEach((v, i) => {
    let x = ((v % P) + P) % P;
    for (let j = 0; j < 32; j++) {
      buf[32 * i + j] = Number(x & 0xffn);
      x >>= 8n;
    }
  });
  return buf;
}

async function snarkWitness(circuit: string, input: Record<string, unknown>): Promise<bigint[]> {
  const wtns = { type: "mem" } as any;
  await snarkjs.wtns.calculate(input as any, `${ARTIFACTS}/${circuit}.wasm`, wtns);
  return (await snarkjs.wtns.exportJson(wtns)) as bigint[];
}

const nPublic = (circuit: string) =>
  JSON.parse(readFileSync(`${ARTIFACTS}/${circuit}.vkey`, "utf8")).nPublic as number;

// ---------- commitment cases ----------
type CommitmentIn = { value: bigint; label: bigint; nullifier: bigint; secret: bigint };
function commitmentCases(): { name: string; input: CommitmentIn }[] {
  const out: { name: string; input: CommitmentIn }[] = [];
  for (let i = 0; i < 6; i++) {
    out.push({
      name: `commitment_rand_${i}`,
      input: { value: randBelow(1n << 128n), label: randField(), nullifier: randField(), secret: randField() },
    });
  }
  out.push({ name: "commitment_zero_value", input: { value: 0n, label: randField(), nullifier: randField(), secret: randField() } });
  out.push({ name: "commitment_max_field", input: { value: P - 1n, label: P - 1n, nullifier: P - 1n, secret: P - 1n } });
  out.push({ name: "commitment_small", input: { value: 1n, label: 1n, nullifier: 1n, secret: 1n } });
  out.push({ name: "commitment_1eth", input: { value: 10n ** 18n, label: randField(), nullifier: randField(), secret: randField() } });
  return out;
}

// ---------- withdraw cases (mirror Kohaku withdrawThunk) ----------
type WithdrawSpec = {
  name: string;
  stateLeaves: number;
  stateIndex: number;
  aspLeaves: number;
  aspIndex: number;
  existingValue: bigint;
  withdrawnValue: bigint;
};

async function buildWithdraw(spec: WithdrawSpec) {
  const { PoseidonT3LeanIMT } = await kohakuLimt();
  const { poseidonT3 } = await kohakuPoseidon();
  const { proofFromTree, calculateContext } = await kohakuProofUtil();

  const label = randField();
  const existingNullifier = randField();
  const existingSecret = randField();
  const newNullifier = randField();
  const newSecret = randField();
  // Kohaku SignatureSecretManager: precommitment = poseidon([nullifier, salt]);
  // utils.commitment: poseidon([value, label, precommitment]).
  const precommitment = poseidon2([existingNullifier, existingSecret]);
  const existingCommitment = poseidon3([spec.existingValue, label, precommitment]);
  if (poseidonT3(existingNullifier, existingSecret) !== precommitment) throw new Error("poseidon mismatch vs Kohaku");

  const stateLeaves = Array.from({ length: spec.stateLeaves }, () => randField());
  stateLeaves[spec.stateIndex] = existingCommitment;
  const aspLeaves = Array.from({ length: spec.aspLeaves }, () => randField());
  aspLeaves[spec.aspIndex] = label;

  const hash = (a: bigint, b: bigint) => poseidon2([a, b]);
  const zkState = new LeanIMT(hash, stateLeaves);
  const zkAsp = new LeanIMT(hash, aspLeaves);
  const kState = new PoseidonT3LeanIMT(stateLeaves);
  const kAsp = new PoseidonT3LeanIMT(aspLeaves);
  if (zkState.root !== kState.root || zkAsp.root !== kAsp.root) throw new Error("zk-kit vs Kohaku LeanIMT root mismatch");

  // Kohaku's proofFromTree (pads siblings to 32 with 0n).
  const stateProof = proofFromTree(kState, existingCommitment);
  const aspProof = proofFromTree(kAsp, label);
  // cross-check against @zk-kit/lean-imt's proof, padded the same way
  const pad = (s: bigint[]) => [...s, ...Array(32 - s.length).fill(0n)];
  const zs = zkState.generateProof(spec.stateIndex);
  const za = zkAsp.generateProof(spec.aspIndex);
  if (zs.index !== stateProof.index || za.index !== aspProof.index) throw new Error("proof index mismatch");
  if (pad(zs.siblings).join() !== stateProof.siblings.join() || pad(za.siblings).join() !== aspProof.siblings.join())
    throw new Error("proof siblings mismatch");

  // context = keccak256(abi.encode(withdrawal, scope)) % SNARK_SCALAR_FIELD (Kohaku calculateContext)
  const processooor = getAddress("0x" + randBelow(1n << 160n).toString(16).padStart(40, "0"));
  const data = ("0x" + randBelow(1n << 256n).toString(16).padStart(64, "0")) as `0x${string}`;
  const context = BigInt(calculateContext({ processooor, data } as any, randField()));

  // Exactly the object withdrawThunk hands to prover.prove("withdraw", ...)
  const input = {
    context,
    label,
    existingNullifier,
    existingSecret,
    existingValue: spec.existingValue,
    newNullifier,
    newSecret,
    withdrawnValue: spec.withdrawnValue,

    stateIndex: BigInt(stateProof.index),
    stateRoot: stateProof.root,
    stateSiblings: stateProof.siblings,
    stateTreeDepth: 32n,

    ASPIndex: BigInt(aspProof.index),
    ASPRoot: aspProof.root,
    ASPSiblings: aspProof.siblings,
    ASPTreeDepth: 32n,
  };
  const expected = {
    newCommitmentHash: poseidon3([spec.existingValue - spec.withdrawnValue, label, poseidon2([newNullifier, newSecret])]),
    existingNullifierHash: poseidon1([existingNullifier]),
  };
  return { input, expected };
}

function withdrawSpecs(): WithdrawSpec[] {
  const v = (x: bigint) => x;
  return [
    { name: "withdraw_single_leaf", stateLeaves: 1, stateIndex: 0, aspLeaves: 1, aspIndex: 0, existingValue: v(10n ** 18n), withdrawnValue: v(10n ** 17n) },
    { name: "withdraw_two_leaves_last", stateLeaves: 2, stateIndex: 1, aspLeaves: 2, aspIndex: 0, existingValue: v(5n * 10n ** 18n), withdrawnValue: v(10n ** 18n) },
    { name: "withdraw_five_leaves_lifted", stateLeaves: 5, stateIndex: 4, aspLeaves: 3, aspIndex: 2, existingValue: v(123456789n), withdrawnValue: v(1n) },
    { name: "withdraw_full_amount", stateLeaves: 37, stateIndex: randInt(37), aspLeaves: 11, aspIndex: randInt(11), existingValue: v(7n * 10n ** 18n), withdrawnValue: v(7n * 10n ** 18n) },
    { name: "withdraw_zero_amount", stateLeaves: 64, stateIndex: 63, aspLeaves: 64, aspIndex: 0, existingValue: v(42n), withdrawnValue: v(0n) },
    { name: "withdraw_200_leaves", stateLeaves: 200, stateIndex: randInt(200), aspLeaves: 150, aspIndex: randInt(150), existingValue: v((1n << 128n) - 1n), withdrawnValue: v(1n << 127n) },
    { name: "withdraw_1500_leaves", stateLeaves: 1500, stateIndex: randInt(1500), aspLeaves: 777, aspIndex: randInt(777), existingValue: v(10n ** 20n), withdrawnValue: v(3n * 10n ** 19n + 12345n) },
  ];
}

// ---------- main ----------
const toJsonInput = (o: Record<string, unknown>): Record<string, unknown> =>
  Object.fromEntries(
    Object.entries(o).map(([k, val]) => [k, Array.isArray(val) ? val.map((x) => dec(x as bigint)) : dec(val as bigint)]),
  );

async function main() {
  mkdirSync(`${HERE}/witness`, { recursive: true });
  const cases: any[] = [];
  const skipped: any[] = [];

  async function addCase(name: string, circuit: string, input: Record<string, unknown>, expected?: Record<string, bigint>) {
    let w: bigint[];
    try {
      w = await snarkWitness(circuit, input);
    } catch (e) {
      skipped.push({ name, reason: String((e as Error).message ?? e).slice(0, 300) });
      console.warn(`  SKIP ${name}: circuit rejected input`);
      return;
    }
    const n = nPublic(circuit);
    const publicSignals = w.slice(1, 1 + n).map(dec);
    if (expected) {
      const vals = Object.values(expected).map(dec);
      if (publicSignals[0] !== vals[0] || publicSignals[1] !== vals[1]) throw new Error(`${name}: output mismatch vs independent poseidon`);
    }
    const bytes = witnessBytes(w);
    writeFileSync(`${HERE}/witness/${name}.bin`, bytes);
    cases.push({
      name,
      circuit,
      input: toJsonInput(input),
      publicSignals,
      witnessLen: w.length,
      witnessSha256: createHash("sha256").update(bytes).digest("hex"),
    });
    console.log(`  ${name}: witness ${w.length} signals`);
  }

  for (const c of commitmentCases()) {
    const { value, label, nullifier, secret } = c.input;
    const pre = poseidon2([nullifier % P, secret % P]);
    await addCase(c.name, "commitment", c.input as any, {
      commitment: poseidon3([value % P, label % P, pre]),
      nullifierHash: poseidon1([nullifier % P]),
    });
  }
  for (const s of withdrawSpecs()) {
    const { input, expected } = await buildWithdraw(s);
    await addCase(s.name, "withdraw", input as any, expected);
  }

  // Fixed proofs (snarkjs) for the Solidity-format check (v).
  const { toWithdrawProof, encodeRagequitPayload } = await kohakuEncoding();
  const { poolAbi } = await import(
    "/Users/betterclever/projects/ethglobal/vendor/kohaku/packages/privacy-pools/src/data/abis/pool.abi.ts"
  );
  const fixedProofs: any[] = [];
  for (const circuit of ["commitment", "withdraw"]) {
    const c = cases.find((x) => x.circuit === circuit)!;
    const { proof, publicSignals } = await snarkjs.groth16.fullProve(
      c.input,
      `${ARTIFACTS}/${circuit}.wasm`,
      `${ARTIFACTS}/${circuit}.zkey`,
    );
    const vkey = JSON.parse(readFileSync(`${ARTIFACTS}/${circuit}.vkey`, "utf8"));
    if (!(await snarkjs.groth16.verify(vkey, publicSignals, proof))) throw new Error("snarkjs self-verify failed");
    let sol: any;
    if (circuit === "withdraw") {
      sol = toWithdrawProof({ proof, publicSignals } as any);
    } else {
      // encodeRagequitPayload returns ragequit(...) calldata; decode the struct back out.
      const data = encodeRagequitPayload({ proof, publicSignals } as any);
      const { args } = decodeFunctionData({ abi: poolAbi, data });
      sol = (args as any)[0];
    }
    const solidity = {
      pA: sol.pA.map(dec),
      pB: sol.pB.map((r: bigint[]) => r.map(dec)),
      pC: sol.pC.map(dec),
      pubSignals: sol.pubSignals.map(dec),
    };
    const calldata = JSON.parse("[" + (await snarkjs.groth16.exportSolidityCallData(proof, publicSignals)) + "]");
    fixedProofs.push({
      circuit,
      case: c.name,
      proof,
      publicSignals,
      solidity,
      snarkjsSolidityCallData: calldata.map((x: any) =>
        Array.isArray(x) ? x.map((y: any) => (Array.isArray(y) ? y.map((z: string) => dec(BigInt(z))) : dec(BigInt(y)))) : dec(BigInt(x)),
      ),
    });
  }

  const out = {
    generator: "vectors/lowki-groth16/gen.ts",
    seed: SEED,
    versions: {
      snarkjs: "0.7.5",
      "@zk-kit/lean-imt": "2.2.4",
      "poseidon-lite": "0.3.0",
      viem: "2.51.3",
      circuits: "@fatsolutions/privacy-pools-core-circuits@1.0.5",
      kohaku: KOHAKU_COMMIT,
    },
    witnessEncoding: "sha256 over concat(32-byte little-endian canonical field elements)",
    cases,
    skipped,
    fixedProofs,
  };
  writeFileSync(`${HERE}/vectors.json`, JSON.stringify(out, null, 1) + "\n");
  console.log(`wrote vectors.json: ${cases.length} cases, ${skipped.length} skipped`);
}

await main();
await (globalThis as any).curve_bn128?.terminate?.();
process.exit(0);
