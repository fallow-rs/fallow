#!/usr/bin/env bash
# Tests of the shared mutation helpers (ci/scripts/mutation_common.sh), of the
# GitLab mutation script and of the mutation wiring of the GitLab template.
# ci/tests/run.sh sources this file, so it uses the pass, fail and assert
# helpers, the DIR and FIXTURES variables, the generated /tmp/fallow-run.sh and
# the run_generated_gitlab_fixture function of run.sh.

echo ""
echo "=== Mutation testing ==="

MUT_SCRIPTS="$DIR/../scripts"
MUT_TMP=$(mktemp -d)
MUT_BIN="$MUT_TMP/bin"
mkdir -p "$MUT_BIN"

mut_assert_eq() {
  local actual="$1" expected="$2" name="$3"
  if [ "$actual" = "$expected" ]; then
    pass "$name"
  else
    fail "$name" "expected '$expected', got '$actual'"
  fi
}

# A fake `fallow`. `fallow mutation ...` logs its arguments, copies
# MOCK_MUTATION_REPORT to the -o path, prints MOCK_MUTATION_STDOUT, and exits
# with MOCK_MUTATION_EXIT.
cat > "$MUT_BIN/fallow" <<'SH'
#!/usr/bin/env bash
printf 'fallow %s\n' "$*" >> "$MOCK_LOG"
if [ "${1:-}" = "mutation" ]; then
  env | grep -E '^(GH_TOKEN|GITHUB_TOKEN|GITLAB_TOKEN|FALLOW_MUTATION_GH_TOKEN|ACTIONS_ID_TOKEN_REQUEST_TOKEN|GITHUB_ENV|BASH_ENV)=' \
    | sed 's/=.*/=<leaked>/' >> "$MOCK_LOG"
  out=""
  prev=""
  for arg in "$@"; do
    [ "$prev" = "-o" ] && out=$arg
    prev=$arg
  done
  if [ -n "${MOCK_MUTATION_DIRTY_CHECK:-}" ] && [ -n "$(git status --porcelain 2>/dev/null)" ]; then
    printf '{"error":true,"message":"dirty worktree","exit_code":2,"code":"FALLOW_MUTATION_DIRTY_WORKTREE"}\n'
    exit 2
  fi
  [ -n "${MOCK_MUTATION_REPORT:-}" ] && [ -n "$out" ] && cp "$MOCK_MUTATION_REPORT" "$out"
  [ -n "${MOCK_MUTATION_STDOUT:-}" ] && printf '%s\n' "$MOCK_MUTATION_STDOUT"
  exit "${MOCK_MUTATION_EXIT:-0}"
fi
exit 0
SH
chmod +x "$MUT_BIN/fallow"

# Runs fallow_mutation_main in a subshell in a new work directory. The
# arguments are VAR=value pairs. Prints the return code.
mut_run_main() {
  local work=$1
  shift
  rm -rf "$work"
  mkdir -p "$work"
  (
    cd "$work" || exit 99
    for pair in "$@"; do export "${pair?}"; done
    export PATH="$MUT_BIN:$PATH" MOCK_LOG="$work/mock.log" TMPDIR="$MUT_TMP"
    : > "$MOCK_LOG"
    # shellcheck source=../scripts/mutation_common.sh
    source "$MUT_SCRIPTS/mutation_common.sh"
    rc=0
    fallow_mutation_main || rc=$?
    printf '%s' "$rc"
  )
}

# Prints the status file as "outcome exit_code not_run_reason".
mut_status() {
  jq -r '"\(.outcome) \(.exit_code) \(.not_run_reason)"' "$1/fallow-mutation-status.json"
}

# --- Exit code mapping ---

(
  # shellcheck source=../scripts/mutation_common.sh
  source "$MUT_SCRIPTS/mutation_common.sh"
  printf '%s %s %s %s %s\n' \
    "$(fallow_mutation_outcome_for_exit 0)" \
    "$(fallow_mutation_outcome_for_exit 1)" \
    "$(fallow_mutation_outcome_for_exit 2)" \
    "$(fallow_mutation_outcome_for_exit 130)" \
    "$(fallow_mutation_outcome_for_exit 3)"
) > "$MUT_TMP/outcomes.txt"
mut_assert_eq "$(cat "$MUT_TMP/outcomes.txt")" "ok gate-failed tool-failed interrupted tool-failed" \
  "mutation: exit codes 0, 1, 2, 128+n and others map to their outcomes"

# --- A partial run with survivors ---

W="$MUT_TMP/partial"
RC=$(mut_run_main "$W" \
  FALLOW_MUTATION_SCOPE=changed \
  FALLOW_MUTATION_DIFF_FILE=/tmp/pr.diff \
  FALLOW_MUTATION_TIME_BUDGET=10m \
  FALLOW_MUTATION_MAX_MUTANTS=2 \
  MOCK_MUTATION_REPORT="$FIXTURES/mutation-partial.json")
LOG=$(cat "$W/mock.log")
mut_assert_eq "$RC" "0" "mutation: a partial run returns 0"
assert_contains "$LOG" "fallow mutation --root . --format json --quiet -o " "mutation: run requests JSON with an output file"
assert_contains "$LOG" "--diff-file /tmp/pr.diff" "mutation: changed scope passes the diff file"
assert_contains "$LOG" "--time-budget 10m" "mutation: time budget reaches the command"
assert_contains "$LOG" "--max-mutants 2" "mutation: mutant limit reaches the command"
assert_not_contains "$LOG" "--allow-dirty" "mutation: run never passes --allow-dirty"
assert_not_contains "$LOG" "fallow mutation tasks" "mutation: the run starts no tasks command"
mut_assert_eq "$(mut_status "$W")" "ok 0 null" "mutation: status records the outcome and the exit code"
mut_assert_eq "$(jq -c 'keys' "$W/fallow-mutation-status.json")" '["exit_code","not_run_reason","outcome"]' \
  "mutation: the status file has only closed fields"
