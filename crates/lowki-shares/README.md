# lowki-shares

Shamir secret sharing over GF(2^8), byte by byte, **format-compatible with Privy's
[`shamir-secret-sharing`](https://github.com/privy-io/shamir-secret-sharing) 0.0.4**.
`no_std` + `alloc`, `#![forbid(unsafe_code)]`, builds for `wasm32-unknown-unknown`.
Dependencies: `rand_core` 0.6 (no `getrandom`: the caller passes the RNG) and `zeroize`.

## Public API

```rust
fn split<R: RngCore + CryptoRng + ?Sized>(secret: &[u8], n: usize, t: usize, rng: &mut R) -> Result<Vec<Share>, Error>
fn combine<S: AsRef<[u8]>>(shares: &[S], threshold: usize) -> Result<Zeroizing<Vec<u8>>, Error>
fn combine_shares(shares: &[Share], threshold: usize) -> Result<Zeroizing<Vec<u8>>, Error>
fn interpolate_share<S: AsRef<[u8]>>(shares: &[S], threshold: usize, x: u8) -> Result<Share, Error>
struct Share        // y bytes || x; ZeroizeOnDrop; Debug = "Share(x=.., len=.., ..)"
  ::from_bytes(&[u8]) / ::from_vec(Vec<u8>) -> Result<Share>   // rejects < 2 bytes and x = 0
  .as_bytes() / .x() / .secret_len()
const MAX_SHARES: usize = 255;
enum Error { EmptySecret, InvalidShareCount, InvalidThreshold, TooFewShares, TooManyShares,
             ShareTooShort, LengthMismatch, DuplicateX, ZeroX }
```

## Format (same as Privy)

- Field: GF(2^8), AES polynomial x^8 + x^4 + x^3 + x + 1 (0x11b). Privy computes with
  log/exp tables for generator 0xe5. This crate uses the same field without tables
  (shift-and-add multiply, inverse = a^254), so it is constant time. The tests check every
  one of the 65 536 products and 65 280 quotients against Privy's own tables.
- Share = `y_0 … y_{len-1} || x`: one y-byte per secret byte, then one trailing
  x-coordinate byte (1..=255). A 32-byte secret gives 33-byte shares.
- `split` draws randomness in Privy's order. First it draws 255 bytes for the biased x-coordinate
  shuffle (`j = r[i] % 255`), then `t - 1` coefficient bytes for each secret byte. So the same
  byte stream gives byte-identical shares.

## Differences from Privy (stricter, on purpose)

| Input | Privy 0.0.4 `combine` | lowki-shares `combine` |
|---|---|---|
| fewer shares than the threshold | **returns a wrong secret** (it has no threshold parameter) | `TooFewShares` |
| duplicate x | throws | `DuplicateX` |
| x = 0 | accepted | `ZeroX` (`split` never makes it) |
| mixed lengths, < 2 shares | throws | `LengthMismatch` / `TooFewShares` |

Like Privy, `combine` interpolates over all the shares it is given. Shares from different
splits are not detected; that would need a MAC over the secret.

## Verification

`tests/vectors.rs` loads `vectors/lowki-shares/vectors.json`. The file is produced by
`gen.ts` from Privy's **unmodified** package. `rng-hook.mjs` redirects the package's
self-import `shamir-secret-sharing/csprng` to a replay source (SHA-256 counter DRBG).
`gen.ts` records exactly the bytes `split()` consumed. It also records the source sha256
(`f5096daf…`) and Node version in `meta`.

| Check | Cases | Result |
|---|---|---|
| Field ops == Privy's LOG/EXP tables (parsed from the installed `src/index.ts`) | all 65 536 products, 65 280 quotients | pass |
| **Fixed-RNG split equality**: Rust `split` with Privy's recorded random bytes == Privy's shares, byte for byte, and consumes exactly the same number of bytes | 217 splits (secret lengths 1–256, n/t from 2/2 to 255/255, random n/t, 16 Lowki-shaped 2-of-3 over 16 and 32 bytes, edge secrets 0x00…/0xff…) | 217/217 |
| **JS split → Rust combine** (threshold, random superset, all shares) | 457 subsets | 457/457; t−1 shares → `TooFewShares` |
| **Rust split → JS combine**: `rust_split_export` writes `out/rust-shares.json`; `npm run verify-rust` runs Privy `combine` | 120 splits × 3 subsets (incl. 255-of-255, 2-of-255) | 360/360 |
| Privy returns garbage from t−1 shares; we reject (and with a lowered threshold we reproduce Privy's exact wrong answer) | 12 | pass |
| Privy error inputs (duplicate x, mixed length, 1 share) rejected | 3 | pass |
| `interpolate_share` re-creates every Privy share from the first t | all 217 cases | pass |
| Rust-only: parameter bounds, x = 0, > 255 shares, redacted `Debug` | — | pass |

```sh
cd vectors/lowki-shares && npm install && npm run gen         # vectors.json (deterministic)
cd crates/lowki-shares
export CARGO_TARGET_DIR=/Users/betterclever/projects/ethglobal/lowki/target/lowki-shares
cargo test --release && cargo clippy --all-targets -- -D warnings
cargo clippy --all-targets --target wasm32-unknown-unknown -- -D warnings
cargo build --release --target wasm32-unknown-unknown
cd ../../vectors/lowki-shares && npm run verify-rust          # Rust split -> Privy combine
```

## Zeroization

The secret result, each `Share`, the coefficient buffer, the shuffle bytes, the
x-coordinate list and the Lagrange basis are `Zeroizing` or `ZeroizeOnDrop`. Field
temporaries are `u8` locals.

## Known gaps

- The random fields are not authenticated: a corrupted share gives a wrong secret, not an error.
  lowki-worker checks only that the result is 32 bytes. A wrong share therefore opens a
  different wallet, so the UI should compare the recovered app address with the known one.
  `rewrap` does check that S and R reproduce the unlocked seed before it re-derives P.
- `combine` time depends on the number of shares and the share length, not on secret values.
  The x-coordinate duplicate check branches on x, but x is public.
