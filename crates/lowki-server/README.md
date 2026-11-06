# lowki-server

The self-hostable auth and user service for Lowki: passkeys, Google (or any OIDC) sign-in, email codes, sessions, Privy-style app access tokens, and storage for the opaque pieces of the user's wallet backup.

**The server never sees keys.** It stores:

- WebAuthn credential public keys;
- `HMAC(server_secret, "google:" || sub)` and `HMAC(server_secret, "email:" || email)`, never the raw subject, and the email only if `LOWKI_STORE_EMAIL=true`;
- the passkey-wrapped seed blob, which is useless without the passkey PRF;
- the server share S, one share of a 2-of-3 Shamir split, which is useless on its own.

See `docs/ARCHITECTURE.md`, "Sign-in and recovery", for where each piece fits.

## Run it

```sh
cp .env.example .env     # set LOWKI_PUBLIC_URL, LOWKI_SERVER_SECRET (openssl rand -hex 32), LOWKI_ADMIN_TOKEN
docker compose up -d     # SQLite in a volume
docker compose --profile postgres up -d   # optional Postgres; set LOWKI_DATABASE_URL=postgres://...
```

Put it behind TLS. Passkeys and `Secure` cookies need https, except on `localhost`.

To run locally without Docker:

```sh
LOWKI_DEV_MODE=true LOWKI_PUBLIC_URL=http://localhost:8080 LOWKI_SERVER_SECRET=$(openssl rand -hex 32) cargo run
```

In dev mode, email codes are printed to stdout.

Then register an app:

```sh
curl -X POST $URL/admin/v1/apps -H "authorization: Bearer $LOWKI_ADMIN_TOKEN" \
  -H 'content-type: application/json' \
  -d '{"name":"My app","allowed_origins":["https://myapp.example"],"webhook_url":"https://myapp.example/hooks/lowki"}'
# -> { "id": "app_…", "app_secret": "lsk_…" (shown once), "webhook_secret": "whsec_…", … }
```

## Environment variables

All configuration comes from the environment. Secrets never appear in logs or `Debug` output.

| Variable | Default | Meaning |
|---|---|---|
| `LOWKI_PUBLIC_URL` | **required** | Public https URL of this server. It is the JWT `iss`, the default WebAuthn origin and the base of the OAuth redirect URI. |
| `LOWKI_SERVER_SECRET` | **required** | 32+ random bytes (hex or base64). Every HMAC, the pairwise IDs, CSRF tokens and the at-rest encryption of the JWT keys derive from it. **Losing it breaks every sign-in lookup; rotating it is not supported yet.** |
| `LOWKI_DATABASE_URL` | `sqlite://lowki.db?mode=rwc` (image: `sqlite:///data/lowki.db?mode=rwc`) | SQLite or `postgres://…` |
| `LOWKI_BIND` | `0.0.0.0:8080` | Listen address |
| `LOWKI_ADMIN_TOKEN` | unset, so the admin API is off | Bearer token for `/admin/v1/*`, at least 24 characters |
| `LOWKI_DEV_MODE` | `false` | Allows `LOWKI_EMAIL_SENDER=log` and any port on WebAuthn origins |
| `LOWKI_RP_ID` | host of `LOWKI_PUBLIC_URL` | WebAuthn RP ID, e.g. `lowki.example` |
| `LOWKI_RP_NAME` | `Lowki` | WebAuthn RP name |
| `LOWKI_ORIGINS` | `LOWKI_PUBLIC_URL` | Comma-separated Lowki origins (the iframe/popup host). These may run ceremonies and call the cookie API. |
| `LOWKI_OIDC_CLIENT_ID` | unset, so OIDC is off | OAuth client ID |
| `LOWKI_OIDC_CLIENT_SECRET` | required with the client ID | OAuth client secret. It stays on the server. |
| `LOWKI_OIDC_ISSUER` | `https://accounts.google.com` | Any OIDC issuer with discovery |
| `LOWKI_OIDC_PROVIDER` | `google` | Provider ID used in URLs and in `HMAC("<id>:" \|\| sub)` |
| `LOWKI_OIDC_SCOPES` | `openid` | Only `openid` by default, so the provider doesn't hand over the email |
| `LOWKI_STORE_EMAIL` | `false` | Store the plaintext (verified) email next to the identity |
| `LOWKI_EMAIL_SENDER` | `log` in dev mode, otherwise `disabled` | `smtp`, `log` or `disabled` |
| `LOWKI_SMTP_URL` | none | e.g. `smtps://user:pass@smtp.example.com:465` |
| `LOWKI_EMAIL_FROM` | none | e.g. `Lowki <no-reply@example.com>` |
| `LOWKI_OTP_TTL_SECS` | `600` | Email code lifetime |
| `LOWKI_OTP_SENDS_PER_15MIN` | `3` | Code sends per email address |
| `LOWKI_SESSION_TTL_SECS` | `3600` | Session cookie lifetime |
| `LOWKI_REFRESH_TTL_SECS` | `2592000` | Refresh cookie lifetime (30 days) |
| `LOWKI_ACCESS_TOKEN_TTL_SECS` | `900` | App access token `exp - iat` |
| `LOWKI_SHARE_FRESH_AUTH_SECS` | `600` | How recent the bound sign-in must be for the share to be released |
| `LOWKI_BLOB_FRESH_AUTH_SECS` | `300` | How recent the passkey assertion must be to read or write the blob |
| `LOWKI_SHARE_RELEASES_PER_HOUR` | `5` | Share releases per user |
| `LOWKI_RECOVERY_DELAY_SECS` | `0` | The "Balanced" dial: delay between a recovery request and the release of S (e.g. `172800`) |
| `LOWKI_COOKIE_SECURE` | `true` if the public URL is https | `Secure` cookies with the `__Host-` prefix |
| `LOWKI_COOKIE_SAMESITE` | `lax` | `strict`, `lax` or `none` (`none` requires secure) |
| `LOWKI_COOKIE_PARTITIONED` | `false` | Adds `Partitioned` (CHIPS) for third-party iframes |
| `LOWKI_TRUST_PROXY_HEADERS` | `false` | Use `X-Forwarded-For` for per-IP rate limits |
| `LOWKI_LOG_JSON` | `false` | JSON logs |
| `RUST_LOG` | `info` | Log filter |

