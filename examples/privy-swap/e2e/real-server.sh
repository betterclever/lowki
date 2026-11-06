#!/bin/sh
# Starts lowki-server for e2e/real.mjs with a fresh database (in the Linux VM or on the host):
# dev mode on :8801, wallet origin http://localhost:8800, the example app http://localhost:8802.
# Log (with the email codes): target/privy-swap-e2e/server.log. App ID: target/privy-swap-e2e/apps.json.
#
# Env: LOWKI_SERVER_BIN (a built lowki-server; else `cargo run` in crates/lowki-server).
# Ports 8800-8809 belong to this example; the wallet-ui e2e uses 8787 and 8740-8742.
set -eu
HERE=$(cd "$(dirname "$0")" && pwd)
REPO=$(cd "$HERE/../../.." && pwd)
STATE=$REPO/target/privy-swap-e2e/server
rm -rf "$STATE"; mkdir -p "$STATE"
STATE=$STATE PORT=8801 WALLET_ORIGIN=http://localhost:8800 DAPP_ORIGINS="http://localhost:8802" RUST_LOG=${RUST_LOG:-warn} \
  exec "$REPO/apps/wallet-ui/scripts/dev-server.sh" > "$STATE/server.log" 2>&1
