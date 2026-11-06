// Imports from the pinned Kohaku clone (commit 8c83d64050040654082d70901a46a339bfa103b0).
import { register } from "node:module";
register(new URL("./kohaku-resolve.mjs", import.meta.url).href);

export const KOHAKU = "/Users/betterclever/projects/ethglobal/vendor/kohaku";
export const KOHAKU_COMMIT = "8c83d64050040654082d70901a46a339bfa103b0";

/** `toWithdrawProof` + `encodeRagequitPayload` (packages/privacy-pools/src/utils/encoding.utils.ts) */
export const kohakuEncoding = () => import(`${KOHAKU}/packages/privacy-pools/src/utils/encoding.utils.ts`);
/** `PoseidonT3LeanIMT` (packages/poseidon-lean-imt/src/limt.ts) */
export const kohakuLimt = () => import(`${KOHAKU}/packages/poseidon-lean-imt/src/limt.ts`);
/** `poseidonT3` (packages/poseidon/src/index.ts) */
export const kohakuPoseidon = () => import(`${KOHAKU}/packages/poseidon/src/index.ts`);
/** `proofFromTree`, `calculateContext` (packages/privacy-pools/src/utils/proof.util.ts) */
export const kohakuProofUtil = () => import(`${KOHAKU}/packages/privacy-pools/src/utils/proof.util.ts`);
