#!/usr/bin/env bash
set -euo pipefail

# Post inline MR discussions with rich markdown formatting and suggestion blocks
# Required env: GITLAB_TOKEN, CI_API_V4_URL, CI_PROJECT_ID,
#   CI_MERGE_REQUEST_IID, CI_COMMIT_SHA, CI_MERGE_REQUEST_DIFF_BASE_SHA,
#   FALLOW_COMMAND, FALLOW_ROOT, MAX_COMMENTS

MAX="${MAX_COMMENTS:-50}"
if ! [[ "$MAX" =~ ^[0-9]+$ ]]; then
  echo "WARNING: max-comments must be a positive integer, got: ${MAX_COMMENTS}. Using default: 50"
  MAX=50
fi

# Reject path traversal in root
if [[ "${FALLOW_ROOT:-}" =~ \.\. ]]; then
  echo "ERROR: root input contains path traversal sequence"
  exit 2
fi

# Auth header
if [ -z "${GITLAB_TOKEN:-}" ]; then
  echo "WARNING: GITLAB_TOKEN is required to create or resolve MR discussions; CI_JOB_TOKEN is read-only for MR notes in the official GitLab API. Skipping inline MR review."
  exit 0
fi
: "${CI_API_V4_URL:?CI_API_V4_URL is required}"
: "${CI_PROJECT_ID:?CI_PROJECT_ID is required}"
: "${CI_MERGE_REQUEST_IID:?CI_MERGE_REQUEST_IID is required}"
AUTH_HEADER="PRIVATE-TOKEN: ${GITLAB_TOKEN}"

NOTES_URL="${CI_API_V4_URL}/projects/${CI_PROJECT_ID}/merge_requests/${CI_MERGE_REQUEST_IID}/notes"
DISCUSSIONS_URL="${CI_API_V4_URL}/projects/${CI_PROJECT_ID}/merge_requests/${CI_MERGE_REQUEST_IID}/discussions"

curl_retry() {
  local attempts="${FALLOW_API_RETRIES:-3}"
  local delay="${FALLOW_API_RETRY_DELAY:-2}"
  local attempt=1
  local err out
  err=$(mktemp)
  out=$(mktemp)
  while true; do
    if curl -sf "$@" >"$out" 2>"$err"; then
      cat "$out"
      rm -f "$err" "$out"
      return 0
    fi
    # Match the Rust `with_rate_limit_retry` decision: 429 + 502/503/504 are
    # transient and worth retrying; persistent 5xx (500, 501, 505) and all
    # other 4xx surface immediately. curl -sf emits stderr like
    # `curl: (22) The requested URL returned error: 502 Bad Gateway`, so we
    # match either the explicit code or the rate-limit / Retry-After hints.
    if [ "$attempt" -ge "$attempts" ] \
        || ! grep -Eqi 'error: (429|502|503|504)|rate limit|Retry-After' "$err"; then
      cat "$err" >&2
      rm -f "$err" "$out"
      return 1
    fi
    echo "WARNING: GitLab API rate limit response; retrying (${attempt}/${attempts})" >&2
    sleep "$delay"
    attempt=$((attempt + 1))
  done
}

