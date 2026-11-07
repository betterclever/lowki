#!/usr/bin/env bash
# Lowki live demo: one command that starts everything for the recording and the judges' link.
#
#   scripts/demo-up.sh          # idempotent: reuses whatever is already running (same URLs)
#   scripts/demo-check.sh       # health check of every public URL
#   scripts/demo-down.sh        # stops only what this script started (keeps the :8830 tunnel)
#
# What runs (VM ports 8850-8859, plus lowki-server on 8830; tunnels are cloudflared quick
# tunnels started on the Mac with `mac sh -lc`, reaching the VM through OrbStack's forwarding):
#
#   8830  lowki-server: auth (passkeys, email codes, app tokens, JWKS) + the ENS CCIP-Read gateway
#         for *.lowki.eth. Public through the EXISTING tunnel whose URL is written on chain
#         (contracts/deployments/sepolia-lowki.json .gatewayUrl). That tunnel is never restarted
#         here; if it died, see scripts/update-gateway-url.sh.
#   8850  anvil fork of Sepolia at a pinned block that contains the live lowki.eth contracts and
#         the creators chai/tabla/pixel (binary copied to demo-live-anvil)
#   8851  plain-HTTP forwarder to a public Sepolia RPC (lowki-server's ENS reads, the e2e CLI)
#   8852  fork gateway (packages/lowki-kohaku-host/demo/stack.ts): /rpc (eth_* only, logged),
#         /ip, the simulated ASP (/asp) and relayer (/relayer)          -> tunnel "fork"
#   8853  stack.ts's own wallet server (internal: /circuits, /pp-snapshot.json)
#   8854  stack.ts's demo-dapp server (internal, unused)
#   8855  wallet (wallet-ui dist; /v1 -> lowki-server, /rpc -> fork gateway, /lowki-config.json)
#                                                                        -> tunnel "wallet"
#   8856  Lowki Tips (+ /rpc -> the fork, eth_* and anvil_setBalance for the faucet)
#                                                                        -> tunnel "tips"
#   8857  privy-swap (examples/privy-swap, built)                         -> tunnel "privy"
#   8858  showcase site (copy of apps/site/dist + its serve.mjs)         -> tunnel "site"
#   8859  (scratch: Kohaku snapshot generation for a new fork block)
#   8860  lowki-server, only if :8830 is held by a server this script did not start
#
# Env:
#   BUILD=1        rebuild lowki-server, the wallet, privy-swap and Tips even if builds exist
#   FRESH_FORK=1   re-fork at the latest Sepolia block (generates a Kohaku snapshot, ~3 min)
#   FORK_URL       upstream for anvil (default https://sepolia.gateway.tenderly.co)
#   SEPOLIA_RPC    upstream for the :8851 forwarder (default https://ethereum-sepolia-rpc.publicnode.com)
#   WALLET_DIST    use this wallet-ui dist instead of building one
#   SITE_REPO_URL, SITE_VIDEO_URL   links on the showcase site (default repo github.com/betterclever/lowki)
#
# State: target/demo-live/ (run/ has pids, logs, URLs; server/ has the database and secrets).
# Secrets (never printed): /Users/betterclever/.lowki/sepolia-test.env (operator key, used by
# lowki-server for sponsored name claims), /Users/betterclever/.lowki/creators.env.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
LIVE="$ROOT/target/demo-live"
RUN="$LIVE/run"
BIN="$LIVE/bin"
SECRETS_DIR="${LOWKI_SECRETS_DIR:-/Users/betterclever/.lowki}"
export PATH="$HOME/.foundry/bin:$HOME/.cargo/bin:$PATH"
mkdir -p "$RUN" "$BIN" "$LIVE/server"

P_SERVER=8830 P_ANVIL=8850 P_SEPOLIA=8851 P_GATEWAY=8852 P_STACKW=8853 P_STACKD=8854
P_WALLET=8855 P_TIPS=8856 P_PRIVY=8857 P_SITE=8858 P_SCRATCH=8859 P_AUTH=8860
FORK_URL="${FORK_URL:-https://sepolia.gateway.tenderly.co}"
SEPOLIA_RPC="${SEPOLIA_RPC:-https://ethereum-sepolia-rpc.publicnode.com}"
DEPLOY="$ROOT/contracts/deployments/sepolia-lowki.json"
ENSV2="$ROOT/contracts/deployments/sepolia-ensv2.json"
GW_URL="$(jq -r .gatewayUrl "$DEPLOY")"               # written on chain; served by :8830
GW_BASE="${GW_URL%/v1/ens/gateway}"
PKG="$ROOT/packages/lowki-kohaku-host"

