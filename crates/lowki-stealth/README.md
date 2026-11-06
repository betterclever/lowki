# lowki-stealth

ERC-5564 stealth addresses (scheme id 1: secp256k1 with view tags), ERC-6538 registry
formats, Announcer calldata and event helpers, and Lowki's BIP-32 stealth key derivation.
`no_std` + `alloc`, `#![forbid(unsafe_code)]`, builds for `wasm32-unknown-unknown`.
Dependencies: `k256` (arithmetic only), `sha3`, `sha2`, `hmac`, `zeroize`. There is no
`getrandom` in the tree: the caller supplies the ephemeral key.

## Public API

```rust
// constants
SCHEME_ID: u64 = 1
ERC5564_ANNOUNCER: Address  // 0x55649E01B5Df198D18D95b5cc5051630cfD45564
ERC6538_REGISTRY:  Address  // 0x6538E6bf4B0eBd30A8Ea093027Ac2422ce5d6538
type Address = [u8; 20];
enum Error { InvalidMetaAddress, InvalidPublicKey, InvalidPrivateKey, InvalidHashScalar,
             PointAtInfinity, InvalidAbi, InvalidLog, InvalidMetadata, Bip32InvalidKey }

// keys
SecretKey::{from_bytes(&[u8;32]), from_slice(&[u8])} -> Result<SecretKey>
SecretKey::{to_bytes() -> Zeroizing<[u8;32]>, public_key() -> [u8;33], address() -> Address}
address_from_pubkey(&[u8]) -> Result<Address>      // 33 or 65 byte SEC1
to_checksum_address(&Address) -> String            // EIP-55
keccak256(&[u8]) -> [u8;32]

// 1. meta-address
StealthMetaAddress::new(&[u8;33] spending, &[u8;33] viewing) -> Result<Self>
StealthMetaAddress::from_raw(&[u8]) -> Result<Self>   // 66 bytes (or 33-byte single-key form)
StealthMetaAddress::to_raw() -> [u8;66]               // ERC-6538 registry value
StealthMetaAddress::parse_uri(&str) / FromStr         // "st:eth:0x<132 hex>"
StealthMetaAddress::to_uri() -> String                // lowercase hex
StealthMetaAddress::{spending_pubkey(), viewing_pubkey()} -> &[u8;33]

// 2. sender
generate_stealth_address(&StealthMetaAddress, &SecretKey ephemeral)
    -> Result<GeneratedStealthAddress { stealth_address, ephemeral_pubkey: [u8;33], view_tag: u8 }>

// 3. recipient
check_announcement(&SecretKey viewing, &[u8;33] spending_pub, &[u8] ephemeral_pub, u8 view_tag)
    -> Option<Address>     // None on view-tag mismatch (fast reject) or malformed input
compute_stealth_private_key(&SecretKey spending, &SecretKey viewing, &[u8] ephemeral_pub)
    -> Result<SecretKey>   // .address() == the stealth address

// 4. announcements / registry
encode_announce_calldata(scheme_id: u64, &Address, ephemeral_pubkey: &[u8], metadata: &[u8]) -> Vec<u8>
announcement_topic0() -> [u8;32]
decode_announcement_log(topics: &[[u8;32]], data: &[u8]) -> Result<Announcement>
Announcement { scheme_id: [u8;32], stealth_address, caller, ephemeral_pubkey: Vec<u8>, metadata: Vec<u8> }
    .is_scheme_1(), .view_tag()
build_metadata(view_tag, selector: [u8;4], contract: &Address, amount: &[u8;32]) -> [u8;57]
metadata_eth / metadata_erc20 / metadata_erc721, parse_metadata(&[u8]) -> Result<MetadataFields>
view_tag_from_metadata(&[u8]) -> Option<u8>
encode_register_keys_calldata(scheme_id, &StealthMetaAddress) -> Vec<u8>
encode_stealth_meta_address_of_calldata(&Address registrant, scheme_id) -> Vec<u8>
decode_stealth_meta_address_of_return(&[u8]) -> Result<Option<StealthMetaAddress>>

// 5. Lowki derivation
derive_stealth_keys(&[u8;64] seed) -> Result<StealthKeys { spending, viewing }>
StealthKeys::meta_address() -> StealthMetaAddress
bip32::{master_key, ExtendedPrivKey::{derive_hardened, derive_path}, SPENDING_PATH, VIEWING_PATH, HARDENED}
```

## Hashing conventions (scheme 1)

These were confirmed by reading the reference SDK
(`dist/utils/crypto/{generateStealthAddress,checkStealthAddress,computeStealthKey}.js`
in `@scopelift/stealth-address-sdk@1.0.0-beta.5` and `@noble/secp256k1@2.3.0`), and they
match it byte for byte:

1. Shared point `S = eph_priv · P_view` (equivalently `view_priv · P_eph`). noble's
   `getSharedSecret(priv, pub)` defaults to `isCompressed = true`.
2. `s_h = keccak256(compressed(S))` over the **33-byte compressed SEC1 point**
   (`0x02/0x03 || x`), not the bare x-coordinate and not the 65-byte form. The vectors
   carry `shared_secret_compressed` and `hashed_shared_secret` to pin this down.
3. View tag = `s_h[0]`, the most significant byte.
4. `s_h` is used as a big-endian scalar **without reduction mod n**. The SDK calls
   `ProjectivePoint.fromPrivateKey(s_h)`, which throws when `s_h ∉ [1, n)`. This crate
   returns `Error::InvalidHashScalar` (or `None` from `check_announcement`) in that case,
   which happens with probability about 2⁻¹²⁸.
