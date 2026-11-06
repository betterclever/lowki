| Area | Lowki module | wasm+glue gzip (opt-level 3, wasm-opt -O3) | wasm+glue gzip (opt-level z, wasm-opt -Oz) | TS equivalent | JS minified | JS gzip | gzip ratio (speed build) |
|---|---|---|---|---|---|---|---|
| keys | lowki-keys | 74.0 KiB (wasm 198.8 KiB raw) | 65.2 KiB (wasm 152.1 KiB raw) | Kohaku MnemonicKeystore + @scure/bip32/39 + viem sign/address | 119.3 KiB | 46.6 KiB | 1.6× larger |
| pp | lowki-pp-core | 91.6 KiB (wasm 230.0 KiB raw) | 512.3 KiB (wasm 1436.2 KiB raw) | Kohaku poseidon + LeanIMT + SignatureSecretManager + commitment/context (maci-crypto, viem) | 1495.3 KiB | 691.2 KiB | **7.5× smaller** |
| stealth | lowki-stealth | 43.2 KiB (wasm 108.5 KiB raw) | 34.4 KiB (wasm 73.7 KiB raw) | ScopeLift SDK (noble/secp256k1) + viem encode/decode | 74.9 KiB | 24.5 KiB | 1.8× larger |
| aa | lowki-aa | 75.0 KiB (wasm 194.4 KiB raw) | 63.0 KiB (wasm 139.1 KiB raw) | Kohaku paymaster encoders + buildSignedUserOp (viem) | 93.0 KiB | 31.5 KiB | 2.4× larger |
| g16 | lowki-groth16 (CPU) | 485.4 KiB (wasm 1664.7 KiB raw) | 367.7 KiB (wasm 1244.5 KiB raw) | snarkjs groth16 (fullProve + verify) | 305.0 KiB | 67.4 KiB | 7.2× larger |
| webgpu | lowki-groth16 + lowki-gpu (WebGPU) | 563.7 KiB (wasm 1867.4 KiB raw) | 421.7 KiB (wasm 1362.3 KiB raw) | snarkjs groth16 (fullProve + verify) | 305.0 KiB | 67.4 KiB | 8.4× larger |
| all | all six crates in one module | 734.8 KiB (wasm 2330.6 KiB raw) | 987.1 KiB (wasm 2941.6 KiB raw) | all of the above in one bundle | 1928.5 KiB | 804.4 KiB | **1.1× smaller** |
