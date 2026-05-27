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
assert_contains "$OUT" "DRY RUN: npm install -g --ignore-scripts fallow@2.7.3" "install: installs project pin"

OUT=$(INPUT_ROOT="$INSTALL_TMP/range" FALLOW_VERSION="" FALLOW_INSTALL_DRY_RUN=true bash "$DIR/../scripts/install.sh" 2>&1)
assert_contains "$OUT" "DRY RUN: npm install -g --ignore-scripts fallow@^2.52.0" "install: supports package.json semver range"

OUT=$(INPUT_ROOT="$INSTALL_TMP/empty" FALLOW_VERSION="2.52.0 - 2.53.0" FALLOW_INSTALL_DRY_RUN=true bash "$DIR/../scripts/install.sh" 2>&1)
assert_contains "$OUT" "DRY RUN: npm install -g --ignore-scripts fallow@2.52.0 - 2.53.0" "install: supports npm hyphen ranges"

OUT=$(INPUT_ROOT="$INSTALL_TMP/pinned" FALLOW_VERSION="latest" FALLOW_INSTALL_DRY_RUN=true bash "$DIR/../scripts/install.sh" 2>&1)
assert_contains "$OUT" "Using fallow version from action input: latest" "install: explicit version wins"
assert_contains "$OUT" "DRY RUN: npm install -g --ignore-scripts fallow" "install: explicit latest installs latest"

OUT=$(INPUT_ROOT="$INSTALL_TMP/unsafe" FALLOW_VERSION="" FALLOW_INSTALL_DRY_RUN=true bash "$DIR/../scripts/install.sh" 2>&1)
assert_contains "$OUT" "Ignoring unsupported fallow package.json spec" "install: warns on unsupported package spec"
assert_contains "$OUT" "DRY RUN: npm install -g --ignore-scripts fallow" "install: unsupported package spec falls back to latest"

OUT=$(INPUT_ROOT="$INSTALL_TMP/empty" FALLOW_VERSION="" FALLOW_INSTALL_DRY_RUN=true bash "$DIR/../scripts/install.sh" 2>&1)
assert_contains "$OUT" "DRY RUN: npm install -g --ignore-scripts fallow" "install: no package spec falls back to latest"

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

# --- Binary verification integration ---
#
# Exercises the same verifier path used by install.sh against a controlled
# fake `node_modules/fallow` tree.
# We can't sign with the production key from a test, so we override the
# verifier with a test keypair via the verifyFn knob. The goal is to prove
# that bad signatures produce a non-zero exit, that good signatures
# produce a zero exit, and that the SKIP_ENV escape hatch is honored.

VERIFY_TMP=$(mktemp -d)
trap 'rm -rf "$INSTALL_TMP" "$VERIFY_TMP"' EXIT

