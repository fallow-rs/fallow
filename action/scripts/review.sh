#!/usr/bin/env bash
set -euo pipefail

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

LEGACY_RENDER_ARGS=()

# Rebuild the direct-render argv from inert data written by the analyze step.
# This avoids executing the legacy workspace shell artifact.
build_legacy_render_args() {
  local format=$1
  local args_json="${FALLOW_ANALYSIS_ARGS_JSON:-}"
  [ -n "$args_json" ] || return 1
  jq -e 'type == "array" and length > 0 and all(.[]; type == "string")' \
    <<< "$args_json" > /dev/null 2>&1 || return 1

  local args=()
  local arg
  while IFS= read -r -d '' arg; do
    args+=("$arg")
  done < <(jq -j '.[] | ., "\u0000"' <<< "$args_json")

  local replaced=false
  local index
  for ((index = 0; index < ${#args[@]}; index++)); do
    if [ "${args[$index]}" = "--format" ] && [ $((index + 1)) -lt "${#args[@]}" ]; then
      args[index + 1]="$format"
      replaced=true
      break
    fi
  done
  [ "$replaced" = "true" ] || args+=(--format "$format")
  [ "${FALLOW_RENDER_PATH_PREFIX_SET:-0}" = "1" ] \
    && args+=(--report-path-prefix "${FALLOW_RENDER_PATH_PREFIX:-}")
  LEGACY_RENDER_ARGS=("${args[@]}")
}

saved_target_is_unsupported() {
  local output=$1
  local stderr_file=$2
  local old_error="fallow report supports --format github-annotations, github-summary, codeclimate, or sarif only"
  grep -Fq "$old_error" "$output" 2>/dev/null \
    || grep -Fq "$old_error" "$stderr_file" 2>/dev/null
}

render_with_fallow() {
  local format=$1
  local output=$2
  local results_file="${FALLOW_RESULTS_FILE:-fallow-results.json}"
  local root="${FALLOW_ROOT:-${INPUT_ROOT:-.}}"
  [ -s "$results_file" ] || return 1
  local args=()
  local legacy_render=false
  if [ "${HAS_NATIVE_REPORT:-}" = "false" ]; then
    build_legacy_render_args "$format" || return 1
    args=("${LEGACY_RENDER_ARGS[@]}")
    legacy_render=true
  else
    args=(report --from "$results_file" --root "$root" --quiet --format "$format")
    [ -n "${INPUT_CONFIG:-}" ] && args+=(--config "$INPUT_CONFIG")
    [ -n "${INPUT_WORKSPACE:-}" ] && args+=(--workspace "$INPUT_WORKSPACE")
    [ "${FALLOW_RENDER_PATH_PREFIX_SET:-0}" = "1" ] \
      && args+=(--report-path-prefix "${FALLOW_RENDER_PATH_PREFIX:-}")
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
  local render_stderr
  local render_status=0
  render_stderr=$(artifact_path fallow-review-stderr.log)
  FALLOW_MAX_COMMENTS="$MAX" fallow "${args[@]}" > "$output" 2> "$render_stderr" || render_status=$?
  if [ "$legacy_render" = "false" ] && [ "$render_status" -ne 0 ] \
      && saved_target_is_unsupported "$output" "$render_stderr"; then
    if ! build_legacy_render_args "$format"; then
      echo "::warning::Pinned fallow CLI cannot render saved reviews and no safe fallback arguments are available"
      return 1
    fi
    echo "::debug::Pinned fallow CLI lacks saved review rendering; using compatibility renderer"
    : > "$output"
    : > "$render_stderr"
    render_status=0
    FALLOW_MAX_COMMENTS="$MAX" fallow "${LEGACY_RENDER_ARGS[@]}" > "$output" 2> "$render_stderr" || render_status=$?
    legacy_render=true
  fi
  [ "$legacy_render" = "true" ] && [ "$render_status" -eq 1 ] && render_status=0
  # Surface fallow's structured-error envelope before the schema check so the
  # CLI message lands in the workflow log rather than a generic warning.
  if jq -e '.error == true' "$output" > /dev/null 2>&1; then
    echo "::warning::fallow render failed: $(jq -r '.message // "unknown error"' "$output")"
    return 1
  fi
  if [ "$render_status" -ne 0 ]; then
    echo "::warning::fallow render failed (exit ${render_status})"
    if [ -s "$render_stderr" ]; then
      while IFS= read -r line || [ -n "$line" ]; do
        printf 'fallow: %s\n' "$line"
      done < "$render_stderr"
    fi
    return 1
  fi
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
