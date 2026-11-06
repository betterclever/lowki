// Differential vector generator for crates/lowki-ens.
//
// Reference implementations (exact versions pinned in package.json / package-lock.json):
//   - viem 2.57.16: ENS name utils (namehash, labelhash, packetToBytes, normalize via ox ->
//     @adraffy/ens-normalize), ABI encoding/decoding, CCIP-Read (ccipRequest,
//     localBatchGatewayRequest, offchainLookup through `call`), EIP-712, EIP-191, signatures
//   - @adraffy/ens-normalize 1.11.1 (the version viem/ox use) + its official validation
//     tests (validate/tests.json at tag v1.11.1, fetched and sha256-pinned below)
//   - @scure/bip32 2.4.0 (identity key derivation)
//   - @noble/curves 2.4.0 (secp256k1 point arithmetic for ENSIP-30 scheme-1 verification data)
// ABIs are written from the Solidity sources (namechain v2-mainnet-deployment-1, ens-contracts,
// stealth-ens-resolver); role constants are evaluated from the Solidity files themselves.
//
// Run: npm install && npx tsx gen.ts   (writes vectors.json)

import { createHash } from 'node:crypto';
import { existsSync, mkdirSync, readFileSync, writeFileSync } from 'node:fs';
import { ens_normalize } from '@adraffy/ens-normalize';
import { HDKey } from '@scure/bip32';
import { secp256k1 } from '@noble/curves/secp256k1.js';
import {
  bytesToHex,
  concat,
  createPublicClient,
  custom,
  decodeAbiParameters,
  encodeAbiParameters,
  encodeErrorResult,
  encodeFunctionData,
  encodeFunctionResult,
  getAddress,
  hashMessage,
  hashTypedData,
  hexToBytes,
  keccak256,
  parseAbi,
  parseAbiParameters,
  recoverMessageAddress,
  stringToBytes,
  toHex,
  type Hex,
} from 'viem';
import { labelhash, namehash, normalize, packetToBytes } from 'viem/ens';
import { ccipRequest } from 'viem';
import { privateKeyToAccount, privateKeyToAddress, sign, signAuthorization } from 'viem/accounts';
import { hashAuthorization } from 'viem/utils';
import { localBatchGatewayRequest } from './node_modules/viem/_esm/utils/ens/localBatchGatewayRequest.js';

const VENDOR = '/Users/betterclever/projects/ethglobal/vendor/stealth-ens-resolver';
const NAMECHAIN = `${VENDOR}/lib/namechain/contracts/src`;

// ---------- deterministic RNG (sha256 counter mode, fixed seed) ----------
const SEED = 'lowki-ens-vectors-v1';
let ctr = 0;
function randBytes(n: number): Uint8Array {
  const out = new Uint8Array(n);
  let off = 0;
  while (off < n) {
    const h = createHash('sha256').update(`${SEED}:${ctr++}`).digest();
    const take = Math.min(32, n - off);
    out.set(h.subarray(0, take), off);
    off += take;
  }
  return out;
}
const h = (b: Uint8Array) => bytesToHex(b);
const N = secp256k1.Point.Fn.ORDER;
const Pt = secp256k1.Point;
function randInt(max: number): number {
  return Number(BigInt(bytesToHex(randBytes(6))) % BigInt(max));
}
function randPriv(): Hex {
  const x = (BigInt(bytesToHex(randBytes(40))) % (N - 1n)) + 1n;
  return toHex(x, { size: 32 });
}
const randAddr = (): Hex => getAddress(h(randBytes(20)));
const randB32 = (): Hex => h(randBytes(32));
const randHex = (max: number): Hex => h(randBytes(randInt(max + 1)));
const randU64 = (): bigint => BigInt(h(randBytes(8)));
const randU256 = (): bigint => BigInt(h(randBytes(32)));
const ASCII = 'abcdefghijklmnopqrstuvwxyz0123456789-';
function randAscii(min: number, max: number, alphabet = ASCII): string {
  const n = min + randInt(max - min + 1);
  let s = '';
  for (let i = 0; i < n; i++) s += alphabet[randInt(alphabet.length)];
  return s;
}
const UNICODE_POOL = ['é', 'ß', 'ø', 'ñ', '中', '文', 'ж', 'λ', '😀', '🦊', '👨‍👩‍👧', '🏳️‍🌈', 'ア', '한', 'ü', '٣', '₿', 'ő', 'ł', 'ç'];
function randUnicode(): string {
  const n = 1 + randInt(6);
  let s = '';
  for (let i = 0; i < n; i++) s += randInt(3) === 0 ? ASCII[randInt(36)] : UNICODE_POOL[randInt(UNICODE_POOL.length)];
  return s;
}
function randString(): string {
  return randInt(2) === 0 ? randAscii(0, 40, ASCII + ' _./:ABCXYZ') : randUnicode();
}
const big = (x: bigint) => x.toString();

// ---------- 0. official ENSIP-15 validation tests ----------
const TESTS_URL = 'https://raw.githubusercontent.com/adraffy/ens-normalize.js/v1.11.1/validate/tests.json';
const TESTS_SHA256 = '7b20fb69f190c07f8b59ff30aea4efcdc0fa592667ff1f7a2a1f7122743d089d';
const CACHE = '.cache/ensip15-tests-v1.11.1.json';
async function loadTests(): Promise<string> {
  if (!existsSync(CACHE)) {
    const r = await fetch(TESTS_URL);
    if (!r.ok) throw new Error(`fetch ${TESTS_URL}: ${r.status}`);
    mkdirSync('.cache', { recursive: true });
    writeFileSync(CACHE, Buffer.from(await r.arrayBuffer()));
  }
  const raw = readFileSync(CACHE);
  const sha = createHash('sha256').update(raw).digest('hex');
  if (sha !== TESTS_SHA256) throw new Error(`tests.json sha256 mismatch: ${sha}`);
  return raw.toString('utf8');
}
const realFetch = globalThis.fetch;
const tests: any[] = JSON.parse(await loadTests());
const normalizeCases: any[] = [];
const validNorms: string[] = [];
for (const t of tests.slice(1)) {
  const name: string = t.name;
  const expectErr = !!t.error;
  const expectNorm: string = t.norm ?? name;
  let got: string | null = null;
  try {
    got = ens_normalize(name);
  } catch {}
  let viemGot: string | null = null;
  try {
    viemGot = normalize(name);
  } catch {}
  if (expectErr ? got !== null : got !== expectNorm) throw new Error(`adraffy disagrees with tests.json on ${JSON.stringify(name)}`);
  if (viemGot !== got) throw new Error(`viem disagrees with adraffy on ${JSON.stringify(name)}`);
  normalizeCases.push(expectErr ? { name, error: true } : { name, norm: expectNorm });
  if (!expectErr && expectNorm.length > 0 && !expectNorm.includes('.') && !expectNorm.startsWith('[')) validNorms.push(expectNorm);
}

