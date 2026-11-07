# Lowki demo: morning runbook and recording script

Live URLs right now (rewritten by `scripts/demo-up.sh` every time it runs):

<!-- URLS:BEGIN -->
| What | URL |
|---|---|
| Lowki Tips | https://jim-cambridge-meals-court.trycloudflare.com/ |
| Creator inbox | https://jim-cambridge-meals-court.trycloudflare.com/creator.html?name=chai.lowki.eth |
| Privy Swap | https://configure-translator-encourage-consortium.trycloudflare.com/?app=app_30f7d419eb5c11072449&wallet=https%3A%2F%2Fsystematic-contemporary-isbn-passive.trycloudflare.com%2F%3Fapp_name%3DPrivy%2520Swap |
| Wallet home | https://systematic-contemporary-isbn-passive.trycloudflare.com/ |
| Showcase site | https://msgid-applied-jar-kay.trycloudflare.com/ |
| Fork RPC (Anon-RPC target) | https://appearing-ban-bizarre-nearby.trycloudflare.com/rpc |
| ENS gateway (on chain) | https://industry-hobbies-travels-exciting.trycloudflare.com/v1/ens/gateway |

_Written by scripts/demo-up.sh at 2026-11-07T01:53:43Z; Sepolia fork at block 12078882._
<!-- URLS:END -->

## 1. Morning steps (about 10 minutes)

In the Linux VM, from the repo root:

```sh
scripts/demo-up.sh       # idempotent: everything already running is reused, URLs stay the same
scripts/demo-check.sh    # must end with ALL CHECKS PASSED (about 20 s)
```

`demo-up.sh` prints one block of URLs and rewrites the table above. If a check fails, see
"Fallbacks" at the end.

Then, on the Mac:

1. **Browser.** Use Google Chrome (WebGPU) in a **new profile named "Lowki demo"** (no extensions,
   no bookmarks bar). Make the window about 1280×800, or record that region. Zoom at 100 %.
2. **Tabs, in this order:**
   1. Lowki Tips
   2. Privy Swap (the long URL, with `?app=…&wallet=…`)
   3. Creator inbox
   4. Showcase site
   
   The wallet opens by itself (a link from Tips opens it in a new tab).
3. **Terminal, off screen**, for the email code:
   ```sh
   scripts/demo-check.sh code      # prints "<email>: <6 digits>"
   ```
   The chai creator seed for the inbox is in `/Users/betterclever/.lowki/creators.env`
   (`CHAI_SEED=0x…`). Copy it to the clipboard before recording; don't show the file on screen:
   ```sh
   grep CHAI_SEED /Users/betterclever/.lowki/creators.env | cut -d= -f2 | pbcopy   # on the Mac
   ```
