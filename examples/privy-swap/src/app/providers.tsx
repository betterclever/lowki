'use client';

import { PrivyProvider } from '@privy-io/react-auth';
import { sepolia, baseSepolia } from 'viem/chains';

// `?app=<id>` lets one build run against any registered app (the real lowki-server issues
// `app_…` IDs at registration); otherwise the build-time ID.
const appId = new URLSearchParams(window.location.search).get('app') ?? import.meta.env.VITE_PRIVY_APP_ID ?? 'privy-swap';

// As in Privy's docs: https://docs.privy.io/basics/react/setup
export default function Providers({ children }: { children: React.ReactNode }) {
  return (
    <PrivyProvider
      appId={appId}
      config={{
        // Create embedded wallets for users who don't have a wallet
        embeddedWallets: {
          ethereum: {
            createOnLogin: 'users-without-wallets',
          },
        },
        loginMethods: ['email', 'passkey'],
        appearance: {
          theme: 'light',
          accentColor: '#676FFF',
          logo: 'https://your-logo-url',
        },
        defaultChain: sepolia,
        supportedChains: [sepolia, baseSepolia],
      }}
    >
      {children}
    </PrivyProvider>
  );
}
