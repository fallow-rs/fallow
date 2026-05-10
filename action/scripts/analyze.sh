#!/usr/bin/env bash
# Disable errexit — composite action runners inject -e via the shell
# invocation, but this script handles errors explicitly with if-guards.
set +e -o pipefail

# Run fallow analysis with CLI argument construction (deduped)
# Required env: INPUT_COMMAND, INPUT_ROOT, INPUT_CONFIG, INPUT_FORMAT, INPUT_PRODUCTION,
#   INPUT_PRODUCTION_DEAD_CODE, INPUT_PRODUCTION_HEALTH, INPUT_PRODUCTION_DUPES,
#   INPUT_CHANGED_SINCE, INPUT_AUTO_CHANGED_SINCE, PR_BASE_SHA, EVENT_NAME,
#   INPUT_BASELINE, INPUT_SAVE_BASELINE, INPUT_FAIL_ON_REGRESSION,
#   INPUT_TOLERANCE, INPUT_REGRESSION_BASELINE, INPUT_SAVE_REGRESSION_BASELINE,
#   INPUT_ARGS, INPUT_DUPES_MODE,
#   INPUT_MIN_TOKENS, INPUT_MIN_LINES, INPUT_THRESHOLD, INPUT_SKIP_LOCAL,
#   INPUT_CROSS_LANGUAGE, INPUT_DRY_RUN, INPUT_WORKSPACE, INPUT_CHANGED_WORKSPACES,
#   INPUT_MAX_CYCLOMATIC,
#   INPUT_MAX_COGNITIVE, INPUT_TOP, INPUT_SORT, INPUT_FILE_SCORES, INPUT_HOTSPOTS,
#   INPUT_TARGETS, INPUT_COMPLEXITY, INPUT_SINCE, INPUT_MIN_COMMITS,
#   INPUT_COVERAGE, INPUT_PRODUCTION_COVERAGE, INPUT_COVERAGE_ROOT, INPUT_MIN_INVOCATIONS_HOT,
#   INPUT_MIN_OBSERVATION_VOLUME, INPUT_LOW_TRAFFIC_THRESHOLD,
#   INPUT_GATE, INPUT_DEAD_CODE_BASELINE, INPUT_HEALTH_BASELINE, INPUT_DUPES_BASELINE,
#   INPUT_SCORE, INPUT_SAVE_SNAPSHOT, INPUT_TREND, INPUT_ISSUE_TYPES, INPUT_NO_CACHE, INPUT_THREADS,
#   INPUT_ONLY, INPUT_SKIP

# --- Shared argument building functions ---
# Uses global ARGS array (avoids bash nameref compatibility issues)

build_common_args() {
  local format=${1:-json}

  ARGS=(--root "$INPUT_ROOT" --quiet --format "$format")
  [ -n "$INPUT_COMMAND" ] && ARGS=("$INPUT_COMMAND" "${ARGS[@]}")

  [ -n "${INPUT_CONFIG:-}" ] && ARGS+=(--config "$INPUT_CONFIG")
  [ "${INPUT_PRODUCTION:-}" = "true" ] && ARGS+=(--production)
  if [ -z "$INPUT_COMMAND" ]; then
    [ "${INPUT_PRODUCTION_DEAD_CODE:-}" = "true" ] && ARGS+=(--production-dead-code)
    [ "${INPUT_PRODUCTION_HEALTH:-}" = "true" ] && ARGS+=(--production-health)
    [ "${INPUT_PRODUCTION_DUPES:-}" = "true" ] && ARGS+=(--production-dupes)
  fi
  [ -n "${INPUT_CHANGED_SINCE:-}" ] && ARGS+=(--changed-since "$INPUT_CHANGED_SINCE")
  [ -n "${INPUT_BASELINE:-}" ] && ARGS+=(--baseline "$INPUT_BASELINE")
  [ -n "${INPUT_SAVE_BASELINE:-}" ] && ARGS+=(--save-baseline "$INPUT_SAVE_BASELINE")
  [ -n "${INPUT_WORKSPACE:-}" ] && ARGS+=(--workspace "$INPUT_WORKSPACE")
  [ -n "${INPUT_CHANGED_WORKSPACES:-}" ] && ARGS+=(--changed-workspaces "$INPUT_CHANGED_WORKSPACES")
  [ "${INPUT_NO_CACHE:-}" = "true" ] && ARGS+=(--no-cache)
  [ -n "${INPUT_THREADS:-}" ] && ARGS+=(--threads "$INPUT_THREADS")

  if [ -z "$INPUT_COMMAND" ]; then
    [ -n "${INPUT_ONLY:-}" ] && ARGS+=(--only "$INPUT_ONLY")
    [ -n "${INPUT_SKIP:-}" ] && ARGS+=(--skip "$INPUT_SKIP")
  fi
}

