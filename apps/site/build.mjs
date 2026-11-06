// Builds the showcase site into dist/. Run on the macOS host (node_modules hold macOS binaries):
//   node build.mjs            # builds the prover wasm (prover/build.sh) if build/pkg is missing
//   node build.mjs --wasm     # always rebuild the prover wasm
// Everything the page loads ends up in dist/: no CDNs, no third-party fonts, no analytics.
// The circuit artifacts (withdraw.zkey is ~18 MB) are copied from the pinned circuits package;
// *.zkey is git-ignored, so they only ever live in dist/.
import * as esbuild from "esbuild";
import { execFileSync } from "node:child_process";
import { cpSync, existsSync, mkdirSync, readFileSync, rmSync, writeFileSync, readdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const HERE = dirname(fileURLToPath(import.meta.url));
const ROOT = join(HERE, "../..");
const DIST = join(HERE, "dist");
const PKG = join(HERE, "build/pkg");
const ART = process.env.LOWKI_PP_ARTIFACTS ?? join(ROOT, "../vendor/npm/circuits/package/artifacts");
const FONTS = join(ROOT, "apps/wallet-ui/assets/fonts");
const LOGO = join(ROOT, "design/logo");

if (process.argv.includes("--wasm") || !existsSync(join(PKG, "lowki_site_prover_bg.wasm"))) {
  execFileSync(join(HERE, "prover/build.sh"), [PKG], { stdio: "inherit" });
}

rmSync(DIST, { recursive: true, force: true });
mkdirSync(join(DIST, "artifacts"), { recursive: true });

// The mark's five layers, straight from the design source (no hand-copied geometry).
const svg = readFileSync(join(LOGO, "lowki-mark-night.svg"), "utf8");
const paths = [...svg.matchAll(/<path d="([^"]+)"/g)].map((m) => m[1]);
const viewBox = svg.match(/viewBox="([^"]+)"/)[1].split(/\s+/).map(Number);
const keyT = svg.match(/<g fill="#B5F23D"><path d="[^"]+" transform="([^"]+)"/)[1];
if (paths.length !== 5) throw new Error(`expected 5 paths in the mark, got ${paths.length}`);
const markModule = {
  name: "mark",
  setup(b) {
    b.onResolve({ filter: /^virtual:mark$/ }, () => ({ path: "mark", namespace: "virtual" }));
    b.onLoad({ filter: /.*/, namespace: "virtual" }, () => ({
      contents: `export const MARK = ${JSON.stringify({ viewBox, body: paths[0], rim: paths[1], keyhole: paths[2], keyholeTransform: keyT, stem: paths[3], tendril: paths[4] })};`,
      loader: "js",
    }));
  },
};

await esbuild.build({
  entryPoints: { app: join(HERE, "src/main.ts") },
  outdir: DIST,
  bundle: true,
  minify: true,
  format: "esm",
  target: "es2022",
  platform: "browser",
  plugins: [markModule],
  external: ["./pkg/*"],
  define: { "process.env.NODE_DEBUG": "false" },
  legalComments: "none",
  logLevel: "warning",
});

cpSync(join(HERE, "src/index.html"), join(DIST, "index.html"));
cpSync(join(HERE, "src/styles.css"), join(DIST, "styles.css"));
cpSync(join(ROOT, "design/tokens.css"), join(DIST, "tokens.css"));
cpSync(join(HERE, "site-config.json"), join(DIST, "site-config.json"));
cpSync(PKG, join(DIST, "pkg"), { recursive: true, filter: (f) => !f.endsWith(".d.ts") });
mkdirSync(join(DIST, "fonts"));
for (const f of readdirSync(FONTS)) cpSync(join(FONTS, f), join(DIST, "fonts", f));
mkdirSync(join(DIST, "logo"));
for (const f of readdirSync(LOGO)) cpSync(join(LOGO, f), join(DIST, "logo", f));

// Circuit artifacts (pinned @fatsolutions/privacy-pools-core-circuits 1.0.5).
for (const f of ["withdraw.wasm", "withdraw.zkey", "withdraw.vkey"]) cpSync(join(ART, f), join(DIST, "artifacts", f));
// The real withdraw inputs: vectors.json's withdraw_1500_leaves (the benchmark case).
const vectors = JSON.parse(readFileSync(join(ROOT, "vectors/lowki-groth16/vectors.json"), "utf8"));
const c = vectors.cases.find((x) => x.name === "withdraw_1500_leaves");
writeFileSync(join(DIST, "artifacts/withdraw-input.json"), JSON.stringify({ name: c.name, circuitsVersion: vectors.versions?.circuits ?? "1.0.5", input: c.input, publicSignals: c.publicSignals }));
console.log("build ok ->", DIST);
