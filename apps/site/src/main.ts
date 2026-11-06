import { noGpu } from "./gpu";
import { startHero } from "./hero";
import { rasterMark } from "./mark";
import { setupRace } from "./race";
import { startVis } from "./vis";

const $ = (id: string) => document.getElementById(id)!;

// ---- theme: system by default, a toggle that remembers (per browser only) ----------------------
const THEME_KEY = "lowki-site-theme";
function applyTheme(t: string | null) {
  if (t === "light" || t === "dark") document.documentElement.dataset.theme = t;
  else delete document.documentElement.dataset.theme;
}
const qsTheme = new URLSearchParams(location.search).get("theme");
try {
  applyTheme(qsTheme ?? localStorage.getItem(THEME_KEY));
} catch {
  applyTheme(qsTheme);
}
$("theme-btn").addEventListener("click", () => {
  const dark = document.documentElement.dataset.theme
    ? document.documentElement.dataset.theme === "dark"
    : matchMedia("(prefers-color-scheme: dark)").matches;
  const next = dark ? "light" : "dark";
  applyTheme(next);
  try {
    localStorage.setItem(THEME_KEY, next);
  } catch {}
});

// ---- links from site-config.json (filled in before publishing) ---------------------------------
interface Config {
  repo?: string;
  video?: string;
  demos?: { name: string; detail?: string; url?: string }[];
}
fetch("site-config.json", { cache: "no-cache" })
  .then((r) => r.json() as Promise<Config>)
  .then((c) => {
    const link = (el: HTMLAnchorElement, url?: string) => {
      if (url) {
        el.href = url;
        el.removeAttribute("aria-disabled");
      } else {
        el.removeAttribute("href");
        el.setAttribute("aria-disabled", "true");
        el.querySelector(".soon")?.removeAttribute("hidden");
      }
    };
    document.querySelectorAll<HTMLAnchorElement>("[data-link=repo]").forEach((a) => link(a, c.repo));
    document.querySelectorAll<HTMLAnchorElement>("[data-link=video]").forEach((a) => link(a, c.video));
    const ul = $("demo-list");
    ul.innerHTML = "";
    for (const d of c.demos ?? []) {
      const li = document.createElement("li");
      const a = document.createElement("a");
      a.className = "demo";
      a.innerHTML = `<span class="demo-name"></span><span class="demo-detail"></span><span class="demo-go" aria-hidden="true"></span>`;
      a.querySelector(".demo-name")!.textContent = d.name;
      a.querySelector(".demo-detail")!.textContent = d.detail ?? "";
      a.querySelector(".demo-go")!.textContent = d.url ? "Open →" : "Link coming soon";
      link(a, d.url);
      li.append(a);
      ul.append(li);
    }
  })
  .catch(() => {});

// ---- the renders and the race -----------------------------------------------------------------
const hasWebGPU = "gpu" in navigator && !noGpu;
const mark = rasterMark(8);
const heroCanvas = $("hero-canvas") as HTMLCanvasElement;
startHero(heroCanvas, $("hero-mark-slot"), mark)
  .then((ok) => {
    document.documentElement.dataset.heroGpu = ok ? "on" : "off";
    $("hero-gpu-label").textContent = ok ? "Rendered live on your GPU with WebGPU" : "WebGPU isn't available here, so this is the static mark";
  })
  .catch((e) => {
    console.warn("hero render:", e);
    document.documentElement.dataset.heroGpu = "off";
  });

startVis($("vis-canvas") as HTMLCanvasElement, mark)
  .catch((e) => (console.warn("proof render:", e), null))
  .then((vis) => {
    document.documentElement.dataset.visGpu = vis ? "on" : "off";
    const PHASES = [
      "36,901 curve points: one per proving-key base",
      "Downloading the circuit",
      "Bases resident on the GPU",
      "MSM: bases stream into Pippenger buckets",
      "Bucket sums fold into the proof",
      "Proof valid",
    ];
    if (vis) {
      const el = $("vis-phase");
      vis.onPhase = (p) => {
        el.textContent = `${String(p).padStart(2, "0")} · ${PHASES[p]}`;
        el.dataset.phase = String(p);
      };
      vis.onPhase(0);
    }
    setupRace(vis, hasWebGPU);
  });

// ---- CROPS, live: count requests this page made to other origins ------------------------------
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

// ---- reveal on scroll (skipped with reduced motion) --------------------------------------------
if (!matchMedia("(prefers-reduced-motion: reduce)").matches && "IntersectionObserver" in window) {
  const io = new IntersectionObserver(
    (es) => es.forEach((e) => e.isIntersecting && (e.target.classList.add("in"), io.unobserve(e.target))),
    { rootMargin: "0px 0px -8% 0px" },
  );
  document.querySelectorAll(".reveal").forEach((el) => io.observe(el));
  document.documentElement.classList.add("reveal-ready");
}
