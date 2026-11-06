/**
 * Public types, shaped like `@privy-io/react-auth` (v3) so that app code
 * written against Privy type-checks unchanged. Only the subset Lowki
 * implements is typed precisely; Privy config keys Lowki ignores are accepted
 * loosely so an existing `config={{ ... }}` literal still compiles.
 */
import type { MouseEvent, ReactElement, ReactNode } from 'react';
import type { PrivyErrorCode } from './errors';
import type { DelegationPolicy } from './protocol';

export type Hex = `0x${string}`;
export type Quantity = string | number | bigint;

/** Structural subset of viem's `Chain`, so `import { sepolia } from 'viem/chains'` fits. */
export interface Chain {
  id: number;
  name: string;
  nativeCurrency: { name: string; symbol: string; decimals: number };
  rpcUrls: { default: { http: readonly string[] }; [key: string]: { http: readonly string[] } | undefined };
  blockExplorers?: { default: { name: string; url: string } } & Record<string, { name: string; url: string } | undefined>;
  testnet?: boolean;
}

// ---------------------------------------------------------------- user

export type LinkedAccountType = 'wallet' | 'email' | 'passkey' | (string & {});

interface LinkMetadata {
  type: LinkedAccountType;
  firstVerifiedAt: Date | null;
  latestVerifiedAt: Date | null;
}

export interface Wallet {
  id?: string | null;
  address: string;
  chainType: 'ethereum' | 'solana';
  /** `'privy'` for drop-in compatibility: apps filter embedded wallets with it. */
  walletClientType?: string;
  connectorType?: string;
  recoveryMethod?: 'privy' | 'user-passcode' | 'google-drive' | 'icloud' | 'recovery-encryption-key' | 'privy-v2';
  imported: boolean;
  delegated: boolean;
  userCanSign: boolean;
  walletIndex: number | null;
  publicKey?: string;
}

export interface Email {
  address: string;
}

export interface Passkey {
  credentialId: string;
  enrolledInMfa: boolean;
  authenticatorName?: string;
  createdWithDevice?: string;
  createdWithOs?: string;
  createdWithBrowser?: string;
}

export interface WalletWithMetadata extends LinkMetadata, Wallet {
  type: 'wallet';
}
export interface EmailWithMetadata extends LinkMetadata, Email {
  type: 'email';
}
export interface PasskeyWithMetadata extends LinkMetadata, Passkey {
  type: 'passkey';
}

// Account kinds Lowki never links. Typed as in Privy so app code that checks
// for them (`a.type === 'google_oauth'`, `user.google?.email`) still compiles.
export interface Phone { number: string }
export interface SmartWallet { address: string; smartWalletType: string; smartWalletVersion?: string }
export interface Google { subject: string; email: string; name: string | null }
export interface Twitter { subject: string; username: string | null; name: string | null; profilePictureUrl: string | null }
export interface Discord { subject: string; username: string | null; email: string | null }
export interface Github { subject: string; username: string | null; name: string | null; email: string | null }
export interface Spotify { subject: string; email: string | null; name: string | null }
export interface Instagram { subject: string; username: string | null }
export interface Tiktok { subject: string; username: string | null; name: string | null }
export interface Line { subject: string; name: string | null; email: string | null; profilePictureUrl: string | null }
export interface Twitch { subject: string; username: string | null }
export interface LinkedIn { subject: string; name: string | null; email: string | null; vanityName: string | null }
export interface Apple { subject: string; email: string }
export interface Farcaster {
  fid: number | null;
  ownerAddress: string;
  username: string | null;
  displayName: string | null;
  bio: string | null;
  pfp: string | null;
  url: string | null;
  signerPublicKey: string | null;
}
export interface Telegram { telegramUserId: string; firstName: string | null; lastName: string | null; username: string | null; photoUrl: string | null }

export interface PhoneWithMetadata extends LinkMetadata, Phone { type: 'phone' }
export interface SmartWalletWithMetadata extends LinkMetadata, SmartWallet { type: 'smart_wallet' }
export interface GoogleOAuthWithMetadata extends LinkMetadata, Google { type: 'google_oauth' }
export interface TwitterOAuthWithMetadata extends LinkMetadata, Twitter { type: 'twitter_oauth' }
export interface DiscordOAuthWithMetadata extends LinkMetadata, Discord { type: 'discord_oauth' }
export interface GithubOAuthWithMetadata extends LinkMetadata, Github { type: 'github_oauth' }
export interface SpotifyOAuthWithMetadata extends LinkMetadata, Spotify { type: 'spotify_oauth' }
export interface InstagramOAuthWithMetadata extends LinkMetadata, Instagram { type: 'instagram_oauth' }
export interface TiktokOAuthWithMetadata extends LinkMetadata, Tiktok { type: 'tiktok_oauth' }
export interface LineOAuthWithMetadata extends LinkMetadata, Line { type: 'line_oauth' }
export interface TwitchOAuthWithMetadata extends LinkMetadata, Twitch { type: 'twitch_oauth' }
export interface LinkedInOAuthWithMetadata extends LinkMetadata, LinkedIn { type: 'linkedin_oauth' }
export interface AppleOAuthWithMetadata extends LinkMetadata, Apple { type: 'apple_oauth' }
export interface FarcasterWithMetadata extends LinkMetadata, Farcaster { type: 'farcaster' }
export interface TelegramWithMetadata extends LinkMetadata, Telegram { type: 'telegram' }

