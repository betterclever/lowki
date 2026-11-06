/**
 * @lowki/react-auth: a drop-in for `@privy-io/react-auth`.
 *
 *   - import { PrivyProvider, usePrivy } from '@privy-io/react-auth';
 *   + import { PrivyProvider, usePrivy } from '@lowki/react-auth';
 */
export {
  PrivyProvider,
  LowkiProvider,
  usePrivy,
  useWallets,
  useLogin,
  useLogout,
  useSignMessage,
  useSignTypedData,
  useSendTransaction,
  useSendPrivately,
  useRequestDelegation,
  useUser,
  useCreateWallet,
  useActiveWallet,
  useToken,
  useIdentityToken,
  useModalStatus,
  getAccessToken,
  getEmbeddedConnectedWallet,
} from './react';
export { LowkiClient, DEFAULT_WALLET_URL, type ClientState, type ClientOptions } from './client';
export { LowkiEthereumProvider, type ProviderBackend } from './provider';
export { LowkiError, ProviderRpcError, PrivyErrorCode, RpcErrorCode } from './errors';
export { sepolia, monadTestnet } from './chains';
export { PROTOCOL_VERSION, parseWalletMessage, type DappToWallet, type WalletToDapp, type RejectReason } from './protocol';
export type * from './types';

export const VERSION = '0.1.0';
