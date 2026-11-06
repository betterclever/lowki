# Lowki architecture

Lowki is a privacy-first embedded wallet with Privy-compatible APIs. Businesses can self-host it. All key handling and cryptography is Rust compiled to wasm; JavaScript is only bindings.

Status of each piece is at the end. Decisions here were made during ETHGlobal Mumbai (Nov 2026).

## Goals

- **Drop-in for Privy.** The same React hooks (`usePrivy`, `useWallets`, `useSigners`, sign/send) and the same server concepts (authorization keys, key quorums, policies, wallet RPC).
- **The user's master key never leaves their device.** No Lowki server, operator or enclave can sign with it.
- **Apps can't link users.** Each app gets a different address and a pairwise user ID.
- **Nothing on chain reveals Lowki.** Delegations, policies and signers stay off chain. Every signature is a plain EOA or ed25519 signature.
- **Private money is built in.** Users can shield into Privacy Pools through Kohaku, send privately with a browser proof, and receive at stealth addresses.

## Components

```
Browser
  @lowki/react-auth      Privy-shaped hooks, EIP-1193 provider, wagmi connector (bindings)
  Lowki iframe/popup     Leptos UI + Rust wasm worker (seed, signing, proving)

Servers (self-hostable, one docker compose)
  lowki-server           auth (passkey, email OTP, Google/Apple with the operator's OAuth client),
                         users, JWT + JWKS, encrypted share storage, webhooks, admin API
  lowki-signer           runs in a TEE: sealed delegated keys, app-owned server wallets,
                         policy engine, authorization keys, key quorums, Privy-shaped REST API
  bundler + paymaster    gas sponsorship (optional, or Pimlico)
```

## Keys

- **Seed:** 32 bytes of entropy (24 words) generated in the Rust worker, turned into a BIP-39 mnemonic and then a BIP-32 seed.
- **Unlock:** the passkey PRF goes through HKDF ("lowki/kek/v1") to give an AES-GCM key, which wraps the entropy. The wrapped blob is `0x01 || nonce12 || ct+tag`. It can be stored anywhere because it is useless without the passkey.
- **Same wallet on every device:** the passkey lives on the Lowki origin, so one RP ID covers every app. Passkey sync (iCloud, Google Password Manager) carries the wallet across devices.
- **Per-app account:** `index = u32_be(keccak("lowki/app/v1" || origin)[0..4]) & 0x7fffffff`, derived on the device.
- **Privacy Pools keys:** at Kohaku's `m/28784'/1'/...`.
- **Stealth meta-address:** ERC-5564 scheme 1, registered through ERC-6538.
- **Hygiene:** keys stay as bytes in memory that is zeroized after use. They are never formatted into strings.

## Sign-in and recovery

**The passkey is the master key.** Google (or email) plus a recovery code is the backup path. Google alone opens nothing.

### First sign-in with Google

1. The user starts sign-in, which opens a popup on the Lowki origin. Google doesn't allow its sign-in page inside iframes.
2. The popup sends the user to Google using PKCE, `state` and `nonce`.
3. lowki-server exchanges the code with the client secret, which stays on the server, and verifies the ID token: JWKS, `aud`, `iss` and `nonce`.
4. The server stores `HMAC(server_secret, "google:" || sub)`. It does not store the email.
5. The worker generates the seed. The user creates a passkey, and the PRF wraps the seed.
6. The worker splits the seed 2-of-3 with Shamir. The scheme is byte-wise GF(2^8), compatible with Privy's `shamir-secret-sharing`, with 33-byte shares for 32-byte entropy:
   - **P:** wrapped by the passkey.
   - **S:** held by the server and released only for a valid Google session.
   - **R:** a recovery code, or a backup to iCloud or Drive.
7. The worker wipes the seed. The app receives a Lowki access token whose `sub` is a pairwise app user ID, and verifies it against Lowki's JWKS.

### Later sign-ins

- **Every day:** a passkey tap; the wallet fetches its encrypted seed blob from lowki-server (not yet cached on the device), the PRF unwraps it, and the user signs. The server stores the blob but can never open it.
- **New device with a synced passkey:** a single tap.
- **New device without the passkey:** Google releases S, the user enters the recovery code R, the worker rebuilds the seed, and the user creates a new passkey.
- **Google account compromised:** the attacker gets only S, and one share opens nothing.

### Security dial for self-hosted deployments

| Mode | Shares | Who could sign without the user |
|---|---|---|
| Easy | device + server + KMS-wrapped recovery released on login | the operator, by misusing its own KMS |
| Balanced | passkey + server + social or email recovery, with a 48 h delay and notification | nobody immediately; an insider takeover is visible and can be cancelled |
| Self-custody | passkey + recovery code | nobody |

## Delegation (Privy "signers") without anything on chain

When a server signs while the user is offline, some party other than the user must be able to sign. Off chain, that policy can only be enforced by attested code. So Lowki signs locally by default and delegates through an attested signer only when the user opts in.

### Setup

1. The app calls `addSigners({ address, signers: [{ signerId, policyIds }] })`.
2. The user approves a consent sheet that turns the policy into plain sentences.
3. The worker re-derives that one app account key. It is the same key and the same address the app already sees.
4. The worker verifies the signer enclave's attestation against the published reproducible build.
5. The worker seals the key to the enclave with HPKE, together with a grant signed by the account key: `{address, signerId, policyIds, expiry, nonce}`.
6. The seed, other app accounts and the private vault are never delegated.

### Each request

1. The app server signs an RFC 8785-canonical request with its P-256 authorization key.
2. The enclave checks the authorization signature, the quorum threshold, the grant (expiry, revocation), the policy (chain, target, selector, decoded calldata, typed-data fields, caps) and the idempotency key.
3. The enclave signs and records the spend.

