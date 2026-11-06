#!/usr/bin/env bash
# Lowki Tips demo stack: a Sepolia fork with lowki.eth + three creator stealth names, lowki-server
# (auth + ENS gateway), the Lowki wallet, and the Lowki Tips app.
#
#   scripts/demo-tips.sh                 # Ctrl-C stops everything this script started
#
# Ports (8820-8829):
#   8820  anvil fork (only when this script starts its own; binary copied to `tips-anvil` so a
#         `pkill -x anvil` elsewhere doesn't take it down)
#   8821  lowki-server (sign-in, tokens, ENS CCIP-Read gateway, stealth records)
#   8822  Lowki wallet (wallet-ui build; /v1 -> 8821, /rpc -> the fork, /lowki-config.json)
#   8823  Lowki Tips (vite preview of apps/lowki-tips; creator inbox at /creator.html)
#
# Reusing scripts/demo-stack.sh's fork (anvil + Privacy Pools ASP + relayer, ports 8810-8819):
#   TIPS_FORK_RPC=http://127.0.0.1:<its anvil port>   use that fork instead of starting one
#   TIPS_PP_RELAYER_URL, TIPS_PP_ASP_URL              its relayer / ASP (for "Tip privately")
#   TIPS_PP_ASSETS_URL                                its wallet server (serves /circuits/ and
#                                                     /pp-snapshot.json; our wallet proxies them)
#   TIPS_DEMO_STACK=1   shorthand: read all of the above from target/demo-stack/stack.json
#                       (anvil :8814, gateway :8815 with /relayer and /asp, wallet :8810)
# Without TIPS_PP_* the wallet has no private-send config (its private send then reports that).
#
# Other env:
#   SEPOLIA_RPC_URL    upstream for our own fork (default https://sepolia.gateway.tenderly.co)
#   TIPS_WALLET_DIST   use an existing wallet-ui build instead of building one with trunk
#   TIPS_CREATOR_PHRASE  demo creator seed phrase (default below; creator seeds are
#                        sha512("<phrase>/<label>"), documented in apps/lowki-tips/README.md)
#   TIPS_STATE         state dir (default target/lowki-tips/state, wiped on each run)
#
# Needs: foundry (forge, anvil, cast), cargo + trunk, node >= 22, openssl, jq, curl.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TIPS="$ROOT/apps/lowki-tips"
CONTRACTS="$ROOT/contracts"
export PATH="$HOME/.foundry/bin:$HOME/.cargo/bin:$PATH"

ANVIL_PORT=8820 SERVER_PORT=8821 WALLET_PORT=8822 TIPS_PORT=8823
SEPOLIA_RPC_URL="${SEPOLIA_RPC_URL:-https://sepolia.gateway.tenderly.co}"
STATE="${TIPS_STATE:-$ROOT/target/lowki-tips/state}"
TIPS_CREATOR_PHRASE="${TIPS_CREATOR_PHRASE:-lowki tips demo creators, not a secret}"
# Fork-only operator/admin key, derived from a public phrase so re-runs on the same fork are
# idempotent. Never use it anywhere else.
OPERATOR_KEY="$(printf 'lowki tips demo operator, fork only' | openssl dgst -sha256 -hex | awk '{print "0x"$NF}')"

if [ "${TIPS_DEMO_STACK:-}" = 1 ]; then
  STACK_JSON="$ROOT/target/demo-stack/stack.json"
  [ -f "$STACK_JSON" ] || { echo "TIPS_DEMO_STACK=1 but $STACK_JSON is missing: start scripts/demo-stack.sh first"; exit 1; }
  TIPS_FORK_RPC="$(jq -r .anvil "$STACK_JSON")"
  TIPS_PP_RELAYER_URL="$(jq -r .gateway "$STACK_JSON")/relayer"
  TIPS_PP_ASP_URL="$(jq -r .gateway "$STACK_JSON")/asp"
  TIPS_PP_ASSETS_URL="$(jq -r .wallet "$STACK_JSON")"
fi
rm -rf "$STATE"; mkdir -p "$STATE"
LOG="$STATE/logs"; mkdir -p "$LOG"
PIDS=()
cleanup() { for p in "${PIDS[@]}"; do kill "$p" 2>/dev/null || true; done; }
trap cleanup EXIT INT TERM

for p in $SERVER_PORT $WALLET_PORT $TIPS_PORT; do
  if (exec 3<>"/dev/tcp/127.0.0.1/$p") 2>/dev/null; then echo "port $p is in use (not by us); stop that first"; exit 1; fi