mut_assert_eq "$(cmp -s "$W/fallow-mutation.json" "$FIXTURES/mutation-partial.json" && echo same)" "same" \
  "mutation: the report is copied to the output directory"
[ ! -e "$W/fallow-mutation-section.md" ] && [ ! -e "$W/fallow-mutation-tasks.json" ] \
  && pass "mutation: the run writes no section file and no tasks file" \
  || fail "mutation: the run writes no section file and no tasks file" "a section or tasks file exists"

# --- Stale result files of an older script ---

W="$MUT_TMP/stale"
rm -rf "$W"; mkdir -p "$W"
printf 'old\n' > "$W/fallow-mutation-section.md"
printf '[]\n' > "$W/fallow-mutation-tasks.json"
(
  cd "$W" || exit 99
  export PATH="$MUT_BIN:$PATH" MOCK_LOG="$W/mock.log" TMPDIR="$MUT_TMP"
  export FALLOW_MUTATION_SCOPE=all MOCK_MUTATION_REPORT="$FIXTURES/mutation-clean.json"
  # shellcheck source=../scripts/mutation_common.sh
  source "$MUT_SCRIPTS/mutation_common.sh"
  fallow_mutation_main > /dev/null 2>&1 || true
)
[ ! -e "$W/fallow-mutation-section.md" ] && [ ! -e "$W/fallow-mutation-tasks.json" ] \
  && pass "mutation: the run removes the section and tasks files of an older script" \
  || fail "mutation: the run removes the section and tasks files of an older script" "a stale file exists"

# --- A complete clean run ---

W="$MUT_TMP/clean"
RC=$(mut_run_main "$W" FALLOW_MUTATION_SCOPE=all MOCK_MUTATION_REPORT="$FIXTURES/mutation-clean.json")
mut_assert_eq "$RC" "0" "mutation: a clean run returns 0"
assert_contains "$(cat "$W/mock.log")" " --all" "mutation: all scope passes --all"

# --- Score gate (exit 1) ---

W="$MUT_TMP/gate"
RC=$(mut_run_main "$W" FALLOW_MUTATION_SCOPE=all FALLOW_MUTATION_MIN_SCORE=90 \
  MOCK_MUTATION_EXIT=1 MOCK_MUTATION_REPORT="$FIXTURES/mutation-partial.json")
mut_assert_eq "$RC" "1" "mutation: a failed score gate returns 1"
assert_contains "$(cat "$W/mock.log")" "--min-score 90" "mutation: minimum score reaches the command"
mut_assert_eq "$(mut_status "$W")" "gate-failed 1 null" "mutation: a failed score gate is gate-failed"

# --- Tool failure (exit 2) ---

W="$MUT_TMP/tool"
RC=$(mut_run_main "$W" FALLOW_MUTATION_SCOPE=all MOCK_MUTATION_EXIT=2 \
  MOCK_MUTATION_STDOUT='{"error":true,"message":"vitest 3.2.4 is not supported","exit_code":2,"code":"FALLOW_MUTATION_RUNNER_UNSUPPORTED","help":"install vitest 4.1.0"}')
mut_assert_eq "$RC" "2" "mutation: exit code 2 returns 2"
mut_assert_eq "$(mut_status "$W")" "tool-failed 2 null" "mutation: exit code 2 is a tool failure"
mut_assert_eq "$(jq -r '.code + " " + .help' "$W/fallow-mutation.json")" \
  "FALLOW_MUTATION_RUNNER_UNSUPPORTED install vitest 4.1.0" \
  "mutation: on exit 2, fallow-mutation.json holds the error envelope of stdout"

# --- Tool failure without an error envelope ---

W="$MUT_TMP/tool-stderr"
rm -rf "$W"; mkdir -p "$W"
(
  cd "$W" || exit 99
  # A fake binary that writes test output with a secret to stderr.
  mkdir -p bin
  printf '#!/usr/bin/env bash\necho "TOKEN=hunter2 from a test" >&2\nexit 2\n' > bin/fallow
  chmod +x bin/fallow
  export PATH="$W/bin:$PATH" TMPDIR="$MUT_TMP" FALLOW_MUTATION_SCOPE=all
  # shellcheck source=../scripts/mutation_common.sh
  source "$MUT_SCRIPTS/mutation_common.sh"
  fallow_mutation_main > /dev/null 2>&1 || true
)
mut_assert_eq "$(jq -r '.code' "$W/fallow-mutation.json")" "FALLOW_MUTATION_NO_REPORT" \
  "mutation: a failure with no report and no envelope gets a fixed-text envelope"
assert_not_contains "$(cat "$W/fallow-mutation.json" "$W/fallow-mutation-status.json")" "hunter2" \
  "mutation: stderr of the run never reaches a file that the render reads"
assert_contains "$(cat "$W/fallow-mutation-stderr.log")" "hunter2" "mutation: stderr of the run stays in the log file"

# --- Signal ---

W="$MUT_TMP/signal"
RC=$(mut_run_main "$W" FALLOW_MUTATION_SCOPE=all MOCK_MUTATION_EXIT=130 MOCK_MUTATION_REPORT="$FIXTURES/mutation-partial.json")
mut_assert_eq "$RC" "2" "mutation: a signal returns 2"
mut_assert_eq "$(mut_status "$W")" "interrupted 130 null" "mutation: a signal with a report keeps the report"

