#!/usr/bin/env bash
# Re-points lowki.eth and the creators' StealthResolver gateway URL on LIVE Sepolia, for when the
# :8830 cloudflared quick tunnel (whose URL is written on chain) died and came back with a new URL.
#
#   scripts/update-gateway-url.sh                  # :8830 tunnel dead? start a new one, re-point
#   scripts/update-gateway-url.sh https://x.trycloudflare.com/v1/ens/gateway   # use this URL
#   DRY_RUN=1 scripts/update-gateway-url.sh [url]  # simulate every transaction, send nothing
#
# Transactions (all sent and paid by the operator key, LOWKI_SEPOLIA_KEY):
#   1. StealthResolver.setConfig(lowki.eth, ParentRecords, [url])                (operator = parent owner)
#   2. per creator (chai, tabla, pixel): <owner>.execute([StealthResolver.setConfig(<name>,
#      <owner's PermissionedResolver>, [url])], deadline, sig): sponsored, as in contracts/e2e; the
#      owner key (from creators.env) only signs the EIP-712 Execute digest, it never holds ETH.
# Then contracts/deployments/sepolia-lowki.json .gatewayUrl is updated, and the fork must be
# re-made from a block that has the new URL:  FRESH_FORK=1 scripts/demo-up.sh
#
# Keys are read from /Users/betterclever/.lowki/{sepolia-test.env,creators.env}; never printed.
set -euo pipefail
ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
LIVE="$ROOT/target/demo-live"
BIN="$LIVE/bin"
SECRETS_DIR="${LOWKI_SECRETS_DIR:-/Users/betterclever/.lowki}"
DEPLOY="$ROOT/contracts/deployments/sepolia-lowki.json"
ENSV2="$ROOT/contracts/deployments/sepolia-ensv2.json"
RPC="${SEPOLIA_RPC:-https://ethereum-sepolia-rpc.publicnode.com}"
mkdir -p "$BIN" "$LIVE/run"

URL="${1:-}"
if [ -z "$URL" ]; then
  if mac sh -lc "pgrep -f 'cloudflared tunnel --no-autoupdate --url http://localhost:8830'" >/dev/null 2>&1; then
    echo "The :8830 tunnel is running; its URL is on chain: $(jq -r .gatewayUrl "$DEPLOY")"
    echo "Pass a URL to re-point anyway: $0 https://<host>/v1/ens/gateway"
    exit 0
  fi
  LOG="$ROOT/target/live/tunnel-server.log"
  mkdir -p "$ROOT/target/live"
  echo "== starting a new :8830 quick tunnel on the Mac (log $LOG)"
  mac sh -lc "nohup cloudflared tunnel --no-autoupdate --url http://localhost:8830 > '$LOG' 2>&1 < /dev/null & echo \$!" >"$LIVE/run/gateway-tunnel.mac-pid"
  for _ in $(seq 1 60); do HOST="$(grep -o 'https://[a-z0-9-]*\.trycloudflare\.com' "$LOG" | head -1 || true)"; [ -n "$HOST" ] && break; sleep 0.5; done
  [ -n "${HOST:-}" ] || { tail -5 "$LOG"; echo "no tunnel URL"; exit 1; }
  URL="$HOST/v1/ens/gateway"
  echo "   new tunnel: $HOST (Mac pid $(cat "$LIVE/run/gateway-tunnel.mac-pid")); it is NOT stopped by demo-down.sh"
fi
case "$URL" in https://*/v1/ens/gateway) ;; *) echo "URL must look like https://<host>/v1/ens/gateway"; exit 1 ;; esac

ln -sfn "$ROOT/apps/lowki-tips/node_modules" "$BIN/node_modules"
cat >"$BIN/update-gateway-url.mjs" <<'EOF'
// Written by scripts/update-gateway-url.sh. Env: RPC, URL, DEPLOY, ENSV2, OPERATOR_KEY, PHRASE, DRY_RUN.
import { readFileSync } from 'node:fs';
import { createHash } from 'node:crypto';
import { HDKey } from '@scure/bip32';
import { createPublicClient, createWalletClient, encodeFunctionData, http, namehash, parseAbi, toHex } from 'viem';
import { privateKeyToAccount, sign } from 'viem/accounts';
import { sepolia } from 'viem/chains';

