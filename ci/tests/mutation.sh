#!/usr/bin/env bash
# Tests of the shared mutation helpers (ci/scripts/mutation_common.sh) and of
# the GitLab mutation script. ci/tests/run.sh sources this file, so it uses
# the pass, fail and assert helpers and the DIR and FIXTURES variables of
# run.sh.

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
# with MOCK_MUTATION_EXIT. `fallow mutation tasks` prints MOCK_MUTATION_TASKS.
cat > "$MUT_BIN/fallow" <<'SH'
#!/usr/bin/env bash
printf 'fallow %s\n' "$*" >> "$MOCK_LOG"
if [ "${1:-}" = "mutation" ] && [ "${2:-}" = "tasks" ]; then
  [ -n "${MOCK_MUTATION_TASKS:-}" ] && cat "$MOCK_MUTATION_TASKS"
  exit 0
fi
if [ "${1:-}" = "mutation" ]; then
  env | grep -E '^(GH_TOKEN|GITHUB_TOKEN|GITLAB_TOKEN|FALLOW_MUTATION_GH_TOKEN|ACTIONS_ID_TOKEN_REQUEST_TOKEN)=' \
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
  MOCK_MUTATION_REPORT="$FIXTURES/mutation-partial.json" \
  MOCK_MUTATION_TASKS="$FIXTURES/mutation-tasks.json")
LOG=$(cat "$W/mock.log")
SECTION=$(cat "$W/fallow-mutation-section.md")
mut_assert_eq "$RC" "0" "mutation: a partial run returns 0"
assert_contains "$LOG" "fallow mutation --root . --format json --quiet -o " "mutation: run requests JSON with an output file"
assert_contains "$LOG" "--diff-file /tmp/pr.diff" "mutation: changed scope passes the diff file"
assert_contains "$LOG" "--time-budget 10m" "mutation: time budget reaches the command"
assert_contains "$LOG" "--max-mutants 2" "mutation: mutant limit reaches the command"
assert_not_contains "$LOG" "--allow-dirty" "mutation: run never passes --allow-dirty"
assert_contains "$LOG" "fallow mutation tasks --report " "mutation: tasks read the saved report"
mut_assert_eq "$(jq -r '.outcome + " " + .completion + " " + (.clean | tostring)' "$W/fallow-mutation-status.json")" \
  "ok partial false" "mutation: status records outcome, completion and clean"
[ -f "$W/fallow-mutation.json" ] && pass "mutation: report is copied to the output directory" \
  || fail "mutation: report is copied to the output directory" "missing fallow-mutation.json"
[ -f "$W/fallow-mutation-tasks.json" ] && pass "mutation: tasks are copied to the output directory" \
  || fail "mutation: tasks are copied to the output directory" "missing fallow-mutation-tasks.json"
assert_contains "$SECTION" "## Mutation testing" "mutation section: heading"
assert_contains "$SECTION" '**Partial run** (`mutant-limit`)' "mutation section: a partial run says partial with the reason"
assert_contains "$SECTION" "| Killed | Survived | No coverage | Not measured | Score |" "mutation section: count table header"
assert_contains "$SECTION" "| 1 | 1 | 9 | 2 | 50% |" "mutation section: killed, survived, no coverage and not measured counts"
assert_contains "$SECTION" 'Budget: time `10m`, 2 mutants.' "mutation section: budget state"
assert_contains "$SECTION" "Top survivors (5 of 10)" "mutation section: top survivors are capped"
assert_contains "$SECTION" '`fallow mutation inspect b7cd788c13dcf918`' "mutation section: survivor has an inspect command"
assert_contains "$SECTION" "+  if (v <= lo) return lo;" "mutation section: survivor shows the mutated line"
FIRST_ITEM=$(printf '%s\n' "$SECTION" | grep -m1 '^1\. ')
assert_contains "$FIRST_ITEM" "(survived)" "mutation section: survived mutants come before no-coverage mutants"

# --- A complete clean run ---

W="$MUT_TMP/clean"
RC=$(mut_run_main "$W" FALLOW_MUTATION_SCOPE=all MOCK_MUTATION_REPORT="$FIXTURES/mutation-clean.json")
SECTION=$(cat "$W/fallow-mutation-section.md")
mut_assert_eq "$RC" "0" "mutation: a clean run returns 0"
assert_contains "$(cat "$W/mock.log")" " --all" "mutation: all scope passes --all"
assert_contains "$SECTION" "The tests detected each mutant that ran." "mutation section: clean headline"
assert_not_contains "$SECTION" "Partial run" "mutation section: a complete run does not say partial"
assert_not_contains "$SECTION" "Top survivors" "mutation section: a clean run lists no survivors"
mut_assert_eq "$(jq -r '.clean' "$W/fallow-mutation-status.json")" "true" "mutation: a clean run is clean"