W="$MUT_TMP/signal-empty"
RC=$(mut_run_main "$W" FALLOW_MUTATION_SCOPE=all MOCK_MUTATION_EXIT=143)
mut_assert_eq "$(mut_status "$W")" "interrupted 143 interrupted" \
  "mutation: a signal with no report gives the not-run reason interrupted"

# --- Changed scope without a diff or a base ref ---

W="$MUT_TMP/skipped"
RC=$(mut_run_main "$W" FALLOW_MUTATION_SCOPE=changed)
mut_assert_eq "$RC" "0" "mutation: a changed scope without a base returns 0"
mut_assert_eq "$(mut_status "$W")" "skipped null no-change-scope" "mutation: a changed scope without a base is skipped"
mut_assert_eq "$(grep -c 'fallow mutation' "$W/mock.log")" "0" "mutation: a skipped run does not start fallow"

W="$MUT_TMP/since"
mut_run_main "$W" FALLOW_MUTATION_SCOPE=changed FALLOW_MUTATION_CHANGED_SINCE=abc123 MOCK_MUTATION_REPORT="$FIXTURES/mutation-clean.json" > /dev/null
assert_contains "$(cat "$W/mock.log")" "--changed-since abc123" "mutation: changed scope falls back to the base ref"

# --- Path scope and input guards ---

W="$MUT_TMP/paths"
mut_run_main "$W" "FALLOW_MUTATION_SCOPE=src/a.ts, src/lib" MOCK_MUTATION_REPORT="$FIXTURES/mutation-clean.json" > /dev/null
assert_contains "$(cat "$W/mock.log")" "--quiet -o " "mutation: path scope still writes the report"
assert_contains "$(cat "$W/mock.log")" " src/a.ts src/lib" "mutation: path scope passes each path"

W="$MUT_TMP/badpath"
RC=$(mut_run_main "$W" "FALLOW_MUTATION_SCOPE=src --allow-dirty")
mut_assert_eq "$RC" "2" "mutation: a scope path that starts with - is refused"
mut_assert_eq "$(grep -c 'fallow mutation' "$W/mock.log")" "0" "mutation: a bad scope path does not start fallow"
mut_assert_eq "$(mut_status "$W") $(jq -r '.code' "$W/fallow-mutation.json")" \
  "tool-failed null null FALLOW_MUTATION_INVALID_SCOPE" "mutation: a bad scope path is a tool failure with an envelope"

# --- Environment of the tests ---

(
  # shellcheck source=../scripts/mutation_common.sh
  source "$MUT_SCRIPTS/mutation_common.sh"
  printf '%s\n' "${FALLOW_MUTATION_UNSET_ENV[@]}"
) > "$MUT_TMP/unset.txt"
for var in GITHUB_ENV GITHUB_OUTPUT GITHUB_PATH GITHUB_STEP_SUMMARY GITHUB_STATE BASH_ENV GITLAB_TOKEN FALLOW_MUTATION_GH_TOKEN; do
  if grep -qx "$var" "$MUT_TMP/unset.txt"; then
    pass "mutation: the run removes $var from the environment of the tests"
  else
    fail "mutation: the run removes $var from the environment of the tests" "missing from FALLOW_MUTATION_UNSET_ENV"
  fi
done
grep -qx CI_JOB_TOKEN "$MUT_TMP/unset.txt" \
  && fail "mutation: CI_JOB_TOKEN stays for package registries" "CI_JOB_TOKEN is removed" \
  || pass "mutation: CI_JOB_TOKEN stays for package registries"

# --- Render arguments ---

# Prints the render arguments on one line. The arguments are VAR=value pairs.
mut_render_args() {
  (
    for pair in "$@"; do export "${pair?}"; done
    # shellcheck source=../scripts/mutation_common.sh
    source "$MUT_SCRIPTS/mutation_common.sh"
    fallow_mutation_render_args "${MUT_RENDER_DIR:-}" | tr '\n' ' '
  )
}

RW="$MUT_TMP/render"
mkdir -p "$RW"
cp "$FIXTURES/mutation-clean.json" "$RW/fallow-mutation.json"
printf '{"outcome":"ok","exit_code":0,"not_run_reason":null}\n' > "$RW/fallow-mutation-status.json"
mut_assert_eq "$(MUT_RENDER_DIR="$RW" mut_render_args FALLOW_HAS_MUTATION_REPORT=false)" "" \
  "mutation render args: a CLI without --mutation-report gets no argument"
mut_assert_eq "$(MUT_RENDER_DIR="$RW" mut_render_args FALLOW_HAS_MUTATION_REPORT=true)" \
  "--mutation-report $RW/fallow-mutation.json " "mutation render args: a report gives --mutation-report"
printf '{"outcome":"refused","exit_code":null,"not_run_reason":"fork-refused"}\n' > "$RW/fallow-mutation-status.json"
mut_assert_eq "$(MUT_RENDER_DIR="$RW" mut_render_args FALLOW_HAS_MUTATION_REPORT=true)" \
  "--mutation-not-run fork-refused " "mutation render args: a not-run reason gives --mutation-not-run"
printf '{"outcome":"refused","exit_code":null,"not_run_reason":"--format sarif"}\n' > "$RW/fallow-mutation-status.json"
mut_assert_eq "$(MUT_RENDER_DIR="$RW" mut_render_args FALLOW_HAS_MUTATION_REPORT=true)" \
  "--mutation-report $RW/fallow-mutation.json " "mutation render args: a reason outside the closed set is ignored"
mut_assert_eq "$(mut_render_args FALLOW_HAS_MUTATION_REPORT=true)" "" \
  "mutation render args: a job with no mutation step gets no argument"

# --- GitLab render arguments (gitlab_common.sh) ---

