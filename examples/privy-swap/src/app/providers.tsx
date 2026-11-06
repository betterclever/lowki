'use client';

import { PrivyProvider } from '@privy-io/react-auth';
import { sepolia, baseSepolia } from 'viem/chains';

// As in Privy's docs: https://docs.privy.io/basics/react/setup
export default function Providers({ children }: { children: React.ReactNode }) {
  return (
    <PrivyProvider
      appId={import.meta.env.VITE_PRIVY_APP_ID ?? 'privy-swap'}
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
