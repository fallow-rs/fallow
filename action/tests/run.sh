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

OUT_CLEAN=$(jq -r -f "$JQ_DIR/summary-dupes.jq" "$FIXTURES/dupes-clean.json" 2>&1)
assert_contains "$OUT_CLEAN" "No code duplication" "clean: no duplication"

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

# --- Review comment jq tests ---

echo ""
echo "=== Review comment scripts ==="

export PREFIX="website/" MAX=50 FALLOW_ROOT="website" GH_REPO="test/repo" PR_NUMBER=1 PR_HEAD_SHA="abc123"

echo "  review-comments-check.jq:"
OUT=$(jq -f "$JQ_DIR/review-comments-check.jq" "$FIXTURES/check.json" 2>&1)
assert_valid_json "$OUT" "produces valid JSON"
assert_contains "$OUT" "Unused" "contains unused findings"
assert_contains "$OUT" "@public" "mentions @public JSDoc tag"
assert_contains "$OUT" "docs.fallow.tools" "has docs links"
assert_contains "$OUT" "Configure or suppress" "has suppress link"
assert_contains "$OUT" "imported in another workspace" "dependency comment includes workspace context"
assert_contains "$OUT" "Move this dependency to the workspace that imports it" "dependency comment avoids unsafe remove hint"

OUT_CLEAN=$(jq -f "$JQ_DIR/review-comments-check.jq" "$FIXTURES/check-clean.json" 2>&1)
assert_json_length "$OUT_CLEAN" "0" "clean: no comments"

echo "  review-comments-dupes.jq:"
OUT=$(jq -f "$JQ_DIR/review-comments-dupes.jq" "$FIXTURES/dupes.json" 2>&1)
assert_valid_json "$OUT" "produces valid JSON"
assert_contains "$OUT" "duplication" "mentions duplication"
assert_contains "$OUT" "github.com" "has GitHub links"
assert_contains "$OUT" "View duplicated code" "includes code fragment"

echo "  review-comments-health.jq:"
OUT=$(jq -f "$JQ_DIR/review-comments-health.jq" "$FIXTURES/health.json" 2>&1)
assert_valid_json "$OUT" "produces valid JSON"
assert_contains "$OUT" "critical" "includes severity in review comment"
assert_contains "$OUT" "parseContentBlocks" "includes function name"

OUT_PROD_REVIEW=$(jq '.runtime_coverage = {"verdict":"cold-code-detected","summary":{"functions_tracked":2,"functions_hit":1,"functions_unhit":1,"functions_untracked":0,"coverage_percent":50,"trace_count":1200,"period_days":7,"deployments_seen":2},"findings":[{"path":"src/cold.ts","function":"coldPath","line":14,"verdict":"review_required","invocations":0,"confidence":"medium","evidence":{"static_status":"used","test_coverage":"not_covered","v8_tracking":"tracked"},"actions":[{"description":"Review before deleting."}]}]}' "$FIXTURES/health-clean.json" | jq -f "$JQ_DIR/review-comments-health.jq" 2>&1)
assert_valid_json "$OUT_PROD_REVIEW" "prod review comments: valid JSON"
assert_contains "$OUT_PROD_REVIEW" "coldPath" "prod review comments: function present"
assert_contains "$OUT_PROD_REVIEW" "review_required" "prod review comments: verdict present"

echo "  review-body.jq:"
OUT=$(jq -r -f "$JQ_DIR/review-body.jq" "$FIXTURES/combined.json" 2>&1)
assert_valid_markdown "$OUT" "produces output"
assert_contains "$OUT" "Fallow Review" "has review title"
assert_contains "$OUT" "fallow-review" "has marker comment"
assert_contains "$OUT" "Maintainability" "shows metrics"

OUT_REVIEW_PROD=$(jq '.health.runtime_coverage = {"verdict":"hot-path-changes-needed","summary":{"functions_tracked":4,"functions_hit":3,"functions_unhit":0,"functions_untracked":1,"coverage_percent":75,"trace_count":2400,"period_days":7,"deployments_seen":2},"findings":[{"path":"src/lazy.ts","function":"lateBound","line":8,"verdict":"coverage_unavailable","confidence":"none"}],"hot_paths":[{"path":"src/hot.ts","function":"hotPath","line":3,"invocations":250,"percentile":99}]}' "$FIXTURES/combined-clean.json" | jq -r -f "$JQ_DIR/review-body.jq" 2>&1)
assert_contains "$OUT_REVIEW_PROD" "Runtime coverage:" "review body prod: summary line present"
assert_contains "$OUT_REVIEW_PROD" "hot path" "review body prod: hot path mentioned"

