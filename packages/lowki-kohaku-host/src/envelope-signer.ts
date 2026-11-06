import type { EnvelopeSigner, ISecretManager, NoteEnvelope, SecretManagerParams } from "./kohaku-pp";
import { SignatureSecretManager } from "./kohaku-pp";
import { getAddress, isAddress, keccak256, recoverTypedDataAddress, stringToHex, type Address } from "viem";
import type { Hex } from "./protocol";
import { call, type WorkerTransport } from "./transport";

// Kohaku's note envelope standard (packages/privacy-pools/src/account/signature-keys.ts:65-147).
// Duplicated here on purpose: the signer refuses anything that is not exactly this envelope, so
// a changed Kohaku (or a caller other than SignatureSecretManager) cannot get the worker to sign
// some other typed data with the identity key. Rust rebuilds the same envelope independently.
export const NOTE_DOMAIN_NAME = "PrivacyPools Keychain";
export const NOTE_DOMAIN_VERSION = "1";
export const NOTE_DOMAIN_SALT = keccak256(stringToHex("privacypools"));
export const NOTE_PRIMARY_TYPE = "PrivacyPoolNote";
export const NOTE_TYPES = {
  PrivacyPoolNote: [
    { name: "depositIndex", type: "uint64" },
    { name: "secretIndex", type: "uint64" },
  ],
} as const;

const MAX_INDEX = 0xffff_ffff; // the worker takes the indices as JS numbers (u32 on the Rust side)

export class EnvelopeRejectedError extends Error {
  constructor(why: string) {
    super(`Note envelope rejected: ${why}`);
    this.name = "EnvelopeRejectedError";
  }
}

const sameKeys = (o: object, keys: string[]) => {
  const k = Object.keys(o).sort();
  const want = [...keys].sort();

  return k.length === want.length && k.every((v, i) => v === want[i]);
};

const toIndex = (v: unknown, name: string): number => {
  if (typeof v !== "bigint" && typeof v !== "number") throw new EnvelopeRejectedError(`${name} is not an integer`);

  const n = BigInt(v);

  if (n < 0n || n > BigInt(MAX_INDEX)) throw new EnvelopeRejectedError(`${name} out of range`);

  return Number(n);
};

/** The fields the worker needs, extracted from an envelope that matched Kohaku's standard exactly. */
export type ValidatedNoteEnvelope = {
  chainId: bigint;
  entrypoint: Address;
  depositIndex: number;
  secretIndex: number;
};

/** Checks that `envelope` is exactly Kohaku's PrivacyPoolNote envelope and returns its variable
 * fields; throws {@link EnvelopeRejectedError} otherwise. */
export function validateNoteEnvelope(envelope: NoteEnvelope): ValidatedNoteEnvelope {
  if (!envelope || typeof envelope !== "object") throw new EnvelopeRejectedError("not an object");

  if (!sameKeys(envelope, ["domain", "types", "primaryType", "message"])) throw new EnvelopeRejectedError("unexpected top-level fields");

  if (envelope.primaryType !== NOTE_PRIMARY_TYPE) throw new EnvelopeRejectedError(`primaryType ${String(envelope.primaryType)}`);

  const { domain, types, message } = envelope;

  if (!domain || !sameKeys(domain, ["name", "version", "chainId", "verifyingContract", "salt"])) {
    throw new EnvelopeRejectedError("domain fields differ from Kohaku's");
  }

  if (domain.name !== NOTE_DOMAIN_NAME) throw new EnvelopeRejectedError("domain.name");

  if (domain.version !== NOTE_DOMAIN_VERSION) throw new EnvelopeRejectedError("domain.version");

  if (typeof domain.salt !== "string" || domain.salt.toLowerCase() !== NOTE_DOMAIN_SALT) throw new EnvelopeRejectedError("domain.salt");

  const chainId = typeof domain.chainId === "number" || typeof domain.chainId === "bigint" ? BigInt(domain.chainId) : -1n;

  if (chainId <= 0n || chainId > BigInt(Number.MAX_SAFE_INTEGER)) throw new EnvelopeRejectedError("domain.chainId");

  if (typeof domain.verifyingContract !== "string" || !isAddress(domain.verifyingContract, { strict: false })) {
    throw new EnvelopeRejectedError("domain.verifyingContract");
  }

  if (!types || !sameKeys(types, [NOTE_PRIMARY_TYPE])) throw new EnvelopeRejectedError("types must only define PrivacyPoolNote");

  const fields = types.PrivacyPoolNote as readonly { name: string; type: string }[];
  const want = NOTE_TYPES.PrivacyPoolNote;

  if (!Array.isArray(fields) || fields.length !== want.length
    || fields.some((f, i) => !sameKeys(f, ["name", "type"]) || f.name !== want[i]!.name || f.type !== want[i]!.type)) {
    throw new EnvelopeRejectedError("PrivacyPoolNote fields differ from Kohaku's");
  }

  if (!message || !sameKeys(message, ["depositIndex", "secretIndex"])) throw new EnvelopeRejectedError("message fields");

  return {
    chainId,
    entrypoint: getAddress(domain.verifyingContract),
    depositIndex: toIndex(message.depositIndex, "depositIndex"),
    secretIndex: toIndex(message.secretIndex, "secretIndex"),
  };
}

