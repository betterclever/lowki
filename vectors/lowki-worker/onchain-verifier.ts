// On-chain check of our Groth16 proof encoding against the deployed Privacy Pools
// withdrawal verifier on Sepolia (read-only eth_call, nothing is broadcast).
//
//   npx tsx onchain-verifier.ts [proofs.json ...]
//
// 1. Reads WITHDRAWAL_VERIFIER() from the Sepolia ETH pool and checks the pool's ENTRYPOINT().
// 2. For every withdraw proof in the given files (Rust-made: lowki-groth16 rust-proofs.json,
//    or lowki-worker's worker-proofs.json), eth_calls
//    verifyProof(uint[2] pA, uint[2][2] pB, uint[2] pC, uint[8] pubSignals) with the proof's
//    `solidity` struct and expects true; then with a tampered pA, pC and pubSignal, expects false.
import { readFileSync } from 'node:fs';
import { createPublicClient, http, parseAbi, getAddress } from 'viem';
import { sepolia } from 'viem/chains';

const RPC = process.env.SEPOLIA_RPC ?? 'https://ethereum-sepolia-rpc.publicnode.com';
const POOL = getAddress('0x644d5A2554d36e27509254F32ccfeBe8cd58861f');
const ENTRYPOINT = getAddress('0x34A2068192b1297f2a7f85D7D8CdE66F8F0921cB');

const poolAbi = parseAbi([
  'function WITHDRAWAL_VERIFIER() view returns (address)',
  'function ENTRYPOINT() view returns (address)',
]);
const verifierAbi = parseAbi([
  'function verifyProof(uint256[2] _pA, uint256[2][2] _pB, uint256[2] _pC, uint256[8] _pubSignals) view returns (bool)',
]);

const client = createPublicClient({ chain: sepolia, transport: http(RPC) });

type Sol = { pA: string[]; pB: string[][]; pC: string[]; pubSignals: string[] };
type Entry = { circuit?: string; name?: string; solidity: Sol };

const files = process.argv.slice(2);
if (files.length === 0) files.push('../lowki-groth16/out/rust-proofs.json');

const block = await client.getBlockNumber();
const verifier = await client.readContract({ address: POOL, abi: poolAbi, functionName: 'WITHDRAWAL_VERIFIER' });
const ep = await client.readContract({ address: POOL, abi: poolAbi, functionName: 'ENTRYPOINT' });
if (getAddress(ep) !== ENTRYPOINT) throw new Error(`pool ENTRYPOINT ${ep} != ${ENTRYPOINT}`);
console.log(`rpc ${RPC} block ${block}`);
console.log(`pool ${POOL} -> WITHDRAWAL_VERIFIER ${verifier} (ENTRYPOINT ${ep})`);

const big = (s: string) => BigInt(s);
const args = (s: Sol) =>
  [
    [big(s.pA[0]), big(s.pA[1])],
    [
      [big(s.pB[0][0]), big(s.pB[0][1])],
      [big(s.pB[1][0]), big(s.pB[1][1])],
    ],
    [big(s.pC[0]), big(s.pC[1])],
    s.pubSignals.map(big),
  ] as const;

const call = (s: Sol) =>
  client.readContract({ address: verifier, abi: verifierAbi, functionName: 'verifyProof', args: args(s) as any, blockNumber: block });

let failed = 0;
let n = 0;
for (const f of files) {
  const text = readFileSync(f, 'utf8').trim();
  const raw = text.startsWith('[') || !text.includes('\n') ? JSON.parse(text) : text.split('\n').filter(Boolean).map((l) => JSON.parse(l));
  const entries: Entry[] = (Array.isArray(raw) ? raw : [raw]).filter(
    (e: Entry) => (e.circuit ?? 'withdraw') === 'withdraw' && e.solidity?.pubSignals?.length === 8,
  );
  for (const e of entries) {
    n++;
    const ok = await call(e.solidity);
    // tamper: (1) pA.x + 1 (off-curve point -> verifier returns false), (2) swap pB limbs back
    // to snarkjs order (catches an encoding mistake), (3) a public signal + 1
    const t1 = structuredClone(e.solidity);
    t1.pA[0] = (big(t1.pA[0]) + 1n).toString();
    const t2 = structuredClone(e.solidity);
    t2.pB = t2.pB.map((c) => [c[1], c[0]]);
    const t3 = structuredClone(e.solidity);
    t3.pubSignals[2] = (big(t3.pubSignals[2]) + 1n).toString();
    const [r1, r2, r3] = [await call(t1), await call(t2), await call(t3)];
    const pass = ok === true && r1 === false && r2 === false && r3 === false;
    if (!pass) failed++;
    console.log(
      `${pass ? 'PASS' : 'FAIL'} ${f}:${e.name ?? n} real=${ok} tamperedPA=${r1} unswappedPB=${r2} tamperedSignal=${r3}`,
    );
  }
}
if (n === 0) throw new Error('no withdraw proofs found');
console.log(failed ? `FAILED ${failed}/${n}` : `ALL ${n} withdraw proofs: verifier true for real, false for tampered`);
process.exit(failed ? 1 : 0);
