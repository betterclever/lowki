#!/usr/bin/env bash
# Local demo stack for the wallet UI's real private send (docs/PRIVATE-SEND.md, "Demo stack"):
#
#   :8814  anvil fork of Sepolia (binary copied to target/demo-stack/bin/lowki-demo-anvil, so a
#          `pkill -x anvil` elsewhere does not kill it)
#   :8813  lowki-server (dev mode; wallet origin :8810, demo dapp origin :8812)
#   :8815  gateway: /rpc (standard eth_* only, proxied to the fork, every call logged with the
#          caller IP), /ip, the fork ASP (/asp, postman root pushes) and the fork relayer (/relayer)
#   TUNNEL=1 only: https://<random>.trycloudflare.com -> :8815 (cloudflared quick tunnel), so
#          the anon network's exits can reach the fork and the wallet's fork reads go through
#          Anon-RPC. Off by default (it publishes the gateway on the internet); without it the
#          wallet reports a direct connection and why.
#   :8810  wallet UI dist (+ /v1 proxy, /lowki-config.json, /circuits, /pp-snapshot.json)
#   :8812  demo dapp (repo root)
#
#   scripts/demo-stack.sh            # foreground; Ctrl-C stops everything it started
#
# Env: FORK_URL (default https://sepolia.gateway.tenderly.co), FORK_BLOCK (12078500; needs the
#      Kohaku snapshot packages/lowki-kohaku-host/.cache/pp-state-aspblind-11155111-<block>.json,
#      written by scripts/fork-private-send.sh), TUNNEL (0), BUILD (auto: 1 rebuilds the glue and
#      the wallet dist), ASP_DELAY_MS (3000), RELAY_FEE_BPS (50), USE_GPU (1), LOWKI_SERVER_BIN.
# Writes target/demo-stack/{stack.json,rpc-access.jsonl,*.log,server/}.
# Only kills processes it started. No live Sepolia transaction is ever sent.
set -euo pipefail
here="$(cd "$(dirname "$0")/.." && pwd)"
export PATH="$HOME/.foundry/bin:$HOME/.cargo/bin:$PATH"
FORK_URL="${FORK_URL:-https://sepolia.gateway.tenderly.co}"
FORK_BLOCK="${FORK_BLOCK:-12078500}"
WALLET_PORT=8810 DAPP_PORT=8812 SERVER_PORT=8813 ANVIL_PORT=8814 GATEWAY_PORT=8815
STATE="$here/target/demo-stack"
PKG="$here/packages/lowki-kohaku-host"
mkdir -p "$STATE/bin"

pids=()
cleanup() { for p in "${pids[@]}"; do kill "$p" 2>/dev/null || true; done; wait 2>/dev/null || true; }
trap cleanup EXIT INT TERM
die() { echo "demo-stack: $*" >&2; exit 1; }
port_free() { ! (exec 3<>"/dev/tcp/127.0.0.1/$1") 2>/dev/null; }
for p in $WALLET_PORT $DAPP_PORT $SERVER_PORT $ANVIL_PORT $GATEWAY_PORT; do port_free "$p" || die "port $p is in use (another demo stack?)"; done

# ---- prerequisites
command -v anvil >/dev/null || die "anvil not found: curl -L https://foundry.paradigm.xyz | bash && foundryup"
[ -f "$here/crates/lowki-worker/pkg/web/lowki_worker.js" ] || die "build the worker first: (cd crates/lowki-worker && make pkg)"
[ -d "$PKG/node_modules" ] || (cd "$PKG" && npm ci)
SNAP="$PKG/.cache/pp-state-aspblind-11155111-$FORK_BLOCK.json"
[ -f "$SNAP" ] || die "no Kohaku snapshot for block $FORK_BLOCK: run FORK_BLOCK=$FORK_BLOCK scripts/fork-private-send.sh once"
[ -d "$here/../vendor/npm/circuits/package/artifacts" ] || die "circuit artifacts missing: ../vendor/npm/circuits/package/artifacts"