// ---------- 1. names: labelhash / namehash / packetToBytes ----------
const names: string[] = [
  '', 'eth', 'vitalik.eth', 'a.b.c.d.e', 'lowki.eth', 'alice.lowki.eth', 'xn--ls8h.eth',
  '.', '..', '...', '.eth', 'eth.', '.eth.', 'a..b', 'a...b', '..a', 'a..', 'a.\n.b', 'a\n.b', '.\nx', 'x\r.', 'a. .b',
  'é.eth', '中文.eth', '😀.eth', '👨‍👩‍👧.eth', '🏳️‍🌈.eth', 'ß.ø.ñ',
  'a'.repeat(63) + '.eth', 'a'.repeat(255) + '.eth', 'a'.repeat(256) + '.eth', 'b'.repeat(300), '😀'.repeat(64) + '.eth', '😀'.repeat(63) + '.eth',
  `[${'ab'.repeat(32)}].eth`, `[${'AB'.repeat(32)}].eth`, `[${'ab'.repeat(31)}].eth`, `[${'zz'.repeat(32)}].eth`, `x[${'ab'.repeat(32)}]`,
  `[${'cd'.repeat(32)}]`, 'with space.eth', 'UPPER.eth',
];
for (let i = 0; i < 80; i++) {
  const n = 1 + randInt(4);
  const labels: string[] = [];
  for (let j = 0; j < n; j++) labels.push(randInt(3) === 0 ? randAscii(1, 20) : validNorms[randInt(validNorms.length)]);
  names.push(labels.join('.') + (randInt(2) ? '.eth' : ''));
}
for (let i = 0; i < 16; i++) names.push(randAscii(250, 320) + '.' + randAscii(1, 10));
const nameCases = names.map((name) => {
  const pkt = packetToBytes(name);
  const labels = name.split('.');
  const value = name.replace(/^\.|\.$/gm, '');
  const labelsStripped = value.length === 0 ? [] : value.split('.');
  const hasEmpty = labelsStripped.some((l) => l.length === 0);
  const hasLong = labelsStripped.some((l) => stringToBytes(l).length > 255);
  const hasEncoded = labels.some((l) => /^\[[0-9a-fA-F]{64}\]$/.test(l));
  const c: any = {
    name,
    namehash: namehash(name),
    packet: h(pkt),
    dns_encode_ok: !hasEmpty,
    labelhashes: labels.map((l) => labelhash(l)),
  };
  // NameCoder-side checks are comparable with viem when no label is hashed/encoded and none
  // is empty, and the stripping did nothing.
  if (!hasEmpty && !hasLong && !hasEncoded && value === name) {
    c.namehash_dns = namehash(name);
    c.dns_decode = name;
  }
  return c;
});

// ---------- 2. ABIs (from the Solidity sources) ----------
const resolverAbi = parseAbi([
  'function addr(bytes32 node) view returns (address)',
  'function addr(bytes32 node, uint256 coinType) view returns (bytes)',
  'function text(bytes32 node, string key) view returns (string)',
  'function data(bytes32 node, string key) view returns (bytes)',
  'function contenthash(bytes32 node) view returns (bytes)',
  'function multicall(bytes[] calls) returns (bytes[] results)',
]);
const urAbi = parseAbi([
  'function resolve(bytes name, bytes data) view returns (bytes result, address resolver)',
  'function resolveWithGateways(bytes name, bytes data, string[] gateways) view returns (bytes result, address resolver)',
  'error ResolverNotFound(bytes name)',
  'error ResolverNotContract(bytes name, address resolver)',
  'error UnsupportedResolverProfile(bytes4 selector)',
  'error ResolverError(bytes errorData)',
  'error ReverseAddressMismatch(string primary, bytes primaryAddress)',
  'error HttpError(uint16 status, string message)',
  'error Error(string message)',
  'error Panic(uint256 code)',
]);
const prAbi = parseAbi([
  'function setText(bytes name, string key, string value)',
  'function setData(bytes name, string key, bytes value)',
  'function setAddress(bytes name, uint256 coinType, bytes addressBytes)',
  'function setContenthash(bytes name, bytes hash)',
  'function multicall(bytes[] calls) returns (bytes[] results)',
  'struct Grant { address account; uint256 roleBitmap; }',
  'function initialize(Grant[] grants, bytes[] calls)',
]);
const factoryAbi = parseAbi([
  'function deployProxy(address implementation, uint256 salt, bytes data) returns (address)',
  'function predictProxyAddress(address deployer, uint256 salt) view returns (address proxy)',
]);
const registryAbi = parseAbi([
  'function register(string label, address owner, address registry, address resolver, uint256 roleBitmap, uint64 expiry) returns (uint256 tokenId)',
  'function setResolver(uint256 anyId, address resolver)',
  'function setSubregistry(uint256 anyId, address registry)',
  'struct State { uint8 status; uint64 expiry; address latestOwner; uint256 tokenId; uint256 resource; }',
  'function getState(uint256 anyId) view returns (State state)',
  'function grantRootRoles(uint256 roleBitmap, address account) returns (bool)',
  'struct Grant { address account; uint256 roleBitmap; }',
  'function initialize(Grant[] grants)',
]);
const ethRegistrarAbi = parseAbi([
  'function commit(bytes32 commitment)',
  'function register(string label, address owner, bytes32 secret, address subregistry, address resolver, uint64 duration, address paymentToken, bytes32 referrer) returns (uint256 tokenId)',
  'function isAvailable(string label) view returns (bool)',
  'function getRegisterPrice(string label, uint64 duration, address paymentToken) view returns (uint256 base, uint256 premium)',
  'function makeCommitment(string label, address owner, bytes32 secret, address subregistry, address resolver, uint64 duration, bytes32 referrer) pure returns (bytes32)',
  'function commitmentAt(bytes32 commitment) view returns (uint64)',
  'function approve(address spender, uint256 value) returns (bool)',
  'function mint(address to, uint256 amount)',
]);
const lowkiAbi = parseAbi([
  'function register(string label, address owner, address resolver, uint64 duration, uint256 deadline, bytes signature)',
  'function nonces(address owner) view returns (uint256)',
  'function available(string label) view returns (bool)',
  'function renew(string label, uint64 duration)',
]);
const stealthAbi = parseAbi([
  'function setConfig(bytes name, address recordsResolver, string[] urls)',
  'function setChains(bytes name, uint256[] coinTypes, bool enabled)',
  'function setMetaAddress(bytes name, bytes value)',
  'function clearRecords(bytes name)',
  'function canSetMetaAddress(bytes name, address account) view returns (bool)',
  'function canSetConfig(bytes name, address account) view returns (bool)',
]);
const batchAbi = parseAbi([
  'struct Request { address sender; string[] urls; bytes data; }',
  'function query(Request[] requests) view returns (bool[] failures, bytes[] responses)',
]);
const offchainLookupAbi = parseAbi(['error OffchainLookup(address sender, string[] urls, bytes callData, bytes4 callbackFunction, bytes extraData)']);

// ---------- 3. role constants, evaluated from the Solidity libraries ----------
function solConstants(path: string): Record<string, string> {
  const src = readFileSync(path, 'utf8');
  const env: Record<string, bigint> = {};
  const re = /uint256\s+internal\s+constant\s+(\w+)\s*=\s*([^;]+);/g;
  for (const m of src.matchAll(re)) {
    const expr = m[2]
      .replace(/\b0x[0-9a-fA-F]+\b|\b\d+\b/g, (lit) => `${lit}n`)
      .replace(/\b([A-Z_][A-Z0-9_]*)\b/g, (id) => `env.${id}`);
    env[m[1]] = new Function('env', `return (${expr});`)(env);
  }
  return Object.fromEntries(Object.entries(env).map(([k, v]) => [k, toHex(v, { size: 32 })]));
}
const roles = {
  registry: solConstants(`${NAMECHAIN}/registry/libraries/RegistryRolesLib.sol`),
  resolver: solConstants(`${NAMECHAIN}/resolver/libraries/PermissionedResolverLib.sol`),
  eac: solConstants(`${NAMECHAIN}/access-control/libraries/EACBaseRolesLib.sol`),
};

