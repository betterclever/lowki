// The only place this package imports @kohaku-eth/privacy-pools. It deep-imports the source
// modules instead of the package index because the index also re-exports src/v2/index.ts,
// which does not typecheck at the pinned commit (v2/index.ts:50, PPv2Broadcaster violates
// Broadcaster's constraint; v2/index.ts:3 imports `Address` from Node's 'cluster'). Against a
// published build, replace these with `from "@kohaku-eth/privacy-pools"`.
export { PrivacyPoolsV1Protocol } from "@kohaku-eth/privacy-pools/src/plugin/base";
export { SignatureSecretManager, KeystoreEnvelopeSigner } from "@kohaku-eth/privacy-pools/src/account/signature-keys";
export type { EnvelopeSigner, NoteEnvelope } from "@kohaku-eth/privacy-pools/src/account/signature-keys";
export { PRIVACY_POOLS_KEYSTORE_PREFIX } from "@kohaku-eth/privacy-pools/src/account/keys";
export type { ISecretManager, SecretManagerParams } from "@kohaku-eth/privacy-pools/src/account/keys";
export { PrivacyPoolsV1_0xBow, E_ADDRESS } from "@kohaku-eth/privacy-pools/src/config";
export { IPFSAspService } from "@kohaku-eth/privacy-pools/src/data/ipfsAsp.service";
export { OxBowAspService } from "@kohaku-eth/privacy-pools/src/data/0xbowAsp.service";
export { DataService } from "@kohaku-eth/privacy-pools/src/data/data.service";
export { EthClient } from "@kohaku-eth/privacy-pools/src/data/eth-client";
export type { GetLogsParams } from "@kohaku-eth/privacy-pools/src/data/eth-client";
