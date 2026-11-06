// The hero: the lauki mark in low-key light, rendered with WebGPU. The geometry is the real mark
// (rasterised from design/logo); the shader lights it as a rounded gourd with one lime light from
// the right. The rim light breathes and a highlight travels along it; dust drifts through the light.
import { fitCanvas, markTextures, onScreen, reducedMotion, renderDevice, shader } from "./gpu";
import type { MarkRaster } from "./mark";

const WGSL = /* wgsl */ `
struct U { res: vec2f, time: f32, motion: f32, rect: vec4f, texel: vec2f, dmax: f32, pad: f32 };
@group(0) @binding(0) var<uniform> u: U;
@group(0) @binding(1) var samp: sampler;
@group(0) @binding(2) var layers: texture_2d<f32>;
@group(0) @binding(3) var field: texture_2d<f32>;

@vertex fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4f {
  var p = array(vec2f(-1, -3), vec2f(-1, 1), vec2f(3, 1));
  return vec4f(p[i], 0, 1);
}

fn hash2(p: vec2f) -> vec2f {
  let q = vec2f(dot(p, vec2f(127.1, 311.7)), dot(p, vec2f(269.5, 183.3)));
  return fract(sin(q) * 43758.5453);
}
fn hash1(p: vec2f) -> f32 { return fract(sin(dot(p, vec2f(12.9898, 78.233))) * 43758.5453); }

const LIME = vec3f(0.710, 0.949, 0.239);
const SKIN = vec3f(0.149, 0.212, 0.122);   // #26361F
const INK  = vec3f(0.039, 0.067, 0.035);   // #0A1109

fn lin(c: vec3f) -> vec3f { return pow(c, vec3f(2.2)); }

@fragment fn fs(@builtin(position) pos: vec4f) -> @location(0) vec4f {
  let t = u.time;
  let px = pos.xy;
  let uv = (px - u.rect.xy) / u.rect.zw;
  let inBox = all(uv >= vec2f(0.0)) && all(uv <= vec2f(1.0));
  let suv = clamp(uv, vec2f(0.0), vec2f(1.0));
  let L = textureSampleLevel(layers, samp, suv, 0.0);
  let F = textureSampleLevel(field, samp, suv, 0.0);
  let mask = select(0.0, 1.0, inBox);

  // light sway: the single light from the right drifts a little
  let sway = 0.22 * sin(t * 0.37) * u.motion;
  let ldir = normalize(vec3f(cos(sway) * 0.9, -0.25 + 0.12 * sin(t * 0.23) * u.motion, 0.38));

  // background: near-black ink, a faint pool of light right of the gourd
  let c = u.rect.xy + u.rect.zw * vec2f(0.62, 0.55);
  let r = length((px - c) / u.rect.w);
  var col = lin(INK) * (0.75 + 0.25 * smoothstep(1.4, 0.0, r));
  col += lin(LIME) * 0.010 * smoothstep(0.9, 0.0, r);

  // dome height from the inside distance -> surface normal
  let e = u.texel * 1.5;
  let h  = F.r;
  let hx = textureSampleLevel(field, samp, suv + vec2f(e.x, 0.0), 0.0).r - textureSampleLevel(field, samp, suv - vec2f(e.x, 0.0), 0.0).r;
  let hy = textureSampleLevel(field, samp, suv + vec2f(0.0, e.y), 0.0).r - textureSampleLevel(field, samp, suv - vec2f(0.0, e.y), 0.0).r;
  let dome = sqrt(max(0.0, 1.0 - pow(1.0 - min(h * 1.15, 1.0), 2.0)));
  let k = 7.0 * (1.0 - dome * 0.85);
  let n = normalize(vec3f(-hx * k * 6.0, -hy * k * 6.0, 1.0));
  let diff = max(dot(n, ldir), 0.0);
  let fres = pow(1.0 - max(n.z, 0.0), 2.0);
  let side = smoothstep(-0.1, 0.8, dot(normalize(n.xy + vec2f(1e-4)), normalize(ldir.xy)));

  // the gourd skin: low-key, mostly shadow
  let body = L.r * mask;
  var skin = lin(SKIN) * (0.42 + 0.75 * diff);
  // a soft sheen inside the lit edge, moving with the light
  skin += lin(LIME) * 0.55 * fres * side * diff;
  // faint vertical striations of a lauki
  let stri = 0.5 + 0.5 * sin(uv.x * 140.0 + sin(uv.y * 9.0) * 2.0);
  skin *= 0.94 + 0.06 * stri;
  col = mix(col, skin, body);

  // the designed rim: alive. It breathes, and a highlight travels down it.
  let breathe = 0.90 + 0.10 * sin(t * 1.1) * u.motion;
  let travel = fract(t * 0.09) * 1.6 - 0.3;
  let glint = exp(-pow((uv.y - travel) / 0.07, 2.0)) * u.motion;
  let rimC = lin(LIME) * (breathe + 0.9 * glint);
  col = mix(col, rimC, L.g * mask);

  // keyhole: light shining through
  let kp = 0.92 + 0.08 * sin(t * 0.8 + 1.3) * u.motion;
  col = mix(col, lin(LIME) * kp, L.b * mask);
  col = mix(col, lin(LIME) * 0.95, L.a * mask);

  // spill: the lit parts throw a little light outward (outside the gourd only)
  let outside = 1.0 - max(body, max(L.g, max(L.b, L.a)));
  col += lin(LIME) * mask * outside * (0.06 * F.g * breathe + 0.07 * F.b + 0.10 * F.g * glint);

  // dust drifting up through the light
  let cell = 34.0;
  let q = (px + vec2f(0.0, t * 9.0 * u.motion)) / cell;
  let id = floor(q);
  let o = hash2(id);
  let dp = (fract(q) - (0.2 + 0.6 * o)) * cell;
  let dd = length(dp);
  let lit = smoothstep(1.2, 0.1, r) * (0.4 + 0.6 * F.g * mask);
  let tw = 0.5 + 0.5 * sin(t * (0.6 + o.x) + o.y * 6.28);
  if (hash1(id + 3.1) > 0.82) {
    col += lin(LIME) * 0.25 * lit * tw * smoothstep(1.3, 0.0, dd) * outside;
  }

  // grain + tone map
  let g = (hash1(px + fract(t) * 91.0) - 0.5) * 0.012;
  col = col / (1.0 + col * 0.15);
  col = pow(col, vec3f(1.0 / 2.2)) + g;
  return vec4f(col, 1.0);
}
`;

