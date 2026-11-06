// Lowki-only: the one API Privy doesn't have. Kept out of src/app so the
// shared files still type-check against @privy-io/react-auth.
import { useState } from 'react';
import { usePrivy, useSendPrivately, type LowkiError } from '@lowki/react-auth';

export default function PrivateSend() {
  const { authenticated } = usePrivy();
  const { sendPrivately } = useSendPrivately();
  const [to, setTo] = useState('0x4b0897b0513fdc7c541b6d9d7e929c4e5364d2db');
  // Privacy Pools on Sepolia holds ETH.
  const [amount, setAmount] = useState('0.001');
  const [asset, setAsset] = useState('ETH');
  const [hash, setHash] = useState('');
  const [error, setError] = useState('');
  if (!authenticated) return null;
  const go = async () => {
    setError('');
    setHash('');
    try {
      const r = await sendPrivately({ to, amount, asset });
      setHash(r.hash);
    } catch (e) {
      const err = e as LowkiError;
      setError(`${err.code}: ${err.message}`);
    }
  };
  return (
    <section>
      <h2>Send privately (Lowki)</h2>
      <input data-testid="private-to" value={to} onChange={(e) => setTo(e.target.value)} />
      <input data-testid="private-amount" value={amount} onChange={(e) => setAmount(e.target.value)} />
      <input data-testid="private-asset" value={asset} onChange={(e) => setAsset(e.target.value)} size={6} />
      <button data-testid="private-send" onClick={go}>
        Send {amount} {asset} privately
      </button>
      <pre data-testid="private-hash">{hash}</pre>
      <pre data-testid="private-error">{error}</pre>
    </section>
  );
}
