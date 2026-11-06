#!/usr/bin/env bash
# Lowki ENSv2 stealth names, end to end on an anvil fork of Sepolia (real ENSv2 contracts).
#
#   contracts/e2e/run.sh
#
# 1. anvil --fork-url $SEPOLIA_RPC_URL (default: Tenderly's public Sepolia gateway, which
#    serves historical state; any Sepolia RPC works for a fork of the latest block)
# 2. a fresh operator key per run (funded on the fork with anvil_setBalance)
# 3. forge scripts: RegisterParent (availability check, commit, +61 s, register, MockUSDC),
#    DeployLowki (registry, registrar, owner-account delegate, Fluidkey verifier + resolver)
# 4. lowki-server with the ENS gateway enabled, on a throwaway SQLite file
# 5. e2e.ts: steps 2-7 (sponsored registration, sponsored owner setup, client-side path vs
#    ScopeLift, plain-viem gateway path, tamper rejection, payments + recipient scan)
#
# Needs: foundry (forge, anvil, cast), cargo, node 24, openssl, jq.
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CONTRACTS="$(cd "$HERE/.." && pwd)"
ROOT="$(cd "$CONTRACTS/.." && pwd)"
export PATH="$HOME/.foundry/bin:$PATH"

SEPOLIA_RPC_URL="${SEPOLIA_RPC_URL:-https://sepolia.gateway.tenderly.co}"
ANVIL_PORT="${ANVIL_PORT:-8545}"
GW_PORT="${GW_PORT:-8797}"
RPC="http://127.0.0.1:$ANVIL_PORT"
for p in "$ANVIL_PORT" "$GW_PORT"; do
  if (exec 3<>"/dev/tcp/127.0.0.1/$p") 2>/dev/null; then echo "port $p is in use; set ANVIL_PORT/GW_PORT"; exit 1; fi
done
WORK="$(mktemp -d)"
LOG="$WORK/logs"; mkdir -p "$LOG"
PIDS=()
# E2E_KEEP=1 leaves anvil and the server running for inspection.
cleanup() { [ "${E2E_KEEP:-}" = 1 ] && return; for p in "${PIDS[@]}"; do kill "$p" 2>/dev/null || true; done; }
trap cleanup EXIT

echo "== anvil fork of Sepolia ($SEPOLIA_RPC_URL)"
anvil --fork-url "$SEPOLIA_RPC_URL" --port "$ANVIL_PORT" --silent >"$LOG/anvil.log" 2>&1 &
PIDS+=($!)
for _ in $(seq 60); do cast chain-id --rpc-url "$RPC" >/dev/null 2>&1 && break; sleep 1; done
echo "   fork block $(cast block-number --rpc-url "$RPC"), chain id $(cast chain-id --rpc-url "$RPC")"

# Operator / Lowki admin key: random per run, never printed. On live Sepolia this is
# LOWKI_SEPOLIA_KEY (see contracts/README.md).
export LOWKI_KEY_ENV=LOWKI_FORK_KEY
LOWKI_FORK_KEY="0x$(openssl rand -hex 32)"; export LOWKI_FORK_KEY
OPERATOR="$(cast wallet address "$LOWKI_FORK_KEY")"
cast rpc anvil_setBalance "$OPERATOR" 0x56BC75E2D63100000 --rpc-url "$RPC" >/dev/null
echo "   operator $OPERATOR (funded on the fork only)"

cd "$CONTRACTS"
export LOWKI_PARENT_LABEL="${LOWKI_PARENT_LABEL:-lowki}"
export LOWKI_COMMIT_SECRET="0x$(openssl rand -hex 32)"
export LOWKI_GATEWAY_URL="http://127.0.0.1:$GW_PORT/v1/ens/gateway"
export LOWKI_DEPLOYMENTS_OUT="deployments/local/fork-lowki.json"

