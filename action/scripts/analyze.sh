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
#   INPUT_PRODUCTION_COVERAGE, INPUT_COVERAGE_ROOT, INPUT_MIN_INVOCATIONS_HOT,
#   INPUT_MIN_OBSERVATION_VOLUME, INPUT_LOW_TRAFFIC_THRESHOLD,
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
    fix)
      if [ "${INPUT_DRY_RUN:-}" = "true" ]; then
        ARGS+=(--dry-run)
      else
        ARGS+=(--yes)
      fi
      ;;
    audit)
      ARGS+=(--gate "${INPUT_GATE:-new-only}")
      [ -n "${INPUT_MAX_CRAP:-}" ] && ARGS+=(--max-crap "$INPUT_MAX_CRAP")
      [ "${INPUT_PRODUCTION_DEAD_CODE:-}" = "true" ] && ARGS+=(--production-dead-code)
      [ "${INPUT_PRODUCTION_HEALTH:-}" = "true" ] && ARGS+=(--production-health)
      [ "${INPUT_PRODUCTION_DUPES:-}" = "true" ] && ARGS+=(--production-dupes)
      [ "${INPUT_INCLUDE_ENTRY_EXPORTS:-}" = "true" ] && ARGS+=(--include-entry-exports)
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

# Validate gate input as a closed enum (only when command=audit, since users on
# other commands shouldn't hard-error on a stray gate value from inherited env)
INPUT_GATE="${INPUT_GATE:-new-only}"
if [ "$INPUT_COMMAND" = "audit" ]; then
  case "$INPUT_GATE" in
    new-only|all) ;;
    *) echo "::error::Invalid gate: ${INPUT_GATE}. Must be new-only or all."; exit 2 ;;
  esac
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

# Audit on non-PR events needs an explicit base. Don't silently fall through
# to fallow's auto-detect, because misconfigured workflows produce confusing
# "verdict: pass" results that simply analyzed nothing.
if [ "$INPUT_COMMAND" = "audit" ] && [ -z "${INPUT_CHANGED_SINCE:-}" ]; then
  if [ "${EVENT_NAME:-}" != "pull_request" ] && [ "${EVENT_NAME:-}" != "pull_request_target" ]; then
    echo "::error::command: audit on event '${EVENT_NAME:-unknown}' needs an explicit base. Set 'changed-since: <ref>' (e.g. origin/main) on the action, or pass --base via the 'args' input."
    exit 2
  fi
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

# --- Fallback SARIF generation ---

if [ "${INPUT_FORMAT:-}" = "sarif" ] && [ "$INPUT_COMMAND" != "fix" ] && \
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

# --- Audit-specific post-processing ---
# Audit produces top-level verdict/attribution and sub-result objects keyed
# `dead_code`, `complexity`, `duplication`. Re-key sub-results to `check`,
# `health`, `dupes` so existing summary/comment/annotation jq scripts work
# unchanged. Preserve top-level audit fields (command, verdict, attribution,
# gate, base_ref, head_sha, summary) so AI consumers can still distinguish
# audit runs from combined runs.

VERDICT=""
GATE=""

