# WebAuthn PRF support on real platforms

Researched 2026-11-06. PRF (`extensions.prf`) is the WebAuthn wrapper around CTAP2 `hmac-secret`.
Every claim below has a source. How strong the source is matters, so each one is labelled:

- **[primary]**: the vendor or browser itself (a bug tracker, a commit, an intent thread, or vendor docs).
- **[secondary]**: third-party testing or write-ups. Mostly Corbado, whose PRF article was last modified 2026-09-29 and whose tables are dated August 2026.
- **UNVERIFIED**: no source found that I could read, or only a search-result snippet.

Support changes quickly, which every source points out. Lowki should detect support at runtime, as described at the end, and not hard-code this table.

## Summary table

| Authenticator / browser | PRF? | Since | Source |
|---|---|---|---|
| Chrome (any OS), security key with hmac-secret | yes | Chrome 116 | [primary] blink-dev I2S thread, reply "This will hit stable in M116" [1] |
| iCloud Keychain, Safari 18+ on macOS 15+ / iOS 18+ | yes | Safari 18 / macOS 15 / iOS 18 | [primary] Chromium commit message "As of macOS 15 and iOS 18, iCloud Keychain supports the PRF extension" [2] (seen as a search snippet only); [secondary] Yubico table, "macOS 15+, Safari 18+: Supported (iCloud Keychain)" [3]; Corbado [4]. Safari 18 release notes [5]: UNVERIFIED (the page body would not load) |
| iCloud Keychain, Chrome on macOS 15+ | yes | Chrome 132 | [secondary] Corbado [4]. Chrome's own iCloud Keychain post (Chrome 118, macOS 13.5) says nothing about PRF [6]. Chrome 132 as the exact version is UNVERIFIED by a primary source |
| iCloud Keychain, Firefox on macOS 15+ | yes | Firefox 139 | [primary] Bugzilla 1935280: fixed in 139, "Requires MacOS 15.0+" [7] |
| iOS/iPadOS 18.0–18.3 | buggy | fixed in 18.4 | [secondary] Corbado: a cross-device data-loss bug when the iPhone is the CDA source [4] |
| Safari, cross-device (hybrid / QR) | buggy | — | [primary] Apple Developer Forums 774112: PRF comes back false/empty over hybrid. An Apple engineer said it was "partially fixed in 18.2" but it **returns a different value than on-device**. The bug was still reported on 18.3.1 and 18.4 beta [8] |
| Safari + external security key | no before Safari 26.4; yes from 26.4 with bugs | Safari 26.4 (Mar 2026) | [secondary] Corbado: WebKit 311099 (hmac-secret output still CBC-encrypted on USB/NFC keys) and 314934 (null result for `intervalUV` keys such as YubiKey Bio) [4]. [secondary] Yubico mid-2025: "Safari: not supported" for roaming keys on macOS, and iOS "does not pass extension data to/from external keys" [3]. The WebKit bug numbers are UNVERIFIED against bugs.webkit.org |
| Google Password Manager, Chrome on Android | yes | UNVERIFIED (version) | [secondary] Yubico table, "Android, Chrome: Supported (Google Password Manager)" [3]; Corbado: "PRF by default", also in Edge and Samsung Internet [4]. I2S thread: "Some passkey providers on Android 14 may not support it" [1] |
| Google Password Manager, Chrome desktop | yes | UNVERIFIED (version) | [secondary] Corbado tested macOS 26.3 + Chrome 151: PRF on create and get [4]. GPM can return PRF on `create()` [4] |
| Chrome "profile" authenticator (local, non-GPM) on desktop | **no** | — | [secondary] Corbado [4]; mera.category.xyz [9] |
| Windows Hello (Windows 11) before Feb 2026 | **no** | — | [secondary] Yubico: "Windows Hello lacks hmac-secret" [3]; Corbado [4] |
| Windows Hello, Windows 11 24H2 build 26100.7840+ / 25H2 26200.7840+ (KB5077181, 2026-02-10) | yes | Feb 2026 CU | [secondary] Corbado: "Microsoft has not documented the change; the build number comes from community tests" [4][10]; Bitwarden community thread [11]. **No Microsoft documentation found: UNVERIFIED by a primary source** |
| Windows Hello, Chrome/Edge | get: yes on the builds above. create-time PRF: Chrome/Edge 147+ | 147 | [secondary] Corbado: "Chrome/Edge 146 and below surface PRF only on authentication"; Chrome 147 enabled PRF-on-create by default (WEBAUTHN_API_VERSION_8) [4]. Bitwarden users report mixed results with Chrome 147 [11] |
| Windows Hello, Firefox | yes | create backported to 147; full from 148 | [secondary] Corbado [4] |
| Microsoft Password Manager (Edge 151) | create OK, **every get fails with NotAllowedError** | — | [secondary] Corbado [4] |
| Windows 10 | no | — | [secondary] Corbado (no support in its tables) [4] |
| Security keys on Windows 11 (Chrome/Edge 116+, Firefox) | yes, if the key has hmac-secret | Chrome 116; Firefox 135 | [secondary] Corbado [4]; [primary] Bugzilla 1935280 comment: Windows (1935278) and Linux (1935277) support landed in Firefox 135 [7] |
| YubiKey 5 series | hmac-secret from firmware 5.2 (PIN/no-PIN from 5.2.3); `hmac-secret-mc` (PRF at create) from 5.8 | — | [primary] Yubico: 5.2 enhancements [12], 5.2.3 blog [13], YubiKey SDK hmac-secret page [14]. Yubico: "any YubiKey 5 Series or YubiKey Bio Series" [3] |
| Security key via Android Chrome | USB yes, NFC no (mid-2025) | — | [secondary] Yubico table [3]. Current status UNVERIFIED |
| Security key via iOS Safari | no before 26.4, then the WebKit bugs above | — | [secondary] [3][4] |
| Hybrid (phone as authenticator) from Chrome | yes | — | [primary] Apple forum 774112: "Chrome handles both flows correctly" [8]; [secondary] Corbado lists CDA as working on macOS and Android [4] |
| Samsung Pass | no PRF at create; **yes at get** | — | [secondary] Corbado [4] |
| 1Password | yes (create + get) | — | [secondary] Corbado L3 table [10] |
| Bitwarden (as passkey provider) | partial / platform-dependent | — | [secondary] Corbado L3 table [10] |
| Firefox, Android | yes | Firefox 149 | [primary] Bugzilla 1958716, fixed in the 149 branch [15] |
| Firefox, iOS | same as Safari (WebKit) | — | [secondary] Corbado [4] |