# Prints FALLOW_RENDER_ARGS of prepare_fallow_render_args on one line. The
# arguments are VAR=value pairs.
mut_gitlab_render_args() {
  (
    cd "$RW" || exit 99
    printf '{}\n' > fallow-results.json
    for pair in "$@"; do export "${pair?}"; done
    export FALLOW_DIFF_FILE=/tmp/mr.diff
    # shellcheck source=../scripts/gitlab_common.sh
    source "$MUT_SCRIPTS/gitlab_common.sh"
    prepare_fallow_render_args pr-comment-gitlab
    printf '%s ' "${FALLOW_RENDER_ARGS[@]}"
  )
}

printf '{"outcome":"ok","exit_code":0,"not_run_reason":null}\n' > "$RW/fallow-mutation-status.json"
GL_ARGS=$(mut_gitlab_render_args FALLOW_HAS_MUTATION_REPORT=true \
  FALLOW_MUTATION_STATUS_FILE="$RW/fallow-mutation-status.json" FALLOW_MUTATION_RESULTS_FILE="$RW/fallow-mutation.json")
assert_contains "$GL_ARGS" "--format pr-comment-gitlab --mutation-report $RW/fallow-mutation.json" \
  "gitlab render args: the comment and the review get the mutation report"
GL_ARGS=$(mut_gitlab_render_args FALLOW_HAS_MUTATION_REPORT=false \
  FALLOW_MUTATION_STATUS_FILE="$RW/fallow-mutation-status.json" FALLOW_MUTATION_RESULTS_FILE="$RW/fallow-mutation.json")
assert_not_contains "$GL_ARGS" "--mutation" "gitlab render args: a CLI without --mutation-report gets no argument"
GL_ARGS=$(mut_gitlab_render_args FALLOW_HAS_MUTATION_REPORT=true FALLOW_COMMAND=fix \
  FALLOW_MUTATION_STATUS_FILE="$RW/fallow-mutation-status.json" FALLOW_MUTATION_RESULTS_FILE="$RW/fallow-mutation.json")
assert_not_contains "$GL_ARGS" "--mutation" "gitlab render args: a fix envelope gets no mutation argument"

for script in comment.sh review.sh gitlab_common.sh mutation.sh mutation_common.sh; do
  if grep -qE 'fallow_mutation_insert_section|FALLOW_MUTATION_SECTION_FILE|FALLOW_MUTATION_CLEAN|fallow_mutation_status_message' \
      "$MUT_SCRIPTS/$script"; then
    fail "gitlab scripts: $script has no section splice" "found splice or section code"
  else
    pass "gitlab scripts: $script has no section splice"
  fi
done

# --- GitLab: fork refusal ---

mut_run_gitlab() {
  local work=$1
  shift
  rm -rf "$work"
  mkdir -p "$work"
  (
    cd "$work" || exit 99
    git init -q
    for pair in "$@"; do export "${pair?}"; done
    export PATH="$MUT_BIN:$PATH" MOCK_LOG="$work/mock.log" TMPDIR="$MUT_TMP"
    : > "$MOCK_LOG"
    rc=0
    bash "$MUT_SCRIPTS/mutation.sh" > "$work/stdout.log" 2>&1 || rc=$?
    printf '%s' "$rc"
  )
}

W="$MUT_TMP/gl-fork"
RC=$(mut_run_gitlab "$W" CI_MERGE_REQUEST_IID=7 CI_MERGE_REQUEST_SOURCE_PROJECT_ID=200 CI_PROJECT_ID=100 \
  FALLOW_MUTATION_SCOPE=all MOCK_MUTATION_REPORT="$FIXTURES/mutation-clean.json")
mut_assert_eq "$RC" "0" "gitlab mutation: the script exits 0 so the analysis still runs"
mut_assert_eq "$(mut_status "$W")" "refused null fork-refused" \
  "gitlab mutation: a fork MR in the target project is refused with fork-refused"
mut_assert_eq "$(grep -c 'fallow mutation' "$W/mock.log")" "0" "gitlab mutation: a refused run does not start the tests"
assert_not_contains "$(cat "$W/stdout.log")" "Unknown mutation refuse reason" \
  "gitlab mutation: the refusal uses a closed reason"

W="$MUT_TMP/gl-token"
mut_run_gitlab "$W" GITLAB_TOKEN=secret-token GITHUB_ENV=/tmp/env BASH_ENV=/tmp/rc FALLOW_MUTATION_SCOPE=all \
  MOCK_MUTATION_REPORT="$FIXTURES/mutation-clean.json" > /dev/null
assert_not_contains "$(cat "$W/mock.log")" "<leaked>" "gitlab mutation: the tests of the project get no token and no env file"

W="$MUT_TMP/gl-forkproject"
mut_run_gitlab "$W" CI_MERGE_REQUEST_IID=7 CI_MERGE_REQUEST_SOURCE_PROJECT_ID=200 CI_PROJECT_ID=200 \
  FALLOW_MUTATION_SCOPE=all MOCK_MUTATION_REPORT="$FIXTURES/mutation-clean.json" > /dev/null
mut_assert_eq "$(jq -r '.outcome' "$W/fallow-mutation-status.json")" "ok" \
  "gitlab mutation: a pipeline in the fork project runs"

# --- GitLab: changed scope uses the scope values of the template ---

W="$MUT_TMP/gl-scope-diff"
mut_run_gitlab "$W" FALLOW_DIFF_FILE=/tmp/scope/mr.diff FALLOW_SCOPE_CHANGED_SINCE=abc123 \
  CI_MERGE_REQUEST_DIFF_BASE_SHA=def456 MOCK_MUTATION_REPORT="$FIXTURES/mutation-clean.json" > /dev/null
