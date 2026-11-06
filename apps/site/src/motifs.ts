// Point sets the particles reform into, one per scene (plus the race's bucket grid).
// Each point: x, y in unit space (y up, square motifs inside [-1, 1]), kind, param.
import { rng, sampleMark, type MarkRaster } from "./mark";

export const N_MAX = 32768;
export const MOTIFS = 11; // 0..9 scenes, 10 = race buckets
export const BUCKET_MOTIF = 10;

type Draw = (g: CanvasRenderingContext2D, w: number, h: number) => void;

/** Rasterises a drawing and returns its filled pixels (as indices). */
function mask(w: number, h: number, draw: Draw): { pix: Int32Array; w: number; h: number } {
  const cv = document.createElement("canvas");
  cv.width = w;
  cv.height = h;
  const g = cv.getContext("2d", { willReadFrequently: true })!;
  g.fillStyle = "#fff";
  g.strokeStyle = "#fff";
  draw(g, w, h);
  const d = g.getImageData(0, 0, w, h).data;
  const out: number[] = [];
  for (let i = 0; i < w * h; i++) if (d[i * 4 + 3] > 127) out.push(i);
  return { pix: Int32Array.from(out), w, h };
}

/** n points from a mask; the canvas maps to x in [-sx, sx], y in [-sy, sy]. */
function fill(out: Float32Array, at: number, n: number, m: ReturnType<typeof mask>, sx: number, sy: number, kind: number, rand: () => number) {
  for (let j = 0; j < n; j++) {
    const p = m.pix[Math.floor(rand() * m.pix.length)];
    const x = (p % m.w) + rand(), y = Math.floor(p / m.w) + rand();
    const o = (at + j) * 4;
    out[o] = (x / m.w * 2 - 1) * sx;
    out[o + 1] = -(y / m.h * 2 - 1) * sy;
    out[o + 2] = kind;
    out[o + 3] = rand();
  }
}

function rr(g: CanvasRenderingContext2D, x: number, y: number, w: number, h: number, r: number) {
  g.beginPath();
  g.roundRect(x, y, w, h, r);
}

