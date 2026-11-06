import type { Storage } from "@kohaku-eth/plugins";

/** Raw string key/value store underneath {@link EncryptedStorage}. */
export interface KeyValueBackend {
  get(key: string): Promise<string | null>;
  set(key: string, value: string): Promise<void>;
}

/**
 * Supplies the AES-256-GCM key used to encrypt plugin state at rest. In Lowki this key should
 * come from the worker (derived from the unlocked seed, so storage is unreadable while locked and
 * without the passkey) and be imported as a non-extractable CryptoKey. The worker has no request
 * for it yet; until it does, the caller passes any provider. It is called on every access, so a
 * provider may throw while the wallet is locked.
 */
export type StorageKeyProvider = () => Promise<CryptoKey>;

/** In-memory backend (tests, Node). */
export class MemoryBackend implements KeyValueBackend {
  readonly #map = new Map<string, string>();

  async get(key: string) {
    return this.#map.get(key) ?? null;
  }

  async set(key: string, value: string) {
    this.#map.set(key, value);
  }

  /** Raw (ciphertext) view, for tests. */
  raw(): ReadonlyMap<string, string> {
    return this.#map;
  }
}

/** Browser backend on IndexedDB (one object store; values are strings). Preferred over
 * localStorage: Kohaku stores each pool state as a single JSON value, which reaches MBs. */
export class IndexedDBBackend implements KeyValueBackend {
  #db: Promise<IDBDatabase> | undefined;

  constructor(private readonly dbName = "lowki-kohaku", private readonly storeName = "kv") {}

  private db(): Promise<IDBDatabase> {
    this.#db ??= new Promise((resolve, reject) => {
      const req = indexedDB.open(this.dbName, 1);

      req.onupgradeneeded = () => req.result.createObjectStore(this.storeName);
      req.onsuccess = () => resolve(req.result);
      req.onerror = () => reject(req.error);
    });

    return this.#db;
  }

  private async tx<T>(mode: IDBTransactionMode, run: (s: IDBObjectStore) => IDBRequest<T>): Promise<T> {
    const db = await this.db();

    return new Promise<T>((resolve, reject) => {
      const req = run(db.transaction(this.storeName, mode).objectStore(this.storeName));

      req.onsuccess = () => resolve(req.result);
      req.onerror = () => reject(req.error);
    });
  }

  async get(key: string) {
    const v = await this.tx("readonly", (s) => s.get(key));

    return typeof v === "string" ? v : null;
  }

  async set(key: string, value: string) {
    await this.tx("readwrite", (s) => s.put(value, key));
  }
}

/** Browser backend on localStorage (small states only; ~5 MB per origin). */
export class LocalStorageBackend implements KeyValueBackend {
  constructor(private readonly prefix = "lowki-kohaku:") {}

  async get(key: string) {
    return localStorage.getItem(this.prefix + key);
  }

  async set(key: string, value: string) {
    localStorage.setItem(this.prefix + key, value);
  }
}

const VERSION = "lk1:";
const enc = new TextEncoder();
const dec = new TextDecoder();

const toB64 = (b: Uint8Array) => {
  let s = "";

  for (const x of b) s += String.fromCharCode(x);

  return btoa(s);
};
const fromB64 = (s: string) => Uint8Array.from(atob(s), (c) => c.charCodeAt(0));

/**
 * Kohaku `Storage` that encrypts every value with AES-GCM (WebCrypto): `lk1:` + base64(iv12 ‖
 * ciphertext‖tag). The storage key name is the AAD, so a ciphertext cannot be moved to another
 * key. Key *names* are stored in clear (Kohaku uses e.g. `privacy-pool-state-<chainId>-<entrypoint>`).
 */
export class EncryptedStorage implements Storage {
  readonly _brand = "Storage" as const;

  constructor(private readonly backend: KeyValueBackend, private readonly key: StorageKeyProvider) {}

  async set(name: string, value: string): Promise<void> {
    const iv = crypto.getRandomValues(new Uint8Array(12));
    const ct = new Uint8Array(await crypto.subtle.encrypt(
      { name: "AES-GCM", iv, additionalData: enc.encode(name) },
      await this.key(),
      enc.encode(value),
    ));
    const out = new Uint8Array(12 + ct.length);

    out.set(iv);
    out.set(ct, 12);
    await this.backend.set(name, VERSION + toB64(out));
  }

  async get(name: string): Promise<string | null> {
    const stored = await this.backend.get(name);

    if (stored === null) return null;

    if (!stored.startsWith(VERSION)) throw new Error(`EncryptedStorage: ${name} is not an lk1 ciphertext`);

    const raw = fromB64(stored.slice(VERSION.length));
    const pt = await crypto.subtle.decrypt(
      { name: "AES-GCM", iv: raw.subarray(0, 12), additionalData: enc.encode(name) },
      await this.key(),
      raw.subarray(12),
    );

    return dec.decode(pt);
  }
}

/** Test/dev helper: a fresh random non-extractable AES-256-GCM key. NOT how Lowki should key storage. */
export async function randomStorageKey(): Promise<StorageKeyProvider> {
  const key = await crypto.subtle.generateKey({ name: "AES-GCM", length: 256 }, false, ["encrypt", "decrypt"]);

  return async () => key;
}