assert_contains "$(cat "$W/mock.log")" "--diff-file /tmp/scope/mr.diff" "gitlab mutation: changed scope passes the scope diff"
assert_not_contains "$(cat "$W/mock.log")" "--changed-since" "gitlab mutation: the scope diff wins over the base ref"

W="$MUT_TMP/gl-scope-since"
mut_run_gitlab "$W" FALLOW_SCOPE_CHANGED_SINCE=abc123 CI_MERGE_REQUEST_DIFF_BASE_SHA=def456 \
  MOCK_MUTATION_REPORT="$FIXTURES/mutation-clean.json" > /dev/null
assert_contains "$(cat "$W/mock.log")" "--changed-since abc123" "gitlab mutation: without a diff, changed scope passes the scope base"
[ -z "$(find "$MUT_TMP" -maxdepth 1 -name 'fallow-mutation-mr.diff')" ] \
  && pass "gitlab mutation: the script computes no diff of its own" \
  || fail "gitlab mutation: the script computes no diff of its own" "found fallow-mutation-mr.diff"

W="$MUT_TMP/gl-scope-none"
mut_run_gitlab "$W" MOCK_MUTATION_REPORT="$FIXTURES/mutation-clean.json" > /dev/null
mut_assert_eq "$(mut_status "$W")" "skipped null no-change-scope" \
  "gitlab mutation: changed scope without a diff or a base is skipped"

# --- GitLab: cache stamp ---

W="$MUT_TMP/gl-stamp"
rm -rf "$W"; mkdir -p "$W/.fallow/mutation"
printf '{"lockfileVersion":3}\n' > "$W/package-lock.json"
printf '{}\n' > "$W/.fallow/mutation/results.json"
printf 'stale\n' > "$W/.fallow/mutation/ci-cache-key"
(
  cd "$W" || exit 99
  export PATH="$MUT_BIN:$PATH" MOCK_LOG="$W/mock.log" TMPDIR="$MUT_TMP"
  export FALLOW_MUTATION_SCOPE=all MOCK_MUTATION_REPORT="$FIXTURES/mutation-clean.json"
  : > "$MOCK_LOG"
  bash "$MUT_SCRIPTS/mutation.sh" > "$W/stdout.log" 2>&1
)
[ ! -f "$W/.fallow/mutation/results.json" ] && pass "gitlab mutation: a cache of other lockfiles is removed before the run" \
  || fail "gitlab mutation: a cache of other lockfiles is removed before the run" "results.json still exists"
STAMP=$(cat "$W/.fallow/mutation/ci-cache-key")
EXPECTED=$(cd "$W" && bash -c "source '$MUT_SCRIPTS/mutation_common.sh'; fallow_mutation_cache_digest .")
mut_assert_eq "$STAMP" "$EXPECTED" "gitlab mutation: the stamp records the digest of this run"
printf '{}\n' > "$W/.fallow/mutation/results.json"
(
  cd "$W" || exit 99
  export PATH="$MUT_BIN:$PATH" MOCK_LOG="$W/mock.log" TMPDIR="$MUT_TMP"
  export FALLOW_MUTATION_SCOPE=all MOCK_MUTATION_REPORT="$FIXTURES/mutation-clean.json"
  bash "$MUT_SCRIPTS/mutation.sh" > /dev/null 2>&1
)
[ -f "$W/.fallow/mutation/results.json" ] && pass "gitlab mutation: a cache of the same lockfiles is kept" \
  || fail "gitlab mutation: a cache of the same lockfiles is kept" "results.json was removed"
printf '{"lockfileVersion":3,"x":1}\n' > "$W/package-lock.json"
NEW=$(cd "$W" && bash -c "source '$MUT_SCRIPTS/mutation_common.sh'; fallow_mutation_cache_digest .")
[ "$NEW" != "$EXPECTED" ] && pass "mutation: the cache digest covers the lockfile" \
  || fail "mutation: the cache digest covers the lockfile" "digest did not change"
printf 'export default {}\n' > "$W/vitest.config.ts"
NEWER=$(cd "$W" && bash -c "source '$MUT_SCRIPTS/mutation_common.sh'; fallow_mutation_cache_digest .")
[ "$NEWER" != "$NEW" ] && pass "mutation: the cache digest covers the runner config" \
  || fail "mutation: the cache digest covers the runner config" "digest did not change"

# --- GitLab template wiring ---

GL_FILE="$DIR/../gitlab-ci.yml"
GL_TEMPLATE=$(cat "$GL_FILE")
assert_contains "$GL_TEMPLATE" 'FALLOW_MUTATION: "false"' "gitlab template: mutation is off by default"
assert_contains "$GL_TEMPLATE" 'FALLOW_MUTATION_TIME_BUDGET: "10m"' "gitlab template: the default has a time budget"
mut_assert_eq "$(grep -c 'for f in comment.sh review.sh gitlab_common.sh mutation.sh mutation_common.sh; do' "$GL_FILE")" "2" \
  "gitlab template: both script preparation lists get the same scripts"
assert_contains "$GL_TEMPLATE" "- fallow-mutation.json" "gitlab template: the mutation report is an artifact"
assert_not_contains "$GL_TEMPLATE" "fallow-mutation-section.md" "gitlab template: no section file is an artifact"
assert_not_contains "$GL_TEMPLATE" "fallow-mutation-tasks.json" "gitlab template: no tasks file is an artifact"
assert_not_contains "$GL_TEMPLATE" "--allow-dirty" "gitlab template: never passes --allow-dirty"
assert_not_contains "$GL_TEMPLATE" "fallow_mutation_status_message" "gitlab template: no shell text for the mutation status"
assert_contains "$GL_TEMPLATE" 'mutation-min-score)' "gitlab template: gate_owning_value knows the mutation gate"

