# Lowki Tips

> **This app was written for Privy. We changed one import.**
>
> ```diff
> - import { PrivyProvider } from '@privy-io/react-auth';
> + import { PrivyProvider } from '@lowki/react-auth';
> ```

Lowki Tips is the hackathon demo app for Lowki: a small third-party app where you tip creators. It
uses Privy's own names (`PrivyProvider`, `usePrivy`, `useWallets`, `useSendTransaction`), plus one
Lowki-only hook, `useSendPrivately`. Every creator has an ENS stealth name under `lowki.eth`
(`chai.lowki.eth`, `tabla.lowki.eth`, `pixel.lowki.eth`) that resolves to a fresh address on every
lookup.

## Screens

| Screen | What it shows |
|---|---|
| Landing | "Tip your favourite creators" and a Sign in button (opens the Lowki wallet: email code, then passkey) |
| Creators | Three creators, each with "Tip 0.01 ETH" and "Tip privately" |
| Tip | The name resolved with **plain viem `getEnsAddress`** (CCIP-Read through the Lowki gateway, verified on chain by the `StealthResolver`), a "Never seen before" badge (not used this session, nonce 0, balance 0), the "Each tip goes to a fresh address" explainer, and this session's past addresses for that creator. Then `sendTransaction` through the SDK. |
| Tip privately | `useSendPrivately()` to a freshly resolved address. The wallet sheet shows the proof progress; the app shows "Sent privately" and the hash. |
| Header | The app's address and the pairwise user ID (short), with the tooltip "Only this app sees this address" |
| Creator inbox (`/creator.html`) | The recipient side: with the creator's keys, sign with the **viewing key**, fetch the gateway's records (`GET /v1/ens/records`), keep the ones that derive to the keys (ERC-5564 scheme 1, `src/stealth.ts`), show balances, and derive each address's spending key ("spend key ✓"). |

Screenshots for the submission: `screens/`.

## Config: one file per target, nothing hard-coded

`config/<target>.json`, target from `VITE_TIPS_TARGET` (`fork` by default, or `sepolia`). It holds
the chain (id, RPC), ENS (UniversalResolver, StealthResolver, parent name), the gateway and records
URLs, the Lowki app ID and wallet URL, the tip amount, and the creators. The app refuses to start
while a field is empty. The built page's CSP is generated from the same file: only this origin, the
RPC, the gateway and the wallet frame.

- `config/fork.json` is **generated** by `scripts/demo-tips.sh` (addresses and app ID change per run).
- `config/sepolia.json` is the live template.

No trackers, no third-party fonts or CDNs: fonts and the Lowki logo are bundled from the repo
(`apps/wallet-ui/assets/fonts`, `design/`).

## Run the demo (local Sepolia fork)

```sh
scripts/demo-tips.sh
# app http://localhost:8823/   inbox http://localhost:8823/creator.html   wallet http://localhost:8822/
```

It starts (ports 8820–8829): an anvil fork (8820), lowki.eth + Lowki registry/registrar +
Fluidkey `StealthResolver` (the `contracts/` forge scripts), the three creator names (sponsored
registration + sponsored EIP-7702 owner setup, through `contracts/e2e/client`), lowki-server with
the ENS gateway (8821), the wallet (8822) and this app (8823). Email codes are in
`target/lowki-tips/state/server.log`. The header's "+1 test ETH" button (fork only) funds you.

To reuse `scripts/demo-stack.sh`'s fork (for a real private send against its Privacy Pools
ASP + relayer): `TIPS_FORK_RPC=http://127.0.0.1:<its anvil port> TIPS_PP_RELAYER_URL=… TIPS_PP_ASP_URL=… scripts/demo-tips.sh`.
All env vars are listed at the top of the script.

**Demo creator keys.** Each creator's 64-byte wallet seed is
`sha512("lowki tips demo creators, not a secret/<label>")` (phrase overridable with
`TIPS_CREATOR_PHRASE`), and the keys follow the wallet's paths: owner `m/137'/0'/0'`, spending
`m/5564'/0'/0'`, viewing `m/5564'/0'/1'`. Public on purpose; fork only.

## Develop and test

```sh
npm install
npm run typecheck && npm run build        # VITE_TIPS_TARGET=sepolia for the live build
node e2e/run.mjs                          # on the macOS host, against scripts/demo-tips.sh
```

The e2e drives headless Chrome over CDP with a virtual authenticator (PRF): sign in, tip
`chai.lowki.eth` twice (two different addresses, both funded), tip privately, then the creator
inbox finds both tips. It writes the screenshots in `screens/`.

## Point it at live Sepolia

1. Deploy per `contracts/README.md` ("Live Sepolia deployment"), with
   `LOWKI_GATEWAY_URL=https://<your lowki-server>/v1/ens/gateway`.
2. Register the creators: run `scripts/fork-setup.mjs` with `TIPS_RPC`/`TIPS_PUBLIC_RPC` = a Sepolia
   RPC, `TIPS_OPERATOR_KEY` = the funded operator key, `TIPS_LOWKI_DEPLOYMENT=contracts/deployments/sepolia-lowki.json`,
   and a **private** `TIPS_CREATOR_PHRASE` (it is the creators' seed). It writes a config; copy
   its `ens` block into `config/sepolia.json`.
3. Run lowki-server with the gateway (`LOWKI_ENS_*`, see the contracts README) and register this
   app's origin (`POST /admin/v1/apps`).
4. Fill `config/sepolia.json`: `chain.rpcUrl`, `ens.stealthResolver`, `ens.gatewayUrl`,
   `ens.recordsUrl`, `lowki.appId`, `lowki.walletUrl`. `demo` stays `null` (no faucet; the inbox
   asks for the creator's seed).
5. `VITE_TIPS_TARGET=sepolia npm run build` and serve `dist/`.
