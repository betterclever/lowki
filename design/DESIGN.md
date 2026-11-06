# Lowki design language

Lowki should feel like a calm, careful friend who keeps your secrets. It shouldn't feel like a trading terminal. The name packs three ideas together:

- **low-key** describes the tone.
- **lauki** (लौकी, bottle gourd) gives the colour and the mark.
- **privacy** is the promise.

Files in this folder:

- `tokens.css` holds the only source of colours, type, space, shape and motion.
- `logo/` holds the marks.
- `index.html` is the visual spec. Open it locally.

## The mark

A club-shaped lauki is a keyhole turned upside down. The mark is a lime gourd with a keyhole cut into its body, plus a leaf in the lighter lauki green. So it reads as the vegetable and as a lock in the same glance.

| File | Use |
|---|---|
| `lowki-mark.svg` | Default mark. Works on light and dark. |
| `lowki-mark-mono.svg` | One colour (`currentColor`), for stamps, favicons in UI, and watermarks. |
| `lowki-app-icon.svg`, `favicon.svg` | Ink tile with a lime mark. Use for app and browser icons. |
| `lowki-wordmark-{ink,light,lime}.svg` | The wordmark: monoline, round caps, all lowercase, and the dot of the i is a tiny gourd. |
| `lowki-lockup.svg`, `lowki-lockup-dark.svg` | The mark and wordmark together, for headers and the sign-in sheet. |

**Rules for the mark**

- Leave clear space equal to the width of the keyhole circle × 2 on every side.
- The mark must be at least 16 px tall. Below 24 px, use the app icon instead.
- Never recolour the keyhole, outline the mark, rotate it upright into a plain keyhole, add gradients or drop shadows, or set the wordmark in a font.
- The wordmark is always lowercase.

## Colour

| Token | Light | Dark | Use |
|---|---|---|---|
| `--lk-lime` | #B5F23D | #B5F23D | Primary actions, the mark, active and focus states on dark |
| `--lk-on-lime` | #0F1B0D | #0F1B0D | Text and icons on lime |
| `--lk-lauki` | #C9DCA6 | #3B4A2C | Brand tint: leaf, selected rows, soft badges |
| `--lk-flesh` | #F1F5E4 | #172214 | Quiet tinted surfaces such as info panels |
| `--lk-ink` | #0F1B0D | — | Text, the private state, the app tile |
| `--lk-accent-text` | #3F6B12 | #B5F23D | Links and accents *on light surfaces* |
| `--lk-private-*` | ink / lime | deep green / lime | Anything shielded or private |
| `--lk-danger`, `--lk-warning`, `--lk-success` | | | Semantic only, kept separate from the brand accent |

- **Lime is a fill, never text on light.** It has poor contrast on white. On light surfaces use `--lk-accent-text`; on dark surfaces lime text is fine.
- **One lime action per view.** Lime marks the thing to do next. Everything else is quiet.
- **Neutrals lean green** (#5C6A52, #DCE4CB), never pure grey.
- **The private state is ink.** Private balances and private sends sit on ink with lime text, so they look sealed.

## Type

| Role | Face | Where |
|---|---|---|
| Display | Bricolage Grotesque 600–700 | Sheet titles, balances, hero numbers. Use sparingly. |
| Text | Instrument Sans 400/500/600 | Everything else |
| Data | IBM Plex Mono 400/500 | Addresses, hashes, amounts in tables, codes |

- **Ship the font files with the app** (all three are SIL OFL). A privacy wallet must never load Google Fonts or any third-party CDN at runtime.
- **Scale:** 12 / 14 / 16 / 20 / 28 / 40. Body is 16 with 1.5 leading. Labels are 12 uppercase with 0.06em tracking.
- **Digits:** use `font-variant-numeric: tabular-nums` wherever they line up.
- **Addresses:** show them as `0x1a2b…9f0e`, in mono, with the full value in a copy button and a tooltip.

## Shape, space and elevation

- **Space:** 4 px base: 4, 8, 12, 16, 20, 24, 32, 40, 56.
- **Radius:**
  - 12 px for buttons and inputs;
  - 20 px for sheets and cards;
  - pills for chips, status and addresses. The pill echoes the gourd neck.
- **The sheet is the main surface.** It's 400 px wide at most, full-width on phones, with 24 px padding and a 20 px radius, using `--lk-shadow-sheet`.
- **Use borders before shadows.** Use one hairline (`--lk-line`). Shadows only lift sheets and popovers.
- **Touch targets** are at least 44 px.

## Motion

- 120 ms for state changes, 200 ms for entering, 320 ms for sheets, all on `--lk-ease`.
- No bounce and no confetti.
- **The signature moment is the gourd filling.** While a private proof runs, the mark fills with lime from the bottom, stage by stage: syncing the pool, building the proof, submitting.
- Honour `prefers-reduced-motion`: reduced motion swaps fills for stepped states.

## Icons

- Line icons on a 20 px grid with a 1.75 px stroke, round caps and joins, `currentColor`.
- The keyhole and gourd glyphs are reserved for privacy states: shielded, private send, sealed key.

## Voice

Plain, short and calm, written from the user's side. No hype and no exclamation marks.

| Do | Don't |
|---|---|
| "Sent privately." | "Woohoo! Your anonymous tx is live 🚀" |
| "This app can't see your other apps." | "Cross-origin unlinkability enabled" |
| "Approve" / "Revoke access" | "Sign" / "Remove signer quorum" |
| "Keep this recovery code somewhere safe. It's the only way back in without this device." | "Back up your Shamir share R" |
| "Your passkey didn't respond. Try again." | "WebAuthn NotAllowedError" |

- **Consent sheets** spell out what an app *can* do and what it *can't*, in that order, as plain sentences.
- **Errors** say what happened and what to do next.

## Components

| Component | Spec |
|---|---|
| **Primary button** | Lime fill, ink text, 12 px radius, 48 px tall, 600 weight. Hover goes one step darker, press one step further. Disabled is lauki-soft with faint text. |
| **Secondary button** | Surface fill with a hairline border, text colour. **Quiet button:** text only, in accent text colour. |
| **Sign-in sheet** | Lockup, then the app name ("Continue to *Game X*"). Below that: Google, passkey and email buttons (secondary). One line of privacy reassurance in muted text. |
| **Address chip** | Pill, mono 14, surface-2 fill, copy icon. Turns lauki on hover. |
| **Private badge** | Pill on private-bg with private-fg text and a keyhole glyph, labelled "Private". |
| **Balance** | Display face at 28/40 px with tabular digits. Private balance sits on an ink card. |
| **Consent card** | Title, then a "Can" list (check icons in accent text) and a "Can't" list (minus icons in muted text). Then expiry, then a verified-signer row with a mono build hash. Primary "Approve" and quiet "Not now". |
| **Proof progress** | Gourd fill plus a three-step list (Syncing pool / Building proof / Submitting) with times. |
| **Confirm sheets** | What, to whom, how much, and the fee (or "Gas covered by *App*"). Decoded call data goes in a collapsible mono block. |
