// The TypeScript side: Kohaku sources from the pinned clone (@K/... is aliased to
// vendor/kohaku/packages by build.mjs), the ScopeLift SDK, viem and noble.
import type { Case } from "./harness.ts";

import { MnemonicKeystore } from "@K/plugins/src/host/mnemonic-keystore.ts";
import { parseDerivationPath } from "@K/plugins/src/host/derivation-path.ts";
import { poseidonT3 } from "@K/poseidon/src/index.ts";
import { PoseidonT3LeanIMT } from "@K/poseidon-lean-imt/src/index.ts";
import { SignatureSecretManager } from "@K/privacy-pools/src/account/signature-keys.ts";
import { commitment } from "@K/privacy-pools/src/utils.ts";
import {
  encodeFeeData,
  encodePaymasterData,
  encodePrivacyPoolAdapterData,
} from "@K/privacy-pools/src/paymaster/adapter-data.ts";
import { buildSignedUserOp } from "@K/privacy-pools/src/paymaster/utils.ts";
import { poseidon as maciPoseidon } from "maci-crypto/build/ts/hashing.js";
import { mnemonicToSeedSync } from "@scure/bip39";
import { HDKey } from "@scure/bip32";
import { secp256k1 } from "@noble/curves/secp256k1.js";
import { privateKeyToAccount, sign as viemSign } from "viem/accounts";
import { checkStealthAddress, generateStealthAddress } from "@scopelift/stealth-address-sdk";

const hex = (b: Uint8Array) => Array.from(b, (x) => x.toString(16).padStart(2, "0")).join("");
const unhex = (h: string) => Uint8Array.from((h.replace(/^0x/, "").match(/../g) ?? []).map((x) => parseInt(x, 16)));

// Kohaku MnemonicKeystore.deriveAt, minus the mnemonicToSeedSync it does on every call.
function deriveFromSeed(seed: Uint8Array, path: string): Uint8Array {
  const child = parseDerivationPath(path).reduce((n: HDKey, i: number) => n.deriveChild(i), HDKey.fromMasterSeed(seed));
  return child.privateKey!;
}