# --- Score gate (exit 1) ---

W="$MUT_TMP/gate"
RC=$(mut_run_main "$W" FALLOW_MUTATION_SCOPE=all FALLOW_MUTATION_MIN_SCORE=90 \
  MOCK_MUTATION_EXIT=1 MOCK_MUTATION_REPORT="$FIXTURES/mutation-partial.json")
mut_assert_eq "$RC" "1" "mutation: a failed score gate returns 1"
assert_contains "$(cat "$W/mock.log")" "--min-score 90" "mutation: minimum score reaches the command"
assert_contains "$(cat "$W/fallow-mutation-section.md")" "**Score below the minimum.** The score is 50%, and the minimum is 90%." \
  "mutation section: gate failure names the score and the minimum"

# --- Tool failure (exit 2) ---

W="$MUT_TMP/tool"
RC=$(mut_run_main "$W" FALLOW_MUTATION_SCOPE=all MOCK_MUTATION_EXIT=2 \
  MOCK_MUTATION_STDOUT='{"error":true,"message":"vitest 3.2.4 is not supported","exit_code":2,"code":"FALLOW_MUTATION_RUNNER_UNSUPPORTED","help":"install vitest 4.1.0"}')
SECTION=$(cat "$W/fallow-mutation-section.md")
mut_assert_eq "$RC" "2" "mutation: exit code 2 returns 2"
mut_assert_eq "$(jq -r '.outcome + " " + (.exit_code | tostring) + " " + .code' "$W/fallow-mutation-status.json")" \
  "tool-failed 2 FALLOW_MUTATION_RUNNER_UNSUPPORTED" "mutation: exit code 2 is a tool failure, not an uncertain result"
assert_contains "$SECTION" '**Mutation testing failed** (exit code 2, `FALLOW_MUTATION_RUNNER_UNSUPPORTED`).' \
  "mutation section: tool failure names the exit code and the error code"
assert_contains "$SECTION" "vitest 3.2.4 is not supported" "mutation section: tool failure shows the message"
assert_contains "$SECTION" "install vitest 4.1.0" "mutation section: tool failure shows the help"
assert_not_contains "$SECTION" "| Killed |" "mutation section: tool failure shows no count table"

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
assert_not_contains "$(cat "$W/fallow-mutation-section.md")" "hunter2" "mutation section: stderr of the run never reaches the comment"
assert_contains "$(cat "$W/fallow-mutation-stderr.log")" "hunter2" "mutation: stderr of the run stays in the log file"

# --- A very long error message ---

LONG_PATHS=$(printf '.pnpm-store/v10/files/%05d, ' $(seq 1 3000))
W="$MUT_TMP/long"
mut_run_main "$W" FALLOW_MUTATION_SCOPE=all MOCK_MUTATION_EXIT=2 \
  MOCK_MUTATION_STDOUT="$(jq -cn --arg m "the git worktree has 3000 uncommitted paths: $LONG_PATHS" '{error: true, message: $m, exit_code: 2, code: "FALLOW_MUTATION_DIRTY_WORKTREE"}')" > /dev/null
SECTION_CHARS=$(wc -c < "$W/fallow-mutation-section.md" | tr -d ' ')
if [ "$SECTION_CHARS" -lt 3000 ]; then
  pass "mutation section: a long error message is shortened"
else
  fail "mutation section: a long error message is shortened" "section has $SECTION_CHARS characters"
fi
assert_contains "$(cat "$W/fallow-mutation-section.md")" "(shortened; fallow-mutation-status.json of this job has the full text)" \
  "mutation section: a shortened message says where the full text is"
assert_contains "$(jq -r '.message' "$W/fallow-mutation-status.json")" ".pnpm-store/v10/files/03000" \
  "mutation: the status file keeps the full message"

# --- Signal ---

W="$MUT_TMP/signal"
RC=$(mut_run_main "$W" FALLOW_MUTATION_SCOPE=all MOCK_MUTATION_EXIT=130 MOCK_MUTATION_REPORT="$FIXTURES/mutation-partial.json")
mut_assert_eq "$RC" "2" "mutation: a signal returns 2"
assert_contains "$(cat "$W/fallow-mutation-section.md")" "exit code 130" "mutation section: a signal names the exit code"

# --- Changed scope without a diff or a base ref ---

