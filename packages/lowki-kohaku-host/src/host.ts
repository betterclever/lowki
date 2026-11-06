import type { EthereumProvider } from "@kohaku-eth/provider";
import type { ExternalSyncProvider, Host, Network, Storage } from "@kohaku-eth/plugins";
import type { ISecretManager, SecretManagerParams } from "./kohaku-pp";
import { lowkiSecretManager, type LowkiEnvelopeSigner } from "./envelope-signer";
import { LowkiKeystore } from "./keystore";
import type { WorkerTransport } from "./transport";

export type KeystoreWiring =
  /** Mode 1: `keystore.deriveAt` -> `kohakuDeriveAt`; Kohaku's default KeystoreEnvelopeSigner
   * signs in JS with the identity key it pulled from the worker. */
  | "deriveAt"
  /** Mode 2: a `LowkiEnvelopeSigner` is injected as Kohaku's `secretManager`; signing happens in
   * Rust and the keystore refuses every Privacy Pools path. */
  | "envelopeSigner";

export type LowkiHostOptions = {
  transport: WorkerTransport;
  storage: Storage;
  network: Network;
  provider: EthereumProvider;
  externalSyncProvider?: ExternalSyncProvider;
  wiring?: KeystoreWiring;
  /** Mode 2 only: Kohaku's determinism double-sign check (default true). */
  verifyDeterminism?: boolean;
  onSigner?: (signer: LowkiEnvelopeSigner) => void;
};

export type LowkiHost = {
  host: Host;
  keystore: LowkiKeystore;
  /** Pass to `new PrivacyPoolsV1Protocol(host, { secretManager, ... })`. Undefined in mode 1
   * (Kohaku's default SignatureSecretManager + KeystoreEnvelopeSigner is used). */
  secretManager?: (params: SecretManagerParams) => ISecretManager;
};

/** Assembles a Kohaku `Host` on the Lowki worker. */
export function createLowkiHost({
  transport, storage, network, provider, externalSyncProvider, wiring = "envelopeSigner", verifyDeterminism, onSigner,
}: LowkiHostOptions): LowkiHost {
  const keystore = new LowkiKeystore(transport, { mode: wiring === "envelopeSigner" ? "signer-only" : "derive" });
  const host: Host = {
    network,
    storage,
    keystore,
    provider,
    ...(externalSyncProvider ? { externalSyncProvider } : {}),
  };

  return {
    host,
    keystore,
    secretManager: wiring === "envelopeSigner"
      ? lowkiSecretManager(transport, { verifyDeterminism, onSigner })
      : undefined,
  };
}