PLATFORM_PKG=$(node -e "
const { getPlatformPackage } = require('$DIR/../../npm/fallow/scripts/platform-package');
let pkg;
if (process.platform !== 'linux') {
  pkg = getPlatformPackage(process.platform, process.arch);
} else {
  let lib;
  try { lib = require('detect-libc').familySync(); } catch {}
  pkg = getPlatformPackage(process.platform, process.arch, lib);
}
console.log(pkg);
" 2>&1)

if [ -z "$PLATFORM_PKG" ] || [ "$PLATFORM_PKG" = "null" ]; then
  echo "  (skipping binary verification tests on unsupported platform $(node -e 'console.log(process.platform + \"-\" + process.arch)'))"
else
  # Build a fake `node_modules/fallow` tree with our scripts and a fake
  # platform package. Use a generated keypair, sign the binaries with it,
  # and have the test invocation override the embedded production key.
  mkdir -p "$VERIFY_TMP/node_modules/fallow/scripts"
  mkdir -p "$VERIFY_TMP/node_modules/$PLATFORM_PKG"
  cp "$DIR/../../npm/fallow/scripts/verify-binary.js" "$VERIFY_TMP/node_modules/fallow/scripts/"
  cp "$DIR/../../npm/fallow/scripts/platform-package.js" "$VERIFY_TMP/node_modules/fallow/scripts/"

  # Generate a keypair, write three binaries, sign them. Also write a
  # minimal package.json so require.resolve('@fallow-cli/<platform>/package.json')
  # succeeds.
  node -e "
const crypto = require('node:crypto');
const fs = require('node:fs');
const path = require('node:path');
const { privateKey, publicKey } = crypto.generateKeyPairSync('ed25519');
const der = publicKey.export({ format: 'der', type: 'spki' });
const rawPub = der.subarray(der.length - 32);
const dir = '$VERIFY_TMP/node_modules/$PLATFORM_PKG';
fs.writeFileSync(path.join(dir, 'package.json'), JSON.stringify({ name: '$PLATFORM_PKG', version: '0.0.0' }));
const ext = process.platform === 'win32' ? '.exe' : '';
for (const base of ['fallow', 'fallow-lsp', 'fallow-mcp']) {
  const bin = path.join(dir, base + ext);
  const data = Buffer.from('mock ' + base);
  fs.writeFileSync(bin, data);
  fs.writeFileSync(bin + '.sig', crypto.sign(null, data, privateKey));
}
fs.writeFileSync('$VERIFY_TMP/testkey.bin', rawPub);
fs.writeFileSync('$VERIFY_TMP/testkey.pem', privateKey.export({ format: 'pem', type: 'pkcs8' }));
"

  # Good sig + digest + override key -> ok=true via test injections.
  GOOD=$(cd "$VERIFY_TMP" && node -e "
const fs = require('node:fs');
const crypto = require('node:crypto');
const rawPub = fs.readFileSync('$VERIFY_TMP/testkey.bin');
const { verifyInstalled, _verifyWithKey } = require('fallow/scripts/verify-binary');
(async () => {
  const result = await verifyInstalled({
    verifyFn: (p) => _verifyWithKey(p, rawPub),
    digestProvider: ({ binaryPath }) => crypto.createHash('sha256').update(fs.readFileSync(binaryPath)).digest('hex'),
  });
  if (!result.ok) { console.error('FAIL: ' + result.code + ': ' + result.message); process.exit(1); }
  console.log('OK ' + result.package);
})().catch((err) => { console.error(err.message); process.exit(1); });
" 2>&1)
  good_status=$?
  if [ "$good_status" -eq 0 ]; then
    pass "install verify: good signatures succeed"
  else
    fail "install verify: good signatures succeed" "exit $good_status, output: $GOOD"
  fi

  # Corrupt the fallow-lsp sig and confirm verifyInstalled returns a failure.
  ext=""
  if [ "$(node -p 'process.platform')" = "win32" ]; then ext=".exe"; fi
  node -e "
const fs = require('node:fs');
const p = '$VERIFY_TMP/node_modules/$PLATFORM_PKG/fallow-lsp${ext}.sig';
const sig = fs.readFileSync(p);
sig[0] ^= 0xff;
fs.writeFileSync(p, sig);
"

  BAD=$(cd "$VERIFY_TMP" && node -e "
const fs = require('node:fs');
const crypto = require('node:crypto');
const rawPub = fs.readFileSync('$VERIFY_TMP/testkey.bin');
const { verifyInstalled, _verifyWithKey } = require('fallow/scripts/verify-binary');
(async () => {
  const result = await verifyInstalled({
    verifyFn: (p) => _verifyWithKey(p, rawPub),
    digestProvider: ({ binaryPath }) => crypto.createHash('sha256').update(fs.readFileSync(binaryPath)).digest('hex'),
  });
  if (result.ok) { console.error('FAIL: expected ok=false'); process.exit(2); }
  console.log('FAILED ' + result.code + ' ' + (result.binary || ''));
  process.exit(1);
})().catch((err) => { console.error(err.message); process.exit(2); });
" 2>&1)
  bad_status=$?
  if [ "$bad_status" -eq 1 ]; then
    pass "install verify: bad signature aborts with non-zero exit"
  else
    fail "install verify: bad signature aborts with non-zero exit" "exit $bad_status, output: $BAD"
  fi
  assert_contains "$BAD" "FAILED sig-invalid" "install verify: bad signature reports sig-invalid"
  assert_contains "$BAD" "fallow-lsp" "install verify: bad signature names the offending binary"

  node -e "
const crypto = require('node:crypto');
const fs = require('node:fs');
const privateKey = crypto.createPrivateKey(fs.readFileSync('$VERIFY_TMP/testkey.pem', 'utf8'));
const bin = '$VERIFY_TMP/node_modules/$PLATFORM_PKG/fallow-lsp${ext}';
fs.writeFileSync(bin + '.sig', crypto.sign(null, fs.readFileSync(bin), privateKey));
"

  DIGEST_BAD=$(cd "$VERIFY_TMP" && node -e "
const fs = require('node:fs');
const crypto = require('node:crypto');
const rawPub = fs.readFileSync('$VERIFY_TMP/testkey.bin');
const { verifyInstalled, _verifyWithKey } = require('fallow/scripts/verify-binary');
(async () => {
  const result = await verifyInstalled({
    verifyFn: (p) => _verifyWithKey(p, rawPub),
    digestProvider: ({ binaryPath }) => {
      const digest = crypto.createHash('sha256').update(fs.readFileSync(binaryPath)).digest('hex');
      return /fallow-mcp/.test(binaryPath) ? '0'.repeat(64) : digest;
    },
  });
  if (result.ok) { console.error('FAIL: expected ok=false'); process.exit(2); }
  console.log('FAILED ' + result.code + ' ' + (result.binary || ''));
  process.exit(1);
})().catch((err) => { console.error(err.message); process.exit(2); });
" 2>&1)
  digest_bad_status=$?
  if [ "$digest_bad_status" -eq 1 ]; then
    pass "install verify: digest mismatch aborts with non-zero exit"
  else
    fail "install verify: digest mismatch aborts with non-zero exit" "exit $digest_bad_status, output: $DIGEST_BAD"
  fi
  assert_contains "$DIGEST_BAD" "FAILED digest-mismatch" "install verify: digest mismatch reports digest-mismatch"
  assert_contains "$DIGEST_BAD" "fallow-mcp" "install verify: digest mismatch names the offending binary"

  # sig-missing: binary present, .sig file absent (partial-deploy scenario,
  # most likely real-world failure mode after a botched release).
  rm -f "$VERIFY_TMP/node_modules/$PLATFORM_PKG/fallow-mcp${ext}.sig"
  MISSING=$(cd "$VERIFY_TMP" && node -e "
const fs = require('node:fs');
const crypto = require('node:crypto');
const rawPub = fs.readFileSync('$VERIFY_TMP/testkey.bin');
const { verifyInstalled, _verifyWithKey } = require('fallow/scripts/verify-binary');
(async () => {
  const result = await verifyInstalled({
    verifyFn: (p) => _verifyWithKey(p, rawPub),
    digestProvider: ({ binaryPath }) => crypto.createHash('sha256').update(fs.readFileSync(binaryPath)).digest('hex'),
  });
  if (result.ok) { console.error('FAIL: expected ok=false'); process.exit(2); }
  console.log('FAILED ' + result.code + ' ' + (result.binary || ''));
  process.exit(1);
})().catch((err) => { console.error(err.message); process.exit(2); });
" 2>&1)
  missing_status=$?
  if [ "$missing_status" -eq 1 ]; then
    pass "install verify: missing .sig file aborts with non-zero exit"
  else
    fail "install verify: missing .sig file aborts with non-zero exit" "exit $missing_status, output: $MISSING"
  fi
  assert_contains "$MISSING" "FAILED sig-missing" "install verify: missing .sig reports sig-missing"
  assert_contains "$MISSING" "fallow-mcp" "install verify: missing .sig names the offending binary"

  # Restore a valid-length .sig so the skip-env test sees an otherwise
  # intact-but-wrong setup.
  node -e "
const fs = require('node:fs');
fs.writeFileSync('$VERIFY_TMP/node_modules/$PLATFORM_PKG/fallow-mcp${ext}.sig', Buffer.alloc(64));
"

  # FALLOW_SKIP_BINARY_VERIFY=1 with intact-but-wrong setup short-circuits.
  SKIP=$(cd "$VERIFY_TMP" && FALLOW_SKIP_BINARY_VERIFY=1 node -e "
const { verifyInstalled } = require('fallow/scripts/verify-binary');
(async () => {
  const result = await verifyInstalled();
  console.log(JSON.stringify(result));
})().catch((err) => { console.error(err.message); process.exit(1); });
" 2>&1)
  skip_status=$?
  if [ "$skip_status" -eq 0 ]; then
    pass "install verify: FALLOW_SKIP_BINARY_VERIFY short-circuits"
  else
    fail "install verify: FALLOW_SKIP_BINARY_VERIFY short-circuits" "exit $skip_status, output: $SKIP"
  fi
  assert_contains "$SKIP" "skipped" "install verify: skip env reports skipped=true"
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

# Issue #735: generated artifacts can be moved out of the workspace root.
cat > "$ANALYZE_TMP/bin/fallow" <<'SH'
#!/usr/bin/env bash
case "$*" in
  *"--help"*)
    printf '%s\n' 'Usage: fallow dead-code --sarif-file <PATH>'
    ;;
  *"--format sarif"*)
    printf '%s\n' '{"version":"2.1.0","runs":[]}'
    ;;
  *)
    printf '%s\n' '{"total_issues":0}'
    ;;
esac
SH
chmod +x "$ANALYZE_TMP/bin/fallow"

CUSTOM_WORK="$ANALYZE_TMP/custom-artifacts"
mkdir -p "$CUSTOM_WORK"
cd "$CUSTOM_WORK" && rm -f "$ANALYZE_TMP/output" "$ANALYZE_TMP/env"
OUT=$(PATH="$ANALYZE_TMP/bin:$PATH" GITHUB_OUTPUT="$ANALYZE_TMP/output" GITHUB_ENV="$ANALYZE_TMP/env" \
  INPUT_ROOT="." INPUT_COMMAND="dead-code" INPUT_FORMAT="sarif" INPUT_SARIF="true" INPUT_ARTIFACTS_DIR=".var/fallow" \
  bash "$DIR/../scripts/analyze.sh" 2>&1) || true
cd "$DIR"
assert_contains "$(grep '^results=' "$ANALYZE_TMP/output")" "results=.var/fallow/fallow-results.json" "analyze: custom artifacts-dir emits results path"
assert_contains "$(grep '^sarif=' "$ANALYZE_TMP/output")" "sarif=.var/fallow/fallow-results.sarif" "analyze: custom artifacts-dir emits sarif path"
[ -f "$CUSTOM_WORK/.var/fallow/fallow-results.json" ] && pass "analyze: custom artifacts-dir writes results file" || fail "analyze: custom artifacts-dir writes results file" "missing results file"
[ -f "$CUSTOM_WORK/.var/fallow/fallow-stderr.log" ] && pass "analyze: custom artifacts-dir writes stderr log" || fail "analyze: custom artifacts-dir writes stderr log" "missing stderr log"
[ -f "$CUSTOM_WORK/.var/fallow/fallow-analysis-args.sh" ] && pass "analyze: custom artifacts-dir writes args file" || fail "analyze: custom artifacts-dir writes args file" "missing args file"
[ ! -e "$CUSTOM_WORK/fallow-results.json" ] && pass "analyze: custom artifacts-dir keeps root clean" || fail "analyze: custom artifacts-dir keeps root clean" "root results file exists"
assert_contains "$(cat "$ANALYZE_TMP/env")" "FALLOW_ANALYSIS_ARGS_FILE=.var/fallow/fallow-analysis-args.sh" "analyze: custom artifacts-dir propagates args path"
assert_contains "$(cat "$CUSTOM_WORK/.var/fallow/fallow-analysis-args.sh")" "--sarif-file .var/fallow/fallow-results.sarif" "analyze: custom artifacts-dir passes sarif path to fallow"

DEFAULT_WORK="$ANALYZE_TMP/default-artifacts"
mkdir -p "$DEFAULT_WORK"
cd "$DEFAULT_WORK" && rm -f "$ANALYZE_TMP/output"
OUT=$(PATH="$ANALYZE_TMP/bin:$PATH" GITHUB_OUTPUT="$ANALYZE_TMP/output" \
  INPUT_ROOT="." INPUT_COMMAND="dead-code" INPUT_FORMAT="json" \
  bash "$DIR/../scripts/analyze.sh" 2>&1) || true
cd "$DIR"
assert_contains "$(grep '^results=' "$ANALYZE_TMP/output")" "results=fallow-results.json" "analyze: default artifacts path is unchanged"
[ -f "$DEFAULT_WORK/fallow-results.json" ] && pass "analyze: default writes root results file" || fail "analyze: default writes root results file" "missing root results file"

INVALID_WORK="$ANALYZE_TMP/invalid-artifacts"
mkdir -p "$INVALID_WORK"
cd "$INVALID_WORK" && rm -f "$ANALYZE_TMP/output"
OUT=$(PATH="$ANALYZE_TMP/bin:$PATH" GITHUB_OUTPUT="$ANALYZE_TMP/output" \
  INPUT_ROOT="." INPUT_COMMAND="dead-code" INPUT_FORMAT="json" INPUT_ARTIFACTS_DIR="../outside" \
  bash "$DIR/../scripts/analyze.sh" 2>&1)
cmd_status=$?
cd "$DIR"
if [ "$cmd_status" -eq 2 ]; then
  pass "analyze: artifacts-dir rejects traversal"
else
  fail "analyze: artifacts-dir rejects traversal" "expected exit 2, got $cmd_status"
fi
assert_contains "$OUT" "artifacts-dir must be a relative path inside the workspace" "analyze: artifacts-dir traversal error is clear"

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
assert_contains "$OUT" "Empty catalog groups" "shows empty catalog group row"
assert_contains "$OUT" 'legacy' "shows empty catalog group name"

OUT_CLEAN=$(jq -r -f "$JQ_DIR/summary-check.jq" "$FIXTURES/check-clean.json" 2>&1)
assert_contains "$OUT_CLEAN" "No issues found" "clean: shows no issues"
assert_not_contains "$OUT_CLEAN" "WARNING" "clean: no warning"

# Issue #449: kind_known: false renders "unknown kind \`token\`" in the table,
# distinguishing it from a stale-but-known kind which renders just \`token\`.
OUT_UNKNOWN_KIND_SUMMARY=$(jq '.unused_files = [] | .unused_exports = [] | .unused_types = [] | .unused_dependencies = [] | .unused_dev_dependencies = [] | .unused_optional_dependencies = [] | .unused_enum_members = [] | .unused_class_members = [] | .unresolved_imports = [] | .unlisted_dependencies = [] | .duplicate_exports = [] | .circular_dependencies = [] | .boundary_violations = [] | .type_only_dependencies = [] | .test_only_dependencies = [] | .unused_catalog_entries = [] | .empty_catalog_groups = [] | .unresolved_catalog_references = [] | .unused_dependency_overrides = [] | .misconfigured_dependency_overrides = [] | .private_type_leaks = [] | .stale_suppressions = [{"path": "src/utils.ts", "line": 1, "col": 0, "origin": {"type": "comment", "issue_kind": "complexity-typo", "is_file_level": false, "kind_known": false}}] | .total_issues = 1' "$FIXTURES/check.json" | jq -r -f "$JQ_DIR/summary-check.jq" 2>&1)
assert_contains "$OUT_UNKNOWN_KIND_SUMMARY" 'unknown kind' "summary unknown kind: prefix renders"
assert_contains "$OUT_UNKNOWN_KIND_SUMMARY" 'complexity-typo' "summary unknown kind: verbatim token renders"

echo "  summary-fix.jq:"
OUT=$(jq -r -f "$JQ_DIR/summary-fix.jq" "$FIXTURES/fix.json" 2>&1)
assert_valid_markdown "$OUT" "produces output"
assert_contains "$OUT" "Auto-fix" "has title"
assert_contains "$OUT" "1 fixes" "counts the real fix attempt, not the skip record"
assert_contains "$OUT" "Export removals | 1" "lists the export removal count"
# Issue #602: low-confidence off-graph skips surface in the headline and
# are NOT counted as fix attempts (the skip record carries skipped: true).
assert_contains "$OUT" "kept exports in 1 file" "surfaces low-confidence skip count"
assert_not_contains "$OUT" "2 fixes" "skip record does not inflate the fix-attempt headline"

# A run whose ONLY outcome is a low-confidence skip must still suppress the
# "No fixable issues found" headline (issue #602): exports were found, just
# not auto-removed.
OUT_ONLY_SKIP=$(jq '.fixes = [.fixes[1]] | .total_fixed = 0' "$FIXTURES/fix.json" | jq -r -f "$JQ_DIR/summary-fix.jq" 2>&1)
assert_not_contains "$OUT_ONLY_SKIP" "No fixable issues found" "low-confidence-only run is not reported as clean"
assert_contains "$OUT_ONLY_SKIP" "kept exports in 1 file" "low-confidence-only run surfaces the skip"

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
OUT_SINGULAR=$(jq '.check.unused_files = [.check.unused_files[0]] | .check.unused_exports = [] | .check.unused_dependencies = [] | .check.unused_dev_dependencies = [] | .check.unused_optional_dependencies = [] | .check.unused_types = [] | .check.unused_enum_members = [] | .check.unused_class_members = [] | .check.unresolved_imports = [] | .check.unlisted_dependencies = [] | .check.duplicate_exports = [] | .check.circular_dependencies = [] | .check.boundary_violations = [] | .check.type_only_dependencies = [] | .check.test_only_dependencies = [] | .check.stale_suppressions = [] | .check.unused_catalog_entries = [] | .check.unresolved_catalog_references = [] | .check.unused_dependency_overrides = [] | .check.misconfigured_dependency_overrides = [] | .check.private_type_leaks = [] | .check.total_issues = 1 | .dupes.stats.clone_groups = 1 | .dupes.clone_groups = [.dupes.clone_groups[0]] | .health.summary.functions_above_threshold = 1 | .health.findings = [.health.findings[0]]' "$FIXTURES/combined.json" | jq -r -f "$JQ_DIR/summary-combined.jq" 2>&1)
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
    check: {total_issues: 0, unused_files: [], unused_exports: [], unused_types: [], unused_dependencies: [], unused_dev_dependencies: [], unused_optional_dependencies: [], unused_enum_members: [], unused_class_members: [], unresolved_imports: [], unlisted_dependencies: [], duplicate_exports: [], circular_dependencies: [], boundary_violations: [], type_only_dependencies: [], test_only_dependencies: [], stale_suppressions: [], unused_catalog_entries: [], unresolved_catalog_references: [], unused_dependency_overrides: [], misconfigured_dependency_overrides: [], private_type_leaks: []},
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
OUT_COMBINED_PROD=$(jq '.health.runtime_coverage = {"verdict":"hot-path-touched","summary":{"functions_tracked":4,"functions_hit":3,"functions_unhit":0,"functions_untracked":1,"coverage_percent":75,"trace_count":2400,"period_days":7,"deployments_seen":2},"findings":[{"path":"src/cold.ts","function":"coldPath","line":14,"verdict":"review_required","invocations":0,"confidence":"medium"}],"hot_paths":[{"path":"src/hot.ts","function":"hotPath","line":3,"invocations":250,"percentile":99}]}' "$FIXTURES/combined-clean.json" | jq -r -f "$JQ_DIR/summary-combined.jq" 2>&1)
assert_contains "$OUT_COMBINED_PROD" "Runtime coverage" "combined prod: has runtime coverage details"
assert_contains "$OUT_COMBINED_PROD" "hotPath" "combined prod: shows hot path"
# Verdict hot-path-touched: header should say "hot path[s] touched", not the
# project-wide "hot path[s]" framing. Single-path counts use the singular form.
assert_contains "$OUT_COMBINED_PROD" "hot path touched" "combined prod (verdict hot-path-touched): header uses 'touched' framing"

echo "  summary-combined.jq (no diff/changed-since: standalone framing):"
OUT_COMBINED_STANDALONE=$(jq '.health.runtime_coverage = {"verdict":"clean","summary":{"functions_tracked":4,"functions_hit":4,"functions_unhit":0,"functions_untracked":0,"coverage_percent":100,"trace_count":2400,"period_days":7,"deployments_seen":2},"findings":[],"hot_paths":[{"path":"src/hot.ts","function":"hotPath","line":3,"invocations":250,"percentile":99}]}' "$FIXTURES/combined-clean.json" | jq -r -f "$JQ_DIR/summary-combined.jq" 2>&1)
# Verdict NOT hot-path-touched (running outside PR context): keep the
# project-wide "hot path" framing so the line does not falsely imply the
# hot path is on this change.
assert_contains "$OUT_COMBINED_STANDALONE" "hot path" "combined prod (verdict clean): header uses 'hot path' framing"
if echo "$OUT_COMBINED_STANDALONE" | grep -q "hot path touched"; then
  echo "  FAIL: standalone (verdict=clean) must not say 'hot path touched'"
  exit 1
fi

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
assert_contains "$OUT" "Empty catalog group" "annotation includes empty catalog group title"
assert_contains "$OUT" "legacy" "annotation includes empty catalog group name"

OUT_CLEAN=$(jq -r -f "$JQ_DIR/annotations-check.jq" "$FIXTURES/check-clean.json" 2>&1)
[ -z "$OUT_CLEAN" ] && pass "clean: no annotations" || fail "clean: no annotations" "got output"

# Issue #449: kind_known: false branch renders a typo-fix annotation rather
# than the "no longer matches any active issue" copy used for stale-but-known.
OUT_UNKNOWN_KIND=$(jq '.unused_files = [] | .unused_exports = [] | .unused_types = [] | .unused_dependencies = [] | .unused_dev_dependencies = [] | .unused_optional_dependencies = [] | .unused_enum_members = [] | .unused_class_members = [] | .unresolved_imports = [] | .unlisted_dependencies = [] | .duplicate_exports = [] | .circular_dependencies = [] | .boundary_violations = [] | .type_only_dependencies = [] | .test_only_dependencies = [] | .unused_catalog_entries = [] | .empty_catalog_groups = [] | .unresolved_catalog_references = [] | .unused_dependency_overrides = [] | .misconfigured_dependency_overrides = [] | .private_type_leaks = [] | .stale_suppressions = [{"path": "src/utils.ts", "line": 1, "col": 0, "origin": {"type": "comment", "issue_kind": "complexity-typo", "is_file_level": false, "kind_known": false}}] | .total_issues = 1' "$FIXTURES/check.json" | jq -r -f "$JQ_DIR/annotations-check.jq" 2>&1)
assert_contains "$OUT_UNKNOWN_KIND" "Unknown suppression kind" "unknown kind: typo title"
assert_contains "$OUT_UNKNOWN_KIND" "complexity-typo" "unknown kind: verbatim token in message"
assert_contains "$OUT_UNKNOWN_KIND" "fallow-ignore-next-line" "unknown kind: directive type preserved"
assert_contains "$OUT_UNKNOWN_KIND" "Fix the typo" "unknown kind: actionable next step"

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
assert_json_value "$OUT" '.total_issues' "7" "recalculates total_issues"

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
printf 'summary_scope=%s\n' "${FALLOW_SUMMARY_SCOPE:-}" >> "$MOCK_LOG"
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
    FALLOW_SUMMARY_SCOPE="diff" \
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
assert_contains "$ACTION_TYPED_OUT" "summary_scope=diff" "comment.sh passes FALLOW_SUMMARY_SCOPE to typed PR comment render"
ACTION_BLANK_SUMMARY_SCOPE_COUNT=$(printf '%s\n' "$ACTION_TYPED_OUT" | grep -c '^summary_scope=$' || true)
if [ "$ACTION_BLANK_SUMMARY_SCOPE_COUNT" -ge 1 ]; then
  pass "review.sh does not receive FALLOW_SUMMARY_SCOPE by default"
else
  fail "review.sh does not receive FALLOW_SUMMARY_SCOPE by default" "$ACTION_TYPED_OUT"
fi
assert_contains "$ACTION_TYPED_OUT" "fallow ci reconcile-review --provider github" "review.sh invokes GitHub reconcile command"
assert_contains "$(cat "$SCRIPTS_DIR/review.sh")" "apply_errors" "review.sh checks reconcile apply errors"
assert_contains "$(cat "$SCRIPTS_DIR/review.sh")" "apply_hint" "review.sh emits reconcile apply hint"
assert_contains "$ACTION_TYPED_OUT" "repos/owner/repo/pulls/123/reviews" "review.sh posts review envelope"
assert_contains "$ACTION_TYPED_OUT" "repos/owner/repo/issues/comments/777 --method PATCH" "review.sh updates existing body-only review comment"
assert_contains "$(cat "$DIR/../../action.yml")" "review-guidance:" "action.yml exposes review-guidance input"
assert_contains "$(cat "$DIR/../../action.yml")" "FALLOW_REVIEW_GUIDANCE: \${{ inputs.review-guidance }}" "action.yml maps review-guidance to env"
assert_contains "$(cat "$DIR/../../action.yml")" "summary-scope:" "action.yml exposes summary-scope input"
assert_contains "$(cat "$DIR/../../action.yml")" "FALLOW_SUMMARY_SCOPE: \${{ inputs.summary-scope }}" "action.yml maps summary-scope to comment env"
assert_contains "$(cat "$SCRIPTS_DIR/comment.sh")" "gh_api_retry" "comment.sh wraps GitHub API calls with retry"
assert_contains "$(cat "$SCRIPTS_DIR/review.sh")" "gh_api_retry" "review.sh wraps GitHub API calls with retry"
assert_contains "$(cat "$SCRIPTS_DIR/comment.sh")" "Unsupported FALLOW_SUMMARY_SCOPE" "comment.sh warns on invalid summary scope"
assert_not_contains "$(cat "$SCRIPTS_DIR/review.sh")" "FALLOW_SUMMARY_SCOPE" "review.sh does not consume summary scope"
if sed -n '/name: Post review comments/,/run: bash/p' "$DIR/../../action.yml" | /usr/bin/grep -q "FALLOW_SUMMARY_SCOPE"; then
  fail "Post review comments action env excludes FALLOW_SUMMARY_SCOPE" "summary scope must not affect inline review comments"
else
  pass "Post review comments action env excludes FALLOW_SUMMARY_SCOPE"
fi
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

# =========================================================================
# API failure handling: changed-files marker + dedup-lookup abort
# =========================================================================
# Covers issue #470: silent gh api failures must surface as both a
# structured GITHUB_OUTPUT marker AND a stderr ::warning::, never as
# unscoped analysis or duplicate PR comments.

echo ""
echo "=== API failure handling (issue #470) ==="

API_FAIL_WORK=$(mktemp -d)
API_FAIL_BIN="$API_FAIL_WORK/bin"
mkdir -p "$API_FAIL_BIN"
SCRIPTS_DIR="$DIR/../scripts"

# --- Test 1: analyze.sh emits changed_files_unavailable=true when gh api fails ---
# Simulate a 500 from the GitHub API. The mock fails the gh api --paginate call
# unconditionally; analyze.sh's git diff fallback also fails (no real git
# history against the bogus SHA), so the script lands in the gh api branch.

cat > "$API_FAIL_BIN/gh" <<'SH'
#!/usr/bin/env bash
if [ "${1:-}" = "api" ]; then
  echo "gh: HTTP 500: Internal Server Error (api.github.com/repos/owner/repo/pulls/123/files)" >&2
  exit 1
fi
exit 0
SH
chmod +x "$API_FAIL_BIN/gh"

API_FAIL_OUTPUT="$API_FAIL_WORK/github_output"
: > "$API_FAIL_OUTPUT"
API_FAIL_STDERR=$(cd "$API_FAIL_WORK" \
  && PATH="$API_FAIL_BIN:$PATH" \
  GITHUB_OUTPUT="$API_FAIL_OUTPUT" \
  INPUT_ROOT="." \
  INPUT_COMMAND="check" \
  INPUT_FORMAT="json" \
  INPUT_CHANGED_SINCE="0000000000000000000000000000000000000000" \
  PR_NUMBER="123" \
  GH_REPO="owner/repo" \
  GH_TOKEN="test" \
  FALLOW_API_RETRIES=1 \
  FALLOW_API_RETRY_DELAY=0 \
  bash "$SCRIPTS_DIR/analyze.sh" 2>&1 1>/dev/null) || true

assert_contains "$(cat "$API_FAIL_OUTPUT")" "changed_files_unavailable=true" \
  "analyze: emits changed_files_unavailable=true on gh api failure"
assert_contains "$API_FAIL_STDERR" "GitHub API call to list PR files failed" \
  "analyze: stderr names API failure mode (not shallow-clone)"
assert_contains "$API_FAIL_STDERR" "gh auth status" \
  "analyze: warning includes actionable hint (gh auth status)"

# --- Test 2: analyze.sh emits changed_files_unavailable=false when gh api succeeds ---

cat > "$API_FAIL_BIN/gh" <<'SH'
#!/usr/bin/env bash
if [ "${1:-}" = "api" ]; then
  printf 'src/a.ts\nsrc/b.ts\n'
  exit 0
fi
exit 0
SH
chmod +x "$API_FAIL_BIN/gh"

: > "$API_FAIL_OUTPUT"
(cd "$API_FAIL_WORK" \
  && PATH="$API_FAIL_BIN:$PATH" \
  GITHUB_OUTPUT="$API_FAIL_OUTPUT" \
  INPUT_ROOT="." \
  INPUT_COMMAND="check" \
  INPUT_FORMAT="json" \
  INPUT_CHANGED_SINCE="0000000000000000000000000000000000000000" \
  PR_NUMBER="123" \
  GH_REPO="owner/repo" \
  GH_TOKEN="test" \
  FALLOW_API_RETRIES=1 \
  FALLOW_API_RETRY_DELAY=0 \
  bash "$SCRIPTS_DIR/analyze.sh" >/dev/null 2>&1) || true

if grep -q '^changed_files_unavailable=false$' "$API_FAIL_OUTPUT" \
    && ! grep -q '^changed_files_unavailable=true$' "$API_FAIL_OUTPUT"; then
  pass "analyze: emits changed_files_unavailable=false on gh api success"
else
  fail "analyze: emits changed_files_unavailable=false on gh api success" \
    "expected only =false, got: $(grep changed_files_unavailable "$API_FAIL_OUTPUT" || echo 'absent')"
fi

# --- Test 2b: analyze.sh emits changed_files_unavailable=false even without INPUT_CHANGED_SINCE ---
# The marker must be unconditional so downstream `if:` gates can match on it
# as a positive signal (== 'false') without seeing an absent-vs-false ambiguity.

: > "$API_FAIL_OUTPUT"
(cd "$API_FAIL_WORK" \
  && PATH="$API_FAIL_BIN:$PATH" \
  GITHUB_OUTPUT="$API_FAIL_OUTPUT" \
  INPUT_ROOT="." \
  INPUT_COMMAND="check" \
  INPUT_FORMAT="json" \
  bash "$SCRIPTS_DIR/analyze.sh" >/dev/null 2>&1) || true

if grep -q '^changed_files_unavailable=false$' "$API_FAIL_OUTPUT"; then
  pass "analyze: emits changed_files_unavailable=false even without INPUT_CHANGED_SINCE"
else
  fail "analyze: emits changed_files_unavailable=false even without INPUT_CHANGED_SINCE" \
    "expected =false (unconditional init), got: $(grep changed_files_unavailable "$API_FAIL_OUTPUT" || echo 'absent')"
fi

# --- Test 3-5: review.sh dedup-lookup failure paths ---
# Shared mock harness: fallow renders the typed envelope; gh fails on the
# pulls/.../comments paginate (multi-comment dedup endpoint) and on the
# issues/.../comments paginate (summary-only dedup endpoint), but succeeds
# on every other gh api call (POST to reviews / comments / reconcile).

api_fail_review_run() {
  local label=$1
  local exit_status_var=$2
  local output_var=$3
  local stderr_var=$4
  local mock_zero=$5     # "1" for summary-only path, empty for multi-comment
  local fail_mode=$6     # "5xx" or "4xx"
  local stderr_msg
  case "$fail_mode" in
    5xx) stderr_msg="HTTP 502: Bad Gateway (api.github.com)" ;;
    4xx) stderr_msg="HTTP 403: Forbidden (api.github.com)" ;;
    *)   stderr_msg="HTTP 502: Bad Gateway" ;;
  esac
  cat > "$API_FAIL_BIN/gh" <<SH
#!/usr/bin/env bash
printf 'gh %s\n' "\$*" >> "\$MOCK_LOG"
if [ "\${1:-}" = "api" ]; then
  if printf '%s\n' "\$*" | grep -q -- '--paginate' && printf '%s\n' "\$*" | grep -qE 'pulls/[0-9]+/comments|issues/[0-9]+/comments'; then
    echo "gh: ${stderr_msg}" >&2
    exit 1
  fi
  exit 0
fi
SH
  chmod +x "$API_FAIL_BIN/gh"

  cat > "$API_FAIL_BIN/fallow" <<'SH'
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
    format="$arg"; break
  fi
  previous="$arg"
done
if [ "$format" = "review-github" ]; then
  if [ "${MOCK_ZERO_REVIEW:-}" = "1" ]; then
    cat <<'JSON'
{"event":"COMMENT","body":"### Fallow smoke\n\n<!-- fallow-review -->","comments":[],"meta":{"schema":"fallow-review-envelope/v1","provider":"github"}}
JSON
  else
    cat <<'JSON'
{"event":"COMMENT","body":"### Fallow smoke\n\n<!-- fallow-review -->","comments":[{"path":"src/a.ts","line":1,"side":"RIGHT","body":"**warn** `fallow/smoke`: smoke\n\n<!-- fallow-fingerprint: abc -->","fingerprint":"abc"}],"meta":{"schema":"fallow-review-envelope/v1","provider":"github"}}
JSON
  fi
fi
SH
  chmod +x "$API_FAIL_BIN/fallow"

  : > "$API_FAIL_OUTPUT"
  : > "$API_FAIL_WORK/mock.log"
  printf 'FALLOW_ANALYSIS_ARGS=(check --format json --root .)\n' > "$API_FAIL_WORK/fallow-analysis-args.sh"
  local _stderr _status
  _stderr=$(cd "$API_FAIL_WORK" \
    && PATH="$API_FAIL_BIN:$PATH" \
    MOCK_LOG="$API_FAIL_WORK/mock.log" \
    MOCK_ZERO_REVIEW="$mock_zero" \
    GH_TOKEN="test" \
    PR_NUMBER="123" \
    GH_REPO="owner/repo" \
    GITHUB_OUTPUT="$API_FAIL_OUTPUT" \
    FALLOW_COMMAND="check" \
    FALLOW_ROOT="." \
    MAX_COMMENTS="5" \
    FALLOW_API_RETRIES=1 \
    FALLOW_API_RETRY_DELAY=0 \
    bash "$SCRIPTS_DIR/review.sh" 2>&1 1>/dev/null)
  _status=$?
  printf -v "$exit_status_var" '%s' "$_status"
  printf -v "$output_var" '%s' "$(cat "$API_FAIL_OUTPUT")"
  printf -v "$stderr_var" '%s' "$_stderr"
}