W="$MUT_TMP/skipped"
RC=$(mut_run_main "$W" FALLOW_MUTATION_SCOPE=changed)
mut_assert_eq "$RC" "0" "mutation: a changed scope without a base returns 0"
mut_assert_eq "$(jq -r '.outcome' "$W/fallow-mutation-status.json")" "skipped" "mutation: a changed scope without a base is skipped"
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
assert_contains "$(cat "$W/fallow-mutation-section.md")" "must not start with" "mutation section: a bad input names the problem"

W="$MUT_TMP/badmax"
RC=$(mut_run_main "$W" FALLOW_MUTATION_SCOPE=all FALLOW_MUTATION_MAX_MUTANTS=-5)
mut_assert_eq "$RC" "2" "mutation: a bad mutant limit returns 2"
mut_assert_eq "$(jq -r '.outcome + " " + (.exit_code | tostring)' "$W/fallow-mutation-status.json")" "invalid-input null" \
  "mutation: a bad input is invalid-input, with no exit code of fallow"
assert_contains "$(cat "$W/fallow-mutation-section.md")" "**Mutation testing did not start.** The input is invalid." \
  "mutation section: a bad input says that the run did not start"

# --- A report of an unknown shape ---

printf '{"completion":"oops","summary":{"counts":{}}}\n' > "$MUT_TMP/badshape.json"
W="$MUT_TMP/badshape"
RC=$(mut_run_main "$W" FALLOW_MUTATION_SCOPE=all MOCK_MUTATION_EXIT=1 MOCK_MUTATION_REPORT="$MUT_TMP/badshape.json")
mut_assert_eq "$RC" "2" "mutation: a report of an unknown shape returns 2"
mut_assert_eq "$(jq -r '.outcome' "$W/fallow-mutation-status.json")" "tool-failed" \
  "mutation: a report of an unknown shape is a tool failure, so the gate result is not lost"
[ ! -f "$W/fallow-mutation.json" ] && pass "mutation: a report of an unknown shape is not copied" \
  || fail "mutation: a report of an unknown shape is not copied" "fallow-mutation.json exists"

# --- Markdown safety ---

jq '.mutants[1].file = "src/a`b <img src=x>.js" | .mutants[1].function = "f`n"' \
  "$FIXTURES/mutation-partial.json" > "$MUT_TMP/hostile.json"
W="$MUT_TMP/hostile"
mut_run_main "$W" FALLOW_MUTATION_SCOPE=all MOCK_MUTATION_REPORT="$MUT_TMP/hostile.json" > /dev/null
assert_contains "$(cat "$W/fallow-mutation-section.md")" "\`src/a'b <img src=x>.js:2\`" \
  "mutation section: a backtick in a path cannot end the code span"

# --- Insert the section into the sticky comment ---

printf '<!-- fallow-id: fallow-results -->\n# Fallow report\n\nbody\n\nGenerated by fallow.\n' > "$MUT_TMP/body.md"
jq -n --rawfile b "$MUT_TMP/body.md" '{marker_id: "fallow-results", body: $b, is_clean: true, details_url: null, check_summary: null, truncation: {truncated: false, shown_findings: 0, total_findings: 0}}' \
  > "$MUT_TMP/envelope.json"
printf '## Mutation testing\n\nsection\n' > "$MUT_TMP/section.md"
(
  # shellcheck source=../scripts/mutation_common.sh
  source "$MUT_SCRIPTS/mutation_common.sh"
  fallow_mutation_insert_section "$MUT_TMP/body.md" "$MUT_TMP/envelope.json" "$MUT_TMP/section.md" false
)
mut_assert_eq "$(cat "$MUT_TMP/body.md")" "$(printf '<!-- fallow-id: fallow-results -->\n# Fallow report\n\nbody\n\n## Mutation testing\n\nsection\n\nGenerated by fallow.')" \
  "mutation comment: the section goes above the footer of the body"
mut_assert_eq "$(jq -r '.body' "$MUT_TMP/envelope.json")" "$(cat "$MUT_TMP/body.md")" \
  "mutation comment: the envelope body gets the same section"
mut_assert_eq "$(jq -r '.is_clean' "$MUT_TMP/envelope.json")" "false" \
  "mutation comment: open mutation items make a clean envelope not clean"

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
mut_assert_eq "$(jq -r '.outcome' "$W/fallow-mutation-status.json")" "refused" \
  "gitlab mutation: a fork MR in the target project is refused"
mut_assert_eq "$(grep -c 'fallow mutation' "$W/mock.log")" "0" "gitlab mutation: a refused run does not start the tests"
assert_contains "$(cat "$W/fallow-mutation-section.md")" "**Mutation testing did not run.**" "gitlab mutation: the section says that the run was refused"

