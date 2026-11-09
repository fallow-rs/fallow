#!/usr/bin/env bash
# Shared render helpers for the Action steps that call `fallow report`:
# comment.sh, review.sh, summary.sh and annotate.sh source this file. It
# defines functions and variables only. The caller owns the EXIT handler.
#
# Optional env: FALLOW_ARTIFACTS_DIR, FALLOW_RESULTS_FILE, FALLOW_ROOT,
#   INPUT_ROOT, INPUT_CONFIG, INPUT_WORKSPACE, HAS_NATIVE_REPORT,
#   FALLOW_ANALYSIS_ARGS_JSON, FALLOW_RENDER_PATH_PREFIX_SET,
#   FALLOW_RENDER_PATH_PREFIX, FALLOW_DIFF_FILTER, and the mutation variables
#   of fallow_mutation_render_args (HAS_MUTATION_REPORT,
#   FALLOW_MUTATION_NOT_RUN, FALLOW_MUTATION_STATUS_FILE,
#   FALLOW_MUTATION_RESULTS_FILE).

# shellcheck source=../../ci/scripts/mutation_common.sh
. "$(dirname -- "${BASH_SOURCE[0]}")/../../ci/scripts/mutation_common.sh"

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

MUTATION_RENDER_ARGS=()

# Sets MUTATION_RENDER_ARGS to the mutation arguments of `fallow report`. The
# array is empty when the job ran no mutation step or the CLI cannot render
# mutation results. Append it with
# ${MUTATION_RENDER_ARGS[@]+"${MUTATION_RENDER_ARGS[@]}"}, because bash before
# 4.4 treats an empty array as unset under `set -u`.
load_mutation_render_args() {
  MUTATION_RENDER_ARGS=()
  local arg
  while IFS= read -r arg; do
    MUTATION_RENDER_ARGS+=("$arg")
  done < <(fallow_mutation_render_args)
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

# Renders a saved PR comment or review with `fallow report --from`, or with
# the direct command of the analyze step on a CLI that cannot render saved
# results. Arguments: format, output file, stderr file, and the name of the
# rendered surface for the log (for example "PR comments"). The caller sets
# the per-format env (FALLOW_COMMENT_ID, FALLOW_MAX_COMMENTS) and checks the
# shape of the output. Returns 1 when the render failed.
render_with_fallow() {
  local format=$1
  local output=$2
  local render_stderr=$3
  local surface=$4
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
    load_mutation_render_args
    args+=(${MUTATION_RENDER_ARGS[@]+"${MUTATION_RENDER_ARGS[@]}"})
  fi
  export FALLOW_DIFF_FILTER="${FALLOW_DIFF_FILTER:-added}"
  local render_status=0
  fallow "${args[@]}" > "$output" 2> "$render_stderr" || render_status=$?
  if [ "$legacy_render" = "false" ] && [ "$render_status" -ne 0 ] \
      && saved_target_is_unsupported "$output" "$render_stderr"; then
    if ! build_legacy_render_args "$format"; then
      echo "::warning::Pinned fallow CLI cannot render saved ${surface} and no safe fallback arguments are available"
      return 1
    fi
    echo "::debug::Pinned fallow CLI lacks saved ${surface} rendering; using compatibility renderer"
    : > "$output"
    : > "$render_stderr"
    render_status=0
    fallow "${LEGACY_RENDER_ARGS[@]}" > "$output" 2> "$render_stderr" || render_status=$?
    legacy_render=true
  fi
  [ "$legacy_render" = "true" ] && [ "$render_status" -eq 1 ] && render_status=0
  # Surface fallow's structured-error envelope before the shape check, so the
  # actual CLI message lands in the workflow log instead of a generic warning.
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
  return 0
}