# --- Suggestion block tests ---

echo ""
echo "=== Suggestion blocks ==="

echo "  unused-export type field:"
OUT=$(jq -f "$JQ_DIR/review-comments-check.jq" "$FIXTURES/check.json" 2>&1)
TYPES=$(echo "$OUT" | jq -r '[.[].type] | unique | join(",")')
assert_contains "$TYPES" "unused-export" "exports have type field for suggestion enrichment"

echo "  single export keeps type:"
SINGLE='{"total_issues":1,"unused_files":[],"unused_exports":[{"path":"x.ts","export_name":"foo","is_type_only":false,"line":5,"col":0,"span_start":0,"is_re_export":false}],"unused_types":[],"unused_dependencies":[],"unused_dev_dependencies":[],"unused_optional_dependencies":[],"unused_enum_members":[],"unused_class_members":[],"unresolved_imports":[],"unlisted_dependencies":[],"duplicate_exports":[],"circular_dependencies":[],"boundary_violations":[],"type_only_dependencies":[]}'
OUT=$(echo "$SINGLE" | jq -f "$JQ_DIR/review-comments-check.jq" 2>&1)
assert_json_length "$OUT" "1" "single export produces 1 comment"
SINGLE_TYPE=$(echo "$OUT" | jq -r '.[0].type')
[ "$SINGLE_TYPE" = "unused-export" ] && pass "type is unused-export (not grouped)" || fail "type is unused-export" "got $SINGLE_TYPE"

echo "  grouped exports get different type:"
MULTI='{"total_issues":2,"unused_files":[],"unused_exports":[{"path":"x.ts","export_name":"foo","is_type_only":false,"line":5,"col":0,"span_start":0,"is_re_export":false},{"path":"x.ts","export_name":"bar","is_type_only":false,"line":10,"col":0,"span_start":0,"is_re_export":false}],"unused_types":[],"unused_dependencies":[],"unused_dev_dependencies":[],"unused_optional_dependencies":[],"unused_enum_members":[],"unused_class_members":[],"unresolved_imports":[],"unlisted_dependencies":[],"duplicate_exports":[],"circular_dependencies":[],"boundary_violations":[],"type_only_dependencies":[]}'
OUT=$(echo "$MULTI" | jq -f "$JQ_DIR/review-comments-check.jq" | jq --argjson max 50 -f "$JQ_DIR/merge-comments.jq" 2>&1)
assert_json_length "$OUT" "1" "2 exports from same file grouped into 1"
GROUP_TYPE=$(echo "$OUT" | jq -r '.[0].type')
[ "$GROUP_TYPE" = "unused-export-group" ] && pass "grouped type is unused-export-group" || fail "grouped type" "got $GROUP_TYPE"
assert_contains "$OUT" "2 unused exports" "grouped comment mentions count"

echo "  boundary violation produces review comment:"
BV_INPUT='{"total_issues":1,"unused_files":[],"unused_exports":[],"unused_types":[],"unused_dependencies":[],"unused_dev_dependencies":[],"unused_optional_dependencies":[],"unused_enum_members":[],"unused_class_members":[],"unresolved_imports":[],"unlisted_dependencies":[],"duplicate_exports":[],"circular_dependencies":[],"boundary_violations":[{"from_path":"src/ui/App.ts","to_path":"src/db/query.ts","from_zone":"ui","to_zone":"db","import_specifier":"src/db/query.ts","line":5,"col":9}],"type_only_dependencies":[]}'
OUT=$(echo "$BV_INPUT" | MAX=50 jq -f "$JQ_DIR/review-comments-check.jq" 2>&1)
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

echo "  boundary violation produces annotation:"
ANN=$(echo "$BV_INPUT" | jq -rf "$JQ_DIR/annotations-check.jq" 2>&1)
assert_contains "$ANN" "::warning" "annotation is warning level"
assert_contains "$ANN" "file=src/ui/App.ts" "annotation has correct file"
assert_contains "$ANN" "line=5" "annotation has correct line"
assert_contains "$ANN" "Boundary violation" "annotation title"
assert_contains "$ANN" "zone" "annotation mentions zone"

