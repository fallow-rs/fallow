#!/usr/bin/env bash
# Test suite for fallow GitLab CI jq scripts and bash helpers
# Run: bash ci/tests/run.sh

set -o pipefail

DIR="$(cd "$(dirname "$0")" && pwd)"
CI_JQ_DIR="$DIR/../jq"
SHARED_JQ_DIR="$DIR/../../action/jq"
FIXTURES="$DIR/fixtures"
PASSED=0
FAILED=0
ERRORS=()

# --- Helpers ---

pass() { PASSED=$((PASSED + 1)); echo "  ✓ $1"; }
fail() { FAILED=$((FAILED + 1)); ERRORS+=("$1: $2"); echo "  x $1: $2"; }

assert_contains() {
  local output="$1" expected="$2" name="$3"
  if [[ "$output" == *"$expected"* ]]; then
    pass "$name"
  else
    fail "$name" "expected to contain: $expected"
  fi
}

assert_not_contains() {
  local output="$1" unexpected="$2" name="$3"
  if [[ "$output" == *"$unexpected"* ]]; then
    fail "$name" "should NOT contain: $unexpected"
  else
    pass "$name"
  fi
}

assert_json_length() {
  local output="$1" expected="$2" name="$3"
  local actual
  actual=$(echo "$output" | jq 'length' 2>/dev/null)
  if [ "$actual" = "$expected" ]; then
    pass "$name"
  else
    fail "$name" "expected length $expected, got $actual"
  fi
}

assert_valid_json() {
  local output="$1" name="$2"
  if echo "$output" | jq -e '.' > /dev/null 2>&1; then
    pass "$name"
  else
    fail "$name" "invalid JSON output"
  fi
}

assert_valid_markdown() {
  local output="$1" name="$2"
  if [ -n "$output" ]; then
    pass "$name"
  else
    fail "$name" "empty markdown output"
  fi
}

# =========================================================================
# GitLab-specific install path tests
# =========================================================================

echo ""
echo "=== GitLab install path ==="

gitlab_before_script_block() {
  local start="$1"
  local end="$2"
  awk -v start="$start" -v end="$end" '
    index($0, start) { seen=1; next }
    seen && /^[[:space:]]*-[[:space:]]*\|[[:space:]]*$/ { in_block=1; next }
    in_block && index($0, end) { exit }
    in_block {
      sub(/^      /, "")
      print
    }
  ' "$DIR/../gitlab-ci.yml"
}

gitlab_install_script() {
  gitlab_before_script_block "# Validate and install fallow" "# Prepare bash scripts"
}

GITLAB_INSTALL_SCRIPT="$(gitlab_install_script)"
GITLAB_SCRIPT_PREP_SCRIPT="$(gitlab_before_script_block "# Prepare bash scripts for MR integration" "# Write the analysis script")"
GITLAB_RUN_WRITER_SCRIPT="$(gitlab_before_script_block "# Write the analysis script" "  script:")"
INSTALL_TMP=$(mktemp -d)
trap 'rm -rf "$INSTALL_TMP"' EXIT
mkdir -p "$INSTALL_TMP/pinned" "$INSTALL_TMP/range" "$INSTALL_TMP/unsafe" "$INSTALL_TMP/empty"

cat > "$INSTALL_TMP/pinned/package.json" <<'JSON'
{"devDependencies":{"fallow":"2.7.3"}}
JSON
cat > "$INSTALL_TMP/range/package.json" <<'JSON'
{"dependencies":{"fallow":"^2.52.0"}}
JSON
cat > "$INSTALL_TMP/unsafe/package.json" <<'JSON'
{"devDependencies":{"fallow":"workspace:*"}}
JSON

run_gitlab_install() {
  local root="$1"
  local version="$2"
  FALLOW_ROOT="$root" FALLOW_VERSION="$version" FALLOW_INSTALL_DRY_RUN=true /bin/sh -c "$GITLAB_INSTALL_SCRIPT" 2>&1
}

assert_contains "$GITLAB_INSTALL_SCRIPT" "bash -eo pipefail <<'FALLOW_INSTALL_EOF'" \
  "install: wrapper invokes bash with pipefail"
assert_contains "$GITLAB_SCRIPT_PREP_SCRIPT" "bash -eo pipefail <<'FALLOW_SCRIPT_PREP_EOF'" \
  "script prep: wrapper invokes bash with pipefail"
assert_contains "$GITLAB_RUN_WRITER_SCRIPT" "bash -eo pipefail <<'FALLOW_RUN_WRITER_EOF'" \
  "run writer: wrapper invokes bash with pipefail"

OUT=$(run_gitlab_install "$INSTALL_TMP/pinned" "")
assert_contains "$OUT" "Using fallow version from" "install: reads package.json pin"
assert_contains "$OUT" "DRY RUN: npm install -g --ignore-scripts fallow@2.7.3" "install: installs project pin"

OUT=$(run_gitlab_install "$INSTALL_TMP/range" "")
assert_contains "$OUT" "DRY RUN: npm install -g --ignore-scripts fallow@^2.52.0" "install: supports package.json semver range"

OUT=$(run_gitlab_install "$INSTALL_TMP/pinned" "latest")
assert_contains "$OUT" "Using fallow version from FALLOW_VERSION: latest" "install: explicit FALLOW_VERSION wins"
assert_contains "$OUT" "DRY RUN: npm install -g --ignore-scripts fallow" "install: explicit latest installs latest"

OUT=$(run_gitlab_install "$INSTALL_TMP/unsafe" "")
assert_contains "$OUT" "Ignoring unsupported fallow package.json spec" "install: warns on unsupported package spec"
assert_contains "$OUT" "DRY RUN: npm install -g --ignore-scripts fallow" "install: unsupported package spec falls back to latest"

OUT=$(run_gitlab_install "$INSTALL_TMP/empty" "")
assert_contains "$OUT" "DRY RUN: npm install -g --ignore-scripts fallow" "install: no package spec falls back to latest"

OUT=$(run_gitlab_install "$INSTALL_TMP/empty" "2.0.0 - 2.5.0")
assert_contains "$OUT" "DRY RUN: npm install -g --ignore-scripts fallow@2.0.0 - 2.5.0" "install: supports npm hyphen ranges"

OUT=$(run_gitlab_install "$INSTALL_TMP/empty" "file:../fallow")
cmd_status=$?
if [ "$cmd_status" -ne 0 ]; then
  pass "install: invalid file spec fails"
else
  fail "install: invalid file spec fails" "expected non-zero exit"
fi
assert_contains "$OUT" "Invalid version specifier" "install: invalid file spec explains failure"

OUT=$(run_gitlab_install "$INSTALL_TMP/empty" "2.0.0 -g malicious")
cmd_status=$?
if [ "$cmd_status" -ne 0 ]; then
  pass "install: rejects dash-prefixed extra args in spec"
else
  fail "install: rejects dash-prefixed extra args in spec" "expected non-zero exit"
fi

# FALLOW_SKIP_INSTALL: reuse a fallow already on PATH instead of npm install.
SKIP_BIN="$INSTALL_TMP/skip-bin"
mkdir -p "$SKIP_BIN"
cat > "$SKIP_BIN/fallow" <<'SH'
#!/usr/bin/env bash
echo "fallow 9.9.9"
SH
chmod +x "$SKIP_BIN/fallow"

# FALLOW_INSTALL_DRY_RUN=true stays set so the assertion proves the skip path
# short-circuits before the npm-install dry-run hook ever runs.
rm -f /tmp/fallow-version-spec
OUT=$(PATH="$SKIP_BIN:$PATH" FALLOW_ROOT="$INSTALL_TMP/empty" \
  FALLOW_SKIP_INSTALL=true FALLOW_INSTALL_DRY_RUN=true \
  /bin/sh -c "$GITLAB_INSTALL_SCRIPT" 2>&1)
skip_status=$?
if [ "$skip_status" -eq 0 ]; then
  pass "install: FALLOW_SKIP_INSTALL succeeds when fallow is on PATH"
else
  fail "install: FALLOW_SKIP_INSTALL succeeds when fallow is on PATH" "exit=$skip_status: $OUT"
fi
assert_contains "$OUT" "using pre-installed fallow 9.9.9" "install: FALLOW_SKIP_INSTALL reuses fallow on PATH"
assert_not_contains "$OUT" "DRY RUN: npm install" "install: FALLOW_SKIP_INSTALL skips npm install"
# The skip path must record the binary's semver to /tmp/fallow-version-spec so the
# MR-integration script-prep block can pin remote scripts (parity with install path).
assert_contains "$(cat /tmp/fallow-version-spec 2>/dev/null || true)" "9.9.9" "install: FALLOW_SKIP_INSTALL records binary semver for script-prep parity"

# No fallow on PATH -> clear, early error (controlled PATH keeps this hermetic).
OUT=$(PATH="/usr/bin:/bin" FALLOW_ROOT="$INSTALL_TMP/empty" \
  FALLOW_SKIP_INSTALL=true FALLOW_INSTALL_DRY_RUN=true \
  /bin/sh -c "$GITLAB_INSTALL_SCRIPT" 2>&1)
skip_status=$?
if [ "$skip_status" -eq 2 ]; then
  pass "install: FALLOW_SKIP_INSTALL fails with exit 2 when fallow is missing"
else
  fail "install: FALLOW_SKIP_INSTALL fails with exit 2 when fallow is missing" "expected exit 2, got $skip_status"
fi
assert_contains "$OUT" "no 'fallow' binary is on PATH" "install: FALLOW_SKIP_INSTALL explains missing binary"
assert_not_contains "$OUT" "DRY RUN: npm install" "install: missing-binary path never reaches npm install"

SCRIPT_PREP_TMP="$INSTALL_TMP/script-prep"
mkdir -p "$SCRIPT_PREP_TMP/ci/scripts"
printf '%s\n' '#!/usr/bin/env bash' 'echo comment' > "$SCRIPT_PREP_TMP/ci/scripts/comment.sh"
printf '%s\n' '#!/usr/bin/env bash' 'echo review' > "$SCRIPT_PREP_TMP/ci/scripts/review.sh"
printf '%s\n' '#!/usr/bin/env bash' 'echo common' > "$SCRIPT_PREP_TMP/ci/scripts/gitlab_common.sh"
rm -rf /tmp/fallow-scripts
OUT=$(cd "$SCRIPT_PREP_TMP" && FALLOW_COMMENT=true FALLOW_REVIEW=false /bin/sh -c "$GITLAB_SCRIPT_PREP_SCRIPT" 2>&1)
cmd_status=$?
if [ "$cmd_status" -eq 0 ]; then
  pass "script prep: wrapped block runs under sh"
else
  fail "script prep: wrapped block runs under sh" "$OUT"
fi
if [ -x /tmp/fallow-scripts/comment.sh ] && [ -x /tmp/fallow-scripts/review.sh ] && [ -x /tmp/fallow-scripts/gitlab_common.sh ]; then
  pass "script prep: copies vendored scripts"
else
  fail "script prep: copies vendored scripts" "expected executable scripts in /tmp/fallow-scripts"
fi

rm -f /tmp/fallow-run.sh
OUT=$(/bin/sh -c "$GITLAB_RUN_WRITER_SCRIPT" 2>&1)
cmd_status=$?
if [ "$cmd_status" -eq 0 ] && [ -x /tmp/fallow-run.sh ]; then
  pass "run writer: wrapped block runs under sh"
else
  fail "run writer: wrapped block runs under sh" "$OUT"
fi
if bash -n /tmp/fallow-run.sh 2>/tmp/fallow-run-syntax.err; then
  pass "run writer: generated analysis script is valid bash"
else
  fail "run writer: generated analysis script is valid bash" "$(cat /tmp/fallow-run-syntax.err)"
fi
RUNNER_TMP="$INSTALL_TMP/runner"
mkdir -p "$RUNNER_TMP/bin" "$RUNNER_TMP/root"
cat > "$RUNNER_TMP/bin/fallow" <<'SH'
#!/usr/bin/env bash
if [ -n "${FALLOW_TEST_LOG:-}" ]; then
  printf 'fallow %s\n' "$*" >> "$FALLOW_TEST_LOG"
fi
if [ -n "${FALLOW_TEST_ENV_FILE:-}" ]; then
  printf '%s\n' \
    "FALLOW_TYPE_AWARE=${FALLOW_TYPE_AWARE:-}" \
    "FALLOW_TYPE_AWARE_PROJECTS=${FALLOW_TYPE_AWARE_PROJECTS:-}" \
    "FALLOW_TYPE_AWARE_REQUIRE=${FALLOW_TYPE_AWARE_REQUIRE:-}" \
    > "$FALLOW_TEST_ENV_FILE"
fi
if [ "${1:-}" = "report" ]; then
  printf '[]\n'
  exit 0
fi
if [ -n "${MOCK_GATE_ENVELOPE:-}" ]; then
  cat "$MOCK_GATE_ENVELOPE"
  exit "${MOCK_GATE_EXIT:-0}"
fi
if [ "${MOCK_TYPE_AWARE_INCOMPLETE:-}" = "1" ]; then
  printf '%s\n' '{"kind":"dead-code","schema_version":9,"version":"test","total_issues":0,"_meta":{"type_aware":{"required_completeness":"complete","identity":{"completeness":"partial"},"queries":[]}}}'
  exit 1
fi
if [ "${MOCK_BASELINE_STALENESS:-}" = "1" ]; then
  if [ -n "${FALLOW_TEST_LOG:-}" ] && [ -n "${FALLOW_DIFF_FILE:-}" ]; then
    printf 'diff_file=set\n' >> "$FALLOW_TEST_LOG"
  fi
  # The real binary serializes scope_reasons in its own declaration order,
  # never in argv order, so the mock sorts into that order too.
  REASON_ORDER="diff changed-since changed-files workspace changed-workspaces scope file issue-type-filter production"
  found=""
  for arg in "$@"; do
    case "$arg" in
      --changed-since|--changed-since=*) found="$found changed-since" ;;
      --production) found="$found production" ;;
    esac
  done
  if [ -n "${FALLOW_DIFF_FILE:-}" ]; then
    found="$found diff"
  fi
  if [ -n "${MOCK_SCOPE_REASONS:-}" ]; then
    found=$(printf '%s' "$MOCK_SCOPE_REASONS" | tr ',' ' ')
  fi
  reasons=""
  for candidate in $REASON_ORDER; do
    case " $found " in
      *" $candidate "*)
        if [ -z "$reasons" ]; then reasons="\"$candidate\""; else reasons="$reasons,\"$candidate\""; fi
        ;;
    esac
  done
  scoped=false
  if [ -n "$reasons" ]; then
    scoped=true
  fi
  if [ "$scoped" = "true" ]; then
    if [ "${MOCK_NO_SCOPE_REASONS:-}" = "1" ]; then
      printf '%s\n' '{"total_issues":0,"baseline_staleness":{"baseline_entries":8,"matched_entries":0,"stale_entries":8,"current_findings":0,"change_scoped":true,"stale":false,"warning":"none","gate_trips":false}}'
    else
      printf '{"total_issues":0,"baseline_staleness":{"baseline_entries":8,"matched_entries":0,"stale_entries":8,"current_findings":0,"change_scoped":true,"stale":false,"warning":"none","gate_trips":false,"scope_reasons":[%s]}}\n' "$reasons"
    fi
    exit 0
  fi
  if [ "${MOCK_GATE_RUN_BROKEN:-}" = "1" ]; then
    printf 'not json at all\n'
    exit 2
  fi
  if [ "${MOCK_AUDIT_BASELINES:-}" = "2" ]; then
    # Every section states its recognition verdict outright, including a
    # literal `false`, which the shared reader keeps distinct from an absent
    # member.
    printf '%s\n' '{"kind":"audit","total_issues":0,"verdict":"pass","dead_code":{"baseline_staleness":{"baseline_entries":12,"matched_entries":4,"stale_entries":8,"current_findings":4,"change_scoped":true,"stale":false,"warning":"none","gate_trips":false,"unrecognised_format":false,"scope_reasons":["changed-since"]}},"complexity":{"summary":{"baseline_staleness":{"baseline_entries":0,"matched_entries":0,"stale_entries":0,"current_findings":0,"change_scoped":true,"stale":false,"warning":"none","gate_trips":true,"unrecognised_format":true,"scope_reasons":["changed-files"]}}},"gate_outcomes":{"stale-baseline":{"status":"skipped","enforced":false},"audit-verdict":{"status":"pass","enforced":true}}}'
    exit 0
  fi
  if [ "${MOCK_AUDIT_BASELINES:-}" = "1" ]; then
    printf '%s\n' '{"kind":"audit","total_issues":0,"verdict":"pass","dead_code":{"baseline_staleness":{"baseline_entries":12,"matched_entries":4,"stale_entries":8,"current_findings":4,"change_scoped":true,"stale":false,"warning":"none","gate_trips":false,"scope_reasons":["changed-since"]}},"duplication":{"baseline_staleness":{"baseline_entries":3,"matched_entries":0,"stale_entries":3,"current_findings":0,"change_scoped":true,"stale":false,"warning":"none","gate_trips":false,"scope_reasons":["changed-files"]}},"complexity":{"summary":{"baseline_staleness":{"baseline_entries":0,"matched_entries":0,"stale_entries":0,"current_findings":0,"change_scoped":true,"stale":false,"warning":"none","gate_trips":false,"unrecognised_format":true,"scope_reasons":["changed-files"]}}},"gate_outcomes":{"stale-baseline":{"status":"skipped","enforced":false},"audit-verdict":{"status":"pass","enforced":true}}}'
    exit 0
  fi
  if [ "${MOCK_UNRECOGNISED_BASELINE:-}" = "1" ]; then
    # gate_trips travels with the recognition verdict, as the binary reports it:
    # a file this command cannot read as its own suppresses nothing.
    printf '%s\n' '{"total_issues":0,"baseline_staleness":{"baseline_entries":0,"matched_entries":0,"stale_entries":0,"current_findings":0,"change_scoped":false,"stale":false,"warning":"none","gate_trips":true,"unrecognised_format":true}}'
    exit 0
  fi
  if [ "${MOCK_ZERO_ENTRY_BASELINE:-}" = "1" ]; then
    printf '%s\n' '{"total_issues":0,"baseline_staleness":{"baseline_entries":0,"matched_entries":0,"stale_entries":0,"current_findings":0,"change_scoped":false,"stale":false,"warning":"none","gate_trips":false}}'
    exit 0
  fi
  printf '%s\n' '{"total_issues":0,"baseline_staleness":{"baseline_entries":8,"matched_entries":3,"stale_entries":5,"current_findings":3,"change_scoped":false,"stale":true,"warning":"partial","gate_trips":true}}'
  exit 0
fi
printf '{"total_issues":0}\n'
SH
chmod +x "$RUNNER_TMP/bin/fallow"
OUT=$(cd "$RUNNER_TMP" && env \
  PATH="$RUNNER_TMP/bin:$PATH" \
  FALLOW_COMMAND= \
  FALLOW_ROOT="$RUNNER_TMP/root" \
  FALLOW_CONFIG= \
  FALLOW_PRODUCTION= \
  FALLOW_PRODUCTION_DEAD_CODE= \
  FALLOW_PRODUCTION_HEALTH= \
  FALLOW_PRODUCTION_DUPES= \
  FALLOW_FAIL_ON_ISSUES=false \
  FALLOW_MIN_SEVERITY= \
  FALLOW_INCLUDE_ENTRY_EXPORTS=false \
  FALLOW_ARGS= \
  FALLOW_COMMENT=false \
  FALLOW_REVIEW=false \
  FALLOW_REVIEW_GUIDANCE=false \
  FALLOW_CODEQUALITY=false \
  FALLOW_MAX_COMMENTS=50 \
  FALLOW_COMMENT_ID= \
  FALLOW_SUMMARY_SCOPE=all \
  FALLOW_DIFF_FILTER=added \
  FALLOW_DIFF_FILE= \
  FALLOW_API_RETRIES=3 \
  FALLOW_API_RETRY_DELAY=2 \
  FALLOW_GITLAB_BASE_SHA= \
  FALLOW_GITLAB_START_SHA= \
  FALLOW_GITLAB_HEAD_SHA= \
  FALLOW_CHANGED_SINCE= \
  FALLOW_BASELINE= \
  FALLOW_SAVE_BASELINE= \
  FALLOW_FAIL_ON_STALE_BASELINE=false \
  FALLOW_WORKSPACE= \
  FALLOW_CHANGED_WORKSPACES= \
  FALLOW_ISSUE_TYPES= \
  FALLOW_FAIL_ON_REGRESSION=false \
  FALLOW_TOLERANCE=0 \
  FALLOW_REGRESSION_BASELINE= \
  FALLOW_SAVE_REGRESSION_BASELINE= \
  FALLOW_DUPES_MODE=mild \
  FALLOW_MIN_TOKENS= \
  FALLOW_MIN_LINES= \
  FALLOW_THRESHOLD= \
  FALLOW_SKIP_LOCAL=false \
  FALLOW_CROSS_LANGUAGE=false \
  FALLOW_IGNORE_IMPORTS=false \
  FALLOW_MAX_CYCLOMATIC= \
  FALLOW_MAX_COGNITIVE= \
  FALLOW_MAX_CRAP= \
  FALLOW_COVERAGE=coverage/coverage-final.json \
  FALLOW_PRODUCTION_COVERAGE= \
  FALLOW_COVERAGE_ROOT=/ci/workspace \
  FALLOW_MIN_INVOCATIONS_HOT= \
  FALLOW_MIN_OBSERVATION_VOLUME= \
  FALLOW_LOW_TRAFFIC_THRESHOLD= \
  FALLOW_TOP= \
  FALLOW_SORT= \
  FALLOW_SCORE=false \
  FALLOW_FILE_SCORES=false \
  FALLOW_HOTSPOTS=false \
  FALLOW_TARGETS=false \
  FALLOW_COMPLEXITY=false \
  FALLOW_SINCE= \
  FALLOW_MIN_COMMITS= \
  FALLOW_SAVE_SNAPSHOT= \
  FALLOW_TREND=false \
  FALLOW_AUDIT_GATE= \
  FALLOW_AUDIT_DEAD_CODE_BASELINE= \
  FALLOW_AUDIT_HEALTH_BASELINE= \
  FALLOW_AUDIT_DUPES_BASELINE= \
  FALLOW_SECURITY_GATE= \
  FALLOW_DRY_RUN=true \
  FALLOW_NO_CACHE=false \
  FALLOW_THREADS= \
  FALLOW_TYPE_AWARE=true \
  FALLOW_TYPE_AWARE_PROJECTS=tsconfig.app.json,tsconfig.test.json \
  FALLOW_TYPE_AWARE_REQUIRE=complete \
  FALLOW_TEST_ENV_FILE="$RUNNER_TMP/type-aware-env" \
  FALLOW_ONLY= \
  FALLOW_SKIP= \
  FALLOW_SCRIPTS_REF= \
  bash /tmp/fallow-run.sh 2>&1)
