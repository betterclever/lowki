# lowki-groth16

Groth16 (bn128) prover for the Privacy Pools v1 `commitment` and `withdraw` circuits
(`@fatsolutions/privacy-pools-core-circuits@1.0.5`, built for snarkjs 0.7.5). It is written
in Rust and runs both natively and as `wasm32-unknown-unknown`. Its proofs are accepted by
snarkjs `groth16.verify`, and it outputs the Solidity `{pA, pB, pC, pubSignals}` struct.

## Public API

```rust
let prover = CircuitProver::load(Circuit::Withdraw, &wasm_bytes, &zkey_bytes)?; // must be the pinned 1.0.5 artifacts
let prover = CircuitProver::load_async(Circuit::Withdraw, &wasm_bytes, &zkey_bytes).await?; // wasm32 only
let prover = CircuitProver::load_unpinned(Circuit::Withdraw, &wasm_bytes, &zkey_bytes)?; // escape hatch, see below
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
- **Pinned artifacts.** `load` and `load_async` first check the sha256 of both artifacts
  against `PINNED_SHA256`, which holds the `@fatsolutions/privacy-pools-core-circuits@1.0.5`
  values listed under "Running". On a mismatch they return
  `Error::Circuit("<circuit>.<wasm|zkey> has sha256 …, not the pinned … (use load_unpinned for custom artifacts)")`.
  The check costs about 40 ms for withdraw natively, and about 37 ms of the one-time load
  in Chrome.
- **Escape hatch.** `CircuitProver::load_unpinned` (and the wasm32-only `load_async_unpinned`)
  skip the pin. They are meant for tests and for custom or re-generated artifacts that you
  trust to match your on-chain verifier. Because the bytes are not pinned, they
  subgroup-check every proving-key G2 point (see "zkey + proving"). For withdraw that makes
  the load slower: 0.85 s natively, 2.5 s in Chrome.
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
- It checks every point is on the curve. On bn254, G1 has cofactor 1, so for G1 that is also
  the subgroup check.
- It checks the verifying key's G2 points (β, γ, δ) for subgroup membership. It also checks
  every `b_g2_query` point (36 901 for withdraw), unless the file is the sha256-pinned
  artifact. Why that check matters: an on-curve G2 base outside the r-order subgroup would
  put a small-order component into the proof's B. Its value would depend on the witness, so
  a malicious zkey could leak witness bits through it. The pinned zkeys pass the full check
  (`tests/artifacts.rs`, which runs it on the exact bytes the pin accepts). So a matching
  pin implies the check, and `load` skips its cost: 0.85 s native and 2.5 s in Chrome for
  withdraw. The check is `is_in_correct_subgroup_assuming_on_curve`, which arkworks
  implements with the ψ endomorphism. The vkey parser (`parse_vkey_json`) and proof parser
  (`SnarkjsProof::to_ark`) already checked G2 subgroup membership and G1 on-curve.
- It fixes upstream's off-by-one in `num_witness_variables`.

`src/qap.rs` is ark-circom's `CircomReduction` (snarkjs' R1CS→QAP map), copied verbatim. The
only change splits the sparse constraint evaluation out (`constraint_evaluations`), so that the
WebGPU path can run the rest of it on the GPU.
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
| artifact pins and zkey points | `tests/artifacts.rs` (native). `load` accepts the pinned files and rejects a single flipped byte in either one with the sha256 message. The pinned zkeys pass the full G2 subgroup check (`load_unpinned`). An on-curve G2 point outside the subgroup, written into `b_g2_query`, is rejected by `load_unpinned` (`Error::Zkey`) and by `load` (pin). Swapped circuits are rejected. | pass |
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
  stay resident on the GPU (a, b_g1, b_g2, l, h: 38 MiB for withdraw). The QAP twiddle and
  coset tables also stay resident (10 MiB for withdraw).
- **What runs on the CPU.** The witness runs there (the host's WebAssembly engine on wasm32,
  wasmi natively), and so does the sparse constraint evaluation (a, b).
- **What runs on the GPU.** The rest of the H witness map runs on the GPU via
  [`lowki-gpu`](../lowki-gpu/README.md)'s `qap` module: c = a∘b, 3 inverse NTTs, the coset
  shift, 3 NTTs, and h = ab − c, all exact. It uses the same ω and g as ark-poly's domain.
  The MSMs also run on the GPU: A, B_G1, L and H in G1, and B in G2. One exception: for
  small circuits (≤ 8 192 bases), B in G2 runs on the CPU instead, alongside the GPU's G1
  MSMs. A small G2 MSM is bound by GPU step latency: each step is one Fq2 point addition per
  thread. Measured for commitment in Chrome, this changes the total from 70 to 48 ms.
- **Same proof as the CPU path.** Assembly uses exactly the CPU path's formula
  (`src/stages.rs`). With the same RNG the proof is byte-identical to
  `create_proof_with_reduction_and_matrices` with `CircomReduction`.
- **Always pairing-checked.** Every GPU proof goes through the same 3-pairing self-check
  before it is returned.
- **Overlap.**
  1. The QAP is submitted first.
  2. The CPU plans the H-independent MSMs (A, B_G2, B_G1, L) and submits them while the GPU
     runs the NTTs.
  3. h is read back (2 MiB) and H is planned and submitted.
  4. On small circuits, the CPU then computes B in G2.
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
- **NTT/FFT: GPU (round 2).** Once the witness took ~25 ms, the CPU FFTs became the largest
  stage of a WebGPU withdraw in Chrome (~176 ms of ~263 ms), and they were on the critical
  path. Two options were measured first:
  - A 32-bit-limb Montgomery multiply for wasm32 brought **no gain**: 31.7 ns against ark's
    32.8 ns in Chrome (`bench/rust/src/fieldbench.rs`), so it was dropped.
  - ark's 2^16 FFT takes 26.5 ms in Chrome. The six FFTs are ~159 ms.

  The GPU QAP does the whole 2^16 map in 6.8 ms on Metal, including upload and readback.
  In Chrome the wait for it is 2–9 ms. The critical path is now the GPU MSMs.

### Timings

Measured on the host: Apple M5 Max, macOS 27.2, Metal, Chrome 156.0.8078.80 headless with
`--enable-unsafe-webgpu`. Adapter: `apple` / `metal-3`. "Prove" includes witness generation,
the FFTs, the MSMs and the pairing self-check.

| | commitment | withdraw |
|---|---|---|
| native CPU, 1 thread | 50–59 ms (profile avg 52 ms) | 856–916 ms (profile avg 886 ms) |
| native Metal (`GpuProver`) | 33–39 ms (`make test-webgpu`) | 187–248 ms (natively the 111 ms wasmi witness dominates; round 1: 182–242 ms) |
| wasm CPU, headless Chrome | 127–152 ms (bench median 127 ms) | 2.07–2.15 s (bench median 2.08 s) |
| **wasm WebGPU, headless Chrome** | **47–53 ms** (bench median 47.6 ms; round 1: 63 ms) | **171–195 ms** (bench median 168 ms; round 1: 263 ms; before the witness fix: 1.08 s) |
| snarkjs 0.7.5 `fullProve`, same Chrome, default web workers (18), for reference | 39 ms | 603 ms (580 ms without COOP/COEP) |

In-browser withdraw on WebGPU is **3.6× faster than snarkjs using all 18 cores**. Commitment
is still 1.2× behind (47.6 ms vs 39 ms): the CPU G2 MSM (~35–41 ms in wasm) is now its
critical path. The bench rows ("bench median") are 10 proofs after 2 warm-ups
(`bench/RESULTS.md`). The ranges are the 7 or 10 vectors.json cases from
`make verify-webgpu-chrome`. Round-2 numbers were taken while other work was running on the
host. snarkjs, whose code didn't change, was re-run in the same session as a control, and
came out within 4 % of round 1.

WebGPU withdraw in Chrome, per stage:

| Stage | Time |
|---|---|
| witness (host WebAssembly engine) | 21–30 ms (with wasmi: 463–919 ms) |
| constraint evaluation (CPU) + QAP submit | 2–3 ms |
| CPU plan + submit of A/B/L (the GPU runs the NTTs meanwhile) | 11–13 ms |
| wait for the GPU QAP + read h | 2–9 ms (round 1: 176–181 ms of CPU FFTs) |
| plan + submit H | 7–8 ms |
| wait for the GPU MSMs | 101–142 ms |
| assemble + self-check | 3–4 ms |

Tuning tried in round 2, measured in Chrome and **not adopted** (no measurable win, within
noise):
- `TASK_MAX` ∈ {8, 16, 32} × window bits ∈ {5…9} (`bench/tune-gpu.sh`).
- Interleaving the MSM jobs' dispatches in one pass. This was slower: commitment 82 ms vs
  65 ms, withdraw 220 ms vs 177 ms. Reverted.

The GPU MSMs (about 90 ms of GPU time natively) are now the floor for withdraw.

Other wasm-path costs:
- zkey parse + `CircuitProver::load` takes 67–75 ms for withdraw and 11–48 ms for
  commitment in Chrome, one time. That includes the sha256 pin (about 37 ms). The first
  load in a page is slower because the hashing code is still cold in the JIT.
- Witness compile is ~1 ms; instantiate per proof is ~0.1 ms.

One-time setup:

| | Native Metal | Chrome |
|---|---|---|
| `Gpu::new` (device + shader compile, MSM and QAP modules) | 17–38 ms with a warm Metal shader cache; 0.8–2.4 s the first time after a shader change | 46–75 ms in the bench (fresh profile); one cold run in the crate test took 600 ms |
| base upload, withdraw (incl. 10 MiB of QAP tables since round 2) | 13–27 ms (38 MiB) | 40–45 ms |
| base upload, commitment | 1–5 ms (1 MiB) | 2 ms |

### Verification

| Check | Command | Result |
|---|---|---|
| GPU proof is byte-identical to the CPU proof (same ChaCha20 seed) for every vectors.json case, and verifies | `make test-webgpu` (`tests/gpu.rs`, native Metal) | 17/17 identical |
| GPU MSMs on the real withdraw proving-key bases (a, b_g1, l, b_g2 with the real witness and with random scalars; h at 2^16 with random scalars) equal ark | same | pass (44 472 points at infinity among the a/b_g2 bases) |
| snarkjs 0.7.5 verifies the native GPU proofs | `npx tsx verify-rust-proofs.ts out/gpu-proofs.json` | 17/17 |
| Same as the first row, in headless Chrome WebGPU (wasm32, CPU and GPU proofs made in the same module) | `make verify-webgpu-chrome` (`tests/wasm_gpu.rs`) | 17/17 identical |
| snarkjs 0.7.5 verifies the browser-made GPU proofs | (last step of the same target) | 17/17 |
| GPU QAP (NTTs) == CPU witness map, element by element, n = 2 … 2^16, plus edge values | `../lowki-gpu/tests/qap.rs` (Metal) | pass |
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
  iframe can't rely on. The `webgpu` feature (see above) is the answer: ~168 ms in Chrome.
  GPU MSM time is the floor now.
- **G2 on the CPU for small circuits.** The threshold of 8 192 bases is a fixed constant
  (`G2_ON_CPU_MAX`). It was measured at 1.5k bases (CPU wins) and 37k bases (GPU wins), and
  not between.
- **wasm32 witness engine.** The host engine was tested in Chrome 156 (main thread, under
  wasm-bindgen-test) and in Node 24. Safari and Firefox were not tested. If a browser refuses
  the synchronous compile in `load`, the witness silently falls back to wasmi: still correct,
  but about 30× slower. Check `witness_backend()`, or use `load_async`.
- **zkey trust.** `load` enforces the sha256 pins, and `load_unpinned` subgroup-checks every
  G2 point (see "zkey + proving"). The vkey and proof parsers check every point. What still
  has to be trusted is the circuit itself: the pinned 1.0.5 release.
- **Zeroization is best-effort.** Input structs zeroize on drop, `Debug` is redacted, the
  witness engine's linear memory (wasmi store or `WebAssembly.Memory`) and the witness `Vec` are zeroed, and the `r`/`s` blinding factors are
  zeroed. Temporaries inside `ark-groth16` (the h vector, bigint assignments) and any JS-side
  copies are not.
- **No EVM run.** The on-chain Solidity verifier was not executed (no anvil/`eth_call`).
  Acceptance is inferred from snarkjs `verify` plus exact equality with Kohaku's on-chain encoding.
- **`witness/*.bin` is not committed** (~9 MB). The committed `vectors.json` pins each full
  witness by sha256 and length.
