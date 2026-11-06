// Bundles page.ts for the browser. Kohaku is consumed from source (vendor/kohaku), the same
// mapping as ../resolve-hook-impl.mjs; Kohaku's bare deps resolve to ../node_modules.
// Run (Linux or mac): node browser-anon/build.mjs
import { build } from "esbuild";
import path from "node:path";
import { fileURLToPath } from "node:url";

const here = path.dirname(fileURLToPath(import.meta.url));
const pkg = path.resolve(here, "..");
const KOHAKU = process.env.KOHAKU_DIR ?? path.resolve(pkg, "../../../vendor/kohaku");

await build({
  entryPoints: [path.join(here, "page.ts")],
  bundle: true,
  format: "esm",
  target: "es2022",
  platform: "browser",
  outfile: path.join(here, "dist/page.js"),
  alias: {
    "@kohaku-eth/provider/raw": path.join(KOHAKU, "packages/provider/src/raw/index.ts"),
    "@kohaku-eth/provider": path.join(KOHAKU, "packages/provider/src/index.ts"),
    "@kohaku-eth/privacy-pools/src/data/eth-client": path.join(KOHAKU, "packages/privacy-pools/src/data/eth-client.ts"),
  },
  nodePaths: [path.join(pkg, "node_modules")],
  logLevel: "warning",
});
console.log("built browser-anon/dist/page.js");
