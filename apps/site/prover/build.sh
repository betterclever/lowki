#!/bin/sh
# Builds the site's prover wasm (lowki-groth16 + webgpu) into ../dist/pkg. Run on the macOS host
# (uses the repo's wasm-bindgen 0.2.130 in target/tools and Homebrew wasm-opt when present).
set -eu
P=$(cd "$(dirname "$0")" && pwd)
ROOT=$(cd "$P/../../.." && pwd)
T=$ROOT/target/site-prover
OUT=${1:-$P/../build/pkg}
WB=${WASM_BINDGEN:-$ROOT/target/tools/bin/wasm-bindgen}
cd "$P"
CARGO_TARGET_DIR=$T cargo build --release --target wasm32-unknown-unknown
mkdir -p "$OUT"
"$WB" --target web --out-dir "$OUT" "$T/wasm32-unknown-unknown/release/lowki_site_prover.wasm"
if command -v wasm-opt >/dev/null 2>&1; then
  wasm-opt -O3 --enable-bulk-memory --enable-mutable-globals --enable-nontrapping-float-to-int \
    --enable-sign-ext --enable-reference-types --enable-multivalue \
    "$OUT/lowki_site_prover_bg.wasm" -o "$OUT/lowki_site_prover_bg.wasm"
fi
ls -la "$OUT"
