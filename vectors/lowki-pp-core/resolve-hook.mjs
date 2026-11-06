// Node module-resolution hook so the pinned Kohaku *source files* can be imported
// directly from the read-only vendor clone (which has no node_modules):
//  - `@kohaku-eth/<pkg>` resolves to the workspace package's `src/index.ts`;
//  - any other bare specifier imported from a vendor file resolves against this
//    directory's node_modules (exact versions pinned in package.json, matching
//    Kohaku's pnpm-lock.yaml).
import { register } from "node:module";

register("./resolve-hook-impl.mjs", import.meta.url);
