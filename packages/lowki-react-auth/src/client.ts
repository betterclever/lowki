/**
 * The framework-agnostic core: owns the transport, the session the wallet
 * announced, the EIP-1193 provider and the Privy-shaped `user` / `wallets`.
 * React binds to it with `useSyncExternalStore`.
 *
 * What it holds: the pairwise address, the pairwise user ID and the app
 * access token (in memory only; never written to storage). No key material
 * ever reaches this side.
 */
import { caip2, resolveChains } from './chains';
import { createIframeChannel, needsPopupForPasskeyCreate, openPopupChannel, windowHost } from './dom';
import { LowkiError, PrivyErrorCode, RpcErrorCode } from './errors';
import { jsonSafe, sameAddress, toWireTx } from './encoding';
import { LowkiEthereumProvider } from './provider';
import type { DelegationPolicy, WalletToDapp, WireTxRequest } from './protocol';
import { Transport, type Channel, type MessageHost } from './transport';
import type {
  ConnectedWallet,
  DelegationResult,
  Hex,
  LinkedAccountWithMetadata,
  PrivyClientConfig,
  PrivyEvents,
  SendPrivatelyInput,
  SendPrivatelyResult,
  SignTypedDataParams,
  UnsignedTransactionRequest,
  User,
  Wallet,
} from './types';

export const DEFAULT_WALLET_URL = 'https://wallet.lowki.money';

export interface ClientState {
  ready: boolean;
  authenticated: boolean;
  user: User | null;
  wallets: ConnectedWallet[];
  error: Error | null;
  modalOpen: boolean;
}

export interface Session {
  address: string;
  userId: string;
  accessToken: string;
  email?: string;
  createdAt: Date;
  isNewUser: boolean;
}

export interface ClientOptions {
  appId: string;
  config?: PrivyClientConfig;
  /** Injection points for tests and non-DOM hosts. */
  host?: MessageHost;
  appOrigin?: string;
  createIframe?: (src: string, onUserClose: () => void) => Channel;
  openPopup?: (src: string) => Channel | null;
  fetch?: typeof fetch;
  userAgent?: string;
}

type LoginComplete = Parameters<NonNullable<PrivyEvents['login']['onComplete']>>[0];
type Listener = () => void;

/** Privy config keys Lowki understands. Everything else gets a one-time dev warning. */
const SUPPORTED_KEYS = new Set(['appearance', 'loginMethods', 'embeddedWallets', 'supportedChains', 'defaultChain', 'lowki']);
const SUPPORTED_LOGIN = new Set(['email', 'passkey', 'google']);

const isDev = () => {
  const p = (globalThis as { process?: { env?: Record<string, string | undefined> } }).process;
  return p?.env?.NODE_ENV !== 'production';
};

export function configWarnings(cfg: PrivyClientConfig | undefined): string[] {
  if (!cfg) return [];
  const w: string[] = [];
  const ignored = Object.keys(cfg).filter((k) => !SUPPORTED_KEYS.has(k) && (cfg as Record<string, unknown>)[k] !== undefined);
  if (ignored.length) w.push(`config keys ignored by Lowki: ${ignored.join(', ')}`);
  const lm = (cfg.loginMethods ?? []).filter((m) => !SUPPORTED_LOGIN.has(m));
  if (lm.length) w.push(`login methods Lowki does not offer (the wallet shows passkey, email and Google): ${lm.join(', ')}`);
  const ew = cfg.embeddedWallets;
  const col = ew?.ethereum?.createOnLogin ?? ew?.createOnLogin;
  if (col === 'off') w.push('embeddedWallets.createOnLogin "off" is ignored: every Lowki user has a wallet for each app');
  if (ew?.solana) w.push('embeddedWallets.solana is ignored: Lowki is EVM-only for now');
  if (cfg.appearance?.walletChainType && cfg.appearance.walletChainType !== 'ethereum-only') w.push('appearance.walletChainType: only ethereum is supported');
  return w;
}

