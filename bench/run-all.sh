#!/bin/sh
# Full benchmark run. Run ON THE macOS HOST (from the Linux VM: mac sh -lc '<path>/bench/run-all.sh').
# Steps run sequentially so nothing competes for the CPU/GPU. Outputs: results/*.json, results/tables.md.
set -eu
B=$(cd "$(dirname "$0")" && pwd)
L=$B/..
cd "$B"
mkdir -p results
log() { echo "== $(date '+%H:%M:%S') $*"; }

log "environment"
{
  echo "date: $(date)"
  echo "cpu: $(sysctl -n machdep.cpu.brand_string), $(sysctl -n hw.ncpu) logical cores ($(sysctl -n hw.perflevel0.logicalcpu 2>/dev/null) P + $(sysctl -n hw.perflevel1.logicalcpu 2>/dev/null) E), $(( $(sysctl -n hw.memsize) / 1073741824 )) GiB"
  echo "os: $(sw_vers -productName) $(sw_vers -productVersion) ($(sw_vers -buildVersion)), $(uname -m)"
  echo "chrome: $("/Applications/Google Chrome.app/Contents/MacOS/Google Chrome" --version)"
  echo "node: $(node -v)"
  echo "rustc: $(rustc -V)"
  echo "wasm-bindgen: $("$L/target/tools/bin/wasm-bindgen" --version)"
  echo "wasm-opt: $(wasm-opt --version)"
  echo "esbuild: $(node_modules/.bin/esbuild --version)"
  echo "npm packages:"; npm ls --depth=0 2>/dev/null | tail -n +2
  echo "  (viem's own @noble/curves: $(node -p "require('./node_modules/viem/node_modules/@noble/curves/package.json').version" 2>/dev/null || echo n/a))"
  echo "  (ffjavascript: $(node -p "require('./node_modules/ffjavascript/package.json').version"))"
  echo "kohaku: $(git -C "$L/../vendor/kohaku" rev-parse HEAD 2>/dev/null || echo 8c83d64050040654082d70901a46a339bfa103b0)"
} > results/env.txt
cat results/env.txt

log "build JS (esbuild) + wasm + native"
node build.mjs
[ -f fixtures/pp.json ] || node dist/gen-fixtures.mjs
./build-wasm.sh
(cd rust && CARGO_TARGET_DIR=$L/target/bench-mac cargo build --release --features webgpu)
(cd rust && CARGO_TARGET_DIR=$L/target/bench-mac-par cargo build --release --features parallel)

log "native (1 thread + Metal)"
"$L/target/bench-mac/release/native" results/native-1t.json
log "native (rayon parallel, groth16 only)"
"$L/target/bench-mac-par/release/native" results/native-par.json --no-micro
log "snarkjs-verify the native proofs"
node dist/node-bench.mjs verify results/native-verify.json results/native-1t.json.proofs.json results/native-par.json.proofs.json

log "Node: TS micro"
node dist/node-bench.mjs micro results/node-micro.json
log "Node: snarkjs (default threads)"
node dist/node-bench.mjs g16-mt results/node-g16-mt.json
log "Node: snarkjs (singleThread)"
node dist/node-bench.mjs g16-st results/node-g16-st.json

for s in wasm-micro ts-micro g16-wasm-cpu g16-wasm-gpu g16-snarkjs-mt g16-snarkjs-st; do
  log "Chrome: $s"
  node run-browser.mjs "$s" 10
done
log "Chrome: g16-snarkjs-mt and g16-wasm-gpu without cross-origin isolation"
NO_ISOLATION=1 node run-browser.mjs g16-snarkjs-mt 10
NO_ISOLATION=1 node run-browser.mjs g16-wasm-gpu 10

log "payload size"
./size.sh
node size-report.mjs

log "report"
node report.mjs > /dev/null
log "done: results/tables.md"
