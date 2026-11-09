#!/usr/bin/env bash
set -euo pipefail

# Run `fallow mutation` for the GitLab template. The template runs this script
# before the analysis, because the run refuses a dirty worktree and the
# analysis writes its result files into the project directory.
#
# Env: the FALLOW_MUTATION_* variables of the template, FALLOW_ROOT,
#   FALLOW_CONFIG, the change scope of the template (FALLOW_DIFF_FILE and
#   FALLOW_SCOPE_CHANGED_SINCE), and the GitLab predefined CI_* variables.
# Result: the files of fallow_mutation_main in the project directory. The
#   script exits 0, so the analysis and the MR comment still run; the template
#   reads fallow-mutation-status.json at the end of the job. Unsafe artifact
#   paths exit 2 before a stale status can be consumed.

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=mutation_common.sh
source "${SCRIPT_DIR}/mutation_common.sh"

export FALLOW_MUTATION_ROOT="${FALLOW_ROOT:-.}"
export FALLOW_MUTATION_CONFIG="${FALLOW_CONFIG:-}"
export FALLOW_MUTATION_OUT_DIR="${FALLOW_MUTATION_OUT_DIR:-.}"
fallow_mutation_check_artifacts "$FALLOW_MUTATION_OUT_DIR" || exit 2

STAMP_FILE="${FALLOW_MUTATION_ROOT}/.fallow/mutation/ci-cache-key"
RESULTS_FILE="${FALLOW_MUTATION_ROOT}/.fallow/mutation/results.json"

# A merge request pipeline from a fork that runs in the target project gets
# the CI/CD variables of the target project. The tests of the merge request
# run in this step, so the step refuses that pipeline. A pipeline that runs in
# the fork project has only the variables of the fork.
fork_refusal_reason() {
  local source_project=${CI_MERGE_REQUEST_SOURCE_PROJECT_ID:-}
  [ -n "$source_project" ] || return 0
  [ "$source_project" != "${CI_PROJECT_ID:-}" ] || return 0
  printf 'fork-refused'
}

# The template resolves the change scope once, before this step, and the
# analysis, the MR comment and the review read the same diff. An older
# template has no scope block, so the base falls back to its variables.
use_change_scope() {
  export FALLOW_MUTATION_DIFF_FILE="${FALLOW_DIFF_FILE:-}"
  export FALLOW_MUTATION_CHANGED_SINCE="${FALLOW_SCOPE_CHANGED_SINCE:-${FALLOW_CHANGED_SINCE:-${CI_MERGE_REQUEST_DIFF_BASE_SHA:-}}}"
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
  use_change_scope
  DIGEST=$(fallow_mutation_cache_digest "$FALLOW_MUTATION_ROOT")
  check_cache_stamp "$DIGEST"
fi

RC=0
fallow_mutation_main || RC=$?
[ -n "$DIGEST" ] && write_cache_stamp "$DIGEST"

STATUS="${FALLOW_MUTATION_OUT_DIR}/fallow-mutation-status.json"
echo "Mutation testing: $(jq -r '[.outcome, (.not_run_reason // empty)] | join(", ")' "$STATUS") (exit ${RC})"
exit 0
