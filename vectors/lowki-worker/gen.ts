// Differential vectors for crates/lowki-worker (npx tsx gen.ts -> vectors.json).
//
// References (exact versions in package.json): viem 2.54.1 for personal_sign, EIP-712,
// EIP-1559 / EIP-7702 transaction serialization+signing and 7702 authorizations;
// @scure/bip39 + @scure/bip32 2.2.0 for keys (the app key is m/44'/60'/{appIndex}'/0/0
// with appIndex = u32be(keccak("lowki/app/v1" || origin)[0..4]) & 0x7fffffff);
// @noble/hashes 2.2.0 HKDF/HMAC for the pairwise user ID (Lowki's own definition).
// Deterministic: fixed mnemonics and a SHA-256 counter stream for random fields.
import { createHash } from 'node:crypto';
import { writeFileSync } from 'node:fs';
import { HDKey } from '@scure/bip32';
import { mnemonicToSeedSync, entropyToMnemonic } from '@scure/bip39';
import { wordlist } from '@scure/bip39/wordlists/english.js';
import { hkdf } from '@noble/hashes/hkdf.js';
import { hmac } from '@noble/hashes/hmac.js';
import { sha256 } from '@noble/hashes/sha2.js';
import { keccak256, toBytes, toHex, bytesToHex, hexToBytes, getAddress, type Hex } from 'viem';
import { privateKeyToAccount } from 'viem/accounts';

let ctr = 0;
const rnd = (n: number): Uint8Array => {
  const out = new Uint8Array(n);
  for (let i = 0; i < n; i += 32) {
    const b = createHash('sha256').update(`lowki-worker-vectors-v1/${ctr++}`).digest();
    out.set(b.subarray(0, Math.min(32, n - i)), i);
  }
  return out;
};
const rint = (lo: number, hi: number) => lo + (Buffer.from(rnd(4)).readUInt32BE(0) % (hi - lo + 1));
const rbig = (bytes: number) => BigInt(bytesToHex(rnd(bytes)));

const mnemonics = [
  'abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon art',
  entropyToMnemonic(rnd(32), wordlist),
  entropyToMnemonic(rnd(32), wordlist),
];
const origins = [
  'https://app.example',
  'https://uniswap.org',
  'http://localhost:3000',
  'https://a.b.c.example.co.uk',
  'https://xn--bcher-kva.example',
  'https://app.example:8443',
];

function appIndex(origin: string) {
  const h = keccak256(toBytes('lowki/app/v1' + origin), 'bytes');
  return ((h[0] << 24) | (h[1] << 16) | (h[2] << 8) | h[3]) & 0x7fffffff;
}
function appKey(seed: Uint8Array, origin: string): Hex {
  const k = HDKey.fromMasterSeed(seed).derive(`m/44'/60'/${appIndex(origin)}'/0/0`);
  return bytesToHex(k.privateKey!);
}
function pairwise(seed: Uint8Array, origin: string): Hex {
  const k = hkdf(sha256, seed, new Uint8Array(0), toBytes('lowki/pairwise-id/key/v1'), 32);
  const msg = new Uint8Array([...toBytes('lowki/pairwise-id/v1'), 0, ...toBytes(origin)]);
  return bytesToHex(hmac(sha256, k, msg));
}

const accounts: any[] = [];
const messages: any[] = [];
const typed: any[] = [];
const txs: any[] = [];
const auths: any[] = [];

const typedSamples = (verifyingContract: Hex, chainId: number) => [
  // EIP-712 spec "Mail" example
  {
    types: {
      EIP712Domain: [
        { name: 'name', type: 'string' },
        { name: 'version', type: 'string' },
        { name: 'chainId', type: 'uint256' },
        { name: 'verifyingContract', type: 'address' },
      ],
      Person: [
        { name: 'name', type: 'string' },
        { name: 'wallet', type: 'address' },
      ],
      Mail: [
        { name: 'from', type: 'Person' },
        { name: 'to', type: 'Person' },
        { name: 'contents', type: 'string' },
      ],
    },
    primaryType: 'Mail',
    domain: { name: 'Ether Mail', version: '1', chainId, verifyingContract },
    message: {
      from: { name: 'Cow', wallet: '0xCD2a3d9F938E13CD947Ec05AbC7FE734Df8DD826' },
      to: { name: 'Bob', wallet: '0xbBbBBBBbbBBBbbbBbbBbbbbBBbBbbbbBbBbbBBbB' },
      contents: 'Hello, Bob!',
    },
  },
  // Permit2-like with arrays, bytes, bool, int, nested arrays; EIP712Domain omitted (viem infers)
  {
    types: {
      Item: [
        { name: 'token', type: 'address' },
        { name: 'amount', type: 'uint160' },
        { name: 'data', type: 'bytes' },
      ],
      Batch: [
        { name: 'items', type: 'Item[]' },
        { name: 'spender', type: 'address' },
        { name: 'deadline', type: 'uint256' },
        { name: 'flag', type: 'bool' },
        { name: 'delta', type: 'int64' },
        { name: 'tags', type: 'bytes32[]' },
        { name: 'note', type: 'string' },
      ],
    },
    primaryType: 'Batch',
    domain: { name: 'Permit2', chainId, verifyingContract },
    message: {
      items: [
        { token: getAddress(bytesToHex(rnd(20))), amount: rbig(19).toString(), data: bytesToHex(rnd(rint(0, 70))) },
        { token: getAddress(bytesToHex(rnd(20))), amount: '0', data: '0x' },
      ],
      spender: getAddress(bytesToHex(rnd(20))),
      deadline: rbig(32).toString(),
      flag: true,
      delta: '-' + rint(1, 1e9).toString(),
      tags: [bytesToHex(rnd(32)), bytesToHex(rnd(32))],
      note: 'ünïcødé ✓',
    },
  },
  // domain with salt only, uint8 + fixed array
  {
    types: {
      EIP712Domain: [{ name: 'salt', type: 'bytes32' }],
      Vote: [
        { name: 'choice', type: 'uint8' },
        { name: 'weights', type: 'uint32[3]' },
      ],
    },
    primaryType: 'Vote',
    domain: { salt: bytesToHex(rnd(32)) },
    message: { choice: 3, weights: [1, 2, rint(0, 2 ** 32 - 1)] },
  },
];