# Test 3: multi-comment dedup path, page-2 5xx -> exit 0, no POST, marker set
api_fail_review_run "multi-5xx" R3_STATUS R3_OUTPUT R3_STDERR "" "5xx"
[ "$R3_STATUS" -eq 0 ] && pass "review.sh: multi-comment dedup-lookup 5xx failure exits 0" \
  || fail "review.sh: multi-comment dedup-lookup 5xx failure exits 0" "got $R3_STATUS"
assert_contains "$R3_OUTPUT" "post_skipped_reason=pagination_failure" \
  "review.sh: emits post_skipped_reason=pagination_failure on dedup-lookup failure"
assert_contains "$R3_STDERR" "skipping inline review to avoid duplicates" \
  "review.sh: warning surfaces dedup-lookup skip"
if cat "$API_FAIL_WORK/mock.log" 2>/dev/null | /usr/bin/grep -q "pulls/123/reviews"; then
  fail "review.sh: no review POST after dedup-lookup failure" "review POST happened"
else
  pass "review.sh: no review POST after dedup-lookup failure"
fi

# Test 4: multi-comment dedup path, page-2 4xx -> exit 1
api_fail_review_run "multi-4xx" R4_STATUS R4_OUTPUT R4_STDERR "" "4xx"
[ "$R4_STATUS" -eq 1 ] && pass "review.sh: multi-comment dedup-lookup 4xx failure exits 1" \
  || fail "review.sh: multi-comment dedup-lookup 4xx failure exits 1" "got $R4_STATUS"