echo "  boundary violation appears in summary:"
SUMMARY=$(echo "$BV_INPUT" | jq -rf "$JQ_DIR/summary-check.jq" 2>&1)
assert_contains "$SUMMARY" "Boundary violations" "summary has boundary section"
assert_contains "$SUMMARY" "src/ui/App.ts" "summary mentions file"
assert_contains "$SUMMARY" "ui" "summary mentions zone"

echo "  private type leak appears in review comments:"
PTL_INPUT='{"total_issues":1,"unused_files":[],"unused_exports":[],"unused_types":[],"private_type_leaks":[{"path":"src/Component.ts","export_name":"Component","type_name":"Props","line":17,"col":33,"span_start":207}],"unused_dependencies":[],"unused_dev_dependencies":[],"unused_optional_dependencies":[],"unused_enum_members":[],"unused_class_members":[],"unresolved_imports":[],"unlisted_dependencies":[],"duplicate_exports":[],"circular_dependencies":[],"boundary_violations":[],"type_only_dependencies":[]}'
OUT=$(echo "$PTL_INPUT" | MAX=50 jq -f "$JQ_DIR/review-comments-check.jq" 2>&1)
assert_valid_json "$OUT" "private type leak JSON valid"
assert_json_length "$OUT" "1" "private type leak produces 1 comment"
assert_contains "$OUT" "Private type leak" "comment mentions Private type leak"
assert_contains "$OUT" "Component" "comment mentions export name"
assert_contains "$OUT" "Props" "comment mentions private type name"

echo "  private type leak appears in annotations:"
ANN=$(echo "$PTL_INPUT" | jq -rf "$JQ_DIR/annotations-check.jq" 2>&1)
assert_contains "$ANN" "::warning" "annotation is warning level"
assert_contains "$ANN" "file=src/Component.ts" "annotation has correct file"
assert_contains "$ANN" "title=Private type leak" "annotation title"
assert_contains "$ANN" "Component" "annotation mentions export name"
assert_contains "$ANN" "Props" "annotation mentions private type name"

echo "  private type leak appears in summary:"
SUMMARY=$(echo "$PTL_INPUT" | jq -rf "$JQ_DIR/summary-check.jq" 2>&1)
assert_contains "$SUMMARY" "Private type leaks" "summary has private type leaks section"
assert_contains "$SUMMARY" "src/Component.ts" "summary mentions file"
assert_contains "$SUMMARY" "Component" "summary mentions export name"
assert_contains "$SUMMARY" "Props" "summary mentions private type name"

echo "  review-body clean state:"
OUT_CLEAN=$(jq -r -f "$JQ_DIR/review-body.jq" "$FIXTURES/combined-clean.json" 2>&1)
assert_contains "$OUT_CLEAN" "No code issues" "clean: no code issues"
assert_contains "$OUT_CLEAN" "No duplication" "clean: no duplication"
assert_contains "$OUT_CLEAN" "fallow-review" "clean: has marker"

# --- Merge script tests ---

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
OUT=$(echo "$EXPORTS" | jq --argjson max 50 -f "$JQ_DIR/merge-comments.jq" 2>&1)
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
OUT=$(echo "$CLONES" | jq --argjson max 50 -f "$JQ_DIR/merge-comments.jq" 2>&1)
assert_valid_json "$OUT" "valid JSON"
assert_json_length "$OUT" "2" "deduplicates to 1 per clone group (4 → 2)"

# Test drop refactoring targets
TARGETS='[
  {"type":"other","path":"a.ts","line":1,"body":"finding"},
  {"type":"refactoring-target","path":"a.ts","line":1,"body":"target"}
]'
OUT=$(echo "$TARGETS" | jq --argjson max 50 -f "$JQ_DIR/merge-comments.jq" 2>&1)
assert_json_length "$OUT" "1" "drops refactoring targets"
assert_not_contains "$OUT" "target" "target body is removed"

