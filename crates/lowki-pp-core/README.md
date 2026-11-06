# lowki-pp-core

Privacy Pools v1 note math for Lowki, bit-exact with Kohaku's Privacy Pools plugin
(`vendor/kohaku` @ `8c83d64050040654082d70901a46a339bfa103b0`). Builds for native and
`wasm32-unknown-unknown`. It has no dependency on other Lowki crates.

## Public API

Field elements are `Fr` (`ark_bn254::Fr`, re-exported), which are always canonical
(< p). Raw uint256 words are big-endian `[u8; 32]`, and addresses are `[u8; 20]`.

| Area | Items |
|---|---|
| field | `SNARK_SCALAR_FIELD`, `fr_from_be_bytes` (strict, errors if >= p), `fr_from_be_bytes_mod_order`, `fr_to_be_bytes` |
| poseidon | `poseidon(&[Fr]) -> Result<Fr>` (1..=3 inputs), `poseidon1/2/3` |
| limt | `LeanImt { new, from_leaves, insert, insert_many, root, depth, size, leaves, index_of, has, generate_proof }`, `MerkleProof { root, leaf, index, siblings }`, `verify_proof`, `proof_from_tree` (pads to `PADDED_PROOF_DEPTH = 32`) |
| secrets | `SignatureSecretManager::new(&[u8;32])` with `sign_note`, `note_secrets`, `deposit_secrets`, `withdrawal_secrets`, `derive_ephemeral_signer`; `NoteMessage`; `Secret { nullifier, salt, precommitment, nullifier_hash }`; the low-level `note_typed_data_hash`, `canonical_rs`, `field_from_rs`, `secrets_from_signature`, `ephemeral_signer_from_signature` (these are for hardware or external `EnvelopeSigner`s) |
| secrets::legacy (deprecated) | `legacy_secrets(chain_id, entrypoint, nullifier_key, salt_key)`, `legacy_ephemeral_signer(chain_id, entrypoint, signer_key)` |
| note | `commitment(value, label, precommitment)`, `calculate_context(processooor, data, scope)`, `compute_scope(pool, chain_id, asset)`, `compute_label(scope, nonce)` |

How secrets are handled: `Secret` and `SignatureSecretManager` zeroize on drop, and their
`Debug` output redacts the nullifier, the salt and the key. Signatures and derived keys are
returned as `Zeroizing<[u8; N]>`.

### Semantics worth knowing

- **Poseidon** is circomlib Poseidon (light-poseidon `new_circom`). The references
  (maci-crypto `Fr.e`, Kohaku T3 `x % p`) **accept inputs >= p and reduce them mod p**,
  including inputs up to 2^256-1. In Rust you get the same result by converting with
  `fr_from_be_bytes_mod_order`. Use `fr_from_be_bytes` to reject such inputs instead.
- **LeanIMT**: `MerkleProof::index` is zk-kit's *compressed* path index. Bit i is set when
  `siblings[i]` is on the left, and levels without a sibling are skipped. `index_of`
  returns the first occurrence. `proof_from_tree` appends zero siblings up to 32, as
  Kohaku does. Like the reference, `verify_proof` does not understand that padding, so
  verify unpadded proofs. Zero leaves are hashed like any other value (Kohaku
  semantics). zk-kit's JS tree diverges on zero leaves, but the on-chain tree rejects
  them anyway.
- **SignatureSecretManager**: `σ = RFC6979-sign(EIP712 PrivacyPoolNote{depositIndex, secretIndex})`.
  The domain is `{name "PrivacyPools Keychain", version "1", chainId, verifyingContract = entrypoint, salt = keccak256("privacypools")}`.
  Then `rs = r ‖ lowS(s)`, `nullifier/salt/signer = keccak256(tag ‖ rs)[0..31]`,
  `precommitment = poseidon([nullifier, salt])`, and `nullifier_hash = poseidon([nullifier])`.
  The key is the account identity at `m/28784'/1'/{accountIndex}'` (derive it elsewhere).
  `sign_note` reproduces viem's 65-byte signature exactly (`v ∈ {27,28}`).
  k256 RFC 6979 is deterministic, so Kohaku's runtime "sign twice" check is not needed.
- `canonical_rs` accepts what Kohaku accepts (viem 2.54.1 `parseSignature`): 65 bytes,
  `v ∈ {0,1,27,28}`, and `1 <= r,s < n`. A high `s` is folded to low-s. **Kohaku's doc
  comment claims ERC-2098 64-byte support, but viem rejects it**, so this crate rejects it
  too (there is a vector for this).
