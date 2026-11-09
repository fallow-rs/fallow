#!/usr/bin/env bash
# Tests of the Action change-scope step (action/scripts/scope.sh) and of the
# way analyze.sh consumes its env. action/tests/run.sh sources this file, so it
# uses the pass, fail and assert helpers and the DIR variable of run.sh.

echo ""
echo "=== Change scope step ==="

case $- in
  *e*) SC_ERREXIT=1 ;;
  *) SC_ERREXIT=0 ;;
esac
set +e

SC_SCRIPTS="$DIR/../scripts"
SC_TMP=$(mktemp -d)
SC_BIN="$SC_TMP/bin"
mkdir -p "$SC_BIN"

# A fake `fallow`. MOCK_REPORT_HELP is the text of `fallow report --help`, and
# MOCK_NO_REPORT=1 makes that command fail like a CLI without `report`.
cat > "$SC_BIN/fallow" <<'SH'
#!/usr/bin/env bash
if [ "${1:-}" = "report" ] && [ "${2:-}" = "--help" ]; then
  [ "${MOCK_NO_REPORT:-0}" = "1" ] && { echo "error: unrecognized subcommand 'report'" >&2; exit 2; }
  printf '%s\n' "${MOCK_REPORT_HELP:-Usage: fallow report --from <PATH>}"
  exit 0
fi
case "$*" in
  *"--help"*) printf '%s\n' 'Usage: fallow dead-code --sarif-file <PATH>' ;;
  *) printf '%s\n' '{"total_issues":0}' ;;
esac
SH
chmod +x "$SC_BIN/fallow"

# A fake `git` for the cascade cases. FAKE_GIT_FAIL_DIFF=1 makes every diff
# fail, so the scope step reaches the GitHub API fallbacks.
mkdir -p "$SC_TMP/fakegit"
cat > "$SC_TMP/fakegit/git" <<'SH'
#!/usr/bin/env bash
[ -n "${FAKE_GIT_LOG:-}" ] && printf '%s\n' "$*" >> "$FAKE_GIT_LOG"
case "$1" in
  diff)
    [ "${FAKE_GIT_FAIL_DIFF:-0}" = "1" ] && { echo "fatal: bad revision" >&2; exit 128; }
    shift
    name_only=false
    nul_delimited=false
    for arg in "$@"; do
      if [ "$arg" = "--name-only" ]; then
        name_only=true
      elif [ "$arg" = "-z" ]; then
        nul_delimited=true
      fi
    done
    if [ "$name_only" = "true" ]; then
      if [ "$nul_delimited" = "true" ]; then
        printf '%s\0' "${FAKE_CHANGED_FILES:-src/a.ts}"
      else
        printf '%s\n' "${FAKE_CHANGED_FILES:-src/a.ts}"
      fi
      exit 0
    fi
    printf '%s\n' 'diff --git a/src/a.ts b/src/a.ts'
    printf '%s\n' '--- a/src/a.ts'
    printf '%s\n' '+++ b/src/a.ts'
    printf '%s\n' '@@ -0,0 +1 @@'
    printf '%s\n' '+export const a = 1;'
    ;;
  *) exit 0 ;;
esac
SH
chmod +x "$SC_TMP/fakegit/git"

sc_git() {
  env -u GIT_DIR -u GIT_INDEX_FILE -u GIT_WORK_TREE \
    git -c user.email=test@example.com -c user.name=test -c commit.gpgsign=false \
      -c core.hooksPath=/dev/null "$@"
}

# Run scope.sh in <work> like the scope step: RUNNER_TEMP, GITHUB_ENV and
# GITHUB_OUTPUT are files next to <work>. Extra arguments are env assignments.
sc_run_scope() {
  local work=$1
  shift
  mkdir -p "$work.runner"
  (
    cd "$work" || exit 1
    env -u GIT_DIR -u GIT_INDEX_FILE -u GIT_WORK_TREE \
      PATH="$SC_BIN:$PATH" \
      RUNNER_TEMP="$work.runner" \
      GITHUB_ENV="$work.env" \
      GITHUB_OUTPUT="$work.scope-output" \
      "$@" bash "$SC_SCRIPTS/scope.sh"
  ) 2>&1
}

# Run analyze.sh in <work> like the analyze step after the scope step: the
# values of GITHUB_ENV come first, and the step env (the extra arguments)
# overrides them, as on a runner.
sc_run_analyze() {
  local work=$1
  shift
  local -a env_lines=()
  local line
  if [ -f "$work.env" ]; then
    while IFS= read -r line; do
      env_lines+=("$line")
    done < "$work.env"
  fi
  (
    cd "$work" || exit 1
    env -u GIT_DIR -u GIT_INDEX_FILE -u GIT_WORK_TREE \
      PATH="$SC_BIN:$PATH" \
      RUNNER_TEMP="$work.runner" \
      ${env_lines[@]+"${env_lines[@]}"} \
      GITHUB_ENV="$work.env" \
      GITHUB_OUTPUT="$work.output" \
      "$@" bash "$SC_SCRIPTS/analyze.sh"
  ) 2>&1
}