build_command_args() {
  local include_top=${1:-true}

  case "$INPUT_COMMAND" in
    dead-code|check)
      if [ "${INPUT_FORMAT:-}" = "sarif" ] && [ "${HAS_SARIF_FILE:-false}" = "true" ]; then
        ARGS+=(--sarif-file fallow-results.sarif)
      fi
      if [ -n "${INPUT_ISSUE_TYPES:-}" ]; then
        IFS=',' read -ra TYPES <<< "$INPUT_ISSUE_TYPES"
        for t in "${TYPES[@]}"; do
          t="$(echo "$t" | xargs)"
          ARGS+=("--${t}")
        done
      fi
      [ "${INPUT_INCLUDE_ENTRY_EXPORTS:-}" = "true" ] && ARGS+=(--include-entry-exports)
      [ "${INPUT_FAIL_ON_REGRESSION:-}" = "true" ] && ARGS+=(--fail-on-regression)
      [ -n "${INPUT_TOLERANCE:-}" ] && [ "${INPUT_TOLERANCE:-}" != "0" ] && ARGS+=(--tolerance "$INPUT_TOLERANCE")
      [ -n "${INPUT_REGRESSION_BASELINE:-}" ] && ARGS+=(--regression-baseline "$INPUT_REGRESSION_BASELINE")
      [ -n "${INPUT_SAVE_REGRESSION_BASELINE:-}" ] && ARGS+=(--save-regression-baseline "$INPUT_SAVE_REGRESSION_BASELINE")
      ;;
    dupes)
      ARGS+=(--mode "${INPUT_DUPES_MODE:-mild}")
      [ -n "${INPUT_MIN_TOKENS:-}" ] && ARGS+=(--min-tokens "$INPUT_MIN_TOKENS")
      [ -n "${INPUT_MIN_LINES:-}" ] && ARGS+=(--min-lines "$INPUT_MIN_LINES")
      [ -n "${INPUT_THRESHOLD:-}" ] && ARGS+=(--threshold "$INPUT_THRESHOLD")
      [ "${INPUT_SKIP_LOCAL:-}" = "true" ] && ARGS+=(--skip-local)
      [ "${INPUT_CROSS_LANGUAGE:-}" = "true" ] && ARGS+=(--cross-language)
      [ "${INPUT_IGNORE_IMPORTS:-}" = "true" ] && ARGS+=(--ignore-imports)
      [ "$include_top" = "true" ] && [ -n "${INPUT_TOP:-}" ] && ARGS+=(--top "$INPUT_TOP")
      ;;
    health)
      [ -n "${INPUT_MAX_CYCLOMATIC:-}" ] && ARGS+=(--max-cyclomatic "$INPUT_MAX_CYCLOMATIC")
      [ -n "${INPUT_MAX_COGNITIVE:-}" ] && ARGS+=(--max-cognitive "$INPUT_MAX_COGNITIVE")
      [ -n "${INPUT_MAX_CRAP:-}" ] && ARGS+=(--max-crap "$INPUT_MAX_CRAP")
      [ -n "${INPUT_COVERAGE:-}" ] && ARGS+=(--coverage "$INPUT_COVERAGE")
      [ -n "${INPUT_PRODUCTION_COVERAGE:-}" ] && ARGS+=(--runtime-coverage "$INPUT_PRODUCTION_COVERAGE")
      [ -n "${INPUT_COVERAGE_ROOT:-}" ] && ARGS+=(--coverage-root "$INPUT_COVERAGE_ROOT")
      [ -n "${INPUT_MIN_INVOCATIONS_HOT:-}" ] && ARGS+=(--min-invocations-hot "$INPUT_MIN_INVOCATIONS_HOT")
      [ -n "${INPUT_MIN_OBSERVATION_VOLUME:-}" ] && ARGS+=(--min-observation-volume "$INPUT_MIN_OBSERVATION_VOLUME")
      [ -n "${INPUT_LOW_TRAFFIC_THRESHOLD:-}" ] && ARGS+=(--low-traffic-threshold "$INPUT_LOW_TRAFFIC_THRESHOLD")
      [ "$include_top" = "true" ] && [ -n "${INPUT_TOP:-}" ] && ARGS+=(--top "$INPUT_TOP")
      [ -n "${INPUT_SORT:-}" ] && ARGS+=(--sort "$INPUT_SORT")
      [ "${INPUT_SCORE:-}" = "true" ] && ARGS+=(--score)
      [ "${INPUT_FILE_SCORES:-}" = "true" ] && ARGS+=(--file-scores)
      [ "${INPUT_HOTSPOTS:-}" = "true" ] && ARGS+=(--hotspots)
      [ "${INPUT_TARGETS:-}" = "true" ] && ARGS+=(--targets)
      [ "${INPUT_COMPLEXITY:-}" = "true" ] && ARGS+=(--complexity)
      [ -n "${INPUT_SINCE:-}" ] && ARGS+=(--since "$INPUT_SINCE")
      [ -n "${INPUT_MIN_COMMITS:-}" ] && ARGS+=(--min-commits "$INPUT_MIN_COMMITS")
      [ -n "${INPUT_MIN_SEVERITY:-}" ] && ARGS+=(--min-severity "$INPUT_MIN_SEVERITY")
      if [ -n "${INPUT_SAVE_SNAPSHOT:-}" ]; then
        if [ "$INPUT_SAVE_SNAPSHOT" = "true" ]; then
          ARGS+=(--save-snapshot)
        else
          ARGS+=(--save-snapshot "$INPUT_SAVE_SNAPSHOT")
        fi
      fi
      [ "${INPUT_TREND:-}" = "true" ] && ARGS+=(--trend)
      ;;
    audit)
      [ "${INPUT_PRODUCTION_DEAD_CODE:-}" = "true" ] && ARGS+=(--production-dead-code)
      [ "${INPUT_PRODUCTION_HEALTH:-}" = "true" ] && ARGS+=(--production-health)
      [ "${INPUT_PRODUCTION_DUPES:-}" = "true" ] && ARGS+=(--production-dupes)
      [ -n "${INPUT_DEAD_CODE_BASELINE:-}" ] && ARGS+=(--dead-code-baseline "$INPUT_DEAD_CODE_BASELINE")
      [ -n "${INPUT_HEALTH_BASELINE:-}" ] && ARGS+=(--health-baseline "$INPUT_HEALTH_BASELINE")
      [ -n "${INPUT_DUPES_BASELINE:-}" ] && ARGS+=(--dupes-baseline "$INPUT_DUPES_BASELINE")
      [ -n "${INPUT_MAX_CRAP:-}" ] && ARGS+=(--max-crap "$INPUT_MAX_CRAP")
      [ -n "${INPUT_COVERAGE:-}" ] && ARGS+=(--coverage "$INPUT_COVERAGE")
      [ -n "${INPUT_COVERAGE_ROOT:-}" ] && ARGS+=(--coverage-root "$INPUT_COVERAGE_ROOT")
      [ -n "${INPUT_GATE:-}" ] && ARGS+=(--gate "$INPUT_GATE")
      [ "${INPUT_INCLUDE_ENTRY_EXPORTS:-}" = "true" ] && ARGS+=(--include-entry-exports)
      ;;
    fix)
      if [ "${INPUT_DRY_RUN:-}" = "true" ]; then
        ARGS+=(--dry-run)
      else
        ARGS+=(--yes)
      fi
      ;;
    "")
      if [ "${INPUT_FORMAT:-}" = "sarif" ] && [ "${HAS_SARIF_FILE:-false}" = "true" ]; then
        ARGS+=(--sarif-file fallow-results.sarif)
      fi
      [ "${INPUT_SCORE:-}" = "true" ] && ARGS+=(--score)
      [ "${INPUT_TREND:-}" = "true" ] && ARGS+=(--trend)
      if [ -n "${INPUT_SAVE_SNAPSHOT:-}" ]; then
        if [ "$INPUT_SAVE_SNAPSHOT" = "true" ]; then
          ARGS+=(--save-snapshot)
        else
          ARGS+=(--save-snapshot "$INPUT_SAVE_SNAPSHOT")
        fi
      fi
      [ "${INPUT_FAIL_ON_REGRESSION:-}" = "true" ] && ARGS+=(--fail-on-regression)
      [ -n "${INPUT_TOLERANCE:-}" ] && [ "${INPUT_TOLERANCE:-}" != "0" ] && ARGS+=(--tolerance "$INPUT_TOLERANCE")
      [ -n "${INPUT_REGRESSION_BASELINE:-}" ] && ARGS+=(--regression-baseline "$INPUT_REGRESSION_BASELINE")
      [ -n "${INPUT_SAVE_REGRESSION_BASELINE:-}" ] && ARGS+=(--save-regression-baseline "$INPUT_SAVE_REGRESSION_BASELINE")
      ;;
  esac
}

