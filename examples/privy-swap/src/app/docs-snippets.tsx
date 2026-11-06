// Patterns lifted from Privy's docs, kept to exercise the API surface in the
// type check (both packages). Not rendered by the app.
import {
  PrivyErrorCode,
  getAccessToken,
  getEmbeddedConnectedWallet,
  usePrivy,
  useSendTransaction,
  useSignMessage,
  useSignTypedData,
  useUser,
  useWallets,
  type ConnectedWallet,
  type EIP1193Provider,
  type LinkedAccountWithMetadata,
  type PrivyClientConfig,
  type User,
  type WalletWithMetadata,
} from '@privy-io/react-auth';
import { createWalletClient, custom, type Hex } from 'viem';
import { base, sepolia } from 'viem/chains';

// A config object typed separately (common in Next.js apps).
export const privyConfig: PrivyClientConfig = {
  loginMethods: ['email', 'passkey', 'google'],
  appearance: { theme: 'dark', accentColor: '#676FFF', showWalletLoginFirst: false, walletChainType: 'ethereum-only' },
  embeddedWallets: { ethereum: { createOnLogin: 'all-users' }, showWalletUIs: true },
  defaultChain: base,
  supportedChains: [base, sepolia],
  legal: { termsAndConditionsUrl: 'https://example.com/terms', privacyPolicyUrl: 'https://example.com/privacy' },
  walletConnectCloudProjectId: 'abc',
};

export function useEmbeddedWallet(): ConnectedWallet | undefined {
  const { wallets } = useWallets();
  return wallets.find((wallet) => wallet.walletClientType === 'privy');
}

export function describeUser(user: User): string {
  const email = user.email?.address;
  const wallet = user.wallet?.address;
  const created = user.createdAt.toISOString();
  const emailAccount = user.linkedAccounts.find((a): a is LinkedAccountWithMetadata & { type: 'email' } => a.type === 'email');
  const wallets = user.linkedAccounts.filter((a): a is WalletWithMetadata => a.type === 'wallet' && a.walletClientType === 'privy');
  return [user.id, email, wallet, created, emailAccount?.address, wallets.length].join(' ');
}

export function Snippets() {
  const { ready, authenticated, user, login, logout, getAccessToken: tokenFromHook, linkEmail, linkPasskey, signMessage, signTypedData } = usePrivy();
  const { user: u2, refreshUser } = useUser();
  const { wallets, ready: walletsReady } = useWallets();
  const embedded = getEmbeddedConnectedWallet(wallets);
  const { signMessage: sign } = useSignMessage({ onError: (e) => e === PrivyErrorCode.USER_EXITED_AUTH_FLOW });
  const { signTypedData: signTyped } = useSignTypedData({ onSuccess: ({ signature }) => signature });
  const { sendTransaction } = useSendTransaction();

  async function everything(wallet: ConnectedWallet) {
    await wallet.switchChain(base.id);
    await wallet.switchChain('0x2105');
    const provider: EIP1193Provider = await wallet.getEthereumProvider();
    const accounts: string[] = await provider.request({ method: 'eth_requestAccounts' });
    const client = createWalletClient({ account: wallet.address as Hex, chain: base, transport: custom(provider) });
    await client.signMessage({ account: wallet.address as Hex, message: 'hi' });
    const chainId: string = wallet.chainId; // 'eip155:8453'
    const sig: string = await wallet.sign('hello');
    await sign({ message: 'hello' }, { uiOptions: { title: 'Sign', description: 'Please sign', buttonText: 'OK' }, address: wallet.address });
    await signTyped({ types: { Mail: [{ name: 'from', type: 'string' }] }, primaryType: 'Mail', domain: { name: 'x', chainId: 8453 }, message: { from: 'a' } });
    await signMessage({ message: 'via usePrivy' });
    await signTypedData({ types: {}, primaryType: 'x', domain: {}, message: {} });
    const { hash } = await sendTransaction(
      { to: '0xE3070d3e4309afA3bC9a6b057685743CF42da77C', value: 100000, chainId: base.id },
      { sponsor: true, uiOptions: { showWalletUIs: false }, address: wallet.address },
    );
    const token: string | null = (await getAccessToken()) ?? (await tokenFromHook());
    await fetch('/api/me', { headers: { Authorization: `Bearer ${token}` } });
    await refreshUser();
    return [accounts, chainId, sig, hash];
  }

  if (!ready || !walletsReady) return null;
  return (
    <div>
      {authenticated ? <button onClick={logout}>Log out</button> : <button onClick={login}>Log in</button>}
      <button onClick={() => login({ loginMethods: ['email'], prefill: { type: 'email', value: 'a@b.c' } })}>Email</button>
      <button onClick={linkEmail}>Link email</button>
      <button onClick={() => linkPasskey()}>Link passkey</button>
      <span>{user?.id ?? u2?.id}</span>
      {embedded && <button onClick={() => everything(embedded)}>Go</button>}
    </div>
  );
}
