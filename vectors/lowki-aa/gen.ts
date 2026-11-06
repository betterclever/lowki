// Differential vector generator for crates/lowki-aa.
//
// Calls the real Kohaku functions from the pinned clone
// (vendor/kohaku @ 8c83d64050040654082d70901a46a339bfa103b0, packages/privacy-pools/src)
// plus viem 2.54.1 (the version Kohaku's lockfile resolves) for the ERC-4337 v0.8 and
// EIP-7702 primitives. Deterministic: fixed-seed PRNG, fixed private keys.
//
// Run: npm run gen   (node --import ./resolve-hook.mjs --import tsx gen.ts)
import { writeFileSync } from "node:fs";
import {
  encodeFunctionData,
  erc20Abi,
  getAddress,
  hashTypedData,
  keccak256,
  recoverAddress,
  toHex,
  type Hex,
} from "viem";
import { privateKeyToAccount } from "viem/accounts";
import { hashAuthorization } from "viem/utils";
import {
  entryPoint08Address,
  getUserOperationHash,
  getUserOperationTypedData,
  toPackedUserOperation,
} from "viem/account-abstraction";
import { encodePaymasterData as sdkEncodePaymasterData } from "@privacy-paymasters/sdk";

const KO = "/Users/betterclever/projects/ethglobal/vendor/kohaku/packages/privacy-pools/src";
const { encodeFeeData, encodePaymasterData, encodePoolWithdraw, encodePrivacyPoolAdapterData } =
  await import(`${KO}/paymaster/adapter-data.ts`);
const { buildSignedUserOp, SIMPLE_7702_IMPLEMENTATION } = await import(`${KO}/paymaster/utils.ts`);
const { computeMinimumViableFee, reasonableGasUnits, reasonableGasUnitsForBatch, TAIL_CALLS_DEFAULT_GAS } =
  await import(`${KO}/paymaster/fee.ts`);
const { assertGasLimitsWithinBounds, assertGasPriceWithinBounds } = await import(`${KO}/paymaster/gas-bounds.ts`);
const { prepareNativeShield, prepareErc20Shield } = await import(`${KO}/account/tx/shield.ts`);
const { toWithdrawProof } = await import(`${KO}/utils/encoding.utils.ts`);
const { PrivacyPoolsPaymasterConfigs, PrivacyPoolsV1_0xBow, E_ADDRESS } = await import(`${KO}/config.ts`);

// ---------- deterministic PRNG ----------
let s = 0x9e3779b97f4a7c15n;
const M64 = (1n << 64n) - 1n;
function next64(): bigint {
  // splitmix64
  s = (s + 0x9e3779b97f4a7c15n) & M64;
  let z = s;
  z = ((z ^ (z >> 30n)) * 0xbf58476d1ce4e5b9n) & M64;
  z = ((z ^ (z >> 27n)) * 0x94d049bb133111ebn) & M64;
  return z ^ (z >> 31n);
}
const randBits = (bits: number): bigint => {
  let v = 0n;
  for (let i = 0; i < bits; i += 64) v = (v << 64n) | next64();
  return v & ((1n << BigInt(bits)) - 1n);
};
const randInt = (n: number) => Number(next64() % BigInt(n));
const pick = <T>(xs: readonly T[]): T => xs[randInt(xs.length)]!;
const randBytes = (n: number): Hex => {
  let h = "0x";
  for (let i = 0; i < n; i++) h += Number(randBits(8)).toString(16).padStart(2, "0");
  return h as Hex;
};
const randAddr = (): `0x${string}` => getAddress(randBytes(20));
const randLower = (): `0x${string}` => randBytes(20).toLowerCase() as `0x${string}`;
/** Values of mixed magnitude, including edge cases. */
const randUint = (maxBits: number): bigint => {
  const r = randInt(10);
  if (r === 0) return 0n;
  if (r === 1) return (1n << BigInt(maxBits)) - 1n;
  if (r === 2) return 1n;
  return randBits(1 + randInt(maxBits));
};
const FIELD = 21888242871839275222246405745257275088548364400416034343698204186575808495617n;
const fieldEl = () => randBits(256) % FIELD;
const str = (x: bigint) => x.toString();

