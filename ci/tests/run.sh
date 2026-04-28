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
fail() { FAILED=$((FAILED + 1)); ERRORS+=("$1: $2"); echo "  ✗ $1 — $2"; }

assert_contains() {
  local output="$1" expected="$2" name="$3"
  if echo "$output" | /usr/bin/grep -q "$expected" 2>/dev/null; then
    pass "$name"
  else
    fail "$name" "expected to contain: $expected"
  fi
}

assert_not_contains() {
  local output="$1" unexpected="$2" name="$3"
  if echo "$output" | /usr/bin/grep -q "$unexpected" 2>/dev/null; then
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

gitlab_install_script() {
  awk '
    /# Validate and install fallow/ { seen=1; next }
    seen && /^[[:space:]]*-[[:space:]]*\|[[:space:]]*$/ { in_block=1; next }
    in_block && /# Prepare jq scripts/ { exit }
    in_block {
      sub(/^      /, "")
      print
    }
  ' "$DIR/../gitlab-ci.yml"
}

GITLAB_INSTALL_SCRIPT="$(gitlab_install_script)"
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
  FALLOW_ROOT="$root" FALLOW_VERSION="$version" FALLOW_INSTALL_DRY_RUN=true bash -eo pipefail -c "$GITLAB_INSTALL_SCRIPT" 2>&1
}

OUT=$(run_gitlab_install "$INSTALL_TMP/pinned" "")
assert_contains "$OUT" "Using fallow version from" "install: reads package.json pin"
assert_contains "$OUT" "DRY RUN: npm install -g fallow@2.7.3" "install: installs project pin"

OUT=$(run_gitlab_install "$INSTALL_TMP/range" "")
assert_contains "$OUT" "DRY RUN: npm install -g fallow@^2.52.0" "install: supports package.json semver range"

OUT=$(run_gitlab_install "$INSTALL_TMP/pinned" "latest")
assert_contains "$OUT" "Using fallow version from FALLOW_VERSION: latest" "install: explicit FALLOW_VERSION wins"
assert_contains "$OUT" "DRY RUN: npm install -g fallow" "install: explicit latest installs latest"

OUT=$(run_gitlab_install "$INSTALL_TMP/unsafe" "")
assert_contains "$OUT" "Ignoring unsupported fallow package.json spec" "install: warns on unsupported package spec"
assert_contains "$OUT" "DRY RUN: npm install -g fallow" "install: unsupported package spec falls back to latest"

OUT=$(run_gitlab_install "$INSTALL_TMP/empty" "")
assert_contains "$OUT" "DRY RUN: npm install -g fallow" "install: no package spec falls back to latest"

OUT=$(run_gitlab_install "$INSTALL_TMP/empty" "2.0.0 - 2.5.0")
assert_contains "$OUT" "DRY RUN: npm install -g fallow@2.0.0 - 2.5.0" "install: supports npm hyphen ranges"

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
    bash -eo pipefail -c "$GITLAB_INSTALL_SCRIPT" 2>&1
}

