import { createContext, useCallback, useContext, useEffect, useMemo, useRef, useState, useSyncExternalStore, type MouseEvent, type ReactNode } from 'react';
import { LowkiClient, activeClient, type ClientState } from './client';
import { LowkiError, PrivyErrorCode, RpcErrorCode } from './errors';
import type {
  ConnectedWallet,
  DelegationPolicy,
  DelegationResult,
  Hex,
  PrivyEvents,
  PrivyInterface,
  PrivyProviderProps,
  SendPrivatelyInput,
  SendPrivatelyResult,
  SendTransactionOptions,
  SignMessageOptions,
  SignTypedDataParams,
  UnsignedTransactionRequest,
  UseWalletsInterface,
  Wallet,
} from './types';

const Ctx = createContext<LowkiClient | null>(null);

const NOT_READY: ClientState = { ready: false, authenticated: false, user: null, wallets: [], error: null, modalOpen: false };
const noopSubscribe = () => () => undefined;
const notReadyState = () => NOT_READY;

/** A stable key for the parts of the config that need a new client when they change. */
function clientKey(appId: string, config: PrivyProviderProps['config']): string {
  const lk = config?.lowki ?? {};
  return JSON.stringify([
    appId,
    lk.walletUrl ?? null,
    lk.popup ?? null,
    lk.restoreSession ?? null,
    lk.rpcUrls ?? null,
    (config?.defaultChain ? [config.defaultChain] : []).concat(config?.supportedChains ?? [], lk.chains ?? []).map((c) => c.id),
    config?.appearance?.theme ?? null,
  ]);
}

/**
 * Drop-in for Privy's `PrivyProvider`. Mounts the Lowki wallet iframe on the
 * client (nothing renders on the server) and provides the hooks below.
 */
