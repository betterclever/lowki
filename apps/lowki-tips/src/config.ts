// The one place the app learns about the chain, ENS, the gateway and the wallet:
// config/<target>.json, where target = VITE_TIPS_TARGET ("fork" or "sepolia"; default "fork").
// Nothing chain-, ENS- or wallet-specific is written in the code.
import { defineChain, type Address, type Chain } from 'viem';

export interface Creator {
  label: string;
  displayName: string;
  tagline: string;
  /** Avatar hue (0-360). */
  hue: number;
}

export interface TipsConfig {
  target: string;
  chain: { id: number; name: string; rpcUrl: string; explorerTxUrl: string | null };
  ens: {
    universalResolver: Address;
    stealthResolver: Address;
    parentName: string;
    gatewayUrl: string;
    recordsUrl: string;
  };
  lowki: { appId: string; walletUrl: string };
  tip: { amountEth: string };
  creators: Creator[];
  /** Fork only: a faucet button and the documented demo creator seed phrase. */
  demo: { faucet: boolean; creatorSeedPhrase: string } | null;
}

const files = import.meta.glob<Record<string, unknown>>('../config/*.json', { eager: true, import: 'default' });
export const TARGET: string = import.meta.env.VITE_TIPS_TARGET ?? 'fork';

function missing(v: unknown, path = ''): string[] {
  if (v === null || v === undefined) return [path];
  if (typeof v === 'object' && !Array.isArray(v)) {
    return Object.entries(v as Record<string, unknown>).flatMap(([k, x]) =>
      k === 'demo' || k === 'explorerTxUrl' || k.startsWith('_') ? [] : missing(x, path ? `${path}.${k}` : k),
    );
  }
  return [];
}

function load(): { config: TipsConfig | null; error: string | null } {
  const raw = files[`../config/${TARGET}.json`];
  if (!raw) {
    return {
      config: null,
      error: `config/${TARGET}.json is missing. For the local fork, run scripts/demo-tips.sh (it writes it).`,
    };
  }
  const holes = missing(raw);
  if (holes.length) return { config: null, error: `config/${TARGET}.json has empty fields: ${holes.join(', ')}` };
  return { config: raw as unknown as TipsConfig, error: null };
}

const loaded = load();
export const configError = loaded.error;
export const config = loaded.config as TipsConfig;

export function ensName(c: Creator): string {
  return `${c.label}.${config.ens.parentName}`;
}

export function tipsChain(): Chain {
  return defineChain({
    id: config.chain.id,
    name: config.chain.name,
    nativeCurrency: { name: 'Ether', symbol: 'ETH', decimals: 18 },
    rpcUrls: { default: { http: [config.chain.rpcUrl] } },
    contracts: { ensUniversalResolver: { address: config.ens.universalResolver } },
  });
}