// Fixed private keys (never use on a real network).
const KEYS: Hex[] = [
  "0x0000000000000000000000000000000000000000000000000000000000000001",
  "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80",
  "0x59c6995e998f97a5a0044966f0945389dc9e86dae88c7a8412f4603b6b78690d",
  "0x5de4111afa1a4b94908f83103eb1f1706367c2e68ca870fc3fb9a804cdab365a",
  "0x7c852118294e51e653712a81e05800f419141751be58f605c371e15141b007a6",
  "0xfffffffffffffffffffffffffffffffebaaedce6af48a03bbfd25e8cd0364140",
  "0x1111111111111111111111111111111111111111111111111111111111111111",
  "0x4c0883a69102937d6231471b5dbb6204fe5129617082792ae468d01a3f362318",
];
const CHAIN_IDS = [1, 11155111, 10, 8453, 42161, 31337, 17000, 137];

// ---------- 1. ERC-4337 v0.8 userOp hash / packing ----------
const userOpHash: unknown[] = [];
for (let i = 0; i < 64; i++) {
  const chainId = pick(CHAIN_IDS);
  const entryPointAddress = i % 4 === 3 ? randAddr() : entryPoint08Address;
  const withPaymaster = i % 3 !== 0;
  const factoryMode = i % 5; // 0,1,2: none; 3: real factory; 4: 7702 marker (+authorization)
  const op: any = {
    sender: randAddr(),
    nonce: i % 7 === 0 ? 0n : randUint(256),
    callData: randBytes(randInt(4) === 0 ? 0 : randInt(300)),
    callGasLimit: randUint(128),
    verificationGasLimit: randUint(128),
    preVerificationGas: randUint(256),
    maxFeePerGas: randUint(128),
    maxPriorityFeePerGas: randUint(128),
    signature: randBytes(65),
  };
  if (withPaymaster) {
    op.paymaster = randAddr();
    op.paymasterVerificationGasLimit = randUint(128);
    op.paymasterPostOpGasLimit = randUint(128);
    op.paymasterData = randBytes(randInt(3) === 0 ? 0 : randInt(400));
  }
  let authAddress: string | null = null;
  if (factoryMode === 3) {
    op.factory = randAddr();
    op.factoryData = randBytes(randInt(100));
  } else if (factoryMode === 4) {
    op.factory = "0x7702";
    op.factoryData = randBytes(randInt(2) === 0 ? 0 : randInt(68));
    authAddress = randAddr();
    op.authorization = { address: authAddress, chainId, nonce: 0, r: "0x0", s: "0x0", yParity: 0 };
  }
  const packed = toPackedUserOperation(op);
  const packedForHash = toPackedUserOperation(op, { forHash: true });
  const hash = getUserOperationHash({ chainId, entryPointAddress, entryPointVersion: "0.8", userOperation: op });
  const typedHash = hashTypedData(getUserOperationTypedData({ chainId, entryPointAddress, userOperation: op }));
  if (hash !== typedHash) throw new Error("viem hash mismatch");
  userOpHash.push({
    chainId,
    entryPoint: entryPointAddress,
    sender: op.sender,
    nonce: str(op.nonce),
    factory: op.factory ?? null,
    factoryData: op.factoryData ?? null,
    authorizationAddress: authAddress,
    callData: op.callData,
    callGasLimit: str(op.callGasLimit),
    verificationGasLimit: str(op.verificationGasLimit),
    preVerificationGas: str(op.preVerificationGas),
    maxFeePerGas: str(op.maxFeePerGas),
    maxPriorityFeePerGas: str(op.maxPriorityFeePerGas),
    paymaster: op.paymaster ?? null,
    paymasterVerificationGasLimit: op.paymaster ? str(op.paymasterVerificationGasLimit) : null,
    paymasterPostOpGasLimit: op.paymaster ? str(op.paymasterPostOpGasLimit) : null,
    paymasterData: op.paymasterData ?? null,
    signature: op.signature,
    packed: {
      initCode: packed.initCode,
      initCodeForHash: packedForHash.initCode,
      accountGasLimits: packed.accountGasLimits,
      gasFees: packed.gasFees,
      paymasterAndData: packed.paymasterAndData,
    },
    userOpHash: hash,
  });
}

