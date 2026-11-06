# lowki-gpu

WebGPU multi-scalar multiplication (MSM) for BN254 G1 and G2, written in Rust with
`wgpu` 30.0.1 and WGSL. It accelerates the Lowki Groth16 prover
(`crates/lowki-groth16`, feature `webgpu`).

It runs on two targets:

- natively, on Metal or Vulkan;
- in the browser, as `wasm32-unknown-unknown` using the browser's WebGPU through wgpu's
  `webgpu` backend.

Why WebGPU and not wasm threads: the prover runs inside a Lowki iframe embedded in
third-party dapps. wasm threads need `SharedArrayBuffer`, which means cross-origin
isolation, and an embedded iframe can't count on having it. WebGPU needs neither.

## Public API

```rust
let gpu = Gpu::new().await?;                        // Err(GpuError::Unavailable(..)) -> fall back to CPU
let a   = gpu.upload_bases(&pk.a_query)?;           // G1 or G2 affine bases; uploaded once, stays resident
let b2  = gpu.upload_bases(&pk.b_g2_query)?;
let s   = BigInts::from_fr(&witness);               // canonical scalars, zeroized on drop
let pending = gpu.submit(&[MsmJob::G1(&a, &s.0), MsmJob::G2(&b2, &s.0)])?;  // returns immediately
// ... CPU work here overlaps with the GPU ...
let out: Vec<MsmOutput> = pending.wait().await?;    // MsmOutput::G1(G1Projective) / ::G2(..)
let p = gpu.msm_g1(&a, &scalars).await?;            // convenience wrapper for one MSM
```

- **Async.** The API is async because WebGPU adapter/device requests and buffer mapping are
  async. Natively, drive it with `pollster::block_on`: waits call `device.poll` with a 30 s
  timeout. On the web, the future resolves from the browser event loop.
- **Errors.** `GpuError::{Unavailable, Device, Input}`. Each of these returns
  `Unavailable`, so the caller can fall back to the CPU:
  - no WebGPU (`navigator.gpu` missing);
  - no adapter, or only a CPU software adapter;
  - a failed device request;
  - a lost device;
  - a native poll timeout.

  Validation and out-of-memory errors are caught with error scopes, and mapping errors
  become `Device`. A result that is malformed or not on the curve also becomes `Device`.
  `Input` covers bad arguments: more scalars than bases, a scalar ≥ r, or a buffer larger
  than the device limit.
- `GpuBases::{len, gpu_bytes, window_bits, set_window_bits}`, `Gpu::{adapter_info, max_storage_binding}`.
- Test-only hooks (`#[doc(hidden)]`): `field_selftest`, `point_selftest`,
  `debug_scratch_g1`.

## Design

**Field.** WGSL has no `u64`, so an Fq element is stored as 20 limbs of 13 bits, one per `u32`
(260 bits). It is kept in Montgomery form with R = 2^260 and always fully reduced (< p).

- Montgomery product: CIOS with limb products of 26 bits, so
  `s + x·y + q·p + carry < 2^28` never overflows. This is the trick used by the ZPrize 2023
  WebGPU MSM entries.
- The inner limb loop is unrolled, with the modulus limbs as literals. The outer loop stays a
  loop (see "Metal compiler" below).
- add/sub: unrolled carry chains with a conditional subtraction.
- `src/field.rs` holds the conversions (`x·2^260 mod p` ↔ arkworks) and a CPU model of the
  WGSL product with overflow checks.

Fq2 = Fq[u]/(u²+1), multiplied with Karatsuba (3 Fq products).

**Curve.** Homogeneous projective coordinates with the complete Renes–Costello–Batina
formulas for a = 0: Algorithm 7 (add), 8 (mixed add) and 9 (double). b3 = 9 for G1, and
3·3/(9+u) for G2. Complete formulas mean P+P, P+(−P) and the point at infinity need no
special cases.

**Pippenger** (`src/plan.rs`, CPU side, per MSM):

1. **Signed digits.** Window c bits, W = ⌈255/c⌉ windows, B = 2^(c−1) buckets per window,
   digits |d| ≤ 2^(c−1). Zero scalars, and bases at infinity (flagged at upload), are skipped.
2. **Counting sort.** Sort the references `base index | sign<<31` by bucket key.
3. **Bounded tasks.** Every bucket becomes tasks of at most 32 points. A larger bucket is
   split, and its partial sums are summed at the next level. Witness vectors are full of
   0/1 and other small values, so a few buckets are huge. Bounded tasks keep that skew from
   serializing a single thread.
4. **Window size.** c = clamp(log2 n − 4, 4, 8). This was tuned on the real withdraw proof:
   c = 8 is best for the whole proof, and a random 2^16 G1 MSM is flat between c = 8 and 13.

**GPU side** (`src/shader.rs`). Every kernel is straight-line, with at most one point
addition per thread per dispatch. Sequential sums become one dispatch per step, all in one
compute pass. WebGPU orders the dispatches within a pass.