# Walk the GitLab REST API's Link-header pagination, concatenating every page
# of a JSON array into a single combined array on stdout. Last positional arg
# is the initial URL; preceding args are passed to curl_retry verbatim. Without
# this, a >100-comment MR can silently lose existing fingerprints outside the
# first page and re-post duplicate inline review notes on every run.
curl_paginate() {
  local args=("$@")
  local last=$(( ${#args[@]} - 1 ))
  local url="${args[$last]}"
  unset 'args[last]'
  local headers body
  headers=$(mktemp)
  body=$(mktemp)
  local combined='[]'
  while [ -n "$url" ]; do
    if ! curl_retry -D "$headers" "${args[@]}" "$url" > "$body"; then
      rm -f "$headers" "$body"
      return 1
    fi
    # Defensively skip non-array pages (e.g. an error envelope) so the
    # caller degrades to "no existing notes seen" instead of crashing on
    # `array + object` jq errors.
    combined=$(jq -s 'map(arrays) | add // []' <(printf '%s' "$combined") "$body")
    url=$(grep -i '^link:' "$headers" \
      | tr ',' '\n' \
      | sed -n 's/.*<\([^>]*\)>.*rel="next".*/\1/p' \
      | head -1)
  done
  rm -f "$headers" "$body"
  printf '%s' "$combined"
}

load_gitlab_diff_refs() {
  if [ -n "${FALLOW_GITLAB_BASE_SHA:-}" ] && [ -n "${FALLOW_GITLAB_HEAD_SHA:-}" ]; then
    return 0
  fi
  local diff_refs=""
  diff_refs=$(curl_retry \
    --header "${AUTH_HEADER}" \
    "${CI_API_V4_URL}/projects/${CI_PROJECT_ID}/merge_requests/${CI_MERGE_REQUEST_IID}" \
    | jq -r '.diff_refs // empty') || {
      echo "WARNING: Failed to fetch MR diff refs; falling back to CI sha variables"
      diff_refs=""
    }
  if [ -n "$diff_refs" ] && echo "$diff_refs" | jq -e '.base_sha and .head_sha' > /dev/null 2>&1; then
    export FALLOW_GITLAB_BASE_SHA
    export FALLOW_GITLAB_START_SHA
    export FALLOW_GITLAB_HEAD_SHA
    FALLOW_GITLAB_BASE_SHA=$(echo "$diff_refs" | jq -r '.base_sha')
    FALLOW_GITLAB_START_SHA=$(echo "$diff_refs" | jq -r '.start_sha // .base_sha')
    FALLOW_GITLAB_HEAD_SHA=$(echo "$diff_refs" | jq -r '.head_sha')
  else
    export FALLOW_GITLAB_BASE_SHA="${FALLOW_GITLAB_BASE_SHA:-${CI_MERGE_REQUEST_DIFF_BASE_SHA:-}}"
    export FALLOW_GITLAB_START_SHA="${FALLOW_GITLAB_START_SHA:-${FALLOW_GITLAB_BASE_SHA:-}}"
    export FALLOW_GITLAB_HEAD_SHA="${FALLOW_GITLAB_HEAD_SHA:-${CI_COMMIT_SHA:-}}"
  fi
}

render_with_fallow() {
  local format=$1
  local output=$2
  [ -f fallow-analysis-args.sh ] || return 1
  # shellcheck disable=SC1091
  source fallow-analysis-args.sh
  local args=("${FALLOW_ANALYSIS_ARGS[@]}")
  local replaced=false
  for i in "${!args[@]}"; do
    if [ "${args[$i]}" = "--format" ] && [ $((i + 1)) -lt "${#args[@]}" ]; then
      args[$((i + 1))]="$format"
      replaced=true
      break
    fi
  done
  if [ "$replaced" != "true" ]; then
    args+=(--format "$format")
  fi
  if [ -z "${FALLOW_DIFF_FILE:-}" ] && [ -n "${CI_MERGE_REQUEST_DIFF_BASE_SHA:-}" ]; then
    if git diff "${CI_MERGE_REQUEST_DIFF_BASE_SHA}..HEAD" > fallow-mr.diff 2>fallow-mr-diff-stderr.log; then
      export FALLOW_DIFF_FILE="$PWD/fallow-mr.diff"
    else
      echo "WARNING: Failed to fetch MR diff; diff filter disabled, reporting all findings"
      rm -f fallow-mr.diff
    fi
  fi
  load_gitlab_diff_refs
  export FALLOW_DIFF_FILTER="${FALLOW_DIFF_FILTER:-added}"
  FALLOW_MAX_COMMENTS="$MAX" fallow "${args[@]}" > "$output" 2> fallow-review-stderr.log || true
  # Surface fallow's structured-error envelope before the schema check so the
  # CLI message lands in the GitLab job log rather than a generic warning.
  if jq -e '.error == true' "$output" > /dev/null 2>&1; then
    echo "WARNING: fallow render failed: $(jq -r '.message // "unknown error"' "$output")"
    return 1
  fi
  jq -e '
    .meta.schema == "fallow-review-envelope/v1"
    and .meta.provider == "gitlab"
    and (.body | type == "string")
    and (.body | contains("<!-- fallow-review -->"))
    and (.comments | type == "array")
  ' "$output" > /dev/null 2>&1
}

if render_with_fallow review-gitlab fallow-review.json; then
  reconcile_review() {
    fallow ci reconcile-review \
      --provider gitlab \
      --mr "$CI_MERGE_REQUEST_IID" \
      --project-id "$CI_PROJECT_ID" \
      --api-url "$CI_API_V4_URL" \
      --envelope fallow-review.json > fallow-review-reconcile.json 2> fallow-review-reconcile-stderr.log \
      || echo "WARNING: Failed to reconcile resolved review discussions"
  }

  TOTAL=$(jq '.comments | length' fallow-review.json)
  if [ "$TOTAL" -eq 0 ]; then
    BODY=$(jq -r '.body' fallow-review.json)
    EXISTING_NOTE_ID=$(curl_paginate \
      --header "${AUTH_HEADER}" \
      "${NOTES_URL}?per_page=100" \
      | jq -r '.[] | select(.body | contains("<!-- fallow-review -->")) | .id' \
      | head -1) || true
    if [ -n "$EXISTING_NOTE_ID" ]; then
      curl_retry \
        --header "${AUTH_HEADER}" \
        --header "Content-Type: application/json" \
        --request PUT \
        --data "$(jq -n --arg body "$BODY" '{body: $body}')" \
        "${NOTES_URL}/${EXISTING_NOTE_ID}" > /dev/null 2>&1 \
        && echo "Updated review body" \
        || echo "WARNING: Failed to update review body"
    else
      curl_retry \
        --header "${AUTH_HEADER}" \
        --header "Content-Type: application/json" \
        --request POST \
        --data "$(jq -n --arg body "$BODY" '{body: $body}')" \
        "${NOTES_URL}" > /dev/null 2>&1 \
        && echo "Posted review body" \
        || echo "WARNING: Failed to post review body"
    fi
    reconcile_review
    exit 0
  fi

  EXISTING_FPS=$(curl_paginate --header "${AUTH_HEADER}" "${DISCUSSIONS_URL}?per_page=100" 2>/dev/null \
    | jq -r '.[].notes[].body? // empty' \
    | sed -n 's/.*fallow-fingerprint: \([^ ]*\) .*/\1/p' \
    | jq -R -s 'split("\n") | map(select(length > 0))' || echo '[]')
  jq --argjson existing "${EXISTING_FPS:-[]}" '
    .comments |= map(select((.fingerprint as $fp | $existing | index($fp)) | not))
  ' fallow-review.json > fallow-review-new.json
  NEW_TOTAL=$(jq '.comments | length' fallow-review-new.json)
  if [ "$NEW_TOTAL" -eq 0 ]; then
    reconcile_review
    echo "No new review comments to post"
    exit 0
  fi

  BASE_SHA="${FALLOW_GITLAB_BASE_SHA:-}"
  START_SHA="${FALLOW_GITLAB_START_SHA:-$BASE_SHA}"
  HEAD_SHA="${FALLOW_GITLAB_HEAD_SHA:-}"

  POSTED=0
  SKIPPED=0
  while IFS= read -r comment; do
    BODY_VAL=$(echo "$comment" | jq -r '.body')
    PATH_VAL=$(echo "$comment" | jq -r '.position.new_path')
    LINE_VAL=$(echo "$comment" | jq -r '.position.new_line')
    if [ -n "$BASE_SHA" ] && [ -n "$HEAD_SHA" ]; then
      PAYLOAD=$(echo "$comment" | jq --arg body "$BODY_VAL" '{body: $body, position: .position}')
      curl_retry --header "${AUTH_HEADER}" --header "Content-Type: application/json" \
        --request POST --data "$PAYLOAD" "${DISCUSSIONS_URL}" > /dev/null 2>&1 \
        && POSTED=$((POSTED + 1)) || SKIPPED=$((SKIPPED + 1))
    else
      FALLBACK_BODY=$(printf "Warning: **%s:%s**\n\n%s" "$PATH_VAL" "$LINE_VAL" "$BODY_VAL")
      curl_retry --header "${AUTH_HEADER}" --header "Content-Type: application/json" \
        --request POST --data "$(jq -n --arg body "$FALLBACK_BODY" '{body: $body}')" \
        "${NOTES_URL}" > /dev/null 2>&1 \
        && POSTED=$((POSTED + 1)) || SKIPPED=$((SKIPPED + 1))
    fi
  done < <(jq -c '.comments[]' fallow-review-new.json)
  echo "Posted ${POSTED} inline comments, skipped ${SKIPPED}"
  reconcile_review
  exit 0
fi

echo "WARNING: Failed to render typed review envelope"
exit 0
