#!/usr/bin/env bash
# Shared `fallow mutation` helpers for the GitHub Action
# (action/scripts/mutation.sh) and the GitLab template (ci/scripts/mutation.sh).
# The caller sources this file, sets the FALLOW_MUTATION_* variables below,
# and calls fallow_mutation_main. The render steps call
# fallow_mutation_render_args to pass the result to `fallow report`, which
# renders all mutation output.
#
# Input variables:
#   FALLOW_MUTATION_ROOT           project root (default: .)
#   FALLOW_MUTATION_CONFIG         config file path (optional)
#   FALLOW_MUTATION_SCOPE          changed (default), all, or paths
#   FALLOW_MUTATION_TIME_BUDGET    for example 10m; empty means no budget
#   FALLOW_MUTATION_MAX_MUTANTS    optional positive integer
#   FALLOW_MUTATION_MIN_SCORE      optional percent from 0 to 100
#   FALLOW_MUTATION_DIFF_FILE      diff of the changed scope (optional)
#   FALLOW_MUTATION_CHANGED_SINCE  base ref of the changed scope (optional)
#   FALLOW_MUTATION_REFUSE_REASON  event-refused or fork-refused: do not run
#   FALLOW_MUTATION_OUT_DIR        directory for the result files (default: .)
#
# Result files in FALLOW_MUTATION_OUT_DIR, written after the run:
#   fallow-mutation.json          the mutation report. When the run wrote no
#                                 report, the error envelope that the run
#                                 printed on stdout, if it printed one.
#   fallow-mutation-status.json   {"outcome", "exit_code", "not_run_reason"},
#                                 closed values only
#   fallow-mutation-stderr.log    the error output of the run, for the job log

# The first fallow release with `fallow report --mutation-report`. The upgrade
# line of a CI job with an older CLI names it.
# shellcheck disable=SC2034
MUTATION_REPORT_VERSION=3.42.0

# Validate paths against the physical working directory. This preflight assumes
# a static checkout; it does not prevent concurrent path replacement.
fallow_artifact_check() {
  local path=$1 kind=${2:-file} workspace part current index
  local parts=()
  workspace=$(pwd -P) || return 2
  case "$path" in
    "$workspace"|"$PWD") path=. ;;
    "$workspace"/*) path=${path#"$workspace"/} ;;
    "$PWD"/*) path=${path#"$PWD"/} ;;
  esac
  if [[ "$path" = /* || "$path" = -* || "$path" == *$'\n'* || "$path" == *$'\r'* \
      || "$path" =~ (^|/)\.\.(/|$) ]]; then
    printf '%s\n' '::error::Artifact paths must stay inside the workspace without traversal or control characters.' >&2
    return 2
  fi
  IFS=/ read -r -a parts <<< "$path"
  current=$workspace
  for ((index = 0; index < ${#parts[@]}; index++)); do
    part=${parts[$index]}
    [ -n "$part" ] && [ "$part" != . ] || continue
    current="$current/$part"
    if [ -L "$current" ]; then
      printf '%s\n' '::error::Artifact paths must not contain symlinks.' >&2
      return 2
    fi
    if [ "$kind" = directory ] || [ "$index" -lt $((${#parts[@]} - 1)) ]; then
      if [ -e "$current" ] && { [ ! -d "$current" ] || [ ! -w "$current" ]; }; then
        printf '%s\n' '::error::Artifact parents must be writable directories.' >&2
        return 2
      fi
    elif [ -e "$current" ] && { [ ! -f "$current" ] || [ ! -w "$current" ]; }; then
      printf '%s\n' '::error::Artifact destinations must be writable regular files.' >&2
      return 2
    fi
  done
}

FALLOW_MUTATION_ARTIFACTS=(fallow-mutation.json fallow-mutation-tasks.json fallow-mutation-status.json
  fallow-mutation-section.md fallow-mutation-stderr.log)

fallow_mutation_check_artifacts() {
  local dir=$1 name
  fallow_artifact_check "$dir" directory || return 2
  for name in "${FALLOW_MUTATION_ARTIFACTS[@]}"; do
    fallow_artifact_check "$dir/$name" || return 2
  done
}

# The values of `fallow report --mutation-not-run`.
FALLOW_MUTATION_NOT_RUN_REASONS=(event-refused fork-refused no-change-scope interrupted)

# The run removes these variables from the environment of `fallow mutation`.
# The tests of the project run in that process and do not need them.
# - Credentials: a test cannot read them. CI_JOB_TOKEN stays: a test can need
#   it for a package registry, and GitLab limits its scope.
# - GitHub workflow command files: a test does not get the paths of the env,
#   the outputs, the PATH, the state or the job summary of the later steps.
#   The files stay on the disk of the runner, and a test can still find them.
# - BASH_ENV: a test shell does not source a file that the job chose.
# This is defense in depth, not a guarantee. The event and fork refusals are
# the boundary.
FALLOW_MUTATION_UNSET_ENV=(
  GH_TOKEN GITHUB_TOKEN GITLAB_TOKEN FALLOW_MUTATION_GH_TOKEN
  ACTIONS_ID_TOKEN_REQUEST_TOKEN ACTIONS_ID_TOKEN_REQUEST_URL ACTIONS_RUNTIME_TOKEN
  GITHUB_ENV GITHUB_OUTPUT GITHUB_PATH GITHUB_STEP_SUMMARY GITHUB_STATE
  BASH_ENV
)

fallow_mutation_sha256() {
  if command -v sha256sum > /dev/null 2>&1; then
    sha256sum | cut -d' ' -f1
  else
    shasum -a 256 | cut -d' ' -f1
  fi
}

# Prints a 16-character digest of the lockfiles and the runner config files
# of a project root. The CI cache key of the mutation result cache uses it, so
# a dependency or runner config change starts a new cache.
fallow_mutation_cache_digest() {
  local root=${1:-.}
  local top dir name path
  top=$(git -C "$root" rev-parse --show-toplevel 2>/dev/null || printf '%s' "$root")
  {
    for dir in "$root" "$top"; do
      for name in package-lock.json npm-shrinkwrap.json yarn.lock pnpm-lock.yaml bun.lock bun.lockb; do
        path="$dir/$name"
        [ -f "$path" ] && printf 'lock %s %s\n' "$name" "$(fallow_mutation_sha256 < "$path")"
      done
    done
    for path in "$root"/vitest.config.* "$root"/vite.config.* "$root"/vitest.workspace.* \
        "$root/bunfig.toml" "$root/package.json" "$root/tsconfig.json"; do
      [ -f "$path" ] && printf 'config %s %s\n' "${path##*/}" "$(fallow_mutation_sha256 < "$path")"
    done
    true
  } | fallow_mutation_sha256 | cut -c1-16
}

