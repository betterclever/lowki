// Picks the worker implementation for Node scripts/tests.
//   LOWKI_WORKER=real  -> crates/lowki-worker/pkg/node/lowki_worker.js (wasm-bindgen --target nodejs)
//   LOWKI_WORKER=stub  -> test/stub-worker.ts (TEST-ONLY JS stub; keys live in JS)
// Default: real if the wasm package has been built, otherwise stub.
import { createRequire } from "node:module";
import { existsSync } from "node:fs";
import { fileURLToPath } from "node:url";
import path from "node:path";
import type { LowkiWorkerLike } from "../src/transport";

const here = path.dirname(fileURLToPath(import.meta.url));

export const REAL_WORKER_PATH = process.env.LOWKI_WORKER_PKG
  ?? path.resolve(here, "../../../crates/lowki-worker/pkg/node/lowki_worker.js");

export type LoadedWorker = { worker: LowkiWorkerLike; kind: "real" | "stub"; source: string };

export async function loadWorker(autoLockMinutes?: number): Promise<LoadedWorker> {
  const want = process.env.LOWKI_WORKER ?? (existsSync(REAL_WORKER_PATH) ? "real" : "stub");

  if (want === "real") {
    const mod = createRequire(import.meta.url)(REAL_WORKER_PATH) as {
      LowkiWorker: new (autoLockMinutes?: number) => LowkiWorkerLike;
    };

    return { worker: new mod.LowkiWorker(autoLockMinutes), kind: "real", source: REAL_WORKER_PATH };
  }

  if (want !== "stub") throw new Error(`LOWKI_WORKER must be "real" or "stub", got ${want}`);

  const { StubLowkiWorker } = await import("../test/stub-worker");

  return { worker: new StubLowkiWorker(), kind: "stub", source: "test/stub-worker.ts (TEST-ONLY)" };
}
