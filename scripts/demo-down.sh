#!/usr/bin/env bash
# Stops what scripts/demo-up.sh started, and nothing else.
#
#   scripts/demo-down.sh                 # services and their tunnels
#   KEEP_TUNNELS=1 scripts/demo-down.sh  # services only: the public URLs (and passkeys, whose RP ID
#                                        # is the wallet's hostname) survive the next demo-up
#
# It reads target/demo-live/run/*.pid. The :8830 tunnel (its URL is written on chain) was not started
# by demo-up and is never touched. No pkill by name: every pid is checked before it is signalled.
set -uo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
RUN="$ROOT/target/demo-live/run"
[ -d "$RUN" ] || { echo "nothing to stop ($RUN does not exist)"; exit 0; }

for f in "$RUN"/*.pid; do
  [ -e "$f" ] || continue
  name="$(basename "$f" .pid)"
  pid="$(cat "$f")"
  case "$name" in
    tunnel-*)
      [ "${KEEP_TUNNELS:-0}" = 1 ] && { echo "kept   $name (Mac pid $pid)"; continue; }
      # Only a cloudflared whose log is ours (the URL it printed is in our log file).
      if mac sh -lc "ps -p $pid -o command= 2>/dev/null | grep -q 'cloudflared tunnel'" 2>/dev/null; then
        mac sh -lc "kill $pid" && echo "stopped $name (Mac pid $pid)"
      else
        echo "gone   $name"
      fi
      rm -f "$f" ;;
    *)
      if kill -0 "$pid" 2>/dev/null; then
        kill "$pid" 2>/dev/null && echo "stopped $name (pid $pid)"
        for _ in $(seq 1 20); do kill -0 "$pid" 2>/dev/null || break; sleep 0.25; done
        kill -0 "$pid" 2>/dev/null && { kill -9 "$pid" 2>/dev/null; echo "        (killed -9)"; }
      else
        echo "gone   $name"
      fi
      rm -f "$f" ;;
  esac
done
rm -f "$RUN/server.fp" "$RUN/wallet.fp" "$RUN/server.port"
echo "done. (The :8830 tunnel and anything this script did not start are untouched.)"