## Firefox summary
- Windows and Linux (security keys, and Windows Hello once the OS supports it): Firefox 135 [7].
- macOS: Firefox 139, needs macOS 15+ [7].
- Android: Firefox 149 [15].
- iOS: WebKit, so Safari's behaviour [4].
- Windows Hello PRF at create: 147 (backport), full support in 148 [4] (secondary).

## Things the spike's virtual authenticator showed that also matter on real platforms
The spike itself checked these (see README.md):
- With Chrome's virtual authenticator, **if `create()` doesn't include `extensions.prf`, later `get()` calls return no PRF result**, even when the authenticator supports it, for both `internal` and `usb`/`ctap2_0`. Yubico says the same about real keys: hmac-secret "is enabled on a create credential request basis" [12]. Windows Hello reportedly returns PRF on get regardless [11], but don't rely on that.
- `prf.enabled` from `create()` is **not** a reliable "no PRF" signal. Samsung Pass, and Chrome/Edge before 147 on Windows Hello, report no PRF at create but return it at get [4][11].

## Implications for Lowki

1. **PRF is not universal, so the passkey-only path needs a fallback.** No PRF from: Windows 10, Windows 11 before the Feb 2026 CU, the Chrome profile authenticator, Microsoft Password Manager get, iOS Safari with a security key (pre-26.4), and older Android providers. This matches ARCHITECTURE.md's Google + recovery-code path (shares S + R). When PRF isn't available, the "P" share needs either no passkey wrap (only S+R) or a non-PRF passkey path. Onboarding must be designed so a user with no PRF still gets a working wallet with S + R.
2. **Decide PRF support with a real ceremony, not with `create().prf.enabled`.** Suggested flow: `create({extensions:{prf:{eval:{first:salt}}}})`. If `results.first` comes back, use it. Otherwise immediately `get({allowCredentials:[id], extensions:{prf:{eval:{first:salt}}}})`. If that has `results.first`, PRF works; otherwise use the fallback. That costs two taps on some platforms. Use `PublicKeyCredential.getClientCapabilities()["extension:prf"]` only as a hint: it describes the browser, not the authenticator the user will pick.
3. **Always request `prf` at `create()`.** Security keys (and the virtual authenticator) won't evaluate PRF on credentials created without it.
4. **A wrong PRF must never overwrite the blob.** Safari over hybrid has been reported to return a *different* PRF value than on-device for the same passkey [8]. `unwrap_entropy` then returns `Decrypt`. Lowki must treat `Decrypt` as "this authenticator/path can't unlock". It should offer the S+R path and must never re-wrap or replace the stored blob based on that output.
5. **Passkey deletion means the P share is lost.** The WebAuthn L3 co-editor (Tim Cappalli) warns against PRF as the only path to encrypted data [16][17]. He explicitly considers designs with separate recovery (recovery codes) acceptable. Lowki's 2-of-3 with R covers this, but the recovery code must be mandatory and not skippable. Following his asks, Lowki should also publish a page explaining that the Lowki passkey unlocks the wallet.
6. **Salt choice.** `eval.first` must be the same on every device for synced passkeys to unwrap. Use a fixed, versioned constant (e.g. `SHA-256("lowki/prf-salt/v1")`), or store the salt next to the blob. The PRF is already per-credential and per-RP, so a constant salt doesn't link users. ARCHITECTURE.md does not pin the salt yet.
7. **Windows Hello passkeys don't sync.** Even with PRF, a Windows Hello credential is device-bound, so "same wallet on every device" relies on GPM/iCloud/1Password passkeys or the recovery path.
8. **Test matrix for the real-device phase (UNVERIFIED until run):** Safari 18/26 + iCloud (macOS, iOS), Chrome + iCloud on macOS, Chrome + GPM (Android, desktop), Chrome/Edge/Firefox + Windows Hello on 26100/26200.7840+, YubiKey 5 (fw ≥ 5.2) over USB and NFC, and hybrid from a Mac to an iPhone and to an Android phone.

