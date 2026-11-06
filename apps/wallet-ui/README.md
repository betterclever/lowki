# Lowki wallet UI

The wallet app that runs inside the Lowki iframe or popup. It is written in Rust with Leptos 0.8 (client-side rendering) and built with trunk. The screens talk to two traits. The **real** backend (the default) runs lowki-worker in a Web Worker and calls lowki-server; the **mock** (`?backend=mock`) keeps screenshots, deep links and the dev panel working.

`apps/demo-dapp/` is a single static page that embeds this wallet and drives it over postMessage.

## Run it

```sh
cargo install trunk --locked          # once
cd apps/wallet-ui
export CARGO_TARGET_DIR=../../target/wallet-ui
trunk serve                            # http://127.0.0.1:8080
trunk build --release                  # dist/
cargo clippy --target wasm32-unknown-unknown -- -D warnings
cargo test                             # policy sentences, amounts, protocol, dates
```

The toolchain is pinned by the repo's `rust-toolchain.toml` (1.98.1, `wasm32-unknown-unknown`). This crate has its own empty `[workspace]`.

**Demo dapp:** serve the **repo root**, because the dapp loads `design/tokens.css` and the bundled fonts by relative path.

```sh
python3 -m http.server 8090            # from the repo root
open "http://localhost:8090/apps/demo-dapp/?wallet=http://localhost:8080/&walletflags=backend%3Dmock"
```

## Run it with the real backend

```sh
(cd crates/lowki-worker && make pkg)   # once: builds pkg/web, which trunk copies into dist/
apps/wallet-ui/scripts/dev.sh          # lowki-server :8787 + demo dapp :8090/:8091 + trunk serve :8080
```

`dev.sh` prints one URL per demo app, for example
`http://localhost:8090/apps/demo-dapp/?wallet=http://localhost:8080/&app=app_…&app_name=Game%20X`.
Email codes are printed in the same terminal (`[lowki dev] email sign-in code for …`).

What the scripts set up:

- `scripts/dev-server.sh` runs lowki-server with `LOWKI_DEV_MODE=true` on port 8787: codes are logged,
  the Lowki origin (`LOWKI_ORIGINS`) is the wallet (`http://localhost:8080`) and the passkey RP ID is
  `localhost`. State (SQLite, a stable dev secret, the admin token) lives in `target/wallet-ui-dev/`.
  It registers one app per demo dapp origin, **Game X** (`http://localhost:8090`) and **Pixel Market**
  (`http://localhost:8091`), and writes their IDs to `target/wallet-ui-dev/apps.json`.
  Env: `PORT`, `WALLET_ORIGIN`, `DAPP_ORIGINS`, `STATE`, `LOWKI_SERVER_BIN`.
- lowki-server's cookie API has no CORS, so the wallet calls it **same-origin**: `Trunk.toml` proxies
  `/v1/` to `localhost:8787`. In production, serve the wallet and lowki-server under one host
  (e.g. `wallet.lowki.money`, which is then also the RP ID), or build with `LOWKI_SERVER_URL`.
- Use `localhost`, not `127.0.0.1`: WebAuthn refuses IP hosts. The wallet and the dapps are then
  same-site, so the session cookie (`SameSite=Lax`) reaches the iframe. A wallet on another site
  needs `LOWKI_COOKIE_SAMESITE=none` (+ `LOWKI_COOKIE_PARTITIONED=true`) and https.

Build-time settings (never URL flags, since the embedding app controls the URL):

| Env | Default | Meaning |
|---|---|---|
| `LOWKI_SERVER_URL` | empty (same origin) | lowki-server base URL |
| `LOWKI_RPC_URL` | `https://ethereum-sepolia-rpc.publicnode.com` | JSON-RPC for balances and broadcasting; also allow it in `index.html`'s CSP `connect-src` |
| `LOWKI_CHAIN_ID`, `LOWKI_NETWORK` | `11155111`, `Sepolia` | chain of that RPC |
| feature `mock-default` | off | make `backend=mock` the default |

**Useful URL flags** (they live in `src/dev.rs`):

