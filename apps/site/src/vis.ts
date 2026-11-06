// The proof visualisation: an artistic WebGPU rendering of the real computation, driven by the
// race's real phases. One particle per proving-key base (36,901 for withdraw):
//   0 idle      curve points drifting
//   1 download  points light up as the zkey's bytes arrive
//   2 resident  the bases sit on the GPU (uploaded once)
//   3 buckets   the MSM: points stream into Pippenger buckets
//   4 collapse  bucket sums collapse into the lauki silhouette
//   5 ignite    the proof verifies: the rim light ignites
import { fitCanvas, onScreen, reducedMotion, renderDevice, shader } from "./gpu";
import { rng, sampleMark, type MarkRaster } from "./mark";

export const N_BASES = 36901;
const WINDOWS = 8;
const BUCKETS = 16;

const WGSL = /* wgsl */ `
struct U { res: vec2f, time: f32, ph0: f32, ph1: f32, prog: f32, download: f32, aspect: f32, dpr: f32, motion: f32, n: f32, glow: f32, dens: f32 };
struct P { seed: vec4f, bucket: vec2f, sil: vec2f, kind: f32, order: f32, pad: vec2f };
@group(0) @binding(0) var<uniform> u: U;
@group(0) @binding(1) var<storage, read> ps: array<P>;

struct VO { @builtin(position) pos: vec4f, @location(0) col: vec4f, @location(1) q: vec2f };

const LIME = vec3f(0.710, 0.949, 0.239);
const LAUKI = vec3f(0.788, 0.863, 0.651);
const LEAF = vec3f(0.36, 0.48, 0.27);
const SKIN = vec3f(0.30, 0.42, 0.22);

fn region() -> vec2f { return vec2f(min(u.aspect * 0.86, 1.5), 0.66); }

fn posFor(ph: i32, p: P, i: f32) -> vec2f {
  let t = u.time * u.motion;
  if (ph <= 1) {
    let base = (p.seed.xy * 2.0 - 1.0) * vec2f(u.aspect * 0.96, 0.94);
    return base + 0.025 * vec2f(sin(t * 0.3 + p.seed.z * 40.0), cos(t * 0.27 + p.seed.w * 40.0));
  }
  if (ph == 2) {
    let r = region();
    let cols = ceil(sqrt(u.n * r.x / r.y));
    let rows = ceil(u.n / cols);
    let c = vec2f(i % cols, floor(i / cols));
    let g = (c / vec2f(cols - 1.0, rows - 1.0) * 2.0 - 1.0) * r;
    return g + vec2f(0.0, 0.006 * sin(g.x * 9.0 + t * 1.5));
  }
  if (ph == 3) {
    let jit = (p.seed.zw * 2.0 - 1.0);
    let ang = p.seed.z * 6.2832;
    return p.bucket + vec2f(cos(ang), sin(ang)) * sqrt(p.seed.w) * 0.032;
  }
  let s = min(1.0, u.aspect) * 0.0 + 1.0;
  return vec2f(p.sil.x, -p.sil.y) * 1.78 * s + vec2f(0.0, 0.0);
}

fn colFor(ph: i32, p: P) -> vec4f {
  if (ph == 0) { return vec4f(LAUKI * 0.8, 0.30); }
  if (ph == 1) {
    let lit = step(p.seed.x, u.download);
    return mix(vec4f(LAUKI * 0.8, 0.26), vec4f(LIME, 0.55), lit);
  }
  if (ph == 2) { return vec4f(LAUKI, 0.5); }
  if (ph == 3) {
    let w = fract(p.order * 7.0);
    return vec4f(mix(LAUKI, LIME, 0.35 + 0.5 * w), 0.6);
  }
  if (ph == 4) {
    if (p.kind > 0.5) { return vec4f(mix(SKIN, LIME, 0.25), 0.55); }
    return vec4f(SKIN, 0.45);
  }
  if (p.kind > 0.5) { return vec4f(LIME, 0.08 + 0.22 * u.glow); }
  return vec4f(SKIN * 0.8, 0.38);
}

fn sizeFor(ph: i32, p: P) -> f32 {
  if (ph == 5 && p.kind > 0.5) { return mix(1.6, 2.6, u.glow); }
  if (ph == 3) { return 1.5; }
  return 1.35;
}

@vertex fn vs(@builtin(vertex_index) vi: u32, @builtin(instance_index) ii: u32) -> VO {
  let p = ps[ii];
  let i = f32(ii);
  let pa = i32(u.ph0);
  let pb = i32(u.ph1);
  // per-particle stagger: the MSM streams in index order, other moves are scattered
  var delay = p.seed.y;
  var spread = 0.6;
  if (pb == 3) { delay = p.order; spread = 3.5; }
  if (pb == 4) { delay = 1.0 - p.sil.y - 0.5; spread = 0.9; }
  let lp = clamp(u.prog * (1.0 + spread) - delay * spread, 0.0, 1.0);
  let e = lp * lp * (3.0 - 2.0 * lp);
  let a = posFor(pa, p, i);
  let b = posFor(pb, p, i);
  var pos = mix(a, b, e);
  // arc the path so streams curve instead of sliding
  let d = b - a;
  let perp = normalize(vec2f(-d.y, d.x) + vec2f(1e-5));
  pos += perp * sin(e * 3.14159) * length(d) * select(0.18, 0.08, pb == 3) * (p.seed.w - 0.5);
  var col = mix(colFor(pa, p), colFor(pb, p), e);
  // particles in flight glow a little
  col = vec4f(col.rgb + LIME * 0.25 * sin(e * 3.14159), col.a);
  let sz = mix(sizeFor(pa, p), sizeFor(pb, p), e) * u.dpr;
  var corners = array(vec2f(-1, -1), vec2f(1, -1), vec2f(-1, 1), vec2f(-1, 1), vec2f(1, -1), vec2f(1, 1));
  let q = corners[vi];
  let clip = vec2f(pos.x / u.aspect, pos.y) + q * sz * 2.0 / u.res;
  var o: VO;
  o.pos = vec4f(clip, 0.0, 1.0);
  o.col = col;
  o.q = q;
  return o;
}

@fragment fn fs(v: VO) -> @location(0) vec4f {
  let r = length(v.q);
  let a = v.col.a * u.dens * smoothstep(1.0, 0.2, r);
  return vec4f(v.col.rgb * a, a);
}
`;