assert_eq_sc() {
  local actual="$1" expected="$2" name="$3"
  if [ "$actual" = "$expected" ]; then
    pass "$name"
  else
    fail "$name" "expected '$expected', got '$actual'"
  fi
}

sc_env_value() {
  local file=$1 key=$2
  grep "^${key}=" "$file" 2>/dev/null | tail -1 | cut -d= -f2-
}

# --- Capability probes ---

SC_W="$SC_TMP/probe-mutation"
mkdir -p "$SC_W"
sc_run_scope "$SC_W" MOCK_REPORT_HELP=$'Usage: fallow report --from <PATH>\n      --mutation-report <PATH>' >/dev/null
assert_eq_sc "$(sc_env_value "$SC_W.env" HAS_NATIVE_REPORT)" "true" \
  "scope: a CLI with report sets HAS_NATIVE_REPORT=true"
assert_eq_sc "$(sc_env_value "$SC_W.env" HAS_MUTATION_REPORT)" "true" \
  "scope: report help with --mutation-report sets HAS_MUTATION_REPORT=true"

SC_W="$SC_TMP/probe-no-mutation"
mkdir -p "$SC_W"
sc_run_scope "$SC_W" >/dev/null
assert_eq_sc "$(sc_env_value "$SC_W.env" HAS_NATIVE_REPORT)" "true" \
  "scope: report help without the flag keeps HAS_NATIVE_REPORT=true"
assert_eq_sc "$(sc_env_value "$SC_W.env" HAS_MUTATION_REPORT)" "false" \
  "scope: report help without --mutation-report sets HAS_MUTATION_REPORT=false"

SC_W="$SC_TMP/probe-no-report"
mkdir -p "$SC_W"
sc_run_scope "$SC_W" MOCK_NO_REPORT=1 >/dev/null
assert_eq_sc "$(sc_env_value "$SC_W.env" HAS_NATIVE_REPORT)" "false" \
  "scope: a CLI without report sets HAS_NATIVE_REPORT=false"
assert_eq_sc "$(sc_env_value "$SC_W.env" HAS_MUTATION_REPORT)" "false" \
  "scope: a CLI without report sets HAS_MUTATION_REPORT=false"

# --- artifacts-dir validation ---

for bad_dir in "../outside" "/abs/path" "-flag"; do
  SC_W="$SC_TMP/artifacts-bad"
  rm -rf "$SC_W" "$SC_W.env"
  mkdir -p "$SC_W"
  OUT=$(sc_run_scope "$SC_W" INPUT_ARTIFACTS_DIR="$bad_dir")
  cmd_status=$?
  assert_eq_sc "$cmd_status" "2" "scope: artifacts-dir '$bad_dir' exits 2"
  assert_contains "$OUT" "artifacts-dir must be a relative path inside the workspace" \
    "scope: artifacts-dir '$bad_dir' error is clear"
  if [ ! -s "$SC_W.env" ]; then
    pass "scope: artifacts-dir '$bad_dir' writes no env"
  else
    fail "scope: artifacts-dir '$bad_dir' writes no env" "env: $(cat "$SC_W.env")"
  fi
done

SC_W="$SC_TMP/artifacts-good"
mkdir -p "$SC_W"
sc_run_scope "$SC_W" INPUT_ARTIFACTS_DIR=".var/fallow" >/dev/null
assert_eq_sc "$(sc_env_value "$SC_W.env" FALLOW_ARTIFACTS_DIR)" ".var/fallow" \
  "scope: exports the validated artifacts-dir"
SC_W="$SC_TMP/artifacts-default"
mkdir -p "$SC_W"
sc_run_scope "$SC_W" INPUT_ARTIFACTS_DIR="" >/dev/null
assert_eq_sc "$(sc_env_value "$SC_W.env" FALLOW_ARTIFACTS_DIR)" "." \
  "scope: an empty artifacts-dir becomes ."

# --- Input validation before Git ---

SC_W="$SC_TMP/option-ref"
mkdir -p "$SC_W"
OUT=$(sc_run_scope "$SC_W" PATH="$SC_TMP/fakegit:$SC_BIN:$PATH" FAKE_GIT_LOG="$SC_W.git" \
  INPUT_CHANGED_SINCE="-main")
cmd_status=$?
assert_eq_sc "$cmd_status" "2" "scope: rejects option-like changed-since"
assert_contains "$OUT" "::error::changed-since must not begin with '-'" \
  "scope: option-like changed-since error is stable"
if [ ! -s "$SC_W.git" ] && [ ! -s "$SC_W.env" ]; then
  pass "scope: rejects option-like changed-since before Git and env writes"
else
  fail "scope: rejects option-like changed-since before Git and env writes" "git or env was used"