echo "== register $LOWKI_PARENT_LABEL.eth (ETHRegistrar commit-reveal, MockUSDC)"
forge script script/RegisterParent.s.sol --sig "check()" --rpc-url "$RPC" 2>&1 | sed -n '/Logs:/,/^$/p'
forge script script/RegisterParent.s.sol --sig "commit()" --rpc-url "$RPC" --broadcast >"$LOG/commit.log" 2>&1 || { cat "$LOG/commit.log"; exit 1; }
cast rpc evm_increaseTime 61 --rpc-url "$RPC" >/dev/null && cast rpc evm_mine --rpc-url "$RPC" >/dev/null
forge script script/RegisterParent.s.sol --sig "register()" --rpc-url "$RPC" --broadcast >"$LOG/register.log" 2>&1 || { cat "$LOG/register.log"; exit 1; }
grep -E "\.eth: status" "$LOG/register.log" | tail -1

echo "== deploy Lowki registry/registrar + Fluidkey StealthResolver, configure parent"
forge script script/DeployLowki.s.sol --rpc-url "$RPC" --broadcast >"$LOG/deploy.log" 2>&1 || { cat "$LOG/deploy.log"; exit 1; }
cat "$LOWKI_DEPLOYMENTS_OUT"

echo "== build lowki-server and the lowki-ens e2e client"
(cd "$ROOT/crates/lowki-server" && CARGO_TARGET_DIR="$ROOT/target/lowki-server" cargo build --quiet)
(cd "$HERE/client" && CARGO_TARGET_DIR="$ROOT/target/lowki-ens-e2e" cargo build --quiet)

ENS_JSON="$CONTRACTS/deployments/sepolia-ensv2.json"
echo "== lowki-server with the ENS gateway on :$GW_PORT"
(
  export LOWKI_DEV_MODE=true LOWKI_PUBLIC_URL="http://localhost:$GW_PORT" LOWKI_BIND="127.0.0.1:$GW_PORT"
  LOWKI_SERVER_SECRET="$(openssl rand -hex 32)"; export LOWKI_SERVER_SECRET
  export LOWKI_DATABASE_URL="sqlite://$WORK/lowki.db?mode=rwc"
  export LOWKI_ENS_GATEWAY=true LOWKI_ENS_RPC_URL="$RPC" LOWKI_ENS_PARENT="$LOWKI_PARENT_LABEL.eth"
  LOWKI_ENS_UNIVERSAL_RESOLVER="$(jq -r .ens.UniversalResolver "$ENS_JSON")"; export LOWKI_ENS_UNIVERSAL_RESOLVER
  LOWKI_ENS_STEALTH_RESOLVER="$(jq -r .StealthResolver "$CONTRACTS/$LOWKI_DEPLOYMENTS_OUT")"; export LOWKI_ENS_STEALTH_RESOLVER
  export LOWKI_ENS_META_CACHE_SECS=0 RUST_LOG=info
  exec "$ROOT/target/lowki-server/debug/lowki-server"
) >"$LOG/server.log" 2>&1 &
PIDS+=($!)
for _ in $(seq 60); do curl -sf "http://127.0.0.1:$GW_PORT/healthz" >/dev/null && break; sleep 0.5; done
curl -sf "http://127.0.0.1:$GW_PORT/healthz" >/dev/null || { echo "lowki-server did not start:"; tail -20 "$LOG/server.log"; exit 1; }

echo "== e2e (viem as an unmodified wallet; Rust for keys, calldata, derivation, scanning)"
cd "$HERE"
[ -d node_modules ] || npm ci --no-audit --no-fund
E2E_RPC="$RPC" E2E_CLI="$ROOT/target/lowki-ens-e2e/debug/lowki-ens-e2e" \
E2E_GATEWAY_BASE="http://127.0.0.1:$GW_PORT" E2E_ENS_CONFIG="$ENS_JSON" \
E2E_LOWKI_DEPLOYMENT="$CONTRACTS/$LOWKI_DEPLOYMENTS_OUT" E2E_OPERATOR_KEY="$LOWKI_FORK_KEY" \
  npx tsx e2e.ts

echo "== server log (should contain no names, addresses or keys)"
echo "   lines mentioning a name or an address: $(grep -ciE "alice|0x[0-9a-f]{40}" "$LOG/server.log" || true)"
echo "logs: $LOG"