# Test 5: summary-only path posts anyway on dedup-lookup failure (Decision 3).
# On this path we set dedup_lookup_failed=true but keep post_skipped_reason=none
# because the post is NOT skipped (the summary is posted, possibly duplicating).
api_fail_review_run "summary-5xx" R5_STATUS R5_OUTPUT R5_STDERR "1" "5xx"
[ "$R5_STATUS" -eq 0 ] && pass "review.sh: summary-only path exits 0 on dedup-lookup failure" \
  || fail "review.sh: summary-only path exits 0 on dedup-lookup failure" "got $R5_STATUS"
assert_contains "$R5_STDERR" "posting a new one (may duplicate)" \
  "review.sh: summary-only path warning explains duplicate-risk fallback"
assert_contains "$R5_OUTPUT" "dedup_lookup_failed=true" \
  "review.sh: summary-only dedup-lookup failure flips dedup_lookup_failed=true"
if grep -q '^post_skipped_reason=pagination_failure$' <(echo "$R5_OUTPUT"); then
  fail "review.sh: summary-only path does NOT set post_skipped_reason=pagination_failure" \
    "post_skipped_reason should remain 'none' because the post still happens"
else
  pass "review.sh: summary-only path does NOT set post_skipped_reason=pagination_failure"
fi
if cat "$API_FAIL_WORK/mock.log" 2>/dev/null | /usr/bin/grep -qE "issues/123/comments .*--method POST"; then
  pass "review.sh: summary-only path POSTs a new summary despite dedup-lookup failure"
