import type { ReactNode } from 'react';
import lowkiMark from '../../../design/logo/lowki-mark-night.svg';
import type { Creator } from './config';

export function Avatar({ creator, size = 56 }: { creator: Creator; size?: number }) {
  const initials = creator.displayName
    .split(/\s+/)
    .map((w) => w[0])
    .join('')
    .slice(0, 2);
  return (
    <span
      className="avatar"
      aria-hidden="true"
      style={{ width: size, height: size, ['--hue' as string]: String(creator.hue), fontSize: size * 0.36 }}
    >
      {initials}
    </span>
  );
}

export function Brand() {
  return (
    <span className="brand">
      <svg className="brand-cup" viewBox="0 0 64 64" aria-hidden="true">
        <rect width="64" height="64" rx="16" fill="var(--tips-accent)" />
        <path d="M20 26h22v10a11 11 0 0 1-22 0z" fill="var(--tips-on-accent)" />
        <path d="M42 29h3a5 5 0 0 1 0 10h-4" fill="none" stroke="var(--tips-on-accent)" strokeWidth="3.5" />
        <path d="M26 14c-2 3 2 5 0 8M33 14c-2 3 2 5 0 8" fill="none" stroke="var(--lk-lime)" strokeWidth="3" strokeLinecap="round" />
      </svg>
      Lowki&nbsp;Tips
    </span>
  );
}

export function Footer() {
  return (
    <footer className="footer">
      <p className="one-import" data-testid="one-import">
        This app was written for Privy. We changed one import.
        <code className="diff">
          <span className="del">- import {'{ PrivyProvider }'} from '@privy-io/react-auth';</span>
          <span className="add">+ import {'{ PrivyProvider }'} from '@lowki/react-auth';</span>
        </code>
      </p>
      <p className="secured">
        <img src={lowkiMark} alt="" height={22} style={{ width: 'auto' }} /> Secured by Lowki
      </p>
    </footer>
  );
}

export function ConfigError({ message }: { message: string }) {
  return (
    <main className="page narrow">
      <Brand />
      <div className="card error-card" role="alert">
        <h1>Lowki Tips isn't configured</h1>
        <p>{message}</p>
      </div>
    </main>
  );
}

export function Badge({ tone, children, testid }: { tone: 'fresh' | 'private' | 'muted' | 'ok'; children: ReactNode; testid?: string }) {
  return (
    <span className={`badge badge-${tone}`} data-testid={testid}>
      {children}
    </span>
  );
}

export function Spinner() {
  return <span className="spinner" aria-hidden="true" />;
}
