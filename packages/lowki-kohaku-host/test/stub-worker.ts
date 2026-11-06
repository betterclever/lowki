// ============================================================================================
// TEST-ONLY STUB. NOT A WALLET. Never import this from src/ or ship it.
//
// Mimics the three crates/lowki-worker requests this package uses (plus importMnemonic) in
// plain JS with @scure/bip32, @scure/bip39 and viem's signTypedData, so the host can be
// developed before the Rust worker exists. It keeps the mnemonic in a JS private field, which
// is exactly what the real worker avoids. Path scoping mirrors the Rust contract:
// strictly below m/28784'/1' plus paths registered with kohakuAllowPath.
// ============================================================================================
import { HDKey } from "@scure/bip32";
import { mnemonicToSeedSync, validateMnemonic } from "@scure/bip39";
import { wordlist } from "@scure/bip39/wordlists/english.js";
import { bytesToHex, keccak256, stringToHex, type Hex } from "viem";
import { privateKeyToAccount } from "viem/accounts";
import { parseDerivationPath } from "@kohaku-eth/plugins";
import type { LowkiWorkerLike } from "../src/transport";

const PREFIX = parseDerivationPath("m/28784'/1'");

export class StubLowkiWorker implements LowkiWorkerLike {
  #root: HDKey | undefined;
  readonly #allowed: number[][] = [];
  /** Every request type seen, for assertions about what crossed. */
  readonly log: string[] = [];

  async handle(req: any): Promise<object> {
    this.log.push(req?.type);

    try {
      return await this.#dispatch(req);
    } catch (e) {
      return { type: "error", code: "invalidRequest", message: (e as Error).message };
    }
  }

  async #dispatch(req: any): Promise<object> {
    const err = (code: string, message: string) => ({ type: "error", code, message });

    switch (req?.type) {
      case "importMnemonic": {
        // Like the Rust worker: 24-word English phrases only.
        if (typeof req.mnemonic !== "string" || req.mnemonic.split(" ").length !== 24 || !validateMnemonic(req.mnemonic, wordlist)) {
          return err("invalidRequest", "mnemonic must be a valid 24-word English BIP-39 phrase");
        }

        if (!(req.prfOutput instanceof Uint8Array) || req.prfOutput.length !== 32) return err("invalidRequest", "prfOutput must be 32 bytes");

        this.#root = HDKey.fromMasterSeed(mnemonicToSeedSync(req.mnemonic));
        // Fake share material: the stub does no wrapping/splitting.
        const fake = (n: number) => new Uint8Array(n);

        return { type: "walletCreated", wrappedBlob: fake(45), wrappedPasskeyShare: fake(46), serverShare: fake(17), recoveryShare: fake(17) };
      }
      case "kohakuAllowPath": {
        this.#allowed.push(parseDerivationPath(req.path));

        return { type: "ok" };
      }
      case "kohakuDeriveAt": {
        if (!this.#root) return err("locked", "wallet locked");

        const idx = parseDerivationPath(req.path);
        const below = idx.length > PREFIX.length && PREFIX.every((v, i) => v === idx[i]);
        const listed = this.#allowed.some((a) => a.length === idx.length && a.every((v, i) => v === idx[i]));

        if (!below && !listed) return err("pathNotAllowed", `path not allowed: ${req.path}`);

        const node = idx.reduce((n, i) => n.deriveChild(i), this.#root);

        return { type: "kohakuKey", key: bytesToHex(node.privateKey!) };
      }
      case "ppSignNoteEnvelope": {
        if (!this.#root) return err("locked", "wallet locked");

        const { accountIndex, chainId, entrypoint, depositIndex, secretIndex } = req;

        if (!Number.isInteger(accountIndex) || accountIndex < 0 || accountIndex >= 2 ** 31) return err("invalidRequest", "accountIndex");

        if (typeof chainId !== "string" || !/^[1-9]\d*$/.test(chainId)) return err("invalidRequest", "chainId");

        if (typeof entrypoint !== "string" || !/^0x[0-9a-fA-F]{40}$/.test(entrypoint)) return err("invalidRequest", "entrypoint");

        const key = this.#root.derive(`m/28784'/1'/${accountIndex}'`).privateKey!;

        const signature = await privateKeyToAccount(bytesToHex(key)).signTypedData({
          domain: { name: "PrivacyPools Keychain", version: "1", chainId: Number(chainId), verifyingContract: entrypoint as Hex, salt: keccak256(stringToHex("privacypools")) },
          types: { PrivacyPoolNote: [{ name: "depositIndex", type: "uint64" }, { name: "secretIndex", type: "uint64" }] },
          primaryType: "PrivacyPoolNote",
          message: { depositIndex: BigInt(depositIndex), secretIndex: BigInt(secretIndex) },
        });

        return { type: "noteSignature", signature };
      }
      default:
        return err("invalidRequest", `unknown request ${String(req?.type)}`);
    }
  }
}