else
  fail "review.sh: summary-only path POSTs a new summary despite dedup-lookup failure" \
    "no POST to issues/123/comments observed"
fi

# Test 5b: retry-exhausted 429 must exit 0 (not 1). 429 looks like 4xx by
# regex but is the rate-limited variant: transient even after retry exhaustion,
# so escalating to a CI failure is wrong.
cat > "$API_FAIL_BIN/gh" <<'SH'
#!/usr/bin/env bash
printf 'gh %s\n' "$*" >> "$MOCK_LOG"
if [ "${1:-}" = "api" ]; then
  if printf '%s\n' "$*" | grep -q -- '--paginate' && printf '%s\n' "$*" | grep -qE 'pulls/[0-9]+/comments'; then
    echo "gh: HTTP 429: API rate limit exceeded (api.github.com)" >&2
    exit 1
  fi
  exit 0
fi
SH
chmod +x "$API_FAIL_BIN/gh"
write_fallow_review_mock_inline() { :; }
cat > "$API_FAIL_BIN/fallow" <<'SH'
#!/usr/bin/env bash
printf 'fallow %s\n' "$*" >> "$MOCK_LOG"
if [ "${1:-}" = "ci" ]; then
  printf '{"schema":"fallow-review-reconcile/v1","stale":[]}\n'
  exit 0
