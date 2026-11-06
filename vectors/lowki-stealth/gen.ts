// Differential vector generator for crates/lowki-stealth.
//
// Reference implementations (exact versions pinned in package.json / package-lock.json):
//   - @scopelift/stealth-address-sdk 1.0.0-beta.5 (ERC-5564 scheme 1 crypto, metadata builders, ABIs, addresses)
//     which itself uses @noble/secp256k1 2.3.0
//   - viem 2.57.16 (ABI calldata / event encoding + decoding, address checksums)
//   - @scure/bip32 2.0.1 (BIP-32 derivation for Lowki stealth keys)
//
// Run: npm install && npx tsx gen.ts   (writes vectors.json)

import { writeFileSync } from 'node:fs';
import { createHash } from 'node:crypto';
import {
  ERC5564AnnouncerAbi,
  ERC6538RegistryAbi,
  ERC5564_CONTRACT_ADDRESS,
  ERC6538_CONTRACT_ADDRESS,
  VALID_SCHEME_ID,
  buildMetadataCustom,
  buildMetadataForERC20,
  buildMetadataForERC721,
  buildMetadataForETH,
  checkStealthAddress,
  computeStealthKey,
  generateStealthAddress,
  generateStealthMetaAddressFromKeys,
  getViewTagFromMetadata,
  parseKeysFromStealthMetaAddress,
} from '@scopelift/stealth-address-sdk';
import { CURVE, getPublicKey, getSharedSecret } from '@noble/secp256k1';
import { HDKey } from '@scure/bip32';
import {
  bytesToHex,
  decodeEventLog,
  encodeAbiParameters,
  encodeEventTopics,
  encodeFunctionData,
  getAddress,
  hexToBytes,
  keccak256,
  type Hex,
} from 'viem';
import { privateKeyToAddress } from 'viem/accounts';

// ---------- deterministic RNG (sha256 counter mode, fixed seed) ----------
const SEED = 'lowki-stealth-vectors-v1';
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
const N = CURVE.n;
function randPriv(): Uint8Array {
  const x = BigInt(bytesToHex(randBytes(40))) % (N - 1n) + 1n;
  return scalarBytes(x);
}
function scalarBytes(x: bigint): Uint8Array {
  return hexToBytes(`0x${x.toString(16).padStart(64, '0')}`);
}
function randInt(max: number): number {
  return Number(BigInt(bytesToHex(randBytes(6))) % BigInt(max));
}
const h = (b: Uint8Array) => bytesToHex(b);

// ---------- 1. meta-address round-trips ----------
const metaAddress: any[] = [];
for (let i = 0; i < 64; i++) {
  const spendPriv = randPriv();
  const viewPriv = randPriv();
  const spendPub = h(getPublicKey(spendPriv, true));
  const viewPub = h(getPublicKey(viewPriv, true));
  const raw = generateStealthMetaAddressFromKeys({ spendingPublicKey: spendPub, viewingPublicKey: viewPub });
  const parsed = parseKeysFromStealthMetaAddress({ stealthMetaAddress: raw, schemeId: VALID_SCHEME_ID.SCHEME_ID_1 });
  metaAddress.push({
    spending_pubkey: spendPub,
    viewing_pubkey: viewPub,
    raw,
    uri: `st:eth:${raw}`,
    parsed_spending: h(parsed.spendingPublicKey),
    parsed_viewing: h(parsed.viewingPublicKey),
  });
}
// single-key (33-byte) form accepted by the SDK: same key used for spending and viewing.
const singleKey: any[] = [];
for (let i = 0; i < 8; i++) {
  const pub = h(getPublicKey(randPriv(), true));
  const parsed = parseKeysFromStealthMetaAddress({ stealthMetaAddress: pub, schemeId: VALID_SCHEME_ID.SCHEME_ID_1 });
  singleKey.push({ raw: pub, parsed_spending: h(parsed.spendingPublicKey), parsed_viewing: h(parsed.viewingPublicKey) });
}