/** Decodes a JWT's `exp` without verifying it (the app's backend verifies). */
function jwtExp(token: string): number | null {
  const part = token.split('.')[1];
  if (!part) return null;
  try {
    const json = atob(part.replace(/-/g, '+').replace(/_/g, '/').padEnd(Math.ceil(part.length / 4) * 4, '='));
    const exp = (JSON.parse(json) as { exp?: unknown }).exp;
    return typeof exp === 'number' ? exp : null;
  } catch {
    return null;
  }
}

/**
 * `<meta name="lowki:wallet-url" content="http://localhost:8080/">` lets a page
 * point at a dev wallet without touching its Privy-shaped provider config.
 */
function metaWalletUrl(): string | undefined {
  if (typeof document === 'undefined') return undefined;
  const v = document.querySelector('meta[name="lowki:wallet-url"]')?.getAttribute('content')?.trim();
  return v || undefined;
}

let active: LowkiClient | null = null;
/** The most recently started client (what the standalone `getAccessToken` and the wagmi connector use). */
export const activeClient = () => active;
const activeWaiters = new Set<(c: LowkiClient) => void>();
export function whenActive(): Promise<LowkiClient> {
  if (active) return Promise.resolve(active);
  return new Promise((r) => activeWaiters.add(r));
}

export class LowkiClient {
  readonly appId: string;
  readonly walletUrl: string;
  readonly provider: LowkiEthereumProvider;
  private readonly opts: ClientOptions;
  private readonly cfg: PrivyClientConfig;
  private transport: Transport | null = null;
  private session: Session | null = null;
  private state: ClientState = { ready: false, authenticated: false, user: null, wallets: [], error: null, modalOpen: false };
  private readonly listeners = new Set<Listener>();
  private readonly loginListeners = new Set<PrivyEvents['login']>();
  private readonly logoutListeners = new Set<PrivyEvents['logout']>();
  private loginInFlight: Promise<void> | null = null;
  private walletObj: ConnectedWallet | null = null;
  private connectedAt = 0;

  constructor(opts: ClientOptions) {
    if (!opts.appId) throw new LowkiError(PrivyErrorCode.MISSING_OR_INVALID_PRIVY_APP_ID, 'PrivyProvider/LowkiProvider needs an appId');
    this.opts = opts;
    this.appId = opts.appId;
    this.cfg = opts.config ?? {};
    this.walletUrl = this.cfg.lowki?.walletUrl ?? metaWalletUrl() ?? DEFAULT_WALLET_URL;
    const chains = resolveChains({ defaultChain: this.cfg.defaultChain, supportedChains: this.cfg.supportedChains, lowkiChains: this.cfg.lowki?.chains });
    this.provider = new LowkiEthereumProvider(
      {
        address: () => this.session?.address ?? null,
        signMessage: (text) => this.rawSignMessage(text),
        signTypedData: (t) => this.rawSignTypedData(t),
        sendTransaction: (tx) => this.rawSendTransaction(tx),
        login: () => this.loginAsync(),
      },
      { chains, rpcUrls: this.cfg.lowki?.rpcUrls, fetch: opts.fetch },
    );
    this.provider.on('chainChanged', () => this.publish());
    if (isDev()) for (const w of configWarnings(this.cfg)) console.warn(`[lowki] ${w}`);
  }

  // ------------------------------------------------------------ store

  getState = (): ClientState => this.state;

  subscribe = (l: Listener): (() => void) => {
    this.listeners.add(l);
    return () => this.listeners.delete(l);
  };

  onLogin(cb: PrivyEvents['login']): () => void {
    this.loginListeners.add(cb);
    return () => this.loginListeners.delete(cb);
  }

  onLogout(cb: PrivyEvents['logout']): () => void {
    this.logoutListeners.add(cb);
    return () => this.logoutListeners.delete(cb);
  }