mut_line() {
  grep -nF -- "$1" "$GL_FILE" | head -1 | cut -d: -f1
}
SCOPE_LINE=$(mut_line 'fallow-scope/mr.diff')
MUT_LINE=$(mut_line 'bash /tmp/fallow-scripts/mutation.sh')
COPY_LINE=$(mut_line '" fallow-mr.diff')
ANALYSIS_LINE=$(mut_line 'fallow "${RUN_ARGS[@]}" > fallow-results.json')
if [ -n "$SCOPE_LINE" ] && [ -n "$MUT_LINE" ] && [ -n "$COPY_LINE" ] && [ -n "$ANALYSIS_LINE" ] \
    && [ "$SCOPE_LINE" -lt "$MUT_LINE" ] && [ "$MUT_LINE" -lt "$COPY_LINE" ] && [ "$COPY_LINE" -lt "$ANALYSIS_LINE" ]; then
  pass "gitlab template: the scope block, the mutation run, the diff copy and the analysis run in this order"
else
  fail "gitlab template: the scope block, the mutation run, the diff copy and the analysis run in this order" \
    "scope ${SCOPE_LINE:-none}, mutation ${MUT_LINE:-none}, copy ${COPY_LINE:-none}, analysis ${ANALYSIS_LINE:-none}"
fi
if grep -qE 'git diff .*> fallow-mr\.diff' "$GL_FILE"; then
  fail "gitlab template: the project directory gets a copy of the scope diff, not a second diff" "found a git diff into fallow-mr.diff"
else
  pass "gitlab template: the project directory gets a copy of the scope diff, not a second diff"
fi
RESULT_LINE=$(mut_line '# ── Mutation result')
DOTENV_LINE=$(mut_line '} > fallow-gates.env')
if [ -n "$RESULT_LINE" ] && [ -n "$DOTENV_LINE" ] && [ "$RESULT_LINE" -lt "$DOTENV_LINE" ]; then
  pass "gitlab template: fallow-gates.env is written after the mutation result block"
else
  fail "gitlab template: fallow-gates.env is written after the mutation result block" \
    "result ${RESULT_LINE:-none}, dotenv ${DOTENV_LINE:-none}"
fi

# --- GitLab template: the generated script with mutation testing ---

# The generated script reads the scripts of the preparation step from
# /tmp/fallow-scripts.
mkdir -p /tmp/fallow-scripts
for script in comment.sh review.sh gitlab_common.sh mutation.sh mutation_common.sh; do
  cp "$MUT_SCRIPTS/$script" "/tmp/fallow-scripts/$script"
  chmod +x "/tmp/fallow-scripts/$script"
done

# A fake `fallow` before the runner stub of run.sh. It answers the capability
# probe, runs `fallow mutation` like the stub above, and passes every other
# call to the runner stub.
MUT_GL_BIN="$MUT_TMP/gl-bin"
mkdir -p "$MUT_GL_BIN"
cat > "$MUT_GL_BIN/fallow" <<SH
#!/usr/bin/env bash
printf 'fallow %s\n' "\$*" >> "\$MOCK_LOG"
if [ "\${1:-} \${2:-}" = "report --help" ]; then
  printf 'Usage: fallow report [OPTIONS] --from <PATH>\n'
  [ "\${MOCK_HAS_MUTATION_REPORT:-}" = "1" ] && printf '      --mutation-report <PATH>\n'
  exit 0
fi
if [ "\${1:-}" = "mutation" ]; then
  exec "$MUT_BIN/fallow" "\$@"
fi
exec "$RUNNER_TMP/bin/fallow" "\$@"
SH
chmod +x "$MUT_GL_BIN/fallow"

MUT_ENVELOPE="$MUT_TMP/analysis.json"
printf '%s\n' '{"kind":"dead-code","schema_version":9,"version":"3.42.0","total_issues":0,"summary":{"total_issues":0},"unused_files":[],"unused_exports":[]}' \
  > "$MUT_ENVELOPE"
jq '.gate_outcomes = {"mutation-min-score": {"status": "fail", "enforced": true, "observed": 50.0, "threshold": 90.0}}' \
  "$FIXTURES/mutation-partial.json" > "$MUT_TMP/mutation-gate.json"

# Runs the generated script in a new work directory with mutation testing on.
# The arguments are VAR=value pairs. Prints the output and sets MUT_GL_STATUS.
mut_run_template() {
  local work=$1
  shift
  rm -rf "$work"
  mkdir -p "$work"
  MUT_GL_STATUS=0
  run_generated_gitlab_fixture "$work" \
    PATH="$MUT_GL_BIN:$RUNNER_TMP/bin:$PATH" \
    MOCK_LOG="$MUT_TMP/$(basename "$work").log" \
    MOCK_GATE_ENVELOPE="$MUT_ENVELOPE" \
    TMPDIR="$MUT_TMP/$(basename "$work")-tmp" \
    FALLOW_COMMAND=dead-code \
    FALLOW_MUTATION=true \
    FALLOW_MUTATION_SCOPE=all \
    FALLOW_CODEQUALITY=true \
    "$@" > "$work/output.log" || MUT_GL_STATUS=$?
}

W="$MUT_TMP/gl-gate"
mkdir -p "$MUT_TMP/gl-gate-tmp"
mut_run_template "$W" MOCK_HAS_MUTATION_REPORT=1 MOCK_MUTATION_EXIT=1 \
  MOCK_MUTATION_REPORT="$MUT_TMP/mutation-gate.json" FALLOW_MUTATION_MIN_SCORE=90