fi

SC_W="$SC_TMP/control-diff"
mkdir -p "$SC_W"
OUT=$(sc_run_scope "$SC_W" INPUT_DIFF_FILE=$'reports/diff\nINJECTED=value')
cmd_status=$?
assert_eq_sc "$cmd_status" "2" "scope: rejects control characters in diff-file"
assert_contains "$OUT" "::error::diff-file must not contain ASCII control characters" \
  "scope: diff-file control-character error is stable"

# --- Standalone run ---

SC_W="$SC_TMP/standalone"
mkdir -p "$SC_W"
OUT=$(cd "$SC_W" && env -u GITHUB_ENV -u GITHUB_OUTPUT PATH="$SC_BIN:$PATH" \
  RUNNER_TEMP="$SC_W.runner" bash "$SC_SCRIPTS/scope.sh" 2>&1)
cmd_status=$?
assert_eq_sc "$cmd_status" "0" "scope: runs by itself without GITHUB_ENV"

# --- Real Git: root-relative diff, scope files outside the worktree ---

SC_REPO="$SC_TMP/repo"
sc_git init -q -b main "$SC_REPO"
mkdir -p "$SC_REPO/packages/app/src" "$SC_REPO/packages/other"
echo "export const a = 0;" > "$SC_REPO/packages/app/src/a.ts"
echo "export const b = 0;" > "$SC_REPO/packages/other/b.ts"
sc_git -C "$SC_REPO" add . && sc_git -C "$SC_REPO" commit -qm base
SC_BASE=$(sc_git -C "$SC_REPO" rev-parse HEAD)
echo "export const a = 1;" > "$SC_REPO/packages/app/src/a.ts"
echo "export const b = 1;" > "$SC_REPO/packages/other/b.ts"
sc_git -C "$SC_REPO" add . && sc_git -C "$SC_REPO" commit -qm change

OUT=$(sc_run_scope "$SC_REPO" INPUT_ROOT="packages/app" INPUT_CHANGED_SINCE="$SC_BASE" \
  INPUT_ARTIFACTS_DIR="fallow-artifacts")
cmd_status=$?
assert_eq_sc "$cmd_status" "0" "scope: real Git run exits 0"
SC_STATUS=$(cd "$SC_REPO" && sc_git status --porcelain --untracked-files=all)
assert_eq_sc "$SC_STATUS" "" "scope: the scope step writes nothing in the worktree"
SC_DIFF="$SC_REPO.runner/fallow-scope/pr.diff"
assert_eq_sc "$(sc_env_value "$SC_REPO.env" FALLOW_DIFF_FILE)" "$SC_DIFF" \
  "scope: exports FALLOW_DIFF_FILE in the scope directory"
assert_eq_sc "$(sc_env_value "$SC_REPO.env" FALLOW_SCOPE_CHANGED_SINCE)" "$SC_BASE" \
  "scope: exports FALLOW_SCOPE_CHANGED_SINCE"
assert_eq_sc "$(sc_env_value "$SC_REPO.env" FALLOW_CHANGED_FILES_FILE)" \
  "$SC_REPO.runner/fallow-scope/changed-files.json" \
  "scope: exports FALLOW_CHANGED_FILES_FILE in the scope directory"
assert_contains "$(cat "$SC_DIFF" 2>/dev/null)" "+++ b/src/a.ts" \
  "scope: the Git diff is root-relative"
assert_not_contains "$(cat "$SC_DIFF" 2>/dev/null)" "other/b.ts" \
  "scope: the Git diff drops files outside the root"
if jq -e '. == ["src/a.ts"]' "$SC_REPO.runner/fallow-scope/changed-files.json" >/dev/null 2>&1; then
  pass "scope: the changed files are root-relative"
else
  fail "scope: the changed files are root-relative" \
    "got: $(cat "$SC_REPO.runner/fallow-scope/changed-files.json" 2>/dev/null || echo absent)"
fi

# analyze.sh copies the scope files into artifacts-dir with the old names.
OUT=$(sc_run_analyze "$SC_REPO" INPUT_ROOT="packages/app" INPUT_COMMAND="dead-code" \
  INPUT_FORMAT="json" INPUT_ARTIFACTS_DIR="fallow-artifacts" INPUT_CHANGED_SINCE="")
cmd_status=$?
assert_eq_sc "$cmd_status" "0" "scope: analyze after the scope step exits 0"
if cmp -s "$SC_DIFF" "$SC_REPO/fallow-artifacts/fallow-pr.diff"; then
  pass "analyze: copies the scope diff to fallow-pr.diff"
else
  fail "analyze: copies the scope diff to fallow-pr.diff" "copy missing or different"
fi
if cmp -s "$SC_REPO.runner/fallow-scope/changed-files.json" "$SC_REPO/fallow-artifacts/fallow-changed-files.json"; then
  pass "analyze: copies the changed files to fallow-changed-files.json"