export function buildMotifs(mark: MarkRaster): Float32Array {
  const N = N_MAX;
  const all = new Float32Array(MOTIFS * N * 4);
  const rand = rng(0x5eed);
  const slot = (k: number) => all.subarray(k * N * 4, (k + 1) * N * 4);

  // 0 + 9: the lauki
  {
    const pts = sampleMark(mark, N, rand);
    const m = slot(0);
    const order = Array.from({ length: N }, (_, i) => i).sort(() => rand() - 0.5);
    for (let i = 0; i < N; i++) {
      const s = order[i];
      m[i * 4] = pts.xy[s * 2] * 1.9;
      m[i * 4 + 1] = -pts.xy[s * 2 + 1] * 1.9;
      m[i * 4 + 2] = pts.kind[s];
      m[i * 4 + 3] = rand();
    }
    slot(9).set(m);
  }

  // 1: a phone with a keyhole in it (keys stay on the device)
  {
    const S = 512;
    const phone = mask(S, S, (g) => {
      g.lineWidth = 14;
      rr(g, S * 0.27, S * 0.06, S * 0.46, S * 0.88, 46);
      g.stroke();
      g.fillRect(S * 0.44, S * 0.11, S * 0.12, 8);
    });
    const key = mask(S, S, (g) => {
      g.beginPath();
      g.arc(S / 2, S * 0.43, S * 0.085, 0, Math.PI * 2);
      g.fill();
      g.beginPath();
      g.moveTo(S * 0.465, S * 0.47);
      g.lineTo(S * 0.535, S * 0.47);
      g.lineTo(S * 0.56, S * 0.66);
      g.lineTo(S * 0.44, S * 0.66);
      g.closePath();
      g.fill();
    });
    const m = slot(1);
    const a = Math.floor(N * 0.42), b = Math.floor(N * 0.4);
    fill(m, 0, a, phone, 1, 1, 0, rand);
    fill(m, a, b, key, 1, 1, 1, rand);
    for (let i = a + b; i < N; i++) {
      // a halo of light around the keyhole
      const r = 0.2 + Math.pow(rand(), 0.6) * 0.9, t = rand() * Math.PI * 2;
      m.set([Math.cos(t) * r * 0.8, -0.12 + Math.sin(t) * r, 2, rand()], i * 4);
    }
  }

  // 2: one import: "{ }" wide braces with a line between
  {
    const W = 1024, H = 512;
    const br = mask(W, H, (g) => {
      g.font = `700 ${H * 0.92}px "IBM Plex Mono", monospace`;
      g.textAlign = "center";
      g.textBaseline = "middle";
      g.fillText("{", W * 0.16, H * 0.5);
      g.fillText("}", W * 0.84, H * 0.5);
    });
    const line = mask(W, H, (g) => {
      rr(g, W * 0.3, H * 0.44, W * 0.4, H * 0.12, H * 0.06);
      g.fill();
    });
    const m = slot(2);
    const a = Math.floor(N * 0.6);
    fill(m, 0, a, br, 1, 0.5, 0, rand);
    fill(m, a, N - a, line, 1, 0.5, 1, rand);
  }

  // 3: four app tiles, each with its own person in it (a wallet per app)
  {
    const S = 256;
    const tile = mask(S, S, (g) => {
      rr(g, 8, 8, S - 16, S - 16, 52);
      g.fill();
      g.globalCompositeOperation = "destination-out";
      g.beginPath();
      g.arc(S / 2, S * 0.4, S * 0.13, 0, Math.PI * 2);
      g.fill();
      g.beginPath();
      g.ellipse(S / 2, S * 0.82, S * 0.26, S * 0.2, 0, Math.PI, 0);
      g.fill();
    });
    const m = slot(3);
    const per = Math.floor(N / 4);
    const pos = [[-0.5, 0.5], [0.5, 0.5], [-0.5, -0.5], [0.5, -0.5]];
    for (let k = 0; k < 4; k++) {
      const at = k * per, n = k === 3 ? N - at : per;
      fill(m, at, n, tile, 0.42, 0.42, k, rand);
      for (let j = 0; j < n; j++) {
        m[(at + j) * 4] += pos[k][0];
        m[(at + j) * 4 + 1] += pos[k][1];
      }
    }
  }

  // 4: private payments: analytic (entry lane, exit lane, phase)
  {
    const m = slot(4);
    for (let i = 0; i < N; i++) m.set([rand() * 2 - 1, rand() * 2 - 1, 0, rand()], i * 4);
  }

  // 5: a name bursting into addresses
  {
    const W = 1024, H = 192;
    const txt = mask(W, H, (g) => {
      g.font = `700 ${H * 0.62}px "Bricolage Grotesque", sans-serif`;
      g.textAlign = "center";
      g.textBaseline = "middle";
      g.fillText("alice.lowki.eth", W / 2, H * 0.52);
    });
    const m = slot(5);
    const a = Math.floor(N * 0.5);
    fill(m, 0, a, txt, 1, H / W, 0, rand);
    const RAYS = 48;
    for (let i = a; i < N; i++) {
      const ray = Math.floor(rand() * RAYS);
      m.set([ray / RAYS * Math.PI * 2 + 0.02 * (rand() - 0.5), rand(), 1, rand()], i * 4);
    }
  }

  // 6: race: analytic ring (uses 0 and 10 for its other phases)
  {
    const m = slot(6);
    for (let i = 0; i < N; i++) m.set([rand(), rand(), 0, rand()], i * 4);
  }

  // 7: radar rings (who sees what)
  {
    const m = slot(7);
    for (let i = 0; i < N; i++) {
      const ring = Math.floor(rand() * 5);
      const r = 0.2 + ring * 0.2 + (rand() - 0.5) * 0.012;
      const t = rand() * Math.PI * 2;
      m.set([Math.cos(t) * r, Math.sin(t) * r, ring, t], i * 4);
    }
  }

  // 8: benchmark bars, four pairs (Lowki, reference), real ratios from bench/RESULTS.md
  {
    const pairs = [[168, 603], [526, 3240], [0.887, 4.18], [768, 804]];
    const m = slot(8);
    let o = 0;
    const per = Math.floor(N / 8);
    pairs.forEach(([l, r], k) => {
      const max = Math.max(l, r);
      [l, r].forEach((v, side) => {
        const n = k === 3 && side === 1 ? N - o : per;
        const h = (v / max) * 1.6;
        const x0 = -0.9 + k * 0.48 + side * 0.17;
        for (let j = 0; j < n; j++, o++) m.set([x0 + rand() * 0.14, -0.8 + rand() * h, side, rand()], o * 4);
      });
    });
  }

  // 10: Pippenger buckets
  {
    const m = slot(10);
    const W = 16, R = 8;
    for (let i = 0; i < N; i++) {
      const bx = Math.floor(rand() * W), by = Math.floor(rand() * R);
      const a = rand() * Math.PI * 2, r = Math.sqrt(rand()) * 0.045;
      m.set([-0.92 + (bx / (W - 1)) * 1.84 + Math.cos(a) * r, -0.75 + (by / (R - 1)) * 1.5 + Math.sin(a) * r, by, i / N], i * 4);
    }
  }
  return all;
}
