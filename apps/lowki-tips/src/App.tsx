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
  const [view, setView] = useState<View>(() => {
    const c = creatorFromName(new URLSearchParams(location.search).get('tip') ?? '');
    return c ? { kind: 'tip', creator: c } : { kind: 'list' };
  });

  if (!ready) {
    return (
      <Shell step={1}>
        <div className="loading" aria-live="polite">
          <Spinner /> Connecting to your wallet…
        </div>
      </Shell>
    );
  }
  if (!authenticated) return <Landing />;
  return (
    <Shell header={<Header />} step={view.kind === 'private' ? 3 : 2}>
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

const MY_NAME_KEY = 'lowki-tips:my-name';

function loadMyName(): string {
  try {
    return localStorage.getItem(MY_NAME_KEY) ?? '';
  } catch {
    return '';
  }
}

/** A tippable "creator" for any name under the parent (e.g. clever.lowki.eth), or null if invalid. */
function creatorFromName(raw: string, displayName?: string): Creator | null {
  const name = raw.trim().toLowerCase();
  const suffix = `.${config.ens.parentName}`;
  const label = name.endsWith(suffix) ? name.slice(0, -suffix.length) : '';
  if (!label || !/^[a-z0-9-]+(\.[a-z0-9-]+)*$/.test(label)) return null;
  return { label, displayName: displayName ?? label.split('.')[0]!.replace(/^./, (ch) => ch.toUpperCase()), tagline: '', hue: 140 };
}

/** The wallet's own home page, top level, no app params. */
function walletHome(): string {
  const u = new URL(config.lowki.walletUrl);
  return u.origin + u.pathname;
}

const STEPS = ['Sign in', 'Tip a creator', 'Tip privately'];

function Steps({ current }: { current: number }) {
  return (
    <nav className="steps-wrap" aria-label="Demo steps">
      <ol className="steps">
        {STEPS.map((label, i) => {
          const n = i + 1;
          const state = n < current ? 'done' : n === current ? 'now' : 'next';
          return (
            <li key={label} className={`step step-${state}${n === 3 ? ' step-private' : ''}`} aria-current={state === 'now' ? 'step' : undefined}>
              <span className="step-dot">{state === 'done' ? '✓' : n}</span>
              <span className="step-label">{label}</span>
            </li>
          );
        })}
      </ol>
    </nav>
  );
}

function Shell({ header, step, children }: { header?: React.ReactNode; step: number; children: React.ReactNode }) {
  return (
    <div className="app">
      {header ?? (
        <header className="topbar">
          <Brand />
        </header>
      )}
      <Steps current={step} />
      <main className="page">{children}</main>
      <Footer />
    </div>
  );
}

// ------------------------------------------------------------------------------------ landing

function Landing() {
  const { login } = usePrivy();
  return (
    <Shell step={1}>
      <section className="hero" data-testid="landing">
        <h1>
          Tip your favourite <span className="hl">creators</span>
        </h1>
        <p className="lede">Every tip lands at a fresh address. Nobody can add up who paid whom.</p>
        <button className="btn btn-accent btn-xl" data-testid="signin" onClick={() => login()}>
          Sign in
        </button>
        <p className="hint">Passkey or email. No seed phrase, no extension.</p>
        <ul className="hero-creators" aria-label="Creators on Lowki Tips">
          {config.creators.map((c) => (
            <li key={c.label}>
              <Avatar creator={c} size={40} />
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
            <span className="id-dot" aria-hidden="true" />
            <span className="id-row">
              <span className="mono" data-testid="user-address" data-full={address}>
                {short(address)}
              </span>
            </span>
            <span className="id-sep" aria-hidden="true" />
            <span className="id-row">
              <span className="id-k">ID</span>
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
  const [myName, setMyName] = useState(loadMyName);
  const me = myName ? creatorFromName(myName, 'You') : null;
  const cards: { c: Creator; id: string; you?: boolean }[] = [
    ...(me ? [{ c: { ...me, tagline: 'Your name. Share it to get tipped.' }, id: 'you', you: true }] : []),
    ...config.creators.map((c) => ({ c, id: c.label })),
  ];
  return (
    <section data-testid="creators">
      <div className="section-head">
        <h1>Pick a creator</h1>
        <p className="muted">Each name gives a new address every time you look it up.</p>
      </div>
      <ul className="creator-grid" style={{ ['--cols' as string]: String(cards.length) }}>
        {cards.map(({ c, id, you }) => {
          const name = ensName(c);
          const count = tipsFor(name).length;
          return (
            <li key={id} className={`card creator${you ? ' creator-you' : ''}`} data-testid={`creator-${id}`}>
              <Avatar creator={c} size={72} />
              <h2>{you ? 'You' : c.displayName}</h2>
              <p className="mono ens">{name}</p>
              <p className="tagline">{c.tagline}</p>
              <p className="tipped small">{count > 0 ? `You tipped ${count === 1 ? 'once' : `${count} times`}` : '\u00a0'}</p>
              <div className="creator-actions">
                <button className="btn btn-accent" data-testid={`tip-${id}`} onClick={() => onTip(c)}>
                  Tip {config.tip.amountEth} ETH
                </button>
                <button className="btn btn-private-outline" data-testid={`private-${id}`} onClick={() => onPrivate(c)}>
                  <LockIcon /> Tip privately
                </button>
              </div>
            </li>
          );
        })}
      </ul>
      <div className="side-cards">
        <TipByName onTip={onTip} />
        <GetTipped myName={myName} setMyName={setMyName} />
      </div>
    </section>
  );
}

function GetTipped({ myName, setMyName }: { myName: string; setMyName: (n: string) => void }) {
  const parent = config.ens.parentName;
  const [value, setValue] = useState('');
  const [error, setError] = useState('');
  const [busy, setBusy] = useState(false);
  const [copied, setCopied] = useState(false);
  const share = `${location.origin}${location.pathname}?tip=${encodeURIComponent(myName)}`;

  const save = async (e: React.FormEvent) => {
    e.preventDefault();
    const name = value.trim().toLowerCase();
    if (!creatorFromName(name)) {
      setError(`Enter your name ending in .${parent}`);
      return;
    }
    setBusy(true);
    setError('');
    try {
      await resolveName(name);
      try {
        localStorage.setItem(MY_NAME_KEY, name);
      } catch {
        /* private mode: keep it for this visit */
      }
      setMyName(name);
    } catch {
      setError(`${name} didn't resolve. Claim it in your Lowki wallet first.`);
    } finally {
      setBusy(false);
    }
  };

  return (
    <div className="card get-tipped" data-testid="get-tipped">
      <div className="by-name-text">
        <h2>Get tipped</h2>
        <p className="muted">Your tips are found by your wallet with your private viewing key. This app can't see them.</p>
      </div>
      {myName ? (
        <div className="share">
          <span className="id-k">Your tip link</span>
          <div className="by-name-row">
            <input className="mono" readOnly value={share} data-testid="share-link" aria-label="Your tip link" onFocus={(e) => e.target.select()} />
            <button
              className="btn btn-ghost"
              type="button"
              onClick={() => {
                void navigator.clipboard?.writeText(share).then(() => {
                  setCopied(true);
                  setTimeout(() => setCopied(false), 1500);
                });
              }}
            >
              {copied ? 'Copied ✓' : 'Copy'}
            </button>
          </div>
          <button
            className="linkish small"
            type="button"
            onClick={() => {
              try {
                localStorage.removeItem(MY_NAME_KEY);
              } catch {
                /* ignore */
              }
              setMyName('');
            }}
          >
            Not {myName}? Change
          </button>
        </div>
      ) : (
        <form className="by-name-row" onSubmit={(e) => void save(e)}>
          <input
            className="mono"
            data-test="get-tipped-input"
            data-testid="get-tipped-input"
            value={value}
            onChange={(e) => setValue(e.target.value)}
            placeholder={`you.${parent}`}
            aria-label="Your name"
            autoCapitalize="off"
            autoCorrect="off"
            spellCheck={false}
          />
          <button className="btn btn-accent" type="submit" disabled={busy} data-test="get-tipped-save" data-testid="get-tipped-save">
            {busy ? <Spinner /> : 'Save'}
          </button>
        </form>
      )}
      {error && (
        <p className="error" role="alert">
          {error}
        </p>
      )}
      <div className="get-tipped-links">
        <a className="btn btn-private" href={walletHome()} target="_blank" rel="noreferrer" data-test="my-tips" data-testid="my-tips">
          <LockIcon /> My tips
        </a>
        {!myName && (
          <a className="small" href={walletHome()} target="_blank" rel="noreferrer">
            Don't have one? Claim a name in your Lowki wallet
          </a>
        )}
      </div>
    </div>
  );
}

/** Tip any stealth name under the parent (e.g. your own), through the same tip screen. */
function TipByName({ onTip }: { onTip: (c: Creator) => void }) {
  const parent = config.ens.parentName;
  const [value, setValue] = useState('');
  const [error, setError] = useState('');
  const go = (e: React.FormEvent) => {
    e.preventDefault();
    const c = creatorFromName(value);
    if (!c) {
      setError(`Enter a name ending in .${parent}, like clever.${parent}`);
      return;
    }
    setError('');
    onTip(c);
  };
  return (
    <form className="card by-name" onSubmit={go} data-testid="tip-by-name">
      <div className="by-name-text">
        <h2>Tip anyone by name</h2>
        <p className="muted">Any <span className="mono">*.{parent}</span> name. Yours too.</p>
      </div>
      <div className="by-name-row">
        <input
          className="mono"
          data-test="tip-name-input"
          data-testid="tip-name-input"
          value={value}
          onChange={(e) => setValue(e.target.value)}
          placeholder={`clever.${parent}`}
          aria-label="Name to tip"
          autoCapitalize="off"
          autoCorrect="off"
          spellCheck={false}
        />
        <button className="btn btn-accent" type="submit" data-test="tip-name-go" data-testid="tip-name-go">
          Tip {config.tip.amountEth} ETH
        </button>
      </div>
      {error && (
        <p className="error" role="alert" data-testid="tip-name-error">
          {error}
        </p>
      )}
    </form>
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

  const done = phase === 'done' && !!hash;
  return (
    <section className="flow" data-testid="tip-screen">
      <button className="back" onClick={onBack}>
        ← All creators
      </button>
      <div className="card flow-card">
        <CreatorHead creator={creator} name={name} eyebrow={`Tip ${config.tip.amountEth} ETH`} />

        <div className={`resolution${done ? ' is-done' : ''}`} aria-live="polite">
          <p className="res-label">
            {done ? (
              <strong className="res-done">✓ Sent to a fresh address</strong>
            ) : (
              <>
                <span className="mono">{name}</span> resolves to
              </>
            )}
          </p>
          {resolving && (
            <p className="res-address pending">
              <Spinner /> Looking up {name}…
            </p>
          )}
          {resolveError && (
            <p className="error res-error" role="alert" data-testid="resolve-error">
              {name} didn't resolve. Check the name and try again.
              <span className="small muted"> ({resolveError})</span>
            </p>
          )}
          {res && (
            <>
              <p className="res-address mono" data-testid="resolved-address">
                {res.address}
              </p>
              <div className="chips">
                {done && <Badge tone="ok">✓ Paid {config.tip.amountEth} ETH</Badge>}
                {res.fresh ? (
                  <Badge tone="fresh" testid={done ? undefined : 'fresh-badge'}>
                    ✦ Never seen before
                  </Badge>
                ) : (
                  !done && <Badge tone="muted">Already used</Badge>
                )}
              </div>
            </>
          )}
        </div>

        {!done && <p className="explainer">Only {creator.displayName} can find and spend it. ENS verifies it on chain.</p>}

        {done ? (
          <div className="sent" data-testid="tip-sent">
            <TxHash hash={hash!} />
            <button className="btn btn-accent btn-lg btn-block" data-testid="tip-again" onClick={again}>
              Tip again → new address
            </button>
          </div>
        ) : (
          <button
            className="btn btn-accent btn-lg btn-block"
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

function CreatorHead({ creator, name, eyebrow }: { creator: Creator; name: string; eyebrow: string }) {
  return (
    <div className="creator-head">
      <Avatar creator={creator} size={56} />
      <div>
        <p className="eyebrow">{eyebrow}</p>
        <h1>{creator.displayName}</h1>
        <p className="mono ens">{name}</p>
      </div>
    </div>
  );
}

function PastTips({ name, history }: { name: string; history: TipRecord[] }) {
  const unique = new Set(history.map((t) => t.address.toLowerCase())).size;
  return (
    <aside className="card past" data-testid="past-tips">
      <div className="past-head">
        <h2>Your tips to {name}</h2>
        <span className="muted small">{history.length === 0 ? 'none yet' : `${history.length} tips · ${unique} addresses`}</span>
      </div>
      {history.length > 0 && (
        <ol className="past-list">
          {history.map((t) => (
            <li key={t.hash} data-testid="past-tip">
              <span className="mono">{short(t.address, 10, 8)}</span>
              {t.private ? <Badge tone="private">private</Badge> : <span className="muted small">{t.amountEth} ETH</span>}
            </li>
          ))}
        </ol>
      )}
      {history.length > 1 && unique === history.length && <p className="all-diff">✓ All different. Nobody can add them up.</p>}
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

  const done = phase === 'done' && !!hash;
  return (
    <section className="flow" data-testid="private-screen">
      <button className="back" onClick={onBack}>
        ← All creators
      </button>
      <div className="card flow-card private-card">
        <CreatorHead creator={creator} name={name} eyebrow="Private tip" />
        {done ? (
          <div className="sent private-sent" data-testid="private-sent">
            <p className="sent-title">
              <LockIcon /> Sent privately
            </p>
            <p className="sent-sub">{config.tip.amountEth} ETH from a Privacy Pool. Nothing links it to you.</p>
            <TxHash hash={hash!} />
          </div>
        ) : (
          <ul className="private-points">
            <li>
              <strong>From your private balance.</strong> Nothing on chain links it to you.
            </li>
            <li>
              <strong>Proved on your device.</strong> Watch the gourd fill.
            </li>
            <li>
              <strong>To a fresh address:</strong>{' '}
              {res ? <span className="mono" data-testid="private-to">{short(res.address, 10, 8)}</span> : resolving ? 'resolving…' : '—'}
            </li>
          </ul>
        )}
        {resolveError && <p className="error">Couldn't resolve {name}: {resolveError}</p>}
        {!done && (
          <button
            className="btn btn-private btn-lg btn-block"
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
        {!done && (
          <p className="small topup">
            Private balance empty?{' '}
            <a href={walletHomeUrl()} target="_blank" rel="noreferrer" data-testid="open-wallet">
              Move ETH in from your Lowki wallet
            </a>
          </p>
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