OUT=$(cat "$W/output.log")
mut_assert_eq "$MUT_GL_STATUS" "1" "gitlab template: a failed mutation score gate exits 1"
assert_contains "$OUT" "ERROR: Fallow mutation-min-score gate failed: mutation score 50% is below the minimum 90%." \
  "gitlab template: the job log names the mutation score and the minimum"
mut_assert_eq "$(grep -c 'mutation-min-score gate failed' "$W/output.log")" "1" \
  "gitlab template: the mutation gate failure prints once"
mut_assert_eq "$(grep '^FALLOW_GATES_FAILED=' "$W/fallow-gates.env")" "FALLOW_GATES_FAILED=mutation-min-score" \
  "gitlab template: fallow-gates.env lists mutation-min-score in FALLOW_GATES_FAILED"
mut_assert_eq "$(grep '^FALLOW_MUTATION_OUTCOME=' "$W/fallow-gates.env")" "FALLOW_MUTATION_OUTCOME=gate-failed" \
  "gitlab template: fallow-gates.env keeps FALLOW_MUTATION_OUTCOME"
assert_contains "$(grep -- '--format codeclimate' "$MUT_TMP/gl-gate.log")" "--mutation-report $W/fallow-mutation.json" \
  "gitlab template: the Code Quality render gets the mutation report"

W="$MUT_TMP/gl-pass"
mkdir -p "$MUT_TMP/gl-pass-tmp"
jq '.gate_outcomes["mutation-min-score"].status = "pass" | .gate_outcomes["mutation-min-score"].threshold = 40.0' \
  "$MUT_TMP/mutation-gate.json" > "$MUT_TMP/mutation-pass.json"
mut_run_template "$W" MOCK_HAS_MUTATION_REPORT=1 MOCK_MUTATION_REPORT="$MUT_TMP/mutation-pass.json" FALLOW_MUTATION_MIN_SCORE=40
mut_assert_eq "$MUT_GL_STATUS" "0" "gitlab template: a passed mutation score gate exits 0"
mut_assert_eq "$(grep '^FALLOW_GATES_PASSED=' "$W/fallow-gates.env")" "FALLOW_GATES_PASSED=mutation-min-score" \
  "gitlab template: fallow-gates.env lists a passed mutation gate in FALLOW_GATES_PASSED"

W="$MUT_TMP/gl-oldcli"
mkdir -p "$MUT_TMP/gl-oldcli-tmp"
mut_run_template "$W" MOCK_MUTATION_EXIT=1 MOCK_MUTATION_REPORT="$FIXTURES/mutation-partial.json" FALLOW_MUTATION_MIN_SCORE=90
OUT=$(cat "$W/output.log")
mut_assert_eq "$MUT_GL_STATUS" "1" "gitlab template: with an older CLI, a failed mutation gate still exits 1"
assert_contains "$OUT" "ERROR: Fallow mutation-min-score gate failed" \
  "gitlab template: with an older CLI, the job log names the failed mutation gate"
assert_not_contains "$(cat "$MUT_TMP/gl-oldcli.log")" "--mutation-" \
  "gitlab template: a CLI without --mutation-report gets no mutation flag"
assert_contains "$OUT" "cannot render them. Use fallow 3." \
  "gitlab template: with an older CLI, the job log names the upgrade"
mut_assert_eq "$(grep '^FALLOW_GATES_FAILED=' "$W/fallow-gates.env")" "FALLOW_GATES_FAILED=" \
  "gitlab template: a report without gate_outcomes adds no gate name"

W="$MUT_TMP/gl-forkmr"
mkdir -p "$MUT_TMP/gl-forkmr-tmp"
mut_run_template "$W" MOCK_HAS_MUTATION_REPORT=1 MOCK_MUTATION_REPORT="$FIXTURES/mutation-clean.json" \
  CI_MERGE_REQUEST_IID=7 CI_MERGE_REQUEST_SOURCE_PROJECT_ID=200 CI_PROJECT_ID=100
mut_assert_eq "$MUT_GL_STATUS" "2" "gitlab template: a refused mutation run exits 2"
assert_contains "$(grep -- '--format codeclimate' "$MUT_TMP/gl-forkmr.log")" "--mutation-not-run fork-refused" \
  "gitlab template: a fork pipeline gives --mutation-not-run fork-refused"
assert_contains "$(cat "$W/output.log")" "ERROR: Mutation testing did not run" \
  "gitlab template: the job log says that mutation testing did not run"

W="$MUT_TMP/gl-toolfail"
mkdir -p "$MUT_TMP/gl-toolfail-tmp"
mut_run_template "$W" MOCK_HAS_MUTATION_REPORT=1 MOCK_MUTATION_EXIT=2 \
  MOCK_MUTATION_STDOUT='{"error":true,"message":"vitest 3.2.4 is not supported","exit_code":2,"code":"FALLOW_MUTATION_RUNNER_UNSUPPORTED"}'
mut_assert_eq "$MUT_GL_STATUS" "2" "gitlab template: a mutation tool failure exits 2"
assert_contains "$(cat "$W/output.log")" "ERROR: fallow mutation failed (exit code 2, FALLOW_MUTATION_RUNNER_UNSUPPORTED)" \
  "gitlab template: the job log names the exit code and the error code"
assert_contains "$(grep -- '--format codeclimate' "$MUT_TMP/gl-toolfail.log")" "--mutation-report $W/fallow-mutation.json" \
  "gitlab template: the Code Quality render gets the error envelope"