  private set(patch: Partial<ClientState>) {
    this.state = { ...this.state, ...patch };
    for (const l of [...this.listeners]) l();
  }

  // ------------------------------------------------------------ lifecycle

  /** Mounts the wallet iframe and restores an existing session if the wallet has one. */
  start(): void {
    if (this.transport) return;
    const host = this.opts.host ?? windowHost();
    const appOrigin = this.opts.appOrigin ?? window.location.origin;
    const theme = this.cfg.appearance?.theme;
    const lk = this.cfg.lowki ?? {};
    try {
      this.transport = new Transport({
        walletUrl: this.walletUrl,
        appId: this.appId,
        appOrigin,
        host,
        theme: theme === 'light' || theme === 'dark' ? theme : undefined,
        createIframe: (src, onClose) => this.tracked((this.opts.createIframe ?? createIframeChannel)(src, onClose)),
        openPopup: (src) => {
          const ch = (this.opts.openPopup ?? openPopupChannel)(src);
          return ch && this.tracked(ch);
        },
        handshakeTimeoutMs: lk.handshakeTimeoutMs,
        requestTimeoutMs: lk.requestTimeoutMs,
        onEvent: (m) => this.onWalletEvent(m),
        log: lk.debug ? (...a) => console.debug('[lowki]', ...a) : undefined,
      });
    } catch (e) {
      this.set({ ready: true, error: e as Error });
      return;
    }
    // eslint-disable-next-line @typescript-eslint/no-this-alias -- module-level registry for getAccessToken()
    active = this;
    for (const w of [...activeWaiters]) w(this);
    activeWaiters.clear();

    const t = this.transport;
    t.connect().then(
      async () => {
        if (lk.restoreSession !== false && !this.session) {
          // v1 has no silent session query: `sign_in` re-announces an existing
          // session at once, and otherwise just parks the (hidden) sign-in sheet.
          await t.request({ type: 'sign_in' }, { interactive: false, timeoutMs: 1500, errorKind: 'login' }).catch(() => undefined);
        }
        if (this.transport === t) this.set({ ready: true });
      },
      (e: Error) => {
        // Still "ready": the app can render its signed-out state, and login() can fall back to a popup.
        if (this.transport === t) this.set({ ready: true, error: e });
      },
    );
  }

  /** Mirrors the wallet's visibility into `modalOpen` (Privy's `isModalOpen`). */
  private tracked(ch: Channel): Channel {
    return {
      kind: ch.kind,
      window: () => ch.window(),
      show: () => {
        ch.show();
        if (!this.state.modalOpen) this.set({ modalOpen: true });
      },
      hide: () => {
        ch.hide();
        if (this.state.modalOpen) this.set({ modalOpen: false });
      },
      destroy: () => {
        ch.destroy();
        if (ch.kind === 'popup' && this.state.modalOpen) this.set({ modalOpen: false });
      },
    };
  }

  destroy(): void {
    this.transport?.destroy();
    this.transport = null;
    if (active === this) active = null;
  }

  private requireTransport(): Transport {
    if (!this.transport) throw new LowkiError(PrivyErrorCode.UNKNOWN_EMBEDDED_WALLET_ERROR, 'Lowki is not ready yet (render inside <PrivyProvider> and wait for `ready`)', { rpcCode: RpcErrorCode.DISCONNECTED, reason: 'not_ready' });
    return this.transport;
  }

  // ------------------------------------------------------------ session

  private onWalletEvent(m: WalletToDapp) {
    if (m.type === 'authenticated') {
      const prev = this.session;
      const same = prev && prev.userId === m.user_id && sameAddress(prev.address, m.address);
      this.session = {
        address: m.address,
        userId: m.user_id,
        accessToken: m.access_token,
        email: m.email ?? (same ? prev.email : undefined),
        createdAt: same ? prev.createdAt : m.created_at ? new Date(m.created_at) : new Date(),
        isNewUser: m.is_new_user ?? false,
      };
      if (!same) {
        this.walletObj = null;
        this.connectedAt = Date.now();
      }
      this.publish();
      if (!same) this.provider.emit('accountsChanged', [m.address]);
      if (!prev) this.provider.emit('connect', { chainId: `0x${this.provider.chainId.toString(16)}` });
    } else if (m.type === 'signed_out') {
      this.clearSession();
    }
  }