done

# ------------------------------------------------------------------------------ 1. fork
if [ -n "${TIPS_FORK_RPC:-}" ]; then
  RPC="$TIPS_FORK_RPC"
  echo "== reusing fork $RPC"
  cast rpc anvil_nodeInfo --rpc-url "$RPC" >/dev/null || { echo "TIPS_FORK_RPC is not an anvil fork"; exit 1; }
else
  RPC="http://127.0.0.1:$ANVIL_PORT"
  if (exec 3<>"/dev/tcp/127.0.0.1/$ANVIL_PORT") 2>/dev/null; then echo "port $ANVIL_PORT is in use; set TIPS_FORK_RPC to reuse a fork"; exit 1; fi
  echo "== anvil fork of Sepolia on :$ANVIL_PORT ($SEPOLIA_RPC_URL)"
  cp "$(command -v anvil)" "$STATE/tips-anvil"
  "$STATE/tips-anvil" --fork-url "$SEPOLIA_RPC_URL" --port "$ANVIL_PORT" --host 127.0.0.1 --silent >"$LOG/anvil.log" 2>&1 &
  PIDS+=($!)
  for _ in $(seq 60); do cast chain-id --rpc-url "$RPC" >/dev/null 2>&1 && break; sleep 1; done
fi
echo "   block $(cast block-number --rpc-url "$RPC"), chain id $(cast chain-id --rpc-url "$RPC")"
OPERATOR="$(cast wallet address "$OPERATOR_KEY")"
cast rpc anvil_setBalance "$OPERATOR" 0x56BC75E2D63100000 --rpc-url "$RPC" >/dev/null

# ------------------------------------------------------------------------------ 2. ENS
echo "== lowki.eth + Lowki registry/registrar + StealthResolver (contracts/ scripts)"
cd "$CONTRACTS"
export LOWKI_KEY_ENV=TIPS_OPERATOR_KEY TIPS_OPERATOR_KEY="$OPERATOR_KEY"
export LOWKI_PARENT_LABEL="${LOWKI_PARENT_LABEL:-lowki}"
# If the label is taken on the fork (by someone else's key), RegisterParent uses <label>-<suffix>.eth.
export LOWKI_PARENT_SUFFIX="${LOWKI_PARENT_SUFFIX:-tips}"
export LOWKI_GATEWAY_URL="http://localhost:$SERVER_PORT/v1/ens/gateway"
export LOWKI_ENS_CONFIG="deployments/sepolia-ensv2.json"
# Our own forge script cache (broadcast sequences + recovery locks), so runs by other tools on
# the same chain id never contend with ours. Seeded with the compile cache to skip a rebuild.
mkdir -p "$ROOT/target/lowki-tips/forge-cache" "$ROOT/target/lowki-tips/forge-broadcast"
cp -f cache/solidity-files-cache.json "$ROOT/target/lowki-tips/forge-cache/" 2>/dev/null || true
export FOUNDRY_CACHE_PATH="$ROOT/target/lowki-tips/forge-cache" FOUNDRY_BROADCAST="$ROOT/target/lowki-tips/forge-broadcast"
export LOWKI_DEPLOYMENTS_OUT="deployments/local/tips-fork-lowki.json"
ENS_JSON="$CONTRACTS/$LOWKI_ENS_CONFIG"
REUSED=0
if [ -f "$LOWKI_DEPLOYMENTS_OUT" ] && [ -n "${TIPS_FORK_RPC:-}" ]; then
  SR="$(jq -r .StealthResolver "$LOWKI_DEPLOYMENTS_OUT")"
  GW="$(jq -r .gatewayUrl "$LOWKI_DEPLOYMENTS_OUT")"
  if [ "$(cast code "$SR" --rpc-url "$RPC")" != "0x" ] && [ "$GW" = "$LOWKI_GATEWAY_URL" ] \
     && [ "$(cast call "$(jq -r .LowkiRegistry "$LOWKI_DEPLOYMENTS_OUT")" 'isEmancipated()(bool)' --rpc-url "$RPC" 2>/dev/null)" = "true" ]; then
    echo "   already deployed on this fork ($LOWKI_DEPLOYMENTS_OUT)"; REUSED=1
  fi
