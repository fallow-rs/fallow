#!/usr/bin/env bash
set -euo pipefail

# Run `fallow mutation` for the GitLab template. The template runs this script
# before the analysis, because the run refuses a dirty worktree and the
# analysis writes its result files into the project directory.
#
# Env: the FALLOW_MUTATION_* variables of the template, FALLOW_ROOT,
#   FALLOW_CONFIG, FALLOW_DIFF_FILE, FALLOW_CHANGED_SINCE, and the GitLab
#   predefined CI_* variables.
# Result: the files of fallow_mutation_main in the project directory. The
#   script exits 0, so the analysis and the MR comment still run; the template
#   reads fallow-mutation-status.json at the end of the job.

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=mutation_common.sh
source "${SCRIPT_DIR}/mutation_common.sh"

export FALLOW_MUTATION_ROOT="${FALLOW_ROOT:-.}"
export FALLOW_MUTATION_CONFIG="${FALLOW_CONFIG:-}"
export FALLOW_MUTATION_OUT_DIR="${FALLOW_MUTATION_OUT_DIR:-.}"

STAMP_FILE="${FALLOW_MUTATION_ROOT}/.fallow/mutation/ci-cache-key"
RESULTS_FILE="${FALLOW_MUTATION_ROOT}/.fallow/mutation/results.json"

# A merge request pipeline from a fork that runs in the target project gets
# the CI/CD variables of the target project. The tests of the merge request
# run in this step, so the step refuses that pipeline. A pipeline that runs in
# the fork project has only the variables of the fork.
# The message names commands in backticks for the comment, not for the shell.
# shellcheck disable=SC2016
fork_refusal_reason() {
  local source_project=${CI_MERGE_REQUEST_SOURCE_PROJECT_ID:-}
  [ -n "$source_project" ] || return 0
  [ "$source_project" != "${CI_PROJECT_ID:-}" ] || return 0
  printf 'This pipeline runs a merge request from a fork in the target project, so the job has the CI/CD variables of the target project. `fallow mutation` runs the tests of the merge request, so this job does not run it. Run the pipeline in the fork project.'
}

prepare_changed_scope() {
  [ "$(fallow_mutation_scope)" = "changed" ] || return 0
  if [ -n "${FALLOW_DIFF_FILE:-}" ]; then
    export FALLOW_MUTATION_DIFF_FILE="$FALLOW_DIFF_FILE"
    return 0
  fi
  local base=${CI_MERGE_REQUEST_DIFF_BASE_SHA:-}
  if [ -n "$base" ]; then
    local diff_file="${TMPDIR:-/tmp}/fallow-mutation-mr.diff"
    if git diff "${base}..HEAD" > "$diff_file" 2> /dev/null && [ -s "$diff_file" ]; then
      export FALLOW_MUTATION_DIFF_FILE="$diff_file"
      return 0
    fi
    rm -f "$diff_file"
  fi
  export FALLOW_MUTATION_CHANGED_SINCE="${FALLOW_CHANGED_SINCE:-$base}"
}

# The GitLab cache key cannot hash files that the template does not know, so
# a stamp file holds the digest of the lockfiles and the runner config files.
# A restored result cache with another digest is removed before the run.
check_cache_stamp() {
  local digest=$1
  [ -f "$RESULTS_FILE" ] || return 0
  if [ ! -f "$STAMP_FILE" ] || [ "$(cat "$STAMP_FILE")" != "$digest" ]; then
    echo "Mutation result cache: the lockfiles or the runner config changed, so the run starts with an empty cache"
    rm -f "$RESULTS_FILE"
  fi
}

write_cache_stamp() {
  local digest=$1
  [ -d "$(dirname -- "$STAMP_FILE")" ] || return 0
  printf '%s\n' "$digest" > "$STAMP_FILE"
}

FALLOW_MUTATION_REFUSE_REASON=$(fork_refusal_reason)
export FALLOW_MUTATION_REFUSE_REASON

DIGEST=""
if [ -z "$FALLOW_MUTATION_REFUSE_REASON" ]; then
  prepare_changed_scope
  DIGEST=$(fallow_mutation_cache_digest "$FALLOW_MUTATION_ROOT")
  check_cache_stamp "$DIGEST"
fi

RC=0
fallow_mutation_main || RC=$?
[ -n "$DIGEST" ] && write_cache_stamp "$DIGEST"

STATUS="${FALLOW_MUTATION_OUT_DIR}/fallow-mutation-status.json"
echo "Mutation testing: $(jq -r '[.outcome, (.completion // empty), (if .score == null then empty else "score \((.score * 1000 | round) / 10)%" end)] | join(", ")' "$STATUS") (exit ${RC})"
exit 0
