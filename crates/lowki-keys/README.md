# lowki-keys

Key primitives for Lowki: BIP-39, a Kohaku-compatible BIP-32 keystore (with scoping),
per-origin EOAs, wrapping the seed with a passkey PRF output, and secp256k1 digest signing.
The crate builds on its own and compiles for `wasm32-unknown-unknown` without `getrandom`
(callers pass in all randomness: entropy and nonces).

## Public API

```rust
// BIP-39 (English, empty passphrase)
fn entropy_to_mnemonic(entropy: &[u8; 32]) -> Zeroizing<String>           // 24 words
fn validate_mnemonic(mnemonic: &str) -> bool                               // == scure validateMnemonic
fn mnemonic_to_entropy(mnemonic: &str) -> Result<Zeroizing<[u8; 32]>>      // 24-word phrases only
fn mnemonic_to_seed(mnemonic: &str) -> Result<Zeroizing<[u8; 64]>>         // validates first
fn entropy_to_seed(entropy: &[u8; 32]) -> Zeroizing<[u8; 64]>

// Derivation paths (Kohaku parseDerivationPath)
const HARDENED_OFFSET: u32 = 0x8000_0000;
fn parse_derivation_path(path: &str) -> Result<Vec<u32>>

// Kohaku MnemonicKeystore.deriveAt / scopeKeystore
fn derive_at(seed: &[u8; 64], path: &str) -> Result<Zeroizing<[u8; 32]>>
fn derive_indices(seed: &[u8; 64], indices: &[u32]) -> Result<Zeroizing<[u8; 32]>>
struct Keystore            // owns the seed, ZeroizeOnDrop, Debug prints "Keystore(..)"
  ::from_seed(&[u8; 64]) / ::from_entropy(&[u8; 32]) / ::from_mnemonic(&str) -> Result<_>
  .derive_at(&str) -> Result<Zeroizing<[u8; 32]>>
  .scope(prefixes: &[&str], paths: &[&str]) -> Result<ScopedKeystore>
struct ScopedKeystore
  ::new(Keystore, prefixes, paths) -> Result<_>   // Err(InvalidScope) if a prefix doesn't parse
  .allow_path(&mut self, &str)                    // Kohaku's `paths` Set may grow
  .is_allowed(&str) -> Result<bool>
  .derive_at(&str) -> Result<Zeroizing<[u8; 32]>> // Err(PathNotAllowed) / Err(InvalidPath)

// Lowki per-app EOA
fn app_index(origin: &str) -> u32   // u32_be(keccak256("lowki/app/v1" || origin)[0..4]) & 0x7fffffff
fn app_path(origin: &str) -> String // m/44'/60'/{app_index}'/0/0
fn app_private_key(seed: &[u8; 64], origin: &str) -> Zeroizing<[u8; 32]>
fn app_address(seed: &[u8; 64], origin: &str) -> [u8; 20]
fn address_from_private_key(&[u8; 32]) -> Result<[u8; 20]>
fn to_checksum_address(&[u8; 20]) -> String       // EIP-55, 0x-prefixed

// Seed wrapping with a passkey PRF output
const WRAPPED_LEN: usize = 61;
fn derive_kek(prf: &[u8; 32]) -> Zeroizing<[u8; 32]>   // HKDF-SHA256(prf, salt="", info="lowki/kek/v1")
fn wrap_entropy(prf: &[u8; 32], entropy: &[u8; 32], nonce: &[u8; 12]) -> [u8; 61]
fn unwrap_entropy(prf: &[u8; 32], blob: &[u8]) -> Result<Zeroizing<[u8; 32]>>
//   blob = 0x01 || nonce(12) || AES-256-GCM(kek, nonce, entropy, aad="lowki/seed/v1") (ct||tag16)
//   errors checked in order: BadLength (len != 61), BadVersion (byte 0 != 1), Decrypt (tag)

// Signing
struct Signature { r: [u8; 32], s: [u8; 32], v: u8 }   // v = 27 + y_parity; .y_parity(), .to_bytes() = r||s||v
fn sign_digest(private_key: &[u8; 32], digest: &[u8; 32]) -> Result<Signature>  // RFC 6979, low-s

enum Error { InvalidMnemonic, InvalidPath, InvalidScope, PathNotAllowed, RailgunNonHardened,
             InvalidDerivedKey, InvalidPrivateKey, BadLength, BadVersion, Decrypt }
```

