#!/usr/bin/env bash
set -euo pipefail

# Run `fallow mutation` for the Action, or apply its result.
#   mutation.sh run   runs the mutation step and sets the step outputs. It
#                     exits 0, so the analysis and the PR comment still run.
#   mutation.sh gate  writes the section to the job summary and fails the
#                     step for a failed gate, a tool failure or a refusal.
# Env contract: see the step env blocks in action.yml.

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=../../ci/scripts/mutation_common.sh
source "${SCRIPT_DIR}/../../ci/scripts/mutation_common.sh"

set_output() {
  [ -n "${GITHUB_OUTPUT:-}" ] || return 0
  printf '%s=%s\n' "$1" "$2" >> "$GITHUB_OUTPUT"
}

# Escapes a workflow command message, so a newline or a percent sign in an
# error message cannot end the command or start a new one.
escape_command_data() {
  local value=$1
  value=${value//'%'/'%25'}
  value=${value//$'\r'/'%0D'}
  value=${value//$'\n'/'%0A'}
  printf '%s' "$value"
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
# request. The step refuses those events.
# The message names commands in backticks for the comment, not for the shell.
# shellcheck disable=SC2016
fork_refusal_reason() {
  case "${EVENT_NAME:-}" in
    pull_request|push|schedule|workflow_dispatch|merge_group) return 0 ;;
  esac
  printf 'The `%s` event gives the job the secrets of the base repository, and the job can check out the code of a pull request from a fork. `fallow mutation` runs the tests of the checked-out code, so it runs only for the `pull_request`, `push`, `schedule`, `workflow_dispatch` and `merge_group` events.' \
    "${EVENT_NAME:-unknown}"
}

prepare_changed_scope() {
  [ "$(fallow_mutation_scope)" = "changed" ] || return 0
  [ -z "${FALLOW_MUTATION_DIFF_FILE:-}" ] || return 0
  if [ -n "${PR_NUMBER:-}" ] && [ -n "${GH_REPO:-}" ]; then
    local diff_file="${RUNNER_TEMP:-${TMPDIR:-/tmp}}/fallow-mutation-pr.diff"
    if gh pr diff "$PR_NUMBER" --repo "$GH_REPO" > "$diff_file" 2> /dev/null && [ -s "$diff_file" ]; then
      export FALLOW_MUTATION_DIFF_FILE="$diff_file"
      return 0
    fi
    echo "::warning::Failed to fetch the PR diff for mutation testing; using the PR base commit instead"
    rm -f "$diff_file"
  fi
  export FALLOW_MUTATION_CHANGED_SINCE="${INPUT_CHANGED_SINCE:-${PR_BASE_SHA:-}}"
}

run_step() {
  export FALLOW_MUTATION_OUT_DIR="${FALLOW_ARTIFACTS_DIR:-.}"
  FALLOW_MUTATION_REFUSE_REASON=$(fork_refusal_reason)
  export FALLOW_MUTATION_REFUSE_REASON
  [ -n "$FALLOW_MUTATION_REFUSE_REASON" ] || prepare_changed_scope

  local rc=0
  fallow_mutation_main || rc=$?

  local out=$FALLOW_MUTATION_OUT_DIR
  local status="$out/fallow-mutation-status.json"
  local outcome completion score clean
  outcome=$(jq -r '.outcome // ""' "$status" 2> /dev/null || true)
  if [ -z "$outcome" ]; then
    echo "::error title=Fallow mutation::The mutation step wrote no valid status file, so its result is unknown."
    exit 2
  fi
  completion=$(jq -r '.completion // ""' "$status")
  score=$(jq -r 'if .score == null then "" else (.score * 1000 | round) / 10 | tostring end' "$status")
  clean=$(jq -r '.clean' "$status")

  set_output outcome "$outcome"
  set_output exit_code "$(jq -r '.exit_code // ""' "$status")"
  set_output completion "$completion"
  set_output score "$score"
  set_output clean "$clean"
  set_output status "$status"
  set_output section "$out/fallow-mutation-section.md"
  if [ -f "$out/fallow-mutation.json" ]; then
    set_output results "$out/fallow-mutation.json"
  else
    set_output results ""
  fi

  # A fork pull request restores the result cache but never saves it, so its
  # tests cannot write results that a later trusted run reads.
  local cache_save=false
  if ! is_fork_pr && [ "$outcome" != "refused" ] && [ "$outcome" != "skipped" ] \
      && [ -f "${FALLOW_MUTATION_ROOT:-.}/.fallow/mutation/results.json" ]; then
    cache_save=true
  fi
  set_output cache_save "$cache_save"

  echo "Mutation testing: ${outcome}${completion:+, ${completion}}${score:+, score ${score}%} (exit ${rc})"
}

gate_step() {
  local status=${FALLOW_MUTATION_STATUS_FILE:-}
  if [ -z "$status" ] || [ ! -f "$status" ]; then
    echo "::error title=Fallow mutation::The mutation step wrote no status file, so its result is unknown."
    exit 2
  fi
  local section=${FALLOW_MUTATION_SECTION_FILE:-}
  if [ -n "${GITHUB_STEP_SUMMARY:-}" ] && [ -n "$section" ] && [ -s "$section" ]; then
    { printf '\n'; cat "$section"; } >> "$GITHUB_STEP_SUMMARY"
  fi

  local outcome exit_code code message
  outcome=$(jq -r '.outcome // ""' "$status" 2> /dev/null || true)
  exit_code=$(jq -r '.exit_code // ""' "$status")
  code=$(jq -r '.code // ""' "$status")
  message=$(jq -r '.message // ""' "$status")
  case "$outcome" in
    ok|skipped)
      exit 0
      ;;
    gate-failed)
      local score min
      score=$(jq -r 'if .score == null then "n/a" else ((.score * 1000 | round) / 10 | tostring) + "%" end' "$status")
      min=$(jq -r 'if .min_score == null then "the minimum score of the config" else "the minimum of \(.min_score)%" end' "$status")
      echo "::error title=Fallow mutation::The mutation score ${score} is lower than ${min}."
      exit 1
      ;;
    refused)
      echo "::error title=Fallow mutation::$(escape_command_data "Mutation testing did not run. ${message}")"
      exit 2
      ;;
    invalid-input)
      echo "::error title=Fallow mutation::$(escape_command_data "The mutation testing input is invalid. ${message}")"
      exit 2
      ;;
    interrupted)
      echo "::error title=Fallow mutation::A signal stopped fallow mutation (exit code ${exit_code})."
      exit 2
      ;;
    *)
      echo "::error title=Fallow mutation::$(escape_command_data "fallow mutation failed with exit code ${exit_code:-unknown}${code:+ (${code})}: ${message:-no error message}")"
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
