#!/usr/bin/env bash
# Test suite for fallow GitHub Action jq scripts and bash helpers
# Run: bash action/tests/run.sh

set -o pipefail

DIR="$(cd "$(dirname "$0")" && pwd)"
JQ_DIR="$DIR/../jq"
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

assert_json_value() {
  local output="$1" jq_expr="$2" expected="$3" name="$4"
  local actual
  actual=$(echo "$output" | jq -r "$jq_expr" 2>/dev/null)
  if [ "$actual" = "$expected" ]; then
    pass "$name"
  else
    fail "$name" "expected $expected, got $actual"
  fi
}

# --- Install script tests ---

echo ""
echo "=== Install script ==="

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

OUT=$(INPUT_ROOT="$INSTALL_TMP/pinned" FALLOW_VERSION="" FALLOW_INSTALL_DRY_RUN=true bash "$DIR/../scripts/install.sh" 2>&1)
assert_contains "$OUT" "Using fallow version from" "install: reads package.json pin"
assert_contains "$OUT" "DRY RUN: npm install -g fallow@2.7.3" "install: installs project pin"

OUT=$(INPUT_ROOT="$INSTALL_TMP/range" FALLOW_VERSION="" FALLOW_INSTALL_DRY_RUN=true bash "$DIR/../scripts/install.sh" 2>&1)
assert_contains "$OUT" "DRY RUN: npm install -g fallow@^2.52.0" "install: supports package.json semver range"

OUT=$(INPUT_ROOT="$INSTALL_TMP/empty" FALLOW_VERSION="2.52.0 - 2.53.0" FALLOW_INSTALL_DRY_RUN=true bash "$DIR/../scripts/install.sh" 2>&1)
assert_contains "$OUT" "DRY RUN: npm install -g fallow@2.52.0 - 2.53.0" "install: supports npm hyphen ranges"

OUT=$(INPUT_ROOT="$INSTALL_TMP/pinned" FALLOW_VERSION="latest" FALLOW_INSTALL_DRY_RUN=true bash "$DIR/../scripts/install.sh" 2>&1)
assert_contains "$OUT" "Using fallow version from action input: latest" "install: explicit version wins"
assert_contains "$OUT" "DRY RUN: npm install -g fallow" "install: explicit latest installs latest"

OUT=$(INPUT_ROOT="$INSTALL_TMP/unsafe" FALLOW_VERSION="" FALLOW_INSTALL_DRY_RUN=true bash "$DIR/../scripts/install.sh" 2>&1)
assert_contains "$OUT" "Ignoring unsupported fallow package.json spec" "install: warns on unsupported package spec"
assert_contains "$OUT" "DRY RUN: npm install -g fallow" "install: unsupported package spec falls back to latest"

OUT=$(INPUT_ROOT="$INSTALL_TMP/empty" FALLOW_VERSION="" FALLOW_INSTALL_DRY_RUN=true bash "$DIR/../scripts/install.sh" 2>&1)
assert_contains "$OUT" "DRY RUN: npm install -g fallow" "install: no package spec falls back to latest"

OUT=$(INPUT_ROOT="$INSTALL_TMP/empty" FALLOW_VERSION="file:../fallow" FALLOW_INSTALL_DRY_RUN=true bash "$DIR/../scripts/install.sh" 2>&1)
cmd_status=$?
if [ "$cmd_status" -ne 0 ]; then
  pass "install: invalid explicit spec fails"
else
  fail "install: invalid explicit spec fails" "expected non-zero exit"
fi
assert_contains "$OUT" "Invalid version specifier" "install: invalid explicit spec explains failure"

OUT=$(INPUT_ROOT="$INSTALL_TMP/empty" FALLOW_VERSION="2.0.0 -g malicious" FALLOW_INSTALL_DRY_RUN=true bash "$DIR/../scripts/install.sh" 2>&1)
cmd_status=$?
if [ "$cmd_status" -ne 0 ]; then
  pass "install: rejects dash-prefixed extra args in spec"
else
  fail "install: rejects dash-prefixed extra args in spec" "expected non-zero exit"
fi

echo ""
echo "=== Analyze script failure handling ==="

ANALYZE_TMP=$(mktemp -d)
trap 'rm -rf "$INSTALL_TMP" "$ANALYZE_TMP"' EXIT
mkdir -p "$ANALYZE_TMP/bin" "$ANALYZE_TMP/work"
cat > "$ANALYZE_TMP/bin/fallow" <<'SH'
#!/usr/bin/env bash
printf '%s\n' '{"error":true,"message":"bad audit config","exit_code":2}'
exit 2
SH
chmod +x "$ANALYZE_TMP/bin/fallow"

OUT=$(cd "$ANALYZE_TMP/work" && PATH="$ANALYZE_TMP/bin:$PATH" GITHUB_OUTPUT="$ANALYZE_TMP/output" INPUT_ROOT="." INPUT_COMMAND="audit" INPUT_FORMAT="json" bash "$DIR/../scripts/analyze.sh" 2>&1)
cmd_status=$?
if [ "$cmd_status" -eq 2 ]; then
  pass "analyze: structured fallow errors fail"
else
  fail "analyze: structured fallow errors fail" "expected exit 2, got $cmd_status"
fi
assert_contains "$OUT" "bad audit config" "analyze: surfaces structured error message"

OUT=$(cd "$ANALYZE_TMP/work" && PATH="$ANALYZE_TMP/bin:$PATH" GITHUB_OUTPUT="$ANALYZE_TMP/output" INPUT_ROOT="." INPUT_COMMAND="audit" INPUT_FORMAT="json" INPUT_BASELINE="baseline.json" bash "$DIR/../scripts/analyze.sh" 2>&1)
cmd_status=$?
if [ "$cmd_status" -eq 2 ]; then
  pass "analyze: audit rejects generic baseline input"
else
  fail "analyze: audit rejects generic baseline input" "expected exit 2, got $cmd_status"
fi
assert_contains "$OUT" "dead-code-baseline" "analyze: baseline error points to audit baselines"

# Audit verdict + gate are emitted to GITHUB_OUTPUT for the Check threshold step.
# Without this, the threshold step gates on raw introduced count, re-introducing
# the issue #302 bug where warn-tier findings fail CI.
cat > "$ANALYZE_TMP/bin/fallow" <<'SH'
#!/usr/bin/env bash
# Synthesize an audit JSON with verdict=warn, dead_code_introduced=1.
# Mimics the warn-tier scenario from issue #302: a project with
# `unused-exports: warn` has a PR introducing a new unused export.
case "$*" in
  *audit*)
    printf '%s\n' '{"command":"audit","verdict":"warn","attribution":{"gate":"new-only","dead_code_introduced":1,"dead_code_inherited":0,"complexity_introduced":0,"complexity_inherited":0,"duplication_introduced":0,"duplication_inherited":0},"summary":{"dead_code_issues":1,"dead_code_has_errors":false,"complexity_findings":0,"max_cyclomatic":null,"duplication_clone_groups":0}}'
    ;;
  *) printf '{"total_issues":0}\n' ;;
esac
SH
chmod +x "$ANALYZE_TMP/bin/fallow"