# --- Validation ---

case "$INPUT_COMMAND" in
  ""|dead-code|check|dupes|health|audit|fix) ;;
  *) echo "::error::Invalid command: ${INPUT_COMMAND}. Must be dead-code, dupes, health, audit, fix, or empty (runs all)."; exit 2 ;;
esac

if [ "$INPUT_COMMAND" = "audit" ] && { [ -n "${INPUT_BASELINE:-}" ] || [ -n "${INPUT_SAVE_BASELINE:-}" ]; }; then
  echo "::error::The audit command does not support the generic baseline/save-baseline inputs. Use dead-code-baseline, health-baseline, or dupes-baseline instead."
  exit 2
fi

if [ -n "${INPUT_GATE:-}" ] && [ "$INPUT_GATE" != "new-only" ] && [ "$INPUT_GATE" != "all" ]; then
  echo "::error::gate must be 'new-only' or 'all', got: ${INPUT_GATE}"; exit 2
fi

for name_val in "min-tokens:${INPUT_MIN_TOKENS:-}" "min-lines:${INPUT_MIN_LINES:-}" \
               "max-cyclomatic:${INPUT_MAX_CYCLOMATIC:-}" "max-cognitive:${INPUT_MAX_COGNITIVE:-}" \
               "top:${INPUT_TOP:-}" "min-commits:${INPUT_MIN_COMMITS:-}" "threads:${INPUT_THREADS:-}" \
               "min-invocations-hot:${INPUT_MIN_INVOCATIONS_HOT:-}" "min-observation-volume:${INPUT_MIN_OBSERVATION_VOLUME:-}"; do
  name="${name_val%%:*}"; val="${name_val#*:}"
  if [ -n "$val" ] && ! [[ "$val" =~ ^[0-9]+$ ]]; then
    echo "::error::${name} must be a positive integer, got: ${val}"; exit 2
  fi
