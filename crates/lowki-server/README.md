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
