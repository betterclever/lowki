# ETHGlobal submission: ready-to-paste fields

Replace the two placeholders before submitting: `<GITHUB_URL>` (after the push below) and
`<VIDEO_URL>`. Live demo links are in `docs/DEMO.md` (top table). They are Cloudflare quick tunnels,
so they only work while the laptop runs `scripts/demo-up.sh`.

---

## Project name

**Lowki**

## Tagline (≤ 100 characters)

> Privy, but private: the same embedded-wallet SDK, keys on your device, unlinkable apps, private payments.

## Short description (≤ 280 characters)

> Lowki is a drop-in replacement for Privy's React SDK: change one import. Keys are made and used on the device in Rust/wasm and wrapped by the passkey's PRF. Each app gets its own unlinkable wallet. Tips go to ENS stealth names and through Privacy Pools, proved on the GPU in 0.17 s.

## Description (long)

Embedded wallets made onboarding easy, but at a cost. One company holds a share of every user's
keys, knows every app each user signs into, and every app can see the user's whole on-chain history.
**Lowki keeps the developer experience of Privy and removes the trust.**

- **Same SDK.** `@lowki/react-auth` implements Privy's React surface: `PrivyProvider`, `usePrivy`,
  `useWallets`, `useSignMessage`, `useSendTransaction`, … To move an app over, change one import.
  Our demo app, Lowki Tips, was written against Privy, and we also run a Privy app written straight from Privy's docs
  (`examples/privy-swap`) through a one-line alias; the same source type-checks against the real `@privy-io/react-auth`. App backends verify an ES256 access token
  against Lowki's JWKS, as they would Privy's.
- **Keys on your device.** The seed is generated and used inside a Rust/wasm Web Worker. It is
  wrapped with a key derived from the passkey's WebAuthn PRF output, so the server stores a blob it
  can't open. Backup is a 2-of-3 Shamir split: passkey, server share, recovery code. Signing happens
  only after the wallet's own confirmation sheet.
- **One wallet per app.** Each app origin gets its own address (derived on the device) and a
  pairwise user ID. Two apps can't link the same user through Lowki. Only the user sees their app
  list (stored encrypted).
- **Private payments.** Tip a creator at `chai.lowki.eth`: the ENSv2 name resolves to a fresh
  ERC-5564 stealth address on every lookup, verified on chain by the resolver (CCIP-Read), so a
  public name doesn't become a public payment history. Or tip from a private balance: Privacy Pools
  v1 through Kohaku's plugin, with the Groth16 withdrawal proof made in the browser on WebGPU in
  168 ms (3.6× faster than snarkjs on 18 web workers).