/** Lowki fills `wallet`, `email` (only if shared) and `passkey`; the rest exist for type compatibility. */
export type LinkedAccountWithMetadata =
  | WalletWithMetadata
  | EmailWithMetadata
  | PasskeyWithMetadata
  | PhoneWithMetadata
  | SmartWalletWithMetadata
  | GoogleOAuthWithMetadata
  | TwitterOAuthWithMetadata
  | DiscordOAuthWithMetadata
  | GithubOAuthWithMetadata
  | SpotifyOAuthWithMetadata
  | InstagramOAuthWithMetadata
  | TiktokOAuthWithMetadata
  | LineOAuthWithMetadata
  | TwitchOAuthWithMetadata
  | LinkedInOAuthWithMetadata
  | AppleOAuthWithMetadata
  | FarcasterWithMetadata
  | TelegramWithMetadata;

export type MfaMethod = 'sms' | 'totp' | 'passkey';
export type LoginMethod = 'email' | 'passkey' | 'google' | (string & {});

export interface User {
  /** Pairwise user ID (`did:lowki:…`): stable for this app, unlinkable across apps. */
  id: string;
  createdAt: Date;
  /** Present only if the user chose to share their email with this app. */
  email?: Email;
  wallet?: Wallet;
  // Never set by Lowki; present so Privy-era code compiles.
  phone?: Phone;
  smartWallet?: SmartWallet;
  google?: Google;
  twitter?: Twitter;
  discord?: Discord;
  github?: Github;
  spotify?: Spotify;
  instagram?: Instagram;
  tiktok?: Tiktok;
  line?: Line;
  twitch?: Twitch;
  linkedin?: LinkedIn;
  apple?: Apple;
  farcaster?: Farcaster;
  telegram?: Telegram;
  linkedAccounts: Array<LinkedAccountWithMetadata>;
  mfaMethods: Array<MfaMethod>;
  hasAcceptedTerms: boolean;
  isGuest: boolean;
  customMetadata?: Record<string, string | number | boolean>;
}

// ---------------------------------------------------------------- wallets

export type EIP1193OnEventHandler = (...args: any[]) => void; // eslint-disable-line @typescript-eslint/no-explicit-any

export interface EIP1193Provider {
  rpcTimeoutDuration?: number;
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  request: (request: { method: string; params?: Array<any> | undefined }) => Promise<any>;
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  on: (eventName: string, listener: EIP1193OnEventHandler) => any;
  // eslint-disable-next-line @typescript-eslint/no-explicit-any
  removeListener: (eventName: string | symbol, listener: (...args: any[]) => void) => any;
}

export interface ConnectedWalletMetadata {
  name: string;
  id: string;
  icon?: string;
}

export interface FundWalletConfig {
  chain?: Chain;
  amount?: string;
  asset?: 'native-currency' | 'USDC' | { erc20: Hex };
}

export interface BaseConnectedWallet {
  type: 'ethereum' | 'solana';
  address: string;
  connectedAt: number;
  walletClientType: string;
  connectorType: string;
  imported: boolean;
  meta: ConnectedWalletMetadata;
  isConnected: () => Promise<boolean>;
  disconnect: () => void;
}

export interface BaseConnectedEthereumWallet extends BaseConnectedWallet {
  type: 'ethereum';
  /** CAIP-2, e.g. `eip155:11155111`. */
  chainId: string;
  switchChain: (targetChainId: Hex | number) => Promise<void>;
  getEthereumProvider: () => Promise<EIP1193Provider>;
  sign: (message: string) => Promise<string>;
}

export interface ConnectedWallet extends BaseConnectedEthereumWallet {
  linked: boolean;
  loginOrLink: () => Promise<void>;
  unlink: () => Promise<void>;
  walletIndex?: number;
  fund: (fundWalletConfig?: FundWalletConfig) => Promise<void>;
}

export interface UseWalletsInterface {
  wallets: ConnectedWallet[];
  ready: boolean;
}

// ---------------------------------------------------------------- requests

