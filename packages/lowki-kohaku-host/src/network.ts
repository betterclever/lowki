import type { Network } from "@kohaku-eth/plugins";

export type NetworkOptions = {
  /**
   * Hosts plugins may reach. `"example.com"` matches that host exactly; `"*.example.com"` matches
   * subdomains only. Omit to allow every host (plain passthrough).
   */
  allowHosts?: readonly string[];
  /** Require https: (default true; http://localhost and 127.0.0.1 are always allowed). */
  httpsOnly?: boolean;
  /** Underlying fetch (default globalThis.fetch). */
  fetch?: typeof fetch;
};

export class NetworkHostNotAllowedError extends Error {
  constructor(url: string) {
    super(`Network request to ${url} is not allowed by the Lowki host allowlist`);
    this.name = "NetworkHostNotAllowedError";
  }
}

const hostMatches = (host: string, pattern: string) => {
  const p = pattern.toLowerCase();

  return p.startsWith("*.") ? host.endsWith(p.slice(1)) && host.length > p.length - 1 : host === p;
};

/** Kohaku `Network`: a fetch passthrough with an optional host allowlist. */
export function createNetwork({ allowHosts, httpsOnly = true, fetch: inner = globalThis.fetch }: NetworkOptions = {}): Network {
  const check = (input: RequestInfo | URL) => {
    const url = new URL(typeof input === "string" ? input : input instanceof URL ? input.href : input.url);
    const local = url.hostname === "localhost" || url.hostname === "127.0.0.1";

    if (httpsOnly && url.protocol !== "https:" && !local) throw new NetworkHostNotAllowedError(url.href);

    if (allowHosts && !allowHosts.some((p) => hostMatches(url.hostname.toLowerCase(), p))) {
      throw new NetworkHostNotAllowedError(url.href);
    }
  };

  return {
    async fetch(input, init) {
      check(input);

      return inner(input, init);
    },
  };
}
