// Hypothesis test: is Sepolia's testnet ASP root rebuildable from chain data (LeanIMT of all
// deposit labels in deposit order)? Prints how many RootUpdated roots equal a prefix of the label
// list. Needs the ASP-blind pool-state cache that scripts/fork-private-send.ts writes.
//   node --import ./resolve-hook.mjs --import tsx scripts/asp-from-chain-probe.ts
// Result 2026-11-06: 20 of 802 roots match (all <= block 8,719,849, prefixes up to 36 labels);
// none after that, also not with 1-2 labels excluded. See docs/PRIVATE-SEND.md.
import { readFileSync } from "node:fs";
import { createPublicClient, decodeAbiParameters, http, toEventSelector, type Hex } from "viem";
import { PoseidonT3LeanIMT as LeanIMT } from "@kohaku-eth/poseidon-lean-imt";
const EP = "0x34A2068192b1297f2a7f85D7D8CdE66F8F0921cB";
const ETH_POOL = "0x644d5a2554d36e27509254f32ccfebe8cd58861f";
const st = Object.values(JSON.parse(readFileSync(new URL("../.cache/pp-state-aspblind-11155111-12078500.json", import.meta.url), "utf8")) as any[])[0];
const deps = (st.deposits.depositsTuples as [string, any][]).map(([, d]) => ({ label: BigInt(d.label), block: BigInt(d.blockNumber), pool: d.pool.toLowerCase() }));
const pub = createPublicClient({ transport: http("https://sepolia.gateway.tenderly.co", { timeout: 120_000 }) });
const topic = toEventSelector("event RootUpdated(uint256 _root, string _ipfsCID, uint256 _timestamp)");
const roots: { root: bigint; block: bigint }[] = [];
for (let from = 8_461_453n; from <= 12_078_500n; from += 200_000n) {
  const logs = await pub.request({ method: "eth_getLogs", params: [{ address: EP, topics: [topic], fromBlock: `0x${from.toString(16)}`, toBlock: `0x${(from + 199_999n).toString(16)}` }] } as never) as any[];
  for (const l of logs) roots.push({ root: decodeAbiParameters([{ type: "uint256" }, { type: "string" }, { type: "uint256" }], l.data as Hex)[0], block: BigInt(l.blockNumber) });
}
const idx = new Map(roots.map((r, i) => [r.root, i]));
for (const [name, list] of [["all pools", deps], ["ETH pool", deps.filter((d) => d.pool === ETH_POOL)]] as const) {
  const t = new LeanIMT(); let lastHit = -1; const hits: string[] = [];
  list.forEach((d, n) => { t.insert(d.label); const i = idx.get(t.root); if (i !== undefined) { hits.push(`n=${n + 1}->root#${i}@${roots[i]!.block}(dep@${d.block})`); lastHit = n; } });
  console.log(`${name}: ${hits.length} prefix matches; last at prefix ${lastHit + 1}/${list.length}`);
  console.log("  " + hits.slice(-6).join("  "));
  if (lastHit + 1 < list.length) console.log(`  first unmatched deposit: block ${list[lastHit + 1]!.block}; next roots after last hit:`, roots.filter((r) => r.block >= list[lastHit]!.block).slice(0, 3).map((r) => String(r.block)).join(","));
}
console.log(`roots total ${roots.length}; distinct ${new Set(roots.map((r) => r.root)).size}`);
