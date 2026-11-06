# lowki-worker build/verification entry points. Browser steps run on the macOS host
# (Chrome 156 + chromedriver + wasm-bindgen 0.2.130 in target/tools); from the Linux VM wrap
# them in `mac sh -lc 'cd <this dir> && make <target>'`.
ROOT        := $(abspath ../..)
export CARGO_TARGET_DIR ?= $(ROOT)/target/lowki-worker
TOOLS       ?= $(ROOT)/target/tools
WB          := $(TOOLS)/bin/wasm-bindgen
RUNNER      := $(TOOLS)/bin/wasm-bindgen-test-runner
CHROMEDRIVER ?= $(firstword $(wildcard $(TOOLS)/chromedriver/*/chromedriver-*/chromedriver))
WASM        := $(CARGO_TARGET_DIR)/wasm32-unknown-unknown/release/lowki_worker.wasm
VEC         := $(ROOT)/vectors/lowki-worker
LOG         := $(VEC)/out/chrome-worker.log

.PHONY: all test clippy wasm-build pkg test-chrome verify-proof onchain

all: test clippy wasm-build

test:
	cargo test --release -- --test-threads=1

clippy:
	cargo clippy --all-targets -- -D warnings
	cargo clippy --all-targets --no-default-features -- -D warnings
	cargo clippy --all-targets --target wasm32-unknown-unknown -- -D warnings

wasm-build:
	cargo build --release --target wasm32-unknown-unknown

# JS bindings: pkg/web (ES module for the iframe's Web Worker) and pkg/node (CommonJS).
pkg: wasm-build
	$(WB) --target web --out-dir pkg/web $(WASM)
	$(WB) --target nodejs --out-dir pkg/node $(WASM)

# wasm-bindgen-test in headless Chrome, inside a dedicated Web Worker (tests/browser.rs).
test-chrome:
	mkdir -p $(VEC)/out
	cargo test --release --target wasm32-unknown-unknown --test browser --no-run
	cd browser && CHROMEDRIVER=$(CHROMEDRIVER) WASM_BINDGEN_TEST_TIMEOUT=600 $(RUNNER) \
	  "$$(command ls -t $(CARGO_TARGET_DIR)/wasm32-unknown-unknown/release/deps/browser-*.wasm | head -1)" \
	  --nocapture > $(LOG) 2>&1 || (cat $(LOG); exit 1)
	grep -E 'LOWKI_|test result' $(LOG)
	grep -o 'LOWKI_PROOF_JSON .*' $(LOG) | sed 's/^LOWKI_PROOF_JSON //' > $(VEC)/out/worker-proofs.jsonl

# snarkjs 0.7.5 verifies the browser-made proofs (run where vectors/lowki-worker/node_modules fit the OS).
verify-proof:
	cd $(VEC) && npx tsx verify-proofs.ts out/worker-proofs.jsonl

# eth_call the deployed Sepolia withdrawal verifier with the browser-made withdraw proof.
onchain:
	cd $(VEC) && npx tsx onchain-verifier.ts out/worker-proofs.jsonl
