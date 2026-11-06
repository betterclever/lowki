import { pathToFileURL } from "node:url";

const HERE = new URL("./", import.meta.url).href;
const KOHAKU = "/Users/betterclever/projects/ethglobal/vendor/kohaku/packages";
const WORKSPACE = {
  "@kohaku-eth/provider": pathToFileURL(`${KOHAKU}/provider/src/index.ts`).href,
};

export async function resolve(specifier, context, next) {
  if (WORKSPACE[specifier]) return next(WORKSPACE[specifier], context);
  try {
    return await next(specifier, context);
  } catch (err) {
    const bare = !specifier.startsWith(".") && !specifier.startsWith("/") && !specifier.includes(":");
    if (bare) return next(specifier, { ...context, parentURL: HERE + "gen.ts" });
    throw err;
  }
}