// ---------- 4. calldata builders vs viem encodeFunctionData ----------
const calldata: any[] = [];
const push = (fn: string, args: any, data: Hex) => calldata.push({ fn, args, data });
const randName = () => {
  const n = 1 + randInt(3);
  const ls: string[] = [];
  for (let i = 0; i < n; i++) ls.push(randInt(2) ? randAscii(1, 12) : validNorms[randInt(validNorms.length)]);
  return ls.join('.') + '.eth';
};
for (let i = 0; i < 24; i++) {
  const node = randB32();
  const key = randString();
  const coin = randInt(4) === 0 ? 60n : randInt(2) ? (0x80000000n | BigInt(randInt(1 << 30))) : randU64();
  push('addr', { node }, encodeFunctionData({ abi: resolverAbi, functionName: 'addr', args: [node] }));
  push('addr_coin', { node, coin_type: big(coin) }, encodeFunctionData({ abi: resolverAbi, functionName: 'addr', args: [node, coin] }));
  push('text', { node, key }, encodeFunctionData({ abi: resolverAbi, functionName: 'text', args: [node, key] }));
  push('data', { node, key }, encodeFunctionData({ abi: resolverAbi, functionName: 'data', args: [node, key] }));
  push('contenthash', { node }, encodeFunctionData({ abi: resolverAbi, functionName: 'contenthash', args: [node] }));
  const calls = Array.from({ length: randInt(5) }, () => randHex(100));
  push('multicall', { calls }, encodeFunctionData({ abi: resolverAbi, functionName: 'multicall', args: [calls] }));

  const name = randName();
  const dns = h(packetToBytes(name));
  const inner = randInt(2) ? encodeFunctionData({ abi: resolverAbi, functionName: 'addr', args: [namehash(name)] }) : randHex(200);
  const gateways = Array.from({ length: randInt(4) }, () => (randInt(3) === 0 ? 'x-batch-gateway:true' : `https://${randAscii(3, 10)}.example/${randString()}`));
  push('ur_resolve', { dns, data: inner }, encodeFunctionData({ abi: urAbi, functionName: 'resolve', args: [dns, inner] }));
  push('ur_resolve_with_gateways', { dns, data: inner, gateways }, encodeFunctionData({ abi: urAbi, functionName: 'resolveWithGateways', args: [dns, inner, gateways] }));

  const value = randString();
  const bval = randHex(120);
  push('pr_set_text', { dns, key, value }, encodeFunctionData({ abi: prAbi, functionName: 'setText', args: [dns, key, value] }));
  push('pr_set_data', { dns, key, value: bval }, encodeFunctionData({ abi: prAbi, functionName: 'setData', args: [dns, key, bval] }));
  push('pr_set_address', { dns, coin_type: big(coin), value: bval }, encodeFunctionData({ abi: prAbi, functionName: 'setAddress', args: [dns, coin, bval] }));
  push('pr_set_contenthash', { dns, value: bval }, encodeFunctionData({ abi: prAbi, functionName: 'setContenthash', args: [dns, bval] }));
  const grants = Array.from({ length: randInt(4) }, () => ({ account: randAddr(), roleBitmap: randU256() }));
  const gj = grants.map((g) => ({ account: g.account, role_bitmap: toHex(g.roleBitmap, { size: 32 }) }));
  push('pr_initialize', { grants: gj, calls }, encodeFunctionData({ abi: prAbi, functionName: 'initialize', args: [grants, calls] }));
  const impl = randAddr();
  const salt = randU256();
  push('factory_deploy_proxy', { implementation: impl, salt: toHex(salt, { size: 32 }), data: bval }, encodeFunctionData({ abi: factoryAbi, functionName: 'deployProxy', args: [impl, salt, bval] }));
  push('factory_predict', { deployer: impl, salt: toHex(salt, { size: 32 }) }, encodeFunctionData({ abi: factoryAbi, functionName: 'predictProxyAddress', args: [impl, salt] }));

  const label = randInt(2) ? randAscii(1, 30) : validNorms[randInt(validNorms.length)];
  const [owner, sub, res] = [randAddr(), randAddr(), randAddr()];
  const bitmap = randU256();
  const expiry = randU64();
  const anyId = randU256();
  push('reg_register', { label, owner, subregistry: sub, resolver: res, role_bitmap: toHex(bitmap, { size: 32 }), expiry: big(expiry) },
    encodeFunctionData({ abi: registryAbi, functionName: 'register', args: [label, owner, sub, res, bitmap, expiry] }));
  push('reg_set_resolver', { any_id: toHex(anyId, { size: 32 }), address: res }, encodeFunctionData({ abi: registryAbi, functionName: 'setResolver', args: [anyId, res] }));
  push('reg_set_subregistry', { any_id: toHex(anyId, { size: 32 }), address: sub }, encodeFunctionData({ abi: registryAbi, functionName: 'setSubregistry', args: [anyId, sub] }));
  push('reg_get_state', { any_id: toHex(anyId, { size: 32 }) }, encodeFunctionData({ abi: registryAbi, functionName: 'getState', args: [anyId] }));
  push('reg_grant_root_roles', { role_bitmap: toHex(bitmap, { size: 32 }), account: owner }, encodeFunctionData({ abi: registryAbi, functionName: 'grantRootRoles', args: [bitmap, owner] }));
  push('reg_initialize', { grants: gj }, encodeFunctionData({ abi: registryAbi, functionName: 'initialize', args: [grants] }));

  const secret = randB32();
  const referrer = randInt(3) ? '0x' + '00'.repeat(32) as Hex : randB32();
  const duration = randU64();
  const token = randAddr();
  const commitment = keccak256(encodeAbiParameters(parseAbiParameters('string, address, bytes32, address, address, uint64, bytes32'), [label, owner, secret, sub, res, duration, referrer]));
  push('eth_commit', { commitment }, encodeFunctionData({ abi: ethRegistrarAbi, functionName: 'commit', args: [commitment] }));
  push('eth_register', { label, owner, secret, subregistry: sub, resolver: res, duration: big(duration), payment_token: token, referrer },
    encodeFunctionData({ abi: ethRegistrarAbi, functionName: 'register', args: [label, owner, secret, sub, res, duration, token, referrer] }));
  push('eth_make_commitment', { label, owner, secret, subregistry: sub, resolver: res, duration: big(duration), referrer, commitment },
    encodeFunctionData({ abi: ethRegistrarAbi, functionName: 'makeCommitment', args: [label, owner, secret, sub, res, duration, referrer] }));
  push('eth_is_available', { label }, encodeFunctionData({ abi: ethRegistrarAbi, functionName: 'isAvailable', args: [label] }));
  push('eth_get_register_price', { label, duration: big(duration), payment_token: token }, encodeFunctionData({ abi: ethRegistrarAbi, functionName: 'getRegisterPrice', args: [label, duration, token] }));
  push('eth_commitment_at', { commitment }, encodeFunctionData({ abi: ethRegistrarAbi, functionName: 'commitmentAt', args: [commitment] }));
  push('erc20_approve', { spender: res, value: toHex(bitmap, { size: 32 }) }, encodeFunctionData({ abi: ethRegistrarAbi, functionName: 'approve', args: [res, bitmap] }));
  push('mock_mint', { to: owner, value: toHex(bitmap, { size: 32 }) }, encodeFunctionData({ abi: ethRegistrarAbi, functionName: 'mint', args: [owner, bitmap] }));

  const deadline = randU64();
  const sig = randHex(70);
  push('lowki_register', { label, owner, resolver: res, duration: big(duration), deadline: big(deadline), signature: sig },
    encodeFunctionData({ abi: lowkiAbi, functionName: 'register', args: [label, owner, res, duration, deadline, sig] }));
  push('lowki_nonces', { owner }, encodeFunctionData({ abi: lowkiAbi, functionName: 'nonces', args: [owner] }));
  push('lowki_available', { label }, encodeFunctionData({ abi: lowkiAbi, functionName: 'available', args: [label] }));
  push('lowki_renew', { label, duration: big(duration) }, encodeFunctionData({ abi: lowkiAbi, functionName: 'renew', args: [label, duration] }));

  const urls = gateways;
  const coins = Array.from({ length: randInt(5) }, () => (randInt(2) ? randU64() : (0x80000000n | BigInt(randInt(1 << 30)))));
  const enabled = randInt(2) === 1;
  push('st_set_config', { dns, address: res, urls }, encodeFunctionData({ abi: stealthAbi, functionName: 'setConfig', args: [dns, res, urls] }));
  push('st_set_chains', { dns, coin_types: coins.map(big), enabled }, encodeFunctionData({ abi: stealthAbi, functionName: 'setChains', args: [dns, coins, enabled] }));
  push('st_set_meta_address', { dns, value: bval }, encodeFunctionData({ abi: stealthAbi, functionName: 'setMetaAddress', args: [dns, bval] }));
  push('st_clear_records', { dns }, encodeFunctionData({ abi: stealthAbi, functionName: 'clearRecords', args: [dns] }));
  push('st_can_set_meta_address', { dns, address: owner }, encodeFunctionData({ abi: stealthAbi, functionName: 'canSetMetaAddress', args: [dns, owner] }));
  push('st_can_set_config', { dns, address: owner }, encodeFunctionData({ abi: stealthAbi, functionName: 'canSetConfig', args: [dns, owner] }));
}

