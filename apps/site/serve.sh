#!/bin/sh
# Serves apps/site/dist on http://localhost:8840 (PORT/HOST to override), building it first if
# needed. WebGPU needs a secure context: use localhost (or https), not a LAN IP over http.
set -eu
cd "$(dirname "$0")"
[ -f dist/index.html ] || node build.mjs
exec node serve.mjs