cmd_status=$?
if [ "$cmd_status" -eq 0 ] && [ -s "$RUNNER_TMP/fallow-results.json" ]; then
  pass "run writer: generated analysis script runs with empty extra args"
else
  fail "run writer: generated analysis script runs with empty extra args" "$OUT"
fi
ARGS=""
while IFS= read -r -d '' arg; do
  ARGS+="${arg} "
done < "$RUNNER_TMP/fallow-analysis-args.bin"
assert_contains "$ARGS" "--coverage coverage/coverage-final.json" "run writer: forwards coverage to default combined command"
assert_contains "$ARGS" "--coverage-root /ci/workspace" "run writer: forwards coverage-root to default combined command"
TYPE_AWARE_ENV=$(cat "$RUNNER_TMP/type-aware-env")
assert_contains "$ARGS" "--type-aware" "run writer: enables type-aware CLI mode"
assert_contains "$ARGS" "--type-aware-project tsconfig.app.json" "run writer: forwards first type-aware project separately"
assert_contains "$ARGS" "--type-aware-project tsconfig.test.json" "run writer: forwards second type-aware project separately"
assert_contains "$ARGS" "--type-aware-require complete" "run writer: forwards type-aware completeness policy"
assert_contains "$TYPE_AWARE_ENV" "FALLOW_TYPE_AWARE_PROJECTS=" "run writer: clears project env after CLI translation"
assert_contains "$TYPE_AWARE_ENV" "FALLOW_TYPE_AWARE_REQUIRE=" "run writer: clears require env after CLI translation"

run_generated_gitlab_fixture() {
  local work=$1
  shift
  local defaults=()
  local name
  while IFS= read -r name; do
    defaults+=("${name}=")
  done < <(grep -oE '\$FALLOW_[A-Z0-9_]+' /tmp/fallow-run.sh | tr -d '$' | sort -u)
  (
    cd "$work" || exit 1
    env "${defaults[@]}" \
      PATH="$RUNNER_TMP/bin:$PATH" \
      FALLOW_COMMAND=check \
      FALLOW_ROOT="$RUNNER_TMP/root" \
      FALLOW_FAIL_ON_ISSUES=false \
      FALLOW_DUPES_MODE=mild \
      FALLOW_DRY_RUN=true \
      FALLOW_MAX_COMMENTS=50 \
      "$@" \
      bash /tmp/fallow-run.sh 2>&1
  )
}

# --- Baseline staleness gate (issue #2673) ---

STALE_WORK="$RUNNER_TMP/stale-baseline"
mkdir -p "$STALE_WORK"

OUT=$(run_generated_gitlab_fixture "$STALE_WORK" \
  MOCK_BASELINE_STALENESS=1 \
  FALLOW_BASELINE=baseline.json)
cmd_status=$?
assert_contains "$OUT" "WARNING: baseline is partially stale: 5 of 8 entries" \
  "stale gate: a partially stale baseline warns without the gate"
if [ "$cmd_status" -eq 0 ]; then
  pass "stale gate: the advisory alone does not fail the pipeline"
else
  fail "stale gate: the advisory alone does not fail the pipeline" "exit $cmd_status"
fi

rm -rf "$STALE_WORK"; mkdir -p "$STALE_WORK"
OUT=$(run_generated_gitlab_fixture "$STALE_WORK" \
  MOCK_BASELINE_STALENESS=1 \
  FALLOW_BASELINE=baseline.json \
  FALLOW_FAIL_ON_STALE_BASELINE=true)
cmd_status=$?
assert_contains "$OUT" "ERROR: Fallow baseline gate failed: 5 of 8 entries" \
  "stale gate: the gate fails the pipeline"
if [ "$cmd_status" -eq 1 ]; then
  pass "stale gate: a tripped gate exits 1"
else
  fail "stale gate: a tripped gate exits 1" "exit $cmd_status"
fi

# The template's own MR auto-scoping is the shape that made a plain variable
# inert, so the gate re-runs the comparison unscoped.
rm -rf "$STALE_WORK"; mkdir -p "$STALE_WORK"
STALE_LOG="$STALE_WORK/fallow.log"
OUT=$(run_generated_gitlab_fixture "$STALE_WORK" \
  MOCK_BASELINE_STALENESS=1 \
  FALLOW_TEST_LOG="$STALE_LOG" \
  FALLOW_BASELINE=baseline.json \
  FALLOW_SAVE_BASELINE=baseline.json \
  FALLOW_CHANGED_SINCE=abc123 \
  FALLOW_FAIL_ON_STALE_BASELINE=true)
cmd_status=$?
GATE_ARGV=$(sed -n '2p' "$STALE_LOG")
assert_not_contains "$GATE_ARGV" "--changed-since" \
  "stale gate: the unscoped re-run drops --changed-since"
assert_not_contains "$GATE_ARGV" "--save-baseline" \
  "stale gate: the unscoped re-run never rewrites the baseline"
assert_contains "$GATE_ARGV" "--baseline baseline.json" \
  "stale gate: the unscoped re-run still loads the baseline"
assert_contains "$OUT" "ERROR: Fallow baseline gate failed" \
  "stale gate: the re-run's verdict fails the merge-request pipeline"
if [ "$cmd_status" -eq 1 ]; then
  pass "stale gate: the merge-request re-run can fail the pipeline"
else
  fail "stale gate: the merge-request re-run can fail the pipeline" "exit $cmd_status"
fi

# The advisory is not gated behind the variable: a merge-request pipeline
# re-reads the baseline unscoped so the warning reaches a job that asked for no
# gate.
rm -rf "$STALE_WORK"; mkdir -p "$STALE_WORK"
STALE_LOG="$STALE_WORK/fallow.log"
OUT=$(run_generated_gitlab_fixture "$STALE_WORK" \
  MOCK_BASELINE_STALENESS=1 \
  FALLOW_TEST_LOG="$STALE_LOG" \
  FALLOW_BASELINE=baseline.json \
  FALLOW_CHANGED_SINCE=abc123)
STALE_RUNS=$(grep -c '^fallow ' "$STALE_LOG" || true)
if [ "$STALE_RUNS" = "2" ]; then
  pass "stale gate: a scoped pipeline re-reads the baseline even with the gate off"
else
  fail "stale gate: a scoped pipeline re-reads the baseline even with the gate off" "ran $STALE_RUNS times"
fi
assert_contains "$OUT" "WARNING: baseline is partially stale: 5 of 8 entries" \
  "stale gate: the advisory reaches a merge-request pipeline with no gate"
assert_not_contains "$OUT" "ERROR: Fallow baseline gate failed" \
  "stale gate: with the gate off the advisory never fails the pipeline"

rm -rf "$STALE_WORK"; mkdir -p "$STALE_WORK"
STALE_LOG="$STALE_WORK/fallow.log"
OUT=$(run_generated_gitlab_fixture "$STALE_WORK" \
  MOCK_BASELINE_STALENESS=1 \
  FALLOW_TEST_LOG="$STALE_LOG" \
  FALLOW_BASELINE=baseline.json)
STALE_RUNS=$(grep -c '^fallow ' "$STALE_LOG" || true)
if [ "$STALE_RUNS" = "1" ]; then
  pass "stale gate: an unscoped pipeline analyzes exactly once"
else
  fail "stale gate: an unscoped pipeline analyzes exactly once" "ran $STALE_RUNS times"
fi

# The stand-down names the channels the run reported, and scoping smuggled
# through FALLOW_ARGS is visible there instead of sending the template into an
# unscoped re-read that comes back narrowed anyway.
rm -rf "$STALE_WORK"; mkdir -p "$STALE_WORK"
STALE_LOG="$STALE_WORK/fallow.log"
OUT=$(run_generated_gitlab_fixture "$STALE_WORK" \
  MOCK_BASELINE_STALENESS=1 \
  MOCK_SCOPE_REASONS=production \
  FALLOW_TEST_LOG="$STALE_LOG" \
  FALLOW_BASELINE=baseline.json)
STALE_RUNS=$(grep -c '^fallow ' "$STALE_LOG" || true)
if [ "$STALE_RUNS" = "1" ]; then
  pass "stale gate: an unremovable reason skips the re-read even with no matching variable"
else
  fail "stale gate: an unremovable reason skips the re-read even with no matching variable" "ran $STALE_RUNS times"
fi
assert_contains "$OUT" "only part of the project (production)" \
  "stale gate: the stand-down names the smuggled channel"

# Reasons this template can remove still earn the re-read.
rm -rf "$STALE_WORK"; mkdir -p "$STALE_WORK"
STALE_LOG="$STALE_WORK/fallow.log"
OUT=$(run_generated_gitlab_fixture "$STALE_WORK" \
  MOCK_BASELINE_STALENESS=1 \
  MOCK_SCOPE_REASONS=changed-files,scope \
  FALLOW_TEST_LOG="$STALE_LOG" \
  FALLOW_BASELINE=baseline.json)
STALE_RUNS=$(grep -c '^fallow ' "$STALE_LOG" || true)
if [ "$STALE_RUNS" = "2" ]; then
  pass "stale gate: removable reasons still earn the unscoped re-read"
else
  fail "stale gate: removable reasons still earn the unscoped re-read" "ran $STALE_RUNS times"
fi

# A binary that predates the member keeps the variable-based guess.
rm -rf "$STALE_WORK"; mkdir -p "$STALE_WORK"
STALE_LOG="$STALE_WORK/fallow.log"
OUT=$(run_generated_gitlab_fixture "$STALE_WORK" \
  MOCK_BASELINE_STALENESS=1 \
  MOCK_NO_SCOPE_REASONS=1 \
  FALLOW_TEST_LOG="$STALE_LOG" \
  FALLOW_BASELINE=baseline.json \
  FALLOW_CHANGED_SINCE=abc123 \
  FALLOW_PRODUCTION=true)
assert_contains "$OUT" "only part of the project (production mode or workspace scoping)" \
  "stale gate: a binary without the member falls back to the variable-based reason"

# A baseline written by another command suppresses nothing, so the pipeline says
# so and an armed gate fails on it. The branch reads the binary's own verdict,
# not the entry count.
rm -rf "$STALE_WORK"; mkdir -p "$STALE_WORK"
OUT=$(run_generated_gitlab_fixture "$STALE_WORK" \
  MOCK_BASELINE_STALENESS=1 \
  MOCK_UNRECOGNISED_BASELINE=1 \
  FALLOW_BASELINE=wrong-kind.json)
assert_contains "$OUT" "WARNING: the baseline at wrong-kind.json has no entries this command recognises" \
  "stale gate: a baseline that recognises nothing is called out"
assert_not_contains "$OUT" "0 of 0 baseline entries matched nothing" \
  "stale gate: the count advisory stands aside for the recognition warning"
assert_not_contains "$OUT" "ERROR: Fallow baseline gate failed" \
  "stale gate: with no gate armed a baseline nothing recognises does not fail the pipeline"

# With the gate armed the pipeline fails, and names the recognition failure
# rather than a count both sides of which are zero.
rm -rf "$STALE_WORK"; mkdir -p "$STALE_WORK"
STALE_UNRECOGNISED_EXIT=0
OUT=$(run_generated_gitlab_fixture "$STALE_WORK" \
  MOCK_BASELINE_STALENESS=1 \
  MOCK_UNRECOGNISED_BASELINE=1 \
  FALLOW_BASELINE=wrong-kind.json \
  FALLOW_FAIL_ON_STALE_BASELINE=true 2>&1) || STALE_UNRECOGNISED_EXIT=$?
assert_contains "$OUT" "ERROR: Fallow baseline gate failed: the baseline wrong-kind.json has no entries this command recognises" \
  "stale gate: the armed gate names the recognition failure"
if [ "$STALE_UNRECOGNISED_EXIT" -eq 1 ]; then
  pass "stale gate: an armed gate fails on a baseline nothing recognises"
else
  fail "stale gate: an armed gate fails on a baseline nothing recognises" "exit $STALE_UNRECOGNISED_EXIT"
fi

# A baseline passed through FALLOW_ARGS never reaches FALLOW_BASELINE, so the
# line degrades to the subject instead of going missing.
rm -rf "$STALE_WORK"; mkdir -p "$STALE_WORK"
OUT=$(run_generated_gitlab_fixture "$STALE_WORK" \
  MOCK_BASELINE_STALENESS=1 \
  MOCK_UNRECOGNISED_BASELINE=1 \
  FALLOW_ARGS="--baseline wrong-kind.json")
assert_contains "$OUT" "WARNING: the loaded baseline has no entries this command recognises" \
  "stale gate: a baseline passed through FALLOW_ARGS is called out without a path"

# A baseline saved on a project with nothing to record carries zero entries and
# is not a mistake, so the documented save-on-green-main workflow stays quiet.
rm -rf "$STALE_WORK"; mkdir -p "$STALE_WORK"
OUT=$(run_generated_gitlab_fixture "$STALE_WORK" \
  MOCK_BASELINE_STALENESS=1 \
  MOCK_ZERO_ENTRY_BASELINE=1 \
  FALLOW_BASELINE=own-empty.json)
assert_not_contains "$OUT" "has no entries this command recognises" \
  "stale gate: a baseline this command saved itself is never called the wrong file"

# A populated baseline never earns that warning.
rm -rf "$STALE_WORK"; mkdir -p "$STALE_WORK"
OUT=$(run_generated_gitlab_fixture "$STALE_WORK" \
  MOCK_BASELINE_STALENESS=1 \
  FALLOW_BASELINE=baseline.json)
assert_not_contains "$OUT" "has no entries this command recognises" \
  "stale gate: a populated baseline says nothing about recognition"

# fallow audit loads up to three baselines and judges none of them, and the
# single-analysis // chain is first-match, so it would report one and hide the
# other two. One line per section instead, naming the command a reader has to
# run rather than the section it sits in.
rm -rf "$STALE_WORK"; mkdir -p "$STALE_WORK"
OUT=$(run_generated_gitlab_fixture "$STALE_WORK" \
  MOCK_BASELINE_STALENESS=1 \
  MOCK_AUDIT_BASELINES=1 \
  FALLOW_COMMAND=audit \
  FALLOW_AUDIT_DEAD_CODE_BASELINE=audit/dc.json \
  FALLOW_AUDIT_DUPES_BASELINE=audit/du.json \
  FALLOW_AUDIT_HEALTH_BASELINE=audit/he.json)
assert_contains "$OUT" "NOTICE: the dead-code baseline (audit/dc.json) has 12 entries and was not judged" \
  "audit baselines: the dead-code baseline is reported with its path"
assert_contains "$OUT" "Run 'fallow dupes --baseline audit/du.json' over the whole project" \
  "audit baselines: duplication points at fallow dupes, not at the section name"
assert_contains "$OUT" "WARNING: the complexity baseline at audit/he.json has no entries this command recognises" \
  "audit baselines: an unrecognised audit baseline gets the recognition warning"

# Each section's own recognition verdict decides its line, including a section
# that states `false` outright: the loop reads it through the shared reader,
# whose `has` guard keeps a literal `false` from reading as an absent member.
rm -rf "$STALE_WORK"; mkdir -p "$STALE_WORK"
OUT=$(run_generated_gitlab_fixture "$STALE_WORK" \
  MOCK_BASELINE_STALENESS=1 \
  MOCK_AUDIT_BASELINES=2 \
  FALLOW_COMMAND=audit \
  FALLOW_AUDIT_DEAD_CODE_BASELINE=audit/dc.json \
  FALLOW_AUDIT_HEALTH_BASELINE=audit/he.json)
assert_contains "$OUT" "NOTICE: the dead-code baseline (audit/dc.json) has 12 entries and was not judged" \
  "audit baselines: a section that reports recognition false keeps the inert-baseline notice"
assert_contains "$OUT" "WARNING: the complexity baseline at audit/he.json has no entries this command recognises" \
  "audit baselines: and the section beside it still earns the recognition warning"
assert_not_contains "$OUT" "the dead-code baseline at audit/dc.json has no entries" \
  "audit baselines: a recognised baseline is never called the wrong file"

# The gate cannot apply to audit and FALLOW_BASELINE is already rejected for
# it, so the pair is only reachable through FALLOW_ARGS.
rm -rf "$STALE_WORK"; mkdir -p "$STALE_WORK"
# This suite runs without `set -e`, so the exit code is captured inline rather
# than by toggling it: enabling it here would abort every later case.
STALE_ARGS_EXIT=0
OUT=$(run_generated_gitlab_fixture "$STALE_WORK" \
  FALLOW_COMMAND=audit \
  FALLOW_ARGS=--fail-on-stale-baseline 2>&1) || STALE_ARGS_EXIT=$?
if [ "$STALE_ARGS_EXIT" -eq 2 ]; then
  pass "audit baselines: --fail-on-stale-baseline smuggled through FALLOW_ARGS is rejected"
else
  fail "audit baselines: --fail-on-stale-baseline smuggled through FALLOW_ARGS is rejected" "exit $STALE_ARGS_EXIT"
fi
assert_contains "$OUT" "cannot apply to command: audit" \
  "audit baselines: the rejection says why"

# Diff scoping reaches the CLI through FALLOW_DIFF_FILE, not argv.
rm -rf "$STALE_WORK"; mkdir -p "$STALE_WORK"
STALE_LOG="$STALE_WORK/fallow.log"
OUT=$(run_generated_gitlab_fixture "$STALE_WORK" \
  MOCK_BASELINE_STALENESS=1 \
  FALLOW_TEST_LOG="$STALE_LOG" \
  FALLOW_BASELINE=baseline.json \
  FALLOW_DIFF_FILE=/tmp/does-not-matter.diff \
  FALLOW_FAIL_ON_STALE_BASELINE=true)
STALE_DIFF_LINES=$(grep -c '^diff_file=set' "$STALE_LOG" || true)
if [ "$STALE_DIFF_LINES" = "1" ]; then
  pass "stale gate: the unscoped re-read runs with FALLOW_DIFF_FILE cleared"
else
  fail "stale gate: the unscoped re-read runs with FALLOW_DIFF_FILE cleared" \
    "saw $STALE_DIFF_LINES invocations with the variable set"
fi
assert_contains "$OUT" "ERROR: Fallow baseline gate failed" \
  "stale gate: clearing the diff file lets the re-read judge the baseline"

# `--save-snapshot` takes an optional value: the strip must not swallow the flag
# that follows the bare form.
rm -rf "$STALE_WORK"; mkdir -p "$STALE_WORK"
STALE_LOG="$STALE_WORK/fallow.log"
OUT=$(run_generated_gitlab_fixture "$STALE_WORK" \
  MOCK_BASELINE_STALENESS=1 \
  FALLOW_TEST_LOG="$STALE_LOG" \
  FALLOW_BASELINE=baseline.json \
  FALLOW_COMMAND=health \
  FALLOW_CHANGED_SINCE=abc123 \
  FALLOW_SAVE_SNAPSHOT=true \
  FALLOW_TREND=true \
  FALLOW_FAIL_ON_STALE_BASELINE=true)
GATE_ARGV=$(grep '^fallow ' "$STALE_LOG" | sed -n '2p')
assert_not_contains "$GATE_ARGV" "--save-snapshot" \
  "stale gate: the re-read never writes a snapshot"
assert_contains "$GATE_ARGV" "--trend" \
  "stale gate: a bare --save-snapshot does not swallow the next flag"

# A baseline re-saved to the path it is read from can never be stale.
rm -rf "$STALE_WORK"; mkdir -p "$STALE_WORK"
OUT=$(run_generated_gitlab_fixture "$STALE_WORK" \
  MOCK_BASELINE_STALENESS=1 \
  FALLOW_BASELINE=baseline.json \
  FALLOW_SAVE_BASELINE=baseline.json)
assert_contains "$OUT" "name the same file" \
  "stale gate: a self-healing baseline is called out"

rm -rf "$STALE_WORK"; mkdir -p "$STALE_WORK"
STALE_LOG="$STALE_WORK/fallow.log"
OUT=$(run_generated_gitlab_fixture "$STALE_WORK" \
  MOCK_BASELINE_STALENESS=1 \
  FALLOW_TEST_LOG="$STALE_LOG" \
  FALLOW_BASELINE=baseline.json \
  FALLOW_CHANGED_SINCE=abc123 \
  FALLOW_PRODUCTION=true \
  FALLOW_FAIL_ON_STALE_BASELINE=true)
cmd_status=$?
STALE_RUNS=$(grep -c '^fallow ' "$STALE_LOG" || true)
if [ "$STALE_RUNS" = "1" ]; then
  pass "stale gate: production mode skips the re-run instead of paying for it"
else
  fail "stale gate: production mode skips the re-run instead of paying for it" "ran $STALE_RUNS times"
fi
assert_contains "$OUT" "WARNING: baseline staleness could not be judged" \
  "stale gate: a stand-down is stated, never silent"
assert_contains "$OUT" "FALLOW_FAIL_ON_STALE_BASELINE stood down." \
  "stale gate: the stand-down names the gate when the gate was asked for"