// ---------- 2. EIP-7702 authorization ----------
const eip7702: unknown[] = [];
for (let i = 0; i < 48; i++) {
  const pk = KEYS[i % KEYS.length]!;
  const account = privateKeyToAccount(pk);
  const chainId = i % 6 === 5 ? 0 : pick(CHAIN_IDS);
  const address = i % 3 === 0 ? SIMPLE_7702_IMPLEMENTATION : randAddr();
  const nonce = i % 4 === 0 ? 0 : i % 4 === 1 ? randInt(256) : Number(randBits(48));
  const hash = hashAuthorization({ address, chainId, nonce });
  const signed = await account.signAuthorization({ address, chainId, nonce });
  const recovered = await recoverAddress({ hash, signature: { r: signed.r, s: signed.s, yParity: signed.yParity! } });
  if (recovered !== account.address) throw new Error("7702 recover mismatch");
  eip7702.push({
    privateKey: pk,
    signer: account.address,
    chainId,
    address,
    nonce: String(nonce),
    hash,
    r: signed.r,
    s: signed.s,
    yParity: signed.yParity,
  });
}

// ---------- 3. Kohaku paymaster encodings ----------
const randSnarkProof = () => ({
  proof: {
    pi_a: [str(fieldEl()), str(fieldEl()), "1"],
    pi_b: [
      [str(fieldEl()), str(fieldEl())],
      [str(fieldEl()), str(fieldEl())],
      ["1", "0"],
    ],
    pi_c: [str(fieldEl()), str(fieldEl()), "1"],
    protocol: "groth16",
    curve: "bn128",
  },
  publicSignals: Array.from({ length: 8 }, () => str(i8() ? 0n : fieldEl())),
});
let ctr = 0;
function i8() {
  return ctr++ % 13 === 0;
}
const proofJson = (p: any) => ({
  pA: p.pA.map(str),
  pB: p.pB.map((r: bigint[]) => r.map(str)),
  pC: p.pC.map(str),
  pubSignals: p.pubSignals.map(str),
});

const paymasterEncodings: unknown[] = [];
for (let i = 0; i < 48; i++) {
  const prove = randSnarkProof();
  const struct = toWithdrawProof(prove as any);
  const feeData = {
    recipient: i % 5 === 0 ? randLower() : randAddr(),
    feeRecipient: randAddr(),
    fee: randUint(256),
    refundRecipient: i % 4 === 0 ? "0x0000000000000000000000000000000000000000" : randAddr(),
  } as const;
  const feeDataEnc = encodeFeeData(feeData);
  const adapter = randAddr();
  const withdrawal = { processooor: adapter, data: feeDataEnc };
  const adapterData = encodePrivacyPoolAdapterData(withdrawal, prove as any);
  const paymasterData = encodePaymasterData(adapter, adapterData);
  if (paymasterData !== sdkEncodePaymasterData(adapter, adapterData)) throw new Error("SDK paymasterData mismatch");
  // Direct pool.withdraw for a batch extra note: processooor = sender, data = 0x
  // (and some with arbitrary data to exercise the encoder).
  const directWithdrawal = { processooor: randAddr(), data: i % 3 === 0 ? randBytes(randInt(200)) : "0x" };
  const poolWithdraw = encodePoolWithdraw(directWithdrawal, prove as any);
  paymasterEncodings.push({
    snarkjs: { pi_a: prove.proof.pi_a, pi_b: prove.proof.pi_b, pi_c: prove.proof.pi_c, publicSignals: prove.publicSignals },
    proof: proofJson(struct),
    feeData: { ...feeData, fee: str(feeData.fee) },
    feeDataEncoded: feeDataEnc,
    adapter,
    adapterData,
    paymasterData,
    directWithdrawal,
    poolWithdraw,
  });
}

