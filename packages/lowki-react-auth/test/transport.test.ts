import { afterEach, describe, expect, it, vi } from 'vitest';
import { LowkiError, PrivyErrorCode, RpcErrorCode } from '../src/errors';
import { parseWalletMessage } from '../src/protocol';
import { Transport, walletOriginOf, walletSrc } from '../src/transport';
import { ADDRESS, APP, FakeHost, USER_ID, WALLET, fakeChannel, flush, type FakeChannel } from './fakes';

function setup(opts: { capabilities?: string[]; walletUrl?: string; handshakeTimeoutMs?: number; requestTimeoutMs?: number } = {}) {
  const host = new FakeHost();
  const iframes: FakeChannel[] = [];
  const popups: FakeChannel[] = [];
  const events: any[] = [];
  let popupBlocked = false;
  const t = new Transport({
    walletUrl: opts.walletUrl ?? `${WALLET}/?speed=0`,
    appId: 'app_1',
    appOrigin: APP,
    host,
    createIframe: (src, onUserClose) => {
      const ch = fakeChannel('iframe', host, src);
      ch.userClose = onUserClose;
      iframes.push(ch);
      return ch;
    },
    openPopup: (src) => {
      if (popupBlocked) return null;
      const ch = fakeChannel('popup', host, src);
      popups.push(ch);
      return ch;
    },
    handshakeTimeoutMs: opts.handshakeTimeoutMs ?? 1000,
    requestTimeoutMs: opts.requestTimeoutMs ?? 5000,
    onEvent: (m, via) => events.push({ m, via }),
  });
  const ready = async () => {
    const p = t.connect();
    iframes[0]!.win.reply({ type: 'ready', app_id: 'app_1', ...(opts.capabilities ? { capabilities: opts.capabilities } : {}) });
    return p;
  };
  return { host, t, iframes, popups, events, ready, blockPopups: () => (popupBlocked = true) };
}

afterEach(() => {
  vi.useRealTimers();
});

describe('wallet URL', () => {
  it('accepts https and http on localhost only', () => {
    expect(walletOriginOf('https://wallet.lowki.money').origin).toBe('https://wallet.lowki.money');
    expect(walletOriginOf('http://localhost:8080/').origin).toBe('http://localhost:8080');
    expect(walletOriginOf('http://127.0.0.1:8080/').origin).toBe('http://127.0.0.1:8080');
    expect(() => walletOriginOf('http://wallet.lowki.money')).toThrow(LowkiError);
    expect(() => walletOriginOf('javascript:alert(1)')).toThrow(LowkiError);
    expect(() => walletOriginOf('https://u:p@wallet.lowki.money')).toThrow(/credentials/);
    expect(() => walletOriginOf('not a url')).toThrow(LowkiError);
  });

  it('keeps dev query params and adds app, origin, embed', () => {
    const u = new URL(walletSrc('http://127.0.0.1:8080/?speed=0&platform=mac#x', { appId: 'app_1', appOrigin: APP, embed: true, theme: 'dark' }));
    expect(u.searchParams.get('speed')).toBe('0');
    expect(u.searchParams.get('platform')).toBe('mac');
    expect(u.searchParams.get('app')).toBe('app_1');
    expect(u.searchParams.get('origin')).toBe(APP);
    expect(u.searchParams.get('embed')).toBe('1');
    expect(u.searchParams.get('theme')).toBe('dark');
    expect(u.hash).toBe('');
  });
});

describe('handshake', () => {
  it('resolves on ready from the iframe, with v1 capabilities when none are announced', async () => {
    const { ready, iframes } = setup();
    const info = await ready();
    expect(info.appId).toBe('app_1');
    expect(info.capabilities).toContain('sign_message');
    expect(info.capabilities).not.toContain('send_privately');
    expect(new URL(iframes[0]!.src).searchParams.get('origin')).toBe(APP);
  });

  it('ignores ready from another origin or another window', async () => {
    const { t, iframes } = setup({ handshakeTimeoutMs: 50 });
    const p = t.connect();
    iframes[0]!.win.reply({ type: 'ready', app_id: 'app_1' }, { origin: 'https://evil.example' });
    iframes[0]!.win.reply({ type: 'ready', app_id: 'app_1' }, { source: {} });
    iframes[0]!.win.reply({ type: 'ready', app_id: 'app_1' }, { source: null });
    await expect(p).rejects.toMatchObject({ code: PrivyErrorCode.CLIENT_REQUEST_TIMEOUT, reason: 'timeout' });
  });

  it('refuses a wallet frame loaded for a different app', async () => {
    const { t, iframes } = setup();
    const p = t.connect();
    iframes[0]!.win.reply({ type: 'ready', app_id: 'someone-else' });
    await expect(p).rejects.toMatchObject({ code: PrivyErrorCode.MISSING_OR_INVALID_PRIVY_APP_ID });
  });

  it('times out if the wallet never loads', async () => {
    vi.useFakeTimers();
    const { t } = setup({ handshakeTimeoutMs: 15_000 });
    const p = t.connect();
    vi.advanceTimersByTime(15_001);
    await expect(p).rejects.toMatchObject({ code: PrivyErrorCode.CLIENT_REQUEST_TIMEOUT, rpcCode: RpcErrorCode.DISCONNECTED });
  });

  it('removes its listener on destroy', async () => {
    const { t, host } = setup();
    expect(host.count).toBe(1);
    t.destroy();
    expect(host.count).toBe(0);
  });
});