// ---------- 2. generate -> check -> compute key ----------
function stealthCase(spendPriv: Uint8Array, viewPriv: Uint8Array, ephPriv: Uint8Array, label: string) {
  const spendPub = h(getPublicKey(spendPriv, true));
  const viewPub = h(getPublicKey(viewPriv, true));
  const raw = generateStealthMetaAddressFromKeys({ spendingPublicKey: spendPub, viewingPublicKey: viewPub });
  const uri = `st:eth:${raw}`;
  const g = generateStealthAddress({
    stealthMetaAddressURI: uri,
    schemeId: VALID_SCHEME_ID.SCHEME_ID_1,
    ephemeralPrivateKey: ephPriv,
  });
  // Recompute the internals exactly as the SDK does (noble getSharedSecret default = 33-byte compressed point).
  const shared = getSharedSecret(ephPriv, hexToBytes(viewPub as Hex));
  const hashed = keccak256(shared);
  const stealthPriv = computeStealthKey({
    ephemeralPublicKey: g.ephemeralPublicKey,
    schemeId: VALID_SCHEME_ID.SCHEME_ID_1,
    spendingPrivateKey: h(spendPriv),
    viewingPrivateKey: h(viewPriv),
  }) as Hex;
  const addrFromKey = privateKeyToAddress(stealthPriv);
  const ok = checkStealthAddress({
    ephemeralPublicKey: g.ephemeralPublicKey,
    schemeId: VALID_SCHEME_ID.SCHEME_ID_1,
    spendingPublicKey: spendPub,
    userStealthAddress: g.stealthAddress,
    viewingPrivateKey: h(viewPriv),
    viewTag: g.viewTag,
  });
  if (!ok || addrFromKey !== g.stealthAddress) throw new Error(`reference self-check failed: ${label}`);

  // Wrong view tag: SDK must reject.
  const wrongTag = `0x${((parseInt(g.viewTag.slice(2), 16) + 1 + randInt(255)) % 256).toString(16).padStart(2, '0')}`;
  const wrongTagOk = checkStealthAddress({
    ephemeralPublicKey: g.ephemeralPublicKey,
    schemeId: VALID_SCHEME_ID.SCHEME_ID_1,
    spendingPublicKey: spendPub,
    userStealthAddress: g.stealthAddress,
    viewingPrivateKey: h(viewPriv),
    viewTag: wrongTag,
  });
  // Wrong viewing key: SDK must reject (we record the SDK's verdict and what the wrong key's view tag would be).
  const wrongViewPriv = randPriv();
  const wrongShared = getSharedSecret(wrongViewPriv, hexToBytes(g.ephemeralPublicKey as Hex));
  const wrongViewTag = keccak256(wrongShared).slice(0, 4);
  const wrongViewOk = checkStealthAddress({
    ephemeralPublicKey: g.ephemeralPublicKey,
    schemeId: VALID_SCHEME_ID.SCHEME_ID_1,
    spendingPublicKey: spendPub,
    userStealthAddress: g.stealthAddress,
    viewingPrivateKey: h(wrongViewPriv),
    viewTag: g.viewTag,
  });
  if (wrongTagOk || wrongViewOk) throw new Error(`reference accepted a wrong input: ${label}`);
  return {
    label,
    spending_priv: h(spendPriv),
    viewing_priv: h(viewPriv),
    ephemeral_priv: h(ephPriv),
    spending_pubkey: spendPub,
    viewing_pubkey: viewPub,
    meta_uri: uri,
    shared_secret_compressed: h(shared),
    hashed_shared_secret: hashed,
    stealth_address: g.stealthAddress,
    ephemeral_pubkey: g.ephemeralPublicKey,
    view_tag: g.viewTag,
    stealth_priv: stealthPriv,
    address_from_stealth_priv: addrFromKey,
    wrong_view_tag: wrongTag,
    wrong_view_tag_accepted: wrongTagOk,
    wrong_viewing_priv: h(wrongViewPriv),
    wrong_viewing_priv_view_tag: wrongViewTag,
    wrong_viewing_priv_accepted: wrongViewOk,
  };
}
const stealth: any[] = [];
const one = scalarBytes(1n);
const nm1 = scalarBytes(N - 1n);
stealth.push(stealthCase(one, one, one, 'all keys = 1'));
stealth.push(stealthCase(nm1, nm1, nm1, 'all keys = n-1'));
stealth.push(stealthCase(nm1, one, scalarBytes(2n), 'spend=n-1 view=1 eph=2'));
stealth.push(stealthCase(scalarBytes(N - 2n), scalarBytes(1n << 255n), scalarBytes((1n << 128n) + 7n), 'high/structured scalars'));
stealth.push(stealthCase(randPriv(), randPriv(), one, 'eph = 1'));
stealth.push(stealthCase(randPriv(), randPriv(), nm1, 'eph = n-1'));
{
  // spending == viewing key (single-key meta-address semantics)
  const k = randPriv();
  stealth.push(stealthCase(k, k, randPriv(), 'spend == view'));
}
for (let i = 0; i < 160; i++) stealth.push(stealthCase(randPriv(), randPriv(), randPriv(), `random #${i}`));