if [ "$cmd_status" -eq 0 ]; then
  pass "stale gate: a stand-down does not fail the pipeline"
else
  fail "stale gate: a stand-down does not fail the pipeline" "exit $cmd_status"
fi

# The same stand-down with no gate asked for is a note, not a warning.
rm -rf "$STALE_WORK"; mkdir -p "$STALE_WORK"
OUT=$(run_generated_gitlab_fixture "$STALE_WORK" \
  MOCK_BASELINE_STALENESS=1 \
  FALLOW_BASELINE=baseline.json \
  FALLOW_CHANGED_SINCE=abc123 \
  FALLOW_PRODUCTION=true)
assert_contains "$OUT" "NOTE: baseline staleness could not be judged" \
  "stale gate: a stand-down with no gate asked for is a note"
assert_not_contains "$OUT" "WARNING: baseline staleness could not be judged" \
  "stale gate: a pipeline that asked for nothing is not warned at"

# A re-read that produces nothing readable warns and leaves the pipeline green.
rm -rf "$STALE_WORK"; mkdir -p "$STALE_WORK"
OUT=$(run_generated_gitlab_fixture "$STALE_WORK" \
  MOCK_BASELINE_STALENESS=1 \
  MOCK_GATE_RUN_BROKEN=1 \
  FALLOW_BASELINE=baseline.json \
  FALLOW_CHANGED_SINCE=abc123 \
  FALLOW_FAIL_ON_STALE_BASELINE=true)
cmd_status=$?
assert_contains "$OUT" "produced no readable result" \
  "stale gate: a broken re-read warns"
if [ "$cmd_status" -eq 0 ]; then
  pass "stale gate: a broken re-read fails open"
else
  fail "stale gate: a broken re-read fails open" "exit $cmd_status"
fi

# A binary older than the envelope field must warn, not fail.
rm -rf "$STALE_WORK"; mkdir -p "$STALE_WORK"
OUT=$(run_generated_gitlab_fixture "$STALE_WORK" \
  FALLOW_BASELINE=baseline.json \
  FALLOW_FAIL_ON_STALE_BASELINE=true)
cmd_status=$?
assert_contains "$OUT" "A fallow that predates this feature cannot report it" \
  "stale gate: an old binary warns instead of failing silently"
if [ "$cmd_status" -eq 0 ]; then
  pass "stale gate: an old binary fails open"
else
  fail "stale gate: an old binary fails open" "exit $cmd_status"
fi

rm -rf "$STALE_WORK"; mkdir -p "$STALE_WORK"
OUT=$(run_generated_gitlab_fixture "$STALE_WORK" \
  FALLOW_FAIL_ON_STALE_BASELINE=true)
cmd_status=$?
assert_contains "$OUT" "has no baseline to judge" \
  "stale gate: the gate without a baseline is rejected"
if [ "$cmd_status" -eq 2 ]; then
  pass "stale gate: an invalid combination exits 2"
else
  fail "stale gate: an invalid combination exits 2" "exit $cmd_status"
fi

INCOMPLETE_WORK="$RUNNER_TMP/type-aware-incomplete"
mkdir -p "$INCOMPLETE_WORK"
OUT=$(run_generated_gitlab_fixture "$INCOMPLETE_WORK" \
  MOCK_TYPE_AWARE_INCOMPLETE=1 \
  FALLOW_COMMENT=true \
  FALLOW_REVIEW=true \
  CI_MERGE_REQUEST_IID=123)
cmd_status=$?
if [ "$cmd_status" -eq 1 ]; then
  pass "run writer: required incomplete type-aware analysis still fails closed"
else
  fail "run writer: required incomplete type-aware analysis still fails closed" "expected exit 1, got $cmd_status"
fi
assert_contains "$OUT" "comment" "run writer: required incomplete analysis renders MR comment before failing"
assert_contains "$OUT" "review" "run writer: required incomplete analysis renders MR review before failing"
assert_contains "$OUT" "Type-aware completeness gate failed" "run writer: deferred completeness failure remains explicit"

CODEQUALITY_WORK="$RUNNER_TMP/codequality-prefix"
CODEQUALITY_LOG="$CODEQUALITY_WORK/fallow.log"
mkdir -p "$CODEQUALITY_WORK"
OUT=$(run_generated_gitlab_fixture "$CODEQUALITY_WORK" \
  FALLOW_CODEQUALITY=true \
  FALLOW_ARGS="--report-path-prefix custom/base" \
  FALLOW_TEST_LOG="$CODEQUALITY_LOG")
cmd_status=$?
if [ "$cmd_status" -eq 0 ]; then
  pass "run writer: Code Quality prefix fixture succeeds"
else
  fail "run writer: Code Quality prefix fixture succeeds" "$OUT"
fi
assert_contains "$(cat "$CODEQUALITY_LOG")" \
  "report --from fallow-results.json --root $RUNNER_TMP/root --format codeclimate --quiet --report-path-prefix custom/base" \
  "run writer: Code Quality rendering preserves explicit path prefix"

# =========================================================================
# Behavioral parity between action/scripts/install.sh and ci/gitlab-ci.yml
# =========================================================================
#
# Both implementations must agree on every spec input. Logic drift between
# the two copies is a covert privilege escalation vector specific to one CI
# provider. Catches divergence even when comments or indentation differ.

echo ""
echo "=== Install path parity (action vs gitlab) ==="

ACTION_INSTALL_SH="$DIR/../../action/scripts/install.sh"

# Drive both implementations through their dry-run path with the same matrix
# of inputs and assert each one's exit code and final install_arg agree.
parity_run_action() {
  local root="$1"
  local version="$2"
  INPUT_ROOT="$root" FALLOW_VERSION="$version" FALLOW_INSTALL_DRY_RUN=true \
    bash "$ACTION_INSTALL_SH" 2>&1
}

parity_run_gitlab() {
  local root="$1"
  local version="$2"
  FALLOW_ROOT="$root" FALLOW_VERSION="$version" FALLOW_INSTALL_DRY_RUN=true \
    /bin/sh -c "$GITLAB_INSTALL_SCRIPT" 2>&1
}

extract_install_arg() {
  printf '%s\n' "$1" | grep -Eo 'DRY RUN: npm install -g --ignore-scripts .*' | head -n 1 \
    | sed 's/^DRY RUN: npm install -g --ignore-scripts //'
}

assert_parity() {
  local name="$1" root="$2" version="$3"
  local action_out gitlab_out action_status gitlab_status
  # ci/tests/run.sh does not run under `set -e`, so we can capture the inner
  # exit code directly. Wrapping with `|| true` would mask divergence in the
  # exit-code half of the comparison.
  action_out="$(parity_run_action "$root" "$version")"
  action_status=$?
  gitlab_out="$(parity_run_gitlab "$root" "$version")"
  gitlab_status=$?

  local action_arg gitlab_arg
  action_arg="$(extract_install_arg "$action_out")"
  gitlab_arg="$(extract_install_arg "$gitlab_out")"

  if [ "$action_status" = "$gitlab_status" ] && [ "$action_arg" = "$gitlab_arg" ]; then
    pass "parity: $name"
  else
    fail "parity: $name" \
      "action exit=$action_status arg='$action_arg' / gitlab exit=$gitlab_status arg='$gitlab_arg'"
  fi
}

# Both must agree on the safe inputs.
assert_parity "reads pinned package.json" "$INSTALL_TMP/pinned" ""
assert_parity "reads semver range from package.json" "$INSTALL_TMP/range" ""
assert_parity "explicit FALLOW_VERSION=latest wins" "$INSTALL_TMP/pinned" "latest"
assert_parity "no spec falls back to latest" "$INSTALL_TMP/empty" ""
assert_parity "explicit semver range is honoured" "$INSTALL_TMP/empty" "^2.52.0"
assert_parity "explicit hyphen range is honoured" "$INSTALL_TMP/empty" "2.0.0 - 2.5.0"
# And on every shape the validator must reject. If the two implementations
# diverge here, one CI provider would silently accept an unsafe spec.
assert_parity "rejects file: scheme" "$INSTALL_TMP/empty" "file:../fallow"
assert_parity "rejects npm: alias" "$INSTALL_TMP/empty" "npm:lodash@1.0.0"
assert_parity "rejects git+ssh URL" "$INSTALL_TMP/empty" "git+ssh://x.example/y.git"
assert_parity "rejects workspace: protocol" "$INSTALL_TMP/empty" "workspace:*"
assert_parity "rejects dash-prefixed extra args" "$INSTALL_TMP/empty" "2.0.0 -g malicious"
assert_parity "rejects semicolon command separator" "$INSTALL_TMP/empty" "2.0.0;rm -rf /"
assert_parity "rejects dollar-paren command sub" "$INSTALL_TMP/empty" '2.0.0$(touch /tmp/x)'
assert_parity "rejects backtick command sub" "$INSTALL_TMP/empty" '2.0.0`touch /tmp/x`'
# Unsupported package.json spec (e.g. workspace:*) must produce the same
# fall-back-to-latest decision in both implementations.
assert_parity "unsupported package.json spec falls back" "$INSTALL_TMP/unsafe" ""

# =========================================================================
# Wrapper trap parity (action vs gitlab)
# =========================================================================
#
# Two trap blocks landed in both action/scripts/analyze.sh and
# ci/gitlab-ci.yml at the same time and must stay in lockstep. If a future
# edit lands in one wrapper but not the other, the two CI providers diverge
# on whether they:
#   1. Reject `--baseline` / `--save-baseline` when command=audit.
#   2. Treat fallow's structured-error JSON envelope as fatal before the
#      issue counter sees null fields and emits issues=0.
# Asserting symmetric presence catches single-side edits without locking
# down indentation or provider-specific env-var prefix differences.

echo ""
echo "=== Wrapper trap parity (action vs gitlab) ==="

ACTION_ANALYZE_SH="$DIR/../../action/scripts/analyze.sh"
CI_TEMPLATE_YAML="$DIR/../gitlab-ci.yml"

# Audit baseline rejection: both must check command=audit AND a non-empty
# generic baseline / save-baseline before invoking fallow.
ACTION_HAS_AUDIT_BASELINE_TRAP=$(grep -cE 'INPUT_COMMAND.*=.*"audit".*INPUT_(SAVE_)?BASELINE' "$ACTION_ANALYZE_SH" 2>/dev/null || echo 0)
CI_HAS_AUDIT_BASELINE_TRAP=$(grep -cE 'FALLOW_COMMAND.*=.*"audit".*FALLOW_(SAVE_)?BASELINE' "$CI_TEMPLATE_YAML" 2>/dev/null || echo 0)
if [ "$ACTION_HAS_AUDIT_BASELINE_TRAP" != "0" ] && [ "$CI_HAS_AUDIT_BASELINE_TRAP" != "0" ]; then
  pass "parity: both wrappers reject generic baseline on audit"
elif [ "$ACTION_HAS_AUDIT_BASELINE_TRAP" = "0" ] && [ "$CI_HAS_AUDIT_BASELINE_TRAP" = "0" ]; then
  pass "parity: neither wrapper has audit baseline trap (consistent)"
else
  fail "parity: audit baseline trap" \
    "asymmetric: action=$ACTION_HAS_AUDIT_BASELINE_TRAP, gitlab=$CI_HAS_AUDIT_BASELINE_TRAP"
fi

# Both must point users at the audit-specific baseline inputs by name.
assert_contains "$(cat "$ACTION_ANALYZE_SH")" "dead-code-baseline" \
  "parity: action error message names dead-code-baseline"
assert_contains "$(cat "$CI_TEMPLATE_YAML")" "FALLOW_AUDIT_DEAD_CODE_BASELINE" \
  "parity: gitlab error message names FALLOW_AUDIT_DEAD_CODE_BASELINE"

# Structured-error trap: both must inspect `.error == true` in
# fallow-results.json BEFORE any `// 0`-defaulted issue extraction.
ACTION_HAS_ERROR_TRAP=$(grep -cE "jq -e.*\.error == true.*fallow-results\.json" "$ACTION_ANALYZE_SH" 2>/dev/null || echo 0)
CI_HAS_ERROR_TRAP=$(grep -cE "jq -e.*\.error == true.*fallow-results\.json" "$CI_TEMPLATE_YAML" 2>/dev/null || echo 0)
if [ "$ACTION_HAS_ERROR_TRAP" != "0" ] && [ "$CI_HAS_ERROR_TRAP" != "0" ]; then
  pass "parity: both wrappers trap structured fallow errors before issue extraction"
elif [ "$ACTION_HAS_ERROR_TRAP" = "0" ] && [ "$CI_HAS_ERROR_TRAP" = "0" ]; then
  pass "parity: neither wrapper has structured-error trap (consistent)"
else
  fail "parity: structured-error trap" \
    "asymmetric: action=$ACTION_HAS_ERROR_TRAP, gitlab=$CI_HAS_ERROR_TRAP"
fi

# Verdict-driven threshold for audit: both wrappers must gate on
# `verdict == "fail"` for audit (severity-aware), not on raw issue count.
# Otherwise warn-tier findings fail CI even though the verdict says "warn"
# (the original issue #302 bug).
ACTION_HAS_VERDICT_GATE=$(grep -cE 'VERDICT.*=.*"fail"|VERDICT" = "fail"' "$ACTION_ANALYZE_SH" "$DIR/../../action.yml" 2>/dev/null | awk -F: '{s+=$2} END {print s}')
CI_HAS_VERDICT_GATE=$(grep -cE 'VERDICT.*=.*"fail"|VERDICT" = "fail"' "$CI_TEMPLATE_YAML" 2>/dev/null || echo 0)
if [ "$ACTION_HAS_VERDICT_GATE" != "0" ] && [ "$CI_HAS_VERDICT_GATE" != "0" ]; then
  pass "parity: both wrappers gate audit on verdict, not raw count"
else
  fail "parity: verdict-driven threshold" \
    "asymmetric: action=$ACTION_HAS_VERDICT_GATE, gitlab=$CI_HAS_VERDICT_GATE"
fi

# Both wrappers must extract verdict + gate from audit JSON before issue count.
ACTION_HAS_VERDICT_EXTRACT=$(grep -cE 'VERDICT=\$\(jq -r .*\.verdict' "$ACTION_ANALYZE_SH" 2>/dev/null || echo 0)
CI_HAS_VERDICT_EXTRACT=$(grep -cE 'VERDICT=\$\(jq -r .*\.verdict' "$CI_TEMPLATE_YAML" 2>/dev/null || echo 0)
if [ "$ACTION_HAS_VERDICT_EXTRACT" != "0" ] && [ "$CI_HAS_VERDICT_EXTRACT" != "0" ]; then
  pass "parity: both wrappers extract verdict from audit JSON"
else
  fail "parity: verdict extraction" \
    "asymmetric: action=$ACTION_HAS_VERDICT_EXTRACT, gitlab=$CI_HAS_VERDICT_EXTRACT"
fi

# Security gate support must stay symmetric across the official wrappers.
assert_contains "$(cat "$ACTION_ANALYZE_SH")" 'INPUT_COMMAND" in' \
  "parity: action validates commands"
assert_contains "$(cat "$ACTION_ANALYZE_SH")" "security-gate must be 'new' or 'newly-reachable'" \
  "parity: action validates security gate values"
assert_contains "$(cat "$ACTION_ANALYZE_SH")" 'INPUT_SECURITY_GATE' \
  "parity: action wires security gate input"
assert_contains "$(cat "$CI_TEMPLATE_YAML")" 'FALLOW_COMMAND" in' \
  "parity: gitlab validates commands"
assert_contains "$(cat "$CI_TEMPLATE_YAML")" "FALLOW_SECURITY_GATE must be 'new' or 'newly-reachable'" \
  "parity: gitlab validates security gate values"
assert_contains "$(cat "$CI_TEMPLATE_YAML")" 'FALLOW_SECURITY_GATE' \
  "parity: gitlab wires security gate variable"
assert_contains "$(cat "$ACTION_ANALYZE_SH")" '.gate.new_count' \
  "parity: action counts security gate new_count"
assert_contains "$(cat "$CI_TEMPLATE_YAML")" '.gate.new_count' \
  "parity: gitlab counts security gate new_count"

# =========================================================================
# GitLab-specific summary jq tests
# =========================================================================

echo ""
echo "=== GitLab Summary scripts ==="

echo "  summary-check.jq (GitLab):"
OUT=$(jq -r -f "$CI_JQ_DIR/summary-check.jq" "$FIXTURES/check.json" 2>&1)
assert_valid_markdown "$OUT" "produces output"
assert_contains "$OUT" "Fallow Analysis" "has title"
assert_contains "$OUT" "issues" "mentions issues"
assert_contains "$OUT" "Unused" "lists unused categories"
assert_contains "$OUT" "Imported elsewhere" "shows dependency workspace context column"
assert_contains "$OUT" 'packages/client' "shows dependency workspace context value"
assert_contains "$OUT" "Empty catalog groups" "shows empty catalog group row"
assert_contains "$OUT" 'legacy' "shows empty catalog group name"
assert_not_contains "$OUT" '!\[NOTE\]' "no GitHub callout NOTE"
assert_not_contains "$OUT" '!\[WARNING\]' "no GitHub callout WARNING"
assert_not_contains "$OUT" '!\[TIP\]' "no GitHub callout TIP"

OUT_POLICY=$(jq '.policy_violations = [{"path": "src/app.ts", "line": 7, "col": 2, "pack": "team-policy", "rule_id": "no-moment", "kind": "banned-import", "matched": "moment", "severity": "error", "actions": []}] | .total_issues = (.total_issues + 1)' "$FIXTURES/check.json" | jq -r -f "$CI_JQ_DIR/summary-check.jq" 2>&1)
assert_contains "$OUT_POLICY" "Policy violations" "policy: shows summary row and section"
assert_contains "$OUT_POLICY" "team-policy/no-moment" "policy: shows pack/rule identity"

OUT_ICE=$(jq '.invalid_client_exports = [{"path": "src/app.ts", "line": 5, "col": 0, "export_name": "metadata", "directive": "use client", "actions": []}] | .total_issues = (.total_issues + 1)' "$FIXTURES/check.json" | jq -r -f "$CI_JQ_DIR/summary-check.jq" 2>&1)
assert_contains "$OUT_ICE" "Invalid client exports" "ice: shows summary row and section"
assert_contains "$OUT_ICE" "metadata" "ice: shows export name in section"

OUT_MCSB=$(jq '.mixed_client_server_barrels = [{"path": "src/index.ts", "line": 2, "col": 0, "client_origin": "./Button", "server_origin": "./fetchUser", "actions": []}] | .total_issues = (.total_issues + 1)' "$FIXTURES/check.json" | jq -r -f "$CI_JQ_DIR/summary-check.jq" 2>&1)
assert_contains "$OUT_MCSB" "Mixed client/server barrels" "mcsb: shows summary row and section"
assert_contains "$OUT_MCSB" "./fetchUser" "mcsb: shows server origin in section"

OUT_MD=$(jq '.misplaced_directives = [{"path": "src/widget.tsx", "line": 4, "col": 0, "directive": "use client", "actions": []}] | .total_issues = (.total_issues + 1)' "$FIXTURES/check.json" | jq -r -f "$CI_JQ_DIR/summary-check.jq" 2>&1)
assert_contains "$OUT_MD" "Misplaced directives" "md: shows summary row and section"
assert_contains "$OUT_MD" "use client" "md: shows directive in section"

# Directive column renders with the surrounding quotes from the `\"\(.directive)\"` template.
# Asserting the export-cell + directive-cell pair so a quote-escaping regression is caught
# (the bare "use client" string also appears in the section header text).
assert_contains "$OUT_ICE" '`metadata` | `"use client"` |' "ice: directive column renders with surrounding quotes"
# `"use server"` directive path (the section description mentions both, so a use-server-only
# fixture proves the row template, not just the header text).
OUT_MD_SERVER=$(jq '.misplaced_directives = [{"path": "src/action.ts", "line": 3, "col": 0, "directive": "use server", "actions": []}] | .total_issues = (.total_issues + 1)' "$FIXTURES/check.json" | jq -r -f "$CI_JQ_DIR/summary-check.jq" 2>&1)
assert_contains "$OUT_MD_SERVER" '`"use server"` |' "md: use-server directive renders in section row"

# Vue/Next framework IssueKinds: summary row + section render in the GitLab variant.
OUT_USA=$(jq '.unused_server_actions = [{"path": "src/actions.ts", "line": 9, "col": 0, "action_name": "submitForm", "actions": []}] | .total_issues = (.total_issues + 1)' "$FIXTURES/check.json" | jq -r -f "$CI_JQ_DIR/summary-check.jq" 2>&1)
assert_contains "$OUT_USA" "Unused server actions" "usa: shows summary row and section"
assert_contains "$OUT_USA" "submitForm" "usa: shows action name in section"

OUT_URC=$(jq '.unrendered_components = [{"path": "src/Foo.vue", "line": 1, "col": 0, "component_name": "Foo", "framework": "vue", "actions": []}] | .total_issues = (.total_issues + 1)' "$FIXTURES/check.json" | jq -r -f "$CI_JQ_DIR/summary-check.jq" 2>&1)
assert_contains "$OUT_URC" "Unrendered components" "urc: shows summary row and section"
assert_contains "$OUT_URC" "Foo" "urc: shows component name in section"

OUT_UCP=$(jq '.unused_component_props = [{"path": "src/Widget.vue", "line": 12, "col": 0, "component_name": "Widget", "prop_name": "variant", "actions": []}] | .total_issues = (.total_issues + 1)' "$FIXTURES/check.json" | jq -r -f "$CI_JQ_DIR/summary-check.jq" 2>&1)
assert_contains "$OUT_UCP" "Unused component props" "ucp: shows summary row and section"
assert_contains "$OUT_UCP" "variant" "ucp: shows prop name in section"

