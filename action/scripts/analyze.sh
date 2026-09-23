#!/usr/bin/env bash
# Disable errexit — composite action runners inject -e via the shell
# invocation, but this script handles errors explicitly with if-guards.
set +e -o pipefail

# Run fallow analysis with CLI argument construction (deduped)
# Env contract: provided by the composite action. action.yml is the
#   authoritative input surface; each INPUT_* variable maps 1:1 to an action
#   input, plus workflow-context variables (PR_BASE_SHA, EVENT_NAME) wired in
#   this script's step env block.

artifact_path() {
  local filename=$1
  if [ "$ARTIFACTS_DIR" = "." ]; then
    printf '%s\n' "$filename"
  else
    printf '%s/%s\n' "$ARTIFACTS_DIR" "$filename"
  fi
}

# Replay a captured stderr file into the step log as ::debug:: lines, then
# remove the file. A discarded stderr is why a degraded change scope, a
# truncated envelope or a failed capability probe reaches the log with no
# cause (issues #2673, #2704, #2740). ::debug:: keeps a green run quiet: the
# lines appear only when the command wrote something, and only for a run with
# ACTIONS_STEP_DEBUG.
replay_stderr_as_debug() {
  local file=$1 label=$2
  if [ -s "$file" ]; then
    while IFS= read -r line; do
      echo "::debug::${label}: ${line}" >&2
    done < "$file"
  fi
  rm -f "$file"
}

# Run jq and keep its stderr in the step log. Every envelope read in this
# script discarded it, so a truncated or unreadable `fallow-results.json` read
# as "no findings": jq wrote the cause to stderr and the caller saw an empty
# value. Only stdout is captured by a command substitution, so the replay is
# safe inside one. jq is silent on success.
jq_debug() {
  local error_file status
  error_file=$(mktemp)
  jq "$@" 2> "$error_file"
  status=$?
  replay_stderr_as_debug "$error_file" "jq"
  return $status
}

is_dead_code_baseline_command() {
  [ -n "${INPUT_BASELINE:-}" ] || return 1
  case "${INPUT_COMMAND:-}" in
    ""|dead-code|check) return 0 ;;
    *) return 1 ;;
  esac
}

normalize_changed_path() {
  local path=$1
  local root="${INPUT_ROOT:-.}"

  path="${path#./}"
  root="${root#./}"

  if [ "$root" != "." ] && [[ "$path" == "$root/"* ]]; then
    path="${path#"$root/"}"
  fi

  printf '%s\n' "$path"
}

