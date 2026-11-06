# Passkey-PRF spike

**Result: PASS** in headless Chrome 156.0.8078.80 (macOS arm64 host) with a CDP virtual CTAP2.1 authenticator.

## What it proves
A wasm build of `lowki-keys` (`derive_kek` / `wrap_entropy` / `unwrap_entropy`, unmodified, linked by path) can wrap and unwrap seed entropy with a real WebAuthn PRF output in the browser:

| Check | Meaning |
|---|---|
| `a_prf_enabled` | `create()` with `extensions: {prf: {}}`, `residentKey: "required"` and `userVerification: "required"` returns `getClientExtensionResults().prf.enabled === true` |
| `b_prf_32_bytes` | `prf.results.first` is 32 bytes |
| `b_same_salt_same_output` | two assertions with the same `eval.first` salt give the same output |
| `b_diff_salt_diff_output` | a different salt gives a different output |
| `b_uv_flag_set` | the UV bit is set in authenticatorData |
| `c_blob_61_bytes` | the wasm `wrap_entropy` gives `0x01 \|\| nonce \|\| ct+tag` (61 bytes). Entropy and nonce come from `crypto.getRandomValues` |
| `c_discoverable_same_credential` | a fresh **discoverable** assertion (no `allowCredentials`, which is what a new device does) picks the same credential |
| `c_fresh_assertion_same_prf` / `c_roundtrip` | that fresh assertion's PRF unwraps the blob back to the original entropy |
| `c_wrong_salt_fails` | the PRF from another salt fails with `Decrypt: wrapped seed failed authentication` |
| `x_webcrypto_*` | an independent WebCrypto HKDF + AES-GCM gives the same KEK and decrypts the wasm blob |

What it does **not** prove: anything about real authenticators (iCloud Keychain, GPM, Windows Hello, YubiKeys, hybrid). See [PLATFORMS.md](PLATFORMS.md).

## Layout
- `wasm/`: a wasm-bindgen crate (`lowki-prf-spike`, edition 2024, empty `[workspace]`, `wasm-bindgen = "=0.2.130"`) that wraps `lowki-keys`. It takes and returns `Uint8Array`/`Vec<u8>` and throws `JsError` (`"Decrypt: …"`, `"prf must be 32 bytes…"`).
- `web/index.html`: the test page. `window.runSpike()` runs everything and writes the JSON result to `window.__PRF_RESULT` and `<pre id="result">`. Query parameters: `?auto` (run on load), `?rpId=` (omit `rp.id`) / `?rpId=x`, `?createPrf=0`, `?evalAtCreate=1`. `web/pkg/` is generated.
- `serve.mjs`: a static server for `web/` (`node serve.mjs 8787` lets you try it by hand with a real passkey at `http://localhost:8787/?auto`).
- `drive.mjs`: starts Chrome and speaks **raw CDP over Node's built-in `WebSocket`**, so there are **no npm dependencies**. `package.json` exists only for scripts and `engines`; `npm install` is a no-op. Needs Node ≥ 22.4; tested with Node 26.13.0.
- `run.sh`: the one-shot script for the host. `matrix.sh` runs the option experiments.

## How to run (macOS host)
```sh
cd spikes/prf && ./run.sh            # from the Linux VM: mac sh -lc 'cd /Users/betterclever/projects/ethglobal/lowki/spikes/prf && ./run.sh'
./matrix.sh                          # option experiments (after run.sh has built web/pkg)
```
`run.sh` does the following:
- `cargo build --release --target wasm32-unknown-unknown` and `cargo clippy … -- -D warnings`, with `CARGO_TARGET_DIR=target/prf-spike-mac`.
- `target/tools/bin/wasm-bindgen --target web --no-typescript --out-dir web/pkg` (0.2.130).
- `node drive.mjs`, which prints `PASS`/`FAIL` and then the JSON. The exit code is 0 on PASS.

The wasm output is about 81 KB (`opt-level="s"`, LTO, no wasm-opt). Clippy with `-D warnings` is also clean in the VM (`CARGO_TARGET_DIR=target/prf-spike`).

### Output (abridged)
```
PASS
{ "ok": true,
  "checks": { "a_prf_enabled": true, "b_prf_32_bytes": true, "b_same_salt_same_output": true,
    "b_diff_salt_diff_output": true, "b_uv_flag_set": true, "c_blob_61_bytes": true,
    "c_discoverable_same_credential": true, "c_fresh_assertion_same_prf": true, "c_roundtrip": true,
    "c_wrong_salt_fails": true, "x_webcrypto_kek_matches": true, "x_webcrypto_unwraps_wasm_blob": true },
  "info": { "isSecureContext": true, "origin": "http://localhost:55120", "rpId": "localhost",
    "clientCapabilities": { "conditionalCreate": true, "conditionalGet": true, "extension:prf": true,
      "hybridTransport": true, "passkeyPlatformAuthenticator": true, "userVerifyingPlatformAuthenticator": true },
    "uvpaa": true, "createExtensions": { "prf": { "enabled": true }, "credProps": { "rk": true } },
    "wrongSaltError": "Decrypt: wrapped seed failed authentication",
    "virtualAuthenticator": { "credentials": [ { "isResidentCredential": true, "rpId": "localhost", "signCount": 5 } ] },
    "browser": "Chrome/156.0.8078.80",
    "launch": ["--headless=new","--remote-debugging-port=0","--no-first-run","--no-default-browser-check","about:blank"] } }
```

## Exact Chrome APIs, CDP commands and flags used
**Launch:** `"/Applications/Google Chrome.app/Contents/MacOS/Google Chrome" --headless=new --remote-debugging-port=0 --user-data-dir=<tmp> --no-first-run --no-default-browser-check about:blank`. The driver reads `DevTools listening on ws://…` from stderr. No WebAuthn-specific flags or `--enable-features` are needed.

