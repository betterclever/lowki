# Lowki wallet UI

The wallet app that runs inside the Lowki iframe or popup. It is written in Rust with Leptos 0.8 (client-side rendering) and built with trunk. The backend is mocked behind two traits, so the real Rust worker and server can plug in without touching any screen.

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
open "http://127.0.0.1:8090/apps/demo-dapp/?wallet=http://127.0.0.1:8080/"
```

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

Errors are a closed `WalletError` enum. `state::Ctx::fail` routes `PasskeyCancelled`, `PrfUnsupported`, `Network` and `PolicyRejected` to full-screen states. Field-level errors such as `InvalidCode`, `InsufficientFunds` and `InvalidAddress` show inline.

## postMessage protocol

The protocol is defined in `src/protocol.rs` as `DappToWallet` and `WalletToDapp`. Every message is a JSON object:

```json
{ "lowki": 1, "type": "<snake_case variant>", ...fields }
```

App requests carry an `id`, and the wallet's answer echoes it. The wallet posts only to the configured `origin` and ignores messages from other origins. It also only ever reveals the pairwise address and user ID for the embedding app.

**App to wallet**

| type | fields | what happens |
|---|---|---|
| `sign_in` | `id` | Opens the sign-in sheet, or re-announces the session if the user is already signed in |
| `sign_message` | `id`, `message` | Opens the message confirmation sheet |
| `sign_typed_data` | `id`, `typed_data` (eth_signTypedData_v4 JSON) | Opens the typed-data sheet |
| `send_transaction` | `id`, `tx: { to, value (wei, decimal string), data }` | Opens the transaction sheet |
| `request_delegation` | `id`, `policy: { chain, expires (YYYY-MM-DD), rules: [ {kind: "swap", router, router_label?, token, max_per_day} \| {kind: "call", contract, contract_label?, function, max_per_day?} \| {kind: "transfer", to, token, max_per_day} ] }` | Opens the consent sheet; Lowki rejects policies with no cap or an open recipient |
| `sign_out` | `id` | Signs out of this app |

**Wallet to app**

| type | fields |
|---|---|
| `ready` | `app_id` |
| `authenticated` | `id?`, `address`, `user_id` (pairwise), `access_token` |
| `message_signed` / `typed_data_signed` | `id`, `signature` |
| `transaction_sent` | `id`, `hash`, `sponsored` |
| `delegation_approved` | `id`, `delegation_id`, `expires`, `attestation_build` |
| `delegation_revoked` | `delegation_id` (the user revoked it in Settings) |
| `rejected` | `id`, `reason`: `user_rejected` \| `policy_rejected` \| `not_signed_in` \| `network_error` \| `busy`, `message` |
| `signed_out` | `id?` |

## What's mocked

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

The tests live in `tests/e2e/`. They are Node with no npm dependencies, plus the repo's chromedriver 156, and they run on the macOS host.

- `node tests/e2e/clickthrough.mjs` drives the demo dapp, so postMessage is exercised end to end. It covers:
  - sign in with Google (mock);
  - create the passkey;
  - save the recovery code;
  - home, including the dapp receiving the address, pairwise ID and token;
  - send privately with staged progress;
  - approve a delegation, asserting the policy sentences and attestation;
  - revoke it in Settings, asserting the dapp gets `delegation_revoked`.
- `node tests/e2e/screens.mjs` takes every screenshot and writes the contact sheet.
