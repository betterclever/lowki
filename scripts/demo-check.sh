#!/usr/bin/env bash
# Health check of the live demo (scripts/demo-up.sh): hits every PUBLIC URL and runs a fast smoke
# test of each piece. Exit code 0 only if everything passed. Takes ~20 s.
#
#   scripts/demo-check.sh
#   scripts/demo-check.sh code [email]   # just print the latest email sign-in code
#   scripts/demo-check.sh pay <name.lowki.eth> [eth]   # pay a (live) name's fresh stealth address on the fork
#
# Checks: lowki-server health + JWKS (through the wallet host), the ENS gateway (plain viem
# getEnsAddress, twice per name: two different verified addresses) on live Sepolia AND on the
# fork, the wallet page and its runtime config, Lowki Tips and the creator inbox, privy-swap,
# the showcase site, the fork RPC through its tunnel, the fork ASP and relayer, and Anon-RPC
# (requests that reached the fork from an anon-network exit, per the gateway's access log).
set -uo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
LIVE="$ROOT/target/demo-live"
RUN="$LIVE/run"
BIN="$LIVE/bin"
U="$LIVE/urls.json"
if [ "${1:-}" = pay ]; then     # scripts/demo-check.sh pay <name.lowki.eth> [eth]: a payment to the name, on the fork
  NAME="${2:?usage: demo-check.sh pay <name.lowki.eth> [eth]}"; AMT="${3:-0.01}"
  export PATH="$HOME/.foundry/bin:$PATH"
  mkdir -p "$BIN"; ln -sfn "$ROOT/apps/lowki-tips/node_modules" "$BIN/node_modules"
  cat >"$BIN/resolve.mjs" <<'EOF2'
import { createPublicClient, http } from 'viem';
import { sepolia } from 'viem/chains';
const c = createPublicClient({ chain: sepolia, transport: http(process.argv[2], { timeout: 30_000 }) });
console.log(await c.getEnsAddress({ name: process.argv[3] }));
EOF2
  # Live Sepolia: the name may be newer than the fork. The gateway records the stealth address it hands out.
  TO="$(node "$BIN/resolve.mjs" "${PUBLIC_SEPOLIA:-https://ethereum-sepolia-rpc.publicnode.com}" "$NAME")"
  case "$TO" in 0x*) ;; *) echo "could not resolve $NAME: $TO"; exit 1 ;; esac
  FROM="$(cast rpc eth_accounts --rpc-url http://127.0.0.1:8850 | jq -r '.[0]')"
  cast send --unlocked --from "$FROM" "$TO" --value "${AMT}ether" --rpc-url http://127.0.0.1:8850 >/dev/null
  echo "$NAME -> $TO: paid $AMT ETH on the fork (balance $(cast balance "$TO" --ether --rpc-url http://127.0.0.1:8850))"
  exit 0
fi
if [ "${1:-}" = code ]; then   # scripts/demo-check.sh code [email]: the latest email sign-in code
  grep -a 'email sign-in code for' "$RUN/server.log" | { [ -n "${2:-}" ] && grep -F "for $2:" || cat; } | tail -1 | sed 's/.*code for //'
  exit 0
fi
[ -f "$U" ] || { echo "no $U: run scripts/demo-up.sh first"; exit 1; }
j() { jq -r "$1" "$U"; }
TIPS="$(j .tips)"; TIPS="${TIPS%/}"; WALLET="$(j .wallet)"; WALLET="${WALLET%/}"; SITE="$(j .site)"; SITE="${SITE%/}"
PRIVY="$(j .privy)"; FORK="$(j .fork)"; GW="$(j .gateway)"
PUBLIC_SEPOLIA="${PUBLIC_SEPOLIA:-https://ethereum-sepolia-rpc.publicnode.com}"
fails=0
ok() { printf '  \033[32mok\033[0m    %-34s %s\n' "$1" "${2:-}"; }
bad() { printf '  \033[31mFAIL\033[0m  %-34s %s\n' "$1" "${2:-}"; fails=$((fails + 1)); }
get() { curl -sf -m 20 "$@"; }
rpc() { curl -sf -m 20 -X POST -H 'content-type: application/json' --data "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"$2\",\"params\":${3:-[]}}" "$1"; }

