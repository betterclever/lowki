# Lowki

**Privy, but private.** Lowki is an embedded wallet with the same React SDK as Privy, built so
that no app, server operator or RPC provider has to be trusted with the user's keys or with
the list of apps they use.

- **Same SDK.** `@lowki/react-auth` is a drop-in for `@privy-io/react-auth`: change one import,
  keep `PrivyProvider`, `usePrivy`, `useWallets` and the sign/send hooks. App backends verify an
  ES256 access token against Lowki's JWKS, as they would Privy's.
- **Keys on the device.** The seed is generated and used inside a Rust/wasm Web Worker. It is
  wrapped with a key derived from the passkey's PRF output, so what the server stores is useless
  without the user's passkey. Backup is a 2-of-3 Shamir split (passkey, server, recovery code).
- **A separate wallet per app.** Every app origin gets its own address, derived on the device,
  and a pairwise user ID. Two apps can't link the same user through Lowki.
- **Private sends.** Shield into Privacy Pools v1 and withdraw privately through Kohaku's
  privacy-pools plugin, with the Groth16 proof made in the browser (WebGPU, 168 ms for a
  withdrawal).
- **Anonymous reads.** An Anon-RPC provider mode (Nox, Nym or tor-js, through Kohaku's provider
  interface) for Privacy Pools traffic, with one isolated session per app address.
- **ENS stealth names.** `<name>.lowki.eth` resolves to a fresh ERC-5564 stealth address on
  every lookup (ENSIP-30, CCIP-Read, verified on chain), so a public name doesn't become a public
  payment history.
- **Self-hostable.** One server binary (`lowki-server`) with SQLite or Postgres. All key
  handling and cryptography is Rust; JavaScript is bindings only.

Built at ETHGlobal (November 2026). Some pieces are complete and tested end to end; others are
still mocked in the UI. "Status" below says which.

## Demo

A walkthrough of 3 minutes or less: _video link to be added_. The flow it shows, all on the
real backend:

1. An unmodified Privy example app (`examples/privy-swap`) switched to Lowki with one import.
2. Sign-up inside the app's iframe: email code, then a passkey (PRF), then the recovery code.
3. The app gets its own address and pairwise ID; a second app gets different ones from the same
   passkey.
4. Sign a message, confirmed in the wallet's own sheet.
5. Lose the device: email plus recovery code restores the same address.
6. Private send through Privacy Pools (Sepolia fork run, see `docs/PRIVATE-SEND.md`) and an ENS
   stealth name resolving to a new address on every lookup.

## Repository map

| Path | What it is |
|---|---|
| `apps/wallet-ui` | The wallet that runs in the Lowki iframe or popup (Leptos, wasm). Talks to lowki-worker and lowki-server. |
| `apps/demo-dapp` | A plain HTML dapp that embeds the wallet and speaks its postMessage protocol |
| `packages/lowki-react-auth` | `@lowki/react-auth`: the Privy-compatible React SDK, EIP-1193 provider and wagmi connector |
| `packages/lowki-kohaku-host` | Lowki as a Kohaku Host: Privacy Pools sync, proving adapter, Anon-RPC provider, Sepolia scripts |
| `examples/privy-swap` | A Privy example app that runs on Lowki through a one-line alias |
| `crates/lowki-worker` | The key worker (wasm): seed, unlock, per-app accounts, signing, Privacy Pools proofs, sealing |
| `crates/lowki-keys` | BIP-39/32, per-origin account derivation, passkey PRF wrapping |
| `crates/lowki-shares` | 2-of-3 Shamir over GF(2^8), compatible with Privy's `shamir-secret-sharing` |
| `crates/lowki-stealth` | ERC-5564 scheme 1 stealth addresses and scanning |
| `crates/lowki-ens` | ENSIP-30 stealth-name encoding and verification |
| `crates/lowki-pp-core` | Privacy Pools v1 notes, Poseidon, LeanIMT |
| `crates/lowki-groth16`, `crates/lowki-gpu` | Groth16 prover (CPU wasm and WebGPU), byte-identical outputs, accepted by snarkjs and the on-chain verifier |
| `crates/lowki-aa` | ERC-4337 / EIP-7702 user operations |
| `crates/lowki-server` | Self-hostable auth and user service: passkeys, email codes, OIDC, sessions, app tokens and JWKS, share storage, ENS stealth gateway |
| `contracts` | ENSv2 registrar and owner account for `lowki.eth` names (Foundry) |
| `vectors/*` | Differential test vectors against the TypeScript reference implementations |
| `bench` | Rust/wasm vs TypeScript benchmarks; results in `bench/RESULTS.md` |
| `docs` | `ARCHITECTURE.md`, `PRIVATE-SEND.md`, `PRIMITIVES.md`, Privy signing explainer |
| `design` | Design tokens, logos and explorations |

