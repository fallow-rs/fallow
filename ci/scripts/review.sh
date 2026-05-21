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

# Initialize two sidecar markers so downstream jobs always see definitive
# values. GitLab CI lacks an equivalent of $GITHUB_OUTPUT for cross-job
# propagation; these greppable text files serve the same role when added to
# `artifacts: paths:`. `fallow-skip-reason.txt` is `pagination_failure` only
# when the inline-review POST is actually skipped (multi-discussion abort);
# `fallow-dedup-lookup-failed.txt` is `true` on any dedup-lookup failure
# (including the summary-only path where we post a potential duplicate).
#
# IMPORTANT: comment.sh runs BEFORE review.sh in the default template
# (ci/gitlab-ci.yml). If comment.sh hit its dedup-lookup failure path it
# already wrote `true` to fallow-dedup-lookup-failed.txt; reinitializing
# unconditionally here would clobber that value and hide the degraded
# state from downstream jobs. Only initialize each marker when the file
# does not already exist.
[ -f fallow-skip-reason.txt ] || printf 'none\n' > fallow-skip-reason.txt
[ -f fallow-dedup-lookup-failed.txt ] || printf 'false\n' > fallow-dedup-lookup-failed.txt

# Track mktemp files so an EXIT trap cleans them up on signal or early exit.
_FALLOW_TMPS=()
trap 'rm -f "${_FALLOW_TMPS[@]:-}"' EXIT

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
  # Accept both v1 (historical) and v2 (issue #528) schema markers so a
  # consumer running an older bundled template against a newer fallow binary
  # continues to render. Future-tolerant: any `fallow-review-envelope/v<N>`
  # passes, on the assumption that the back-compat fields (`body`,
  # `comments[].{body,position}`) remain in every future version.
  jq -e '
    (.meta.schema | test("^fallow-review-envelope/v[0-9]+$"))
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
    # Summary-only path: dedup-lookup failure means we cannot find an
    # existing body note. Posting a fresh one (potential duplicate) beats
    # a missing summary, which is silently broken from the MR author's
    # view. The WARNING + sidecar artifact still surface the degradation.
    _NOTE_LOOKUP_TMP=$(mktemp); _NOTE_LOOKUP_ERR=$(mktemp)
    _FALLOW_TMPS+=("$_NOTE_LOOKUP_TMP" "$_NOTE_LOOKUP_ERR")
    if curl_paginate --header "${AUTH_HEADER}" "${NOTES_URL}?per_page=100" \
         > "$_NOTE_LOOKUP_TMP" 2> "$_NOTE_LOOKUP_ERR"; then
      EXISTING_NOTE_ID=$(jq -r '.[] | select(.body | contains("<!-- fallow-review -->")) | .id' "$_NOTE_LOOKUP_TMP" \
        | head -1)
    else
      EXISTING_NOTE_ID=""
      _STDERR_HEAD=$(head -3 "$_NOTE_LOOKUP_ERR" | tr '\n' ' ')
      echo "WARNING: fallow: failed to look up existing MR summary note; posting a new one (may duplicate). stderr: ${_STDERR_HEAD} Re-run the job to retry. If persistent, verify GITLAB_TOKEN scopes (api, read_api)." >&2
      # Summary-only path: the post proceeds anyway, so do NOT flip
      # fallow-skip-reason.txt. Mark dedup-lookup-failed instead.
      printf 'true\n' > fallow-dedup-lookup-failed.txt
    fi
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

  # Multi-discussion dedup path: a failed lookup here means we cannot
  # enumerate existing fingerprints, so posting any new inline discussions
  # risks N duplicate threads. Abort the post step (skip reconcile_review
  # for the same root-cause reason) and surface the failure as both a
  # stderr warning and a sidecar artifact. 4xx is a configuration error
  # and warrants a loud CI failure (exit 1); 5xx / 429 / network blips
  # warrant exit 0 since a re-run may succeed.
  _DEDUP_TMP=$(mktemp); _DEDUP_ERR=$(mktemp)
  _FALLOW_TMPS+=("$_DEDUP_TMP" "$_DEDUP_ERR")
  if curl_paginate --header "${AUTH_HEADER}" "${DISCUSSIONS_URL}?per_page=100" \
       > "$_DEDUP_TMP" 2> "$_DEDUP_ERR"; then
    # Extract fingerprints from both v1 (`<!-- fallow-fingerprint: <fp> -->`)
    # and v2 (`<!-- fallow-fingerprint:v2: <fp> -->`) marker shapes so dedup
    # idempotency survives the issue #528 migration. v2 markers use the
    # `:v2:` namespace; the v1 substring would otherwise capture `v2:` as the
    # fingerprint instead of the actual hex string. Two sed expressions, sort
    # -u to dedupe in case a single note carries both markers (impossible by
    # construction today, defensive).
    EXISTING_FPS=$(jq -r '.[].notes[].body? // empty' "$_DEDUP_TMP" \
      | sed -n \
        -e 's/.*fallow-fingerprint:v2: \([^ ]*\) .*/\1/p' \
        -e 's/.*fallow-fingerprint: \([^ ]*\) .*/\1/p' \
      | sort -u \
      | jq -R -s 'split("\n") | map(select(length > 0))')
  else
    _STDERR_HEAD=$(head -3 "$_DEDUP_ERR" | tr '\n' ' ')
    echo "WARNING: fallow: failed to fetch existing MR discussions; skipping inline review to avoid duplicates. stderr: ${_STDERR_HEAD} Re-run the job to retry. If persistent, verify GITLAB_TOKEN scopes (api, read_api)." >&2
    printf 'pagination_failure\n' > fallow-skip-reason.txt
    printf 'true\n' > fallow-dedup-lookup-failed.txt
    # 4xx (auth, scope, permission) is a configuration error: a re-run
    # will not help, so escalate to exit 1 for loud CI failure. Exclude
    # 429 explicitly: it is the rate-limited variant and is transient
    # even though curl_retry has already exhausted its budget. 5xx, 429,
    # and network errors fall through to exit 0 (re-run may help).
    # Note: ci/gitlab-ci.yml currently calls this script as
    # `bash review.sh || echo "WARNING: ..."`, which swallows exit 1.
    # Operators who want strict CI gating on 4xx should remove the
    # `|| echo` from their gitlab-ci.yml, or gate on
    # `fallow-skip-reason.txt` and `fallow-dedup-lookup-failed.txt`
    # in a downstream job.
    if grep -qE 'HTTP 4[0-9][0-9]|error: 4[0-9][0-9]' "$_DEDUP_ERR" \
        && ! grep -qE 'HTTP 429|error: 429|rate.limit' "$_DEDUP_ERR"; then
      exit 1
    fi
    exit 0
  fi
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