OUT_UCI=$(jq '.unused_component_inputs = [{"path": "src/widget.component.ts", "line": 12, "col": 0, "component_name": "Widget", "input_name": "variant", "actions": []}] | .total_issues = (.total_issues + 1)' "$FIXTURES/check.json" | jq -r -f "$CI_JQ_DIR/summary-check.jq" 2>&1)
assert_contains "$OUT_UCI" "Unused component inputs" "uci: shows summary row and section"
assert_contains "$OUT_UCI" "variant" "uci: shows input name in section"

OUT_UCE=$(jq '.unused_component_emits = [{"path": "src/Widget.vue", "line": 14, "col": 0, "component_name": "Widget", "emit_name": "submit", "actions": []}] | .total_issues = (.total_issues + 1)' "$FIXTURES/check.json" | jq -r -f "$CI_JQ_DIR/summary-check.jq" 2>&1)
assert_contains "$OUT_UCE" "Unused component emits" "uce: shows summary row and section"
assert_contains "$OUT_UCE" "submit" "uce: shows emit name in section"

OUT_UCO=$(jq '.unused_component_outputs = [{"path": "src/widget.component.ts", "line": 14, "col": 0, "component_name": "Widget", "output_name": "submit", "actions": []}] | .total_issues = (.total_issues + 1)' "$FIXTURES/check.json" | jq -r -f "$CI_JQ_DIR/summary-check.jq" 2>&1)
assert_contains "$OUT_UCO" "Unused component outputs" "uco: shows summary row and section"
assert_contains "$OUT_UCO" "submit" "uco: shows output name in section"

OUT_USE=$(jq '.unused_svelte_events = [{"path": "src/Child.svelte", "line": 6, "col": 0, "component_name": "Child", "event_name": "dead", "actions": []}] | .total_issues = (.total_issues + 1)' "$FIXTURES/check.json" | jq -r -f "$CI_JQ_DIR/summary-check.jq" 2>&1)
assert_contains "$OUT_USE" "Unused Svelte events" "use: shows summary row and section"
assert_contains "$OUT_USE" "dead" "use: shows event name in section"

OUT_UPI=$(jq '.unprovided_injects = [{"path": "src/useTheme.ts", "line": 7, "col": 0, "key_name": "themeKey", "framework": "vue", "actions": []}] | .total_issues = (.total_issues + 1)' "$FIXTURES/check.json" | jq -r -f "$CI_JQ_DIR/summary-check.jq" 2>&1)
assert_contains "$OUT_UPI" "Unprovided injects" "upi: shows summary row and section"
assert_contains "$OUT_UPI" "themeKey" "upi: shows inject key in section"

# Missing keys must never crash jq (defensive `// []` / null-safe helpers).
OUT_NO_FRAMEWORK_KEYS=$(jq 'del(.unused_server_actions, .unrendered_components, .unused_component_props, .unused_component_inputs, .unused_component_emits, .unused_component_outputs, .unused_svelte_events, .unprovided_injects, .route_collisions, .dynamic_segment_name_conflicts, .invalid_client_exports, .mixed_client_server_barrels, .misplaced_directives)' "$FIXTURES/check.json" | jq -r -f "$CI_JQ_DIR/summary-check.jq" 2>&1)
assert_contains "$OUT_NO_FRAMEWORK_KEYS" "Fallow Analysis" "missing-keys: GitLab summary-check survives absent framework keys"

OUT_CLEAN=$(jq -r -f "$CI_JQ_DIR/summary-check.jq" "$FIXTURES/check-clean.json" 2>&1)
assert_contains "$OUT_CLEAN" "No issues found" "clean: shows no issues"

# Issue #449: kind_known: false renders "unknown kind \`token\`" in the table.
OUT_UNKNOWN_KIND_SUMMARY=$(jq '.unused_files = [] | .unused_exports = [] | .unused_types = [] | .unused_dependencies = [] | .unused_dev_dependencies = [] | .unused_optional_dependencies = [] | .unused_enum_members = [] | .unused_class_members = [] | .unresolved_imports = [] | .unlisted_dependencies = [] | .duplicate_exports = [] | .circular_dependencies = [] | .boundary_violations = [] | .type_only_dependencies = [] | .test_only_dependencies = [] | .unused_catalog_entries = [] | .empty_catalog_groups = [] | .unresolved_catalog_references = [] | .unused_dependency_overrides = [] | .misconfigured_dependency_overrides = [] | .private_type_leaks = [] | .stale_suppressions = [{"path": "src/utils.ts", "line": 1, "col": 0, "origin": {"type": "comment", "issue_kind": "complexity-typo", "is_file_level": false, "kind_known": false}}] | .total_issues = 1' "$FIXTURES/check.json" | jq -r -f "$CI_JQ_DIR/summary-check.jq" 2>&1)
assert_contains "$OUT_UNKNOWN_KIND_SUMMARY" 'unknown kind' "GitLab summary unknown kind: prefix renders"
assert_contains "$OUT_UNKNOWN_KIND_SUMMARY" 'complexity-typo' "GitLab summary unknown kind: verbatim token renders"

echo "  summary-health.jq (GitLab):"
OUT=$(jq -r -f "$CI_JQ_DIR/summary-health.jq" "$FIXTURES/health.json" 2>&1)
assert_valid_markdown "$OUT" "produces output"
assert_not_contains "$OUT" '!\[NOTE\]' "no GitHub callout NOTE"
assert_not_contains "$OUT" '!\[WARNING\]' "no GitHub callout WARNING"

OUT_CLEAN=$(jq -r -f "$CI_JQ_DIR/summary-health.jq" "$FIXTURES/health-clean.json" 2>&1)
assert_contains "$OUT_CLEAN" "No functions exceed" "clean: no functions exceed"

echo "  summary-health.jq (delta header with trend, GitLab):"
assert_contains "$OUT" "Health: B (72.3)" "delta: shows grade and score"
assert_contains "$OUT" "+7.2 pts vs previous" "delta: shows score delta"
assert_contains "$OUT" "C 65.1" "delta: shows previous grade and score"
assert_contains "$OUT" "dead exports 41.2%" "delta: shows dead export pct"
assert_contains "$OUT" "(-3.8%)" "delta: shows dead export delta"
assert_contains "$OUT" "avg complexity 7.1 (-1.2)" "delta: shows complexity delta"
assert_contains "$OUT" "chart_with_upwards_trend" "delta: uses GitLab emoji (no GitHub callout)"

echo "  summary-health.jq (delta header without trend, GitLab):"
assert_contains "$OUT_CLEAN" "Health: A (92.5)" "no-trend: shows absolute score"
assert_not_contains "$OUT_CLEAN" "vs previous" "no-trend: no delta line"
assert_contains "$OUT_CLEAN" "FALLOW_SAVE_SNAPSHOT" "no-trend: shows save-snapshot hint"

echo "  summary-health.jq (no delta header without score, GitLab):"
OUT_NO_SCORE=$(jq 'del(.health_score) | del(.health_trend)' "$FIXTURES/health.json" | jq -r -f "$CI_JQ_DIR/summary-health.jq" 2>&1)
assert_not_contains "$OUT_NO_SCORE" "Health:" "no-score: no delta header"

echo "  summary-health.jq (runtime coverage findings and hot paths, GitLab):"
OUT_PROD=$(jq '.runtime_coverage = {"verdict":"cold-code-detected","summary":{"functions_tracked":4,"functions_hit":2,"functions_unhit":1,"functions_untracked":1,"coverage_percent":50,"trace_count":1200,"period_days":7,"deployments_seen":2},"findings":[{"path":"src/cold.ts","function":"coldPath","line":14,"verdict":"review_required","invocations":0,"confidence":"medium"},{"path":"src/lazy.ts","function":"lateBound","line":8,"verdict":"coverage_unavailable","confidence":"none"}],"hot_paths":[{"path":"src/hot.ts","function":"hotPath","line":3,"invocations":250,"percentile":99}]}' "$FIXTURES/health-clean.json" | jq -r -f "$CI_JQ_DIR/summary-health.jq" 2>&1)
assert_contains "$OUT_PROD" "Runtime Coverage" "prod: has runtime coverage section"
assert_contains "$OUT_PROD" "hotPath" "prod: shows hot path function"

echo "  summary-audit.jq (GitLab):"
OUT_AUDIT=$(jq -n --slurpfile h "$FIXTURES/health.json" --slurpfile c "$FIXTURES/check.json" --slurpfile d "$FIXTURES/dupes.json" '{
  schema_version: 3,
  command: "audit",
  verdict: "fail",
  changed_files_count: 2,
  elapsed_ms: 42,
  summary: {dead_code_issues: 1, complexity_findings: 3, duplication_clone_groups: 1},
  attribution: {gate: "new-only", dead_code_introduced: 1, dead_code_inherited: 0, complexity_introduced: 2, complexity_inherited: 1, duplication_introduced: 0, duplication_inherited: 1, styling_introduced: 1, styling_inherited: 1, duplication_demoted: 1},
  dead_code: ($c[0] | .unused_exports |= map(. + {introduced: true}) | .unused_dependencies |= map(. + {introduced: false})),
  complexity: ($h[0]
    | .findings |= [.[0] + {coverage_tier: "partial"}, .[1] + {coverage_tier: "high"}, .[2]]
    | .summary.coverage_model = "istanbul"
    | .summary.istanbul_matched = 8
    | .summary.istanbul_total = 10
    | .styling_findings = [
        {code: "css-selector-complexity", sub_kind: "high-specificity", path: "src/styles.css", line: 4, value: "#app .card .title", effective_severity: "error", introduced: true},
        {code: "css-important", sub_kind: "important", path: "src/legacy.css", line: 9, value: "!important", effective_severity: "warn", introduced: false}
      ]),
  duplication: ($d[0] | .clone_groups |= map(. + {introduced: false, demotion_reason: "no-added-lines"}))
}' | jq -r -f "$CI_JQ_DIR/summary-audit.jq" 2>&1)
assert_valid_markdown "$OUT_AUDIT" "produces audit output"
assert_contains "$OUT_AUDIT" "Fallow Audit" "audit: has title"
assert_contains "$OUT_AUDIT" "Audit failed" "audit: shows failed verdict"
assert_contains "$OUT_AUDIT" "Dead Code" "audit: has dead-code details"
assert_contains "$OUT_AUDIT" "fetchFromApi" "audit: lists dead-code findings"
assert_contains "$OUT_AUDIT" "parseContentBlocks" "audit: lists complexity findings"
assert_contains "$OUT_AUDIT" "Duplication" "audit: has duplication details"
assert_contains "$OUT_AUDIT" "24 lines / 125 tokens" "audit: lists clone group size"
assert_contains "$OUT_AUDIT" "Inherited" "audit: has inherited column"
assert_contains "$OUT_AUDIT" "Coverage |" "audit: has coverage column header"
assert_contains "$OUT_AUDIT" "| partial |" "audit: shows coverage tier value"
assert_contains "$OUT_AUDIT" "| high |" "audit: shows alt coverage tier"
assert_contains "$OUT_AUDIT" "| - |" "audit: missing coverage_tier renders as dash"
assert_contains "$OUT_AUDIT" "Coverage model: istanbul" "audit: shows istanbul coverage model footer"
assert_contains "$OUT_AUDIT" "Matched 8/10" "audit: shows istanbul match rate"
assert_contains "$OUT_AUDIT" "### Styling" "audit: has styling details"
assert_contains "$OUT_AUDIT" "css-selector-complexity" "audit: lists styling rule"
assert_contains "$OUT_AUDIT" "src/styles.css:4" "audit: lists styling location"
assert_contains "$OUT_AUDIT" "1 introduced clone group demoted to inherited" "audit: shows demotion footnote"
assert_not_contains "$OUT_AUDIT" '!\[WARNING\]' "audit: no GitHub callout warning"

OUT_AUDIT_STYLE_NEW=$(jq -n '{
  command: "audit", verdict: "fail", changed_files_count: 1, elapsed_ms: 4,
  summary: {dead_code_issues: 0, complexity_findings: 0, duplication_clone_groups: 0},
  attribution: {gate: "new-only", styling_introduced: 1, styling_inherited: 0},
  complexity: {styling_findings: [{code: "css-important", path: "src/styles.css", line: 2, value: "!important", effective_severity: "error", introduced: true}]}
}' | jq -r -f "$CI_JQ_DIR/summary-audit.jq" 2>&1)
assert_contains "$OUT_AUDIT_STYLE_NEW" "| Styling | 1 | 1 | 0 |" "audit: styling-only new-only totals are visible"
assert_contains "$OUT_AUDIT_STYLE_NEW" "| new |" "audit: styling-only new-only status is visible"
assert_not_contains "$OUT_AUDIT_STYLE_NEW" "demoted to inherited" "audit: no demotion footnote without demotions"

OUT_AUDIT_STYLE_ALL=$(jq -n '{
  command: "audit", verdict: "fail", changed_files_count: 1, elapsed_ms: 4,
  summary: {dead_code_issues: 0, complexity_findings: 0, duplication_clone_groups: 0},
  attribution: {gate: "all"},
  complexity: {styling_findings: [{code: "css-important", path: null, line: null, value: "!important", effective_severity: "error"}]}
}' | jq -r -f "$CI_JQ_DIR/summary-audit.jq" 2>&1)
assert_contains "$OUT_AUDIT_STYLE_ALL" "| Styling | 1 | 0 | 0 |" "audit: styling-only all totals are visible"
assert_contains "$OUT_AUDIT_STYLE_ALL" '| - | `css-important`' "audit: null styling path uses a safe placeholder"
assert_contains "$OUT_AUDIT_STYLE_ALL" "Audit gate: all" "audit: styling-only all gate is visible"

# Low match-rate variant: footer should warn about --coverage-root
OUT_AUDIT_LOWMATCH=$(jq -n --slurpfile h "$FIXTURES/health.json" '{
  schema_version: 3, command: "audit", verdict: "fail", changed_files_count: 2, elapsed_ms: 42,
  summary: {dead_code_issues: 0, complexity_findings: 3, duplication_clone_groups: 0},
  attribution: {gate: "new-only", dead_code_introduced: 0, dead_code_inherited: 0, complexity_introduced: 3, complexity_inherited: 0, duplication_introduced: 0, duplication_inherited: 0},
  complexity: ($h[0] | .summary.coverage_model = "istanbul" | .summary.istanbul_matched = 1 | .summary.istanbul_total = 10)
}' | jq -r -f "$CI_JQ_DIR/summary-audit.jq" 2>&1)
assert_contains "$OUT_AUDIT_LOWMATCH" "Low match rate" "audit: low match rate flags --coverage-root"

# Static-estimate variant: footer should suggest --coverage
OUT_AUDIT_STATIC=$(jq -n --slurpfile h "$FIXTURES/health.json" --slurpfile c "$FIXTURES/check.json" --slurpfile d "$FIXTURES/dupes.json" '{
  schema_version: 3, command: "audit", verdict: "fail", changed_files_count: 2, elapsed_ms: 42,
  summary: {dead_code_issues: 0, complexity_findings: 3, duplication_clone_groups: 0},
  attribution: {gate: "new-only", dead_code_introduced: 0, dead_code_inherited: 0, complexity_introduced: 3, complexity_inherited: 0, duplication_introduced: 0, duplication_inherited: 0},
  complexity: ($h[0] | .summary.coverage_model = "static_estimated")
}' | jq -r -f "$CI_JQ_DIR/summary-audit.jq" 2>&1)
assert_contains "$OUT_AUDIT_STATIC" "Coverage model: static (estimated)" "audit: static-estimate footer suggests --coverage"
assert_contains "$OUT_AUDIT_STATIC" "for measured coverage" "audit: static branch reworded"

# Absent-model variant: footer should not be present at all
OUT_AUDIT_NOMODEL=$(jq -n --slurpfile h "$FIXTURES/health.json" '{
  schema_version: 3, command: "audit", verdict: "fail", changed_files_count: 2, elapsed_ms: 42,
  summary: {dead_code_issues: 0, complexity_findings: 3, duplication_clone_groups: 0},
  attribution: {gate: "new-only", dead_code_introduced: 0, dead_code_inherited: 0, complexity_introduced: 3, complexity_inherited: 0, duplication_introduced: 0, duplication_inherited: 0},
  complexity: ($h[0] | del(.summary.coverage_model))
}' | jq -r -f "$CI_JQ_DIR/summary-audit.jq" 2>&1)
assert_not_contains "$OUT_AUDIT_NOMODEL" "Coverage model:" "audit: absent coverage_model omits footer"

echo "  summary-combined.jq (GitLab):"
OUT=$(jq -r -f "$CI_JQ_DIR/summary-combined.jq" "$FIXTURES/combined.json" 2>&1)
assert_valid_markdown "$OUT" "produces output"
assert_contains "$OUT" "Fallow" "has title"
assert_contains "$OUT" "code issues" "mentions code issues"
assert_contains "$OUT" "Maintainability" "shows vital signs"
assert_not_contains "$OUT" '!\[NOTE\]' "no GitHub callout NOTE"
assert_not_contains "$OUT" '!\[TIP\]' "no GitHub callout TIP"

assert_contains "$OUT" "Codebase health" "has codebase health header"
assert_contains "$OUT" "CRAP" "combined: shows CRAP column"
assert_contains "$OUT" "thresholds: cyclomatic" "combined: shows complexity threshold line"
assert_not_contains "$OUT" "Dead exports" "no dead_export_pct in PR comment"

# Duplication block: locations table replaces metric-only table
assert_contains "$OUT" "Locations | Lines | Tokens" "dupes: locations table header"
assert_contains "$OUT" "content-parser.ts:27-50" "dupes: shows first clone instance line range"
assert_contains "$OUT" "Across 2 files" "dupes: footer reports file count"
assert_contains "$OUT" "2 groups · 66 lines" "dupes: header carries group count and total lines"
assert_not_contains "$OUT" "| [Duplicated lines]" "dupes: old metric table is gone"

OUT_EMPTY_DUPES_GL=$(jq '.dupes.clone_groups = [] | .dupes.clone_families = [] | .dupes.stats.clone_groups = 2 | .dupes.stats.clone_instances = 5 | .dupes.stats.files_with_clones = 4 | .dupes.stats.duplicated_lines = 59 | .dupes.stats.duplication_percentage = 0.16' "$FIXTURES/combined-clean.json" | jq -r -f "$CI_JQ_DIR/summary-combined.jq" 2>&1)
assert_contains "$OUT_EMPTY_DUPES_GL" "Quality gate passed" "combined: empty dupes groups keep clean GitLab summary"
assert_contains "$OUT_EMPTY_DUPES_GL" "No duplication" "combined: empty dupes groups render no GitLab duplication"
assert_not_contains "$OUT_EMPTY_DUPES_GL" "2 groups" "combined: nonzero dupes stats do not render GitLab actionable groups"

# Linkified cells engage when CI_PROJECT_URL + CI_COMMIT_SHA are set; GitLab fragment is #L<start>-<end> (single L)
OUT_LINKED_GL=$(CI_PROJECT_URL="https://gitlab.com/foo/bar" CI_COMMIT_SHA="deadbeef" jq -r -f "$CI_JQ_DIR/summary-combined.jq" "$FIXTURES/combined.json" 2>&1)
assert_contains "$OUT_LINKED_GL" "https://gitlab.com/foo/bar/-/blob/deadbeef/src/helpers/content-parser.ts#L27-50" "dupes: file_link engages with GitLab env vars"

# Deep paths (>3 segments): display is rel_path-truncated but URL keeps the full path
OUT_DEEP_GL=$(jq '.dupes.clone_groups = [{line_count: 10, token_count: 50, instances: [{file: "apps/web/src/services/billing/calculator.ts", start_line: 5, end_line: 15}, {file: "apps/api/src/services/billing/calculator.ts", start_line: 8, end_line: 18}]}] | .dupes.stats.clone_groups = 1 | .dupes.stats.files_with_clones = 2' "$FIXTURES/combined.json" | CI_PROJECT_URL="https://gitlab.com/foo/bar" CI_COMMIT_SHA="deadbeef" jq -r -f "$CI_JQ_DIR/summary-combined.jq" 2>&1)
assert_contains "$OUT_DEEP_GL" "\`services/billing/calculator.ts:5-15\`" "deep-path: display uses rel_path"
assert_contains "$OUT_DEEP_GL" "/-/blob/deadbeef/apps/web/src/services/billing/calculator.ts#L5-15" "deep-path: URL keeps full path"
assert_contains "$OUT_DEEP_GL" "/-/blob/deadbeef/apps/api/src/services/billing/calculator.ts#L8-18" "deep-path: URL keeps full path (sibling)"

# Singular-group header
OUT_ONE_GL=$(jq '.dupes.stats.clone_groups = 1 | .dupes.clone_groups = [.dupes.clone_groups[0]]' "$FIXTURES/combined.json" | jq -r -f "$CI_JQ_DIR/summary-combined.jq" 2>&1)
assert_contains "$OUT_ONE_GL" "(1 group ·" "dupes: singular group header"
assert_not_contains "$OUT_ONE_GL" "(1 groups ·" "dupes: no '1 groups' grammar"

# Status-bar pluralization: 1 of each renders singular
OUT_SINGULAR_GL=$(jq '.check.unused_files = [.check.unused_files[0]] | .check.unused_exports = [] | .check.unused_dependencies = [] | .check.unused_dev_dependencies = [] | .check.unused_optional_dependencies = [] | .check.unused_types = [] | .check.unused_enum_members = [] | .check.unused_class_members = [] | .check.unresolved_imports = [] | .check.unlisted_dependencies = [] | .check.duplicate_exports = [] | .check.circular_dependencies = [] | .check.boundary_violations = [] | .check.type_only_dependencies = [] | .check.test_only_dependencies = [] | .check.stale_suppressions = [] | .check.unused_catalog_entries = [] | .check.unresolved_catalog_references = [] | .check.unused_dependency_overrides = [] | .check.misconfigured_dependency_overrides = [] | .check.private_type_leaks = [] | .check.total_issues = 1 | .dupes.stats.clone_groups = 1 | .dupes.clone_groups = [.dupes.clone_groups[0]] | .health.summary.functions_above_threshold = 1 | .health.findings = [.health.findings[0]]' "$FIXTURES/combined.json" | jq -r -f "$CI_JQ_DIR/summary-combined.jq" 2>&1)
assert_contains "$OUT_SINGULAR_GL" "**1** code issue " "status-bar: singular code issue"
assert_not_contains "$OUT_SINGULAR_GL" "**1** code issues" "status-bar: no '1 code issues' grammar"
assert_contains "$OUT_SINGULAR_GL" "**1** clone group " "status-bar: singular clone group"
assert_not_contains "$OUT_SINGULAR_GL" "**1** clone groups" "status-bar: no '1 clone groups' grammar"
assert_not_contains "$OUT_SINGULAR_GL" "**1** health findings" "status-bar: no '1 health findings' grammar"