interface Step {
  phase: number;
  ms: number;
}

export interface Vis {
  /** queue a move to `phase` taking at least `ms` (0 under reduced motion) */
  go(phase: number, ms: number): Promise<void>;
  download(p: number): void;
  reset(): void;
  pause(p: boolean): void;
  readonly phase: number;
  onPhase: ((phase: number) => void) | null;
}

export async function startVis(cv: HTMLCanvasElement, mark: MarkRaster): Promise<Vis | null> {
  const dev = await renderDevice();
  if (!dev) return null;
  const ctx = cv.getContext("webgpu");
  if (!ctx) return null;
  const format = navigator.gpu.getPreferredCanvasFormat();
  ctx.configure({ device: dev, format, alphaMode: "premultiplied" });

  // static per-particle data
  const N = N_BASES;
  const rand = rng(0x10_4b1);
  const pts = sampleMark(mark, N, rand);
  // shuffle the silhouette points so neighbouring bases don't land next to each other
  const perm = Array.from({ length: N }, (_, i) => i);
  for (let i = N - 1; i > 0; i--) {
    const j = Math.floor(rand() * (i + 1));
    [perm[i], perm[j]] = [perm[j], perm[i]];
  }
  const data = new Float32Array(N * 12);
  for (let i = 0; i < N; i++) {
    const o = i * 12;
    const s = perm[i];
    data.set([rand(), rand(), rand(), rand()], o);
    // Pippenger: each base lands in one bucket per window; draw it in a pseudo-random one
    const win = Math.floor(rand() * WINDOWS);
    const bk = Math.floor(Math.pow(rand(), 1.3) * BUCKETS);
    data[o + 4] = (bk / (BUCKETS - 1)) * 2 - 1; // x in [-1, 1], scaled in JS below
    data[o + 5] = (win / (WINDOWS - 1)) * 2 - 1;
    data[o + 6] = pts.xy[s * 2];
    data[o + 7] = pts.xy[s * 2 + 1];
    data[o + 8] = pts.kind[s];
    data[o + 9] = i / N;
  }
  const sbuf = dev.createBuffer({ size: data.byteLength, usage: GPUBufferUsage.STORAGE | GPUBufferUsage.COPY_DST });
  const ubuf = dev.createBuffer({ size: 64, usage: GPUBufferUsage.UNIFORM | GPUBufferUsage.COPY_DST });
  const mod = await shader(dev, WGSL);
  const add: GPUBlendComponent = { srcFactor: "one", dstFactor: "one", operation: "add" };
  const pipe = dev.createRenderPipeline({
    layout: "auto",
    vertex: { module: mod, entryPoint: "vs" },
    fragment: { module: mod, entryPoint: "fs", targets: [{ format, blend: { color: add, alpha: add } }] },
    primitive: { topology: "triangle-list" },
  });
  const bind = dev.createBindGroup({ layout: pipe.getBindGroupLayout(0), entries: [{ binding: 0, resource: { buffer: ubuf } }, { binding: 1, resource: { buffer: sbuf } }] });

  let aspectUsed = -1;
  const layoutBuckets = (aspect: number) => {
    // bucket grid fills the same region the resident lattice uses
    const rx = Math.min(aspect * 0.86, 1.5) * 0.92, ry = 0.56;
    const d = Float32Array.from(data);
    for (let i = 0; i < N; i++) {
      d[i * 12 + 4] = data[i * 12 + 4] * rx;
      d[i * 12 + 5] = data[i * 12 + 5] * ry;
    }
    dev.queue.writeBuffer(sbuf, 0, d);
    aspectUsed = aspect;
  };

  let from = 0, to = 0, start = performance.now(), dur = 1, download = 0, glow = 0;
  let paused = false;
  const queue: (Step & { done: () => void })[] = [];
  let current: (() => void) | null = null;
  const u = new Float32Array(16);
  const t0 = performance.now();
  let visible = true, raf = 0;

  const advance = (now: number) => {
    if (now - start < dur) return;
    if (current) {
      current();
      current = null;
    }
    const nx = queue.shift();
    if (!nx) return;
    from = to;
    to = nx.phase;
    start = now;
    dur = reducedMotion() ? 1 : Math.max(1, nx.ms);
    current = nx.done;
    if (to === 5) glow = 0;
    api.onPhase?.(to);
  };

  const frame = () => {
    raf = 0;
    const now = performance.now();
    advance(now);
    if (!paused) {
      fitCanvas(cv);
      const aspect = cv.width / cv.height;
      if (Math.abs(aspect - aspectUsed) > 1e-3) layoutBuckets(aspect);
      const prog = Math.min(1, (now - start) / dur);
      if (to === 5) glow = reducedMotion() ? 1 : Math.min(1, glow + 0.03);
      const motion = reducedMotion() ? 0 : 1;
      // fewer CSS pixels -> the same 36,901 points overlap more: dim them to keep the colours
      const dens = Math.min(1, Math.max(0.4, (cv.clientWidth * cv.clientHeight) / (700 * 620)));
      u.set([cv.width, cv.height, (now - t0) / 1000, from, to, prog, download, aspect, Math.min(window.devicePixelRatio || 1, 2), motion, N, glow, dens]);
      dev.queue.writeBuffer(ubuf, 0, u);
      const enc = dev.createCommandEncoder();
      const pass = enc.beginRenderPass({ colorAttachments: [{ view: ctx.getCurrentTexture().createView(), loadOp: "clear", storeOp: "store", clearValue: [0, 0, 0, 0] }] });
      pass.setPipeline(pipe);
      pass.setBindGroup(0, bind);
      pass.draw(6, N);
      pass.end();
      dev.queue.submit([enc.finish()]);
    }
    const animating = queue.length > 0 || now - start < dur || (to === 5 && glow < 1);
    if (visible && !document.hidden && (!reducedMotion() || animating)) raf = requestAnimationFrame(frame);
  };
  const kick = () => {
    if (!raf) raf = requestAnimationFrame(frame);
  };
  onScreen(cv, (v) => {
    visible = v;
    if (v) kick();
  });
  document.addEventListener("visibilitychange", kick);
  window.addEventListener("resize", kick);
  kick();

  const api: Vis = {
    onPhase: null,
    go(phase, ms) {
      return new Promise<void>((done) => {
        queue.push({ phase, ms, done });
        kick();
      });
    },
    download(p) {
      download = p;
      kick();
    },
    reset() {
      queue.length = 0;
      current?.();
      current = null;
      from = to;
      to = 0;
      start = performance.now();
      dur = reducedMotion() ? 1 : 900;
      download = 0;
      glow = 0;
      api.onPhase?.(0);
      kick();
    },
    pause(p) {
      paused = p;
      if (!p) kick();
    },
    get phase() {
      return to;
    },
  };
  return api;
}
