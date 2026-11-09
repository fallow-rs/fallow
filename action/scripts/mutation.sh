#!/usr/bin/env bash
set -euo pipefail

# Run `fallow mutation` for the Action, or apply its result.
#   mutation.sh run   runs the mutation step, sets the step outputs and exports
#                     the result paths through $GITHUB_ENV. It exits 0, so the
#                     analysis and the PR comment still run.
#   mutation.sh gate  fails the step for a failed gate, a tool failure, an
#                     interruption or a refusal. `fallow report` renders the
#                     result in the comment, the review, the annotations and
#                     the job summary.
# Env contract: see the step env blocks in action.yml. The scope step exports
# FALLOW_DIFF_FILE, FALLOW_SCOPE_CHANGED_SINCE, FALLOW_ARTIFACTS_DIR and
# HAS_MUTATION_REPORT through $GITHUB_ENV.

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=../../ci/scripts/mutation_common.sh
source "${SCRIPT_DIR}/../../ci/scripts/mutation_common.sh"

# The longest error message of the CLI that the gate line repeats. The job
# summary and the comment show the full message.
GATE_MESSAGE_MAX_CHARS=500

set_output() {
  [ -n "${GITHUB_OUTPUT:-}" ] || return 0
  printf '%s=%s\n' "$1" "$2" >> "$GITHUB_OUTPUT"
}

set_env() {
  [ -n "${GITHUB_ENV:-}" ] || return 0
  printf '%s=%s\n' "$1" "$2" >> "$GITHUB_ENV"
}

# shellcheck source=workflow-command.sh
source "${SCRIPT_DIR}/workflow-command.sh"

gate_error() {
  echo "::error title=Fallow mutation::$(escape_command_data "$1")"
}

# True for a pull request whose head branch lives in another repository. A
# deleted fork has no head repository, so it counts as a fork.
is_fork_pr() {
  [ -n "${PR_NUMBER:-}" ] && [ "${PR_HEAD_REPO:-}" != "${GH_REPO:-}" ]
}

# The step runs the tests of the checked-out code. A `pull_request` job from a
# fork gets no secrets and a read-only token. `push`, `schedule`,
# `workflow_dispatch` and `merge_group` run code of the repository itself.
# Each other event (for example `pull_request_target`, `issue_comment` or
# `workflow_run`) runs with the secrets of the base repository and can check
# out the code of a fork, and the event payload does not always name the pull
# request. The step refuses those events. This allowlist is the security
# boundary of the step.
event_refusal_reason() {
  case "${EVENT_NAME:-}" in
    pull_request|push|schedule|workflow_dispatch|merge_group) return 0 ;;
  esac
  printf 'event-refused'
}

# The scope step resolved the diff and the base ref before this step, so the
# mutation run, the analysis and the review all use the same changed lines.
select_changed_scope() {
  [ "$(fallow_mutation_scope)" = "changed" ] || return 0
  if [ -n "${FALLOW_DIFF_FILE:-}" ]; then
    export FALLOW_MUTATION_DIFF_FILE="$FALLOW_DIFF_FILE"
  elif [ -n "${FALLOW_SCOPE_CHANGED_SINCE:-}" ]; then
    export FALLOW_MUTATION_CHANGED_SINCE="$FALLOW_SCOPE_CHANGED_SINCE"
  fi
}

# An older CLI has no `fallow mutation`. Its usage line then does not name the
# command.
has_mutation_command() {
  local help
  help=$(fallow mutation --help 2> /dev/null) || return 1
  grep -q 'fallow mutation' <<< "$help"
}

# Writes the result files of a CLI without the mutation command: a tool
# failure with a fixed-text error envelope that names the version.
write_missing_command_result() {
  local out=$1 version name message
  for name in fallow-mutation.json fallow-mutation-status.json fallow-mutation-stderr.log \
      fallow-mutation-tasks.json fallow-mutation-section.md; do
    rm -f "${out}/${name}"
  done
  mkdir -p "$out"
  version=$(fallow --version 2> /dev/null | grep -oE '[0-9]+\.[0-9]+\.[0-9]+' | head -n 1 || true)
  if [ -n "$version" ]; then
    message="fallow ${version} has no mutation command."
  else
    message="This fallow version has no mutation command."
  fi
  message="${message} Use fallow ${MUTATION_REPORT_VERSION} or later."
  fallow_mutation_write_fallback_envelope "$out/fallow-mutation.json" FALLOW_MUTATION_UNAVAILABLE "$message"
  fallow_mutation_write_status "$out/fallow-mutation-status.json" tool-failed "" ""
}

# Prints the completion status of a mutation report, or nothing.
report_completion() {
  local value
  value=$(jq -r 'if .kind == "mutation" then (.completion.status // "") else "" end' "$1" 2> /dev/null || true)
  case "$value" in
    complete|partial) printf '%s' "$value" ;;
  esac
}

# Prints the score of a mutation report in percent, or nothing.
report_score() {
  jq -r 'if .kind == "mutation" and (.summary.score | type) == "number"
    then (.summary.score * 1000 | round) / 10 | tostring else "" end' "$1" 2> /dev/null || true
}