- **Private connection.** Wallet reads, the ASP fetch and the relayer call go through Anon-RPC
  (Nox, Nym or tor-js, via Kohaku's provider interface), so the RPC can't see the user's IP.
- **Self-hostable, open source.** One server binary (`lowki-server`, SQLite or Postgres) is the
  auth service, the token issuer and the ENS gateway. Everything is MIT/Apache-2.0. The client loads
  no third-party scripts, fonts or trackers.

What's live: `lowki.eth` and its ENSv2 contracts, and the creator names `chai`, `tabla` and
`pixel.lowki.eth`, on **Sepolia**. Any wallet resolves them to a new verified address each time. The
private withdrawal runs on a Sepolia fork, because Sepolia's Privacy Pools ASP publishes
placeholder data. We tried to rebuild its root from chain data and could not (see
`docs/PRIVATE-SEND.md`). On the fork, the ASP and the relayer are simulated; the contracts, the
proof and Kohaku are real.

## How it's made

**Rust everywhere keys or proofs are involved; TypeScript only as bindings.** The repository is a
set of small crates, each tested against differential vectors from the TypeScript reference
implementations (`vectors/*`):

- `lowki-keys`: BIP-39/32, per-origin account derivation, and passkey-PRF key wrapping
  (HKDF → AES-GCM).
- `lowki-shares`: 2-of-3 Shamir over GF(2^8), byte-compatible with Privy's `shamir-secret-sharing`.
- `lowki-stealth`: ERC-5564 scheme 1 stealth addresses and scanning. Scanning 10k announcements is
  6.2× faster than ScopeLift's SDK.
- `lowki-ens`: ENSIP-30 stealth-name records, CCIP-Read client and verification, and the calldata
  and EIP-712 signatures for our registrar and owner account.
- `lowki-pp-core`: Privacy Pools v1 notes, Poseidon and LeanIMT.
- `lowki-groth16` + `lowki-gpu`: a Groth16 prover for the Privacy Pools circuits. It runs on the CPU
  (wasm) and on **WebGPU** (MSMs and the QAP polynomial work in WGSL compute shaders), with
  byte-identical output, accepted by snarkjs and by the on-chain verifier. Chrome 156, Apple M5 Max:
  withdraw proof **168 ms on WebGPU** against snarkjs's 603 ms with 18 workers; 2.05 s on one CPU
  thread against 4.38 s single-threaded. Artifacts are sha256-pinned.
- `lowki-aa`: ERC-4337 / EIP-7702 user operations.
- `lowki-worker`: the wasm key worker that holds the seed, unlocks with the PRF, derives per-app
  accounts, signs, and proves.
- `lowki-server` (axum, sqlx): passkeys via webauthn-rs (including passkeys created inside an app's
  cross-origin iframe, accepted only under a registered app origin), email codes, OIDC, sessions,
  ES256 app tokens with rotating keys and a JWKS, Shamir share storage with fresh-auth release, and
  the **ENS CCIP-Read gateway** with its stealth-records API, authenticated by the name's viewing key.

**Wallet UI:** Leptos 0.8 compiled to wasm, running in an iframe (or a popup) on the wallet's own
origin. It talks postMessage to the SDK and has a strict CSP.

**Privy-compatible SDK** (`@lowki/react-auth`): Privy's provider, hooks and types, an EIP-1193
provider and a wagmi connector over the iframe. One addition, `useSendPrivately`. Privy-only config
keys are accepted and ignored, so they make no network request.

**Kohaku:** `@lowki/kohaku-host` implements Kohaku's `Host` (network, storage, keystore, provider)
on the Lowki worker. Kohaku's Privacy Pools v1 plugin runs unchanged on top of it: sync, shield,
`prepareUnshield`, `PrivacyPoolsBroadcaster`, relayer client. Its prover is swapped for our
WebGPU one through `proverFactory`. Its provider interface carries **Anon-RPC**
(`@anon-rpc/browser-harness`: Nox/Nym/tor-js), with one isolated session per app address.

**ENSv2:** `lowki.eth` is registered through ENSv2's `ETHRegistrar` on Sepolia. Its subregistry is
a `VerifiableFactory` proxy of ENS's canonical `UserRegistryImpl` (a `PermissionedRegistry`),
emancipated so the Lowki admin can't touch a registered name. Our `LowkiRegistrar` does sponsored
registrations: the user signs an EIP-712 `Register`, and an operator pays. `LowkiOwnerAccount` is an
EIP-7702 delegate, so the name owner's whole setup is one sponsored transaction and the owner key
never holds ETH. That setup deploys the owner's own `PermissionedResolver` and configures Fluidkey's
`StealthResolver` (ENSIP-30 draft) in verified mode. Resolution is plain CCIP-Read:
`getEnsAddress` → `OffchainLookup` → lowki-server derives a fresh stealth address →
`StealthResolver` re-derives it on chain from the published meta-address, and rejects it if the
gateway lied.

**The demo stack:** anvil forks of Sepolia; a fork ASP (impersonating the test postman, as Kohaku's
tests do) and a fork relayer; Cloudflare tunnels so the anon network's exits can reach the fork. It
is tested end to end in headless Chrome with a virtual passkey (PRF) and WebGPU.

## Sponsor sections

### Ethereum Foundation: privacy, CROPS, Kohaku, Anon-RPC

- **CROPS, measured honestly** (README, "CROPS"):
  - **Censorship resistance:** every server piece is self-hostable, addresses are plain EOAs, and
    ragequit is an exit no ASP or relayer can block. The ENS gateway can refuse to answer, but it
    can't redirect funds (verified mode), and senders can derive from the on-chain meta-address
    without it.
  - **Open source:** MIT/Apache-2.0. Third-party pieces (Kohaku, Fluidkey's StealthResolver, the ENS
    contracts, the Privacy Pools circuits) are pinned and unmodified.
  - **Privacy:** a per-party table of what each party learns and doesn't learn (app, operator, RPC,
    relayer, ASP, ENS gateway, email provider, the chain).
  - **Security:** Rust for every key operation, zeroized bytes, passkey-gated unlock.
  - Gaps are listed too, for example that the wallet origin's operator must be trusted for code
    delivery.
