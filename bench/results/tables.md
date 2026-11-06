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
| snarkjs fullProve, default (web workers / worker_threads) | 39.9 ms / 42.1 ms | 2.88 ms | 37.0 ms | none (snarkjs re-reads the zkey on every proof) | ✓ | ✓ |
| snarkjs fullProve, singleThread | 286 ms / 296 ms | 2.64 ms | 283 ms | none (snarkjs re-reads the zkey on every proof) | ✓ | ✓ |
| lowki-groth16 wasm, CPU (1 thread) | 126 ms / 127 ms | 1.27 ms | 125 ms | zkey load 8.94 ms | ✓ | ✓ |
| lowki-groth16 wasm, WebGPU | 62.9 ms / 70.9 ms | – | – | GPU open 36.0 ms, zkey load 9.00 ms, base upload 2.00 ms | ✓ | ✓ |

**withdraw**

| Config | Full proof median / p90 | Witness median | Proof minus witness | One-time setup | Verifies (snarkjs) | Public signals = fixture |
|---|---|---|---|---|---|---|
| snarkjs fullProve, default (web workers / worker_threads) | 581 ms / 591 ms | 23.2 ms | 558 ms | none (snarkjs re-reads the zkey on every proof) | ✓ | ✓ |
| snarkjs fullProve, singleThread | 4.41 s / 4.50 s | 23.5 ms | 4.38 s | none (snarkjs re-reads the zkey on every proof) | ✓ | ✓ |
| lowki-groth16 wasm, CPU (1 thread) | 2.05 s / 2.05 s | 25.6 ms | 2.02 s | zkey load 29.7 ms | ✓ | ✓ |
| lowki-groth16 wasm, WebGPU | 263 ms / 268 ms | – | – | GPU open 5.00 ms, zkey load 25.0 ms, base upload 25.0 ms | ✓ | ✓ |

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

### Groth16 in Chrome: before / after running the witness on the host WebAssembly engine

| Config | commitment before | commitment after | withdraw before | withdraw after | withdraw witness before → after |
|---|---|---|---|---|---|
| lowki wasm, CPU (1 thread) | 142 ms / 144 ms | 126 ms / 127 ms | 2.85 s / 2.88 s | 2.05 s / 2.05 s | 828 ms → 25.6 ms |
| lowki wasm, WebGPU | 91.9 ms / 106 ms | 62.9 ms / 70.9 ms | 1.08 s / 1.10 s | 263 ms / 268 ms | – |
| lowki wasm, WebGPU, no COOP/COEP | 89.9 ms / 93.4 ms | 62.8 ms / 80.2 ms | 1.10 s / 1.11 s | 263 ms / 269 ms | – |
| snarkjs, default workers (reference, unchanged code) | 40.3 ms / 41.2 ms | 39.9 ms / 42.1 ms | 606 ms / 619 ms | 581 ms / 591 ms | 23.7 ms → 23.2 ms |
| snarkjs, default workers, no COOP/COEP | 41.5 ms / 42.1 ms | 38.3 ms / 42.0 ms | 605 ms / 624 ms | 588 ms / 593 ms | 24.6 ms → 23.0 ms |

### Payload size

| Area | Lowki module | wasm+glue gzip (opt-level 3, wasm-opt -O3) | wasm+glue gzip (opt-level z, wasm-opt -Oz) | TS equivalent | JS minified | JS gzip | gzip ratio (speed build) |
|---|---|---|---|---|---|---|---|
| keys | lowki-keys | 74.0 KiB (wasm 198.8 KiB raw) | 65.2 KiB (wasm 152.1 KiB raw) | Kohaku MnemonicKeystore + @scure/bip32/39 + viem sign/address | 119.3 KiB | 46.6 KiB | 1.6× larger |
| pp | lowki-pp-core | 91.6 KiB (wasm 230.0 KiB raw) | 512.3 KiB (wasm 1436.2 KiB raw) | Kohaku poseidon + LeanIMT + SignatureSecretManager + commitment/context (maci-crypto, viem) | 1495.3 KiB | 691.2 KiB | **7.5× smaller** |
| stealth | lowki-stealth | 43.2 KiB (wasm 108.5 KiB raw) | 34.4 KiB (wasm 73.7 KiB raw) | ScopeLift SDK (noble/secp256k1) + viem encode/decode | 74.9 KiB | 24.5 KiB | 1.8× larger |
| aa | lowki-aa | 75.0 KiB (wasm 194.4 KiB raw) | 63.0 KiB (wasm 139.1 KiB raw) | Kohaku paymaster encoders + buildSignedUserOp (viem) | 93.0 KiB | 31.5 KiB | 2.4× larger |
| g16 | lowki-groth16 (CPU) | 485.4 KiB (wasm 1664.7 KiB raw) | 367.7 KiB (wasm 1244.5 KiB raw) | snarkjs groth16 (fullProve + verify) | 305.0 KiB | 67.4 KiB | 7.2× larger |
| webgpu | lowki-groth16 + lowki-gpu (WebGPU) | 563.7 KiB (wasm 1867.4 KiB raw) | 421.7 KiB (wasm 1362.3 KiB raw) | snarkjs groth16 (fullProve + verify) | 305.0 KiB | 67.4 KiB | 8.4× larger |
| all | all six crates in one module | 734.8 KiB (wasm 2330.6 KiB raw) | 987.1 KiB (wasm 2941.6 KiB raw) | all of the above in one bundle | 1928.5 KiB | 804.4 KiB | **1.1× smaller** |