cd "$ANALYZE_TMP/work" && rm -f "$ANALYZE_TMP/output"
OUT=$(PATH="$ANALYZE_TMP/bin:$PATH" GITHUB_OUTPUT="$ANALYZE_TMP/output" \
  INPUT_ROOT="." INPUT_COMMAND="audit" INPUT_FORMAT="json" \
  bash "$DIR/../scripts/analyze.sh" 2>&1) || true
cd "$DIR"
VERDICT=$(grep '^verdict=' "$ANALYZE_TMP/output" | cut -d= -f2)
GATE=$(grep '^gate=' "$ANALYZE_TMP/output" | cut -d= -f2)
ISSUES=$(grep '^issues=' "$ANALYZE_TMP/output" | cut -d= -f2)
[ "$VERDICT" = "warn" ] && pass "analyze: emits verdict to GITHUB_OUTPUT for audit" || fail "analyze: verdict output" "expected warn, got '$VERDICT'"
[ "$GATE" = "new-only" ] && pass "analyze: emits gate to GITHUB_OUTPUT for audit" || fail "analyze: gate output" "expected new-only, got '$GATE'"
[ "$ISSUES" = "1" ] && pass "analyze: still emits issues count for audit" || fail "analyze: issues output" "expected 1, got '$ISSUES'"

# Non-audit commands must NOT emit verdict / gate (empty values are fine).
cat > "$ANALYZE_TMP/bin/fallow" <<'SH'
#!/usr/bin/env bash
case "$*" in
  *dead-code*) printf '{"total_issues":3}\n' ;;
  *) printf '{"check":{"total_issues":3}}\n' ;;
esac
SH
chmod +x "$ANALYZE_TMP/bin/fallow"
cd "$ANALYZE_TMP/work" && rm -f "$ANALYZE_TMP/output"
OUT=$(PATH="$ANALYZE_TMP/bin:$PATH" GITHUB_OUTPUT="$ANALYZE_TMP/output" \
  INPUT_ROOT="." INPUT_COMMAND="dead-code" INPUT_FORMAT="json" \
  bash "$DIR/../scripts/analyze.sh" 2>&1) || true
cd "$DIR"
VERDICT=$(grep '^verdict=' "$ANALYZE_TMP/output" | cut -d= -f2)
[ -z "$VERDICT" ] && pass "analyze: verdict empty for non-audit command" || fail "analyze: non-audit verdict" "expected empty, got '$VERDICT'"

# --- Summary jq tests ---

echo ""
echo "=== Summary scripts ==="

echo "  summary-check.jq:"
OUT=$(jq -r -f "$JQ_DIR/summary-check.jq" "$FIXTURES/check.json" 2>&1)
assert_valid_markdown "$OUT" "produces output"
assert_contains "$OUT" "Fallow Analysis" "has title"
assert_contains "$OUT" "issues" "mentions issues"
assert_contains "$OUT" "Unused" "lists unused categories"
assert_contains "$OUT" "Imported elsewhere" "shows dependency workspace context column"
assert_contains "$OUT" 'packages/client' "shows dependency workspace context value"

OUT_CLEAN=$(jq -r -f "$JQ_DIR/summary-check.jq" "$FIXTURES/check-clean.json" 2>&1)
assert_contains "$OUT_CLEAN" "No issues found" "clean: shows no issues"
assert_not_contains "$OUT_CLEAN" "WARNING" "clean: no warning"

echo "  summary-dupes.jq:"
OUT=$(jq -r -f "$JQ_DIR/summary-dupes.jq" "$FIXTURES/dupes.json" 2>&1)
assert_valid_markdown "$OUT" "produces output"
assert_contains "$OUT" "clone groups" "mentions clone groups"
assert_contains "$OUT" "Duplicated lines" "shows duplication stats"
assert_contains "$OUT" "content-parser.ts:27-50" "shows clone instance line range"

OUT_CLEAN=$(jq -r -f "$JQ_DIR/summary-dupes.jq" "$FIXTURES/dupes-clean.json" 2>&1)
assert_contains "$OUT_CLEAN" "No code duplication" "clean: no duplication"

# clone_groups bullet branch (no clone_families): line ranges per group
OUT_GROUPS=$(jq '.clone_families = []' "$FIXTURES/dupes.json" | jq -r -f "$JQ_DIR/summary-dupes.jq" 2>&1)
assert_contains "$OUT_GROUPS" "content-parser.ts:27-50" "groups branch: shows line range"
assert_contains "$OUT_GROUPS" "24 lines, 125 tokens" "groups branch: shows lines/tokens lead"

# Null duplication_percentage must not crash the standalone summary
OUT_DUPES_NULL_PCT=$(jq 'del(.stats.duplication_percentage)' "$FIXTURES/dupes.json" | jq -r -f "$JQ_DIR/summary-dupes.jq" 2>&1)
assert_contains "$OUT_DUPES_NULL_PCT" "66 / 478 (0%)" "summary-dupes: missing duplication_percentage renders as 0%"
assert_not_contains "$OUT_DUPES_NULL_PCT" "cannot be multiplied" "summary-dupes: null does not crash"

echo "  summary-health.jq:"
OUT=$(jq -r -f "$JQ_DIR/summary-health.jq" "$FIXTURES/health.json" 2>&1)
assert_valid_markdown "$OUT" "produces output"
assert_contains "$OUT" "Severity" "severity column header present"
assert_contains "$OUT" "critical" "critical severity in table"
assert_contains "$OUT" "high" "high severity in table"
assert_contains "$OUT" "moderate" "moderate severity in table"

OUT_CLEAN=$(jq -r -f "$JQ_DIR/summary-health.jq" "$FIXTURES/health-clean.json" 2>&1)
assert_contains "$OUT_CLEAN" "No functions exceed" "clean: no functions exceed"

echo "  summary-health.jq (delta header with trend):"
assert_contains "$OUT" "Health: B (72.3)" "delta: shows grade and score"
assert_contains "$OUT" "+7.2 pts vs previous" "delta: shows score delta"
assert_contains "$OUT" "C 65.1" "delta: shows previous grade and score"
assert_contains "$OUT" "dead exports 41.2%" "delta: shows dead export pct"
assert_contains "$OUT" "(-3.8%)" "delta: shows dead export delta"
assert_contains "$OUT" "avg complexity 7.1 (-1.2)" "delta: shows complexity delta"

echo "  summary-health.jq (delta header without trend):"
assert_contains "$OUT_CLEAN" "Health: A (92.5)" "no-trend: shows absolute score"
assert_not_contains "$OUT_CLEAN" "vs previous" "no-trend: no delta line"
assert_contains "$OUT_CLEAN" "save-snapshot: true" "no-trend: shows save-snapshot hint"

echo "  summary-health.jq (no delta header without score):"
OUT_NO_SCORE=$(jq 'del(.health_score) | del(.health_trend)' "$FIXTURES/health.json" | jq -r -f "$JQ_DIR/summary-health.jq" 2>&1)
assert_not_contains "$OUT_NO_SCORE" "Health:" "no-score: no delta header"

