# Lowki vs the TypeScript reference: benchmark results

Everything below ran on one machine: the macOS host, Apple M5 Max. Every number is the
**median / p90 of N timed samples taken after warm-up**. Each row was checked for equivalence:
on at least one input, both sides must give the same output (the "Same output" column, or
"Verifies" for proofs). No number here is estimated or copied from elsewhere.
Produced by `bench/run-all.sh`. Raw data is in `bench/results/*.json`.


> **Round 2.** The H witness map (6 NTTs) now runs on the GPU. Small circuits compute the G2
> MSM on the CPU, in parallel with the GPU. Artifacts are sha256-pinned, and zkey G2 points
> are subgroup-checked. WebGPU withdraw went **263 → 168 ms, 3.6× faster than snarkjs with 18
> web workers (603 ms)**. Commitment went **63 → 48 ms**; snarkjs is at 39 ms. See
> "Round 2" below.
>
> **Update: the witness fix.** lowki-groth16 now runs the circom witness `.wasm` on the browser's own
> WebAssembly engine on wasm32, instead of the wasmi interpreter. The Groth16 browser numbers below
> are from after that change. The table "Groth16 in Chrome: before / after" has the old ones.
> Withdraw on WebGPU went from **1.08 s → 263 ms** and now **beats snarkjs with 18 web workers (581 ms) by 2.2×**.
> Commitment on WebGPU went from 92 → 63 ms, and is still behind snarkjs's 40 ms.

## TL;DR

**Wins (browser, Chrome 156, wasm vs TS in the same tab):**
1. **Keys:** mnemonic → key is **4.7× faster** (0.89 ms vs 4.2 ms). Re-deriving from an unlocked keystore is **~1300× faster** (3 µs vs 4.1 ms), because Kohaku re-runs PBKDF2 inside every `deriveAt`. That second number is a design difference, not a language one (¹).
2. **Stealth scan:** **6.2× faster** (10k announcements: 0.53 s vs 3.24 s). Generating a stealth address is **2.8× faster**.
3. **Groth16 withdraw:** with WebGPU it takes **168 ms, 3.6× faster than snarkjs with all 18 web workers** (603 ms in the same session) and 26× faster than snarkjs `singleThread` (4.38 s). Round 1 got it to 263 ms, and before the witness fix it was 1.08 s. On one CPU thread with no GPU it takes 2.05 s, **2.2× faster** than snarkjs `singleThread`. Commitment on one CPU thread is also 2.3× faster than snarkjs `singleThread` (126 vs 286 ms). These are the numbers after the witness fix; before, WebGPU withdraw took 1.08 s.

Native vs Node, the gaps are larger: 2.5×–60× on every operation. For example, stealth scan is 62×, note secrets 12×, Poseidon 3–12×, and the userOp 5.3×.

**Losses (browser):**
1. **Groth16 commitment against snarkjs with its default web workers:** snarkjs takes 39 ms against our 47.6 ms, so it is 1.2× faster (round 1: 63 ms). Commitment is a tiny circuit (n = 1 542). Its G2 MSM is now computed on the CPU (~35–41 ms in wasm), in parallel with the GPU's G1 work, and that is now the critical path. *(Fixed for withdraw: the first run lost withdraw too, 1.08 s vs 0.61 s, because the witness ran in the wasmi interpreter (828 ms vs snarkjs's 24 ms). The witness now runs on the browser's WebAssembly engine: 25.6 ms, identical output.)* In round 2 the FFTs moved to the GPU (they were 176 ms of 263 ms; they are now 2–9 ms of waiting). The floor is now the GPU MSMs, about 100–140 ms of waiting.
2. **Kohaku's parallel LeanIMT** (`fromLeavesParallel`, 16 workers) is **10× faster** than our single-threaded wasm tree on 100k leaves (0.31 s vs 3.09 s). Against Kohaku's single-threaded tree we are still **1.2× slower** in Chrome. Poseidon T3 in our wasm build (light-poseidon on ark-ff) takes 31 µs, vs 25 µs for Kohaku's hand-written BigInt Montgomery code in V8.
3. **secp256k1 signing in wasm is 1.9× slower than noble in Chrome** (122 µs vs 65 µs). Natively it is 2.5× faster than Node.
4. **Payload:** the wasm modules are **larger** than the JS for keys (1.6×), stealth (1.8×), aa (2.4×) and especially Groth16 (CPU 501 KiB vs snarkjs 67 KiB gzip, 7.4×; WebGPU 600 KiB, 8.9×). The witness fix added 7 KiB, since wasmi is still linked in as the fallback. Round 2 added 16 KiB to the CPU module and 36 KiB to the WebGPU one, for sha256 pinning and the QAP shader generator. The exception is pp-core, which is **7.5× smaller** (92 KiB vs 691 KiB), because Kohaku's maci-crypto pulls in ethers and all Poseidon widths. With everything combined, the two are about the same (768 KiB vs 804 KiB gzip).