### Revocation and the second mode

- **Revocation:** the enclave deletes the sealed key. This relies on trust in attested code. For a hard guarantee the user rotates the app account.
- **Sub-account mode (optional):** the user delegates a separate budget account instead. Revocation is then cryptographic, but the address is different from the user's own, so this mode is not Privy-compatible.

### Risks to state plainly

- The delegated account key does leave the device, sealed in a TEE. TEEs have been broken before.
- Policy-engine bugs are real bugs.
- The operator can refuse or delay delegated signing, but cannot steal.
- On EVM, the signer coordinates nonces for delegated accounts.
- **Roadmap mitigation:** two independent co-signers using threshold signatures (DKLs23 for ECDSA, FROST for ed25519), so the key never exists in one place.

### Who sees what

| Party | Sees | Never sees |
|---|---|---|
| Chain | ordinary signatures | any delegation, policy or signer |
| App | its own account, pairwise ID, its own requests | identity, other apps, the vault |
| Signer enclave | delegated requests for one account | local signing, other apps, the seed |
| Operator | encrypted blobs, its own app's users | keys, local transactions |
| Across apps | (each business runs its own signer) | no single party sees across apps |

## Private payments

- **Privacy Pools v1** (0xbow) runs through Kohaku. It supports any amount and partial withdrawals. An ASP approval set proves a deposit isn't illicit, and ragequit is an exit that can't be blocked.
- **Shield:** a deposit with signature-derived secrets.
- **Private send:**
  1. Sync the tree and build a LeanIMT Merkle proof.
  2. Prove the withdraw circuit (Groth16) in the browser.
  3. Submit through the Privacy Pools relayer, or through a 4337 paymaster from an ephemeral 7702 account.
- **Sepolia:** Kohaku has no Privacy Pools paymaster configured there. Two options:
  - **A:** use the testnet relayer at `testnet-relayer.privacypools.com`, which is live with a 10 bps fee.
  - **B:** deploy `privacy-paymaster` and stake it ourselves.

  The plan is A first, then B as an upgrade.
- **Landscape on Ethereum** (DefiLlama, Nov 2026):

  | Protocol | TVL | Notes |
  |---|---|---|
  | Tornado Cash | $722M | |
  | Railgun | $103M | Kohaku's TS package is stubbed. A Rust rebuild is on the kohaku-rs `feat/kohaku-railgun` branch. |
  | Privacy Pools V1 | $8.9M | |

## Performance (Chrome 156, Apple M5 Max)

| Operation | Lowki | Reference |
|---|---|---|
| Withdraw proof | WebGPU 263 ms; CPU wasm 2.05 s | snarkjs: 581 ms with 18 workers, 4.41 s single-thread |
| Commitment proof | 63 ms | snarkjs: 40 ms |
| Stealth scan | 6.2× faster | ScopeLift SDK |
| mnemonic → key | 4.7× faster | Kohaku |
| secp256k1 signing | 1.9× slower | noble |
| Poseidon / LeanIMT | 1.2× slower; 10× slower than Kohaku's 16-worker tree build at 100k leaves | Kohaku |

- **Witness:** generation runs on the browser's own WebAssembly engine (25 ms); native builds use wasmi.
- **Remaining bottleneck:** the H-polynomial FFTs, about 176 ms.
- **Cross-origin isolation:** none of these numbers depend on it. snarkjs workers and WebGPU both work in a non-isolated iframe.

Full method: `bench/RESULTS.md`.

## How Privy works (for compatibility)

- **TEE mode (default):**
  - 128-bit entropy is split 2-of-2 between an enclave share (in AWS Nitro) and an auth share stored by Privy.
  - Requests are signed with a time-bound P-256 user key. Privy issues that key in exchange for the user's access token, and the SDK obtains and uses it inside Privy's iframe.
  - The access token is the real credential: Privy's Node SDK can sign with `user_jwts`.
- **On-device mode:** device, auth and recovery shares, 2-of-3, combined in the iframe.
- **Signers:**
  - Authorization keys are P-256.
  - Key quorums carry a threshold, and policies are enforced in the enclave.
  - Requests are canonicalized with RFC 8785 and the P-256 signature goes in the `privy-authorization-signature` header.

Explainer: `docs/privy-signing.html`.

## Existing alternatives

| Product | Self-hosted | Notes |
|---|---|---|
| OpenSigner (Openfort) | yes | MIT, Shamir 2-of-3 device/hot/cold. A key layer only: no Privy API, no cross-app privacy, no private payments |
| Turnkey, Web3Auth, thirdweb, Dynamic, Para, Magic | no | hosted key infrastructure |
| Porto | no | archived in Aug 2026 |

## Status

| Piece | State |
|---|---|
| lowki-keys, lowki-stealth, lowki-pp-core, lowki-aa | done; differential vectors |
| lowki-groth16 + lowki-gpu | done; byte-identical to CPU, accepted by snarkjs |
| Wallet UI (Leptos, mocked backend) | in progress |
| lowki-worker (crates combined, Kohaku Host) | next |
| Sepolia end-to-end run (shield, sync, prove, withdraw) | next; biggest demo risk |
| Passkey PRF spike (Chrome, Safari) | next |
| lowki-server, @lowki/react-auth | next |
| lowki-signer (policy engine, Privy-shaped API) | after; attestation stubbed for the demo |
| Solana, threshold co-signers, Railgun plugin | roadmap |

## Kohaku pieces to reuse

- `@kohaku-eth/plugins`: the Host interface and scoped keystore.
- `@kohaku-eth/provider`: the anon, Helios and Colibri providers. Lowki needs one of these so its RPC provider doesn't see user addresses.
- `@kohaku-eth/social-recovery`.
- kohaku-rs `userop-kit`: use it if it builds for wasm.