else
  fail "analyze: copies the changed files to fallow-changed-files.json" "copy missing or different"
fi
assert_contains "$(cat "$SC_REPO/fallow-artifacts/fallow-analysis-args.sh")" "--changed-since $SC_BASE" \
  "analyze: uses the changed-since of the scope step"
assert_contains "$(cat "$SC_REPO.output")" "changed_since=$SC_BASE" \
  "analyze: writes the changed_since output from the scope step"

# A second run in the same job must not reuse files of the first run.
OUT=$(sc_run_scope "$SC_REPO" INPUT_ROOT="packages/app" INPUT_CHANGED_SINCE="")
if [ ! -e "$SC_DIFF" ] && [ ! -e "$SC_REPO.runner/fallow-scope/changed-files.json" ]; then
  pass "scope: removes the scope files of an earlier run"
else
  fail "scope: removes the scope files of an earlier run" "stale scope files remain"
fi

# --- gh pr diff fallback: root-relative paths ---

SC_W="$SC_TMP/gh-diff"
mkdir -p "$SC_W/packages/app" "$SC_TMP/gh-diff-bin"
cat > "$SC_TMP/gh-diff-bin/gh" <<'SH'
#!/usr/bin/env bash
if [ "${1:-}" = "pr" ] && [ "${2:-}" = "diff" ]; then
  cat <<'DIFF'
diff --git a/packages/app/src/a.ts b/packages/app/src/a.ts
index 1111111..2222222 100644
--- a/packages/app/src/a.ts
+++ b/packages/app/src/a.ts
@@ -1 +1 @@
-export const a = 0;
+export const a = 1;
diff --git a/packages/other/b.ts b/packages/other/b.ts
index 1111111..2222222 100644
--- a/packages/other/b.ts
+++ b/packages/other/b.ts
@@ -1 +1 @@
-export const b = 0;
+export const b = 1;
diff --git a/packages/app/src/new.ts b/packages/app/src/new.ts
new file mode 100644
--- /dev/null
+++ b/packages/app/src/new.ts
@@ -0,0 +1 @@
+export const n = 1;
diff --git a/packages/app/src/old.ts b/packages/app/src/renamed.ts
similarity index 90%
rename from packages/app/src/old.ts
rename to packages/app/src/renamed.ts
DIFF
  exit 0
fi
if [ "${1:-}" = "api" ]; then
  printf '%s\n' '"packages/app/src/a.ts"' '"packages/other/b.ts"'
  exit 0
fi
exit 0
SH
chmod +x "$SC_TMP/gh-diff-bin/gh"
OUT=$(sc_run_scope "$SC_W" PATH="$SC_TMP/gh-diff-bin:$SC_TMP/fakegit:$SC_BIN:$PATH" \
  FAKE_GIT_FAIL_DIFF=1 INPUT_ROOT="packages/app" INPUT_CHANGED_SINCE="base1234" \
  PR_NUMBER="7" GH_REPO="owner/repo" GH_TOKEN="test")
SC_DIFF_TEXT=$(cat "$SC_W.runner/fallow-scope/pr.diff" 2>/dev/null)
assert_contains "$SC_DIFF_TEXT" "diff --git a/src/a.ts b/src/a.ts" \
  "scope: gh pr diff fallback strips the root from the diff header"
assert_contains "$SC_DIFF_TEXT" "+++ b/src/a.ts" \
  "scope: gh pr diff fallback strips the root from the new path"
assert_contains "$SC_DIFF_TEXT" "--- a/src/a.ts" \
  "scope: gh pr diff fallback strips the root from the old path"
assert_contains "$SC_DIFF_TEXT" "+++ b/src/new.ts" \
  "scope: gh pr diff fallback keeps an added file"
assert_contains "$SC_DIFF_TEXT" "rename to src/renamed.ts" \
  "scope: gh pr diff fallback strips the root from a rename"
assert_not_contains "$SC_DIFF_TEXT" "packages/" \
  "scope: gh pr diff fallback drops files outside the root"
assert_contains "$SC_DIFF_TEXT" "+export const a = 1;" \
  "scope: gh pr diff fallback keeps the hunks"

# A PR job with auto-changed-since: false and no changed-since has no base ref,
# but the comment and the review still filter on the PR diff.
SC_W="$SC_TMP/gh-diff-no-auto"
mkdir -p "$SC_W/packages/app"
sc_run_scope "$SC_W" PATH="$SC_TMP/gh-diff-bin:$SC_TMP/fakegit:$SC_BIN:$PATH" \
  INPUT_ROOT="packages/app" INPUT_CHANGED_SINCE="" INPUT_AUTO_CHANGED_SINCE="false" \
  EVENT_NAME="pull_request" PR_NUMBER="7" GH_REPO="owner/repo" GH_TOKEN="test" > /dev/null
assert_eq_sc "$(sc_env_value "$SC_W.env" FALLOW_DIFF_FILE)" "$SC_W.runner/fallow-scope/pr.diff" \
  "scope: a PR without changed-since still exports the PR diff"