for (const [mi, mnemonic] of mnemonics.entries()) {
  const seed = mnemonicToSeedSync(mnemonic);
  for (const origin of origins) {
    const key = appKey(seed, origin);
    const acct = privateKeyToAccount(key);
    accounts.push({ mnemonic: mi, origin, appIndex: appIndex(origin), address: acct.address, pairwiseUserId: pairwise(seed, origin) });
  }
  for (const origin of origins.slice(0, 3)) {
    const acct = privateKeyToAccount(appKey(seed, origin));
    // personal_sign
    for (const m of ['hello', '', 'ünïcødé ✓\n', 'x'.repeat(200)]) {
      messages.push({ mnemonic: mi, origin, message: m, signature: await acct.signMessage({ message: m }) });
    }
    for (let i = 0; i < 3; i++) {
      const raw = bytesToHex(rnd(rint(0, 100)));
      messages.push({ mnemonic: mi, origin, messageHex: raw, signature: await acct.signMessage({ message: { raw } }) });
    }
    // EIP-712
    for (const td of typedSamples(getAddress(bytesToHex(rnd(20))), [1, 11155111, 10][rint(0, 2)])) {
      typed.push({ mnemonic: mi, origin, typedData: td, signature: await acct.signTypedData(td as any) });
    }
    // 7702 authorizations
    for (const chainId of [0, 1, 11155111]) {
      const a = await acct.signAuthorization({ chainId, address: getAddress(bytesToHex(rnd(20))), nonce: rint(0, 1000) });
      auths.push({
        mnemonic: mi,
        origin,
        chainId: String(chainId),
        address: a.address,
        nonce: String(a.nonce),
        yParity: a.yParity,
        r: a.r,
        s: a.s,
      });
    }
    // transactions
    for (let i = 0; i < 6; i++) {
      const chainId = [1, 11155111, 10, 8453][rint(0, 3)];
      const tip = BigInt(rint(0, 5e9));
      const base: any = {
        chainId,
        nonce: [0, 1, rint(2, 1e6)][i % 3],
        maxPriorityFeePerGas: tip,
        maxFeePerGas: tip + BigInt(rint(0, 1e11)),
        gas: BigInt(rint(21000, 3e7)),
        value: i % 2 ? rbig(rint(1, 12)) : 0n,
        data: i % 3 ? bytesToHex(rnd(rint(1, 300))) : undefined,
        accessList:
          i % 2
            ? [{ address: getAddress(bytesToHex(rnd(20))), storageKeys: [bytesToHex(rnd(32)), bytesToHex(rnd(32))] }, { address: getAddress(bytesToHex(rnd(20))), storageKeys: [] }]
            : [],
      };
      const to = getAddress(bytesToHex(rnd(20)));
      // eip1559 (one contract creation)
      const tx1 = { ...base, type: 'eip1559' as const, to: i === 4 ? undefined : to };
      const raw1 = await acct.signTransaction(tx1);
      txs.push({ mnemonic: mi, origin, tx: jsonTx(tx1), raw: raw1, hash: keccak256(raw1) });
      // eip7702 with 1-2 authorizations (from fresh keys)
      const list = [];
      for (let k = 0; k < 1 + (i % 2); k++) {
        const signer = privateKeyToAccount(bytesToHex(rnd(32)));
        list.push(await signer.signAuthorization({ chainId: [0, chainId][k % 2], address: getAddress(bytesToHex(rnd(20))), nonce: rint(0, 50) }));
      }
      const tx4 = { ...base, type: 'eip7702' as const, to, authorizationList: list };
      const raw4 = await acct.signTransaction(tx4);
      txs.push({ mnemonic: mi, origin, tx: jsonTx(tx4), raw: raw4, hash: keccak256(raw4) });
    }
  }
}

function jsonTx(t: any) {
  return {
    type: t.type,
    chainId: String(t.chainId),
    nonce: String(t.nonce),
    maxPriorityFeePerGas: t.maxPriorityFeePerGas.toString(),
    maxFeePerGas: '0x' + t.maxFeePerGas.toString(16),
    gas: t.gas.toString(),
    to: t.to,
    value: t.value.toString(),
    data: t.data,
    accessList: t.accessList,
    authorizationList: (t.authorizationList ?? []).map((a: any) => ({
      chainId: String(a.chainId),
      address: a.address,
      nonce: String(a.nonce),
      yParity: a.yParity,
      r: a.r,
      s: a.s,
    })),
  };
}

const pkg = JSON.parse((await import('node:fs')).readFileSync(new URL('./package.json', import.meta.url), 'utf8'));
writeFileSync(
  new URL('./vectors.json', import.meta.url),
  JSON.stringify({ meta: { deps: pkg.dependencies, node: process.version }, mnemonics, accounts, messages, typed, txs, auths }, (_, v) => (typeof v === 'bigint' ? v.toString() : v), 1) + '\n',
);
console.log(`accounts ${accounts.length}, messages ${messages.length}, typed ${typed.length}, txs ${txs.length}, auths ${auths.length}`);
void hexToBytes;
void toHex;