| Flag | Effect |
|---|---|
| `screen=` | Deep-link to a screen: `signin`, `email`, `unlock`, `passkey`, `recovery`, `home`, `send`, `vault-in`, `vault-out`, `settings`, `confirm-message`, `confirm-typed`, `confirm-tx`, `delegation`, `delegation-rejected`, `error-passkey`, `error-prf`, `error-network`, `error-policy` |
| `fail=` | Force an error: `passkey_cancelled`, `prf_unsupported`, `network`, `policy_rejected` |
| `speed=` | Mock delay multiplier (`0` is instant, `8` is slow) |
| `empty=1` | Start with an empty wallet |
| `reminder=1` | Start with the recovery code not saved |
| `returning=1` | Make the Google sign-in a returning user |
| `platform=` | `iphone`, `mac`, `windows`, `android` or `other`, for the passkey wording |
| `theme=` | Force `light` or `dark` |
| `dev=1` | Show the dev panel: force errors, change speed, simulate app requests, jump between screens |
| `app=` | The embedding app's id (`game-x`, `pixel-market`, `kingside`) |
| `origin=` | The parent origin to accept messages from and reply to |
| `embed=1` | Transparent backdrop, for iframes |
| `backend=` | `real` (default) or `mock`. `screen=` deep links imply `mock` |
| `google=` | `1` shows "Continue with Google" with the real backend (it has no Google yet); `0` hides it with the mock |
| `app_name=` | Name shown for the embedding app (with the real backend, `app=` is the lowki-server app ID) |

## Screens

| # | Screen | File |
|---|---|---|
| 1 | Sign-in sheet: "Continue to Game X". Passkey is the primary action; Google and email are secondary. | `screens/onboarding.rs` |
| 2 | Email code entry | `screens/onboarding.rs` |
| 3 | Secure your wallet: passkey creation. Wording follows the platform (Face ID, Touch ID, Windows Hello…). | `screens/onboarding.rs` |
| 3b | Unlock with passkey (returning user) | `screens/onboarding.rs` |
| 4 | Recovery code: shown once, with copy, "Back up to iCloud/Google Drive" (mock), and "Remind me later" | `screens/onboarding.rs` |
| 5 | Wallet home: app account address chip, balance, private balance on an ink card with Move in / Move out, tokens, activity, empty state | `screens/home.rs` |
| 6 | Send with a "Send privately" toggle. Proof progress is the gourd fill with real stage timings (Syncing pool / Building proof / Submitting). Also Move in/out. | `screens/send.rs` |
| 7 | Confirmation sheets: message; typed data (EIP-712 decoded by the declared field order, with token amounts and dates); transaction (to, action, amount, fee or "Covered by *App*", plus call data in a collapsible block) | `screens/confirm.rs`, `format.rs` |
| 8 | Delegation consent: Can / Can't sentences generated from the policy, expiry, "Nothing is posted" on chain, verified signer and build hash, enclave measurement | `screens/delegation.rs`, `format.rs` |
| 9 | Settings: sign-in methods (link/unlink), apps that can act for you with Revoke access (confirmed inline), 2-of-3 recovery status, and "your apps", which only the user sees | `screens/settings.rs` |
| 10 | Errors: passkey cancelled, PRF unsupported (copy a link for another browser, or fall back to Google + recovery code), network, policy rejected | `screens/errors.rs` |

Screenshots of every screen, in light and dark at 390 and 1280 px, are in `docs/ui-screens/`. Open `docs/ui-screens/index.html` for the contact sheet.

## Backend traits

`src/backend/mod.rs` defines two traits. The UI only calls them through `backend()`.

- **`AuthBackend`** covers:
  - `sign_in_google`, `send_email_code` / `verify_email_code` and `sign_in_passkey`;
  - `create_passkey` (PRF-wrapped seed, split 2-of-3) and `continue_without_prf`;
  - `backup_recovery` and `acknowledge_recovery`;
  - `session(app)`, which returns the pairwise address, user ID and access token;
  - sign-in method `link_method` / `unlink_method`, `recovery_status` and `sign_out`.
- **`WalletBackend`** covers:
  - `balances`, `activity` and `send` (public, or private with a `ProgressFn` that reports `ProofProgress`);
  - `move_private` (shield and unshield);
  - `sign_message`, `sign_typed_data`, `preview_transaction` and `send_transaction`;
  - `review_delegation` (validation plus attestation), `approve_delegation`, `delegations` and `revoke_delegation`;
  - `app_accounts` and `token_at` (token registry lookup).

All methods return non-`Send` boxed futures (`BoxFut`), because the browser runs single-threaded. `main.rs` is the only place that picks an implementation:

```rust
let mock = backend::mock::new_shared();   // swap for the real worker client
backend::install(mock);
```

`recover_with_code` (lost passkey: email sign-in, then the recovery code) was added to `AuthBackend`; the next `create_passkey` wraps the same seed for a new passkey.

Errors are a closed `WalletError` enum (`Other(String)` carries any other user-facing sentence, shown inline). `state::Ctx::fail` routes `PasskeyCancelled`, `PrfUnsupported`, `Network` and `PolicyRejected` to full-screen states. Field-level errors such as `InvalidCode`, `InsufficientFunds` and `InvalidAddress` show inline.

## postMessage protocol

