import { pathToFileURL, fileURLToPath } from "node:url";
import path from "node:path";

const here = path.dirname(fileURLToPath(import.meta.url));
export const KOHAKU = path.resolve(here, "../../../vendor/kohaku");
const WORKSPACE = {
  "@kohaku-eth/poseidon": "packages/poseidon/src/index.ts",
  "@kohaku-eth/poseidon-lean-imt": "packages/poseidon-lean-imt/src/index.ts",
  "@kohaku-eth/plugins": "packages/plugins/src/index.ts",
  "@kohaku-eth/provider": "packages/provider/src/index.ts",
};
const vendorPrefix = pathToFileURL(KOHAKU).href + "/";
const localParent = pathToFileURL(path.join(here, "gen.ts")).href;

export async function resolve(specifier, context, next) {
  if (WORKSPACE[specifier]) {
    return next(pathToFileURL(path.join(KOHAKU, WORKSPACE[specifier])).href, context);
  }
  const bare = !specifier.startsWith(".") && !specifier.startsWith("/") && !specifier.includes(":");
  if (bare && context.parentURL && context.parentURL.startsWith(vendorPrefix)) {
    try {
      return await next(specifier, { ...context, parentURL: localParent });
    } catch (e) {
      // Extensionless deep imports into CJS packages (e.g. "maci-crypto/build/ts/hashing").
      return next(specifier + ".js", { ...context, parentURL: localParent });
    }
  }
  return next(specifier, context);
}
