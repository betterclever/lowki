// Node module-resolution hook so Kohaku *source files* from the pinned clone (which has
// no node_modules of its own) can be imported unmodified:
//  - `@kohaku-eth/poseidon{,-lean-imt}` -> the clone's packages/*/src/index.ts (workspace deps)
//  - other bare specifiers (e.g. `viem`) -> this harness's node_modules
// Registered by kohaku.ts. Nothing in the Kohaku clone is modified.
const KOHAKU = "/Users/betterclever/projects/ethglobal/vendor/kohaku";
const HARNESS = new URL("./package.json", import.meta.url).href;
const WORKSPACE = {
  "@kohaku-eth/poseidon": `file://${KOHAKU}/packages/poseidon/src/index.ts`,
  "@kohaku-eth/poseidon-lean-imt": `file://${KOHAKU}/packages/poseidon-lean-imt/src/index.ts`,
};

export async function resolve(specifier, context, next) {
  const fromKohaku = context.parentURL && context.parentURL.includes("/vendor/kohaku/");
  if (fromKohaku && WORKSPACE[specifier]) {
    return { url: WORKSPACE[specifier], shortCircuit: true };
  }
  try {
    return await next(specifier, context);
  } catch (err) {
    const bare = !specifier.startsWith(".") && !specifier.startsWith("/") && !specifier.includes(":");
    if (bare && fromKohaku) return next(specifier, { ...context, parentURL: HARNESS });
    throw err;
  }
}
