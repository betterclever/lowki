// Bundles the wallet glue (entry.ts + the lazy private.ts chunk) for apps/wallet-ui, which
// copies browser-wallet/dist/ to lowki-glue/ (index.html). Same Kohaku source mapping as
// ../resolve-hook-impl.mjs. Run (Linux or mac): node browser-wallet/build.mjs
import { build } from "esbuild";
import { existsSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const pkg = path.resolve(here, "..");
const KOHAKU = process.env.KOHAKU_DIR ?? path.resolve(pkg, "../../../vendor/kohaku");
const P = (p) => path.join(KOHAKU, "packages", p);
const WORKSPACE = {
  "@kohaku-eth/plugins": P("plugins/src/index.ts"),
  "@kohaku-eth/plugins/broadcaster": P("plugins/src/broadcaster/base.ts"),
  "@kohaku-eth/provider": P("provider/src/index.ts"),
  "@kohaku-eth/provider/viem": P("provider/src/viem/index.ts"),
  "@kohaku-eth/provider/raw": P("provider/src/raw/index.ts"),
  "@kohaku-eth/poseidon": P("poseidon/src/index.ts"),
  "@kohaku-eth/poseidon-lean-imt": P("poseidon-lean-imt/src/index.ts"),
  "@kohaku-eth/privacy-pools": P("privacy-pools/src/index.ts"),
};
const PP_SRC = "@kohaku-eth/privacy-pools/src/";
const pluginsSrc = P("plugins/src");

const kohaku = {
  name: "kohaku-source",
  setup(b) {
    b.onResolve({ filter: /^@kohaku-eth\// }, (a) => {
      if (WORKSPACE[a.path]) return { path: WORKSPACE[a.path] };
      if (a.path.startsWith(PP_SRC)) return { path: P(`privacy-pools/src/${a.path.slice(PP_SRC.length).replace(/\.js$/, "")}.ts`) };
      return undefined;
    });
    // maci-crypto requires Node's `assert` (only as assert(cond)).
    b.onResolve({ filter: /^assert$/ }, () => ({ path: "assert", namespace: "shim" }));
    b.onLoad({ filter: /.*/, namespace: "shim" }, () => ({
      contents: "function assert(c, m) { if (!c) throw new Error(m || 'assertion failed'); }\nassert.ok = assert;\nmodule.exports = assert;\nmodule.exports.default = assert;",
      loader: "js",
    }));
    // Kohaku plugins' `~/x` tsconfig alias.
    b.onResolve({ filter: /^~\// }, (a) => {
      const base = path.join(pluginsSrc, a.path.slice(2));
      for (const c of [base + ".ts", path.join(base, "index.ts")]) if (existsSync(c)) return { path: c };
      return undefined;
    });
  },
};

await build({
  entryPoints: { "lowki-glue": path.join(here, "entry.ts") },
  bundle: true,
  splitting: true,
  format: "esm",
  target: "es2022",
  platform: "browser",
  outdir: path.join(here, "dist"),
  chunkNames: "chunks/[name]-[hash]",
  plugins: [kohaku],
  nodePaths: [path.join(pkg, "node_modules")],
  // Node-only branches in snarkjs/ffjavascript/ethers are never taken in the browser.
  external: ["worker_threads", "node:*", "fs", "os", "crypto", "path", "url", "readline", "constants", "cluster"],
  define: { "process.env.NODE_DEBUG": "false", global: "globalThis" },
  minify: process.env.MINIFY !== "0",
  sourcemap: false,
  legalComments: "none",
  logLevel: "warning",
});
console.log("built browser-wallet/dist/lowki-glue.js");