The protocol is defined in `src/protocol.rs` as `DappToWallet` and `WalletToDapp`. Every message is a JSON object:

```json
{ "lowki": 1, "type": "<snake_case variant>", ...fields }
```

App requests carry an `id`, and the wallet's answer echoes it. The wallet posts only to the configured `origin` and ignores messages from other origins. It also only ever reveals the pairwise address and user ID for the embedding app. Additions after the first v1 wallet are optional fields or new types announced in `ready.capabilities`; old messages parse unchanged. In a popup the wallet answers its opener, in an iframe its parent.

**App to wallet**

| type | fields | what happens |
|---|---|---|
| `sign_in` | `id` | Opens the sign-in sheet, or re-announces the session if the user is already signed in |
| `get_session` | `id` | Silent: `authenticated` (token re-issued if it has under a minute left) or `rejected{not_signed_in}`; never changes the screen |
| `sign_message` | `id`, `message` | Opens the message confirmation sheet |
| `sign_typed_data` | `id`, `typed_data` (eth_signTypedData_v4 JSON), `chain_id?` | Opens the typed-data sheet; `invalid_request` if `domain.chainId` and `chain_id` differ |
| `send_transaction` | `id`, `tx: { to, value (wei, decimal string), data }`, `chain_id?` | Opens the transaction sheet; `unsupported_chain` if the real backend is on another chain |
| `send_privately` | `id`, `to`, `amount` (decimal string), `asset` (`ETH`, `USDC`, `MON`), `chain_id?` | Opens the "Send privately?" sheet, then the backend's private send |
| `request_delegation` | `id`, `policy: { chain, expires (YYYY-MM-DD), rules: [ {kind: "swap", router, router_label?, token, max_per_day} \| {kind: "call", contract, contract_label?, function, max_per_day?} \| {kind: "transfer", to, token, max_per_day} ] }` | Opens the consent sheet; Lowki rejects policies with no cap or an open recipient |
| `sign_out` | `id` | Signs out of this app |
| `cancel` | `id` (of the open request) | The app closed its modal: the sheet closes and that request is answered `rejected{user_rejected}` |

**Wallet to app**

| type | fields |
|---|---|
| `ready` | `app_id`, `capabilities` (request types this build understands) |
| `authenticated` | `id?`, `address`, `user_id` (pairwise), `access_token`, `created_at` (ISO 8601, when this wallet first saw the app account), `is_new_user`, `email?` (only if the user ticked "Share this email") |
| `message_signed` / `typed_data_signed` | `id`, `signature` |
| `transaction_sent` | `id`, `hash`, `sponsored` |
| `private_sent` | `id`, `hash` |
| `delegation_approved` | `id`, `delegation_id`, `expires`, `attestation_build` |
| `delegation_revoked` | `delegation_id` (the user revoked it in Settings) |
| `rejected` | `id`, `reason`: `user_rejected` \| `policy_rejected` \| `not_signed_in` \| `network_error` \| `busy` \| `unsupported_chain` \| `invalid_request`, `message` |

