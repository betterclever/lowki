import type { Chain } from './types';

export const sepolia: Chain = {
  id: 11_155_111,
  name: 'Sepolia',
  nativeCurrency: { name: 'Sepolia Ether', symbol: 'ETH', decimals: 18 },
  rpcUrls: { default: { http: ['https://ethereum-sepolia-rpc.publicnode.com'] } },
  blockExplorers: { default: { name: 'Etherscan', url: 'https://sepolia.etherscan.io' } },
  testnet: true,
};

export const monadTestnet: Chain = {
  id: 10_143,
  name: 'Monad Testnet',
  nativeCurrency: { name: 'Monad', symbol: 'MON', decimals: 18 },
  rpcUrls: { default: { http: ['https://testnet-rpc.monad.xyz'] } },
  blockExplorers: { default: { name: 'Monad Explorer', url: 'https://testnet.monadexplorer.com' } },
  testnet: true,
};

/** `defaultChain` first, then `supportedChains`, then `lowki.chains`; Sepolia if none. */
export function resolveChains(cfg: { defaultChain?: Chain; supportedChains?: Chain[]; lowkiChains?: Chain[] }): Chain[] {
  const out: Chain[] = [];
  for (const c of [cfg.defaultChain, ...(cfg.supportedChains ?? []), ...(cfg.lowkiChains ?? [])]) {
    if (c && !out.some((x) => x.id === c.id)) out.push(c);
  }
  return out.length ? out : [sepolia];
}

export const toHexChainId = (id: number): `0x${string}` => `0x${id.toString(16)}`;
export const caip2 = (id: number) => `eip155:${id}`;

/** Accepts `1`, `"1"`, `"0x1"`, `"eip155:1"`. */
export function parseChainId(v: unknown): number | null {
  if (typeof v === 'number' && Number.isSafeInteger(v) && v > 0) return v;
  if (typeof v === 'bigint' && v > 0n && v <= BigInt(Number.MAX_SAFE_INTEGER)) return Number(v);
  if (typeof v !== 'string') return null;
  const s = v.startsWith('eip155:') ? v.slice(7) : v;
  const n = /^0x[0-9a-fA-F]+$/.test(s) ? Number.parseInt(s, 16) : /^\d+$/.test(s) ? Number.parseInt(s, 10) : NaN;
  return Number.isSafeInteger(n) && n > 0 ? n : null;
}
