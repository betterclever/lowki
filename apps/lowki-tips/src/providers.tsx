// Written exactly as Privy's docs show (https://docs.privy.io/basics/react/setup).
// This app was written for Privy. We changed one import:
//
// - import { PrivyProvider } from '@privy-io/react-auth';
// + import { PrivyProvider } from '@lowki/react-auth';
import { PrivyProvider } from '@lowki/react-auth';
import type { ReactNode } from 'react';
import { config, tipsChain } from './config';

export default function Providers({ children }: { children: ReactNode }) {
  const chain = tipsChain();
  return (
    <PrivyProvider
      appId={config.lowki.appId}
      config={{
        embeddedWallets: {
          ethereum: {
            createOnLogin: 'users-without-wallets',
          },
        },
        loginMethods: ['email', 'passkey'],
        appearance: {
          theme: 'light',
          accentColor: '#B9471A',
        },
        defaultChain: chain,
        supportedChains: [chain],
        // Privy ignores this key; Lowki reads the wallet location from it.
        lowki: { walletUrl: config.lowki.walletUrl },
      }}
    >
      {children}
    </PrivyProvider>
  );
}