say() { printf '== %s\n' "$*"; }
die() { printf 'demo-up: %s\n' "$*" >&2; exit 1; }
port_open() { (exec 3<>"/dev/tcp/127.0.0.1/$1") 2>/dev/null; }
alive() { [ -f "$RUN/$1.pid" ] && kill -0 "$(cat "$RUN/$1.pid")" 2>/dev/null; }
wait_port() { for _ in $(seq 1 "${2:-120}"); do port_open "$1" && return 0; sleep 0.5; done; return 1; }
# start NAME PORT CMD...: runs CMD detached (survives this shell), pid in run/NAME.pid, log in run/NAME.log.
start() {
  local name=$1 port=$2; shift 2
  if alive "$name"; then echo "   $name: running (pid $(cat "$RUN/$name.pid"))"; return 0; fi
  if [ "$port" != - ] && port_open "$port"; then die "port $port is in use by something this script did not start ($name)"; fi
  setsid nohup "$@" >"$RUN/$name.log" 2>&1 < /dev/null &
  echo $! >"$RUN/$name.pid"
  if [ "$port" != - ] && ! wait_port "$port" 240; then tail -30 "$RUN/$name.log" >&2; die "$name did not open :$port"; fi
  echo "   $name: started (pid $(cat "$RUN/$name.pid"), log run/$name.log)"
}
stop() { if alive "$1"; then kill "$(cat "$RUN/$1.pid")" 2>/dev/null || true; for _ in $(seq 1 20); do alive "$1" || break; sleep 0.25; done; fi; rm -f "$RUN/$1.pid"; }
rpc() { curl -sf -X POST -H 'content-type: application/json' --data "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"$2\",\"params\":${3:-[]}}" "$1"; }

# ------------------------------------------------------------------------------ helpers on disk
# Small Node helpers, written fresh on every run so this script is their only source.
cat >"$BIN/serve.mjs" <<'EOF'
// demo-live static server with same-origin proxies (written by scripts/demo-up.sh).
// Env: PORT, ROOT, V1 (lowki-server base: /v1/*, /.well-known/*), RPC (JSON-RPC upstream for /rpc),
// RPC_ALLOW (regex of allowed methods), CONFIG (file served as /lowki-config.json),
// ASSETS (upstream for /circuits/* and /pp-snapshot.json). Binds 127.0.0.1.
import http from 'node:http';
import fs from 'node:fs';
import path from 'node:path';
const E = process.env;
const PORT = Number(E.PORT);
const ROOT = path.resolve(E.ROOT);
const ALLOW = E.RPC_ALLOW ? new RegExp(E.RPC_ALLOW) : null;
const DENY = new Set(['eth_sendTransaction', 'eth_sign', 'eth_signTransaction', 'eth_accounts', 'eth_signTypedData_v4']);
const TYPES = {
  '.html': 'text/html; charset=utf-8', '.js': 'text/javascript', '.mjs': 'text/javascript', '.wasm': 'application/wasm',
  '.css': 'text/css', '.svg': 'image/svg+xml', '.woff2': 'font/woff2', '.png': 'image/png', '.json': 'application/json',
  '.zkey': 'application/octet-stream', '.ico': 'image/x-icon', '.txt': 'text/plain; charset=utf-8',
};
function proxy(req, res, target, extra = {}) {
  const up = new URL(target);
  const p = http.request(up, { method: req.method, headers: { ...req.headers, host: up.host } }, (r) => {
    res.writeHead(r.statusCode ?? 502, { ...r.headers, ...extra });
    r.pipe(res);
  });
  p.on('error', () => { if (!res.headersSent) res.writeHead(502, { 'content-type': 'application/json' }); res.end('{"error":"upstream unavailable"}'); });
  req.pipe(p);
}
async function rpc(req, res) {
  const chunks = [];
  for await (const c of req) chunks.push(c);
  const body = Buffer.concat(chunks).toString();
  let calls;
  try { const j = JSON.parse(body); calls = Array.isArray(j) ? j : [j]; } catch { res.writeHead(400); return res.end('bad json'); }
  const bad = calls.find((c) => DENY.has(String(c.method)) || (ALLOW && !ALLOW.test(String(c.method))));
  if (bad) {
    res.writeHead(200, { 'content-type': 'application/json' });
    return res.end(JSON.stringify({ jsonrpc: '2.0', id: bad.id ?? null, error: { code: -32601, message: `method ${bad.method} is not served here` } }));
  }
  try {
    const r = await fetch(E.RPC, { method: 'POST', headers: { 'content-type': 'application/json' }, body });
    res.writeHead(r.status, { 'content-type': 'application/json', 'cache-control': 'no-store' });
    res.end(Buffer.from(await r.arrayBuffer()));
  } catch { res.writeHead(502); res.end('{"error":"rpc upstream unavailable"}'); }
}
http.createServer((req, res) => {
  const url = new URL(req.url ?? '/', 'http://x');
  const p = url.pathname;
  // ENS records live with the gateway that answered the lookups (whatever serves the on-chain :8830 URL).
  if (E.V1_ENS && p.startsWith('/v1/ens/')) return proxy(req, res, new URL(req.url, E.V1_ENS).toString());
  if (E.V1 && (p.startsWith('/v1/') || p.startsWith('/.well-known/'))) return proxy(req, res, new URL(req.url, E.V1).toString());
  if (E.RPC && p === '/rpc') return req.method === 'POST' ? void rpc(req, res) : (res.writeHead(405), res.end());
  if (E.ASSETS && (p.startsWith('/circuits/') || p === '/pp-snapshot.json')) {
    return proxy(req, res, new URL(req.url, E.ASSETS).toString(), p.startsWith('/circuits/') ? { 'cache-control': 'public, max-age=86400' } : {});
  }
  if (E.CONFIG && p === '/lowki-config.json') {
    res.writeHead(200, { 'content-type': 'application/json', 'cache-control': 'no-store' });
    return void res.end(fs.readFileSync(E.CONFIG));
  }
  let rel = decodeURIComponent(p);
  if (rel.endsWith('/')) rel += 'index.html';
  const file = path.join(ROOT, rel);
  if (!file.startsWith(ROOT) || !fs.existsSync(file) || fs.statSync(file).isDirectory()) { res.writeHead(404); return void res.end('not found'); }
  const ext = path.extname(file);
  res.writeHead(200, {
    'content-type': TYPES[ext] ?? 'application/octet-stream',
    'content-length': fs.statSync(file).size,
    'cache-control': ext === '.wasm' || ext === '.woff2' ? 'public, max-age=3600' : 'no-store',
  });
  if (req.method === 'HEAD') return void res.end();
  fs.createReadStream(file).pipe(res);
}).listen(PORT, '127.0.0.1', () => console.log(`serving ${ROOT} on http://127.0.0.1:${PORT}`));
EOF