echo "  summary-health.jq (runtime coverage findings and hot paths):"
OUT_PROD=$(jq '.runtime_coverage = {"verdict":"cold-code-detected","summary":{"functions_tracked":4,"functions_hit":2,"functions_unhit":1,"functions_untracked":1,"coverage_percent":50,"trace_count":1200,"period_days":7,"deployments_seen":2},"findings":[{"path":"src/cold.ts","function":"coldPath","line":14,"verdict":"review_required","invocations":0,"confidence":"medium"},{"path":"src/lazy.ts","function":"lateBound","line":8,"verdict":"coverage_unavailable","confidence":"none"}],"hot_paths":[{"path":"src/hot.ts","function":"hotPath","line":3,"invocations":250,"percentile":99}]}' "$FIXTURES/health-clean.json" | jq -r -f "$JQ_DIR/summary-health.jq" 2>&1)
assert_contains "$OUT_PROD" "Runtime Coverage" "prod: has runtime coverage section"
assert_contains "$OUT_PROD" "review_required" "prod: shows production verdict"
assert_contains "$OUT_PROD" "Hot Paths" "prod: has hot paths section"
assert_contains "$OUT_PROD" "hotPath" "prod: shows hot path function"

echo "  summary-audit.jq:"
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
}' | jq -r -f "$JQ_DIR/summary-audit.jq" 2>&1)
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

# Low match-rate variant: footer should warn about --coverage-root
OUT_AUDIT_LOWMATCH=$(jq -n --slurpfile h "$FIXTURES/health.json" '{
  schema_version: 3, command: "audit", verdict: "fail", changed_files_count: 2, elapsed_ms: 42,
  summary: {dead_code_issues: 0, complexity_findings: 3, duplication_clone_groups: 0},
  attribution: {gate: "new-only", dead_code_introduced: 0, dead_code_inherited: 0, complexity_introduced: 3, complexity_inherited: 0, duplication_introduced: 0, duplication_inherited: 0},
  complexity: ($h[0] | .summary.coverage_model = "istanbul" | .summary.istanbul_matched = 1 | .summary.istanbul_total = 10)
}' | jq -r -f "$JQ_DIR/summary-audit.jq" 2>&1)
assert_contains "$OUT_AUDIT_LOWMATCH" "Low match rate" "audit: low match rate flags --coverage-root"

# Static-estimate variant: footer should suggest --coverage
OUT_AUDIT_STATIC=$(jq -n --slurpfile h "$FIXTURES/health.json" --slurpfile c "$FIXTURES/check.json" --slurpfile d "$FIXTURES/dupes.json" '{
  schema_version: 3, command: "audit", verdict: "fail", changed_files_count: 2, elapsed_ms: 42,
  summary: {dead_code_issues: 0, complexity_findings: 3, duplication_clone_groups: 0},
  attribution: {gate: "new-only", dead_code_introduced: 0, dead_code_inherited: 0, complexity_introduced: 3, complexity_inherited: 0, duplication_introduced: 0, duplication_inherited: 0},
  complexity: ($h[0] | .summary.coverage_model = "static_estimated")
}' | jq -r -f "$JQ_DIR/summary-audit.jq" 2>&1)
assert_contains "$OUT_AUDIT_STATIC" "Coverage model: static (estimated)" "audit: static-estimate footer suggests --coverage"
assert_contains "$OUT_AUDIT_STATIC" "for measured coverage" "audit: static branch reworded"

# Absent-model variant: footer should not be present at all
OUT_AUDIT_NOMODEL=$(jq -n --slurpfile h "$FIXTURES/health.json" '{
  schema_version: 3, command: "audit", verdict: "fail", changed_files_count: 2, elapsed_ms: 42,
  summary: {dead_code_issues: 0, complexity_findings: 3, duplication_clone_groups: 0},
  attribution: {gate: "new-only", dead_code_introduced: 0, dead_code_inherited: 0, complexity_introduced: 3, complexity_inherited: 0, duplication_introduced: 0, duplication_inherited: 0},
  complexity: ($h[0] | del(.summary.coverage_model))
}' | jq -r -f "$JQ_DIR/summary-audit.jq" 2>&1)
assert_not_contains "$OUT_AUDIT_NOMODEL" "Coverage model:" "audit: absent coverage_model omits footer"

echo "  summary-combined.jq:"
OUT=$(jq -r -f "$JQ_DIR/summary-combined.jq" "$FIXTURES/combined.json" 2>&1)
assert_valid_markdown "$OUT" "produces output"
assert_contains "$OUT" "Fallow" "has title"
assert_contains "$OUT" "code issues" "mentions code issues"
assert_contains "$OUT" "Maintainability" "shows vital signs"

assert_contains "$OUT" "Codebase health" "has codebase health header"
assert_contains "$OUT" "CRAP" "combined: shows CRAP column"
assert_contains "$OUT" "thresholds: cyclomatic" "combined: shows complexity threshold line"

# Duplication block: locations table replaces metric-only table
assert_contains "$OUT" "Locations | Lines | Tokens" "dupes: locations table header"
assert_contains "$OUT" "content-parser.ts:27-50" "dupes: shows first clone instance line range"
assert_contains "$OUT" "content-parser.ts:168-191" "dupes: shows second clone instance line range"
assert_contains "$OUT" "Across 2 files" "dupes: footer reports file count"
assert_contains "$OUT" "2 groups · 66 lines" "dupes: header carries group count and total lines"
assert_not_contains "$OUT" "| [Duplicated lines]" "dupes: old metric table is gone"
assert_not_contains "$OUT" "| Files with clones | 2 |" "dupes: old files-with-clones row is gone"

# Linkified cells engage when GH_REPO + PR_HEAD_SHA are set
OUT_LINKED=$(GH_REPO="fallow-rs/fallow" PR_HEAD_SHA="abcdef1234567890" jq -r -f "$JQ_DIR/summary-combined.jq" "$FIXTURES/combined.json" 2>&1)
assert_contains "$OUT_LINKED" "https://github.com/fallow-rs/fallow/blob/abcdef1234567890/src/helpers/content-parser.ts#L27-L50" "dupes: file_link engages with env vars"

# Deep paths (>3 segments): display is rel_path-truncated but URL keeps the full path
OUT_DEEP=$(jq '.dupes.clone_groups = [{line_count: 10, token_count: 50, instances: [{file: "apps/web/src/services/billing/calculator.ts", start_line: 5, end_line: 15}, {file: "apps/api/src/services/billing/calculator.ts", start_line: 8, end_line: 18}]}] | .dupes.stats.clone_groups = 1 | .dupes.stats.files_with_clones = 2' "$FIXTURES/combined.json" | GH_REPO="fallow-rs/fallow" PR_HEAD_SHA="deadbeef" jq -r -f "$JQ_DIR/summary-combined.jq" 2>&1)
# Display truncates to last 3 segments
assert_contains "$OUT_DEEP" "\`services/billing/calculator.ts:5-15\`" "deep-path: display uses rel_path"
# URL must contain the FULL path including 'apps/web/' prefix, otherwise the link 404s
assert_contains "$OUT_DEEP" "/blob/deadbeef/apps/web/src/services/billing/calculator.ts#L5-L15" "deep-path: URL keeps full path"
assert_contains "$OUT_DEEP" "/blob/deadbeef/apps/api/src/services/billing/calculator.ts#L8-L18" "deep-path: URL keeps full path (sibling)"