// ---------- 3b. buildSignedUserOp ----------
type Call = { to: string; value: bigint; data: string };
const randCall = (): Call => ({
  to: randInt(2) ? randAddr() : randLower(),
  value: randInt(3) === 0 ? 0n : randUint(128),
  data: randInt(3) === 0 ? "0x" : randBytes(randInt(200)),
});
const signedUserOps: unknown[] = [];
for (let i = 0; i < 48; i++) {
  const pk = KEYS[i % KEYS.length]!;
  const signer = privateKeyToAccount(pk);
  const chainId = pick(CHAIN_IDS);
  const nCalls = i % 4 === 0 ? -1 : i % 4 === 1 ? 0 : i % 4 === 2 ? 1 : 2 + randInt(4); // -1 = no tailCalls fn
  const calls: Call[] = nCalls > 0 ? Array.from({ length: nCalls }, randCall) : [];
  const isERC20 = randInt(2) === 1;
  const gas =
    nCalls === -1
      ? { ...reasonableGasUnits(isERC20), callGasLimit: 0n }
      : i % 3 === 0
        ? reasonableGasUnitsForBatch(isERC20, randInt(4), nCalls > 0, randInt(2) ? randUint(40) : undefined)
        : {
            callGasLimit: randUint(32),
            verificationGasLimit: randUint(32),
            preVerificationGas: randUint(32),
            paymasterVerificationGasLimit: randUint(32),
            paymasterPostOpGasLimit: randUint(32),
          };
  const maxFeePerGas = randUint(64);
  const maxPriorityFeePerGas = randUint(64);
  // Kohaku passes Number(authorizationNonce ?? nonce) to viem, which rejects values
  // above 2^53-1; the EntryPoint nonce itself is a full uint256 (key || seq).
  const authorizationNonce = i % 3 === 0 ? undefined : randUint(40);
  const nonce = i % 5 === 0 ? undefined : i % 5 === 1 ? 0n : authorizationNonce === undefined ? randUint(53) : randUint(256);
  const paymasterAddress = i % 2 === 0 ? (PrivacyPoolsPaymasterConfigs[1].paymasterAddress as `0x${string}`) : randAddr();
  const paymasterData = randBytes(randInt(5) === 0 ? 0 : 32 * randInt(30));
  const userOp = await buildSignedUserOp({
    signer,
    chainId,
    paymasterAddress,
    paymasterData,
    gas,
    maxFeePerGas,
    maxPriorityFeePerGas,
    nonce,
    authorizationNonce,
    tailCalls: nCalls === -1 ? undefined : async (sender: string) => {
      if (sender !== signer.address) throw new Error("sender mismatch");
      return calls;
    },
  });
  // Cross-check: the signature is over viem's v0.8 userOpHash.
  const hash = getUserOperationHash({
    chainId,
    entryPointAddress: entryPoint08Address,
    entryPointVersion: "0.8",
    userOperation: {
      ...userOp,
      nonce: BigInt(userOp.nonce),
      callGasLimit: BigInt(userOp.callGasLimit),
      verificationGasLimit: BigInt(userOp.verificationGasLimit),
      preVerificationGas: BigInt(userOp.preVerificationGas),
      maxFeePerGas: BigInt(userOp.maxFeePerGas),
      maxPriorityFeePerGas: BigInt(userOp.maxPriorityFeePerGas),
      paymasterVerificationGasLimit: BigInt(userOp.paymasterVerificationGasLimit),
      paymasterPostOpGasLimit: BigInt(userOp.paymasterPostOpGasLimit),
      eip7702Auth: undefined,
    } as any,
  });
  if ((await recoverAddress({ hash, signature: userOp.signature })) !== signer.address) {
    throw new Error("userOp signature does not recover to sender");
  }
  signedUserOps.push({
    privateKey: pk,
    chainId,
    paymasterAddress,
    paymasterData,
    gas: Object.fromEntries(Object.entries(gas).map(([k, v]) => [k, str(v as bigint)])),
    maxFeePerGas: str(maxFeePerGas),
    maxPriorityFeePerGas: str(maxPriorityFeePerGas),
    nonce: nonce === undefined ? null : str(nonce),
    authorizationNonce: authorizationNonce === undefined ? null : str(authorizationNonce),
    tailCalls: nCalls === -1 ? null : calls.map((c) => ({ ...c, value: str(c.value) })),
    userOpHash: hash,
    userOperation: userOp,
  });
}