assert_contains "$(cat "$SC_W.runner/fallow-scope/pr.diff" 2>/dev/null)" "+++ b/src/a.ts" \
  "scope: the PR diff without changed-since is rebased onto the root"
assert_eq_sc "$(sc_env_value "$SC_W.env" FALLOW_SCOPE_CHANGED_SINCE)" "" \
  "scope: a PR without changed-since sets no base ref"

# `root: ./` names the workspace root, so the PR diff keeps its paths.
SC_W="$SC_TMP/gh-diff-dot-slash"
mkdir -p "$SC_W"
OUT=$(sc_run_scope "$SC_W" PATH="$SC_TMP/gh-diff-bin:$SC_TMP/fakegit:$SC_BIN:$PATH" \
  INPUT_ROOT="./" INPUT_CHANGED_SINCE="" INPUT_AUTO_CHANGED_SINCE="false" \
  EVENT_NAME="pull_request" PR_NUMBER="7" GH_REPO="owner/repo" GH_TOKEN="test")
assert_contains "$(cat "$SC_W.runner/fallow-scope/pr.diff" 2>/dev/null)" "+++ b/packages/app/src/a.ts" \
  "scope: root ./ keeps the repository-relative PR diff"
assert_not_contains "$OUT" "outside GITHUB_WORKSPACE" \
  "scope: root ./ gives no outside-the-workspace warning"

# --- Config-change unscope and explicit scope ---

# Run the scope step and then the analysis with the step env of action.yml.
run_scope_case() {
  local case_name=$1
  local changed_files=$2
  local baseline=$3
  local changed_since=$4
  local diff_file=$5
  local root=${6:-.}
  local config=${7:-}
  local work="$SC_TMP/scope-$case_name"
  mkdir -p "$work/$root"
  rm -f "$work.env" "$work.output"
  if [ -n "$diff_file" ]; then
    printf '%s\n' 'diff --git a/src/a.ts b/src/a.ts' > "$diff_file"
  fi
  local -a step_env=(
    PATH="$SC_TMP/fakegit:$SC_BIN:$PATH"
    FAKE_CHANGED_FILES="$changed_files"
    INPUT_ROOT="$root"
    INPUT_CONFIG="$config"
    INPUT_COMMAND="dead-code"
    INPUT_AUTO_CHANGED_SINCE="true"
    INPUT_CHANGED_SINCE="$changed_since"
    EVENT_NAME="pull_request"
    PR_BASE_SHA="base1234"
    INPUT_BASELINE="$baseline"
  )
  sc_run_scope "$work" "${step_env[@]}" INPUT_DIFF_FILE="$diff_file"
  sc_run_analyze "$work" "${step_env[@]}" INPUT_FORMAT="json"
}

OUT=$(run_scope_case "config-baseline-auto" ".fallowrc.json" "baseline.json" "" "")
ARGS=$(cat "$SC_TMP/scope-config-baseline-auto/fallow-analysis-args.sh")
OUTPUTS=$(cat "$SC_TMP/scope-config-baseline-auto.output")
ENV_OUT=$(cat "$SC_TMP/scope-config-baseline-auto.env")
assert_not_contains "$ARGS" "--changed-since" "scope: config baseline auto-scope removes changed-since"
if grep -qx 'changed_since=' "$SC_TMP/scope-config-baseline-auto.output"; then
  pass "scope: config baseline auto-scope clears changed_since output"
else
  fail "scope: config baseline auto-scope clears changed_since output" "outputs were: $OUTPUTS"
fi
assert_not_contains "$ENV_OUT" "FALLOW_DIFF_FILE=" "scope: config baseline auto-scope skips auto diff file"
assert_contains "$OUT" "dead-code baseline comparison is running unscoped because '.fallowrc.json' changed" "scope: config baseline auto-scope warns"

OUT=$(run_scope_case "source-baseline-auto" "src/a.ts" "baseline.json" "" "")
ARGS=$(cat "$SC_TMP/scope-source-baseline-auto/fallow-analysis-args.sh")
OUTPUTS=$(cat "$SC_TMP/scope-source-baseline-auto.output")
ENV_OUT=$(cat "$SC_TMP/scope-source-baseline-auto.env")
assert_contains "$ARGS" "--changed-since base1234" "scope: source baseline auto-scope keeps changed-since"
assert_contains "$OUTPUTS" "changed_since=base1234" "scope: source baseline auto-scope keeps changed_since output"
assert_contains "$ENV_OUT" "FALLOW_DIFF_FILE=" "scope: source baseline auto-scope writes auto diff file"
assert_contains "$OUT" "Auto-scoping analysis to files changed since PR base (base123)" "scope: auto-scope notice names the base"