Dependencies: `bip39` 2.2 (no default features, `zeroize`), `k256` 0.13, `hmac`, `sha2`,
`sha3` (Keccak-256), `hkdf`, `aes-gcm` 0.10 (no `getrandom`), `zeroize`.

## What is verified against what

`tests/vectors.rs` loads `vectors/lowki-keys/vectors.json` and checks byte-for-byte equality.

| Area | Cases | Reference |
|---|---|---|
| BIP-39 generate (entropy → mnemonic → seed, and back) | 64 | `@scure/bip39` 2.2.0 |
| BIP-39 validate (+ seed for valid 12–24-word phrases) | 53 (23 valid, 30 invalid) | `@scure/bip39` 2.2.0 `validateMnemonic` |
| Derivation-path parse | 123 (50 valid, 73 invalid) | Kohaku `parseDerivationPath` |
| `deriveAt` | 228 (200 keys, 28 expected errors), 4 mnemonics × 57 paths | Kohaku `MnemonicKeystore.deriveAt` (`@scure/bip32` 2.2.0), incl. PP paths `m/28784'/1'/0'…` and RAILGUN `m/44'/1984'/…`, `m/420'/1984'/…` |
| Scoped keystore | 225 (9 scopes × 25 paths) | Kohaku `scopeKeystore` (ok / not allowed / invalid path / invalid scope) |
| Per-app EOA | 180 (3 seeds × 60 origins) | `@noble/hashes` keccak + `@scure/bip32` + Kohaku `deriveAt` + viem `privateKeyToAddress` |
| EIP-55 | 36 | viem `getAddress` |
| Seed wrap (kek + blob + unwrap) | 40 | Node 24 WebCrypto `crypto.subtle` HKDF + AES-GCM |
| Unwrap tamper / errors | 84 | Every case that reaches AES-GCM was also checked to fail in WebCrypto |
| `sign_digest` | 96 (12 keys × 8 digests) | viem 2.38.0 `sign` (noble-curves 1.9.1), cross-checked with `@noble/curves` 2.0.1 |

Kohaku: commit `8c83d64050040654082d70901a46a339bfa103b0`. The generator copies
`packages/plugins/src/host/{derivation-path,mnemonic-keystore,scoped-keystore}.ts` verbatim
into `vectors/lowki-keys/kohaku-src/` (so their bare imports resolve to the generator's
pinned `node_modules`), records their SHA-256 in `vectors.json` `meta`, and imports them.
Package versions are pinned exactly in `vectors/lowki-keys/package.json` and recorded in `meta`
(`ox` is 0.12.0).

## Regenerating vectors

```sh
cd vectors/lowki-keys
npm install
npx tsx gen.ts         # writes vectors.json (deterministic: fixed-seed SHA-256 DRBG)
cd ../../crates/lowki-keys
CARGO_TARGET_DIR=../../target/lowki-keys cargo test
```

## Known gaps / deviations

* **Non-ASCII mnemonics are always rejected.** `@scure/bip39` NFKD-normalises first, so it
  would accept e.g. full-width Latin letters that normalise to an English word. We skip
  Unicode normalisation to keep the wasm small. Every ASCII input behaves like scure.
* `mnemonic_to_seed` validates the phrase; `mnemonicToSeedSync` does not (Kohaku validates
  in the `MnemonicKeystore` constructor, so the observable keystore behaviour is the same).
* `mnemonic_to_entropy` accepts only 24-word phrases, because the wrapping scheme wraps 32 bytes.
* `sign_digest` reduces the digest mod n before RFC 6979, because noble does that. The
  `ecdsa` crate on its own feeds the unreduced digest into the nonce DRBG, which gives a
  different (still valid) signature for digests ≥ n, such as `0xff…ff`. That case is in the vectors.
* BIP-32 edge cases where IL ≥ n or the child key is zero give `InvalidDerivedKey`, as
  scure throws. We do not skip to the next index the way BIP-32 suggests. The chance of
  this is about 2^-127, and it is not covered by vectors.
* The RAILGUN branch exists only for parity with Kohaku. Lowki does not use it.
* The caller must supply a fresh random 12-byte nonce to `wrap_entropy`.