if [ "$INPUT_COMMAND" = "audit" ]; then
  VERDICT=$(jq -r '.verdict // ""' fallow-results.json)
  GATE=$(jq -r '.attribution.gate // ""' fallow-results.json)

  # When gate=new-only, prune findings to only `introduced: true` entries across
  # all three categories so PR annotations/comments reflect what the gate
  # actually fails on. The CLI annotates dead_code findings, complexity (health)
  # findings, and duplication clone_groups with `introduced: true|false` whenever
  # the audit base-snapshot pass runs. Findings without an `introduced` field
  # (older CLI binaries) are kept by the `!= false` predicate.
  if [ "$GATE" = "new-only" ]; then
    jq '
      def keep_introduced(arr):
        (arr // []) | map(select(.introduced != false));
      .dead_code //= {}
      | .dead_code.unused_files                = keep_introduced(.dead_code.unused_files)
      | .dead_code.unused_exports              = keep_introduced(.dead_code.unused_exports)
      | .dead_code.unused_types                = keep_introduced(.dead_code.unused_types)
      | .dead_code.private_type_leaks          = keep_introduced(.dead_code.private_type_leaks)
      | .dead_code.unused_dependencies         = keep_introduced(.dead_code.unused_dependencies)
      | .dead_code.unused_dev_dependencies     = keep_introduced(.dead_code.unused_dev_dependencies)
      | .dead_code.unused_optional_dependencies = keep_introduced(.dead_code.unused_optional_dependencies)
      | .dead_code.unused_enum_members         = keep_introduced(.dead_code.unused_enum_members)
      | .dead_code.unused_class_members        = keep_introduced(.dead_code.unused_class_members)
      | .dead_code.unresolved_imports          = keep_introduced(.dead_code.unresolved_imports)
      | .dead_code.unlisted_dependencies       = keep_introduced(.dead_code.unlisted_dependencies)
      | .dead_code.duplicate_exports           = keep_introduced(.dead_code.duplicate_exports)
      | .dead_code.circular_dependencies       = keep_introduced(.dead_code.circular_dependencies)
      | .dead_code.boundary_violations         = keep_introduced(.dead_code.boundary_violations)
      | .dead_code.type_only_dependencies      = keep_introduced(.dead_code.type_only_dependencies)
      | .dead_code.test_only_dependencies      = keep_introduced(.dead_code.test_only_dependencies)
      | .dead_code.stale_suppressions          = keep_introduced(.dead_code.stale_suppressions)
      | (if .complexity then .complexity.findings = keep_introduced(.complexity.findings) else . end)
      | (if .duplication then .duplication.clone_groups = keep_introduced(.duplication.clone_groups) else . end)
    ' fallow-results.json > fallow-results.tmp.json && mv fallow-results.tmp.json fallow-results.json || {
      echo "::error::Audit JSON prune transform failed"
      rm -f fallow-results.tmp.json
      exit 2
    }
  fi

  # Re-key sub-results to combined-mode keys, recompute dead-code total_issues
  # against the (possibly pruned) finding arrays, and surface verdict + attribution
  # at the top level so downstream consumers can detect the audit run.
  jq '
    def list_sum(o):
      ((o.unused_files // []) | length)
      + ((o.unused_exports // []) | length)
      + ((o.unused_types // []) | length)
      + ((o.private_type_leaks // []) | length)
      + ((o.unused_dependencies // []) | length)
      + ((o.unused_dev_dependencies // []) | length)
      + ((o.unused_optional_dependencies // []) | length)
      + ((o.unused_enum_members // []) | length)
      + ((o.unused_class_members // []) | length)
      + ((o.unresolved_imports // []) | length)
      + ((o.unlisted_dependencies // []) | length)
      + ((o.duplicate_exports // []) | length)
      + ((o.circular_dependencies // []) | length)
      + ((o.boundary_violations // []) | length)
      + ((o.type_only_dependencies // []) | length)
      + ((o.test_only_dependencies // []) | length)
      + ((o.stale_suppressions // []) | length);
    .dead_code   as $dc
    | .complexity  as $cx
    | .duplication as $du
    | .check  = ($dc | if . then . + {total_issues: list_sum(.)} else null end)
    | .dupes  = $du
    | .health = $cx
    | del(.dead_code, .complexity, .duplication)
  ' fallow-results.json > fallow-results.tmp.json && mv fallow-results.tmp.json fallow-results.json || {
    echo "::error::Audit JSON re-key transform failed"
    rm -f fallow-results.tmp.json
    exit 2
  }
fi

# --- Extract issue count ---

case "$INPUT_COMMAND" in
  dead-code|check) ISSUES=$(jq -r '.total_issues' fallow-results.json) ;;
  dupes)           ISSUES=$(jq -r '.stats.clone_groups' fallow-results.json) ;;
  health)          ISSUES=$(jq -r '((.summary.functions_above_threshold // 0) + ((.runtime_coverage.findings // []) | map(select(.verdict == "safe_to_delete" or .verdict == "review_required" or .verdict == "low_traffic")) | length))' fallow-results.json) ;;
  fix)             ISSUES=$(jq -r '(.fixes | length)' fallow-results.json) ;;
  audit)
    if [ "$GATE" = "all" ]; then
      ISSUES=$(jq -r '((.summary.dead_code_issues // 0) + (.summary.complexity_findings // 0) + (.summary.duplication_clone_groups // 0))' fallow-results.json)
    else
      ISSUES=$(jq -r '((.attribution.dead_code_introduced // 0) + (.attribution.complexity_introduced // 0) + (.attribution.duplication_introduced // 0))' fallow-results.json)
    fi
    ;;
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
    audit)           echo "::warning::Fallow audit verdict: ${VERDICT} (${ISSUES} ${GATE:-new-only} finding(s))" ;;
    fix)             echo "::warning::Fallow proposed ${ISSUES} fixes" ;;
    "")              echo "::warning::Fallow found ${ISSUES} issues" ;;
  esac
fi
