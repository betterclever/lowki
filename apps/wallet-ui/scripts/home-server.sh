#!/bin/sh
# lowki-server for the wallet home on LIVE Sepolia: passkeys/email (dev mode, codes in the log),
# the ENS stealth gateway for lowki.eth and sponsored name claims.
#
#   apps/wallet-ui/scripts/home-server.sh           # foreground
#
# - Listens on :8830. The Cloudflare tunnel in contracts/deployments/sepolia-lowki.json
#   (`gatewayUrl`, also written on chain for every name) points here.
# - Wallet origin http://localhost:8831 (tests/e2e/home.mjs serves dist/ there with a /v1 proxy);
#   demo dapps http://localhost:8832 ("Game X") and :8833 ("Pixel Market"). App IDs go to
#   $STATE/apps.json.
# - The operator key is read from $LOWKI_KEY_FILE (default /Users/betterclever/.lowki/sepolia-test.env,
#   LOWKI_SEPOLIA_KEY) into LOWKI_ENS_OPERATOR_KEY. It is never printed.
#
# Env: PORT (8830), WALLET_ORIGIN, DAPP_ORIGINS, STATE (target/wallet-home), SEPOLIA_RPC_URL,
#      LOWKI_KEY_FILE, LOWKI_SERVER_BIN (else target/lowki-server/release/lowki-server).
set -eu
HERE=$(cd "$(dirname "$0")" && pwd)
REPO=$(cd "$HERE/../../.." && pwd)
PORT=${PORT:-8830}
WALLET_ORIGIN=${WALLET_ORIGIN:-http://localhost:8831}
DAPP_ORIGINS=${DAPP_ORIGINS:-"http://localhost:8832 http://localhost:8833"}
STATE=${STATE:-$REPO/target/wallet-home}
KEY_FILE=${LOWKI_KEY_FILE:-/Users/betterclever/.lowki/sepolia-test.env}
RPC=${SEPOLIA_RPC_URL:-https://ethereum-sepolia-rpc.publicnode.com}
DEP=$REPO/contracts/deployments/sepolia-lowki.json
ENS=$REPO/contracts/deployments/sepolia-ensv2.json
BIN=${LOWKI_SERVER_BIN:-$REPO/target/lowki-server/release/lowki-server}
mkdir -p "$STATE"
[ -f "$STATE/secret" ] || openssl rand -hex 32 > "$STATE/secret"
[ -f "$STATE/admin" ] || openssl rand -hex 24 > "$STATE/admin"
ADMIN=$(cat "$STATE/admin")
j() { node -e 'const j=JSON.parse(require("fs").readFileSync(process.argv[1],"utf8"));let v=j;for(const k of process.argv[2].split("."))v=v[k];process.stdout.write(String(v))' "$1" "$2"; }

export LOWKI_DEV_MODE=true
export LOWKI_BIND=0.0.0.0:$PORT
export LOWKI_PUBLIC_URL=http://localhost:$PORT
export LOWKI_ORIGINS=$WALLET_ORIGIN
export LOWKI_RP_ID=localhost
export LOWKI_SERVER_SECRET=$(cat "$STATE/secret")
export LOWKI_ADMIN_TOKEN=$ADMIN
export LOWKI_DATABASE_URL="sqlite://$STATE/lowki.db?mode=rwc"
export RUST_LOG=${RUST_LOG:-info}
# ENS stealth gateway (lowki.eth on live Sepolia)
export LOWKI_ENS_GATEWAY=true LOWKI_ENS_RPC_URL=$RPC
export LOWKI_ENS_PARENT=$(j "$DEP" parentName)
export LOWKI_ENS_UNIVERSAL_RESOLVER=$(j "$ENS" ens.UniversalResolver)
export LOWKI_ENS_STEALTH_RESOLVER=$(j "$DEP" StealthResolver)
export LOWKI_ENS_LOOKUPS_PER_MIN_IP=120 LOWKI_ENS_META_CACHE_SECS=5
# Sponsored name claims
export LOWKI_ENS_REGISTRAR=$(j "$DEP" LowkiRegistrar)
export LOWKI_ENS_REGISTRY=$(j "$DEP" LowkiRegistry)
export LOWKI_ENS_OWNER_ACCOUNT=$(j "$DEP" LowkiOwnerAccount)
export LOWKI_ENS_GATEWAY_URL=$(j "$DEP" gatewayUrl)
export LOWKI_ENS_VERIFIABLE_FACTORY=$(j "$ENS" ens.VerifiableFactory)
export LOWKI_ENS_PERMISSIONED_RESOLVER_IMPL=$(j "$ENS" ens.PermissionedResolverImpl)
export LOWKI_ENS_CHAIN_ID=$(j "$DEP" chainId)
if [ -f "$KEY_FILE" ]; then
  set -a; . "$KEY_FILE"; set +a
  [ -n "${LOWKI_SEPOLIA_KEY:-}" ] && export LOWKI_ENS_OPERATOR_KEY=$LOWKI_SEPOLIA_KEY
  unset LOWKI_SEPOLIA_KEY
fi
[ -n "${LOWKI_ENS_OPERATOR_KEY:-}" ] || echo "no operator key: name claims are off" >&2

"$BIN" &
PID=$!
trap 'kill $PID 2>/dev/null || true' INT TERM EXIT
unset LOWKI_ENS_OPERATOR_KEY

URL=http://localhost:$PORT
for _ in $(seq 1 120); do curl -sf "$URL/healthz" >/dev/null 2>&1 && break; sleep 0.5; done
curl -sf "$URL/healthz" >/dev/null || { echo "lowki-server did not start" >&2; exit 1; }

EXISTING=$(curl -sf "$URL/admin/v1/apps" -H "authorization: Bearer $ADMIN")
N=0
JSON="{"
for ORIGIN in $DAPP_ORIGINS; do
  N=$((N + 1))
  case $N in 1) NAME="Game X" ;; 2) NAME="Pixel Market" ;; *) NAME="Demo app $N" ;; esac
  ID=$(printf '%s' "$EXISTING" | node -e '
    let s = ""; process.stdin.on("data", (d) => (s += d)).on("end", () => {
      const j = JSON.parse(s); const list = Array.isArray(j) ? j : (j.apps || []);
      const a = list.find((a) => (a.allowed_origins || []).includes(process.argv[1]));
      process.stdout.write(a ? a.id : "");
    });' "$ORIGIN")
  if [ -z "$ID" ]; then
    ID=$(curl -sf -X POST "$URL/admin/v1/apps" -H "authorization: Bearer $ADMIN" -H 'content-type: application/json' \
      -d "{\"name\":\"$NAME\",\"allowed_origins\":[\"$ORIGIN\"]}" | node -e '
      let s = ""; process.stdin.on("data", (d) => (s += d)).on("end", () => process.stdout.write(JSON.parse(s).id));')
  fi
  [ $N -gt 1 ] && JSON="$JSON,"
  JSON="$JSON\"$ORIGIN\":{\"id\":\"$ID\",\"name\":\"$NAME\"}"
done
echo "$JSON}" > "$STATE/apps.json"
echo "lowki-server (live Sepolia ENS) ready on $URL; wallet origin $WALLET_ORIGIN; apps in $STATE/apps.json"
wait $PID
