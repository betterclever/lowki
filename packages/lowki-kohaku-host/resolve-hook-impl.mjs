// Node module-resolution hook for the pinned Kohaku clone
// (vendor/kohaku @ 8c83d64050040654082d70901a46a339bfa103b0, read-only):
//  - `@kohaku-eth/<pkg>[/<subpath>]` -> the workspace package's TypeScript source;
//  - `~/<x>` imported from packages/plugins/src -> packages/plugins/src/<x> (its tsconfig path alias);
//  - any other bare specifier imported from a vendor file resolves against this package's
//    node_modules (exact versions pinned in package.json, matching Kohaku's pnpm-lock.yaml).
import { pathToFileURL, fileURLToPath } from "node:url";
import { existsSync } from "node:fs";
import path from "node:path";

const here = path.dirname(fileURLToPath(import.meta.url));
export const KOHAKU = process.env.KOHAKU_DIR ?? path.resolve(here, "../../../vendor/kohaku");
const P = (p) => pathToFileURL(path.join(KOHAKU, "packages", p)).href;
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
const vendorPrefix = pathToFileURL(KOHAKU).href + "/";
const pluginsSrc = path.join(KOHAKU, "packages/plugins/src");
const localParent = pathToFileURL(path.join(here, "package.json")).href;

function tilde(specifier) {
  const base = path.join(pluginsSrc, specifier.slice(2));
  for (const cand of [base + ".ts", path.join(base, "index.ts")]) {
    if (existsSync(cand)) return pathToFileURL(cand).href;
  }
  return null;
}

const PP_SRC = "@kohaku-eth/privacy-pools/src/";

export async function resolve(specifier, context, next) {
  if (WORKSPACE[specifier]) return next(WORKSPACE[specifier], context);
  // Deep imports of privacy-pools source modules (see src/kohaku-pp.ts).
  if (specifier.startsWith(PP_SRC)) {
    const rel = specifier.slice(PP_SRC.length).replace(/\.js$/, "");
    return next(P(`privacy-pools/src/${rel}.ts`), context);
  }
  if (specifier.startsWith("~/") && context.parentURL?.startsWith(vendorPrefix)) {
    const hit = tilde(specifier);
    if (hit) return next(hit, context);
  }
  const bare = !specifier.startsWith(".") && !specifier.startsWith("/") && !specifier.includes(":")
    && !specifier.startsWith("~/");
  if (bare && context.parentURL?.startsWith(vendorPrefix)) {
    try {
      return await next(specifier, { ...context, parentURL: localParent });
    } catch {
      // Extensionless deep imports into CJS packages (e.g. "maci-crypto/build/ts/hashing").
      return next(specifier + ".js", { ...context, parentURL: localParent });
    }
  }
  return next(specifier, context);
}
