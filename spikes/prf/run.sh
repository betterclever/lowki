#!/usr/bin/env bash
# Runs on the macOS host: build wasm -> wasm-bindgen --target web -> serve -> headless Chrome
# with a CDP virtual authenticator -> PASS/FAIL + JSON.
set -euo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
ROOT="$(cd "$HERE/../.." && pwd)"
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$ROOT/target/prf-spike-mac}"
WASM_BINDGEN="${WASM_BINDGEN:-$ROOT/target/tools/bin/wasm-bindgen}"

(cd "$HERE/wasm" && cargo build --release --target wasm32-unknown-unknown --quiet)
(cd "$HERE/wasm" && cargo clippy --release --target wasm32-unknown-unknown --quiet -- -D warnings)
"$WASM_BINDGEN" --target web --out-dir "$HERE/web/pkg" --no-typescript \
  "$CARGO_TARGET_DIR/wasm32-unknown-unknown/release/lowki_prf_spike.wasm"

set +e
OUT="$(node "$HERE/drive.mjs")"
CODE=$?
set -e
if [ $CODE -eq 0 ]; then echo "PASS"; else echo "FAIL"; fi
echo "$OUT"
exit $CODE