export type UnsignedTransactionRequest = {
  from?: string;
  to?: string;
  nonce?: Quantity;
  gasLimit?: Quantity;
  gasPrice?: Quantity;
  data?: ArrayLike<number> | string;
  value?: Quantity;
  chainId?: number;
  type?: number;
  accessList?: Array<{ address: string; storageKeys: Array<string> }> | Array<[string, Array<string>]> | Record<string, Array<string>>;
  maxPriorityFeePerGas?: Quantity;
  maxFeePerGas?: Quantity;
};

export type MessageTypeProperty = { name: string; type: string };
export type MessageTypes = { [additionalProperties: string]: MessageTypeProperty[] };
export type TypedMessage<T extends MessageTypes> = {
  types: T;
  primaryType: keyof T;
  domain: { name?: string; version?: string; chainId?: number; verifyingContract?: string; salt?: ArrayBuffer };
  message: Record<string, unknown>;
};
export type SignTypedDataParams = TypedMessage<MessageTypes>;

/** Accepted for compatibility. The Lowki wallet always shows its own confirmation sheet. */
export type SignMessageModalUIOptions = {
  showWalletUIs?: boolean;
  title?: string;
  description?: string;
  buttonText?: string;
  iconUrl?: string;
  isCancellable?: boolean;
};
export type SendTransactionModalUIOptions = SignMessageModalUIOptions & {
  header?: string;
  transactionInfo?: { title?: string; action?: string; contractInfo?: unknown };
  successHeader?: string;
  successDescription?: string;
};

export interface SendPrivatelyInput {
  /** Recipient: an address (or, when the wallet supports it, an ENS name). */
  to: string;
  /** Decimal amount in whole units of `asset`, e.g. `"12.5"`. */
  amount: string;
  /** Token symbol (`"ETH"`, `"USDC"`) or ERC-20 address. Defaults to the chain's native currency. */
  asset?: string;
}

export interface SendPrivatelyResult {
  hash: Hex;
}

export interface DelegationResult {
  delegationId: string;
  expires: string;
  attestationBuild: string;
}
export type { DelegationPolicy };

// ---------------------------------------------------------------- config

export type HexColor = `#${string}`;
export type EmbeddedWalletCreateOnLoginConfig = 'users-without-wallets' | 'all-users' | 'off';
export type PrivyLoginMethod =
  | 'wallet' | 'email' | 'sms' | 'google' | 'twitter' | 'discord' | 'github' | 'linkedin' | 'spotify'
  | 'instagram' | 'tiktok' | 'line' | 'twitch' | 'apple' | 'farcaster' | 'telegram' | 'passkey' | `privy:${string}`;

/** Lowki-only settings, under `config.lowki`. Privy ignores this key. */
export interface LowkiConfig {
  /**
   * Where the Lowki wallet is served. Defaults to the page's
   * `<meta name="lowki:wallet-url">`, then `https://wallet.lowki.money`.
   * Query parameters you add are kept (for local dev flags such as `speed=0`).
   * Must be https, except on localhost.
   */
  walletUrl?: string;
  /** Chains the EIP-1193 provider can switch to. Merged with `supportedChains`. */
  chains?: Chain[];
  /** Per-chain RPC override for read calls, e.g. a privacy-preserving relay. */
  rpcUrls?: Record<number, string>;
  /**
   * `auto` (default): iframe, but a popup for sign-in where the iframe can't
   * create a passkey (Safari) or didn't load. `popup`: always a popup for
   * sign-in. `iframe`: never a popup.
   */
  popup?: 'auto' | 'always' | 'never';
  /** How long an interactive request may stay open. Default 10 minutes. */
  requestTimeoutMs?: number;
  /** How long to wait for the wallet frame's `ready`. Default 15 s. */
  handshakeTimeoutMs?: number;
  /** Probe the wallet for an existing session on load. Default true. */
  restoreSession?: boolean;
  /** Log protocol traffic (never tokens) to the console. */
  debug?: boolean;
}

export interface PrivyClientConfig {
  appearance?: {
    theme?: 'light' | 'dark' | HexColor;
    accentColor?: HexColor;
    logo?: string | ReactElement;
    emailDomain?: string;
    landingHeader?: string;
    loginMessage?: string;
    showWalletLoginFirst?: boolean;
    walletList?: unknown[];
    walletChainType?: 'ethereum-only' | 'solana-only' | 'ethereum-and-solana';
  };
  loginMethods?: PrivyLoginMethod[];
  loginMethodsAndOrder?: { primary: unknown[]; overflow?: unknown[] };
  embeddedWallets?: {
    /** Privy v1/v2 shape. */
    createOnLogin?: EmbeddedWalletCreateOnLoginConfig;
    ethereum?: { createOnLogin?: EmbeddedWalletCreateOnLoginConfig };
    solana?: { createOnLogin?: EmbeddedWalletCreateOnLoginConfig };
    showWalletUIs?: boolean;
    requireUserPasswordOnCreate?: boolean;
    disableAutomaticMigration?: boolean;
    priceDisplay?: unknown;
  };
  supportedChains?: Chain[];
  defaultChain?: Chain;
  // Accepted and ignored (a dev warning names them):
  intl?: unknown;
  customAuth?: unknown;
  legal?: { termsAndConditionsUrl?: string | null; privacyPolicyUrl?: string | null };
  walletConnectCloudProjectId?: string;
  plugins?: unknown[];
  scriptNonce?: string;
  customOAuthRedirectUrl?: string;
  allowOAuthInEmbeddedBrowsers?: boolean;
  captchaEnabled?: boolean;
  externalWallets?: unknown;
  mfa?: unknown;
  passkeys?: unknown;
  fundingMethodConfig?: unknown;
  solana?: unknown;
  sessions?: unknown;
  /** Lowki-only settings. */
  lowki?: LowkiConfig;
}