## Quick start

Needs Rust (pinned by `rust-toolchain.toml`, with `wasm32-unknown-unknown`), `trunk`, Node 22+,
Python 3 and OpenSSL.

```sh
(cd crates/lowki-worker && make pkg)   # once: the worker's wasm package
apps/wallet-ui/scripts/dev.sh          # lowki-server :8787, demo dapps :8090/:8091, wallet :8080
```

`dev.sh` prints a URL for each demo app, e.g.
`http://localhost:8090/apps/demo-dapp/?wallet=http://localhost:8080/&app=app_…&app_name=Game%20X`.
Email codes appear in the same terminal (dev mode). Use `localhost`, not `127.0.0.1`: WebAuthn
refuses IP hosts. For a UI-only run without a server, add `&walletflags=backend%3Dmock`.

To run the server alone, register an app and verify its tokens, see
`crates/lowki-server/README.md`. To swap Privy for Lowki in an app, see
`packages/lowki-react-auth/README.md`.

Tests:

```sh
(cd crates/lowki-server && cargo test)               # API, OIDC, tokens, cross-origin passkeys, ENS gateway
crates/lowki-server/tests/e2e/run.sh                 # real Chrome + virtual authenticator, incl. iframe sign-up
apps/wallet-ui/tests/e2e/real-server.sh &            # then, on the machine with Chrome:
node apps/wallet-ui/tests/e2e/real.mjs               # sign-up, login, two apps, signing, recovery
scripts/fork-private-send.sh                         # Privacy Pools shield / private withdraw / ragequit on a Sepolia fork
```

## Benchmarks

Apple M5 Max, Chrome 156, medians after warm-up, every row checked for identical output against
the TypeScript reference. Full method and raw data: `bench/RESULTS.md`.

| Operation (browser) | Lowki (Rust/wasm) | Reference (TypeScript) |
|---|---|---|
| Privacy Pools withdraw proof | **168 ms** WebGPU; 2.05 s one CPU thread | snarkjs: 603 ms with 18 workers; 4.38 s single-thread |
| Commitment (ragequit) proof | 48 ms WebGPU; 126 ms one CPU thread | snarkjs: 39 ms with workers; 286 ms single-thread |
| Mnemonic → key | **4.7× faster** (0.89 ms) | Kohaku (4.2 ms) |
| Stealth scan, 10k announcements | **6.2× faster** (0.53 s) | ScopeLift SDK (3.24 s) |
| secp256k1 signing | 1.9× slower (122 µs) | noble (65 µs) |
| LeanIMT, 100k leaves | 1.2× slower than Kohaku single-threaded; 10× slower than its 16-worker build | Kohaku |
| Payload, everything combined (gzip) | 768 KiB | 804 KiB |

Cross-origin isolation (COOP/COEP) changes none of these numbers, so they hold in a plain iframe.

## CROPS

How Lowki measures against the Ethereum Foundation's CROPS properties (censorship resistance,
open source, privacy, security). Where Lowki falls short today, it says so.

**C: Censorship resistance.**
- Every server piece is self-hostable: one binary, SQLite or Postgres, no Lowki-run service is
  required. An app can run its own lowki-server, wallet origin and ENS gateway.
- Addresses are plain EOAs and signatures are ordinary ECDSA; nothing on chain depends on a Lowki
  contract or reveals Lowki.
- Privacy Pools' ragequit is an exit no ASP or relayer can block. Private withdrawals can go
  through any relayer, or a 4337 paymaster.
- ENS stealth names use verified mode: the gateway can refuse to answer, but can't redirect
  funds; senders can also derive stealth addresses themselves from the on-chain meta-address.