run_step() {
  export FALLOW_MUTATION_OUT_DIR="${FALLOW_ARTIFACTS_DIR:-.}"
  fallow_mutation_check_artifacts "$FALLOW_MUTATION_OUT_DIR" || return 2
  local out=$FALLOW_MUTATION_OUT_DIR
  local status="$out/fallow-mutation-status.json"
  local report="$out/fallow-mutation.json"
  FALLOW_MUTATION_REFUSE_REASON=$(event_refusal_reason)
  export FALLOW_MUTATION_REFUSE_REASON

  local rc=0
  if [ -z "$FALLOW_MUTATION_REFUSE_REASON" ] && ! has_mutation_command; then
    write_missing_command_result "$out"
    rc=2
  else
    [ -n "$FALLOW_MUTATION_REFUSE_REASON" ] || select_changed_scope
    fallow_mutation_main || rc=$?
  fi

  local outcome
  outcome=$(jq -r '.outcome // ""' "$status" 2> /dev/null || true)
  if [ -z "$outcome" ]; then
    echo "::error title=Fallow mutation::The mutation step wrote no valid status file, so its result is unknown."
    exit 2
  fi
  local reason exit_code completion="" score="" results=""
  reason=$(jq -r '.not_run_reason // ""' "$status")
  fallow_mutation_is_not_run_reason "$reason" || reason=""
  exit_code=$(jq -r '.exit_code // ""' "$status")
  if [ -s "$report" ]; then
    results=$report
    completion=$(report_completion "$report")
    score=$(report_score "$report")
  fi

  set_output outcome "$outcome"
  set_output exit_code "$exit_code"
  set_output completion "$completion"
  set_output score "$score"
  set_output status "$status"
  set_output results "$results"
  set_output report_rendered "${HAS_MUTATION_REPORT:-false}"
  set_env FALLOW_MUTATION_RESULTS_FILE "$results"
  set_env FALLOW_MUTATION_STATUS_FILE "$status"
  set_env FALLOW_MUTATION_NOT_RUN "$reason"

  # A fork pull request restores the result cache but never saves it, so its
  # tests cannot write results that a later trusted run reads.
  local cache_save=false
  if ! is_fork_pr && [ "$outcome" != "refused" ] && [ "$outcome" != "skipped" ] \
      && [ -f "${FALLOW_MUTATION_ROOT:-.}/.fallow/mutation/results.json" ]; then
    cache_save=true
  fi
  set_output cache_save "$cache_save"

  echo "Mutation testing: ${outcome}${reason:+ (${reason})}${completion:+, ${completion}}${score:+, score ${score}%} (exit ${rc})"
}

# Prints the error of a tool failure: the code and the message of the error
# envelope in the report file, when the file holds one.
tool_failure_line() {
  local exit_code=$1 report=${FALLOW_MUTATION_RESULTS_FILE:-} code="" message="" detail
  if [ -n "$report" ] && [ -s "$report" ] && jq -e '.error == true' "$report" > /dev/null 2>&1; then
    code=$(jq -r '.code // "" | tostring' "$report" 2> /dev/null || true)
    [[ "$code" =~ ^[A-Z0-9_]+$ ]] || code=""
    message=$(jq -r '.message // "" | tostring' "$report" 2> /dev/null || true)
  fi
  if [ "${#message}" -gt "$GATE_MESSAGE_MAX_CHARS" ]; then
    message="${message:0:$GATE_MESSAGE_MAX_CHARS} ... (shortened)"
  fi
  detail="outcome tool-failed${exit_code:+, exit code ${exit_code}}${code:+, ${code}}"
  printf 'fallow mutation failed (%s): %s' "$detail" "${message:-no error message. The fallow-mutation-stderr.log file of this job has the error output.}"
}

gate_step() {
  local status=${FALLOW_MUTATION_STATUS_FILE:-}
  if [ -z "$status" ] || [ ! -f "$status" ]; then
    gate_error "The mutation step wrote no status file, so its result is unknown."
    exit 2
  fi

  local outcome exit_code reason
  outcome=$(jq -r '.outcome // ""' "$status" 2> /dev/null || true)
  exit_code=$(jq -r '.exit_code // ""' "$status" 2> /dev/null || true)
  [[ "$exit_code" =~ ^[0-9]+$ ]] || exit_code=""
  reason=$(jq -r '.not_run_reason // ""' "$status" 2> /dev/null || true)
  fallow_mutation_is_not_run_reason "$reason" || reason=""
  case "$outcome" in
    ok|skipped)
      exit 0
      ;;
    gate-failed)
      gate_error "The mutation score is below the minimum (outcome gate-failed, exit code ${exit_code:-1})."
      exit 1
      ;;
    refused)
      gate_error "Mutation testing did not run (outcome refused${reason:+, ${reason}}). Mutation testing runs only for the pull_request, push, schedule, workflow_dispatch and merge_group events."
      exit 2
      ;;
    interrupted)
      gate_error "A signal stopped fallow mutation (outcome interrupted${exit_code:+, exit code ${exit_code}})."
      exit 2
      ;;
    *)
      gate_error "$(tool_failure_line "$exit_code")"
      exit 2
      ;;
  esac
}

case "${1:-run}" in
  run) run_step ;;
  gate) gate_step ;;
  *)
    echo "::error::Unknown mutation.sh mode: $1"
    exit 2
    ;;
esac