export type LowkiEnvelopeSignerOptions = {
  accountIndex?: number;
  /**
   * Recover the signer address of every signature and require it to be the same address each
   * time (default true). The first recovered address is pinned per instance; it is public data
   * (the account's Privacy Pools identity address) and is exposed as `identityAddress`.
   */
  checkSigner?: boolean;
};

/**
 * Kohaku `EnvelopeSigner` that signs in the Lowki worker (`ppSignNoteEnvelope`). Only the
 * 65-byte per-note signature crosses into JS; the identity key `m/28784'/1'/{account}'` stays in
 * Rust. Inject it through {@link lowkiSecretManager}.
 */
export class LowkiEnvelopeSigner implements EnvelopeSigner {
  readonly #transport: WorkerTransport;
  readonly accountIndex: number;
  readonly #checkSigner: boolean;
  #identity: Address | undefined;

  constructor(transport: WorkerTransport, { accountIndex = 0, checkSigner = true }: LowkiEnvelopeSignerOptions = {}) {
    if (!Number.isInteger(accountIndex) || accountIndex < 0 || accountIndex >= 0x8000_0000) {
      throw new RangeError("accountIndex must be an integer in [0, 2^31)");
    }

    this.#transport = transport;
    this.accountIndex = accountIndex;
    this.#checkSigner = checkSigner;
  }

  /** The address that signed the notes so far (undefined before the first signature). */
  get identityAddress(): Address | undefined {
    return this.#identity;
  }

  async signEnvelope(envelope: NoteEnvelope): Promise<Hex> {
    const { chainId, entrypoint, depositIndex, secretIndex } = validateNoteEnvelope(envelope);
    const { signature } = await call(this.#transport, {
      type: "ppSignNoteEnvelope",
      accountIndex: this.accountIndex,
      chainId: chainId.toString(10),
      entrypoint,
      depositIndex,
      secretIndex,
    });

    if (!/^0x[0-9a-f]{130}$/i.test(signature)) throw new Error("lowki-worker returned a malformed signature");

    if (this.#checkSigner) {
      const who = await recoverTypedDataAddress({ ...envelope, signature });

      this.#identity ??= who;

      if (who !== this.#identity) throw new Error("lowki-worker signed a note with a different key than before");
    }

    return signature;
  }
}

/**
 * `secretManager` factory for `new PrivacyPoolsV1Protocol(host, { secretManager })`: Kohaku's own
 * SignatureSecretManager (nullifier/salt derivation unchanged) with the Lowki worker as its
 * envelope signer. `accountIndex` comes from the plugin params, so it always matches.
 */
export function lowkiSecretManager(
  transport: WorkerTransport,
  { verifyDeterminism = true, checkSigner = true, onSigner }: {
    /** Kohaku signs the session's first envelope twice to check RFC 6979 (default true). */
    verifyDeterminism?: boolean;
    checkSigner?: boolean;
    /** Receives the signer instance (e.g. to read `identityAddress` afterwards). */
    onSigner?: (signer: LowkiEnvelopeSigner) => void;
  } = {},
): (params: SecretManagerParams) => ISecretManager {
  return (params) => {
    const signer = new LowkiEnvelopeSigner(transport, { accountIndex: params.accountIndex ?? 0, checkSigner });

    onSigner?.(signer);

    return SignatureSecretManager({ ...params, signer, verifyDeterminism });
  };
}