# Returns 0 when the argument is a value of `fallow report --mutation-not-run`.
fallow_mutation_is_not_run_reason() {
  local value=${1:-} reason
  for reason in "${FALLOW_MUTATION_NOT_RUN_REASONS[@]}"; do
    [ "$value" = "$reason" ] && return 0
  done
  return 1
}

# Checks the scope paths before they reach the command line. Fallow validates
# all the other values. A path that starts with "-" would be read as a flag.
# Prints the problem and returns 1 for a bad path.
fallow_mutation_validate_inputs() {
  local token
  for token in $(fallow_mutation_scope_paths); do
    case "$token" in
      -*)
        printf 'A mutation scope path must not start with "-", got: %s\n' "$token"
        return 1
        ;;
    esac
  done
  return 0
}

fallow_mutation_scope() {
  local scope=${FALLOW_MUTATION_SCOPE:-changed}
  printf '%s' "${scope:-changed}"
}

# Prints the paths of a path scope, one for each line. Commas and white space
# separate the paths. Prints nothing for the changed and all scopes.
fallow_mutation_scope_paths() {
  local scope
  scope=$(fallow_mutation_scope)
  case "$scope" in
    changed|all) return 0 ;;
  esac
  printf '%s\n' "$scope" | tr ',' ' ' | tr -s '[:space:]' '\n' | sed '/^$/d'
}

# Sets FALLOW_MUTATION_ARGS. Returns 3 when the changed scope has no diff
# and no base ref, so the run cannot know the changed lines.
fallow_mutation_build_args() {
  local report_file=$1
  local scope path
  scope=$(fallow_mutation_scope)
  FALLOW_MUTATION_ARGS=(mutation --root "${FALLOW_MUTATION_ROOT:-.}" --format json --quiet -o "$report_file")
  [ -n "${FALLOW_MUTATION_CONFIG:-}" ] && FALLOW_MUTATION_ARGS+=(--config "$FALLOW_MUTATION_CONFIG")
  case "$scope" in
    changed)
      if [ -n "${FALLOW_MUTATION_DIFF_FILE:-}" ]; then
        FALLOW_MUTATION_ARGS+=(--diff-file "$FALLOW_MUTATION_DIFF_FILE")
      elif [ -n "${FALLOW_MUTATION_CHANGED_SINCE:-}" ]; then
        FALLOW_MUTATION_ARGS+=(--changed-since "$FALLOW_MUTATION_CHANGED_SINCE")
      else
        return 3
      fi
      ;;
    all)
      FALLOW_MUTATION_ARGS+=(--all)
      ;;
    *)
      while IFS= read -r path; do
        FALLOW_MUTATION_ARGS+=("$path")
      done < <(fallow_mutation_scope_paths)
      ;;
  esac
  [ -n "${FALLOW_MUTATION_TIME_BUDGET:-}" ] && FALLOW_MUTATION_ARGS+=(--time-budget "$FALLOW_MUTATION_TIME_BUDGET")
  [ -n "${FALLOW_MUTATION_MAX_MUTANTS:-}" ] && FALLOW_MUTATION_ARGS+=(--max-mutants "$FALLOW_MUTATION_MAX_MUTANTS")
  [ -n "${FALLOW_MUTATION_MIN_SCORE:-}" ] && FALLOW_MUTATION_ARGS+=(--min-score "$FALLOW_MUTATION_MIN_SCORE")
  return 0
}