done
if [ -n "${INPUT_THRESHOLD:-}" ] && ! [[ "$INPUT_THRESHOLD" =~ ^[0-9]+\.?[0-9]*$ ]]; then
  echo "::error::threshold must be a number, got: ${INPUT_THRESHOLD}"; exit 2
fi
# max-crap accepts floating-point values (e.g. 30.0, 45.5) because CRAP scores
# are non-integer. Use the same numeric regex as threshold.
if [ -n "${INPUT_MAX_CRAP:-}" ] && ! [[ "$INPUT_MAX_CRAP" =~ ^[0-9]+\.?[0-9]*$ ]]; then
  echo "::error::max-crap must be a non-negative number, got: ${INPUT_MAX_CRAP}"; exit 2
fi
if [ -n "${INPUT_LOW_TRAFFIC_THRESHOLD:-}" ] && ! [[ "$INPUT_LOW_TRAFFIC_THRESHOLD" =~ ^[0-9]+\.?[0-9]*$ ]]; then
  echo "::error::low-traffic-threshold must be a non-negative number, got: ${INPUT_LOW_TRAFFIC_THRESHOLD}"; exit 2
fi

# --- Check for --sarif-file support ---

HAS_SARIF_FILE=false
if { [ "$INPUT_COMMAND" = "dead-code" ] || [ "$INPUT_COMMAND" = "check" ] || [ -z "$INPUT_COMMAND" ]; }; then
  HELP_TMP=$(mktemp)
  fallow dead-code --help > "$HELP_TMP" 2>/dev/null || true
  if /usr/bin/grep -q -- '--sarif-file' "$HELP_TMP"; then
    HAS_SARIF_FILE=true
  fi
  rm -f "$HELP_TMP"
fi

# --- Auto-detect changed-since in PR context ---

