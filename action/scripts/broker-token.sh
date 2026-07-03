#!/usr/bin/env bash
# Mint a Fallow-branded GitHub App installation token from the Fallow token
# broker so PR comments and reviews are authored by the Fallow app instead of
# github-actions. Fails safe: on any problem it emits branded=false and the
# action posts with the default GITHUB_TOKEN. Never exits non-zero, so a broker
# outage or a workflow without id-token permission never breaks the check.
set -uo pipefail

emit_fallback() {
  echo "fallow: posting unbranded via GITHUB_TOKEN ($1)" >&2
  echo "branded=false" >>"$GITHUB_OUTPUT"
  exit 0
}

[ "${BRANDED_TOKEN:-true}" = "false" ] && emit_fallback "branded token disabled"
[ -n "${ACTIONS_ID_TOKEN_REQUEST_URL:-}" ] || emit_fallback "no id-token permission (add 'permissions: id-token: write')"
[ -n "${ACTIONS_ID_TOKEN_REQUEST_TOKEN:-}" ] || emit_fallback "no id-token permission"
command -v jq >/dev/null 2>&1 || emit_fallback "jq unavailable"

broker="${BROKER_URL:-https://api.fallow.cloud}"
audience="fallow-ci"

oidc_json=$(curl -sf --max-time 10 \
  -H "Authorization: Bearer ${ACTIONS_ID_TOKEN_REQUEST_TOKEN}" \
  "${ACTIONS_ID_TOKEN_REQUEST_URL}&audience=${audience}") || emit_fallback "OIDC request failed"
oidc=$(printf '%s' "$oidc_json" | jq -r '.value // empty')
[ -n "$oidc" ] || emit_fallback "empty OIDC token"
echo "::add-mask::${oidc}"

# POST body via stdin so the OIDC token never lands in the process argv.
resp=$(jq -nc --arg t "$oidc" '{token: $t}' |
  curl -sf --max-time 10 -X POST "${broker%/}/v1/ci/github-token" \
    -H 'content-type: application/json' --data @-) || emit_fallback "broker unavailable or declined"
token=$(printf '%s' "$resp" | jq -r '.data.token // empty')
[ -n "$token" ] || emit_fallback "broker returned no token"
echo "::add-mask::${token}"

{
  echo "token=${token}"
  echo "branded=true"
} >>"$GITHUB_OUTPUT"
echo "fallow: posting as the Fallow app (branded)" >&2
