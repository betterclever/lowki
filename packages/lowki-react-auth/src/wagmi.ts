/**
 * wagmi connector for Lowki:
 *
 *   import { lowkiConnector } from '@lowki/react-auth/wagmi';
 *   createConfig({ connectors: [lowkiConnector()], chains: [sepolia], transports: { ... } });
 *
 * It drives the client mounted by `<PrivyProvider>` / `<LowkiProvider>`, so
 * render the provider above `WagmiProvider`'s consumers.
 */
import { createConnector } from '@wagmi/core';
import { activeClient, whenActive, type LowkiClient } from './client';
import type { LowkiEthereumProvider } from './provider';

type Addr = `0x${string}`;

export function lowkiConnector() {
  let client: LowkiClient | null = null;
  let unsub: (() => void) | null = null;

  return createConnector<LowkiEthereumProvider>((config) => {
    const provider = async () => (client ??= activeClient() ?? (await whenActive())).provider;
    const accounts = (): Addr[] => {
      const a = client?.getState().wallets[0]?.address;
      return a ? [a as Addr] : [];
    };

    return {
      id: 'lowki',
      name: 'Lowki',
      type: 'lowki',
      async setup() {
        client = activeClient();
      },
      async connect(params) {
        const p = await provider();
        if (!client!.getState().authenticated) await client!.loginAsync();
        if (params?.chainId && params.chainId !== p.chainId) p.switchChain(params.chainId);
        unsub?.();
        unsub = client!.subscribe(() => {
          const a = accounts();
          if (a.length) config.emitter.emit('change', { accounts: a });
          else config.emitter.emit('disconnect');
        });
        p.on('chainChanged', this.onChainChanged.bind(this));
        // wagmi 2 types `connect` with a conditional `withCapabilities` result; we return plain accounts.
        return { accounts: accounts(), chainId: p.chainId } as never;
      },
      async disconnect() {
        unsub?.();
        unsub = null;
        await client?.logout();
      },
      async getAccounts() {
        await provider();
        return accounts();
      },
      async getChainId() {
        return (await provider()).chainId;
      },
      async getProvider() {
        return provider();
      },
      async isAuthorized() {
        return accounts().length > 0;
      },
      async switchChain({ chainId }) {
        const p = await provider();
        p.switchChain(chainId);
        const chain = config.chains.find((c) => c.id === chainId);
        if (!chain) throw new Error(`Chain ${chainId} is not in the wagmi config`);
        return chain;
      },
      onAccountsChanged(a) {
        if (a.length === 0) config.emitter.emit('disconnect');
        else config.emitter.emit('change', { accounts: a as Addr[] });
      },
      onChainChanged(chain) {
        config.emitter.emit('change', { chainId: Number(chain) });
      },
      onDisconnect() {
        config.emitter.emit('disconnect');
      },
    };
  });
}