4. **Passkey.** When Chrome asks where to save the passkey, choose **iCloud Keychain** ("iCloud
   Passwords", Touch ID) or Google Password Manager. Do **not** choose "This Chrome profile": it has
   no PRF, and the wallet will report "PRF unsupported".
5. **Do one full dry run with a throwaway email** (e.g. `dryrun1@lowki.demo`). It also warms the
   tunnels, the proof artifacts and Nox. Then record with a new email (e.g. `you@lowki.demo`). In
   any passkey picker, choose the newest "Lowki" passkey.
   - The dry run's tips stay in the inbox. They add rows, which is fine.
   - For a clean list, `scripts/demo-down.sh && scripts/demo-up.sh` gives new URLs and a new RP ID,
     so old passkeys no longer appear. Re-open the tabs after that.

## 2. Recording script (2:58)

Timestamps are targets. Cut out the waits in editing: the Move in (ASP approval), the name claim
(two live Sepolia transactions, about 20-40 s) and the first Nox connection.

| Time | Screen and clicks | Voiceover |
|---|---|---|
| **0:00 – 0:08** | **Tips tab**, landing page ("Tip your favourite creators"). | "This is Lowki: Privy, but private. Same SDK as Privy, but the keys live on your device, every app gets its own wallet, and payments can be private." |
| **0:08 – 0:14** | Scroll to the footer: the diff `@privy-io/react-auth` → `@lowki/react-auth`. | "Lowki Tips was written for Privy's React SDK. We changed one import." |
| **0:14 – 0:38** | **Sign in** → **Continue with email** → type the email → continue → type the code (terminal: `scripts/demo-check.sh code`) → verify → **Use Touch ID** → Touch ID → recovery code → **I've saved it**. | "Sign-in is email plus a passkey. The seed is created in a Rust worker in this browser and wrapped with the passkey's PRF output, so the server never holds a usable key. The recovery code is the third share of a 2-of-3 backup." |
| **0:38 – 0:44** | Hover the header's address / user ID ("Only this app sees this address"). Click **+1 test ETH**. | "Tips gets its own address and a pairwise user ID. On this Sepolia fork, a faucet funds me." |
| **0:44 – 1:08** | **Chai Stories → Tip 0.01 ETH**: "chai.lowki.eth resolves to 0x…", **Never seen before**. **Send tip** → wallet sheet → **Approve** → "Tip sent". **Tip again** → a different address → **Approve**. Point at "Your tips … All different". | "chai.lowki.eth is an ENSv2 name on Sepolia. Every lookup, here plain viem getEnsAddress, returns a fresh stealth address, verified on chain before the wallet sees it. Tip again: a new address. Nobody can add these up." |
| **1:08 – 1:22** | **Privy Swap tab** → **Log in** → **Sign in with passkey** → Touch ID. Point at User ID and Wallet. | "This is Privy's own docs example, unmodified apart from the import alias. Same passkey, but a different wallet and a different user ID. The apps can't link me." |
| **1:22 – 1:40** | Back to **Tips** → **All creators** → Chai **Tip privately** → **Move ETH in from your Lowki wallet** (new tab) → **Sign in with passkey** → point at **Private connection … exit <ip>** → **Move in** → `0.1` → submit. *(Cut the wait.)* "Now private" → **Done**. Close the tab. | "The wallet's reads go through Anon-RPC, over Nox, so the RPC can't see my IP. Move in deposits into Privacy Pools through Kohaku's plugin." |
| **1:40 – 2:00** | In Tips: **Send privately** → **Approve**. The lauki (gourd) fills: Syncing pool → Building proof → Submitting. "Sent privately". | "Now a private tip. The zero-knowledge proof is built on my GPU, in about two tenths of a second. A relayer submits it; nothing on chain links it to me." |
| **2:00 – 2:14** | **Creator inbox tab** → paste the chai seed (⌘V) → **Sign with viewing key and scan** → rows with amounts and "spend key ✓". | "Chai's side: with the viewing key, the inbox finds every tip in the gateway's records and can derive the key to spend each one." |
| **2:14 – 2:40** | **Wallet home tab** (wallet URL, no parameters) → **Sign in with passkey**. Point at **Your apps**: Lowki Tips and Privy Swap with their addresses. In **Your name**, type a label (e.g. `ana`) → **Claim ana.lowki.eth**. *(Cut the wait: three live steps.)* The name shows. In the terminal: `scripts/demo-check.sh pay ana.lowki.eth` → in the home, refresh payments → the payment row. | "My wallet home. Only I can see which apps I use: the list is encrypted on my device. And I can claim my own name: ana.lowki.eth, registered live on Sepolia, sponsored, so I need no ETH. Someone pays it, and my wallet finds the payment at a stealth address." |
| **2:40 – 2:52** | **Showcase site tab** → scroll to the race → **Run it again**. | "The same withdrawal proof, side by side: Lowki on WebGPU against snarkjs." |
| **2:52 – 2:58** | Stay on the result ("3.5× faster than snarkjs"). | "Keys on your device. Open source. Self-hostable. That's Lowki." |

Notes for the takes:
- **Run the race once in the dry run** and keep the site tab open. The first WebGPU start compiles
  the shaders, which took 11 s in the rehearsal. "Run it again" is then instant.
- **The claim is live** (two sponsored Sepolia transactions from the operator key, about 0.004 ETH
  of Sepolia gas). Use a new label per take; the label must be 3-32 of `a-z 0-9 -`. The payment is
  made on the fork (`demo-check.sh pay` resolves the name on live Sepolia, which records the fresh
  stealth address, and sends 0.01 ETH to it on the fork, where the wallet reads balances).
- If time is short, drop the Privy Swap beat (1:08-1:22) or the site race.

### The honest notes (say briefly on camera, or put them in the description)

- **The private withdraw runs on a Sepolia fork.** On live Sepolia the 0xbow ASP publishes
  placeholder IPFS CIDs, and its leaves return 404. So no withdrawal can prove ASP membership there
  today. We checked whether the root could be rebuilt from chain data instead: it can't
  ([`docs/PRIVATE-SEND.md`, "Can the ASP root be rebuilt from chain data?"](PRIVATE-SEND.md#can-the-asp-root-be-rebuilt-from-chain-data-tested-2026-11-06-no)).
  On the fork, the ASP is simulated by impersonating Sepolia's test postman (as Kohaku's own tests
  do) and the relayer is a local one. The proof, the Privacy Pools contracts and Kohaku's plugin
  are the real ones.
- **The ENS names are live on Sepolia.** `lowki.eth`, the registry and registrar, the
  `StealthResolver` and `chai`/`tabla`/`pixel.lowki.eth` resolve from any wallet. The fork is a
  copy of Sepolia, so it has them too.
- **The URLs are Cloudflare quick tunnels** to a laptop, so they are temporary, and the gateway URL
  written on chain is one of them.
- **Faucet:** the "+1 test ETH" button only works on the fork.

Suggested one-liner: *"Names and contracts are live on Sepolia. The private withdraw runs on a
Sepolia fork, because Sepolia's ASP doesn't publish its data yet. Everything is served from a
laptop through Cloudflare tunnels."*

## 3. Fallbacks

| Problem | Fix |
|---|---|
| `demo-check.sh` fails on one service | `scripts/demo-up.sh` again: it restarts only what is down. Logs are in `target/demo-live/run/<name>.log`. |
| A public URL gives Cloudflare 502/1033 | That tunnel died. `scripts/demo-down.sh && scripts/demo-up.sh` gives all-new tunnels. **New wallet URL = new passkey RP ID**: sign up again (new email). |
| `ENS gateway host` fails, or names don't resolve | The **:8830 tunnel** (its URL is on chain) died. Run `scripts/update-gateway-url.sh`: it starts a new tunnel and re-points `lowki.eth` + the 3 creators on live Sepolia (4 sponsored transactions, about 1 min). Then `FRESH_FORK=1 scripts/demo-up.sh` (new fork + Kohaku snapshot, about 4 min). |
| Wallet says "Direct connection" (Nox didn't start) | Fine to continue: everything works, only the IP privacy is off. Say "Anon-RPC fell back to direct". To skip Nox: add `&rpc=direct` to the wallet URL. |
| "PRF unsupported" at the passkey step | The passkey went to "This Chrome profile". Sign out, use a new email, and pick iCloud Keychain. |
| Private tip fails or hangs | Retry once. If the ASP approval never came, open the wallet tab again: Move in shows the pending note. Otherwise cut to the backup video's private-tip segment (`target/demo-live/backup-demo.mp4`, labelled automated). |
| Tip shows "Couldn't resolve chai.lowki.eth" | Run `scripts/demo-check.sh`. If the ENS rows fail, see the :8830 row above. |
| Inbox finds nothing | You pasted the wrong seed, or used a different creator tab. Use `CHAI_SEED` and the Chai tab. |
| Site race doesn't start | WebGPU is off. Use Chrome, not Safari, and check `chrome://gpu`. The race falls back to CPU. |
| Claim fails or hangs | Skip it on camera: the apps list is the point. The claim needs the operator key's Sepolia ETH (`cast balance 0x12333c99fe454F547772dc6D04D21Afef2b9A559`). |
| Payment row doesn't appear | Click the payments refresh button again after 5 s. `demo-check.sh pay` prints the stealth address it paid. |

## 4. What runs where

`scripts/demo-up.sh` (VM; ports 8850-8860, plus 8830). If `demo-check.sh` says the auth server is
on :8860 and you want one server, stop the other :8830 server yourself (it is
`apps/wallet-ui/scripts/home-server.sh` and its `lowki-server` child), then run `scripts/demo-up.sh`.
Accounts made on :8860 stay in `target/demo-live/server/lowki.db`, which the :8830 server then uses.

| Port | What | Public through |
|---|---|---|
| 8830 | lowki-server: auth, app tokens, JWKS, ENS CCIP-Read gateway + stealth records | the existing tunnel written on chain (never restarted by the scripts) |
| 8860 | lowki-server for auth, **only while :8830 is held by a server demo-up didn't start** (tonight: the wallet-home agent's `home-server.sh`, same live ENS config). ENS lookups and records then stay with :8830; the wallet proxies `/v1/ens/*` there. Once :8830 is free, the next `demo-up.sh` moves auth back to :8830 (a new passkey RP ID is not needed: the RP ID is the wallet host). | via the wallet host |
| 8850 | anvil fork of Sepolia at a pinned block (has `lowki.eth` + the creators) | — |
| 8851 | plain-HTTP forwarder to a public Sepolia RPC (the gateway's on-chain reads) | — |
| 8852 | fork gateway: `/rpc` (logged), `/ip`, simulated ASP `/asp`, relayer `/relayer` | tunnel "fork" (the Anon-RPC target) |
| 8855 | wallet (`/v1` → lowki-server, `/lowki-config.json`, circuits) | tunnel "wallet" (= passkey RP ID, JWT issuer) |
| 8856 | Lowki Tips (+ `/rpc` → fork, with the faucet) | tunnel "tips" |
| 8857 | privy-swap | tunnel "privy" |
| 8858 | showcase site | tunnel "site" |

- **State:** `target/demo-live/`. `run/` holds pids, logs and tunnel logs; `server/` holds the
  SQLite database and generated secrets; `urls.json` has the URLs.
- **Secrets:** read from `/Users/betterclever/.lowki/` (`sepolia-test.env`: operator key;
  `creators.env`: creator phrase and seeds, mode 600). They are never printed and never written
  into an app build.
- **Rehearsal:** `mac sh -lc 'cd <repo> && node target/demo-live/rehearsal.mjs'` replays this
  script in headless Chrome against the public URLs. It writes screenshots to
  `target/demo-live/rehearsal/` and the automated backup video to `target/demo-live/backup-demo.mp4`.
- **Stop:** `scripts/demo-down.sh` stops only what `demo-up.sh` started. `KEEP_TUNNELS=1` keeps
  the URLs.