extract_install_arg() {
  printf '%s\n' "$1" | grep -Eo 'DRY RUN: npm install -g .*' | head -n 1 \
    | sed 's/^DRY RUN: npm install -g //'
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
assert_not_contains "$OUT" '!\[NOTE\]' "no GitHub callout NOTE"
assert_not_contains "$OUT" '!\[WARNING\]' "no GitHub callout WARNING"
assert_not_contains "$OUT" '!\[TIP\]' "no GitHub callout TIP"

OUT_CLEAN=$(jq -r -f "$CI_JQ_DIR/summary-check.jq" "$FIXTURES/check-clean.json" 2>&1)
assert_contains "$OUT_CLEAN" "No issues found" "clean: shows no issues"

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
assert_contains "$OUT_CLEAN" "No issues found" "clean: no issues"
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
OUT_COMBINED_PROD=$(jq '.health.runtime_coverage = {"verdict":"hot-path-changes-needed","summary":{"functions_tracked":4,"functions_hit":3,"functions_unhit":0,"functions_untracked":1,"coverage_percent":75,"trace_count":2400,"period_days":7,"deployments_seen":2},"findings":[{"path":"src/cold.ts","function":"coldPath","line":14,"verdict":"review_required","invocations":0,"confidence":"medium"}],"hot_paths":[{"path":"src/hot.ts","function":"hotPath","line":3,"invocations":250,"percentile":99}]}' "$FIXTURES/combined-clean.json" | jq -r -f "$CI_JQ_DIR/summary-combined.jq" 2>&1)
assert_contains "$OUT_COMBINED_PROD" "Runtime coverage" "combined prod: has runtime coverage details"
assert_contains "$OUT_COMBINED_PROD" "hotPath" "combined prod: shows hot path"

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

OUT_CLEAN=$(jq -r -f "$SHARED_JQ_DIR/summary-dupes.jq" "$FIXTURES/dupes-clean.json" 2>&1)
assert_contains "$OUT_CLEAN" "No code duplication" "clean: no duplication"

echo "  summary-fix.jq:"
# summary-fix needs fix results — test with combined (may not have fix data)
# Just verify it doesn't crash on missing data
OUT=$(echo '{"fixes":[],"dry_run":true}' | jq -r -f "$SHARED_JQ_DIR/summary-fix.jq" 2>&1)
assert_contains "$OUT" "No fixable issues" "empty fix: no fixable issues"

# =========================================================================
# GitLab review comments (dupes variant with GitLab URLs)
# =========================================================================

echo ""
echo "=== GitLab Review comment scripts ==="

export PREFIX="website/" MAX=50 FALLOW_ROOT="website" CI_PROJECT_URL="https://gitlab.com/test/repo" CI_COMMIT_SHA="abc123"

echo "  review-comments-dupes.jq (GitLab):"
OUT=$(jq -f "$CI_JQ_DIR/review-comments-dupes.jq" "$FIXTURES/dupes.json" 2>&1)
assert_valid_json "$OUT" "produces valid JSON"
assert_contains "$OUT" "duplication" "mentions duplication"
assert_contains "$OUT" "gitlab.com" "has GitLab links (not GitHub)"
assert_not_contains "$OUT" "github.com" "no GitHub links"
assert_contains "$OUT" "View duplicated code" "includes code fragment"

# =========================================================================
# Shared review comment scripts (from action/jq/)
# =========================================================================

echo ""
echo "=== Shared Review comment scripts (from action/jq/) ==="

# Re-export env vars for shared jq scripts (they use GH_REPO etc. but we test with GitLab env)
export GH_REPO="" PR_NUMBER="" PR_HEAD_SHA=""

echo "  review-comments-check.jq:"
OUT=$(jq -f "$SHARED_JQ_DIR/review-comments-check.jq" "$FIXTURES/check.json" 2>&1)
assert_valid_json "$OUT" "produces valid JSON"
assert_contains "$OUT" "Unused" "contains unused findings"
assert_contains "$OUT" "@public" "mentions @public JSDoc tag"
assert_contains "$OUT" "docs.fallow.tools" "has docs links"
assert_contains "$OUT" "Configure or suppress" "has suppress link"
assert_contains "$OUT" "imported in another workspace" "dependency comment includes workspace context"
assert_contains "$OUT" "Move this dependency to the workspace that imports it" "dependency comment avoids unsafe remove hint"

OUT_CLEAN=$(jq -f "$SHARED_JQ_DIR/review-comments-check.jq" "$FIXTURES/check-clean.json" 2>&1)
assert_json_length "$OUT_CLEAN" "0" "clean: no comments"

echo "  review-comments-health.jq:"
OUT=$(jq -f "$SHARED_JQ_DIR/review-comments-health.jq" "$FIXTURES/health.json" 2>&1)
assert_valid_json "$OUT" "produces valid JSON"

OUT_PROD_REVIEW=$(jq '.runtime_coverage = {"verdict":"cold-code-detected","summary":{"functions_tracked":2,"functions_hit":1,"functions_unhit":1,"functions_untracked":0,"coverage_percent":50,"trace_count":1200,"period_days":7,"deployments_seen":2},"findings":[{"path":"src/cold.ts","function":"coldPath","line":14,"verdict":"review_required","invocations":0,"confidence":"medium","evidence":{"static_status":"used","test_coverage":"not_covered","v8_tracking":"tracked"},"actions":[{"description":"Review before deleting."}]}]}' "$FIXTURES/health-clean.json" | jq -f "$SHARED_JQ_DIR/review-comments-health.jq" 2>&1)
assert_valid_json "$OUT_PROD_REVIEW" "prod review comments: valid JSON"
assert_contains "$OUT_PROD_REVIEW" "coldPath" "prod review comments: function present"

echo "  review-body.jq:"
OUT=$(jq -r -f "$SHARED_JQ_DIR/review-body.jq" "$FIXTURES/combined.json" 2>&1)
assert_valid_markdown "$OUT" "produces output"
assert_contains "$OUT" "Fallow Review" "has review title"
assert_contains "$OUT" "fallow-review" "has marker comment"
assert_contains "$OUT" "Maintainability" "shows metrics"

OUT_REVIEW_PROD=$(jq '.health.runtime_coverage = {"verdict":"hot-path-changes-needed","summary":{"functions_tracked":4,"functions_hit":3,"functions_unhit":0,"functions_untracked":1,"coverage_percent":75,"trace_count":2400,"period_days":7,"deployments_seen":2},"findings":[{"path":"src/lazy.ts","function":"lateBound","line":8,"verdict":"coverage_unavailable","confidence":"none"}],"hot_paths":[{"path":"src/hot.ts","function":"hotPath","line":3,"invocations":250,"percentile":99}]}' "$FIXTURES/combined-clean.json" | jq -r -f "$SHARED_JQ_DIR/review-body.jq" 2>&1)
assert_contains "$OUT_REVIEW_PROD" "Runtime coverage:" "review body prod: summary line present"
assert_contains "$OUT_REVIEW_PROD" "hot path" "review body prod: hot path mentioned"

# =========================================================================
# Suggestion block tests
# =========================================================================

echo ""
echo "=== Suggestion blocks ==="

echo "  unused-export type field:"
OUT=$(jq -f "$SHARED_JQ_DIR/review-comments-check.jq" "$FIXTURES/check.json" 2>&1)
TYPES=$(echo "$OUT" | jq -r '[.[].type] | unique | join(",")')
assert_contains "$TYPES" "unused-export" "exports have type field for suggestion enrichment"

echo "  single export keeps type:"
SINGLE='{"total_issues":1,"unused_files":[],"unused_exports":[{"path":"x.ts","export_name":"foo","is_type_only":false,"line":5,"col":0,"span_start":0,"is_re_export":false}],"unused_types":[],"unused_dependencies":[],"unused_dev_dependencies":[],"unused_optional_dependencies":[],"unused_enum_members":[],"unused_class_members":[],"unresolved_imports":[],"unlisted_dependencies":[],"duplicate_exports":[],"circular_dependencies":[],"boundary_violations":[],"type_only_dependencies":[]}'
OUT=$(echo "$SINGLE" | jq -f "$SHARED_JQ_DIR/review-comments-check.jq" 2>&1)
assert_json_length "$OUT" "1" "single export produces 1 comment"
SINGLE_TYPE=$(echo "$OUT" | jq -r '.[0].type')
[ "$SINGLE_TYPE" = "unused-export" ] && pass "type is unused-export (not grouped)" || fail "type is unused-export" "got $SINGLE_TYPE"

echo "  grouped exports get different type:"
MULTI='{"total_issues":2,"unused_files":[],"unused_exports":[{"path":"x.ts","export_name":"foo","is_type_only":false,"line":5,"col":0,"span_start":0,"is_re_export":false},{"path":"x.ts","export_name":"bar","is_type_only":false,"line":10,"col":0,"span_start":0,"is_re_export":false}],"unused_types":[],"unused_dependencies":[],"unused_dev_dependencies":[],"unused_optional_dependencies":[],"unused_enum_members":[],"unused_class_members":[],"unresolved_imports":[],"unlisted_dependencies":[],"duplicate_exports":[],"circular_dependencies":[],"boundary_violations":[],"type_only_dependencies":[]}'
OUT=$(echo "$MULTI" | jq -f "$SHARED_JQ_DIR/review-comments-check.jq" | jq --argjson max 50 -f "$SHARED_JQ_DIR/merge-comments.jq" 2>&1)
assert_json_length "$OUT" "1" "2 exports from same file grouped into 1"
GROUP_TYPE=$(echo "$OUT" | jq -r '.[0].type')
[ "$GROUP_TYPE" = "unused-export-group" ] && pass "grouped type is unused-export-group" || fail "grouped type" "got $GROUP_TYPE"
assert_contains "$OUT" "2 unused exports" "grouped comment mentions count"

echo "  boundary violation produces review comment:"
BV_INPUT='{"total_issues":1,"unused_files":[],"unused_exports":[],"unused_types":[],"unused_dependencies":[],"unused_dev_dependencies":[],"unused_optional_dependencies":[],"unused_enum_members":[],"unused_class_members":[],"unresolved_imports":[],"unlisted_dependencies":[],"duplicate_exports":[],"circular_dependencies":[],"boundary_violations":[{"from_path":"src/ui/App.ts","to_path":"src/db/query.ts","from_zone":"ui","to_zone":"db","import_specifier":"src/db/query.ts","line":5,"col":9}],"type_only_dependencies":[]}'
OUT=$(echo "$BV_INPUT" | MAX=50 jq -f "$SHARED_JQ_DIR/review-comments-check.jq" 2>&1)
assert_valid_json "$OUT" "boundary violation JSON valid"
assert_json_length "$OUT" "1" "boundary violation produces 1 comment"
assert_contains "$OUT" "Boundary violation" "comment mentions boundary violation"
assert_contains "$OUT" "ui" "comment mentions from_zone"
assert_contains "$OUT" "db" "comment mentions to_zone"
assert_contains "$OUT" "src/ui/App.ts" "comment mentions from_path"
assert_contains "$OUT" "src/db/query.ts" "comment mentions to_path"
BV_PATH=$(echo "$OUT" | jq -r '.[0].path')
[ "$BV_PATH" = "${PREFIX}src/ui/App.ts" ] && pass "path has prefix + from_path" || fail "path has prefix + from_path" "got $BV_PATH"
BV_LINE=$(echo "$OUT" | jq -r '.[0].line')
[ "$BV_LINE" = "5" ] && pass "line is 5" || fail "line is 5" "got $BV_LINE"

echo "  boundary violation appears in summary:"
SUMMARY=$(echo "$BV_INPUT" | jq -rf "$CI_JQ_DIR/summary-check.jq" 2>&1)
assert_contains "$SUMMARY" "Boundary violations" "summary has boundary section"
assert_contains "$SUMMARY" "src/ui/App.ts" "summary mentions file"
assert_contains "$SUMMARY" "ui" "summary mentions zone"

echo "  review-body clean state:"
OUT_CLEAN=$(jq -r -f "$SHARED_JQ_DIR/review-body.jq" "$FIXTURES/combined-clean.json" 2>&1)
assert_contains "$OUT_CLEAN" "No code issues" "clean: no code issues"
assert_contains "$OUT_CLEAN" "No duplication" "clean: no duplication"
assert_contains "$OUT_CLEAN" "fallow-review" "clean: has marker"

# =========================================================================
# Merge script tests (shared from action/jq/)
# =========================================================================

echo ""
echo "=== Merge script ==="

echo "  merge-comments.jq:"

# Test grouping unused exports
EXPORTS='[
  {"type":"unused-export","export_name":"foo","path":"a.ts","line":1,"body":"unused foo"},
  {"type":"unused-export","export_name":"bar","path":"a.ts","line":5,"body":"unused bar"},
  {"type":"unused-export","export_name":"baz","path":"b.ts","line":1,"body":"unused baz"},
  {"type":"other","path":"c.ts","line":1,"body":"something else"}
]'
OUT=$(echo "$EXPORTS" | jq --argjson max 50 -f "$SHARED_JQ_DIR/merge-comments.jq" 2>&1)
assert_valid_json "$OUT" "valid JSON"
assert_json_length "$OUT" "3" "groups 2 exports from a.ts into 1 (2 + 1 other = 3)"
assert_contains "$OUT" "2 unused exports" "grouped comment mentions count"
assert_contains "$OUT" "foo" "grouped comment lists export names"
assert_contains "$OUT" "bar" "grouped comment lists export names"