const E = process.env;
const d = JSON.parse(readFileSync(E.DEPLOY, 'utf8'));
const ens = JSON.parse(readFileSync(E.ENSV2, 'utf8')).ens;
const operator = privateKeyToAccount(E.OPERATOR_KEY);
const pub = createPublicClient({ chain: sepolia, transport: http(E.RPC) });
const wallet = createWalletClient({ account: operator, chain: sepolia, transport: http(E.RPC) });
const DRY = E.DRY_RUN === '1';
const dns = (name) => toHex(new Uint8Array([...name.split('.').flatMap((l) => [l.length, ...new TextEncoder().encode(l)]), 0]));
const srAbi = parseAbi(['function setConfig(bytes name, address recordsResolver, string[] urls)', 'function canSetConfig(bytes name, address account) view returns (bool)']);
const ownerAbi = parseAbi([
  'function nonce() view returns (uint256)',
  'function executeDigest((address target, uint256 value, bytes data)[] calls, uint256 n, uint256 deadline) view returns (bytes32)',
  'function execute((address target, uint256 value, bytes data)[] calls, uint256 deadline, bytes signature) payable',
]);
const factoryAbi = parseAbi(['function predictProxyAddress(address deployer, uint256 salt) view returns (address)']);

async function send(label, to, data) {
  const gas = await pub.estimateGas({ account: operator, to, data });
  if (DRY) return console.log(`   ${label}: simulated ok (gas ${gas})`);
  const hash = await wallet.sendTransaction({ to, data, gas: (gas * 13n) / 10n });
  const r = await pub.waitForTransactionReceipt({ hash });
  if (r.status !== 'success') throw new Error(`${label}: reverted ${hash}`);
  console.log(`   ${label}: ${hash}`);
}

console.log(`${DRY ? 'DRY RUN: ' : ''}gateway URL -> ${E.URL}`);
const parent = d.parentName;
await send(`${parent} (parent config)`, d.StealthResolver,
  encodeFunctionData({ abi: srAbi, functionName: 'setConfig', args: [dns(parent), d.ParentRecords, [E.URL]] }));

const now = Number((await pub.getBlock()).timestamp);
for (const label of ['chai', 'tabla', 'pixel']) {
  const name = `${label}.${parent}`;
  const seed = createHash('sha512').update(`${E.PHRASE}/${label}`).digest();
  const ownerKey = HDKey.fromMasterSeed(seed).derive("m/137'/0'/0'").privateKey;
  const owner = privateKeyToAccount(toHex(ownerKey)).address;
  const records = await pub.readContract({ address: ens.VerifiableFactory, abi: factoryAbi, functionName: 'predictProxyAddress', args: [owner, BigInt(namehash(name))] });
  const calls = [{ target: d.StealthResolver, value: 0n, data: encodeFunctionData({ abi: srAbi, functionName: 'setConfig', args: [dns(name), records, [E.URL]] }) }];
  const n = await pub.readContract({ address: owner, abi: ownerAbi, functionName: 'nonce' });
  const deadline = BigInt(now + 3600);
  const digest = await pub.readContract({ address: owner, abi: ownerAbi, functionName: 'executeDigest', args: [calls, n, deadline] });
  const sig = await sign({ hash: digest, privateKey: toHex(ownerKey), to: 'hex' });
  await send(`${name} (owner ${owner}, records ${records})`, owner, encodeFunctionData({ abi: ownerAbi, functionName: 'execute', args: [calls, deadline, sig] }));
}
EOF

set -a; . "$SECRETS_DIR/sepolia-test.env"; . "$SECRETS_DIR/creators.env"; set +a
RPC="$RPC" URL="$URL" DEPLOY="$DEPLOY" ENSV2="$ENSV2" OPERATOR_KEY="$LOWKI_SEPOLIA_KEY" PHRASE="$TIPS_CREATOR_PHRASE" DRY_RUN="${DRY_RUN:-0}" \
  node "$BIN/update-gateway-url.mjs"

if [ "${DRY_RUN:-0}" != 1 ]; then
  tmp="$(mktemp)"; jq --arg u "$URL" '.gatewayUrl = $u' "$DEPLOY" >"$tmp" && mv "$tmp" "$DEPLOY"
  echo "updated $DEPLOY (.gatewayUrl)"
  echo "next: FRESH_FORK=1 scripts/demo-up.sh   (re-forks Sepolia at a block with the new URL; restarts lowki-server)"
fi
