#!/bin/sh
# Runs lowki-server in dev mode for the wallet UI and registers the demo dapp's apps.
#
#   apps/wallet-ui/scripts/dev-server.sh          # foreground; email codes are printed here
#
# - Email codes are logged (`[lowki dev] email sign-in code for …: 123456`).
# - The wallet origin (trunk serve, http://localhost:8080) is the Lowki origin and passkey
#   RP ID `localhost`. The server listens on 8787; trunk proxies /v1/ to it (Trunk.toml).
# - Two apps are registered, one per demo dapp origin, and their IDs written to
#   $STATE/apps.json: "Game X" (http://localhost:8090) and "Pixel Market" (http://localhost:8091).
#
# Env: PORT (8787), WALLET_ORIGIN (http://localhost:8080), DAPP_ORIGINS (space separated),
#      STATE (target/wallet-ui-dev), LOWKI_SERVER_BIN (else `cargo run` in crates/lowki-server).
set -eu
HERE=$(cd "$(dirname "$0")" && pwd)
REPO=$(cd "$HERE/../../.." && pwd)
PORT=${PORT:-8787}
WALLET_ORIGIN=${WALLET_ORIGIN:-http://localhost:8080}
DAPP_ORIGINS=${DAPP_ORIGINS:-"http://localhost:8090 http://localhost:8091"}
STATE=${STATE:-$REPO/target/wallet-ui-dev}
mkdir -p "$STATE"
# A stable dev secret, so the database and the JWT keys survive restarts.
[ -f "$STATE/secret" ] || openssl rand -hex 32 > "$STATE/secret"
[ -f "$STATE/admin" ] || openssl rand -hex 24 > "$STATE/admin"
ADMIN=$(cat "$STATE/admin")

export LOWKI_DEV_MODE=true
export LOWKI_BIND=0.0.0.0:$PORT
export LOWKI_PUBLIC_URL=http://localhost:$PORT
export LOWKI_ORIGINS=$WALLET_ORIGIN
export LOWKI_RP_ID=localhost
export LOWKI_SERVER_SECRET=$(cat "$STATE/secret")
export LOWKI_ADMIN_TOKEN=$ADMIN
export LOWKI_DATABASE_URL="sqlite://$STATE/lowki.db?mode=rwc"
export RUST_LOG=${RUST_LOG:-info}

if [ -n "${LOWKI_SERVER_BIN:-}" ]; then
  "$LOWKI_SERVER_BIN" &
else
  (cd "$REPO/crates/lowki-server" && CARGO_TARGET_DIR=${CARGO_TARGET_DIR:-$REPO/target/lowki-server} cargo run --quiet) &
fi
PID=$!
trap 'kill $PID 2>/dev/null || true' INT TERM EXIT

URL=http://localhost:$PORT
for _ in $(seq 1 600); do curl -sf "$URL/healthz" >/dev/null 2>&1 && break; sleep 0.5; done
curl -sf "$URL/healthz" >/dev/null || { echo "lowki-server did not start" >&2; exit 1; }

# Register (or reuse) one app per dapp origin.
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
  echo "app $NAME ($ORIGIN): $ID"
  echo "  open $ORIGIN/apps/demo-dapp/?wallet=$WALLET_ORIGIN/&app=$ID&app_name=$(printf '%s' "$NAME" | sed 's/ /%20/g')"
done
echo "$JSON}" > "$STATE/apps.json"
echo "lowki-server ready on $URL (wallet origin $WALLET_ORIGIN); app IDs in $STATE/apps.json"
wait $PID
