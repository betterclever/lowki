#!/usr/bin/env bash
# Option matrix: which Chrome/CDP knobs actually matter. Run ./run.sh first (builds web/pkg).
# Prints one line per variant: name | ok | error | failed checks | prf.enabled
set -uo pipefail
HERE="$(cd "$(dirname "$0")" && pwd)"
v() { # name, then env assignments
  local name="$1"; shift
  ( env "$@" node "$HERE/drive.mjs" 2>&1 | node -e '
    let s="";process.stdin.on("data",d=>s+=d).on("end",()=>{let r;try{r=JSON.parse(s)}catch{console.log(process.argv[1]+" | unparsable | "+s.slice(0,200).replace(/\n/g," "));return}
    const failed=Object.entries(r.checks??{}).filter(([,v])=>!v).map(([k])=>k);
    console.log([process.argv[1], r.ok?"PASS":"FAIL", r.error??"-", failed.join(",")||"-", JSON.stringify(r.info?.createExtensions?.prf??null)].join(" | "))})' "$name" ) &
}
v baseline                       X=1
v no_prf_at_create               PRF_QUERY=createPrf=0
v no_prf_at_create_usb_ctap20    PRF_QUERY=createPrf=0 PRF_AUTH='{"transport":"usb","ctap2Version":"ctap2_0"}'
v eval_at_create                 PRF_QUERY=evalAtCreate=1
v eval_at_create_ctap20          PRF_QUERY=evalAtCreate=1 PRF_AUTH='{"ctap2Version":"ctap2_0"}'
v ctap2_0                        PRF_AUTH='{"ctap2Version":"ctap2_0"}'
v hasPrf_false                   PRF_AUTH='{"hasPrf":false}'
v transport_usb                  PRF_AUTH='{"transport":"usb"}'
v transport_hybrid               PRF_AUTH='{"transport":"hybrid"}'
v u2f                            PRF_AUTH='{"protocol":"u2f","ctap2Version":null}'
v hasResidentKey_false           PRF_AUTH='{"hasResidentKey":false}'
v hasUserVerification_false      PRF_AUTH='{"hasUserVerification":false}'
v isUserVerified_false           PRF_AUTH='{"isUserVerified":false}'
v no_presence_simulation         PRF_AUTH='{"automaticPresenceSimulation":false}'
v no_virtual_authenticator       PRF_NO_VA=1
v no_user_gesture                PRF_NO_GESTURE=1
v headless_plain                 PRF_HEADLESS=--headless
v host_127_rpid_default          PRF_HOST=127.0.0.1
v host_127_rpid_omitted          PRF_HOST=127.0.0.1 PRF_QUERY=rpId=
v localhost_rpid_omitted         PRF_QUERY=rpId=
v localhost_rpid_example_com     PRF_QUERY=rpId=example.com
wait
