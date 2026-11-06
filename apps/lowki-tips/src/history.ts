// Tips sent in this browser session, per creator. sessionStorage is a convenience only:
// everything still works (with an empty history) when storage is unavailable.
import type { Address, Hex } from 'viem';

export interface TipRecord {
  address: Address;
  hash: Hex | string;
  private: boolean;
  amountEth: string;
  at: number;
}

const KEY = 'lowki-tips/history/v1';

function readAll(): Record<string, TipRecord[]> {
  try {
    return JSON.parse(sessionStorage.getItem(KEY) ?? '{}') as Record<string, TipRecord[]>;
  } catch {
    return {};
  }
}

export function tipsFor(name: string): TipRecord[] {
  return readAll()[name] ?? [];
}

export function addTip(name: string, t: TipRecord): void {
  const all = readAll();
  all[name] = [...(all[name] ?? []), t];
  try {
    sessionStorage.setItem(KEY, JSON.stringify(all));
  } catch {
    /* storage unavailable: history lives only until reload */
  }
}
