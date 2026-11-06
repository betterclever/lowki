// Lets the pinned Kohaku sources (which have no node_modules of their own) resolve
// their bare-specifier deps (viem, ox, ...) from this directory's node_modules, and
// maps the workspace package @kohaku-eth/provider to its source in the clone.
import { register } from "node:module";

register("./resolve-hooks-impl.mjs", import.meta.url);