# The scope block runs in a merge request with the base commit, in a clean
# worktree. The fake `fallow mutation` refuses a dirty worktree.
W="$MUT_TMP/gl-scope"
rm -rf "$W"; mkdir -p "$W" "$MUT_TMP/gl-scope-tmp"
(
  cd "$W" || exit 99
  git init -q
  printf 'export const b = 1;\n' > b.js
  git add b.js
  git -c user.email=t@e -c user.name=t commit -q -m base
  printf 'export const a = 1;\n' > a.js
  git add a.js
  git -c user.email=t@e -c user.name=t commit -q -m change
) > /dev/null 2>&1
BASE=$(git -C "$W" rev-parse HEAD~1)
MUT_GL_STATUS=0
run_generated_gitlab_fixture "$W" \
  PATH="$MUT_GL_BIN:$RUNNER_TMP/bin:$PATH" \
  MOCK_LOG="$MUT_TMP/gl-scope.log" \
  MOCK_GATE_ENVELOPE="$MUT_ENVELOPE" \
  MOCK_MUTATION_DIRTY_CHECK=1 \
  MOCK_HAS_MUTATION_REPORT=1 \
  MOCK_MUTATION_REPORT="$FIXTURES/mutation-clean.json" \
  TMPDIR="$MUT_TMP/gl-scope-tmp" \
  FALLOW_ROOT=. \
  FALLOW_COMMAND=dead-code \
  FALLOW_MUTATION=true \
  FALLOW_MUTATION_SCOPE=changed \
  FALLOW_CODEQUALITY=true \
  CI_MERGE_REQUEST_IID=7 \
  CI_MERGE_REQUEST_DIFF_BASE_SHA="$BASE" > "$MUT_TMP/gl-scope-output.log" || MUT_GL_STATUS=$?
SCOPE_LOG=$(cat "$MUT_TMP/gl-scope.log")
mut_assert_eq "$(jq -r '.outcome' "$W/fallow-mutation-status.json" 2>/dev/null)" "ok" \
  "gitlab template: the scope block writes nothing into the worktree before the mutation run"
assert_contains "$SCOPE_LOG" "--diff-file $MUT_TMP/gl-scope-tmp/fallow-scope/mr.diff" \
  "gitlab template: the mutation run gets the scope diff"
assert_contains "$(cat "$MUT_TMP/gl-scope-tmp/fallow-scope/mr.diff" 2>/dev/null)" "+++ b/a.js" \
  "gitlab template: the scope diff names the changed file"
mut_assert_eq "$(cmp -s "$W/fallow-mr.diff" "$MUT_TMP/gl-scope-tmp/fallow-scope/mr.diff" && echo same)" "same" \
  "gitlab template: fallow-mr.diff is a copy of the scope diff"
assert_contains "$(grep -- '--changed-since' "$MUT_TMP/gl-scope.log" | grep -v 'fallow mutation' | head -1)" "--changed-since $BASE" \
  "gitlab template: the analysis keeps the merge request base"
mut_assert_eq "$MUT_GL_STATUS" "0" "gitlab template: a clean mutation run in a merge request exits 0"

W="$MUT_TMP/gl-off"
mkdir -p "$MUT_TMP/gl-off-tmp"
mut_run_template "$W" FALLOW_MUTATION=false MOCK_HAS_MUTATION_REPORT=1
mut_assert_eq "$MUT_GL_STATUS" "0" "gitlab template: a job without mutation testing exits 0"
assert_not_contains "$(cat "$MUT_TMP/gl-off.log")" "--mutation-" \
  "gitlab template: a job without mutation testing passes no mutation flag"
assert_not_contains "$(cat "$MUT_TMP/gl-off.log")" "report --help" \
  "gitlab template: a job without mutation testing does not probe the CLI"

# Artifact cleanup must not follow a checkout link before input validation.
for shape in parent leaf dangling directory; do
  W="$MUT_TMP/artifact-$shape"
  mkdir -p "$W/work" "$W/outside"
  printf 'preserve outside report\n' > "$W/outside/fallow-mutation.json"
  case "$shape" in
    parent) ln -s "$W/outside" "$W/work/out" ;;
    *)
      mkdir -p "$W/work/out"
      case "$shape" in
        leaf) ln -s "$W/outside/fallow-mutation.json" "$W/work/out/fallow-mutation.json" ;;
        dangling) ln -s "$W/missing" "$W/work/out/fallow-mutation.json" ;;
        directory) mkdir "$W/work/out/fallow-mutation.json" ;;
      esac
      ;;
  esac
  RC=0
  (
    cd "$W/work" || exit 99
    source "$MUT_SCRIPTS/mutation_common.sh"
    FALLOW_MUTATION_OUT_DIR=out FALLOW_MUTATION_REFUSE_REASON=test fallow_mutation_main
  ) > "$W/log" 2>&1 || RC=$?
  mut_assert_eq "$RC" 2 "mutation artifacts: rejects $shape before cleanup"
  mut_assert_eq "$(cat "$W/outside/fallow-mutation.json")" 'preserve outside report' \
    "mutation artifacts: $shape preserves the outside report"
  assert_contains "$(cat "$W/log")" '::error::Artifact' "mutation artifacts: $shape explains the refusal"
done

W="$MUT_TMP/artifact-regular"
mkdir -p "$W"
for attempt in first repeated; do
  RC=0
  (
    cd "$W" || exit 99
    source "$MUT_SCRIPTS/mutation_common.sh"
    FALLOW_MUTATION_OUT_DIR=reports/nested FALLOW_MUTATION_REFUSE_REASON=test fallow_mutation_main
  ) > "$W/log" 2>&1 || RC=$?
  mut_assert_eq "$RC" 2 "mutation artifacts: $attempt run reports the requested refusal"
  mut_assert_eq "$(jq -r .outcome "$W/reports/nested/fallow-mutation-status.json")" refused \
    "mutation artifacts: $attempt run writes its status through regular paths"
done

rm -rf "$MUT_TMP"
