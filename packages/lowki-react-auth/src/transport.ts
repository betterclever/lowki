/**
 * Transport to the Lowki wallet: a hidden iframe on the wallet origin, shown
 * as a modal while the user has something to approve, plus a popup for steps
 * an iframe can't do (passkey creation on Safari, or a frame that never loaded).
 *
 * Security rules, both directions:
 * - We post only with `targetOrigin = walletOrigin` (never `*`).
 * - We accept a message only if `event.origin === walletOrigin` AND
 *   `event.source` is the exact window we opened (iframe or popup) AND it
 *   parses as a well-formed protocol message.
 * - `ready` must name our own app ID, or the channel is refused.
 * - An answer must carry the id of a pending request sent on the same channel
 *   and be one of the answer types for that request.
 * - The wallet is told our origin (`origin=`), and replies only to it.
 */
import { LowkiError, PrivyErrorCode, RpcErrorCode, fromReject } from './errors';
import {
  ANSWERS,
  V1_CAPABILITIES,
  answerId,
  envelope,
  parseWalletMessage,
  type DappRequestType,
  type DappToWallet,
  type WalletToDapp,
} from './protocol';

/** Something we can post to and recognise as `event.source`. */
export interface WalletWindow {
  postMessage(message: unknown, targetOrigin: string): void;
  readonly closed?: boolean;
  close?(): void;
}

export interface MessageEventLike {
  data: unknown;
  origin: string;
  source: unknown;
}

export interface MessageHost {
  addEventListener(type: 'message', listener: (ev: MessageEventLike) => void): void;
  removeEventListener(type: 'message', listener: (ev: MessageEventLike) => void): void;
}

export interface Channel {
  readonly kind: 'iframe' | 'popup';
  /** The window to post to, once it exists. */
  window(): WalletWindow | null;
  show(): void;
  hide(): void;
  destroy(): void;
}

export interface ReadyInfo {
  appId: string;
  capabilities: readonly string[];
}

export type RequestBody = DappToWallet extends infer M ? (M extends { id: string } ? Omit<M, 'id'> : never) : never;

export interface TransportOptions {
  walletUrl: string;
  appId: string;
  /** This page's origin (sent to the wallet as `origin=`). */
  appOrigin: string;
  host: MessageHost;
  createIframe: (src: string, onUserClose: () => void) => Channel;
  /** Must be called synchronously inside a user gesture. Returns null if blocked. */
  openPopup: (src: string) => Channel | null;
  handshakeTimeoutMs?: number;
  requestTimeoutMs?: number;
  /** Session-level events (authenticated, signed_out, delegation_revoked), from any channel. */
  onEvent?: (m: WalletToDapp, via: Channel['kind']) => void;
  /** Wallet theme, passed as `theme=`. */
  theme?: 'light' | 'dark';
  log?: (...args: unknown[]) => void;
  /** Polls popup.closed. Injected for tests. */
  setInterval?: (fn: () => void, ms: number) => unknown;
  clearInterval?: (h: unknown) => void;
}

interface Pending {
  id: string;
  type: DappRequestType;
  channel: ChannelState;
  resolve: (m: WalletToDapp) => void;
  reject: (e: LowkiError) => void;
  timer: ReturnType<typeof setTimeout>;
  interactive: boolean;
  kind: ErrorKind;
}

type ErrorKind = 'login' | 'sign' | 'transaction' | 'other';

interface ChannelState {
  ch: Channel;
  ready: Promise<ReadyInfo>;
  resolveReady: (r: ReadyInfo) => void;
  rejectReady: (e: LowkiError) => void;
  info: ReadyInfo | null;
  failed: LowkiError | null;
  closePoll?: unknown;
}

const LOCAL_HOSTS = new Set(['localhost', '127.0.0.1', '[::1]']);

/** Parses and checks the wallet URL. https only, except on localhost. */
export function walletOriginOf(walletUrl: string): URL {
  let u: URL;
  try {
    u = new URL(walletUrl);
  } catch {
    throw new LowkiError(PrivyErrorCode.INVALID_DATA, `Lowki walletUrl is not a URL: ${walletUrl}`);
  }
  const local = LOCAL_HOSTS.has(u.hostname) || u.hostname.endsWith('.localhost');
  if (!(u.protocol === 'https:' || (u.protocol === 'http:' && local))) {
    throw new LowkiError(PrivyErrorCode.INVALID_DATA, `Lowki walletUrl must be https (http is allowed on localhost only): ${walletUrl}`);
  }
  if (u.username || u.password) throw new LowkiError(PrivyErrorCode.INVALID_DATA, 'Lowki walletUrl must not contain credentials');
  return u;
}