export function tsCases(F: any, opts: { workerUrl?: string | URL; parallel?: boolean } = {}): Case[] {
  const K = F.keys, PP = F.pp, ST = F.stealth, AA = F.aa;
  const cases: Case[] = [];
  let sink: unknown;

  // ---------------- keys ----------------
  const seed = mnemonicToSeedSync(K.mnemonic);
  cases.push({
    id: "keys.mnemonic_to_seed", batch: 1,
    run: () => (sink = mnemonicToSeedSync(K.mnemonic)),
    check: () => hex(mnemonicToSeedSync(K.mnemonic)),
  });
  cases.push({
    id: "keys.derive_pp_from_seed", batch: 1,
    run: () => (sink = deriveFromSeed(seed, K.ppPath)),
    check: () => hex(deriveFromSeed(seed, K.ppPath)),
  });
  cases.push({
    id: "keys.derive_eoa10_from_seed", batch: 10,
    run: () => { for (const p of K.eoaPaths) sink = deriveFromSeed(seed, p); },
    check: () => K.eoaPaths.map((p: string) => hex(deriveFromSeed(seed, p))).join(","),
  });
  cases.push({
    id: "keys.mnemonic_to_key", batch: 1,
    note: "new MnemonicKeystore(m).deriveAt(pp path): validate + PBKDF2 + BIP-32",
    run: () => new MnemonicKeystore(K.mnemonic).deriveAt(K.ppPath).then((k: string) => (sink = k)),
    check: async () => (await new MnemonicKeystore(K.mnemonic).deriveAt(K.ppPath)).slice(2),
  });
  const ks = new MnemonicKeystore(K.mnemonic);
  cases.push({
    id: "keys.keystore_derive", batch: 1,
    note: "existing keystore; Kohaku re-runs PBKDF2 inside every deriveAt, Lowki keeps the seed",
    run: () => ks.deriveAt(K.ppPath).then((k: string) => (sink = k)),
    check: async () => (await ks.deriveAt(K.ppPath)).slice(2),
  });
  const digests = K.digests.map((d: string) => "0x" + d);
  cases.push({
    id: "keys.sign_digest", batch: digests.length,
    note: "viem sign({hash, privateKey})",
    run: async () => { for (const h of digests) sink = await viemSign({ hash: h, privateKey: "0x" + K.signKey }); },
    check: async () => {
      const out = [];
      for (const h of digests.slice(0, 8)) {
        const s: any = await viemSign({ hash: h, privateKey: "0x" + K.signKey });
        out.push(s.r.slice(2).padStart(64, "0") + s.s.slice(2).padStart(64, "0") + (27 + s.yParity).toString(16));
      }
      return out.join(",");
    },
  });
  const skBytes = unhex(K.signKey);
  const digestBytes = K.digests.map(unhex);
  cases.push({
    id: "keys.sign_digest_noble2", batch: digests.length,
    note: "@noble/curves 2.0.1 secp256k1.sign(prehash:false)",
    run: () => { for (const d of digestBytes) sink = secp256k1.sign(d, skBytes, { prehash: false, format: "recovered" }); },
    check: () => digestBytes.slice(0, 8).map((d: Uint8Array) => {
      const s = secp256k1.sign(d, skBytes, { prehash: false, format: "recovered" }) as Uint8Array;
      return hex(s.slice(1)) + (27 + s[0]).toString(16); // recovered = rec || r || s
    }).join(","),
  });

  // ---------------- pp-core ----------------
  const pairs: [bigint, bigint][] = PP.poseidonPairs.map((p: string[]) => [BigInt(p[0]), BigInt(p[1])]);
  cases.push({
    id: "pp.poseidon_t3", batch: pairs.length,
    note: "Kohaku @kohaku-eth/poseidon poseidonT3",
    run: () => { let x = 0n; for (const [a, b] of pairs) x = poseidonT3(a, b); sink = x; },
    check: () => pairs.slice(0, 4).map(([a, b]) => poseidonT3(a, b).toString()).join(","),
  });
  cases.push({
    id: "pp.poseidon_t3_maci", batch: pairs.length,
    note: "maci-crypto poseidon([a,b]) (what Kohaku uses for precommitments)",
    run: () => { let x = 0n; for (const [a, b] of pairs) x = maciPoseidon([a, b]); sink = x; },
    check: () => pairs.slice(0, 4).map(([a, b]) => maciPoseidon([a, b]).toString()).join(","),
  });
  const leaves: bigint[] = PP.leaves.map(BigInt);
  for (const [n, tag, iters] of [[1000, "1k", undefined], [10_000, "10k", undefined], [100_000, "100k", 10]] as const) {
    const ls = leaves.slice(0, n);
    cases.push({
      id: `pp.limt_build_${tag}`, batch: 1, iters, warmup: iters ? 2 : undefined,
      run: () => (sink = new PoseidonT3LeanIMT(ls)),
      check: () => new PoseidonT3LeanIMT(ls).root.toString(),
    });
    if (opts.parallel) {
      cases.push({
        id: `pp.limt_build_${tag}_parallel`, batch: 1, iters: iters ?? 20, warmup: 2,
        note: "Kohaku PoseidonT3LeanIMT.fromLeavesParallel (default workers = min(cores,16), started per call)",
        run: () => PoseidonT3LeanIMT.fromLeavesParallel(ls, { workerUrl: opts.workerUrl }).then((t: any) => (sink = t)),
        check: async () => (await PoseidonT3LeanIMT.fromLeavesParallel(ls, { workerUrl: opts.workerUrl })).root.toString(),
      });
    }
  }
  let tree: any;
  cases.push({
    id: "pp.limt_proof_100k", batch: PP.proofIndices.length,
    setup: () => { tree = new PoseidonT3LeanIMT(leaves); },
    run: () => { for (const i of PP.proofIndices) sink = tree.generateProof(i); },
    check: () => {
      const p = tree.generateProof(PP.proofIndices[0]);
      return [p.root, p.leaf, p.index, ...p.siblings].map(String).join(",");
    },
  });
  const accountKeyHex = "0x" + PP.accountKey;
  const host = { keystore: { deriveAt: async () => accountKeyHex } } as any;
  const noteParams = (i: number) => ({ chainId: BigInt(PP.chainId), entrypointAddress: BigInt(PP.entrypoint), depositIndex: i });
  const secretsStr = (s: any) => [s.nullifier, s.salt, s.precommitment, s.nullifierHash].map(String).join(",");
  cases.push({
    id: "pp.note_secrets", batch: PP.depositIndices.length,
    note: "Kohaku SignatureSecretManager, default KeystoreEnvelopeSigner (privateKeyToAccount per signature), verifyDeterminism off, fresh manager per sample (no cache hits)",
    run: async () => {
      const sm = SignatureSecretManager({ host, verifyDeterminism: false });
      for (const i of PP.depositIndices) sink = await sm.getDepositSecrets(noteParams(i));
    },
    check: async () => secretsStr(await SignatureSecretManager({ host, verifyDeterminism: false }).getDepositSecrets(noteParams(3))),
  });
  const account = privateKeyToAccount(accountKeyHex as `0x${string}`);
  const signer = { signEnvelope: (env: any) => account.signTypedData(env) };
  cases.push({
    id: "pp.note_secrets_cached_account", batch: PP.depositIndices.length,
    note: "same, but with an EnvelopeSigner that reuses one viem account (no per-signature pubkey derivation)",
    run: async () => {
      const sm = SignatureSecretManager({ host, signer, verifyDeterminism: false });
      for (const i of PP.depositIndices) sink = await sm.getDepositSecrets(noteParams(i));
    },
    check: async () => secretsStr(await SignatureSecretManager({ host, signer, verifyDeterminism: false }).getDepositSecrets(noteParams(3))),
  });
  const notes = PP.commitments.map((c: any) => ({ balance: BigInt(c.value), label: BigInt(c.label), precommitment: BigInt(c.precommitment) }));
  cases.push({
    id: "pp.commitment", batch: notes.length,
    run: () => { for (const n of notes) sink = commitment(n); },
    check: () => notes.slice(0, 4).map((n: any) => commitment(n).toString()).join(","),
  });

  // ---------------- stealth ----------------
  const ephs: Uint8Array[] = ST.ephKeys.map(unhex); // the SDK wants bytes (a hex string would silently fall back to a random key)
  cases.push({
    id: "stealth.generate", batch: ephs.length,
    run: () => { for (const e of ephs) sink = generateStealthAddress({ stealthMetaAddressURI: ST.metaUri, ephemeralPrivateKey: e }); },
    check: () => ephs.slice(0, 4).map((e: Uint8Array) => {
      const g = generateStealthAddress({ stealthMetaAddressURI: ST.metaUri, ephemeralPrivateKey: e });
      return `${g.stealthAddress.toLowerCase()}:${g.ephemeralPublicKey.slice(2)}:${parseInt(g.viewTag.slice(2), 16)}`;
    }).join(","),
  });
  const anns = ST.announcements.map((a: any) => ({
    ephemeralPublicKey: "0x" + a.eph, userStealthAddress: a.addr, viewTag: "0x" + a.tag.toString(16).padStart(2, "0"),
  }));
  const scan = (n: number) => {
    const hits: number[] = [];
    for (let i = 0; i < n; i++) {
      const a = anns[i];
      if (checkStealthAddress({ ...a, schemeId: 1, spendingPublicKey: "0x" + ST.spendingPub, viewingPrivateKey: "0x" + ST.viewingKey })) hits.push(i);
    }
    return hits;
  };
  for (const [n, tag] of [[1000, "1k"], [10_000, "10k"]] as const) {
    cases.push({
      id: `stealth.scan_${tag}`, batch: 1, iters: n >= 10_000 ? 10 : undefined,
      note: "SDK checkStealthAddress per announcement; 1 % are ours",
      run: () => (sink = scan(n)),
      check: () => scan(n).join(","),
    });
  }

  // ---------------- aa ----------------
  const proveOutput = { proof: { pi_a: AA.proof.pi_a, pi_b: AA.proof.pi_b, pi_c: AA.proof.pi_c }, publicSignals: AA.proof.publicSignals };
  const gas = Object.fromEntries(Object.entries(AA.gas).map(([k, v]) => [k, BigInt(v as string)]));
  const buildOp = async () => {
    const signer = privateKeyToAccount(("0x" + AA.ephemeralKey) as `0x${string}`);
    const data = encodeFeeData({ recipient: AA.recipient, feeRecipient: AA.paymaster, fee: BigInt(AA.fee), refundRecipient: AA.refundRecipient });
    const adapterData = encodePrivacyPoolAdapterData({ processooor: AA.adapter, data } as any, proveOutput as any);
    const paymasterData = encodePaymasterData(AA.adapter, adapterData);
    return buildSignedUserOp({
      signer, chainId: AA.chainId, paymasterAddress: AA.paymaster, paymasterData, gas: gas as any,
      maxFeePerGas: BigInt(AA.maxFeePerGas), maxPriorityFeePerGas: BigInt(AA.maxPriorityFeePerGas),
    } as any);
  };
  cases.push({
    id: "aa.build_signed_userop", batch: 1,
    note: "privateKeyToAccount + encodeFeeData + encodePrivacyPoolAdapterData + encodePaymasterData + buildSignedUserOp",
    run: () => buildOp().then((o) => (sink = o)),
    check: async () => canonicalJson(await buildOp()),
  });

  void sink;
  return cases;
}

export function canonicalJson(o: any): string {
  const norm = (v: any): any =>
    Array.isArray(v) ? v.map(norm)
      : v && typeof v === "object" ? Object.fromEntries(Object.keys(v).sort().map((k) => [k, norm(v[k])]))
      : typeof v === "string" ? v.toLowerCase() : typeof v === "bigint" ? v.toString() : v;
  return JSON.stringify(norm(o));
}
