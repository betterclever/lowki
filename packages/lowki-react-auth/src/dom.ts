/** Browser implementations of the transport's iframe and popup channels. */
import type { Channel, MessageHost, WalletWindow } from './transport';

const Z = '2147483000';

/**
 * A hidden iframe that becomes a centred modal while a request is open.
 * The wallet draws its own sheet (`embed=1` gives it a transparent
 * backdrop); we add the dimmed page backdrop and a close button.
 */
export function createIframeChannel(src: string, onUserClose: () => void): Channel {
  const doc = document;
  const walletOrigin = new URL(src).origin;

  const root = doc.createElement('div');
  root.setAttribute('data-lowki', 'wallet');
  root.setAttribute('role', 'dialog');
  root.setAttribute('aria-modal', 'true');
  root.setAttribute('aria-label', 'Lowki wallet');
  Object.assign(root.style, {
    position: 'fixed',
    inset: '0',
    zIndex: Z,
    display: 'none',
    alignItems: 'center',
    justifyContent: 'center',
    background: 'rgba(15, 17, 21, 0.45)',
  } satisfies Partial<CSSStyleDeclaration>);

  const frame = doc.createElement('iframe');
  frame.title = 'Lowki wallet';
  // WebAuthn in a cross-origin iframe needs these permissions delegated.
  frame.allow = `publickey-credentials-get ${walletOrigin}; publickey-credentials-create ${walletOrigin}; clipboard-write ${walletOrigin}`;
  frame.referrerPolicy = 'origin';
  Object.assign(frame.style, {
    width: 'min(440px, 100vw)',
    height: 'min(780px, 100dvh)',
    border: '0',
    borderRadius: '20px',
    background: 'transparent',
    colorScheme: 'normal',
  } satisfies Partial<CSSStyleDeclaration>);

  const close = doc.createElement('button');
  close.type = 'button';
  close.setAttribute('aria-label', 'Close Lowki wallet');
  close.setAttribute('data-lowki', 'close');
  close.textContent = '×';
  Object.assign(close.style, {
    position: 'fixed',
    top: '12px',
    right: '12px',
    width: '36px',
    height: '36px',
    borderRadius: '18px',
    border: '0',
    font: '600 22px/36px system-ui, sans-serif',
    background: 'rgba(255,255,255,0.9)',
    color: '#111',
    cursor: 'pointer',
  } satisfies Partial<CSSStyleDeclaration>);
  close.addEventListener('click', onUserClose);
  const onKey = (e: KeyboardEvent) => {
    if (e.key === 'Escape' && root.style.display !== 'none') onUserClose();
  };
  doc.addEventListener('keydown', onKey);

  root.append(frame, close);
  frame.src = src;
  doc.body.appendChild(root);

  let lastFocus: Element | null = null;
  return {
    kind: 'iframe',
    window: () => (frame.contentWindow as WalletWindow | null) ?? null,
    show() {
      if (root.style.display === 'flex') return;
      lastFocus = doc.activeElement;
      root.style.display = 'flex';
      frame.focus();
    },
    hide() {
      root.style.display = 'none';
      if (lastFocus instanceof HTMLElement) lastFocus.focus();
    },
    destroy() {
      doc.removeEventListener('keydown', onKey);
      root.remove();
    },
  };
}

/** Opens the wallet in a popup window. Must run inside a user gesture. */
export function openPopupChannel(src: string): Channel | null {
  const w = 420;
  const h = 720;
  const left = Math.max(0, Math.round(window.screenX + (window.outerWidth - w) / 2));
  const top = Math.max(0, Math.round(window.screenY + (window.outerHeight - h) / 2));
  // No `noopener`: the wallet answers through window.opener, restricted to our origin.
  const win = window.open(src, 'lowki-wallet', `popup=yes,width=${w},height=${h},left=${left},top=${top}`);
  if (!win) return null;
  return {
    kind: 'popup',
    window: () => win as unknown as WalletWindow,
    show() {
      try {
        win.focus();
      } catch {
        /* cross-origin focus can throw in some browsers */
      }
    },
    hide() {},
    destroy() {
      try {
        if (!win.closed) win.close();
      } catch {
        /* already gone */
      }
    },
  };
}

export const windowHost: () => MessageHost = () => window as unknown as MessageHost;

/**
 * Safari (macOS and iOS) can't create a passkey inside a cross-origin iframe,
 * so sign-in there goes through a popup. Chromium and Firefox can.
 */
export function needsPopupForPasskeyCreate(ua = typeof navigator === 'undefined' ? '' : navigator.userAgent): boolean {
  if (/iPhone|iPad|iPod/.test(ua)) return true; // every iOS browser is WebKit
  const webkit = /AppleWebKit/.test(ua);
  const other = /Chrome|Chromium|CriOS|FxiOS|Firefox|Edg|OPR/.test(ua);
  return webkit && !other;
}
