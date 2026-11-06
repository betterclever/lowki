#!/bin/sh
# Payload size: one wasm module per area (bench/rust/src/size.rs, feature size-only + area),
# vs one minified esbuild bundle per area (bench/size/js-*.ts). gzip -9 for both. Host only.
#   "speed" = opt-level 3 + wasm-opt -O3 (what the timing module uses)
#   "size"  = opt-level z + wasm-opt -Oz
set -eu
B=$(cd "$(dirname "$0")" && pwd)
TOOLS=$B/../target/tools/bin
F="--enable-bulk-memory --enable-mutable-globals --enable-nontrapping-float-to-int --enable-sign-ext --enable-reference-types --enable-multivalue"
OUT=$B/size/wasm
rm -rf "$OUT"; mkdir -p "$OUT"
gz() { gzip -9 -c "$1" | wc -c | tr -d ' '; }
raw() { wc -c < "$1" | tr -d ' '; }
cd "$B/rust"
echo "area,variant,wasm_raw,wasm_gz,glue_raw,glue_gz" > "$B/results/size-wasm.csv"
for area in keys pp stealth aa g16 webgpu all; do
  feats="size-only,$area"
  [ "$area" = all ] && feats="size-only,keys,pp,stealth,aa,webgpu"
  for variant in speed size; do
    if [ $variant = speed ]; then T=$B/../target/bench-mac; OL=3; WO=-O3; else T=$B/../target/bench-mac-z; OL=z; WO=-Oz; fi
    CARGO_PROFILE_RELEASE_OPT_LEVEL=$OL CARGO_TARGET_DIR=$T cargo build -q --release --target wasm32-unknown-unknown --lib --no-default-features --features "$feats"
    d=$OUT/$area-$variant; mkdir -p "$d"
    "$TOOLS/wasm-bindgen" --target web --out-dir "$d" "$T/wasm32-unknown-unknown/release/lowki_bench.wasm"
    wasm-opt $WO $F "$d/lowki_bench_bg.wasm" -o "$d/lowki_bench_bg.wasm"
    # minify the glue like the JS side (esbuild --minify)
    "$B/node_modules/.bin/esbuild" --minify "$d/lowki_bench.js" --outfile="$d/glue.min.js" --log-level=warning
    echo "$area,$variant,$(raw "$d/lowki_bench_bg.wasm"),$(gz "$d/lowki_bench_bg.wasm"),$(raw "$d/glue.min.js"),$(gz "$d/glue.min.js")" | tee -a "$B/results/size-wasm.csv"
  done
done
cd "$B"
node build.mjs size
echo "area,js_raw,js_gz" > "$B/results/size-js.csv"
for a in keys pp stealth aa g16 all; do
  echo "$a,$(raw size/out/$a.js),$(gz size/out/$a.js)" | tee -a "$B/results/size-js.csv"
done
