// The stage: one full-page WebGPU canvas behind everything. A flowing colour field per scene
// that floods into the next one as you scroll, and up to 32k particles that reform into each
// scene's motif (lauki, keyhole, app tiles, a private pool, a name bursting into addresses, the
// proof race, radar, benchmark bars).
import { reducedMotion, renderDevice, shader } from "./gpu";
import { buildMotifs, MOTIFS, N_MAX } from "./motifs";
import type { MarkRaster } from "./mark";

export interface SceneStyle {
  bg: [string, string, string];
  p: [string, string];
  add: number; // 1 = additive light, 0 = ink on colour
  alpha: number;
  size: number;
  ext: [number, number]; // motif extent in unit space (x, y)
}

const hex = (h: string) => [1, 3, 5].map((i) => parseInt(h.slice(i, i + 2), 16) / 255);

const WGSL = /* wgsl */ `
struct U {
  res: vec2f, time: f32, f: f32,
  slotA: vec4f,
  slotB: vec4f,
  sa: f32, sb: f32, dpr: f32, motion: f32,
  mouse: vec2f, mouseOn: f32, ndraw: f32,
  r0: f32, r1: f32, rp: f32, burst: f32,
  download: f32, dens: f32, bx: f32, by: f32,
};
struct Pal { c: array<vec4f, 70> };
@group(0) @binding(0) var<uniform> u: U;
@group(0) @binding(1) var<uniform> pal: Pal;
@group(0) @binding(2) var<storage, read> motifs: array<vec4f>;
@group(0) @binding(3) var<storage, read> seeds: array<vec4f>;

const NMAX: u32 = ${N_MAX}u;
const PI = 3.14159265;
const LIME = vec3f(0.710, 0.949, 0.239);
const TEAL = vec3f(0.180, 0.949, 0.816);
const MAGENTA = vec3f(1.0, 0.18, 0.49);
const SAFFRON = vec3f(1.0, 0.69, 0.0);
const CORAL = vec3f(1.0, 0.42, 0.29);

fn P(s: i32, j: i32) -> vec4f { return pal.c[s * 7 + j]; }

// ---------- noise ----------
fn h21(p: vec2f) -> f32 { return fract(sin(dot(p, vec2f(127.1, 311.7))) * 43758.5453); }
fn vnoise(p: vec2f) -> f32 {
  let i = floor(p); let f = fract(p);
  let w = f * f * (3.0 - 2.0 * f);
  return mix(mix(h21(i), h21(i + vec2f(1, 0)), w.x), mix(h21(i + vec2f(0, 1)), h21(i + vec2f(1, 1)), w.x), w.y);
}
fn fbm(p0: vec2f) -> f32 {
  var p = p0; var a = 0.5; var s = 0.0;
  for (var i = 0; i < 4; i++) { s += a * vnoise(p); p = p * 2.03 + vec2f(1.7, 9.2); a *= 0.5; }
  return s;
}

// ---------- background ----------
struct VB { @builtin(position) pos: vec4f };
@vertex fn vsBg(@builtin(vertex_index) i: u32) -> VB {
  var p = array(vec2f(-1, -3), vec2f(-1, 1), vec2f(3, 1));
  var o: VB; o.pos = vec4f(p[i], 0, 1); return o;
}

fn field(s: i32, uv: vec2f, t: f32) -> vec3f {
  let p = (uv - 0.5) * vec2f(u.res.x / u.res.y, 1.0) * 1.7 + vec2f(f32(s) * 3.1, 0.0);
  let q = vec2f(fbm(p + vec2f(0.0, t * 0.05)), fbm(p + vec2f(5.2, 1.3) - vec2f(t * 0.04, 0.0)));
  let r = fbm(p + 3.2 * q + vec2f(t * 0.03, -t * 0.02));
  var c = mix(P(s, 0).rgb, P(s, 1).rgb, smoothstep(0.28, 0.72, r));
  c = mix(c, P(s, 2).rgb, smoothstep(0.45, 0.85, q.x) * 0.85);
  return c;
}

@fragment fn fsBg(v: VB) -> @location(0) vec4f {
  let px = v.pos.xy / u.dpr;
  let uv = px / u.res;
  let t = u.time * u.motion;
  let sa = i32(u.sa); let sb = i32(u.sb);
  var c = field(sa, uv, t);
  if (u.f > 0.0005) {
    let cb = field(sb, uv, t);
    let nz = fbm(uv * vec2f(u.res.x / u.res.y, 1.0) * 2.5 + vec2f(t * 0.1, 0.0));
    let m = smoothstep(-0.07, 0.07, u.f * 1.45 - ((1.0 - uv.y) * 0.85 + nz * 0.55));
    c = mix(c, cb, m);
    // a bright seam where the new world flows in
    let seam = exp(-pow((u.f * 1.45 - ((1.0 - uv.y) * 0.85 + nz * 0.55)) / 0.025, 2.0));
    c += mix(P(sb, 4).rgb, vec3f(1.0), 0.3) * seam * 0.35 * sin(u.f * PI);
  }
  // the proof burst: a ring of colour
  if (u.burst > 0.0 && u.burst < 3.0) {
    let d = length((px - vec2f(u.bx, u.by)) / u.res.y);
    let a = atan2(px.y - u.by, px.x - u.bx);
    let hue = 0.5 + 0.5 * sin(a * 3.0 + u.burst * 2.0);
    let col = mix(mix(LIME, TEAL, hue), mix(MAGENTA, SAFFRON, hue), 0.5 + 0.5 * sin(a * 2.0 - 1.0));
    let k = 1.0 - u.burst / 3.0;
    for (var j = 0; j < 3; j++) {
      let rr = u.burst * (0.55 + 0.25 * f32(j));
      c += col * exp(-pow((d - rr) / (0.05 + 0.03 * f32(j)), 2.0)) * k * 0.9;
    }
    c += col * exp(-d * 4.0) * k * k * 0.5;
  }
  // vignette + grain
  let vg = smoothstep(1.25, 0.35, length((uv - 0.5) * vec2f(1.1, 1.0)));
  c *= 0.82 + 0.18 * vg;
  c += (h21(px + fract(t * 7.0) * 100.0) - 0.5) * 0.025;
  return vec4f(c, 1.0);
}

// ---------- particles ----------
struct VO { @builtin(position) pos: vec4f, @location(0) col: vec4f, @location(1) q: vec2f, @location(2) add: f32 };

fn motif(k: i32, i: u32) -> vec4f { return motifs[u32(k) * NMAX + i]; }

// Unit-space position of particle i in scene k (y up), and a colour selector in .z, alpha in .w
fn racePos(ph: i32, i: u32, sd: vec4f, t: f32) -> vec4f {
  if (ph <= 1) {
    let m = motif(6, i);
    let a = m.x * 2.0 * PI + t * (0.12 + 0.25 * m.y);
    let r = 0.55 + 0.42 * pow(m.y, 1.6) + 0.03 * sin(t * 2.0 + m.x * 40.0);
    var lit = 0.35;
    if (ph == 1) { lit = mix(0.25, 1.0, step(sd.w, u.download)); }
    return vec4f(cos(a) * r, sin(a) * r, 0.0, lit);
  }
  if (ph == 2) {
    let cols = 192.0;
    let fi = f32(i % 36864u);
    let c = vec2f(fi % cols, floor(fi / cols));
    return vec4f(c.x / cols * 1.84 - 0.92, c.y / 192.0 * 1.5 - 0.75, 1.0, 0.7);
  }
  if (ph == 3) { let m = motif(10, i); return vec4f(m.xy, 1.0, 0.9); }
  let m = motif(0, i);
  return vec4f(m.xy, 2.0 + m.z, 1.0);
}

fn unitPos(k: i32, i: u32, sd: vec4f, t: f32) -> vec4f {
  let m = motif(k, i);
  if (k == 0 || k == 9) {
    let breathe = 1.0 + 0.012 * sin(t * 1.2);
    let wob = vec2f(sin(t * 0.7 + sd.x * 30.0), cos(t * 0.6 + sd.y * 30.0)) * 0.006;
    return vec4f(m.xy * breathe + wob, 10.0 + m.z, 1.0);
  }
  if (k == 1) {
    if (m.z > 1.5) {
      let a = atan2(m.y + 0.12, m.x) + t * 0.15 * (sd.x - 0.3);
      let r = length(vec2f(m.x, m.y + 0.12));
      return vec4f(cos(a) * r, sin(a) * r - 0.12, 20.0 + m.z, 0.35);
    }
    return vec4f(m.xy, 20.0 + m.z, 1.0);
  }
  if (k == 3) {
    let sep = 0.05 + 0.07 * (0.5 + 0.5 * sin(t * 0.9));
    let tile = sign(m.xy);
    return vec4f(m.xy + tile * sep, 30.0 + m.z, 1.0);
  }
  if (k == 4) {
    let ph = fract(m.w + t * 0.06);
    let yE = m.x * 0.85;
    let yX = m.y * 0.85;
    var p: vec2f; var seg = 0.0;
    if (ph < 0.4) {
      let s = ph / 0.4;
      p = vec2f(mix(-1.75, -0.42, s), yE * (1.0 - s * s));
    } else if (ph < 0.62) {
      let s = (ph - 0.4) / 0.22;
      let a = s * 2.0 * PI * 0.8 + m.w * 40.0 + t * 0.6;
      let r = 0.42 * sqrt(fract(m.w * 7.31)) * (0.85 + 0.15 * sin(s * PI));
      p = vec2f(cos(a) * r * 1.2, sin(a) * r * 0.85);
      seg = 1.0;
    } else {
      let s = (ph - 0.62) / 0.38;
      p = vec2f(mix(0.42, 1.75, s), yX * s * s);
      seg = 2.0;
    }
    return vec4f(p, 40.0 + seg, 1.0);
  }
  if (k == 5) {
    if (m.z < 0.5) { return vec4f(m.xy + vec2f(0.0, 0.004 * sin(t * 3.0 + m.x * 30.0)), 50.0, 1.0); }
    let ph = fract(m.w + t * 0.12);
    let r = 0.06 + pow(ph, 0.7) * 1.15;
    let a = m.x + 0.04 * sin(t + m.y * 6.0);
    let tip = smoothstep(0.75, 1.0, ph);
    return vec4f(cos(a) * r, sin(a) * r * 0.62, 51.0, (1.0 - ph * 0.6) * (0.5 + tip));
  }
  if (k == 6) {
    let ia = i32(u.r0); let ib = i32(u.r1);
    var d = sd.y; var sp = 0.7;
    if (ib == 3) { d = motif(10, i).w; sp = 3.5; }
    if (ib == 4) { d = 0.5 - motif(0, i).y * 0.5; sp = 1.2; }
    let lp = clamp(u.rp * (1.0 + sp) - d * sp, 0.0, 1.0);
    let e = lp * lp * (3.0 - 2.0 * lp);
    let a = racePos(ia, i, sd, t);
    let b = racePos(ib, i, sd, t);
    var p = mix(a.xy, b.xy, e);
    let dd = b.xy - a.xy;
    p += normalize(vec2f(-dd.y, dd.x) + vec2f(1e-5)) * sin(e * PI) * length(dd) * 0.12 * (sd.z - 0.5);
    var sel = mix(60.0 + a.z, 60.0 + b.z, step(0.5, e));
    var al = mix(a.w, b.w, e);
    if (u.burst > 0.0) {
      // the burst: the lauki blows out in colour, then settles back
      let k2 = exp(-u.burst * 1.6) * (1.0 - exp(-u.burst * 14.0));
      let dir = normalize(p + vec2f(0.0001, 0.0001)) * (0.4 + 1.8 * sd.x);
      p += dir * k2;
      sel = 70.0;
      al = 1.0;
    }
    return vec4f(p, sel, al);
  }
  if (k == 7) {
    let dir = select(-1.0, 1.0, (i32(m.z) % 2) == 0);
    let a = m.w + t * 0.05 * dir * (1.0 + m.z * 0.3);
    let r = length(m.xy);
    let sweep = fract((a - t * 0.9) / (2.0 * PI));
    return vec4f(cos(a) * r, sin(a) * r, 80.0, 0.35 + 0.65 * smoothstep(0.92, 1.0, sweep));
  }
  if (k == 8) {
    return vec4f(m.xy + vec2f(0.0, 0.003 * sin(t * 2.0 + m.x * 20.0)), 90.0 + m.z, 1.0);
  }
  return vec4f(m.xy, 100.0, 1.0);
}

fn colOf(k: i32, sel: f32, sd: vec4f) -> vec3f {
  let p0 = P(k, 3).rgb; let p1 = P(k, 4).rgb;
  if (sel >= 70.0 && sel < 71.0) {
    let h = sd.w;
    if (h < 0.25) { return LIME; } if (h < 0.5) { return TEAL; } if (h < 0.75) { return MAGENTA; } return SAFFRON;
  }
  if (sel >= 10.0 && sel < 14.0) {          // lauki: body teal-dim, lit parts lime
    if (sel > 10.5) { return p0; }
    return mix(p1 * 0.32, p0 * 0.30, sd.x * 0.6);
  }
  if (sel >= 20.0 && sel < 23.0) {          // phone ink, keyhole + halo
    if (sel > 20.5) { return p1; } return p0;
  }
  if (sel >= 30.0 && sel < 34.0) {          // four apps, four colours
    let j = i32(sel - 30.0);
    if (j == 0) { return p0; } if (j == 1) { return vec3f(1.0, 0.97, 0.91); } if (j == 2) { return vec3f(0.36, 0.06, 0.29); } return vec3f(0.55, 0.0, 0.12);
  }
  if (sel >= 40.0 && sel < 43.0) {          // public in (warm), pool, private out (lime)
    if (sel < 40.5) { return p0; } if (sel < 41.5) { return mix(p0, p1, 0.5) * 0.6; } return p1;
  }
  if (sel >= 50.0 && sel < 52.0) { if (sel > 50.5) { return p1; } return p0; }
  if (sel >= 60.0 && sel < 70.0) {          // race
    if (sel > 61.5) { if (sel > 62.5) { return LIME; } return p1 * 0.6; }
    return mix(p1, p0, sd.x);
  }
  if (sel >= 90.0 && sel < 92.0) { if (sel > 90.5) { return p1; } return p0; }
  return mix(p0, p1, sd.x);
}

fn toPx(k: i32, unit: vec2f, slot: vec4f) -> vec2f {
  let ext = P(k, 6).xy;
  let s = min(slot.z * 0.5 / ext.x, slot.w * 0.5 / ext.y);
  return slot.xy + slot.zw * 0.5 + vec2f(unit.x, -unit.y) * s;
}

@vertex fn vsP(@builtin(vertex_index) vi: u32, @builtin(instance_index) ii: u32) -> VO {
  let sd = seeds[ii];
  let t = u.time * u.motion;
  let ka = i32(u.sa); let kb = i32(u.sb);
  let ua = unitPos(ka, ii, sd, t);
  var pos = toPx(ka, ua.xy, u.slotA);
  var col = vec4f(colOf(ka, ua.z, sd), ua.w * P(ka, 5).y);
  var add = P(ka, 5).x;
  var size = P(ka, 5).z;
  if (u.f > 0.0005) {
    let ub = unitPos(kb, ii, sd, t);
    let pb = toPx(kb, ub.xy, u.slotB);
    let sp = 0.9;
    let lp = clamp(u.f * (1.0 + sp) - sd.x * sp, 0.0, 1.0);
    let e = lp * lp * (3.0 - 2.0 * lp);
    let swirl = vec2f(sin(sd.y * 40.0 + t * 0.8), cos(sd.z * 40.0 + t * 0.7)) * sin(e * PI) * u.res.y * (0.12 + 0.2 * sd.w);
    pos = mix(pos, pb, e) + swirl;
    col = mix(col, vec4f(colOf(kb, ub.z, sd), ub.w * P(kb, 5).y), e);
    add = mix(add, P(kb, 5).x, e);
    size = mix(size, P(kb, 5).z, e);
    // in flight, particles become light
    col = vec4f(mix(col.rgb, vec3f(1.0), 0.25 * sin(e * PI)), col.a);
  }
  // the cursor parts the particles
  let d = pos - u.mouse;
  let l = length(d);
  let R = 130.0;
  pos += normalize(d + vec2f(1e-4)) * max(0.0, R - l) * 0.7 * u.mouseOn;
  col = vec4f(col.rgb, col.a * u.dens);
  var corners = array(vec2f(-1, -1), vec2f(1, -1), vec2f(-1, 1), vec2f(-1, 1), vec2f(1, -1), vec2f(1, 1));
  let q = corners[vi];
  let clip = (pos + q * size) / u.res * vec2f(2.0, -2.0) + vec2f(-1.0, 1.0);
  var o: VO;
  o.pos = vec4f(clip, 0.0, 1.0);
  if (ii >= u32(u.ndraw)) { o.pos = vec4f(9.0, 9.0, 0.0, 1.0); }
  o.col = col;
  o.q = q;
  o.add = add;
  return o;
}

@fragment fn fsP(v: VO) -> @location(0) vec4f {
  let r = length(v.q);
  let a = v.col.a * smoothstep(1.0, 0.15, r);
  return vec4f(v.col.rgb * a, a * (1.0 - v.add));
}
`;