// ---------- 4. Shield calldata ----------
const shield: unknown[] = [];
for (let i = 0; i < 40; i++) {
  const native = i % 2 === 0;
  const precommitment = i % 9 === 0 ? 0n : fieldEl();
  const amount = randUint(i % 3 === 0 ? 256 : 96);
  const entrypointAddress = i % 4 === 0 ? PrivacyPoolsV1_0xBow[11155111].entrypoint.entrypointAddress : randAddr();
  if (native) {
    const tx = prepareNativeShield({ precommitment, amount, entrypointAddress });
    shield.push({ kind: "native", precommitment: str(precommitment), amount: str(amount), entrypointAddress, tx: { ...tx, value: str(tx.value) } });
  } else {
    const tokenAddress = randInt(2) ? randAddr() : randLower();
    const tx = prepareErc20Shield({ precommitment, amount, tokenAddress, entrypointAddress });
    // Kohaku's PP plugin does not emit the approve; the caller must. Standard ERC-20
    // approve(entrypoint, amount) calldata, via viem's erc20Abi.
    const approve = encodeFunctionData({ abi: erc20Abi, functionName: "approve", args: [entrypointAddress as `0x${string}`, amount] });
    shield.push({
      kind: "erc20",
      precommitment: str(precommitment),
      amount: str(amount),
      entrypointAddress,
      tokenAddress,
      tx: { ...tx, value: str(tx.value) },
      approve: { to: tokenAddress, data: approve, value: "0" },
    });
  }
}