echo "== processes (VM pids, Mac tunnels)"
for f in "$RUN"/*.pid; do
  n="$(basename "$f" .pid)"; p="$(cat "$f")"
  case "$n" in
    tunnel-*) mac sh -lc "ps -p $p -o command= | grep -q 'cloudflared tunnel'" 2>/dev/null && ok "$n" "Mac pid $p" || bad "$n" "Mac pid $p is gone" ;;
    *) kill -0 "$p" 2>/dev/null && ok "$n" "pid $p" || bad "$n" "pid $p is gone" ;;
  esac
done
GWT="$(mac sh -lc "pgrep -f 'cloudflared tunnel --no-autoupdate --url http://localhost:8830'" 2>/dev/null | head -1)"
[ -n "$GWT" ] && ok "tunnel :8830 (on-chain URL)" "Mac pid $GWT" || bad "tunnel :8830 (on-chain URL)" "not running: scripts/update-gateway-url.sh"

echo "== lowki-server"
get "$GW/healthz" | grep -q '"ok":true' && ok "ENS gateway host /healthz" "$GW" || bad "ENS gateway host /healthz" "$GW"
P="$(cat "$RUN/server.port" 2>/dev/null || echo 8830)"
get "http://127.0.0.1:$P/healthz" | grep -q '"ok":true' && ok "auth server /healthz (:$P)" || bad "auth server /healthz (:$P)"
n="$(get "$WALLET/.well-known/jwks.json" | jq '.keys | length' 2>/dev/null)"
[ "${n:-0}" -ge 1 ] && ok "JWKS" "$n key(s) at $WALLET/.well-known/jwks.json" || bad "JWKS" "$WALLET/.well-known/jwks.json"
iss="$(get "$WALLET/.well-known/openid-configuration" | jq -r .issuer 2>/dev/null)"
[ "$iss" = "$WALLET" ] && ok "issuer = wallet origin" "$iss" || bad "issuer = wallet origin" "got '$iss'"

echo "== ENS: <name>.lowki.eth -> fresh verified address per lookup (plain viem getEnsAddress)"
mkdir -p "$BIN"; ln -sfn "$ROOT/apps/lowki-tips/node_modules" "$BIN/node_modules"
cat >"$BIN/ens-check.mjs" <<'EOF'
// node ens-check.mjs <rpc> <name...>: getEnsAddress twice per name; ok if two different addresses.
import { createPublicClient, http } from 'viem';
import { sepolia } from 'viem/chains';
const [rpc, ...names] = process.argv.slice(2);
const c = createPublicClient({ chain: sepolia, transport: http(rpc, { timeout: 30_000 }) });
let bad = 0;
for (const name of names) {
  const t = Date.now();
  try {
    const a = await c.getEnsAddress({ name }), b = await c.getEnsAddress({ name });
    const fresh = a && b && a !== b;
    if (!fresh) bad++;
    console.log(`${fresh ? 'ok' : 'FAIL'} ${name} ${a} ${b} ${Date.now() - t}ms`);
  } catch (e) { bad++; console.log(`FAIL ${name} - - ${(e.shortMessage ?? e.message).split('\n')[0]}`); }
}
process.exit(bad ? 1 : 0);
EOF
while read -r st name a b ms; do
  [ "$st" = ok ] && ok "live Sepolia: $name" "$(printf '%.10s… %.10s… %s' "$a" "$b" "$ms")" || bad "live Sepolia: $name" "$a $b $ms"
done < <(node "$BIN/ens-check.mjs" "$PUBLIC_SEPOLIA" chai.lowki.eth tabla.lowki.eth pixel.lowki.eth)
while read -r st name a b ms; do
  [ "$st" = ok ] && ok "fork (via Tips /rpc): $name" "$(printf '%.10s… %.10s… %s' "$a" "$b" "$ms")" || bad "fork (via Tips /rpc): $name" "$a $b $ms"
done < <(node "$BIN/ens-check.mjs" "$TIPS/rpc" chai.lowki.eth)

echo "== apps"
get "$WALLET/" | grep -qi '<title>Lowki' && ok "wallet page" "$WALLET/" || bad "wallet page" "$WALLET/"
cfg="$(get "$WALLET/lowki-config.json")"
[ "$(echo "$cfg" | jq -r .rpcUrl 2>/dev/null)" = "$FORK/rpc" ] && ok "wallet lowki-config.json" "anon RPC $FORK/rpc" || bad "wallet lowki-config.json" "$cfg"
code="$(curl -s -o /dev/null -w '%{http_code}' -m 20 -I "$WALLET/circuits/withdraw.wasm" 2>/dev/null)"
[ "$code" = 200 ] && ok "wallet /circuits (proof artifacts)" || bad "wallet /circuits (proof artifacts)" "HTTP $code"
code="$(curl -s -o /dev/null -w '%{http_code}' -m 30 "$WALLET/pp-snapshot.json")"
[ "$code" = 200 ] && ok "wallet /pp-snapshot.json (pool state)" || bad "wallet /pp-snapshot.json" "HTTP $code"
get "$TIPS/" | grep -q 'Content-Security-Policy' && ok "Lowki Tips" "$TIPS/" || bad "Lowki Tips" "$TIPS/"
get "$TIPS/creator.html" | grep -q 'script' && ok "creator inbox" "$TIPS/creator.html" || bad "creator inbox" "$TIPS/creator.html"
[ "$(rpc "$TIPS/rpc" anvil_setBalance '["0x000000000000000000000000000000000000dEaD","0x1"]' | jq -r '.error // "ok"')" = ok ] && ok "Tips faucet (anvil_setBalance)" || bad "Tips faucet (anvil_setBalance)"
get "${PRIVY%%\?*}" | grep -q 'lowki:wallet-url' && ok "privy-swap" "${PRIVY%%\?*}" || bad "privy-swap" "${PRIVY%%\?*}"
get "$SITE/" -o /dev/null && ok "showcase site" "$SITE/" || bad "showcase site" "$SITE/"
hdr="$(curl -sI -m 20 "$SITE/" | tr -d '\r' | grep -i '^cross-origin-embedder-policy' || true)"
[ -n "$hdr" ] && ok "site cross-origin isolation" "$hdr" || bad "site cross-origin isolation" "no COEP header"
get "$SITE/site-config.json" | jq -e '.live.tips' >/dev/null 2>&1 && ok "site-config.json (demo links)" || bad "site-config.json (demo links)"

echo "== fork (through the public tunnel)"
cid="$(rpc "$FORK/rpc" eth_chainId | jq -r .result 2>/dev/null)"
[ "$cid" = 0xaa36a7 ] && ok "fork RPC eth_chainId" "$cid (Sepolia) $FORK/rpc" || bad "fork RPC eth_chainId" "got '$cid'"
bn="$(rpc "$FORK/rpc" eth_blockNumber | jq -r .result 2>/dev/null)"
[ -n "$bn" ] && [ "$bn" != null ] && ok "fork head" "block $((bn)) (forked at $(j .forkBlock))" || bad "fork head"
get "$FORK/asp/status" | jq -e '.roots | length >= 1' >/dev/null 2>&1 && ok "ASP (simulated, fork)" "$(get "$FORK/asp/status" | jq -c '{approved: (.approved|length), roots: (.roots|length)}')" || bad "ASP"
get "$FORK/relayer/details" | jq -e .feeBPS >/dev/null 2>&1 && ok "relayer (simulated, fork)" "fee $(get "$FORK/relayer/details" | jq -r .feeBPS) bps" || bad "relayer"
me="$(get "$FORK/ip")"; via="$(echo "$me" | jq -r .via 2>/dev/null)"; myip="$(echo "$me" | jq -r .ip 2>/dev/null)"
[ "$via" = tunnel ] && ok "fork /ip through the tunnel" "via=$via (this machine: $myip)" || bad "fork /ip through the tunnel" "via=$via"

echo "== Anon-RPC"
LOG="$LIVE/stack/rpc-access.jsonl"
if [ -f "$LOG" ]; then
  # Requests through the tunnel from an IP that is not this machine's: anon-network exits.
  exits="$(grep '"via":"tunnel"' "$LOG" | jq -r --arg me "$myip" 'select(.ip != $me) | .t + " " + .ip' 2>/dev/null)"
  if [ -n "$exits" ]; then
    ok "fork reached from anon exits" "$(echo "$exits" | wc -l | tr -d ' ') requests from $(echo "$exits" | awk '{print $2}' | sort -u | wc -l | tr -d ' ') exit IP(s); last $(echo "$exits" | tail -1)"
  else
    ok "fork reached from anon exits" "none yet (open the wallet: it boots Nox and reads through it)"
  fi
else
  bad "fork access log" "$LOG missing"
fi
echo "   (The wallet shows 'Private connection … exit <ip>' on its home once Nox has booted; ~3-10 s.)"

echo
if [ $fails = 0 ]; then echo "ALL CHECKS PASSED"; else echo "$fails CHECK(S) FAILED"; fi
exit $((fails > 0))