cat >"$BIN/sepolia-forwarder.mjs" <<'EOF'
// Plain-HTTP JSON-RPC forwarder on 127.0.0.1 (written by scripts/demo-up.sh). Env: PORT, UPSTREAM.
import http from 'node:http';
const PORT = Number(process.env.PORT), UP = process.env.UPSTREAM;
http.createServer(async (req, res) => {
  const chunks = [];
  for await (const c of req) chunks.push(c);
  try {
    const r = await fetch(UP, { method: 'POST', headers: { 'content-type': 'application/json' }, body: Buffer.concat(chunks) });
    res.writeHead(r.status, { 'content-type': 'application/json' });
    res.end(Buffer.from(await r.arrayBuffer()));
  } catch (e) { res.writeHead(502); res.end(String(e)); }
}).listen(PORT, '127.0.0.1', () => console.log(`sepolia forwarder :${PORT} -> ${UP}`));
EOF

# ------------------------------------------------------------------------------ prerequisites
command -v anvil >/dev/null || die "anvil not found (foundryup)"
command -v jq >/dev/null || die "jq not found"
command -v mac >/dev/null || die "the 'mac' helper is missing: tunnels are started on the Mac with mac sh -lc"
[ -f "$ROOT/crates/lowki-worker/pkg/web/lowki_worker.js" ] || die "build the worker first: (cd crates/lowki-worker && make pkg)"
[ -f "$SECRETS_DIR/sepolia-test.env" ] || die "missing $SECRETS_DIR/sepolia-test.env (operator key)"
[ -d "$PKG/node_modules" ] || (cd "$PKG" && npm ci --no-audit --no-fund)

# ------------------------------------------------------------------------------ builds (once)
say "builds (BUILD=1 forces)"
SERVER_BIN="$BIN/lowki-server"
if [ "${BUILD:-}" = 1 ] || [ ! -x "$SERVER_BIN" ]; then
  echo "   lowki-server (cargo build --release)"
  (cd "$ROOT/crates/lowki-server" && CARGO_TARGET_DIR="$LIVE/cargo-server" cargo build --release --quiet)
  cp -f "$LIVE/cargo-server/release/lowki-server" "$SERVER_BIN"
fi
if [ "${BUILD:-}" = 1 ] || [ ! -f "$PKG/browser-wallet/dist/lowki-glue.js" ]; then
  echo "   wallet glue (browser-wallet/build.mjs)"
  (cd "$PKG" && node browser-wallet/build.mjs >"$RUN/glue-build.log" 2>&1) || { tail -20 "$RUN/glue-build.log"; die "glue build failed"; }
fi
WDIST="${WALLET_DIST:-$LIVE/wallet-dist}"
if [ -z "${WALLET_DIST:-}" ] && { [ "${BUILD:-}" = 1 ] || [ ! -f "$WDIST/index.html" ]; }; then
  echo "   wallet-ui (trunk build --release; first build takes a few minutes)"
  (cd "$ROOT/apps/wallet-ui" && CARGO_TARGET_DIR="$LIVE/cargo-wallet" LOWKI_CHAIN_ID=11155111 LOWKI_NETWORK="Sepolia fork" \
    trunk build --release --dist "$WDIST" >"$RUN/wallet-build.log" 2>&1) || { tail -30 "$RUN/wallet-build.log"; die "wallet-ui build failed"; }
fi
[ -f "$WDIST/index.html" ] || die "no wallet dist at $WDIST"
if [ "${BUILD:-}" = 1 ] || [ ! -f "$ROOT/packages/lowki-react-auth/dist/index.js" ]; then
  echo "   @lowki/react-auth"
  (cd "$ROOT/examples/privy-swap" && npm run --silent build:sdk >"$RUN/sdk-build.log" 2>&1) || { tail -20 "$RUN/sdk-build.log"; die "SDK build failed"; }
fi
if [ "${BUILD:-}" = 1 ] || [ ! -f "$LIVE/privy-dist/index.html" ]; then
  echo "   privy-swap (vite build)"
  (cd "$ROOT/examples/privy-swap" && npx vite build --outDir "$LIVE/privy-dist" --emptyOutDir >"$RUN/privy-build.log" 2>&1) || { tail -20 "$RUN/privy-build.log"; die "privy-swap build failed"; }
fi
[ -d "$ROOT/apps/lowki-tips/node_modules" ] || (cd "$ROOT/apps/lowki-tips" && npm ci --no-audit --no-fund)

