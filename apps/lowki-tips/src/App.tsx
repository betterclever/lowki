import { useCallback, useEffect, useRef, useState } from 'react';
import { usePrivy, useSendPrivately, useSendTransaction, useWallets } from '@lowki/react-auth';
import { formatEther, parseEther, type Address, type Hex } from 'viem';
import { balanceOf, faucet, publicClient, resolveName, short, txUrl } from './chain';
import { config, ensName, type Creator } from './config';
import { addTip, tipsFor, type TipRecord } from './history';
import { Avatar, Badge, Brand, Footer, Spinner } from './ui';

type View = { kind: 'list' } | { kind: 'tip'; creator: Creator } | { kind: 'private'; creator: Creator };

export default function App() {
  const { ready, authenticated } = usePrivy();
  const [view, setView] = useState<View>({ kind: 'list' });

  if (!ready) {
    return (
      <Shell>
        <div className="loading" aria-live="polite">
          <Spinner /> Connecting to your wallet…
        </div>
      </Shell>
    );
  }
  if (!authenticated) return <Landing />;
  return (
    <Shell header={<Header />}>
      {view.kind === 'list' && (
        <CreatorList onTip={(creator) => setView({ kind: 'tip', creator })} onPrivate={(creator) => setView({ kind: 'private', creator })} />
      )}
      {view.kind === 'tip' && <TipScreen key={view.creator.label} creator={view.creator} onBack={() => setView({ kind: 'list' })} />}
      {view.kind === 'private' && (
        <PrivateTipScreen key={view.creator.label} creator={view.creator} onBack={() => setView({ kind: 'list' })} />
      )}
    </Shell>
  );
}

function Shell({ header, children }: { header?: React.ReactNode; children: React.ReactNode }) {
  return (
    <div className="app">
      {header ?? (
        <header className="topbar">
          <Brand />
        </header>
      )}
      <main className="page">{children}</main>
      <Footer />
    </div>
  );
}

// ------------------------------------------------------------------------------------ landing

function Landing() {
  const { login } = usePrivy();
  return (
    <Shell>
      <section className="hero" data-testid="landing">
        <p className="eyebrow">For the people who make your internet better</p>
        <h1>
          Tip your favourite <span className="hl">creators</span>
        </h1>
        <p className="lede">
          Send a little ETH to the writers, musicians and makers you love. Every tip lands at a fresh address, so nobody can
          add up who paid whom.
        </p>
        <button className="btn btn-accent btn-lg" data-testid="signin" onClick={() => login()}>
          Sign in
        </button>
        <p className="hint">Passkey or email. No seed phrase, no extension.</p>
        <ul className="hero-creators" aria-label="Creators on Lowki Tips">
          {config.creators.map((c) => (
            <li key={c.label}>
              <Avatar creator={c} size={44} />
              <span>
                <strong>{c.displayName}</strong>
                <span className="mono">{ensName(c)}</span>
              </span>
            </li>
          ))}
        </ul>
      </section>
    </Shell>
  );
}

// ------------------------------------------------------------------------------------ header

function Header() {
  const { user, logout } = usePrivy();
  const { wallets } = useWallets();
  const address = (wallets[0]?.address ?? user?.wallet?.address) as Address | undefined;
  const [balance, setBalance] = useState<bigint | null>(null);
  const [funding, setFunding] = useState(false);

  const refresh = useCallback(async () => {
    if (address) setBalance(await balanceOf(address).catch(() => null));
  }, [address]);
  useEffect(() => {
    void refresh();
    const t = setInterval(() => void refresh(), 4000);
    return () => clearInterval(t);
  }, [refresh]);

  return (
    <header className="topbar">
      <Brand />
      <div className="who">
        {address && (
          <span className="id-chip" tabIndex={0} aria-describedby="who-tip" data-testid="who">
            <span className="id-row">
              <span className="id-k">Address</span>
              <span className="mono" data-testid="user-address" data-full={address}>
                {short(address)}
              </span>
            </span>
            <span className="id-row">
              <span className="id-k">User ID</span>
              <span className="mono" data-testid="user-id" data-full={user?.id}>
                {short(user?.id ?? '', 14, 4)}
              </span>
            </span>
            <span role="tooltip" id="who-tip" className="tooltip">
              Only this app sees this address
            </span>
          </span>
        )}
        <span className="balance" data-testid="balance">
          {balance === null ? '…' : `${Number(formatEther(balance)).toFixed(3)} ETH`}
        </span>
        {config.demo?.faucet && address && (
          <button
            className="btn btn-ghost btn-sm"
            data-testid="faucet"
            disabled={funding}
            onClick={async () => {
              setFunding(true);
              try {
                await faucet(address, parseEther('1'));
                await refresh();
              } finally {
                setFunding(false);
              }
            }}
            title="Local fork only: adds 1 test ETH"
          >
            {funding ? 'Adding…' : '+1 test ETH'}
          </button>
        )}
        <button className="btn btn-ghost btn-sm" onClick={() => void logout()}>
          Sign out
        </button>
      </div>
    </header>
  );
}