// ---------- 5. decoders vs viem encodeFunctionResult / encodeErrorResult ----------
const decoders: any[] = [];
for (let i = 0; i < 24; i++) {
  const a = randAddr();
  const b = randHex(150);
  const s = randString();
  decoders.push({ fn: 'addr', data: encodeFunctionResult({ abi: parseAbi(['function addr(bytes32 node) view returns (address)']), functionName: 'addr', result: a }), address: a });
  decoders.push({ fn: 'addr_coin', data: encodeFunctionResult({ abi: parseAbi(['function addr(bytes32 node, uint256 coinType) view returns (bytes)']), functionName: 'addr', result: b }), bytes: b });
  decoders.push({ fn: 'text', data: encodeFunctionResult({ abi: resolverAbi, functionName: 'text', result: s }), string: s });
  decoders.push({ fn: 'data', data: encodeFunctionResult({ abi: resolverAbi, functionName: 'data', result: b }), bytes: b });
  decoders.push({ fn: 'contenthash', data: encodeFunctionResult({ abi: resolverAbi, functionName: 'contenthash', result: b }), bytes: b });
  const arr = Array.from({ length: randInt(5) }, () => randHex(80));
  decoders.push({ fn: 'multicall', data: encodeFunctionResult({ abi: resolverAbi, functionName: 'multicall', result: arr }), list: arr });
  decoders.push({ fn: 'ur_resolve', data: encodeFunctionResult({ abi: urAbi, functionName: 'resolve', result: [b, a] }), bytes: b, address: a });
  const st = { status: randInt(3), expiry: randU64(), latestOwner: a, tokenId: randU256(), resource: randU256() };
  decoders.push({ fn: 'get_state', data: encodeFunctionResult({ abi: registryAbi, functionName: 'getState', result: st }),
    status: st.status, expiry: big(st.expiry), latest_owner: a, token_id: toHex(st.tokenId, { size: 32 }), resource: toHex(st.resource, { size: 32 }) });
  const [base, premium] = [randU256(), randInt(2) ? 0n : randU256()];
  decoders.push({ fn: 'register_price', data: encodeFunctionResult({ abi: ethRegistrarAbi, functionName: 'getRegisterPrice', result: [base, premium] }), base: toHex(base, { size: 32 }), premium: toHex(premium, { size: 32 }) });
  const bool = randInt(2) === 1;
  decoders.push({ fn: 'bool', data: encodeFunctionResult({ abi: ethRegistrarAbi, functionName: 'isAvailable', result: bool }), bool });
  const nonce = randU64();
  decoders.push({ fn: 'nonce', data: encodeFunctionResult({ abi: lowkiAbi, functionName: 'nonces', result: nonce }), nonce: big(nonce) });
  decoders.push({ fn: 'commitment_at', data: encodeFunctionResult({ abi: ethRegistrarAbi, functionName: 'commitmentAt', result: nonce }), nonce: big(nonce) });
  decoders.push({ fn: 'address_result', data: encodeFunctionResult({ abi: factoryAbi, functionName: 'predictProxyAddress', result: a }), address: a });
  // UR errors
  const dns = h(packetToBytes(randName()));
  const sel = h(randBytes(4));
  const status = randInt(65536);
  const errs: [string, any[], any][] = [
    ['ResolverNotFound', [dns], { kind: 'ResolverNotFound', name: dns }],
    ['ResolverNotContract', [dns, a], { kind: 'ResolverNotContract', name: dns, address: a }],
    ['UnsupportedResolverProfile', [sel], { kind: 'UnsupportedResolverProfile', selector: sel }],
    ['ResolverError', [b], { kind: 'ResolverError', bytes: b }],
    ['ReverseAddressMismatch', [s, b], { kind: 'ReverseAddressMismatch', string: s, bytes: b }],
    ['HttpError', [status, s], { kind: 'HttpError', status, string: s }],
    ['Error', [s], { kind: 'Error', string: s }],
    ['Panic', [randU256()], null],
  ];
  for (const [errorName, args, expect] of errs) {
    const data = encodeErrorResult({ abi: urAbi, errorName: errorName as any, args: args as any });
    decoders.push({ fn: 'ur_error', data, expect: expect ?? { kind: 'Panic', code: toHex(args[0], { size: 32 }) } });
  }
  decoders.push({ fn: 'ur_error', data: randHex(80), expect: { kind: 'Unknown' } });
}

// generic ABI round-trips (decoder and encoder)
const abiRoundtrip: any[] = [];
const genericParams = parseAbiParameters('bytes, string, bytes[], string[], address[], uint256[], (address,uint256)[], bool, bytes32, uint64');
for (let i = 0; i < 64; i++) {
  const v = [
    randHex(100),
    randString(),
    Array.from({ length: randInt(4) }, () => randHex(70)),
    Array.from({ length: randInt(4) }, () => randString()),
    Array.from({ length: randInt(4) }, () => randAddr()),
    Array.from({ length: randInt(4) }, () => randU256()),
    Array.from({ length: randInt(4) }, () => ({ 0: randAddr(), 1: randU256() })).map((x) => [x[0], x[1]] as const),
    randInt(2) === 1,
    randB32(),
    randU64(),
  ] as const;
  const enc = encodeAbiParameters(genericParams, v as any);
  const dec = decodeAbiParameters(genericParams, enc);
  if (JSON.stringify(dec, (_, x) => (typeof x === 'bigint' ? x.toString() : x)) !== JSON.stringify(v, (_, x) => (typeof x === 'bigint' ? x.toString() : x))) throw new Error('viem roundtrip');
  abiRoundtrip.push({
    encoded: enc,
    bytes: v[0], string: v[1], bytes_array: v[2], string_array: v[3], address_array: v[4],
    u256_array: v[5].map((x) => toHex(x, { size: 32 })),
    grants: v[6].map(([a, r]) => ({ account: a, role_bitmap: toHex(r, { size: 32 }) })),
    bool: v[7], b32: v[8], u64: big(v[9]),
  });
}