# Maps the exit code of `fallow mutation` to the outcome of the step.
# 0: the run finished (complete or partial). 1: the --min-score gate failed.
# 2: invalid input or an execution error. 128 + n: a signal stopped the run.
fallow_mutation_outcome_for_exit() {
  local code=$1
  if [ "$code" -eq 0 ]; then
    printf 'ok'
  elif [ "$code" -eq 1 ]; then
    printf 'gate-failed'
  elif [ "$code" -ge 128 ]; then
    printf 'interrupted'
  else
    printf 'tool-failed'
  fi
}

# Writes the status file. Arguments: file, outcome, exit code (or empty),
# not-run reason (or empty). The file holds closed values only: the render
# gets free text only from the error envelope of the CLI.
fallow_mutation_write_status() {
  local file=$1 outcome=$2 exit_code=${3:-} reason=${4:-}
  jq -n \
    --arg outcome "$outcome" \
    --arg exit_code "$exit_code" \
    --arg reason "$reason" \
    '{
      outcome: $outcome,
      exit_code: (if $exit_code == "" then null else ($exit_code | tonumber) end),
      not_run_reason: (if $reason == "" then null else $reason end)
    }' > "$file"
}

# Writes the first error envelope of a stdout file to the target file.
# Returns 1 when stdout has no envelope.
fallow_mutation_write_envelope() {
  local stdout_file=$1 target=$2
  jq -cs 'map(select(type == "object" and .error == true)) | first // empty' \
    "$stdout_file" > "$target" 2> /dev/null || true
  if [ -s "$target" ]; then
    return 0
  fi
  rm -f "$target"
  return 1
}

# Writes an error envelope with a fixed code and message, for a failure that
# left no report and no envelope of the CLI. The render then shows the
# failure. Arguments: target file, code, message, exit code (or empty).
fallow_mutation_write_fallback_envelope() {
  local target=$1 code=$2 message=$3 exit_code=${4:-}
  jq -cn --arg code "$code" --arg message "$message" --arg exit_code "$exit_code" \
    '{error: true, code: $code, message: $message,
      exit_code: (if $exit_code == "" then 2 else ($exit_code | tonumber) end)}' > "$target"
}

# Prints the `fallow report` arguments that pass the mutation result, one
# argument for each line: `--mutation-not-run <reason>`, `--mutation-report
# <file>`, or nothing. Prints nothing when the capability probe
# (HAS_MUTATION_REPORT in the Action, FALLOW_HAS_MUTATION_REPORT in GitLab) is
# not true, so an older CLI renders as before.
# Argument: the directory of the result files (optional).
# FALLOW_MUTATION_NOT_RUN, FALLOW_MUTATION_STATUS_FILE and
# FALLOW_MUTATION_RESULTS_FILE take precedence over the files in that
# directory. Without a directory and without these variables, the job ran no
# mutation step, and the function prints nothing.
fallow_mutation_render_args() {
  [ "${HAS_MUTATION_REPORT:-}" = "true" ] || [ "${FALLOW_HAS_MUTATION_REPORT:-}" = "true" ] || return 0
  local dir=${1:-}
  local reason=${FALLOW_MUTATION_NOT_RUN:-}
  local status_file=${FALLOW_MUTATION_STATUS_FILE:-}
  local report=${FALLOW_MUTATION_RESULTS_FILE:-}
  if [ -n "$dir" ]; then
    status_file=${status_file:-${dir%/}/fallow-mutation-status.json}
    report=${report:-${dir%/}/fallow-mutation.json}
  fi
  if [ -z "$reason" ] && [ -n "$status_file" ] && [ -f "$status_file" ]; then
    reason=$(jq -r '.not_run_reason // empty' "$status_file" 2> /dev/null || true)
  fi
  if fallow_mutation_is_not_run_reason "$reason"; then
    printf '%s\n%s\n' --mutation-not-run "$reason"
    return 0
  fi
  if [ -n "$report" ] && [ -s "$report" ]; then
    printf '%s\n%s\n' --mutation-report "$report"
  fi
  return 0
}

