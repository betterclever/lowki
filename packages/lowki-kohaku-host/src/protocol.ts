// Wire types for the subset of the Lowki worker protocol this package uses.
// Requests and responses are plain objects tagged by `type` (camelCase); the worker is
// `class LowkiWorker { constructor(autoLockMinutes?: number); handle(request): Promise<object> }`
// built from crates/lowki-worker with wasm-bindgen. Every key operation runs in Rust.

export type Hex = `0x${string}`;

export type ImportMnemonicRequest = {
  type: "importMnemonic";
  mnemonic: string;
  /** 32-byte passkey PRF output that wraps the seed. */
  prfOutput: Uint8Array;
};

/** Derives the secp256k1 key at `path`. Rust only allows paths strictly below
 * `m/28784'/1'` plus paths registered with `kohakuAllowPath`. */
export type KohakuDeriveAtRequest = { type: "kohakuDeriveAt"; path: string };

/** Registers one extra single path (Kohaku delegation path) for `kohakuDeriveAt`. */
export type KohakuAllowPathRequest = { type: "kohakuAllowPath"; path: string };

/** Signs Kohaku's PrivacyPoolNote EIP-712 envelope in Rust with the key at
 * `m/28784'/1'/{accountIndex}'`. Rust rebuilds the domain/types itself; only the
 * variable fields cross. */
export type PpSignNoteEnvelopeRequest = {
  type: "ppSignNoteEnvelope";
  accountIndex: number;
  /** Decimal string. */
  chainId: string;
  /** 0x + 40 hex. */
  entrypoint: Hex;
  depositIndex: number;
  secretIndex: number;
};

export type WorkerRequest =
  | ImportMnemonicRequest
  | KohakuDeriveAtRequest
  | KohakuAllowPathRequest
  | PpSignNoteEnvelopeRequest;

export type WalletCreatedResponse = {
  type: "walletCreated";
  wrappedBlob: Uint8Array;
  wrappedPasskeyShare: Uint8Array;
  serverShare: Uint8Array;
  recoveryShare: Uint8Array;
};
export type KohakuKeyResponse = { type: "kohakuKey"; key: Hex };
export type NoteSignatureResponse = { type: "noteSignature"; signature: Hex };
export type OkResponse = { type: "ok" };
export type ErrorResponse = { type: "error"; code: string; message: string };

export type WorkerResponse =
  | WalletCreatedResponse
  | KohakuKeyResponse
  | NoteSignatureResponse
  | OkResponse
  | ErrorResponse;

/** Maps each request to the success response it expects. */
export type ResponseFor<R extends WorkerRequest> =
  R extends ImportMnemonicRequest ? WalletCreatedResponse
  : R extends KohakuDeriveAtRequest ? KohakuKeyResponse
  : R extends KohakuAllowPathRequest ? OkResponse
  : R extends PpSignNoteEnvelopeRequest ? NoteSignatureResponse
  : never;

const EXPECTED: Record<WorkerRequest["type"], WorkerResponse["type"]> = {
  importMnemonic: "walletCreated",
  kohakuDeriveAt: "kohakuKey",
  kohakuAllowPath: "ok",
  ppSignNoteEnvelope: "noteSignature",
};

/** A worker `{type:"error"}` response, or a response of the wrong type. */
export class LowkiWorkerError extends Error {
  constructor(readonly code: string, message: string) {
    super(`lowki-worker ${code}: ${message}`);
    this.name = "LowkiWorkerError";
  }
}

/** Raises a worker error response as {@link LowkiWorkerError} and checks the response tag. */
export function expectResponse<R extends WorkerRequest>(req: R, resp: unknown): ResponseFor<R> {
  const r = resp as WorkerResponse | undefined;

  if (!r || typeof r !== "object" || typeof (r as { type?: unknown }).type !== "string") {
    throw new LowkiWorkerError("invalidResponse", `malformed response to ${req.type}`);
  }

  if (r.type === "error") throw new LowkiWorkerError(r.code, r.message);

  if (r.type !== EXPECTED[req.type]) {
    throw new LowkiWorkerError("invalidResponse", `expected ${EXPECTED[req.type]} for ${req.type}, got ${r.type}`);
  }

  return r as ResponseFor<R>;
}