if [ -z "${INPUT_CHANGED_SINCE:-}" ] && [ "${INPUT_AUTO_CHANGED_SINCE:-}" = "true" ] && \
   { [ "${EVENT_NAME:-}" = "pull_request" ] || [ "${EVENT_NAME:-}" = "pull_request_target" ]; } && \
   [ -n "${PR_BASE_SHA:-}" ]; then
  INPUT_CHANGED_SINCE="$PR_BASE_SHA"
  echo "::notice::Auto-scoping analysis to files changed since PR base (${PR_BASE_SHA:0:7})"
fi

# Propagate the effective changed-since value so downstream steps can filter
echo "changed_since=${INPUT_CHANGED_SINCE:-}" >> "$GITHUB_OUTPUT"

# --- Pre-compute changed files list for downstream filtering ---
# Downstream scripts (comment, summary, annotations, review) need the list of
# changed files to scope results to the PR. On shallow clones (the default
# actions/checkout depth), git diff against the base SHA fails. We compute the
# list here once — trying git first, then the GitHub API — and save it for reuse.

if [ -n "${INPUT_CHANGED_SINCE:-}" ]; then
  _ROOT="${INPUT_ROOT:-.}"
  _CHANGED=""

  # Try three-dot diff (precise: changes since merge-base, needs full history)
  _CHANGED=$(cd "$_ROOT" && git diff --name-only --relative "${INPUT_CHANGED_SINCE}...HEAD" -- . 2>/dev/null || true)

  # Shallow clone fallback: fetch the base commit and try two-dot diff
  if [ -z "$_CHANGED" ]; then
    if ! git cat-file -e "${INPUT_CHANGED_SINCE}^{commit}" 2>/dev/null; then
      git fetch --depth=1 origin "$INPUT_CHANGED_SINCE" 2>/dev/null || true
    fi
    _CHANGED=$(cd "$_ROOT" && git diff --name-only --relative "${INPUT_CHANGED_SINCE}" HEAD -- . 2>/dev/null || true)
  fi

  # Last resort: GitHub API (works regardless of clone depth)
  if [ -z "$_CHANGED" ] && [ -n "${GH_TOKEN:-}" ] && [ -n "${PR_NUMBER:-}" ] && [ -n "${GH_REPO:-}" ]; then
    _API_FILES=$(gh api --paginate "repos/${GH_REPO}/pulls/${PR_NUMBER}/files" --jq '.[].filename' 2>/dev/null || true)
    if [ -n "$_API_FILES" ]; then
      if [ "$_ROOT" != "." ]; then
        # Strip root prefix — API returns repo-root-relative paths, fallow JSON uses root-relative
        _CHANGED=$(echo "$_API_FILES" | sed -n "s|^${_ROOT}/||p")
      else
        _CHANGED="$_API_FILES"
      fi
    fi
  fi

  if [ -n "$_CHANGED" ]; then
    echo "$_CHANGED" | jq -R -s 'split("\n") | map(select(length > 0))' > fallow-changed-files.json
  else
    echo "::warning::Could not determine changed files for --changed-since scoping. Use fetch-depth: 0 in actions/checkout for best results."
  fi
fi

# --- Pre-compute unified diff for line-level hot-path scoping ---
# `fallow audit` and `fallow health` consume a unified diff to do
# line-overlap matching against runtime hot paths so the
# `hot-path-touched` verdict only fires when an added line falls inside
# a hot function's body, not merely when the file was touched. Mirrors
# the changed-files cascade above (three-dot diff, shallow-clone fetch
# fallback, GitHub API last resort) so behavior is consistent across
# checkout depths.
#
# Skip when the user already supplied `inputs.diff-file` (FALLOW_DIFF_FILE
# is non-empty in that case): respect their choice. Skip when there is no
# changed-since, since there is nothing to scope against.
#
# Export via $GITHUB_ENV so the comment / review render steps later in
# the composite action reuse the same diff file we wrote here, instead
# of re-running `gh pr diff` and double-paying the API quota.

# When the user supplied --diff-file via the action input, the env block
# already set FALLOW_DIFF_FILE on this step. Propagate it to subsequent
# composite steps via $GITHUB_ENV so the comment / review steps don't
# need to declare their own FALLOW_DIFF_FILE env (which would override
# the analyze-step propagation otherwise). User-supplied path wins.
if [ -n "${FALLOW_DIFF_FILE:-}" ] && [ -n "${GITHUB_ENV:-}" ]; then
  echo "FALLOW_DIFF_FILE=${FALLOW_DIFF_FILE}" >> "$GITHUB_ENV"
