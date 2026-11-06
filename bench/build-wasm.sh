#!/bin/sh
# Builds the timing wasm module (all crates + webgpu) into web/pkg. Host only.
set -eu
B=$(cd "$(dirname "$0")" && pwd)
T=$B/../target/bench-mac
TOOLS=$B/../target/tools/bin
WASM_OPT_FEATURES="--enable-bulk-memory --enable-mutable-globals --enable-nontrapping-float-to-int --enable-sign-ext --enable-reference-types --enable-multivalue"
cd "$B/rust"
CARGO_TARGET_DIR=$T cargo build --release --target wasm32-unknown-unknown --lib --features webgpu
"$TOOLS/wasm-bindgen" --target web --out-dir "$B/web/pkg" "$T/wasm32-unknown-unknown/release/lowki_bench.wasm"
wasm-opt -O3 $WASM_OPT_FEATURES "$B/web/pkg/lowki_bench_bg.wasm" -o "$B/web/pkg/lowki_bench_bg.wasm"
ls -la "$B/web/pkg"
