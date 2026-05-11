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

gitlab_install_script() {
  awk '
    /# Validate and install fallow/ { seen=1; next }
    seen && /^[[:space:]]*-[[:space:]]*\|[[:space:]]*$/ { in_block=1; next }
    in_block && /# Prepare bash scripts/ { exit }
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

echo "  summary-audit.jq (GitLab):"
OUT_AUDIT=$(jq -n --slurpfile h "$FIXTURES/health.json" --slurpfile c "$FIXTURES/check.json" --slurpfile d "$FIXTURES/dupes.json" '{
  schema_version: 3,
  command: "audit",
  verdict: "fail",
  changed_files_count: 2,
  elapsed_ms: 42,
  summary: {dead_code_issues: 1, complexity_findings: 3, duplication_clone_groups: 1},
  attribution: {gate: "new-only", dead_code_introduced: 1, dead_code_inherited: 0, complexity_introduced: 2, complexity_inherited: 1, duplication_introduced: 0, duplication_inherited: 1},
  dead_code: ($c[0] | .unused_exports |= map(. + {introduced: true}) | .unused_dependencies |= map(. + {introduced: false})),
  complexity: ($h[0]
    | .findings |= [.[0] + {coverage_tier: "partial"}, .[1] + {coverage_tier: "high"}, .[2]]
    | .summary.coverage_model = "istanbul"
    | .summary.istanbul_matched = 8
    | .summary.istanbul_total = 10),
  duplication: ($d[0] | .clone_groups |= map(. + {introduced: false}))
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
assert_not_contains "$OUT_AUDIT" '!\[WARNING\]' "audit: no GitHub callout warning"

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
OUT_SINGULAR_GL=$(jq '.check.unused_files = [.check.unused_files[0]] | .check.unused_exports = [] | .check.unused_dependencies = [] | .check.unused_dev_dependencies = [] | .check.unused_optional_dependencies = [] | .check.unused_types = [] | .check.unused_enum_members = [] | .check.unused_class_members = [] | .check.unresolved_imports = [] | .check.unlisted_dependencies = [] | .check.duplicate_exports = [] | .check.circular_dependencies = [] | .check.boundary_violations = [] | .check.type_only_dependencies = [] | .check.test_only_dependencies = [] | .check.stale_suppressions = [] | .check.unused_catalog_entries = [] | .check.private_type_leaks = [] | .check.total_issues = 1 | .dupes.stats.clone_groups = 1 | .dupes.clone_groups = [.dupes.clone_groups[0]] | .health.summary.functions_above_threshold = 1 | .health.findings = [.health.findings[0]]' "$FIXTURES/combined.json" | jq -r -f "$CI_JQ_DIR/summary-combined.jq" 2>&1)
assert_contains "$OUT_SINGULAR_GL" "**1** code issue " "status-bar: singular code issue"
assert_not_contains "$OUT_SINGULAR_GL" "**1** code issues" "status-bar: no '1 code issues' grammar"
assert_contains "$OUT_SINGULAR_GL" "**1** clone group " "status-bar: singular clone group"
assert_not_contains "$OUT_SINGULAR_GL" "**1** clone groups" "status-bar: no '1 clone groups' grammar"
assert_not_contains "$OUT_SINGULAR_GL" "**1** health findings" "status-bar: no '1 health findings' grammar"

# Complexity <details> summary pluralizes when functions_above_threshold == 1
assert_contains "$OUT_SINGULAR_GL" "(1 function above threshold)" "complexity dropdown: singular function"
assert_not_contains "$OUT_SINGULAR_GL" "(1 functions above threshold)" "complexity dropdown: no '1 functions' grammar"

# Worst-case truncation: 50 groups (paths differentiated per-group via `. as $g |`),
# top-5 + overflow line, output stays under 65k chars.
# line_count is ASCENDING in input order so the sort_by in summary-combined.jq must do work.
OUT_LARGE_GL=$(jq -n '
  {
    schema_version: 3,
    check: {total_issues: 0, unused_files: [], unused_exports: [], unused_types: [], unused_dependencies: [], unused_dev_dependencies: [], unused_optional_dependencies: [], unused_enum_members: [], unused_class_members: [], unresolved_imports: [], unlisted_dependencies: [], duplicate_exports: [], circular_dependencies: [], boundary_violations: [], type_only_dependencies: [], test_only_dependencies: [], stale_suppressions: [], unused_catalog_entries: [], private_type_leaks: []},
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
OUT_COMBINED_PROD=$(jq '.health.runtime_coverage = {"verdict":"hot-path-touched","summary":{"functions_tracked":4,"functions_hit":3,"functions_unhit":0,"functions_untracked":1,"coverage_percent":75,"trace_count":2400,"period_days":7,"deployments_seen":2},"findings":[{"path":"src/cold.ts","function":"coldPath","line":14,"verdict":"review_required","invocations":0,"confidence":"medium"}],"hot_paths":[{"path":"src/hot.ts","function":"hotPath","line":3,"invocations":250,"percentile":99}]}' "$FIXTURES/combined-clean.json" | jq -r -f "$CI_JQ_DIR/summary-combined.jq" 2>&1)
assert_contains "$OUT_COMBINED_PROD" "Runtime coverage" "combined prod: has runtime coverage details"
assert_contains "$OUT_COMBINED_PROD" "hotPath" "combined prod: shows hot path"
assert_contains "$OUT_COMBINED_PROD" "hot path touched" "combined prod (GitLab, verdict hot-path-touched): header uses 'touched' framing"

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
# summary-fix needs fix results — test with combined (may not have fix data)
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
assert_contains "$(cat "$CI_YAML")" 'type == "array"' "preserves valid Code Quality reports from nonzero audit exits"
assert_contains "$(cat "$CI_YAML")" '.error == true' "fails on structured fallow error JSON"
assert_contains "$(cat "$CI_YAML")" "does not support FALLOW_BASELINE/FALLOW_SAVE_BASELINE" "audit rejects generic baseline variables"
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
assert_contains "$(cat "$SCRIPTS_DIR/comment.sh")" "curl_retry" "wraps GitLab API calls with retry"
assert_contains "$(cat "$SCRIPTS_DIR/comment.sh")" "rate limit response; retrying" "retries GitLab rate-limit responses"

echo "  review.sh:"
assert_contains "$(cat "$SCRIPTS_DIR/review.sh")" "review-gitlab" "renders typed GitLab review envelope"
assert_contains "$(cat "$SCRIPTS_DIR/review.sh")" "fallow ci reconcile-review" "reconciles resolved discussions"
assert_contains "$(cat "$SCRIPTS_DIR/review.sh")" "--provider gitlab" "uses GitLab reconcile provider"
assert_contains "$(cat "$SCRIPTS_DIR/review.sh")" "discussions" "uses GitLab Discussions API"
assert_contains "$(cat "$SCRIPTS_DIR/review.sh")" "position" "posts with position for inline comments"
assert_contains "$(cat "$SCRIPTS_DIR/review.sh")" "suggestion" "adds suggestion blocks"
assert_contains "$(cat "$SCRIPTS_DIR/review.sh")" "fallow-review" "uses fallow-review marker"
assert_contains "$(cat "$SCRIPTS_DIR/review.sh")" "fallow-fingerprint" "deduplicates by typed fingerprint"
assert_contains "$(cat "$SCRIPTS_DIR/review.sh")" "curl_retry" "wraps GitLab API calls with retry"
assert_contains "$(cat "$SCRIPTS_DIR/review.sh")" "rate limit response; retrying" "retries GitLab rate-limit responses"
assert_not_contains "$(cat "$SCRIPTS_DIR/review.sh")" "merge-comments" "does not keep legacy jq merge fallback"
assert_not_contains "$(cat "$SCRIPTS_DIR/review.sh")" "FALLOW_SHARED_JQ_DIR" "does not use shared jq fallback scripts"

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
if [ "${1:-}" = "ci" ]; then
  printf '{"schema":"fallow-review-reconcile/v1","stale":[]}\n'
  exit 0
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
    printf '<!-- fallow-id: fallow-results -->\n### Fallow smoke\n\nGenerated by fallow.\n'
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

printf 'FALLOW_ANALYSIS_ARGS=(check --format json --root .)\n' > "$CI_TYPED_WORK/fallow-analysis-args.sh"
(
  cd "$CI_TYPED_WORK"
  PATH="$CI_TYPED_BIN:$PATH" \
    MOCK_LOG="$CI_TYPED_LOG" \
    GITLAB_TOKEN="test" \
    CI_API_V4_URL="https://gitlab.example/api/v4" \
    CI_PROJECT_ID="18" \
    CI_MERGE_REQUEST_IID="123" \
    FALLOW_COMMAND="check" \
    bash "$SCRIPTS_DIR/comment.sh" > /dev/null
  PATH="$CI_TYPED_BIN:$PATH" \
    MOCK_LOG="$CI_TYPED_LOG" \
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
)
CI_TYPED_OUT=$(cat "$CI_TYPED_LOG")
assert_contains "$CI_TYPED_OUT" "--format pr-comment-gitlab" "comment.sh invokes typed MR comment format"
assert_contains "$CI_TYPED_OUT" "--format review-gitlab" "review.sh invokes typed GitLab review format"
assert_contains "$CI_TYPED_OUT" "fallow ci reconcile-review --provider gitlab" "review.sh invokes GitLab reconcile command"
assert_contains "$CI_TYPED_OUT" "merge_requests/123/discussions" "review.sh posts discussion payload"
assert_contains "$CI_TYPED_OUT" "merge_requests/123/notes/777" "review.sh updates existing body-only review note"
rm -rf "$CI_TYPED_WORK"

# =========================================================================
# curl_paginate Link-header walk: confirms multi-page concatenation
# =========================================================================
#
# The single-page short-circuit is exercised indirectly by every typed-
# integration test above (the mock returns a single-page body with no Link
# header). This block exercises the multi-page path explicitly: page 1
# returns content + `link: <URL>; rel="next"`, page 2 returns content
# without a Link header. curl_paginate must visit both URLs and concatenate
# the two arrays into one.
echo ""
echo "=== curl_paginate Link-header walk ==="

# Extract curl_paginate at top level (outside any nested $()) so the awk
# pattern parses cleanly, then define paginate_test_run as a regular
# function and capture its output once. Disable pipefail just for the test
# run because curl_paginate uses `url=$(grep | tr | sed | head -1)` and
# `head -1` SIGPIPE-cancels the upstream pipeline on the no-Link-header
# page, which under pipefail propagates as a non-zero exit.
PAGINATE_FN_SRC=$(awk '/^curl_paginate\(\) \{/,/^\}$/' "$SCRIPTS_DIR/comment.sh")
eval "$PAGINATE_FN_SRC"

paginate_test_run() {
  set +o pipefail
  PAGINATE_HITS=0
  curl_retry() {
    local args=("$@")
    local headers_file=""
    local i
    for ((i=0; i<${#args[@]}; i++)); do
      if [ "${args[$i]}" = "-D" ] && [ $((i+1)) -lt ${#args[@]} ]; then
        headers_file="${args[$((i+1))]}"
      fi
    done
    local last_idx=$(( ${#args[@]} - 1 ))
    local url="${args[$last_idx]}"
    PAGINATE_HITS=$((PAGINATE_HITS + 1))
    case "$url" in
      *page=2*)
        : > "$headers_file"
        printf '[{"id":2,"body":"second"}]'
        ;;
      *)
        printf 'link: <https://example.test/api/notes?page=2>; rel="next"\n' \
          > "$headers_file"
        printf '[{"id":1,"body":"first"}]'
        ;;
    esac
  }

  curl_paginate --header "PRIVATE-TOKEN: t" \
    "https://example.test/api/notes?page=1&per_page=100"
  printf '\nHITS=%d' "$PAGINATE_HITS"
}

PAGINATE_TEST_OUT=$(paginate_test_run)

assert_contains "$PAGINATE_TEST_OUT" '"first"' "curl_paginate captures page 1 body"
assert_contains "$PAGINATE_TEST_OUT" '"second"' "curl_paginate follows Link rel=next to page 2"
assert_contains "$PAGINATE_TEST_OUT" "HITS=2" "curl_paginate stops after page 2 (no Link header)"

# Strip the trailing "\nHITS=N" tail before piping the array body to jq.
PAGINATE_BODY="${PAGINATE_TEST_OUT%$'\n'HITS=*}"
PAGINATE_LEN=$(printf '%s' "$PAGINATE_BODY" | jq 'length' 2>/dev/null || echo 0)
if [ "$PAGINATE_LEN" = "2" ]; then
  pass "curl_paginate concatenates pages into a single array of length 2"
else
  fail "curl_paginate concatenates pages into a single array of length 2" \
    "got length $PAGINATE_LEN"
fi

# Defensive non-array safety: a 401 / 403 envelope ({"message":"Unauthorized"})
# returned mid-walk must NOT crash the helper. The defensive
# `jq -s 'map(arrays) | add // []'` skips non-array pages.
paginate_defensive_run() {
  set +o pipefail
  curl_retry() {
    local args=("$@")
    local headers_file=""
    local i
    for ((i=0; i<${#args[@]}; i++)); do
      if [ "${args[$i]}" = "-D" ] && [ $((i+1)) -lt ${#args[@]} ]; then
        headers_file="${args[$((i+1))]}"
      fi
    done
    : > "$headers_file"
    printf '{"message":"401 Unauthorized"}'
  }
  curl_paginate --header "PRIVATE-TOKEN: t" "https://example.test/api/notes"
}

PAGINATE_DEFENSIVE_OUT=$(paginate_defensive_run)
assert_contains "$PAGINATE_DEFENSIVE_OUT" "[]" \
  "curl_paginate returns empty array when API returns non-array error envelope"

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