# Test dedup clones
CLONES='[
  {"type":"duplication","group_id":"g1","path":"a.ts","line":5,"body":"clone 1 instance 1"},
  {"type":"duplication","group_id":"g1","path":"a.ts","line":20,"body":"clone 1 instance 2"},
  {"type":"duplication","group_id":"g2","path":"b.ts","line":10,"body":"clone 2 instance 1"},
  {"type":"duplication","group_id":"g2","path":"b.ts","line":30,"body":"clone 2 instance 2"}
]'
OUT=$(echo "$CLONES" | jq --argjson max 50 -f "$SHARED_JQ_DIR/merge-comments.jq" 2>&1)
assert_valid_json "$OUT" "valid JSON"
assert_json_length "$OUT" "2" "deduplicates to 1 per clone group (4 → 2)"

# Test drop refactoring targets
TARGETS='[
  {"type":"other","path":"a.ts","line":1,"body":"finding"},
  {"type":"refactoring-target","path":"a.ts","line":1,"body":"target"}
]'
OUT=$(echo "$TARGETS" | jq --argjson max 50 -f "$SHARED_JQ_DIR/merge-comments.jq" 2>&1)
assert_json_length "$OUT" "1" "drops refactoring targets"
assert_not_contains "$OUT" "target" "target body is removed"