OUT=$(run_scope_case "config-no-baseline" ".fallowrc.json" "" "" "")
ARGS=$(cat "$SC_TMP/scope-config-no-baseline/fallow-analysis-args.sh")
OUTPUTS=$(cat "$SC_TMP/scope-config-no-baseline.output")
assert_contains "$ARGS" "--changed-since base1234" "scope: config without baseline keeps changed-since"
assert_contains "$OUTPUTS" "changed_since=base1234" "scope: config without baseline keeps changed_since output"

OUT=$(run_scope_case "explicit-config-root-prefix" "packages/app/config/fallow.jsonc" "baseline.json" "" "" "packages/app" "config/fallow.jsonc")
ARGS=$(cat "$SC_TMP/scope-explicit-config-root-prefix/fallow-analysis-args.sh")
assert_not_contains "$ARGS" "--changed-since" "scope: explicit config path with root prefix removes auto changed-since"
assert_contains "$OUT" "because 'config/fallow.jsonc' changed" "scope: explicit config path warning uses root-relative path"

OUT=$(run_scope_case "config-explicit-changed-since" ".fallowrc.json" "baseline.json" "manual-base" "")
ARGS=$(cat "$SC_TMP/scope-config-explicit-changed-since/fallow-analysis-args.sh")
OUTPUTS=$(cat "$SC_TMP/scope-config-explicit-changed-since.output")
assert_contains "$ARGS" "--changed-since manual-base" "scope: explicit changed-since is preserved"
assert_contains "$OUTPUTS" "changed_since=manual-base" "scope: explicit changed-since output is preserved"
assert_contains "$OUT" "explicitly scoped" "scope: explicit changed-since warns about baseline drift"

EXPLICIT_DIFF="$SC_TMP/user.diff"
OUT=$(run_scope_case "config-explicit-diff" ".fallowrc.json" "baseline.json" "" "$EXPLICIT_DIFF")
ARGS=$(cat "$SC_TMP/scope-config-explicit-diff/fallow-analysis-args.sh")
OUTPUTS=$(cat "$SC_TMP/scope-config-explicit-diff.output")
ENV_OUT=$(cat "$SC_TMP/scope-config-explicit-diff.env")
assert_not_contains "$ARGS" "--changed-since base1234" "scope: explicit diff still clears auto changed-since"
if grep -qx 'changed_since=' "$SC_TMP/scope-config-explicit-diff.output"; then
  pass "scope: explicit diff clears auto changed_since output"
else
  fail "scope: explicit diff clears auto changed_since output" "outputs were: $OUTPUTS"
fi
assert_contains "$ENV_OUT" "FALLOW_DIFF_FILE=$EXPLICIT_DIFF" "scope: explicit diff file is preserved"
assert_contains "$OUT" "explicit diff file remains active" "scope: explicit diff warns about remaining scope"

OUT=$(run_scope_case "newline-filename" $'src/line\nbreak.ts' "" "" "")
CHANGED_FILE="$SC_TMP/scope-newline-filename/fallow-changed-files.json"
if jq -e 'length == 1 and .[0] == "src/line\nbreak.ts"' "$CHANGED_FILE" >/dev/null 2>&1; then
  pass "scope: changed-file JSON preserves newline-bearing filename"
else
  fail "scope: changed-file JSON preserves newline-bearing filename" "got: $(cat "$CHANGED_FILE" 2>/dev/null || echo absent)"
fi

# --- API failure handling (issue #470) ---
# A failed `gh api` call must reach both the changed_files_unavailable output
# of the analyze step and a ::warning:: line, never a silent unscoped run.

API_FAIL_WORK="$SC_TMP/api"
API_FAIL_BIN="$SC_TMP/api-bin"
mkdir -p "$API_FAIL_WORK/packages/app" "$API_FAIL_BIN"

run_api_case() {
  local root=$1
  shift
  rm -f "$API_FAIL_WORK.env" "$API_FAIL_WORK.output" "$API_FAIL_WORK/fallow-changed-files.json"
  local -a step_env=(
    PATH="$API_FAIL_BIN:$SC_TMP/fakegit:$SC_BIN:$PATH"
    FAKE_GIT_FAIL_DIFF=1
    INPUT_ROOT="$root"
    INPUT_COMMAND="check"
    INPUT_CHANGED_SINCE="0000000000000000000000000000000000000000"
    PR_NUMBER="123"
    GH_REPO="owner/repo"
    GH_TOKEN="test"
    "$@"
  )
  sc_run_scope "$API_FAIL_WORK" "${step_env[@]}"
  sc_run_analyze "$API_FAIL_WORK" "${step_env[@]}" INPUT_FORMAT="json" >/dev/null
}

cat > "$API_FAIL_BIN/gh" <<'SH'
#!/usr/bin/env bash
if [ "${1:-}" = "api" ]; then
  case " $* " in
    *" --paginate "*) ;;
    *) echo "missing --paginate" >&2; exit 2 ;;
  esac
  echo "gh: HTTP 500: Internal Server Error (api.github.com/repos/owner/repo/pulls/123/files)" >&2
  exit 1