export function walletSrc(walletUrl: string, p: { appId: string; appOrigin: string; embed: boolean; theme?: 'light' | 'dark' }): string {
  const u = walletOriginOf(walletUrl);
  u.searchParams.set('app', p.appId);
  u.searchParams.set('origin', p.appOrigin);
  if (p.embed) u.searchParams.set('embed', '1');
  else u.searchParams.delete('embed');
  if (p.theme) u.searchParams.set('theme', p.theme);
  u.hash = '';
  return u.href;
}

function randomId(): string {
  const c = (globalThis as { crypto?: Crypto }).crypto;
  if (c?.randomUUID) return c.randomUUID();
  const b = new Uint8Array(16);
  if (c?.getRandomValues) c.getRandomValues(b);
  else for (let i = 0; i < b.length; i++) b[i] = Math.floor(Math.random() * 256);
  return Array.from(b, (x) => x.toString(16).padStart(2, '0')).join('');
}

export class Transport {
  readonly walletOrigin: string;
  private readonly o: TransportOptions;
  private iframe: ChannelState | null = null;
  private readonly popups = new Set<ChannelState>();
  /** Every channel we ever opened, so a request on a popup that already closed sees why. */
  private readonly states = new WeakMap<Channel, ChannelState>();
  private readonly pending = new Map<string, Pending>();
  private interactiveTail: Promise<unknown> = Promise.resolve();
  private interactiveOpen = 0;
  private seq = 0;
  private readonly prefix = randomId().slice(0, 8);
  private destroyed = false;
  private readonly listener = (ev: MessageEventLike) => this.onMessage(ev);

  constructor(o: TransportOptions) {
    this.o = o;
    this.walletOrigin = walletOriginOf(o.walletUrl).origin;
    o.host.addEventListener('message', this.listener);
  }

  /** Mounts the hidden iframe (once). Resolves when it says `ready`. */
  connect(): Promise<ReadyInfo> {
    if (this.destroyed) return Promise.reject(new LowkiError(PrivyErrorCode.UNKNOWN_EMBEDDED_WALLET_ERROR, 'Transport destroyed', { reason: 'not_ready' }));
    if (!this.iframe) {
      const src = walletSrc(this.o.walletUrl, { appId: this.o.appId, appOrigin: this.o.appOrigin, embed: true, theme: this.o.theme });
      // The state object must exist before the frame loads, or we could miss `ready`.
      const st = this.newState();
      this.iframe = st;
      st.ch = this.o.createIframe(src, () => this.cancelInteractive('The user closed the wallet'));
      this.states.set(st.ch, st);
      this.armHandshake(st);
    }
    return this.iframe.ready;
  }

  /** The iframe's state: `null` before connect, otherwise ready info or the failure. */
  iframeStatus(): { info: ReadyInfo | null; failed: LowkiError | null } | null {
    return this.iframe ? { info: this.iframe.info, failed: this.iframe.failed } : null;
  }

  /** Request types the wallet says it understands. v1 wallets don't say; we assume the v1 set. */
  supports(type: DappRequestType, via: Channel['kind'] = 'iframe'): boolean | null {
    const st = via === 'iframe' ? this.iframe : [...this.popups][0];
    if (!st?.info) return null;
    return st.info.capabilities.includes(type);
  }

  /**
   * Opens the wallet in a popup. Call synchronously from a click handler, or
   * the browser blocks it. The popup closes itself after one answered request.
   */
  openPopup(): Channel {
    const src = walletSrc(this.o.walletUrl, { appId: this.o.appId, appOrigin: this.o.appOrigin, embed: false, theme: this.o.theme });
    const st = this.newState();
    const ch = this.o.openPopup(src);
    if (!ch) {
      throw new LowkiError(PrivyErrorCode.UNKNOWN_AUTH_ERROR, 'The browser blocked the Lowki popup. Call login() from a click handler.', {
        reason: 'closed',
      });
    }
    st.ch = ch;
    this.states.set(ch, st);
    this.popups.add(st);
    this.armHandshake(st);
    const every = this.o.setInterval ?? ((fn: () => void, ms: number) => setInterval(fn, ms));
    st.closePoll = every(() => {
      if (ch.window()?.closed) this.dropPopup(st, new LowkiError(PrivyErrorCode.USER_EXITED_AUTH_FLOW, 'The user closed the Lowki window', { rpcCode: RpcErrorCode.USER_REJECTED, reason: 'closed' }));
    }, 300);
    return ch;
  }

