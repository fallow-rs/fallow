#!/usr/bin/env bash
set -euo pipefail

# shellcheck source=artifacts.sh
source "$(dirname -- "${BASH_SOURCE[0]}")/artifacts.sh"
prepare_artifacts "${FALLOW_ARTIFACTS_DIR:-.}" \
  fallow-review.json \
  fallow-review-stderr.log fallow-review-post.json fallow-review-post-stderr.log || exit 2

# Post review comments with rich markdown formatting
# Env contract: see this script's step env block in action.yml, the
#   authoritative list of consumed variables. Hard requirements are asserted
#   below.

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

# shellcheck source=action/scripts/render-common.sh
. "$(dirname -- "${BASH_SOURCE[0]}")/render-common.sh"

# Renders the review envelope and checks its shape.
render_review() {
  local output=$1
  FALLOW_MAX_COMMENTS="$MAX" \
    render_with_fallow review-github "$output" "$(artifact_path fallow-review-stderr.log)" "reviews" \
    || return 1
  # Accept versioned schema markers so a consumer running an older bundled
  # action against a newer fallow binary continues to render. Future-tolerant:
  # any `fallow-review-envelope/v<N>`
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

if render_review "$REVIEW_FILE"; then
  if fallow ci post-review \
      --provider github \
      --pr "$PR_NUMBER" \
      --repo "$GH_REPO" \
      --envelope "$REVIEW_FILE" > "$POST_FILE" 2> "$POST_STDERR_FILE"; then
    if jq -e '((.apply_errors // []) | length > 0) or ((.post_errors // []) | length > 0)' "$POST_FILE" > /dev/null 2>&1; then
      HINT=$(jq -r '.apply_hint // "refresh provider state and rerun the job"' "$POST_FILE")
      UNAPPLIED=$(jq -r '(.unapplied_fingerprints // []) | join(", ")' "$POST_FILE")
      if [ -n "$UNAPPLIED" ]; then
        echo "::warning::fallow post-review incomplete: $HINT (unapplied fingerprints: $UNAPPLIED)"
      else
        echo "::warning::fallow post-review incomplete: $HINT"
      fi
    fi
    ACTION=$(jq -r '.action // "unknown"' "$POST_FILE")
    POSTED=$(jq -r '.comments_posted // 0' "$POST_FILE")
    RESOLUTIONS=$(jq -r '.resolution_comments_posted // 0' "$POST_FILE")
    THREADS=$(jq -r '.threads_resolved // 0' "$POST_FILE")
    COMMENT_NOUN="inline comments"
    RESOLUTION_NOUN="resolution replies"
    THREAD_NOUN="threads"
    if [ "$POSTED" = "1" ]; then COMMENT_NOUN="inline comment"; fi
    if [ "$RESOLUTIONS" = "1" ]; then RESOLUTION_NOUN="resolution reply"; fi
    if [ "$THREADS" = "1" ]; then THREAD_NOUN="thread"; fi
    echo "Review action: ${ACTION} (${POSTED} ${COMMENT_NOUN} posted, ${RESOLUTIONS} ${RESOLUTION_NOUN} posted, ${THREADS} ${THREAD_NOUN} resolved)"
  else
    echo "::warning::Failed to post review comments"
  fi
  exit 0
fi

echo "::warning::Failed to render typed review envelope"
exit 0
