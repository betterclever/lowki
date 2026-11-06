// Live check of the name claim path WITHOUT the browser: lowki-worker (Node build) signs, the
// running lowki-server (scripts/home-server.sh, :8830) submits on live Sepolia, then plain viem
// resolves the name through the gateway (public RPC -> UniversalResolver -> StealthResolver ->
// CCIP-Read -> the tunnel -> lowki-server).
//
//   node apps/wallet-ui/scripts/claim-live.mjs test-<label>
//
// Registers a REAL name: use only `test-` labels. The wallet is random per run (fake PRF).
import fs from "node:fs";
import path from "node:path";
import { createRequire } from "node:module";
import { fileURLToPath, pathToFileURL } from "node:url";
import { randomBytes } from "node:crypto";

const here = path.dirname(fileURLToPath(import.meta.url));
const REPO = path.resolve(here, "../../..");
const require = createRequire(import.meta.url);
const { LowkiWorker } = require(path.join(REPO, "crates/lowki-worker/pkg/node/lowki_worker.js"));
const { createPublicClient, http } = await import(pathToFileURL(path.join(REPO, "contracts/e2e/node_modules/viem/_esm/index.js")));
const { sepolia } = await import(pathToFileURL(path.join(REPO, "contracts/e2e/node_modules/viem/_esm/chains/index.js")));

const LABEL = process.argv[2];
if (!/^test-[a-z0-9-]{1,27}$/.test(LABEL || "")) throw new Error("give a test-… label");
const SERVER = process.env.LOWKI_SERVER || "http://localhost:8830";
const ORIGIN = process.env.WALLET_ORIGIN || "http://localhost:8831";
const LOG = path.join(REPO, "target/wallet-home/server.log");
const RPC = process.env.SEPOLIA_RPC_URL || "https://ethereum-sepolia-rpc.publicnode.com";

let cookie = "";
let csrf = "";
async function api(method, p, body) {
  const r = await fetch(SERVER + p, {
    method,
    headers: { origin: ORIGIN, "content-type": "application/json", cookie, ...(csrf ? { "x-lowki-csrf": csrf } : {}) },
    body: body ? JSON.stringify(body) : undefined,
  });
  for (const c of r.headers.getSetCookie?.() || []) {
    const kv = c.split(";")[0];
    const k = kv.split("=")[0];
    cookie = [...cookie.split("; ").filter((x) => x && !x.startsWith(k + "=")), kv].join("; ");
  }
  const j = await r.json().catch(() => ({}));
  if (j.csrf_token) csrf = j.csrf_token;
  if (!r.ok) throw new Error(`${method} ${p} -> ${r.status} ${JSON.stringify(j)}`);
  return j;
}

const email = `claim-${Date.now()}@lowki.test`;
const since = fs.statSync(LOG).size;
await api("POST", "/v1/email/start", { email });
await new Promise((r) => setTimeout(r, 500));
const code = fs.readFileSync(LOG, "utf8").slice(since).match(/code for [^:]+: (\d{6})/)[1];
await api("POST", "/v1/email/verify", { email, code });

const w = new LowkiWorker(15);
const call = async (req) => { const r = await w.handle(req); if (r.type === "error") throw new Error(JSON.stringify(r)); return r; };
await call({ type: "createWallet", prfOutput: new Uint8Array(randomBytes(32)) });
const id = await call({ type: "ensIdentity" });
console.log("owner (m/137'/0'/0')", id.address);

const p = await api("POST", "/v1/names/prepare", { label: LABEL, owner: id.address });
console.log("prepare:", p.name, "records resolver", p.records_resolver);
const reg = await call({ type: "ensSignRegistration", chainId: String(p.chain_id), registrar: p.registrar, label: LABEL,
  duration: p.duration, nonce: String(p.registration_nonce), deadline: p.deadline });
let t = Date.now();
const r1 = await api("POST", "/v1/names/register", { label: LABEL, owner: id.address, nonce: p.registration_nonce, deadline: p.deadline, signature: reg.signature });
console.log(`register tx ${r1.register_tx} (${Date.now() - t} ms)`);
const set = await call({ type: "ensSignOwnerSetup", chainId: String(p.chain_id), name: p.name, ownerAccount: p.owner_account,
  authNonce: String(p.auth_nonce), execNonce: String(p.exec_nonce), deadline: p.deadline, factory: p.factory,
  permissionedResolverImpl: p.permissioned_resolver_impl, stealthResolver: p.stealth_resolver, registry: p.registry,
  gatewayUrl: p.gateway_url, recordsResolver: p.records_resolver });
t = Date.now();
const r2 = await api("POST", "/v1/names/setup", { record: set.record, exec_nonce: p.exec_nonce, deadline: p.deadline,
  execute_signature: set.executeSignature, authorization: set.authorization });
console.log(`setup tx ${r2.setup_tx} (${Date.now() - t} ms) status ${r2.status}`);

const client = createPublicClient({ chain: sepolia, transport: http(RPC) });
const seen = [];
for (let i = 0; i < 3; i++) {
  const a = await client.getEnsAddress({ name: p.name });
  console.log(`getEnsAddress(${p.name}) #${i + 1}: ${a}`);
  seen.push(a);
}
if (new Set(seen).size !== 3 || seen.some((a) => !a)) throw new Error("expected 3 fresh addresses");
const ts = Math.floor(Date.now() / 1000);
const auth = await call({ type: "ensRecordsAuth", name: p.name, timestamp: ts });
const rec = await (await fetch(`${SERVER}/v1/ens/records?name=${p.name}`, { headers: { "x-lowki-auth-timestamp": String(ts), "x-lowki-auth-signature": auth.signature } })).json();
const scan = await call({ type: "stealthScan", records: rec.records.map((x) => ({ stealthAddress: x.stealth_address, ephemeralPubkey: x.ephemeral_pubkey, viewTag: x.view_tag })) });
console.log(`records ${rec.records.length}, found by the worker's viewing key: ${scan.found.length}`);
if (!seen.every((a) => scan.found.some((f) => f.stealthAddress.toLowerCase() === a.toLowerCase()))) throw new Error("scan missed a lookup");
console.log("LIVE CLAIM OK", JSON.stringify({ name: p.name, owner: id.address, register_tx: r1.register_tx, setup_tx: r2.setup_tx, lookups: seen }));
