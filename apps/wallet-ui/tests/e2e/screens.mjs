// Screenshots of every screen and state, light + dark (emulated
// prefers-color-scheme), at 390 px (2x) and 1280 px. Writes PNGs and a
// contact sheet to docs/ui-screens/.
//
//   node tests/e2e/screens.mjs [filter]
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { Browser, serve, startDriver, sleep } from "./wd.mjs";

const here = path.dirname(fileURLToPath(import.meta.url));
const ui = path.resolve(here, "../..");
const REPO = path.resolve(ui, "../..");
const DIST = path.join(ui, "dist");
const OUT = path.join(REPO, "docs/ui-screens");
const DRIVER = process.env.CHROMEDRIVER || path.join(REPO, "target/tools/chromedriver/mac_arm-156.0.8078.80/chromedriver-mac-arm64/chromedriver");
const W = "http://127.0.0.1:8711/";
const filter = process.argv[2];

const ready = (b) => b.waitFor("#main h1");
const shots = [
  ["01-sign-in", "Sign-in sheet", "screen=signin", ready],
  ["02-sign-in-google-waiting", "Sign-in: waiting for Google popup", "screen=signin&speed=30", async (b) => { await b.click('[data-test="google"]'); await b.waitFor('[data-test="google"]', { re: /Waiting/ }); }],
  ["03-sign-in-email", "Sign-in: email path", "screen=signin", async (b) => { await b.click('[data-test="email-open"]'); await b.type('[data-test="email-input"]', "ada@lovelace.dev"); }],
  ["04-email-code", "Email code entry", "screen=email", async (b) => { await b.type('[data-test="email-code"]', "4192"); }],
  ["05-secure-wallet-mac", "Secure your wallet (Touch ID wording)", "screen=passkey&platform=mac", ready],
  ["06-secure-wallet-windows", "Secure your wallet (Windows Hello wording)", "screen=passkey&platform=windows", ready],
  ["07-recovery-code", "Recovery code, shown once", "screen=recovery&platform=mac", (b) => b.waitFor('[data-test="recovery-code"]')],
  ["08-home", "Wallet home", "screen=home", (b) => b.waitFor('[data-test="activity"] li')],
  ["09-home-reminder", "Home with recovery reminder", "screen=home&reminder=1", (b) => b.waitFor('[data-test="activity"] li')],
  ["10-home-empty", "Home, empty wallet", "screen=home&empty=1", (b) => b.waitFor(".empty")],
  ["11-send", "Send, private on", "screen=send", async (b) => { await b.type('[data-test="send-to"]', "0x4b0897b0513fdc7c541b6d9d7e929c4e5364d2db"); await b.type('[data-test="send-amount"]', "25"); await b.waitFor(".label-aside", { re: /available/ }); }],
  ["12-send-public", "Send, private off", "screen=send", async (b) => { await b.click('[data-test="private-toggle"]'); await b.type('[data-test="send-to"]', "0x4b0897b0513fdc7c541b6d9d7e929c4e5364d2db"); await b.type('[data-test="send-amount"]', "12.50"); await b.waitFor(".label-aside", { re: /available/ }); }],
  ["13-send-proving", "Private send: building proof (gourd fills)", "screen=send&speed=8", async (b) => { await b.type('[data-test="send-to"]', "0x4b0897b0513fdc7c541b6d9d7e929c4e5364d2db"); await b.type('[data-test="send-amount"]', "25"); await b.click('[data-test="send-submit"]'); await b.waitFor(".step.active", { re: /Building proof/, timeout: 20000 }); await sleep(2600); }],
  ["14-send-done", "Sent privately", "screen=send&speed=0.1", async (b) => { await b.type('[data-test="send-to"]', "0x4b0897b0513fdc7c541b6d9d7e929c4e5364d2db"); await b.type('[data-test="send-amount"]', "25"); await b.click('[data-test="send-submit"]'); await b.waitFor('[data-test="send-done"]'); }],
  ["15-move-in", "Move to private balance", "screen=vault-in", (b) => b.waitFor(".label-aside", { re: /available/ })],
  ["16-confirm-message", "Confirm: sign message", "screen=confirm-message", ready],
  ["17-confirm-typed", "Confirm: typed data (decoded EIP-712)", "screen=confirm-typed", async (b) => { await b.click(".details summary"); }],
  ["18-confirm-tx", "Confirm: transaction, gas covered", "screen=confirm-tx", async (b) => { await b.waitFor('[data-test="tx-preview"]'); await b.click(".details summary"); }],
  ["19-delegation", "Delegation consent", "screen=delegation", (b) => b.waitFor('[data-test="attestation"]', { re: /Verified/ })],
  ["20-settings", "Settings", "screen=settings", (b) => b.waitFor('[data-test="your-apps"] li')],
  ["21-settings-revoke", "Settings: confirm revoke", "screen=settings", async (b) => { await b.waitFor('[data-test="your-apps"] li'); await b.click('[data-test="revoke"]'); }],
  ["22-error-passkey-cancelled", "Error: passkey cancelled", "screen=error-passkey&platform=mac", ready],
  ["23-error-prf-unsupported", "Error: PRF unsupported, with fallback", "screen=error-prf", ready],
  ["24-error-network", "Error: network", "screen=error-network", ready],
  ["25-error-policy-rejected", "Error: delegation policy rejected", "screen=delegation-rejected", (b) => b.waitFor('[data-test="error-policy"]')],
];

const variants = [];
for (const theme of ["light", "dark"]) for (const [w, h, mobile] of [[390, 844, true], [1280, 900, false]]) variants.push({ theme, w, h, mobile });