5. `P_stealth = P_spend + s_h·G`. The address is the last 20 bytes of
   `keccak256(uncompressed(P_stealth)[1..])`.
6. `p_stealth = (p_spend + s_h) mod n` (the SDK's `addPriv`). A zero result or an identity
   point gives `Error::PointAtInfinity`; the SDK has no explicit check for this.

## Lowki key derivation (Lowki-specific)

From the 64-byte wallet seed, using standard BIP-32 (`HMAC-SHA512("Bitcoin seed", seed)`,
hardened CKDpriv):

- spending key: `m/5564'/0'/0'`
- viewing key: `m/5564'/0'/1'`

The BIP-32 code is a minimal local implementation (hardened, private derivation only). It
is checked against `@scure/bip32@2.0.1` and against BIP-32 test vector 1 (master key and `m/0'`).

## Contract addresses

| Contract | Address | Source |
|---|---|---|
| ERC-5564 Announcer | `0x55649E01B5Df198D18D95b5cc5051630cfD45564` | EIP-5564 "Deployment Method" (CREATE2, same on all chains); SDK `dist/config/contractAddresses.js` |
| ERC-6538 Registry | `0x6538E6bf4B0eBd30A8Ea093027Ac2422ce5d6538` | EIP-6538 "Deployment Method" (CREATE2); SDK `dist/config/contractAddresses.js` |

## Verification

Differential vectors are in `vectors/lowki-stealth/vectors.json`, produced by `gen.ts`
from a fixed-seed sha256 counter RNG (seed `lowki-stealth-vectors-v1`). Regeneration is
deterministic. Reference versions:

- `@scopelift/stealth-address-sdk` **1.0.0-beta.5** (latest on npm), using `@noble/secp256k1` **2.3.0**
- `viem` **2.57.16** (ABI encoding and decoding, EIP-55)
- `@scure/bip32` **2.0.1**

| Area | Cases | Checked against |
|---|---|---|
| Meta-address encode/parse round-trips | 64 (+8 single-key form) | SDK `generateStealthMetaAddressFromKeys`, `parseKeysFromStealthMetaAddress` |
| generate → check → compute key → address | 167 (7 edge cases: keys 1, n-1, n-2, 2²⁵⁵, spend == view; 160 random) | SDK `generateStealthAddress`, `checkStealthAddress`, `computeStealthKey`; viem `privateKeyToAddress` |
| Wrong view tag rejected | 167 (plus an exhaustive 255-tag sweep on 12 cases) | SDK `checkStealthAddress` returns false |
| Wrong viewing key rejected | 167 | SDK `checkStealthAddress` returns false |
| Metadata layouts (view-tag-only, ETH, ERC-20, ERC-721, custom, arbitrary) | 96 | SDK `buildMetadataFor*`, `getViewTagFromMetadata` |
| `announce` calldata | 96 | viem `encodeFunctionData` with the SDK's Announcer ABI |
| `Announcement` log decoding | 96 (incl. random uint256 scheme ids) | viem `encodeEventTopics` / `encodeAbiParameters` / `decodeEventLog` |
| Registry `registerKeys` / `stealthMetaAddressOf` calldata and return decoding | 32 | viem with the SDK's Registry ABI |
| BIP-32 `m/5564'/0'/{0,1}'` | 64 seeds (zero, all-0xff, 62 random) | `@scure/bip32` `HDKey.fromMasterSeed().derive()` |

Rust-only tests: malformed URIs, off-curve keys, bad lengths, out-of-range private keys,
truncated or malformed ABI data, wrong topic0, dirty address topics, an engineered
point-at-infinity stealth key, uncompressed ephemeral keys, the redacted `Debug` output,
and the official BIP-32 vector.

```sh
# regenerate vectors (Node 24)
cd vectors/lowki-stealth && npm install && npx tsx gen.ts

# test / lint / wasm
cd crates/lowki-stealth
export CARGO_TARGET_DIR=/Users/betterclever/projects/ethglobal/lowki/target/lowki-stealth
cargo test && cargo clippy --all-targets -- -D warnings
cargo build --release --target wasm32-unknown-unknown
```

## Known gaps

- Only the `st:eth:` chain tag is accepted by `parse_uri`. The SDK accepts any `st:<chain>:`
  and bare `0x…` hex. Use `from_raw` for raw bytes.
- `check_announcement` returns the derived address. The caller must still compare it with
  the announced `stealthAddress`, because a matching view tag is only a 1/256 filter.
- `s_h ≥ n` cannot be tested differentially: it needs a keccak preimage, and the reference
  throws in that case anyway.
- Zeroization: `SecretKey` wraps `k256::SecretKey` (zeroize-on-drop), and intermediate
  hashes and HMAC outputs are `Zeroizing`. `k256::Scalar`/`ProjectivePoint` values are
  `Copy`, so stack temporaries from scalar/point arithmetic are not wiped.
- k256 scalar multiplication is constant-time. The view-tag comparison is an early return
  by design (the tag is public).
- BIP-32: hardened private derivation only, with no xpub/xprv serialization. If a derived
  key is invalid (probability about 2⁻¹²⁷), the function returns `Bip32InvalidKey` instead
  of skipping to the next index, which matches scure, which throws.
- The ERC-6538 `registerKeysOnBehalf` (EIP-712 signature) path is not implemented.
- The announce encoder takes a `u64` scheme id. The log decoder returns the full uint256.
