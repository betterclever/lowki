import type { Keystore } from "@kohaku-eth/plugins";
import { parseDerivationPath } from "@kohaku-eth/plugins";
import type { Hex } from "./protocol";
import { call, type WorkerTransport } from "./transport";

/** Kohaku's PRIVACY_POOLS_KEYSTORE_PREFIX (packages/privacy-pools/src/account/keys.ts). */
export const PRIVACY_POOLS_PREFIX = "m/28784'/1'";

const PP_PREFIX_INDICES = parseDerivationPath(PRIVACY_POOLS_PREFIX);

export type LowkiKeystoreMode =
  /** Mode 1: Kohaku's Host contract. `deriveAt` forwards to the worker's `kohakuDeriveAt`, so the
   * Privacy Pools identity key `m/28784'/1'/{account}'` (and any HD child) crosses into JS. */
  | "derive"
  /** Mode 2: used together with `LowkiEnvelopeSigner`. Every path below `m/28784'/1'` is refused
   * in JS, so if anything in the plugin still asks for the identity key (a misconfigured
   * secretManager, the legacy HD `SecretManager`) it fails loudly instead of pulling the key out
   * of Rust. Only paths registered with {@link LowkiKeystore.allowPath} (delegation paths) are
   * forwarded. */
  | "signer-only";

const isBelowPrivacyPools = (path: string): boolean => {
  const idx = parseDerivationPath(path);

  return idx.length > PP_PREFIX_INDICES.length && PP_PREFIX_INDICES.every((v, i) => v === idx[i]);
};

export class LowkiKeystorePathError extends Error {
  constructor(path: string, why: string) {
    super(`Lowki keystore refused ${path}: ${why}`);
    this.name = "LowkiKeystorePathError";
  }
}

/**
 * Kohaku `Keystore` backed by the Lowki worker. The scope is enforced in Rust (paths strictly
 * below `m/28784'/1'` plus paths registered with `kohakuAllowPath`); this class adds an
 * identical JS-side pre-check so a bad path fails before a round-trip, and in `signer-only`
 * mode a stricter one. Kohaku itself wraps this again with `scopeKeystore` (plugin/base.ts:128).
 */
export class LowkiKeystore implements Keystore {
  readonly _brand = "Keystore" as const;
  readonly #transport: WorkerTransport;
  readonly #mode: LowkiKeystoreMode;
  readonly #allowed = new Set<string>();

  constructor(transport: WorkerTransport, { mode = "derive" }: { mode?: LowkiKeystoreMode } = {}) {
    this.#transport = transport;
    this.#mode = mode;
  }

  get mode(): LowkiKeystoreMode {
    return this.#mode;
  }

  /**
   * Registers a single extra path (e.g. the `delegation.path` passed to Kohaku's
   * `prepareUnshield({ mode: 'paymaster', delegation: { mode: 'deterministic', path } })`) with
   * the worker. Call it before the plugin needs the key: Kohaku adds the path to its own JS scope
   * on its own, but Rust only learns it from here.
   */
  async allowPath(path: string): Promise<void> {
    parseDerivationPath(path); // reject malformed paths before they reach the worker

    if (this.#mode === "signer-only" && isBelowPrivacyPools(path)) {
      throw new LowkiKeystorePathError(path, "paths below m/28784'/1' are never released in signer-only mode");
    }

    await call(this.#transport, { type: "kohakuAllowPath", path });
    this.#allowed.add(path);
  }

  async deriveAt(path: string): Promise<Hex> {
    const below = isBelowPrivacyPools(path); // also validates the path syntax

    if (this.#mode === "signer-only") {
      if (below) {
        throw new LowkiKeystorePathError(path, "signer-only mode: Privacy Pools keys stay in the worker; inject LowkiEnvelopeSigner");
      }

      if (!this.#allowed.has(path)) throw new LowkiKeystorePathError(path, "not registered with allowPath()");
    } else if (!below && !this.#allowed.has(path)) {
      throw new LowkiKeystorePathError(path, "outside m/28784'/1' and not registered with allowPath()");
    }

    const { key } = await call(this.#transport, { type: "kohakuDeriveAt", path });

    if (!/^0x[0-9a-f]{64}$/i.test(key)) throw new Error("lowki-worker returned a malformed key");

    return key;
  }
}
