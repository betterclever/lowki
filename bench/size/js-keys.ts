// Equivalent of lowki-keys' wasm surface: Kohaku keystore (BIP-39/32) + viem signing + EIP-55 + WebCrypto wrap (built in, 0 bytes).
export { MnemonicKeystore } from "@K/plugins/src/host/mnemonic-keystore.ts";
export { sign, privateKeyToAddress } from "viem/accounts";
export { getAddress, keccak256 } from "viem";