  /**
   * Sends a request and resolves with the wallet's answer (`rejected` becomes a
   * thrown `LowkiError`). Interactive requests are queued one at a time (the
   * wallet answers `busy` to overlapping ones) and show the wallet while open.
   */
  request(
    body: RequestBody,
    opts: { interactive: boolean; via?: Channel; timeoutMs?: number; errorKind?: ErrorKind } = { interactive: true },
  ): Promise<WalletToDapp> {
    const run = () => this.send(body, opts);
    if (!opts.interactive) return run();
    const p = this.interactiveTail.then(run, run);
    this.interactiveTail = p.catch(() => undefined);
    return p;
  }

  /** Rejects the open interactive request(s), e.g. when the user closes the modal. */
  cancelInteractive(message: string): void {
    for (const p of this.pending.values()) {
      if (!p.interactive) continue;
      const kind = p.type === 'sign_in' ? PrivyErrorCode.USER_EXITED_AUTH_FLOW : PrivyErrorCode.UNABLE_TO_SIGN;
      this.settle(p, null, new LowkiError(kind, message, { rpcCode: RpcErrorCode.USER_REJECTED, reason: 'closed' }));
    }
  }

  destroy(): void {
    if (this.destroyed) return;
    this.destroyed = true;
    this.o.host.removeEventListener('message', this.listener);
    const err = new LowkiError(PrivyErrorCode.UNKNOWN_EMBEDDED_WALLET_ERROR, 'Lowki was unmounted', { rpcCode: RpcErrorCode.DISCONNECTED, reason: 'not_ready' });
    for (const p of [...this.pending.values()]) this.settle(p, null, err);
    for (const st of [...this.popups]) this.dropPopup(st, err);
    this.iframe?.ch.destroy();
    this.iframe = null;
  }

  // ------------------------------------------------------------ internals

  private log(...a: unknown[]) {
    this.o.log?.(...a);
  }

  private newState(): ChannelState {
    let resolveReady!: (r: ReadyInfo) => void;
    let rejectReady!: (e: LowkiError) => void;
    const ready = new Promise<ReadyInfo>((res, rej) => {
      resolveReady = res;
      rejectReady = rej;
    });
    ready.catch(() => undefined); // observed later by callers
    const st: ChannelState = {
      ch: null as unknown as Channel,
      ready,
      resolveReady,
      rejectReady,
      info: null,
      failed: null,
    };
    return st;
  }

  private armHandshake(st: ChannelState) {
    const ms = this.o.handshakeTimeoutMs ?? 15_000;
    const t = setTimeout(() => {
      if (st.info || st.failed) return;
      this.fail(st, new LowkiError(PrivyErrorCode.CLIENT_REQUEST_TIMEOUT, `The Lowki wallet at ${this.walletOrigin} did not load within ${ms} ms`, {
        rpcCode: RpcErrorCode.DISCONNECTED,
        reason: 'timeout',
      }));
    }, ms);
    st.ready.then(() => clearTimeout(t), () => clearTimeout(t));
  }

  private fail(st: ChannelState, e: LowkiError) {
    if (st.failed) return;
    st.failed = e;
    st.rejectReady(e);
  }

  private dropPopup(st: ChannelState, e: LowkiError) {
    if (!this.popups.has(st)) return;
    this.popups.delete(st);
    (this.o.clearInterval ?? ((h: unknown) => clearInterval(h as ReturnType<typeof setInterval>)))(st.closePoll);
    this.fail(st, e);
    for (const p of [...this.pending.values()]) if (p.channel === st) this.settle(p, null, e);
    st.ch.destroy();
  }

  private stateOf(ch: Channel | undefined): ChannelState | null {
    if (!ch) return this.iframe;
    return this.states.get(ch) ?? null;
  }

