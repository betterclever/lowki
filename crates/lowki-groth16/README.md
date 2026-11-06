# lowki-groth16

Groth16 (bn128) prover for the Privacy Pools v1 `commitment` and `withdraw` circuits
(`@fatsolutions/privacy-pools-core-circuits@1.0.5`, built for snarkjs 0.7.5). It is written
in Rust and runs both natively and as `wasm32-unknown-unknown`. Its proofs are accepted by
snarkjs `groth16.verify`, and it outputs the Solidity `{pA, pB, pC, pubSignals}` struct.

## Public API

```rust
let prover = CircuitProver::load(Circuit::Withdraw, &wasm_bytes, &zkey_bytes)?; // artifact bytes (browser-fetched)
let prover = CircuitProver::load_async(Circuit::Withdraw, &wasm_bytes, &zkey_bytes).await?; // wasm32 only
let out: ProofOutput = prove_withdraw(&prover, &WithdrawInputs::from_json(json)?)?;
let out: ProofOutput = prove_commitment(&commitment_prover, &CommitmentInputs::from_json(json)?)?;
verify(&parse_vkey_json(vkey_json)?, &out.proof, &out.public_signals)?; // -> bool
```

- `ProofOutput { proof: SnarkjsProof /* pi_a, pi_b, pi_c, protocol, curve */, public_signals: Vec<String>, solidity: SolidityProof }`
- `SolidityProof { pA, pB, pC, pubSignals }`: decimal strings. `pB` holds each G2 coordinate's
  Fq2 limbs swapped (`[c1, c0]`), the same as Kohaku `toWithdrawProof` / `encodeRagequitPayload`
  and snarkjs `exportSolidityCallData`.
- `SnarkjsProof::to_solidity(&public_signals)`, `SnarkjsProof::{from_ark, to_ark}`.
- `prove_*_with_rng` takes a caller RNG. The plain `prove_*` functions use `OsRng`
  (getrandom 0.2 with the `js` backend on wasm32).
- `CircuitProver::witness(&inputs.as_signals())` returns the full witness. It is exposed for
  differential tests. `CircuitProver::witness_backend()` says which engine computes it
  (`"webassembly"` or `"wasmi"`, see "Design").
- wasm32 only: `CircuitProver::load_async` compiles the witness `.wasm` with
  `WebAssembly.compile` instead of synchronously. Use it on a browser main thread.
- Inputs use the circuit signal names in JSON (`withdrawnValue`, `stateSiblings`, `ASPRoot`, …).
  Values are decimal or `0x` strings, reduced mod r like snarkjs `normalize`.
- Public signal order: commitment `[commitment, nullifierHash, value, label]`; withdraw
  `[newCommitmentHash, existingNullifierHash, withdrawnValue, stateRoot, stateTreeDepth, ASPRoot, ASPTreeDepth, context]`.
- Every proof is self-checked (3 pairings) before it is returned, so a proof the chain would
  reject is never handed out.

## Design

**Witness generation: the circuit's own circom `.wasm`, run natively by `wasmi` 2.0 (a pure-Rust
interpreter) and on wasm32 by the host's own `WebAssembly` engine.**
`src/witness.rs` re-implements the host protocol of circom 2.x `witness_calculator.js`:
`runtime.{exceptionHandler, printErrorMessage, writeBufferMessage, showSharedRWMemory}`,
FNV-1a signal hashes, `writeSharedRWMemory` / `setInputSignal`, `getWitness` / `readSharedRWMemory`.
The protocol is written once, against a small `Instance` trait. It has two engines:

