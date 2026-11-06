# Lowki — Rust primitives (build phase 1)

Lowki is a privacy-first, Privy-compatible embedded wallet. All key material and
crypto live in Rust (compiled to `wasm32-unknown-unknown` for the browser). The
privacy vault is **Kohaku's Privacy Pools v1 plugin**. Railgun has been removed
from Kohaku upstream (`packages/railgun` is a placeholder), so we don't target it.

## Reference sources (ground truth)

- Kohaku monorepo, pinned at commit `8c83d64050040654082d70901a46a339bfa103b0`:
  `/Users/betterclever/projects/ethglobal/vendor/kohaku` (read-only; do not modify).
  - Keystore/BIP-32: `packages/plugins/src/host/{mnemonic-keystore,derivation-path,scoped-keystore}.ts`
  - Poseidon T3: `packages/poseidon/src`; LeanIMT: `packages/poseidon-lean-imt/src/limt.ts`
  - PP secrets: `packages/privacy-pools/src/account/{keys,signature-keys}.ts` (SignatureSecretManager is the default)
  - Commitment / context / merkle proof: `packages/privacy-pools/src/{utils.ts,utils/proof.util.ts}`
  - Paymaster withdraw / userOp / 7702: `packages/privacy-pools/src/{paymaster/*,state/thunks/paymasterWithdrawThunk.ts,account/tx/*}`
  - Config (Sepolia entrypoint `0x34A2068192b1297f2a7f85D7D8CdE66F8F0921cB`, EntryPoint `0x4337084D9E255Ff0702461CF8895CE9E3b5Ff108`): `packages/privacy-pools/src/config.ts`
- Circuits package `@fatsolutions/privacy-pools-core-circuits@1.0.5` unpacked at
  `/Users/betterclever/projects/ethglobal/vendor/npm/circuits/package` (artifacts: `commitment.{wasm,zkey,vkey}`, `withdraw.{wasm,zkey,vkey}`; Groth16 on bn128, snarkjs 0.7.5).

## Rules for every primitive

1. **Standalone crate** in `crates/<name>/`. Its `Cargo.toml` must contain an empty
   `[workspace]` table so it builds on its own (no root workspace yet). Edition 2024.
   Use `CARGO_TARGET_DIR=/Users/betterclever/projects/ethglobal/lowki/target/<name>` for every
   cargo command so parallel builders don't block on each other's build lock.
2. **Must compile for `wasm32-unknown-unknown`** (`cargo build --target wasm32-unknown-unknown`).
   Gate native-only deps/tests behind `cfg(not(target_arch = "wasm32"))` or dev-deps.
   If `getrandom` is pulled in, enable its wasm JS backend correctly for the version used.
3. **Secrets:** use `zeroize` (`Zeroizing<..>` / `ZeroizeOnDrop`) for secret bytes, never format keys
   into `String`, no `Debug` that prints secrets.
4. **Verifiable = differential tests against the TypeScript reference.** Put a Node/TS
   vector generator in `vectors/<name>/` (its own `package.json`; use `npm`, pnpm is not
   installed; Node 24; `npx tsx` is fine). Prefer importing the Kohaku **source files**
   from the pinned clone; if that's impractical, use the published npm packages and record
   the exact versions. The generator writes `vectors/<name>/vectors.json` (committed), and
   the crate's tests load it (`include_str!`) and assert byte-for-byte equality. Use many
   cases (randomized with a fixed seed + edge cases), not one.
5. `cargo test` and `cargo clippy -- -D warnings` must pass; the wasm build must pass.
6. Write `crates/<name>/README.md`: the public API, what was verified against what (with
   versions/commit), how to regenerate vectors, and known gaps.
7. Don't touch other builders' directories. Don't commit; the lead will review and commit.