fs.mkdirSync(OUT, { recursive: true });
const s1 = await serve(DIST, 8711);
const s2 = await serve(REPO, 8712);
const { proc } = await startDriver(DRIVER, 9618);
let b = await new Browser("http://127.0.0.1:9618").open();
// Headless Chrome occasionally drops the session; reopen and retry once.
async function revive(v) { await b.close(); b = await new Browser("http://127.0.0.1:9618").open(); await b.viewport(v.w, v.h, v.mobile); await b.theme(v.theme === "dark"); }
const index = [];
let failures = 0;

async function capture(name, v) {
  const png = await b.shot(v.w, v.h, v.mobile);
  const file = `${name}--${v.theme}--${v.w}.png`;
  fs.writeFileSync(path.join(OUT, file), Buffer.from(png, "base64"));
  return file;
}

try {
  for (const v of variants) {
    await b.viewport(v.w, v.h, v.mobile);
    await b.theme(v.theme === "dark");
    for (const [name, title, q, prep] of shots) {
      if (filter && !name.includes(filter)) continue;
      for (let attempt = 0; attempt < 2; attempt++) {
        try {
          await b.go(W + "?backend=mock&" + q);
          await b.waitFor("#main h1");
          await b.exec("return document.fonts.ready.then(() => true)");
          await prep(b);
          await sleep(450);
          const file = await capture(name, v);
          index.push({ name, title, ...v, file });
          process.stdout.write(".");
          break;
        } catch (e) {
          if (attempt === 0 && /invalid session|no such window|disconnected/.test(e.message)) { console.log(`\nretry ${name}: ${e.message}`); await revive(v); continue; }
          failures++; console.log(`\nFAIL ${name} ${v.theme} ${v.w}: ${e.message}`); break;
        }
      }
    }
    // Demo dapp with the wallet embedded, after sign-in, with a consent sheet open.
    if (!filter || "26-demo-dapp".includes(filter)) {
      try {
        await b.go("http://127.0.0.1:8712/apps/demo-dapp/?wallet=" + encodeURIComponent(W) + "&walletflags=backend%3Dmock%26speed%3D0.05%26platform%3Dmac");
        await b.click("#signin");
        await sleep(150); // sign_in re-renders the sheet; don't click the old button
        await b.frame("#wallet");
        await b.click('[data-test="google"]');
        await b.click('[data-test="create-passkey"]');
        await b.click('[data-test="recovery-done"]');
        await b.waitFor('[data-test="balance"]', { re: /^\d/ });
        await b.frame(null);
        await b.waitFor("#address", { re: /^0x/ });
        await b.click("#delegate");
        await b.frame("#wallet");
        await b.waitFor('[data-test="attestation"]', { re: /Verified/ });
        await b.frame(null);
        await sleep(450);
        const file = await capture("26-demo-dapp", v);
        index.push({ name: "26-demo-dapp", title: "Demo dapp: events received, consent open in the iframe", ...v, file });
        process.stdout.write(".");
      } catch (e) { failures++; console.log(`\nFAIL demo-dapp ${v.theme} ${v.w}: ${e.message}`); await b.frame(null).catch(() => {}); }
    }
  }
} finally {
  await b.close(); proc.kill(); s1.close(); s2.close();
}

// Contact sheet: one row per screen, four variants per row.
if (!filter) {
  const byName = new Map();
  for (const s of index) { if (!byName.has(s.name)) byName.set(s.name, { title: s.title, files: [] }); byName.get(s.name).files.push(s); }
  const order = (s) => (s.theme === "light" ? 0 : 2) + (s.w === 390 ? 0 : 1);
  const rows = [...byName.entries()].map(([name, { title, files }]) => `
  <section>
    <h2><span class="mono">${name}</span> ${title}</h2>
    <div class="grid">${files.sort((a, b) => order(a) - order(b)).map((f) => `
      <figure class="${f.w === 390 ? "phone" : "desk"}"><a href="${f.file}"><img src="${f.file}" alt="${title}, ${f.theme}, ${f.w} px" loading="lazy"></a><figcaption>${f.theme} · ${f.w} px</figcaption></figure>`).join("")}
    </div>
  </section>`).join("");
  fs.writeFileSync(path.join(OUT, "index.html"), `<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<title>Lowki wallet UI screens</title>
<link rel="stylesheet" href="../../design/tokens.css">
<style>
  body { margin: 0; padding: 24px; background: var(--lk-bg); color: var(--lk-text); font: 15px/1.5 system-ui, sans-serif; }
  h1 { font-size: 24px; margin: 0 0 4px; } p { margin: 0 0 24px; color: var(--lk-text-muted); }
  section { border-top: 1px solid var(--lk-line); padding: 20px 0; }
  h2 { font-size: 15px; margin: 0 0 12px; font-weight: 600; } h2 .mono { color: var(--lk-text-muted); margin-right: 8px; }
  .mono { font-family: ui-monospace, Menlo, monospace; font-size: 13px; }
  .grid { display: grid; grid-template-columns: 1fr 2.2fr 1fr 2.2fr; gap: 12px; align-items: start; }
  @media (max-width: 900px) { .grid { grid-template-columns: 1fr 1fr; } }
  figure { margin: 0; } img { width: 100%; display: block; border: 1px solid var(--lk-line); border-radius: 8px; }
  figcaption { font-size: 12px; color: var(--lk-text-muted); margin-top: 4px; }
</style></head><body>
<h1>Lowki wallet UI: every screen and state</h1>
<p>${index.length} screenshots. Headless Chrome 156, light and dark via emulated prefers-color-scheme, 390 px (2x) and 1280 px. Generated by apps/wallet-ui/tests/e2e/screens.mjs.</p>
${rows}
</body></html>
`);
}
console.log(`\n${index.length} screenshots, ${failures} failures -> ${OUT}`);
process.exitCode = failures ? 1 : 0;