export async function startHero(cv: HTMLCanvasElement, slot: HTMLElement, mark: MarkRaster): Promise<boolean> {
  const dev = await renderDevice();
  if (!dev) return false;
  const ctx = cv.getContext("webgpu");
  if (!ctx) return false;
  const format = navigator.gpu.getPreferredCanvasFormat();
  ctx.configure({ device: dev, format, alphaMode: "opaque" });
  const tex = markTextures(dev, mark);
  const mod = await shader(dev, WGSL);
  const pipe = dev.createRenderPipeline({
    layout: "auto",
    vertex: { module: mod, entryPoint: "vs" },
    fragment: { module: mod, entryPoint: "fs", targets: [{ format }] },
    primitive: { topology: "triangle-list" },
  });
  const ubuf = dev.createBuffer({ size: 48, usage: GPUBufferUsage.UNIFORM | GPUBufferUsage.COPY_DST });
  const bind = dev.createBindGroup({
    layout: pipe.getBindGroupLayout(0),
    entries: [
      { binding: 0, resource: { buffer: ubuf } },
      { binding: 1, resource: dev.createSampler({ magFilter: "linear", minFilter: "linear" }) },
      { binding: 2, resource: tex.layers.createView() },
      { binding: 3, resource: tex.field.createView() },
    ],
  });
  const u = new Float32Array(12);
  let visible = true;
  let raf = 0;
  const t0 = performance.now();
  const frame = () => {
    raf = 0;
    fitCanvas(cv);
    const cr = cv.getBoundingClientRect();
    const sr = slot.getBoundingClientRect();
    const sx = cv.width / Math.max(1, cr.width);
    // fit the mark box (with its padding) into the slot, keeping its aspect
    const aspect = mark.w / mark.h;
    let h = sr.height, w = h * aspect;
    if (w > sr.width) (w = sr.width), (h = w / aspect);
    const x = sr.left - cr.left + (sr.width - w) / 2;
    const y = sr.top - cr.top + (sr.height - h) / 2;
    const motion = reducedMotion() ? 0 : 1;
    u.set([cv.width, cv.height, motion ? (performance.now() - t0) / 1000 : 2.0, motion, x * sx, y * sx, w * sx, h * sx, 1 / mark.w, 1 / mark.h, mark.dmax, 0]);
    dev.queue.writeBuffer(ubuf, 0, u);
    const enc = dev.createCommandEncoder();
    const pass = enc.beginRenderPass({ colorAttachments: [{ view: ctx.getCurrentTexture().createView(), loadOp: "clear", storeOp: "store", clearValue: [0, 0, 0, 1] }] });
    pass.setPipeline(pipe);
    pass.setBindGroup(0, bind);
    pass.draw(3);
    pass.end();
    dev.queue.submit([enc.finish()]);
    if (visible && motion && !document.hidden) raf = requestAnimationFrame(frame);
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
  window.matchMedia("(prefers-reduced-motion: reduce)").addEventListener("change", kick);
  kick();
  return true;
}