export interface Stage {
  setRace(p: { r0?: number; r1?: number; rp?: number; download?: number; burst?: boolean }): void;
  racePhase(phase: number, ms: number): Promise<void>;
  pause(p: boolean): void;
}

export async function startStage(cv: HTMLCanvasElement, scenes: HTMLElement[], styles: SceneStyle[], mark: MarkRaster): Promise<Stage | null> {
  const dev = await renderDevice();
  if (!dev) return null;
  const ctx = cv.getContext("webgpu");
  if (!ctx) return null;
  const format = navigator.gpu.getPreferredCanvasFormat();
  ctx.configure({ device: dev, format, alphaMode: "opaque" });

  const mod = await shader(dev, WGSL);

  const pal = new Float32Array(70 * 4);
  styles.forEach((s, k) => {
    const put = (j: number, v: number[]) => pal.set(v, (k * 7 + j) * 4);
    put(0, [...hex(s.bg[0]), 1]);
    put(1, [...hex(s.bg[1]), 1]);
    put(2, [...hex(s.bg[2]), 1]);
    put(3, [...hex(s.p[0]), 1]);
    put(4, [...hex(s.p[1]), 1]);
    put(5, [s.add, s.alpha, s.size, 0]);
    put(6, [s.ext[0], s.ext[1], 0, 0]);
  });
  const palBuf = dev.createBuffer({ size: pal.byteLength, usage: GPUBufferUsage.UNIFORM | GPUBufferUsage.COPY_DST });
  dev.queue.writeBuffer(palBuf, 0, pal);
  const motifData = buildMotifs(mark);
  const motifBuf = dev.createBuffer({ size: motifData.byteLength, usage: GPUBufferUsage.STORAGE | GPUBufferUsage.COPY_DST });
  dev.queue.writeBuffer(motifBuf, 0, motifData);
  const seeds = new Float32Array(N_MAX * 4);
  for (let i = 0; i < seeds.length; i++) seeds[i] = Math.random();
  const seedBuf = dev.createBuffer({ size: seeds.byteLength, usage: GPUBufferUsage.STORAGE | GPUBufferUsage.COPY_DST });
  dev.queue.writeBuffer(seedBuf, 0, seeds);
  void MOTIFS;
  const ubuf = dev.createBuffer({ size: 112, usage: GPUBufferUsage.UNIFORM | GPUBufferUsage.COPY_DST });

  const layout = dev.createBindGroupLayout({
    entries: [
      { binding: 0, visibility: GPUShaderStage.VERTEX | GPUShaderStage.FRAGMENT, buffer: { type: "uniform" } },
      { binding: 1, visibility: GPUShaderStage.VERTEX | GPUShaderStage.FRAGMENT, buffer: { type: "uniform" } },
      { binding: 2, visibility: GPUShaderStage.VERTEX, buffer: { type: "read-only-storage" } },
      { binding: 3, visibility: GPUShaderStage.VERTEX, buffer: { type: "read-only-storage" } },
    ],
  });
  const pl = dev.createPipelineLayout({ bindGroupLayouts: [layout] });
  const bg = dev.createRenderPipeline({ layout: pl, vertex: { module: mod, entryPoint: "vsBg" }, fragment: { module: mod, entryPoint: "fsBg", targets: [{ format }] } });
  const blend: GPUBlendState = {
    color: { srcFactor: "one", dstFactor: "one-minus-src-alpha", operation: "add" },
    alpha: { srcFactor: "one", dstFactor: "one-minus-src-alpha", operation: "add" },
  };
  const parts = dev.createRenderPipeline({ layout: pl, vertex: { module: mod, entryPoint: "vsP" }, fragment: { module: mod, entryPoint: "fsP", targets: [{ format, blend }] } });
  const bind = dev.createBindGroup({
    layout,
    entries: [
      { binding: 0, resource: { buffer: ubuf } },
      { binding: 1, resource: { buffer: palBuf } },
      { binding: 2, resource: { buffer: motifBuf } },
      { binding: 3, resource: { buffer: seedBuf } },
    ],
  });

  // ---- state ----
  const mobile = Math.min(innerWidth, innerHeight) < 600;
  let ndraw = mobile ? 18000 : N_MAX;
  let mouse = [-9999, -9999], mouseOn = 0, mouseTarget = 0;
  addEventListener("pointermove", (e) => {
    if (e.pointerType === "touch") return;
    mouse = [e.clientX, e.clientY];
    mouseTarget = 1;
  });
  document.addEventListener("pointerleave", () => (mouseTarget = 0));
  let paused = false;
  const race = { r0: 0, r1: 0, rp: 1, download: 0, burst: -1, burstAt: 0 };
  const queue: { phase: number; ms: number; done: () => void }[] = [];
  let rStart = 0, rDur = 1, rDone: (() => void) | null = null;
  const u = new Float32Array(28);
  const t0 = performance.now();
  let lastT = performance.now(), slowFrames = 0, fastFrames = 0;

  const rectOf = (el: HTMLElement) => {
    const r = el.getBoundingClientRect();
    return [r.left, r.top, Math.max(1, r.width), Math.max(1, r.height)];
  };

  const frame = (now: number) => {
    requestAnimationFrame(frame);
    if (document.hidden) return;
    // adapt the particle count to keep the frame rate
    const dt = now - lastT;
    lastT = now;
    if (dt > 24) slowFrames++;
    else if (dt < 18) fastFrames++;
    if (slowFrames > 20) (ndraw = Math.max(6000, Math.floor(ndraw * 0.75))), (slowFrames = fastFrames = 0);
    if (fastFrames > 240 && ndraw < (mobile ? 20000 : N_MAX)) (ndraw = Math.min(N_MAX, Math.floor(ndraw * 1.1))), (slowFrames = fastFrames = 0);
    if (paused) return;

    // race phase queue
    if (now - rStart >= rDur) {
      if (rDone) rDone(), (rDone = null);
      const nx = queue.shift();
      if (nx) {
        race.r0 = race.r1;
        race.r1 = nx.phase;
        rStart = now;
        rDur = reducedMotion() ? 1 : Math.max(1, nx.ms);
        rDone = nx.done;
      }
    }
    race.rp = Math.min(1, (now - rStart) / rDur);

    const dpr = Math.min(devicePixelRatio || 1, 2);
    const w = Math.round(innerWidth * dpr), h = Math.round(innerHeight * dpr);
    if (cv.width !== w || cv.height !== h) (cv.width = w), (cv.height = h);

    // scroll -> scene float: blend across each boundary over ~60% of a screen
    const mid = innerHeight * 0.5;
    let sf = 0;
    const W = innerHeight * 0.6;
    for (let k = 1; k < scenes.length; k++) {
      const b = scenes[k].getBoundingClientRect().top - mid; // boundary relative to the viewport middle
      sf += Math.min(1, Math.max(0, (-b + W / 2) / W));
    }
    const sa = Math.min(scenes.length - 1, Math.floor(sf));
    const sb = Math.min(scenes.length - 1, sa + 1);
    const f = sf - sa;
    const slot = (k: number) => rectOf(scenes[k].querySelector<HTMLElement>(".art") ?? scenes[k]);
    const sid = (k: number) => Number(scenes[k].dataset.scene ?? k);
    mouseOn += (mouseTarget * (sid(sa) === 0 || sid(sa) === 9 ? 1 : 0.6) - mouseOn) * 0.08;
    const motion = reducedMotion() ? 0 : 1;
    const burst = race.burst >= 0 ? (now - race.burstAt) / 1000 : 0;
    const raceScene = scenes.find((x) => x.dataset.scene === "6") ?? scenes[0];
    const rs = rectOf(raceScene.querySelector<HTMLElement>(".art") ?? raceScene);
    const dens = Math.min(1, Math.max(0.45, (innerWidth * innerHeight) / (1300 * 820)));
    u.set([innerWidth, innerHeight, (now - t0) / 1000, f, ...slot(sa), ...slot(sb), sid(sa), sid(sb), dpr, motion, mouse[0], mouse[1], mouseOn, ndraw,
      race.r0, race.r1, race.rp, burst > 6 ? 0 : burst, race.download, dens, rs[0] + rs[2] / 2, rs[1] + rs[3] / 2]);
    dev.queue.writeBuffer(ubuf, 0, u);
    const enc = dev.createCommandEncoder();
    const pass = enc.beginRenderPass({ colorAttachments: [{ view: ctx.getCurrentTexture().createView(), loadOp: "clear", storeOp: "store", clearValue: [0, 0, 0, 1] }] });
    pass.setBindGroup(0, bind);
    pass.setPipeline(bg);
    pass.draw(3);
    pass.setPipeline(parts);
    pass.draw(6, ndraw);
    pass.end();
    dev.queue.submit([enc.finish()]);
    (window as any).__stage = { sf, sa, sb, f, ndraw, fps: Math.round(1000 / Math.max(1, dt)) };
  };
  requestAnimationFrame(frame);

  return {
    setRace(p) {
      if (p.download !== undefined) race.download = p.download;
      if (p.burst) (race.burst = 1), (race.burstAt = performance.now());
      if (p.burst === false) race.burst = -1;
      if (p.r1 !== undefined) {
        queue.length = 0;
        rDone?.();
        rDone = null;
        race.r0 = race.r1 = p.r1;
        race.rp = 1;
      }
    },
    racePhase(phase, ms) {
      return new Promise<void>((done) => queue.push({ phase, ms, done }));
    },
    pause(p) {
      paused = p;
    },
  };
}