# Singular-group header: 1 group renders "group" not "groups"
OUT_ONE=$(jq '.dupes.stats.clone_groups = 1 | .dupes.clone_groups = [.dupes.clone_groups[0]]' "$FIXTURES/combined.json" | jq -r -f "$JQ_DIR/summary-combined.jq" 2>&1)
assert_contains "$OUT_ONE" "(1 group ·" "dupes: singular group header"
assert_not_contains "$OUT_ONE" "(1 groups ·" "dupes: no '1 groups' grammar"

# Status-bar pluralization: 1 of each renders singular
OUT_SINGULAR=$(jq '.check.unused_files = [.check.unused_files[0]] | .check.unused_exports = [] | .check.unused_dependencies = [] | .check.unused_dev_dependencies = [] | .check.unused_optional_dependencies = [] | .check.unused_types = [] | .check.unused_enum_members = [] | .check.unused_class_members = [] | .check.unresolved_imports = [] | .check.unlisted_dependencies = [] | .check.duplicate_exports = [] | .check.circular_dependencies = [] | .check.boundary_violations = [] | .check.type_only_dependencies = [] | .check.test_only_dependencies = [] | .check.stale_suppressions = [] | .check.private_type_leaks = [] | .check.total_issues = 1 | .dupes.stats.clone_groups = 1 | .dupes.clone_groups = [.dupes.clone_groups[0]] | .health.summary.functions_above_threshold = 1 | .health.findings = [.health.findings[0]]' "$FIXTURES/combined.json" | jq -r -f "$JQ_DIR/summary-combined.jq" 2>&1)
assert_contains "$OUT_SINGULAR" "**1** code issue " "status-bar: singular code issue"
assert_not_contains "$OUT_SINGULAR" "**1** code issues" "status-bar: no '1 code issues' grammar"
assert_contains "$OUT_SINGULAR" "**1** clone group " "status-bar: singular clone group"
assert_not_contains "$OUT_SINGULAR" "**1** clone groups" "status-bar: no '1 clone groups' grammar"
assert_not_contains "$OUT_SINGULAR" "**1** health findings" "status-bar: no '1 health findings' grammar"

# Complexity <details> summary pluralizes when functions_above_threshold == 1
assert_contains "$OUT_SINGULAR" "(1 function above threshold)" "complexity dropdown: singular function"
assert_not_contains "$OUT_SINGULAR" "(1 functions above threshold)" "complexity dropdown: no '1 functions' grammar"

