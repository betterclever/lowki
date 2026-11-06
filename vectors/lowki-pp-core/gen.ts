// Differential-vector generator for crates/lowki-pp-core.
//
// Every expected value is produced by the *Kohaku source files* of the pinned clone
// (vendor/kohaku @ 8c83d64050040654082d70901a46a339bfa103b0), imported directly via
// resolve-hook.mjs, plus maci-crypto 2.5.0 (the exact version Kohaku pins) for
// arbitrary-arity Poseidon. Run: `npm run gen` (writes vectors.json).
import { readFileSync, writeFileSync } from "node:fs";
import { execSync } from "node:child_process";
import {
  concat, encodePacked, hashTypedData, keccak256, parseSignature, stringToBytes, toBytes, toHex,
  type Hex,
} from "viem";
import { poseidon as maciPoseidon } from "maci-crypto/build/ts/hashing.js";
import { LeanIMT as ZkKitLeanIMT } from "@zk-kit/lean-imt";

const K = "../../../vendor/kohaku/packages";
const KOHAKU_DIR = new URL("../../../vendor/kohaku", import.meta.url).pathname;
const { poseidonT3 } = await import(`${K}/poseidon/src/index.ts`);
const { PoseidonT3LeanIMT } = await import(`${K}/poseidon-lean-imt/src/index.ts`);
const { SignatureSecretManager, KeystoreEnvelopeSigner } = await import(`${K}/privacy-pools/src/account/signature-keys.ts`);
const { SecretManager } = await import(`${K}/privacy-pools/src/account/keys.ts`);
const { MnemonicKeystore } = await import(`${K}/plugins/src/host/mnemonic-keystore.ts`);
const { calculateContext, proofFromTree, SNARK_SCALAR_FIELD } = await import(`${K}/privacy-pools/src/utils/proof.util.ts`);
const { commitment } = await import(`${K}/privacy-pools/src/utils.ts`);

const P: bigint = SNARK_SCALAR_FIELD;
const U256_MAX = (1n << 256n) - 1n;
const SECP_N = 0xfffffffffffffffffffffffffffffffebaaedce6af48a03bbfd25e8cd0364141n;

const h32 = (x: bigint): string => "0x" + x.toString(16).padStart(64, "0");
const assert = (c: boolean, msg: string) => { if (!c) throw new Error("assert: " + msg); };

// ---------- deterministic PRNG: keccak256("lowki-pp-core-vectors" ‖ u64 counter) ----------
const SEED = "lowki-pp-core-vectors";
let ctr = 0n;
function randBytes32(): bigint {
  const h = keccak256(concat([stringToBytes(SEED), toBytes(ctr++, { size: 8 })]));
  return BigInt(h);
}
const randField = () => randBytes32() % P;
const randInt = (n: number) => Number(randBytes32() % BigInt(n));
function randBytes(len: number): Hex {
  let out = "";
  while (out.length < len * 2) out += randBytes32().toString(16).padStart(64, "0");
  return ("0x" + out.slice(0, len * 2)) as Hex;
}
const randAddr = () => randBytes(20);

// ---------- 1. Poseidon ----------
const poseidonCases: any[] = [];
function addPoseidon(inputs: bigint[], note: string) {
  const out = maciPoseidon(inputs);
  const sources = ["maci-crypto"];
  if (inputs.length === 2) {
    const t3 = poseidonT3(inputs[0], inputs[1]);
    assert(t3 === out, `T3 vs maci mismatch for ${inputs}`);
    sources.push("@kohaku-eth/poseidon T3");
  }
  poseidonCases.push({ note, inputs: inputs.map(h32), output: h32(out), sources });
}
for (const arity of [1, 2, 3]) {
  const edge: bigint[] = [0n, 1n, 2n, P - 1n];
  for (const e of edge) addPoseidon(Array(arity).fill(e), "edge");
  addPoseidon(Array.from({ length: arity }, (_, i) => BigInt(i + 1)), "edge 1..n");
  // Inputs >= p: both references reduce mod p (maci: Fr.e, T3: x % p).
  for (const e of [P, P + 1n, U256_MAX]) {
    const ins = Array.from({ length: arity }, (_, i) => (i === arity - 1 ? e : BigInt(i + 7)));
    addPoseidon(ins, "input >= p (reduced mod p)");
    assert(maciPoseidon(ins) === maciPoseidon(ins.map((x) => x % P)), "maci reduces");
  }
  for (let i = 0; i < 100; i++) {
    const raw = i % 10 === 9; // every 10th case uses full 256-bit inputs
    addPoseidon(Array.from({ length: arity }, () => (raw ? randBytes32() : randField())), raw ? "random u256 (reduced)" : "random");
  }
}