# Complexity <details> summary pluralizes when functions_above_threshold == 1
assert_contains "$OUT_SINGULAR_GL" "(1 function above threshold)" "complexity dropdown: singular function"
assert_not_contains "$OUT_SINGULAR_GL" "(1 functions above threshold)" "complexity dropdown: no '1 functions' grammar"

# RSC findings appear in the combined-mode Code issues breakdown table (not just
# summary-check.jq standalone). All three RSC types injected into .check at once.
OUT_RSC_GL=$(jq '.check.invalid_client_exports = [{"path": "src/app.tsx", "line": 5, "col": 0, "export_name": "metadata", "directive": "use client", "actions": []}] | .check.mixed_client_server_barrels = [{"path": "src/index.ts", "line": 2, "col": 0, "client_origin": "./Button", "server_origin": "./fetchUser", "actions": []}] | .check.misplaced_directives = [{"path": "src/widget.tsx", "line": 4, "col": 0, "directive": "use server", "actions": []}] | .check.total_issues = (.check.total_issues + 3)' "$FIXTURES/combined.json" | jq -r -f "$CI_JQ_DIR/summary-combined.jq" 2>&1)
assert_contains "$OUT_RSC_GL" "| [Invalid client exports](" "combined: RSC invalid-client-exports row in breakdown"
assert_contains "$OUT_RSC_GL" "| [Mixed client/server barrels](" "combined: RSC mixed-barrel row in breakdown"
assert_contains "$OUT_RSC_GL" "| [Misplaced directives](" "combined: RSC misplaced-directives row in breakdown"

# Next.js routing keys (route_collisions + dynamic_segment_name_conflicts) were previously
# absent from the GitLab combined-mode Code issues breakdown; assert they now render.
OUT_ROUTING_GL=$(jq '.check.route_collisions = [{"path": "src/app/(a)/p/page.tsx", "url": "/p", "conflicting_paths": ["src/app/(b)/p/page.tsx"], "actions": []}] | .check.dynamic_segment_name_conflicts = [{"path": "src/app/[id]/page.tsx", "position": "0", "conflicting_segments": ["id", "slug"], "actions": []}] | .check.total_issues = (.check.total_issues + 2)' "$FIXTURES/combined.json" | jq -r -f "$CI_JQ_DIR/summary-combined.jq" 2>&1)
assert_contains "$OUT_ROUTING_GL" "| [Route collisions](" "combined: route-collisions row in breakdown"
assert_contains "$OUT_ROUTING_GL" "| [Dynamic segment conflicts](" "combined: dynamic-segment-conflicts row in breakdown"

# Vue/Next framework keys appear in the GitLab combined-mode Code issues breakdown table.
OUT_FRAMEWORK_GL=$(jq '.check.unused_server_actions = [{"path": "src/actions.ts", "line": 9, "col": 0, "action_name": "submitForm", "actions": []}] | .check.unrendered_components = [{"path": "src/Foo.vue", "line": 1, "col": 0, "component_name": "Foo", "framework": "vue", "actions": []}] | .check.unused_component_props = [{"path": "src/Widget.vue", "line": 12, "col": 0, "component_name": "Widget", "prop_name": "variant", "actions": []}] | .check.unused_component_inputs = [{"path": "src/widget.component.ts", "line": 12, "col": 0, "component_name": "Widget", "input_name": "variant", "actions": []}] | .check.unused_component_emits = [{"path": "src/Widget.vue", "line": 14, "col": 0, "component_name": "Widget", "emit_name": "submit", "actions": []}] | .check.unused_component_outputs = [{"path": "src/widget.component.ts", "line": 14, "col": 0, "component_name": "Widget", "output_name": "submit", "actions": []}] | .check.unused_svelte_events = [{"path": "src/Child.svelte", "line": 6, "col": 0, "component_name": "Child", "event_name": "dead", "actions": []}] | .check.unprovided_injects = [{"path": "src/useTheme.ts", "line": 7, "col": 0, "key_name": "themeKey", "framework": "vue", "actions": []}] | .check.total_issues = (.check.total_issues + 8)' "$FIXTURES/combined.json" | jq -r -f "$CI_JQ_DIR/summary-combined.jq" 2>&1)
assert_contains "$OUT_FRAMEWORK_GL" "| [Unused server actions](" "combined: unused-server-actions row in breakdown"
assert_contains "$OUT_FRAMEWORK_GL" "| [Unrendered components](" "combined: unrendered-components row in breakdown"
assert_contains "$OUT_FRAMEWORK_GL" "| [Unused component props](" "combined: unused-component-props row in breakdown"
assert_contains "$OUT_FRAMEWORK_GL" "| [Unused component inputs](" "combined: unused-component-inputs row in breakdown"
assert_contains "$OUT_FRAMEWORK_GL" "| [Unused component emits](" "combined: unused-component-emits row in breakdown"
assert_contains "$OUT_FRAMEWORK_GL" "| [Unused component outputs](" "combined: unused-component-outputs row in breakdown"
assert_contains "$OUT_FRAMEWORK_GL" "| [Unused Svelte events](" "combined: unused-svelte-events row in breakdown"
assert_contains "$OUT_FRAMEWORK_GL" "| [Unprovided injects](" "combined: unprovided-injects row in breakdown"

