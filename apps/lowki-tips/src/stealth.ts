// Recipient side of ERC-5564 scheme 1 (the same maths as crates/lowki-stealth): find which
// gateway records belong to a name, and prove control by deriving each stealth private key.
// Used by the creator inbox only. The keys come from the creator's seed (demo seed on the fork).
import { secp256k1 } from '@noble/curves/secp256k1';
import { sha512 } from '@noble/hashes/sha2';
import { HDKey } from '@scure/bip32';
import { bytesToHex, hexToBytes, keccak256, type Address, type Hex } from 'viem';
import { privateKeyToAccount, publicKeyToAddress } from 'viem/accounts';

const N = secp256k1.CURVE.n;

export interface StealthKeys {
  spendingPriv: Uint8Array;
  viewingPriv: Uint8Array;
  spendingPub: Uint8Array; // compressed
  viewingPub: Uint8Array; // compressed
}

/** Demo creators' 64-byte wallet seed: sha512("<phrase>/<label>"). Same as scripts/demo-tips. */
export function demoSeed(phrase: string, label: string): Uint8Array {
  return sha512(new TextEncoder().encode(`${phrase}/${label}`));
}

/** `m/5564'/0'/0'` (spending) and `m/5564'/0'/1'` (viewing), as lowki-stealth derives them. */
export function stealthKeys(seed: Uint8Array): StealthKeys {
  const m = HDKey.fromMasterSeed(seed);
  const s = m.derive("m/5564'/0'/0'").privateKey!;
  const v = m.derive("m/5564'/0'/1'").privateKey!;
  return { spendingPriv: s, viewingPriv: v, spendingPub: secp256k1.getPublicKey(s, true), viewingPub: secp256k1.getPublicKey(v, true) };
}

export function metaAddress(k: StealthKeys): string {
  return `st:eth:${bytesToHex(new Uint8Array([...k.spendingPub, ...k.viewingPub]))}`;
}

function hashedSharedSecret(priv: Uint8Array, pub: Uint8Array): Uint8Array {
  return hexToBytes(keccak256(secp256k1.getSharedSecret(priv, pub, true)));
}

function addressOfPoint(p: InstanceType<typeof secp256k1.ProjectivePoint>): Address {
  return publicKeyToAddress(bytesToHex(p.toRawBytes(false)));
}

/** The stealth address an announcement pays, if it is ours (else null). */
export function checkAnnouncement(k: StealthKeys, ephemeralPub: Hex, viewTag: number): Address | null {
  let h: Uint8Array;
  try {
    h = hashedSharedSecret(k.viewingPriv, hexToBytes(ephemeralPub));
  } catch {
    return null;
  }
  if (h[0] !== viewTag) return null;
  const s = BigInt(bytesToHex(h));
  if (s === 0n || s >= N) return null;
  const p = secp256k1.ProjectivePoint.fromHex(k.spendingPub).add(secp256k1.ProjectivePoint.BASE.multiply(s));
  return addressOfPoint(p);
}

/** `(spending + keccak(compressed(viewing · E))) mod n`, the key that controls the stealth address. */
export function stealthPrivateKey(k: StealthKeys, ephemeralPub: Hex): Hex {
  const h = hashedSharedSecret(k.viewingPriv, hexToBytes(ephemeralPub));
  const key = (BigInt(bytesToHex(k.spendingPriv)) + BigInt(bytesToHex(h))) % N;
  return `0x${key.toString(16).padStart(64, '0')}`;
}

/** Viewing-key signature over lowki_ens::stealth::records_auth_message(name, ts). */
export async function recordsAuth(k: StealthKeys, name: string): Promise<{ ts: number; signature: Hex }> {
  const ts = Math.floor(Date.now() / 1000);
  const account = privateKeyToAccount(bytesToHex(k.viewingPriv));
  const signature = await account.signMessage({ message: `Lowki stealth records\nname: ${name}\ntimestamp: ${ts}` });
  return { ts, signature };
}

export function addressOfKey(key: Hex): Address {
  return privateKeyToAccount(key).address;
}