// ---------- 2. LeanIMT ----------
// Leaves of a tree are either explicit or derived: leaf(i) = keccak256("lowki-limt" ‖ u32 tree ‖ u32 (i % distinct)) mod p
function derivedLeaf(tree: number, i: number): bigint {
  return BigInt(keccak256(concat([stringToBytes("lowki-limt"), toBytes(tree, { size: 4 }), toBytes(i, { size: 4 })]))) % P;
}
const limtCases: any[] = [];
const sizes = [1, 2, 3, 4, 5, 6, 7, 8, 9, 11, 15, 16, 17, 31, 32, 33, 63, 64, 65, 100, 127, 128, 129, 255, 256, 257, 511, 1000, 1023, 1024, 1025, 2047, 2048, 2049];
type Spec = { size: number; distinct?: number; explicit?: bigint[]; mode: "many" | "insert" | "chunks" };
const specs: Spec[] = [];
sizes.forEach((size, k) => specs.push({ size, mode: (["many", "insert", "chunks"] as const)[k % 3] }));
specs.push({ size: 10, distinct: 3, mode: "many" });            // duplicates
specs.push({ size: 37, distinct: 5, mode: "chunks" });
specs.push({ size: 300, distinct: 290, mode: "insert" });
specs.push({ size: 4, explicit: [7n, 7n, 7n, 7n], mode: "many" }); // all equal
specs.push({ size: 5, explicit: [0n, P - 1n, 0n, 1n, P - 1n], mode: "insert" }); // edge values
specs.push({ size: 3, explicit: [0n, 0n, 0n], mode: "chunks" });

specs.forEach((spec, treeId) => {
  const distinct = spec.distinct ?? spec.size;
  const leaves = spec.explicit ?? Array.from({ length: spec.size }, (_, i) => derivedLeaf(treeId, i % distinct));
  const ops: any[] = [];
  let tree: any;
  if (spec.mode === "many") {
    tree = new PoseidonT3LeanIMT(leaves);
    ops.push({ kind: "insert_many", count: leaves.length, root: h32(tree.root) });
  } else if (spec.mode === "insert") {
    tree = new PoseidonT3LeanIMT();
    for (const l of leaves) {
      tree.insert(l);
      if (leaves.length <= 64) ops.push({ kind: "insert", count: 1, root: h32(tree.root) });
    }
    if (leaves.length > 64) ops.push({ kind: "insert", count: leaves.length, root: h32(tree.root) });
  } else {
    tree = new PoseidonT3LeanIMT();
    let pos = 0;
    while (pos < leaves.length) {
      const n = Math.min(leaves.length - pos, 1 + randInt(Math.max(2, Math.ceil(leaves.length / 3))));
      tree.insertMany(leaves.slice(pos, pos + n));
      pos += n;
      ops.push({ kind: "insert_many", count: n, root: h32(tree.root) });
    }
  }
  // Cross-check with @zk-kit/lean-imt (what Kohaku's tree claims compatibility with).
  // Skipped when a leaf is 0: zk-kit's JS insertMany tests `if (rightNode)`, so a zero
  // right child is treated as absent and the trees diverge. On-chain the zk-kit
  // Solidity LeanIMT rejects zero leaves (LeafCannotBeZero), so this never matters in
  // practice; Kohaku's tree (the ground truth here) hashes zeros like any other value.
  if (!leaves.includes(0n)) {
    const zk = new ZkKitLeanIMT((a: bigint, b: bigint) => poseidonT3(a, b), leaves);
    assert(zk.root === tree.root, `zk-kit root mismatch tree ${treeId}`);
  }
  assert(new PoseidonT3LeanIMT(leaves).root === tree.root, "build modes disagree");

  const n = leaves.length;
  const idxs = new Set<number>([0, n - 1, Math.floor(n / 2), Math.max(0, n - 2)]);
  for (let i = 0; i < 3; i++) idxs.add(randInt(n));
  const proofs = [...idxs].sort((a, b) => a - b).map((index) => {
    const p = tree.generateProof(index);
    assert(tree.verifyProof(p), "verify");
    return { index, root: h32(p.root), leaf: h32(p.leaf), path_index: p.index, siblings: p.siblings.map(h32) };
  });
  // proofFromTree (Kohaku privacy-pools): indexOf(first occurrence) + generateProof + pad to 32 with 0.
  const fromTreeLeaves = [...new Set([leaves[0], leaves[n - 1], leaves[randInt(n)]])];
  const proofs_from_tree = fromTreeLeaves.map((leaf) => {
    const p = proofFromTree(tree, leaf);
    return { leaf: h32(leaf), root: h32(p.root), path_index: p.index, siblings: p.siblings.map(h32) };
  });
  const index_of = [...fromTreeLeaves.map((l) => ({ leaf: h32(l), index: tree.indexOf(l) })),
    { leaf: h32(P - 2n), index: tree.indexOf(P - 2n) }];
  // Negative verification cases (Kohaku's static verifyProof).
  const p0 = tree.generateProof(n - 1);
  const bad: any[] = [];
  if (p0.siblings.length > 0) {
    const tampered = { ...p0, siblings: [...p0.siblings] };
    tampered.siblings[0] = (tampered.siblings[0] + 1n) % P;
    bad.push(tampered);
    bad.push({ ...p0, index: p0.index ^ 1 });
  }
  bad.push({ ...p0, leaf: (p0.leaf + 1n) % P });
  bad.push({ ...p0, root: (p0.root + 1n) % P });
  const verify_cases = [p0, ...bad].map((p) => ({
    root: h32(p.root), leaf: h32(p.leaf), path_index: p.index, siblings: p.siblings.map(h32),
    valid: PoseidonT3LeanIMT.verifyProof(p),
  }));

  limtCases.push({
    tree_id: treeId, mode: spec.mode, size: n,
    leaves: spec.explicit || n <= 16 ? leaves.map(h32) : null,
    derived: spec.explicit || n <= 16 ? null : { tree: treeId, distinct },
    ops, root: h32(tree.root), depth: tree.depth, proofs, proofs_from_tree, index_of, verify_cases,
  });
});