describe('requests', () => {
  it('posts an enveloped request to the wallet origin only, with unique ids', async () => {
    const { t, ready, iframes } = setup();
    await ready();
    const w = iframes[0]!.win;
    const p1 = t.request({ type: 'sign_message', message: 'hi' }, { interactive: false });
    const p2 = t.request({ type: 'sign_message', message: 'yo' }, { interactive: false });
    await flush();
    expect(w.sent).toHaveLength(2);
    for (const s of w.sent) {
      expect(s.targetOrigin).toBe(WALLET);
      expect(s.msg.lowki).toBe(1);
      expect(s.msg.type).toBe('sign_message');
    }
    expect(w.sent[0]!.msg.id).not.toBe(w.sent[1]!.msg.id);
    w.reply({ type: 'message_signed', id: w.sent[1]!.msg.id, signature: '0xbb' });
    w.reply({ type: 'message_signed', id: w.sent[0]!.msg.id, signature: '0xaa' });
    expect(await p1).toMatchObject({ signature: '0xaa' });
    expect(await p2).toMatchObject({ signature: '0xbb' });
  });

  it('ignores answers from the wrong origin, the wrong window, or for unknown ids', async () => {
    const { t, ready, iframes } = setup();
    await ready();
    const w = iframes[0]!.win;
    let done = false;
    const p = t.request({ type: 'sign_message', message: 'hi' }, { interactive: false }).then((m) => ((done = true), m));
    await flush();
    const id = w.last.id;
    w.reply({ type: 'message_signed', id, signature: '0x01' }, { origin: 'https://evil.example' });
    w.reply({ type: 'message_signed', id, signature: '0x02' }, { source: {} });
    w.reply({ type: 'message_signed', id: 'other', signature: '0x03' });
    w.reply({ type: 'message_signed', id, signature: 'not-hex' }); // malformed: dropped
    await flush();
    expect(done).toBe(false);
    w.reply({ type: 'message_signed', id, signature: '0x04' });
    expect(await p).toMatchObject({ signature: '0x04' });
  });

  it('rejects an answer of the wrong type for the request', async () => {
    const { t, ready, iframes } = setup();
    await ready();
    const p = t.request({ type: 'sign_message', message: 'hi' }, { interactive: false });
    await flush();
    iframes[0]!.win.reply({ type: 'transaction_sent', id: iframes[0]!.win.last.id, hash: '0x12', sponsored: false });
    await expect(p).rejects.toMatchObject({ code: PrivyErrorCode.INVALID_DATA });
  });

  it.each([
    ['user_rejected', 'sign', PrivyErrorCode.UNABLE_TO_SIGN, RpcErrorCode.USER_REJECTED],
    ['user_rejected', 'login', PrivyErrorCode.USER_EXITED_AUTH_FLOW, RpcErrorCode.USER_REJECTED],
    ['not_signed_in', 'sign', PrivyErrorCode.MUST_BE_AUTHENTICATED, RpcErrorCode.UNAUTHORIZED],
    ['busy', 'sign', PrivyErrorCode.TOO_MANY_REQUESTS, RpcErrorCode.RESOURCE_UNAVAILABLE],
    ['policy_rejected', 'other', PrivyErrorCode.INVALID_DATA, RpcErrorCode.INVALID_PARAMS],
    ['network_error', 'transaction', PrivyErrorCode.TRANSACTION_FAILURE, RpcErrorCode.INTERNAL],
  ] as const)('maps rejected/%s (%s) to %s', async (reason, kind, code, rpcCode) => {
    const { t, ready, iframes } = setup();
    await ready();
    const p = t.request({ type: 'sign_message', message: 'hi' }, { interactive: false, errorKind: kind });
    await flush();
    iframes[0]!.win.reply({ type: 'rejected', id: iframes[0]!.win.last.id, reason, message: 'nope' });
    await expect(p).rejects.toMatchObject({ code, rpcCode, reason, message: 'nope' });
  });

  it('times out a request the wallet never answers', async () => {
    const { t, ready } = setup({ requestTimeoutMs: 30 });
    await ready();
    await expect(t.request({ type: 'sign_message', message: 'hi' }, { interactive: false })).rejects.toMatchObject({
      code: PrivyErrorCode.CLIENT_REQUEST_TIMEOUT,
      reason: 'timeout',
    });
  });

  it('runs interactive requests one at a time and shows the wallet while one is open', async () => {
    const { t, ready, iframes } = setup();
    await ready();
    const ch = iframes[0]!;
    const p1 = t.request({ type: 'sign_message', message: 'one' }, { interactive: true });
    const p2 = t.request({ type: 'sign_message', message: 'two' }, { interactive: true });
    await flush();
    expect(ch.win.sent).toHaveLength(1); // the second waits; the wallet would answer `busy`
    expect(ch.shown).toBe(true);
    ch.win.reply({ type: 'message_signed', id: ch.win.last.id, signature: '0x01' });
    await p1;
    await flush();
    expect(ch.win.sent).toHaveLength(2);
    expect(ch.win.last.message).toBe('two');
    ch.win.reply({ type: 'message_signed', id: ch.win.last.id, signature: '0x02' });
    await p2;
    expect(ch.shown).toBe(false);
  });

  it('a failed interactive request does not block the queue', async () => {
    const { t, ready, iframes } = setup();
    await ready();
    const ch = iframes[0]!;
    const p1 = t.request({ type: 'sign_message', message: 'one' }, { interactive: true });
    const p2 = t.request({ type: 'sign_message', message: 'two' }, { interactive: true });
    await flush();
    ch.win.reply({ type: 'rejected', id: ch.win.last.id, reason: 'user_rejected', message: 'no' });
    await expect(p1).rejects.toBeInstanceOf(LowkiError);
    await flush();
    ch.win.reply({ type: 'message_signed', id: ch.win.last.id, signature: '0x02' });
    await expect(p2).resolves.toMatchObject({ signature: '0x02' });
  });

  it('closing the modal rejects the open request', async () => {
    const { t, ready, iframes } = setup();
    await ready();
    const p = t.request({ type: 'sign_in' }, { interactive: true, errorKind: 'login' });
    await flush();
    iframes[0]!.userClose!();
    await expect(p).rejects.toMatchObject({ code: PrivyErrorCode.USER_EXITED_AUTH_FLOW, rpcCode: RpcErrorCode.USER_REJECTED });
    expect(iframes[0]!.shown).toBe(false);
  });

  it('refuses request types the wallet does not support, without posting', async () => {
    const { t, ready, iframes } = setup();
    await ready();
    await expect(t.request({ type: 'send_privately', to: ADDRESS, amount: '1', asset: 'ETH' }, { interactive: true })).rejects.toMatchObject({
      code: PrivyErrorCode.NOT_SUPPORTED,
      rpcCode: RpcErrorCode.UNSUPPORTED_METHOD,
    });
    expect(iframes[0]!.win.sent).toHaveLength(0);
  });

  it('sends send_privately when the wallet announces it, and accepts private_sent', async () => {
    const { t, ready, iframes } = setup({ capabilities: ['sign_in', 'send_privately'] });
    await ready();
    const p = t.request({ type: 'send_privately', to: ADDRESS, amount: '1.5', asset: 'USDC' }, { interactive: true });
    await flush();
    const w = iframes[0]!.win;
    expect(w.last).toMatchObject({ lowki: 1, type: 'send_privately', to: ADDRESS, amount: '1.5', asset: 'USDC' });
    w.reply({ type: 'private_sent', id: w.last.id, hash: `0x${'ab'.repeat(32)}` });
    await expect(p).resolves.toMatchObject({ type: 'private_sent' });
  });

  it('reports session events from the wallet even without a pending request', async () => {
    const { ready, iframes, events } = setup();
    await ready();
    iframes[0]!.win.reply({ type: 'authenticated', address: ADDRESS, user_id: USER_ID, access_token: 't' });
    iframes[0]!.win.reply({ type: 'delegation_revoked', delegation_id: 'd1' });
    iframes[0]!.win.reply({ type: 'signed_out' });
    expect(events.map((e) => e.m.type)).toEqual(['authenticated', 'delegation_revoked', 'signed_out']);
  });
});

