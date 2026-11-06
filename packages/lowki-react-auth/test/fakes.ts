import type { Channel, MessageEventLike, MessageHost, WalletWindow } from '../src/transport';

export const WALLET = 'https://wallet.lowki.test';
export const APP = 'https://app.example';

export class FakeHost implements MessageHost {
  private readonly ls = new Set<(ev: MessageEventLike) => void>();
  addEventListener(_t: 'message', l: (ev: MessageEventLike) => void) {
    this.ls.add(l);
  }
  removeEventListener(_t: 'message', l: (ev: MessageEventLike) => void) {
    this.ls.delete(l);
  }
  get count() {
    return this.ls.size;
  }
  dispatch(ev: MessageEventLike) {
    for (const l of [...this.ls]) l(ev);
  }
}

/** A wallet window: records what the app posts, and can answer as the wallet. */
export class FakeWalletWindow implements WalletWindow {
  sent: Array<{ msg: any; targetOrigin: string }> = [];
  closed = false;
  onRequest?: (msg: any) => void;
  constructor(
    private readonly host: FakeHost,
    readonly origin = WALLET,
  ) {}
  postMessage(msg: unknown, targetOrigin: string) {
    // structuredClone, like the real postMessage
    const m = structuredClone(msg);
    this.sent.push({ msg: m, targetOrigin });
    queueMicrotask(() => this.onRequest?.(m));
  }
  close() {
    this.closed = true;
  }
  /** Post a wallet -> app message, as this window. */
  reply(data: Record<string, unknown>, opts: { origin?: string; source?: unknown } = {}) {
    this.host.dispatch({ data: { lowki: 1, ...data }, origin: opts.origin ?? this.origin, source: opts.source === undefined ? this : opts.source });
  }
  get last() {
    return this.sent[this.sent.length - 1]?.msg;
  }
}

export interface FakeChannel extends Channel {
  win: FakeWalletWindow;
  src: string;
  shown: boolean;
  destroyed: boolean;
  shows: number;
  userClose?: () => void;
}

export function fakeChannel(kind: 'iframe' | 'popup', host: FakeHost, src: string): FakeChannel {
  const win = new FakeWalletWindow(host);
  const ch: FakeChannel = {
    kind,
    win,
    src,
    shown: false,
    destroyed: false,
    shows: 0,
    window: () => win,
    show() {
      ch.shown = true;
      ch.shows++;
    },
    hide() {
      ch.shown = false;
    },
    destroy() {
      ch.destroyed = true;
      win.closed = true;
    },
  };
  return ch;
}

export const flush = () => new Promise((r) => setTimeout(r, 0));

export const ADDRESS = '0x5c6b0f7bf3e7ce046039bd8fabdfd3f9f5021678';
export const USER_ID = 'did:lowki:0123456789abcdef0123456789abcdef';

/** base64url JSON */
const b64 = (o: unknown) => Buffer.from(JSON.stringify(o)).toString('base64url');
export const jwt = (claims: Record<string, unknown>) => `${b64({ alg: 'ES256', typ: 'JWT' })}.${b64(claims)}.sig`;
