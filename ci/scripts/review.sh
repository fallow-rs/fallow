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

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=gitlab_common.sh
source "${SCRIPT_DIR}/gitlab_common.sh"

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
  prepare_fallow_render_args "$format" || return 1
  load_gitlab_diff_refs
  FALLOW_MAX_COMMENTS="$MAX" fallow "${FALLOW_RENDER_ARGS[@]}" > "$output" 2> fallow-review-stderr.log || true
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
  if fallow ci post-review \
      --provider gitlab \
      --mr "$CI_MERGE_REQUEST_IID" \
      --project-id "$CI_PROJECT_ID" \
      --api-url "$CI_API_V4_URL" \
      --envelope fallow-review.json > fallow-review-post.json 2> fallow-review-post-stderr.log; then
    if jq -e '(.apply_errors // []) | length > 0 or (.post_errors // []) | length > 0' fallow-review-post.json > /dev/null 2>&1; then
      HINT=$(jq -r '.apply_hint // "refresh provider state and rerun the job"' fallow-review-post.json)
      echo "WARNING: fallow post-review incomplete: $HINT"
    fi
    ACTION=$(jq -r '.action // "unknown"' fallow-review-post.json)
    POSTED=$(jq -r '.comments_posted // 0' fallow-review-post.json)
    echo "Review action: ${ACTION} (${POSTED} inline comments posted)"
  else
    echo "WARNING: Failed to post review comments"
  fi
  exit 0
fi

echo "WARNING: Failed to render typed review envelope"
exit 0