fi

if [ -n "${INPUT_CHANGED_SINCE:-}" ] && [ -z "${FALLOW_DIFF_FILE:-}" ]; then
  _ROOT="${INPUT_ROOT:-.}"
  _DIFF_PATH="$PWD/fallow-pr.diff"

  # Three-dot diff (precise: changes since merge-base, needs full history).
  if (cd "$_ROOT" && git diff --unified=0 --relative "${INPUT_CHANGED_SINCE}...HEAD" -- .) > "$_DIFF_PATH" 2>/dev/null; then
    :
  fi

  # Shallow-clone fallback: fetch the base commit, retry two-dot diff.
  if [ ! -s "$_DIFF_PATH" ]; then
    if ! git cat-file -e "${INPUT_CHANGED_SINCE}^{commit}" 2>/dev/null; then
      git fetch --depth=1 origin "$INPUT_CHANGED_SINCE" 2>/dev/null || true
    fi
    (cd "$_ROOT" && git diff --unified=0 --relative "${INPUT_CHANGED_SINCE}" HEAD -- .) > "$_DIFF_PATH" 2>/dev/null || true
  fi

  # Last resort: GitHub API. `gh pr diff` returns the same unified-diff
  # format git produces, so the downstream DiffIndex parser is identical.
  if [ ! -s "$_DIFF_PATH" ] && [ -n "${GH_TOKEN:-}" ] && [ -n "${PR_NUMBER:-}" ] && [ -n "${GH_REPO:-}" ]; then
    gh pr diff "$PR_NUMBER" --repo "$GH_REPO" > "$_DIFF_PATH" 2>/dev/null || true
  fi

  if [ -s "$_DIFF_PATH" ]; then
    export FALLOW_DIFF_FILE="$_DIFF_PATH"
    # Propagate to the comment / review render steps (separate composite
    # steps see only $GITHUB_ENV, not exported shell variables).
    if [ -n "${GITHUB_ENV:-}" ]; then
      echo "FALLOW_DIFF_FILE=${_DIFF_PATH}" >> "$GITHUB_ENV"
    fi
  else
    rm -f "$_DIFF_PATH"
    # Soft-degrade: line-level filtering disabled, the runtime-coverage
    # filter falls back to file-level via `--changed-since`. Emit a
    # machine-greppable warning so dashboards can alert on it without
    # parsing free-form text.
    echo "::warning::fallow: warning [shallow-clone]: could not produce unified diff for line-level hot-path scoping. Use fetch-depth: 0 in actions/checkout for line-precision."
  fi
fi

# --- Build and run main analysis ---

ARGS=()
build_common_args json
build_command_args true

# Parse extra arguments safely
EXTRA_ARGS=()
if [ -n "${INPUT_ARGS:-}" ]; then
  read -ra EXTRA_ARGS <<< "$INPUT_ARGS"
fi

# Run analysis — no --fail-on-issues so subsequent steps always run.
# Bare invocations may emit an error JSON (e.g., health on a non-git repo)
# followed by the actual combined results. Use jq -s 'last' to extract only
# the final JSON object so downstream parsing sees a single valid result.
{
  printf 'FALLOW_ANALYSIS_ARGS=('
  printf '%q ' "${ARGS[@]}" "${EXTRA_ARGS[@]}"
  printf ')\n'
} > fallow-analysis-args.sh

if ! fallow "${ARGS[@]}" "${EXTRA_ARGS[@]}" > fallow-results-raw.json 2> fallow-stderr.log; then
  if [ ! -s fallow-results-raw.json ] || ! jq -e '.' fallow-results-raw.json > /dev/null 2>&1; then
    echo "::error::Fallow failed to run"
    [ -s fallow-stderr.log ] && cat fallow-stderr.log
    [ -s fallow-results-raw.json ] && cat fallow-results-raw.json
    exit 2
  fi
fi
jq -s 'last' fallow-results-raw.json > fallow-results.json
rm -f fallow-results-raw.json
if jq -e '.error == true' fallow-results.json > /dev/null 2>&1; then
  MESSAGE=$(jq -r '.message // "Fallow failed"' fallow-results.json)
  EXIT_CODE=$(jq -r '.exit_code // 2' fallow-results.json)
  echo "::error::${MESSAGE}"
  exit "$EXIT_CODE"
