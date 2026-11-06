# lowki-groth16 verification entry points. Every step is automated; `make all` runs them all.
ROOT        := $(abspath ../..)
VEC         := $(ROOT)/vectors/lowki-groth16
export CARGO_TARGET_DIR ?= $(ROOT)/target/lowki-groth16
export LOWKI_PP_ARTIFACTS ?= $(abspath $(ROOT)/../vendor/npm/circuits/package/artifacts)
WASM_RUNNER := CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUNNER=wasm-bindgen-test-runner
WASM_LOG    := $(VEC)/out/wasm-test.log

.PHONY: all node vectors test clippy wasm-build verify verify-wasm bench test-webgpu verify-webgpu verify-webgpu-chrome verify-witness-chrome profile

all: test clippy wasm-build verify verify-wasm

node:
	@test -d $(VEC)/node_modules || (cd $(VEC) && npm ci --no-audit --no-fund)

# Regenerate vectors.json + witness/*.bin with snarkjs 0.7.5 (witness/*.bin are needed for
# element-wise diffs; the committed vectors.json alone already pins each full witness by sha256).
vectors: node
	cd $(VEC) && npx tsx gen.ts

# Checks (i) witness, (ii) public signals, (iii) ark verify, (v) Solidity format.
# (iii) also writes $(VEC)/out/rust-proofs.json.
test:
	cargo test --release -- --test-threads=1

clippy:
	cargo clippy --all-targets -- -D warnings
	cargo clippy --all-targets --features parallel -- -D warnings
	cargo clippy --all-targets --target wasm32-unknown-unknown -- -D warnings
	cargo clippy --all-targets --features webgpu -- -D warnings
	cargo clippy --all-targets --features webgpu --target wasm32-unknown-unknown -- -D warnings

wasm-build:
	cargo build --release --target wasm32-unknown-unknown

# Check (iv): snarkjs.groth16.verify on every Rust-generated native proof.
verify: node
	cargo test --release --test vectors iii_rust_proofs_verify_and_export -- --test-threads=1
	cd $(VEC) && npx tsx verify-rust-proofs.ts out/rust-proofs.json

# Prove both circuits inside wasm32 (wasm-bindgen-test on Node), then snarkjs-verify those proofs.
# Needs `cargo install wasm-bindgen-cli --version 0.2.130`.
verify-wasm: node
	mkdir -p $(VEC)/out
	$(WASM_RUNNER) cargo test --release --target wasm32-unknown-unknown --test wasm -- --nocapture > $(WASM_LOG) 2>&1 \
	  || (cat $(WASM_LOG); exit 1)
	grep -E 'LOWKI_(TIMING|MEMORY)|test result' $(WASM_LOG)
	grep -o 'LOWKI_PROOF .*' $(WASM_LOG) | sed 's/^LOWKI_PROOF //' \
	  | node -e 'const l=require("fs").readFileSync(0,"utf8").trim().split("\n").map(JSON.parse);require("fs").writeFileSync(process.argv[1],JSON.stringify(l,null,1))' $(VEC)/out/wasm-proofs.json
	cd $(VEC) && npx tsx verify-rust-proofs.ts out/wasm-proofs.json

bench:
	cargo run --release --example bench -- 5
	cargo run --release --features parallel --example bench -- 5

# ---- WebGPU (feature `webgpu`, crate ../lowki-gpu) ------------------------------------------
# Browser tools live in target/tools (see ../lowki-gpu/README.md):
#   cargo install wasm-bindgen-cli --version 0.2.130 --root <lowki>/target/tools
#   cd <lowki>/target/tools && npx @puppeteer/browsers install chromedriver@<installed Chrome version>
TOOLS        ?= $(ROOT)/target/tools
CHROMEDRIVER ?= $(firstword $(wildcard $(TOOLS)/chromedriver/*/chromedriver-*/chromedriver))
GPU_LOG      := $(VEC)/out/webgpu-chrome.log

# Native GPU (Metal/Vulkan): GPU proof == CPU proof byte for byte for every case, and GPU MSMs
# on the real proving-key bases == ark. Prints SKIP and passes when there is no GPU.
test-webgpu:
	cargo test --release --features webgpu --test gpu -- --nocapture --test-threads=1

# snarkjs.groth16.verify on the native GPU proofs written by test-webgpu.
verify-webgpu: node test-webgpu
	cd $(VEC) && npx tsx verify-rust-proofs.ts out/gpu-proofs.json

# Headless Chrome + WebGPU (wasm32): byte-identical to the CPU proof made in the same wasm
# module, timings, then snarkjs verifies the browser-made GPU proofs.
verify-webgpu-chrome: node
	mkdir -p $(VEC)/out
	cargo test --release --target wasm32-unknown-unknown --features webgpu --test wasm_gpu --no-run
	cd ../lowki-gpu/browser && CHROMEDRIVER=$(CHROMEDRIVER) WASM_BINDGEN_TEST_TIMEOUT=900 \
	  $(TOOLS)/bin/wasm-bindgen-test-runner \
	  "$$(command ls -t $(CARGO_TARGET_DIR)/wasm32-unknown-unknown/release/deps/wasm_gpu-*.wasm | head -1)" \
	  --nocapture > $(GPU_LOG) 2>&1 || (cat $(GPU_LOG); exit 1)
	grep -E 'LOWKI_(GPU|SETUP|TIMING|RESULT|MEMORY)|SKIP|test result' $(GPU_LOG)
	grep -q LOWKI_RESULT $(GPU_LOG)
	grep -o 'LOWKI_PROOF .*' $(GPU_LOG) | sed 's/^LOWKI_PROOF //' \
	  | node -e 'const l=require("fs").readFileSync(0,"utf8").trim().split("\n").map(JSON.parse);require("fs").writeFileSync(process.argv[1],JSON.stringify(l,null,1))' $(VEC)/out/webgpu-proofs.json
	cd $(VEC) && npx tsx verify-rust-proofs.ts out/webgpu-proofs.json

# Headless Chrome (wasm32, main thread): the host-WebAssembly witness path == wasmi == snarkjs
# (witness/*.bin), element-wise, for every case; plus circuit-error parity. Needs the
# witness/*.bin files from `make vectors` and the browser tools above.
verify-witness-chrome:
	cargo test --release --target wasm32-unknown-unknown --test wasm_witness --no-run
	cd ../lowki-gpu/browser && CHROMEDRIVER=$(CHROMEDRIVER) WASM_BINDGEN_TEST_TIMEOUT=900 \
	  $(TOOLS)/bin/wasm-bindgen-test-runner \
	  "$$(command ls -t $(CARGO_TARGET_DIR)/wasm32-unknown-unknown/release/deps/wasm_witness-*.wasm | head -1)" \
	  --nocapture

# Per-stage timings, native: CPU prover and WebGPU prover.
profile:
	cargo run --release --example profile -- 5
	cargo run --release --features webgpu --example gpu_profile -- 6
