#!/usr/bin/env bash

# Shared helpers for GitLab MR integration scripts.

# Track mktemp files so an EXIT trap cleans them up on signal or early exit.
_FALLOW_TMPS=()
trap 'rm -f "${_FALLOW_TMPS[@]:-}"' EXIT

FALLOW_RENDER_ARGS=()

prepare_fallow_render_args() {
  local format=$1
  local results_file="${FALLOW_RESULTS_FILE:-fallow-results.json}"
  local root="${FALLOW_ROOT:-${INPUT_ROOT:-.}}"
  [ -s "$results_file" ] || return 1
  FALLOW_RENDER_ARGS=(
    report
    --from "$results_file"
    --root "$root"
    --quiet
    --format "$format"
  )
  [ -n "${FALLOW_CONFIG:-}" ] && FALLOW_RENDER_ARGS+=(--config "$FALLOW_CONFIG")
  [ -n "${FALLOW_WORKSPACE:-}" ] && FALLOW_RENDER_ARGS+=(--workspace "$FALLOW_WORKSPACE")
  [ "${FALLOW_RENDER_PATH_PREFIX_SET:-0}" = "1" ] \
    && FALLOW_RENDER_ARGS+=(--report-path-prefix "${FALLOW_RENDER_PATH_PREFIX:-}")
  if [ -z "${FALLOW_DIFF_FILE:-}" ] && [ -n "${CI_MERGE_REQUEST_DIFF_BASE_SHA:-}" ]; then
    if git diff "${CI_MERGE_REQUEST_DIFF_BASE_SHA}..HEAD" > fallow-mr.diff 2>fallow-mr-diff-stderr.log; then
      export FALLOW_DIFF_FILE="$PWD/fallow-mr.diff"
    else
      echo "WARNING: Failed to fetch MR diff; diff filter disabled, reporting all findings"
      rm -f fallow-mr.diff
    fi
  fi
  export FALLOW_DIFF_FILTER="${FALLOW_DIFF_FILTER:-added}"
}

saved_render_is_unsupported() {
  local stderr_file=$1
  [ -s "$stderr_file" ] || return 1
  grep -Fq \
    'fallow report supports --format github-annotations, github-summary, codeclimate, or sarif only' \
    "$stderr_file" \
    || grep -Eq "unrecognized subcommand ['\"]?report" "$stderr_file"
}

prepare_fallow_direct_render_args() {
  local format=$1
  local args_file="${FALLOW_ANALYSIS_ARGS_FILE:-fallow-analysis-args.bin}"
  [ -s "$args_file" ] || return 1
  FALLOW_RENDER_ARGS=()
  local arg
  while IFS= read -r -d '' arg; do
    FALLOW_RENDER_ARGS+=("$arg")
  done < "$args_file"
  [ "${#FALLOW_RENDER_ARGS[@]}" -gt 0 ] || return 1
  local replaced=false
  local i
  for i in "${!FALLOW_RENDER_ARGS[@]}"; do
    if [ "${FALLOW_RENDER_ARGS[$i]}" = "--format" ] \
        && [ $((i + 1)) -lt "${#FALLOW_RENDER_ARGS[@]}" ]; then
      FALLOW_RENDER_ARGS[i + 1]="$format"
      replaced=true
      break
    fi
  done
  [ "$replaced" = "true" ] || FALLOW_RENDER_ARGS+=(--format "$format")
  [ "${FALLOW_RENDER_PATH_PREFIX_SET:-0}" = "1" ] \
    && FALLOW_RENDER_ARGS+=(--report-path-prefix "${FALLOW_RENDER_PATH_PREFIX:-}")
  return 0
}

curl_retry() {
  local attempts="${FALLOW_API_RETRIES:-3}"
  local delay="${FALLOW_API_RETRY_DELAY:-2}"
  local attempt=1
  local err out
  err=$(mktemp)
  _FALLOW_TMPS+=("$err")
  out=$(mktemp)
  _FALLOW_TMPS+=("$out")
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