# Worst-case truncation: 50 groups synthesized (paths differentiated per-group via `. as $g |`),
# top-5 displayed + "and N more" line, total under 65k chars.
# line_count is ASCENDING in input order (group_0 has line_count=1, group_49 has line_count=50)
# so the sort_by + reverse in summary-combined.jq must actually do work to surface the largest
# groups. If the sort is reverted, group_0 (smallest) would lead and the regression assertions fail.
OUT_LARGE=$(jq -n '
  {
    schema_version: 3,
    check: {total_issues: 0, unused_files: [], unused_exports: [], unused_types: [], unused_dependencies: [], unused_dev_dependencies: [], unused_optional_dependencies: [], unused_enum_members: [], unused_class_members: [], unresolved_imports: [], unlisted_dependencies: [], duplicate_exports: [], circular_dependencies: [], boundary_violations: [], type_only_dependencies: [], test_only_dependencies: [], stale_suppressions: [], private_type_leaks: []},
    dupes: {
      stats: {clone_groups: 50, clone_instances: 200, files_with_clones: 50, duplicated_lines: 5000, total_lines: 100000, duplication_percentage: 5.0},
      clone_groups: ([range(0;50)] | map(. as $g | {line_count: ($g + 1), token_count: ($g * 5 + 50), instances: ([range(0;4)] | map(. as $i | {file: ("src/group_\($g)/file_\($i).ts"), start_line: ($i * 10 + 1), end_line: ($i * 10 + 9)}))}))
    },
    health: {summary: {functions_above_threshold: 0}, vital_signs: {}, file_scores: [], findings: []}
  }
' | jq -r -f "$JQ_DIR/summary-combined.jq" 2>&1)
assert_contains "$OUT_LARGE" "and 45 more groups" "dupes: large input truncates with overflow line"
assert_contains "$OUT_LARGE" "Across 50 files" "dupes: large input footer count is correct"
LARGE_LEN=${#OUT_LARGE}
if [ "$LARGE_LEN" -lt 65000 ]; then
  pass "dupes: large input stays under GitHub PR comment cap (got $LARGE_LEN chars)"
else
  fail "dupes: large input over PR comment cap" "got $LARGE_LEN chars (cap 65000)"
fi
# Top-5 sort order: largest line_count first. group_49 has line_count=50, group_45=46, group_44=45 is just outside top-5.
# This assertion fails if sort_by is reverted: input order would put group_0 (line_count=1) first.
assert_contains "$OUT_LARGE" "src/group_49/file_0.ts:1-9" "dupes: largest group (49) ranks first after sort"
assert_contains "$OUT_LARGE" "src/group_45/file_0.ts" "dupes: top-5 contains group_45 (5th largest)"
assert_not_contains "$OUT_LARGE" "src/group_44/file_0.ts" "dupes: group_44 (6th largest) is truncated"
assert_not_contains "$OUT_LARGE" "src/group_0/file_0.ts" "dupes: smallest group is truncated"

# Null duplication_percentage must not crash pct(); render as 0%
OUT_NULL_PCT=$(jq 'del(.dupes.stats.duplication_percentage)' "$FIXTURES/combined.json" | jq -r -f "$JQ_DIR/summary-combined.jq" 2>&1)
assert_contains "$OUT_NULL_PCT" "66 lines · 0%" "dupes: missing duplication_percentage renders as 0%"
assert_not_contains "$OUT_NULL_PCT" "cannot be multiplied" "dupes: pct(null) does not crash"

assert_not_contains "$OUT" "Dead exports" "no dead_export_pct in PR comment"

OUT_CRAP_ONLY=$(jq '.health.summary.functions_above_threshold = 1 | .health.findings = [{"path":"src/ui/pagination.tsx","name":"buildPageItems","line":42,"col":0,"cyclomatic":17,"cognitive":8,"crap":30,"line_count":13,"severity":"moderate","exceeded":"crap"}]' "$FIXTURES/combined.json" | jq -r -f "$JQ_DIR/summary-combined.jq" 2>&1)
assert_contains "$OUT_CRAP_ONLY" "buildPageItems" "combined: renders CRAP-only finding"
assert_contains "$OUT_CRAP_ONLY" "CRAP >= 30" "combined: explains CRAP threshold"

OUT_CRAP_SORT=$(jq '.health.summary.functions_above_threshold = 6 | .health.findings = [
  {"path":"src/a.ts","name":"cyclo1","line":1,"col":0,"cyclomatic":80,"cognitive":4,"line_count":10,"severity":"critical","exceeded":"cyclomatic"},
  {"path":"src/a.ts","name":"cyclo2","line":2,"col":0,"cyclomatic":70,"cognitive":4,"line_count":10,"severity":"critical","exceeded":"cyclomatic"},
  {"path":"src/a.ts","name":"cyclo3","line":3,"col":0,"cyclomatic":60,"cognitive":4,"line_count":10,"severity":"critical","exceeded":"cyclomatic"},
  {"path":"src/a.ts","name":"cyclo4","line":4,"col":0,"cyclomatic":50,"cognitive":4,"line_count":10,"severity":"critical","exceeded":"cyclomatic"},
  {"path":"src/a.ts","name":"cyclo5","line":5,"col":0,"cyclomatic":40,"cognitive":4,"line_count":10,"severity":"high","exceeded":"cyclomatic"},
  {"path":"src/a.ts","name":"crapOnly","line":6,"col":0,"cyclomatic":8,"cognitive":4,"crap":30,"line_count":10,"severity":"moderate","exceeded":"crap"}
]' "$FIXTURES/combined.json" | jq -r -f "$JQ_DIR/summary-combined.jq" 2>&1)
assert_contains "$OUT_CRAP_SORT" "crapOnly" "combined: severity sort surfaces CRAP-only finding in visible rows"

OUT_OLD_HEALTH=$(jq 'del(.health.summary.max_cyclomatic_threshold) | del(.health.summary.max_cognitive_threshold) | del(.health.summary.max_crap_threshold) | .health.findings = [{"path":"src/a.ts","name":"legacyComplex","line":1,"col":0,"cyclomatic":25,"cognitive":20,"line_count":10,"severity":"moderate","exceeded":"both"}]' "$FIXTURES/combined.json" | jq -r -f "$JQ_DIR/summary-combined.jq" 2>&1)
assert_contains "$OUT_OLD_HEALTH" "thresholds: cyclomatic > default, cognitive > default" "combined: old JSON threshold fallback is explicit"
assert_not_contains "$OUT_OLD_HEALTH" "CRAP" "combined: old JSON without CRAP metadata hides CRAP column"

echo "  summary-combined.jq (scoped maintainability):"
# Simulate --changed-since filtering: keep only 1 file_score (76.2) vs codebase avg (86.8)
OUT_SCOPED=$(jq '.health.file_scores = [.health.file_scores[0]]' "$FIXTURES/combined.json" | jq -r -f "$JQ_DIR/summary-combined.jq" 2>&1)
assert_contains "$OUT_SCOPED" "changed files" "scoped: shows changed files maintainability row"
assert_contains "$OUT_SCOPED" "76.2" "scoped: shows scoped maintainability value"
assert_contains "$OUT_SCOPED" "86.8" "scoped: still shows codebase maintainability"

echo "  summary-combined.jq (no scoped row when unfiltered):"
assert_not_contains "$OUT" "changed files" "unfiltered: no scoped maintainability row"

echo "  summary-combined.jq (conditional tips):"
# Fixture has unused_exports and unused_dependencies → fix tip + @public tip
assert_contains "$OUT" "fallow fix --dry-run" "tip: shows fix tip when fixable issues present"
assert_contains "$OUT" "@public" "tip: shows @public tip when unused exports present"
# Remove fixable categories → no tip block
OUT_NO_FIX=$(jq '.check.unused_exports = [] | .check.unused_dependencies = [] | .check.unused_enum_members = [] | .check.circular_dependencies = [{"files":["a.ts","b.ts"],"length":2}] | .check.total_issues = 1' "$FIXTURES/combined.json" | jq -r -f "$JQ_DIR/summary-combined.jq" 2>&1)
assert_not_contains "$OUT_NO_FIX" "fallow fix" "tip: no fix tip when no fixable issues"
assert_not_contains "$OUT_NO_FIX" "@public" "tip: no @public tip when no unused exports"

echo "  summary-combined.jq (clean state):"
OUT_CLEAN=$(jq -r -f "$JQ_DIR/summary-combined.jq" "$FIXTURES/combined-clean.json" 2>&1)
assert_contains "$OUT_CLEAN" "No issues found" "clean: no issues"
assert_contains "$OUT_CLEAN" "Maintainability" "clean: shows maintainability"

echo "  summary-combined.jq (delta header with trend):"
assert_contains "$OUT" "Health: B (72.3)" "delta: shows grade and score"
assert_contains "$OUT" "+7.2 pts vs previous" "delta: shows score delta"
assert_contains "$OUT" "C 65.1" "delta: shows previous grade and score"
assert_contains "$OUT" "dead exports 41.2%" "delta: shows dead export pct"
assert_contains "$OUT" "avg complexity 7.1 (-1.2)" "delta: shows complexity delta"

echo "  summary-combined.jq (delta header without trend):"
assert_contains "$OUT_CLEAN" "Health: A (92.5)" "clean+score: shows absolute score"
assert_not_contains "$OUT_CLEAN" "vs previous" "clean+score: no delta when no trend"
assert_contains "$OUT_CLEAN" "save-snapshot: true" "clean+score: shows save-snapshot hint"

echo "  summary-combined.jq (no delta header without score):"
OUT_NO_SCORE=$(jq 'del(.health.health_score) | del(.health.health_trend)' "$FIXTURES/combined.json" | jq -r -f "$JQ_DIR/summary-combined.jq" 2>&1)
assert_not_contains "$OUT_NO_SCORE" "Health:" "no-score: no delta header"

echo "  summary-combined.jq (delta header with increasing dead exports shows suppress link):"
OUT_WORSE=$(jq '.health.health_trend.metrics[1].delta = 5.0 | .health.health_trend.metrics[1].current = 50.0' "$FIXTURES/combined.json" | jq -r -f "$JQ_DIR/summary-combined.jq" 2>&1)
assert_contains "$OUT_WORSE" "suppress?" "worsening: shows suppress link when dead exports increase"

echo "  summary-combined.jq (runtime coverage details):"
OUT_COMBINED_PROD=$(jq '.health.runtime_coverage = {"verdict":"hot-path-changes-needed","summary":{"functions_tracked":4,"functions_hit":3,"functions_unhit":0,"functions_untracked":1,"coverage_percent":75,"trace_count":2400,"period_days":7,"deployments_seen":2},"findings":[{"path":"src/cold.ts","function":"coldPath","line":14,"verdict":"review_required","invocations":0,"confidence":"medium"}],"hot_paths":[{"path":"src/hot.ts","function":"hotPath","line":3,"invocations":250,"percentile":99}]}' "$FIXTURES/combined-clean.json" | jq -r -f "$JQ_DIR/summary-combined.jq" 2>&1)
assert_contains "$OUT_COMBINED_PROD" "Runtime coverage" "combined prod: has runtime coverage details"
assert_contains "$OUT_COMBINED_PROD" "hotPath" "combined prod: shows hot path"

# --- Annotation jq tests ---

echo ""
echo "=== Annotation scripts ==="

echo "  annotations-check.jq:"
OUT=$(jq -r -f "$JQ_DIR/annotations-check.jq" "$FIXTURES/check.json" 2>&1)
assert_contains "$OUT" "::warning" "emits warning commands"
assert_contains "$OUT" "file=" "has file references"
assert_contains "$OUT" "title=" "has titles"
assert_contains "$OUT" "Imported in other workspaces" "dependency annotation includes workspace context"
assert_contains "$OUT" "Move this dependency to the consuming workspace package.json" "dependency annotation avoids unsafe remove hint"

OUT_CLEAN=$(jq -r -f "$JQ_DIR/annotations-check.jq" "$FIXTURES/check-clean.json" 2>&1)
[ -z "$OUT_CLEAN" ] && pass "clean: no annotations" || fail "clean: no annotations" "got output"

echo "  annotations-dupes.jq:"
OUT=$(jq -r -f "$JQ_DIR/annotations-dupes.jq" "$FIXTURES/dupes.json" 2>&1)
assert_contains "$OUT" "::warning" "emits warning commands"
assert_contains "$OUT" "Code duplication" "mentions duplication"

echo "  annotations-health.jq:"
OUT=$(jq -r -f "$JQ_DIR/annotations-health.jq" "$FIXTURES/health.json" 2>&1)
assert_contains "$OUT" "::error" "critical finding emits ::error annotation"
assert_contains "$OUT" "::warning" "high/moderate findings emit ::warning annotation"
assert_contains "$OUT" "(critical)" "critical severity in annotation title"
assert_contains "$OUT" "(high)" "high severity in annotation title"
assert_contains "$OUT" "parseContentBlocks" "includes function name"

OUT_PROD_ANN=$(jq '.runtime_coverage = {"verdict":"cold-code-detected","summary":{"functions_tracked":2,"functions_hit":1,"functions_unhit":1,"functions_untracked":0,"coverage_percent":50,"trace_count":1200,"period_days":7,"deployments_seen":2},"findings":[{"path":"src/cold.ts","function":"coldPath","line":14,"verdict":"review_required","invocations":0,"confidence":"medium","evidence":{"static_status":"used","test_coverage":"not_covered","v8_tracking":"tracked"},"actions":[{"description":"Review before deleting."}]},{"path":"src/lazy.ts","function":"lateBound","line":8,"verdict":"coverage_unavailable","confidence":"none","evidence":{"static_status":"used","test_coverage":"not_covered","v8_tracking":"untracked","untracked_reason":"lazy_parsed"}}]}' "$FIXTURES/health-clean.json" | jq -r -f "$JQ_DIR/annotations-health.jq" 2>&1)
assert_contains "$OUT_PROD_ANN" "Runtime coverage" "prod annotation: title present"
assert_contains "$OUT_PROD_ANN" "coldPath" "prod annotation: function name present"

# --- Changed-file filter tests ---

echo ""
echo "=== Changed-file filter (filter-changed.jq) ==="

echo "  check format:"
OUT=$(jq --argjson changed '["src/helpers/api.ts"]' -f "$JQ_DIR/filter-changed.jq" "$FIXTURES/check.json" 2>&1)
assert_valid_json "$OUT" "valid JSON"
assert_json_value "$OUT" '.unused_exports | length' "3" "keeps only exports in changed files"
assert_json_value "$OUT" '.unused_files | length' "0" "no unused files match changed path"
assert_json_value "$OUT" '.unused_dependencies | length' "3" "preserves dependency issues (not file-scoped)"
assert_json_value "$OUT" '.total_issues' "6" "recalculates total_issues"

echo "  check with no matching files:"
OUT=$(jq --argjson changed '["nonexistent.ts"]' -f "$JQ_DIR/filter-changed.jq" "$FIXTURES/check.json" 2>&1)
assert_json_value "$OUT" '.unused_exports | length' "0" "filters all exports"
assert_json_value "$OUT" '.unused_dependencies | length' "3" "deps preserved even with no file matches"

echo "  check clean passthrough:"
OUT=$(jq --argjson changed '["src/a.ts"]' -f "$JQ_DIR/filter-changed.jq" "$FIXTURES/check-clean.json" 2>&1)
assert_json_value "$OUT" '.total_issues' "0" "clean results stay at 0"

echo "  health format:"
OUT=$(jq --argjson changed '["src/helpers/content-parser.ts"]' -f "$JQ_DIR/filter-changed.jq" "$FIXTURES/health.json" 2>&1)
assert_valid_json "$OUT" "valid JSON"
assert_json_value "$OUT" '.file_scores | length' "1" "keeps only changed file scores"
assert_json_value "$OUT" '.file_scores[0].path' "src/helpers/content-parser.ts" "correct file retained"

echo "  dupes format:"
DUPES_PATH=$(jq -r '.clone_groups[0].instances[0].file' "$FIXTURES/dupes.json")
OUT=$(jq --argjson changed "[\"$DUPES_PATH\"]" -f "$JQ_DIR/filter-changed.jq" "$FIXTURES/dupes.json" 2>&1)
assert_valid_json "$OUT" "valid JSON"
assert_json_value "$OUT" '.stats.clone_groups' "1" "retains group with changed instance"

OUT=$(jq --argjson changed '["nonexistent.ts"]' -f "$JQ_DIR/filter-changed.jq" "$FIXTURES/dupes.json" 2>&1)
assert_json_value "$OUT" '.stats.clone_groups' "0" "removes all groups when no match"

echo "  combined format:"
OUT=$(jq --argjson changed '["src/helpers/api.ts"]' -f "$JQ_DIR/filter-changed.jq" "$FIXTURES/combined.json" 2>&1)
assert_valid_json "$OUT" "valid JSON"
assert_json_value "$OUT" '.check.unused_exports | length' "3" "filters check sub-object"
assert_json_value "$OUT" '.check.total_issues' "6" "recalculates check total"

echo "  combined clean passthrough:"
OUT=$(jq --argjson changed '["src/a.ts"]' -f "$JQ_DIR/filter-changed.jq" "$FIXTURES/combined-clean.json" 2>&1)
assert_json_value "$OUT" '.check.total_issues' "0" "clean combined stays at 0"

echo "  boundary violation filter:"
BV_INPUT='{"total_issues":2,"unused_files":[],"unused_exports":[],"unused_types":[],"unused_dependencies":[],"unused_dev_dependencies":[],"unused_optional_dependencies":[],"unused_enum_members":[],"unused_class_members":[],"unresolved_imports":[],"unlisted_dependencies":[],"duplicate_exports":[],"circular_dependencies":[],"boundary_violations":[{"from_path":"src/ui/App.ts","to_path":"src/db/query.ts","from_zone":"ui","to_zone":"db","import_specifier":"src/db/query.ts","line":5,"col":9},{"from_path":"src/api/handler.ts","to_path":"src/db/repo.ts","from_zone":"api","to_zone":"db","import_specifier":"src/db/repo.ts","line":10,"col":9}],"type_only_dependencies":[]}'
OUT=$(echo "$BV_INPUT" | jq --argjson changed '["src/ui/App.ts"]' -f "$JQ_DIR/filter-changed.jq" 2>&1)
assert_json_value "$OUT" '.boundary_violations | length' "1" "keeps only violations from changed files"
assert_json_value "$OUT" '.total_issues' "1" "recalculates total after filtering"

echo "  circular dependency filter:"
CD_INPUT='{"total_issues":1,"unused_files":[],"unused_exports":[],"unused_types":[],"unused_dependencies":[],"unused_dev_dependencies":[],"unused_optional_dependencies":[],"unused_enum_members":[],"unused_class_members":[],"unresolved_imports":[],"unlisted_dependencies":[],"duplicate_exports":[],"circular_dependencies":[{"files":["src/a.ts","src/b.ts"],"length":2,"line":1,"col":0}],"boundary_violations":[],"type_only_dependencies":[]}'
OUT=$(echo "$CD_INPUT" | jq --argjson changed '["src/a.ts"]' -f "$JQ_DIR/filter-changed.jq" 2>&1)
assert_json_value "$OUT" '.circular_dependencies | length' "1" "keeps cycle if any file changed"
OUT=$(echo "$CD_INPUT" | jq --argjson changed '["src/c.ts"]' -f "$JQ_DIR/filter-changed.jq" 2>&1)
assert_json_value "$OUT" '.circular_dependencies | length' "0" "removes cycle if no file changed"

# --- Typed Action script integration tests ---

echo ""
echo "=== Typed Action script integration ==="

ACTION_TYPED_WORK=$(mktemp -d)
ACTION_TYPED_BIN="$ACTION_TYPED_WORK/bin"
ACTION_TYPED_LOG="$ACTION_TYPED_WORK/mock.log"
SCRIPTS_DIR="$DIR/../scripts"
mkdir -p "$ACTION_TYPED_BIN"

cat > "$ACTION_TYPED_BIN/fallow" <<'SH'
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
  pr-comment-github)
    printf '<!-- fallow-id: fallow-results -->\n### Fallow smoke\n\nGenerated by fallow.\n'
    ;;
  review-github)
    if [ "${MOCK_ZERO_REVIEW:-}" = "1" ]; then
      cat <<'JSON'
{"event":"COMMENT","body":"### Fallow smoke\n\n<!-- fallow-review -->","comments":[],"meta":{"schema":"fallow-review-envelope/v1","provider":"github"}}
JSON
      exit 0
    fi
    cat <<'JSON'
{"event":"COMMENT","body":"### Fallow smoke\n\n<!-- fallow-review -->","comments":[{"path":"src/a.ts","line":1,"side":"RIGHT","body":"**warn** `fallow/smoke`: smoke\n\n<!-- fallow-fingerprint: abc -->","fingerprint":"abc"}],"meta":{"schema":"fallow-review-envelope/v1","provider":"github"}}
JSON
    ;;
  *)
    printf '{}\n'
    ;;