fi

# --- Fallback SARIF generation ---

if { [ "${INPUT_FORMAT:-}" = "sarif" ] || [ "${INPUT_SARIF:-}" = "true" ]; } && \
   [ "$INPUT_COMMAND" != "fix" ] && \
   { [ ! -f fallow-results.sarif ] || ! jq -e '.' fallow-results.sarif > /dev/null 2>&1; }; then
  ARGS=()
  build_common_args sarif
  build_command_args false  # omit --top for SARIF

  if ! fallow "${ARGS[@]}" "${EXTRA_ARGS[@]}" > fallow-results.sarif 2>/dev/null; then
    echo "::warning::SARIF generation failed"
  fi
fi

# --- Surface warnings from stderr ---

if [ -s fallow-stderr.log ]; then
  while IFS= read -r line; do
    echo "::debug::${line}"
  done < fallow-stderr.log
fi

# --- Extract verdict / gate (audit only) and issue count ---
# Audit's verdict (pass/warn/fail) is the load-bearing severity-aware signal:
# warn means "warn-tier issues only, do not fail CI". Threshold step gates on
# verdict for audit; raw issue counts only gate non-audit commands.

VERDICT=""
GATE=""
if [ "$INPUT_COMMAND" = "audit" ]; then
  VERDICT=$(jq -r '.verdict // ""' fallow-results.json)
  GATE=$(jq -r '.attribution.gate // ""' fallow-results.json)
fi

case "$INPUT_COMMAND" in
  dead-code|check) ISSUES=$(jq -r '.total_issues' fallow-results.json) ;;
  dupes)           ISSUES=$(jq -r '.stats.clone_groups' fallow-results.json) ;;
  health)          ISSUES=$(jq -r '((.summary.functions_above_threshold // 0) + ((.runtime_coverage.findings // []) | map(select(.verdict == "safe_to_delete" or .verdict == "review_required" or .verdict == "low_traffic")) | length))' fallow-results.json) ;;
  audit)           ISSUES=$(jq -r 'if (.attribution.gate // "new-only") == "all" then ((.summary.dead_code_issues // 0) + (.summary.complexity_findings // 0) + (.summary.duplication_clone_groups // 0)) else ((.attribution.dead_code_introduced // 0) + (.attribution.complexity_introduced // 0) + (.attribution.duplication_introduced // 0)) end' fallow-results.json) ;;
  fix)             ISSUES=$(jq -r '(.fixes | length)' fallow-results.json) ;;
  "")              ISSUES=$(jq -r '((.check.total_issues // 0) + (.dupes.stats.clone_groups // 0) + (.health.summary.functions_above_threshold // 0) + ((.health.runtime_coverage.findings // []) | map(select(.verdict == "safe_to_delete" or .verdict == "review_required" or .verdict == "low_traffic")) | length))' fallow-results.json) ;;
esac

if ! [[ "$ISSUES" =~ ^[0-9]+$ ]]; then
  echo "::error::Unexpected issue count: ${ISSUES}"
  exit 2
fi

echo "issues=${ISSUES}" >> "$GITHUB_OUTPUT"
echo "results=fallow-results.json" >> "$GITHUB_OUTPUT"
echo "command=${INPUT_COMMAND}" >> "$GITHUB_OUTPUT"
echo "verdict=${VERDICT}" >> "$GITHUB_OUTPUT"
echo "gate=${GATE}" >> "$GITHUB_OUTPUT"

if [ -f fallow-results.sarif ]; then
  echo "sarif=fallow-results.sarif" >> "$GITHUB_OUTPUT"
fi

if [ "$ISSUES" -gt 0 ]; then
  case "$INPUT_COMMAND" in
    dead-code|check) echo "::warning::Fallow found ${ISSUES} unused code issues" ;;
    dupes)           echo "::warning::Fallow found ${ISSUES} clone groups" ;;
    health)          echo "::warning::Fallow found ${ISSUES} high complexity functions" ;;
    audit)           echo "::warning::Fallow audit found ${ISSUES} introduced issues in changed files" ;;
    fix)             echo "::warning::Fallow proposed ${ISSUES} fixes" ;;
    "")              echo "::warning::Fallow found ${ISSUES} issues" ;;
  esac
fi