// ---------- 3. SignatureSecretManager (default) and legacy SecretManager ----------
const MNEMONIC = "test test test test test test test test test test test junk";
const keystore = new MnemonicKeystore(MNEMONIC);
const host = { keystore } as any;
const MAINNET_EP = 0x6818809eefce719e480a7526d76bd3e561526b46n;
const SEPOLIA_EP = 0x34a2068192b1297f2a7f85d7d8cde66f8f0921cbn;
const pairs: [bigint, bigint][] = [
  [1n, MAINNET_EP], [11155111n, SEPOLIA_EP], [1n, SEPOLIA_EP], [11155111n, MAINNET_EP],
  [10n, BigInt(randAddr())], [31337n, 1n], [137n, (1n << 160n) - 1n], [11155111n, BigInt(randAddr())],
];
const depositIdx = [0, 1, 2, 7, 1000, 4294967295, Number.MAX_SAFE_INTEGER];
const secretIdx = [0, 1, 2, 5, 255];

class CapturingSigner {
  last: any;
  inner: any;
  constructor(accountIndex: number) { this.inner = new KeystoreEnvelopeSigner(keystore, accountIndex); }
  async signEnvelope(env: any) { this.last = env; return this.inner.signEnvelope(env); }
}

const accounts: any[] = [];
const sigCases: any[] = [];
for (const accountIndex of [0, 1, 2]) {
  const accountKey = await keystore.deriveAt(`m/28784'/1'/${accountIndex}'`);
  accounts.push({ account_index: accountIndex, path: `m/28784'/1'/${accountIndex}'`, private_key: accountKey });
  const sm = SignatureSecretManager({ host, accountIndex });
  const cap = new CapturingSigner(accountIndex);
  const smCap = SignatureSecretManager({ host, accountIndex, signer: cap, verifyDeterminism: false });
  for (const [chainId, ep] of pairs) {
    for (const d of depositIdx) {
      for (const s of secretIdx) {
        // keep the matrix manageable: full matrix on account 0, sampled on others
        if (accountIndex > 0 && randInt(4) !== 0) continue;
        const secret = s === 0
          ? await sm.getDepositSecrets({ chainId, entrypointAddress: ep, depositIndex: d })
          : await sm.getSecrets({ chainId, entrypointAddress: ep, depositIndex: d, withdrawIndex: s });
        const eph = await sm.deriveEphemeralSigner({ chainId, entrypointAddress: ep, depositIndex: d, withdrawIndex: s });
        const capSecret = await smCap.getSecrets({ chainId, entrypointAddress: ep, depositIndex: d, withdrawIndex: s });
        assert(capSecret.nullifier === secret.nullifier, "capturing signer agrees");
        const env = cap.last;
        const sig: Hex = await cap.inner.signEnvelope(env);
        sigCases.push({
          account_index: accountIndex, chain_id: chainId.toString(), entrypoint: toHex(ep, { size: 20 }),
          deposit_index: d.toString(), secret_index: s.toString(),
          typed_data_hash: hashTypedData(env), signature: sig,
          nullifier: h32(secret.nullifier), salt: h32(secret.salt),
          precommitment: h32(secret.precommitment), nullifier_hash: h32(secret.nullifierHash),
          ephemeral_signer: eph,
        });
      }
    }
  }
}