  private clearSession() {
    if (!this.session) return;
    this.session = null;
    this.walletObj = null;
    this.publish();
    this.provider.emit('accountsChanged', []);
    for (const l of [...this.logoutListeners]) l.onSuccess?.();
  }

  private publish() {
    const s = this.session;
    this.set({ authenticated: !!s, user: s ? this.buildUser(s) : null, wallets: s ? [this.wallet(s)] : [] });
  }

  private walletRecord(s: Session): Wallet {
    return {
      id: null,
      address: s.address,
      chainType: 'ethereum',
      walletClientType: 'privy',
      connectorType: 'embedded',
      recoveryMethod: 'privy',
      imported: false,
      delegated: false,
      userCanSign: true,
      walletIndex: 0,
    };
  }

  private buildUser(s: Session): User {
    const wallet = this.walletRecord(s);
    const linked: LinkedAccountWithMetadata[] = [{ ...wallet, type: 'wallet', firstVerifiedAt: s.createdAt, latestVerifiedAt: s.createdAt }];
    if (s.email) linked.push({ type: 'email', address: s.email, firstVerifiedAt: s.createdAt, latestVerifiedAt: s.createdAt });
    return {
      id: s.userId,
      createdAt: s.createdAt,
      ...(s.email ? { email: { address: s.email } } : {}),
      wallet,
      linkedAccounts: linked,
      mfaMethods: [],
      hasAcceptedTerms: false,
      isGuest: false,
    };
  }

  /** The app's one wallet, as Privy's `ConnectedWallet`. A new object when the chain changes, so React re-renders. */
  private wallet(s: Session): ConnectedWallet {
    const chainId = caip2(this.provider.chainId);
    if (this.walletObj && this.walletObj.address === s.address && this.walletObj.chainId === chainId) return this.walletObj;
    const provider = this.provider;
    const w: ConnectedWallet = {
      type: 'ethereum',
      address: s.address,
      chainId,
      connectedAt: this.connectedAt || Date.now(),
      walletClientType: 'privy',
      connectorType: 'embedded',
      imported: false,
      walletIndex: 0,
      linked: true,
      meta: { name: 'Lowki', id: 'lowki' },
      isConnected: async () => this.session?.address === s.address,
      disconnect: () => void this.logout(),
      switchChain: async (target) => provider.switchChain(target),
      getEthereumProvider: async () => provider,
      sign: async (message) => (await this.signMessage({ message }, { address: s.address })).signature,
      loginOrLink: async () => this.loginAsync(),
      unlink: async () => {
        throw new LowkiError(PrivyErrorCode.NOT_SUPPORTED, 'The Lowki wallet cannot be unlinked from its user');
      },
      fund: async () => {
        throw new LowkiError(PrivyErrorCode.NOT_SUPPORTED, 'Funding flows are not supported by Lowki yet');
      },
    };
    this.walletObj = w;
    return w;
  }

  // ------------------------------------------------------------ auth

  /**
   * Opens Lowki sign-in. Call from a click handler: on Safari (or if the
   * iframe didn't load) this opens a popup, which browsers only allow inside
   * a user gesture.
   */
  login(): void {
    this.loginAsync().catch(() => undefined);
  }

