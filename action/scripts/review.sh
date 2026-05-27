#!/usr/bin/env bash
set -euo pipefail

# Post review comments with rich markdown formatting
# Required env: GH_TOKEN, PR_NUMBER, GH_REPO, FALLOW_COMMAND, FALLOW_ROOT,
#   MAX_COMMENTS
# Optional env: CHANGED_SINCE, FALLOW_ANALYSIS_ARGS_FILE, FALLOW_ARTIFACTS_DIR

: "${GH_TOKEN:?GH_TOKEN is required}"
: "${PR_NUMBER:?PR_NUMBER is required}"
: "${GH_REPO:?GH_REPO is required}"

gh_api_retry() {
  local attempts="${FALLOW_API_RETRIES:-3}"
  local delay="${FALLOW_API_RETRY_DELAY:-2}"
  local attempt=1
  local err
  local out
  err=$(mktemp)
  out=$(mktemp)
  while true; do
    if gh api "$@" >"$out" 2>"$err"; then
      cat "$out"
      rm -f "$err" "$out"
      return 0
    fi
    # Match the Rust `with_rate_limit_retry` decision: 429 + 502/503/504 are
    # transient and worth retrying; persistent 5xx (500, 501, 505) and all
    # other 4xx surface immediately so a real bug doesn't burn the budget.
    if [ "$attempt" -ge "$attempts" ] \
        || ! grep -Eqi 'HTTP (429|502|503|504)|rate limit|secondary rate limit|Retry-After' "$err"; then
      cat "$err" >&2
      rm -f "$err" "$out"
      return 1
    fi
    echo "::warning::GitHub API rate limit response; retrying (${attempt}/${attempts})" >&2
    sleep "$delay"
    attempt=$((attempt + 1))
  done
}

MAX="${MAX_COMMENTS:-50}"
if ! [[ "$MAX" =~ ^[0-9]+$ ]]; then
  echo "::warning::max-comments must be a positive integer, got: ${MAX_COMMENTS}. Using default: 50"
  MAX=50
fi

# Reject path traversal in root
if [[ "${FALLOW_ROOT:-}" =~ \.\. ]]; then
  echo "::error::root input contains path traversal sequence"
  exit 2
fi

# Initialize two markers so downstream gates always see definitive values.
# `post_skipped_reason` is only set to `pagination_failure` when we actually
# skip POSTing (multi-comment dedup abort). `dedup_lookup_failed` is set to
# `true` on any dedup-lookup failure, including the summary-only path where
# we proceed and may post a duplicate.
if [ -n "${GITHUB_OUTPUT:-}" ]; then
  echo "post_skipped_reason=none" >> "$GITHUB_OUTPUT"
  echo "dedup_lookup_failed=false" >> "$GITHUB_OUTPUT"
fi

# Track every mktemp file so an EXIT trap cleans them up on signal or early
# exit. Avoids leaks when an abort path skips inline `rm -f`.
_FALLOW_TMPS=()
trap 'rm -f "${_FALLOW_TMPS[@]:-}"' EXIT

artifact_path() {
  local filename=$1
  local dir="${FALLOW_ARTIFACTS_DIR:-.}"
  if [ "$dir" = "." ]; then
    printf '%s\n' "$filename"
  else
    mkdir -p "$dir"
    printf '%s/%s\n' "$dir" "$filename"
  fi
}

render_with_fallow() {
  local format=$1
  local output=$2
  local analysis_args_file="${FALLOW_ANALYSIS_ARGS_FILE:-fallow-analysis-args.sh}"
  [ -f "$analysis_args_file" ] || return 1
  # shellcheck disable=SC1091
  source "$analysis_args_file"
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
  if [ -z "${FALLOW_DIFF_FILE:-}" ] && [ -n "${GH_REPO:-}" ] && [ -n "${PR_NUMBER:-}" ]; then
    diff_file=$(artifact_path fallow-pr.diff)
    diff_stderr_file=$(artifact_path fallow-pr-diff-stderr.log)
    if gh pr diff "$PR_NUMBER" --repo "$GH_REPO" > "$diff_file" 2>"$diff_stderr_file"; then
      export FALLOW_DIFF_FILE="$PWD/$diff_file"
    else
      echo "::warning::Failed to fetch PR diff; diff filter disabled, reporting all findings"
      rm -f "$diff_file"
    fi
  fi
  export FALLOW_DIFF_FILTER="${FALLOW_DIFF_FILTER:-added}"
  FALLOW_MAX_COMMENTS="$MAX" fallow "${args[@]}" > "$output" 2> "$(artifact_path fallow-review-stderr.log)" || true
  # Surface fallow's structured-error envelope before the schema check so the
  # CLI message lands in the workflow log rather than a generic warning.
  if jq -e '.error == true' "$output" > /dev/null 2>&1; then
    echo "::warning::fallow render failed: $(jq -r '.message // "unknown error"' "$output")"
    return 1
  fi
  # Accept both v1 (historical) and v2 (issue #528) schema markers so a
  # consumer running an older bundled action against a newer fallow binary
  # continues to render. Future-tolerant: any `fallow-review-envelope/v<N>`
  # passes, on the assumption that the back-compat fields (`body`,
  # `comments[].{path,line,side,body}`) remain in every future version.
  jq -e '
    (.meta.schema | test("^fallow-review-envelope/v[0-9]+$"))
    and .meta.provider == "github"
    and (.body | type == "string")
    and (.body | contains("<!-- fallow-review -->"))
    and (.comments | type == "array")
  ' "$output" > /dev/null 2>&1
}