**CDP, in order** (on the browser websocket, with flat sessions):
1. `Browser.getVersion`
2. `Target.createTarget {url:"about:blank"}` → `Target.attachToTarget {targetId, flatten:true}`. All following calls carry `sessionId`, because the WebAuthn domain is per-target.
3. `WebAuthn.enable {enableUI:false}`. This must come before `addVirtualAuthenticator`.
4. `WebAuthn.addVirtualAuthenticator {options:{protocol:"ctap2", ctap2Version:"ctap2_1", transport:"internal", hasResidentKey:true, hasUserVerification:true, isUserVerified:true, hasPrf:true, automaticPresenceSimulation:true}}`
5. `Page.enable`, `Page.navigate {url:"http://localhost:<port>/"}`, then wait for `Page.loadEventFired`
6. `Runtime.evaluate {expression:"window.runSpike()", awaitPromise:true, returnByValue:true, userGesture:true}`
7. `WebAuthn.getCredentials {authenticatorId}`. This is only for reporting (resident = true, signCount 5).

**Page APIs:** `navigator.credentials.create` (rp `{name, id:"localhost"}`, user id 16 random bytes, `pubKeyCredParams` −7/−257, `authenticatorSelection {residentKey:"required", requireResidentKey:true, userVerification:"required"}`, `attestation:"none"`, `extensions {prf:{}, credProps:true}`), `navigator.credentials.get` (`userVerification:"required"`, `extensions {prf:{eval:{first: <32-byte salt>}}}`, with and without `allowCredentials`), `getClientExtensionResults()`, `PublicKeyCredential.getClientCapabilities()`, `crypto.getRandomValues`, `crypto.subtle` (cross-check only).

### Option matrix (`./matrix.sh`, Chrome 156, 2026-11-06)
| Variant | Result | Takeaway |
|---|---|---|
| baseline (above) | PASS | |
| `ctap2Version:"ctap2_0"` | PASS | **`hasPrf` does not need ctap2_1.** The virtual hmac-secret works on CTAP 2.0 |
| `transport:"usb"` / `"hybrid"` | PASS | transport doesn't matter for the virtual authenticator |
| `hasPrf:false` | FAIL: `prf.enabled:false`, no `results` on get | `hasPrf:true` is required |
| page `?createPrf=0` (no `prf` in `create()`) | FAIL: get returns no `prf.results` (internal, and usb+ctap2_0) | **you must request `prf` at create time** |
| page `?evalAtCreate=1` (ctap2_1 and ctap2_0) | PASS, `create()` also returns a 32-byte `prf.results.first` | the virtual authenticator can evaluate at create time |
| `hasResidentKey:false` | NotAllowedError | needed for `residentKey:"required"` |
| `hasUserVerification:false` or `isUserVerified:false` | NotAllowedError | needed for `userVerification:"required"` |
| `automaticPresenceSimulation:false` | NotAllowedError (after the 30 s timeout) | needed unless you drive presence yourself |
| `protocol:"u2f"` (ctap2Version removed) | NotAllowedError | U2F has no RK/UV/hmac-secret |
| no virtual authenticator | hangs past the 60 s driver timeout (the page's 30 s WebAuthn timeout didn't fire) | headless Chrome on macOS doesn't fail fast without one |
| `Runtime.evaluate` without `userGesture` | PASS | **no user activation is needed** for create/get here |
| `--headless` (instead of `--headless=new`) | PASS | in Chrome 156 `--headless` *is* the new headless. `=new` isn't needed |
| page at `http://127.0.0.1:<port>`, rp.id default or omitted | SecurityError "This is an invalid domain." | **IP hosts can't do WebAuthn. Use `localhost`** (a secure context over plain http) |
| `localhost`, `rp.id` omitted | PASS | rp.id defaults to `localhost` |
| `localhost`, `rp.id:"example.com"` | SecurityError (it also tried `.well-known/webauthn` Related Origins) | |

Non-obvious points: `localhost` (not `127.0.0.1`) is required. WebAuthn CDP calls must go on the page session (flatten), not on the browser target. `hasPrf` alone is enough, with no `extensions` option and no ctap2_1. PRF must be requested at `create()`.

## Problems noticed in lowki-keys / docs (not fixed)
1. **Entropy size disagrees with ARCHITECTURE.md.** ARCHITECTURE.md ("Keys") says the seed is **16 bytes** of entropy (12 words), and Shamir uses 17-byte shares for 16-byte entropy. `lowki-keys` only wraps **32 bytes** (`wrap_entropy(&[u8;32])`, `WRAPPED_LEN = 61`), and `mnemonic_to_entropy` accepts only 24-word phrases.
2. **"P: wrapped by the passkey" is a Shamir share, but the wrap API takes only 32-byte entropy.** A 17-byte share (or 33 bytes for 32-byte entropy) can't be wrapped with `wrap_entropy` as it stands. The wrap format needs to be length-generic, or the docs need to say what is wrapped.
3. **The PRF salt (`eval.first`) is not specified anywhere.** Every device must use the same salt to unwrap. The spike uses a random salt per run; Lowki needs a fixed, versioned constant or a stored salt. See PLATFORMS.md §Implications.
4. The AAD (`"lowki/seed/v1"`) doesn't bind the credential ID or user. That's harmless (the KEK is per-credential already), but it's worth noting if blobs end up stored server-side by user.
5. `derive_kek` returns the KEK to the caller. That's fine in Rust, but a wasm binding shouldn't export it in production (the spike exports it only to cross-check against WebCrypto).