# Test merge same line
SAME_LINE='[
  {"type":"other","path":"a.ts","line":5,"body":"complexity warning"},
  {"type":"other","path":"a.ts","line":5,"body":"unused export warning"}
]'
OUT=$(echo "$SAME_LINE" | jq --argjson max 50 -f "$SHARED_JQ_DIR/merge-comments.jq" 2>&1)
assert_json_length "$OUT" "1" "merges same-line comments"
assert_contains "$OUT" "complexity warning" "merged comment has first body"
assert_contains "$OUT" "unused export warning" "merged comment has second body"
assert_contains "$OUT" "\\\\n---\\\\n" "merged comment has separator"

# Test empty input
OUT=$(echo '[]' | jq --argjson max 50 -f "$SHARED_JQ_DIR/merge-comments.jq" 2>&1)
assert_json_length "$OUT" "0" "empty input produces empty output"

# Test max limit
MANY='[
  {"type":"other","path":"a.ts","line":1,"body":"1"},
  {"type":"other","path":"a.ts","line":2,"body":"2"},
  {"type":"other","path":"a.ts","line":3,"body":"3"},
  {"type":"other","path":"a.ts","line":4,"body":"4"},
  {"type":"other","path":"a.ts","line":5,"body":"5"}
]'
OUT=$(echo "$MANY" | jq --argjson max 3 -f "$SHARED_JQ_DIR/merge-comments.jq" 2>&1)
assert_json_length "$OUT" "3" "respects max limit"

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