**Popup to iframe.** A popup sign-in (Safari can't create a passkey in a cross-origin iframe) unlocks only the popup's key worker. The seed never leaves a worker, so the iframe signs itself in: the app's next request finds it without a session (`get_session` → `not_signed_in`), the SDK sends `sign_in` to the iframe, and the user taps the passkey (`get` works in cross-origin iframes). That also gives the iframe its own lowki-server session cookie.
| `signed_out` | `id?` |

## The real backend (`src/backend/real/`)

| File | What it does |
|---|---|
| `mod.rs` | `RealBackend`: the flows below, error mapping to the UI's states |
| `worker.rs` | Spawns `lowki-worker/js/worker.js` (module Web Worker; trunk copies it and `pkg/web` into `dist/lowki-worker/`) and does `postMessage({id, request})` / `{id, response}` |
| `server.rs` | lowki-server over `fetch` with cookies and `x-lowki-csrf` (the token from each sign-in response) |
| `passkey.rs` | WebAuthn create/get from the server's options, PRF with the fixed salt `PRF_SALT = "lowki/prf/v1"`; PRF output goes to the worker only, `prf.results` is stripped before posting |
| `chain.rs` | `ChainProvider` trait + `JsonRpc` (Sepolia by default): balances, `eth_call`, nonce, gas, fees, `eth_sendRawTransaction` |
| `codec.rs` | base64url, the recovery code encoding (Crockford base32, 14 groups of 4, 16-bit typo check) |

Flows:

- **Sign-up:** email code → `register/start` → `create()` (PRF evaluated) → `register/finish`. If `create()` returned no PRF output, a real `get()` asks for it; **no PRF output from a real `get()` is "PRF unsupported"** (`prf.enabled` is not trusted). Then worker `createWallet` → `PUT /v1/passkeys/{id}/blob` → `PUT /v1/me/share` (S) → the recovery code R is shown.
- **Returning user:** `login/start` → `get()` with PRF → `login/finish` returns the blob → worker `unlock`. An email sign-in of a known user goes to the passkey step; a known email without passkey + share (sign-up stopped halfway) starts sign-up again.
- **Lost everything:** email code → "Lost your passkey? Use your recovery code" → `POST /v1/me/share/release` → worker `recover(S, R)` → new passkey → worker `rewrap` → `PUT` blob. Same seed, same addresses.
- **Per app:** worker `appAccount(origin)` gives the address; `POST /v1/apps/{app}/token {origin}` gives the access token and `user_id` (the token's `sub`). `origin` is the parent page's origin as the browser reports it (`location.ancestorOrigins[0]`); the `origin=` flag is only a fallback, so an app can't claim another app's account.
- **Signing** (`signMessage`, `signTypedData`, `signTransaction`) runs in the worker after the confirm sheet, for the app's origin. If the worker auto-locked (15 min), the passkey unlocks it again first.
- **Balances / sends:** public USDC (Circle's Sepolia USDC) and ETH from the RPC; public sends and app transactions are EIP-1559, signed in the worker and broadcast with the provider.
- **Still on the mock:** the private balance, private send, Move in/out, delegation (review, approve, list, revoke), activity (empty: there is no indexer), sign-in method link/unlink, and cloud backup of the recovery code.
- **Passkey registration inside the app's iframe (stopgap):** the browser marks it `crossOrigin: true`, which webauthn-rs 0.5 always rejects. Registration is `attestation: "none"`, so nothing signs `clientDataJSON`; `passkey.rs` clears that flag (keeping type, challenge and origin, which the server checks). Assertions are untouched. The real fix is in lowki-server: accept `crossOrigin` when `topOrigin` is a registered app origin.

## What's mocked (`?backend=mock`)

**Everything behind the traits is mocked** (`src/backend/mock.rs`):

- Delays are realistic: about 1.4 s for the Google popup, 1.3 s for the passkey, and about 2.3 s for a private send (the proof stage itself is about 1.1 s).
- Data is deterministic. Pairwise addresses and IDs are derived from the user and app with FNV, so they're stable fakes and not cryptography.
- Balances, activity and the attestation (build `3f2a9c1`) are fake.
- An existing Pixel Market delegation is seeded so Settings has something to show.

**The UI layer contains:**

- no cryptography;
- no network calls;
- no analytics;
- no third-party requests.

Fonts (Bricolage Grotesque, Instrument Sans and IBM Plex Mono, all SIL OFL, with their licences in `assets/fonts/`) are bundled. A CSP meta tag restricts the page to `'self'`. Design tokens and logos are read from `design/` at build time.

## Tests

The tests live in `tests/e2e/`. They are Node with no npm dependencies of their own, plus the repo's chromedriver 156 (or raw CDP), and they run on the macOS host. `real.mjs` borrows `jose` from `crates/lowki-server/tests/node/node_modules` and `viem` from `vectors/lowki-worker/node_modules`.

- `node tests/e2e/clickthrough.mjs` drives the demo dapp, so postMessage is exercised end to end. It covers:
  - sign in with Google (mock);
  - create the passkey;
  - save the recovery code;
  - home, including the dapp receiving the address, pairwise ID and token;
  - send privately with staged progress;
  - approve a delegation, asserting the policy sentences and attestation;
  - revoke it in Settings, asserting the dapp gets `delegation_revoked`.
- `node tests/e2e/screens.mjs` takes every screenshot and writes the contact sheet.
- Both run with `backend=mock`.
- `node tests/e2e/real.mjs` is the **real backend** end to end, with a CDP virtual authenticator (ctap2.1, UV, PRF). Start lowki-server first with `tests/e2e/real-server.sh` (VM or host; fresh database, wallet origin `http://localhost:8740`, dapps `:8741`/`:8742`). The script serves `dist/` on `:8740` with a same-origin `/v1/` proxy and the demo dapp on `:8741` and `:8742`, then checks:
  1. sign-up with email (the code is read from `target/wallet-ui-e2e/server.log`) and a passkey; the dapp gets the address, pairwise ID and a JWT that verifies against the server's JWKS (`jose`);
  2. reload and passkey login: same address and ID; the Sepolia balance loads;
  3. `sign_message` from the dapp: the signature recovers to the app address (`viem`);
  4. the second origin: a different address and ID with the same passkey;
  5. storage cleared and passkey gone: email + recovery code, same address, and the new passkey unlocks after a reload.
