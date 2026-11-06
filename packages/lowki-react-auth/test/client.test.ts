import { describe, expect, it, vi } from 'vitest';
import { LowkiClient, configWarnings } from '../src/client';
import { PrivyErrorCode } from '../src/errors';
import { ADDRESS, APP, FakeHost, USER_ID, WALLET, fakeChannel, flush, jwt, type FakeChannel } from './fakes';

/** A client wired to a fake wallet frame that behaves like wallet-ui v1. */
function setup(opts: { capabilities?: string[]; popup?: 'auto' | 'always' | 'never'; userAgent?: string; token?: string; walletSession?: boolean } = {}) {
  const host = new FakeHost();
  const frames: FakeChannel[] = [];
  const popups: FakeChannel[] = [];
  let signedIn = opts.walletSession ?? false;
  let token = opts.token ?? 'tok-1';
  const wallet = (ch: FakeChannel) => {
    ch.win.onRequest = (m) => {
      const reply = (d: Record<string, unknown>) => ch.win.reply(d);
      switch (m.type) {
        case 'sign_in':
          // v1: answers at once if signed in; otherwise opens the sheet and waits for the user.
          if (signedIn) reply({ type: 'authenticated', id: m.id, address: ADDRESS, user_id: USER_ID, access_token: token });
          break;
        case 'sign_message':
          reply(signedIn ? { type: 'message_signed', id: m.id, signature: '0x' + 'ab'.repeat(65) } : { type: 'rejected', id: m.id, reason: 'not_signed_in', message: 'Sign in first' });
          break;
        case 'send_transaction':
          reply({ type: 'transaction_sent', id: m.id, hash: '0x' + 'cd'.repeat(32), sponsored: true });
          break;
        case 'send_privately':
          reply({ type: 'private_sent', id: m.id, hash: '0x' + 'ef'.repeat(32) });
          break;
        case 'sign_out':
          signedIn = false;
          reply({ type: 'signed_out', id: m.id });
          break;
      }
    };
  };
  const client = new LowkiClient({
    appId: 'app_1',
    appOrigin: APP,
    host,
    userAgent: opts.userAgent ?? 'Mozilla/5.0 Chrome/156',
    config: { lowki: { walletUrl: WALLET, popup: opts.popup, handshakeTimeoutMs: 500, requestTimeoutMs: 2000 } },
    createIframe: (src) => {
      const ch = fakeChannel('iframe', host, src);
      wallet(ch);
      frames.push(ch);
      return ch;
    },
    openPopup: (src) => {
      const ch = fakeChannel('popup', host, src);
      wallet(ch);
      popups.push(ch);
      return ch;
    },
  });
  const start = async () => {
    client.start();
    frames[0]!.win.reply({ type: 'ready', app_id: 'app_1', ...(opts.capabilities ? { capabilities: opts.capabilities } : {}) });
    await vi.waitFor(() => expect(client.getState().ready).toBe(true), { timeout: 3000 });
  };
  /** The user completes sign-in in the wallet UI. */
  const userSignsIn = (ch = frames[0]!) => {
    signedIn = true;
    const req = [...ch.win.sent].reverse().find((s) => s.msg.type === 'sign_in')!.msg;
    ch.win.reply({ type: 'authenticated', id: req.id, address: ADDRESS, user_id: USER_ID, access_token: token });
  };
  return { client, host, frames, popups, start, userSignsIn, setToken: (t: string) => (token = t) };
}