// Signer-framing independence: feed Kohaku's manager malleated / compact signatures
// of the same note through a custom EnvelopeSigner; secrets must not change.
const framingCases: any[] = [];
for (let i = 0; i < 12; i++) {
  const c = sigCases[randInt(sigCases.length)];
  const { r, s, yParity } = parseSignature(c.signature);
  const sBig = BigInt(s);
  const variants: [string, Hex][] = [
    ["high-s, v flipped", concat([r, toHex(SECP_N - sBig, { size: 32 }), toHex(yParity === 0 ? 28 : 27, { size: 1 })])],
    ["v as 0/1", concat([r, s, toHex(yParity, { size: 1 })])],
    // Kohaku's comment claims ERC-2098 support, but viem 2.54.1 parseSignature rejects
    // 64-byte input (and any v outside {0,1,27,28}); recorded as rejections.
    ["erc-2098 compact", concat([r, toHex(sBig | (BigInt(yParity) << 255n), { size: 32 })])],
    ["v = 29", concat([r, s, toHex(29, { size: 1 })])],
    ["s = 0", concat([r, toHex(0n, { size: 32 }), toHex(27, { size: 1 })])],
    ["s = n", concat([r, toHex(SECP_N, { size: 32 }), toHex(27, { size: 1 })])],
  ];
  for (const [kind, framed] of variants) {
    const sm = SignatureSecretManager({ host, signer: { signEnvelope: async () => framed }, verifyDeterminism: false });
    const params = { chainId: BigInt(c.chain_id), entrypointAddress: BigInt(c.entrypoint), depositIndex: Number(c.deposit_index), withdrawIndex: Number(c.secret_index) };
    let sec: any, eph: any, error: string | null = null;
    try {
      sec = await sm.getSecrets(params);
      eph = await sm.deriveEphemeralSigner(params);
    } catch (e: any) {
      error = String(e.message ?? e).split("\n")[0];
    }
    if (error === null) {
      assert(h32(sec.nullifier) === c.nullifier && eph === c.ephemeral_signer, "framing invariance");
      framingCases.push({ kind, signature: framed, accepted: true, nullifier: h32(sec.nullifier), salt: h32(sec.salt),
        precommitment: h32(sec.precommitment), nullifier_hash: h32(sec.nullifierHash), ephemeral_signer: eph });
    } else {
      framingCases.push({ kind, signature: framed, accepted: false, error });
    }
  }
}

// Legacy (deprecated) HD SecretManager.
const legacyCases: any[] = [];
for (const accountIndex of [0, 1]) {
  const sm = SecretManager({ host, accountIndex });
  for (const [chainId, ep] of pairs.slice(0, 5)) {
    for (const d of [0, 1, 3, 42]) {
      for (const s of [0, 1, 2]) {
        const path = (t: number) => `m/28784'/1'/${accountIndex}'/${t}'/${d}'/${s}'`;
        const secret = s === 0
          ? await sm.getDepositSecrets({ chainId, entrypointAddress: ep, depositIndex: d })
          : await sm.getSecrets({ chainId, entrypointAddress: ep, depositIndex: d, withdrawIndex: s });
        legacyCases.push({
          account_index: accountIndex, chain_id: chainId.toString(), entrypoint: toHex(ep, { size: 20 }),
          deposit_index: d, secret_index: s,
          nullifier_secret: await keystore.deriveAt(path(0)),
          salt_secret: await keystore.deriveAt(path(1)),
          signer_secret: await keystore.deriveAt(path(2)),
          nullifier: h32(secret.nullifier), salt: h32(secret.salt),
          precommitment: h32(secret.precommitment), nullifier_hash: h32(secret.nullifierHash),
          ephemeral_signer: await sm.deriveEphemeralSigner({ chainId, entrypointAddress: ep, depositIndex: d, withdrawIndex: s }),
        });
      }
    }
  }
}