W="$MUT_TMP/gl-token"
mut_run_gitlab "$W" GITLAB_TOKEN=secret-token FALLOW_MUTATION_SCOPE=all \
  MOCK_MUTATION_REPORT="$FIXTURES/mutation-clean.json" > /dev/null
assert_not_contains "$(cat "$W/mock.log")" "<leaked>" "gitlab mutation: the tests of the project get no GITLAB_TOKEN"

W="$MUT_TMP/gl-forkproject"
mut_run_gitlab "$W" CI_MERGE_REQUEST_IID=7 CI_MERGE_REQUEST_SOURCE_PROJECT_ID=200 CI_PROJECT_ID=200 \
  FALLOW_MUTATION_SCOPE=all MOCK_MUTATION_REPORT="$FIXTURES/mutation-clean.json" > /dev/null
mut_assert_eq "$(jq -r '.outcome' "$W/fallow-mutation-status.json")" "ok" \
  "gitlab mutation: a pipeline in the fork project runs"

# --- GitLab: changed scope uses the MR diff ---

W="$MUT_TMP/gl-diff"
rm -rf "$W"; mkdir -p "$W"
(
  cd "$W" || exit 99
  git init -q
  git -c user.email=t@e -c user.name=t commit -q --allow-empty -m base
  printf 'export const a = 1;\n' > a.js
  git add a.js
  git -c user.email=t@e -c user.name=t commit -q -m change
) > /dev/null 2>&1
BASE=$(git -C "$W" rev-parse HEAD~1)
(
  cd "$W" || exit 99
  # The mock log stays outside the worktree, so the dirty check sees only
  # what the script writes.
  export PATH="$MUT_BIN:$PATH" MOCK_LOG="$MUT_TMP/gl-diff.log" TMPDIR="$MUT_TMP"
  export CI_MERGE_REQUEST_DIFF_BASE_SHA="$BASE" MOCK_MUTATION_DIRTY_CHECK=1
  export MOCK_MUTATION_REPORT="$FIXTURES/mutation-clean.json"
  : > "$MOCK_LOG"
  bash "$MUT_SCRIPTS/mutation.sh" > /dev/null 2>&1
)
assert_contains "$(cat "$MUT_TMP/gl-diff.log")" "--diff-file $MUT_TMP/fallow-mutation-mr.diff" "gitlab mutation: changed scope passes the MR diff"
mut_assert_eq "$(jq -r '.outcome' "$W/fallow-mutation-status.json")" "ok" \
  "gitlab mutation: the script writes nothing into the worktree before the run"

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

GL_TEMPLATE=$(cat "$DIR/../gitlab-ci.yml")
assert_contains "$GL_TEMPLATE" 'FALLOW_MUTATION: "false"' "gitlab template: mutation is off by default"
assert_contains "$GL_TEMPLATE" 'FALLOW_MUTATION_TIME_BUDGET: "10m"' "gitlab template: the default has a time budget"
assert_contains "$GL_TEMPLATE" "for f in comment.sh review.sh gitlab_common.sh mutation.sh mutation_common.sh; do" \
  "gitlab template: script preparation gets the mutation scripts"
assert_contains "$GL_TEMPLATE" "- fallow-mutation.json" "gitlab template: the mutation report is an artifact"
assert_not_contains "$GL_TEMPLATE" "--allow-dirty" "gitlab template: never passes --allow-dirty"
MUT_LINE=$(grep -n 'bash /tmp/fallow-scripts/mutation.sh' "$DIR/../gitlab-ci.yml" | head -1 | cut -d: -f1)
ANALYSIS_LINE=$(grep -n 'fallow "${RUN_ARGS\[@\]}" > fallow-results.json' "$DIR/../gitlab-ci.yml" | head -1 | cut -d: -f1)
DIFF_LINE=$(grep -n '> fallow-mr.diff' "$DIR/../gitlab-ci.yml" | head -1 | cut -d: -f1)
if [ -n "$MUT_LINE" ] && [ "$MUT_LINE" -lt "$ANALYSIS_LINE" ] && [ "$MUT_LINE" -lt "$DIFF_LINE" ]; then
  pass "gitlab template: mutation runs before the template writes into the project directory"
else
  fail "gitlab template: mutation runs before the template writes into the project directory" \
    "mutation line ${MUT_LINE:-none}, diff line $DIFF_LINE, analysis line $ANALYSIS_LINE"
fi

rm -rf "$MUT_TMP"