// ---------- 6. OffchainLookup + callback ----------
const offchain: any[] = [];
for (let i = 0; i < 48; i++) {
  const sender = randAddr();
  const urls = Array.from({ length: randInt(4) }, () => (randInt(4) === 0 ? 'x-batch-gateway:true' : `https://gw${randInt(9)}.example/${randInt(2) ? '{sender}/{data}.json' : 'ccip'}`));
  const callData = randHex(300);
  const cb = h(randBytes(4));
  const extra = randHex(200);
  const revert = encodeErrorResult({ abi: offchainLookupAbi, errorName: 'OffchainLookup', args: [sender, urls, callData, cb, extra] });
  const response = randHex(200);
  const callback = concat([cb, encodeAbiParameters([{ type: 'bytes' }, { type: 'bytes' }], [response, extra])]);
  offchain.push({ revert, sender, urls, call_data: callData, callback_function: cb, extra_data: extra, response, callback });
}

// ---------- 7. ccipRequest: request construction and response parsing (fetch stubbed) ----------
let served: Record<string, any> = {};
type Captured = { url: string; method: string; body: string | null; content_type: string | null };
type Canned = { status: number; ct?: string | null; body: string } | 'network' | 'hang';
let captured: Captured[] = [];
let respond: (url: string) => Canned = () => ({ status: 200, ct: 'application/json', body: '{"data":"0x"}' });
globalThis.fetch = (async (input: any, init: any) => {
  const url = String(input);
  const hdrs = new Headers(init?.headers ?? {});
  captured.push({ url, method: init?.method ?? 'GET', body: init?.body ?? null, content_type: hdrs.get('Content-Type') });
  const r = respond(url);
  served[url] = r === 'network' ? { failure: 'Network' } : r === 'hang' ? { failure: 'Timeout' } : r;
  if (r === 'network') throw new TypeError('fetch failed');
  if (r === 'hang')
    return new Promise((_, reject) => {
      const sig: AbortSignal | undefined = init?.signal;
      sig?.addEventListener('abort', () => reject(new DOMException('This operation was aborted', 'AbortError')));
    });
  const headers: Record<string, string> = {};
  if (r.ct) headers['Content-Type'] = r.ct;
  return new Response(r.body, { status: r.status, headers });
}) as any;

function classify(err: any): any {
  if (err?.name === 'HttpRequestError') return err.status ? { kind: 'Http', status: err.status } : { kind: 'Request' };
  if (err?.name === 'OffchainLookupResponseMalformedError') return { kind: 'Malformed' };
  if (err?.name === 'TimeoutError') return { kind: 'Timeout' };
  if (err?.message === 'An unknown error occurred.') return { kind: 'NoUrls' };
  throw new Error(`unexpected error ${err?.name}: ${err?.message}`);
}

const ccipRequests: any[] = [];
const templates = [
  'https://gw.example/{sender}/{data}.json', 'https://gw.example/{data}', 'https://gw.example/lookup', 'https://gw.example/{sender}',
  'https://gw.example/{data}/{data}', 'https://gw.example/{sender}/{sender}/{data}', 'http://localhost:8080/ccip/{sender}?q={data}',
  'HTTPS://GW.EXAMPLE/x', ' https://gw.example/padded', 'ftp://gw.example/{data}', 'x-batch-gateway:true', 'not a url', 'https://gw.example/{DATA}',
];
for (let i = 0; i < 40; i++) {
  const url = templates[i % templates.length];
  const sender = randAddr();
  const data = randHex(120);
  captured = [];
  respond = () => ({ status: 200, ct: 'application/json', body: '{"data":"0x"}' });
  let error: any = null;
  try {
    await ccipRequest({ data, sender, urls: [url] });
  } catch (e) {
    error = classify(e);
  }
  const c = captured[0] ?? null;
  ccipRequests.push({ template: url, sender, data, request: c, error });
}

const responseCases: any[] = [];
const bodies: Canned[] = [
  { status: 200, ct: 'application/json', body: '{"data":"0x1234"}' },
  { status: 200, ct: 'application/json; charset=utf-8', body: '{"data":"0xABcd"}' },
  { status: 200, ct: 'application/json', body: '{"data":"0x"}' },
  { status: 200, ct: 'application/json', body: '{"data":"1234"}' },
  { status: 200, ct: 'application/json', body: '{"data":1234}' },
  { status: 200, ct: 'application/json', body: '{"result":"0x12"}' },
  { status: 200, ct: 'application/json', body: 'null' },
  { status: 200, ct: 'application/json', body: '"0x12"' },
  { status: 200, ct: 'application/json', body: '[1,2]' },
  { status: 200, ct: 'application/json', body: 'not json' },
  { status: 200, ct: 'application/json', body: ' {"data":"0xff"} ' },
  { status: 200, ct: 'text/plain', body: '0xdeadbeef' },
  { status: 200, ct: null, body: '0xdeadbeef' },
  { status: 200, ct: 'text/plain', body: '0xdeadbeef\n' },
  { status: 200, ct: 'text/plain', body: '{"data":"0x12"}' },
  { status: 200, ct: 'Application/JSON', body: '{"data":"0x12"}' },
  { status: 201, ct: 'application/json', body: '{"data":"0x12"}' },
  { status: 299, ct: 'application/json', body: '{"data":"0x12"}' },
  { status: 300, ct: 'application/json', body: '{"data":"0x12"}' },
  { status: 404, ct: 'application/json', body: '{"message":"not found"}' },
  { status: 404, ct: 'application/json', body: 'garbage' },
  { status: 500, ct: 'text/plain', body: 'boom' },
  { status: 503, ct: null, body: '' },
  { status: 418, ct: 'application/json', body: '{"data":"0x12","error":"teapot"}' },
  'network',
  'hang',
];
for (const r of bodies) {
  for (let k = 0; k < 2; k++) {
    respond = () => r;
    let result: any = null;
    let error: any = null;
    try {
      result = await ccipRequest({ data: randHex(40), sender: randAddr(), urls: ['https://gw.example/x'], timeout: 30 });
    } catch (e) {
      error = classify(e);
    }
    // the same failure as a local batch gateway response item
    let batch: any = null;
    respond = () => r;
    const q = encodeFunctionData({ abi: batchAbi, functionName: 'query', args: [[{ sender: randAddr(), urls: ['https://gw.example/x'], data: '0x01' }]] });
    batch = await localBatchGatewayRequest({ data: q, ccipRequest: (p: any) => ccipRequest({ ...p, timeout: 30 }) });
    responseCases.push({ response: r === 'network' || r === 'hang' ? { failure: r === 'network' ? 'Network' : 'Timeout' } : r, result, error, batch });
  }
}