fi
format=""; previous=""
for arg in "$@"; do
  if [ "$previous" = "--format" ]; then format="$arg"; break; fi
  previous="$arg"
done
if [ "$format" = "review-github" ]; then
  cat <<'JSON'
{"event":"COMMENT","body":"### Fallow smoke\n\n<!-- fallow-review -->","comments":[{"path":"src/a.ts","line":1,"side":"RIGHT","body":"**warn** `fallow/smoke`: smoke\n\n<!-- fallow-fingerprint: abc -->","fingerprint":"abc"}],"meta":{"schema":"fallow-review-envelope/v1","provider":"github"}}
JSON
fi
SH
chmod +x "$API_FAIL_BIN/fallow"

: > "$API_FAIL_OUTPUT"
: > "$API_FAIL_WORK/mock.log"
R5B_STDERR=$(cd "$API_FAIL_WORK" \
  && PATH="$API_FAIL_BIN:$PATH" \
  MOCK_LOG="$API_FAIL_WORK/mock.log" \
  GH_TOKEN=test PR_NUMBER=123 GH_REPO=owner/repo \
  GITHUB_OUTPUT="$API_FAIL_OUTPUT" \
  FALLOW_COMMAND=check FALLOW_ROOT=. MAX_COMMENTS=5 \
  FALLOW_API_RETRIES=1 FALLOW_API_RETRY_DELAY=0 \
  bash "$SCRIPTS_DIR/review.sh" 2>&1 1>/dev/null)