# Test merge same line
SAME_LINE='[
  {"type":"other","path":"a.ts","line":5,"body":"complexity warning"},
  {"type":"other","path":"a.ts","line":5,"body":"unused export warning"}
]'
OUT=$(echo "$SAME_LINE" | jq --argjson max 50 -f "$JQ_DIR/merge-comments.jq" 2>&1)
assert_json_length "$OUT" "1" "merges same-line comments"
assert_contains "$OUT" "complexity warning" "merged comment has first body"
assert_contains "$OUT" "unused export warning" "merged comment has second body"
assert_contains "$OUT" "\\n---\\n" "merged comment has separator"

# Test empty input
OUT=$(echo '[]' | jq --argjson max 50 -f "$JQ_DIR/merge-comments.jq" 2>&1)
assert_json_length "$OUT" "0" "empty input produces empty output"

# Test max limit
MANY='[
  {"type":"other","path":"a.ts","line":1,"body":"1"},
  {"type":"other","path":"a.ts","line":2,"body":"2"},
  {"type":"other","path":"a.ts","line":3,"body":"3"},
  {"type":"other","path":"a.ts","line":4,"body":"4"},
  {"type":"other","path":"a.ts","line":5,"body":"5"}
]'
OUT=$(echo "$MANY" | jq --argjson max 3 -f "$JQ_DIR/merge-comments.jq" 2>&1)
assert_json_length "$OUT" "3" "respects max limit"

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

# --- Diff-hunk filter tests ---

echo ""
echo "=== Diff-hunk filter (filter-diff-hunks.jq) ==="

# Mock PR files API response with patch hunks
PR_FILES='[
  {"filename": "src/foo.ts", "patch": "@@ -10,3 +10,5 @@ function foo() {\n+  added line\n+  another added line\n context\n"},
  {"filename": "src/bar.ts", "patch": "@@ -1,2 +1,3 @@ header\n+new\n@@ -20,3 +21,4 @@ other\n+more\n"},
  {"filename": "src/binary.png", "patch": null},
  {"filename": "src/big-file.ts"}
]'

# Comments: some inside hunks, some outside
COMMENTS='[
  {"type": "other", "path": "src/foo.ts", "line": 12, "body": "inside hunk"},
  {"type": "other", "path": "src/foo.ts", "line": 50, "body": "outside hunk"},
  {"type": "other", "path": "src/bar.ts", "line": 2, "body": "inside first hunk"},
  {"type": "other", "path": "src/bar.ts", "line": 22, "body": "inside second hunk"},
  {"type": "other", "path": "src/bar.ts", "line": 100, "body": "outside all hunks"},
  {"type": "other", "path": "src/binary.png", "line": 5, "body": "null patch file"},
  {"type": "other", "path": "src/big-file.ts", "line": 3, "body": "missing patch field"},
  {"type": "other", "path": "src/unknown.ts", "line": 1, "body": "file not in PR"}
]'

echo "  filter-diff-hunks.jq:"
OUT=$(echo "$COMMENTS" | jq --argjson pr_files "$PR_FILES" -f "$JQ_DIR/filter-diff-hunks.jq" 2>&1)
assert_valid_json "$OUT" "produces valid JSON"

echo "  keeps comments inside hunks:"
assert_json_value "$OUT" '[.[] | select(.body == "inside hunk")] | length' "1" "foo.ts line 12 inside hunk kept"
assert_json_value "$OUT" '[.[] | select(.body == "inside first hunk")] | length' "1" "bar.ts line 2 inside first hunk kept"
assert_json_value "$OUT" '[.[] | select(.body == "inside second hunk")] | length' "1" "bar.ts line 22 inside second hunk kept"

echo "  removes comments outside hunks:"
assert_json_value "$OUT" '[.[] | select(.body == "outside hunk")] | length' "0" "foo.ts line 50 outside hunk removed"
assert_json_value "$OUT" '[.[] | select(.body == "outside all hunks")] | length' "0" "bar.ts line 100 outside all hunks removed"

echo "  fail-open for null/missing patch:"
assert_json_value "$OUT" '[.[] | select(.body == "null patch file")] | length' "1" "null patch: comment kept (fail-open)"
assert_json_value "$OUT" '[.[] | select(.body == "missing patch field")] | length' "1" "missing patch: comment kept (fail-open)"

echo "  fail-open for files not in PR:"
assert_json_value "$OUT" '[.[] | select(.body == "file not in PR")] | length' "1" "unknown file: comment kept (fail-open)"

echo "  total filtered count:"
assert_json_length "$OUT" "6" "keeps 6 of 8 comments (2 outside hunks removed)"

