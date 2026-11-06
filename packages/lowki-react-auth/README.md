# @lowki/react-auth

The app-facing SDK for Lowki: a drop-in replacement for `@privy-io/react-auth` (v3).

The private keys stay on the user's device. Each app sees its own address and a pairwise user ID, so two apps can't link the same user. Private sends are built in.

## Migrate in one line

```diff
- import { PrivyProvider, usePrivy, useWallets } from '@privy-io/react-auth';
+ import { PrivyProvider, usePrivy, useWallets } from '@lowki/react-auth';
```

```sh
npm install @lowki/react-auth
# every file at once:
grep -rl "@privy-io/react-auth" src | xargs sed -i '' 's#@privy-io/react-auth#@lowki/react-auth#g'
```

Your Privy code keeps working unchanged: `<PrivyProvider appId config>`, `usePrivy`, `useWallets`, `useSignMessage`, `useSendTransaction` and the other hooks. `LowkiProvider` is the same component as `PrivyProvider`.

You also need to:

1. **Use a Lowki app ID.** Register your origin with lowki-server (`POST /admin/v1/apps`) and pass the `app_…` ID as `appId`.
2. **Point at your wallet** if you don't use the hosted one (`https://wallet.lowki.money`). Use either of:
   - `config={{ ..., lowki: { walletUrl: 'http://localhost:8080/' } }}`. Privy ignores the `lowki` key, so the same config also type-checks against Privy.
   - `<meta name="lowki:wallet-url" content="http://localhost:8080/">` in your HTML, which needs no code change at all.

   The URL must be https, except on localhost. Query parameters on it are kept, which is useful for wallet-ui dev flags such as `?speed=0`.
3. **Verify tokens with Lowki's JWKS** on your backend, as shown [below](#verify-the-access-token-on-your-backend).

### What changes for your users

Sign-in happens in the Lowki wallet: passkey first, with email code or Google as secondary options. The passkey belongs to the wallet origin, so one passkey covers every app.

Your app receives:

| Field | Value |
|---|---|
| `user.id` | Pairwise: `did:lowki:<32 hex>`. Stable for your app; a different value in every other app. |
| `user.wallet.address` | Your app's own account, also different in every app |
| `user.email` | Set only if the user ticks "Share this email with *App*" when signing in with an email code |

The wallet shows its own confirmation sheet for every signature and transaction. Any `uiOptions` you pass are accepted but have no effect.

## API coverage against Privy

The coverage comes from the TypeScript compiler, comparing `@privy-io/react-auth@3.51.3` with this package. Privy exports 109 runtime names; Lowki exports 31, and 13 of those are Lowki-only.