esac
SH
chmod +x "$ACTION_TYPED_BIN/fallow"

cat > "$ACTION_TYPED_BIN/gh" <<'SH'
#!/usr/bin/env bash
printf 'gh %s\n' "$*" >> "$MOCK_LOG"
if [ "${1:-}" = "pr" ] && [ "${2:-}" = "diff" ]; then
  printf 'diff --git a/src/a.ts b/src/a.ts\n--- a/src/a.ts\n+++ b/src/a.ts\n@@ -0,0 +1 @@\n+export const a = 1;\n'
  exit 0
fi
if [ "${1:-}" = "api" ]; then
  if printf '%s\n' "$*" | grep -q -- '--input -'; then
    cat > /dev/null
  fi
  if printf '%s\n' "$*" | grep -q -- '--jq'; then
    if [ "${MOCK_EXISTING_REVIEW:-}" = "1" ] && printf '%s\n' "$*" | grep -q 'issues/123/comments'; then
      printf '777\n'
    fi
    exit 0
  fi
  printf '{}\n'
fi
SH
chmod +x "$ACTION_TYPED_BIN/gh"

printf 'FALLOW_ANALYSIS_ARGS=(check --format json --root .)\n' > "$ACTION_TYPED_WORK/fallow-analysis-args.sh"
(
  cd "$ACTION_TYPED_WORK"
  PATH="$ACTION_TYPED_BIN:$PATH" \
    MOCK_LOG="$ACTION_TYPED_LOG" \
    GH_TOKEN="test" \
    PR_NUMBER="123" \
    GH_REPO="owner/repo" \
    FALLOW_COMMAND="check" \
    bash "$SCRIPTS_DIR/comment.sh" > /dev/null
  PATH="$ACTION_TYPED_BIN:$PATH" \
    MOCK_LOG="$ACTION_TYPED_LOG" \
    GH_TOKEN="test" \
    PR_NUMBER="123" \
    GH_REPO="owner/repo" \
    FALLOW_COMMAND="check" \
    FALLOW_ROOT="." \
    MAX_COMMENTS="5" \
    bash "$SCRIPTS_DIR/review.sh" > /dev/null
  PATH="$ACTION_TYPED_BIN:$PATH" \
    MOCK_LOG="$ACTION_TYPED_LOG" \
    MOCK_ZERO_REVIEW="1" \
    MOCK_EXISTING_REVIEW="1" \
    GH_TOKEN="test" \
    PR_NUMBER="123" \
    GH_REPO="owner/repo" \
    FALLOW_COMMAND="check" \
    FALLOW_ROOT="." \
    MAX_COMMENTS="5" \
    bash "$SCRIPTS_DIR/review.sh" > /dev/null
)
ACTION_TYPED_OUT=$(cat "$ACTION_TYPED_LOG")
assert_contains "$ACTION_TYPED_OUT" "--format pr-comment-github" "comment.sh invokes typed PR comment format"
assert_contains "$ACTION_TYPED_OUT" "--format review-github" "review.sh invokes typed GitHub review format"
assert_contains "$ACTION_TYPED_OUT" "fallow ci reconcile-review --provider github" "review.sh invokes GitHub reconcile command"
assert_contains "$ACTION_TYPED_OUT" "repos/owner/repo/pulls/123/reviews" "review.sh posts review envelope"
assert_contains "$ACTION_TYPED_OUT" "repos/owner/repo/issues/comments/777 --method PATCH" "review.sh updates existing body-only review comment"
assert_contains "$(cat "$SCRIPTS_DIR/comment.sh")" "gh_api_retry" "comment.sh wraps GitHub API calls with retry"
assert_contains "$(cat "$SCRIPTS_DIR/review.sh")" "gh_api_retry" "review.sh wraps GitHub API calls with retry"
assert_contains "$(cat "$SCRIPTS_DIR/comment.sh")" "rate limit response; retrying" "comment.sh retries GitHub rate-limit responses"
assert_contains "$(cat "$SCRIPTS_DIR/review.sh")" "rate limit response; retrying" "review.sh retries GitHub rate-limit responses"
assert_contains "$(cat "$SCRIPTS_DIR/review.sh")" "fallow-review-payload.json" "review.sh stores retryable review payload"
assert_not_contains "$(cat "$SCRIPTS_DIR/review.sh")" "--input -" "review.sh does not retry with consumed stdin"
if sed -n '/name: Post review comments/,/run: bash/p' "$DIR/../../action.yml" | /usr/bin/grep -q "steps.analyze.outputs.issues != '0'"; then
  fail "Post review comments action condition" "must run on zero-issue analyses so stale inline review threads can be resolved"