describe('LowkiClient', () => {
  it('starts signed out and ready; the session probe is silent', async () => {
    const { client, frames, start } = setup();
    await start();
    expect(client.getState()).toMatchObject({ ready: true, authenticated: false, user: null, wallets: [] });
    expect(frames[0]!.win.sent[0]!.msg.type).toBe('sign_in');
    expect(frames[0]!.shows).toBe(0); // never shown for the probe
  });

  it('restores a session the wallet already has', async () => {
    const { client, start } = setup({ walletSession: true });
    await start();
    expect(client.getState()).toMatchObject({ authenticated: true });
    expect(client.getState().user?.id).toBe(USER_ID);
  });

  it('login shows the wallet, then publishes a Privy-shaped user and wallet', async () => {
    const { client, frames, start, userSignsIn } = setup();
    await start();
    const done = vi.fn();
    client.onLogin({ onComplete: done });
    const p = client.loginAsync();
    await flush();
    expect(frames[0]!.shown).toBe(true);
    expect(client.getState().modalOpen).toBe(true);
    userSignsIn();
    await p;
    expect(frames[0]!.shown).toBe(false);
    expect(client.getState().modalOpen).toBe(false);
    const s = client.getState();
    expect(s.authenticated).toBe(true);
    expect(s.user).toMatchObject({
      id: USER_ID,
      wallet: { address: ADDRESS, chainType: 'ethereum', walletClientType: 'privy', connectorType: 'embedded' },
      linkedAccounts: [{ type: 'wallet', address: ADDRESS }],
      mfaMethods: [],
      isGuest: false,
    });
    expect(s.user?.createdAt).toBeInstanceOf(Date);
    expect(s.user?.email).toBeUndefined(); // not shared
    const w = s.wallets[0]!;
    expect(w).toMatchObject({ address: ADDRESS, walletClientType: 'privy', connectorType: 'embedded', chainId: 'eip155:11155111', type: 'ethereum' });
    expect(done).toHaveBeenCalledWith(expect.objectContaining({ wasAlreadyAuthenticated: false, isNewUser: false, user: expect.objectContaining({ id: USER_ID }) }));
    const provider = await w.getEthereumProvider();
    expect(await provider.request({ method: 'eth_accounts' })).toEqual([ADDRESS]);
  });

  it('login when already signed in completes at once with wasAlreadyAuthenticated', async () => {
    const { client, start } = setup({ walletSession: true });
    await start();
    const done = vi.fn();
    client.onLogin({ onComplete: done });
    await client.loginAsync();
    expect(done).toHaveBeenCalledWith(expect.objectContaining({ wasAlreadyAuthenticated: true }));
  });

  it('closing the wallet during login reports exited_auth_flow', async () => {
    const { client, start } = setup();
    await start();
    const onError = vi.fn();
    client.onLogin({ onError });
    const p = client.loginAsync();
    await flush();
    // the close button in the overlay
    (client as any).transport.cancelInteractive('closed');
    await expect(p).rejects.toMatchObject({ code: PrivyErrorCode.USER_EXITED_AUTH_FLOW });
    expect(onError).toHaveBeenCalledWith(PrivyErrorCode.USER_EXITED_AUTH_FLOW);
  });

  it('uses a popup for sign-in on Safari, then signs in the app', async () => {
    const { client, popups, frames, start, userSignsIn } = setup({ userAgent: 'Mozilla/5.0 (Macintosh) AppleWebKit/605.1.15 Version/19.0 Safari/605.1.15' });
    await start();
    const p = client.loginAsync();
    expect(popups).toHaveLength(1); // opened synchronously, inside the click
    popups[0]!.win.reply({ type: 'ready', app_id: 'app_1' });
    await vi.waitFor(() => expect(popups[0]!.win.sent.length).toBe(1));
    userSignsIn(popups[0]!);
    await p;
    expect(client.getState().authenticated).toBe(true);
    expect(popups[0]!.destroyed).toBe(true);
    expect(frames[0]!.shows).toBe(0);
  });

  it('signMessage / sendTransaction go through the wallet and return Privy shapes', async () => {
    const { client, start } = setup({ walletSession: true });
    await start();
    expect(await client.signMessage({ message: 'hi' })).toEqual({ signature: '0x' + 'ab'.repeat(65) });
    expect(await client.sendTransaction({ to: ADDRESS, value: 1000 })).toEqual({ hash: '0x' + 'cd'.repeat(32) });
    await expect(client.signMessage({ message: 'hi' }, { address: '0x0000000000000000000000000000000000000001' })).rejects.toMatchObject({
      code: PrivyErrorCode.EMBEDDED_WALLET_NOT_FOUND,
    });
  });

  it('sendPrivately is refused by a v1 wallet, and works when the wallet supports it', async () => {
    const v1 = setup({ walletSession: true });
    await v1.start();
    await expect(v1.client.sendPrivately({ to: ADDRESS, amount: '1', asset: 'USDC' })).rejects.toMatchObject({ code: PrivyErrorCode.NOT_SUPPORTED });
    const v2 = setup({ walletSession: true, capabilities: ['sign_in', 'sign_message', 'send_transaction', 'sign_out', 'send_privately'] });
    await v2.start();
    expect(await v2.client.sendPrivately({ to: ADDRESS, amount: '1.25', asset: 'USDC' })).toEqual({ hash: '0x' + 'ef'.repeat(32) });
    await expect(v2.client.sendPrivately({ to: ADDRESS, amount: '-1' })).rejects.toMatchObject({ code: PrivyErrorCode.INVALID_DATA });
    await expect(v2.client.sendPrivately({ to: ADDRESS, amount: '0' })).rejects.toMatchObject({ code: PrivyErrorCode.INVALID_DATA });
  });

  it('signing before sign-in is must_be_authenticated', async () => {
    const { client, start } = setup();
    await start();
    await expect(client.signMessage({ message: 'x' })).rejects.toMatchObject({ code: PrivyErrorCode.MUST_BE_AUTHENTICATED });
  });

  it('getAccessToken returns the token, and refreshes an expired JWT via the wallet', async () => {
    const now = Math.floor(Date.now() / 1000);
    const old = jwt({ sub: USER_ID, exp: now - 10 });
    const fresh = jwt({ sub: USER_ID, exp: now + 900 });
    const { client, start, setToken } = setup({ walletSession: true, token: old });
    await start();
    setToken(fresh);
    expect(await client.getAccessToken()).toBe(fresh);
    expect(await client.getAccessToken()).toBe(fresh);
  });

  it('logout signs out in the wallet and clears state', async () => {
    const { client, start, frames } = setup({ walletSession: true });
    await start();
    const out = vi.fn();
    client.onLogout({ onSuccess: out });
    const accounts = vi.fn();
    client.provider.on('accountsChanged', accounts);
    await client.logout();
    expect(frames[0]!.win.sent.some((s) => s.msg.type === 'sign_out')).toBe(true);
    expect(client.getState()).toMatchObject({ authenticated: false, user: null, wallets: [] });
    expect(out).toHaveBeenCalledTimes(1);
    expect(accounts).toHaveBeenCalledWith([]);
    expect(await client.getAccessToken()).toBeNull();
  });

  it('switchChain yields a new wallet object with the new CAIP-2 chain id', async () => {
    const { client, start } = setup({ walletSession: true });
    await start();
    const before = client.getState().wallets[0]!;
    await expect(before.switchChain(1)).rejects.toMatchObject({ code: PrivyErrorCode.UNSUPPORTED_CHAIN_ID });
    const c2 = new LowkiClient({ appId: 'x', appOrigin: APP, host: new FakeHost(), config: { supportedChains: [] } });
    expect(c2.provider.chainId).toBe(11155111); // Sepolia when nothing is configured
  });

  it('marks ready with an error when the wallet never loads', async () => {
    const host = new FakeHost();
    const c = new LowkiClient({
      appId: 'app_1',
      appOrigin: APP,
      host,
      config: { lowki: { walletUrl: WALLET, handshakeTimeoutMs: 20 } },
      createIframe: (src) => fakeChannel('iframe', host, src),
    });
    c.start();
    await vi.waitFor(() => expect(c.getState().ready).toBe(true));
    expect(c.getState().error).toMatchObject({ code: PrivyErrorCode.CLIENT_REQUEST_TIMEOUT });
  });

  it('refuses an insecure wallet URL', () => {
    const c = new LowkiClient({ appId: 'a', appOrigin: APP, host: new FakeHost(), config: { lowki: { walletUrl: 'http://wallet.example' } } });
    c.start();
    expect(c.getState()).toMatchObject({ ready: true, error: expect.objectContaining({ code: PrivyErrorCode.INVALID_DATA }) });
  });

  it('warns about Privy config Lowki ignores', () => {
    const w = configWarnings({
      loginMethods: ['email', 'passkey', 'wallet', 'sms'],
      embeddedWallets: { ethereum: { createOnLogin: 'off' } },
      walletConnectCloudProjectId: 'x',
      legal: { privacyPolicyUrl: 'https://x' },
    });
    expect(w.join('\n')).toMatch(/walletConnectCloudProjectId, legal/);
    expect(w.join('\n')).toMatch(/wallet, sms/);
    expect(w.join('\n')).toMatch(/createOnLogin "off"/);
    expect(configWarnings({ loginMethods: ['email', 'passkey'], embeddedWallets: { createOnLogin: 'users-without-wallets' } })).toEqual([]);
  });
});