## Endpoints

### Cookie API (Lowki origin only)

Any non-GET request must carry an `Origin` that is one of `LOWKI_ORIGINS`; anything else gets 403. Endpoints that need a session also require the header `x-lowki-csrf: <csrf_token>`. The CSRF token comes back from every sign-in and from `GET /v1/session`. There is no CORS on these routes.

| Method and path | Auth | Purpose |
|---|---|---|
| `GET /v1/session` | optional | `{authenticated, csrf_token, expires_at, methods[], auths[], share}` |
| `POST /v1/session/refresh` | refresh cookie | Rotates both cookies (single use) |
| `POST /v1/session/logout` | – | Deletes the session and clears the cookies |
| `POST /v1/passkeys/register/start` | optional session (CSRF if present) | Creation options (`residentKey: required`, UV required, `extensions.prf: {}`) and a `flow_id`. With a session, the passkey is added to that user; without one, a user is created. |
| `POST /v1/passkeys/register/finish` | same | `{flow_id, credential}` gives a session plus `credential_id` |
| `POST /v1/passkeys/login/start` | – | Discoverable assertion options, UV required |
| `POST /v1/passkeys/login/finish` | – | `{flow_id, credential}` gives a session plus `credential_id` and `wrapped_blob` |
| `PUT /v1/passkeys/{cred_id}/blob` | session with a fresh assertion by that passkey | `{blob: base64url}` stores the wrapped seed (at most 4 KiB) |
| `GET /v1/passkeys/{cred_id}/blob` | same | `{wrapped_blob}` |
| `GET /v1/oauth/{provider}/start[?link=1]` | optional session | Opens in the popup and redirects to the provider (PKCE S256, `state`, `nonce`) |
| `GET /v1/oauth/{provider}/callback` | binding cookie | Exchanges the code, verifies the ID token, sets the session, then `postMessage({type:"lowki:oauth", ok, new_user, linked, method_id \| error})` to `window.opener` on the Lowki origin |
| `POST /v1/email/start` | – | `{email}` sends a 6-digit code |
| `POST /v1/email/verify` | – (CSRF when `link: true`) | `{email, code, link?}` gives a session |
| `GET /v1/me/methods` | session | Sign-in methods |
| `DELETE /v1/me/methods/{id}` | session, a fresh sign-in, CSRF | Unlinks a method. It refuses to remove the last method or the one guarding the share. |
| `GET /v1/me/share` | session | Share metadata only (whether it exists, the bound method, any pending recovery) |
| `PUT /v1/me/share` | session, CSRF, a fresh Google/email sign-in | `{share: base64url}` stores S and binds it to that method |
| `POST /v1/me/share/release` | session, CSRF, a fresh sign-in with the **bound** method | `{status:"released", share}`, or `202 {status:"pending", available_at}` when the recovery delay is on |
| `DELETE /v1/me/share/recovery` | session, CSRF, a fresh passkey sign-in | Cancels a pending recovery |