# ------------------------------------------------------------------------------ Sepolia forwarder
say "Sepolia forwarder :$P_SEPOLIA -> $SEPOLIA_RPC"
start sepolia $P_SEPOLIA env PORT=$P_SEPOLIA UPSTREAM="$SEPOLIA_RPC" node "$BIN/sepolia-forwarder.mjs"

# ------------------------------------------------------------------------------ fork
if [ "${FRESH_FORK:-}" = 1 ] || [ ! -f "$LIVE/fork-block" ]; then
  stop stack; stop anvil
  rpc "http://127.0.0.1:$P_SEPOLIA" eth_blockNumber | jq -r .result | xargs printf '%d\n' >"$LIVE/fork-block"
fi
FORK_BLOCK="$(cat "$LIVE/fork-block")"
SNAP="$PKG/.cache/pp-state-aspblind-11155111-$FORK_BLOCK.json"
if [ ! -f "$SNAP" ]; then
  say "Kohaku snapshot for block $FORK_BLOCK (one-time; runs the fork private-send e2e on :$P_SCRATCH, ~3 min)"
  cp -f "$(command -v anvil)" "$BIN/demo-live-snap-anvil"
  FORK_URL="$FORK_URL" FORK_BLOCK="$FORK_BLOCK" PORT=$P_SCRATCH ANVIL="$BIN/demo-live-snap-anvil" \
    "$ROOT/scripts/fork-private-send.sh" >"$RUN/snapshot.log" 2>&1 || true
  [ -f "$SNAP" ] || { tail -30 "$RUN/snapshot.log"; die "snapshot not written"; }
fi
say "anvil fork of Sepolia @ $FORK_BLOCK on :$P_ANVIL"
if ! alive anvil; then cp -f "$(command -v anvil)" "$BIN/demo-live-anvil"; fi
start anvil $P_ANVIL "$BIN/demo-live-anvil" --fork-url "$FORK_URL" --fork-block-number "$FORK_BLOCK" --chain-id 11155111 \
  --port $P_ANVIL --host 127.0.0.1 --silent
ANVIL_RPC="http://127.0.0.1:$P_ANVIL"
for _ in $(seq 1 60); do rpc "$ANVIL_RPC" eth_chainId >/dev/null && break; sleep 0.5; done
[ "$(cast call "$(jq -r .LowkiRegistry "$DEPLOY")" 'getResolver(string)(address)' chai --rpc-url "$ANVIL_RPC")" = "$(jq -r .StealthResolver "$DEPLOY")" ] \
  || die "chai.lowki.eth is not set up on the fork (fork block $FORK_BLOCK predates the creators? FRESH_FORK=1)"

# ------------------------------------------------------------------------------ tunnels (Mac)
# tunnel NAME PORT -> prints the URL. Reuses a live tunnel (same URL across runs).
tunnel_url() { grep -o 'https://[a-z0-9-]*\.trycloudflare\.com' "$RUN/tunnel-$1.log" 2>/dev/null | head -1 || true; }
tunnel_alive() { [ -f "$RUN/tunnel-$1.pid" ] && mac sh -lc "ps -p $(cat "$RUN/tunnel-$1.pid") -o command= 2>/dev/null | grep -q 'cloudflared tunnel'" 2>/dev/null; }
tunnel() {
  local name=$1 port=$2
  if tunnel_alive "$name" && [ -n "$(tunnel_url "$name")" ]; then return 0; fi
  rm -f "$RUN/tunnel-$name.log"
  mac sh -lc "nohup cloudflared tunnel --no-autoupdate --url http://localhost:$port > '$RUN/tunnel-$name.log' 2>&1 < /dev/null & echo \$!" >"$RUN/tunnel-$name.pid"
  echo "   tunnel $name -> :$port started (Mac pid $(cat "$RUN/tunnel-$name.pid"))"
}
if [ -n "${PUBLIC_DOMAIN:-}" ]; then
  # Fixed hostnames through a named Cloudflare tunnel (see ~/.cloudflared/lowki.yml on the Mac):
  # site at $PUBLIC_DOMAIN, the rest at lowki-<name>.<parent domain>.
  say "named tunnel hostnames ($PUBLIC_DOMAIN)"
  PARENT="${PUBLIC_DOMAIN#*.}"
  FORK_PUB="https://lowki-rpc.$PARENT" WALLET_PUB="https://lowki-wallet.$PARENT" TIPS_PUB="https://lowki-tips.$PARENT"
  PRIVY_PUB="https://lowki-swap.$PARENT" SITE_PUB="https://$PUBLIC_DOMAIN"
  WALLET_HOST="${WALLET_PUB#https://}"
  for v in "$FORK_PUB" "$WALLET_PUB" "$TIPS_PUB" "$PRIVY_PUB" "$SITE_PUB"; do echo "   $v"; done
else
say "Cloudflare quick tunnels on the Mac"
command -v mac >/dev/null && mac sh -lc 'command -v cloudflared' >/dev/null || die "cloudflared not found on the Mac"
for t in fork:$P_GATEWAY wallet:$P_WALLET tips:$P_TIPS privy:$P_PRIVY site:$P_SITE; do tunnel "${t%%:*}" "${t##*:}"; done
for t in fork wallet tips privy site; do
  for _ in $(seq 1 60); do [ -n "$(tunnel_url $t)" ] && break; sleep 0.5; done
  [ -n "$(tunnel_url $t)" ] || { tail -5 "$RUN/tunnel-$t.log"; die "tunnel $t has no URL"; }