| Area | Status | Notes |
|---|---|---|
| `PrivyProvider` (`appId`, `config`, `children`) | supported | `clientId` and `apiUrl` are accepted and ignored |
| config `loginMethods`, `appearance.theme` | partial | `theme: 'light' \| 'dark'` is passed to the wallet. The wallet always offers passkey, email and Google, and other login methods trigger a dev warning. |
| config `embeddedWallets.createOnLogin` (v1/v2 shape and v3 `ethereum.createOnLogin`) | supported | Every Lowki user has a wallet for each app. `'off'` triggers a dev warning. |
| config `defaultChain`, `supportedChains` (viem `Chain`) | supported | These set the provider's chains. With none configured, the default is Sepolia. |
| Other config keys (`legal`, `walletConnectCloudProjectId`, `externalWallets`, `mfa`, `intl`, `plugins`, …) | accepted, ignored | All keys are typed, so existing config literals compile. A dev warning lists the ignored keys. |
| `usePrivy`: `ready`, `authenticated`, `user`, `error`, `login`, `logout`, `getAccessToken`, `isModalOpen` | supported | |
| `usePrivy`: `signMessage`, `signTypedData`, `sendTransaction` | supported | `sendTransaction` on `usePrivy` is the v1 location; v3 apps use `useSendTransaction`. |
| `usePrivy`: `createWallet` | partial | Returns the existing wallet. A user can't have more than one wallet per app. |
| `usePrivy`: `linkEmail`, `linkPasskey`, `linkGoogle`, `connectWallet` | stub | These do nothing and log a dev warning. Linking happens in the wallet's Settings. |
| `usePrivy`: MFA (`enrollInMfa`, `promptMfa`, …), the other `link*` / `unlink*` methods, `exportWallet`, `setWalletRecovery`, `signTransaction`, `updateEmail` | not yet | 56 of Privy's 71 members are missing. Most don't fit the model: the passkey is the second factor, and recovery uses the 2-of-3 shares. |
| `useWallets` → `ConnectedWallet` (`address`, `chainId` (CAIP-2), `walletClientType: 'privy'`, `connectorType: 'embedded'`, `switchChain`, `getEthereumProvider`, `sign`, `isConnected`, `disconnect`) | supported | `walletClientType` is `'privy'` so that `wallets.find(w => w.walletClientType === 'privy')` keeps working. `meta.name` is `'Lowki'`. |
| `ConnectedWallet.fund`, `unlink` | not yet | These throw `not_supported`. |
| `useLogin({ onComplete, onError })`, `useLogout({ onSuccess })` | supported | `isNewUser` is true when the wallet was created in this sign-in. `loginMethod` is `null` (the wallet doesn't report it). |
| `useSignMessage`, `useSignTypedData`, `useSendTransaction` (with callbacks) | supported | |
| `getAccessToken` (standalone export), `useToken`, `useUser` | supported | |
| `useCreateWallet`, `useActiveWallet`, `useModalStatus`, `getEmbeddedConnectedWallet` | supported | There is one wallet per app. |
| `useIdentityToken` | partial | Always `null`. An identity token would link the user across apps. |
| `User` shape (`id`, `createdAt`, `wallet`, `linkedAccounts`, `mfaMethods`, `isGuest`, `hasAcceptedTerms`, and the `google`/`twitter`/`phone`/… fields) | supported | `linkedAccounts` contains only the wallet. OAuth and phone fields stay `undefined` but are typed, so code that checks them compiles. `createdAt` is the wallet's `created_at`: when this wallet first saw the app account (lowki-server's per-app creation time isn't exposed yet). |
| `PrivyErrorCode` | partial | The same string values as Privy for every code Lowki emits. |
| EIP-1193 provider: `eth_accounts`, `eth_requestAccounts`, `eth_chainId`, `net_version`, `personal_sign`, `eth_signTypedData_v4`, `eth_sendTransaction`, `wallet_switchEthereumChain`, `wallet_addEthereumChain` (configured chains), `wallet_getPermissions`; read calls (`eth_call`, `eth_getBalance`, `eth_estimateGas`, `eth_getLogs`, … 33 in total) forwarded to the chain RPC | supported | See [Provider limits](#provider-limits). |
| `eth_sign`, `eth_signTransaction`, `wallet_sendCalls`, `eth_subscribe` | not yet | These return 4200. |
| `useSigners` / `useSessionSigners` | not yet | Lowki's form is `useRequestDelegation(policy)`, which takes an explicit policy instead of `policyIds`. |
| `useFundWallet`, onramps, `useSignTransaction`, `useSign7702Authorization`, `toViemAccount`, Solana, Farcaster, SIWE/SIWS, cross-app, headless `useLoginWith*` | not yet | |
| wagmi | supported | `import { lowkiConnector } from '@lowki/react-auth/wagmi'`. Privy has a separate `@privy-io/wagmi` package. |

### Lowki-only APIs

```tsx
import { useSendPrivately, useRequestDelegation } from '@lowki/react-auth';

const { sendPrivately } = useSendPrivately();
const { hash } = await sendPrivately({ to: '0x…', amount: '12.5', asset: 'USDC' });
// The proof is built in the user's browser, and the recipient can't see who sent it.

const { requestDelegation } = useRequestDelegation();
await requestDelegation({ chain: 'sepolia', expires: '2026-12-31', rules: [{ kind: 'transfer', to: '0x…', token: 'USDC', max_per_day: '50' }] });
```

`sendPrivately` needs a wallet that announces `send_privately` in `ready.capabilities` (wallet-ui does). An older wallet makes the call fail at once with `not_supported`. The wallet shows its own "Send privately?" sheet, then answers `private_sent`, or `rejected` with its reason (for example when its Privacy Pools relayer isn't configured).

## How it works

The SDK mounts the wallet in a hidden iframe from `walletUrl` (`?app=<appId>&origin=<your origin>&embed=1`).

- **Showing the wallet.** While a request needs the user (sign-in, a signature, a transaction), the iframe is shown as a modal with a close button; Esc also closes it. Interactive requests are queued one at a time, because the wallet answers `busy` to overlapping ones.
- **Popup fallback.** Sign-in opens the wallet in a popup instead when:
  - the browser is Safari or runs on iOS, where a cross-origin iframe can't create a passkey;
  - the iframe never loaded;
  - you set `config.lowki.popup: 'always'`.

  Opening the popup happens synchronously inside `login()`, so call `login()` from a click handler.
- **Session hand-off, popup to iframe.** The popup and the iframe are separate documents, each with its own key worker; the seed never leaves the worker, so it isn't passed between them. After a popup sign-in the SDK asks the iframe with `get_session`. If the iframe has no session, the next request first sends `sign_in` to the iframe: the wallet shows its sign-in sheet and a returning user taps the passkey (`get` works in a cross-origin iframe, Safari included; only `create` needs the popup). That unlocks the iframe's worker and gives it its own server session cookie (in its own partition when the wallet is cross-site). If a different account signs in there, the request isn't sent and fails with `embedded_wallet_not_found`. The same recovery runs whenever the iframe answers `not_signed_in` while the app has a session (for example after the wallet frame reloaded).
- **Closing the modal** sends `cancel { id }` for the open request, so the wallet closes its sheet and the next request doesn't get `busy`.
- **Security, in both directions:**
  - Every message is posted with `targetOrigin` set to the wallet origin, never `*`.
  - An incoming message is accepted only if all of these hold:
    - `event.origin` equals the wallet origin;
    - `event.source` is the exact iframe or popup the SDK opened;
    - the message parses as a well-formed protocol v1 message (strict field checks, with a size cap);
    - it answers a pending request id sent on that same channel, with an answer type valid for that request.
  - `ready` must name your own `appId`.
- **Requests and timeouts:**
  - request ids are random per page plus a counter;
  - interactive requests time out after 10 minutes (`lowki.requestTimeoutMs`);
  - the handshake times out after 15 s (`lowki.handshakeTimeoutMs`).
- **What the SDK holds.** Nothing secret lives on the app side. The SDK holds only the pairwise address, the pairwise ID and the access token, in memory; nothing is written to storage.

### Errors

Every error is a `LowkiError` with these fields:

- `code`: Privy's `PrivyErrorCode` string, which is what `onError` callbacks receive;
- `rpcCode`: an EIP-1193 code;
- `reason`: the wallet's reject reason, or `timeout`, `closed`, `origin`, `not_supported` or `not_ready`.

The provider throws `ProviderRpcError` with the numeric code, so viem and wagmi map it as usual.

| Wallet / SDK condition | `code` | `rpcCode` |
|---|---|---|
| `user_rejected` (sign-in) or the modal/popup was closed | `exited_auth_flow` | 4001 |
| `user_rejected` (signature or transaction) | `unable_to_sign` | 4001 |
| `not_signed_in` | `must_be_authenticated` | 4100 |
| `unsupported_chain` (the wallet is on another chain) | `unsupported_chain_id` | 4902 |
| `invalid_request` (bad amount or asset, typed-data domain on another chain) | `invalid_data` | -32602 |
| address not this app's account | `embedded_wallet_not_found` | 4100 |
| `busy` | `too_many_requests` | -32002 |
| `policy_rejected` | `invalid_data` | -32602 |
| `network_error` | `transaction_failure` / `unknown_embedded_wallet_error` | -32603 |
| no answer in time | `client_request_timeout` | -32603 (4900 for the handshake) |
| request type not supported by this wallet | `not_supported` | 4200 |
| chain not configured | `unsupported_chain_id` | 4902 |

### Provider limits

These come from protocol v1:

- `personal_sign` signs UTF-8 text. Hex input is decoded and must be valid UTF-8; raw byte messages are refused with -32602.
- `eth_sendTransaction` passes only `to`, `value` and `data` to the wallet. `gas`, `nonce` and the fee fields are dropped, and contract deployment (no `to`) is refused.
- `send_transaction`, `sign_typed_data` and `send_privately` carry the provider's `chain_id`. The wallet sends on one chain (Sepolia by default) and answers `unsupported_chain` for any other, so `switchChain` to a chain the wallet isn't on makes sends fail with 4902 instead of going to the wrong chain. A typed-data domain whose `chainId` differs from the provider's is refused.

## Verify the access token on your backend

`getAccessToken()` returns an ES256 JWT with these claims:

- `iss`: your Lowki server URL;
- `aud`: your app ID;
- `sub`: the pairwise `did:lowki:…` ID;
- `sid`, `iat`, `exp` (15-minute lifetime by default) and `jti`.

Verify it against Lowki's JWKS, the same way a Privy backend uses `verifyAuthToken`:

```js
import { createRemoteJWKSet, jwtVerify } from 'jose';

const LOWKI = 'https://auth.example.com';            // LOWKI_PUBLIC_URL of your lowki-server
const jwks = createRemoteJWKSet(new URL(`${LOWKI}/.well-known/jwks.json`));

export async function verifyLowkiToken(token) {
  const { payload } = await jwtVerify(token, jwks, {
    issuer: LOWKI,
    audience: process.env.LOWKI_APP_ID,               // app_…
    algorithms: ['ES256'],
  });
  return { userId: payload.sub, sessionId: payload.sid, expiration: payload.exp };
}

// Express:
app.get('/api/me', async (req, res) => {
  const token = req.headers.authorization?.replace(/^Bearer /, '');
  try { res.json(await verifyLowkiToken(token)); } catch { res.status(401).end(); }
});
```

If your Privy backend stored `did:privy:…` IDs, Lowki users are new IDs (`did:lowki:…`), so map them at sign-up. Retired signing keys stay in the JWKS for one token lifetime plus 5 minutes, so key rotation needs no change on your side.

## Develop

```sh
npm install
npm run typecheck && npm run lint && npm test && npm run build
```

The type-compatibility proof and the end-to-end test live in `examples/privy-swap/`:

- **Same source, two type checks.** `src/app/*` is written as in Privy's docs and imports `@privy-io/react-auth`.
  - `tsconfig.privy.json` type-checks it against the real Privy package (3.51.3, installed for types only).
  - `tsconfig.lowki.json` type-checks the same files with that import pointed at this package's built `.d.ts`.
  - The vite build swaps the import with an alias.

  ```sh
  cd examples/privy-swap && npm install && npm run typecheck
  ```
- **End to end against the real wallet** (`e2e/real.mjs`, raw CDP, a virtual authenticator with PRF): wallet-ui with its real backend (key worker + lowki-server), a local anvil posing as Sepolia, and the example app. It checks login (email code + passkey; the JWT verifies against the JWKS), `signMessage`, `signTypedData`, `sendTransaction` (mined on anvil, from the app address), `sendPrivately` (a protocol round trip), and the Safari popup path with the hand-off to the iframe. Ports 8800–8809:

  ```sh
  examples/privy-swap/e2e/real-server.sh &        # lowki-server :8801 (VM or host), fresh database
  (cd apps/wallet-ui && LOWKI_RPC_URL=http://localhost:8800/rpc trunk build --release --dist ../../target/privy-swap-e2e/wallet-dist)
  cd examples/privy-swap && npm run build && node e2e/real.mjs   # on the macOS host (Chrome, anvil)
  ```
- **Run it yourself against the real wallet:** `examples/privy-swap/scripts/dev-real.sh` (add `ANVIL=1` for a local chain) starts lowki-server with the example's origin registered, the wallet on :8800 and the example on :8802, and prints the URL to open. Email codes appear in its output.
- **End to end against the mock and a reference wallet.** This runs headless Chrome against the wallet-ui mock backend and against a protocol-conformant reference wallet that signs with a real key (`e2e/reference-wallet`):

  ```sh
  (cd apps/wallet-ui && trunk build --release --dist ../../target/react-auth-e2e/wallet-dist)
  cd examples/privy-swap && npm run build && node e2e/run.mjs
  ```

## Protocol status

The wallet-ui protocol lives in `apps/wallet-ui/src/protocol.rs`. Every addition is optional on the wire: old messages parse unchanged, and the SDK uses a new request type only when `ready.capabilities` lists it.

Done:

1. **`send_privately { id, to, amount, asset, chain_id? }` → `private_sent { id, hash }`**, after the wallet's "Send privately?" sheet. It runs the wallet backend's private send.
2. **`capabilities: string[]` on `ready`.**
3. **`get_session { id }`**: answers `authenticated` (re-issuing an access token with less than a minute left) or `rejected { not_signed_in }`, without UI. The SDK uses it on load and in `getAccessToken`.
4. **`cancel { id }`**: closes the sheet for that request and answers it `rejected { user_rejected }`.
5. **Popup replies**: `protocol::post` takes the opener after a null check and `unchecked_into` (a cross-origin opener fails `instanceof Window`).
6. **Session hand-off between popup and iframe**: see [How it works](#how-it-works). The iframe signs itself in with the passkey; no secret crosses documents.
7. **`authenticated` carries `created_at`, `is_new_user`, and `email`** (only when the user shared it).
8. **`chain_id`** on `sign_typed_data`, `send_transaction` and `send_privately`, with the new reject reasons `unsupported_chain` and `invalid_request`.

Still open:

- `tx` has no `gas`, `nonce` or fee fields; the wallet estimates them.
- `sign_message.message` is a string, so non-UTF-8 payloads can't be signed.
- No request-time UI options like Privy's `uiOptions`.
- `created_at` is when this wallet (this browser's storage for the app) first saw the account, and `is_new_user` means "the wallet was created in this sign-in". lowki-server knows the real per-app values (`app_users.created_at`, and whether `issue_token` inserted the row) but doesn't return them from `POST /v1/apps/{id}/token` yet.
- (mock only) The mock's `sign_message` and `sign_typed_data` return FNV-hash hex, not ECDSA, and its addresses aren't derived from a key, so signatures from the mock never verify. The real backend's do.