fi
if [ $REUSED = 0 ]; then
  rm -f "$LOWKI_DEPLOYMENTS_OUT"
  # Idempotent: both steps skip when the parent is already registered to the operator.
  export LOWKI_COMMIT_SECRET="0x$(openssl rand -hex 32)"
  forge script script/RegisterParent.s.sol --sig "commit()" --rpc-url "$RPC" --broadcast >"$LOG/commit.log" 2>&1 || { tail -30 "$LOG/commit.log"; exit 1; }
  cast rpc evm_increaseTime 61 --rpc-url "$RPC" >/dev/null && cast rpc evm_mine --rpc-url "$RPC" >/dev/null
  forge script script/RegisterParent.s.sol --sig "register()" --rpc-url "$RPC" --broadcast >"$LOG/register.log" 2>&1 || { tail -30 "$LOG/register.log"; exit 1; }
  ALT="$(grep -oE 'using [a-z0-9-]+\.eth' "$LOG/register.log" | head -1 | sed -E 's/using ([a-z0-9-]+)\.eth/\1/' || true)"
  [ -n "$ALT" ] && { export LOWKI_PARENT_LABEL="$ALT"; echo "   lowki.eth is taken on this fork; using $ALT.eth"; }
  forge script script/DeployLowki.s.sol --rpc-url "$RPC" --broadcast >"$LOG/deploy.log" 2>&1 || { tail -40 "$LOG/deploy.log"; exit 1; }
fi
cp "$LOWKI_DEPLOYMENTS_OUT" "$STATE/lowki-deployment.json"
echo "   StealthResolver $(jq -r .StealthResolver "$STATE/lowki-deployment.json"), parent $(jq -r .parentName "$STATE/lowki-deployment.json")"

# ------------------------------------------------------------------------------ 3. lowki-server
echo "== lowki-server (auth + ENS gateway) on :$SERVER_PORT"
CLI="$ROOT/target/lowki-ens-e2e/debug/lowki-ens-e2e"
SERVER_BIN="$ROOT/target/lowki-tips/server/debug/lowki-server"
(cd "$ROOT/crates/lowki-server" && CARGO_TARGET_DIR="$ROOT/target/lowki-tips/server" cargo build --quiet)
[ -x "$CLI" ] || (cd "$CONTRACTS/e2e/client" && CARGO_TARGET_DIR="$ROOT/target/lowki-ens-e2e" cargo build --quiet)
ADMIN="$(openssl rand -hex 24)"
(
  export LOWKI_DEV_MODE=true LOWKI_BIND="127.0.0.1:$SERVER_PORT" LOWKI_PUBLIC_URL="http://localhost:$SERVER_PORT"
  export LOWKI_ORIGINS="http://localhost:$WALLET_PORT" LOWKI_RP_ID=localhost LOWKI_ADMIN_TOKEN="$ADMIN"
  LOWKI_SERVER_SECRET="$(openssl rand -hex 32)"; export LOWKI_SERVER_SECRET
  export LOWKI_DATABASE_URL="sqlite://$STATE/lowki.db?mode=rwc"
  export LOWKI_ENS_GATEWAY=true LOWKI_ENS_RPC_URL="$RPC" LOWKI_ENS_PARENT="$(jq -r .parentName "$STATE/lowki-deployment.json")"
  LOWKI_ENS_UNIVERSAL_RESOLVER="$(jq -r .ens.UniversalResolver "$ENS_JSON")"; export LOWKI_ENS_UNIVERSAL_RESOLVER
  LOWKI_ENS_STEALTH_RESOLVER="$(jq -r .StealthResolver "$STATE/lowki-deployment.json")"; export LOWKI_ENS_STEALTH_RESOLVER
  export LOWKI_ENS_META_CACHE_SECS=0 RUST_LOG=info
  exec "$SERVER_BIN"
) >"$STATE/server.log" 2>&1 &
PIDS+=($!)
for _ in $(seq 60); do curl -sf "http://127.0.0.1:$SERVER_PORT/healthz" >/dev/null && break; sleep 0.5; done
curl -sf "http://127.0.0.1:$SERVER_PORT/healthz" >/dev/null || { echo "lowki-server did not start:"; tail -20 "$STATE/server.log"; exit 1; }
APP_ID="$(curl -sf -X POST "http://127.0.0.1:$SERVER_PORT/admin/v1/apps" -H "authorization: Bearer $ADMIN" \
  -H 'content-type: application/json' -d "{\"name\":\"Lowki Tips\",\"allowed_origins\":[\"http://localhost:$TIPS_PORT\"]}" | jq -r .id)"
echo "   app Lowki Tips (http://localhost:$TIPS_PORT): $APP_ID"