// ---------- 3. metadata + announce calldata + event logs ----------
const metadata: any[] = [];
const announce: any[] = [];
const events: any[] = [];
function randAddr(): Hex {
  return getAddress(h(randBytes(20)));
}
for (let i = 0; i < 96; i++) {
  const s = stealth[i % stealth.length];
  const viewTag = s.view_tag as Hex;
  const kind = i % 6;
  let md: Hex;
  let mdKind: string;
  let token: Hex | null = null;
  let selector: Hex | null = null;
  let amount: string | null = null;
  const amt = i % 12 === 0 ? (1n << 256n) - 1n : i % 12 === 1 ? 0n : BigInt(h(randBytes(1 + randInt(32))));
  if (kind === 0) {
    md = viewTag;
    mdKind = 'view_tag_only';
  } else if (kind === 1) {
    md = buildMetadataForETH({ viewTag, amount: amt }) as Hex;
    mdKind = 'eth';
    amount = amt.toString();
  } else if (kind === 2) {
    token = randAddr();
    md = buildMetadataForERC20({ viewTag, tokenAddress: token, amount: amt }) as Hex;
    mdKind = 'erc20';
    selector = '0xa9059cbb';
    amount = amt.toString();
  } else if (kind === 3) {
    token = randAddr();
    md = buildMetadataForERC721({ viewTag, tokenAddress: token, tokenId: amt }) as Hex;
    mdKind = 'erc721';
    selector = '0x23b872dd';
    amount = amt.toString();
  } else if (kind === 4) {
    token = randAddr();
    selector = h(randBytes(4));
    md = buildMetadataCustom({ viewTag, functionSelector: selector, contractAddress: token, data: amt }) as Hex;
    mdKind = 'custom';
    amount = amt.toString();
  } else {
    md = `${viewTag}${h(randBytes(randInt(80))).slice(2)}` as Hex;
    mdKind = 'arbitrary';
  }
  if (getViewTagFromMetadata(md) !== viewTag) throw new Error('view tag extraction mismatch');
  metadata.push({ kind: mdKind, view_tag: viewTag, token, selector, amount, metadata: md });

  const calldata = encodeFunctionData({
    abi: ERC5564AnnouncerAbi,
    functionName: 'announce',
    args: [1n, s.stealth_address, s.ephemeral_pubkey, md],
  });
  announce.push({
    scheme_id: '1',
    stealth_address: s.stealth_address,
    ephemeral_pubkey: s.ephemeral_pubkey,
    metadata: md,
    calldata,
  });

  // Announcement event log as emitted by the Announcer, then decoded by viem.
  const schemeId = i % 10 === 9 ? BigInt(h(randBytes(32))) : 1n;
  const caller = randAddr();
  const topics = encodeEventTopics({
    abi: ERC5564AnnouncerAbi,
    eventName: 'Announcement',
    args: { schemeId, stealthAddress: s.stealth_address, caller },
  });
  const data = encodeAbiParameters([{ type: 'bytes' }, { type: 'bytes' }], [s.ephemeral_pubkey, md]);
  const decoded = decodeEventLog({ abi: ERC5564AnnouncerAbi, topics: topics as any, data }) as any;
  events.push({
    topics,
    data,
    decoded: {
      scheme_id: decoded.args.schemeId.toString(),
      stealth_address: decoded.args.stealthAddress,
      caller: decoded.args.caller,
      ephemeral_pubkey: decoded.args.ephemeralPubKey,
      metadata: decoded.args.metadata,
    },
  });
}

