#!/usr/bin/env bash
set -euo pipefail

# Post review comments with rich markdown formatting
# Required env: GH_TOKEN, PR_NUMBER, GH_REPO, FALLOW_COMMAND, FALLOW_ROOT,
#   MAX_COMMENTS
# Optional env: CHANGED_SINCE, FALLOW_ANALYSIS_ARGS_FILE, FALLOW_ARTIFACTS_DIR

: "${GH_TOKEN:?GH_TOKEN is required}"
: "${PR_NUMBER:?PR_NUMBER is required}"
: "${GH_REPO:?GH_REPO is required}"

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
POST_FILE=$(artifact_path fallow-review-post.json)
POST_STDERR_FILE=$(artifact_path fallow-review-post-stderr.log)

if render_with_fallow review-github "$REVIEW_FILE"; then
  if fallow ci post-review \
      --provider github \
      --pr "$PR_NUMBER" \
      --repo "$GH_REPO" \
      --envelope "$REVIEW_FILE" > "$POST_FILE" 2> "$POST_STDERR_FILE"; then
    if jq -e '(.apply_errors // []) | length > 0 or (.post_errors // []) | length > 0' "$POST_FILE" > /dev/null 2>&1; then
      HINT=$(jq -r '.apply_hint // "refresh provider state and rerun the job"' "$POST_FILE")
      echo "::warning::fallow post-review incomplete: $HINT"
    fi
    ACTION=$(jq -r '.action // "unknown"' "$POST_FILE")
    POSTED=$(jq -r '.comments_posted // 0' "$POST_FILE")
    echo "Review action: ${ACTION} (${POSTED} inline comments posted)"
  else
    echo "::warning::Failed to post review comments"
  fi
  exit 0
fi

echo "::warning::Failed to render typed review envelope"
exit 0