  loginAsync(): Promise<void> {
    if (this.session) {
      const user = this.buildUser(this.session);
      for (const l of [...this.loginListeners]) {
        l.onComplete?.({ user, isNewUser: false, wasAlreadyAuthenticated: true, loginMethod: null, loginAccount: null });
      }
      return Promise.resolve();
    }
    if (this.loginInFlight) return this.loginInFlight;
    let p: Promise<void>;
    try {
      p = this.doLogin();
    } catch (e) {
      p = Promise.reject(e);
    }
    this.loginInFlight = p
      .catch((e: unknown) => {
        const err = e instanceof LowkiError ? e : new LowkiError(PrivyErrorCode.UNKNOWN_AUTH_ERROR, String(e));
        for (const l of [...this.loginListeners]) l.onError?.(err.code as never);
        throw err;
      })
      .finally(() => {
        this.loginInFlight = null;
      });
    return this.loginInFlight;
  }

  /** Synchronous up to `openPopup`, so the popup counts as user-initiated. */
  private doLogin(): Promise<void> {
    const t = this.requireTransport();
    const mode = this.cfg.lowki?.popup ?? 'auto';
    const iframe = t.iframeStatus();
    const iframeBroken = !!iframe?.failed;
    const popup = mode === 'always' || (mode === 'auto' && (iframeBroken || needsPopupForPasskeyCreate(this.opts.userAgent)));
    const via = popup ? t.openPopup() : undefined;
    return t.request({ type: 'sign_in' }, { interactive: true, via, errorKind: 'login' }).then((m) => {
      if (m.type !== 'authenticated') return;
      this.onWalletEvent(m); // also delivered via onEvent; idempotent
      const s = this.session!;
      const user = this.buildUser(s);
      const loginAccount = user.linkedAccounts[0] ?? null;
      for (const l of [...this.loginListeners]) {
        l.onComplete?.({ user, isNewUser: s.isNewUser, wasAlreadyAuthenticated: false, loginMethod: null, loginAccount } satisfies LoginComplete);
      }
    });
  }

  async logout(): Promise<void> {
    const t = this.transport;
    if (t && t.iframeStatus()?.info) {
      await t.request({ type: 'sign_out' }, { interactive: false, timeoutMs: 10_000 }).catch(() => undefined);
    }
    // Clear locally even if the wallet didn't answer.
    this.clearSession();
  }

  /**
   * The current Lowki access token (ES256 JWT, `sub` = pairwise ID, `aud` =
   * app ID), or null when signed out. If it has expired, asks the wallet to
   * re-announce the session (which refreshes it) before giving up.
   */
  async getAccessToken(): Promise<string | null> {
    const s = this.session;
    if (!s) return null;
    const exp = jwtExp(s.accessToken);
    if (exp === null || exp - 30 > Date.now() / 1000) return s.accessToken;
    const t = this.transport;
    if (!t) return null;
    try {
      const m = await t.request({ type: 'sign_in' }, { interactive: false, timeoutMs: 5000, errorKind: 'login' });
      if (m.type === 'authenticated') {
        this.onWalletEvent(m);
        return m.access_token;
      }
    } catch {
      /* fall through */
    }
    return null;
  }

  // ------------------------------------------------------------ signing

  private checkAddress(address?: string) {
    const s = this.session;
    if (!s) throw new LowkiError(PrivyErrorCode.MUST_BE_AUTHENTICATED, 'Not signed in to Lowki', { rpcCode: RpcErrorCode.UNAUTHORIZED });
    if (address && !sameAddress(address, s.address)) {
      throw new LowkiError(PrivyErrorCode.EMBEDDED_WALLET_NOT_FOUND, `No Lowki wallet with address ${address}`, { rpcCode: RpcErrorCode.UNAUTHORIZED });
    }
  }

  /**
   * Requests go to the iframe. If it never loaded (blocked third-party frame,
   * CSP), each request opens the wallet in a popup instead; that only works
   * when the call comes from a click handler.
   */
  private via(t: Transport): Channel | undefined {
    return t.iframeStatus()?.failed && this.cfg.lowki?.popup !== 'never' ? t.openPopup() : undefined;
  }