export function PrivyProvider({ appId, config, children }: PrivyProviderProps): ReactNode {
  const [client, setClient] = useState<LowkiClient | null>(null);
  const key = clientKey(appId, config);
  const configRef = useRef(config);
  // Runs before the effect below (effects run in order), so the client gets the latest config.
  useEffect(() => {
    configRef.current = config;
  });

  useEffect(() => {
    const c = new LowkiClient({ appId, config: configRef.current });
    setClient(c);
    c.start();
    return () => {
      c.destroy();
      setClient((cur) => (cur === c ? null : cur));
    };
    // `key` covers appId and the client-shaping config.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [key]);

  return <Ctx.Provider value={client}>{children}</Ctx.Provider>;
}

/** Same component as `PrivyProvider`, under Lowki's name. */
export const LowkiProvider = PrivyProvider;

function useClient(): LowkiClient | null {
  return useContext(Ctx);
}

function useClientState(): ClientState {
  const c = useClient();
  return useSyncExternalStore(c ? c.subscribe : noopSubscribe, c ? c.getState : notReadyState, notReadyState);
}

const notReady = () =>
  new LowkiError(PrivyErrorCode.UNKNOWN_EMBEDDED_WALLET_ERROR, 'Lowki is not ready yet: render inside <PrivyProvider> and wait for `ready`', {
    rpcCode: RpcErrorCode.DISCONNECTED,
    reason: 'not_ready',
  });

/** Keeps the latest callbacks without re-subscribing every render. */
function useLatest<T>(v: T) {
  const r = useRef(v);
  useEffect(() => {
    r.current = v;
  });
  return r;
}

const errCode = (e: unknown) => (e instanceof LowkiError ? e.code : PrivyErrorCode.UNKNOWN_EMBEDDED_WALLET_ERROR);

const warned = new Set<string>();
function notYet(name: string) {
  if (warned.has(name)) return;
  warned.add(name);
  console.warn(`[lowki] ${name} is not supported yet; it does nothing.`);
}

export function usePrivy(): PrivyInterface {
  const c = useClient();
  const s = useClientState();
  return useMemo<PrivyInterface>(() => {
    const need = () => {
      if (!c) throw notReady();
      return c;
    };
    return {
      ready: s.ready,
      authenticated: s.authenticated,
      user: s.user,
      error: s.error,
      isModalOpen: s.modalOpen,
      login: () => {
        if (!c) {
          console.warn('[lowki] login() called before ready');
          return;
        }
        c.login();
      },
      logout: async () => need().logout(),
      getAccessToken: async () => (c ? c.getAccessToken() : null),
      linkEmail: () => notYet('linkEmail'),
      linkPasskey: () => notYet('linkPasskey'),
      linkGoogle: () => notYet('linkGoogle'),
      connectWallet: () => notYet('connectWallet'),
      createWallet: async () => {
        const w: Wallet | null = c?.embeddedWallet ?? null;
        if (!w) throw new LowkiError(PrivyErrorCode.MUST_BE_AUTHENTICATED, 'Sign in first: the Lowki wallet is created at sign-in');
        return w;
      },
      signMessage: (input, options) => need().signMessage(input, options),
      signTypedData: (input, options) => need().signTypedData(input, options),
      sendTransaction: (input, options) => need().sendTransaction(input, options),
    };
  }, [c, s]);
}

export function useWallets(): UseWalletsInterface {
  const s = useClientState();
  return useMemo(() => ({ wallets: s.wallets, ready: s.ready }), [s.wallets, s.ready]);
}

/** Privy's `useLogin`: `onComplete` / `onError` fire for any sign-in, wherever it was started. */
export function useLogin(callbacks?: PrivyEvents['login']): { login: PrivyInterface['login'] } {
  const c = useClient();
  const cb = useLatest(callbacks);
  useEffect(() => {
    if (!c) return;
    return c.onLogin({
      onComplete: (p) => cb.current?.onComplete?.(p),
      onError: (e) => cb.current?.onError?.(e),
    });
  }, [c, cb]);
  const login = useCallback<PrivyInterface['login']>(() => {
    if (!c) {
      console.warn('[lowki] login() called before ready');
      return;
    }
    c.login();
  }, [c]);
  return { login };
}

export function useLogout(callbacks?: PrivyEvents['logout']): { logout: () => Promise<void> } {
  const c = useClient();
  const cb = useLatest(callbacks);
  useEffect(() => {
    if (!c) return;
    return c.onLogout({ onSuccess: () => cb.current?.onSuccess?.() });
  }, [c, cb]);
  const logout = useCallback(async () => {
    if (!c) throw notReady();
    await c.logout();
  }, [c]);
  return { logout };
}

/** Wraps a client action with Privy-style `onSuccess` / `onError` callbacks. */
function useAction<A extends unknown[], R>(
  run: (c: LowkiClient, ...a: A) => Promise<R>,
  callbacks: { onSuccess?: (r: R) => void; onError?: (code: PrivyErrorCode) => void } | undefined,
): (...a: A) => Promise<R> {
  const c = useClient();
  const cb = useLatest(callbacks);
  const runRef = useLatest(run);
  return useCallback(
    async (...a: A) => {
      try {
        if (!c) throw notReady();
        const r = await runRef.current(c, ...a);
        cb.current?.onSuccess?.(r);
        return r;
      } catch (e) {
        cb.current?.onError?.(errCode(e));
        throw e;
      }
    },
    [c, cb, runRef],
  );
}

export function useSignMessage(callbacks?: PrivyEvents['signMessage']): {
  signMessage: (input: { message: string }, options?: SignMessageOptions) => Promise<{ signature: string }>;
} {
  const signMessage = useAction((c, input: { message: string }, options?: SignMessageOptions) => c.signMessage(input, options), callbacks);
  return { signMessage };
}

export function useSignTypedData(callbacks?: PrivyEvents['signTypedData']): {
  signTypedData: (input: SignTypedDataParams, options?: SignMessageOptions) => Promise<{ signature: string }>;
} {
  const signTypedData = useAction((c, input: SignTypedDataParams, options?: SignMessageOptions) => c.signTypedData(input, options), callbacks);
  return { signTypedData };
}

export function useSendTransaction(callbacks?: PrivyEvents['sendTransaction']): {
  sendTransaction: (input: UnsignedTransactionRequest, options?: SendTransactionOptions) => Promise<{ hash: Hex }>;
} {
  const sendTransaction = useAction(
    (c, input: UnsignedTransactionRequest, options?: SendTransactionOptions) => c.sendTransaction(input, options),
    callbacks,
  );
  return { sendTransaction };
}

/** Lowki-only. A private transfer: the proof is built in the user's browser, and the recipient can't see the sender. */
export function useSendPrivately(callbacks?: PrivyEvents['sendPrivately']): {
  sendPrivately: (input: SendPrivatelyInput) => Promise<SendPrivatelyResult>;
} {
  const sendPrivately = useAction((c, input: SendPrivatelyInput) => c.sendPrivately(input), callbacks);
  return { sendPrivately };
}

/** Lowki-only. Ask the user to approve a policy-limited signer for this app account. */
export function useRequestDelegation(): { requestDelegation: (policy: DelegationPolicy) => Promise<DelegationResult> } {
  const requestDelegation = useAction((c, policy: DelegationPolicy) => c.requestDelegation(policy), undefined);
  return { requestDelegation };
}

/** Privy's `useUser`: the current user plus a `refreshUser` that re-reads the session. */
export function useUser(): { user: PrivyInterface['user']; refreshUser: () => Promise<PrivyInterface['user']> } {
  const c = useClient();
  const s = useClientState();
  const refreshUser = useCallback(async () => {
    if (c) await c.getAccessToken();
    return c?.getState().user ?? null;
  }, [c]);
  return { user: s.user, refreshUser };
}

/** Privy's standalone `getAccessToken()`, usable outside React (e.g. in a fetch wrapper). */
export async function getAccessToken(): Promise<string | null> {
  const c = activeClient();
  return c ? c.getAccessToken() : null;
}

/** Privy's `getEmbeddedConnectedWallet`. */
export function getEmbeddedConnectedWallet(wallets: ConnectedWallet[]): ConnectedWallet | null {
  return wallets.find((w) => w.walletClientType === 'privy' && w.connectorType === 'embedded') ?? null;
}

/** Privy's `useCreateWallet`. Lowki creates the app's wallet at sign-in, so this returns it. */
export function useCreateWallet(callbacks?: PrivyEvents['createWallet']): {
  createWallet: (options?: { createAdditional?: boolean } | MouseEvent<any, any>) => Promise<Wallet>; // eslint-disable-line @typescript-eslint/no-explicit-any
} {
  const createWallet = useAction(async (c, options?: { createAdditional?: boolean } | MouseEvent<any, any>) => { // eslint-disable-line @typescript-eslint/no-explicit-any
    if (options && 'createAdditional' in options && options.createAdditional) {
      throw new LowkiError(PrivyErrorCode.NOT_SUPPORTED, 'Lowki gives each app exactly one wallet per user');
    }
    const w = c.embeddedWallet;
    if (!w) throw new LowkiError(PrivyErrorCode.MUST_BE_AUTHENTICATED, 'Sign in first: the Lowki wallet is created at sign-in');
    return w;
  }, callbacks ? { onSuccess: (wallet: Wallet) => callbacks.onSuccess?.({ wallet }), onError: callbacks.onError } : undefined);
  return { createWallet };
}

/** Privy's `useActiveWallet`. There is one Lowki wallet per app, so it is always the active one. */
export function useActiveWallet(): {
  wallet: ConnectedWallet | undefined;
  network: string | undefined;
  setActiveWallet: (wallet: ConnectedWallet) => void;
  connect: (opts?: { reset?: boolean }) => Promise<{ wallet?: ConnectedWallet; network?: string }>;
} {
  const c = useClient();
  const s = useClientState();
  const wallet = s.wallets[0];
  const connect = useCallback(async () => {
    if (!c) throw notReady();
    await c.loginAsync();
    const w = c.getState().wallets[0];
    return { wallet: w, network: w?.chainId };
  }, [c]);
  return { wallet, network: wallet?.chainId, setActiveWallet: () => undefined, connect };
}

/** Privy's `useToken`. */
export function useToken(callbacks?: PrivyEvents['accessToken']): { getAccessToken: () => Promise<string | null> } {
  const c = useClient();
  const s = useClientState();
  const cb = useLatest(callbacks);
  const had = useRef(false);
  useEffect(() => {
    if (s.authenticated && c) {
      had.current = true;
      void c.getAccessToken().then((t) => t && cb.current?.onAccessTokenGranted({ accessToken: t }));
    } else if (!s.authenticated && had.current) {
      had.current = false;
      cb.current?.onAccessTokenRemoved();
    }
  }, [s.authenticated, s.user, c, cb]);
  const getAccessToken = useCallback(async () => (c ? c.getAccessToken() : null), [c]);
  return { getAccessToken };
}

/** Privy's `useIdentityToken`. Lowki has no identity token (it would link the user across apps). */
export function useIdentityToken(): { identityToken: string | null } {
  return { identityToken: null };
}

/** Privy's `useModalStatus`. */
export function useModalStatus(): { isOpen: boolean } {
  return { isOpen: useClientState().modalOpen };
}