// ---------- 8. local batch gateway (nested), vs viem localBatchGatewayRequest ----------
function cannedFor(url: string): Canned {
  // deterministic per URL: the URL encodes its behavior
  if (url.includes('/net/')) return 'network';
  if (url.includes('/s500/')) return { status: 500, ct: 'text/plain', body: 'err' };
  if (url.includes('/s404/')) return { status: 404, ct: 'application/json', body: '{"message":"nope"}' };
  if (url.includes('/bad/')) return { status: 200, ct: 'application/json', body: '{"data":"zz"}' };
  if (url.includes('/raw/')) return { status: 200, ct: 'text/plain', body: '0x' + createHash('sha256').update(url).digest('hex') };
  return { status: 200, ct: 'application/json', body: JSON.stringify({ data: '0x' + createHash('sha256').update(url).digest('hex') + createHash('sha256').update('b' + url).digest('hex').slice(0, randIntFromUrl(url)) }) };
}
function randIntFromUrl(url: string): number {
  return (createHash('sha256').update(url).digest()[0] % 32) * 2;
}
const KINDS = ['ok', 'ok', 'ok', 'raw', 'net', 's500', 's404', 'bad'];
function randGatewayUrl(): string {
  const kind = KINDS[randInt(KINDS.length)];
  const t = randInt(3);
  const id = randAscii(4, 8);
  if (t === 0) return `https://gw${randInt(5)}.example/${kind}/${id}/{sender}/{data}`;
  if (t === 1) return `https://gw${randInt(5)}.example/${kind}/${id}`;
  return randInt(6) === 0 ? `ftp://gw.example/${kind}/${id}` : `https://gw${randInt(5)}.example/${kind}/${id}/{data}`;
}
function randBatch(depth: number): Hex {
  const n = randInt(5);
  const qs = [];
  for (let i = 0; i < n; i++) {
    if (randInt(4) === 0) {
      const nested = depth < 5 ? randBatch(depth + 1) : '0x12345678';
      qs.push({ sender: randAddr(), urls: ['x-batch-gateway:true'], data: randInt(8) === 0 ? randHex(40) : nested });
    } else {
      qs.push({ sender: randAddr(), urls: Array.from({ length: randInt(4) }, randGatewayUrl), data: randHex(60) });
    }
  }
  return encodeFunctionData({ abi: batchAbi, functionName: 'query', args: [qs] });
}
const batchCases: any[] = [];
respond = cannedFor;
for (let i = 0; i < 40; i++) {
  const data = randBatch(0);
  captured = [];
  served = {};
  const result = await localBatchGatewayRequest({ data, ccipRequest: (p: any) => ccipRequest(p) });
  batchCases.push({ call_data: data, result, responses: served, requests: captured.map((c) => ({ url: c.url, method: c.method, body: c.body })).sort((a, b) => (a.url + a.body < b.url + b.body ? -1 : 1)) });
}

// special batches: nesting beyond the depth limit, unknown nested selectors, query budget
{
  const specials: Hex[] = [];
  let deep = encodeFunctionData({ abi: batchAbi, functionName: 'query', args: [[{ sender: randAddr(), urls: ['https://gw.example/ok/deep/{data}'], data: '0x01' }]] });
  for (let d = 0; d < 6; d++) {
    deep = encodeFunctionData({ abi: batchAbi, functionName: 'query', args: [[
      { sender: randAddr(), urls: ['x-batch-gateway:true'], data: deep },
      { sender: randAddr(), urls: [`https://gw.example/ok/lvl${d}/{data}`], data: randHex(8) },
    ]] });
    specials.push(deep);
  }
  specials.push(encodeFunctionData({ abi: batchAbi, functionName: 'query', args: [['0x', '0x12', '0x123456', '0xdeadbeef', '0xdeadbeef00'].map((d) => ({ sender: randAddr(), urls: ['https://x.example/{data}', 'x-batch-gateway:true'], data: d as Hex }))] }));
  specials.push(encodeFunctionData({ abi: batchAbi, functionName: 'query', args: [[]] }));
  specials.push(encodeFunctionData({ abi: batchAbi, functionName: 'query', args: [[{ sender: randAddr(), urls: [], data: '0x' }]] }));
  const many = Array.from({ length: 9 }, (_, q) => ({ sender: randAddr(), urls: Array.from({ length: 40 }, (_, i) => `https://gw${i % 3}.example/${['s500', 'bad', 'raw', 'ok'][(q + i) % 4]}/b${q}-${i}/{data}`), data: randHex(16) }));
  specials.push(encodeFunctionData({ abi: batchAbi, functionName: 'query', args: [many] }));
  for (const data of specials) {
    captured = [];
    served = {};
    const result = await localBatchGatewayRequest({ data, ccipRequest: (p: any) => ccipRequest(p) });
    batchCases.push({ call_data: data, result, responses: served, requests: captured.map((c) => ({ url: c.url, method: c.method, body: c.body })).sort((a, b) => (a.url + a.body < b.url + b.body ? -1 : 1)) });
  }
}

// ---------- 9. full CCIP-Read flows through viem `call` (eth_call mocked from a table) ----------
type Outcome = { ok: Hex } | { revert: Hex };
let missing = 0;
async function runFlow(to: Hex, data: Hex, calls: Record<string, Outcome>): Promise<any> {
  const client = createPublicClient({
    transport: custom({
      async request({ method, params }: any) {
        if (method === 'eth_call') {
          const p = params[0];
          if (getAddress(p.to) !== getAddress(to)) throw new Error('wrong to');
          const o = calls[p.data];
          if (!o) {
            missing++;
            throw new Error(`no mock for ${p.data.slice(0, 20)}`);
          }
          if ('ok' in o) return o.ok;
          const e: any = new Error('execution reverted');
          e.code = 3;
          e.data = o.revert;
          throw e;
        }
        if (method === 'eth_chainId') return '0xaa36a7';
        throw new Error(`unsupported ${method}`);
      },
    }, { retryCount: 0 }),
  });
  captured = [];
  try {
    const r = await client.call({ to, data });
    return { result: r.data ?? '0x', requests: captured.map((c) => ({ url: c.url, method: c.method, body: c.body })) };
  } catch {
    return { result: null, requests: captured.map((c) => ({ url: c.url, method: c.method, body: c.body })) };
  }
}
function responseFor(url: string, sender: Hex, callData: Hex): Hex | null {
  // what ccipRequest would return for this single URL (cannedFor semantics), or null on failure
  const req = url.replace('{sender}', sender.toLowerCase()).replace('{data}', callData);
  if (!/^https?:/i.test(req.trim())) return null;
  const c = cannedFor(req);
  if (c === 'network' || c === 'hang' || c.status < 200 || c.status > 299) return null;
  const v = c.ct?.startsWith('application/json') ? JSON.parse(c.body).data : c.body;
  return typeof v === 'string' && /^0x[0-9a-fA-F]*$/.test(v) ? (v as Hex) : null;
}
const flows: any[] = [];
for (let i = 0; i < 48; i++) {
  const to = randAddr();
  const data = randHex(68);
  const calls: Record<string, Outcome> = {};
  const lookups = 1 + randInt(5); // 5 => exceeds the depth limit
  let cur = data;
  let expectFail = false;
  const mode = randInt(10);
  for (let d = 0; d < lookups; d++) {
    const useBatch = randInt(4) === 0;
    const sender = mode === 0 && d === lookups - 1 ? randAddr() : to; // sender mismatch
    const callData = useBatch ? randBatch(1) : randHex(100);
    const urls = useBatch
      ? ['https://ignored.example/{data}', 'x-batch-gateway:true']
      : [...Array.from({ length: randInt(3) }, randGatewayUrl), ...(randInt(5) ? [`https://gw.example/ok/${randAscii(4, 8)}/{data}`] : [])];
    const cb = h(randBytes(4));
    const extra = randHex(64);
    calls[cur] = { revert: encodeErrorResult({ abi: offchainLookupAbi, errorName: 'OffchainLookup', args: [sender, urls, callData, cb, extra] }) };
    let resp: Hex | null;
    if (useBatch) {
      resp = await localBatchGatewayRequest({ data: callData, ccipRequest: (p: any) => ccipRequest(p) });
    } else {
      resp = null;
      for (const u of urls) {
        resp = responseFor(u, sender, callData);
        if (resp) break;
      }
    }
    if (!resp) {
      expectFail = true;
      break;
    }
    cur = concat([cb, encodeAbiParameters([{ type: 'bytes' }, { type: 'bytes' }], [resp, extra])]);
  }
  if (!expectFail) calls[cur] = mode === 1 ? { revert: encodeErrorResult({ abi: urAbi, errorName: 'Error', args: ['nope'] }) } : { ok: randHex(96) };
  served = {};
  const out = await runFlow(to, data, calls);
  const sorted = [...out.requests].sort((a, b) => (a.url + a.body < b.url + b.body ? -1 : 1));
  flows.push({ to, data, calls: Object.entries(calls).map(([k, v]) => ({ data: k, ...('ok' in v ? { ok: v.ok } : { revert: v.revert }) })), result: out.result, responses: served, requests_sorted: sorted });
}
// special flow: one direct lookup with 300 URLs; only the first 256 are tried
{
  const to = randAddr();
  const data = randHex(36);
  const urls = [...Array.from({ length: 300 }, (_, i) => `https://gw.example/s500/u${i}`), 'https://gw.example/ok/last/{data}'];
  const calls: Record<string, Outcome> = { [data]: { revert: encodeErrorResult({ abi: offchainLookupAbi, errorName: 'OffchainLookup', args: [to, urls, '0x01', '0x11223344', '0x'] }) } };
  served = {};
  const out = await runFlow(to, data, calls);
  if (out.result !== null || out.requests.length !== 256) throw new Error('budget flow');
  flows.push({ to, data, calls: Object.entries(calls).map(([k, v]) => ({ data: k, ...('ok' in v ? { ok: v.ok } : { revert: v.revert }) })), result: out.result, responses: served, requests_sorted: [...out.requests].sort((a, b) => (a.url + a.body < b.url + b.body ? -1 : 1)) });
}
globalThis.fetch = realFetch;
if (missing) throw new Error(`viem made ${missing} eth_calls the generator did not predict`);

