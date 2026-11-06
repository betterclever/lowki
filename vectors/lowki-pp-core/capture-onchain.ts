// One-off capture of real mainnet Privacy Pools deposits, used to prove the
// scope / label / commitment derivation against chain data (not just against a
// TS re-implementation). Output: onchain.json (committed; gen.ts folds it into
// vectors.json). Re-run with `npx tsx capture-onchain.ts [rpcUrl]`.
import { createPublicClient, http, parseAbi, keccak256, encodePacked } from "viem";
import { mainnet } from "viem/chains";
import { writeFileSync } from "node:fs";
import { poseidon } from "maci-crypto/build/ts/hashing.js";

const rpc = process.argv[2] ?? "https://ethereum-rpc.publicnode.com";
const c = createPublicClient({ chain: mainnet, transport: http(rpc) });
const P = 21888242871839275222246405745257275088548364400416034343698204186575808495617n;
// 0xbow ETH pool on mainnet (see kohaku packages/privacy-pools/src/config.ts)
const pool = "0xf241d57c6debae225c0f2e6ea1529373c9a9c9fb";
const abi = parseAbi([
  "function SCOPE() view returns (uint256)",
  "function nonce() view returns (uint256)",
  "function ASSET() view returns (address)",
  "event Deposited(address indexed _depositor, uint256 _commitment, uint256 _label, uint256 _value, uint256 _precommitment)",
]);
const scope = await c.readContract({ address: pool, abi, functionName: "SCOPE" });
const nonce = await c.readContract({ address: pool, abi, functionName: "nonce" });
const asset = await c.readContract({ address: pool, abi, functionName: "ASSET" });
const recomputed = BigInt(keccak256(encodePacked(["address", "uint256", "address"], [pool, 1n, asset]))) % P;
if (recomputed !== scope) throw new Error("scope mismatch");

const latest = await c.getBlockNumber();
let logs: any[] = [];
let to = latest;
for (let i = 0; i < 60 && logs.length < 4; i++) {
  const from = to - 9000n;
  logs = logs.concat(await c.getLogs({ address: pool, event: abi[3], fromBlock: from, toBlock: to }));
  to = from - 1n;
}
const deposits = [];
for (const l of logs.slice(0, 4)) {
  const { _label, _value, _precommitment, _commitment } = l.args;
  let found = 0n;
  for (let n = nonce; n >= 1n; n--) {
    if (BigInt(keccak256(encodePacked(["uint256", "uint256"], [scope, n]))) % P === _label) { found = n; break; }
  }
  if (found === 0n) throw new Error("label nonce not found");
  if (poseidon([_value, _label, _precommitment]) !== _commitment) throw new Error("commitment mismatch");
  deposits.push({
    tx: l.transactionHash, block: l.blockNumber.toString(), nonce: found.toString(),
    label: _label.toString(), value: _value.toString(), precommitment: _precommitment.toString(), commitment: _commitment.toString(),
  });
}
writeFileSync("onchain.json", JSON.stringify({
  chainId: "1", pool, asset, scope: scope.toString(), capturedAtBlock: latest.toString(), deposits,
}, null, 2) + "\n");
console.log("wrote onchain.json with", deposits.length, "deposits");
