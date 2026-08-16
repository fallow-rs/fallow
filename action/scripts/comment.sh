#!/usr/bin/env bash
set -euo pipefail

# Post or update a PR comment with analysis results
# Env contract: see this script's step env block in action.yml, the
#   authoritative list of consumed variables. Hard requirements are asserted
#   below.

: "${GH_TOKEN:?GH_TOKEN is required}"
: "${PR_NUMBER:?PR_NUMBER is required}"
: "${GH_REPO:?GH_REPO is required}"

# Initialize markers so downstream gates always see definitive values. The
# Rust post adapter may later overwrite `post_skipped_reason` for clean runs.
if [ -n "${GITHUB_OUTPUT:-}" ]; then
  echo "post_skipped_reason=none" >> "$GITHUB_OUTPUT"
  echo "dedup_lookup_failed=false" >> "$GITHUB_OUTPUT"
fi

# Track mktemp files so an EXIT trap cleans them up on signal or early exit.
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
  case "${FALLOW_SUMMARY_SCOPE:-all}" in
    ""|all|diff)
      export FALLOW_SUMMARY_SCOPE="${FALLOW_SUMMARY_SCOPE:-all}"
      ;;
    *)
      echo "::warning::Unsupported FALLOW_SUMMARY_SCOPE '${FALLOW_SUMMARY_SCOPE}', expected 'all' or 'diff'; using 'all'"
      export FALLOW_SUMMARY_SCOPE="all"
      ;;
  esac
  local render_stderr
  local render_status=0
  render_stderr=$(artifact_path fallow-comment-stderr.log)
  FALLOW_COMMENT_ID="${FALLOW_COMMENT_ID:-fallow-results}" fallow "${args[@]}" > "$output" 2> "$render_stderr" || render_status=$?
  if [ "$legacy_render" = "false" ] && [ "$render_status" -ne 0 ] \
      && saved_target_is_unsupported "$output" "$render_stderr"; then
    if ! build_legacy_render_args "$format"; then
      echo "::warning::Pinned fallow CLI cannot render saved PR comments and no safe fallback arguments are available"
      return 1
    fi
    echo "::debug::Pinned fallow CLI lacks saved PR comment rendering; using compatibility renderer"
    : > "$output"
    : > "$render_stderr"
    render_status=0
    FALLOW_COMMENT_ID="${FALLOW_COMMENT_ID:-fallow-results}" fallow "${LEGACY_RENDER_ARGS[@]}" > "$output" 2> "$render_stderr" || render_status=$?
    legacy_render=true
  fi
  [ "$legacy_render" = "true" ] && [ "$render_status" -eq 1 ] && render_status=0
  # Surface fallow's structured-error envelope before the marker check, so the
  # actual CLI message lands in the workflow log instead of a generic "Failed
  # to render typed PR comment" warning. The envelope is JSON; if the file
  # parses as JSON with `.error == true`, treat it as a render failure and
  # echo the message.
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
  grep -q "^<!-- fallow-id: ${FALLOW_COMMENT_ID:-fallow-results} -->" "$output" \
    && grep -q "Generated by fallow\\." "$output"
}

PR_COMMENT_FILE=$(artifact_path fallow-pr-comment.md)
PR_COMMENT_ENVELOPE_FILE=$(artifact_path fallow-pr-comment-envelope.json)
PR_DECISION_FILE=$(artifact_path fallow-pr-decision.json)
PR_DETAILS_FILE=$(artifact_path fallow-pr-details.json)
if FALLOW_PR_COMMENT_ENVELOPE_FILE="$PR_COMMENT_ENVELOPE_FILE" \
   FALLOW_PR_DECISION_FILE="$PR_DECISION_FILE" \
   FALLOW_PR_DETAILS_FILE="$PR_DETAILS_FILE" \
   render_with_fallow pr-comment-github "$PR_COMMENT_FILE"; then
  PLAN_FILE=$(artifact_path fallow-pr-comment-plan.json)
  POST_COMMENT_ARGS=(
    ci post-pr-comment
    --provider github
    --pr "$PR_NUMBER"
    --repo "$GH_REPO"
    --body "$PR_COMMENT_FILE"
  )
  [ -f "$PR_COMMENT_ENVELOPE_FILE" ] && POST_COMMENT_ARGS+=(--envelope "$PR_COMMENT_ENVELOPE_FILE")
  POST_COMMENT_ARGS+=(--marker-id "${FALLOW_COMMENT_ID:-fallow-results}")
  if ! fallow "${POST_COMMENT_ARGS[@]}" > "$PLAN_FILE"; then
    echo "::warning::Failed to post PR comment"
    exit 0
  fi
  ACTION=$(jq -r '.action' "$PLAN_FILE")
  REASON=$(jq -r '.skip_reason // "none"' "$PLAN_FILE")
  if [ "$ACTION" = "skip" ] && [ -n "${GITHUB_OUTPUT:-}" ]; then
    echo "post_skipped_reason=${REASON}" >> "$GITHUB_OUTPUT"
  fi
  CHECK_HEAD_SHA="${PR_HEAD_SHA:-${GITHUB_SHA:-}}"
  if [ -f "$PR_DECISION_FILE" ] && [ -n "$CHECK_HEAD_SHA" ]; then
    CHECK_RUN_ARGS=(
      ci post-check-run
      --provider github
      --decision "$PR_DECISION_FILE"
      --repo "$GH_REPO"
      --head-sha "$CHECK_HEAD_SHA"
    )
    [ -n "${GITHUB_API_URL:-}" ] && CHECK_RUN_ARGS+=(--api-url "$GITHUB_API_URL")
    if ! fallow "${CHECK_RUN_ARGS[@]}" \
        > "$(artifact_path fallow-check-run.json)" \
        2> "$(artifact_path fallow-check-run-stderr.log)"; then
      echo "::warning::Failed to post Fallow check run"
    fi
  fi
  echo "PR comment action: ${ACTION} (${REASON})"
  exit 0
fi

echo "::warning::Failed to render typed PR comment"
exit 0