repo_relative_root() {
  local root="${INPUT_ROOT:-.}"
  root="${root#./}"

  if [ "$root" = "." ]; then
    printf '.\n'
    return 0
  fi

  if [[ "$root" != /* ]]; then
    printf '%s\n' "${root%/}"
    return 0
  fi

  local workspace="${GITHUB_WORKSPACE:-}"
  local abs_root
  local abs_workspace
  [ -n "$workspace" ] || return 1
  # The stderr of `cd` is discarded because it restates the test: the `return 1`
  # is the answer, and the caller warns on it.
  abs_root=$(cd "$root" 2>/dev/null && pwd -P) || return 1
  abs_workspace=$(cd "$workspace" 2>/dev/null && pwd -P) || return 1

  if [ "$abs_root" = "$abs_workspace" ]; then
    printf '.\n'
  elif [[ "$abs_root" == "$abs_workspace/"* ]]; then
    printf '%s\n' "${abs_root#"$abs_workspace/"}"
  else
    return 1
  fi
}

normalize_config_path() {
  local path=$1
  local root="${INPUT_ROOT:-.}"

  path="${path#./}"
  root="${root#./}"

  if [[ "$path" = /* ]]; then
    local abs_root
    # The stderr of `cd` is discarded because a missing root is reported by the
    # command that needs it, not by this path normalizer.
    abs_root=$(cd "${INPUT_ROOT:-.}" 2>/dev/null && pwd -P)
    if [ -n "$abs_root" ] && [[ "$path" == "$abs_root/"* ]]; then
      path="${path#"$abs_root/"}"
    fi
  elif [ "$root" != "." ] && [[ "$path" == "$root/"* ]]; then
    path="${path#"$root/"}"
  fi

  printf '%s\n' "$path"
}

find_changed_fallow_config() {
  local changed_files_json=$1
  local explicit_config=""
  local matched=""
  local root_prefix=""

  if [ -n "${INPUT_CONFIG:-}" ]; then
    explicit_config=$(normalize_config_path "$INPUT_CONFIG")
  fi
  if [ -n "${INPUT_ROOT:-}" ] && [ "$INPUT_ROOT" != "." ]; then
    root_prefix="${INPUT_ROOT#./}/"
  fi

  matched=$(printf '%s' "$changed_files_json" | jq -r \
    --arg explicit "$explicit_config" --arg root "$root_prefix" '
    .[]
    | sub("^\\./"; "")
    | if ($root != "" and startswith($root)) then ltrimstr($root) else . end
    | select(
        . == ".fallowrc.json"
        or . == ".fallowrc.jsonc"
        or . == "fallow.toml"
        or . == ".fallow.toml"
        or ($explicit != "" and . == $explicit)
      )
  ' | head -1)

  if [ -n "$matched" ]; then
    printf '%s\n' "$matched"
    return 0
  fi

  return 1
}

# --- Shared argument building functions ---
# Uses global ARGS array (avoids bash nameref compatibility issues)

# Issue-type filter flags this script added, recorded so the stale-baseline
# gate's unscoped re-run can remove exactly what the action put in.
ISSUE_TYPE_FLAGS=()

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
  if [ "${INPUT_TYPE_AWARE:-}" = "true" ]; then
    ARGS+=(--type-aware)
    if [ -n "${INPUT_TYPE_AWARE_PROJECTS:-}" ]; then
      IFS=',' read -ra TYPE_AWARE_PROJECTS <<< "$INPUT_TYPE_AWARE_PROJECTS"
      for project in "${TYPE_AWARE_PROJECTS[@]}"; do
        [ -n "$project" ] && ARGS+=(--type-aware-project "$project")
      done
    fi
    [ -n "${INPUT_TYPE_AWARE_REQUIRE:-}" ] && \
      ARGS+=(--type-aware-require "$INPUT_TYPE_AWARE_REQUIRE")
  elif [ "${INPUT_TYPE_AWARE:-}" = "false" ] && [ "${HAS_NO_TYPE_AWARE:-false}" = "true" ]; then
    # Explicit opt-out overrides a config-enabled typeAware, keeping the run
    # syntactic even though the sidecar was not provisioned (#2107). The
    # 'auto' default adds no flag: the project config drives the analysis.
    # Gated on binary support so older CLIs keep their previous behavior.
    ARGS+=(--no-type-aware)
  fi

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
        ARGS+=(--sarif-file "$SARIF_FILE")
      fi
      if [ -n "${INPUT_ISSUE_TYPES:-}" ]; then
        IFS=',' read -ra TYPES <<< "$INPUT_ISSUE_TYPES"
        for t in "${TYPES[@]}"; do
          t="$(echo "$t" | xargs)"
          ARGS+=("--${t}")
          # An issue-type filter narrows the run, which stands the
          # stale-baseline gate down. Remember the exact flags so the gate's
          # own unscoped re-run can drop them again.
          ISSUE_TYPE_FLAGS+=("--${t}")
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
      if [ -n "${INPUT_MIN_SCORE:-}" ]; then
        ARGS+=(--min-score "$INPUT_MIN_SCORE")
        # `--min-score` implies `--score`, which is a section selector: without
        # this the envelope carries the score and nothing else, and the
        # annotations, the SARIF upload and the pull-request comment all render
        # empty. Only when the caller selected no health section of their own.
        if [ "${INPUT_COMPLEXITY:-}" != "true" ] && [ "${INPUT_FILE_SCORES:-}" != "true" ] \
          && [ "${INPUT_HOTSPOTS:-}" != "true" ] && [ "${INPUT_TARGETS:-}" != "true" ]; then
          ARGS+=(--complexity)
        fi
      fi
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
    security)
      [ -n "${INPUT_SECURITY_GATE:-}" ] && ARGS+=(--gate "$INPUT_SECURITY_GATE")
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
        ARGS+=(--sarif-file "$SARIF_FILE")
      fi
      # The bare run never forwarded the threshold, so `duplication-threshold`
      # could not appear in its envelope and the input was inert on the default
      # command (issue #2681). GitLab already forwards it here.
      [ -n "${INPUT_THRESHOLD:-}" ] && ARGS+=(--dupes-threshold "$INPUT_THRESHOLD")
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
      [ -n "${INPUT_COVERAGE:-}" ] && ARGS+=(--coverage "$INPUT_COVERAGE")
      [ -n "${INPUT_COVERAGE_ROOT:-}" ] && ARGS+=(--coverage-root "$INPUT_COVERAGE_ROOT")
      ;;
  esac
}

# --- Validation ---

contains_ascii_control() {
  local LC_ALL=C
  local value=$1
  [[ "$value" =~ [[:cntrl:]] ]]
}

validate_action_scalars() {
  local changed_since="${INPUT_CHANGED_SINCE:-}"
  local diff_file="${FALLOW_DIFF_FILE:-}"

  if [ -z "$changed_since" ] && [ "${INPUT_AUTO_CHANGED_SINCE:-}" = "true" ] && \
     { [ "${EVENT_NAME:-}" = "pull_request" ] || [ "${EVENT_NAME:-}" = "pull_request_target" ]; }; then
    changed_since="${PR_BASE_SHA:-}"
  fi

  if [[ "$changed_since" = -* ]]; then
    printf '%s\n' "::error::changed-since must not begin with '-'"
    exit 2
  fi
  if contains_ascii_control "$changed_since"; then
    printf '%s\n' "::error::changed-since must not contain ASCII control characters"
    exit 2
  fi
  if contains_ascii_control "$diff_file"; then
    printf '%s\n' "::error::diff-file must not contain ASCII control characters"
    exit 2
  fi
}

validate_action_scalars

case "$INPUT_COMMAND" in
  ""|dead-code|check|dupes|health|audit|security|fix) ;;
  *) echo "::error::Invalid command: ${INPUT_COMMAND}. Must be dead-code, dupes, health, audit, security, fix, or empty (runs all)."; exit 2 ;;
esac

if [ "$INPUT_COMMAND" = "audit" ] && { [ -n "${INPUT_BASELINE:-}" ] || [ -n "${INPUT_SAVE_BASELINE:-}" ]; }; then
  echo "::error::The audit command does not support the generic baseline/save-baseline inputs. Use dead-code-baseline, health-baseline, or dupes-baseline instead."
  exit 2
fi

# `--min-score` and `--min-severity` exist on `fallow health` only, so a gate
# configured on any other command would arm nothing and pass in silence. Reject
# it up front rather than after the run.
if [ -n "${INPUT_MIN_SCORE:-}" ] && [ "$INPUT_COMMAND" != "health" ]; then
  echo "::error::The min-score input applies to command: health only, and this run is '${INPUT_COMMAND:-the combined run}'. Remove it, or set command: health."
  exit 2
fi
if [ -n "${INPUT_MIN_SEVERITY:-}" ] && [ "$INPUT_COMMAND" != "health" ]; then
  echo "::error::The min-severity input applies to command: health only, and this run is '${INPUT_COMMAND:-the combined run}'. Remove it, or set command: health."
  exit 2
fi

# `--report-only` is mutually exclusive with both health gate flags, and a user
# reaching for it through args: would otherwise get a bare CLI usage error.
if [ -n "${INPUT_MIN_SCORE:-}${INPUT_MIN_SEVERITY:-}" ] \
  && printf '%s' "${INPUT_ARGS:-}" | grep -q -- '--report-only'; then
  echo "::error::--report-only in args: cannot be combined with the min-score or min-severity inputs; the CLI rejects the pair. Drop one."
  exit 2
fi

# `fallow audit` cannot judge a whole-project baseline, and the `baseline` input
# is already rejected for it above, so the pair is unreachable through the
# inputs. It is still reachable through `args`, where it buys a green run plus a
# note this script replays as `::debug::`. Grep for it the way the
# `--report-only` check above does.
if [ "$INPUT_COMMAND" = "audit" ] && printf '%s' "${INPUT_ARGS:-}" | grep -q -- '--fail-on-stale-baseline'; then
  echo "::error::--fail-on-stale-baseline in args: cannot apply to command: audit, which analyzes only the files that changed against its base and cannot judge a whole-project baseline. Run the gate on dead-code, dupes or health."
  exit 2
fi

# The stale-baseline gate reads the analysis envelope, so it needs a baseline to
# judge and a command that reports one. Saying so here beats a silent pass.
if [ "${INPUT_FAIL_ON_STALE_BASELINE:-}" = "true" ]; then
  if [ -z "${INPUT_BASELINE:-}" ]; then
    echo "::error::fail-on-stale-baseline has no baseline to judge. Set the 'baseline' input, or turn the gate off."
    exit 2
  fi
  case "$INPUT_COMMAND" in
    fix|security)
      echo "::error::The ${INPUT_COMMAND} command reports no baseline staleness, so fail-on-stale-baseline cannot apply. Run the gate on dead-code, dupes or health."
      exit 2
      ;;
  esac
fi

# `--save-baseline` runs before the comparison, so a baseline that is re-saved
# to the path it is loaded from can never be stale and no gate on it can ever
# fire. Cheap to configure by accident, and silent without this line.
if [ -n "${INPUT_BASELINE:-}" ] && [ "${INPUT_BASELINE:-}" = "${INPUT_SAVE_BASELINE:-}" ]; then
  echo "::warning::fallow: baseline and save-baseline name the same file (${INPUT_BASELINE}). The run saves before it compares, so the baseline is rewritten from this run and can never report stale entries. Save to a different path, or drop save-baseline from the job that reads the baseline."
fi

if [ -n "${INPUT_GATE:-}" ] && [ "$INPUT_GATE" != "new-only" ] && [ "$INPUT_GATE" != "all" ]; then
  echo "::error::gate must be 'new-only' or 'all', got: ${INPUT_GATE}"; exit 2
fi
if [ -n "${INPUT_SECURITY_GATE:-}" ] && [ "$INPUT_SECURITY_GATE" != "new" ] && [ "$INPUT_SECURITY_GATE" != "newly-reachable" ]; then
  echo "::error::security-gate must be 'new' or 'newly-reachable', got: ${INPUT_SECURITY_GATE}"; exit 2
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

# The score is a 0-100 threshold, so the same numeric shape as the duplication
# one; a typo'd value would otherwise reach the CLI as a usage error late.
if [ -n "${INPUT_MIN_SCORE:-}" ] && ! [[ "$INPUT_MIN_SCORE" =~ ^[0-9]+\.?[0-9]*$ ]]; then
  echo "::error::min-score must be a number between 0 and 100, got: ${INPUT_MIN_SCORE}"
  exit 2
fi
# max-crap accepts floating-point values (e.g. 30.0, 45.5) because CRAP scores
# are non-integer. Use the same numeric regex as threshold.
if [ -n "${INPUT_MAX_CRAP:-}" ] && ! [[ "$INPUT_MAX_CRAP" =~ ^[0-9]+\.?[0-9]*$ ]]; then
  echo "::error::max-crap must be a non-negative number, got: ${INPUT_MAX_CRAP}"; exit 2
fi
if [ -n "${INPUT_LOW_TRAFFIC_THRESHOLD:-}" ] && ! [[ "$INPUT_LOW_TRAFFIC_THRESHOLD" =~ ^[0-9]+\.?[0-9]*$ ]]; then
  echo "::error::low-traffic-threshold must be a non-negative number, got: ${INPUT_LOW_TRAFFIC_THRESHOLD}"; exit 2
fi

# --- Resolve artifact paths ---

ARTIFACTS_DIR="${INPUT_ARTIFACTS_DIR:-.}"
if [ -z "$ARTIFACTS_DIR" ]; then
  ARTIFACTS_DIR="."
fi
if [[ "$ARTIFACTS_DIR" = /* ]] || [[ "$ARTIFACTS_DIR" = -* ]] || \
   [[ "$ARTIFACTS_DIR" == *$'\n'* ]] || [[ "$ARTIFACTS_DIR" == *$'\r'* ]] || \
   [[ "$ARTIFACTS_DIR" =~ (^|/)\.\.(/|$) ]]; then
  echo "::error::artifacts-dir must be a relative path inside the workspace, got: ${ARTIFACTS_DIR}"
  exit 2
fi
if ! mkdir -p "$ARTIFACTS_DIR"; then
  echo "::error::Failed to create artifacts-dir: ${ARTIFACTS_DIR}"
  exit 2
fi

RESULTS_FILE=$(artifact_path fallow-results.json)
RESULTS_RAW_FILE=$(artifact_path fallow-results-raw.json)
SCOPED_RESULTS_FILE=$(artifact_path fallow-results-scoped.json)
SARIF_FILE=$(artifact_path fallow-results.sarif)
STDERR_FILE=$(artifact_path fallow-stderr.log)
ANALYSIS_ARGS_FILE=$(artifact_path fallow-analysis-args.sh)
CHANGED_FILES_FILE=$(artifact_path fallow-changed-files.json)
AUTO_DIFF_FILE="$PWD/$(artifact_path fallow-pr.diff)"

if [ -n "${GITHUB_ENV:-}" ]; then
  printf '%s\n' \
    "FALLOW_RESULTS_FILE=${RESULTS_FILE}" \
    "FALLOW_SCOPED_RESULTS_FILE=${SCOPED_RESULTS_FILE}" \
    "FALLOW_ANALYSIS_ARGS_FILE=${ANALYSIS_ARGS_FILE}" \
    "FALLOW_CHANGED_FILES_FILE=${CHANGED_FILES_FILE}" \
    "FALLOW_SARIF_FILE=${SARIF_FILE}" \
    "FALLOW_ARTIFACTS_DIR=${ARTIFACTS_DIR}" >> "$GITHUB_ENV"
fi

# --- Check for --sarif-file support ---

HAS_SARIF_FILE=false
if { [ "$INPUT_COMMAND" = "dead-code" ] || [ "$INPUT_COMMAND" = "check" ] || [ -z "$INPUT_COMMAND" ]; }; then
  HELP_TMP=$(mktemp)
  HELP_ERR=$(mktemp)
  fallow dead-code --help > "$HELP_TMP" 2> "$HELP_ERR" || true
  replay_stderr_as_debug "$HELP_ERR" "fallow dead-code --help"
  if /usr/bin/grep -q -- '--sarif-file' "$HELP_TMP"; then
    HAS_SARIF_FILE=true
  fi
  rm -f "$HELP_TMP"
fi

# --- Check for native `fallow report` support ---
# `fallow report --from <results.json> --format github-annotations|github-summary`
# lets the annotate / summary steps re-render the saved envelope instead of the
# bundled jq. One probe covers both formats (they shipped together). The
# annotate / summary steps run in separate step processes, so the result flows
# through $GITHUB_ENV like the other analyze outputs above. On older binaries
# the probe is false, and those steps use the frozen legacy jq renderers.

HAS_NATIVE_REPORT=false
if fallow report --help > /dev/null 2>&1; then
  HAS_NATIVE_REPORT=true
fi

# --- Check for --no-type-aware support ---
# Only probed for the explicit `type-aware: false` opt-out; the flag is global
# on supporting CLIs, so any subcommand help lists it.

HAS_NO_TYPE_AWARE=false
if [ "${INPUT_TYPE_AWARE:-}" = "false" ]; then
  TYPE_AWARE_PROBE_ERR=$(mktemp)
  if fallow dead-code --help 2> "$TYPE_AWARE_PROBE_ERR" | /usr/bin/grep -q -- '--no-type-aware'; then
    HAS_NO_TYPE_AWARE=true
  fi
  replay_stderr_as_debug "$TYPE_AWARE_PROBE_ERR" "fallow dead-code --help"
fi
if [ -n "${GITHUB_ENV:-}" ]; then
  printf '%s\n' "HAS_NATIVE_REPORT=${HAS_NATIVE_REPORT}" >> "$GITHUB_ENV"
fi

# --- Auto-detect changed-since in PR context ---

AUTO_CHANGED_SINCE=false
USER_DIFF_FILE=false
[ -n "${FALLOW_DIFF_FILE:-}" ] && USER_DIFF_FILE=true

if [ -z "${INPUT_CHANGED_SINCE:-}" ] && [ "${INPUT_AUTO_CHANGED_SINCE:-}" = "true" ] && \
   { [ "${EVENT_NAME:-}" = "pull_request" ] || [ "${EVENT_NAME:-}" = "pull_request_target" ]; } && \
   [ -n "${PR_BASE_SHA:-}" ]; then
  INPUT_CHANGED_SINCE="$PR_BASE_SHA"
  AUTO_CHANGED_SINCE=true
  echo "::notice::Auto-scoping analysis to files changed since PR base (${PR_BASE_SHA:0:7})"
fi

# --- Pre-compute changed files list for downstream filtering ---
# Downstream scripts (comment, summary, annotations, review) need the list of
# changed files to scope results to the PR. On shallow clones (the default
# actions/checkout depth), git diff against the base SHA fails. We compute the
# list here once — trying git first, then the GitHub API — and save it for reuse.

# Initialize the API-failure marker unconditionally so downstream gates always
# see a definitive value (false), regardless of whether changed-since was
# requested. Without this, `if:` conditions using
# `outputs.changed_files_unavailable == 'false'` as a positive signal see an
# absent field instead of false when changed-since is not set.
if [ -n "${GITHUB_OUTPUT:-}" ]; then
  printf '%s\n' "changed_files_unavailable=false" >> "$GITHUB_OUTPUT"
fi

_CHANGED_JSON=""

if [ -n "${INPUT_CHANGED_SINCE:-}" ]; then
  _ROOT="${INPUT_ROOT:-.}"
  _CHANGED_JSON=""

  # Try three-dot diff (precise: changes since merge-base, needs full history)
  _SCOPE_ERR=$(mktemp)
  _CHANGED_JSON=$(cd "$_ROOT" && git diff --name-only -z --relative "${INPUT_CHANGED_SINCE}...HEAD" -- . 2> "$_SCOPE_ERR" | jq -Rs 'split("\u0000") | map(select(length > 0))' || true)
  replay_stderr_as_debug "$_SCOPE_ERR" "git diff --name-only"

  # Shallow clone fallback: fetch the base commit and try two-dot diff
  if ! printf '%s' "$_CHANGED_JSON" | jq -e 'length > 0' >/dev/null 2>&1; then
    # The stderr of `git cat-file -e` is discarded because it is a pure
    # existence test and the fetch below is the answer to a missing commit.
    if ! git cat-file -e "${INPUT_CHANGED_SINCE}^{commit}" 2>/dev/null; then
      _FETCH_ERR=$(mktemp)
      git fetch --depth=1 origin "$INPUT_CHANGED_SINCE" 2> "$_FETCH_ERR" || true
      replay_stderr_as_debug "$_FETCH_ERR" "git fetch"
    fi
    _SCOPE_ERR=$(mktemp)
    _CHANGED_JSON=$(cd "$_ROOT" && git diff --name-only -z --relative "${INPUT_CHANGED_SINCE}" HEAD -- . 2> "$_SCOPE_ERR" | jq -Rs 'split("\u0000") | map(select(length > 0))' || true)
    replay_stderr_as_debug "$_SCOPE_ERR" "git diff --name-only"
  fi

  # Last resort: GitHub API (works regardless of clone depth).
  # Distinguish API failure (rate limit, 5xx, expired token, missing
  # permissions) from "no PR context" (no GH_TOKEN / PR_NUMBER / GH_REPO).
  # On API failure, set `changed_files_unavailable=true` so downstream
  # workflow steps can gate on the degraded state rather than silently
  # running unscoped analysis. The existing shallow-clone warning below
  # keeps its framing for the no-API-credentials case.
  if ! printf '%s' "$_CHANGED_JSON" | jq -e 'length > 0' >/dev/null 2>&1 \
      && [ -n "${GH_TOKEN:-}" ] && [ -n "${PR_NUMBER:-}" ] && [ -n "${GH_REPO:-}" ]; then
    _API_TMP=$(mktemp)
    _API_ERR=$(mktemp)
    trap 'rm -f "$_API_TMP" "$_API_ERR"' EXIT
    if gh api --paginate "repos/${GH_REPO}/pulls/${PR_NUMBER}/files" --jq '.[].filename | @json' \
         > "$_API_TMP" 2> "$_API_ERR"; then
      _CHANGED_JSON=$(jq -s '.' "$_API_TMP")
      if printf '%s' "$_CHANGED_JSON" | jq -e 'length > 0' >/dev/null 2>&1; then
        _API_ROOT=$(repo_relative_root || true)
        if [ -z "$_API_ROOT" ]; then
          echo "::warning::fallow: absolute root is outside GITHUB_WORKSPACE; GitHub API paths cannot be scoped safely." >&2
          [ -n "${GITHUB_OUTPUT:-}" ] && printf '%s\n' "changed_files_unavailable=true" >> "$GITHUB_OUTPUT"
          _CHANGED_JSON='[]'
        elif [ "$_API_ROOT" != "." ]; then
          # Strip root prefix; API returns repo-root-relative paths, fallow JSON uses root-relative.
          _CHANGED_JSON=$(printf '%s' "$_CHANGED_JSON" | jq -c --arg prefix "${_API_ROOT%/}/" \
            'map(select(startswith($prefix)) | ltrimstr($prefix))')
        fi
      fi
    else
      _STDERR_HEAD=$(head -3 "$_API_ERR" | tr '\n' ' ')
      echo "::warning::fallow: GitHub API call to list PR files failed; analysis will run against the full codebase, not just files changed in this PR. stderr: ${_STDERR_HEAD} Re-run the job to retry. If persistent, check 'gh auth status' and repo permissions." >&2
      [ -n "${GITHUB_OUTPUT:-}" ] && printf '%s\n' "changed_files_unavailable=true" >> "$GITHUB_OUTPUT"
    fi
  fi

  if printf '%s' "$_CHANGED_JSON" | jq -e 'length > 0' >/dev/null 2>&1; then
    printf '%s\n' "$_CHANGED_JSON" > "$CHANGED_FILES_FILE"
  else
    echo "::warning::Could not determine changed files for --changed-since scoping. Use fetch-depth: 0 in actions/checkout for best results."
  fi
fi

if is_dead_code_baseline_command \
    && printf '%s' "$_CHANGED_JSON" | jq -e 'length > 0' >/dev/null 2>&1; then
  CONFIG_SCOPE_TRIGGER=$(find_changed_fallow_config "$_CHANGED_JSON" || true)
  if [ -n "$CONFIG_SCOPE_TRIGGER" ]; then
    if [ "$AUTO_CHANGED_SINCE" = "true" ]; then
      if [ "$USER_DIFF_FILE" = "true" ]; then
        echo "::warning::fallow: '${CONFIG_SCOPE_TRIGGER}' changed, so auto changed-since scoping is disabled for dead-code baseline comparison. The explicit diff file remains active and may still hide baseline drift until an unscoped run." >&2
      else
        echo "::warning::fallow: dead-code baseline comparison is running unscoped because '${CONFIG_SCOPE_TRIGGER}' changed. Fallow config can change baseline membership; downstream PR filtering is disabled for this run." >&2
      fi
      INPUT_CHANGED_SINCE=""
      rm -f "$CHANGED_FILES_FILE" "$AUTO_DIFF_FILE"
    else
      echo "::warning::fallow: '${CONFIG_SCOPE_TRIGGER}' changed while dead-code baseline comparison is explicitly scoped. Fallow config can change baseline membership, so baseline drift may stay hidden until an unscoped run." >&2
    fi
  fi
fi

# Propagate the effective changed-since value after config safety logic so
# downstream steps do not reapply stale PR scope.
if [ -n "${GITHUB_OUTPUT:-}" ]; then
  printf '%s\n' "changed_since=${INPUT_CHANGED_SINCE:-}" >> "$GITHUB_OUTPUT"
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
  printf '%s\n' "FALLOW_DIFF_FILE=${FALLOW_DIFF_FILE}" >> "$GITHUB_ENV"
fi

if [ -n "${INPUT_CHANGED_SINCE:-}" ] && [ -z "${FALLOW_DIFF_FILE:-}" ]; then
  _ROOT="${INPUT_ROOT:-.}"
  _DIFF_PATH="$AUTO_DIFF_FILE"

  # Three-dot diff (precise: changes since merge-base, needs full history).
  _DIFF_ERR=$(mktemp)
  if (cd "$_ROOT" && git diff --unified=0 --relative "${INPUT_CHANGED_SINCE}...HEAD" -- .) > "$_DIFF_PATH" 2> "$_DIFF_ERR"; then
    :
  fi
  replay_stderr_as_debug "$_DIFF_ERR" "git diff --unified=0"

  # Shallow-clone fallback: fetch the base commit, retry two-dot diff.
  if [ ! -s "$_DIFF_PATH" ]; then
    # The stderr of `git cat-file -e` is discarded because it is a pure
    # existence test and the fetch below is the answer to a missing commit.
    if ! git cat-file -e "${INPUT_CHANGED_SINCE}^{commit}" 2>/dev/null; then
      _FETCH_ERR=$(mktemp)
      git fetch --depth=1 origin "$INPUT_CHANGED_SINCE" 2> "$_FETCH_ERR" || true
      replay_stderr_as_debug "$_FETCH_ERR" "git fetch"
    fi
    _DIFF_ERR=$(mktemp)
    (cd "$_ROOT" && git diff --unified=0 --relative "${INPUT_CHANGED_SINCE}" HEAD -- .) > "$_DIFF_PATH" 2> "$_DIFF_ERR" || true
    replay_stderr_as_debug "$_DIFF_ERR" "git diff --unified=0"
  fi

  # Last resort: GitHub API. `gh pr diff` returns the same unified-diff
  # format git produces, so the downstream DiffIndex parser is identical.
  if [ ! -s "$_DIFF_PATH" ] && [ -n "${GH_TOKEN:-}" ] && [ -n "${PR_NUMBER:-}" ] && [ -n "${GH_REPO:-}" ]; then
    _GH_DIFF_ERR=$(mktemp)
    gh pr diff "$PR_NUMBER" --repo "$GH_REPO" > "$_DIFF_PATH" 2> "$_GH_DIFF_ERR" || true
    replay_stderr_as_debug "$_GH_DIFF_ERR" "gh pr diff"
  fi

  if [ -s "$_DIFF_PATH" ]; then
    export FALLOW_DIFF_FILE="$_DIFF_PATH"
    # Propagate to the comment / review render steps (separate composite
    # steps see only $GITHUB_ENV, not exported shell variables).
    if [ -n "${GITHUB_ENV:-}" ]; then
      printf '%s\n' "FALLOW_DIFF_FILE=${_DIFF_PATH}" >> "$GITHUB_ENV"
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

# Path prefixes affect only repository-facing presentation formats. Pull them
# out of the analysis invocation (whose primary format is JSON) and propagate
# the explicit override to later composite-action steps via GITHUB_ENV.
FALLOW_RENDER_PATH_PREFIX_SET=0
FALLOW_RENDER_PATH_PREFIX=""
FILTERED_EXTRA_ARGS=()
for ((i = 0; i < ${#EXTRA_ARGS[@]}; i++)); do
  arg="${EXTRA_ARGS[$i]}"
  case "$arg" in
    --report-path-prefix|--annotations-path-prefix)
      if [ $((i + 1)) -ge "${#EXTRA_ARGS[@]}" ]; then
        echo "::error::${arg} requires a prefix value"
        exit 2
      fi
      i=$((i + 1))
      FALLOW_RENDER_PATH_PREFIX="${EXTRA_ARGS[$i]}"
      FALLOW_RENDER_PATH_PREFIX_SET=1
      ;;
    --report-path-prefix=*|--annotations-path-prefix=*)
      FALLOW_RENDER_PATH_PREFIX="${arg#*=}"
      FALLOW_RENDER_PATH_PREFIX_SET=1
      ;;
    *) FILTERED_EXTRA_ARGS+=("$arg") ;;
  esac
done
EXTRA_ARGS=()
if [ "${#FILTERED_EXTRA_ARGS[@]}" -gt 0 ]; then
  EXTRA_ARGS=("${FILTERED_EXTRA_ARGS[@]}")
fi
if [ -n "${GITHUB_ENV:-}" ]; then
  # Preserve exact argv for pinned-CLI compatibility as inert runner-owned
  # data. Later steps parse this JSON instead of executing a workspace file.
  ANALYSIS_ARGS_JSON=$(jq -cn --args '$ARGS.positional' -- "${ARGS[@]}" "${EXTRA_ARGS[@]}")
  printf '%s\n' \
    "FALLOW_RENDER_PATH_PREFIX_SET=${FALLOW_RENDER_PATH_PREFIX_SET}" \
    "FALLOW_RENDER_PATH_PREFIX=${FALLOW_RENDER_PATH_PREFIX}" \
    "FALLOW_ANALYSIS_ARGS_JSON=${ANALYSIS_ARGS_JSON}" >> "$GITHUB_ENV"
fi

# Run analysis — no --fail-on-issues so subsequent steps always run.
# Bare invocations may emit an error JSON (e.g., health on a non-git repo)
# followed by the actual combined results. Use jq -s 'last' to extract only
# the final JSON object so downstream parsing sees a single valid result.
{
  printf 'FALLOW_ANALYSIS_ARGS=('
  printf '%q ' "${ARGS[@]}" "${EXTRA_ARGS[@]}"
  printf ')\n'
} > "$ANALYSIS_ARGS_FILE"

if ! fallow "${ARGS[@]}" "${EXTRA_ARGS[@]}" > "$RESULTS_RAW_FILE" 2> "$STDERR_FILE"; then
  if [ ! -s "$RESULTS_RAW_FILE" ] || ! jq -e '.' "$RESULTS_RAW_FILE" > /dev/null 2>&1; then
    echo "::error::Fallow failed to run"
    [ -s "$STDERR_FILE" ] && cat "$STDERR_FILE"
    [ -s "$RESULTS_RAW_FILE" ] && cat "$RESULTS_RAW_FILE"
    exit 2
  fi
fi
jq -s 'last' "$RESULTS_RAW_FILE" > "$RESULTS_FILE"
rm -f "$RESULTS_RAW_FILE"
if jq -e '.error == true' "$RESULTS_FILE" > /dev/null 2>&1; then
  MESSAGE=$(jq -r '.message // "Fallow failed"' "$RESULTS_FILE")
  EXIT_CODE=$(jq -r '.exit_code // 2' "$RESULTS_FILE")
  echo "::error::${MESSAGE}"
  exit "$EXIT_CODE"
fi

# --- Baseline staleness and the opt-in stale-baseline gate ---------------
# The CLI reports both on stderr only, which `--quiet` removes and which this
# script would replay as `::debug::`, so 3.26.0's warning and gate never reached
# an action user (issue #2673). Read them from the envelope instead: it carries
# `baseline_staleness` whenever a baseline was loaded, and `gate_trips` is the
# same boolean `--fail-on-stale-baseline` exits on, so the rule stays in Rust.
#
# Reading the advisory is independent of the gate. A pull-request run is scoped
# and a scoped run cannot judge a whole-project baseline, so the unscoped
# re-read below happens for any run that loaded one; the gate input only decides
# whether a stale baseline fails the job.
#
# Every branch that cannot read an answer fails OPEN. A pinned older binary, or
# a command that reports no staleness, produces a warning and a green run: a
# gate that fires because it could not read its input is worse than no gate.
# Combinations that cannot work at all are rejected earlier, at input
# validation, with exit 2.

STALE_BASELINE_GATE_FAILED=false
BASELINE_STALENESS_JQ='.baseline_staleness // .summary.baseline_staleness // .check.baseline_staleness // empty'

# Read one member of the staleness object from an envelope file. Prints nothing
# when the object or the member is absent.
#
# `section` selects which staleness object to read: empty for the single-analysis
# commands, whose object the `//` chain finds, or one section prefix for `audit`,
# which carries up to three and whose first-match chain would report one of them
# under every label.
read_staleness_field() {
  local file=$1 field=$2 section=${3:-}
  local selector="${BASELINE_STALENESS_JQ}"
  if [ -n "$section" ]; then
    selector="${section}.baseline_staleness // empty"
  fi
  # `// empty` cannot be used here: jq treats `false` as absent, which would
  # silently blank `change_scoped: false` and `gate_trips: false`.
  jq_debug -r --arg field "$field" \
    "(${selector}) | if has(\$field) then .[\$field] else empty end" \
    "$file" || true
}

# `scope_reasons` needs its own reader: it is an array, and the scalar reader
# above returns the raw jq rendering of one, which is not a log line.
read_staleness_scope_reasons() {
  local file=$1
  jq_debug -r "(${BASELINE_STALENESS_JQ}) | (.scope_reasons // []) | join(\", \")" \
    "$file" || true
}

read_all_staleness_fields() {
  local file=$1
  BASELINE_ENTRIES=$(read_staleness_field "$file" baseline_entries)
  BASELINE_MATCHED=$(read_staleness_field "$file" matched_entries)
  BASELINE_STALE_ENTRIES=$(read_staleness_field "$file" stale_entries)
  BASELINE_ADVISORY=$(read_staleness_field "$file" warning)
  BASELINE_GATE_TRIPS=$(read_staleness_field "$file" gate_trips)
  BASELINE_CHANGE_SCOPED=$(read_staleness_field "$file" change_scoped)
  BASELINE_UNRECOGNISED=$(read_staleness_field "$file" unrecognised_format)
  BASELINE_SCOPE_REASONS=$(read_staleness_scope_reasons "$file")
}

read_all_staleness_fields "$RESULTS_FILE"

# True when every channel that narrowed the run is one this script added, so
# removing them can produce a run that CAN judge the baseline. Production mode
# and workspace scoping are the user's own choice about what to analyze, are
# never removed, and make the re-read pointless.
#
# Driven by the run's own scope_reasons when the binary reports them, so
# scoping smuggled through the 'args' input is visible here instead of sending
# the script into a re-read that comes back narrowed anyway. A binary that
# predates the member falls back to the input-based guess, which is the only
# reading available there.
BASELINE_REMOVABLE_SCOPE_REASONS="diff changed-since changed-files scope file issue-type-filter"

action_can_rerun_unscoped() {
  if [ -n "${BASELINE_SCOPE_REASONS:-}" ]; then
    local reason
    for reason in $(printf '%s' "$BASELINE_SCOPE_REASONS" | tr ',' ' '); do
      case " ${BASELINE_REMOVABLE_SCOPE_REASONS} " in
        *" ${reason} "*) ;;
        *) return 1 ;;
      esac
    done
    return 0
  fi
  if [ "${INPUT_PRODUCTION:-}" = "true" ]; then return 1; fi
  if [ "${INPUT_PRODUCTION_DEAD_CODE:-}" = "true" ]; then return 1; fi
  if [ "${INPUT_PRODUCTION_HEALTH:-}" = "true" ]; then return 1; fi
  if [ "${INPUT_PRODUCTION_DUPES:-}" = "true" ]; then return 1; fi
  if [ -n "${INPUT_WORKSPACE:-}" ]; then return 1; fi
  if [ -n "${INPUT_CHANGED_WORKSPACES:-}" ]; then return 1; fi
  return 0
}

# The channels that narrowed the run, as a parenthetical for a log line. Empty
# when the binary does not report them.
baseline_scope_clause() {
  if [ -n "${BASELINE_SCOPE_REASONS:-}" ]; then
    printf ' (%s)' "$BASELINE_SCOPE_REASONS"
  fi
}

# Why a narrowed run cannot be re-read unscoped. Falls back to the two inputs
# the guess is built from, for a binary that reports no scope_reasons.
baseline_unremovable_scope_clause() {
  if [ -n "${BASELINE_SCOPE_REASONS:-}" ]; then
    printf ' (%s)' "$BASELINE_SCOPE_REASONS"
  else
    printf ' (production mode or workspace scoping)'
  fi
}

# Build the re-read's argv as an element-wise copy of the analysis argv with
# every narrowing flag and every workspace-writing flag removed. Never rebuilt
# from $INPUT_ARGS: re-splitting user input would reintroduce word splitting.
build_stale_gate_args() {
  GATE_ARGS=()
  local skip_next=false skip_next_if_value=false arg
  for arg in "${ARGS[@]}" "${EXTRA_ARGS[@]}"; do
    if [ "$skip_next" = "true" ]; then
      skip_next=false
      continue
    fi
    # `--save-snapshot` takes an optional value, so its argument is only the
    # next element when that element is not itself a flag. Skipping
    # unconditionally would eat whatever followed the bare form.
    if [ "$skip_next_if_value" = "true" ]; then
      skip_next_if_value=false
      case "$arg" in
        --*) ;;
        *) continue ;;
      esac
    fi
    case "$arg" in
      # Narrowing channels the action added.
      --changed-since|--scope|--file)
        skip_next=true
        continue
        ;;
      --changed-since=*|--scope=*|--file=*)
        continue
        ;;
      # Writing flags: a re-read must not rewrite a baseline, a regression
      # baseline, the uploaded SARIF, or apply fixes.
      --save-baseline|--save-regression-baseline|--sarif-file)
        skip_next=true
        continue
        ;;
      --save-baseline=*|--save-regression-baseline=*|--sarif-file=*|--save-snapshot=*)
        continue
        ;;
      --save-snapshot)
        skip_next_if_value=true
        continue
        ;;
      # `--min-score` is a section selector, so a score-only envelope may carry
      # no `baseline_staleness` and the #2674 gate would go silent with no
      # message at all. Stripping is free: this run's status is discarded and
      # only the staleness object is read from it. `--complexity` rides along
      # because the action only added it to keep that envelope populated.
      --min-score)
        skip_next=true
        continue
        ;;
      --min-score=*|--complexity)
        continue
        ;;
      --fail-on-regression|--yes)
        continue
        ;;
    esac
    local dropped=false flag
    for flag in ${ISSUE_TYPE_FLAGS[@]+"${ISSUE_TYPE_FLAGS[@]}"}; do
      if [ "$arg" = "$flag" ]; then
        dropped=true
        break
      fi
    done
    if [ "$dropped" = "true" ]; then
      continue
    fi
    GATE_ARGS+=("$arg")
  done
}

# Re-read the baseline over the whole project so the advisory and the gate have
# something they can judge. Report-discarding: its envelope feeds nothing but
# the staleness read.
run_stale_gate_analysis() {
  build_stale_gate_args
  local started ended
  started=$SECONDS
  echo "fallow: re-reading the baseline over the whole project, which a scoped run cannot judge" >&2
  # This run exits 1 on findings, which is not an error here, so its status is
  # discarded and the file is validated instead. FALLOW_DIFF_FILE is cleared
  # because diff scoping reaches the CLI through the environment, not argv.
  env -u FALLOW_DIFF_FILE fallow "${GATE_ARGS[@]}" \
    > "$GATE_RESULTS_RAW_FILE" 2> "$GATE_STDERR_FILE" || true
  ended=$SECONDS
  echo "fallow: unscoped baseline re-read finished in $((ended - started))s" >&2
  if [ -s "$GATE_STDERR_FILE" ]; then
    while IFS= read -r line; do
      echo "::debug::baseline re-read: ${line}"
    done < "$GATE_STDERR_FILE"
  fi
  if [ ! -s "$GATE_RESULTS_RAW_FILE" ] || ! jq -e '.' "$GATE_RESULTS_RAW_FILE" > /dev/null 2>&1; then
    return 1
  fi
  jq_debug -s 'last' "$GATE_RESULTS_RAW_FILE" > "$GATE_RESULTS_FILE" || return 1
  if jq -e '.error == true' "$GATE_RESULTS_FILE" > /dev/null 2>&1; then
    return 1
  fi
  return 0
}

# Name the gate only when it was asked for, so a run that wanted no gate does
# not read as if one failed.
# A run that asked for the gate and did not get one has a problem worth a
# warning. A run that asked for nothing does not: production mode plus a
# baseline is an ordinary configuration, and warning on every pull request about
# a judgement nobody requested is noise the repository cannot turn off. The CLI
# itself is silent there, so the notice level matches it.
stale_baseline_stand_down() {
  local reason=$1 remedy=$2
  if [ "${INPUT_FAIL_ON_STALE_BASELINE:-}" = "true" ]; then
    echo "::warning::fallow: baseline staleness could not be judged on this run because ${reason}. fail-on-stale-baseline stood down. ${remedy}"
  else
    echo "::notice::fallow: baseline staleness could not be judged on this run because ${reason}. ${remedy}"
  fi
}

if [ -n "${INPUT_BASELINE:-}" ] && [ -z "$BASELINE_ENTRIES" ]; then
  if [ "${INPUT_FAIL_ON_STALE_BASELINE:-}" = "true" ]; then
    stale_baseline_stand_down "it reported no baseline staleness" "A fallow that predates this feature cannot report it: pin a current version, or run the gate on dead-code, dupes or health."
  fi
elif [ "$BASELINE_CHANGE_SCOPED" = "true" ]; then
  if action_can_rerun_unscoped; then
    GATE_RESULTS_RAW_FILE="${ARTIFACTS_DIR}/fallow-stale-baseline-gate-raw.json"
    GATE_RESULTS_FILE="${ARTIFACTS_DIR}/fallow-stale-baseline-gate.json"
    GATE_STDERR_FILE="${ARTIFACTS_DIR}/fallow-stale-baseline-gate-stderr.log"
    if run_stale_gate_analysis; then
      read_all_staleness_fields "$GATE_RESULTS_FILE"
      if [ "$BASELINE_CHANGE_SCOPED" = "true" ]; then
        stale_baseline_stand_down "the unscoped re-read was still narrowed to part of the project$(baseline_scope_clause)" "Remove the positional path from the 'args' input to judge the baseline."
      fi
    else
      stale_baseline_stand_down "the unscoped baseline re-read produced no readable result" "The primary analysis is unaffected; the step debug log carries its stderr."
    fi
    rm -f "$GATE_RESULTS_RAW_FILE" "$GATE_RESULTS_FILE" "$GATE_STDERR_FILE"
  else
    stale_baseline_stand_down "it analyzed only part of the project$(baseline_unremovable_scope_clause)" "Run an unscoped job to judge the baseline."
  fi
fi

# `fallow audit` loads up to three baselines and judges none of them: every
# audit narrows to the files that changed against its base, so a whole-project
# baseline matches less of the run for reasons that are not rot. It says so once
# on stderr, which `--quiet` removes and this script replays as `::debug::`, so
# an audit user never learned that the baseline they pass is inert (issue
# #2677).
#
# Read each section separately rather than lengthening the single-analysis `//`
# chain: that chain is first-match, so an audit with three baselines would
# report one of them and hide the other two. One notice per object found, and
# the single-analysis step outputs stay bound to their own read, because
# overloading them would make `baseline-stale-entries` mean a different baseline
# from one run to the next.
#
# A notice, not a warning: nobody asked for a judgement here, and the CLI itself
# is silent unless the gate flag was passed. The unreachable-combination check
# at input validation already rejects `command: audit` with the gate.
audit_baseline_notices() {
  local file=$1 row label command input section entries unrecognised path
  # label:jq-prefix:command:input-variable. The label names the envelope
  # section a reader goes looking in; the command is what they have to run, and
  # the two differ:
  # `duplication` is served by `fallow dupes` and `complexity` by
  # `fallow health`.
  for row in \
    'dead-code:.dead_code:dead-code:INPUT_DEAD_CODE_BASELINE' \
    'duplication:.duplication:dupes:INPUT_DUPES_BASELINE' \
    'complexity:.complexity.summary:health:INPUT_HEALTH_BASELINE'
  do
    label=${row%%:*}
    section=$(printf '%s' "$row" | cut -d: -f2)
    command=$(printf '%s' "$row" | cut -d: -f3)
    input=${row##*:}
    # Through the shared reader, so this loop reads a member the same way the
    # single-analysis path does. The reader guards with `has`, which keeps a
    # literal `false` distinct from an absent member. The inline `// empty` it
    # replaces collapsed the two. No consumer here saw a difference, because each
    # one compares the value against the string `true`.
    entries=$(read_staleness_field "$file" baseline_entries "$section")
    # Absent means that baseline was never loaded, which is not worth a line.
    if [ -z "$entries" ]; then
      continue
    fi
    unrecognised=$(read_staleness_field "$file" unrecognised_format "$section")
    # Audit resolves all three from project config as well as from inputs, so
    # there is not always a path to echo back.
    path=$(eval "printf '%s' \"\${${input}:-}\"")
    if [ "$unrecognised" = "true" ]; then
      if [ -n "$path" ]; then
        echo "::warning::fallow: the ${label} baseline at ${path} has no entries this command recognises. It may be a baseline saved by another command, or an empty file. Either way it suppresses nothing."
      else
        echo "::warning::fallow: the ${label} baseline has no entries this command recognises. It may be a baseline saved by another command, or an empty file. Either way it suppresses nothing."
      fi
      continue
    fi
    if [ -n "$path" ]; then
      echo "::notice::fallow: the ${label} baseline (${path}) has ${entries} entries and was not judged on this run: fallow audit analyzes only the files that changed against its base. Run 'fallow ${command} --baseline ${path}' over the whole project to check it."
    else
      echo "::notice::fallow: the ${label} baseline has ${entries} entries and was not judged on this run: fallow audit analyzes only the files that changed against its base. Run 'fallow ${command}' with that baseline over the whole project to check it."
    fi
  done
}

if [ "$INPUT_COMMAND" = "audit" ]; then
  audit_baseline_notices "$RESULTS_FILE"
fi

# A baseline written by another command suppresses nothing, so the counts below
# are all zero and read exactly like a baseline saved on a project that had
# nothing to record. A repository that pointed `baseline` at the wrong file
# would otherwise gate on it forever. Distinct from the `-z` branch above, which
# means the run reported no staleness at all.
#
# Ahead of the advisory rather than beside it: the binary now trips the gate on
# such a file, so the `*)` arm below would add "0 of 0 baseline entries matched
# nothing this run" next to the line that says what is actually wrong.
#
# Keyed on the binary's own verdict rather than on a zero entry count, which a
# baseline saved on a green main with nothing to record carries too: warning on
# every run about a correctly saved baseline is noise the repository cannot turn
# off. Not gated on the `baseline` input either, so a baseline passed through
# `args` earns the same line; the path is named only when this script knows it.
#
# The advisory and the gate answer different questions and legitimately
# disagree, so warn on either. A rotted baseline on a project with nothing left
# to report is `warning: none` with `gate_trips: true`, and that is exactly the
# case issue #2673 was filed about.
if [ "${BASELINE_UNRECOGNISED:-}" = "true" ]; then
  if [ -n "${INPUT_BASELINE:-}" ]; then
    echo "::warning::fallow: the baseline at ${INPUT_BASELINE} has no entries this command recognises. It may be a baseline saved by another command, or an empty file. Either way it suppresses nothing."
  else
    echo "::warning::fallow: the loaded baseline has no entries this command recognises. It may be a baseline saved by another command, or an empty file. Either way it suppresses nothing."
  fi
elif [ -n "$BASELINE_ENTRIES" ]; then
  case "$BASELINE_ADVISORY" in
    partial)
      echo "::warning::fallow: baseline is partially stale: ${BASELINE_STALE_ENTRIES} of ${BASELINE_ENTRIES} entries matched nothing this run, so it protects less than what was saved. Re-save it with the save-baseline input."
      ;;
    zero-overlap)
      echo "::warning::fallow: baseline has ${BASELINE_ENTRIES} entries but matched nothing this run. Paths may have changed, or the baseline was saved elsewhere. Re-save it with the save-baseline input."
      ;;
    *)
      if [ "$BASELINE_GATE_TRIPS" = "true" ]; then
        echo "::warning::fallow: ${BASELINE_STALE_ENTRIES} of ${BASELINE_ENTRIES} baseline entries matched nothing this run. The project may be clean, or the baseline may no longer describe it. Re-save it with the save-baseline input."
      fi
      ;;
  esac
fi

if [ "${INPUT_FAIL_ON_STALE_BASELINE:-}" = "true" ] && [ "$BASELINE_GATE_TRIPS" = "true" ]; then
  STALE_BASELINE_GATE_FAILED=true
fi

# A findings exit code can still carry a valid JSON envelope, so the analysis
# command above cannot rely on its exit status alone. Record the effective
# semantic completeness gate now, then fail only after artifacts and outputs
# have been published for downstream action steps.
TYPE_AWARE_COMPLETENESS_FAILED=false
if ! jq -e --arg requested "${INPUT_TYPE_AWARE_REQUIRE:-}" '
    (
      ._meta.type_aware
      // ._meta.check.type_aware
      // .check._meta.type_aware
      // .dead_code._meta.type_aware
      // null
    ) as $type_aware
    | (($type_aware.required_completeness // $requested) != "complete")
      or (
        ($type_aware != null)
        and
        ($type_aware.identity.completeness == "complete")
        and ([ $type_aware.queries[]? | select(.status != "complete") ] | length == 0)
      )
  ' "$RESULTS_FILE" > /dev/null 2>&1; then
  TYPE_AWARE_COMPLETENESS_FAILED=true
fi

# --- Gate verdicts (issues #2680, #2681, #2683, #2685) ---
#
# Every gate the run armed publishes `status` and `enforced` in
# `gate_outcomes` at the envelope root, computed by the same Rust rule that
# decides the exit code. The action reads that instead of the process status,
# which it deliberately discards whenever stdout parses as JSON.
#
# A gate fails the job only when all three hold: the input that owns it asked
# for it, its status is `fail`, and the CLI marked it `enforced`. The first
# condition is what keeps `fail-on-issues: false` authoritative: a flag that
# arrived through `args:` produces a warning, never a failure.
GATE_FAILURES=()
GATE_FAILED_NAMES=()
GATE_WARNED_NAMES=()
GATE_SKIPPED_NAMES=()
GATE_PASSED_NAMES=()
SECURITY_GATE_FAILED=false

HAS_GATE_OUTCOMES=false
if jq -e 'has("gate_outcomes")' "$RESULTS_FILE" > /dev/null 2>&1; then
  HAS_GATE_OUTCOMES=true
fi

# A gate name reaches both `$GITHUB_OUTPUT` and a workflow command, so anything
# that is not a plain kebab-case identifier is dropped rather than echoed.
gate_name_is_safe() {
  case "$1" in
    *[!a-z0-9-]*) return 1 ;;
    "") return 1 ;;
    *) return 0 ;;
  esac
}

# `has()` rather than `// empty`: jq treats a `false` value as absent under the
# alternative operator, which would blank every `enforced: false`.
read_gate_member() {
  jq_debug -r --arg gate "$1" --arg member "$2" '
    (.gate_outcomes // {}) as $gates
    | if ($gates | has($gate)) and ($gates[$gate] | has($member))
      then ($gates[$gate][$member] | tostring)
      else "" end
  ' "$RESULTS_FILE" || true
}

# Which input owns which gate. A gate with no owning input set is reported and
# never fails the job.
gate_input_value() {
  case "$1" in
    regression)            printf '%s' "${INPUT_FAIL_ON_REGRESSION:-}" ;;
    duplication-threshold) printf '%s' "${INPUT_THRESHOLD:-}" ;;
    health-min-severity)   printf '%s' "${INPUT_MIN_SEVERITY:-}" ;;
    health-min-score)      printf '%s' "${INPUT_MIN_SCORE:-}" ;;
    security)              printf '%s' "${INPUT_SECURITY_GATE:-}" ;;
    stale-baseline)        printf '%s' "${INPUT_FAIL_ON_STALE_BASELINE:-}" ;;
    type-aware-require)    printf '%s' "${INPUT_TYPE_AWARE_REQUIRE:-}" ;;
    *)                     printf '%s' "" ;;
  esac
}

gate_is_owned() {
  local value
  value=$(gate_input_value "$1")
  # `false` and `0` are the documented off switches for the boolean inputs, so
  # a gate whose input is explicitly disabled is not owned either.
  case "$value" in
    ""|false|0) return 1 ;;
    *) return 0 ;;
  esac
}

# The envelope carries these as JSON numbers, so a whole value arrives as
# "3.0". Trim it for prose; the wire keeps the number.
trim_gate_number() {
  case "$1" in
    *.0) printf '%s' "${1%.0}" ;;
    *) printf '%s' "$1" ;;
  esac
}

gate_detail() {
  case "$1" in
    regression)
      local baseline current delta
      baseline=$(jq_debug -r '(.regression.baseline_total // .check.regression.baseline_total // "") | tostring' "$RESULTS_FILE" || true)
      current=$(jq_debug -r '(.regression.current_total // .check.regression.current_total // "") | tostring' "$RESULTS_FILE" || true)
      delta=$(jq_debug -r '(.regression.delta // .check.regression.delta // "") | tostring' "$RESULTS_FILE" || true)
      if [ -n "$delta" ]; then
        printf 'issue count rose from %s to %s (delta %s, tolerance %s)' \
          "${baseline:-?}" "${current:-?}" "$delta" "${INPUT_TOLERANCE:-0}"
      fi
      ;;
    duplication-threshold)
      local observed threshold
      observed=$(read_gate_member duplication-threshold observed)
      threshold=$(read_gate_member duplication-threshold threshold)
      [ -n "$observed" ] && printf 'duplication %s%% exceeds the %s%% threshold' "$(trim_gate_number "$observed")" "$(trim_gate_number "$threshold")"
      ;;
    health-min-score)
      local observed threshold
      observed=$(read_gate_member health-min-score observed)
      threshold=$(read_gate_member health-min-score threshold)
      [ -n "$observed" ] && printf 'health score %s is below the minimum %s' "$(trim_gate_number "$observed")" "$(trim_gate_number "$threshold")"
      ;;
    health-min-severity)
      local observed floor
      observed=$(read_gate_member health-min-severity observed)
      floor=$(read_gate_member health-min-severity threshold_label)
      [ -n "$observed" ] && printf '%s finding(s) at or above %s' "$(trim_gate_number "$observed")" "${floor:-${INPUT_MIN_SEVERITY:-the configured floor}}"
      ;;
    security)
      local new_count
      new_count=$(jq_debug -r '(.gate.new_count // "") | tostring' "$RESULTS_FILE" || true)
      [ -n "$new_count" ] && printf '%s new security candidate(s) on changed lines (gate: %s)' "$new_count" "${INPUT_SECURITY_GATE:-}"
      ;;
    stale-baseline)
      printf '%s of %s entries in %s matched nothing this run' \
        "${BASELINE_STALE_ENTRIES:-?}" "${BASELINE_ENTRIES:-?}" "${INPUT_BASELINE:-the baseline}"
      ;;
    type-aware-require)
      printf 'semantic analysis was unavailable or partial'
      ;;
  esac
  # An empty detail must not make this function return non-zero: the case
  # branches end in `&&` lists, and the caller assigns the result under
  # errexit.
  return 0
}

gate_remedy() {
  case "$1" in
    regression)            printf 'Re-save the regression baseline, raise tolerance, or set fail-on-regression: false.' ;;
    duplication-threshold) printf 'Reduce duplication or raise the threshold input.' ;;
    health-min-score)      printf 'Improve the health score or lower the min-score input.' ;;
    health-min-severity)   printf 'Fix the findings or raise the min-severity input.' ;;
    security)              printf 'Review the introduced candidates, or unset security-gate.' ;;
    stale-baseline)        printf 'Re-save the baseline, or set fail-on-stale-baseline: false.' ;;
    type-aware-require)    printf 'Install the semantic sidecar, or set type-aware-require: best-effort.' ;;
  esac
}

# Comma-separated, and empty rather than a stray comma when nothing qualified.
join_gate_names() {
  local out=""
  for name in "$@"; do
    [ -z "$name" ] && continue
    out="${out:+${out},}${name}"
  done
  printf '%s' "$out"
}

record_gate_failure() {
  local gate=$1 detail remedy line
  # The two gates that shipped before the index keep their exact wording: both
  # are user-facing strings a repository may already match on.
  case "$gate" in
    stale-baseline)
      # The gate also trips on a file this command cannot read as its own, whose
      # counts are all zero: re-saving is not the remedy there, and "0 of 0
      # entries matched nothing" names nothing the reader can act on.
      if [ "${BASELINE_UNRECOGNISED:-}" = "true" ]; then
        GATE_FAILURES+=("Fallow baseline gate failed: the baseline ${INPUT_BASELINE:-passed to this run} has no entries this command recognises, so it suppresses nothing. Point the baseline input at this command's own baseline, or set fail-on-stale-baseline: false.")
        return
      fi
      GATE_FAILURES+=("Fallow baseline gate failed: ${BASELINE_STALE_ENTRIES} of ${BASELINE_ENTRIES} entries in ${INPUT_BASELINE} matched nothing this run. Re-save the baseline, or set fail-on-stale-baseline: false.")
      return
      ;;
    type-aware-require)
      GATE_FAILURES+=("Type-aware completeness gate failed because semantic analysis was unavailable or partial.")
      return
      ;;
  esac
  detail=$(gate_detail "$gate")
  remedy=$(gate_remedy "$gate")
  line="Fallow ${gate} gate failed"
  [ -n "$detail" ] && line="${line}: ${detail}"
  line="${line}."
  [ -n "$remedy" ] && line="${line} ${remedy}"
  GATE_FAILURES+=("$line")
  if [ "$gate" = "security" ]; then
    SECURITY_GATE_FAILED=true
  fi
}

# Classify every gate the envelope reports. `skipped` is neither a pass nor a
# failure: the gate stood down (a change-scoped baseline, `--report-only`, a
# security advisory shadowed by a configured gate), and #2674 established that
# a gate a repository asked for and did not get is worth a warning.
classify_gate() {
  local gate=$1 status=$2 enforced=$3
  case "$status" in
    fail)
      GATE_FAILED_NAMES+=("$gate")
      if gate_is_owned "$gate" && [ "$enforced" = "true" ]; then
        record_gate_failure "$gate"
      elif [ "$gate" = "error-severity-findings" ] || [ "$gate" = "health-findings" ] || [ "$gate" = "audit-verdict" ]; then
        # All three are default exit rules, governed by fail-on-issues rather
        # than by an input of their own, and every envelope carries them.
        # `error-severity-findings` and `health-findings` are the CLI's own
        # findings rules, which the action's count gate deliberately does not
        # follow; `audit-verdict` is already applied by the count gate below,
        # and an audit job with fail-on-issues: false is a deliberate reporting
        # configuration. All three are reported in the outputs and never in
        # the log.
        :
      elif gate_is_owned "$gate"; then
        # The input asked for the gate, and the CLI still reports the verdict
        # as unenforced. That is the CLI saying this run could not have exited
        # on it: `health --report-only` clamps every gate, and combined mode
        # collapses every gate but the baseline and regression ones. Honour it,
        # and say which it was rather than blaming the input.
        local detail
        detail=$(gate_detail "$gate")
        echo "::warning::Fallow ${gate} gate reports a failure${detail:+: ${detail}}. It does not fail this job: this run does not enforce that gate (combined mode and --report-only both report without enforcing). Run the dedicated command to gate on it."
      else
        local detail
        detail=$(gate_detail "$gate")
        echo "::warning::Fallow ${gate} gate reports a failure${detail:+: ${detail}}. It does not fail this job, because its input is not set."
      fi
      ;;
    warn)
      GATE_WARNED_NAMES+=("$gate")
      echo "::warning::Fallow ${gate} gate reports a warning."
      ;;
    skipped)
      GATE_SKIPPED_NAMES+=("$gate")
      if gate_is_owned "$gate"; then
        echo "::warning::Fallow ${gate} gate stood down, so the run it was asked to judge was not judged."
      else
        echo "::notice::Fallow ${gate} gate stood down."
      fi
      ;;
    pass)
      GATE_PASSED_NAMES+=("$gate")
      ;;
    *)
      # The status set is open. A value this build does not recognise is
      # reported rather than silently counted as a pass, matching what
      # `fallow report` does with the same envelope.
      GATE_WARNED_NAMES+=("$gate")
      echo "::warning::Fallow ${gate} gate reported an unrecognised status '${status}'. Upgrade the action, or read gate_outcomes directly."
      ;;
  esac
}

if [ "$HAS_GATE_OUTCOMES" = "true" ]; then
  while IFS= read -r gate_entry; do
    [ -z "$gate_entry" ] && continue
    gate_key=${gate_entry%% *}
    gate_name_is_safe "$gate_key" || continue
    # The stale-baseline gate is owned by the #2674 machinery below, which
    # judges the unscoped re-read rather than this envelope. On a pull request
    # the primary run is change-scoped and reports `skipped`, so classifying it
    # here would print a stand-down beside that block's own error and list the
    # gate in both gates_skipped and gates_failed.
    if [ "$gate_key" = "stale-baseline" ] && [ -n "${INPUT_BASELINE:-}" ]; then
      continue
    fi
    classify_gate "$gate_key" "$(read_gate_member "$gate_key" status)" "$(read_gate_member "$gate_key" enforced)"
  done < <(jq_debug -r '(.gate_outcomes // {}) | keys[]?' "$RESULTS_FILE" || true)
else
  # A pinned binary older than the gate index. Read the feature-local field each
  # gate already published, and fail OPEN for the three that never had one. The
  # fallback warning is scoped to gates whose input was actually set, so a
  # pinned user configuring nothing sees nothing.
  FALLBACK_UNAVAILABLE=()
  if gate_is_owned regression; then
    if jq -e '(.regression.exceeded // .check.regression.exceeded) == true' "$RESULTS_FILE" > /dev/null 2>&1; then
      classify_gate regression fail true
    fi
  fi
  if gate_is_owned security; then
    if jq -e '.gate.verdict == "fail"' "$RESULTS_FILE" > /dev/null 2>&1; then
      classify_gate security fail true
    fi
  fi
  for fallback_gate in duplication-threshold health-min-score health-min-severity; do
    if gate_is_owned "$fallback_gate"; then
      FALLBACK_UNAVAILABLE+=("$fallback_gate")
    fi
  done
  if [ ${#FALLBACK_UNAVAILABLE[@]} -gt 0 ]; then
    echo "::warning::Fallow did not publish gate verdicts, so $(IFS=', '; echo "${FALLBACK_UNAVAILABLE[*]}") could not be checked. Upgrade the version input to 3.27.0 or later."
  fi
fi

# The stale-baseline gate keeps its own #2674 derivation, because on a pull
# request it reads the unscoped re-read's envelope rather than the primary one.
if [ "$STALE_BASELINE_GATE_FAILED" = "true" ]; then
  case " ${GATE_FAILED_NAMES[*]:-} " in
    *" stale-baseline "*) ;;
    *) GATE_FAILED_NAMES+=("stale-baseline") ;;
  esac
  gate_already_recorded=false
  for existing in "${GATE_FAILURES[@]:-}"; do
    case "$existing" in
      "Fallow baseline gate failed"*) gate_already_recorded=true ;;
    esac
  done
  if [ "$gate_already_recorded" = "false" ]; then
    record_gate_failure stale-baseline
  fi
fi

if [ "$TYPE_AWARE_COMPLETENESS_FAILED" = "true" ]; then
  case " ${GATE_FAILED_NAMES[*]:-} " in
    *" type-aware-require "*) ;;
    *)
      GATE_FAILED_NAMES+=("type-aware-require")
      record_gate_failure type-aware-require
      ;;
  esac
fi

# --- Degraded analysis (issue #2686) ---
#
# One aggregated warning rather than one per kind: GitHub caps annotations at
# ten per level per step, and thirteen kinds would silently drop the tail while
# competing with the baseline advisory for the same budget.
ANALYSIS_DEGRADED=false
EMPTY_ANALYSIS=false
DEGRADED_SUMMARY=$(jq_debug -r '
  [ (.workspace_diagnostics // .dead_code.workspace_diagnostics // [])[] | select(.degrades_analysis == true) ]
  | group_by(.kind)
  | map("\(.[0].kind) (\(length))")
  | join(", ")
' "$RESULTS_FILE" || true)
if [ -n "$DEGRADED_SUMMARY" ]; then
  ANALYSIS_DEGRADED=true
  echo "::warning::Fallow ran with degraded inputs: ${DEGRADED_SUMMARY}. Some findings or scores were computed over less than the whole project, or from an input that did not load."
fi
# --- Requests the run could not apply (issues #2687, #2688) ---
#
# The CLI writes this to stderr too, and this step replays stderr as
# ::debug:: (see below), which nobody reads without ACTIONS_STEP_DEBUG. The
# envelope is the channel that survives `--quiet --format json`, which is how
# this step always invokes fallow.
#
# One aggregated warning, for the same annotation-budget reason as the
# degraded-analysis block above. Honoured requests are deliberately not named:
# the interesting fact is a report that is wider than what was asked for.
#
# Selected on `affects == "scope"`, never on a name list. The object also
# carries requests that produce a file beside the report (`sarif-file`), whose
# failure says nothing about the report's scope; warning "the findings below
# cover more of the project" for one of those states the opposite of what
# happened, and the SARIF-absence warning below already owns that case. A
# request name added in a later release carries its own class, so this selector
# keeps saying the right thing about it.
REQUESTS_UNAPPLIED=$(jq_debug -r '
  [ (.request_outcomes // {}) | to_entries[]
    | select(.value.status != "applied" and .value.affects == "scope")
    | if .value.reason then "\(.key) (\(.value.reason))" else .key end ]
  | join(", ")
' "$RESULTS_FILE" || true)
if [ -n "$REQUESTS_UNAPPLIED" ]; then
  echo "::warning::Fallow could not apply: ${REQUESTS_UNAPPLIED}. The findings below cover more of the project than was requested, so do not read this run as scoped to the change."
fi
# The opposite shape, and the one a green report cannot state for itself: a
# narrowing request that DID apply, over a scope it measured as empty. Every
# finding then filters out, so the clean report below covered nothing (issue
# #2734). Keyed on `scope_size == 0` beside `status == "applied"`, so a binary
# that publishes no such member says nothing here.
REQUESTS_EMPTY_SCOPE=$(jq_debug -r '
  [ (.request_outcomes // {}) | to_entries[]
    | select(.value.status == "applied" and .value.affects == "scope" and .value.scope_size == 0)
    | .key ]
  | join(", ")
' "$RESULTS_FILE" || true)
if [ -n "$REQUESTS_EMPTY_SCOPE" ]; then
  echo "::warning::Fallow applied ${REQUESTS_EMPTY_SCOPE} over an empty scope, so no finding could survive it and the report below is clean because nothing was analyzable. Check the diff or ref this run was given before reading it as a clean result."
fi

if jq -e '[ (.workspace_diagnostics // .dead_code.workspace_diagnostics // [])[] | select(.kind == "no-source-files-analyzed") ] | length > 0' "$RESULTS_FILE" > /dev/null 2>&1; then
  EMPTY_ANALYSIS=true
  EMPTY_ANALYSIS_MESSAGE="Fallow analyzed no source file at all, so every count this run reports is zero because nothing was measured, not because the project is clean. Check the analysis root, ignorePatterns, and any path or workspace filter."
  if [ "${INPUT_FAIL_ON_EMPTY_ANALYSIS:-}" = "true" ]; then
    GATE_FAILURES+=("$EMPTY_ANALYSIS_MESSAGE")
  else
    echo "::warning::${EMPTY_ANALYSIS_MESSAGE} Set fail-on-empty-analysis: true to fail the job on this."
  fi
fi

# --- Analyze-once SARIF generation ---

valid_sarif() {
  [ -s "$1" ] && jq -e '
    .version == "2.1.0"
    and (.runs | type == "array")
    and all(.runs[]?; ((.results // []) | type == "array"))
  ' "$1" > /dev/null 2>&1
}

if { [ "${INPUT_FORMAT:-}" = "sarif" ] || [ "${INPUT_SARIF:-}" = "true" ]; } && \
   [ "$INPUT_COMMAND" != "fix" ] && \
   ! valid_sarif "$SARIF_FILE"; then
  # Render the saved JSON envelope instead of running semantic analysis again.
  # A valid SARIF run with zero results is required to clear stale alerts.
  if [ "$HAS_NATIVE_REPORT" = "true" ]; then
    REPORT_ARGS=(report --from "$RESULTS_FILE" --root "$INPUT_ROOT" --quiet --format sarif)
    [ -n "${INPUT_CONFIG:-}" ] && REPORT_ARGS+=(--config "$INPUT_CONFIG")
    # Appended rather than discarded: the re-render is the last chance to
    # produce the artefact, so the reason it failed is the only useful thing
    # left. The stderr replay below picks it up (issue #2690).
    fallow "${REPORT_ARGS[@]}" > "$SARIF_FILE" 2>> "$STDERR_FILE" || true
  fi
  if ! valid_sarif "$SARIF_FILE"; then
    # Compatibility path for pinned binaries that either lack `report` or
    # support `report` without saved-envelope SARIF rendering.
    SARIF_ARGS=()
    skip_next=false
    for ((index = 0; index < ${#ARGS[@]}; index++)); do
      arg=${ARGS[$index]}
      if [ "$skip_next" = "true" ]; then
        skip_next=false
        continue
      fi
      if [ "$arg" = "--sarif-file" ]; then
        skip_next=true
        continue
      fi
      SARIF_ARGS+=("$arg")
      if [ "$arg" = "--format" ]; then
        index=$((index + 1))
        SARIF_ARGS+=("sarif")
      fi
    done
    fallow "${SARIF_ARGS[@]}" "${EXTRA_ARGS[@]}" > "$SARIF_FILE" 2>> "$STDERR_FILE" || true
  fi
  if ! valid_sarif "$SARIF_FILE"; then
    # A missing artefact keeps the step green and uploads nothing, so code
    # scanning silently stops receiving alerts. Driven by file absence rather
    # than by the envelope, so it also fires for a pinned older binary that
    # publishes no `request_outcomes` (issue #2690).
    # Root only: `--sarif-file` is rejected for command: audit, which is the one
    # envelope with a nested dead_code section, so there is no second carrier.
    SARIF_FILE_REASON=$(jq_debug -r '
      (.request_outcomes // {})["sarif-file"]
      | if . == null or .status == "applied" then empty else (.message // .reason) end
    ' "$RESULTS_FILE" || true)
    echo "::warning::Fallow produced no SARIF document, so this run uploads nothing and code scanning keeps the alerts from the previous upload.${SARIF_FILE_REASON:+ ${SARIF_FILE_REASON}} Check the earlier log lines for the cause, or drop format: sarif if code scanning is not wanted."
    rm -f "$SARIF_FILE"
  fi
fi

# Never expose a malformed renderer artifact to the upload step.
if [ -f "$SARIF_FILE" ] && ! valid_sarif "$SARIF_FILE"; then
  rm -f "$SARIF_FILE"
fi

# --- Surface warnings from stderr ---

if [ -s "$STDERR_FILE" ]; then
  while IFS= read -r line; do
    echo "::debug::${line}"
  done < "$STDERR_FILE"
fi

# --- Extract verdict / gate (audit only) and issue count ---
# Audit's verdict (pass/warn/fail) is the load-bearing severity-aware signal:
# warn means "warn-tier issues only, do not fail CI". Threshold step gates on
# verdict for audit; raw issue counts only gate non-audit commands.

VERDICT=""
GATE=""
if [ "$INPUT_COMMAND" = "audit" ]; then
  VERDICT=$(jq -r '.verdict // ""' "$RESULTS_FILE")
  GATE=$(jq -r '.attribution.gate // ""' "$RESULTS_FILE")
elif [ "$INPUT_COMMAND" = "security" ]; then
  GATE=$(jq -r '.gate.mode // ""' "$RESULTS_FILE")
fi

case "$INPUT_COMMAND" in
  dead-code|check) ISSUES=$(jq -r '.total_issues' "$RESULTS_FILE") ;;
  dupes)           ISSUES=$(jq -r '.stats.clone_groups' "$RESULTS_FILE") ;;
  health)          ISSUES=$(jq -r '((.summary.functions_above_threshold // 0) + ((.runtime_coverage.findings // []) | map(select(.verdict == "safe_to_delete" or .verdict == "review_required" or .verdict == "low_traffic")) | length))' "$RESULTS_FILE") ;;
  audit)           ISSUES=$(jq -r 'if (.attribution.gate // "new-only") == "all" then ((.summary.dead_code_issues // 0) + (.summary.complexity_findings // 0) + (.summary.duplication_clone_groups // 0) + ((.complexity.styling_findings // []) | length)) else ((.attribution.dead_code_introduced // 0) + (.attribution.complexity_introduced // 0) + (.attribution.duplication_introduced // 0) + (.attribution.styling_introduced // 0)) end' "$RESULTS_FILE") ;;
  security)        ISSUES=$(jq -r 'if .gate then (.gate.new_count // 0) else (.summary.security_findings // ((.security_findings // []) | length)) end' "$RESULTS_FILE") ;;
  fix)             ISSUES=$(jq -r '(.fixes | length)' "$RESULTS_FILE") ;;
  "")              ISSUES=$(jq -r '((.check.total_issues // 0) + (((.dupes.clone_groups // []) | length) + (.dupes.clone_groups_omitted // 0)) + (.health.summary.functions_above_threshold // 0) + ((.health.runtime_coverage.findings // []) | map(select(.verdict == "safe_to_delete" or .verdict == "review_required" or .verdict == "low_traffic")) | length))' "$RESULTS_FILE") ;;
esac

if ! [[ "$ISSUES" =~ ^[0-9]+$ ]]; then
  echo "::error::Unexpected issue count: ${ISSUES}"
  exit 2
fi

{
  printf '%s\n' \
    "issues=${ISSUES}" \
    "results=${RESULTS_FILE}" \
    "command=${INPUT_COMMAND}" \
    "verdict=${VERDICT}" \
    "gate=${GATE}" \
    "baseline_entries=${BASELINE_ENTRIES}" \
    "baseline_matched=${BASELINE_MATCHED}" \
    "baseline_stale_entries=${BASELINE_STALE_ENTRIES}" \
    "baseline_advisory=${BASELINE_ADVISORY}" \
    "baseline_change_scoped=${BASELINE_CHANGE_SCOPED}" \
    "baseline_scope_reasons=${BASELINE_SCOPE_REASONS}" \
    "baseline_unrecognised=${BASELINE_UNRECOGNISED}" \
    "baseline_path=${INPUT_BASELINE:-}" \
    "baseline_gate_trips=${BASELINE_GATE_TRIPS}" \
    "gates_failed=$(join_gate_names "${GATE_FAILED_NAMES[@]:-}")" \
    "gates_warned=$(join_gate_names "${GATE_WARNED_NAMES[@]:-}")" \
    "gates_skipped=$(join_gate_names "${GATE_SKIPPED_NAMES[@]:-}")" \
    "gates_passed=$(join_gate_names "${GATE_PASSED_NAMES[@]:-}")" \
    "analysis_degraded=${ANALYSIS_DEGRADED}" \
    "requests_unapplied=${REQUESTS_UNAPPLIED}"
  if [ -f "$SARIF_FILE" ]; then
    printf '%s\n' "sarif=${SARIF_FILE}"
  fi
} >> "$GITHUB_OUTPUT"

# One accumulator for every failure reason, so none can hide another and the
# documented exit 8 cannot be downgraded by a later step. Everything above has
# already published its outputs and artifacts, so the downstream steps still
# run; this is the last thing the script does.
#
# The count gate lives here too. It used to sit in an inline `run:` block in
# action.yml whose first line returned when `fail-on-issues` was not true,
# which is what made the security gate unreachable for anyone who set
# `fail-on-issues: false` (issue #2685).
COUNT_FAILURES_BEFORE=${#GATE_FAILURES[@]}
if [ "${INPUT_FAIL_ON_ISSUES:-}" = "true" ]; then
  if [ "$INPUT_COMMAND" = "audit" ]; then
    # Audit gates on rule severity. The verdict already encodes the gate
    # decision: pass means no issues, warn means warn-tier only and does not
    # fail, fail means error-tier. Counting introduced findings instead would
    # re-introduce the bug issue #302 was filed to fix.
    if [ "$VERDICT" = "fail" ]; then
      GATE_FAILURES+=("Fallow audit failed (gate: ${INPUT_GATE:-new-only}, ${ISSUES} finding(s) at error severity in changed files).")
    fi
  elif [ "$ISSUES" -gt 0 ] && [ "$(read_gate_member health-findings status)" != "skipped" ]; then
    # `--min-score` turns the CLI's findings rule off ("complexity findings
    # become informational"), and the envelope says so with
    # `health-findings: skipped`. Counting them here would fail a run the CLI
    # deliberately passed, which is the inversion issue #2682 describes.
    case "$INPUT_COMMAND" in
      dead-code|check) GATE_FAILURES+=("Fallow found ${ISSUES} unused code issues.") ;;
      dupes)           GATE_FAILURES+=("Fallow found ${ISSUES} clone groups.") ;;
      health)          GATE_FAILURES+=("Fallow found ${ISSUES} health findings.") ;;
      security)        GATE_FAILURES+=("Fallow found ${ISSUES} security candidates.") ;;
      fix)             GATE_FAILURES+=("Fallow found ${ISSUES} fixable issues.") ;;
      "")              GATE_FAILURES+=("Fallow found ${ISSUES} issues.") ;;
    esac
  fi
fi

# The advisory count line, for runs whose count produced no error. Keying on
# `fail-on-issues` alone left two paths reporting the count neither way: a
# health run with `min-score` set, where the count gate stands down because the
# CLI turned its own findings rule off, and an audit run whose verdict is `warn`,
# which the count gate deliberately does not fail on.
if [ "$ISSUES" -gt 0 ] && [ ${#GATE_FAILURES[@]} -eq "$COUNT_FAILURES_BEFORE" ]; then
  case "$INPUT_COMMAND" in
    dead-code|check) echo "::warning::Fallow found ${ISSUES} unused code issues" ;;
    dupes)           echo "::warning::Fallow found ${ISSUES} clone groups" ;;
    health)          echo "::warning::Fallow found ${ISSUES} high complexity functions" ;;
    audit)           echo "::warning::Fallow audit found ${ISSUES} introduced issues in changed files" ;;
    security)        echo "::warning::Fallow found ${ISSUES} security candidates" ;;
    fix)             echo "::warning::Fallow proposed ${ISSUES} fixes" ;;
    "")              echo "::warning::Fallow found ${ISSUES} issues" ;;
  esac
fi

if [ ${#GATE_FAILURES[@]} -gt 0 ]; then
  for failure in "${GATE_FAILURES[@]}"; do
    echo "::error::${failure}"
  done
  # 8 is the documented security-gate exit and outranks the generic 1, so a run
  # that trips the security gate keeps reporting 8 however many other gates
  # tripped alongside it.
  if [ "$SECURITY_GATE_FAILED" = "true" ]; then
    exit 8
  fi
  exit 1
fi