// ---------- 10. EIP-712 LowkiRegistrar registrations ----------
const eip712: any[] = [];
const types = {
  Register: [
    { name: 'label', type: 'string' },
    { name: 'owner', type: 'address' },
    { name: 'resolver', type: 'address' },
    { name: 'duration', type: 'uint64' },
    { name: 'nonce', type: 'uint256' },
    { name: 'deadline', type: 'uint256' },
  ],
} as const;
for (let i = 0; i < 64; i++) {
  const pk = randPriv();
  const chainId = [1, 11155111, 84532, 10][randInt(4)] + (i > 40 ? randInt(1000000) : 0);
  const registrar = randAddr();
  const message = {
    label: randInt(4) ? randAscii(3, 32) : validNorms[randInt(validNorms.length)],
    owner: randAddr(),
    resolver: randAddr(),
    duration: randInt(3) ? BigInt(31536000 * (1 + randInt(5))) : randU64(),
    nonce: BigInt(randInt(5) ? randInt(10) : Number(randU64() >> 12n)),
    deadline: randU64(),
  };
  const domain = { name: 'Lowki Registrar', version: '1', chainId, verifyingContract: registrar } as const;
  const digest = hashTypedData({ domain, types, primaryType: 'Register', message });
  const signature = await privateKeyToAccount(pk).signTypedData({ domain, types, primaryType: 'Register', message });
  eip712.push({ private_key: pk, chain_id: chainId, registrar, label: message.label, owner: message.owner, resolver: message.resolver,
    duration: big(message.duration), nonce: big(message.nonce), deadline: big(message.deadline), digest, signature, signer: privateKeyToAddress(pk) });
}

// ---------- 11. personal_sign (records auth) ----------
const personal: any[] = [];
for (let i = 0; i < 48; i++) {
  const pk = randPriv();
  const name = i % 3 === 0 ? validNorms[randInt(validNorms.length)] + '.eth' : randName();
  const ts = i % 5 === 0 ? 0n : randU64() >> BigInt(randInt(40));
  const message = `Lowki stealth records\nname: ${name}\ntimestamp: ${ts}`;
  const signature = await privateKeyToAccount(pk).signMessage({ message });
  const recovered = await recoverMessageAddress({ message, signature });
  personal.push({ private_key: pk, name, timestamp: big(ts), message, hash: hashMessage(message), signature, address: recovered });
}
// raw digest signatures (identity sign_digest)
const digestSigs: any[] = [];
for (let i = 0; i < 32; i++) {
  const pk = randPriv();
  const hash = i < 4 ? toHex(N - 1n + BigInt(i), { size: 32 }) : randB32();
  const s = await sign({ hash, privateKey: pk, to: 'hex' });
  digestSigs.push({ private_key: pk, digest: hash, signature: s });
}

// ---------- 12. ENSIP-30 stealth records, keys, scheme-1 verification data ----------
const P_FIELD = secp256k1.Point.Fp.ORDER;
function isValidCompressed(b: Uint8Array): boolean {
  try {
    Pt.fromBytes(b);
    return b.length === 33;
  } catch {
    return false;
  }
}
const stealthRecords: any[] = [];
for (let i = 0; i < 64; i++) {
  const spend = Pt.BASE.multiply(BigInt(randPriv())).toBytes(true);
  const view = Pt.BASE.multiply(BigInt(randPriv())).toBytes(true);
  const flag = randInt(2);
  const rec = concat([toHex(flag, { size: 1 }), h(spend), h(view)]);
  stealthRecords.push({ value: rec, ok: true, client_side_allowed: flag === 1, spend: h(spend), view: h(view) });
}
const goodRec = hexToBytes(stealthRecords[0].value);
const malformed: Uint8Array[] = [
  new Uint8Array(0), goodRec.slice(0, 66), hexToBytes(concat([h(goodRec), '0x00'])), goodRec.slice(1),
];
for (const f of [2, 3, 0x80, 0xff, 0x41]) {
  const r = goodRec.slice();
  r[0] = f;
  malformed.push(r);
}
{
  const r = goodRec.slice();
  r[1] = 0x04;
  malformed.push(r);
  const r2 = goodRec.slice();
  r2[34] = 0x05;
  malformed.push(r2);
  // x not on the curve
  for (let x = 1n; malformed.length < 16; x++) {
    const cand = concat(['0x02', toHex(x, { size: 32 })]);
    if (!isValidCompressed(hexToBytes(cand))) {
      const r3 = goodRec.slice();
      r3.set(hexToBytes(cand), 1 + (malformed.length % 2) * 33);
      malformed.push(r3);
    }
  }
  const r4 = goodRec.slice();
  r4.set(hexToBytes(toHex(P_FIELD + 1n, { size: 32 })), 35);
  malformed.push(r4);
  const r5 = goodRec.slice();
  r5.set(new Uint8Array(32), 2);
  malformed.push(r5);
}
for (const m of malformed) {
  const ok = m.length === 67 && (m[0] & 0xfe) === 0 && isValidCompressed(m.slice(1, 34)) && isValidCompressed(m.slice(34, 67));
  if (ok) throw new Error('malformed case is valid');
  stealthRecords.push({ value: h(m), ok: false });
}
const stealthKeys: any[] = [];
for (const [s, c] of [[1n, 60n], [1n, 0x80000000n], [1n, 2147483658n], [0n, 0n], [2n, 18446744073709551615n], [1n, 2147568180n]] as const) {
  stealthKeys.push({ scheme: big(s), coin: big(c), data_key: `stealth-meta-address[${s}][${c}]`, text_key: `stealth-meta-address[${s}]` });
}

