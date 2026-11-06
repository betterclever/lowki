// Unit tests for the scoped keystore, envelope-signer validation, storage, network and
// transports, run against the TEST-ONLY StubLowkiWorker (or the real wasm worker when
// LOWKI_WORKER=real). Run: npm test
import { test } from "node:test";
import assert from "node:assert/strict";
import { MessageChannel } from "node:worker_threads";
import { MnemonicKeystore } from "@kohaku-eth/plugins";
import { entropyToMnemonic } from "@scure/bip39";
import { wordlist } from "@scure/bip39/wordlists/english.js";
import { KeystoreEnvelopeSigner, SignatureSecretManager, type NoteEnvelope } from "../src/kohaku-pp";
import { keccak256, stringToHex } from "viem";
import {
  EncryptedStorage, EnvelopeRejectedError, InProcessTransport, LowkiEnvelopeSigner, LowkiKeystore,
  LowkiWorkerError, MemoryBackend, WebWorkerTransport, call, createNetwork, lowkiSecretManager,
  randomStorageKey, serveLowkiWorker, validateNoteEnvelope, type PostMessageEndpoint, type WorkerTransport,
} from "../src/index";
import { loadWorker } from "../scripts/load-worker";

// Public, fixed 24-word test phrase (entropy 0x01 * 32); the worker only imports 24-word phrases.
const MNEMONIC = entropyToMnemonic(new Uint8Array(32).fill(1), wordlist);
const PRF = new Uint8Array(32).fill(7);
const ENTRYPOINT = 0x34a2068192b1297f2a7f85d7d8cde66f8f0921cbn;

test("worker under test", async (t) => {
  const { kind, source } = await loadWorker();

  t.diagnostic(`worker: ${kind} (${source})`);
});

async function unlocked(): Promise<WorkerTransport> {
  const { worker } = await loadWorker();
  const t = new InProcessTransport(worker);

  await call(t, { type: "importMnemonic", mnemonic: MNEMONIC, prfOutput: PRF });

  return t;
}

const envelope = (over: Record<string, unknown> = {}): NoteEnvelope => ({
  domain: {
    name: "PrivacyPools Keychain", version: "1", chainId: 11155111,
    verifyingContract: "0x34A2068192b1297f2a7f85D7D8CdE66F8F0921cB", salt: keccak256(stringToHex("privacypools")),
  },
  types: { PrivacyPoolNote: [{ name: "depositIndex", type: "uint64" }, { name: "secretIndex", type: "uint64" }] },
  primaryType: "PrivacyPoolNote",
  message: { depositIndex: 3n, secretIndex: 1n },
  ...over,
}) as NoteEnvelope;

test("locked worker refuses derivation", async () => {
  const { worker } = await loadWorker();
  const ks = new LowkiKeystore(new InProcessTransport(worker));

  await assert.rejects(ks.deriveAt("m/28784'/1'/0'"), (e: unknown) => e instanceof LowkiWorkerError && e.code === "locked");
});

test("derive mode: matches Kohaku MnemonicKeystore below m/28784'/1', refuses everything else", async () => {
  const t = await unlocked();
  const ks = new LowkiKeystore(t);
  const ref = new MnemonicKeystore(MNEMONIC);

  for (const p of ["m/28784'/1'/0'", "m/28784'/1'/5'/1'/2'/3'"]) assert.equal(await ks.deriveAt(p), await ref.deriveAt(p));

  for (const p of ["m/28784'/1'", "m/28784'", "m/44'/60'/0'/0/0", "m/28784'/2'/0'", "m/28784/1'/0'", "garbage"]) {
    await assert.rejects(ks.deriveAt(p), `${p} must be refused`);
  }

  // The worker enforces the same scope on its own (bypassing the JS pre-check).
  await assert.rejects(call(t, { type: "kohakuDeriveAt", path: "m/44'/60'/0'/0/0" }),
    (e: unknown) => e instanceof LowkiWorkerError && e.code === "pathNotAllowed");
  await assert.rejects(call(t, { type: "kohakuDeriveAt", path: "m/28784'/1'" }), LowkiWorkerError);

  // A delegation path works only after allowPath (registered in the worker too).
  await ks.allowPath("m/44'/60'/0'/0/0");
  assert.equal(await ks.deriveAt("m/44'/60'/0'/0/0"), await ref.deriveAt("m/44'/60'/0'/0/0"));
  await assert.rejects(ks.deriveAt("m/44'/60'/0'/0/1"));
});