  private async rawSignMessage(message: string): Promise<Hex> {
    this.checkAddress();
    const t = this.requireTransport();
    const m = await t.request({ type: 'sign_message', message }, { interactive: true, via: this.via(t), errorKind: 'sign' });
    return (m as Extract<WalletToDapp, { type: 'message_signed' }>).signature as Hex;
  }

  private async rawSignTypedData(typed: unknown): Promise<Hex> {
    this.checkAddress();
    const t = this.requireTransport();
    const m = await t.request({ type: 'sign_typed_data', typed_data: typed }, { interactive: true, via: this.via(t), errorKind: 'sign' });
    return (m as Extract<WalletToDapp, { type: 'typed_data_signed' }>).signature as Hex;
  }

  private async rawSendTransaction(tx: WireTxRequest): Promise<Hex> {
    this.checkAddress();
    const t = this.requireTransport();
    const m = await t.request({ type: 'send_transaction', tx }, { interactive: true, via: this.via(t), errorKind: 'transaction' });
    return (m as Extract<WalletToDapp, { type: 'transaction_sent' }>).hash as Hex;
  }

  /** Privy `signMessage`: signs the string as given (UTF-8, EIP-191). */
  async signMessage(input: { message: string }, options?: { address?: string }): Promise<{ signature: string }> {
    this.checkAddress(options?.address);
    if (typeof input?.message !== 'string') throw new LowkiError(PrivyErrorCode.INVALID_MESSAGE, 'message must be a string');
    return { signature: await this.rawSignMessage(input.message) };
  }

  async signTypedData(input: SignTypedDataParams, options?: { address?: string }): Promise<{ signature: string }> {
    this.checkAddress(options?.address);
    return { signature: await this.rawSignTypedData(jsonSafe(input)) };
  }

  async sendTransaction(input: UnsignedTransactionRequest, options?: { address?: string }): Promise<{ hash: Hex }> {
    this.checkAddress(options?.address ?? input?.from);
    if (input?.chainId !== undefined && input.chainId !== this.provider.chainId) this.provider.switchChain(input.chainId);
    return { hash: await this.rawSendTransaction(toWireTx(input)) };
  }

  /** Lowki-only: a private transfer from the user's private balance (Privacy Pools, proved in the browser). */
  async sendPrivately(input: SendPrivatelyInput): Promise<SendPrivatelyResult> {
    this.checkAddress();
    if (!input || typeof input.to !== 'string' || !input.to) throw new LowkiError(PrivyErrorCode.INVALID_DATA, 'sendPrivately: `to` is required', { rpcCode: RpcErrorCode.INVALID_PARAMS });
    if (typeof input.amount !== 'string' || !/^\d+(\.\d+)?$/.test(input.amount) || /^0+(\.0+)?$/.test(input.amount)) {
      throw new LowkiError(PrivyErrorCode.INVALID_DATA, 'sendPrivately: `amount` must be a positive decimal string such as "12.5"', { rpcCode: RpcErrorCode.INVALID_PARAMS });
    }
    const asset = input.asset ?? this.provider.chain.nativeCurrency.symbol;
    const t = this.requireTransport();
    const m = await t.request({ type: 'send_privately', to: input.to, amount: input.amount, asset }, { interactive: true, via: this.via(t), errorKind: 'transaction' });
    return { hash: (m as { hash: string }).hash as Hex };
  }

  /** Lowki-only: ask the user for a policy-limited signer (Privy "session signers"). */
  async requestDelegation(policy: DelegationPolicy): Promise<DelegationResult> {
    this.checkAddress();
    const t = this.requireTransport();
    const m = await t.request({ type: 'request_delegation', policy }, { interactive: true, via: this.via(t), errorKind: 'other' });
    const d = m as Extract<WalletToDapp, { type: 'delegation_approved' }>;
    return { delegationId: d.delegation_id, expires: d.expires, attestationBuild: d.attestation_build };
  }

  /** Current embedded wallet record (or null). */
  get embeddedWallet(): Wallet | null {
    return this.session ? this.walletRecord(this.session) : null;
  }
}