// scheme-1 verification data and the verifier's derivation (mod n), with noble
const scheme1: any[] = [];
for (let i = 0; i < 64; i++) {
  const spendPriv = BigInt(randPriv());
  const viewPriv = BigInt(randPriv());
  const e = i === 0 ? 1n : i === 1 ? N - 1n : BigInt(randPriv());
  const P_spend = Pt.BASE.multiply(spendPriv);
  const P_view = Pt.BASE.multiply(viewPriv);
  const S = P_view.multiply(e).toAffine();
  const vd = concat([toHex(e, { size: 32 }), toHex(S.x, { size: 32 }), toHex(S.y, { size: 32 })]);
  const sCompressed = concat([toHex(2 + Number(S.y & 1n), { size: 1 }), toHex(S.x, { size: 32 })]);
  const sh = BigInt(keccak256(sCompressed)) % N;
  const stealthPub = P_spend.add(Pt.BASE.multiply(sh)).toBytes(false);
  const stealth = getAddress('0x' + keccak256(stealthPub.slice(1)).slice(-40));
  const ephPub = h(Pt.BASE.multiply(e).toBytes(true));
  const viewTag = hexToBytes(keccak256(sCompressed))[0];
  const meta = concat([h(P_spend.toBytes(true)), h(P_view.toBytes(true))]);
  const gatewayResponse = encodeAbiParameters(parseAbiParameters('address, bytes'), [stealth, vd]);
  scheme1.push({ spend_priv: toHex(spendPriv, { size: 32 }), view_priv: toHex(viewPriv, { size: 32 }), ephemeral: toHex(e, { size: 32 }), meta, verification_data: vd, stealth_address: stealth, ephemeral_pubkey: ephPub, view_tag: viewTag, gateway_response: gatewayResponse });
}

// resolve() calldata as the StealthResolver sends it to the gateway
const resolveCalls: any[] = [];
for (let i = 0; i < 32; i++) {
  const name = randName();
  const dns = h(packetToBytes(name));
  const node = namehash(name);
  const k = randInt(4);
  const coin = randInt(2) ? (0x80000000n | BigInt(randInt(1 << 30))) : randU256();
  const inner =
    k === 0 ? encodeFunctionData({ abi: resolverAbi, functionName: 'addr', args: [node] })
    : k === 1 ? encodeFunctionData({ abi: resolverAbi, functionName: 'addr', args: [node, coin] })
    : k === 2 ? encodeFunctionData({ abi: resolverAbi, functionName: 'text', args: [node, 'url'] })
    : encodeFunctionData({ abi: resolverAbi, functionName: 'data', args: [node, `stealth-meta-address[1][${coin}]`] });
  const calldata_ = encodeFunctionData({ abi: urAbi, functionName: 'resolve', args: [dns, inner] });
  resolveCalls.push({ calldata: calldata_, dns, inner, addr: k === 0 ? { node, coin_type: toHex(60n, { size: 32 }), legacy: true } : k === 1 ? { node, coin_type: toHex(coin, { size: 32 }), legacy: false } : null });
}

// ---------- 13. identity keys m/137'/0'/{i}' vs @scure/bip32 ----------
const identity: any[] = [];
const seeds = [new Uint8Array(64), new Uint8Array(64).fill(0xff), ...Array.from({ length: 30 }, () => randBytes(64))];
for (let i = 0; i < seeds.length; i++) {
  const index = i === 0 ? 0 : i === 1 ? 2147483647 : i < 6 ? i - 2 : randInt(2147483648);
  const path = `m/137'/0'/${index}'`;
  const k = HDKey.fromMasterSeed(seeds[i]).derive(path);
  const pk = h(k.privateKey!);
  identity.push({ seed: h(seeds[i]), index, path, private_key: pk, address: privateKeyToAddress(pk) });
}

// ---------- 14. lowki labels: validator and ENSIP-15 ----------
const labelCases: any[] = [];
const lbls = ['abc', 'ab', 'a'.repeat(32), 'a'.repeat(33), '-abc', 'abc-', 'ab--c', 'xn--abc', 'a--b', 'abc--', 'a-b-c', '0x1', '123', 'ABC', 'ab_c', 'ab.c', 'añb', 'a b', 'ab-', '---', 'abcd--e', ''];
for (let i = 0; i < 200; i++) lbls.push(randAscii(1, 34, i % 2 ? 'abcdefghijklmnopqrstuvwxyz0123456789-' : 'ab-'));
for (const l of lbls) {
  let norm: string | null = null;
  try {
    norm = ens_normalize(l);
  } catch {}
  labelCases.push({ label: l, normalized: norm });
}


// ---------- LowkiOwnerAccount: EIP-712 Execute + EIP-7702 authorisation ----------
// contracts/src/LowkiOwnerAccount.sol; crates/lowki-ens/src/owner_account.rs.
const ownerAccountAbi = parseAbi([
  'function execute((address target, uint256 value, bytes data)[] calls, uint256 deadline, bytes signature)',
]);
const ownerAccount: any[] = [];
for (let i = 0; i < 48; i++) {
  const key = randPriv();
  const account = privateKeyToAddress(key);
  const chainId = [1, 11155111, 8453, 31337][i % 4] + (i >= 40 ? randInt(1 << 30) : 0);
  const n = i === 0 ? 0 : randInt(5);
  const calls = Array.from({ length: n }, () => ({
    target: randAddr(),
    value: i % 3 === 0 ? randU256() : BigInt(randInt(1000)),
    data: randHex(200),
  }));
  const nonce = i < 4 ? BigInt(i) : randU64() >> 12n;
  const deadline = randU64() >> 12n;
  const digest = hashTypedData({
    domain: { name: 'Lowki Owner Account', version: '1', chainId, verifyingContract: account },
    types: {
      Execute: [
        { name: 'calls', type: 'Call[]' },
        { name: 'nonce', type: 'uint256' },
        { name: 'deadline', type: 'uint256' },
      ],
      Call: [
        { name: 'target', type: 'address' },
        { name: 'value', type: 'uint256' },
        { name: 'data', type: 'bytes' },
      ],
    },
    primaryType: 'Execute',
    message: { calls, nonce, deadline },
  });
  const signature = await sign({ hash: digest, privateKey: key, to: 'hex' });
  const delegate = randAddr();
  const authNonce = [0, 1, 127, 128, 255, 256, 65535][i % 7] + (i >= 28 ? randInt(1 << 30) : 0);
  const authHash = hashAuthorization({ address: delegate, chainId, nonce: authNonce });
  const auth = await signAuthorization({ address: delegate, chainId, nonce: authNonce, privateKey: key });
  ownerAccount.push({
    key, account, chain_id: chainId,
    calls: calls.map((c) => ({ target: c.target, value: toHex(c.value, { size: 32 }), data: c.data })),
    nonce: nonce.toString(), deadline: deadline.toString(), digest, signature,
    calldata: encodeFunctionData({ abi: ownerAccountAbi, functionName: 'execute', args: [calls, deadline, signature] }),
    delegate, auth_nonce: authNonce, auth_hash: authHash,
    auth_r: toHex(BigInt(auth.r), { size: 32 }), auth_s: toHex(BigInt(auth.s), { size: 32 }), auth_y_parity: auth.yParity,
  });
}

// ---------- write ----------
const out = {
  meta: {
    generator: 'vectors/lowki-ens/gen.ts',
    seed: SEED,
    viem: '2.57.16',
    'ens-normalize': '1.11.1',
    'scure-bip32': '2.4.0',
    'noble-curves': '2.4.0',
    ensip15_tests: { url: TESTS_URL, sha256: TESTS_SHA256 },
  },
  normalize: normalizeCases,
  names: nameCases,
  roles,
  calldata,
  decoders,
  abi_roundtrip: abiRoundtrip,
  offchain_lookup: offchain,
  ccip_requests: ccipRequests,
  ccip_responses: responseCases,
  batch_gateway: batchCases,
  ccip_flows: flows,
  eip712,
  personal_sign: personal,
  digest_signatures: digestSigs,
  stealth_records: stealthRecords,
  stealth_keys: stealthKeys,
  scheme1,
  resolve_calldata: resolveCalls,
  identity,
  lowki_labels: labelCases,
  owner_account: ownerAccount,
};
writeFileSync('vectors.json', JSON.stringify(out, null, 1) + '\n');
const counts = Object.fromEntries(Object.entries(out).filter(([k]) => k !== 'meta' && k !== 'roles').map(([k, v]) => [k, (v as any[]).length]));
console.log('wrote vectors.json', counts);