export interface PrivyProviderProps {
  appId: string;
  /** Accepted for compatibility; Lowki has no client IDs. */
  clientId?: string;
  config?: PrivyClientConfig;
  /** Accepted for compatibility; use `config.lowki.walletUrl`. */
  apiUrl?: string;
  children: ReactNode;
}

export type LoginModalOptions = {
  loginMethods?: PrivyClientConfig['loginMethods'];
  prefill?: { type: 'email' | 'phone'; value: string };
  disableSignup?: boolean;
  walletChainType?: NonNullable<PrivyClientConfig['appearance']>['walletChainType'];
};

// ---------------------------------------------------------------- events

export type CallbackError<Details extends Record<string, unknown> | undefined = undefined> = Details extends Record<string, unknown>
  ? (error: PrivyErrorCode, details: Details) => void
  : (error: PrivyErrorCode) => void;

export interface PrivyEvents {
  login: {
    onComplete?: (params: {
      user: User;
      isNewUser: boolean;
      wasAlreadyAuthenticated: boolean;
      loginMethod: LoginMethod | null;
      loginAccount: LinkedAccountWithMetadata | null;
    }) => void;
    onError?: CallbackError;
  };
  logout: { onSuccess?: () => void };
  createWallet: { onSuccess?: (params: { wallet: Wallet }) => void; onError?: CallbackError };
  accessToken: { onAccessTokenGranted: (params: { accessToken: string }) => void; onAccessTokenRemoved: () => void };
  signMessage: { onSuccess?: (params: { signature: string }) => void; onError?: CallbackError };
  signTypedData: { onSuccess?: (params: { signature: string }) => void; onError?: CallbackError };
  sendTransaction: { onSuccess?: (params: { hash: Hex }) => void; onError?: CallbackError };
  sendPrivately: { onSuccess?: (params: SendPrivatelyResult) => void; onError?: CallbackError };
}

// ---------------------------------------------------------------- usePrivy

type ClickOrOptions<T> = T | MouseEvent<any, any>; // eslint-disable-line @typescript-eslint/no-explicit-any

export interface SignMessageOptions {
  uiOptions?: SignMessageModalUIOptions;
  address?: string;
}
export interface SendTransactionOptions {
  sponsor?: boolean;
  uiOptions?: SendTransactionModalUIOptions;
  fundWalletConfig?: FundWalletConfig;
  address?: string;
}

export interface PrivyInterface {
  ready: boolean;
  authenticated: boolean;
  user: User | null;
  error: Error | null;
  /** True while the Lowki wallet is shown (sign-in, or a request to approve). */
  isModalOpen: boolean;
  login: (options?: ClickOrOptions<LoginModalOptions>) => void;
  logout: () => Promise<void>;
  getAccessToken: () => Promise<string | null>;
  /** Not yet: opens nothing, warns in dev. */
  linkEmail: () => void;
  /** Not yet: opens nothing, warns in dev. */
  linkPasskey: (options?: { name?: string }) => void;
  /** Not yet: opens nothing, warns in dev. */
  linkGoogle: () => void;
  /** Lowki has no external-wallet connect; warns in dev. */
  connectWallet: (options?: ClickOrOptions<{ suggestedAddress?: string }>) => void;
  /** Lowki creates the wallet at sign-in; resolves with it. */
  createWallet: (options?: ClickOrOptions<{ createAdditional?: boolean }>) => Promise<Wallet>;
  signMessage: (input: { message: string }, options?: SignMessageOptions) => Promise<{ signature: string }>;
  signTypedData: (input: SignTypedDataParams, options?: SignMessageOptions) => Promise<{ signature: string }>;
  /** Privy v1 had this on `usePrivy`; v3 apps use `useSendTransaction`. Both work. */
  sendTransaction: (input: UnsignedTransactionRequest, options?: SendTransactionOptions) => Promise<{ hash: Hex }>;
}
