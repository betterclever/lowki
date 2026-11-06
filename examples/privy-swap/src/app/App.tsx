import { useState } from 'react';
import {
  getAccessToken,
  useLogin,
  useLogout,
  usePrivy,
  useSendTransaction,
  useSignMessage,
  useSignTypedData,
  useWallets,
  type ConnectedWallet,
} from '@privy-io/react-auth';
import { createWalletClient, custom, type Hex } from 'viem';
import { sepolia } from 'viem/chains';
import { buildSwap, ROUTER, USDC } from './swap';

export const SWAP_MESSAGE = 'Privy Swap wants you to sign in.\n\nNonce: 7f3a9c';

export default function App() {
  const { ready, authenticated, user } = usePrivy();
  const { wallets } = useWallets();
  const [log, setLog] = useState<string[]>([]);
  const say = (line: string) => setLog((l) => [line, ...l].slice(0, 20));

  const { login } = useLogin({
    onComplete: ({ user, isNewUser, wasAlreadyAuthenticated }) => {
      say(`login complete: ${user.id} (new: ${isNewUser}, already: ${wasAlreadyAuthenticated})`);
    },
    onError: (error) => say(`login error: ${error}`),
  });
  const { logout } = useLogout({ onSuccess: () => say('logged out') });

  // Disable login when Privy is not ready or the user is already authenticated
  const disableLogin = !ready || (ready && authenticated);

  const wallet = wallets.find((w) => w.walletClientType === 'privy');

  return (
    <main>
      <h1>Privy Swap</h1>
      <p data-testid="status">{!ready ? 'loading' : authenticated ? 'authenticated' : 'signed-out'}</p>
      {!authenticated ? (
        <button data-testid="login" disabled={disableLogin} onClick={login}>
          Log in
        </button>
      ) : (
        <>
          <dl>
            <dt>User ID</dt>
            <dd data-testid="user-id">{user?.id}</dd>
            <dt>Wallet</dt>
            <dd data-testid="address">{wallet?.address ?? user?.wallet?.address}</dd>
            <dt>Chain</dt>
            <dd data-testid="chain">{wallet?.chainId}</dd>
            <dt>Linked accounts</dt>
            <dd data-testid="linked">{user?.linkedAccounts.map((a) => a.type).join(', ')}</dd>
          </dl>
          {wallet && <Actions wallet={wallet} say={say} />}
          <button data-testid="logout" onClick={logout}>
            Log out
          </button>
        </>
      )}
      <ul data-testid="log">
        {log.map((l, i) => (
          <li key={i}>{l}</li>
        ))}
      </ul>
    </main>
  );
}

function Actions({ wallet, say }: { wallet: ConnectedWallet; say: (l: string) => void }) {
  const [signature, setSignature] = useState('');
  const [typedSignature, setTypedSignature] = useState('');
  const [hash, setHash] = useState('');
  const [token, setToken] = useState('');
  const [chainFromProvider, setChainFromProvider] = useState('');

  const { signMessage } = useSignMessage({
    onSuccess: ({ signature }) => say(`signed: ${signature.slice(0, 18)}…`),
    onError: (error) => say(`sign error: ${error}`),
  });
  const { signTypedData } = useSignTypedData();
  const { sendTransaction } = useSendTransaction({
    onSuccess: ({ hash }) => say(`sent: ${hash}`),
    onError: (error) => say(`send error: ${error}`),
  });

  const onSign = async () => {
    const uiOptions = { title: 'Sign in to Privy Swap', buttonText: 'Sign' };
    const { signature } = await signMessage({ message: SWAP_MESSAGE }, { uiOptions, address: wallet.address });
    setSignature(signature);
  };

  const onSignTyped = async () => {
    const { signature } = await signTypedData(
      {
        domain: { name: 'Privy Swap', version: '1', chainId: sepolia.id, verifyingContract: ROUTER },
        types: {
          Quote: [
            { name: 'sellToken', type: 'address' },
            { name: 'buyToken', type: 'address' },
            { name: 'sellAmount', type: 'uint256' },
          ],
        },
        primaryType: 'Quote',
        message: { sellToken: '0xfFf9976782d46CC05630D1f6eBAb18b2324d6B14', buyToken: USDC, sellAmount: '10000000000000000' },
      },
      { address: wallet.address },
    );
    setTypedSignature(signature);
  };

  const onSwap = async () => {
    const tx = buildSwap(wallet.address as Hex, '0.01', 0n);
    const { hash } = await sendTransaction({ to: tx.to, value: tx.value, data: tx.data, chainId: sepolia.id }, { address: wallet.address });
    setHash(hash);
  };

  const onViem = async () => {
    await wallet.switchChain(sepolia.id);
    const provider = await wallet.getEthereumProvider();
    const walletClient = createWalletClient({ account: wallet.address as Hex, chain: sepolia, transport: custom(provider) });
    setChainFromProvider(String(await walletClient.getChainId()));
  };

  const onToken = async () => {
    const accessToken = await getAccessToken();
    setToken(accessToken ?? '');
  };

  return (
    <section>
      <button data-testid="sign" onClick={() => onSign().catch((e: Error) => say(`sign failed: ${e.message}`))}>
        Sign message
      </button>
      <pre data-testid="signature">{signature}</pre>
      <button data-testid="sign-typed" onClick={() => onSignTyped().catch((e: Error) => say(`typed failed: ${e.message}`))}>
        Sign quote (EIP-712)
      </button>
      <pre data-testid="typed-signature">{typedSignature}</pre>
      <button data-testid="swap" onClick={() => onSwap().catch((e: Error) => say(`swap failed: ${e.message}`))}>
        Swap 0.01 ETH → USDC
      </button>
      <pre data-testid="tx-hash">{hash}</pre>
      <button data-testid="viem" onClick={() => onViem().catch((e: Error) => say(`viem failed: ${e.message}`))}>
        viem getChainId
      </button>
      <pre data-testid="viem-chain">{chainFromProvider}</pre>
      <button data-testid="token" onClick={() => onToken()}>
        Get access token
      </button>
      <pre data-testid="access-token">{token}</pre>
    </section>
  );
}