test("signer-only mode: no Privacy Pools key is released", async () => {
  const t = await unlocked();
  const ks = new LowkiKeystore(t, { mode: "signer-only" });

  await assert.rejects(ks.deriveAt("m/28784'/1'/0'"), /signer-only/);
  await assert.rejects(ks.allowPath("m/28784'/1'/0'"), /signer-only/);
  await assert.rejects(ks.deriveAt("m/44'/60'/0'/0/0"), /allowPath/);
  await ks.allowPath("m/44'/60'/0'/0/0");
  assert.match(await ks.deriveAt("m/44'/60'/0'/0/0"), /^0x[0-9a-f]{64}$/);
});

test("envelope signer: same signature and note secrets as Kohaku's KeystoreEnvelopeSigner", async () => {
  const t = await unlocked();
  const ref = SignatureSecretManager({ host: { keystore: new MnemonicKeystore(MNEMONIC) } as never, accountIndex: 2 });
  const ours = lowkiSecretManager(t)({ host: { keystore: new LowkiKeystore(t, { mode: "signer-only" }) } as never, accountIndex: 2 });

  for (const [depositIndex, withdrawIndex] of [[0, 0], [0, 1], [7, 3], [4_294_967_295, 0]] as const) {
    const p = { chainId: 11155111n, entrypointAddress: ENTRYPOINT, depositIndex, withdrawIndex };

    assert.deepEqual(await ours.getSecrets(p), await ref.getSecrets(p));
    assert.equal(await ours.deriveEphemeralSigner(p), await ref.deriveEphemeralSigner(p));
  }

  assert.deepEqual(
    await ours.getDepositSecrets({ chainId: 1n, entrypointAddress: ENTRYPOINT, depositIndex: 9 }),
    await ref.getDepositSecrets({ chainId: 1n, entrypointAddress: ENTRYPOINT, depositIndex: 9 }),
  );
});

test("ppSignNoteEnvelope == Kohaku KeystoreEnvelopeSigner byte-for-byte", async () => {
  const t = await unlocked();
  const ks = new MnemonicKeystore(MNEMONIC);

  for (const accountIndex of [0, 1, 2_147_483_647]) {
    const ref = new KeystoreEnvelopeSigner(ks, accountIndex);
    const ours = new LowkiEnvelopeSigner(t, { accountIndex });

    for (const [chainId, depositIndex, secretIndex] of [[11155111, 0, 0], [1, 5, 2], [11155111, 4_294_967_295, 4_294_967_295]] as const) {
      const env = envelope({
        domain: { ...envelope().domain, chainId },
        message: { depositIndex: BigInt(depositIndex), secretIndex: BigInt(secretIndex) },
      });

      assert.equal(await ours.signEnvelope(env), await ref.signEnvelope(env));
    }
  }
});