fi
exit 0
SH
chmod +x "$API_FAIL_BIN/gh"

API_FAIL_STDERR=$(run_api_case ".")
assert_contains "$(cat "$API_FAIL_WORK.output")" "changed_files_unavailable=true" \
  "scope: analyze emits changed_files_unavailable=true on gh api failure"
assert_contains "$API_FAIL_STDERR" "GitHub API call to list PR files failed" \
  "scope: stderr names API failure mode (not shallow-clone)"
assert_contains "$API_FAIL_STDERR" "gh auth status" \
  "scope: warning includes actionable hint (gh auth status)"

cat > "$API_FAIL_BIN/gh" <<'SH'
#!/usr/bin/env bash
if [ "${1:-}" = "api" ]; then
  case " $* " in
    *" --paginate "*) ;;
    *) echo "missing --paginate" >&2; exit 2 ;;
  esac
  printf '%s\n' \
    '"src/a.ts"' \
    '"src/line\nbreak.ts"' \
    '"src/quote\"file.ts"' \
    '"src/back\\slash.ts"'
  exit 0
fi
exit 0
SH
chmod +x "$API_FAIL_BIN/gh"

run_api_case "." >/dev/null
if grep -q '^changed_files_unavailable=false$' "$API_FAIL_WORK.output" \
    && ! grep -q '^changed_files_unavailable=true$' "$API_FAIL_WORK.output"; then
  pass "scope: analyze emits changed_files_unavailable=false on gh api success"
else
  fail "scope: analyze emits changed_files_unavailable=false on gh api success" \
    "expected only =false, got: $(grep changed_files_unavailable "$API_FAIL_WORK.output" || echo 'absent')"
fi
API_CHANGED_FILE="$API_FAIL_WORK/fallow-changed-files.json"
if jq -e '
  length == 4
  and .[0] == "src/a.ts"
  and .[1] == "src/line\nbreak.ts"
  and .[2] == "src/quote\"file.ts"
  and .[3] == "src/back\\slash.ts"
' "$API_CHANGED_FILE" >/dev/null 2>&1; then
  pass "scope: API fallback preserves JSON-escaped filenames"
else
  fail "scope: API fallback preserves JSON-escaped filenames" \
    "got: $(cat "$API_CHANGED_FILE" 2>/dev/null || echo absent)"
fi

cat > "$API_FAIL_BIN/gh" <<'SH'
#!/usr/bin/env bash
if [ "${1:-}" = "api" ]; then
  printf '%s\n' \
    '"packages/app/src/a.ts"' \
    '"packages/other/src/ignored.ts"'
  exit 0
fi
exit 0
SH
chmod +x "$API_FAIL_BIN/gh"

run_api_case "$API_FAIL_WORK/packages/app" GITHUB_WORKSPACE="$API_FAIL_WORK" >/dev/null
if jq -e 'length == 1 and .[0] == "src/a.ts"' "$API_CHANGED_FILE" >/dev/null 2>&1; then
  pass "scope: API fallback scopes changed files for an absolute root"
else
  fail "scope: API fallback scopes changed files for an absolute root" \
    "got: $(cat "$API_CHANGED_FILE" 2>/dev/null || echo absent)"
fi

# The marker must be unconditional so downstream `if:` gates can match on it
# as a positive signal (== 'false') without an absent-or-false ambiguity.
rm -f "$API_FAIL_WORK.env" "$API_FAIL_WORK.output"
sc_run_scope "$API_FAIL_WORK" INPUT_ROOT="." >/dev/null
sc_run_analyze "$API_FAIL_WORK" INPUT_ROOT="." INPUT_COMMAND="check" INPUT_FORMAT="json" >/dev/null
if grep -q '^changed_files_unavailable=false$' "$API_FAIL_WORK.output"; then
  pass "scope: analyze emits changed_files_unavailable=false without changed-since"
else
  fail "scope: analyze emits changed_files_unavailable=false without changed-since" \
    "got: $(grep changed_files_unavailable "$API_FAIL_WORK.output" || echo 'absent')"
fi

# --- Shallow clone merge base for changed-since ---
# A pull_request job checks out the merge commit at depth 1 by default. The CLI
# diffs against merge-base(base, HEAD), so the scope step must fetch enough
# history for that merge base. The fake fallow fails like the real CLI without
# one.

MB_TMP="$SC_TMP/mb"
mkdir -p "$MB_TMP/bin"
cat > "$MB_TMP/bin/fallow" <<'SH'
#!/usr/bin/env bash
base=""
prev=""
for arg in "$@"; do
  [ "$prev" = "--changed-since" ] && base=$arg
  prev=$arg
done
case "$*" in
  *audit*) ;;
  *) printf '{"total_issues":0}\n'; exit 0 ;;
