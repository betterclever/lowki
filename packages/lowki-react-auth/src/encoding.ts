import { LowkiError, PrivyErrorCode, RpcErrorCode } from './errors';
import type { Quantity, UnsignedTransactionRequest } from './types';
import type { WireTxRequest } from './protocol';

const bad = (msg: string) => new LowkiError(PrivyErrorCode.INVALID_DATA, msg, { rpcCode: RpcErrorCode.INVALID_PARAMS });

const ADDRESS = /^0x[0-9a-fA-F]{40}$/;
export const isAddress = (v: unknown): v is `0x${string}` => typeof v === 'string' && ADDRESS.test(v);
export const sameAddress = (a: string, b: string) => a.toLowerCase() === b.toLowerCase();

/** A JSON-RPC quantity, number, bigint or decimal string, as a decimal string (the wallet's `value` format). */
export function quantityToDecimal(q: Quantity | undefined | null, field = 'value'): string {
  if (q === undefined || q === null || q === '') return '0';
  let n: bigint;
  try {
    if (typeof q === 'bigint') n = q;
    else if (typeof q === 'number') {
      if (!Number.isSafeInteger(q)) throw new Error();
      n = BigInt(q);
    } else if (/^0x[0-9a-fA-F]+$/.test(q) || /^\d+$/.test(q)) n = BigInt(q);
    else throw new Error();
  } catch {
    throw bad(`${field} must be an integer quantity, got ${String(q)}`);
  }
  if (n < 0n) throw bad(`${field} must not be negative`);
  return n.toString(10);
}

export function bytesToHex(b: ArrayLike<number>): `0x${string}` {
  let s = '0x';
  for (let i = 0; i < b.length; i++) {
    const x = b[i]!;
    if (!Number.isInteger(x) || x < 0 || x > 255) throw bad('data must be bytes');
    s += x.toString(16).padStart(2, '0');
  }
  return s as `0x${string}`;
}

export function dataToHex(d: ArrayLike<number> | string | undefined | null): `0x${string}` {
  if (d === undefined || d === null || d === '') return '0x';
  if (typeof d === 'string') {
    if (!/^0x([0-9a-fA-F]{2})*$/.test(d)) throw bad('data must be 0x-prefixed hex bytes');
    return d.toLowerCase() as `0x${string}`;
  }
  return bytesToHex(d);
}

export function hexToBytes(h: string): Uint8Array {
  if (!/^0x([0-9a-fA-F]{2})*$/.test(h)) throw bad('expected 0x-prefixed hex bytes');
  const out = new Uint8Array((h.length - 2) / 2);
  for (let i = 0; i < out.length; i++) out[i] = Number.parseInt(h.slice(2 + i * 2, 4 + i * 2), 16);
  return out;
}

/**
 * `personal_sign` takes hex bytes; the wallet signs UTF-8 text (protocol v1
 * `sign_message.message` is a string). Hex that isn't valid UTF-8 can't be
 * represented, so we refuse it rather than sign something else.
 */
export function personalSignMessageToText(m: string): string {
  if (!/^0x([0-9a-fA-F]{2})*$/.test(m)) return m;
  try {
    return new TextDecoder('utf-8', { fatal: true }).decode(hexToBytes(m));
  } catch {
    throw new LowkiError(PrivyErrorCode.INVALID_MESSAGE, 'Lowki can only sign messages that are UTF-8 text (raw byte messages are not supported by wallet protocol v1)', {
      rpcCode: RpcErrorCode.INVALID_PARAMS,
    });
  }
}

/** A Privy / JSON-RPC transaction → the wallet's `tx`. Fields the wallet can't take are checked, then dropped. */
export function toWireTx(tx: UnsignedTransactionRequest & { gas?: Quantity }): WireTxRequest {
  if (!tx || typeof tx !== 'object') throw bad('transaction must be an object');
  if (!tx.to) {
    throw new LowkiError(PrivyErrorCode.NOT_SUPPORTED, 'Contract deployment (a transaction without `to`) is not supported by the Lowki wallet', {
      rpcCode: RpcErrorCode.INVALID_PARAMS,
    });
  }
  if (!isAddress(tx.to)) throw bad(`to must be an address, got ${String(tx.to)}`);
  return { to: tx.to, value: quantityToDecimal(tx.value), data: dataToHex(tx.data) };
}

/** JSON with bigints as decimal strings (typed data from viem can carry bigints). */
export function jsonSafe<T>(v: T): unknown {
  return JSON.parse(JSON.stringify(v, (_k, x: unknown) => (typeof x === 'bigint' ? x.toString(10) : x)));
}
