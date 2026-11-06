#!/bin/sh
# Everything for local development with the real backend:
#   lowki-server (dev mode, :8787) + the demo dapp on :8090 and :8091 + `trunk serve` (:8080).
#
#   apps/wallet-ui/scripts/dev.sh
#
# Then open the "open …" URL printed for Game X. Email codes appear in this terminal.
# Needs: cargo, trunk, node, python3, openssl, and crates/lowki-worker/pkg/web (`make pkg`).
set -eu
HERE=$(cd "$(dirname "$0")" && pwd)
UI=$(cd "$HERE/.." && pwd)
REPO=$(cd "$UI/../.." && pwd)
[ -f "$REPO/crates/lowki-worker/pkg/web/lowki_worker_bg.wasm" ] || { echo "build the worker first: (cd crates/lowki-worker && make pkg)" >&2; exit 1; }

PIDS=""
cleanup() { for p in $PIDS; do kill "$p" 2>/dev/null || true; done; }
trap cleanup INT TERM EXIT

"$HERE/dev-server.sh" & PIDS="$PIDS $!"
# The dapp is served from the repo root (it loads design/tokens.css and the fonts).
(cd "$REPO" && python3 -m http.server 8090 --bind 127.0.0.1 >/dev/null 2>&1) & PIDS="$PIDS $!"
(cd "$REPO" && python3 -m http.server 8091 --bind 127.0.0.1 >/dev/null 2>&1) & PIDS="$PIDS $!"
cd "$UI"
CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-$REPO/target/wallet-ui} trunk serve