done
FORK_PUB="$(tunnel_url fork)" WALLET_PUB="$(tunnel_url wallet)" TIPS_PUB="$(tunnel_url tips)" PRIVY_PUB="$(tunnel_url privy)" SITE_PUB="$(tunnel_url site)"
WALLET_HOST="${WALLET_PUB#https://}"
for t in fork wallet tips privy site; do echo "   $t: $(tunnel_url $t)"; done
fi

# ------------------------------------------------------------------------------ fork gateway (ASP + relayer)
say "fork gateway :$P_GATEWAY (rpc, ip, ASP, relayer)"
start stack $P_GATEWAY bash -c "cd '$PKG' && ANVIL_RPC='$ANVIL_RPC' GATEWAY_PORT=$P_GATEWAY WALLET_PORT=$P_STACKW DAPP_PORT=$P_STACKD \
  LOWKI_SERVER='http://127.0.0.1:$P_SERVER' WALLET_DIST='$WDIST' STATE='$LIVE/stack' PUBLIC_URL='$FORK_PUB' PP_SNAPSHOT='$SNAP' \
  exec node --import ./resolve-hook.mjs --import tsx demo/stack.ts"
for _ in $(seq 1 120); do grep -q READY "$RUN/stack.log" 2>/dev/null && break; sleep 0.5; done

# ------------------------------------------------------------------------------ lowki-server :8830
# Served under the wallet's host (the wallet proxies /v1 and /.well-known to it): the wallet origin
# is the passkey RP ID and the JWT issuer, as in a production deployment. The same server answers
# the ENS gateway through the on-chain :8830 tunnel. If :8830 is held by a server this script did
# not start, auth moves to :$P_AUTH and ENS lookups/records stay with whatever answers :8830.
if alive server; then
  cur="$(cat "$RUN/server.port" 2>/dev/null || echo $P_SERVER)"
  if [ "$cur" = $P_AUTH ] && ! port_open $P_SERVER; then echo "   :$P_SERVER is free again: moving lowki-server back"; stop server; else P_SERVER=$cur; fi
fi
if ! alive server && port_open $P_SERVER; then
  echo "   :$P_SERVER is busy with a server this script did not start: auth on :$P_AUTH, ENS gateway left to :$P_SERVER"
  P_SERVER=$P_AUTH
fi
say "lowki-server :$P_SERVER (RP ID $WALLET_HOST, gateway $GW_BASE)"
SEC="$LIVE/server/secrets.env"
if [ ! -f "$SEC" ]; then
  (umask 077; printf 'LOWKI_SERVER_SECRET=%s\nLOWKI_ADMIN_TOKEN=%s\n' "$(openssl rand -hex 32)" "$(openssl rand -hex 24)" >"$SEC")
fi
ADMIN="$(sed -n 's/^LOWKI_ADMIN_TOKEN=//p' "$SEC")"
# The server is restarted only when its configuration (tunnel URLs, binary) changes.
FP="$(printf '%s|%s|%s|%s|%s' "$P_SERVER" "$WALLET_PUB" "$GW_BASE" "$(stat -c %Y "$SERVER_BIN")" "$(jq -c . "$DEPLOY")" | sha256sum | cut -c1-16)"
if alive server && [ "$(cat "$RUN/server.fp" 2>/dev/null)" != "$FP" ]; then echo "   configuration changed: restarting"; stop server; fi
if ! alive server; then
  cat >"$RUN/server-run.sh" <<EOF
#!/usr/bin/env bash
# Written by scripts/demo-up.sh. Secrets come from files at start; nothing secret is in this file.
set -a; . "$SEC"; . "$SECRETS_DIR/sepolia-test.env"; set +a
export LOWKI_ENS_OPERATOR_KEY="\$LOWKI_SEPOLIA_KEY"; unset LOWKI_SEPOLIA_KEY
export LOWKI_DEV_MODE=true LOWKI_EMAIL_SENDER=log LOWKI_BIND=127.0.0.1:$P_SERVER
export LOWKI_PUBLIC_URL='$WALLET_PUB' LOWKI_RP_ID='$WALLET_HOST' LOWKI_ORIGINS='$WALLET_PUB' LOWKI_RP_NAME=Lowki
export LOWKI_COOKIE_SAMESITE=none LOWKI_COOKIE_PARTITIONED=true LOWKI_COOKIE_SECURE=true LOWKI_TRUST_PROXY_HEADERS=true
export LOWKI_DATABASE_URL='sqlite://$LIVE/server/lowki.db?mode=rwc'
export LOWKI_OTP_SENDS_PER_15MIN=100 LOWKI_SHARE_RELEASES_PER_HOUR=100
export LOWKI_ENS_GATEWAY=true LOWKI_ENS_RPC_URL='http://127.0.0.1:$P_SEPOLIA' LOWKI_ENS_PARENT='$(jq -r .parentName "$DEPLOY")'
export LOWKI_ENS_UNIVERSAL_RESOLVER='$(jq -r .ens.UniversalResolver "$ENSV2")' LOWKI_ENS_STEALTH_RESOLVER='$(jq -r .StealthResolver "$DEPLOY")'
export LOWKI_ENS_LOOKUPS_PER_MIN_IP=600 LOWKI_ENS_LOOKUPS_PER_HOUR_NAME=6000 LOWKI_ENS_RECORDS_PER_MIN_IP=600 LOWKI_ENS_META_CACHE_SECS=30
# Live name claims (wallet home): sponsored registration through LowkiRegistrar on live Sepolia.
export LOWKI_ENS_GATEWAY_URL='$GW_URL' LOWKI_ENS_CHAIN_ID=11155111
export LOWKI_ENS_REGISTRAR='$(jq -r .LowkiRegistrar "$DEPLOY")' LOWKI_ENS_REGISTRY='$(jq -r .LowkiRegistry "$DEPLOY")'
export LOWKI_ENS_OWNER_ACCOUNT='$(jq -r .LowkiOwnerAccount "$DEPLOY")'
export LOWKI_ENS_VERIFIABLE_FACTORY='$(jq -r .ens.VerifiableFactory "$ENSV2")' LOWKI_ENS_PERMISSIONED_RESOLVER_IMPL='$(jq -r .ens.PermissionedResolverImpl "$ENSV2")'
export RUST_LOG=info,lowki_server=info
exec '$SERVER_BIN'
EOF
  chmod 700 "$RUN/server-run.sh"
  start server $P_SERVER "$RUN/server-run.sh"
  echo "$FP" >"$RUN/server.fp"; echo "$P_SERVER" >"$RUN/server.port"