DIST="$STATE/wallet-dist"
if [ "${BUILD:-}" = 1 ] || [ ! -f "$PKG/browser-wallet/dist/lowki-glue.js" ]; then
  echo "== building the wallet glue (browser-wallet/build.mjs)"
  (cd "$PKG" && node browser-wallet/build.mjs)
fi
if [ "${BUILD:-}" = 1 ] || [ ! -f "$DIST/index.html" ]; then
  echo "== building the wallet UI (trunk, release) -> $DIST"
  (cd "$here/apps/wallet-ui" && CARGO_TARGET_DIR="$here/target/wallet-ui-demo" trunk build --release --dist "$DIST")
fi

# ---- anvil fork (renamed binary)
cp -f "$(command -v anvil)" "$STATE/bin/lowki-demo-anvil"
echo "== anvil fork of $FORK_URL @ $FORK_BLOCK on :$ANVIL_PORT"
"$STATE/bin/lowki-demo-anvil" --fork-url "$FORK_URL" --fork-block-number "$FORK_BLOCK" --chain-id 11155111 \
  --port "$ANVIL_PORT" --host 0.0.0.0 --silent >"$STATE/anvil.log" 2>&1 &
pids+=($!)
for _ in $(seq 1 120); do
  curl -s -o /dev/null -X POST -H 'content-type: application/json' \
    --data '{"jsonrpc":"2.0","id":1,"method":"eth_chainId","params":[]}' "http://127.0.0.1:$ANVIL_PORT" && break
  kill -0 "${pids[0]}" 2>/dev/null || { cat "$STATE/anvil.log" >&2; die "anvil exited"; }
  sleep 0.5
done

# ---- lowki-server
BIN="${LOWKI_SERVER_BIN:-}"
if [ -z "$BIN" ]; then
  BIN="$STATE/bin/lowki-server"
  if [ ! -x "$BIN" ] || [ "${BUILD:-}" = 1 ]; then
    echo "== building lowki-server"
    (cd "$here/crates/lowki-server" && CARGO_TARGET_DIR="$here/target/lowki-server-demo" cargo build --quiet)
    cp -f "$here/target/lowki-server-demo/debug/lowki-server" "$BIN"
  fi
fi
echo "== lowki-server on :$SERVER_PORT"
rm -rf "$STATE/server"
STATE="$STATE/server" PORT=$SERVER_PORT WALLET_ORIGIN="http://localhost:$WALLET_PORT" DAPP_ORIGINS="http://localhost:$DAPP_PORT" \
  LOWKI_SERVER_BIN="$BIN" RUST_LOG=warn "$here/apps/wallet-ui/scripts/dev-server.sh" >"$STATE/server.log" 2>&1 &
pids+=($!)
for _ in $(seq 1 120); do [ -f "$STATE/server/apps.json" ] && break; sleep 0.5; done
[ -f "$STATE/server/apps.json" ] || { tail -20 "$STATE/server.log" >&2; die "lowki-server did not start"; }

# ---- tunnel (so the anon network's exits can reach the fork)
PUBLIC_URL=""
if [ "${TUNNEL:-0}" = 1 ]; then
  CF="$(command -v cloudflared || true)"
  if [ -z "$CF" ]; then
    os=$(uname -s | tr 'A-Z' 'a-z'); arch=$(uname -m); case $arch in aarch64|arm64) arch=arm64 ;; x86_64) arch=amd64 ;; esac
    CF="$here/target/tools/cloudflared-$os-$arch"
    if [ ! -x "$CF" ]; then
      [ "$os" = linux ] || die "install cloudflared (brew install cloudflared) or run with TUNNEL=0"
      echo "== downloading cloudflared ($os-$arch)"
      mkdir -p "$here/target/tools"
      curl -fsSL -o "$CF" "https://github.com/cloudflare/cloudflared/releases/latest/download/cloudflared-$os-$arch"
      chmod +x "$CF"
    fi
  fi
  echo "== cloudflared quick tunnel -> :$GATEWAY_PORT"
  "$CF" tunnel --no-autoupdate --url "http://localhost:$GATEWAY_PORT" >"$STATE/tunnel.log" 2>&1 &
  pids+=($!)
  for _ in $(seq 1 60); do
    PUBLIC_URL=$(grep -o 'https://[a-z0-9-]*\.trycloudflare\.com' "$STATE/tunnel.log" | head -1 || true)
    [ -n "$PUBLIC_URL" ] && break
    sleep 0.5
  done
  [ -n "$PUBLIC_URL" ] && echo "   public gateway: $PUBLIC_URL" || echo "   no tunnel URL (see $STATE/tunnel.log): the wallet will report a direct connection"
