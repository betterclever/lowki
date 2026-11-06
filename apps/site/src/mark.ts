// Rasterises the Lowki mark (paths from design/logo/lowki-mark-night.svg, injected at build
// time) into the textures the WebGPU renderers use:
//   layers  rgba8: r = gourd body, g = lime rim, b = keyhole, a = stem + tendril
//   field   rgba8: r = distance inside the body, g = wide light blur, b = narrow light blur,
//                  a = distance outside the body
// and samples points on it for the proof visualisation.
import { MARK } from "virtual:mark";

export interface MarkRaster {
  w: number;
  h: number;
  /** mark-space box the raster covers: [x, y, w, h] in SVG units */
  box: [number, number, number, number];
  layers: Uint8Array;
  field: Uint8Array;
  /** px distance that maps to 1.0 in field.r / field.a */
  dmax: number;
}

const PAD = 7; // SVG units of room around the mark for light spill

export function rasterMark(scale = 8): MarkRaster {
  const [vx, vy, vw, vh] = MARK.viewBox as number[];
  const box: [number, number, number, number] = [vx - PAD, vy - PAD, vw + 2 * PAD, vh + 2 * PAD];
  const w = Math.ceil(box[2] * scale);
  const h = Math.ceil(box[3] * scale);
  const cv = document.createElement("canvas");
  cv.width = w;
  cv.height = h;
  const g = cv.getContext("2d", { willReadFrequently: true })!;
  const layer = (draw: (g: CanvasRenderingContext2D) => void): Uint8ClampedArray => {
    g.setTransform(1, 0, 0, 1, 0, 0);
    g.clearRect(0, 0, w, h);
    g.setTransform(scale, 0, 0, scale, -box[0] * scale, -box[1] * scale);
    g.fillStyle = "#fff";
    g.strokeStyle = "#fff";
    g.lineCap = "round";
    draw(g);
    return g.getImageData(0, 0, w, h).data;
  };
  const body = layer((g) => g.fill(new Path2D(MARK.body)));
  const rim = layer((g) => g.fill(new Path2D(MARK.rim)));
  const key = layer((g) => {
    // "translate(44.26 75.49) rotate(0) translate(0 -3.85)"
    const nums = (MARK.keyholeTransform as string).match(/-?[\d.]+/g)!.map(Number);
    g.translate(nums[0], nums[1]);
    g.translate(nums[3] ?? 0, nums[4] ?? 0);
    g.fill(new Path2D(MARK.keyhole));
  });
  const stroke = layer((g) => {
    g.lineWidth = 3.2;
    g.stroke(new Path2D(MARK.stem));
    g.lineWidth = 1.7;
    g.stroke(new Path2D(MARK.tendril));
  });
  const n = w * h;
  const layers = new Uint8Array(n * 4);
  const light = new Float32Array(n);
  const inside = new Uint8Array(n);
  for (let i = 0; i < n; i++) {
    const b = body[i * 4 + 3], r = rim[i * 4 + 3], k = key[i * 4 + 3], s = stroke[i * 4 + 3];
    layers[i * 4] = b;
    layers[i * 4 + 1] = r;
    layers[i * 4 + 2] = k;
    layers[i * 4 + 3] = s;
    light[i] = Math.max(r, k, s) / 255;
    inside[i] = Math.max(b, r) > 127 ? 1 : 0;
  }
  const din = edt(inside, w, h, 1);
  const dout = edt(inside, w, h, 0);
  const wide = blur(light, w, h, Math.round(scale * 3.2));
  const narrow = blur(light, w, h, Math.round(scale * 0.9));
  const dmax = scale * 16;
  const field = new Uint8Array(n * 4);
  for (let i = 0; i < n; i++) {
    field[i * 4] = Math.min(255, (din[i] / dmax) * 255);
    field[i * 4 + 1] = Math.min(255, wide[i] * 255 * 2.2);
    field[i * 4 + 2] = Math.min(255, narrow[i] * 255 * 1.6);
    field[i * 4 + 3] = Math.min(255, (dout[i] / dmax) * 255);
  }
  return { w, h, box, layers, field, dmax };
}

