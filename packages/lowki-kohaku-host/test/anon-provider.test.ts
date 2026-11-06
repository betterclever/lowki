// Fallback/isolation logic of the anon-rpc provider, against a fake harness (no browser).
// Run: npm test
import { test } from "node:test";
import assert from "node:assert/strict";
import {
  ANON_NETWORKS, AnonUnavailableError, createAnonProvider, createAnonSessionPool, pickAnonNetwork,
  type AnonHarnessFactory, type AnonHarnessInit,
} from "../src/index";

const rpcResponse = (result: unknown) => new Response(JSON.stringify({ jsonrpc: "2.0", id: 1, result }));

type Seen = { url: string; isolationKey?: string; via: "anon" | "direct" };

function fakes(boot: "ok" | "fail" | "hang") {
  const seen: Seen[] = [];
  const inits: AnonHarnessInit[] = [];
  let closed = 0;
  const factory: AnonHarnessFactory = (init) => {
    inits.push(init);

    return {
      ready: boot === "ok" ? Promise.resolve() : boot === "fail" ? Promise.reject(new Error("kps dial failed")) : new Promise(() => {}),
      fetch: async (url, i) => {
        seen.push({ url, isolationKey: i?.anonOptions?.isolationKey, via: "anon" });

        return rpcResponse("0x10");
      },
      close: () => { closed++; },
    };
  };
  const fetchFn = async (url: string) => {
    seen.push({ url, via: "direct" });

    return rpcResponse("0x20");
  };

  return { seen, inits, factory, fetchFn, closed: () => closed };
}

test("pickAnonNetwork: Nox on Chromium, Nym on Safari/WebKit and every iOS browser", () => {
  assert.equal(pickAnonNetwork("Mozilla/5.0 (Macintosh) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/156.0.0.0 Safari/537.36"), "nox");
  assert.equal(pickAnonNetwork("Mozilla/5.0 (Macintosh) AppleWebKit/605.1.15 (KHTML, like Gecko) Version/19.0 Safari/605.1.15"), "nym");
  assert.equal(pickAnonNetwork("Mozilla/5.0 (iPhone; CPU iPhone OS 19_0) AppleWebKit/605.1.15 CriOS/156.0 Mobile/15E148 Safari/604.1"), "nym");
  assert.equal(pickAnonNetwork("Mozilla/5.0 (X11; Linux x86_64; rv:140.0) Gecko/20100101 Firefox/140.0"), "nox");
});

test("anonymous boot: calls go through the harness with the isolationKey, never direct", async () => {
  const f = fakes("ok");
  const r = await createAnonProvider({
    rpcUrl: "https://rpc.example/", network: "nox", isolationKey: "0xabc", harnessFactory: f.factory, fetchFn: f.fetchFn,
  });

  assert.equal(r.anonymous, true);
  assert.equal(await r.provider.getBlockNumber(), 16n);
  assert.deepEqual(f.seen, [{ url: "https://rpc.example/", isolationKey: "0xabc", via: "anon" }]);
  assert.equal(f.inits[0]!.address, ANON_NETWORKS.nox.specifier);
});

test("tor-js gets the gateway config", async () => {
  const f = fakes("ok");

  await createAnonProvider({ rpcUrl: "https://rpc.example/", network: "tor-js", torGateways: ["1.2.3.4:1:x"], harnessFactory: f.factory });
  assert.deepEqual(f.inits[0]!.config, { gateways: ["1.2.3.4:1:x"] });
});

for (const mode of ["fail", "hang"] as const) {
  test(`boot ${mode}: explicit fallback reported (anonymous: false + reason + onFallback)`, async () => {
    const f = fakes(mode);
    const reported: string[] = [];
    const r = await createAnonProvider({
      rpcUrl: "https://rpc.example/", network: "nym", bootTimeoutMs: 50, harnessFactory: f.factory, fetchFn: f.fetchFn,
      fallback: { rpcUrl: "https://direct.example/" }, onFallback: ({ reason }) => reported.push(reason),
    });

    assert.equal(r.anonymous, false);
    assert.ok(!r.anonymous && r.reason.length > 0);
    assert.match(reported[0]!, mode === "fail" ? /kps dial failed/ : /timed out after 50 ms/);
    assert.equal(await r.provider.getBlockNumber(), 32n);
    assert.deepEqual(f.seen.at(-1), { url: "https://direct.example/", via: "direct" });
    assert.ok(f.closed() >= 1, "harness closed after a failed boot");
  });
}

test("fallback: false rejects instead of going direct", async () => {
  const f = fakes("fail");

  await assert.rejects(
    createAnonProvider({ rpcUrl: "https://rpc.example/", network: "nox", fallback: false, harnessFactory: f.factory, fetchFn: f.fetchFn }),
    (e: unknown) => e instanceof AnonUnavailableError && e.network === "nox",
  );
  assert.equal(f.seen.filter((s) => s.via === "direct").length, 0);
});

test("session pool: one worker per address (isolation=worker), cached", async () => {
  const f = fakes("ok");
  const pool = createAnonSessionPool({ rpcUrl: "https://rpc.example/", network: "nox", harnessFactory: f.factory });
  const a = await pool.forAddress("0xAAA");
  const b = await pool.forAddress("0xbbb");

  assert.equal(await pool.forAddress("0xaaa"), a);
  await a.provider.getBlockNumber();
  await b.provider.getBlockNumber();
  assert.equal(f.inits.length, 2);
  assert.deepEqual(f.seen.map((s) => s.isolationKey), ["0xaaa", "0xbbb"]);
  await pool.closeAll();
  assert.equal(f.closed(), 2);
});

test("session pool: isolation=isolationKey shares one worker, distinct keys per address", async () => {
  const f = fakes("ok");
  const pool = createAnonSessionPool({ rpcUrl: "https://rpc.example/", network: "nox", isolation: "isolationKey", harnessFactory: f.factory });

  await (await pool.forAddress("0xaaa")).provider.getBlockNumber();
  await (await pool.forAddress("0xbbb")).provider.getBlockNumber();
  assert.equal(f.inits.length, 1);
  assert.deepEqual(f.seen.map((s) => s.isolationKey), ["0xaaa", "0xbbb"]);
});
