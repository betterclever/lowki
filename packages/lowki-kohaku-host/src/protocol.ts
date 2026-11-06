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

/** Note coordinates as the worker takes them (`note` in crates/lowki-worker/README.md). */
export type PpNoteRef = {
  accountIndex: number;
  /** Decimal string. */
  chainId: string;
  entrypoint: Hex;
  depositIndex: number;
  secretIndex: number;
};

/** Public hashes of one note (decimal strings); secrets stay in Rust. */
export type PpNoteRequest = { type: "ppNote"; note: PpNoteRef; value?: string; label?: string };

/** Entrypoint `deposit(precommitment)` calldata for a native-ETH shield of `value` wei. */
export type PpShieldRequest = { type: "ppShield"; note: PpNoteRef; value: string };

/** Circuit artifacts (sha256-pinned by lowki-groth16). */
export type PpLoadCircuitsRequest = {
  type: "ppLoadCircuits";
  withdrawWasm: Uint8Array;
  withdrawZkey: Uint8Array;
  commitmentWasm: Uint8Array;
  commitmentZkey: Uint8Array;
  useGpu: boolean;
};

export type PpMerkleProof = { root: string; index: number | string; siblings: string[] };

/** Withdraw proof; Rust derives the existing note's and the change note's (secretIndex + 1) secrets. */
export type PpProveWithdrawRequest = {
  type: "ppProveWithdraw";
  note: PpNoteRef;
  label: string;
  existingValue: string;
  withdrawnValue: string;
  context: { context: string } | { withdrawal: { processooor: Hex; data: Hex }; scope: string };
  stateProof: PpMerkleProof;
  aspProof: PpMerkleProof;
};

/** Commitment proof (ragequit). */
export type PpProveCommitmentRequest = { type: "ppProveCommitment"; note: PpNoteRef; value: string; label: string };

export type AppAccountRequest = { type: "appAccount"; origin: string };

export type SignTransactionRequest = {
  type: "signTransaction";
  origin: string;
  tx: {
    type: "eip1559";
    chainId: string;
    nonce: string;
    maxPriorityFeePerGas: string;
    maxFeePerGas: string;
    gas: string;
    to?: Hex;
    value?: string;
    data?: Hex;
  };
};

export type WorkerRequest =
  | ImportMnemonicRequest
  | KohakuDeriveAtRequest
  | KohakuAllowPathRequest
  | PpSignNoteEnvelopeRequest
  | PpNoteRequest
  | PpShieldRequest
  | PpLoadCircuitsRequest
  | PpProveWithdrawRequest
  | PpProveCommitmentRequest
  | AppAccountRequest
  | SignTransactionRequest;

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
export type PpNoteResponse = { type: "ppNote"; precommitment: string; nullifierHash: string; commitment?: string };
export type PpShieldResponse = { type: "ppShield"; precommitment: string; tx: { to: Hex; data: Hex; value: string } };
export type CircuitsLoadedResponse = { type: "circuitsLoaded"; circuits: string[]; witnessBackend: string; gpu: boolean; gpuError?: string };
/** `proof` is snarkjs-shaped (`pi_a`, `pi_b`, `pi_c`); `solidity` is `{pA, pB, pC, pubSignals}`. */
export type ProofResponse = {
  type: "proof";
  circuit: "withdraw" | "commitment";
  proof: { pi_a: string[]; pi_b: string[][]; pi_c: string[]; protocol?: string; curve?: string };
  publicSignals: string[];
  solidity: unknown;
  backend: "webgpu" | "cpu";
  ms: number;
};
export type AppAccountResponse = { type: "appAccount"; origin: string; address: Hex; pairwiseUserId: string; appIndex: number };
export type SignedTransactionResponse = { type: "signedTransaction"; address: Hex; rawTransaction: Hex; hash: Hex };
export type ErrorResponse = { type: "error"; code: string; message: string };

export type WorkerResponse =
  | WalletCreatedResponse
  | KohakuKeyResponse
  | NoteSignatureResponse
  | OkResponse
  | PpNoteResponse
  | PpShieldResponse
  | CircuitsLoadedResponse
  | ProofResponse
  | AppAccountResponse
  | SignedTransactionResponse
  | ErrorResponse;

/** Maps each request to the success response it expects. */
export type ResponseFor<R extends WorkerRequest> =
  R extends ImportMnemonicRequest ? WalletCreatedResponse
  : R extends KohakuDeriveAtRequest ? KohakuKeyResponse
  : R extends KohakuAllowPathRequest ? OkResponse
  : R extends PpSignNoteEnvelopeRequest ? NoteSignatureResponse
  : R extends PpNoteRequest ? PpNoteResponse
  : R extends PpShieldRequest ? PpShieldResponse
  : R extends PpLoadCircuitsRequest ? CircuitsLoadedResponse
  : R extends PpProveWithdrawRequest ? ProofResponse
  : R extends PpProveCommitmentRequest ? ProofResponse
  : R extends AppAccountRequest ? AppAccountResponse
  : R extends SignTransactionRequest ? SignedTransactionResponse
  : never;

const EXPECTED: Record<WorkerRequest["type"], WorkerResponse["type"]> = {
  importMnemonic: "walletCreated",
  kohakuDeriveAt: "kohakuKey",
  kohakuAllowPath: "ok",
  ppSignNoteEnvelope: "noteSignature",
  ppNote: "ppNote",
  ppShield: "ppShield",
  ppLoadCircuits: "circuitsLoaded",
  ppProveWithdraw: "proof",
  ppProveCommitment: "proof",
  appAccount: "appAccount",
  signTransaction: "signedTransaction",
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