### App tokens

`POST /v1/apps/{app_id}/token` returns `{access_token, token_type:"Bearer", expires_in, user_id, app_id}`. You can call it two ways:

- **from the Lowki iframe** (Lowki origin): send the CSRF header, and set `origin` in the body to the parent page's origin, which must be registered for the app;
- **directly from a registered app origin**, using `fetch(..., {credentials: "include"})`. CORS allows only that app's registered origins, and the browser-set `Origin` is the CSRF defence.

### Public

- `GET /.well-known/jwks.json`: ES256 public keys. Retired keys stay published for one token lifetime plus 5 minutes.
- `GET /.well-known/openid-configuration`: `issuer` and `jwks_uri`.
- `GET /healthz`

### Admin (`Authorization: Bearer $LOWKI_ADMIN_TOKEN`)

| Method and path | Purpose |
|---|---|
| `POST /admin/v1/apps` | `{name, allowed_origins[], webhook_url?}` gives the app plus `app_secret` and `webhook_secret`, both shown once |
| `GET /admin/v1/apps`, `GET/PATCH /admin/v1/apps/{id}` | List, read or update an app (name, origins, webhook) |
| `GET /admin/v1/apps/{id}/users?limit&offset` | The app's users, by **pairwise ID only** |
| `POST /admin/v1/keys/rotate` | Rotates the JWT signing key |
| `GET /admin/v1/audit?limit&event` | Audit log: sign-ins, links, share and blob releases and denials, app changes, key rotations. Metadata only. |

### App server API (`Authorization: Basic base64(app_id:app_secret)`)

- `GET /api/v1/users/{pairwise_id}` returns `{id, created_at, last_seen_at, linked_account_types[]}` for users of that app only.

### Webhooks (stub)

Events go to an outbox table and get one best-effort POST. Each delivery carries these headers:

- `lowki-webhook-id`;
- `lowki-webhook-timestamp`;
- `lowki-webhook-signature: v1=hex(HMAC-SHA256(webhook_secret, id + "." + timestamp + "." + body))`.

The only event so far is `user.created` (`{user_id: <pairwise>}`), sent the first time a user gets a token for the app. Retries are a TODO.

## Access token claims

Header: `{"alg":"ES256","typ":"JWT","kid":"…"}`

| Claim | Value |
|---|---|
| `iss` | `LOWKI_PUBLIC_URL` without the trailing slash |
| `aud` | the app ID (`app_…`) |
| `sub` | pairwise user ID: `did:lowki:` + 32 hex characters, stable per (user, app) and unlinkable across apps |
| `sid` | per-app session identifier (also pairwise) |
| `iat`, `exp` | issued-at and expiry; the default lifetime is 15 minutes |
| `jti` | random token ID |

How `sub` is derived:

- `user_key = HMAC(K_pairwise, "lowki/pairwise-user/v1" ‖ user_salt)`;
- `sub = "did:lowki:" ‖ hex(HMAC(user_key, "lowki/pairwise-sub/v1" ‖ app_id)[0..16])`.

Here `user_salt` is 32 random bytes per user, and `K_pairwise = HKDF(LOWKI_SERVER_SECRET, "lowki/pairwise/v1")`. Every HMAC input is length-prefixed. A database dump alone can't compute or link the IDs.

### Verifying on your backend (like Privy's `verifyAuthToken`)