  private async send(
    body: RequestBody,
    opts: { interactive: boolean; via?: Channel; timeoutMs?: number; errorKind?: ErrorKind },
  ): Promise<WalletToDapp> {
    if (this.destroyed) throw new LowkiError(PrivyErrorCode.UNKNOWN_EMBEDDED_WALLET_ERROR, 'Lowki was unmounted', { rpcCode: RpcErrorCode.DISCONNECTED, reason: 'not_ready' });
    if (!opts.via) await this.connect();
    const st = this.stateOf(opts.via);
    if (!st) throw new LowkiError(PrivyErrorCode.UNKNOWN_EMBEDDED_WALLET_ERROR, 'Unknown Lowki channel', { reason: 'not_ready' });
    const info = await st.ready;
    if (!info.capabilities.includes(body.type)) {
      throw new LowkiError(
        PrivyErrorCode.NOT_SUPPORTED,
        `The Lowki wallet at ${this.walletOrigin} does not support "${body.type}" (it supports: ${info.capabilities.join(', ')}).`,
        { rpcCode: RpcErrorCode.UNSUPPORTED_METHOD, reason: 'not_supported' },
      );
    }
    const win = st.ch.window();
    if (!win) throw new LowkiError(PrivyErrorCode.UNKNOWN_EMBEDDED_WALLET_ERROR, 'The Lowki wallet window is gone', { rpcCode: RpcErrorCode.DISCONNECTED, reason: 'not_ready' });

    const id = `${this.prefix}-${++this.seq}`;
    const msg = { ...body, id } as DappToWallet;
    const timeoutMs = opts.timeoutMs ?? this.o.requestTimeoutMs ?? 10 * 60_000;
    const kind = opts.errorKind ?? 'other';

    return new Promise<WalletToDapp>((resolve, reject) => {
      const p: Pending = {
        id,
        type: body.type,
        channel: st,
        interactive: opts.interactive,
        kind,
        resolve,
        reject,
        timer: setTimeout(() => {
          this.settle(p, null, new LowkiError(PrivyErrorCode.CLIENT_REQUEST_TIMEOUT, `Lowki did not answer "${body.type}" within ${timeoutMs} ms`, {
            rpcCode: RpcErrorCode.INTERNAL,
            reason: 'timeout',
          }));
        }, timeoutMs),
      };
      this.pending.set(id, p);
      if (opts.interactive) {
        this.interactiveOpen++;
        st.ch.show();
      }
      this.log('→', msg.type, id);
      win.postMessage(envelope(msg), this.walletOrigin);
    });
  }

  private settle(p: Pending, answer: WalletToDapp | null, err: LowkiError | null) {
    if (!this.pending.delete(p.id)) return;
    clearTimeout(p.timer);
    if (p.interactive) {
      this.interactiveOpen--;
      if (this.interactiveOpen <= 0) {
        this.interactiveOpen = 0;
        p.channel.ch.hide();
      }
    }
    if (err) p.reject(err);
    else if (answer) p.resolve(answer);
    // A popup does one job.
    if (p.channel.ch.kind === 'popup') {
      this.dropPopup(p.channel, new LowkiError(PrivyErrorCode.USER_EXITED_AUTH_FLOW, 'Popup finished', { reason: 'closed' }));
    }
  }

  private onMessage(ev: MessageEventLike) {
    if (this.destroyed) return;
    if (ev.origin !== this.walletOrigin) return; // not the wallet
    const st = this.sourceState(ev.source);
    if (!st) return; // right origin, but not a window we opened
    const m = parseWalletMessage(ev.data);
    if (!m) return;
    this.log('←', m.type, answerId(m) ?? '');

    if (m.type === 'ready') {
      if (m.app_id !== this.o.appId) {
        this.fail(st, new LowkiError(PrivyErrorCode.MISSING_OR_INVALID_PRIVY_APP_ID, `The Lowki wallet loaded for app "${m.app_id}", expected "${this.o.appId}"`, {
          reason: 'origin',
        }));
        return;
      }
      if (!st.info && !st.failed) {
        st.info = { appId: m.app_id, capabilities: m.capabilities ?? V1_CAPABILITIES };
        st.resolveReady(st.info);
      }
      return;
    }

    const id = answerId(m);
    const p = id ? this.pending.get(id) : undefined;
    if (p && p.channel === st) {
      if (m.type === 'rejected') {
        this.settle(p, null, fromReject(p.kind, m.reason, m.message));
      } else if (ANSWERS[p.type].includes(m.type)) {
        this.settle(p, m, null);
      } else {
        this.settle(p, null, new LowkiError(PrivyErrorCode.INVALID_DATA, `Lowki answered "${p.type}" with "${m.type}"`, { reason: 'origin' }));
      }
    }
    if (m.type === 'authenticated' || m.type === 'signed_out' || m.type === 'delegation_revoked') {
      this.o.onEvent?.(m, st.ch.kind);
    }
  }

  private sourceState(source: unknown): ChannelState | null {
    if (source == null) return null;
    if (this.iframe && this.iframe.ch?.window() === source) return this.iframe;
    for (const st of this.popups) if (st.ch.window() === source) return st;
    return null;
  }
}