describe('popup', () => {
  it('opens a non-embedded popup and closes it after the answer', async () => {
    const { t, popups } = setup();
    const ch = t.openPopup();
    const pp = popups[0]!;
    expect(ch).toBe(pp);
    expect(new URL(pp.src).searchParams.get('embed')).toBeNull();
    const p = t.request({ type: 'sign_in' }, { interactive: true, via: ch, errorKind: 'login' });
    pp.win.reply({ type: 'ready', app_id: 'app_1' });
    await flush();
    await flush();
    expect(pp.win.last).toMatchObject({ type: 'sign_in' });
    pp.win.reply({ type: 'authenticated', id: pp.win.last.id, address: ADDRESS, user_id: USER_ID, access_token: 't' });
    await expect(p).resolves.toMatchObject({ type: 'authenticated', address: ADDRESS });
    expect(pp.destroyed).toBe(true);
  });

  it('a popup cannot answer for the iframe', async () => {
    const { t, ready, iframes, popups } = setup({ requestTimeoutMs: 50 });
    await ready();
    t.openPopup();
    popups[0]!.win.reply({ type: 'ready', app_id: 'app_1' });
    const p = t.request({ type: 'sign_message', message: 'x' }, { interactive: false });
    await flush();
    popups[0]!.win.reply({ type: 'message_signed', id: iframes[0]!.win.last.id, signature: '0x01' });
    await expect(p).rejects.toMatchObject({ reason: 'timeout' });
  });

  it('rejects when the user closes the popup', async () => {
    const intervals: Array<() => void> = [];
    const host = new FakeHost();
    const popups: FakeChannel[] = [];
    const t = new Transport({
      walletUrl: WALLET,
      appId: 'app_1',
      appOrigin: APP,
      host,
      createIframe: (src) => fakeChannel('iframe', host, src),
      openPopup: (src) => {
        const c = fakeChannel('popup', host, src);
        popups.push(c);
        return c;
      },
      setInterval: (fn) => intervals.push(fn),
      clearInterval: () => undefined,
    });
    const ch = t.openPopup();
    const p = t.request({ type: 'sign_in' }, { interactive: true, via: ch, errorKind: 'login' });
    popups[0]!.win.closed = true;
    intervals[0]!();
    await expect(p).rejects.toMatchObject({ code: PrivyErrorCode.USER_EXITED_AUTH_FLOW, reason: 'closed' });
  });

  it('throws a typed error when the popup is blocked', () => {
    const { t, blockPopups } = setup();
    blockPopups();
    expect(() => t.openPopup()).toThrow(/blocked/);
  });
});

