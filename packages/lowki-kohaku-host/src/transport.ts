import { expectResponse, type ResponseFor, type WorkerRequest, type WorkerResponse } from "./protocol";

/** How the host reaches the Lowki worker. Every implementation returns the worker's raw
 * response; {@link call} raises error responses and checks the response tag. */
export interface WorkerTransport {
  request(req: WorkerRequest): Promise<WorkerResponse>;
}

/** Sends `req` and returns the typed success response, throwing `LowkiWorkerError` otherwise. */
export async function call<R extends WorkerRequest>(transport: WorkerTransport, req: R): Promise<ResponseFor<R>> {
  return expectResponse(req, await transport.request(req));
}

// serde-wasm-bindgen serializes Rust maps as JS `Map` unless told otherwise; accept both.
function normalize(value: unknown): WorkerResponse {
  if (value instanceof Map) return Object.fromEntries(value) as WorkerResponse;

  return value as WorkerResponse;
}

/** Shape of the wasm-bindgen class exported by crates/lowki-worker. */
export interface LowkiWorkerLike {
  handle(request: object): Promise<unknown>;
}

/** (b) In-process transport: calls a `LowkiWorker` instance in the same JS realm (Node, tests).
 * The wasm instance still owns the keys; this only removes the postMessage hop. */
export class InProcessTransport implements WorkerTransport {
  constructor(private readonly worker: LowkiWorkerLike) {}

  async request(req: WorkerRequest): Promise<WorkerResponse> {
    return normalize(await this.worker.handle(req));
  }
}

/** Message envelopes for the dedicated Web Worker protocol. */
export type WorkerRequestMessage = { lowki: 1; id: number; request: WorkerRequest };
export type WorkerResponseMessage = { lowki: 1; id: number; response: WorkerResponse };

/** Minimal surface of a dedicated `Worker` (or `MessagePort`). */
export interface PostMessageEndpoint {
  postMessage(message: unknown): void;
  addEventListener(type: "message", listener: (event: { data: unknown }) => void): void;
  addEventListener(type: "error", listener: (event: unknown) => void): void;
  removeEventListener(type: "message", listener: (event: { data: unknown }) => void): void;
  removeEventListener(type: "error", listener: (event: unknown) => void): void;
}

/** (a) Browser transport: talks to a dedicated Web Worker that hosts the wasm `LowkiWorker`
 * (see {@link serveLowkiWorker} for the worker side). Requests carry an id so concurrent
 * calls resolve independently; responses arrive by structured clone. */
export class WebWorkerTransport implements WorkerTransport {
  private nextId = 1;
  private readonly pending = new Map<number, { resolve: (r: WorkerResponse) => void; reject: (e: unknown) => void }>();

  constructor(private readonly endpoint: PostMessageEndpoint, private readonly timeoutMs = 60_000) {
    endpoint.addEventListener("message", this.onMessage);
    endpoint.addEventListener("error", this.onError);
  }

  private readonly onMessage = (event: { data: unknown }) => {
    const msg = event.data as Partial<WorkerResponseMessage> | null;

    if (!msg || msg.lowki !== 1 || typeof msg.id !== "number") return;

    const entry = this.pending.get(msg.id);

    if (!entry) return;

    this.pending.delete(msg.id);
    entry.resolve(normalize(msg.response));
  };

  private readonly onError = (event: unknown) => {
    const err = new Error(`lowki worker error: ${String((event as { message?: string })?.message ?? event)}`);

    for (const { reject } of this.pending.values()) reject(err);
    this.pending.clear();
  };

  request(req: WorkerRequest): Promise<WorkerResponse> {
    const id = this.nextId++;

    return new Promise<WorkerResponse>((resolve, reject) => {
      const timer = setTimeout(() => {
        if (this.pending.delete(id)) reject(new Error(`lowki worker request ${req.type} timed out`));
      }, this.timeoutMs);

      this.pending.set(id, {
        resolve: (r) => { clearTimeout(timer); resolve(r); },
        reject: (e) => { clearTimeout(timer); reject(e); },
      });
      this.endpoint.postMessage({ lowki: 1, id, request: req } satisfies WorkerRequestMessage);
    });
  }

  /** Stops listening and rejects anything still in flight. */
  close(): void {
    this.endpoint.removeEventListener("message", this.onMessage);
    this.endpoint.removeEventListener("error", this.onError);
    this.onError(new Error("transport closed"));
  }
}

/** Worker-side counterpart of {@link WebWorkerTransport}: call it inside the dedicated worker
 * script with the worker global scope and a `LowkiWorker` instance. A thrown exception becomes
 * an `{type:"error", code:"internal"}` response so the caller never hangs. */
export function serveLowkiWorker(
  scope: { postMessage(message: unknown): void; addEventListener(type: "message", l: (e: { data: unknown }) => void): void },
  worker: LowkiWorkerLike,
): void {
  scope.addEventListener("message", async (event) => {
    const msg = event.data as Partial<WorkerRequestMessage> | null;

    if (!msg || msg.lowki !== 1 || typeof msg.id !== "number" || !msg.request) return;

    let response: WorkerResponse;

    try {
      response = normalize(await worker.handle(msg.request));
    } catch (e) {
      response = { type: "error", code: "internal", message: e instanceof Error ? e.message : String(e) };
    }

    scope.postMessage({ lowki: 1, id: msg.id, response } satisfies WorkerResponseMessage);
  });
}