else
  pass "Post review comments action condition runs on zero-issue analyses"
fi
rm -rf "$ACTION_TYPED_WORK"

# --- Pre-computed changed files (shallow clone fallback) tests ---

echo ""
echo "=== Pre-computed changed files (fallow-changed-files.json) ==="

WORK_DIR=$(mktemp -d)
SCRIPTS_DIR="$DIR/../scripts"

# Copy fixtures into work dir to simulate the action working directory
cp "$FIXTURES/check.json" "$WORK_DIR/fallow-results.json"

echo "  comment.sh filtering with pre-computed file:"

# Create a pre-computed changed files list (what analyze.sh produces)
echo '["src/helpers/api.ts"]' > "$WORK_DIR/fallow-changed-files.json"

# Run the filtering logic from comment.sh in the work dir
OUT=$(cd "$WORK_DIR" && \
  CHANGED_SINCE="abc123" \
  INPUT_ROOT="." \
  ACTION_JQ_DIR="$JQ_DIR" \
  FALLOW_COMMAND="dead-code" \
  bash -c '
    RESULTS_FILE="fallow-results.json"
    CHANGED_JSON=""
    if [ -f fallow-changed-files.json ]; then
      CHANGED_JSON=$(cat fallow-changed-files.json)
    fi
    if [ -n "$CHANGED_JSON" ] && [ "$CHANGED_JSON" != "[]" ]; then
      if jq --argjson changed "$CHANGED_JSON" -f "${ACTION_JQ_DIR}/filter-changed.jq" fallow-results.json > fallow-results-scoped.json 2>/dev/null; then
        RESULTS_FILE="fallow-results-scoped.json"
      fi
    fi
    jq -r ".total_issues" "$RESULTS_FILE"
  ' 2>&1)
[ "$OUT" = "6" ] && pass "filters to 6 issues (pre-computed)" || fail "pre-computed filter" "expected 6, got $OUT"

echo "  fallback to unfiltered when no pre-computed file:"
rm -f "$WORK_DIR/fallow-changed-files.json"

# Without fallow-changed-files.json AND without git, falls through to unfiltered
OUT=$(cd "$WORK_DIR" && \
  CHANGED_SINCE="abc123" \
  INPUT_ROOT="." \
  ACTION_JQ_DIR="$JQ_DIR" \
  bash -c '
    RESULTS_FILE="fallow-results.json"
    CHANGED_JSON=""
    if [ -f fallow-changed-files.json ]; then
      CHANGED_JSON=$(cat fallow-changed-files.json)
    else
      CHANGED_FILES=$(git diff --name-only --relative "abc123...HEAD" -- . 2>/dev/null || true)
      if [ -n "$CHANGED_FILES" ]; then
        CHANGED_JSON=$(echo "$CHANGED_FILES" | jq -R -s "split(\"\n\") | map(select(length > 0))")
      fi
    fi
    if [ -n "$CHANGED_JSON" ] && [ "$CHANGED_JSON" != "[]" ]; then
      jq --argjson changed "$CHANGED_JSON" -f "${ACTION_JQ_DIR}/filter-changed.jq" fallow-results.json > fallow-results-scoped.json 2>/dev/null && RESULTS_FILE="fallow-results-scoped.json"
    fi
    jq -r ".total_issues" "$RESULTS_FILE"
  ' 2>&1)
EXPECTED_TOTAL=$(jq -r '.total_issues' "$FIXTURES/check.json")
[ "$OUT" = "$EXPECTED_TOTAL" ] && pass "unfiltered when no pre-computed file" || fail "no pre-computed fallback" "expected $EXPECTED_TOTAL, got $OUT"

echo "  empty changed list produces no filtering:"
echo '[]' > "$WORK_DIR/fallow-changed-files.json"
OUT=$(cd "$WORK_DIR" && \
  CHANGED_SINCE="abc123" \
  ACTION_JQ_DIR="$JQ_DIR" \
  bash -c '
    RESULTS_FILE="fallow-results.json"
    CHANGED_JSON=""
    if [ -f fallow-changed-files.json ]; then
      CHANGED_JSON=$(cat fallow-changed-files.json)
    fi
    if [ -n "$CHANGED_JSON" ] && [ "$CHANGED_JSON" != "[]" ]; then
      jq --argjson changed "$CHANGED_JSON" -f "${ACTION_JQ_DIR}/filter-changed.jq" fallow-results.json > fallow-results-scoped.json 2>/dev/null && RESULTS_FILE="fallow-results-scoped.json"
    fi
    jq -r ".total_issues" "$RESULTS_FILE"
  ' 2>&1)
[ "$OUT" = "$EXPECTED_TOTAL" ] && pass "empty list skips filtering" || fail "empty list guard" "expected $EXPECTED_TOTAL, got $OUT"

echo "  combined format with pre-computed file:"
cp "$FIXTURES/combined.json" "$WORK_DIR/fallow-results.json"
echo '["src/helpers/api.ts"]' > "$WORK_DIR/fallow-changed-files.json"
OUT=$(cd "$WORK_DIR" && \
  CHANGED_SINCE="abc123" \
  ACTION_JQ_DIR="$JQ_DIR" \
  bash -c '
    RESULTS_FILE="fallow-results.json"
    CHANGED_JSON=""
    if [ -f fallow-changed-files.json ]; then
      CHANGED_JSON=$(cat fallow-changed-files.json)
    fi
    if [ -n "$CHANGED_JSON" ] && [ "$CHANGED_JSON" != "[]" ]; then
      jq --argjson changed "$CHANGED_JSON" -f "${ACTION_JQ_DIR}/filter-changed.jq" fallow-results.json > fallow-results-scoped.json 2>/dev/null && RESULTS_FILE="fallow-results-scoped.json"
    fi
    jq -r ".check.total_issues" "$RESULTS_FILE"
  ' 2>&1)
[ "$OUT" = "6" ] && pass "combined format filters check section" || fail "combined pre-computed" "expected 6, got $OUT"

echo "  no CHANGED_SINCE skips filtering entirely:"
cp "$FIXTURES/check.json" "$WORK_DIR/fallow-results.json"
echo '["src/helpers/api.ts"]' > "$WORK_DIR/fallow-changed-files.json"
OUT=$(cd "$WORK_DIR" && \
  ACTION_JQ_DIR="$JQ_DIR" \
  bash -c '
    RESULTS_FILE="fallow-results.json"
    if [ -n "${CHANGED_SINCE:-}" ]; then
      echo "ERROR: should not enter filter block"
    fi
    jq -r ".total_issues" "$RESULTS_FILE"
  ' 2>&1)
[ "$OUT" = "$EXPECTED_TOTAL" ] && pass "no CHANGED_SINCE skips filtering" || fail "no CHANGED_SINCE guard" "expected $EXPECTED_TOTAL, got $OUT"

rm -rf "$WORK_DIR"

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