echo "  verify GitLab dupes links use CI_PROJECT_URL:"
if /usr/bin/grep -q 'CI_PROJECT_URL' "$CI_JQ_DIR/review-comments-dupes.jq" 2>/dev/null; then
  pass "review-comments-dupes.jq uses CI_PROJECT_URL"
else
  fail "review-comments-dupes.jq" "missing CI_PROJECT_URL reference"
fi

if /usr/bin/grep -q 'GH_REPO' "$CI_JQ_DIR/review-comments-dupes.jq" 2>/dev/null; then
  fail "review-comments-dupes.jq" "still references GH_REPO"
else
  pass "review-comments-dupes.jq has no GH_REPO reference"
fi

# =========================================================================
# GitLab CI YAML structure tests
# =========================================================================

echo ""
echo "=== GitLab CI YAML structure ==="

CI_YAML="$DIR/../gitlab-ci.yml"

echo "  gitlab-ci.yml:"
assert_contains "$(cat "$CI_YAML")" "FALLOW_REVIEW" "has FALLOW_REVIEW variable"
assert_contains "$(cat "$CI_YAML")" "FALLOW_MAX_COMMENTS" "has FALLOW_MAX_COMMENTS variable"
assert_contains "$(cat "$CI_YAML")" "FALLOW_COMMENT" "has FALLOW_COMMENT variable"
assert_contains "$(cat "$CI_YAML")" "FALLOW_CODEQUALITY" "has FALLOW_CODEQUALITY variable"
assert_contains "$(cat "$CI_YAML")" "project_fallow_spec" "reads package.json fallow pin"
assert_contains "$(cat "$CI_YAML")" "is_safe_version_spec" "validates fallow install spec"
assert_contains "$(cat "$CI_YAML")" "FALLOW_INSTALL_DRY_RUN" "supports install dry-run testing"
assert_contains "$(cat "$CI_YAML")" "GIT_STRATEGY" "overrides shared template git strategy"
assert_contains "$(cat "$CI_YAML")" "GIT_DEPTH" "fetches full history for changed-since"
assert_contains "$(cat "$CI_YAML")" "CI_MERGE_REQUEST_DIFF_BASE_SHA" "auto changed-since uses diff base SHA"
assert_contains "$(cat "$CI_YAML")" "comment.sh" "references comment.sh"
assert_contains "$(cat "$CI_YAML")" "review.sh" "references review.sh"
assert_contains "$(cat "$CI_YAML")" "gl-code-quality-report" "generates Code Quality report"
assert_contains "$(cat "$CI_YAML")" "suggestion" "mentions suggestion blocks in docs"

