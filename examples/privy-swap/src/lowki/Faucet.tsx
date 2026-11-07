// Lowki demo only: tops up this app's wallet on the local Sepolia fork, so the swap has ETH.
// The page's own origin proxies /rpc to the fork and allows only anvil_setBalance.
import { useState } from 'react';
import { usePrivy, useWallets } from '@privy-io/react-auth';

const ONE_ETH = '0xde0b6b3a7640000';

export default function Faucet() {
  const { authenticated } = usePrivy();
  const { wallets } = useWallets();
  const [status, setStatus] = useState('');
  const wallet = wallets[0];
  if (!authenticated || !wallet) return null;

  const request = async () => {
    setStatus('Requesting…');
    try {
      const res = await fetch('/rpc', {
        method: 'POST',
        headers: { 'content-type': 'application/json' },
        body: JSON.stringify({ jsonrpc: '2.0', id: 1, method: 'anvil_setBalance', params: [wallet.address, ONE_ETH] }),
      });
      const body = await res.json();
      setStatus(body.error ? `Failed: ${body.error.message}` : 'Funded with 1 test ETH');
    } catch (e) {
      setStatus(`Failed: ${(e as Error).message}`);
    }
  };

  return (
    <section>
      <button data-testid="faucet" onClick={() => void request()}>
        Request test ETH
      </button>
      <pre data-testid="faucet-status">{status}</pre>
    </section>
  );
}
