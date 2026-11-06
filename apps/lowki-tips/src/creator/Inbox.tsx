// Creator inbox: the recipient side of a stealth name. With only the name's viewing and
// spending keys, fetch the gateway's records (authenticated by a viewing-key signature), keep
// the ones that derive to our keys, read balances, and derive each address's private key.
import { useState } from 'react';
import { formatEther, hexToBytes, isHex, type Address, type Hex } from 'viem';
import { balanceOf, short } from '../chain';
import { config, ensName, type Creator } from '../config';
import { addressOfKey, checkAnnouncement, demoSeed, metaAddress, recordsAuth, stealthKeys, stealthPrivateKey } from '../stealth';
import { Avatar, Badge, Brand, Footer, Spinner } from '../ui';

interface Found {
  address: Address;
  createdAt: number;
  balance: bigint;
  canSpend: boolean;
}

interface Scan {
  records: number;
  found: Found[];
}

export default function Inbox() {
  const initial = config.creators.find((c) => c.label === new URLSearchParams(location.search).get('name')?.split('.')[0]) ?? config.creators[0]!;
  const [creator, setCreator] = useState<Creator>(initial);
  const [seedHex, setSeedHex] = useState('');
  const [scan, setScan] = useState<Scan | null>(null);
  const [busy, setBusy] = useState(false);
  const [error, setError] = useState('');
  const name = ensName(creator);

  const seed = (): Uint8Array => {
    if (config.demo) return demoSeed(config.demo.creatorSeedPhrase, creator.label);
    if (!isHex(seedHex) || seedHex.length !== 130) throw new Error('Paste the 64-byte wallet seed (0x + 128 hex characters).');
    return hexToBytes(seedHex);
  };

  const run = async () => {
    setBusy(true);
    setError('');
    setScan(null);
    try {
      const keys = stealthKeys(seed());
      const { ts, signature } = await recordsAuth(keys, name);
      const url = new URL(config.ens.recordsUrl);
      url.searchParams.set('name', name);
      const r = await fetch(url, {
        headers: { 'x-lowki-auth-timestamp': String(ts), 'x-lowki-auth-signature': signature },
        cache: 'no-store',
      });
      if (!r.ok) throw new Error(`The gateway refused: ${r.status} ${(await r.text()).slice(0, 160)}`);
      const body = (await r.json()) as {
        records: { stealth_address: string; ephemeral_pubkey: Hex; view_tag: number; created_at: number }[];
      };
      const found: Found[] = [];
      for (const rec of body.records) {
        const eph = (rec.ephemeral_pubkey.startsWith('0x') ? rec.ephemeral_pubkey : `0x${rec.ephemeral_pubkey}`) as Hex;
        const a = checkAnnouncement(keys, eph, rec.view_tag);
        if (!a || a.toLowerCase() !== rec.stealth_address.toLowerCase()) continue;
        const canSpend = addressOfKey(stealthPrivateKey(keys, eph)).toLowerCase() === a.toLowerCase();
        found.push({ address: a, createdAt: rec.created_at, balance: await balanceOf(a), canSpend });
      }
      setScan({ records: body.records.length, found });
    } catch (e) {
      setError(e instanceof Error ? e.message : String(e));
    } finally {
      setBusy(false);
    }
  };

  const funded = scan?.found.filter((f) => f.balance > 0n) ?? [];
  const total = funded.reduce((s, f) => s + f.balance, 0n);
  let meta = '';
  try {
    meta = config.demo ? metaAddress(stealthKeys(demoSeed(config.demo.creatorSeedPhrase, creator.label))) : '';
  } catch {
    meta = '';
  }

  return (
    <div className="app">
      <header className="topbar">
        <Brand />
        <span className="badge badge-muted">Creator inbox</span>
      </header>
      <main className="page" data-testid="inbox">
        <div className="section-head">
          <h1>Find your tips</h1>
          <p className="muted">
            Tippers paid one-time addresses. Your viewing key finds them in the gateway's records; your spending key
            controls them. Nobody else can link them to you.
          </p>
        </div>

        <div className="tabs" role="tablist">
          {config.creators.map((c) => (
            <button
              key={c.label}
              role="tab"
              aria-selected={c.label === creator.label}
              className={`tab ${c.label === creator.label ? 'on' : ''}`}
              data-testid={`inbox-tab-${c.label}`}
              onClick={() => {
                setCreator(c);
                setScan(null);
              }}
            >
              <Avatar creator={c} size={28} /> {c.displayName}
            </button>
          ))}
        </div>

        <div className="card inbox-card">
          <div className="creator-top">
            <Avatar creator={creator} size={56} />
            <div>
              <h2>{creator.displayName}</h2>
              <p className="mono ens">{name}</p>
            </div>
          </div>
          {config.demo ? (
            <p className="muted small">
              Demo keys: derived from the documented demo seed (<span className="mono">sha512("{config.demo.creatorSeedPhrase}/{creator.label}")</span>
              ). Meta-address <span className="mono">{short(meta, 16, 8)}</span>
            </p>
          ) : (
            <label className="field">
              <span>Wallet seed (stays in this tab)</span>
              <input className="mono" value={seedHex} onChange={(e) => setSeedHex(e.target.value.trim())} placeholder="0x…" />
            </label>
          )}
          <button className="btn btn-accent" data-testid="scan" disabled={busy} onClick={() => void run()}>
            {busy ? (
              <>
                <Spinner /> Scanning…
              </>
            ) : (
              'Sign with viewing key and scan'
            )}
          </button>
          {error && (
            <p className="error" role="alert">
              {error}
            </p>
          )}
        </div>

        {scan && (
          <section className="card results" data-testid="inbox-results">
            <div className="totals">
              <div>
                <span className="id-k">Received</span>
                <strong className="big" data-testid="inbox-total">
                  {formatEther(total)} ETH
                </strong>
              </div>
              <div>
                <span className="id-k">Tips found</span>
                <strong className="big" data-testid="inbox-funded">
                  {funded.length}
                </strong>
              </div>
              <div>
                <span className="id-k">Lookups</span>
                <strong className="big">{scan.records}</strong>
              </div>
            </div>
            <p className="muted small">
              Every lookup of {name} mints a fresh address and a gateway record. {scan.found.length} of {scan.records} records derive
              to this name's keys; {funded.length} received funds.
            </p>
            <ol className="found-list">
              {[...scan.found].reverse().map((f) => (
                <li key={f.address} className={f.balance > 0n ? 'funded' : 'unfunded'} data-testid="inbox-row" data-address={f.address}>
                  <span className="mono">{f.address}</span>
                  <span className="row-meta">
                    {f.balance > 0n ? <Badge tone="ok">{formatEther(f.balance)} ETH</Badge> : <span className="muted small">looked up, not paid</span>}
                    {f.canSpend && <span className="muted small">spend key ✓</span>}
                    <span className="muted small">{new Date(f.createdAt * 1000).toLocaleTimeString()}</span>
                  </span>
                </li>
              ))}
            </ol>
          </section>
        )}
      </main>
      <Footer />
    </div>
  );
}