REVIEW_FILE=$(artifact_path fallow-review.json)
RECONCILE_FILE=$(artifact_path fallow-review-reconcile.json)
RECONCILE_STDERR_FILE=$(artifact_path fallow-review-reconcile-stderr.log)
NEW_REVIEW_FILE=$(artifact_path fallow-review-new.json)
PAYLOAD_FILE=$(artifact_path fallow-review-payload.json)

if render_with_fallow review-github "$REVIEW_FILE"; then
  reconcile_review() {
    if fallow ci reconcile-review \
      --provider github \
      --pr "$PR_NUMBER" \
      --repo "$GH_REPO" \
      --envelope "$REVIEW_FILE" > "$RECONCILE_FILE" 2> "$RECONCILE_STDERR_FILE"; then
      if jq -e '(.apply_errors // []) | length > 0' "$RECONCILE_FILE" > /dev/null 2>&1; then
        HINT=$(jq -r '.apply_hint // "refresh provider state and rerun the job"' "$RECONCILE_FILE")
        echo "::warning::fallow reconcile-review apply incomplete: $HINT"
      fi
    else
      echo "::warning::Failed to reconcile resolved review threads"
    fi
  }

  TOTAL=$(jq '.comments | length' "$REVIEW_FILE")
  if [ "$TOTAL" -eq 0 ]; then
    BODY=$(jq -r '.body' "$REVIEW_FILE")
    # Summary-only path: a dedup-lookup failure here means we cannot tell
    # whether a previous summary comment exists. Posting anyway (creating a
    # duplicate) is less bad than not posting at all, since a missing
    # summary is silently broken from the PR author's view while a duplicate
    # is collapsible. The warning + post_skipped_reason marker still fire.
    _REVIEW_LOOKUP_TMP=$(mktemp); _REVIEW_LOOKUP_ERR=$(mktemp)
    _FALLOW_TMPS+=("$_REVIEW_LOOKUP_TMP" "$_REVIEW_LOOKUP_ERR")
    if gh_api_retry --paginate \
         "repos/${GH_REPO}/issues/${PR_NUMBER}/comments?per_page=100" \
         --jq '.[] | select(.body | contains("<!-- fallow-review -->")) | .id' \
         > "$_REVIEW_LOOKUP_TMP" 2> "$_REVIEW_LOOKUP_ERR"; then
      REVIEW_COMMENT_ID=$(head -1 "$_REVIEW_LOOKUP_TMP")
    else
      REVIEW_COMMENT_ID=""
      _STDERR_HEAD=$(head -3 "$_REVIEW_LOOKUP_ERR" | tr '\n' ' ')
      echo "::warning::fallow: failed to look up existing summary comment; posting a new one (may duplicate). stderr: ${_STDERR_HEAD} Re-run the job to retry. If persistent, check 'gh auth status' and repo permissions." >&2
      # Summary-only path: the post proceeds anyway, so do NOT flip
      # post_skipped_reason. Use dedup_lookup_failed so operators can still
      # detect the degraded state without misreading it as a skipped post.
      [ -n "${GITHUB_OUTPUT:-}" ] && echo "dedup_lookup_failed=true" >> "$GITHUB_OUTPUT"
    fi
    if [ -n "$REVIEW_COMMENT_ID" ]; then
      gh_api_retry "repos/${GH_REPO}/issues/comments/${REVIEW_COMMENT_ID}" \
        --method PATCH \
        --field body="$BODY" > /dev/null 2>&1 \
        && echo "Updated summary comment (no inline comments)" \
        || echo "::warning::Failed to update summary comment"
    else
      gh_api_retry "repos/${GH_REPO}/issues/${PR_NUMBER}/comments" \
        --method POST \
        --field body="$BODY" > /dev/null 2>&1 \
        && echo "Posted summary comment (no inline comments)" \
        || echo "::warning::Failed to post summary comment"
    fi
    reconcile_review
    exit 0
  fi

  # Multi-comment dedup path: a failed lookup here means we cannot
  # enumerate existing fingerprints, so posting any new inline comments
  # risks N duplicate threads. Abort the post step (skip reconcile_review
  # for the same root-cause reason) and surface the failure as both a
  # stderr warning and a structured output marker. 4xx is a configuration
  # error and warrants a loud CI failure; 5xx / 429 / network blips warrant
  # exit 0 since a re-run may succeed.
  _DEDUP_TMP=$(mktemp); _DEDUP_ERR=$(mktemp)
  _FALLOW_TMPS+=("$_DEDUP_TMP" "$_DEDUP_ERR")
  if gh_api_retry --paginate \
       "repos/${GH_REPO}/pulls/${PR_NUMBER}/comments?per_page=100" \
       --jq '.[].body' \
       > "$_DEDUP_TMP" 2> "$_DEDUP_ERR"; then
    # Extract fingerprints from both v1 (`<!-- fallow-fingerprint: <fp> -->`)
    # and v2 (`<!-- fallow-fingerprint:v2: <fp> -->`) marker shapes so dedup
    # idempotency survives the issue #528 migration. v2 markers use the
    # `:v2:` namespace; the v1 substring would otherwise capture `v2:` as the
    # fingerprint instead of the actual hex string. Two sed expressions, sort
    # -u to dedupe in case a single comment carries both markers (impossible
    # by construction today, defensive).
    EXISTING_FPS=$(sed -n \
      -e 's/.*fallow-fingerprint:v2: \([^ ]*\) .*/\1/p' \
      -e 's/.*fallow-fingerprint: \([^ ]*\) .*/\1/p' \
      "$_DEDUP_TMP" \
      | sort -u \
      | jq -R -s 'split("\n") | map(select(length > 0))')
  else
    _STDERR_HEAD=$(head -3 "$_DEDUP_ERR" | tr '\n' ' ')
    echo "::warning::fallow: failed to fetch existing PR review comments; skipping inline review to avoid duplicates. stderr: ${_STDERR_HEAD} Re-run the job to retry. If persistent, check 'gh auth status' and repo permissions." >&2
    if [ -n "${GITHUB_OUTPUT:-}" ]; then
      echo "post_skipped_reason=pagination_failure" >> "$GITHUB_OUTPUT"
      echo "dedup_lookup_failed=true" >> "$GITHUB_OUTPUT"
    fi
    # 4xx (auth, scope, permission) is a configuration error: a re-run
    # will not help, so escalate to exit 1 for loud CI failure. Exclude
    # 429 explicitly: it is the rate-limited variant and is transient
    # even though gh_api_retry has already exhausted its budget. 5xx,
    # 429, and network errors fall through to exit 0 (re-run may help).
    if grep -qE 'HTTP 4[0-9][0-9]|error: 4[0-9][0-9]' "$_DEDUP_ERR" \
        && ! grep -qE 'HTTP 429|error: 429|rate.limit' "$_DEDUP_ERR"; then
      exit 1
    fi
    exit 0
  fi
  jq --argjson existing "${EXISTING_FPS:-[]}" '
    .comments |= map(select((.fingerprint as $fp | $existing | index($fp)) | not))
  ' "$REVIEW_FILE" > "$NEW_REVIEW_FILE"
  NEW_TOTAL=$(jq '.comments | length' "$NEW_REVIEW_FILE")
  if [ "$NEW_TOTAL" -eq 0 ]; then
    reconcile_review
    echo "No new review comments to post"
    exit 0
  fi

  jq '{event, body, comments: [.comments[] | {path, line, side, body}]}' "$NEW_REVIEW_FILE" > "$PAYLOAD_FILE"
  gh_api_retry "repos/${GH_REPO}/pulls/${PR_NUMBER}/reviews" \
    --method POST \
    --input "$PAYLOAD_FILE" > /dev/null 2>&1 \
    && echo "Posted review with ${NEW_TOTAL} inline comments" \
    || echo "::warning::Failed to post review comments"
  reconcile_review
  exit 0
fi

echo "::warning::Failed to render typed review envelope"
exit 0