## Sources
1. Intent to Ship: WebAuthn PRF extension (blink-dev) — https://groups.google.com/a/chromium.org/g/blink-dev/c/iTNOgLwD2bI ; chromestatus entry https://chromestatus.com/feature/5138422207348736 (linked from the thread; not fetched)
2. Chromium commit "device/fido: support iCloud Keychain PRF" — https://chromium.googlesource.com/chromium/src/+/7fe5155fed538b1d2317c516ddc633ee7fa206ef%5E%21 (search snippet only)
3. Yubico, A Developer's Guide to Deriving Keys with WebAuthn PRF — https://developers.yubico.com/WebAuthn/Concepts/PRF_Extension/Developers_Guide_to_PRF.html
4. Corbado, Passkeys & WebAuthn PRF for End-to-End Encryption (modified 2026-09-29) — https://www.corbado.com/blog/passkeys-prf-webauthn
5. Apple, Safari 18 Release Notes — https://developer.apple.com/documentation/safari-release-notes/safari-18-release-notes (could not read the body); secondary summary: https://browserdigest.pages.dev/blog/webkit-safari-18
6. Chrome for Developers, Chrome now supports passkeys on iCloud Keychain on macOS — https://developer.chrome.com/blog/passkeys-on-icloud-keychain
7. Mozilla Bugzilla 1935280 (macOS PRF, Firefox 139) — https://bugzilla.mozilla.org/show_bug.cgi?id=1935280
8. Apple Developer Forums, PRF Extension Not Supported in Safari's Cross-Device WebAuthn Flow — https://developer.apple.com/forums/thread/774112
9. mera, Authenticator support — https://mera.category.xyz/authenticator-support/
10. Corbado, WebAuthn L3 Support in Credential Managers (modified 2026-10-03) — https://www.corbado.com/blog/webauthn-l3-credential-manager-support
11. Bitwarden community, Encryption (PRF) via Windows Hello passkey — https://community.bitwarden.com/t/encryption-prf-via-windows-hello-passkey/94236
12. Yubico, YubiKey 5.2.3 Enhancements to FIDO 2 Support — https://support.yubico.com/hc/en-us/articles/360016649319-YubiKey-5-2-3-Enhancements-to-FIDO-2-Support
13. Yubico, What's New in YubiKey Firmware 5.2.3 — https://www.yubico.com/blog/whats-new-in-yubikey-firmware-5-2-3
14. Yubico YubiKey SDK, hmac-secret — https://docs.yubico.com/yesdk/users-manual/application-fido2/hmac-secret.html
15. Mozilla Bugzilla 1958716 (Android PRF, Firefox 149) — https://bugzilla.mozilla.org/show_bug.cgi?id=1958716
16. Tim Cappalli, Please, please, please stop using passkeys for encrypting user data — https://blog.timcappalli.me/p/passkeys-prf-warning (via Simon Willison, https://simonwillison.net/b/9320)
17. lilting.ch summary of [16] — https://lilting.ch/en/articles/passkeys-prf-extension-encryption-risk