# Runs `fallow mutation` and writes the result files. Returns 0 when the run
# finished or was skipped, 1 when the --min-score gate failed, and 2 for a
# refusal, a tool failure or a signal. The caller decides when to apply the
# return code, so the analysis and the comment still run after a failure.
fallow_mutation_main() {
  local out_dir=${FALLOW_MUTATION_OUT_DIR:-.}
  fallow_mutation_check_artifacts "$out_dir" || return 2
  local tmp_parent=${RUNNER_TEMP:-${TMPDIR:-/tmp}}
  local tmp
  tmp=$(mktemp -d "${tmp_parent%/}/fallow-mutation.XXXXXX")
  local report="$tmp/report.json"
  local status_tmp="$tmp/status.json"
  local outcome exit_code="" reason="" problem build_status=0
  local name

  # A result file of an earlier run in the same directory must not look like
  # the result of this run. The section and tasks files of older scripts are
  # removed too, because no step of this version writes them.
  for name in "${FALLOW_MUTATION_ARTIFACTS[@]}"; do
    rm -f "${out_dir}/${name}"
  done

  if [ -n "${FALLOW_MUTATION_REFUSE_REASON:-}" ]; then
    outcome=refused
    case "$FALLOW_MUTATION_REFUSE_REASON" in
      event-refused|fork-refused) reason=$FALLOW_MUTATION_REFUSE_REASON ;;
      *) printf 'Unknown mutation refuse reason: %s\n' "$FALLOW_MUTATION_REFUSE_REASON" >&2 ;;
    esac
  elif ! problem=$(fallow_mutation_validate_inputs); then
    outcome=tool-failed
    fallow_mutation_write_fallback_envelope "$report" FALLOW_MUTATION_INVALID_SCOPE "$problem"
  else
    fallow_mutation_build_args "$report" || build_status=$?
    if [ "$build_status" -eq 3 ]; then
      outcome=skipped
      reason=no-change-scope
    else
      # The tests of the project run inside this command. Its stdout and
      # stderr go to the temp directory: a new file in the worktree would
      # make the dirty-worktree check of the run refuse the run. The stderr
      # can hold test output, so it goes to a log file and never into a
      # rendered surface.
      exit_code=0
      local unset_args=() var
      for var in "${FALLOW_MUTATION_UNSET_ENV[@]}"; do
        unset_args+=(-u "$var")
      done
      env "${unset_args[@]}" fallow "${FALLOW_MUTATION_ARGS[@]}" > "$tmp/stdout.json" 2> "$tmp/stderr.log" || exit_code=$?
      outcome=$(fallow_mutation_outcome_for_exit "$exit_code")
      if [ ! -s "$report" ]; then
        rm -f "$report"
        [ "$outcome" = "interrupted" ] || outcome=tool-failed
        if fallow_mutation_write_envelope "$tmp/stdout.json" "$report"; then
          :
        elif [ "$outcome" = "interrupted" ]; then
          reason=interrupted
        else
          fallow_mutation_write_fallback_envelope "$report" FALLOW_MUTATION_NO_REPORT \
            "fallow mutation exited with code ${exit_code}, wrote no report and printed no error message. The fallow-mutation-stderr.log file of this job has its error output." \
            "$exit_code"
        fi
      fi
    fi
  fi

  if ! fallow_mutation_check_artifacts "$out_dir" || ! mkdir -p -- "$out_dir"; then
    rm -rf "$tmp"
    return 2
  fi
  if ! fallow_mutation_write_status "$status_tmp" "$outcome" "$exit_code" "$reason"; then
    printf 'The CI step could not write the mutation status file.\n' >&2
    outcome=tool-failed
    printf '{"outcome":"tool-failed","exit_code":null,"not_run_reason":null}\n' > "$status_tmp"
  fi
  cp "$status_tmp" "$out_dir/fallow-mutation-status.json"
  [ -s "$report" ] && cp "$report" "$out_dir/fallow-mutation.json"
  [ -s "$tmp/stderr.log" ] && cp "$tmp/stderr.log" "$out_dir/fallow-mutation-stderr.log"
  rm -rf "$tmp"

  case "$outcome" in
    ok|skipped) return 0 ;;
    gate-failed) return 1 ;;
    *) return 2 ;;
  esac
}