fi
for _ in $(seq 1 60); do curl -sf "http://127.0.0.1:$P_SERVER/healthz" >/dev/null && break; sleep 0.5; done
curl -sf "http://127.0.0.1:$P_SERVER/healthz" >/dev/null || { tail -20 "$RUN/server.log"; die "lowki-server is not healthy"; }

# ------------------------------------------------------------------------------ app registrations
say "apps (registered once, origins kept in sync with the tunnels)"
app_id() { # app_id NAME ORIGIN -> id (create or PATCH allowed_origins)
  local name=$1 origin=$2 id
  id="$(curl -sf "http://127.0.0.1:$P_SERVER/admin/v1/apps" -H "authorization: Bearer $ADMIN" | jq -r --arg n "$name" '[.apps[] | select(.name == $n)][0].id // empty')"
  if [ -z "$id" ]; then
    id="$(curl -sf -X POST "http://127.0.0.1:$P_SERVER/admin/v1/apps" -H "authorization: Bearer $ADMIN" -H 'content-type: application/json' \
      -d "$(jq -nc --arg n "$name" --arg o "$origin" '{name: $n, allowed_origins: [$o]}')" | jq -r .id)"
  else
    curl -sf -X PATCH "http://127.0.0.1:$P_SERVER/admin/v1/apps/$id" -H "authorization: Bearer $ADMIN" -H 'content-type: application/json' \
      -d "$(jq -nc --arg o "$origin" '{allowed_origins: [$o]}')" >/dev/null
  fi
  [ -n "$id" ] && [ "$id" != null ] || die "could not register app $name"
  echo "$id"
}
TIPS_APP="$(app_id 'Lowki Tips' "$TIPS_PUB")"
PRIVY_APP="$(app_id 'Privy Swap' "$PRIVY_PUB")"
WALLET_APP="$(app_id 'Lowki Wallet' "$WALLET_PUB")"
echo "   Lowki Tips $TIPS_APP, Privy Swap $PRIVY_APP, Lowki Wallet $WALLET_APP"

# ------------------------------------------------------------------------------ wallet :8855
say "wallet :$P_WALLET"
jq -n --arg fork "$FORK_PUB" --arg wallet "$WALLET_PUB" --arg gw "$GW_BASE" --arg app "$WALLET_APP" \
  --arg tips "$TIPS_PUB/" --arg privy "$PRIVY_PUB/?app=$PRIVY_APP&wallet=$(jq -rn --arg w "$WALLET_PUB/?app_name=Privy%20Swap" '$w|@uri')" '{
  chainId: 11155111, network: "Sepolia fork",
  rpcUrl: ($fork + "/rpc"), directRpcUrl: ($wallet + "/rpc"), ipEchoUrl: ($fork + "/ip"),
  anonUnavailable: null,
  private: { relayerUrl: ($fork + "/relayer"), aspUrl: ($fork + "/asp"),
             directRelayerUrl: ($fork + "/relayer"), directAspUrl: ($fork + "/asp"),
             snapshotUrl: "pp-snapshot.json", circuitsUrl: "circuits/", useGpu: true },
  ens: { parent: "lowki.eth", gatewayUrl: ($gw + "/v1/ens/gateway"), recordsUrl: ($gw + "/v1/ens/records") },
  walletAppId: $app,
  demoApps: [
    { name: "Lowki Tips", url: $tips, blurb: "Tip creators at name.lowki.eth, publicly or privately" },
    { name: "Privy Swap", url: $privy, blurb: "Privy'"'"'s docs example, on Lowki with one import" }
  ]
}' >"$RUN/wallet-config.json"
WFP="$(printf 'ens8830|%s|%s|%s' "$P_SERVER" "$WDIST" "$(stat -c %Y "$WDIST/index.html")" | sha256sum | cut -c1-16)"
if alive wallet && [ "$(cat "$RUN/wallet.fp" 2>/dev/null)" != "$WFP" ]; then stop wallet; fi
start wallet $P_WALLET env PORT=$P_WALLET ROOT="$WDIST" V1="http://127.0.0.1:$P_SERVER" V1_ENS="http://127.0.0.1:8830" RPC="http://127.0.0.1:$P_GATEWAY/rpc" \
  CONFIG="$RUN/wallet-config.json" ASSETS="http://127.0.0.1:$P_STACKW" node "$BIN/serve.mjs"