echo "  single-line hunk (no count in @@):"
SINGLE_LINE_PR='[{"filename": "src/x.ts", "patch": "@@ -5 +5 @@ ctx\n-old\n+new"}]'
SINGLE_LINE_COMMENTS='[
  {"type": "other", "path": "src/x.ts", "line": 5, "body": "on single-line hunk"},
  {"type": "other", "path": "src/x.ts", "line": 6, "body": "outside single-line hunk"}
]'
OUT=$(echo "$SINGLE_LINE_COMMENTS" | jq --argjson pr_files "$SINGLE_LINE_PR" -f "$JQ_DIR/filter-diff-hunks.jq" 2>&1)
assert_json_length "$OUT" "1" "single-line hunk: keeps line 5, removes line 6"
assert_json_value "$OUT" '.[0].line' "5" "single-line hunk: kept line is 5"

echo "  empty comments array:"
OUT=$(echo '[]' | jq --argjson pr_files "$PR_FILES" -f "$JQ_DIR/filter-diff-hunks.jq" 2>&1)
assert_json_length "$OUT" "0" "empty input produces empty output"

echo "  empty PR files array:"
OUT=$(echo "$COMMENTS" | jq --argjson pr_files '[]' -f "$JQ_DIR/filter-diff-hunks.jq" 2>&1)
assert_json_length "$OUT" "8" "empty PR files: all comments kept (fail-open)"

echo "  deleted file (count=0 hunk):"
DELETED_PR='[{"filename": "src/gone.ts", "patch": "@@ -1,5 +0,0 @@ removed\n-line1\n-line2"}]'
DELETED_COMMENTS='[{"type": "other", "path": "src/gone.ts", "line": 1, "body": "in deleted file"}]'
OUT=$(echo "$DELETED_COMMENTS" | jq --argjson pr_files "$DELETED_PR" -f "$JQ_DIR/filter-diff-hunks.jq" 2>&1)
assert_json_length "$OUT" "0" "deleted file (count=0): no new-side lines, comment removed"

echo "  --slurpfile format (outer array wrapper):"
SLURP_TMP=$(mktemp)
echo "$PR_FILES" > "$SLURP_TMP"
OUT=$(echo "$COMMENTS" | jq --slurpfile pr_files "$SLURP_TMP" -f "$JQ_DIR/filter-diff-hunks.jq" 2>&1)
rm -f "$SLURP_TMP"
assert_valid_json "$OUT" "slurpfile produces valid JSON"
assert_json_length "$OUT" "6" "slurpfile: same result as argjson (6 of 8 kept)"

# --- Review body with filtered counts ---

echo ""
echo "=== Review body with diff-hunk counts ==="

echo "  review-body.jq with filtered findings:"
OUT=$(INLINE_COUNT=5 FILTERED_COUNT=3 jq -r -f "$JQ_DIR/review-body.jq" "$FIXTURES/combined.json" 2>&1)
assert_contains "$OUT" "inline comments" "shows inline count"
assert_contains "$OUT" "findings in files not changed in this PR" "shows filtered count"
assert_not_contains "$OUT" "See inline comments for details" "no generic message when filtered"

echo "  review-body.jq with no filtered findings:"
OUT=$(INLINE_COUNT=5 FILTERED_COUNT=0 jq -r -f "$JQ_DIR/review-body.jq" "$FIXTURES/combined.json" 2>&1)
assert_contains "$OUT" "inline comments on your changes" "shows inline count when no filtered"
assert_not_contains "$OUT" "additional findings" "no filtered mention when count is 0"

echo "  review-body.jq with all findings filtered (body-only):"
OUT=$(INLINE_COUNT=0 FILTERED_COUNT=8 jq -r -f "$JQ_DIR/review-body.jq" "$FIXTURES/combined.json" 2>&1)
assert_not_contains "$OUT" "See inline comments" "no inline mention when all filtered"
assert_contains "$OUT" "none are on lines changed" "body-only explains why no inline comments"
assert_contains "$OUT" "fallow-review" "still has marker"

echo "  review-body.jq without env vars (backwards compat):"
OUT=$(jq -r -f "$JQ_DIR/review-body.jq" "$FIXTURES/combined.json" 2>&1)
assert_contains "$OUT" "fallow-review" "marker present without env vars"

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
