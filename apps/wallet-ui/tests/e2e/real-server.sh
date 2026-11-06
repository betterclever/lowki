#!/bin/sh
# Starts lowki-server for tests/e2e/real.mjs (in the Linux VM or on the host) with a fresh
# database: wallet origin http://localhost:8740, dapp origins :8741 and :8742.
# Log (with the email codes): target/wallet-ui-e2e/server.log. App IDs: target/wallet-ui-e2e/apps.json.
set -eu
HERE=$(cd "$(dirname "$0")" && pwd)
REPO=$(cd "$HERE/../../../.." && pwd)
STATE=$REPO/target/wallet-ui-e2e
rm -rf "$STATE"; mkdir -p "$STATE"
STATE=$STATE WALLET_ORIGIN=http://localhost:8740 DAPP_ORIGINS="http://localhost:8741 http://localhost:8742" RUST_LOG=warn \
  exec "$REPO/apps/wallet-ui/scripts/dev-server.sh" > "$STATE/server.log" 2>&1