fi

# ---- gateway + ASP + relayer + static servers
echo "== gateway :$GATEWAY_PORT, wallet :$WALLET_PORT, dapp :$DAPP_PORT"
: >"$STATE/rpc-access.jsonl"
(cd "$PKG" && ANVIL_RPC="http://127.0.0.1:$ANVIL_PORT" GATEWAY_PORT=$GATEWAY_PORT WALLET_PORT=$WALLET_PORT DAPP_PORT=$DAPP_PORT \
  LOWKI_SERVER="http://localhost:$SERVER_PORT" WALLET_DIST="$DIST" STATE="$STATE" PUBLIC_URL="$PUBLIC_URL" PP_SNAPSHOT="$SNAP" \
  exec node --import ./resolve-hook.mjs --import tsx demo/stack.ts) >"$STATE/stack.log" 2>&1 &
pids+=($!)
for _ in $(seq 1 120); do grep -q READY "$STATE/stack.log" 2>/dev/null && break; sleep 0.5; done
grep -q READY "$STATE/stack.log" || { tail -20 "$STATE/stack.log" >&2; die "the gateway did not start"; }
cat "$STATE/stack.log"

# The tunnel takes a few seconds to route; wait until the public /ip answers.
if [ -n "$PUBLIC_URL" ]; then
  for _ in $(seq 1 60); do curl -sf "$PUBLIC_URL/ip" >/dev/null && break; sleep 1; done
  curl -sf "$PUBLIC_URL/ip" >/dev/null && echo "   tunnel up: $PUBLIC_URL/ip" || echo "   tunnel not answering yet"
fi

APP=$(node -e 'const a=require(process.argv[1]); const o=Object.keys(a)[0]; process.stdout.write(a[o].id)' "$STATE/server/apps.json")
DAPP_URL="http://localhost:$DAPP_PORT/apps/demo-dapp/?wallet=http%3A%2F%2Flocalhost%3A$WALLET_PORT%2F&app=$APP&app_name=Game%20X"
node -e 'const fs=require("fs"),p=process.argv[1],j=JSON.parse(fs.readFileSync(p));j.dappUrl=process.argv[2];j.appId=process.argv[3];j.anvilRpc=process.argv[4];j.serverLog=process.argv[5];fs.writeFileSync(p,JSON.stringify(j,null,2))' \
  "$STATE/stack.json" "$DAPP_URL" "$APP" "http://localhost:$ANVIL_PORT" "$STATE/server.log"
cat <<EOF

Demo stack ready.
  open      $DAPP_URL
  direct    add &walletflags=rpc%3Ddirect to the URL above to skip Anon-RPC
  fund      cast rpc --rpc-url http://localhost:$ANVIL_PORT anvil_setBalance <app address> 0xDE0B6B3A7640000
  codes     email sign-in codes are in $STATE/server.log
  rpc log   $STATE/rpc-access.jsonl (via=tunnel: arrived from an anon exit; via=local: direct)
EOF
echo "(logs: $STATE/*.log; Ctrl-C to stop)"
wait -n "${pids[@]}" || true
echo "demo-stack: a component exited; stopping" >&2