# ------------------------------------------------------------------------------ 4. creators + app config
echo "== creator names (seeds from TIPS_CREATOR_PHRASE) and apps/lowki-tips/config/fork.json"
(cd "$TIPS" && [ -d node_modules ] || npm ci --no-audit --no-fund >/dev/null)
TIPS_RPC="$RPC" TIPS_PUBLIC_RPC="$RPC" TIPS_CLI="$CLI" TIPS_ENS_CONFIG="$ENS_JSON" \
TIPS_LOWKI_DEPLOYMENT="$STATE/lowki-deployment.json" TIPS_OPERATOR_KEY="$OPERATOR_KEY" \
TIPS_CREATOR_PHRASE="$TIPS_CREATOR_PHRASE" TIPS_APP_ID="$APP_ID" TIPS_WALLET_URL="http://localhost:$WALLET_PORT/" \
TIPS_GATEWAY_BASE="http://localhost:$SERVER_PORT" TIPS_OUT="$TIPS/config/fork.json" \
  node "$TIPS/scripts/fork-setup.mjs"

# ------------------------------------------------------------------------------ 5. wallet
echo "== Lowki wallet on :$WALLET_PORT"
WALLET_DIST="${TIPS_WALLET_DIST:-$ROOT/target/lowki-tips/wallet-dist}"
if [ -z "${TIPS_WALLET_DIST:-}" ]; then
  (cd "$ROOT/apps/wallet-ui" && CARGO_TARGET_DIR="$ROOT/target/lowki-tips/wallet-target" \
    LOWKI_RPC_URL="http://localhost:$WALLET_PORT/rpc" LOWKI_CHAIN_ID="$(cast chain-id --rpc-url "$RPC")" LOWKI_NETWORK="Sepolia fork" \
    trunk build --release --dist "$WALLET_DIST" >"$LOG/wallet-build.log" 2>&1) \
    || { echo "   wallet-ui build failed (see $LOG/wallet-build.log)"; [ -f "$WALLET_DIST/index.html" ] && echo "   using the previous build in $WALLET_DIST" || exit 1; }
fi
jq -n --argjson chainId "$(cast chain-id --rpc-url "$RPC")" --arg rpc "http://localhost:$WALLET_PORT/rpc" \
  --arg relayer "${TIPS_PP_RELAYER_URL:-}" --arg asp "${TIPS_PP_ASP_URL:-}" \
  --arg assets "${TIPS_PP_ASSETS_URL:-}" '
  { chainId: $chainId, network: "Sepolia fork", rpcUrl: null, directRpcUrl: $rpc,
    anonUnavailable: "Local fork: the anon network cannot reach a localhost RPC." }
  + (if $relayer != "" and $asp != "" then { private: ({ relayerUrl: null, aspUrl: null,
      directRelayerUrl: $relayer, directAspUrl: $asp, circuitsUrl: "circuits/" }
      + (if $assets != "" then { snapshotUrl: "pp-snapshot.json" } else {} end)) } else {} end)' >"$STATE/lowki-config.json"
PORT=$WALLET_PORT DIST="$WALLET_DIST" SERVER="http://127.0.0.1:$SERVER_PORT" FORK_RPC="$RPC" WALLET_CONFIG="$STATE/lowki-config.json" \
  ASSETS_UPSTREAM="${TIPS_PP_ASSETS_URL:-}" node "$TIPS/scripts/serve-wallet.mjs" >"$LOG/wallet.log" 2>&1 &
PIDS+=($!)

# ------------------------------------------------------------------------------ 6. Lowki Tips
echo "== Lowki Tips on :$TIPS_PORT"
(cd "$TIPS" && VITE_TIPS_TARGET=fork npx vite build >"$LOG/tips-build.log" 2>&1) || { tail -30 "$LOG/tips-build.log"; exit 1; }
(cd "$TIPS" && TIPS_PORT=$TIPS_PORT exec npx vite preview) >"$LOG/tips.log" 2>&1 &
PIDS+=($!)
for _ in $(seq 40); do curl -sf "http://localhost:$TIPS_PORT/" >/dev/null && break; sleep 0.5; done

cat <<EOF

Lowki Tips is up.
  app            http://localhost:$TIPS_PORT/
  creator inbox  http://localhost:$TIPS_PORT/creator.html
  wallet         http://localhost:$WALLET_PORT/
  gateway        http://localhost:$SERVER_PORT/v1/ens/gateway
  fork RPC       $RPC
  email codes    grep "email sign-in code" $STATE/server.log
  state, logs    $STATE
Ctrl-C stops what this script started.
EOF
wait
