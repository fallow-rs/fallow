#!/usr/bin/env bash
set -euo pipefail

# Post review comments with rich markdown formatting
# Required env: GH_TOKEN, PR_NUMBER, GH_REPO, FALLOW_COMMAND, FALLOW_ROOT,
#   MAX_COMMENTS
# Optional env: CHANGED_SINCE (for scoping results to changed files)

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
  if [ -z "${FALLOW_DIFF_FILE:-}" ] && [ -n "${GH_REPO:-}" ] && [ -n "${PR_NUMBER:-}" ]; then
    if gh pr diff "$PR_NUMBER" --repo "$GH_REPO" > fallow-pr.diff 2>fallow-pr-diff-stderr.log; then
      export FALLOW_DIFF_FILE="$PWD/fallow-pr.diff"
    else
      echo "::warning::Failed to fetch PR diff; diff filter disabled, reporting all findings"
      rm -f fallow-pr.diff
    fi
  fi
  export FALLOW_DIFF_FILTER="${FALLOW_DIFF_FILTER:-added}"
  FALLOW_MAX_COMMENTS="$MAX" fallow "${args[@]}" > "$output" 2> fallow-review-stderr.log || true
  # Surface fallow's structured-error envelope before the schema check so the
  # CLI message lands in the workflow log rather than a generic warning.
  if jq -e '.error == true' "$output" > /dev/null 2>&1; then
    echo "::warning::fallow render failed: $(jq -r '.message // "unknown error"' "$output")"
    return 1
  fi
  jq -e '
    .meta.schema == "fallow-review-envelope/v1"
    and .meta.provider == "github"
    and (.body | type == "string")
    and (.body | contains("<!-- fallow-review -->"))
    and (.comments | type == "array")
  ' "$output" > /dev/null 2>&1
}

if render_with_fallow review-github fallow-review.json; then
  reconcile_review() {
    fallow ci reconcile-review \
      --provider github \
      --pr "$PR_NUMBER" \
      --repo "$GH_REPO" \
      --envelope fallow-review.json > fallow-review-reconcile.json 2> fallow-review-reconcile-stderr.log \
      || echo "::warning::Failed to reconcile resolved review threads"
  }

  TOTAL=$(jq '.comments | length' fallow-review.json)
  if [ "$TOTAL" -eq 0 ]; then
    BODY=$(jq -r '.body' fallow-review.json)
    REVIEW_COMMENT_ID=$(gh_api_retry \
      --paginate \
      "repos/${GH_REPO}/issues/${PR_NUMBER}/comments?per_page=100" \
      --jq '.[] | select(.body | contains("<!-- fallow-review -->")) | .id' \
      2>/dev/null | head -1 || true)
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

  EXISTING_FPS=$(gh_api_retry --paginate "repos/${GH_REPO}/pulls/${PR_NUMBER}/comments?per_page=100" --jq '.[].body' 2>/dev/null \
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

  jq '{event, body, comments: [.comments[] | {path, line, side, body}]}' fallow-review-new.json > fallow-review-payload.json
  gh_api_retry "repos/${GH_REPO}/pulls/${PR_NUMBER}/reviews" \
    --method POST \
    --input fallow-review-payload.json > /dev/null 2>&1 \
    && echo "Posted review with ${NEW_TOTAL} inline comments" \
    || echo "::warning::Failed to post review comments"
  reconcile_review
  exit 0
fi

echo "::warning::Failed to render typed review envelope"
exit 0