R5B_STATUS=$?
[ "$R5B_STATUS" -eq 0 ] \
  && pass "review.sh: retry-exhausted 429 exits 0 (transient, not auth error)" \
  || fail "review.sh: retry-exhausted 429 exits 0 (transient, not auth error)" "got $R5B_STATUS"

# Test 6: comment.sh summary-only path posts anyway on dedup-lookup failure
cat > "$API_FAIL_BIN/gh" <<'SH'
#!/usr/bin/env bash
printf 'gh %s\n' "$*" >> "$MOCK_LOG"
if [ "${1:-}" = "api" ]; then
  if printf '%s\n' "$*" | grep -q -- '--paginate' && printf '%s\n' "$*" | grep -q 'issues/123/comments'; then
    echo "gh: HTTP 502: Bad Gateway" >&2
    exit 1
  fi
  exit 0
fi
SH
chmod +x "$API_FAIL_BIN/gh"

cat > "$API_FAIL_BIN/fallow" <<'SH'
#!/usr/bin/env bash
printf 'fallow %s\n' "$*" >> "$MOCK_LOG"
format=""; previous=""
for arg in "$@"; do
  if [ "$previous" = "--format" ]; then format="$arg"; break; fi
  previous="$arg"
done
if [ "$format" = "pr-comment-github" ]; then
  cat <<'BODY'