```js
import { createRemoteJWKSet, jwtVerify } from 'jose';

const LOWKI = 'https://auth.example.com';            // LOWKI_PUBLIC_URL
const jwks = createRemoteJWKSet(new URL(`${LOWKI}/.well-known/jwks.json`));

export async function verifyLowkiToken(token) {
  const { payload } = await jwtVerify(token, jwks, {
    issuer: LOWKI,
    audience: process.env.LOWKI_APP_ID,
    algorithms: ['ES256'],
  });
  return { userId: payload.sub, sessionId: payload.sid, expiration: payload.exp };
}
```

This mirrors how a Privy backend verifies Privy tokens: swap Privy's verification key and issuer for Lowki's JWKS and issuer, and `did:privy:…` becomes `did:lowki:…`. `tests/node/verify-jose.mjs` is the same check, and the test suite runs it against a real token.

## Security notes

- **Sessions:** a random 256-bit token in an `HttpOnly` cookie (`__Host-` prefix and `Secure` behind TLS). Only SHA-256 hashes are stored. Refresh rotates both tokens and is single use.
- **Step-up:** each session records every sign-in done in it, with a timestamp:
  - S needs a recent sign-in with the bound Google or email method;
  - the blob needs a recent assertion with that exact passkey.

  Google alone never gets the blob, and a passkey alone never gets S.
- **OIDC:** the flow uses PKCE S256, a single-use `state` stored as a hash, a browser-binding cookie (`SameSite=Lax`) and a `nonce`. The ID token is checked for its signature against the cached JWKS (refetched on an unknown `kid`), `iss`, `aud`, `exp` (60 s leeway) and `nonce`. HS* algorithms are refused.
- **Email codes:** six uniform digits, an HMAC at rest, constant-time comparison, single use, five wrong guesses per code, and per-email and per-IP limits.
- **PRF:** registration options include `extensions.prf = {}`, and the client adds `eval`. Clients must delete `clientExtensionResults.prf.results` before posting. If they don't, the server drops it on receipt, logs a warning, and never stores or logs the values.
- **Logs:** only the method and path, never query strings (OAuth `code` and `state`), bodies, cookies or secrets.
- **Rate limits:** kept in memory, so they apply per instance.

## Tests

```sh
export CARGO_TARGET_DIR=../../target/lowki-server
(cd tests/node && npm ci)        # optional: enables the node `jose` check
cargo test                       # SQLite
LOWKI_TEST_DATABASE_URL=postgres://user@127.0.0.1:5432/postgres cargo test   # each test gets a fresh Postgres DB
cargo clippy --all-targets -- -D warnings
tests/e2e/run.sh                 # headless Chrome + CDP virtual authenticator (ctap2.1, UV, PRF)
```

The integration tests (`tests/api.rs`) run the server in-process against a **mock OIDC issuer** that signs RS256 ID tokens with a key generated per test. They cover:

- the full popup OIDC flow, plus bad nonce, `aud`, `iss`, `exp` and `kid`, and replayed or unbound state;
- email OTP: the flow, expiry and the attempt cap;
- JWT issuance, verification against the JWKS (in Rust and in node `jose`) and key rotation;
- pairwise IDs: stable within an app and different across apps;
- share release refused without the right session, a fresh sign-in or the bound method;
- the recovery delay;
- rate limits;
- CSRF, origin and CORS rules;
- linking and unlinking methods;
- the admin and app APIs.

## ENS stealth gateway (ENSIP-30, CCIP-Read)

`src/ens_gateway.rs` turns lowki-server into the ERC-3668 gateway behind `<label>.lowki.eth`
names. The names use Fluidkey's `StealthResolver` in **verified** mode (see
`contracts/README.md`): for an address lookup the resolver reverts with `OffchainLookup` to this
gateway, the gateway answers with a fresh stealth address plus the data needed to check it, and
the resolver's callback re-derives the address on chain from the name's published meta-address.
If the gateway lies, resolution reverts. Wallets need nothing special: plain viem
`getEnsAddress({ name })` gets a new address on every lookup.

### Enable it