# Worst-case truncation: 50 groups (paths differentiated per-group via `. as $g |`),
# top-5 + overflow line, output stays under 65k chars.
# line_count is ASCENDING in input order so the sort_by in summary-combined.jq must do work.
OUT_LARGE_GL=$(jq -n '
  {
    schema_version: 3,
    check: {total_issues: 0, unused_files: [], unused_exports: [], unused_types: [], unused_dependencies: [], unused_dev_dependencies: [], unused_optional_dependencies: [], unused_enum_members: [], unused_class_members: [], unresolved_imports: [], unlisted_dependencies: [], duplicate_exports: [], circular_dependencies: [], boundary_violations: [], type_only_dependencies: [], test_only_dependencies: [], stale_suppressions: [], unused_catalog_entries: [], unresolved_catalog_references: [], unused_dependency_overrides: [], misconfigured_dependency_overrides: [], private_type_leaks: []},
    dupes: {
      stats: {clone_groups: 50, clone_instances: 200, files_with_clones: 50, duplicated_lines: 5000, total_lines: 100000, duplication_percentage: 5.0},
      clone_groups: ([range(0;50)] | map(. as $g | {line_count: ($g + 1), token_count: ($g * 5 + 50), instances: ([range(0;4)] | map(. as $i | {file: ("src/group_\($g)/file_\($i).ts"), start_line: ($i * 10 + 1), end_line: ($i * 10 + 9)}))}))
    },
    health: {summary: {functions_above_threshold: 0}, vital_signs: {}, file_scores: [], findings: []}
  }
' | jq -r -f "$CI_JQ_DIR/summary-combined.jq" 2>&1)
assert_contains "$OUT_LARGE_GL" "and 45 more groups" "dupes: large input truncates with overflow line"
assert_contains "$OUT_LARGE_GL" "Across 50 files" "dupes: large input footer count is correct"
LARGE_LEN_GL=${#OUT_LARGE_GL}
if [ "$LARGE_LEN_GL" -lt 65000 ]; then
  pass "dupes: large input stays under PR comment cap (got $LARGE_LEN_GL chars)"
else
  fail "dupes: large input over PR comment cap" "got $LARGE_LEN_GL chars (cap 65000)"
fi
assert_contains "$OUT_LARGE_GL" "src/group_49/file_0.ts:1-9" "dupes: largest group (49) ranks first after sort"
assert_contains "$OUT_LARGE_GL" "src/group_45/file_0.ts" "dupes: top-5 contains group_45 (5th largest)"
assert_not_contains "$OUT_LARGE_GL" "src/group_44/file_0.ts" "dupes: group_44 (6th largest) is truncated"
assert_not_contains "$OUT_LARGE_GL" "src/group_0/file_0.ts" "dupes: smallest group is truncated"

# Null duplication_percentage must not crash pct(); render as 0%
OUT_NULL_PCT_GL=$(jq 'del(.dupes.stats.duplication_percentage)' "$FIXTURES/combined.json" | jq -r -f "$CI_JQ_DIR/summary-combined.jq" 2>&1)
assert_contains "$OUT_NULL_PCT_GL" "66 lines · 0%" "dupes: missing duplication_percentage renders as 0%"
assert_not_contains "$OUT_NULL_PCT_GL" "cannot be multiplied" "dupes: pct(null) does not crash"

OUT_CRAP_ONLY=$(jq '.health.summary.functions_above_threshold = 1 | .health.findings = [{"path":"src/ui/pagination.tsx","name":"buildPageItems","line":42,"col":0,"cyclomatic":17,"cognitive":8,"crap":30,"line_count":13,"severity":"moderate","exceeded":"crap"}]' "$FIXTURES/combined.json" | jq -r -f "$CI_JQ_DIR/summary-combined.jq" 2>&1)
assert_contains "$OUT_CRAP_ONLY" "buildPageItems" "combined: renders CRAP-only finding"
assert_contains "$OUT_CRAP_ONLY" "CRAP >= 30" "combined: explains CRAP threshold"

OUT_CRAP_SORT=$(jq '.health.summary.functions_above_threshold = 6 | .health.findings = [
  {"path":"src/a.ts","name":"cyclo1","line":1,"col":0,"cyclomatic":80,"cognitive":4,"line_count":10,"severity":"critical","exceeded":"cyclomatic"},
  {"path":"src/a.ts","name":"cyclo2","line":2,"col":0,"cyclomatic":70,"cognitive":4,"line_count":10,"severity":"critical","exceeded":"cyclomatic"},
  {"path":"src/a.ts","name":"cyclo3","line":3,"col":0,"cyclomatic":60,"cognitive":4,"line_count":10,"severity":"critical","exceeded":"cyclomatic"},
  {"path":"src/a.ts","name":"cyclo4","line":4,"col":0,"cyclomatic":50,"cognitive":4,"line_count":10,"severity":"critical","exceeded":"cyclomatic"},
  {"path":"src/a.ts","name":"cyclo5","line":5,"col":0,"cyclomatic":40,"cognitive":4,"line_count":10,"severity":"high","exceeded":"cyclomatic"},
  {"path":"src/a.ts","name":"crapOnly","line":6,"col":0,"cyclomatic":8,"cognitive":4,"crap":30,"line_count":10,"severity":"moderate","exceeded":"crap"}
]' "$FIXTURES/combined.json" | jq -r -f "$CI_JQ_DIR/summary-combined.jq" 2>&1)
assert_contains "$OUT_CRAP_SORT" "crapOnly" "combined: severity sort surfaces CRAP-only finding in visible rows"

OUT_OLD_HEALTH=$(jq 'del(.health.summary.max_cyclomatic_threshold) | del(.health.summary.max_cognitive_threshold) | del(.health.summary.max_crap_threshold) | .health.findings = [{"path":"src/a.ts","name":"legacyComplex","line":1,"col":0,"cyclomatic":25,"cognitive":20,"line_count":10,"severity":"moderate","exceeded":"both"}]' "$FIXTURES/combined.json" | jq -r -f "$CI_JQ_DIR/summary-combined.jq" 2>&1)
assert_contains "$OUT_OLD_HEALTH" "thresholds: cyclomatic > default, cognitive > default" "combined: old JSON threshold fallback is explicit"
assert_not_contains "$OUT_OLD_HEALTH" "CRAP" "combined: old JSON without CRAP metadata hides CRAP column"

echo "  summary-combined.jq (scoped maintainability, GitLab):"
OUT_SCOPED=$(jq '.health.file_scores = [.health.file_scores[0]]' "$FIXTURES/combined.json" | jq -r -f "$CI_JQ_DIR/summary-combined.jq" 2>&1)
assert_contains "$OUT_SCOPED" "changed files" "scoped: shows changed files maintainability row"
assert_contains "$OUT_SCOPED" "76.2" "scoped: shows scoped maintainability value"
assert_contains "$OUT_SCOPED" "86.8" "scoped: still shows codebase maintainability"

echo "  summary-combined.jq (no scoped row when unfiltered, GitLab):"
assert_not_contains "$OUT" "changed files" "unfiltered: no scoped maintainability row"

echo "  summary-combined.jq (conditional tips, GitLab):"
assert_contains "$OUT" "fallow fix --dry-run" "tip: shows fix tip when fixable issues present"
assert_contains "$OUT" "@public" "tip: shows @public tip when unused exports present"
OUT_NO_FIX=$(jq '.check.unused_exports = [] | .check.unused_dependencies = [] | .check.unused_enum_members = [] | .check.circular_dependencies = [{"files":["a.ts","b.ts"],"length":2}] | .check.total_issues = 1' "$FIXTURES/combined.json" | jq -r -f "$CI_JQ_DIR/summary-combined.jq" 2>&1)
assert_not_contains "$OUT_NO_FIX" "fallow fix" "tip: no fix tip when no fixable issues"
assert_not_contains "$OUT_NO_FIX" "@public" "tip: no @public tip when no unused exports"

echo "  summary-combined.jq (clean state, GitLab):"
OUT_CLEAN=$(jq -r -f "$CI_JQ_DIR/summary-combined.jq" "$FIXTURES/combined-clean.json" 2>&1)
assert_contains "$OUT_CLEAN" "Quality gate passed" "clean: no issues"
assert_contains "$OUT_CLEAN" "Maintainability" "clean: shows maintainability"

echo "  summary-combined.jq (delta header with trend, GitLab):"
assert_contains "$OUT" "Health: B (72.3)" "delta: shows grade and score"
assert_contains "$OUT" "+7.2 pts vs previous" "delta: shows score delta"
assert_contains "$OUT" "C 65.1" "delta: shows previous grade and score"
assert_contains "$OUT" "dead exports 41.2%" "delta: shows dead export pct"
assert_contains "$OUT" "(-3.8%)" "delta: shows dead export delta"
assert_contains "$OUT" "avg complexity 7.1 (-1.2)" "delta: shows complexity delta"
assert_contains "$OUT" "chart_with_upwards_trend" "delta: uses GitLab emoji"

echo "  summary-combined.jq (delta header without trend, GitLab):"
assert_contains "$OUT_CLEAN" "Health: A (92.5)" "clean+score: shows absolute score"
assert_not_contains "$OUT_CLEAN" "vs previous" "clean+score: no delta when no trend"
assert_contains "$OUT_CLEAN" "FALLOW_SAVE_SNAPSHOT" "clean+score: shows save-snapshot hint"

echo "  summary-combined.jq (no delta header without score, GitLab):"
OUT_NO_SCORE=$(jq 'del(.health.health_score) | del(.health.health_trend)' "$FIXTURES/combined.json" | jq -r -f "$CI_JQ_DIR/summary-combined.jq" 2>&1)
assert_not_contains "$OUT_NO_SCORE" "Health:" "no-score: no delta header"

echo "  summary-combined.jq (delta header with increasing dead exports, GitLab):"
OUT_WORSE=$(jq '.health.health_trend.metrics[1].delta = 5.0 | .health.health_trend.metrics[1].current = 50.0' "$FIXTURES/combined.json" | jq -r -f "$CI_JQ_DIR/summary-combined.jq" 2>&1)
assert_contains "$OUT_WORSE" "suppress?" "worsening: shows suppress link when dead exports increase"

echo "  summary-combined.jq (runtime coverage details, GitLab):"
OUT_COMBINED_PROD=$(jq '.health.runtime_coverage = {"verdict":"hot-path-touched","summary":{"functions_tracked":4,"functions_hit":3,"functions_unhit":0,"functions_untracked":1,"coverage_percent":75,"trace_count":2400,"period_days":7,"deployments_seen":2},"findings":[{"path":"src/cold.ts","function":"coldPath","line":14,"verdict":"review_required","invocations":0,"confidence":"medium"}],"hot_paths":[{"path":"src/hot.ts","function":"hotPath","line":3,"invocations":250,"percentile":99}]}' "$FIXTURES/combined-clean.json" | jq -r -f "$CI_JQ_DIR/summary-combined.jq" 2>&1)
assert_contains "$OUT_COMBINED_PROD" "Runtime coverage" "combined prod: has runtime coverage details"
assert_contains "$OUT_COMBINED_PROD" "hotPath" "combined prod: shows hot path"
assert_contains "$OUT_COMBINED_PROD" "hot path touched" "combined prod (GitLab, verdict hot-path-touched): header uses 'touched' framing"

echo "  renderer semantic parity (GitHub vs GitLab):"
PARITY_OUT=$(node - "$SHARED_JQ_DIR" "$CI_JQ_DIR" "$DIR/../../action/tests/fixtures" <<'NODE'
const { execFileSync } = require("node:child_process");
const { readFileSync } = require("node:fs");
const [actionJqDir, gitlabJqDir, fixturesDir] = process.argv.slice(2);

const readFixture = (fixture) => JSON.parse(readFileSync(`${fixturesDir}/${fixture}`, "utf8"));
const checkFixture = readFixture("check.json");
const healthFixture = readFixture("health.json");
const dupesFixture = readFixture("dupes.json");
const auditFixture = {
  schema_version: 3,
  command: "audit",
  verdict: "fail",
  changed_files_count: 2,
  elapsed_ms: 42,
  summary: { dead_code_issues: 1, complexity_findings: 3, duplication_clone_groups: 1 },
  attribution: {
    gate: "new-only",
    dead_code_introduced: 1,
    dead_code_inherited: 0,
    complexity_introduced: 2,
    complexity_inherited: 1,
    duplication_introduced: 0,
    duplication_inherited: 1,
  },
  dead_code: {
    ...checkFixture,
    unused_exports: checkFixture.unused_exports.map((item) => ({ ...item, introduced: true })),
    unused_dependencies: checkFixture.unused_dependencies.map((item) => ({
      ...item,
      introduced: false,
    })),
  },
  complexity: {
    ...healthFixture,
    findings: [
      { ...healthFixture.findings[0], coverage_tier: "partial" },
      { ...healthFixture.findings[1], coverage_tier: "high" },
      healthFixture.findings[2],
    ],
    summary: {
      ...healthFixture.summary,
      coverage_model: "istanbul",
      istanbul_matched: 8,
      istanbul_total: 10,
    },
  },
  duplication: {
    ...dupesFixture,
    clone_groups: dupesFixture.clone_groups.map((item) => ({ ...item, introduced: false })),
  },
};

const cases = [
  { name: "summary-check", fixture: "check.json" },
  { name: "summary-health", fixture: "health.json" },
  { name: "summary-audit", input: auditFixture },
  { name: "summary-combined", fixture: "combined.json" },
];

const render = (dir, testCase) => {
  const args = ["-r", "-f", `${dir}/${testCase.name}.jq`];
  const options = { encoding: "utf8" };
  if (testCase.fixture) {
    args.push(`${fixturesDir}/${testCase.fixture}`);
  } else {
    options.input = JSON.stringify(testCase.input);
  }
  return execFileSync("jq", args, options);
};

const normalize = (text) =>
  text
    .split(/\r?\n/)
    .map((line) =>
      line
        .replace(/^> \[![A-Z]+\]$/, "")
        .replace(/^> :warning: /, "> ")
        .replace(/^> :bulb: /, "> ")
        .replace(/^> :chart_with_upwards_trend: /, "> ")
        .replace(/^# :seedling: Fallow$/, "# Fallow")
        .replace(/^# .* Fallow$/, "# Fallow"),
    )
    .filter((line) => line.trim() !== "")
    .filter((line) => !line.startsWith("> Run `fallow fix --dry-run`"))
    .filter((line) => !line.startsWith("> Intentionally public?"))
    .filter((line) => !line.startsWith("> Add [`/** @public */`"))
    .filter((line) => !line.startsWith("> Add [`// fallow-ignore-next-line`"))
    .join("\n");

const failures = [];
for (const testCase of cases) {
  const github = normalize(render(actionJqDir, testCase));
  const gitlab = normalize(render(gitlabJqDir, testCase));
  if (github !== gitlab) {
    failures.push(`${testCase.name}: normalized output drifted`);
  }
}

if (failures.length > 0) {
  console.log(failures.join("\n"));
  process.exit(1);
}
NODE
)
if [ -z "$PARITY_OUT" ]; then
  pass "renderer parity: normalized GitHub and GitLab summaries match"
else
  fail "renderer parity: normalized GitHub and GitLab summaries match" "$PARITY_OUT"
fi

# =========================================================================
# Shared summary scripts (reused from action/jq/, should still work)
# =========================================================================

echo ""
echo "=== Shared Summary scripts (from action/jq/) ==="

echo "  summary-dupes.jq:"
OUT=$(jq -r -f "$SHARED_JQ_DIR/summary-dupes.jq" "$FIXTURES/dupes.json" 2>&1)
assert_valid_markdown "$OUT" "produces output"
assert_contains "$OUT" "clone groups" "mentions clone groups"
assert_contains "$OUT" "Duplicated lines" "shows duplication stats"
assert_contains "$OUT" "content-parser.ts:27-50" "shows clone instance line range"

OUT_CLEAN=$(jq -r -f "$SHARED_JQ_DIR/summary-dupes.jq" "$FIXTURES/dupes-clean.json" 2>&1)
assert_contains "$OUT_CLEAN" "No code duplication" "clean: no duplication"

echo "  summary-fix.jq:"
# summary-fix needs fix results, test with combined (may not have fix data)
# Just verify it doesn't crash on missing data
OUT=$(echo '{"fixes":[],"dry_run":true}' | jq -r -f "$SHARED_JQ_DIR/summary-fix.jq" 2>&1)
assert_contains "$OUT" "No fixable issues" "empty fix: no fixable issues"

# =========================================================================
# GitLab-specific: no GitHub callouts in any output
# =========================================================================

echo ""
echo "=== GitLab markdown compatibility ==="

echo "  verify no GitHub-specific callouts in GitLab scripts:"
for jq_file in "$CI_JQ_DIR"/*.jq; do
  name=$(basename "$jq_file")
  if /usr/bin/grep -q '!\[NOTE\]\|!\[WARNING\]\|!\[TIP\]\|!\[IMPORTANT\]\|!\[CAUTION\]' "$jq_file" 2>/dev/null; then
    fail "$name" "contains GitHub callout syntax"
  else
    pass "$name has no GitHub callouts"
  fi
done

# =========================================================================
# GitLab CI YAML structure tests
# =========================================================================

echo ""
echo "=== GitLab CI YAML structure ==="

CI_YAML="$DIR/../gitlab-ci.yml"

echo "  gitlab-ci.yml:"
assert_contains "$(cat "$CI_YAML")" 'FALLOW_TYPE_AWARE: ""' "GitLab defaults defer type-aware enablement to repository config"
assert_contains "$(cat "$CI_YAML")" 'FALLOW_TYPE_AWARE_REQUIRE: ""' "GitLab defaults defer completeness policy to repository config"
assert_contains "$(cat "$CI_YAML")" 'unset FALLOW_TYPE_AWARE_PROJECTS FALLOW_TYPE_AWARE_REQUIRE' "GitLab removes empty env overrides before analysis"
assert_contains "$(cat "$CI_YAML")" 'Type-aware completeness gate failed' "GitLab preserves semantic completeness failures from valid JSON"
assert_contains "$(cat "$CI_YAML")" 'FALLOW_RENDER_PATH_PREFIX_SET' "GitLab separates renderer path prefixes from JSON analysis"
assert_contains "$(cat "$CI_YAML")" 'FILTERED_EXTRA_ARGS' "GitLab preserves non-presentation extra arguments"
assert_contains "$(cat "$CI_YAML")" "FALLOW_REVIEW" "has FALLOW_REVIEW variable"
assert_contains "$(cat "$CI_YAML")" "FALLOW_REVIEW_GUIDANCE" "has FALLOW_REVIEW_GUIDANCE variable"
assert_contains "$(cat "$CI_YAML")" "FALLOW_REVIEW_ID" "has FALLOW_REVIEW_ID variable"
assert_contains "$(cat "$CI_YAML")" "FALLOW_MAX_COMMENTS" "has FALLOW_MAX_COMMENTS variable"
assert_contains "$(cat "$CI_YAML")" "FALLOW_COMMENT" "has FALLOW_COMMENT variable"
assert_contains "$(cat "$CI_YAML")" "FALLOW_SUMMARY_SCOPE" "has FALLOW_SUMMARY_SCOPE variable"
assert_contains "$(cat "$CI_YAML")" "FALLOW_CODEQUALITY" "has FALLOW_CODEQUALITY variable"
assert_contains "$(cat "$CI_YAML")" "FALLOW_SECURITY_GATE" "has FALLOW_SECURITY_GATE variable"
assert_contains "$(cat "$CI_YAML")" '((.dupes.clone_groups // []) | length)' "combined issues use actionable dupes groups"
assert_contains "$(cat "$CI_YAML")" "project_fallow_spec" "reads package.json fallow pin"
assert_contains "$(cat "$CI_YAML")" "is_safe_version_spec" "validates fallow install spec"
assert_contains "$(cat "$CI_YAML")" "FALLOW_INSTALL_DRY_RUN" "supports install dry-run testing"
assert_contains "$(cat "$CI_YAML")" "FALLOW_SKIP_INSTALL" "supports skip-install for pre-installed fallow"
assert_contains "$(cat "$CI_YAML")" "GIT_STRATEGY" "overrides shared template git strategy"
assert_contains "$(cat "$CI_YAML")" "GIT_DEPTH" "fetches full history for changed-since"
assert_contains "$(cat "$CI_YAML")" "CI_MERGE_REQUEST_DIFF_BASE_SHA" "auto changed-since uses diff base SHA"
assert_contains "$(cat "$CI_YAML")" "comment.sh" "references comment.sh"
assert_contains "$(cat "$CI_YAML")" "review.sh" "references review.sh"
assert_contains "$(cat "$CI_YAML")" "gitlab_common.sh" "references shared GitLab helper script"
assert_contains "$(cat "$CI_YAML")" "gl-code-quality-report" "generates Code Quality report"
assert_contains "$(cat "$CI_YAML")" 'type == "array"' "preserves valid Code Quality reports from nonzero audit exits"
assert_contains "$(cat "$CI_YAML")" "fallow-mr-comment-envelope.json" "keeps typed MR comment envelope artifact"
assert_contains "$(cat "$CI_YAML")" "fallow-mr-decision.json" "keeps typed MR decision artifact"
assert_contains "$(cat "$CI_YAML")" "fallow-review-post.json" "keeps typed MR review post artifact"
assert_contains "$(cat "$CI_YAML")" '.error == true' "fails on structured fallow error JSON"
assert_contains "$(cat "$CI_YAML")" "does not support FALLOW_BASELINE/FALLOW_SAVE_BASELINE" "audit rejects generic baseline variables"
assert_contains "$(cat "$CI_YAML")" "suggestion" "mentions suggestion blocks in docs"

# =========================================================================
# Bash script structure tests
# =========================================================================

echo ""
echo "=== Bash script structure ==="

SCRIPTS_DIR="$DIR/../scripts"
GITLAB_COMMON="$(cat "$SCRIPTS_DIR/gitlab_common.sh")"

echo "  comment.sh:"
assert_contains "$(cat "$SCRIPTS_DIR/comment.sh")" "GITLAB_TOKEN" "requires GitLab token"
assert_contains "$(cat "$SCRIPTS_DIR/comment.sh")" "CI_JOB_TOKEN is read-only" "explains CI_JOB_TOKEN write limitation"
assert_contains "$(cat "$SCRIPTS_DIR/comment.sh")" "fallow-results" "uses fallow-results marker"
assert_contains "$(cat "$SCRIPTS_DIR/comment.sh")" "POST_COMMENT_ARGS=(" "builds MR comment post arguments"
assert_contains "$(cat "$SCRIPTS_DIR/comment.sh")" 'fallow "${POST_COMMENT_ARGS[@]}"' "delegates MR comment posting to Rust"
assert_contains "$(cat "$SCRIPTS_DIR/comment.sh")" "--provider gitlab" "uses GitLab post provider"
assert_contains "$(cat "$SCRIPTS_DIR/comment.sh")" "FALLOW_PR_COMMENT_ENVELOPE_FILE" "comment.sh asks fallow for typed PR comment envelope"
assert_contains "$(cat "$SCRIPTS_DIR/comment.sh")" "--envelope" "comment.sh passes typed PR comment envelope when present"
assert_contains "$(cat "$SCRIPTS_DIR/comment.sh")" "gitlab_common.sh" "loads shared GitLab API helpers"
assert_contains "$GITLAB_COMMON" "curl_retry" "wraps GitLab API calls with retry"
assert_not_contains "$GITLAB_COMMON" "source fallow-analysis-args" "legacy render fallback does not source workspace shell"
assert_contains "$GITLAB_COMMON" "rate limit response; retrying" "retries GitLab rate-limit responses"

TMP_CLEANUP_WORK=$(mktemp -d)
TMP_CLEANUP_REGISTRY="$TMP_CLEANUP_WORK/registry"
{
  FALLOW_TMP_REGISTRY="$TMP_CLEANUP_REGISTRY" \
    FALLOW_TMP_HELPER="$SCRIPTS_DIR/gitlab_common.sh" \
    bash -c '
      source "$FALLOW_TMP_HELPER"
      curl() {
        printf "%s\n" "${_FALLOW_TMPS[@]}" > "$FALLOW_TMP_REGISTRY"
        kill -TERM "$$"
      }
      curl_retry https://example.test/api
    '
} > /dev/null 2>&1
TMP_CLEANUP_STATUS=$?
if [ "$TMP_CLEANUP_STATUS" -eq 143 ]; then
  pass "curl retry cleanup fixture exits through its TERM path"
else
  fail "curl retry cleanup fixture exits through its TERM path" \
    "expected exit 143, got $TMP_CLEANUP_STATUS"
fi
TMP_FILES_CLEANED=true
while IFS= read -r temp_file; do
  if [ -e "$temp_file" ]; then
    TMP_FILES_CLEANED=false
  fi
done < "$TMP_CLEANUP_REGISTRY"
if [ "$TMP_FILES_CLEANED" = "true" ] && [ "$(wc -l < "$TMP_CLEANUP_REGISTRY" | tr -d ' ')" = "2" ]; then
  pass "curl retry exit trap removes registered temporary files"
else
  fail "curl retry exit trap removes registered temporary files" \
    "registered files remained after TERM or the fixture did not create both files"
fi
rm -rf "$TMP_CLEANUP_WORK"
assert_contains "$(cat "$SCRIPTS_DIR/comment.sh")" "Unsupported FALLOW_SUMMARY_SCOPE" "comment.sh warns on invalid summary scope"

echo "  review.sh:"
assert_contains "$(cat "$SCRIPTS_DIR/review.sh")" "review-gitlab" "renders typed GitLab review envelope"
assert_contains "$(cat "$SCRIPTS_DIR/review.sh")" "fallow ci post-review" "delegates GitLab review posting to Rust"
assert_contains "$(cat "$SCRIPTS_DIR/review.sh")" "RESOLUTION_NOUN" "logs and pluralizes reconciliation reply counts"
assert_contains "$(cat "$SCRIPTS_DIR/review.sh")" "THREAD_NOUN" "logs and pluralizes resolved-thread counts"
assert_contains "$(cat "$SCRIPTS_DIR/review.sh")" "--provider gitlab" "uses GitLab post provider"
assert_contains "$(cat "$SCRIPTS_DIR/review.sh")" "suggestion" "adds suggestion blocks"
assert_contains "$(cat "$SCRIPTS_DIR/review.sh")" "fallow-review" "uses fallow-review marker"
assert_contains "$(cat "$DIR/../../crates/cli/src/ci_review_post.rs")" "fingerprint" "Rust review post deduplicates by typed fingerprint"
assert_contains "$(cat "$SCRIPTS_DIR/review.sh")" "gitlab_common.sh" "loads shared GitLab API helpers"
assert_not_contains "$(cat "$SCRIPTS_DIR/review.sh")" "merge-comments" "does not keep legacy jq merge fallback"
assert_not_contains "$(cat "$SCRIPTS_DIR/review.sh")" "FALLOW_SHARED_JQ_DIR" "does not use shared jq fallback scripts"
assert_not_contains "$(cat "$SCRIPTS_DIR/review.sh")" "FALLOW_SUMMARY_SCOPE" "review.sh does not consume summary scope"

# =========================================================================
# Typed GitLab script integration tests
# =========================================================================

echo ""
echo "=== Typed GitLab script integration ==="

CI_TYPED_WORK=$(mktemp -d)
CI_TYPED_BIN="$CI_TYPED_WORK/bin"
CI_TYPED_LOG="$CI_TYPED_WORK/mock.log"
mkdir -p "$CI_TYPED_BIN"

cat > "$CI_TYPED_BIN/fallow" <<'SH'
#!/usr/bin/env bash
printf 'fallow %s\n' "$*" >> "$MOCK_LOG"
printf 'summary_scope=%s\n' "${FALLOW_SUMMARY_SCOPE:-}" >> "$MOCK_LOG"
if [ "${1:-}" = "ci" ]; then
  if [ "${2:-}" = "post-pr-comment" ]; then
    printf '{"action":"update","marker_id":"fallow-results","comment_id":"777","body":"ok"}\n'
  elif [ "${2:-}" = "post-review" ]; then
    case "${MOCK_POST_REVIEW_ERRORS:-}" in
      apply)
        printf '{"action":"post_review","comments_posted":1,"apply_errors":["resolve failed"],"post_errors":[],"apply_hint":"refresh provider state","failed_fingerprints":["a"],"unapplied_fingerprints":["a"]}\n'
        ;;
      post)
        printf '{"action":"post_review","comments_posted":0,"apply_errors":[],"post_errors":["post failed"],"apply_hint":"rerun the job"}\n'
        ;;
      *)
        printf '{"action":"post_review","comments_posted":1,"apply_errors":[],"post_errors":[]}\n'
        ;;
    esac
  else
    printf '{"schema":"fallow-review-reconcile/v1","stale":[]}\n'
  fi
  exit 0
fi
if [ "${MOCK_RENDER_FAILURE:-}" = "1" ]; then
  echo 'saved audit envelope is missing required field `version`' >&2
  exit 2
fi
if [ "${MOCK_SAVED_RENDER_UNSUPPORTED:-}" = "1" ] && [ "${1:-}" = "report" ]; then
  echo 'Error: fallow report supports --format github-annotations, github-summary, codeclimate, or sarif only' >&2
  exit 2
fi
format=""
previous=""
for arg in "$@"; do
  if [ "$previous" = "--format" ]; then
    format="$arg"
    break
  fi
  previous="$arg"
done
case "$format" in
  pr-comment-gitlab)
    # MOCK_BASELINE_ADVISORY mirrors what the real renderer emits for a rotted
    # baseline with the gate armed: the advisory plus the gate inventory in the
    # body's status blockquote, and one decision row per armed gate.
    if [ -n "${FALLOW_PR_DECISION_FILE:-}" ]; then
      if [ "${MOCK_BASELINE_ADVISORY:-}" = "1" ]; then
        printf '{"schema":"fallow-pr-decision/v1","title":"Fallow","conclusion":"success","gates":[{"id":"check","label":"Dead code","status":"success","observed":"0 findings","threshold":null,"scope":"new code"},{"id":"stale-baseline","label":"Stale baseline","status":"failure","observed":"fail","threshold":null,"scope":"this run"}],"annotations":[],"details":{"summary_markdown":"Clean","full_report_path":null,"details_url":null}}\n' > "$FALLOW_PR_DECISION_FILE"
      else
        printf '{"schema":"fallow-pr-decision/v1","title":"Fallow","conclusion":"success","gates":[],"annotations":[],"details":{"summary_markdown":"Clean","full_report_path":null,"details_url":null}}\n' > "$FALLOW_PR_DECISION_FILE"
      fi
    fi
    if [ -n "${FALLOW_PR_DETAILS_FILE:-}" ]; then
      printf '{"schema":"fallow-pr-details/v1","title":"Fallow","sections":[]}\n' > "$FALLOW_PR_DETAILS_FILE"
    fi
    printf '<!-- fallow-id: fallow-results -->\n### Fallow smoke\n\nGenerated by fallow.\n'
    if [ "${MOCK_BASELINE_ADVISORY:-}" = "1" ]; then
      printf '\n> **Baseline matched nothing.** All 8 saved entries went unmatched. Paths may have changed, or the baseline was saved elsewhere. Re-save it from a whole-project run. Gate outcomes: failed stale-baseline.\n'
    fi
    ;;
  review-gitlab)
    if [ "${MOCK_ZERO_REVIEW:-}" = "1" ]; then
      cat <<'JSON'
{"body":"### Fallow smoke\n\n<!-- fallow-review -->","comments":[],"meta":{"schema":"fallow-review-envelope/v1","provider":"gitlab"}}
JSON
      exit 0
    fi
    cat <<'JSON'
{"body":"### Fallow smoke\n\n<!-- fallow-review -->","comments":[{"body":"**warn** `fallow/smoke`: smoke\n\n<!-- fallow-fingerprint: abc -->","position":{"base_sha":"base","start_sha":"start","head_sha":"head","position_type":"text","old_path":"src/a.ts","new_path":"src/a.ts","new_line":1},"fingerprint":"abc"}],"meta":{"schema":"fallow-review-envelope/v1","provider":"gitlab"}}
JSON
    ;;
  *)
    printf '{}\n'
    ;;
esac
SH
chmod +x "$CI_TYPED_BIN/fallow"

cat > "$CI_TYPED_BIN/curl" <<'SH'
#!/usr/bin/env bash
printf 'curl %s\n' "$*" >> "$MOCK_LOG"
last=""
for arg in "$@"; do
  last="$arg"
done
case "$last" in
  *"/notes?per_page=100")
    if [ "${MOCK_EXISTING_REVIEW:-}" = "1" ]; then
      printf '[{"id":777,"body":"<!-- fallow-review -->"}]\n'
    else
      printf '[]\n'
    fi
    ;;
  *"/discussions?per_page=100")
    printf '[]\n'
    ;;
  *"/merge_requests/123")
    printf '{"diff_refs":{"base_sha":"base","start_sha":"start","head_sha":"head"}}\n'
    ;;
  *)
    printf '{}\n'
    ;;
esac
SH
chmod +x "$CI_TYPED_BIN/curl"

printf '{"kind":"dead-code","schema_version":9}\n' > "$CI_TYPED_WORK/fallow-results.json"
printf '%s\0' check --format json --root . > "$CI_TYPED_WORK/fallow-analysis-args.bin"
(
  cd "$CI_TYPED_WORK"
  PATH="$CI_TYPED_BIN:$PATH" \
    MOCK_LOG="$CI_TYPED_LOG" \
    GITLAB_TOKEN="test" \
    CI_API_V4_URL="https://gitlab.example/api/v4" \
    CI_PROJECT_ID="18" \
    CI_MERGE_REQUEST_IID="123" \
    FALLOW_COMMAND="check" \
    FALLOW_RENDER_PATH_PREFIX_SET="1" \
    FALLOW_RENDER_PATH_PREFIX="custom/base" \
    FALLOW_SUMMARY_SCOPE="diff" \
    bash "$SCRIPTS_DIR/comment.sh" > /dev/null
  PATH="$CI_TYPED_BIN:$PATH" \
    MOCK_LOG="$CI_TYPED_LOG" \
    GITLAB_TOKEN="test" \
    CI_API_V4_URL="https://gitlab.example/api/v4" \
    CI_PROJECT_ID="18" \
    CI_MERGE_REQUEST_IID="123" \
    CI_COMMIT_SHA="abcdef1234567890" \
    FALLOW_COMMAND="check" \
    FALLOW_RENDER_PATH_PREFIX_SET="1" \
    FALLOW_RENDER_PATH_PREFIX="custom/base" \
    FALLOW_ROOT="." \
    MAX_COMMENTS="5" \
    bash "$SCRIPTS_DIR/review.sh" > "$CI_TYPED_WORK/review-clean.out"
  PATH="$CI_TYPED_BIN:$PATH" \
    MOCK_LOG="$CI_TYPED_LOG" \
    MOCK_ZERO_REVIEW="1" \
    MOCK_EXISTING_REVIEW="1" \
    GITLAB_TOKEN="test" \
    CI_API_V4_URL="https://gitlab.example/api/v4" \
    CI_PROJECT_ID="18" \
    CI_MERGE_REQUEST_IID="123" \
    CI_COMMIT_SHA="abcdef1234567890" \
    FALLOW_COMMAND="check" \
    FALLOW_ROOT="." \
    MAX_COMMENTS="5" \
    bash "$SCRIPTS_DIR/review.sh" > /dev/null
  PATH="$CI_TYPED_BIN:$PATH" \
    MOCK_LOG="$CI_TYPED_LOG" \
    MOCK_POST_REVIEW_ERRORS="apply" \
    GITLAB_TOKEN="test" \
    CI_API_V4_URL="https://gitlab.example/api/v4" \
    CI_PROJECT_ID="18" \
    CI_MERGE_REQUEST_IID="123" \
    CI_COMMIT_SHA="abcdef1234567890" \
    FALLOW_COMMAND="check" \
    FALLOW_ROOT="." \
    MAX_COMMENTS="5" \
    bash "$SCRIPTS_DIR/review.sh" > "$CI_TYPED_WORK/review-apply-error.out"
  PATH="$CI_TYPED_BIN:$PATH" \
    MOCK_LOG="$CI_TYPED_LOG" \
    MOCK_POST_REVIEW_ERRORS="post" \
    GITLAB_TOKEN="test" \
    CI_API_V4_URL="https://gitlab.example/api/v4" \
    CI_PROJECT_ID="18" \
    CI_MERGE_REQUEST_IID="123" \
    CI_COMMIT_SHA="abcdef1234567890" \
    FALLOW_COMMAND="check" \
    FALLOW_ROOT="." \
    MAX_COMMENTS="5" \
    bash "$SCRIPTS_DIR/review.sh" > "$CI_TYPED_WORK/review-post-error.out"
  PATH="$CI_TYPED_BIN:$PATH" \
    MOCK_LOG="$CI_TYPED_LOG" \
    MOCK_RENDER_FAILURE="1" \
    GITLAB_TOKEN="test" \
    CI_API_V4_URL="https://gitlab.example/api/v4" \
    CI_PROJECT_ID="18" \
    CI_MERGE_REQUEST_IID="123" \
    FALLOW_COMMAND="check" \
    bash "$SCRIPTS_DIR/comment.sh" > "$CI_TYPED_WORK/comment-render-failure.out"
  PATH="$CI_TYPED_BIN:$PATH" \
    MOCK_LOG="$CI_TYPED_LOG" \
    MOCK_RENDER_FAILURE="1" \
    GITLAB_TOKEN="test" \
    CI_API_V4_URL="https://gitlab.example/api/v4" \
    CI_PROJECT_ID="18" \
    CI_MERGE_REQUEST_IID="123" \
    CI_COMMIT_SHA="abcdef1234567890" \
    FALLOW_COMMAND="check" \
    FALLOW_DIFF_FILE="$CI_TYPED_WORK/fallow-mr.diff" \
    bash "$SCRIPTS_DIR/review.sh" > "$CI_TYPED_WORK/review-render-failure.out"
  PATH="$CI_TYPED_BIN:$PATH" \
    MOCK_LOG="$CI_TYPED_LOG" \
    MOCK_SAVED_RENDER_UNSUPPORTED="1" \
    GITLAB_TOKEN="test" \
    CI_API_V4_URL="https://gitlab.example/api/v4" \
    CI_PROJECT_ID="18" \
    CI_MERGE_REQUEST_IID="123" \
    FALLOW_COMMAND="check" \
    FALLOW_RENDER_PATH_PREFIX_SET="1" \
    FALLOW_RENDER_PATH_PREFIX="legacy/base" \
    bash "$SCRIPTS_DIR/comment.sh" > "$CI_TYPED_WORK/comment-legacy-fallback.out"
  PATH="$CI_TYPED_BIN:$PATH" \
    MOCK_LOG="$CI_TYPED_LOG" \
    MOCK_SAVED_RENDER_UNSUPPORTED="1" \
    GITLAB_TOKEN="test" \
    CI_API_V4_URL="https://gitlab.example/api/v4" \
    CI_PROJECT_ID="18" \
    CI_MERGE_REQUEST_IID="123" \
    CI_COMMIT_SHA="abcdef1234567890" \
    FALLOW_COMMAND="check" \
    FALLOW_ROOT="." \
    MAX_COMMENTS="5" \
    bash "$SCRIPTS_DIR/review.sh" > "$CI_TYPED_WORK/review-legacy-fallback.out"
)
CI_TYPED_OUT=$(cat "$CI_TYPED_LOG")
assert_contains "$CI_TYPED_OUT" "--format pr-comment-gitlab" "comment.sh invokes typed MR comment format"
assert_contains "$CI_TYPED_OUT" "--format review-gitlab" "review.sh invokes typed GitLab review format"
assert_contains "$CI_TYPED_OUT" "report --from fallow-results.json" "GitLab renderers reuse the saved analysis envelope"
assert_contains "$CI_TYPED_OUT" "--report-path-prefix custom/base" "GitLab renderers preserve presentation path prefixes"
assert_contains "$CI_TYPED_OUT" "fallow check --format pr-comment-gitlab --root . --report-path-prefix legacy/base" \
  "comment.sh safely falls back to direct rendering for older pinned fallow"
assert_contains "$CI_TYPED_OUT" "fallow check --format review-gitlab --root ." \
  "review.sh safely falls back to direct rendering for older pinned fallow"
assert_contains "$(cat "$CI_TYPED_WORK/comment-legacy-fallback.out")" "compatible direct rendering" \
  "comment.sh discloses older-binary fallback"
assert_contains "$(cat "$CI_TYPED_WORK/review-legacy-fallback.out")" "compatible direct rendering" \
  "review.sh discloses older-binary fallback"
assert_contains "$CI_TYPED_OUT" "fallow ci post-pr-comment --provider gitlab" "comment.sh invokes GitLab MR comment post command"
assert_contains "$CI_TYPED_OUT" "summary_scope=diff" "comment.sh passes FALLOW_SUMMARY_SCOPE to typed MR comment render"
assert_contains "$(cat "$SCRIPTS_DIR/comment.sh")" "FALLOW_PR_DECISION_FILE" "comment.sh asks fallow for typed MR decision sidecar"
assert_contains "$(cat "$SCRIPTS_DIR/comment.sh")" "FALLOW_PR_DETAILS_FILE" "comment.sh asks fallow for typed MR details artifact"

# #2675, mirroring the GitHub assertions: the advisory and the gate row are the
# CLI's render, and the template's job is to carry them to the MR note and to
# the decision sidecar a downstream job reads.
CI_BASELINE_LOG="$CI_TYPED_WORK/baseline-comment.log"
: > "$CI_BASELINE_LOG"
(
  cd "$CI_TYPED_WORK"
  PATH="$CI_TYPED_BIN:$PATH" \
    MOCK_LOG="$CI_BASELINE_LOG" \
    MOCK_BASELINE_ADVISORY="1" \
    GITLAB_TOKEN="test" \
    CI_API_V4_URL="https://gitlab.example/api/v4" \
    CI_PROJECT_ID="18" \
    CI_MERGE_REQUEST_IID="123" \
    FALLOW_COMMAND="check" \
    bash "$SCRIPTS_DIR/comment.sh" > /dev/null
)
CI_BASELINE_BODY=$(cat "$CI_TYPED_WORK/fallow-mr-comment.md")
CI_BASELINE_DECISION=$(cat "$CI_TYPED_WORK/fallow-mr-decision.json")
assert_contains "$CI_BASELINE_BODY" "**Baseline matched nothing.**" \
  "the posted MR note carries the baseline advisory"
assert_contains "$CI_BASELINE_BODY" "Gate outcomes: failed stale-baseline." \
  "the posted MR note keeps the gate inventory beside the advisory"
assert_contains "$CI_BASELINE_DECISION" '"id":"stale-baseline"' \
  "the MR decision sidecar carries the stale-baseline gate row"
assert_contains "$(cat "$CI_BASELINE_LOG")" "ci post-pr-comment --provider gitlab" \
  "the note carrying the advisory is the one posted"
assert_contains "$(cat "$DIR/../../ci/gitlab-ci.yml")" "FALLOW_PR_COMMENT_LAYOUT" "GitLab template exposes sticky MR comment layout"
CI_BLANK_SUMMARY_SCOPE_COUNT=$(printf '%s\n' "$CI_TYPED_OUT" | grep -c '^summary_scope=$' || true)
if [ "$CI_BLANK_SUMMARY_SCOPE_COUNT" -ge 1 ]; then
  pass "review.sh does not receive FALLOW_SUMMARY_SCOPE by default"
else
  fail "review.sh does not receive FALLOW_SUMMARY_SCOPE by default" "$CI_TYPED_OUT"
fi
assert_contains "$CI_TYPED_OUT" "fallow ci post-review --provider gitlab" "review.sh invokes GitLab review post command"
assert_contains "$(cat "$CI_TYPED_WORK/review-clean.out")" \
  "0 resolution replies posted, 0 threads resolved" \
  "review.sh exposes successful reconciliation counters"
assert_contains "$(cat "$SCRIPTS_DIR/review.sh")" "apply_errors" "review.sh checks reconcile apply errors"
assert_contains "$(cat "$SCRIPTS_DIR/review.sh")" "apply_hint" "review.sh emits reconcile apply hint"
assert_not_contains "$(cat "$CI_TYPED_WORK/review-clean.out")" "WARNING: fallow post-review incomplete" \
  "review.sh stays quiet when reconciliation fully succeeds"
assert_contains "$(cat "$CI_TYPED_WORK/review-apply-error.out")" \
  "WARNING: fallow post-review incomplete: refresh provider state" \
  "review.sh warns when applying reconciliation is incomplete"
assert_contains "$(cat "$CI_TYPED_WORK/review-apply-error.out")" \
  "(unapplied fingerprints: a)" \
  "review.sh names the fingerprints reconciliation did not apply"
assert_contains "$(cat "$CI_TYPED_WORK/review-post-error.out")" \
  "WARNING: fallow post-review incomplete: rerun the job" \
  "review.sh warns when posting review comments is incomplete"
assert_contains "$(cat "$CI_TYPED_WORK/comment-render-failure.out")" \
  'saved audit envelope is missing required field `version`' \
  "comment.sh surfaces saved-render stderr"
assert_contains "$(cat "$CI_TYPED_WORK/review-render-failure.out")" \
  'saved audit envelope is missing required field `version`' \
  "review.sh surfaces saved-render stderr"
rm -rf "$CI_TYPED_WORK"

# =========================================================================
# API failure handling: provider failure policy remains delegated to Rust
# =========================================================================
# Covers the issue #470 behavior after provider pagination moved into the typed
# Rust posting path. The shell wrapper must delegate without recreating lookup,
# deduplication, or 4xx/5xx policy.

echo ""
echo "=== API failure handling (issue #470) ==="

CI_API_FAIL_WORK=$(mktemp -d)
CI_API_FAIL_BIN="$CI_API_FAIL_WORK/bin"
mkdir -p "$CI_API_FAIL_BIN"
SCRIPTS_DIR="$DIR/../scripts"

# Shared fallow + curl mocks. Legacy pagination failure switches remain in the
# curl mock to prove neither review.sh nor comment.sh performs provider lookup
# in shell; both delegate posting to the typed Rust commands.

write_ci_api_fail_mocks() {
  cat > "$CI_API_FAIL_BIN/fallow" <<'SH'
#!/usr/bin/env bash
printf 'fallow %s\n' "$*" >> "$MOCK_LOG"
if [ "${1:-}" = "ci" ]; then
  if [ "${2:-}" = "post-pr-comment" ]; then
    printf '{"action":"update","marker_id":"fallow-results","comment_id":"777","body":"ok"}\n'
  elif [ "${2:-}" = "post-review" ]; then
    printf '{"action":"post_review","comments_posted":1,"apply_errors":[],"post_errors":[]}\n'
  else
    printf '{"schema":"fallow-review-reconcile/v1","stale":[]}\n'
  fi
  exit 0
fi
format=""
previous=""
for arg in "$@"; do
  if [ "$previous" = "--format" ]; then
    format="$arg"; break
  fi
  previous="$arg"
done
case "$format" in
  pr-comment-gitlab)
    cat <<'BODY'
<!-- fallow-id: fallow-results -->
### Fallow smoke

Generated by fallow.
BODY
    ;;
  review-gitlab)
    if [ "${MOCK_ZERO_REVIEW:-}" = "1" ]; then
      cat <<'JSON'
{"body":"### Fallow smoke\n\n<!-- fallow-review -->","comments":[],"meta":{"schema":"fallow-review-envelope/v1","provider":"gitlab"}}
JSON
    else
      cat <<'JSON'
{"body":"### Fallow smoke\n\n<!-- fallow-review -->","comments":[{"body":"**warn** `fallow/smoke`: smoke\n\n<!-- fallow-fingerprint: abc -->","position":{"base_sha":"base","start_sha":"start","head_sha":"head","position_type":"text","old_path":"src/a.ts","new_path":"src/a.ts","new_line":1},"fingerprint":"abc"}],"meta":{"schema":"fallow-review-envelope/v1","provider":"gitlab"}}
JSON
    fi
    ;;
esac
SH
  chmod +x "$CI_API_FAIL_BIN/fallow"

  cat > "$CI_API_FAIL_BIN/curl" <<'SH'
#!/usr/bin/env bash
printf 'curl %s\n' "$*" >> "$MOCK_LOG"
# Find any curl header-output file and the last URL argument.
headers_file=""
last=""
i=1
while [ $i -le $# ]; do
  arg=$(eval echo \"\${$i}\")
  if [ "$arg" = "-D" ]; then
    nexti=$((i + 1))
    headers_file=$(eval echo \"\${$nexti}\")
  fi
  last="$arg"
  i=$((i + 1))
done
case "$last" in
  *"/discussions?per_page=100"|*"/notes?per_page=100")
    if [ "${MOCK_PAGINATE_FAIL:-}" = "5xx" ]; then
      echo "curl: (22) The requested URL returned error: 502 Bad Gateway" >&2
      exit 22
    fi
    if [ "${MOCK_PAGINATE_FAIL:-}" = "4xx" ]; then
      echo "curl: (22) The requested URL returned error: 403 Forbidden" >&2
      exit 22
    fi
    [ -n "$headers_file" ] && : > "$headers_file"
    printf '[]\n'
    ;;
  *"/merge_requests/123")
    [ -n "$headers_file" ] && : > "$headers_file"
    printf '{"diff_refs":{"base_sha":"base","start_sha":"start","head_sha":"head"}}\n'
    ;;
  *)
    [ -n "$headers_file" ] && : > "$headers_file"
    printf '{}\n'
    ;;
esac
exit 0
SH
  chmod +x "$CI_API_FAIL_BIN/curl"
}

ci_api_fail_review_run() {
  local fail_mode=$1
  local exit_status_var=$2
  local stderr_var=$3
  local mock_zero=$4   # "1" for summary-only path
  write_ci_api_fail_mocks
  printf '{"kind":"dead-code","schema_version":9}\n' > "$CI_API_FAIL_WORK/fallow-results.json"
  : > "$CI_API_FAIL_WORK/mock.log"
  rm -f "$CI_API_FAIL_WORK/fallow-skip-reason.txt"
  local _stderr _status
  _stderr=$(cd "$CI_API_FAIL_WORK" \
    && PATH="$CI_API_FAIL_BIN:$PATH" \
    MOCK_LOG="$CI_API_FAIL_WORK/mock.log" \
    MOCK_PAGINATE_FAIL="$fail_mode" \
    MOCK_ZERO_REVIEW="$mock_zero" \
    GITLAB_TOKEN="test" \
    CI_API_V4_URL="https://gitlab.example/api/v4" \
    CI_PROJECT_ID="18" \
    CI_MERGE_REQUEST_IID="123" \
    CI_COMMIT_SHA="abcdef1234567890" \
    FALLOW_COMMAND="check" \
    FALLOW_ROOT="." \
    MAX_COMMENTS="5" \
    FALLOW_API_RETRIES=1 \
    FALLOW_API_RETRY_DELAY=0 \
    bash "$SCRIPTS_DIR/review.sh" 2>&1 1>/dev/null)
  _status=$?
  printf -v "$exit_status_var" '%s' "$_status"
  printf -v "$stderr_var" '%s' "$_stderr"
}

# Test 7: review.sh delegates provider posting and dedup to Rust.
ci_api_fail_review_run "5xx" R7_STATUS R7_STDERR ""
[ "$R7_STATUS" -eq 0 ] \
  && pass "review.sh: Rust-delegated review post exits 0" \
  || fail "review.sh: Rust-delegated review post exits 0" "got $R7_STATUS"
if [ -f "$CI_API_FAIL_WORK/fallow-skip-reason.txt" ] && grep -q '^pagination_failure$' "$CI_API_FAIL_WORK/fallow-skip-reason.txt"; then
  fail "review.sh: leaves skip reason untouched while Rust owns dedup policy" \
    "got: $(cat "$CI_API_FAIL_WORK/fallow-skip-reason.txt" 2>/dev/null || echo absent)"
else
  pass "review.sh: leaves skip reason untouched while Rust owns dedup policy"
fi
assert_contains "$(cat "$CI_API_FAIL_WORK/mock.log")" "fallow ci post-review --provider gitlab" \
  "review.sh: delegates provider review posting to Rust"
if /usr/bin/grep -q -- "--request POST" "$CI_API_FAIL_WORK/mock.log"; then
  fail "review.sh: does not call curl POST for review posting" "$(cat "$CI_API_FAIL_WORK/mock.log")"
else
  pass "review.sh: does not call curl POST for review posting"
fi

# Test 7b: summary-only path also delegates provider posting to Rust.
ci_api_fail_review_run "5xx" R7B_STATUS R7B_STDERR "1"
[ "$R7B_STATUS" -eq 0 ] \
  && pass "review.sh: summary-only path delegates and exits 0" \
  || fail "review.sh: summary-only path delegates and exits 0" "got $R7B_STATUS"
if [ -f "$CI_API_FAIL_WORK/fallow-dedup-lookup-failed.txt" ] && grep -q '^true$' "$CI_API_FAIL_WORK/fallow-dedup-lookup-failed.txt"; then
  fail "review.sh: summary-only path leaves dedup marker false while Rust owns dedup policy" \
    "got: $(cat "$CI_API_FAIL_WORK/fallow-dedup-lookup-failed.txt" 2>/dev/null || echo absent)"
else
  pass "review.sh: summary-only path leaves dedup marker false while Rust owns dedup policy"
fi
if [ -f "$CI_API_FAIL_WORK/fallow-skip-reason.txt" ] && grep -q '^none$' "$CI_API_FAIL_WORK/fallow-skip-reason.txt"; then
  pass "review.sh: summary-only path keeps fallow-skip-reason.txt at none"
else
  fail "review.sh: summary-only path keeps fallow-skip-reason.txt at none" \
    "got: $(cat "$CI_API_FAIL_WORK/fallow-skip-reason.txt" 2>/dev/null || echo absent)"
fi

# Test 8: provider 4xx policy now lives in Rust; shell wrapper remains non-fatal.
ci_api_fail_review_run "4xx" R8_STATUS R8_STDERR ""
[ "$R8_STATUS" -eq 0 ] \
  && pass "review.sh: provider policy is delegated for 4xx path" \
  || fail "review.sh: provider policy is delegated for 4xx path" "got $R8_STATUS"

# Test 8b: retry-exhausted 429 behavior now lives in the Rust post-review
# command; the shell wrapper should still delegate and stay non-fatal.
write_ci_api_fail_mocks
# Override the curl mock with one that returns a 429 error string.
cat > "$CI_API_FAIL_BIN/curl" <<'SH'
#!/usr/bin/env bash
printf 'curl %s\n' "$*" >> "$MOCK_LOG"
headers_file=""; last=""
i=1
while [ $i -le $# ]; do
  arg=$(eval echo \"\${$i}\")
  if [ "$arg" = "-D" ]; then
    nexti=$((i + 1)); headers_file=$(eval echo \"\${$nexti}\")
  fi
  last="$arg"; i=$((i + 1))
done
case "$last" in
  *"/discussions?per_page=100")
    echo "curl: (22) The requested URL returned error: 429 Too Many Requests" >&2
    exit 22
    ;;
  *"/merge_requests/123")
    [ -n "$headers_file" ] && : > "$headers_file"
    printf '{"diff_refs":{"base_sha":"base","start_sha":"start","head_sha":"head"}}\n'
    ;;
  *)
    [ -n "$headers_file" ] && : > "$headers_file"
    printf '{}\n'
    ;;
esac
SH
chmod +x "$CI_API_FAIL_BIN/curl"

printf '{"kind":"dead-code","schema_version":9}\n' > "$CI_API_FAIL_WORK/fallow-results.json"
: > "$CI_API_FAIL_WORK/mock.log"
R8B_STDERR=$(cd "$CI_API_FAIL_WORK" \
  && PATH="$CI_API_FAIL_BIN:$PATH" \
  MOCK_LOG="$CI_API_FAIL_WORK/mock.log" \
  GITLAB_TOKEN=test \
  CI_API_V4_URL="https://gitlab.example/api/v4" \
  CI_PROJECT_ID=18 CI_MERGE_REQUEST_IID=123 CI_COMMIT_SHA=abcdef1234567890 \
  FALLOW_COMMAND=check FALLOW_ROOT=. MAX_COMMENTS=5 \
  FALLOW_API_RETRIES=1 FALLOW_API_RETRY_DELAY=0 \
  bash "$SCRIPTS_DIR/review.sh" 2>&1 1>/dev/null)
R8B_STATUS=$?
[ "$R8B_STATUS" -eq 0 ] \
  && pass "review.sh: retry-exhausted 429 remains non-fatal in shell wrapper" \
  || fail "review.sh: retry-exhausted 429 remains non-fatal in shell wrapper" "got $R8B_STATUS"
assert_contains "$(cat "$CI_API_FAIL_WORK/mock.log")" "fallow ci post-review --provider gitlab" \
  "review.sh: 429 path still delegates review posting to Rust"

# Test 9b: review.sh must preserve an existing dedup marker from an earlier
# job step. comment.sh no longer writes this marker, but downstream jobs can
# still create it before review.sh runs.
write_ci_api_fail_mocks
printf '{"kind":"dead-code","schema_version":9}\n' > "$CI_API_FAIL_WORK/fallow-results.json"
: > "$CI_API_FAIL_WORK/mock.log"
printf 'true\n' > "$CI_API_FAIL_WORK/fallow-dedup-lookup-failed.txt"

# Run review.sh against the same working dir with no pagination failure. Its
# init must not reset an existing marker.
(cd "$CI_API_FAIL_WORK" \
  && PATH="$CI_API_FAIL_BIN:$PATH" \
  MOCK_LOG="$CI_API_FAIL_WORK/mock.log" \
  MOCK_PAGINATE_FAIL="" \
  GITLAB_TOKEN=test \
  CI_API_V4_URL="https://gitlab.example/api/v4" \
  CI_PROJECT_ID=18 CI_MERGE_REQUEST_IID=123 CI_COMMIT_SHA=abcdef1234567890 \
  FALLOW_COMMAND=check FALLOW_ROOT=. MAX_COMMENTS=5 \
  FALLOW_API_RETRIES=1 FALLOW_API_RETRY_DELAY=0 \
  bash "$SCRIPTS_DIR/review.sh" >/dev/null 2>&1) || true

if [ -f "$CI_API_FAIL_WORK/fallow-dedup-lookup-failed.txt" ] && grep -q '^true$' "$CI_API_FAIL_WORK/fallow-dedup-lookup-failed.txt"; then
  pass "review.sh: preserves preexisting dedup_lookup_failed=true marker"
else
  fail "review.sh: preserves preexisting dedup_lookup_failed=true marker" \
    "got: $(cat "$CI_API_FAIL_WORK/fallow-dedup-lookup-failed.txt" 2>/dev/null || echo absent) (review.sh clobbered comment.sh's value)"
fi

# Test 9: comment.sh delegates sticky MR posting to Rust and leaves the dedup
# marker false when the Rust post command succeeds.
write_ci_api_fail_mocks
printf '{"kind":"dead-code","schema_version":9}\n' > "$CI_API_FAIL_WORK/fallow-results.json"
: > "$CI_API_FAIL_WORK/mock.log"
rm -f "$CI_API_FAIL_WORK/fallow-skip-reason.txt" "$CI_API_FAIL_WORK/fallow-dedup-lookup-failed.txt"
(cd "$CI_API_FAIL_WORK" \
  && PATH="$CI_API_FAIL_BIN:$PATH" \
  MOCK_LOG="$CI_API_FAIL_WORK/mock.log" \
  GITLAB_TOKEN="test" \
  CI_API_V4_URL="https://gitlab.example/api/v4" \
  CI_PROJECT_ID="18" \
  CI_MERGE_REQUEST_IID="123" \
  FALLOW_COMMAND="check" \
  FALLOW_API_RETRIES=1 \
  FALLOW_API_RETRY_DELAY=0 \
  bash "$SCRIPTS_DIR/comment.sh" >/dev/null)
if /usr/bin/grep -q "fallow ci post-pr-comment --provider gitlab" "$CI_API_FAIL_WORK/mock.log"; then
  pass "comment.sh: delegates MR summary posting to Rust"
else
  fail "comment.sh: delegates MR summary posting to Rust" "$(cat "$CI_API_FAIL_WORK/mock.log")"
fi
if [ -f "$CI_API_FAIL_WORK/fallow-skip-reason.txt" ] && grep -q '^none$' "$CI_API_FAIL_WORK/fallow-skip-reason.txt"; then
  pass "comment.sh: leaves fallow-skip-reason.txt at none after Rust update"
else
  fail "comment.sh: leaves fallow-skip-reason.txt at none after Rust update" \
    "got: $(cat "$CI_API_FAIL_WORK/fallow-skip-reason.txt" 2>/dev/null || echo absent)"
fi
if [ -f "$CI_API_FAIL_WORK/fallow-dedup-lookup-failed.txt" ] && grep -q '^false$' "$CI_API_FAIL_WORK/fallow-dedup-lookup-failed.txt"; then
  pass "comment.sh: leaves fallow-dedup-lookup-failed.txt false"
else
  fail "comment.sh: leaves fallow-dedup-lookup-failed.txt false" \
    "got: $(cat "$CI_API_FAIL_WORK/fallow-dedup-lookup-failed.txt" 2>/dev/null || echo absent)"
fi

rm -rf "$CI_API_FAIL_WORK"

# --- IssueKind summary drift guard ---
#
# Same guard as the GitHub Action suite, run against every GitLab jq surface
# that carries the full dead-code set. A new dead-code IssueKind not wired into
# one of these would otherwise vanish silently from MR output. GitLab has no
# annotations / filter-changed surfaces, so all three are gated "all".
#
#   summary-check.jq      dead-code summary table
#   summary-combined.jq   combined-mode Code-issues breakdown
#   summary-audit.jq      audit dead_code_rows

echo ""
echo "=== IssueKind summary drift guard (GitLab) ==="

GUARD_DIR="$DIR/../../action/tests"
# shellcheck source=action/tests/issuekind-drift-guard.sh
. "$GUARD_DIR/issuekind-drift-guard.sh"
fallback_rows="$(
  FALLOW_BIN="$INSTALL_TMP/missing-fallow-binary"
  FALLOW_DEAD_CODE_SCHEMA_ROWS_CACHE="__unset__"
  fallow_dead_code_schema_rows
)"
assert_contains "$fallback_rows" $'unused-optional-dependency\tunused_optional_dependencies\ttrue' \
  "issuekind guard: source fallback includes optional dependencies"
assert_contains "$fallback_rows" $'boundary-coverage\tboundary_coverage_violations\ttrue' \
  "issuekind guard: source fallback includes boundary coverage"
assert_contains "$fallback_rows" $'boundary-call-violation\tboundary_call_violations\ttrue' \
  "issuekind guard: source fallback includes boundary call violations"
if issuekind_key_present '# .unused_files' "unused_files"; then
  fail "issuekind guard: comments do not satisfy key coverage" "comment-only jq source matched unused_files"
else
  pass "issuekind guard: comments do not satisfy key coverage"
fi
if issuekind_key_present 'true # .unused_files' "unused_files"; then
  fail "issuekind guard: inline comments do not satisfy key coverage" "inline comment matched unused_files"
else
  pass "issuekind guard: inline comments do not satisfy key coverage"
fi
if issuekind_key_present '["unused_files"] # rendered table row' "unused_files"; then
  pass "issuekind guard: string tokens still satisfy key coverage"
else
  fail "issuekind guard: string tokens still satisfy key coverage" "quoted key token did not match"
fi
assert_issuekind_summary_coverage "gitlab summary-check"    "$CI_JQ_DIR/summary-check.jq"
assert_issuekind_summary_table_contract "gitlab summary-check" "$CI_JQ_DIR/summary-check.jq"
assert_issuekind_summary_coverage "gitlab summary-combined" "$CI_JQ_DIR/summary-combined.jq"
assert_issuekind_summary_coverage "gitlab summary-audit"    "$CI_JQ_DIR/summary-audit.jq"



# --- Every variable the script reads is declared (issue #2693 review) ---
#
# The generated script runs under `set -euo pipefail`, so a `$FALLOW_X` that is
# not in the template's `variables:` block is unbound and kills the job on the
# first read. The rest of this suite cannot catch that: the fixture runner seeds
# every scraped name to empty, which is exactly the safety net production does
# not have.

echo ""
echo "Variable declarations"

UNDECLARED=""
DECLARED=$(awk '/^variables:/{f=1;next} /^[^ #]/{f=0} f' "$DIR/../gitlab-ci.yml" \
  | grep -oE '^  FALLOW_[A-Z0-9_]+' | tr -d ' ' | sort -u)
while IFS= read -r used; do
  [ -z "$used" ] && continue
  case "$used" in
    # Set by the script itself or by GitLab, never declared as an input.
    FALLOW_EXIT_CODE|FALLOW_RENDER_PATH_PREFIX_SET|FALLOW_SCRIPT_EOF|FALLOW_RUN_WRITER_EOF|FALLOW_TEST_LOG|FALLOW_TEST_ENV_FILE) continue ;;
  esac
  case "
$DECLARED
" in
    *"
$used
"*) ;;
    *) UNDECLARED="${UNDECLARED:+${UNDECLARED} }${used}" ;;
  esac
done < <(grep -oE '\$FALLOW_[A-Z0-9_]+' /tmp/fallow-run.sh | tr -d '$' | sort -u)

if [ -z "$UNDECLARED" ]; then
  pass "every \$FALLOW_* the generated script reads is declared in variables:"
else
  fail "every \$FALLOW_* the generated script reads is declared in variables:" \
    "undeclared, so set -u kills the job: $UNDECLARED"
fi

# --- Gate verdicts (issues #2680, #2681, #2683, #2685, #2686) ---

echo ""
echo "Gate verdicts"

GATE_WORK="$RUNNER_TMP/gate-verdicts"
mkdir -p "$GATE_WORK"

gitlab_gate_envelope() {
  local gates=$1 extra=${2:-} body
  body='"kind":"dead-code","schema_version":9,"version":"3.27.0","total_issues":0,"summary":{"total_issues":0},"unused_files":[],"unused_exports":[]'
  [ -n "$gates" ] && body="${body},\"gate_outcomes\":${gates}"
  [ -n "$extra" ] && body="${body},${extra}"
  printf '{%s}\n' "$body" > "$GATE_WORK/envelope.json"
  printf '%s' "$GATE_WORK/envelope.json"
}

# A gate the variable asked for fails the pipeline even with
# FALLOW_FAIL_ON_ISSUES false, which is the whole point of every issue here.
ENVELOPE=$(gitlab_gate_envelope '{"regression":{"status":"fail","enforced":true}}' '"regression":{"delta":4}')
OUT=$(run_generated_gitlab_fixture "$GATE_WORK" \
  MOCK_GATE_ENVELOPE="$ENVELOPE" \
  FALLOW_COMMAND=dead-code \
  FALLOW_FAIL_ON_ISSUES=false \
  FALLOW_FAIL_ON_REGRESSION=true \
  FALLOW_TOLERANCE=0) || true
assert_contains "$OUT" "ERROR: Fallow regression gate failed" \
  "gitlab gate: regression fails with FALLOW_FAIL_ON_ISSUES false"

# #2685: the security gate keeps exit 8 and used to sit inside the
# FALLOW_FAIL_ON_ISSUES conditional, where it could not be reached.
ENVELOPE=$(gitlab_gate_envelope '{"security":{"status":"fail","enforced":true}}' '"gate":{"mode":"new","verdict":"fail","new_count":2}')
set +e
OUT=$(run_generated_gitlab_fixture "$GATE_WORK" \
  MOCK_GATE_ENVELOPE="$ENVELOPE" \
  FALLOW_COMMAND=security \
  FALLOW_FAIL_ON_ISSUES=false \
  FALLOW_SECURITY_GATE=new)
GATE_STATUS=$?
set -e
assert_contains "$OUT" "ERROR: Fallow security gate failed" \
  "gitlab gate: security fails with FALLOW_FAIL_ON_ISSUES false"
if [ "$GATE_STATUS" = "8" ]; then
  pass "gitlab gate: security keeps exit 8"
else
  fail "gitlab gate: security keeps exit 8" "got $GATE_STATUS"
fi

# A gate nobody asked for reports and never fails.
ENVELOPE=$(gitlab_gate_envelope '{"regression":{"status":"fail","enforced":true}}')
set +e
OUT=$(run_generated_gitlab_fixture "$GATE_WORK" \
  MOCK_GATE_ENVELOPE="$ENVELOPE" \
  FALLOW_COMMAND=dead-code \
  FALLOW_FAIL_ON_ISSUES=false)
GATE_STATUS=$?
set -e
assert_contains "$OUT" "WARNING: Fallow regression gate reports a failure" \
  "gitlab gate: an unowned failure warns"
if [ "$GATE_STATUS" = "0" ]; then
  pass "gitlab gate: an unowned failure leaves the pipeline green"
else
  fail "gitlab gate: an unowned failure leaves the pipeline green" "got $GATE_STATUS"
fi

# A pinned binary older than the index still delivers the verdicts that had a
# feature-local field, and warns only about the gates that never had one.
ENVELOPE=$(gitlab_gate_envelope '' '"regression":{"exceeded":true,"delta":4}')
OUT=$(run_generated_gitlab_fixture "$GATE_WORK" \
  MOCK_GATE_ENVELOPE="$ENVELOPE" \
  FALLOW_COMMAND=dead-code \
  FALLOW_FAIL_ON_ISSUES=false \
  FALLOW_FAIL_ON_REGRESSION=true \
  FALLOW_TOLERANCE=0) || true
assert_contains "$OUT" "ERROR: Fallow regression gate failed" \
  "gitlab gate: the regression fallback reads .regression.exceeded"
ENVELOPE=$(gitlab_gate_envelope '')
OUT=$(run_generated_gitlab_fixture "$GATE_WORK" \
  MOCK_GATE_ENVELOPE="$ENVELOPE" \
  FALLOW_COMMAND=dead-code \
  FALLOW_FAIL_ON_ISSUES=false) || true
assert_not_contains "$OUT" "could not be checked" \
  "gitlab gate: a pinned binary warns about nothing the pipeline did not configure"

# #2686: one aggregated warning, and the empty case behind its own variable.
DEGRADED='"workspace_diagnostics":[{"path":"a","kind":"skipped-large-file","message":"m","degrades_analysis":true},{"path":"b","kind":"skipped-large-file","message":"m","degrades_analysis":true},{"path":".","kind":"boundaries-not-configured","message":"m"}]'
ENVELOPE=$(gitlab_gate_envelope '' "$DEGRADED")
OUT=$(run_generated_gitlab_fixture "$GATE_WORK" \
  MOCK_GATE_ENVELOPE="$ENVELOPE" \
  FALLOW_COMMAND=dead-code \
  FALLOW_FAIL_ON_ISSUES=false) || true
assert_contains "$OUT" "skipped-large-file (2)" "gitlab degraded: kinds and counts are aggregated"
assert_not_contains "$OUT" "boundaries-not-configured" "gitlab degraded: unconfigured-check kinds are not reported"
assert_contains "$OUT" "Fallow ran with degraded inputs" \
  "gitlab degraded: the sentence covers a degraded input as well as a narrower file set"

# #2689: the health pipeline's own degraded inputs reach the same aggregated
# line through the same selector, with no change to this template's jq.
HEALTH_DEGRADED='"workspace_diagnostics":[{"path":".","kind":"hotspots-skipped","message":"m","degrades_analysis":true},{"path":"coverage/coverage-final.json","kind":"coverage-auto-detected","message":"m"}]'
ENVELOPE=$(gitlab_gate_envelope '' "$HEALTH_DEGRADED")
OUT=$(run_generated_gitlab_fixture "$GATE_WORK" \
  MOCK_GATE_ENVELOPE="$ENVELOPE" \
  FALLOW_COMMAND=health \
  FALLOW_FAIL_ON_ISSUES=false) || true
assert_contains "$OUT" "hotspots-skipped (1)" \
  "gitlab degraded: the health kinds are reported without a template change"
assert_not_contains "$OUT" "coverage-auto-detected" \
  "gitlab degraded: auto-detected coverage is provenance and not a degraded run"

# #2736: a build config a framework plugin could not read reaches the same line
# through the same selector, and the quiet sibling kind stays out of it.
PLUGIN_DEGRADED='"workspace_diagnostics":[{"path":"module-federation.config.ts","kind":"plugin-config-unreadable","plugin":"module-federation","key":"exposes","reason":"not-object-literal","message":"m","degrades_analysis":true},{"path":"module-federation.config.ts","kind":"plugin-config-unreadable","plugin":"module-federation","key":"remotes","reason":"spread","message":"m","degrades_analysis":true},{"path":"nuxt.config.ts","kind":"plugin-effect-not-modeled","plugin":"nuxt","key":"imports","reason":"key-effect-not-modeled","message":"m"}]'
ENVELOPE=$(gitlab_gate_envelope '' "$PLUGIN_DEGRADED")
OUT=$(run_generated_gitlab_fixture "$GATE_WORK" \
  MOCK_GATE_ENVELOPE="$ENVELOPE" \
  FALLOW_COMMAND=dead-code \
  FALLOW_FAIL_ON_ISSUES=false) || true
assert_contains "$OUT" "plugin-config-unreadable (2)" \
  "gitlab degraded: two unreadable keys in one config are counted separately"
assert_not_contains "$OUT" "plugin-effect-not-modeled" \
  "gitlab degraded: a config whose effect is not modeled lost nothing measurable"

# #2687, #2688: this job runs fallow with --quiet and a machine format, so the
# envelope is the only channel that reaches the pipeline.
REQUESTS='"request_outcomes":{"changed-since":{"status":"not-applied","affects":"scope","requested":"origin/main","reason":"git-failed","message":"m"},"diff-filter":{"status":"applied","affects":"scope","requested":"--diff-stdin"}}'
ENVELOPE=$(gitlab_gate_envelope '' "$REQUESTS")
set +e
OUT=$(run_generated_gitlab_fixture "$GATE_WORK" \
  MOCK_GATE_ENVELOPE="$ENVELOPE" \
  FALLOW_COMMAND=dead-code \
  FALLOW_FAIL_ON_ISSUES=false)
GATE_STATUS=$?
set -e
assert_contains "$OUT" "WARNING: Fallow could not apply: changed-since (git-failed)" \
  "gitlab requests: an unapplied request warns once with its reason"
assert_not_contains "$OUT" "could not apply: changed-since (git-failed), diff-filter" \
  "gitlab requests: an honoured request is not named in the warning"
if [ "$GATE_STATUS" = "0" ]; then
  pass "gitlab requests: an unapplied request leaves the pipeline green"
else
  fail "gitlab requests: an unapplied request leaves the pipeline green" "got $GATE_STATUS"
fi

APPLIED='"request_outcomes":{"diff-filter":{"status":"applied","affects":"scope","requested":"--diff-stdin"}}'
ENVELOPE=$(gitlab_gate_envelope '' "$APPLIED")
OUT=$(run_generated_gitlab_fixture "$GATE_WORK" \
  MOCK_GATE_ENVELOPE="$ENVELOPE" \
  FALLOW_COMMAND=dead-code \
  FALLOW_FAIL_ON_ISSUES=false) || true
assert_not_contains "$OUT" "could not apply" \
  "gitlab requests: a run that applied everything it was asked stays silent"
assert_not_contains "$OUT" "empty scope" \
  "gitlab requests: an applied request that measured nothing is not called empty"

# #2734: an applied request over a scope it measured as empty. The unapplied
# line must stay clear of it while the advisory names it.
EMPTY_SCOPE='"request_outcomes":{"diff-filter":{"status":"applied","affects":"scope","requested":"--diff-file pr.diff","scope_size":0}}'
ENVELOPE=$(gitlab_gate_envelope '' "$EMPTY_SCOPE")
set +e
OUT=$(run_generated_gitlab_fixture "$GATE_WORK" \
  MOCK_GATE_ENVELOPE="$ENVELOPE" \
  FALLOW_COMMAND=dead-code \
  FALLOW_FAIL_ON_ISSUES=false)
GATE_STATUS=$?
set -e
assert_contains "$OUT" "WARNING: Fallow applied diff-filter over an empty scope" \
  "gitlab requests: an applied request over an empty scope is advised"
assert_not_contains "$OUT" "could not apply" \
  "gitlab requests: an empty scope is not reported as an unapplied request"
if [ "$GATE_STATUS" = "0" ]; then
  pass "gitlab requests: an empty scope leaves the pipeline green"
else
  fail "gitlab requests: an empty scope leaves the pipeline green" "got $GATE_STATUS"
fi

FULL_SCOPE='"request_outcomes":{"diff-filter":{"status":"applied","affects":"scope","requested":"--diff-file pr.diff","scope_size":12}}'
ENVELOPE=$(gitlab_gate_envelope '' "$FULL_SCOPE")
OUT=$(run_generated_gitlab_fixture "$GATE_WORK" \
  MOCK_GATE_ENVELOPE="$ENVELOPE" \
  FALLOW_COMMAND=dead-code \
  FALLOW_FAIL_ON_ISSUES=false) || true
assert_not_contains "$OUT" "empty scope" \
  "gitlab requests: a measured non-empty scope stays silent"

ENVELOPE=$(gitlab_gate_envelope '')
OUT=$(run_generated_gitlab_fixture "$GATE_WORK" \
  MOCK_GATE_ENVELOPE="$ENVELOPE" \
  FALLOW_COMMAND=dead-code \
  FALLOW_FAIL_ON_ISSUES=false) || true
assert_not_contains "$OUT" "could not apply" \
  "gitlab requests: a pinned binary that publishes no object warns about nothing"

# A request that writes a file beside the report narrows nothing.
ARTIFACT='"request_outcomes":{"sarif-file":{"status":"not-applied","affects":"artifact","requested":"gl-fallow.sarif","reason":"write-failed","message":"m"}}'
ENVELOPE=$(gitlab_gate_envelope '' "$ARTIFACT")
OUT=$(run_generated_gitlab_fixture "$GATE_WORK" \
  MOCK_GATE_ENVELOPE="$ENVELOPE" \
  FALLOW_COMMAND=dead-code \
  FALLOW_FAIL_ON_ISSUES=false) || true
assert_not_contains "$OUT" "could not apply" \
  "gitlab requests: an unwritten output file is not reported as an unscoped run"

EMPTY='"workspace_diagnostics":[{"path":".","kind":"no-source-files-analyzed","message":"m","excluded_file_count":3,"degrades_analysis":true}]'
ENVELOPE=$(gitlab_gate_envelope '' "$EMPTY")
set +e
OUT=$(run_generated_gitlab_fixture "$GATE_WORK" \
  MOCK_GATE_ENVELOPE="$ENVELOPE" \
  FALLOW_COMMAND=dead-code \
  FALLOW_FAIL_ON_ISSUES=false)
GATE_STATUS=$?
set -e
assert_contains "$OUT" "WARNING: Fallow analyzed no source file at all" "gitlab empty analysis: warns by default"
if [ "$GATE_STATUS" = "0" ]; then
  pass "gitlab empty analysis: passes by default"
else
  fail "gitlab empty analysis: passes by default" "got $GATE_STATUS"
fi
set +e
OUT=$(run_generated_gitlab_fixture "$GATE_WORK" \
  MOCK_GATE_ENVELOPE="$ENVELOPE" \
  FALLOW_COMMAND=dead-code \
  FALLOW_FAIL_ON_ISSUES=false \
  FALLOW_FAIL_ON_EMPTY_ANALYSIS=true)
GATE_STATUS=$?
set -e
assert_contains "$OUT" "ERROR: Fallow analyzed no source file at all" "gitlab empty analysis: fails behind the variable"
if [ "$GATE_STATUS" = "1" ]; then
  pass "gitlab empty analysis: exits 1 behind the variable"
else
  fail "gitlab empty analysis: exits 1 behind the variable" "got $GATE_STATUS"
fi

# #2682: --min-score implies --score, so the template keeps the reporting
# surfaces populated unless a health section variable is set.
ENVELOPE=$(gitlab_gate_envelope '')
FALLOW_TEST_LOG="$GATE_WORK/argv.log"
: > "$FALLOW_TEST_LOG"
run_generated_gitlab_fixture "$GATE_WORK" \
  MOCK_GATE_ENVELOPE="$ENVELOPE" \
  FALLOW_TEST_LOG="$FALLOW_TEST_LOG" \
  FALLOW_COMMAND=health \
  FALLOW_MIN_SCORE=90 \
  FALLOW_FAIL_ON_ISSUES=false > /dev/null 2>&1 || true
ARGV=$(cat "$FALLOW_TEST_LOG")
assert_contains "$ARGV" "--min-score 90" "gitlab gate: min-score reaches the CLI"
assert_contains "$ARGV" "--complexity" "gitlab gate: min-score adds --complexity"

rm -rf "$GATE_WORK"

# --- Summary ---

echo ""
echo "================================"
echo "  $PASSED passed, $FAILED failed"
echo "================================"

if [ "$FAILED" -gt 0 ]; then
  echo ""
  echo "Failures:"
  for err in "${ERRORS[@]}"; do
    echo "  ✗ $err"
  done
  exit 1
fi