// ---------- 5. Fee helpers ----------
const gasJson = (g: any) => Object.fromEntries(Object.entries(g).map(([k, v]) => [k, str(v as bigint)]));
const fees: any = {
  tailCallsDefaultGas: str(TAIL_CALLS_DEFAULT_GAS),
  reasonableGasUnits: [false, true].map((isERC20) => ({ isERC20, gas: gasJson(reasonableGasUnits(isERC20)) })),
  reasonableGasUnitsForBatch: [] as unknown[],
  computeMinimumViableFee: [] as unknown[],
  gasLimitsBounds: [] as unknown[],
  gasPriceBounds: [] as unknown[],
};
for (let i = 0; i < 40; i++) {
  const isERC20 = i % 2 === 1;
  const extra = randInt(6);
  const hasTailCalls = i % 4 >= 2;
  const est = i % 3 === 0 ? undefined : randUint(40);
  fees.reasonableGasUnitsForBatch.push({
    isERC20, extraWithdrawals: extra, hasTailCalls, tailCallsGasEstimate: est === undefined ? null : str(est),
    gas: gasJson(reasonableGasUnitsForBatch(isERC20, extra, hasTailCalls, est)),
  });
}
const randGas = (bits: number) => ({
  callGasLimit: randUint(bits),
  verificationGasLimit: randUint(bits),
  preVerificationGas: randUint(bits),
  paymasterVerificationGasLimit: randUint(bits),
  paymasterPostOpGasLimit: randUint(bits),
});
for (let i = 0; i < 64; i++) {
  const gas = i < 4 ? reasonableGasUnits(i % 2 === 1) : randGas(i % 5 === 0 ? 128 : 32);
  const maxFeePerGas = randUint(i % 7 === 0 ? 128 : 48);
  fees.computeMinimumViableFee.push({ gas: gasJson(gas), maxFeePerGas: str(maxFeePerGas), fee: str(computeMinimumViableFee(gas, maxFeePerGas)) });
}
const FIELDS = ["callGasLimit", "verificationGasLimit", "preVerificationGas", "paymasterVerificationGasLimit", "paymasterPostOpGasLimit"] as const;
for (let i = 0; i < 80; i++) {
  const baseline = i % 2 === 0 ? reasonableGasUnits(i % 4 === 0) : reasonableGasUnitsForBatch(true, randInt(3), true);
  // Perturb: mostly near the 2x caps so both pass and fail are exercised.
  const gas: any = {};
  for (const f of FIELDS) {
    const b = (baseline as any)[f] as bigint;
    const r = randInt(6);
    gas[f] = r === 0 ? 0n : r === 1 ? b * 2n : r === 2 ? b * 2n + 1n : r === 3 ? (b * BigInt(1 + randInt(30))) / 10n : f === "preVerificationGas" ? BigInt(500_000 + randInt(500_000)) : b;
  }
  const withOp = i % 3 !== 0;
  const op = withOp
    ? { callData: randBytes(randInt(3000)), paymasterData: randInt(4) === 0 ? undefined : randBytes(randInt(3000)) }
    : undefined;
  const limits: any = {};
  if (i % 4 === 1) for (const f of FIELDS) if (randInt(2)) limits[f] = randUint(24);
  let error: string | null = null;
  try {
    assertGasLimitsWithinBounds(gas, baseline, op as any, limits);
  } catch (e: any) {
    error = e.message;
  }
  fees.gasLimitsBounds.push({
    gas: gasJson(gas), baseline: gasJson(baseline),
    callDataLen: op ? (op.callData.length - 2) / 2 : null,
    paymasterDataLen: op ? (op.paymasterData ? (op.paymasterData.length - 2) / 2 : 0) : null,
    callData: op?.callData ?? null, paymasterData: op?.paymasterData ?? null,
    limits: Object.fromEntries(Object.entries(limits).map(([k, v]) => [k, str(v as bigint)])),
    error,
  });
}
for (let i = 0; i < 80; i++) {
  const reference = { baseFeePerGas: randUint(i % 5 === 0 ? 64 : 36), maxPriorityFeePerGas: i % 4 === 0 ? 0n : randUint(33) };
  const tipCap = reference.maxPriorityFeePerGas * 3n > 2_000_000_000n ? reference.maxPriorityFeePerGas * 3n : 2_000_000_000n;
  const feeCap = reference.baseFeePerGas * 3n + tipCap;
  const r = randInt(6);
  const maxPriorityFeePerGas = r === 0 ? tipCap : r === 1 ? tipCap + 1n : randUint(34);
  const maxFeePerGas = r === 2 ? feeCap : r === 3 ? feeCap + 1n : r === 4 ? maxPriorityFeePerGas : randInt(2) ? maxPriorityFeePerGas + randUint(36) : randUint(36);
  let error: string | null = null;
  try {
    assertGasPriceWithinBounds({ maxFeePerGas, maxPriorityFeePerGas }, reference);
  } catch (e: any) {
    error = e.message;
  }
  fees.gasPriceBounds.push({
    maxFeePerGas: str(maxFeePerGas), maxPriorityFeePerGas: str(maxPriorityFeePerGas),
    baseFeePerGas: str(reference.baseFeePerGas), referenceMaxPriorityFeePerGas: str(reference.maxPriorityFeePerGas),
    error,
  });
}

const out = {
  meta: {
    kohakuCommit: "8c83d64050040654082d70901a46a339bfa103b0",
    kohakuPackage: "@kohaku-eth/privacy-pools@0.1.0-alpha.37 (source)",
    viem: "2.54.1",
    privacyPaymastersSdk: "0.0.5",
    entryPoint08: entryPoint08Address,
    simple7702Implementation: SIMPLE_7702_IMPLEMENTATION,
    eAddress: E_ADDRESS,
    paymasterConfigs: PrivacyPoolsPaymasterConfigs,
  },
  userOpHash,
  eip7702,
  paymasterEncodings,
  signedUserOps,
  shield,
  fees,
};
writeFileSync(new URL("./vectors.json", import.meta.url), JSON.stringify(out, null, 1) + "\n");
const count = (x: any) => (Array.isArray(x) ? x.length : 0);
console.log(
  `userOpHash=${count(userOpHash)} eip7702=${count(eip7702)} paymasterEncodings=${count(paymasterEncodings)} ` +
    `signedUserOps=${count(signedUserOps)} shield=${count(shield)} batchGas=${fees.reasonableGasUnitsForBatch.length} ` +
    `minFee=${fees.computeMinimumViableFee.length} gasLimitsBounds=${fees.gasLimitsBounds.length} gasPriceBounds=${fees.gasPriceBounds.length}`,
);
console.log("keccak(vectors) =", keccak256(toHex(JSON.stringify(out))));
