#!/bin/sh
# Tuning sweep (host): TASK_MAX x window bits for the WebGPU prover in Chrome. Restores TASK_MAX.
set -u
B=$(cd "$(dirname "$0")" && pwd)
PLAN=$B/../crates/lowki-gpu/src/plan.rs
ORIG=$(grep -o 'pub const TASK_MAX: usize = [0-9]*' "$PLAN" | grep -o '[0-9]*$')
for K in ${KS:-32 16 8}; do
  sed -i '' "s/pub const TASK_MAX: usize = [0-9]*/pub const TASK_MAX: usize = $K/" "$PLAN"
  "$B/build-wasm.sh" >/dev/null 2>&1 || { echo "build failed"; break; }
  for C in ${CS:-0 5 6 7 8 9}; do
    printf "K=%s c=%s: " "$K" "$C"
    (cd "$B" && QS="&c=$C" node run-browser.mjs g16-wasm-gpu ${ITERS:-5} 2>&1 | grep -oE "g16\.[a-z]+\.wasm_webgpu: prove median [0-9.]+ ms p90 [0-9.]+" | sed 's/g16\.//; s/\.wasm_webgpu: prove//' | tr '\n' ' ')
    echo
  done
done
sed -i '' "s/pub const TASK_MAX: usize = [0-9]*/pub const TASK_MAX: usize = $ORIG/" "$PLAN"
