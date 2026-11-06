import { createPublicClient, http, type Address, type Hex, type PublicClient } from 'viem';
import { config, tipsChain } from './config';

let client: PublicClient | null = null;

/** A plain viem client: no custom CCIP-Read handler, exactly what any wallet or app would use. */
export function publicClient(): PublicClient {
  client ??= createPublicClient({ chain: tipsChain(), transport: http(config.chain.rpcUrl) }) as PublicClient;
  return client;
}

/** Resolves `<label>.lowki.eth` with plain viem `getEnsAddress` (CCIP-Read through the Lowki gateway). */
export async function resolveName(name: string): Promise<Address> {
  const a = await publicClient().getEnsAddress({ name, universalResolverAddress: config.ens.universalResolver });
  if (!a) throw new Error(`${name} did not resolve`);
  return a;
}

export function balanceOf(address: Address): Promise<bigint> {
  return publicClient().getBalance({ address });
}

/** Fork only (config.demo.faucet): sets a test balance with anvil_setBalance. */
export async function faucet(address: Address, wei: bigint): Promise<void> {
  const current = await balanceOf(address);
  await publicClient().request({
    method: 'anvil_setBalance' as never,
    params: [address, `0x${(current + wei).toString(16)}` as Hex] as never,
  });
}

export function short(s: string, head = 6, tail = 4): string {
  return s.length > head + tail + 1 ? `${s.slice(0, head)}…${s.slice(-tail)}` : s;
}

export function txUrl(hash: string): string | null {
  return config.chain.explorerTxUrl ? `${config.chain.explorerTxUrl}${hash}` : null;
}
