#!/bin/sh
# Builds lowki-server, starts it on localhost with a throwaway database and secret, and runs the
# headless-Chrome WebAuthn test against it. Runs the browser part on the macOS host through
# `mac sh -lc` when that helper exists (OrbStack dev VM), otherwise locally.
set -eu
HERE=$(cd "$(dirname "$0")" && pwd)
CRATE=$(cd "$HERE/../.." && pwd)
PORT=${PORT:-18080}
export CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-$CRATE/../../target/lowki-server}
TMP=$(mktemp -d)
trap 'kill $PID 2>/dev/null || true; rm -rf "$TMP"' EXIT

(cd "$CRATE" && cargo build --quiet)
LOWKI_DEV_MODE=true LOWKI_BIND=0.0.0.0:$PORT LOWKI_PUBLIC_URL=http://localhost:$PORT \
LOWKI_SERVER_SECRET=$(openssl rand -hex 32) LOWKI_DATABASE_URL="sqlite://$TMP/e2e.db?mode=rwc" \
RUST_LOG=warn "$CARGO_TARGET_DIR/debug/lowki-server" > "$TMP/server.log" 2>&1 &
PID=$!
for _ in $(seq 1 50); do curl -sf "http://localhost:$PORT/healthz" >/dev/null && break; sleep 0.2; done

if command -v mac >/dev/null 2>&1; then
  mac sh -lc "cd '$HERE' && node webauthn-e2e.mjs http://localhost:$PORT"
else
  node "$HERE/webauthn-e2e.mjs" "http://localhost:$PORT"
fi