esac
if [ -n "$base" ] && ! git merge-base "$base" HEAD >/dev/null 2>&1; then
  printf '{"error":true,"message":"could not determine changed files for base ref %s: no merge base","exit_code":2}\n' "$base"
  exit 2
fi
printf '%s\n' '{"command":"audit","verdict":"pass","attribution":{"gate":"new-only","dead_code_introduced":0,"dead_code_inherited":0,"complexity_introduced":0,"complexity_inherited":0,"duplication_introduced":0,"duplication_inherited":0},"summary":{"dead_code_issues":0,"dead_code_has_errors":false,"complexity_findings":0,"max_cyclomatic":null,"duplication_clone_groups":0}}'
SH
chmod +x "$MB_TMP/bin/fallow"

# History: main has five commits, a PR branch adds two, main adds one more,
# and the PR merge commit joins the two tips (first parent: the base tip).
sc_git init -q -b main "$MB_TMP/src"
for i in 1 2 3 4 5; do
  echo "$i" > "$MB_TMP/src/f$i.ts"
  sc_git -C "$MB_TMP/src" add . && sc_git -C "$MB_TMP/src" commit -qm "main $i"
done
sc_git -C "$MB_TMP/src" switch -qc feature
for i in 1 2; do
  echo "$i" > "$MB_TMP/src/pr$i.ts"
  sc_git -C "$MB_TMP/src" add . && sc_git -C "$MB_TMP/src" commit -qm "pr $i"
done
sc_git -C "$MB_TMP/src" switch -q main
echo "base" > "$MB_TMP/src/base.ts"
sc_git -C "$MB_TMP/src" add . && sc_git -C "$MB_TMP/src" commit -qm "main base"
MB_BASE=$(sc_git -C "$MB_TMP/src" rev-parse main)
MB_HEAD=$(sc_git -C "$MB_TMP/src" rev-parse feature)
sc_git -C "$MB_TMP/src" -c merge.gpgsign=false merge -q --no-ff feature -m "merge pr"
MB_MERGE=$(sc_git -C "$MB_TMP/src" rev-parse HEAD)
sc_git -C "$MB_TMP/src" reset -q --hard "$MB_BASE"
sc_git -C "$MB_TMP/src" update-ref refs/pull/1/merge "$MB_MERGE"
sc_git clone -q --bare "$MB_TMP/src" "$MB_TMP/remote.git"
# A bare clone copies only branches and tags. Keep the merge commit on a ref,
# as GitHub does, so a fetch by its SHA works with every git version.
sc_git -C "$MB_TMP/remote.git" update-ref refs/pull/1/merge "$MB_MERGE"

# Make a depth-1 checkout of one commit, like actions/checkout does.
mb_checkout() {
  local dir=$1 sha=$2
  sc_git init -q "$dir"
  sc_git -C "$dir" remote add origin "file://$MB_TMP/remote.git"
  sc_git -C "$dir" fetch -q --no-tags --depth=1 origin "$sha"
  sc_git -C "$dir" checkout -q "$sha"
}

run_merge_base_case() {
  local dir=$1
  local -a step_env=(
    PATH="$MB_TMP/bin:$PATH"
    INPUT_ROOT="."
    INPUT_COMMAND="audit"
    INPUT_AUTO_CHANGED_SINCE="true"
    EVENT_NAME="pull_request"
    PR_BASE_SHA="$MB_BASE"
  )
  sc_run_scope "$dir" "${step_env[@]}" || return $?
  sc_run_analyze "$dir" "${step_env[@]}" INPUT_FORMAT="json"
}

mb_checkout "$MB_TMP/merge" "$MB_MERGE"
OUT=$(run_merge_base_case "$MB_TMP/merge")
cmd_status=$?
if [ "$cmd_status" -eq 0 ]; then
  pass "merge base: depth-1 merge commit checkout runs audit"
else
  fail "merge base: depth-1 merge commit checkout runs audit" "exit $cmd_status, output: $OUT"
fi
assert_not_contains "$OUT" "no merge base" "merge base: merge commit checkout gets a merge base"
if [ "$(sc_git -C "$MB_TMP/merge" merge-base "$MB_BASE" HEAD 2>/dev/null)" = "$MB_BASE" ]; then
  pass "merge base: merge commit parents give the PR base as merge base"
else
  fail "merge base: merge commit parents give the PR base as merge base" "merge base not found"
fi

mb_checkout "$MB_TMP/head" "$MB_HEAD"
OUT=$(run_merge_base_case "$MB_TMP/head")
cmd_status=$?
if [ "$cmd_status" -eq 0 ]; then
  pass "merge base: depth-1 PR head checkout deepens to a merge base"
else
  fail "merge base: depth-1 PR head checkout deepens to a merge base" "exit $cmd_status, output: $OUT"
fi
assert_not_contains "$OUT" "no merge base" "merge base: PR head checkout gets a merge base"

rm -rf "$SC_TMP"
[ "$SC_ERREXIT" = "1" ] && set -e
