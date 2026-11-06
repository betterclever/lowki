// Shared timing harness (Chrome and Node). One "sample" = one call of `run()`, which does
// `batch` operations; reported times are per operation (sample / batch).

export interface Case {
  id: string; // shared with the Rust side; results are joined on it
  batch: number; // operations per sample
  setup?: () => unknown | Promise<unknown>; // untimed, once
  run: () => unknown | Promise<unknown>; // timed
  check?: () => string | Promise<string>; // untimed: canonical output for the equivalence check
  warmup?: number; // fixed number of warm-up samples (default: adaptive, >= 3 and >= 300 ms)
  iters?: number; // fixed number of samples (default: adaptive, 10..200 within ~2.5 s)
  note?: string;
}

export interface Stat {
  id: string;
  batch: number;
  n: number;
  warmup: number;
  median_ms: number; // per op
  p90_ms: number; // per op
  min_ms: number;
  mean_ms: number;
  check?: string;
  extra?: Record<string, unknown>;
  note?: string;
  error?: string;
}

const now = () => performance.now();

export function stats(samples: number[], batch: number) {
  const s = [...samples].sort((a, b) => a - b);
  const q = (p: number) => s[Math.min(s.length - 1, Math.ceil(p * s.length) - 1)];
  return {
    median_ms: (s.length % 2 ? s[(s.length - 1) / 2] : (s[s.length / 2 - 1] + s[s.length / 2]) / 2) / batch,
    p90_ms: q(0.9) / batch,
    min_ms: s[0] / batch,
    mean_ms: s.reduce((a, b) => a + b, 0) / s.length / batch,
  };
}

export async function runCase(c: Case, log: (s: string) => void = () => {}): Promise<Stat> {
  try {
    if (c.setup) await c.setup();
    let w = 0;
    const tw = now();
    const minW = c.warmup ?? 3;
    let fastest = Infinity;
    while (w < minW || (c.warmup === undefined && now() - tw < 300 && w < 50)) {
      const t = now();
      await c.run();
      fastest = Math.min(fastest, now() - t);
      w++;
    }
    // Browser timers are coarse (5 µs when cross-origin isolated): repeat very fast cases
    // so that one sample takes >= ~2 ms. Applied identically to the TS and wasm sides.
    const reps = c.iters === undefined && fastest < 2 ? Math.min(1000, Math.ceil(2 / Math.max(fastest, 0.005))) : 1;
    const samples: number[] = [];
    const t0 = now();
    const maxN = c.iters ?? 200;
    const minN = c.iters ?? 10;
    while (samples.length < minN || (samples.length < maxN && now() - t0 < 2500)) {
      const t = now();
      for (let r = 0; r < reps; r++) await c.run();
      samples.push(now() - t);
    }
    const check = c.check ? await c.check() : undefined;
    const batch = c.batch * reps;
    const st: Stat = { id: c.id, batch, n: samples.length, warmup: w, ...stats(samples, batch), check, note: c.note };
    log(`${c.id}: median ${fmt(st.median_ms)} p90 ${fmt(st.p90_ms)} per op (n=${st.n}, batch=${c.batch}x${reps})`);
    return st;
  } catch (e: any) {
    log(`${c.id}: ERROR ${e?.stack ?? e}`);
    return { id: c.id, batch: c.batch, n: 0, warmup: 0, median_ms: NaN, p90_ms: NaN, min_ms: NaN, mean_ms: NaN, error: String(e?.message ?? e) };
  }
}

export function fmt(ms: number): string {
  if (!isFinite(ms)) return String(ms);
  if (ms >= 1000) return (ms / 1000).toFixed(2) + " s";
  if (ms >= 1) return ms.toFixed(2) + " ms";
  return (ms * 1000).toFixed(1) + " µs";
}
