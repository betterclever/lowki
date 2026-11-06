// Scripted click-through of the main flow, driven through the demo dapp so
// the postMessage protocol is exercised end to end.
//
//   node tests/e2e/clickthrough.mjs            (after `trunk build --release`)
//
// Env: CHROMEDRIVER (path), WALLET_DIST, DAPP_DIR.
import path from "node:path";
import { fileURLToPath } from "node:url";
import { Browser, serve, startDriver, sleep } from "./wd.mjs";

const here = path.dirname(fileURLToPath(import.meta.url));
const ui = path.resolve(here, "../..");
const DIST = process.env.WALLET_DIST || path.join(ui, "dist");
// The dapp is served from the repo root so it can load design/tokens.css.
const REPO = process.env.REPO_ROOT || path.resolve(ui, "../..");
const DRIVER = process.env.CHROMEDRIVER || path.resolve(ui, "../../target/tools/chromedriver/mac_arm-156.0.8078.80/chromedriver-mac-arm64/chromedriver");

const results = [];
async function step(name, fn) {
  const t = Date.now();
  try { await fn(); results.push(["PASS", name, Date.now() - t]); console.log(`PASS  ${name} (${Date.now() - t} ms)`); }
  catch (e) { results.push(["FAIL", name]); console.log(`FAIL  ${name}\n      ${e.message}`); throw e; }
}
const assert = (c, msg) => { if (!c) throw new Error("assertion failed: " + msg); };

const s1 = await serve(DIST, 8711);
const s2 = await serve(REPO, 8712);
const { proc } = await startDriver(DRIVER);
const b = await new Browser("http://127.0.0.1:9617").open();
let ok = false;
try {
  await b.go("http://127.0.0.1:8712/apps/demo-dapp/?wallet=" + encodeURIComponent("http://127.0.0.1:8711/") + "&walletflags=backend%3Dmock%26platform%3Dmac");

  await step("dapp asks to sign in; wallet shows the sign-in sheet", async () => {
    await b.frame("#wallet");
    await b.waitFor("#main h1", { re: /Continue to Game X/ });
    await b.frame(null);
    await b.click("#signin");
    await sleep(150);
  });

  await step("sign in with Google (mock popup)", async () => {
    await b.frame("#wallet");
    await b.click('[data-test="google"]');
    await b.waitFor('[data-test="google"]', { re: /Waiting for Google/ , timeout: 2000 });
    await b.waitFor("#main h1", { re: /Secure your wallet/ });
  });

  await step("create passkey (Touch ID wording on mac)", async () => {
    const label = await b.text('[data-test="create-passkey"]');
    assert(/Touch ID/.test(label), "button says Touch ID, got " + label);
    await b.click('[data-test="create-passkey"]');
    await b.waitFor("#main h1", { re: /Save your recovery code/ });
  });

  await step("save recovery code (6 groups, back up, confirm)", async () => {
    const groups = await b.findAll('[data-test="recovery-code"] li');
    assert(groups.length === 6, "6 code groups, got " + groups.length);
    await b.click('[data-test="backup"]');
    await b.waitFor('[data-test="backup"]', { re: /Backed up to iCloud/ });
    await b.click('[data-test="recovery-done"]');
  });

  let address = "";
  await step("home shows balance, private balance and activity", async () => {
    await b.waitFor('[data-test="balance"]', { re: /^\d/ });
    await b.waitFor('[data-test="private-balance"]', { re: /250\.00/ });
    await b.waitFor('[data-test="activity"] li');
    address = await b.attr(".account .chip-addr", "title");
    assert(/^0x[0-9a-f]{40}$/.test(address), "address " + address);
  });

  await step("dapp received authenticated(address, pairwise user id, token)", async () => {
    await b.frame(null);
    await b.waitFor("#address", { re: /^0x/ });
    assert((await b.text("#address")) === address, "dapp address matches wallet");
    assert(/^lk_[0-9a-f]{24}$/.test(await b.text("#userid")), "pairwise id");
    assert((await b.text("#token")).length > 20, "access token");
  });

  await step("send privately with proof progress", async () => {
    await b.frame("#wallet");
    await b.click('[data-test="send"]');
    await b.type('[data-test="send-to"]', "0x4b0897b0513fdc7c541b6d9d7e929c4e5364d2db");
    await b.type('[data-test="send-amount"]', "25");
    assert((await b.attr('[data-test="private-toggle"]', "aria-checked")) === "true", "private by default");
    await b.click('[data-test="send-submit"]');
    await b.waitFor(".step.active", { re: /Syncing pool/, timeout: 3000 });
    await b.waitFor(".step.active", { re: /Building proof/, timeout: 3000 });
    const now = Number(await b.attr('[role="progressbar"]', "aria-valuenow"));
    assert(now > 20 && now < 90, "overall progress mid-way, got " + now);
    await b.waitFor(".step.active", { re: /Submitting/, timeout: 3000 });
    await b.waitFor('[data-test="send-done"]', { re: /Sent privately\./, timeout: 5000 });
    await b.click('[data-test="done"]');
    await b.waitFor('[data-test="private-balance"]', { re: /225\.00/ });
  });

  await step("approve delegation from the consent sheet", async () => {
    await b.frame(null);
    await b.click("#delegate");
    await b.frame("#wallet");
    await b.waitFor("#main h1", { re: /Let Game X act for you/ });
    const can = await b.text('[data-test="policy-can"]');
    assert(can.includes("Swap on router 0x7a25…488d, up to 50 USDC per day, on Monad, until 31 Dec 2026"), "can sentence: " + can);
    const cant = await b.text('[data-test="policy-cant"]');
    for (const s of ["Send to other addresses", "See your other apps", "Touch your private balance"]) assert(cant.includes(s), "can't: " + s);
    await b.waitFor('[data-test="attestation"]', { re: /Verified signer, build\s*3f2a9c1/ });
    await b.waitFor('[data-test="approve"]', { enabled: true });
    await b.click('[data-test="approve"]');
    await b.waitFor('[data-test="balance"]', { re: /^\d/ });
    await b.frame(null);
    await b.waitFor("#log", { re: /"type":"delegation_approved"/ });
  });

  await step("revoke delegation in Settings", async () => {
    await b.frame("#wallet");
    await b.click('[data-test="open-settings"]');
    await b.waitFor('[data-test="delegations"]', { re: /Game X/ });
    const before = (await b.findAll('[data-test="delegations"] .deleg')).length;
    await b.click('[data-test="delegations"] .deleg:first-child [data-test="revoke"]');
    await b.click('[data-test="revoke-confirm"]');
    await b.waitFor('[data-test="toast"]', { re: /Game X can no longer act for you/ });
    const after = (await b.findAll('[data-test="delegations"] .deleg')).length;
    assert(after === before - 1, `one fewer delegation (${before} -> ${after})`);
    if (after === 0) await b.waitFor(".empty", { re: /No app can act for you/ });
    else assert(!/Game X/.test(await b.text('[data-test="delegations"]')), "Game X gone");
    await b.frame(null);
    await b.waitFor("#log", { re: /"type":"delegation_revoked"/ });
  });
  ok = true;
} finally {
  await b.close();
  proc.kill();
  s1.close(); s2.close();
  const pass = results.filter((r) => r[0] === "PASS").length;
  console.log(`\n${pass}/${results.length} steps passed${ok ? "" : " (stopped at first failure)"}`);
  process.exitCode = ok ? 0 : 1;
}