- **Gap:** today the wrapped seed blob is fetched from lowki-server at every passkey sign-in (it
  isn't cached on the device), so an unavailable server blocks unlocking, and new-device
  recovery needs the server's share. Mnemonic export is not built yet.

**R: Open source.** All code in this repository is free and open source under MIT OR
Apache-2.0 (see `LICENSE`; a few components are MIT only, a subset). Third-party pieces
(Kohaku, Fluidkey's StealthResolver, ENS contracts, Privacy Pools circuits) are pinned and used
unmodified. The client loads no third-party scripts, fonts or trackers (see "Client audit").

**O: Ownership and self-custody.** The 24-word seed is generated in the user's browser, in a
Rust/wasm worker, and never leaves it in usable form:
- the copy the server stores is AES-GCM-wrapped under a key derived from the passkey PRF, which
  only the user's authenticator can produce;
- the server's Shamir share S is one of three (2-of-3); alone it reveals nothing;
- the recovery code R stays with the user.

Signing happens on the device after the wallet's own confirmation sheet; apps never see keys.
Neither the operator nor Google nor an app can sign for the user. (Opt-in delegation to an
attested signer, for signing while the user is offline, is designed in `docs/ARCHITECTURE.md`
but not shipped; the worker can already seal one app key to an enclave key.)

**P: Privacy.** What each third party learns about the user:

| Party | Learns | Does not learn |
|---|---|---|
| **The app** | Its own pairwise user ID (`did:lowki:…`), its own address for the user (and so that address's public on-chain history), the requests it sends to the wallet, when the user is signed in | The user's email or Google account (never shared), the seed, the user's addresses or IDs in other apps, private balances |
| **lowki-server operator** | Passkey public keys; `HMAC(secret, "google:" ‖ sub)` and `HMAC(secret, "email:" ‖ email)` (plaintext email only if `LOWKI_STORE_EMAIL=true`); the wrapped seed blob and share S (both useless alone); sign-in times and methods; **which apps each user gets tokens for**, since it mints them (it can link a user across apps even though apps can't); client IPs, held in memory for rate limits (and in any reverse-proxy logs). The operator also serves the wallet code (see trust assumptions) | Keys, the seed, PRF outputs (stripped before posting, dropped if sent), the recovery code, transactions signed on the device, the user's addresses (derived on the device) |
| **RPC provider** | Today the wallet reads balances and broadcasts transactions through a plain JSON-RPC endpoint (`LOWKI_RPC_URL`, default publicnode Sepolia): it sees the user's IP and every address queried, so it **can link the user's per-app addresses** that one browser looks up. The app's SDK sends read calls to the app's configured RPC | With the Anon-RPC mode (Nox/Nym/tor-js, `packages/lowki-kohaku-host`): the IP; with per-address sessions, which addresses belong together. Anon-RPC is used for Privacy Pools traffic but **not yet wired into the wallet UI** |
| **Privacy Pools relayer** | The withdrawal it submits: recipient, amount, fee (all public once mined); the requester's IP unless the request goes through Anon-RPC (`anonFetchFor`) | Which deposit is being spent, or the depositor (hidden by the zero-knowledge proof) |
| **ASP (0xbow)** | Every deposit and its depositor address (public on chain) and its approval decision; when the client fetches the approval set, the IP and timing of the fetch unless it goes through Anon-RPC | Which note a withdrawal spends, the recipient's link to the deposit |
| **ENS gateway operator** | Every lookup of a `.lowki.eth` name (name, time, client IP held in memory), and every stealth address it hands out for a name, so it **can link all of a name's stealth addresses to the name** and watch payments to them | The viewing or spending key; it can't redirect funds (verified on chain) or read the records API without the viewing key. Senders who derive client-side bypass it |
| **Google / email provider** | Google: that the Google account signed in to the operator's OAuth client, and when (scope is `openid` only, so Lowki never receives the email). Email: the operator's SMTP provider and the user's mailbox provider see the address and a sign-in code | Which app the user was using (the popup and codes are on the Lowki origin), addresses, keys |
| **The chain (public)** | Each app address and its ordinary transactions; shielding into Privacy Pools from an app address; withdrawals to a recipient; payments to stealth addresses | The link between per-app addresses (unless the user funds them from a common source); the link between a deposit and a private withdrawal, within the pool's anonymity set; that a stealth address belongs to a name (unless the gateway operator says so); any delegation or Lowki-specific metadata |

**S: Security.**
- Rust for every key operation and proof; keys stay as bytes and are zeroized; never formatted
  into strings or logs. JavaScript is bindings only.
- Passkeys are the master key (user verification required). Google or email alone opens
  nothing: releasing S needs a fresh sign-in with the bound method, the blob needs a fresh
  assertion by that passkey, and an optional recovery delay (the "Balanced" dial) makes an
  insider takeover visible and cancellable.
- App tokens are ES256 with rotating keys; pairwise IDs are HMACs keyed by the server secret, so
  a database dump alone can't compute or link them.
- Passkeys created inside an app's iframe are accepted only under a registered app origin
  (`crates/lowki-server/README.md`, "Passkeys created inside an app's iframe").
- Circuit artifacts are sha256-pinned; proofs are checked against snarkjs and the on-chain
  verifier; Rust crates are tested against differential vectors from the TypeScript references.
- The wallet page has a strict CSP (`default-src 'self'`, only the RPC in `connect-src`) and
  `referrer: no-referrer`.

### Trust assumptions, stated plainly

- **Whoever serves the wallet origin can steal keys.** The wallet's code (HTML, JS, wasm) comes
  from the wallet origin. A malicious or compromised host could serve code that reads the seed
  after the user unlocks. There are no reproducible-build attestations or pinned hashes for the
  wallet bundle yet. Self-hosting the wallet origin moves this trust to the operator you choose.
- **The lowki-server operator is trusted for availability and for unlinkability across apps**
  (see the table), not for custody.
- **The ENS gateway operator** can link a name's stealth addresses, as above.
- **The RPC provider** can link per-app addresses until the wallet's reads go through Anon-RPC.
- **Delegated signing** (not shipped) would trust a TEE and its policy engine.

### Keys and personal data on the device

Keys: yes. The seed and every derived key exist only in the worker on the user's device; what
leaves it is the passkey-wrapped blob and one Shamir share, neither usable alone.

Personal data: mostly. The server keeps what sign-in and recovery need, and no more: HMACs of
the Google subject and email (not the values; the email is stored only if the operator turns
it on), passkey public keys, and timestamps. It needs those to find the user's account and
release share S on a new device. The user's addresses, balances and transactions are never
sent to lowki-server.

### Client audit

Searched `apps/wallet-ui`, `apps/demo-dapp`, `packages/*`, `examples/*` and `design/` for
third-party requests, analytics, trackers, CDNs and web fonts:

- **No analytics or trackers** anywhere (no Google Analytics/Tag Manager, Segment, Mixpanel,
  PostHog, Sentry, Plausible, Vercel Analytics, Hotjar, …).
- **wallet-ui:** fonts (OFL) and logos are bundled; CSP limits everything to `'self'` plus the
  RPC endpoint. Its only third-party request is the JSON-RPC provider (above). The CSP still
  allows `'unsafe-inline'` scripts.
- **demo-dapp:** no external requests; it loads its styles from the repo.
- **@lowki/react-auth:** read calls go to the configured chain RPC (default
  `ethereum-sepolia-rpc.publicnode.com` for Sepolia and `testnet-rpc.monad.xyz` for Monad
  testnet; apps can override with `rpcUrls`). Privy config keys such as
  `walletConnectCloudProjectId` and `appearance.logo` are accepted and ignored, so they cause no
  request.
- **examples/privy-swap:** depends on `@privy-io/react-auth` only for type-checking; the runtime
  build aliases it to `@lowki/react-auth`, so no Privy code ships.
- **lowki-kohaku-host:** Anon-RPC bootstraps with one plain `eth_call` pair per session to
  `sepolia.gateway.tenderly.co` / `ethereum-rpc.publicnode.com` (not user-specific); the tor-js
  default gateway is a temporary host posted by an EF developer (pass `torGateways` to use your
  own). The `browser-anon/` probe page (a test tool, not shipped) calls `api.ipify.org` to show
  the exit IP.
- **Work in progress, not yet committed (`packages/lowki-kohaku-host/browser-wallet/`):** the
  bundled private-send code carries library default endpoints (ethers' default provider list,
  Pimlico's public bundler, public IPFS gateways). Before it ships in the wallet, check that
  none is contacted at runtime and that the wallet CSP's `connect-src` stays limited to the
  endpoints Lowki chooses.
- **design/:** `design/index.html` and `design/explorations/logo-variants.html` load Google Fonts.
  They are design previews and are not part of the wallet or the dapp.

## Status

| Piece | State |
|---|---|
| Key crates (keys, shares, stealth, ENS, pp-core, aa) | Done; differential vectors against the TS references |
| Groth16 prover (CPU and WebGPU) | Done; byte-identical, accepted by snarkjs and the on-chain verifier |
| lowki-worker | Done: wallet, unlock, recovery, per-app accounts, signing, Privacy Pools proofs |
| lowki-server | Done: passkeys (including inside app iframes), email, OIDC, tokens, shares, ENS gateway |
| Wallet UI on the real backend | Sign-up, login, per-app accounts, signing, public sends, recovery. The private balance, private send, delegation and activity screens still use the mock |
| `@lowki/react-auth` | Done for the common Privy surface; coverage table in its README |
| Private send | End to end on a Sepolia fork (shield, private withdraw via relayer, ragequit). Live Sepolia is blocked on the testnet ASP (`docs/PRIVATE-SEND.md`) |
| ENS stealth names | Contracts and gateway done; end to end on an anvil fork of Sepolia with the real ENSv2 contracts (`contracts/e2e/run.sh`); not yet deployed on live Sepolia |
| Anon-RPC | Provider mode done and probed in Chrome; not yet used by the wallet UI's own reads |
| Delegated signing (TEE signer) | Designed; worker sealing done; signer not built |

## License

Licensed under either of [Apache License, Version 2.0](LICENSE-APACHE) or
[MIT license](LICENSE-MIT) at your option. See [`LICENSE`](LICENSE) for the exceptions.
