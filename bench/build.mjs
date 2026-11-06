// Bundles the TS side with esbuild (production: minified, tree-shaken).
//   dist/*.mjs   Node entry points
//   web/app.js   Chrome entry point (+ web/limt-worker.js for Kohaku's parallel LeanIMT)
//   size/out/*   one minified bundle per area, for the payload-size comparison
// Kohaku sources come straight from the pinned clone (read-only): `@K/...` and
// `@kohaku-eth/*` are aliased to vendor/kohaku/packages, and their bare imports resolve
// against this directory's node_modules (versions = Kohaku's pnpm-lock.yaml).
import * as esbuild from "esbuild";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

const HERE = dirname(fileURLToPath(import.meta.url));
const KOHAKU = join(HERE, "../../vendor/kohaku/packages");
const WORKSPACE = {
  "@kohaku-eth/poseidon": "poseidon/src/index.ts",
  "@kohaku-eth/poseidon-lean-imt": "poseidon-lean-imt/src/index.ts",
  "@kohaku-eth/plugins": "plugins/src/index.ts",
  "@kohaku-eth/provider": "provider/src/index.ts",
};
const kohaku = {
  name: "kohaku",
  setup(b) {
    b.onResolve({ filter: /^@K\// }, (a) => ({ path: join(KOHAKU, a.path.slice(3)) }));
    b.onResolve({ filter: /^@kohaku-eth\// }, (a) => (WORKSPACE[a.path] ? { path: join(KOHAKU, WORKSPACE[a.path]) } : undefined));
  },
};
const common = {
  bundle: true,
  minify: true,
  format: "esm",
  target: "es2022",
  plugins: [kohaku],
  nodePaths: [join(HERE, "node_modules")],
  logLevel: "warning",
  legalComments: "none",
};
const nodeCfg = {
  ...common,
  platform: "node",
  // snarkjs/ffjavascript start worker_threads from their own files (web-worker uses __filename),
  // so in Node they stay unbundled and load from node_modules.
  external: ["snarkjs"],
  banner: { js: "import { createRequire as __cr } from 'module'; const require = __cr(import.meta.url);" },
};
const webCfg = { ...common, platform: "browser", define: { "process.env.NODE_DEBUG": "false" }, alias: { assert: join(HERE, "js/assert-shim.cjs") } };

const only = process.argv[2];
const jobs = [];
if (!only || only === "node") {
  jobs.push(esbuild.build({ ...nodeCfg, entryPoints: { "gen-fixtures": "js/gen-fixtures.ts", "node-bench": "js/node-main.ts" }, outdir: "dist", outExtension: { ".js": ".mjs" } }));
  jobs.push(esbuild.build({ ...nodeCfg, entryPoints: { "limt-worker": join(KOHAKU, "poseidon-lean-imt/src/worker.ts") }, outdir: "dist", outExtension: { ".js": ".mjs" } }));
}
if (!only || only === "web") {
  jobs.push(esbuild.build({ ...webCfg, entryPoints: { app: "js/browser-main.ts", "limt-worker": join(KOHAKU, "poseidon-lean-imt/src/worker.ts") }, outdir: "web", external: ["./pkg/*"] }));
}
if (!only || only === "size") {
  jobs.push(esbuild.build({ ...webCfg, entryPoints: { keys: "size/js-keys.ts", pp: "size/js-pp.ts", stealth: "size/js-stealth.ts", aa: "size/js-aa.ts", g16: "size/js-g16.ts", all: "size/js-all.ts" }, outdir: "size/out", metafile: true })
    .then((r) => import("node:fs").then((fs) => fs.writeFileSync("size/out/meta.json", JSON.stringify(r.metafile)))));
}
await Promise.all(jobs);
console.log("build ok");