/** Euclidean distance from every pixel where mask == want to the nearest pixel where it isn't. */
function edt(mask: Uint8Array, w: number, h: number, want: number): Float32Array {
  const INF = 1e20;
  const f = new Float32Array(Math.max(w, h));
  const d = new Float32Array(Math.max(w, h));
  const v = new Int32Array(Math.max(w, h));
  const z = new Float32Array(Math.max(w, h) + 1);
  const grid = new Float32Array(w * h);
  for (let i = 0; i < w * h; i++) grid[i] = mask[i] === want ? INF : 0;
  const pass = (n: number) => {
    let k = 0;
    v[0] = 0;
    z[0] = -INF;
    z[1] = INF;
    for (let q = 1; q < n; q++) {
      let s = (f[q] + q * q - (f[v[k]] + v[k] * v[k])) / (2 * q - 2 * v[k]);
      while (s <= z[k]) {
        k--;
        s = (f[q] + q * q - (f[v[k]] + v[k] * v[k])) / (2 * q - 2 * v[k]);
      }
      k++;
      v[k] = q;
      z[k] = s;
      z[k + 1] = INF;
    }
    k = 0;
    for (let q = 0; q < n; q++) {
      while (z[k + 1] < q) k++;
      d[q] = (q - v[k]) * (q - v[k]) + f[v[k]];
    }
  };
  for (let x = 0; x < w; x++) {
    for (let y = 0; y < h; y++) f[y] = grid[y * w + x];
    pass(h);
    for (let y = 0; y < h; y++) grid[y * w + x] = d[y];
  }
  for (let y = 0; y < h; y++) {
    for (let x = 0; x < w; x++) f[x] = grid[y * w + x];
    pass(w);
    for (let x = 0; x < w; x++) grid[y * w + x] = Math.sqrt(d[x]);
  }
  return grid;
}

/** Three box blurs (≈ gaussian) of radius r. */
function blur(src: Float32Array, w: number, h: number, r: number): Float32Array {
  let a = Float32Array.from(src);
  let b = new Float32Array(a.length);
  const box = (from: Float32Array, to: Float32Array, horiz: boolean) => {
    const len = horiz ? w : h, lines = horiz ? h : w;
    const at = (line: number, i: number) => (horiz ? line * w + i : i * w + line);
    const inv = 1 / (2 * r + 1);
    for (let l = 0; l < lines; l++) {
      let acc = 0;
      for (let i = -r; i <= r; i++) acc += from[at(l, Math.min(len - 1, Math.max(0, i)))];
      for (let i = 0; i < len; i++) {
        to[at(l, i)] = acc * inv;
        acc += from[at(l, Math.min(len - 1, i + r + 1))] - from[at(l, Math.max(0, i - r))];
      }
    }
  };
  for (let it = 0; it < 3; it++) {
    box(a, b, true);
    box(b, a, false);
  }
  return a;
}

export interface MarkPoints {
  /** xy in [-0.5, 0.5] * aspect (y down), and a kind: 0 body, 1 rim, 2 keyhole, 3 stem/tendril */
  xy: Float32Array;
  kind: Uint8Array;
  aspect: number; // box w / h
}

/** n points on the mark: mostly body, a fixed share on the lit parts (they ignite last). */
export function sampleMark(r: MarkRaster, n: number, rand: () => number): MarkPoints {
  const shares = [0.66, 0.24, 0.06, 0.04];
  const pools: number[][] = [[], [], [], []];
  for (let i = 0; i < r.w * r.h; i++) {
    const L = r.layers;
    const k = L[i * 4 + 1] > 127 ? 1 : L[i * 4 + 2] > 127 ? 2 : L[i * 4 + 3] > 127 ? 3 : L[i * 4] > 127 ? 0 : -1;
    if (k >= 0) pools[k].push(i);
  }
  const xy = new Float32Array(n * 2);
  const kind = new Uint8Array(n);
  let o = 0;
  for (let k = 0; k < 4; k++) {
    const cnt = k === 3 ? n - o : Math.round(n * shares[k]);
    const pool = pools[k].length ? pools[k] : pools[0];
    for (let j = 0; j < cnt; j++, o++) {
      const p = pool[Math.floor(rand() * pool.length)];
      const x = (p % r.w) + rand(), y = Math.floor(p / r.w) + rand();
      xy[o * 2] = (x / r.w - 0.5) * (r.w / r.h);
      xy[o * 2 + 1] = y / r.h - 0.5;
      kind[o] = k;
    }
  }
  return { xy, kind, aspect: r.w / r.h };
}

export function rng(seed: number) {
  let s = seed >>> 0;
  return () => {
    s = (s + 0x6d2b79f5) >>> 0;
    let t = s;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}