- **Kohaku as the privacy layer of a mainstream wallet SDK.** Kohaku's Privacy Pools plugin runs
  unchanged on a Host whose keystore, envelope signing and proving live in a Rust/wasm worker. We
  replaced its prover with a WebGPU Groth16 that is 3.6× faster than snarkjs, and checked our shield
  calldata byte for byte against Kohaku's `prepareShield`.
- **Anon-RPC in the wallet.** Wallet reads, broadcasts, ASP fetches and relayer calls go over Nox
  (Nym and tor-js are also wired) through Kohaku's provider interface, with one session per app
  address so the exits can't group a user's addresses. The wallet shows "Private connection … exit
  <ip>", and the fork gateway logs the IP each request arrived from, so you can check it.
- **Why ENSv2 is central:** a payment name is the most linkable thing a user owns. Verified stealth
  resolution makes `name.lowki.eth` safe to publish: every lookup is a fresh address, it is checked
  on chain, and only the name's keys can find and spend the payments. ENSv2's permissioned
  registries, roles and verifiable factories let us do this without a custom registry or any trust
  in the operator once a name is registered.

### ENS

**Live on Sepolia** (ENSv2 deployment of 2026-10-01):

| What | Address |
|---|---|
| `lowki.eth` subregistry (VerifiableFactory proxy of `UserRegistryImpl`) | [`0xEEF136dcce9AE2130028990f28eD260f11A1BDB3`](https://sepolia.etherscan.io/address/0xEEF136dcce9AE2130028990f28eD260f11A1BDB3) |
| `LowkiRegistrar` (sponsored, owner-signed registrations) | [`0xB5CbecBd2a52C76cdceb65DdCc3bf8f4d924c03e`](https://sepolia.etherscan.io/address/0xB5CbecBd2a52C76cdceb65DdCc3bf8f4d924c03e) |
| `LowkiOwnerAccount` (EIP-7702 delegate for name owners) | [`0x8Ed05C155ABB6aef00481Ca6aEc2Fc4815089228`](https://sepolia.etherscan.io/address/0x8Ed05C155ABB6aef00481Ca6aEc2Fc4815089228) |
| `StealthResolver` (Fluidkey, ENSIP-30, unmodified, canonical CREATE2 salt) | [`0xb78F3D4F40Eb9b28139cc116A94d79F976da1Dc6`](https://sepolia.etherscan.io/address/0xb78F3D4F40Eb9b28139cc116A94d79F976da1Dc6) |
| `Scheme1Verifier` | [`0x0D53f61Ac69909faEB373355dF829c455dA73Ea1`](https://sepolia.etherscan.io/address/0x0D53f61Ac69909faEB373355dF829c455dA73Ea1) |
| `lowki.eth` records (`PermissionedResolver`) | [`0xCe8B913eAc4B634BbC0A3390203D16ca5c2664DA`](https://sepolia.etherscan.io/address/0xCe8B913eAc4B634BbC0A3390203D16ca5c2664DA) |
| Names | `chai.lowki.eth` (owner `0xAa46…bAa2`), `tabla.lowki.eth` (`0xD206…877b`), `pixel.lowki.eth` (`0x6970…0a6A`). Users can claim more from the wallet home. |

Try it: `viem.getEnsAddress({ name: 'chai.lowki.eth' })` on Sepolia returns a different address on
every call. Each one is verified on chain against chai's published meta-address. (Resolution needs
the gateway, which runs on our laptop through a Cloudflare tunnel.)

**How we use ENSv2:**

- **Subregistry:** `lowki.eth` has its own `PermissionedRegistry`. It isn't new code: it's a
  `VerifiableFactory` proxy of ENS's `UserRegistryImpl`, and `DeployLowki` asserts
  `verifyContract(registry) == UserRegistryImpl`.
- **EAC roles:** the registry is **emancipated**. The Lowki admin keeps only registrar-admin, renew,
  reserve, set-parent and set-URI roles on the root, none of which can touch a registered name.
  `LowkiRegistrar` holds `ROLE_REGISTRAR | ROLE_RENEW` only. Each owner gets
  `ROLE_SET_RESOLVER(+_ADMIN) | ROLE_SET_SUBREGISTRY(+_ADMIN) | ROLE_CAN_TRANSFER_ADMIN`.
  Re-registration after expiry drops the old roles.
- **Sponsored registration:** the owner signs an EIP-712 `Register(label, owner, resolver,
  duration, nonce, deadline)`, checked with `SignatureChecker`, so EOAs and ERC-1271 accounts both
  work. The operator can't register a name without the owner's consent and can't pick its resolver.
  Labels are restricted to ASCII `a-z0-9-`, already ENSIP-15-normalized, so payment names have no
  confusables.
- **PermissionedResolver:** each owner deploys their own `PermissionedResolver` through the
  `VerifiableFactory` (salt = namehash), with all roles granted to themselves, as their records
  resolver.
- **StealthResolver / ENSIP-30 draft** ([ensips#88](https://github.com/ensdomains/ensips/pull/88)):
  - the name's resolver is Fluidkey's `StealthResolver` in **verified mode**, with the ENSIP-30
    `data()` record `stealth-meta-address[1][60]` (and the older `st:eth:` text record for
    compatibility);
  - `addr()` reverts with `OffchainLookup` to lowki-server's gateway, which returns
    `abi.encode(stealthAddress, e ‖ S.x ‖ S.y)`;
  - `Scheme1Verifier` re-derives the address on chain, and a wrong address reverts;
  - the gateway stores every address it hands out (ENSIP-30 §6), so the recipient can find them
    with a viewing-key-signed request.
- **Everything sponsored:** the owner's setup is one EIP-7702 type-4 transaction:
  1. delegate to `LowkiOwnerAccount`;
  2. deploy the resolver;
  3. `setConfig`, `setChains` and `setMetaAddress`;
  4. `setResolver`.
  
  The owner key never holds ETH.
- 37 Foundry tests, including namechain's ENSv2 fixture with Fluidkey's resolver and ScopeLift's
  vectors. End to end on a fork with the real ENSv2 contracts (`contracts/e2e`), and live on
  Sepolia.

## Links

- Code: `<GITHUB_URL>`
- Demo video: `<VIDEO_URL>`
- Live demo: see the URL table at the top of `docs/DEMO.md` (Lowki Tips, Privy Swap, the wallet, the showcase site), while the laptop is up.
- Docs in the repo: `README.md` (CROPS, status), `docs/ARCHITECTURE.md`, `docs/PRIVATE-SEND.md`, `contracts/README.md`, `bench/RESULTS.md`.

## Publish the code (run this yourself; nothing has been pushed)

From the repo root, after checking `git status` and that no secret is tracked (secrets live in
`/Users/betterclever/.lowki/`, outside the repo; `target/` is ignored):

```sh
gh repo create betterclever/lowki --public --source . --push
```

Then replace `<GITHUB_URL>` above with `https://github.com/betterclever/lowki`.