// ---------- ERC-6538 registry: registerKeys calldata + stealthMetaAddressOf return decoding ----------
const registry: any[] = [];
for (let i = 0; i < 32; i++) {
  const m = metaAddress[i];
  const calldata = encodeFunctionData({
    abi: ERC6538RegistryAbi,
    functionName: 'registerKeys',
    args: [1n, m.raw],
  });
  const registrant = randAddr();
  const lookup = encodeFunctionData({
    abi: ERC6538RegistryAbi,
    functionName: 'stealthMetaAddressOf',
    args: [registrant, 1n],
  });
  const returnData = encodeAbiParameters([{ type: 'bytes' }], [m.raw]);
  registry.push({
    scheme_id: '1',
    meta_raw: m.raw,
    register_keys_calldata: calldata,
    registrant,
    stealth_meta_address_of_calldata: lookup,
    stealth_meta_address_of_return: returnData,
  });
}

// ---------- 4. Lowki BIP-32 key derivation ----------
const SPEND_PATH = "m/5564'/0'/0'";
const VIEW_PATH = "m/5564'/0'/1'";
const bip32: any[] = [];
const seeds: Uint8Array[] = [new Uint8Array(64), new Uint8Array(64).fill(0xff)];
for (let i = 0; i < 62; i++) seeds.push(randBytes(64));
for (const seed of seeds) {
  const root = HDKey.fromMasterSeed(seed);
  const sp = root.derive(SPEND_PATH);
  const vp = root.derive(VIEW_PATH);
  const spendPub = h(sp.publicKey!);
  const viewPub = h(vp.publicKey!);
  const raw = generateStealthMetaAddressFromKeys({ spendingPublicKey: spendPub, viewingPublicKey: viewPub });
  bip32.push({
    seed: h(seed),
    master_priv: h(root.privateKey!),
    master_chain_code: h(root.chainCode!),
    spending_priv: h(sp.privateKey!),
    spending_pubkey: spendPub,
    viewing_priv: h(vp.privateKey!),
    viewing_pubkey: viewPub,
    meta_uri: `st:eth:${raw}`,
  });
}

const out = {
  generator: {
    seed: SEED,
    references: {
      '@scopelift/stealth-address-sdk': '1.0.0-beta.5',
      '@noble/secp256k1': '2.3.0',
      viem: '2.57.16',
      '@scure/bip32': '2.0.1',
    },
  },
  constants: {
    erc5564_announcer: ERC5564_CONTRACT_ADDRESS,
    erc6538_registry: ERC6538_CONTRACT_ADDRESS,
    announcement_topic0: encodeEventTopics({ abi: ERC5564AnnouncerAbi, eventName: 'Announcement' })[0],
  },
  meta_address: metaAddress,
  single_key_meta_address: singleKey,
  stealth,
  metadata,
  announce,
  events,
  registry,
  bip32: { spending_path: SPEND_PATH, viewing_path: VIEW_PATH, cases: bip32 },
};
writeFileSync(new URL('./vectors.json', import.meta.url), JSON.stringify(out, null, 1) + '\n');
console.log(
  `wrote vectors.json: meta=${metaAddress.length} single=${singleKey.length} stealth=${stealth.length} metadata=${metadata.length} announce=${announce.length} events=${events.length} registry=${registry.length} bip32=${bip32.length}`,
);