- `acc_base_step` / `acc_scr_step`: step k adds the k-th element of each task, either a
  signed base via mixed addition or a partial sum. Tasks are sorted by length, so step k only
  dispatches the tasks that are longer than k.
- `run_step`: in-place running sums inside each chunk of m = 2^⌊(c−1)/2⌋ buckets, then
  inside each window's chunk-sum array.
- `sum_step`: segment sums, giving U_j = Σ running sums and T_j.
- Each window yields two points, `ΣU` and `Σ_{k≥1} suffix(T)_k`. The window sum is
  `ΣU + m·Σ suffix`; the CPU computes it and then combines the windows with Horner's rule.

All of this is cheap on the CPU: W ≤ 64 points, and a few hundred doublings.

**Batching and overlap.** `submit` encodes several MSMs into one command buffer and returns
immediately. The Groth16 integration submits A, B_G2, B_G1 and L first. It then computes the
H witness map (FFTs) on the CPU while the GPU works, and submits H afterwards.

**Secrets.** Scalars are secret witness data.

- **Uploaded per proof:** only scalar-derived data (the sorted references, task list and
  step parameters). Bases are uploaded once per `CircuitProver`.
- **After the result is read:**
  - the reference, task and parameter buffers are overwritten with zeros via
    `queue.writeBuffer`;
  - the scratch buffer (bucket sums) and the readback buffer are cleared on the GPU;
  - once that work is done, the buffers are `destroy()`ed.
- **If a `Pending` is dropped without `wait()`:** the same zeroing is queued and the buffers
  are destroyed.
- **CPU copies:** digits, counts, plan and words are zeroized (`Zeroizing` /
  `ZeroizeOnDrop` / `BigInts`).

### Metal compiler

On Apple M5 Max / Metal 4, kernels with a loop around a full point addition did not run
correctly at this code size: they hung the GPU indefinitely or produced points off the curve.
This hit G1 kernels with two point accumulators in one loop, and every G2 loop. A kernel that
inlined two different point-addition bodies (mixed and projective, in two branches) gave
wrong results in some lanes.

The point formulas themselves were verified correct by a straight-line self-test kernel
(`point_selftest`, G1 and G2, including P+P, P+(−P), infinity on either side and Z = 1). A
CPU simulator of the exact same plan (`plan::sim`) also matches ark.

The fix is structural: one point-addition body per kernel and no loops around it, with the
host stepping the loops. The test `gpu_scratch_matches_simulator` compares every
intermediate GPU scratch slot with the simulator.

A native `device.poll` timeout (30 s) turns a hung GPU into `GpuError::Device`, and marks the
device lost so that later calls return `Unavailable`. Dropping that `Gpu` can still block in
wgpu's queue teardown if the GPU really is stuck.

## Verification

Hardware: Apple M5 Max, macOS 27.2 (Darwin 27.2.0), Metal. Browser: Google Chrome
156.0.8078.80, headless (`--headless=new --enable-unsafe-webgpu`), through chromedriver
156.0.8078.80 and `wasm-bindgen-test-runner` 0.2.130.

The browser adapter reported by `navigator.gpu.requestAdapter().info` was
`{vendor: "apple", architecture: "metal-3"}`, with maxStorageBufferBindingSize and
maxBufferSize of 4294967292. wgpu's own `AdapterInfo` is empty on the web.

| Test | What | Result |
|---|---|---|
| `field::tests` (CPU) | 13-bit limb CIOS model == ark Fq mul (2000 random + edge cases, overflow-checked) | pass |
| `plan::tests` (CPU) | signed-digit recomposition for c = 2..16; plan simulator == ark `VariableBaseMSM` at n ∈ {1,2,3,7,255,256,1000} × c ∈ {2,3,4,5,8,11}, random and small scalars, infinity and repeated bases; G2 n = 300 | pass |
| `field_ops_match_ark` (GPU) | WGSL Fq mul/add/sub == ark on 4096 random pairs + 0, −1, 1, 2 | pass on Metal |
| `point_formulas_match_ark` (GPU) | add, mixed add, dbl, P+P == ark for G1 and G2 (300 cases each: P=Q, P=−Q, infinity on either side, Z=1) | pass on Metal |
| `gpu_scratch_matches_simulator` (GPU) | every intermediate GPU slot == CPU simulator for 5 (n, c) configurations | pass on Metal |
| `g1_msm_matches_ark` (GPU) | n ∈ {1, 2, 3, 255, 256, 1000, 2^16} × {random, mixed 0/1/−1/u64::MAX/random, small, all-zero, all r−1} + {infinity among bases, repeated bases, P and −P, (r−1)·P + 1·P, all-equal bases}; also 1·G, (r−1)·G, (r−1)·G+G = 0, 3·∞, fewer scalars than bases | pass on Metal (affine equality) |
| `g1_window_sizes` (GPU) | n = 3000 at every c = 2..16 | pass on Metal |
| `g2_msm_matches_ark` (GPU) | n ∈ {1, 2, 3, 255, 256, 1000, 2^15} × {random, mixed, all-zero, all r−1} + edge cases | pass on Metal |
| `batch_of_jobs`, `rejects_bad_input` (GPU) | mixed G1/G2 batch; too many scalars and unreduced scalars are rejected | pass on Metal |
| `tests/wasm.rs` (headless Chrome) | G1 n ∈ {1, 2, 3, 255, 256, 1000, 2^16} with random scalars (plus r−1, 0, infinity and a repeated base) and all-zero scalars; G2 n = 1000; vs ark compiled into the same wasm | pass in Chrome WebGPU |
| real proving-key bases | see `lowki-groth16/tests/gpu.rs`: a/b_g1/l/b_g2 with the real witness and random scalars, h (2^16) with random scalars, == ark | pass on Metal |