// ------------------------------------------------------------------------------------ creators

function CreatorList({ onTip, onPrivate }: { onTip: (c: Creator) => void; onPrivate: (c: Creator) => void }) {
  return (
    <section data-testid="creators">
      <div className="section-head">
        <h1>Creators</h1>
        <p className="muted">Each name is a Lowki stealth name. Look it up twice and you get two different addresses.</p>
      </div>
      <ul className="creator-grid">
        {config.creators.map((c) => {
          const name = ensName(c);
          const count = tipsFor(name).length;
          return (
            <li key={c.label} className="card creator" data-testid={`creator-${c.label}`}>
              <div className="creator-top">
                <Avatar creator={c} />
                <div>
                  <h2>{c.displayName}</h2>
                  <p className="mono ens">{name}</p>
                </div>
              </div>
              <p className="tagline">{c.tagline}</p>
              {count > 0 && <p className="muted small">You tipped {count === 1 ? 'once' : `${count} times`} this session</p>}
              <div className="creator-actions">
                <button className="btn btn-accent" data-testid={`tip-${c.label}`} onClick={() => onTip(c)}>
                  Tip {config.tip.amountEth} ETH
                </button>
                <button className="btn btn-private" data-testid={`private-${c.label}`} onClick={() => onPrivate(c)}>
                  <LockIcon /> Tip privately
                </button>
              </div>
            </li>
          );
        })}
      </ul>
    </section>
  );
}

// ------------------------------------------------------------------------------------ tip

type Resolution = { address: Address; fresh: boolean };

/** Resolves the name once per call (each call is a new stealth address) and checks it's unused. */
function useFreshAddress(name: string) {
  const [res, setRes] = useState<Resolution | null>(null);
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(false);
  const started = useRef(false);

  const resolve = useCallback(async () => {
    setBusy(true);
    setError('');
    setRes(null);
    try {
      const address = await resolveName(name);
      const [nonce, bal] = await Promise.all([
        publicClient().getTransactionCount({ address }),
        balanceOf(address),
      ]);
      const seen = tipsFor(name).some((t) => t.address.toLowerCase() === address.toLowerCase());
      setRes({ address, fresh: !seen && nonce === 0 && bal === 0n });
    } catch (e) {
      setError(e instanceof Error ? e.message.split('\n')[0]! : String(e));
    } finally {
      setBusy(false);
    }
  }, [name]);

  useEffect(() => {
    // One lookup per visit (StrictMode mounts twice in dev; every lookup mints a new address).
    if (started.current) return;
    started.current = true;
    void resolve();
  }, [resolve]);

  return { res, error, busy, resolve };
}