echo "$WFP" >"$RUN/wallet.fp"

# ------------------------------------------------------------------------------ Lowki Tips :8856
# Two builds merged: the app (with the fork faucet) and the creator inbox (demo: null, so it asks
# for the creator's seed instead of embedding a phrase). config/*.json is bundled into every page
# by import.meta.glob, so the private creator phrase is never written there.
say "Lowki Tips :$P_TIPS"
TIPS="$ROOT/apps/lowki-tips"
tips_cfg() { # tips_cfg DEMO_JSON
  jq --arg rpc "$TIPS_PUB/rpc" --arg sr "$(jq -r .StealthResolver "$DEPLOY")" --arg gw "$GW_URL" --arg rec "$GW_BASE/v1/ens/records" \
     --arg app "$TIPS_APP" --arg wallet "$WALLET_PUB/?app_name=Lowki%20Tips" --argjson demo "$1" '
    ._comment = "Generated by scripts/demo-up.sh (live demo on the Sepolia fork). Do not edit." | .target = "live"
    | .chain = { id: 11155111, name: "Sepolia (demo fork)", rpcUrl: $rpc, explorerTxUrl: null }
    | .ens.stealthResolver = $sr | .ens.gatewayUrl = $gw | .ens.recordsUrl = $rec
    | .lowki = { appId: $app, walletUrl: $wallet } | .demo = $demo' "$TIPS/config/sepolia.json"
}
tips_cfg '{"faucet": true, "creatorSeedPhrase": "(not in this build)"}' >"$RUN/tips-live.json"
tips_cfg 'null' >"$RUN/tips-live-inbox.json"
TFP="$(cat "$RUN/tips-live.json" "$RUN/tips-live-inbox.json" | sha256sum | cut -c1-16)"
if [ "${BUILD:-}" = 1 ] || [ "$(cat "$LIVE/tips-dist/.fp" 2>/dev/null)" != "$TFP" ]; then
  echo "   building (configs changed)"
  cp "$RUN/tips-live.json" "$TIPS/config/live.json"
  cp "$RUN/tips-live-inbox.json" "$TIPS/config/live-inbox.json"
  (cd "$TIPS" && VITE_TIPS_TARGET=live npx vite build --outDir "$LIVE/tips-build-app" --emptyOutDir >"$RUN/tips-build.log" 2>&1 \
     && VITE_TIPS_TARGET=live-inbox npx vite build --outDir "$LIVE/tips-build-inbox" --emptyOutDir >>"$RUN/tips-build.log" 2>&1) \
    || { tail -30 "$RUN/tips-build.log"; die "Tips build failed"; }
  rm -rf "$LIVE/tips-dist.new" && cp -a "$LIVE/tips-build-app" "$LIVE/tips-dist.new"
  cp -f "$LIVE/tips-build-inbox/creator.html" "$LIVE/tips-dist.new/creator.html"
  cp -an "$LIVE/tips-build-inbox/assets/." "$LIVE/tips-dist.new/assets/"
  echo "$TFP" >"$LIVE/tips-dist.new/.fp"
  rm -rf "$LIVE/tips-dist" && mv "$LIVE/tips-dist.new" "$LIVE/tips-dist"
fi
start tips $P_TIPS env PORT=$P_TIPS ROOT="$LIVE/tips-dist" RPC="$ANVIL_RPC" \
  RPC_ALLOW='^(eth_|net_version$|web3_clientVersion$|anvil_setBalance$)' node "$BIN/serve.mjs"

# ------------------------------------------------------------------------------ privy-swap :8857
say "privy-swap :$P_PRIVY"
start privy $P_PRIVY env PORT=$P_PRIVY ROOT="$LIVE/privy-dist" RPC="http://127.0.0.1:$P_ANVIL" RPC_ALLOW='^anvil_setBalance$' node "$BIN/serve.mjs"
PRIVY_URL="$PRIVY_PUB/?app=$PRIVY_APP&wallet=$(jq -rn --arg w "$WALLET_PUB/?app_name=Privy%20Swap" '$w|@uri')"

