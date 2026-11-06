#!/usr/bin/env bash
# Private send (Privacy Pools v1 withdraw through the relay path) + ragequit, end to end on an
# anvil fork of Sepolia, with Lowki's worker for shielding, signing and proving and Kohaku's
# privacy-pools plugin on the Lowki Host for sync. See docs/PRIVATE-SEND.md.
#
#   scripts/fork-private-send.sh
#
# Env: FORK_URL (archive-capable Sepolia RPC that serves 5000-block eth_getLogs;
#      default https://sepolia.gateway.tenderly.co), FORK_BLOCK (default 12078500; a new block
#      means one ~80 s catch-up sync, then it is cached), PORT (default 8547), ANVIL (binary),
#      plus everything packages/lowki-kohaku-host/scripts/fork-private-send.ts reads.
set -euo pipefail
here="$(cd "$(dirname "$0")/.." && pwd)"
export PATH="$HOME/.foundry/bin:$PATH"
FORK_URL="${FORK_URL:-https://sepolia.gateway.tenderly.co}"
FORK_BLOCK="${FORK_BLOCK:-12078500}"
PORT="${PORT:-8547}"
ANVIL="${ANVIL:-anvil}"
command -v "$ANVIL" >/dev/null || { echo "anvil not found: curl -L https://foundry.paradigm.xyz | bash && foundryup" >&2; exit 1; }
[ -f "$here/crates/lowki-worker/pkg/node/lowki_worker.js" ] || { echo "build the worker first: (cd crates/lowki-worker && make pkg)" >&2; exit 1; }
[ -d "$here/packages/lowki-kohaku-host/node_modules" ] || (cd "$here/packages/lowki-kohaku-host" && npm ci)

log="$(mktemp -t lowki-anvil.XXXXXX)"
"$ANVIL" --fork-url "$FORK_URL" --fork-block-number "$FORK_BLOCK" --chain-id 11155111 --port "$PORT" --silent >"$log" 2>&1 &
anvil_pid=$!
trap 'kill $anvil_pid 2>/dev/null || true' EXIT
for _ in $(seq 1 60); do
  curl -s -o /dev/null -X POST -H 'content-type: application/json' \
    --data '{"jsonrpc":"2.0","id":1,"method":"eth_chainId","params":[]}' "http://127.0.0.1:$PORT" && break
  kill -0 $anvil_pid 2>/dev/null || { cat "$log" >&2; exit 1; }
  sleep 0.5
done

cd "$here/packages/lowki-kohaku-host"
FORK_RPC="http://127.0.0.1:$PORT" node --import ./resolve-hook.mjs --import tsx scripts/fork-private-send.ts