function TipScreen({ creator, onBack }: { creator: Creator; onBack: () => void }) {
  const name = ensName(creator);
  const { res, error: resolveError, busy: resolving, resolve } = useFreshAddress(name);
  const { sendTransaction } = useSendTransaction();
  const [phase, setPhase] = useState<'idle' | 'signing' | 'mining' | 'done'>('idle');
  const [hash, setHash] = useState<Hex | null>(null);
  const [error, setError] = useState('');
  const [history, setHistory] = useState<TipRecord[]>(() => tipsFor(name));

  const send = async () => {
    if (!res) return;
    setError('');
    setPhase('signing');
    try {
      const { hash } = await sendTransaction({ to: res.address, value: parseEther(config.tip.amountEth) });
      setHash(hash);
      setPhase('mining');
      const receipt = await publicClient().waitForTransactionReceipt({ hash });
      if (receipt.status !== 'success') throw new Error('The transaction reverted');
      addTip(name, { address: res.address, hash, private: false, amountEth: config.tip.amountEth, at: Date.now() });
      setHistory(tipsFor(name));
      setPhase('done');
    } catch (e) {
      setPhase('idle');
      setError(e instanceof Error ? e.message.split('\n')[0]! : String(e));
    }
  };

  const again = () => {
    setPhase('idle');
    setHash(null);
    void resolve();
  };

  return (
    <section className="tip-layout" data-testid="tip-screen">
      <button className="back" onClick={onBack}>
        ← All creators
      </button>
      <div className="card tip-card">
        <div className="creator-top">
          <Avatar creator={creator} size={64} />
          <div>
            <p className="eyebrow">Tip</p>
            <h1>{creator.displayName}</h1>
            <p className="mono ens">{name}</p>
          </div>
        </div>

        <div className="resolution" aria-live="polite">
          <p className="res-label">
            <span className="mono">{name}</span> resolves to
          </p>
          {resolving && (
            <p className="res-address pending">
              <Spinner /> Asking ENS (CCIP-Read through the Lowki gateway)…
            </p>
          )}
          {resolveError && <p className="error">Couldn't resolve {name}: {resolveError}</p>}
          {res && (
            <>
              <p className="res-address mono" data-testid="resolved-address">
                {res.address}
              </p>
              {phase === 'done' ? (
                <Badge tone="ok">✓ Paid just now</Badge>
              ) : res.fresh ? (
                <Badge tone="fresh" testid="fresh-badge">
                  ✦ Never seen before
                </Badge>
              ) : (
                <Badge tone="muted">Already used</Badge>
              )}
            </>
          )}
        </div>

        <p className="explainer">
          <strong>Each tip goes to a fresh address.</strong> {creator.displayName}'s name holds a stealth meta-address, so every
          lookup derives a new one-time address that only {creator.displayName} can find and spend. ENS verifies it on chain
          before your wallet sees it.
        </p>

        {phase === 'done' && hash ? (
          <div className="sent" data-testid="tip-sent">
            <p className="sent-title">Tip sent · {config.tip.amountEth} ETH</p>
            <TxHash hash={hash} />
            <button className="btn btn-accent" data-testid="tip-again" onClick={again}>
              Tip again
            </button>
          </div>
        ) : (
          <button
            className="btn btn-accent btn-lg"
            data-testid="send-tip"
            disabled={!res || phase !== 'idle'}
            onClick={() => void send()}
          >
            {phase === 'signing' ? (
              <>
                <Spinner /> Confirm in your wallet…
              </>
            ) : phase === 'mining' ? (
              <>
                <Spinner /> Sending…
              </>
            ) : (
              `Send ${config.tip.amountEth} ETH`
            )}
          </button>
        )}
        {error && (
          <p className="error" role="alert">
            {error}
          </p>
        )}
      </div>

      <PastTips name={name} history={history} />
    </section>
  );
}

function PastTips({ name, history }: { name: string; history: TipRecord[] }) {
  const unique = new Set(history.map((t) => t.address.toLowerCase())).size;
  return (
    <aside className="card past" data-testid="past-tips">
      <h2>Your tips to {name}</h2>
      <p className="muted small">This session · {history.length === 0 ? 'none yet' : `${history.length} tips, ${unique} different addresses`}</p>
      <ol className="past-list">
        {history.map((t) => (
          <li key={t.hash} data-testid="past-tip">
            <span className="mono">{short(t.address, 10, 8)}</span>
            {t.private ? <Badge tone="private">private</Badge> : <span className="muted small">{t.amountEth} ETH</span>}
          </li>
        ))}
      </ol>
      {history.length > 1 && unique === history.length && <p className="ok small">✓ All different. Nobody can add them up.</p>}
    </aside>
  );
}

// ------------------------------------------------------------------------------------ private

function PrivateTipScreen({ creator, onBack }: { creator: Creator; onBack: () => void }) {
  const name = ensName(creator);
  const { res, error: resolveError, busy: resolving } = useFreshAddress(name);
  const { sendPrivately } = useSendPrivately();
  const [phase, setPhase] = useState<'idle' | 'proving' | 'done'>('idle');
  const [hash, setHash] = useState<string | null>(null);
  const [error, setError] = useState('');
  const [history, setHistory] = useState<TipRecord[]>(() => tipsFor(name));

  const send = async () => {
    if (!res) return;
    setError('');
    setPhase('proving');
    try {
      const { hash } = await sendPrivately({ to: res.address, amount: config.tip.amountEth, asset: 'ETH' });
      addTip(name, { address: res.address, hash, private: true, amountEth: config.tip.amountEth, at: Date.now() });
      setHistory(tipsFor(name));
      setHash(hash);
      setPhase('done');
    } catch (e) {
      setPhase('idle');
      const err = e as { code?: string; message?: string };
      setError(`${err.message ?? String(e)}${err.code ? ` (${err.code})` : ''}`);
    }
  };

  return (
    <section className="tip-layout" data-testid="private-screen">
      <button className="back" onClick={onBack}>
        ← All creators
      </button>
      <div className="card private-card">
        <div className="creator-top">
          <Avatar creator={creator} size={64} />
          <div>
            <p className="eyebrow">Private tip</p>
            <h1>{creator.displayName}</h1>
            <p className="mono ens">{name}</p>
          </div>
        </div>
        <ul className="private-points">
          <li>
            <strong>From your private balance.</strong> The tip leaves a Privacy Pool, so nothing on chain links it to your
            address.
          </li>
          <li>
            <strong>Proved on your device.</strong> Your wallet builds the zero-knowledge proof in this browser. Watch the
            gourd fill.
          </li>
          <li>
            <strong>To a fresh address.</strong> {res ? <span className="mono" data-testid="private-to">{short(res.address, 10, 8)}</span> : resolving ? 'resolving…' : '—'},
            derived from {name} just now.
          </li>
        </ul>
        <p className="small topup">
          Private balance empty?{' '}
          <a href={walletHomeUrl()} target="_blank" rel="noreferrer" data-testid="open-wallet">
            Move ETH in from your Lowki wallet
          </a>
        </p>
        {resolveError && <p className="error">Couldn't resolve {name}: {resolveError}</p>}
        {phase === 'done' && hash ? (
          <div className="sent private-sent" data-testid="private-sent">
            <p className="sent-title">
              <LockIcon /> Sent privately · {config.tip.amountEth} ETH
            </p>
            <TxHash hash={hash} />
          </div>
        ) : (
          <button
            className="btn btn-private btn-lg"
            data-testid="send-private"
            disabled={!res || phase !== 'idle'}
            onClick={() => void send()}
          >
            {phase === 'proving' ? (
              <>
                <Spinner /> Proving in your wallet…
              </>
            ) : (
              <>
                <LockIcon /> Tip {config.tip.amountEth} ETH privately
              </>
            )}
          </button>
        )}
        {error && (
          <p className="error" role="alert" data-testid="private-error">
            {error}
          </p>
        )}
      </div>
      <PastTips name={name} history={history} />
    </section>
  );
}

// ------------------------------------------------------------------------------------ bits

/** The user's Lowki wallet, opened for this app's account (Home has "Move in"). */
function walletHomeUrl(): string {
  const u = new URL(config.lowki.walletUrl);
  u.searchParams.set('app', config.lowki.appId);
  u.searchParams.set('origin', location.origin);
  u.searchParams.set('app_name', 'Lowki Tips');
  return u.toString();
}

function TxHash({ hash }: { hash: string }) {
  const url = txUrl(hash);
  return (
    <p className="hash">
      <span className="id-k">Transaction</span>{' '}
      {url ? (
        <a className="mono" href={url} target="_blank" rel="noreferrer" data-testid="tx-hash">
          {short(hash, 12, 10)}
        </a>
      ) : (
        <span className="mono" data-testid="tx-hash" title={hash}>
          {short(hash, 12, 10)}
        </span>
      )}
    </p>
  );
}

function LockIcon() {
  return (
    <svg className="lock" viewBox="0 0 16 16" width="14" height="14" aria-hidden="true">
      <rect x="3" y="7" width="10" height="7" rx="2" fill="currentColor" />
      <path d="M5.5 7V5a2.5 2.5 0 0 1 5 0v2" fill="none" stroke="currentColor" strokeWidth="1.6" />
    </svg>
  );
}
