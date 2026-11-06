// Deterministic byte source installed by rng-hook.mjs. gen.ts sets globalThis.__lowkiRng to
// a function (n) => Uint8Array; every call is also appended to globalThis.__lowkiRngLog.
export function getRandomBytes(numBytes) {
  const src = globalThis.__lowkiRng;
  if (!src) throw new Error('replay csprng: no source installed');
  const out = src(numBytes);
  (globalThis.__lowkiRngLog ??= []).push(Uint8Array.from(out));
  return out;
}