<!-- fallow-id: fallow-results -->
### Fallow smoke

Generated by fallow.
BODY
fi
SH
chmod +x "$API_FAIL_BIN/fallow"

: > "$API_FAIL_OUTPUT"
: > "$API_FAIL_WORK/mock.log"
C6_STDERR=$(cd "$API_FAIL_WORK" \
  && PATH="$API_FAIL_BIN:$PATH" \
  MOCK_LOG="$API_FAIL_WORK/mock.log" \
  GH_TOKEN="test" \
  PR_NUMBER="123" \
  GH_REPO="owner/repo" \
  GITHUB_OUTPUT="$API_FAIL_OUTPUT" \
  FALLOW_COMMAND="check" \
  FALLOW_API_RETRIES=1 \
  FALLOW_API_RETRY_DELAY=0 \
  bash "$SCRIPTS_DIR/comment.sh" 2>&1 1>/dev/null) || true

assert_contains "$(cat "$API_FAIL_OUTPUT")" "dedup_lookup_failed=true" \
  "comment.sh: emits dedup_lookup_failed=true on dedup-lookup failure"
if grep -q '^post_skipped_reason=pagination_failure$' "$API_FAIL_OUTPUT"; then
  fail "comment.sh: does NOT set post_skipped_reason=pagination_failure" \
    "post_skipped_reason should remain 'none' because comment.sh always posts"
else
  pass "comment.sh: does NOT set post_skipped_reason=pagination_failure"
fi
assert_contains "$C6_STDERR" "posting a new one (may duplicate)" \
  "comment.sh: warning explains duplicate-risk fallback"
if /usr/bin/grep -qE "issues/123/comments .*--method POST" "$API_FAIL_WORK/mock.log"; then
  pass "comment.sh: POSTs a new summary despite dedup-lookup failure"
else
  fail "comment.sh: POSTs a new summary despite dedup-lookup failure" \
    "no POST to issues/123/comments observed"
fi

rm -rf "$API_FAIL_WORK"

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
[ "$OUT" = "7" ] && pass "filters to 7 issues (pre-computed)" || fail "pre-computed filter" "expected 7, got $OUT"

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

echo "  summary.sh and annotate.sh with custom artifact paths:"
CUSTOM_ARTIFACTS="$WORK_DIR/.var/fallow"
mkdir -p "$CUSTOM_ARTIFACTS"
cp "$FIXTURES/check.json" "$CUSTOM_ARTIFACTS/fallow-results.json"
echo '["src/helpers/api.ts"]' > "$CUSTOM_ARTIFACTS/fallow-changed-files.json"
SUMMARY_FILE="$WORK_DIR/summary.md"
OUT=$(cd "$WORK_DIR" && \
  GITHUB_STEP_SUMMARY="$SUMMARY_FILE" \
  FALLOW_COMMAND="dead-code" \
  ACTION_JQ_DIR="$JQ_DIR" \
  CHANGED_SINCE="abc123" \
  FALLOW_RESULTS_FILE=".var/fallow/fallow-results.json" \
  FALLOW_SCOPED_RESULTS_FILE=".var/fallow/fallow-results-scoped.json" \
  FALLOW_CHANGED_FILES_FILE=".var/fallow/fallow-changed-files.json" \
  bash "$SCRIPTS_DIR/summary.sh" 2>&1)
cmd_status=$?
if [ "$cmd_status" -eq 0 ] && [ -f "$CUSTOM_ARTIFACTS/fallow-results-scoped.json" ]; then
  pass "summary.sh: custom artifacts path writes scoped results beside source"
else
  fail "summary.sh: custom artifacts path writes scoped results beside source" "exit $cmd_status, output: $OUT"
fi
assert_contains "$(cat "$SUMMARY_FILE")" "Issue counts scoped" "summary.sh: custom artifacts path still appends scoping note"

OUT=$(cd "$WORK_DIR" && \
  FALLOW_COMMAND="dead-code" \
  MAX_ANNOTATIONS="3" \
  ACTION_JQ_DIR="$JQ_DIR" \
  CHANGED_SINCE="abc123" \
  FALLOW_RESULTS_FILE=".var/fallow/fallow-results.json" \
  FALLOW_SCOPED_RESULTS_FILE=".var/fallow/fallow-results-scoped.json" \
  FALLOW_CHANGED_FILES_FILE=".var/fallow/fallow-changed-files.json" \
  bash "$SCRIPTS_DIR/annotate.sh" 2>&1)
cmd_status=$?
if [ "$cmd_status" -eq 0 ]; then
  pass "annotate.sh: custom artifacts path succeeds"
else
  fail "annotate.sh: custom artifacts path succeeds" "exit $cmd_status, output: $OUT"
fi
assert_contains "$OUT" "::" "annotate.sh: custom artifacts path emits annotations"

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
