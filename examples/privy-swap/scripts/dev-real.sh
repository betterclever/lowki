#!/bin/sh
# The Privy-docs example app against the REAL Lowki wallet, locally:
#   lowki-server (dev mode, :8801, the example's origin registered)
#   + the wallet UI (trunk build of apps/wallet-ui, served on :8800 with /v1/ proxied to the server)
#   + the example app (vite dev server, :8802).
#
#   examples/privy-swap/scripts/dev-real.sh            # Sepolia (public RPC) for balances and sends
#   ANVIL=1 examples/privy-swap/scripts/dev-real.sh    # + anvil on :8803 posing as Sepolia (chain 11155111)
#
# Then open the URL it prints. Email sign-in codes appear in this terminal ([lowki dev] …).
# Needs: cargo, trunk, node >= 22, npm deps installed in examples/privy-swap, openssl, the worker
# package (`(cd crates/lowki-worker && make pkg)`), and with ANVIL=1 foundry's anvil.
#
# Ports: 8800 wallet, 8801 lowki-server, 8802 app, 8803 anvil (8800-8809 are this example's;
# the wallet-ui scripts use 8080/8090/8091/8787 and its e2e 8740-8742).
# Env: STATE (target/privy-swap-dev), LOWKI_SERVER_BIN, ANVIL.
set -eu
HERE=$(cd "$(dirname "$0")" && pwd)
EX=$(cd "$HERE/.." && pwd)
REPO=$(cd "$EX/../.." && pwd)
STATE=${STATE:-$REPO/target/privy-swap-dev}
WALLET=http://localhost:8800
APP=http://localhost:8802
[ -f "$REPO/crates/lowki-worker/pkg/web/lowki_worker_bg.wasm" ] || { echo "build the worker first: (cd crates/lowki-worker && make pkg)" >&2; exit 1; }
mkdir -p "$STATE"

PIDS=""
cleanup() { for p in $PIDS; do kill "$p" 2>/dev/null || true; done; }
trap cleanup INT TERM EXIT

# 1. lowki-server, with the example's origin registered as an app.
rm -f "$STATE/apps.json"
STATE=$STATE PORT=8801 WALLET_ORIGIN=$WALLET DAPP_ORIGINS=$APP "$REPO/apps/wallet-ui/scripts/dev-server.sh" & PIDS="$PIDS $!"

# 2. Optional local chain. It runs with Sepolia's chain ID, so the wallet (built for Sepolia)
#    and the example's viem chains agree; fund an account with `cast rpc anvil_setBalance`.
RPC_FLAG=""
if [ "${ANVIL:-0}" = 1 ]; then
  anvil --port 8803 --chain-id 11155111 --silent & PIDS="$PIDS $!"
  RPC_FLAG="--rpc http://localhost:8803"
  export LOWKI_RPC_URL=$WALLET/rpc
fi

# 3. The wallet UI, built with the real backend, served same-origin with lowki-server.
(cd "$REPO/apps/wallet-ui" && CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-$REPO/target/privy-swap-dev/cargo} \
  trunk build --dist "$STATE/wallet-dist")
# shellcheck disable=SC2086
node "$EX/e2e/serve.mjs" "$STATE/wallet-dist" 8800 --v1 http://localhost:8801 $RPC_FLAG & PIDS="$PIDS $!"

# 4. The example app (the SDK is built first; vite aliases @privy-io/react-auth to it).
for _ in $(seq 1 900); do [ -f "$STATE/apps.json" ] && break; sleep 0.5; done
APP_ID=$(node -e 'process.stdout.write(JSON.parse(require("fs").readFileSync(process.argv[1], "utf8"))[process.argv[2]].id)' "$STATE/apps.json" "$APP")
(cd "$EX" && npm run --silent build:sdk && exec npx vite --host localhost --port 8802 --strictPort) & PIDS="$PIDS $!"

WALLET_URL=$(node -e 'process.stdout.write(encodeURIComponent(process.argv[1]))' "$WALLET/?app_name=Privy%20Swap")
echo
echo "open $APP/?app=$APP_ID&wallet=$WALLET_URL"
echo
wait