| Variable | Default | Meaning |
|---|---|---|
| `LOWKI_ENS_GATEWAY` | `false` | Turns the gateway on |
| `LOWKI_ENS_RPC_URL` | **required** | JSON-RPC of the ENS chain (Sepolia). May contain an API key; it is never logged. |
| `LOWKI_ENS_UNIVERSAL_RESOLVER` | **required** | From `contracts/deployments/sepolia-ensv2.json` (`.ens.UniversalResolver`) |
| `LOWKI_ENS_PARENT` | **required** | e.g. `lowki.eth`. Served names are exactly one label below it. |
| `LOWKI_ENS_STEALTH_RESOLVER` | unset | If set (recommended; `.StealthResolver` in `contracts/deployments/sepolia-lowki.json`), the name must resolve through it (checked on chain). The request's `sender` is not checked: through the UniversalResolver's batching it is the UR proxy, and it is unauthenticated anyway. |
| `LOWKI_ENS_LOOKUPS_PER_MIN_IP` | `30` | Lookups per client IP per minute |
| `LOWKI_ENS_LOOKUPS_PER_HOUR_NAME` | `600` | Lookups per name per hour (bounds how fast anyone can grow a recipient's record list) |
| `LOWKI_ENS_RECORDS_PER_MIN_IP` | `30` | Records-endpoint calls per IP per minute |
| `LOWKI_ENS_META_CACHE_SECS` | `30` | How long an on-chain meta-address read is reused |
| `LOWKI_ENS_AUTH_SKEW_SECS` | `300` | Accepted clock skew on records-auth timestamps |
| `LOWKI_ENS_RECORDS_PAGE` | `500` | Records per response (page with `since`) |

The URL written on chain (`StealthResolver.setConfig`) is `https://<host>/v1/ens/gateway`, the
POST form: clients send `{"sender","data"}` in the body, so looked-up names never appear in URLs
or proxy access logs. The EIP-3668 GET form `https://<host>/v1/ens/gateway/{sender}/{data}.json`
works too; the server logs its path as the route only.

### Endpoints

| Method and path | Auth | Purpose |
|---|---|---|
| `POST /v1/ens/gateway` (`{sender, data}`) and `GET /v1/ens/gateway/{sender}/{data}.json` | none (CORS `*`, as ERC-3668 requires) | `data` must be `resolve(bytes name, bytes addrCall)` for `<label>.<parent>` with `addr(node)` or `addr(node, coinType)` (EVM coin types only). Answers `{"data": abi.encode(address stealthAddress, bytes verificationData)}`, `verificationData = e ‖ S.x ‖ S.y` (Scheme1Verifier's format), `Cache-Control: no-store`. Errors are `{"message"}` with 4xx/5xx. |
| `GET /v1/ens/records?name=<name>&since=<unix>` | `x-lowki-auth-timestamp: <unix>` and `x-lowki-auth-signature: 0x<65 bytes>`: EIP-191 `personal_sign` by the name's **viewing key** of `"Lowki stealth records\nname: <name>\ntimestamp: <unix>"` (`lowki_ens::stealth::records_auth_message`) | `{name, scheme_id, records: [{stealth_address, coin_type, ephemeral_pubkey, view_tag, created_at}], complete}` |

Per lookup the gateway:

1. checks the calldata, the name (one normalized label under the parent), the node
   and the coin type, then the per-IP and per-name limits;
2. reads `data(node, "stealth-meta-address[1][<coinType>]")` **on chain** through the
   UniversalResolver, and checks that the answering resolver is the configured `StealthResolver`. Names without the record are refused: Lowki serves verified mode only;
3. draws an ephemeral key from the OS RNG, derives the address with lowki-stealth
   (`lowki_ens::stealth::gateway_derive`), and builds the verification data;
4. stores `(stealth address, HMAC(name), coin type, ephemeral public key, view tag, time)` and
   answers only after the insert succeeds (ENSIP-30 §6: the recipient must be able to find every
   address the gateway hands out). The ephemeral private key is dropped after the response is
   built; it is never stored or logged.

**Why the records endpoint is authenticated by the viewing key.** The records are useful exactly
to whoever can scan them, which is whoever holds the viewing key. Signing with it proves that
without a Lowki session or the name-owner key, and lets a watch-only scanner (viewing key only)
fetch them. The server takes the viewing public key from the on-chain meta-address, so a
rotation of the record rotates access too (records created under an old meta-address are then
reachable only through the database).

### Does the gateway post ERC-5564 announcements? No.

Measured on the Sepolia fork, `Announcer.announce` costs **33,688 gas** with a view-tag-only
metadata byte and **35,736 gas** with the 57-byte ETH metadata layout, about 0.00002 ETH per
lookup at a 0.65 gwei mainnet base fee. The gateway would have to pay that for every lookup,
and:

- **Lookups are not payments.** Resolution is a free `eth_call`; most lookups (wallet previews,
  address-book refreshes, retries) are never paid. Announcing each one pays for nothing, and lets
  anyone drain the operator's gas budget by calling `getEnsAddress` in a loop.
- **Privacy gets worse, not better.** Every announcement would come from the gateway's address,
  tagging each announced stealth address as "a lowki.eth payment address" on chain and splitting
  it from the shared Announcer anonymity set. Announcing at lookup time also puts the lookup's
  timestamp on chain, next to the payment that follows it.
- **Detection does not need it.** The recipient finds payments from the gateway records (above),
  which ENSIP-30 §6 explicitly allows ("for example by deriving ephemeral secrets that the
  recipient can regenerate, or by publishing the ERC-5564 announcement"). Senders who derive
  client-side (the record's flag permits it) announce themselves, as ERC-5564 intends.

The cost of this choice is availability: if the gateway's database is lost, payments to addresses
it handed out can only be found by scanning chain state (balances) with the viewing key, which
needs the ephemeral public keys. Back up `ens_stealth_records`.

### What the gateway operator learns

Plainly, whoever runs this gateway (or can read its traffic or database) learns:

- **Every lookup:** which name, when, from which IP (IPs are used for rate limiting in memory and
  are not stored), and for which chain.
- **Every address handed out for a name.** The operator can link all of a name's stealth addresses
  to each other and to the name, and so watch every payment to them on chain. Stealth addresses
  hide the recipient from the public and from senders of other payments, **not from the gateway
  operator**. Senders who want the operator out of the picture derive client-side from the
  on-chain record (ENSIP-30 §3).
- **The ephemeral private key of each answer** (it is in the response, as the on-chain verifier
  needs it). Together with the public viewing key this gives nothing beyond the address the
  operator already chose; combined with one leaked stealth private key it would give the spending
  key, which is why it is never stored.

The operator **cannot**: spend (no spending or viewing private key), redirect funds (verified mode:
the chain rejects any address not derived from the published meta-address), or read the records
without the viewing key through the API. It **can**: refuse or delay lookups (availability), or
reuse an ephemeral key to hand the same address to two senders, linking their payments (ENSIP-30
calls this out; it cannot be prevented on chain).

The database stores names only as `HMAC(server key, "lowki/ens-name/v1" ‖ name)`. Logs contain
method and route only: no names, addresses, calldata or keys.

### Tests

`src/ens_gateway.rs` unit tests (name and coin-type rules, redacted config `Debug`) and
`tests/ens_gateway.rs` (the HTTP surface against a mock JSON-RPC chain: verified-answer format
re-checked with lowki-ens' verifier, a new address per lookup, records stored, refusals, records
auth by viewing key, rate limits). The full flow against real contracts is
`contracts/e2e/run.sh`.

## Google Cloud setup (operator)

1. In Google Cloud Console, open **APIs & Services → OAuth consent screen**. Choose External, set the app name, and the only scope needed is `openid`. Publish the app; while it's in "Testing", only test users can sign in.
2. Open **Credentials → Create credentials → OAuth client ID** and choose **Web application**.
   - **Authorized redirect URI:** `https://<LOWKI_PUBLIC_URL host>/v1/oauth/google/callback` (exact match).
   - **Authorized JavaScript origins:** not needed. The code exchange happens on the server.
3. Put the client ID and secret in `LOWKI_OIDC_CLIENT_ID` and `LOWKI_OIDC_CLIENT_SECRET`.

## Known gaps / TODO

- Rate limits are per process. Running multiple replicas needs a shared store.
- Webhook retries, a delivery log and a recovery notification (email or webhook) for the Balanced dial are not built yet.
- Rotating `LOWKI_SERVER_SECRET` is not supported: identity lookups, pairwise IDs and the sealed JWT keys all depend on it.
- A share can't be rebound to a different method. The method guarding it can't be unlinked.
- Apple sign-in, which needs a `form_post` callback and a JWT client secret, is not implemented. Any standard OIDC provider works through `LOWKI_OIDC_ISSUER`, one provider per deployment.