- **Label**: Kohaku only reads `_label` from the `Deposited` event. On-chain
  (privacy-pools-core `PrivacyPool.deposit`), `label = keccak256(abi.encodePacked(SCOPE, ++nonce)) % SNARK_SCALAR_FIELD`
  and `SCOPE = keccak256(abi.encodePacked(pool, chainid, asset)) % SNARK_SCALAR_FIELD`
  (`State` constructor). `nonce` is the pool's global deposit counter after the increment,
  so the first deposit uses 1. Because it is on-chain state, `compute_label` is for
  prediction and verification. Wallets should still take the label from the event.
- `calculate_context` takes any uint256 scope. Kohaku returns unpadded hex
  (`numberToHex`), and this crate returns the `Fr`.

## Verification

`tests/vectors.rs` loads `vectors/lowki-pp-core/vectors.json` with `include_str!` and
asserts equality byte for byte. The generator imports the **Kohaku source files of the
pinned clone directly** (through `resolve-hook.mjs`). The npm dependencies are pinned to
the versions in Kohaku's `pnpm-lock.yaml`: maci-crypto 2.5.0 (@zk-kit/poseidon-cipher
0.3.2), viem 2.54.1, ox 0.12.4, @scure/bip32 2.2.0, @scure/bip39 2.2.0,
@noble/hashes 2.2.0, and @zk-kit/lean-imt 2.2.4 (used only for a cross-check).

| Area | Cases | Reference |
|---|---|---|
| Poseidon | 324 (108 each for arity 1, 2 and 3: 0, 1, 2, p-1, inputs >= p, 2^256-1, and random) | maci-crypto `poseidon`. For arity 2, Kohaku `poseidonT3` is also asserted equal in the generator. |
| LeanIMT | 40 trees (sizes 1–2049, including 2047/2048/2049, powers of two ±1, duplicate leaves, all-equal leaves, zero and p-1 leaves). Built with insert, insert_many, or random insert_many chunks, with 216 intermediate roots. Also 230 proofs, 106 padded `proofFromTree` proofs, 146 `indexOf`, and 198 `verifyProof` cases (true and tampered). | Kohaku `PoseidonT3LeanIMT` and `proofFromTree`. Roots are also checked against @zk-kit/lean-imt when there are no zero leaves. |
| SignatureSecretManager | 418 cases: accounts 0, 1 and 2; chainIds 1, 11155111, 10, 31337 and 137; mainnet, Sepolia, random, 0x…01 and 0xff…ff entrypoints; deposit indices 0..2^53-1; secret indices 0..255. Each case checks the typed-data hash, the signature, the secrets and the ephemeral signer. | Kohaku `SignatureSecretManager` with a `MnemonicKeystore` host (the "test test … junk" mnemonic). The account keys come from the same keystore at `m/28784'/1'/i'`. The EIP-712 envelope is captured through a wrapping `EnvelopeSigner`. |
| Signature framing | 72 (24 accepted: high-s, and v as 0/1; 48 rejected: ERC-2098, v=29, s=0, s=n) | Kohaku manager with an injected signer |
| Legacy SecretManager | 120 (the HD keys are recorded in the vectors) | Kohaku `SecretManager` (deprecated) |
| commitment | 84 | Kohaku `utils.ts` `commitment` |
| calculate_context | 80 (data length 0–500; scope 0, 1, p-1, p, 2^256-1, and random) | Kohaku `calculateContext` |
| scope and label | 40 | viem `encodePacked` + keccak, which mirrors the Solidity |
| on-chain | 4 real mainnet deposits (0xbow ETH pool): scope from `SCOPE()`, label nonce, and commitment | Ethereum mainnet, captured with `capture-onchain.ts` |

## Regenerating vectors

```sh
cd vectors/lowki-pp-core
npm install
npm run gen                      # writes vectors.json (deterministic: keccak-counter PRNG)
npx tsx capture-onchain.ts [rpc] # optional: refresh onchain.json from mainnet
```

## Build and test

```sh
export CARGO_TARGET_DIR=/Users/betterclever/projects/ethglobal/lowki/target/lowki-pp-core
cargo test
cargo clippy --all-targets -- -D warnings
cargo build --target wasm32-unknown-unknown
```

The dependency tree contains no `getrandom`: k256 is used without `rand_core/getrandom`,
and ark-std's `rand` is used without OS randomness.

## Known gaps

- Poseidon is exposed and verified only for 1–3 inputs, which are the arities Privacy Pools uses.
- `NoteMessage` takes `u64` indices (EIP-712 `uint64`). Kohaku passes JS numbers, so
  vectors cover indices only up to 2^53-1.
- `chain_id` is a `u64`, which covers every real chain id. Kohaku uses `Number(chainId)`.
- No HD derivation here. The legacy manager and `SignatureSecretManager` take keys that
  were derived elsewhere (the keystore crate).
- `deriveDelegatorSigner` is plain keystore derivation and is out of scope.
- The on-chain label check covers one pool and 4 deposits captured at block 26349232.