**Surprises:**
- Chrome 156's V8 runs the same TS code 1.5–4× faster than Node 26 on this machine: maci Poseidon 36 vs 140 µs, `sign` 65 vs 132 µs, stealth scan 3.2 vs 14.5 s. So the Node-vs-native ratios overstate the browser story by a lot. Read the browser table.
- The BIP-32 rows (56× in Chrome, 193× native) are mostly algorithmic. `@scure/bip32` computes every node's public key, which is an EC multiplication. Lowki does not compute a public key on a hardened path unless it needs one. The non-hardened `m/44'/60'/0'/0/i` path does need public keys, and there the gap is only 2.6× in Chrome.
- Natively, rayon CPU proving (229 ms) is as fast as Metal (228 ms) for withdraw. Our witness (111 ms) is half of either. Natively the witness still runs in wasmi, and it is 2.5× slower there than the JIT path in Chrome (25 ms). A native JIT (wasmtime) would fix that if native ever matters.
- Cross-origin isolation (COOP/COEP) changes nothing. snarkjs with default workers ran 605 ms with it and 606 ms without; WebGPU ran 1.10 s with and without. So the browser numbers apply to a non-isolated iframe.
- `opt-level = "z"` makes the pp-core wasm **6× bigger** (1.4 MiB vs 230 KiB raw). That also makes the "all" module larger at `z` than at `3`. Use opt-level 3 for these crates. I did not investigate why.

## Environment

```
date: Fri Nov  6 22:03:00 IST 2026
cpu: Apple M5 Max, 18 logical cores (6 P + 12 E), 48 GiB
os: macOS 27.3 (26C5035r), arm64
chrome: Google Chrome 156.0.8078.80 
node: v26.13.0
rustc: rustc 1.98.1 (48a229cea 2026-09-01)
wasm-bindgen: wasm-bindgen 0.2.130
wasm-opt: wasm-opt version 133
esbuild: 0.25.12
npm packages:
├── @noble/curves@2.0.1
├── @noble/hashes@2.2.0
├── @noble/secp256k1@2.3.0
├── @scopelift/stealth-address-sdk@1.0.0-beta.5
├── @scure/bip32@2.2.0
├── @scure/bip39@2.2.0
├── esbuild@0.25.12
├── maci-crypto@2.5.0
├── ox@0.12.4
├── snarkjs@0.7.5
└── viem@2.54.1

  (viem's own @noble/curves: 1.9.1)
  (ffjavascript: 0.3.1)
kohaku: 8c83d64050040654082d70901a46a339bfa103b0
```

Kohaku sources are imported **unmodified from the pinned clone**. esbuild aliases `@K/…` and
`@kohaku-eth/*` to `vendor/kohaku/packages/*/src`. The npm versions match Kohaku's pnpm-lock
(viem 2.54.1, ox 0.12.4, maci-crypto 2.5.0, @scure/bip32 and @scure/bip39 2.2.0). The
stealth reference is `@scopelift/stealth-address-sdk` 1.0.0-beta.5 with `@noble/secp256k1` 2.3.0.

## Method

- **Same inputs everywhere.** `bench/js/gen-fixtures.ts` writes deterministic fixtures (a sha256-counter RNG) to `bench/fixtures/`. Every side reads them. Groth16 uses the `commitment_rand_0` and `withdraw_1500_leaves` inputs from `vectors/lowki-groth16/vectors.json`. Stealth scans use 10k real announcements made with the SDK, 1 % of them addressed to us. The AA fixture uses a real withdraw proof.
- **One harness.** In Chrome, the TS side and the Lowki wasm side are timed by the same JS harness (`js/harness.ts`) with `performance.now()`. The wasm calls go through wasm-bindgen with pre-parsed inputs; the TS side gets pre-parsed BigInt inputs. Native Rust uses the same sampling rules (`rust/src/bin/native.rs`).
- **Sampling.** Warm-up is at least 3 samples and at least 300 ms. Then come 10–200 samples within about 2.5 s (fixed at 10 for the slow cases). Results are per operation: sample time / batch. Very fast cases are repeated so that one sample takes 2 ms or more, the same on both sides, because browser timers have 5 µs granularity even with cross-origin isolation. Groth16: 2 warm-ups, then 10 proofs per configuration.
- **Each Chrome suite gets a fresh headless Chrome process** (`--headless=new --enable-unsafe-webgpu`; the adapter is `apple / metal-3`). Pages are served with COOP/COEP for timer resolution. A no-isolation rerun of the Groth16 suites gave the same numbers.
- **Release builds:** `opt-level=3`, fat LTO, `codegen-units=1`, wasm-bindgen 0.2.130, `wasm-opt -O3`. The JS is esbuild-bundled and minified. In Node, snarkjs is loaded from node_modules, unbundled, because its worker threads need their own files.
- **Equivalence:** every micro row compares canonical outputs. These are seeds, keys, signatures (r‖s‖v), hashes, roots, a full Merkle proof, the secrets, the stealth address, ephemeral key and view tag, the hit indices of a scan, and the whole `eth_sendUserOperation` JSON. All match ✓. For Groth16, every configuration's public signals equal the fixture's, and snarkjs `groth16.verify` accepts the proofs from every configuration, both the in-browser ones and the native CPU, rayon and Metal ones.

### Not apples-to-apples (read before quoting)

1. ¹ `keystore_derive`: Kohaku `MnemonicKeystore.deriveAt` calls `mnemonicToSeedSync` (PBKDF2) on every call. Lowki's `Keystore` holds the zeroized seed. `mnemonic_to_key` is the like-for-like row: validate, then PBKDF2, then BIP-32 on both sides.
2. ² `fromLeavesParallel` uses `min(cores, 16) = 16` workers, started on every call, as Kohaku does. Lowki is one thread.
3. ³ Note secrets: Kohaku's host keystore is stubbed so that `deriveAt` returns the precomputed account key, which avoids PBKDF2. `verifyDeterminism` is off, and a fresh manager is used per sample, so there are no cache hits. Kohaku's default signer still runs `privateKeyToAccount` on every signature; the "cached account" row removes that cost.
4. Groth16 threading: snarkjs "default" uses `navigator.hardwareConcurrency` (18) web workers, or worker_threads in Node. Lowki in the browser uses one CPU thread, or one CPU thread plus the GPU. snarkjs `singleThread` also rebuilds its bn128 wasm engine on every proof; that is how the library behaves. snarkjs re-reads the zkey on every proof, while Lowki parses it once, and that one-time "zkey load" is listed separately: 9–30 ms in Chrome, which includes compiling the witness `.wasm` (~1 ms). Witness and full proof were timed separately; "proof minus witness" is the difference of the two medians.
5. Stealth scan: the SDK hex-decodes each announcement inside `checkStealthAddress`, while the Lowki benchmark pre-decodes them. That costs about 1 µs, against about 50 µs of ECDH per announcement.
6. Payload size: the JS bundles use a tiny `assert` shim for maci-crypto instead of a full Node polyfill, which favours JS. The circuit artifacts (`.wasm` and `.zkey`) are the same for both and are not counted. The wasm figures include the minified wasm-bindgen glue. Each wasm size probe (`rust/src/size.rs`) exports the API surface an iframe would actually call.

### Skipped

- WebGPU: this suite has no per-stage split. The crate test `make verify-webgpu-chrome` has one: witness 21–30 ms, FFTs 176–181 ms (overlapped with the GPU), GPU wait 45–49 ms. Safari, Firefox and other GPUs were not run.
- snarkjs with WebGPU does not exist; ffjavascript has no GPU backend.

## Results

### Chrome: Lowki wasm vs TS in the same browser

| Operation | TS (Chrome) median / p90 | Lowki wasm (Chrome) median / p90 | Lowki vs TS | Same output |
|---|---|---|---|---|
| BIP-39 mnemonic → seed (PBKDF2-SHA512 ×2048) | 3.96 ms / 4.06 ms | 885 µs / 898 µs | **4.5× faster** | ✓ |
| BIP-32 seed → key at m/28784'/1'/0' | 171 µs / 180 µs | 3.0 µs / 3.1 µs | **56× faster** | ✓ |
| BIP-32 seed → key at m/44'/60'/0'/0/i (per key, 10 keys) | 263 µs / 285 µs | 101 µs / 102 µs | **2.6× faster** | ✓ |
| mnemonic → key (new keystore + deriveAt, end to end) | 4.18 ms / 4.28 ms | 887 µs / 897 µs | **4.7× faster** | ✓ |
| deriveAt on an existing keystore ¹ | 4.08 ms / 4.18 ms | 3.0 µs / 3.1 µs | **1336× faster** | ✓ |
| secp256k1 sign 32-byte digest (viem `sign`) | 64.5 µs / 68.0 µs | 122 µs / 122 µs | 1.9× slower | ✓ |
| secp256k1 sign 32-byte digest (@noble/curves 2.0.1) | 64.8 µs / 68.6 µs | 122 µs / 122 µs | 1.9× slower | ✓ |
| Poseidon T3 hash (Kohaku poseidonT3) | 25.3 µs / 25.4 µs | 31.0 µs / 31.0 µs | 1.2× slower | ✓ |
| Poseidon T3 hash (maci-crypto, used by Kohaku for secrets) | 36.5 µs / 36.5 µs | 31.0 µs / 31.0 µs | **1.2× faster** | ✓ |
| LeanIMT build, 1k leaves | 25.5 ms / 25.7 ms | 30.9 ms / 31.1 ms | 1.2× slower | ✓ |
| LeanIMT build, 10k leaves | 255 ms / 256 ms | 309 ms / 311 ms | 1.2× slower | ✓ |
| LeanIMT build, 100k leaves | 2.56 s / 2.56 s | 3.09 s / 3.10 s | 1.2× slower | ✓ |
| LeanIMT build, 1k leaves (Kohaku fromLeavesParallel, workers) ² | 17.6 ms / 19.2 ms | 30.9 ms / 31.1 ms | 1.8× slower | ✓ |
| LeanIMT build, 10k leaves (Kohaku fromLeavesParallel, workers) ² | 48.3 ms / 50.1 ms | 309 ms / 311 ms | 6.4× slower | ✓ |
| LeanIMT build, 100k leaves (Kohaku fromLeavesParallel, workers) ² | 307 ms / 316 ms | 3.09 s / 3.10 s | 10.1× slower | ✓ |
| Merkle proof generation (100k tree, per proof) | 71 ns / 83 ns | 51 ns / 54 ns | **1.4× faster** | ✓ |
| Note secrets (SignatureSecretManager.getDepositSecrets, per note) ³ | 222 µs / 236 µs | 127 µs / 128 µs | **1.7× faster** | ✓ |
| Note secrets, TS signer reusing one viem account ³ | 184 µs / 201 µs | 127 µs / 128 µs | **1.5× faster** | ✓ |
| Commitment poseidon(value, label, precommitment) | 57.8 µs / 58.1 µs | 48.5 µs / 48.7 µs | **1.2× faster** | ✓ |
| Generate stealth address (per address) | 451 µs / 456 µs | 160 µs / 161 µs | **2.8× faster** | ✓ |
| Scan 1,000 announcements (view tag; 10 ours) | 324 ms / 324 ms | 52.5 ms / 52.7 ms | **6.2× faster** | ✓ |
| Scan 10,000 announcements (view tag; 100 ours) | 3.24 s / 3.32 s | 526 ms / 527 ms | **6.2× faster** | ✓ |
| Build + sign paymaster-withdraw userOp (encoders + 7702 auth + EIP-712 sig) | 289 µs / 317 µs | 211 µs / 213 µs | **1.4× faster** | ✓ |

### Native Rust vs Node

| Operation | TS (Node) median / p90 | Lowki native median / p90 | Lowki vs TS | Same output |
|---|---|---|---|---|
| BIP-39 mnemonic → seed (PBKDF2-SHA512 ×2048) | 4.53 ms / 4.64 ms | 622 µs / 635 µs | **7.3× faster** | ✓ |
| BIP-32 seed → key at m/28784'/1'/0' | 449 µs / 485 µs | 2.3 µs / 2.4 µs | **193× faster** | ✓ |
| BIP-32 seed → key at m/44'/60'/0'/0/i (per key, 10 keys) | 683 µs / 699 µs | 42.2 µs / 43.4 µs | **16× faster** | ✓ |
| mnemonic → key (new keystore + deriveAt, end to end) | 5.17 ms / 5.30 ms | 623 µs / 639 µs | **8.3× faster** | ✓ |
| deriveAt on an existing keystore ¹ | 4.95 ms / 5.09 ms | 2.4 µs / 2.5 µs | **2083× faster** | ✓ |
| secp256k1 sign 32-byte digest (viem `sign`) | 132 µs / 136 µs | 52.3 µs / 53.7 µs | **2.5× faster** | ✓ |
| secp256k1 sign 32-byte digest (@noble/curves 2.0.1) | 133 µs / 136 µs | 52.3 µs / 53.7 µs | **2.5× faster** | ✓ |
| Poseidon T3 hash (Kohaku poseidonT3) | 37.5 µs / 38.1 µs | 12.2 µs / 12.3 µs | **3.1× faster** | ✓ |
| Poseidon T3 hash (maci-crypto, used by Kohaku for secrets) | 140 µs / 140 µs | 12.2 µs / 12.3 µs | **12× faster** | ✓ |
| LeanIMT build, 1k leaves | 38.0 ms / 38.9 ms | 12.4 ms / 12.6 ms | **3.1× faster** | ✓ |
| LeanIMT build, 10k leaves | 388 ms / 389 ms | 123 ms / 124 ms | **3.1× faster** | ✓ |
| LeanIMT build, 100k leaves | 4.04 s / 4.17 s | 1.22 s / 1.22 s | **3.3× faster** | ✓ |
| LeanIMT build, 1k leaves (Kohaku fromLeavesParallel, workers) ² | 20.3 ms / 21.4 ms | 12.4 ms / 12.6 ms | **1.6× faster** | ✓ |
| LeanIMT build, 10k leaves (Kohaku fromLeavesParallel, workers) ² | 74.9 ms / 79.6 ms | 123 ms / 124 ms | 1.6× slower | ✓ |
| LeanIMT build, 100k leaves (Kohaku fromLeavesParallel, workers) ² | 613 ms / 783 ms | 1.22 s / 1.22 s | 2.0× slower | ✓ |
| Merkle proof generation (100k tree, per proof) | 104 ns / 121 ns | 41 ns / 42 ns | **2.5× faster** | ✓ |
| Note secrets (SignatureSecretManager.getDepositSecrets, per note) ³ | 622 µs / 653 µs | 52.1 µs / 53.6 µs | **12× faster** | ✓ |
| Note secrets, TS signer reusing one viem account ³ | 477 µs / 512 µs | 52.1 µs / 53.6 µs | **9.2× faster** | ✓ |
| Commitment poseidon(value, label, precommitment) | 261 µs / 273 µs | 18.5 µs / 19.1 µs | **14× faster** | ✓ |
| Generate stealth address (per address) | 2.20 ms / 2.30 ms | 71.3 µs / 73.6 µs | **31× faster** | ✓ |
| Scan 1,000 announcements (view tag; 10 ours) | 1.41 s / 1.62 s | 23.2 ms / 23.5 ms | **61× faster** | ✓ |
| Scan 10,000 announcements (view tag; 100 ours) | 14.54 s / 15.47 s | 233 ms / 237 ms | **62× faster** | ✓ |
| Build + sign paymaster-withdraw userOp (encoders + 7702 auth + EIP-712 sig) | 473 µs / 512 µs | 88.9 µs / 89.9 µs | **5.3× faster** | ✓ |

### Groth16


**commitment**

| Config | Full proof median / p90 | Witness median | Proof minus witness | One-time setup | Verifies (snarkjs) | Public signals = fixture |
|---|---|---|---|---|---|---|
| snarkjs fullProve, default (web workers / worker_threads) | 39.0 ms / 41.5 ms | 2.90 ms | 36.1 ms | none (snarkjs re-reads the zkey on every proof) | ✓ | ✓ |
| snarkjs fullProve, singleThread | 286 ms / 291 ms | 2.98 ms | 283 ms | none (snarkjs re-reads the zkey on every proof) | ✓ | ✓ |
| lowki-groth16 wasm, CPU (1 thread) | 127 ms / 134 ms | 2.01 ms | 125 ms | zkey load 47.5 ms | ✓ | ✓ |
| lowki-groth16 wasm, WebGPU | 47.6 ms / 48.5 ms | – | – | GPU open 39.0 ms, zkey load 47.0 ms, base upload 3.00 ms | ✓ | ✓ |

**withdraw**

| Config | Full proof median / p90 | Witness median | Proof minus witness | One-time setup | Verifies (snarkjs) | Public signals = fixture |
|---|---|---|---|---|---|---|
| snarkjs fullProve, default (web workers / worker_threads) | 603 ms / 607 ms | 23.5 ms | 580 ms | none (snarkjs re-reads the zkey on every proof) | ✓ | ✓ |
| snarkjs fullProve, singleThread | 4.38 s / 4.41 s | 23.8 ms | 4.36 s | none (snarkjs re-reads the zkey on every proof) | ✓ | ✓ |
| lowki-groth16 wasm, CPU (1 thread) | 2.08 s / 2.09 s | 28.3 ms | 2.05 s | zkey load 69.6 ms | ✓ | ✓ |
| lowki-groth16 wasm, WebGPU | 168 ms / 177 ms | – | – | GPU open 6.00 ms, zkey load 66.0 ms, base upload 39.0 ms | ✓ | ✓ |

#### Groth16, native / Node


**commitment**

| Config | Full proof median / p90 | Witness median | Proof minus witness | One-time setup | Verifies (snarkjs) | Public signals = fixture |
|---|---|---|---|---|---|---|
| snarkjs fullProve, default (web workers / worker_threads) | 59.2 ms / 62.5 ms | 22.1 ms | 37.1 ms | none (snarkjs re-reads the zkey on every proof) | ✓ | ✓ |
| snarkjs fullProve, singleThread | 350 ms / 371 ms | 21.9 ms | 328 ms | none (snarkjs re-reads the zkey on every proof) | ✓ | ✓ |
| lowki-groth16 native, 1 thread | 51.3 ms / 52.1 ms | 3.10 ms | 48.2 ms | zkey load 3.47 ms | ✓ | ✓ |
| lowki-groth16 native, rayon (`parallel`) | 21.2 ms / 22.1 ms | 3.21 ms | 18.0 ms | zkey load 4.10 ms | ✓ | ✓ |
| lowki-groth16 native, Metal | 34.8 ms / 37.2 ms | – | – | GPU open 28.3 ms, base upload 3.61 ms | ✓ | ✓ |

**withdraw**

| Config | Full proof median / p90 | Witness median | Proof minus witness | One-time setup | Verifies (snarkjs) | Public signals = fixture |
|---|---|---|---|---|---|---|
| snarkjs fullProve, default (web workers / worker_threads) | 593 ms / 607 ms | 44.5 ms | 548 ms | none (snarkjs re-reads the zkey on every proof) | ✓ | ✓ |
| snarkjs fullProve, singleThread | 4.53 s / 4.83 s | 44.1 ms | 4.48 s | none (snarkjs re-reads the zkey on every proof) | ✓ | ✓ |
| lowki-groth16 native, 1 thread | 932 ms / 938 ms | 112 ms | 820 ms | zkey load 11.4 ms | ✓ | ✓ |
| lowki-groth16 native, rayon (`parallel`) | 229 ms / 233 ms | 111 ms | 118 ms | zkey load 12.4 ms | ✓ | ✓ |
| lowki-groth16 native, Metal | 228 ms / 238 ms | – | – | GPU open 11.8 ms, base upload 39.0 ms | ✓ | ✓ |

### Groth16 in Chrome, round 1: witness on the host WebAssembly engine (before → after)

| Config | commitment before | commitment after | withdraw before | withdraw after | withdraw witness before → after |
|---|---|---|---|---|---|
| lowki wasm, CPU (1 thread) | 142 ms / 144 ms | 126 ms / 127 ms | 2.85 s / 2.88 s | 2.05 s / 2.05 s | 828 ms → 25.6 ms |
| lowki wasm, WebGPU | 91.9 ms / 106 ms | 62.9 ms / 70.9 ms | 1.08 s / 1.10 s | 263 ms / 268 ms | – |
| lowki wasm, WebGPU, no COOP/COEP | 89.9 ms / 93.4 ms | 62.8 ms / 80.2 ms | 1.10 s / 1.11 s | 263 ms / 269 ms | – |
| snarkjs, default workers (reference, unchanged code) | 40.3 ms / 41.2 ms | 39.9 ms / 42.1 ms | 606 ms / 619 ms | 581 ms / 591 ms | – |
| snarkjs, default workers, no COOP/COEP | 41.5 ms / 42.1 ms | 38.3 ms / 42.0 ms | 605 ms / 624 ms | 588 ms / 593 ms | – |

### Groth16 in Chrome, round 2: GPU QAP/NTT + CPU G2 for small circuits (round 1 → round 2)

| Config | commitment before | commitment after | withdraw before | withdraw after | withdraw witness before → after |
|---|---|---|---|---|---|
| lowki wasm, CPU (1 thread) | 126 ms / 127 ms | 127 ms / 134 ms | 2.05 s / 2.05 s | 2.08 s / 2.09 s | 25.6 ms → 28.3 ms |
| lowki wasm, WebGPU | 62.9 ms / 70.9 ms | 47.6 ms / 48.5 ms | 263 ms / 268 ms | 168 ms / 177 ms | – |
| lowki wasm, WebGPU, no COOP/COEP | 62.8 ms / 80.2 ms | 46.7 ms / 47.2 ms | 263 ms / 269 ms | 173 ms / 218 ms | – |
| snarkjs, default workers (reference, unchanged code) | 39.9 ms / 42.1 ms | 39.0 ms / 41.5 ms | 581 ms / 591 ms | 603 ms / 607 ms | – |
| snarkjs, default workers, no COOP/COEP | 38.3 ms / 42.0 ms | 39.1 ms / 41.2 ms | 588 ms / 593 ms | 580 ms / 590 ms | – |

### Payload size

| Area | Lowki module | wasm+glue gzip (opt-level 3, wasm-opt -O3) | wasm+glue gzip (opt-level z, wasm-opt -Oz) | TS equivalent | JS minified | JS gzip | gzip ratio (speed build) |
|---|---|---|---|---|---|---|---|
| keys | lowki-keys | 74.0 KiB (wasm 198.8 KiB raw) | 65.2 KiB (wasm 152.1 KiB raw) | Kohaku MnemonicKeystore + @scure/bip32/39 + viem sign/address | 119.3 KiB | 46.6 KiB | 1.6× larger |
| pp | lowki-pp-core | 91.6 KiB (wasm 230.0 KiB raw) | 512.3 KiB (wasm 1436.2 KiB raw) | Kohaku poseidon + LeanIMT + SignatureSecretManager + commitment/context (maci-crypto, viem) | 1495.3 KiB | 691.2 KiB | **7.5× smaller** |
| stealth | lowki-stealth | 43.2 KiB (wasm 108.5 KiB raw) | 34.4 KiB (wasm 73.7 KiB raw) | ScopeLift SDK (noble/secp256k1) + viem encode/decode | 74.9 KiB | 24.5 KiB | 1.8× larger |
| aa | lowki-aa | 75.0 KiB (wasm 194.4 KiB raw) | 63.0 KiB (wasm 139.1 KiB raw) | Kohaku paymaster encoders + buildSignedUserOp (viem) | 93.0 KiB | 31.5 KiB | 2.4× larger |
| g16 | lowki-groth16 (CPU) | 500.8 KiB (wasm 1705.0 KiB raw) | 376.0 KiB (wasm 1261.9 KiB raw) | snarkjs groth16 (fullProve + verify) | 305.0 KiB | 67.4 KiB | 7.4× larger |
| webgpu | lowki-groth16 + lowki-gpu (WebGPU) | 599.8 KiB (wasm 1982.3 KiB raw) | 436.5 KiB (wasm 1397.6 KiB raw) | snarkjs groth16 (fullProve + verify) | 305.0 KiB | 67.4 KiB | 8.9× larger |
| all | all six crates in one module | 768.1 KiB (wasm 2437.8 KiB raw) | 999.1 KiB (wasm 2971.7 KiB raw) | all of the above in one bundle | 1928.5 KiB | 804.4 KiB | **1.0× smaller** |


## Round 2: prover hill-climb (lowki-groth16, lowki-gpu)

What changed, in order. Each step was verified before the next.

1. **Pinned artifacts.**
   - `CircuitProver::load` and `load_async` require the sha256 of the circuits-1.0.5
     `.wasm` and `.zkey`. Otherwise they return `Error::Circuit` naming the file, its hash
     and the pin.
   - The escape hatch is `load_unpinned` / `load_async_unpinned`, documented for tests and
     custom artifacts.
   - Cost: about 37 ms of one-time load in Chrome for withdraw (load is now 67–75 ms), and
     about 40 ms natively.
2. **Subgroup checks.**
   - The zkey's verifying-key G2 points are always checked. All 36 901 `b_g2_query` points
     are checked when the file is unpinned.
   - That full check costs **2.53 s in Chrome and 0.85 s natively for withdraw**. So `load`
     skips it when the pin matches: `tests/artifacts.rs` runs the full check on exactly the
     pinned bytes.
   - G1 has cofactor 1, so the on-curve check suffices. The vkey and proof parsers already
     checked G2 subgroup membership and G1 on-curve.
   - A crafted on-curve, off-subgroup G2 point is rejected.
3. **FFT stage, measured first.**
   - A 32-bit-limb Montgomery multiply for wasm32 brought **no gain**: 31.7 ns against ark's
     32.8 ns in Chrome (`suite=fieldbench`). Dropped.
   - ark's 2^16 FFT takes 26.5 ms in Chrome, and the stage runs six of them.
   - The GPU sat idle for over 100 ms of each withdraw: A/B/L take about 50 ms of GPU time
     against 176 ms of CPU FFTs.
   - So the whole H witness map moved to the GPU (new `lowki-gpu` `qap` module: Fr NTTs,
     coset shift, ab − c). It is exact, and it uses ark-poly's own ω and g.
   - It takes 6.8–8.1 ms on Metal, including transfers. Withdraw in Chrome went **263 → 168 ms**.
4. **Commitment.**
   - Profiling showed the GPU G2 MSM dominates: 18 ms of about 33 ms natively, for 1 542
     points. It is latency-bound, since each sequential step is one Fq2 point addition per
     thread.
   - For circuits with at most 8 192 bases, B (G2) now runs on the CPU, after H has been
     submitted, while the GPU does all the G1 MSMs.
   - A/B/A/B runs in Chrome measured 70.9/70.0 ms (GPU G2) against 48.1/48.2 ms (CPU G2).
     Commitment **63 → 48 ms**.
5. **Tried and reverted** (no win, or worse):
   - `TASK_MAX` ∈ {8, 16, 32} × window bits ∈ {5…9}: within noise (`tune-gpu.sh`).
   - Interleaving the MSM jobs' dispatches in one pass: slower (82 vs 65 ms, 220 vs 177 ms).

Verification (all on the final code):
- **Proof identity:** GPU vs CPU proofs byte-identical, 17/17 on Metal (`make test-webgpu`)
  and 17/17 in Chrome (`tests/wasm_gpu.rs`).
- **Witnesses:** browser witnesses identical, 17/17 (`make verify-witness-chrome`).
- **snarkjs (run on Linux) accepts:**
  - 17/17 Chrome WebGPU proofs;
  - 17/17 native Metal proofs;
  - 17/17 native CPU proofs (`make verify`);
  - 2/2 wasm proofs (`make verify-wasm`).
- **Linux suite:** `make test`, `clippy` (5 configurations), `wasm-build` all pass, as does
  lowki-gpu's clippy (native and wasm32).
- **lowki-gpu tests:** pass on Linux (GPU tests SKIP) and on Metal, including the new QAP
  test, which compares the GPU QAP element by element with the CPU at n = 2…2^16.

Conditions: other agents' workloads ran on the host during round 2 (load average 6–13). The
round-2 numbers come from one session with snarkjs re-run as a control: unchanged code,
within 4 % of round 1. Round-1 values are transcribed in `results/round1.json`.

## Reproduce

```sh
# from the Linux VM (everything runs on the host):
mac sh -lc 'cd /Users/betterclever/projects/ethglobal/lowki/bench && npm ci && ./run-all.sh'
# -> results/*.json, results/tables.md (this file's tables), results/env.txt
```

Pieces: `build.mjs` (esbuild), `build-wasm.sh` (timing wasm → `web/pkg`), `rust/` (crate `lowki-bench`:
`src/cases.rs` shared Rust cases, `src/wasm.rs` browser exports, `src/bin/native.rs`, `src/size.rs`
size probes), `js/ts-ops.ts` (TS cases), `run-browser.mjs` (headless Chrome driver), `size.sh`,
`report.mjs`. Target dirs: `target/bench-mac`, `target/bench-mac-par`, `target/bench-mac-z`.
`bench/node_modules` holds macOS binaries; it is separate from `vectors/*/node_modules`.
