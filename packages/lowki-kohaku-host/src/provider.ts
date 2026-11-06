import type { EthereumProvider } from "@kohaku-eth/provider";
import type { Network } from "@kohaku-eth/plugins";
import { viem } from "@kohaku-eth/provider/viem";
import { createPublicClient, fallback, http, type PublicClient } from "viem";

/**
 * Plain JSON-RPC provider: Kohaku's viem adapter (packages/provider/src/viem) over a viem
 * PublicClient whose HTTP transport fetches through the host `Network`, so the allowlist covers
 * RPC traffic too. Several URLs become a viem `fallback` transport (tried in order on error).
 * This RPC sees every read the plugin makes (pool/entrypoint log scans, which are user-neutral,
 * but also later per-user calls); Kohaku's anon/Helios/Colibri providers are the privacy
 * upgrade (see docs/ARCHITECTURE.md "Kohaku pieces to reuse").
 */
export function createRpcProvider(
  rpcUrls: string | readonly string[],
  network: Network,
  { retryCount = 3 }: { retryCount?: number } = {},
): EthereumProvider<PublicClient> {
  const urls = typeof rpcUrls === "string" ? [rpcUrls] : [...rpcUrls];
  const one = (url: string) => http(url, {
    fetchFn: (input, init) => network.fetch(input, init),
    retryCount,
    retryDelay: 500,
    timeout: 30_000,
  });
  const client = createPublicClient({
    transport: urls.length === 1 ? one(urls[0]!) : fallback(urls.map(one), { retryCount: 0 }),
    cacheTime: 0,
  });

  return viem(client as PublicClient);
}