# ------------------------------------------------------------------------------ showcase site :8858
say "showcase site :$P_SITE"
SITE_SRC="$ROOT/apps/site"
if [ -f "$SITE_SRC/dist/index.html" ]; then
  SFP="$(find "$SITE_SRC/dist" "$SITE_SRC/serve.mjs" -type f -printf '%T@ %p\n' 2>/dev/null | sort | sha256sum | cut -c1-16)"
  if [ "$(cat "$LIVE/site/.fp" 2>/dev/null)" != "$SFP" ]; then
    stop site
    rm -rf "$LIVE/site" && mkdir -p "$LIVE/site" && cp -a "$SITE_SRC/dist" "$LIVE/site/dist" && cp -f "$SITE_SRC/serve.mjs" "$LIVE/site/serve.mjs"
    echo "$SFP" >"$LIVE/site/.fp"
  fi
  # Runtime links (the site reads dist/site-config.json at load; keys: repo, video, demos[]).
  jq -n --arg tips "$TIPS_PUB/" --arg privy "$PRIVY_URL" --arg wallet "$WALLET_PUB/" --arg inbox "$TIPS_PUB/creator.html?name=chai.lowki.eth" \
     --arg repo "${SITE_REPO_URL:-https://github.com/betterclever/lowki}" --arg video "${SITE_VIDEO_URL:-https://youtu.be/mQe8CaCkrUA}" '{
    _comment: "Written by scripts/demo-up.sh (SITE_REPO_URL, SITE_VIDEO_URL). Live links are Cloudflare quick tunnels.",
    repo: $repo, video: $video,
    demos: [
      { name: "Lowki Tips", detail: "Tip chai.lowki.eth: a fresh stealth address every time, or privately through Privacy Pools", url: $tips },
      { name: "Privy example app, on Lowki", detail: "Privy'"'"'s docs example, switched with one import: a different wallet, same passkey", url: $privy },
      { name: "Lowki wallet", detail: "Your apps, your private balance, your name.lowki.eth", url: $wallet },
      { name: "Creator inbox", detail: "The creator'"'"'s side: find every tip with the viewing key", url: $inbox }
    ],
    live: { tips: $tips, privy: $privy, wallet: $wallet, inbox: $inbox } }' >"$LIVE/site/dist/site-config.json"
  start site $P_SITE env PORT=$P_SITE HOST=127.0.0.1 node "$LIVE/site/serve.mjs"
else
  echo "   apps/site/dist is missing: site not started (build it: cd apps/site && node build.mjs)"
fi

# ------------------------------------------------------------------------------ public URLs answer?
say "waiting for the public URLs"
check_pub() { for _ in $(seq 1 60); do curl -sf -o /dev/null -m 10 "$1" && return 0; sleep 1; done; return 1; }
for u in "$GW_BASE/healthz" "$WALLET_PUB/.well-known/jwks.json" "$FORK_PUB/ip" "$WALLET_PUB/" "$TIPS_PUB/" "$PRIVY_PUB/" "$SITE_PUB/"; do
  if check_pub "$u"; then echo "   ok   $u"; else echo "   FAIL $u"; fi
done

jq -n --arg tips "$TIPS_PUB/" --arg inbox "$TIPS_PUB/creator.html?name=chai.lowki.eth" --arg privy "$PRIVY_URL" \
  --arg wallet "$WALLET_PUB/" --arg site "$SITE_PUB/" --arg fork "$FORK_PUB" --arg gw "$GW_BASE" \
  --arg tipsApp "$TIPS_APP" --arg privyApp "$PRIVY_APP" --arg walletApp "$WALLET_APP" --arg block "$FORK_BLOCK" \
  '{tips: $tips, inbox: $inbox, privy: $privy, wallet: $wallet, site: $site, forkRpc: ($fork + "/rpc"), fork: $fork,
    gateway: $gw, apps: {tips: $tipsApp, privy: $privyApp, wallet: $walletApp}, forkBlock: $block}' >"$LIVE/urls.json"

# Keep the URL block at the top of docs/DEMO.md current (between its URLS markers).
if [ -f "$ROOT/docs/DEMO.md" ] && grep -q '<!-- URLS:BEGIN -->' "$ROOT/docs/DEMO.md"; then
  node -e '
    const fs = require("fs"); const [doc, uj] = process.argv.slice(1); const u = JSON.parse(fs.readFileSync(uj, "utf8"));
    const block = ["<!-- URLS:BEGIN -->", "| What | URL |", "|---|---|",
      `| Lowki Tips | ${u.tips} |`, `| Creator inbox | ${u.inbox} |`, `| Privy Swap | ${u.privy} |`,
      `| Wallet home | ${u.wallet} |`, `| Showcase site | ${u.site} |`, `| Fork RPC (Anon-RPC target) | ${u.forkRpc} |`,
      `| ENS gateway (on chain) | ${u.gateway}/v1/ens/gateway |`,
      "", `_Written by scripts/demo-up.sh at ${new Date().toISOString().replace(/\.\d+Z$/, "Z")}; Sepolia fork at block ${u.forkBlock}._`,
      "<!-- URLS:END -->"].join("\n");
    const s = fs.readFileSync(doc, "utf8");
    fs.writeFileSync(doc, s.replace(/<!-- URLS:BEGIN -->[\s\S]*?<!-- URLS:END -->/, block));' "$ROOT/docs/DEMO.md" "$LIVE/urls.json"
fi

cat <<EOF | tee "$LIVE/urls.txt"

================================================================ Lowki live demo
  Lowki Tips      $TIPS_PUB/
  Creator inbox   $TIPS_PUB/creator.html?name=chai.lowki.eth
  Privy Swap      $PRIVY_URL
  Wallet home     $WALLET_PUB/
  Showcase site   $SITE_PUB/
  Fork RPC        $FORK_PUB/rpc   (Anon-RPC target; ASP $FORK_PUB/asp, relayer $FORK_PUB/relayer)
  ENS gateway     $GW_URL   (on chain; :8830)
  lowki-server    :$P_SERVER, public as $WALLET_PUB/v1 (JWKS $WALLET_PUB/.well-known/jwks.json)
  fork            Sepolia @ block $FORK_BLOCK (anvil :$P_ANVIL)
  email codes     grep 'sign-in code' $RUN/server.log | tail -1
  creator seeds   $SECRETS_DIR/creators.env (CHAI_SEED etc., for the inbox)
================================================================
EOF
