// Shared WebGPU helpers for the two renders (hero and proof visualisation).
import type { MarkRaster } from "./mark";

/** `?nogpu` simulates a browser without WebGPU (renders and prover), for testing the fallback. */
export const noGpu = new URLSearchParams(location.search).has("nogpu");
let devicePromise: Promise<GPUDevice | null> | null = null;
/** "apple · metal-3", from the render adapter (the browser's own description of it). */
export let adapterLabel = "";

/** One device for the page's renders (the prover opens its own through wgpu). */
export function renderDevice(): Promise<GPUDevice | null> {
  devicePromise ??= (async () => {
    try {
      if (!("gpu" in navigator) || noGpu) return null;
      const adapter = await navigator.gpu.requestAdapter({ powerPreference: "high-performance" });
      if (!adapter) return null;
      const info: any = (adapter as any).info ?? {};
      adapterLabel = [info.vendor, info.architecture].filter(Boolean).join(" · ");
      const dev = await adapter.requestDevice();
      dev.lost.then(() => (devicePromise = null));
      return dev;
    } catch {
      return null;
    }
  })();
  return devicePromise;
}

export function markTextures(dev: GPUDevice, m: MarkRaster) {
  const mk = (data: Uint8Array) => {
    const t = dev.createTexture({ size: [m.w, m.h], format: "rgba8unorm", usage: GPUTextureUsage.TEXTURE_BINDING | GPUTextureUsage.COPY_DST });
    dev.queue.writeTexture({ texture: t }, data, { bytesPerRow: m.w * 4 }, [m.w, m.h]);
    return t;
  };
  return { layers: mk(m.layers), field: mk(m.field) };
}

/** Keeps a canvas's backing store matched to its CSS size (DPR capped at 2). */
export function fitCanvas(cv: HTMLCanvasElement, maxDpr = 2): boolean {
  const dpr = Math.min(window.devicePixelRatio || 1, maxDpr);
  const w = Math.max(1, Math.round(cv.clientWidth * dpr));
  const h = Math.max(1, Math.round(cv.clientHeight * dpr));
  if (cv.width !== w || cv.height !== h) {
    cv.width = w;
    cv.height = h;
    return true;
  }
  return false;
}

export const reducedMotion = () => window.matchMedia("(prefers-reduced-motion: reduce)").matches;

/** Calls fn only while the element is on screen and the tab is visible. */
export function onScreen(el: Element, cb: (visible: boolean) => void) {
  const io = new IntersectionObserver((es) => cb(es[0].isIntersecting), { rootMargin: "100px" });
  io.observe(el);
}

/** Creates a shader module and rejects (instead of rendering nothing) if WGSL fails to compile. */
export async function shader(dev: GPUDevice, code: string): Promise<GPUShaderModule> {
  const mod = dev.createShaderModule({ code });
  const info = await mod.getCompilationInfo();
  const errs = info.messages.filter((m) => m.type === "error");
  if (errs.length) throw new Error(`WGSL: ${errs.map((e) => `${e.lineNum}:${e.linePos} ${e.message}`).join("; ")}`);
  return mod;
}