- **wasmi** runs natively, and on wasm32 it is the fallback.
- **The host engine** runs on wasm32 (`src/witness/js.rs`). In Chrome and Node that is V8,
  which JIT-compiles the circuit. Rust drives it through `js_sys::WebAssembly` and
  wasm-bindgen bindings. The four `runtime` imports are Rust closures, and there is no
  hand-written JS. Running the circuit there instead of interpreting it inside our wasm
  module makes withdraw witness generation about 33× faster in Chrome (828 ms → 25 ms). That
  is on par with snarkjs (23 ms).
  - The engine is selected by `cfg(target_arch = "wasm32")`, not by a feature.
    `wasm32-unknown-unknown` always runs under a JS host here (browser, worker or Node), and
    the output is identical. A feature would only add a configuration that is never the
    right choice.
  - Compilation happens once per `CircuitProver`. Each witness gets a fresh
    `WebAssembly.Instance`, with fresh imports and fresh linear memory.
  - **Main thread vs worker.** `CircuitProver::load` compiles synchronously with
    `new WebAssembly.Module`. A worker accepts any size. Chrome's main thread caps the size
    of synchronous compiles. Chrome 156 accepted both circuits on the main thread, with
    compile plus instantiate taking about 1 ms each (lazy compilation; measured with
    `bench/web/probe-sync-compile.html`). If an engine refuses, `load` falls back to wasmi:
    the result is still correct, just slow, and `witness_backend()` reports it.
    `CircuitProver::load_async` uses `WebAssembly.compile`, which has no cap on any thread,
    so it is the right call on a main thread. Per-witness instantiation is synchronous
    (`new WebAssembly.Instance` of an already compiled module, about 0.1 ms) on both kinds
    of thread.
  - **Secrets.** As with wasmi, input and witness limbs cross the boundary one `i32` at a
    time, straight into `Zeroizing` buffers. The instance's whole linear memory is
    zero-filled before it is dropped, including on error. JS-engine temporaries (numbers on
    the JS stack) cannot be wiped by any library.
  - On wasm32, `CircuitProver` holds a JS object and is therefore `!Send`. That was already
    true in practice there: there are no threads, and `GpuProver` holds an `Rc`.

Options considered:

- (a) **wasmi** (chosen for native, and as the wasm32 fallback). It compiles to wasm32 and
  natively, needs no JS glue, and uses the exact published artifact, so there is no second
  source of truth. Inside wasm32 it is an interpreter running in an already-compiled module,
  and that is about 33× slower than the host engine.
- (b) `ark-circom` witness calculator. It uses wasmer, which is native-only. On wasm32,
  wasmer's `js` backend needs the browser's WebAssembly through wasm-bindgen, and ark-circom
  0.6 moved to ark 0.6.
- (c) `circom-witness-rs` / witness graph. It needs the circuit sources and a build step,
  which would add a second artifact that has to stay in sync with the published `.wasm`.

Each calculation gets a fresh `Store` (wasmi) or `WebAssembly.Instance` (host engine). Its
linear memory held the secrets, and it is zeroed before it is dropped.

**zkey + proving.** `src/zkey.rs` is a port of ark-circom 0.5.0's zkey reader:

- It reads from `&[u8]`.
- It returns errors where upstream panics.
- It checks the field and curve.
- It checks every point is on the curve, but skips G2 subgroup checks.
- It fixes upstream's off-by-one in `num_witness_variables`.

`src/qap.rs` is ark-circom's `CircomReduction` (snarkjs' R1CS→QAP map), copied verbatim.
Proving uses `ark-groth16` 0.5 `create_proof_with_reduction_and_matrices`. I did **not**
depend on `ark-circom` itself, because it pulls in wasmer unconditionally.

**Versions:** ark-{bn254,ec,ff,groth16,poly,std,serialize} 0.5.0, ark-relations 0.5.1,
wasmi 2.0.0 (`std`, `validate`, `auto-dispatch`), rand_core 0.6.4, getrandom 0.2.17 (`js` on
wasm32), zeroize 1.9.1. wasm32 only: wasm-bindgen 0.2.130, js-sys 0.3.107 and
wasm-bindgen-futures 0.4.80, all three already in the tree through getrandom's `js` backend
and wgpu. Dev/wasm tests: wasm-bindgen-test 0.3.80.

**Features:** `parallel` turns on rayon-based multi-threaded proving. Use it natively only.
Don't enable it on wasm32. `webgpu` adds `GpuProver` (MSMs on the GPU through
`../lowki-gpu`) for native and wasm32; see "WebGPU proving" below. Without features, the
crate and its CPU path are unchanged.

## Verification (what was checked against what)

Reference: snarkjs 0.7.5 and Kohaku source at commit `8c83d64050040654082d70901a46a339bfa103b0`,
imported unmodified through a Node resolve hook. Vector generator: `vectors/lowki-groth16/gen.ts`
(fixed seed). It produces 17 cases: 10 commitment and 7 withdraw.

The withdraw inputs are built the same way Kohaku's `withdrawThunk` builds them:

- A real state tree and ASP tree, built with `@zk-kit/lean-imt` 2.2.4 and poseidon-lite 0.3.0.
  They are cross-checked against Kohaku's `PoseidonT3LeanIMT` and `proofFromTree`, with siblings
  padded to 32.
- Real precommitment, commitment and nullifier values.
- `context` from Kohaku's `calculateContext`.
- Both depths set to 32.

