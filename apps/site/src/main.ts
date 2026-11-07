import { noGpu } from "./gpu";
import { rasterMark } from "./mark";
import { setupRace } from "./race";
import { startStage, type SceneStyle } from "./stage";

const $ = (id: string) => document.getElementById(id)!;

// One colour world per scene: background field (3 colours), particle colours, light or ink.
const LIME = "#B5F23D", TEAL = "#2EF2D0", INK = "#0F1B0D";
export const STYLES: SceneStyle[] = [
  { bg: ["#0F1B0D", "#1F4A14", "#050A05"], p: [LIME, TEAL], add: 1, alpha: 0.6, size: 1.7, ext: [1, 1] }, // night + lime light
  { bg: ["#B5F23D", "#2EF2D0", "#0FB89C"], p: [INK, "#06463C"], add: 0, alpha: 0.9, size: 1.5, ext: [1, 1] }, // lime -> teal
  { bg: ["#021A1E", "#00566A", "#160A45"], p: [LIME, TEAL], add: 1, alpha: 0.55, size: 1.4, ext: [1, 0.5] }, // teal night
  { bg: ["#FFB000", "#FF6A00", "#FF2E7E"], p: ["#1A0612", "#FFF6E8"], add: 0, alpha: 0.92, size: 1.5, ext: [1.12, 1.12] }, // saffron -> magenta
  { bg: ["#020402", "#0B1F0D", "#173A14"], p: ["#FF6B4A", LIME], add: 1, alpha: 0.65, size: 1.6, ext: [1.75, 0.95] }, // private: ink + lime glow
  { bg: ["#FF5A5F", "#E8336D", "#5B1A8C"], p: ["#FFF4E6", "#FFD23F"], add: 0, alpha: 0.95, size: 1.5, ext: [1.2, 0.8] }, // sunset coral
  { bg: ["#050A05", "#0F1B0D", "#1A3310"], p: [LIME, TEAL], add: 1, alpha: 0.55, size: 1.5, ext: [1, 1] }, // the race: night
  { bg: ["#FFB000", "#FF6A00", "#E8336D"], p: ["#1A0612", "#FFF6E8"], add: 0, alpha: 0.25, size: 1.4, ext: [1, 1] }, // electric blue
  { bg: ["#FFD23F", "#B5F23D", "#FF9F45"], p: [INK, "#FFFFFF"], add: 0, alpha: 0.38, size: 1.5, ext: [1, 0.85] }, // sunrise
  { bg: ["#0F1B0D", "#2E4A0E", "#050A05"], p: [LIME, TEAL], add: 1, alpha: 0.6, size: 1.7, ext: [1, 1] }, // back to night
];

// ---- links from site-config.json ----------------------------------------------------------------
interface Config { repo?: string; video?: string; demos?: { name: string; detail?: string; url?: string }[] }
fetch("site-config.json", { cache: "no-cache" })
  .then((r) => r.json() as Promise<Config>)
  .then((c) => {
    const link = (el: HTMLAnchorElement, url?: string) => {
      if (url) el.href = url;
      else {
        el.removeAttribute("href");
        el.setAttribute("aria-disabled", "true");
        el.querySelector(".soon")?.removeAttribute("hidden");
      }
    };
    document.querySelectorAll<HTMLAnchorElement>("[data-link=repo]").forEach((a) => link(a, c.repo));
    document.querySelectorAll<HTMLAnchorElement>("[data-link=video]").forEach((a) => link(a, c.video));
    const ul = $("demo-list");
    for (const d of c.demos ?? []) {
      const li = document.createElement("li");
      const a = document.createElement("a");
      a.className = "demo";
      a.textContent = d.name;
      const s = document.createElement("span");
      s.textContent = d.url ? "→" : "soon";
      a.append(s);
      link(a, d.url);
      li.append(a);
      ul.append(li);
    }
  })
  .catch(() => {});

// ---- the stage --------------------------------------------------------------------------------
const scenes = [...document.querySelectorAll<HTMLElement>(".scene")];
(async () => {
  try {
    await Promise.all([
      document.fonts.load('700 100px "Bricolage Grotesque"'),
      document.fonts.load('700 100px "IBM Plex Mono"').catch(() => null),
    ]);
  } catch {}
  let stage = null;
  try {
    stage = noGpu ? null : await startStage($("stage") as HTMLCanvasElement, scenes, STYLES, rasterMark(6));
  } catch (e) {
    console.warn("stage:", e);
  }
  document.documentElement.classList.toggle("gpu", !!stage);
  document.documentElement.classList.add("ready");
  setupRace(stage);
})();

// ---- CROPS, live: requests this page made to other origins -------------------------------------
function audit() {
  const other = performance
    .getEntriesByType("resource")
    .map((e) => new URL(e.name, location.href))
    .filter((u) => (u.protocol === "http:" || u.protocol === "https:") && u.origin !== location.origin);
  $("audit-n").textContent = String(other.length);
  $("audit").dataset.ok = String(other.length === 0);
}
audit();
new PerformanceObserver(audit).observe({ type: "resource", buffered: true });

// ---- copy rises in as each scene arrives -----------------------------------------------------------
if (!matchMedia("(prefers-reduced-motion: reduce)").matches) {
  const io = new IntersectionObserver((es) => es.forEach((e) => e.target.classList.toggle("in", e.isIntersecting)), { threshold: 0.35 });
  scenes.forEach((s) => io.observe(s));
  document.documentElement.classList.add("anim");
}