Without a GPU, as on the Linux VM, every GPU test prints `SKIP` and passes; the CPU tests
still run.

MSM timings on Metal at c = 8:

| MSM | Time |
|---|---|
| G1, 2^16, random scalars | 27 ms |
| G1, 2^16, all-zero scalars | 7 ms (fixed cost) |
| G2, 2^15, random scalars | 56 ms |
| G1, 2^16, random scalars, headless Chrome | 21 ms warm, 60 ms first call |

For whole-proof numbers, see `lowki-groth16/README.md`.

## Running

Every native command uses `CARGO_TARGET_DIR=<lowki>/target/lowki-gpu`, or `lowki-gpu-mac` on
the host.

```sh
cargo test --release -- --nocapture --test-threads=1          # Metal/Vulkan if present, else SKIP
cargo clippy --all-targets -- -D warnings
cargo clippy --all-targets --target wasm32-unknown-unknown -- -D warnings
cargo build --release --target wasm32-unknown-unknown
cargo run --release --example msm_bench -- 65536 [c]          # G2=1 for G2
```

Browser (headless Chrome with WebGPU). The runner reads `browser/webdriver.json`, which holds
the Chrome flags, from its working directory:

```sh
cargo install wasm-bindgen-cli --version 0.2.130 --root <lowki>/target/tools
(cd <lowki>/target/tools && npx @puppeteer/browsers install chromedriver@<installed Chrome version>)
cargo test --release --target wasm32-unknown-unknown --test wasm --no-run   # prints the .wasm path
cd browser && CHROMEDRIVER=<chromedriver> <lowki>/target/tools/bin/wasm-bindgen-test-runner <wasm> --nocapture
```

The test prints `LOWKI_GPU adapter ...`, then `LOWKI_MSM ... ok` lines. If it prints
`SKIP: ...`, WebGPU was not available and nothing was tested.

## Known gaps and limitations

- **Browsers.** Only Chrome on macOS (Metal) was tested. Safari and Firefox WebGPU are
  untested, and so are Windows (D3D12), Linux and Android (Vulkan) and discrete GPUs.
  - The Metal compiler workaround (straight-line kernels) is defensive, but other drivers
    can have their own bugs.
  - The pairing self-check in `lowki-groth16`, and the on-curve check here, mean a
    miscompiling GPU produces an error and a CPU fallback, never a bad proof.
- **Memory.**
  - Resident bases take 38 MiB for withdraw (a, b_g1, l, h: 160 B per point; b_g2: 320 B)
    and 1 MiB for commitment.
  - Per proof, the transient buffers (references, tasks, scratch) are estimated, not
    measured, at roughly 20–30 MiB for withdraw. The estimate is n·W·4 B of references per
    MSM plus up to (W·B + partials + 2·W·√B)·240/480 B of scratch.
  - The largest single binding is the h bases at 10 MiB, or the references, which are up to
    n·W·4 B: 8.4 MiB for H at c = 8. Both are far below the WebGPU default
    `maxStorageBufferBindingSize` of 128 MiB. The device requests the adapter's own maxima.
  - A buffer over the limit is an `Input` error, which callers treat as "fall back".
- **CPU work per proof.** The digits, counting sort and task list run on the CPU: about
  10–16 ms in wasm for withdraw, and the code is single-threaded. A GPU sort would remove
  this.
- **Zeroization is best effort.** `queue.writeBuffer` copies the data through wgpu or
  browser staging memory, which this crate cannot wipe. GPU memory freed by `destroy()` is
  zeroed by the WebGPU implementation before reuse, but not necessarily right away.
- **Native hang.** If the GPU really hangs, the 30 s poll timeout returns an error, but
  dropping the `Gpu` may then block (wgpu waits for the queue).
- **Cold shader compile.** The first `Gpu::new` after a shader change took 0.8–2.4 s on the
  host (Metal compiler cache cold); warm, it takes 15–40 ms. In headless Chrome,
  `Gpu::new` measured about 30 ms. Chrome may compile pipelines lazily or keep its own
  cache, so a cold start on a user's machine may cost more than measured here.
- **Fixed parameters.** BN254 only; scalars must be < r.
