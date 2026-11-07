// The Lowki build's entry: the same Providers and App as src/main.tsx (they
// import '@privy-io/react-auth', which vite aliases to '@lowki/react-auth'),
// plus the Lowki-only private send panel.
import { StrictMode } from 'react';
import { createRoot } from 'react-dom/client';
import Providers from '../app/providers';
import App from '../app/App';
import PrivateSend from './PrivateSend';
import Faucet from './Faucet';

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <Providers>
      <Faucet />
      <App />
      <PrivateSend />
    </Providers>
  </StrictMode>,
);