describe('message parsing', () => {
  it('drops malformed and foreign messages', () => {
    expect(parseWalletMessage(null)).toBeNull();
    expect(parseWalletMessage('{"lowki":1}')).toBeNull();
    expect(parseWalletMessage({ type: 'ready', app_id: 'x' })).toBeNull(); // no envelope
    expect(parseWalletMessage({ lowki: 2, type: 'ready', app_id: 'x' })).toBeNull();
    expect(parseWalletMessage({ lowki: 1, type: 'authenticated', address: '0x1234', user_id: 'u', access_token: 't' })).toBeNull();
    expect(parseWalletMessage({ lowki: 1, type: 'something_new', id: '1' })).toBeNull();
    expect(parseWalletMessage({ lowki: 1, type: 'rejected', id: '1', reason: 'brand_new', message: 'm' })).toMatchObject({ reason: 'network_error' });
    expect(parseWalletMessage({ lowki: 1, type: 'ready', app_id: 'x'.repeat(70_000) })).toBeNull();
  });

  it('reads the exact v1 shapes the Rust wallet emits', () => {
    // Strings copied from WalletToDapp::to_json (serde, tag = "type", snake_case).
    const auth = parseWalletMessage(JSON.parse(`{"lowki":1,"type":"authenticated","id":null,"address":"${ADDRESS}","user_id":"lk_0123","access_token":"eyJ.x.y"}`));
    expect(auth).toMatchObject({ type: 'authenticated', id: null, user_id: 'lk_0123' });
    expect(parseWalletMessage(JSON.parse('{"lowki":1,"type":"signed_out","id":null}'))).toEqual({ type: 'signed_out', id: null });
    expect(parseWalletMessage(JSON.parse('{"lowki":1,"type":"transaction_sent","id":"r1","hash":"0xab","sponsored":true}'))).toMatchObject({ sponsored: true });
  });
});