Tree sizes range from 1 to 1500 leaves. The cases include the lifted-node case, full withdrawal,
zero withdrawal, and a 2^128−1 value.

| Check | How | Result |
|---|---|---|
| (i) Rust witness == snarkjs witness | `tests/vectors.rs::i_witness_equals_snarkjs`: compared against the sha256 of the full witness pinned in `vectors.json`, plus element-wise against `witness/*.bin` | 17/17 pass (1542 / 36901 signals) |
| (ii) public signals | `ii_public_signals_equal_snarkjs` | 17/17 pass |
| (iii) ark-groth16 verify against the `.vkey` JSON | `iii_rust_proofs_verify_and_export` (also asserts vkey JSON == zkey vk, and that tampered signals fail) | 17/17 pass |
| (iv) snarkjs verifies Rust proofs | `make verify` → `verify-rust-proofs.ts` (also re-derives Kohaku's Solidity struct from each Rust proof) | 17/17 native, 2/2 wasm-made |
| (v) Solidity format | `v_solidity_format_matches_reference`: compares `to_solidity` on a fixed snarkjs proof with Kohaku `toWithdrawProof` / `encodeRagequitPayload` **and** snarkjs `exportSolidityCallData` | pass (both circuits) |
| wasm32 | `tests/wasm.rs` (wasm-bindgen-test under Node): proves both circuits in wasm, then `make verify-wasm` has snarkjs verify them | pass |
| wasm32 witness engines | `make verify-witness-chrome` (`tests/wasm_witness.rs`, headless Chrome 156, main thread). The host-WebAssembly witness, from both `load` and `load_async`, is compared element by element against the wasmi witness **and** snarkjs `witness/*.bin`, plus the sha256 pin. The test also checks that a circuit assert failure (withdraw > balance) gives the same `Error::Witness` text from both engines, and that the prover still works afterwards. | 17/17 identical (1542 / 36901 signals); error parity pass |

Note on (v): in package 1.0.5, `snarkjsProofToSolidityFormat` (`dist/types.js`) is a stub that
throws `"Not implemented"`. The reference is therefore Kohaku's `encoding.utils.ts`, which is the
real on-chain path, together with snarkjs' own calldata export.

## Running

Fetch the artifacts. They are not committed.

```sh
cd /Users/betterclever/projects/ethglobal/vendor/npm/circuits   # or anywhere; then set LOWKI_PP_ARTIFACTS
npm pack @fatsolutions/privacy-pools-core-circuits@1.0.5        # integrity sha512-0E15p9wm…tza4ZA==
tar xzf fatsolutions-privacy-pools-core-circuits-1.0.5.tgz      # -> package/artifacts/*
```

Pinned sha256 of the files in `artifacts/`:

```
254d2130607182fd6fd1aee67971526b13cfe178c88e360da96dce92663828d8  commitment.wasm
494ae92d64098fda2a5649690ddc5821fcd7449ca5fe8ef99ee7447544d7e1f3  commitment.zkey
7d48b4eb3dedc12fb774348287b587f0c18c3c7254cd60e9cf0f8b3636a570d8  commitment.vkey
36cda22791def3d520a55c0fc808369cd5849532a75fab65686e666ed3d55c10  withdraw.wasm
2a893b42174c813566e5c40c715a8b90cd49fc4ecf384e3a6024158c3d6de677  withdraw.zkey
666bd0983b20c1611543b04f7712e067fbe8cad69f07ada8a310837ff398d21e  withdraw.vkey
```

Commands (all use `CARGO_TARGET_DIR=../../target/lowki-groth16`):

- `make vectors`: regenerate `vectors.json` and `witness/*.bin`. Run `npm ci` in `vectors/lowki-groth16` first.
- `make test`: runs checks (i), (ii), (iii) and (v).
- `make verify`: check (iv).
- `make verify-wasm`: the wasm proofs, verified by snarkjs. Requires `cargo install wasm-bindgen-cli --version 0.2.130`.
- `make clippy`, `make wasm-build`, `make bench`, and `make all`, which runs them all.
- `make test-webgpu`, `make verify-webgpu` and `make verify-webgpu-chrome`: the GPU checks
  (see "WebGPU proving").
- `make verify-witness-chrome`: the wasm32 witness-engine differential test in headless Chrome.
  It needs `witness/*.bin` from `make vectors` and the browser tools in `target/tools`.
- `make profile`: per-stage timings for the CPU and WebGPU provers, native.
- `tests/wasm_profile.rs`: per-stage timings of the CPU prover in wasm (Node by default;
  set `WASM_BINDGEN_USE_BROWSER=1` to run it in Chrome).

`tests/wasm.rs` embeds the artifacts with `include_bytes!` from
`../../../vendor/npm/circuits/package/artifacts`.

## Performance and memory

Measured on a 17-core aarch64 Linux VM (OrbStack), release builds. "prove" includes witness
generation and the self-check. The wasm32 rows use the host-engine witness path (one
`make verify-wasm` run). For Chrome, see the WebGPU timings below.

| | load | witness | prove |
|---|---|---|---|
| commitment, native 1 thread | ~10 ms | ~10 ms | ~90–115 ms |
| commitment, native `parallel` | ~10 ms | ~10 ms | ~50–70 ms |
| commitment, wasm32 (Node 24) | 5 ms | 23 ms (was 68–75 ms with wasmi) | 139 ms (was 170–250 ms) |
| withdraw, native 1 thread | ~30 ms | 120–160 ms | 1.5–2.1 s |
| withdraw, native `parallel` | ~25 ms | ~135 ms | ~0.36–0.42 s |
| withdraw, wasm32 (Node 24) | 37 ms | 47 ms (was 1.3–3.1 s with wasmi) | 2.19 s (was ~5.2–5.3 s) |
| snarkjs `fullProve` (Node, multi-threaded), for reference | | | commitment ~470 ms, withdraw ~1.6 s |

Memory:

- Native peak RSS with both circuits loaded and proved: ~80 MiB single-threaded, ~115 MiB with `parallel`.
- wasm linear memory high-water: 90 MiB. This includes ~21 MiB of embedded artifact bytes, and
  wasm memory never shrinks.
- The parsed withdraw proving key takes roughly 20–25 MiB.

## WebGPU proving (feature `webgpu`)

```rust
// async: WebGPU device creation and buffer mapping are async (natively: pollster::block_on)
let gpu = open_gpu().await;                                   // Err(GpuError::Unavailable) -> keep using the CPU
let prover = GpuProver::new(gpu?, CircuitProver::load(Circuit::Withdraw, &wasm, &zkey)?)
    .map_err(|(cpu, e)| /* GPU upload failed: keep `cpu` */ e)?;
let out = prove_withdraw_gpu(&prover, &inputs).await;          // or prover.prove_withdraw_with_rng(..)
match out {
    Ok(out) => { /* same ProofOutput as prove_withdraw */ }
    Err(Error::Gpu(_)) => { /* fall back: */ prove_withdraw(prover.cpu(), &inputs)?; }
    Err(e) => return Err(e),                                    // witness/input errors are not GPU errors
}
```

How it works:

- **Shared base path.** `GpuProver` holds a `CircuitProver`, and its proving-key queries
  stay resident on the GPU (a, b_g1, b_g2, l, h: 38 MiB for withdraw). The witness (the
  host's WebAssembly engine on wasm32, wasmi natively) and the H witness map (FFTs) run on the CPU. The five MSMs run on the GPU via
  [`lowki-gpu`](../lowki-gpu/README.md): A, B_G1, L and H in G1, and B in G2.
- **Same proof as the CPU path.** Assembly uses exactly the CPU path's formula
  (`src/stages.rs`). With the same RNG the proof is byte-identical to
  `create_proof_with_reduction_and_matrices` with `CircomReduction`.
- **Always pairing-checked.** Every GPU proof goes through the same 3-pairing self-check
  before it is returned.
- **Overlap.** The H-independent MSMs (A, B_G2, B_G1, L) are submitted first, and the CPU
  computes the FFTs while the GPU works.
- **CPU path unchanged.** The default CPU path (`prove_withdraw`, `prove_commitment`) does
  not change and stays the default. The feature adds `lowki-gpu` (wgpu 30) as a dependency.

### What runs where, and why (profile first)

Withdraw proof, per stage. "Native" is one thread on an Apple M5 Max
(`examples/profile.rs`, average of 5). "wasm" is the same CPU code in headless Chrome 156
(`tests/wasm_profile.rs`, 2 runs).

| Stage | Native CPU | % | wasm CPU (Chrome) | % | Where it runs in `GpuProver` |
|---|---|---|---|---|---|
| witness (wasmi; **now ~25 ms in Chrome on the host engine**) | 69 ms | 8 % | 446–647 ms | 18–24 % | CPU (not GPU work) |
| H witness map (7 FFTs, 2^16) | 70 ms | 8 % | 180–186 ms | 7 % | CPU, overlapped with the GPU MSMs |
| MSM A (G1, 36 901) | 86 ms | 10 % | 210–214 ms | 8 % | GPU |
| MSM B (G2, 36 901) | 173 ms | 20 % | 519–521 ms | 20 % | GPU |
| MSM B (G1) | 66 ms | 7 % | 154–158 ms | 6 % | GPU |
| MSM C/L (G1, 36 892) | 154 ms | 17 % | 371–374 ms | 14 % | GPU |
| MSM H (G1, 65 536) | 267 ms | 30 % | 640–642 ms | 25 % | GPU |
| assemble + pairing self-check | 1.4 ms | 0 % | 4–5 ms | 0 % | CPU |
| **total** | **886 ms** | | **2.53–2.74 s** | | |

Commitment (n = 1 542, domain 4 096) splits the same way. Total: 52 ms native and
148–167 ms in Chrome. G2 is 24 % of it.

- **G1 MSMs: GPU.** They are 64 % of the native time and 53 % in wasm.
- **G2 MSM: GPU.** It is 20 % in both. With only G1 on the GPU it would become the largest
  remaining stage, about 0.5 s in wasm.
- **NTT/FFT: CPU, for now.** The FFTs already run while the GPU computes A/B/L. Now that the
  witness takes ~25 ms, they are the **largest stage of a WebGPU withdraw proof in Chrome**:
  ~176 ms of ~263 ms, on the critical path. The GPU then waits only ~47 ms for H. A GPU NTT, or
  a faster wasm32 field multiply (ark-ff's 64×64→128-bit products are emulated on wasm32), is
  the next step. It could save up to ~0.13 s and was not done here: neither is a cheap change.

### Timings

Measured on the host: Apple M5 Max, macOS 27.2, Metal, Chrome 156.0.8078.80 headless with
`--enable-unsafe-webgpu`. Adapter: `apple` / `metal-3`. "Prove" includes witness generation,
the FFTs, the MSMs and the pairing self-check.

| | commitment | withdraw |
|---|---|---|
| native CPU, 1 thread | 50–59 ms (profile avg 52 ms) | 856–916 ms (profile avg 886 ms) |
| native Metal (`GpuProver`) | 32–49 ms (warm avg 36 ms, first 67 ms) | 182–242 ms (warm avg 182 ms, first 193 ms) |
| wasm CPU, headless Chrome | 127–152 ms (bench median 126 ms; was 147–191 ms with wasmi) | 2.07–2.15 s (bench median 2.05 s; was 2.54–3.02 s) |
| **wasm WebGPU, headless Chrome** | **57–89 ms** (bench median 63 ms; was 74–117 ms) | **262–272 ms** (bench median 263 ms; was 0.72–1.16 s) |
| snarkjs 0.7.5 `fullProve`, same Chrome, default web workers (18), for reference | 40 ms | 581 ms |

In-browser withdraw on WebGPU is now **2.2× faster than snarkjs with all 18 cores**. Commitment
is not: it is a tiny circuit (n = 1 542), where GPU dispatch latency (~45–75 ms of waiting)
dominates, so snarkjs's 40 ms beats our 63 ms. The bench rows ("bench median") are 10 proofs
after 2 warm-ups (`bench/RESULTS.md`). The ranges are the 7 or 10 vectors.json cases from
`make verify-webgpu-chrome`.

WebGPU withdraw in Chrome, per stage:

| Stage | Time |
|---|---|
| witness (host WebAssembly engine) | 21–30 ms (was 463–919 ms with wasmi) |
| CPU plan + submit of A/B/L | 11–13 ms |
| H witness map (overlapped) | 176–181 ms |
| wait for the GPU after that | 45–49 ms |
| assemble + self-check | 3–4 ms |

Other wasm-path costs, checked while doing this:

- zkey parse + `CircuitProver::load` takes 25–30 ms for withdraw and 9 ms for commitment
  in Chrome. That is a one-time cost and was not worth changing.
- Witness compile is ~1 ms; instantiate per proof is ~0.1 ms.

One-time setup:

| | Native Metal | Chrome |
|---|---|---|
| `Gpu::new` (device + shader compile) | 17–38 ms with a warm Metal shader cache; 0.8–2.4 s the first time after a shader change | 30 ms (Chrome may compile lazily: the first proof ran ~30 ms slower than later ones) |
| base upload, withdraw | 13–27 ms (38 MiB) | 25 ms |
| base upload, commitment | 1–5 ms (1 MiB) | 2 ms |

### Verification

| Check | Command | Result |
|---|---|---|
| GPU proof is byte-identical to the CPU proof (same ChaCha20 seed) for every vectors.json case, and verifies | `make test-webgpu` (`tests/gpu.rs`, native Metal) | 17/17 identical |
| GPU MSMs on the real withdraw proving-key bases (a, b_g1, l, b_g2 with the real witness and with random scalars; h at 2^16 with random scalars) equal ark | same | pass (44 472 points at infinity among the a/b_g2 bases) |
| snarkjs 0.7.5 verifies the native GPU proofs | `npx tsx verify-rust-proofs.ts out/gpu-proofs.json` | 17/17 |
| Same as the first row, in headless Chrome WebGPU (wasm32, CPU and GPU proofs made in the same module) | `make verify-webgpu-chrome` (`tests/wasm_gpu.rs`) | 17/17 identical |
| snarkjs 0.7.5 verifies the browser-made GPU proofs | (last step of the same target) | 17/17 |
| No GPU (Linux VM) | `cargo test --features webgpu` | GPU tests print `SKIP` and pass |

The existing checks were re-run after the change: `make test`, `make clippy`,
`make wasm-build`, `make verify` (17/17) and `make verify-wasm` (2/2). `make clippy` now also
covers `--features webgpu` on native and wasm32.

`make verify-webgpu-chrome` needs Chrome, a matching chromedriver and wasm-bindgen-cli
0.2.130 in `target/tools` (see `../lowki-gpu/README.md`). Its snarkjs step needs
`vectors/lowki-groth16/node_modules` installed for the machine that runs it. Here the
browser part ran on the macOS host and the snarkjs part on the Linux VM, because the
`node_modules` there hold Linux binaries.

### Limits of the GPU path

- Only Chrome on macOS/Metal was tested. Safari, Firefox and other GPUs or drivers are
  untested.
  - Any GPU failure (no adapter, lost device, validation or OOM, timeout, an off-curve
    result, or a failed pairing check) is an error, never a wrong proof. Callers should fall
    back to the CPU on `Error::Gpu`.
- The `Msms` intermediate points and wgpu/browser staging copies of uploaded data are not
  zeroized; see `../lowki-gpu/README.md`.
- No wasmi profile overrides were needed for `lowki-gpu`, because it does not depend on
  this crate.

## Known gaps and risks

- **wasmi dispatch in native debug builds.** wasmi 2.0's tail-call dispatch overflowed the stack
  in dev/test profiles. This crate sets `[profile.dev.package.wasmi] opt-level = 3`,
  `debug-assertions = false` and `overflow-checks = false`. **Any crate that depends on this one
  must copy that override.** Release builds and wasm32 (loop dispatch) are unaffected.
- **No threads in the browser.** CPU wasm proving is single-threaded: about 2.05 s for
  withdraw in Chrome and ~2.2 s under Node. Threads would need COOP/COEP, which an embedded
  iframe can't rely on. The `webgpu` feature (see above) is the answer: ~263 ms in Chrome.
  The H witness map (FFTs, ~176 ms) is now the largest stage.
- **wasm32 witness engine.** The host engine was tested in Chrome 156 (main thread, under
  wasm-bindgen-test) and in Node 24. Safari and Firefox were not tested. If a browser refuses
  the synchronous compile in `load`, the witness silently falls back to wasmi: still correct,
  but about 30× slower. Check `witness_backend()`, or use `load_async`.
- **zkey trust.** zkey G2 points are checked to be on the curve but not in the subgroup. A
  malicious zkey can only produce proofs that fail the self-check or fail on-chain, not leak
  secrets. Still, pin the artifact sha256 values above in the browser loader; that is not yet
  enforced in code.
- **Zeroization is best-effort.** Input structs zeroize on drop, `Debug` is redacted, the
  witness engine's linear memory (wasmi store or `WebAssembly.Memory`) and the witness `Vec` are zeroed, and the `r`/`s` blinding factors are
  zeroed. Temporaries inside `ark-groth16` (the h vector, bigint assignments) and any JS-side
  copies are not.
- **No EVM run.** The on-chain Solidity verifier was not executed (no anvil/`eth_call`).
  Acceptance is inferred from snarkjs `verify` plus exact equality with Kohaku's on-chain encoding.
- **`witness/*.bin` is not committed** (~9 MB). The committed `vectors.json` pins each full
  witness by sha256 and length.