// ---------- 4. commitment / context / scope / label ----------
const commitmentCases: any[] = [];
const cEdges: [bigint, bigint, bigint][] = [[0n, 0n, 0n], [P - 1n, P - 1n, P - 1n], [1n, 2n, 3n], [10n ** 18n, P - 1n, 0n]];
for (const [v, l, pc] of cEdges) commitmentCases.push({ value: h32(v), label: h32(l), precommitment: h32(pc), commitment: h32(commitment({ balance: v, label: l, precommitment: pc } as any)) });
for (let i = 0; i < 80; i++) {
  const v = i % 2 ? randBytes32() >> 128n : randField(); // realistic balances and full-field values
  const l = randField(), pc = randField();
  commitmentCases.push({ value: h32(v), label: h32(l), precommitment: h32(pc), commitment: h32(commitment({ balance: v, label: l, precommitment: pc } as any)) });
}

const contextCases: any[] = [];
const dataLens = [0, 1, 20, 31, 32, 33, 64, 65, 100, 256, 500];
const scopeEdges = [0n, 1n, P - 1n, P, U256_MAX];
for (let i = 0; i < 80; i++) {
  const processooor = i === 0 ? "0x0000000000000000000000000000000000000000" : randAddr();
  const data = randBytes(dataLens[i % dataLens.length]);
  const scope = i < scopeEdges.length ? scopeEdges[i] : (i % 3 ? randField() : randBytes32());
  const ctx = calculateContext({ processooor, data } as any, scope);
  contextCases.push({ processooor, data, scope: h32(scope), context: ctx, context_padded: h32(BigInt(ctx)) });
}

// scope/label: Solidity derivation (privacy-pools-core State.sol / PrivacyPool.sol):
//   SCOPE = keccak256(abi.encodePacked(address(pool), block.chainid, asset)) % SNARK_SCALAR_FIELD
//   label = keccak256(abi.encodePacked(SCOPE, ++nonce)) % SNARK_SCALAR_FIELD
const labelCases: any[] = [];
for (let i = 0; i < 40; i++) {
  const pool = randAddr();
  const chainId = [1n, 11155111n, 10n, 8453n][i % 4];
  const asset = i % 5 === 0 ? "0xEeeeeEeeeEeEeeEeEeEeeEEEeeeeEeeeeeeeEEeE" : randAddr();
  const scope = BigInt(keccak256(encodePacked(["address", "uint256", "address"], [pool, chainId, asset]))) % P;
  const nonce = i === 0 ? 1n : i === 1 ? 2n : i === 2 ? U256_MAX : BigInt(1 + randInt(100000));
  const label = BigInt(keccak256(encodePacked(["uint256", "uint256"], [scope, nonce]))) % P;
  labelCases.push({ pool, chain_id: chainId.toString(), asset, scope: h32(scope), nonce: h32(nonce), label: h32(label) });
}
const onchain = JSON.parse(readFileSync(new URL("./onchain.json", import.meta.url), "utf8"));

const pkgv = (name: string) => JSON.parse(readFileSync(new URL(`./node_modules/${name}/package.json`, import.meta.url), "utf8")).version;
const kohakuCommit = execSync("git rev-parse HEAD", { cwd: KOHAKU_DIR }).toString().trim();
const out = {
  meta: {
    kohaku_commit: kohakuCommit,
    packages: Object.fromEntries(["maci-crypto", "@zk-kit/poseidon-cipher", "@zk-kit/lean-imt", "viem", "@scure/bip32", "@scure/bip39", "@noble/hashes", "ox"].map((n) => [n, pkgv(n)])),
    prng: `keccak256("${SEED}" ‖ u64be counter)`,
    mnemonic: MNEMONIC,
    snark_scalar_field: h32(P),
  },
  poseidon: poseidonCases,
  limt: limtCases,
  accounts,
  signature_secrets: sigCases,
  signature_framing: framingCases,
  legacy_secrets: legacyCases,
  commitment: commitmentCases,
  context: contextCases,
  label: labelCases,
  onchain,
};
writeFileSync(new URL("./vectors.json", import.meta.url), JSON.stringify(out, null, 1) + "\n");
console.log(`kohaku ${kohakuCommit}`);
console.log(Object.entries(out).filter(([k]) => k !== "meta").map(([k, v]: any) => `${k}: ${Array.isArray(v) ? v.length : (v.deposits?.length ?? "-")}`).join("\n"));