# =========================================================================
# Bash script structure tests
# =========================================================================

echo ""
echo "=== Bash script structure ==="

SCRIPTS_DIR="$DIR/../scripts"

echo "  comment.sh:"
assert_contains "$(cat "$SCRIPTS_DIR/comment.sh")" "PRIVATE-TOKEN" "supports GITLAB_TOKEN"
assert_contains "$(cat "$SCRIPTS_DIR/comment.sh")" "CI_JOB_TOKEN is read-only" "explains CI_JOB_TOKEN write limitation"
assert_contains "$(cat "$SCRIPTS_DIR/comment.sh")" "fallow-results" "uses fallow-results marker"
assert_contains "$(cat "$SCRIPTS_DIR/comment.sh")" "PUT" "can update existing comment"
assert_contains "$(cat "$SCRIPTS_DIR/comment.sh")" "POST" "can create new comment"

echo "  review.sh:"
assert_contains "$(cat "$SCRIPTS_DIR/review.sh")" "discussions" "uses GitLab Discussions API"
assert_contains "$(cat "$SCRIPTS_DIR/review.sh")" "position" "posts with position for inline comments"
assert_contains "$(cat "$SCRIPTS_DIR/review.sh")" "suggestion" "adds suggestion blocks"
assert_contains "$(cat "$SCRIPTS_DIR/review.sh")" "merge-comments" "runs merge pipeline"
assert_contains "$(cat "$SCRIPTS_DIR/review.sh")" "fallow-review" "uses fallow-review marker"
assert_contains "$(cat "$SCRIPTS_DIR/review.sh")" "DELETE" "cleans up previous comments"
assert_contains "$(cat "$SCRIPTS_DIR/review.sh")" "unused-export" "handles unused export suggestions"
assert_contains "$(cat "$SCRIPTS_DIR/review.sh")" "FALLOW_SHARED_JQ_DIR" "can use shared jq scripts"

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
