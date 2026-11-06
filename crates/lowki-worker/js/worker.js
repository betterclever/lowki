// Lowki key worker entry (bindings only). Load as a module Web Worker inside the Lowki
// iframe:  new Worker(new URL('./worker.js', import.meta.url), { type: 'module' })
// Protocol: postMessage({ id, request }) -> postMessage({ id, response }). See README.md.
// All logic, keys and errors live in Rust; this file only moves messages.
import init, { LowkiWorker } from '../pkg/web/lowki_worker.js';

const ready = init();
let worker;
const queue = [];
let busy = false;

async function pump() {
  if (busy) return;
  busy = true;
  try {
    await ready;
    worker ??= new LowkiWorker(self.LOWKI_AUTO_LOCK_MINUTES ?? 15);
    // One request at a time: the Rust core is single-threaded and refuses re-entry.
    while (queue.length) {
      const { id, request } = queue.shift();
      const response = await worker.handle(request);
      self.postMessage({ id, response });
    }
  } finally {
    busy = false;
  }
}

self.onmessage = (e) => {
  queue.push(e.data);
  pump();
};
