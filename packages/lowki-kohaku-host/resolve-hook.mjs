// Registers resolve-hook-impl.mjs so the pinned Kohaku *source files* (read-only clone,
// no node_modules) can be imported from Node. Use: node --import ./resolve-hook.mjs --import tsx ...
import { register } from "node:module";

register("./resolve-hook-impl.mjs", import.meta.url);