test("envelope signer: validation rejects anything but Kohaku's PrivacyPoolNote envelope", async () => {
  const t = await unlocked();
  const signer = new LowkiEnvelopeSigner(t, { accountIndex: 0 });
  const good = envelope();

  assert.deepEqual(validateNoteEnvelope(good), {
    chainId: 11155111n, entrypoint: "0x34A2068192b1297f2a7f85D7D8CdE66F8F0921cB", depositIndex: 3, secretIndex: 1,
  });
  assert.match(await signer.signEnvelope(good), /^0x[0-9a-f]{130}$/);
  assert.ok(signer.identityAddress);

  const d = good.domain;
  const bad: Record<string, NoteEnvelope> = {
    name: envelope({ domain: { ...d, name: "Permit2" } }),
    version: envelope({ domain: { ...d, version: "2" } }),
    salt: envelope({ domain: { ...d, salt: keccak256(stringToHex("other")) } }),
    noSalt: envelope({ domain: { ...d, salt: undefined } }),
    extraDomain: envelope({ domain: { ...d, extra: 1 } }),
    chainId0: envelope({ domain: { ...d, chainId: 0 } }),
    contract: envelope({ domain: { ...d, verifyingContract: "0x1234" } }),
    primaryType: envelope({ primaryType: "Permit" }),
    extraType: envelope({ types: { ...good.types, Mail: [{ name: "x", type: "uint256" }] } }),
    fieldType: envelope({ types: { PrivacyPoolNote: [{ name: "depositIndex", type: "uint256" }, { name: "secretIndex", type: "uint64" }] } }),
    fieldOrder: envelope({ types: { PrivacyPoolNote: [{ name: "secretIndex", type: "uint64" }, { name: "depositIndex", type: "uint64" }] } }),
    extraField: envelope({ types: { PrivacyPoolNote: [...good.types.PrivacyPoolNote, { name: "to", type: "address" }] } }),
    extraMsg: envelope({ message: { depositIndex: 1n, secretIndex: 0n, to: "0x00" } }),
    negIndex: envelope({ message: { depositIndex: -1n, secretIndex: 0n } }),
    bigIndex: envelope({ message: { depositIndex: 2n ** 32n, secretIndex: 0n } }),
    strIndex: envelope({ message: { depositIndex: "1", secretIndex: 0n } }),
  };

  for (const [name, env] of Object.entries(bad)) {
    await assert.rejects(signer.signEnvelope(env), EnvelopeRejectedError, name);
  }
});

test("encrypted storage: round-trips, hides plaintext, binds value to key name", async () => {
  const backend = new MemoryBackend();
  const s = new EncryptedStorage(backend, await randomStorageKey());
  const secret = JSON.stringify({ userSecrets: "nullifier-123456789" });

  assert.equal(await s.get("missing"), null);
  await s.set("a", secret);
  assert.equal(await s.get("a"), secret);
  assert.ok(!backend.raw().get("a")!.includes("nullifier"));
  await backend.set("b", backend.raw().get("a")!); // move ciphertext to another key
  await assert.rejects(s.get("b"));
  const other = new EncryptedStorage(backend, await randomStorageKey());

  await assert.rejects(other.get("a"));
});

test("network: allowlist and https", async () => {
  const seen: string[] = [];
  const net = createNetwork({
    allowHosts: ["ethereum-sepolia-rpc.publicnode.com", "*.filebase.io"],
    fetch: (async (u: RequestInfo | URL) => { seen.push(String(u)); return new Response("ok"); }) as typeof fetch,
  });

  await net.fetch("https://ethereum-sepolia-rpc.publicnode.com");
  await net.fetch("https://ipfs.filebase.io/ipfs/x");
  await assert.rejects(net.fetch("https://filebase.io.evil.com/"));
  await assert.rejects(net.fetch("https://evil.com/"));
  await assert.rejects(net.fetch("http://ethereum-sepolia-rpc.publicnode.com"));
  assert.equal(seen.length, 2);
});

test("web worker transport: request ids keep concurrent responses apart", async () => {
  const { worker } = await loadWorker();
  const { port1, port2 } = new MessageChannel();

  serveLowkiWorker(port2 as never, worker);
  port1.start(); port2.start();
  const t = new WebWorkerTransport(port1 as unknown as PostMessageEndpoint);

  try {
  await call(t, { type: "importMnemonic", mnemonic: MNEMONIC, prfOutput: PRF });
  const paths = Array.from({ length: 8 }, (_, i) => `m/28784'/1'/${i}'`);
  const keys = await Promise.all(paths.map((path) => call(t, { type: "kohakuDeriveAt", path })));
  const ref = new MnemonicKeystore(MNEMONIC);

  for (let i = 0; i < paths.length; i++) assert.equal(keys[i]!.key, await ref.deriveAt(paths[i]!));

  await assert.rejects(call(t, { type: "kohakuDeriveAt", path: "m/0" }), LowkiWorkerError);
  } finally {
    t.close();
    port1.close();
  }
});
