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
fail() { FAILED=$((FAILED + 1)); ERRORS+=("$1: $2"); echo "  ✗ $1 - $2"; }

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

assert_safe_workflow_output() {
  local output="$1" expected_lines="$2" name="$3"
  local actual_lines
  actual_lines=$(printf '%s\n' "$output" | awk 'END { print NR }')
  if [[ "$output" == *$'\r'* ]]; then
    fail "$name" "contains a raw carriage return"
  elif [ "$actual_lines" != "$expected_lines" ]; then
    fail "$name" "expected $expected_lines command lines, got $actual_lines"
  elif printf '%s\n' "$output" | grep -qv '^::'; then
    fail "$name" "contains a non-command continuation line"
  else
    pass "$name"
  fi
}

# --- Repository config hygiene ---

echo ""
echo "=== Repository config hygiene ==="

CONFIG_HYGIENE_JS=$(mktemp)
cat > "$CONFIG_HYGIENE_JS" <<'NODE'
const { readdirSync, readFileSync, statSync } = require("node:fs");
const { join } = require("node:path");

const ignored = new Set([".git", "target", "node_modules"]);
const configNames = new Set([".fallowrc.json", ".fallowrc.jsonc"]);
const issues = [];

const stripJsonc = (input) => {
  let output = "";
  let inString = false;
  let escaped = false;
  for (let i = 0; i < input.length; i += 1) {
    const char = input[i];
    const next = input[i + 1];
    if (inString) {
      output += char;
      if (escaped) {
        escaped = false;
      } else if (char === "\\") {
        escaped = true;
      } else if (char === "\"") {
        inString = false;
      }
      continue;
    }
    if (char === "\"") {
      inString = true;
      output += char;
      continue;
    }
    if (char === "/" && next === "/") {
      while (i < input.length && input[i] !== "\n") i += 1;
      output += "\n";
      continue;
    }
    if (char === "/" && next === "*") {
      i += 2;
      while (i < input.length && !(input[i] === "*" && input[i + 1] === "/")) i += 1;
      i += 1;
      output += " ";
      continue;
    }
    output += char;
  }
  return output;
};

const findDuplicateKeys = (source, path) => {
  const stripped = stripJsonc(source);
  const duplicatePattern = /"([^"\\]*(?:\\.[^"\\]*)*)"\s*:/g;
  const stack = [];
  const objectKeys = [new Set()];
  let match;
  let i = 0;
  let inString = false;
  let escaped = false;
  while (i < stripped.length) {
    const char = stripped[i];
    if (inString) {
      if (escaped) {
        escaped = false;
      } else if (char === "\\") {
        escaped = true;
      } else if (char === "\"") {
        inString = false;
      }
      i += 1;
      continue;
    }
    if (char === "\"") {
      duplicatePattern.lastIndex = i;
      match = duplicatePattern.exec(stripped);
      if (match && match.index === i) {
        const key = JSON.parse(`"${match[1]}"`);
        const current = objectKeys[objectKeys.length - 1];
        if (current.has(key)) {
          issues.push(`${path}: duplicate key ${[...stack, key].join(".")}`);
        }
        current.add(key);
        i = duplicatePattern.lastIndex;
        continue;
      }
      inString = true;
    } else if (char === "{") {
      objectKeys.push(new Set());
      stack.push("<object>");
    } else if (char === "}") {
      objectKeys.pop();
      stack.pop();
    }
    i += 1;
  }
};

const walk = (dir) => {
  for (const entry of readdirSync(dir)) {
    if (ignored.has(entry)) continue;
    const path = join(dir, entry);
    const stat = statSync(path);
    if (stat.isDirectory()) {
      walk(path);
    } else if (configNames.has(entry)) {
      findDuplicateKeys(readFileSync(path, "utf8"), path);
    }
  }
};

walk(".");
for (const issue of issues) console.log(issue);
process.exit(issues.length === 0 ? 0 : 1);
NODE
DUPLICATE_CONFIG_KEYS=$(cd "$DIR/../.." && node "$CONFIG_HYGIENE_JS")
rm -f "$CONFIG_HYGIENE_JS"
if [ -z "$DUPLICATE_CONFIG_KEYS" ]; then
  pass "repo fallow configs have no duplicate JSON keys"
else
  fail "repo fallow configs have no duplicate JSON keys" "$DUPLICATE_CONFIG_KEYS"
fi

# --- Install script tests ---

echo ""
echo "=== Install script ==="

if node --test "$DIR/install-verification-timeout.test.mjs"; then
  pass "install: binary verification is supervised"
else
  fail "install: binary verification is supervised" "regression test failed"
fi

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

# --- Type-aware sidecar provisioning ---

echo ""
echo "=== Install script: type-aware sidecar ==="

mkdir -p "$INSTALL_TMP/ta-jsonc" "$INSTALL_TMP/ta-toml" "$INSTALL_TMP/ta-off" \
  "$INSTALL_TMP/ta-audit" "$INSTALL_TMP/ta-pinned" "$INSTALL_TMP/ta-explicit"

cat > "$INSTALL_TMP/ta-jsonc/.fallowrc.jsonc" <<'JSONC'
{
  // semantic evidence for dead-code candidates
  "typeAware": { "enabled": true, "require": "best-effort" },
}
JSONC
cat > "$INSTALL_TMP/ta-toml/fallow.toml" <<'TOML'
[typeAware]
enabled = true
TOML
cat > "$INSTALL_TMP/ta-off/.fallowrc.json" <<'JSON'
{"typeAware":{"enabled":false}}
JSON
cat > "$INSTALL_TMP/ta-audit/.fallowrc.json" <<'JSON'
{"audit":{"typeAware":true}}
JSON
cat > "$INSTALL_TMP/ta-pinned/package.json" <<'JSON'
{"devDependencies":{"fallow":"2.7.3"}}
JSON
cat > "$INSTALL_TMP/ta-pinned/.fallowrc.json" <<'JSON'
{"typeAware":{"enabled":true}}
JSON
cat > "$INSTALL_TMP/ta-explicit/custom-config.jsonc" <<'JSONC'
{"typeAware":{"enabled":true}}
JSONC

OUT=$(INPUT_ROOT="$INSTALL_TMP/ta-jsonc" FALLOW_VERSION="" FALLOW_INSTALL_DRY_RUN=true bash "$DIR/../scripts/install.sh" 2>&1)
assert_contains "$OUT" "Type-aware sidecar enabled via the project fallow config" "sidecar: auto detects jsonc typeAware.enabled"
assert_contains "$OUT" "DRY RUN: npm install --prefix <tool-dir> --ignore-scripts fallow-type-aware@<resolved CLI version>" "sidecar: auto installs sidecar for unpinned CLI"

OUT=$(INPUT_ROOT="$INSTALL_TMP/ta-toml" FALLOW_VERSION="" FALLOW_INSTALL_DRY_RUN=true bash "$DIR/../scripts/install.sh" 2>&1)
assert_contains "$OUT" "fallow-type-aware@" "sidecar: auto detects fallow.toml typeAware section"

OUT=$(INPUT_ROOT="$INSTALL_TMP/ta-audit" FALLOW_VERSION="" FALLOW_INSTALL_DRY_RUN=true bash "$DIR/../scripts/install.sh" 2>&1)
assert_contains "$OUT" "fallow-type-aware@" "sidecar: auto detects audit.typeAware override"

OUT=$(INPUT_ROOT="$INSTALL_TMP/ta-explicit" INPUT_CONFIG="$INSTALL_TMP/ta-explicit/custom-config.jsonc" FALLOW_VERSION="" FALLOW_INSTALL_DRY_RUN=true bash "$DIR/../scripts/install.sh" 2>&1)
assert_contains "$OUT" "fallow-type-aware@" "sidecar: auto respects explicit config path"

OUT=$(INPUT_ROOT="$INSTALL_TMP/empty" INPUT_TYPE_AWARE=true FALLOW_VERSION="" FALLOW_INSTALL_DRY_RUN=true bash "$DIR/../scripts/install.sh" 2>&1)
assert_contains "$OUT" "Type-aware sidecar enabled via the action 'type-aware' input" "sidecar: input true forces provisioning"
assert_contains "$OUT" "fallow-type-aware@" "sidecar: input true installs sidecar"

OUT=$(INPUT_ROOT="$INSTALL_TMP/ta-off" FALLOW_VERSION="" FALLOW_INSTALL_DRY_RUN=true bash "$DIR/../scripts/install.sh" 2>&1)
assert_not_contains "$OUT" "fallow-type-aware@" "sidecar: config enabled=false skips provisioning"

OUT=$(INPUT_ROOT="$INSTALL_TMP/ta-jsonc" INPUT_TYPE_AWARE=false FALLOW_VERSION="" FALLOW_INSTALL_DRY_RUN=true bash "$DIR/../scripts/install.sh" 2>&1)
assert_not_contains "$OUT" "fallow-type-aware@" "sidecar: input false wins over enabled config"

OUT=$(INPUT_ROOT="$INSTALL_TMP/empty" FALLOW_VERSION="" FALLOW_INSTALL_DRY_RUN=true bash "$DIR/../scripts/install.sh" 2>&1)
assert_not_contains "$OUT" "fallow-type-aware@" "sidecar: auto without config skips provisioning"

OUT=$(INPUT_ROOT="$INSTALL_TMP/ta-pinned" FALLOW_VERSION="" FALLOW_INSTALL_DRY_RUN=true bash "$DIR/../scripts/install.sh" 2>&1)
assert_contains "$OUT" "DRY RUN: npm install -g --ignore-scripts fallow@2.7.3" "sidecar: pinned CLI still installs pin"
assert_contains "$OUT" "DRY RUN: npm install --prefix <tool-dir> --ignore-scripts fallow-type-aware@2.7.3" "sidecar: exact pin propagates to sidecar version"

OUT=$(INPUT_ROOT="$INSTALL_TMP/ta-pinned" FALLOW_VERSION="3.11.0" FALLOW_INSTALL_DRY_RUN=true bash "$DIR/../scripts/install.sh" 2>&1)
assert_contains "$OUT" "DRY RUN: npm install --prefix <tool-dir> --ignore-scripts fallow-type-aware@3.11.0" "sidecar: version input wins for sidecar version"

OUT=$(INPUT_ROOT="$INSTALL_TMP/empty" INPUT_TYPE_AWARE=bogus FALLOW_VERSION="" FALLOW_INSTALL_DRY_RUN=true bash "$DIR/../scripts/install.sh" 2>&1)
cmd_status=$?
if [ "$cmd_status" -ne 0 ]; then
  pass "sidecar: invalid type-aware input fails"
else
  fail "sidecar: invalid type-aware input fails" "expected non-zero exit"
fi
assert_contains "$OUT" "Invalid 'type-aware' input" "sidecar: invalid type-aware input explains failure"

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

  # Generate a keypair, write both verified binaries, and sign them. Also write a
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
for (const base of ['fallow', 'fallow-similar-code']) {
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

  # Corrupt the fallow sig and confirm verifyInstalled returns a failure.
  ext=""
  if [ "$(node -p 'process.platform')" = "win32" ]; then ext=".exe"; fi
  node -e "
const fs = require('node:fs');
const p = '$VERIFY_TMP/node_modules/$PLATFORM_PKG/fallow${ext}.sig';
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
  assert_contains "$BAD" "fallow" "install verify: bad signature names the offending binary"

  node -e "
const crypto = require('node:crypto');
const fs = require('node:fs');
const privateKey = crypto.createPrivateKey(fs.readFileSync('$VERIFY_TMP/testkey.pem', 'utf8'));
const bin = '$VERIFY_TMP/node_modules/$PLATFORM_PKG/fallow${ext}';
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
      return /fallow/.test(binaryPath) ? '0'.repeat(64) : digest;
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
  assert_contains "$DIGEST_BAD" "fallow" "install verify: digest mismatch names the offending binary"

  # sig-missing: binary present, .sig file absent (partial-deploy scenario,
  # most likely real-world failure mode after a botched release).
  rm -f "$VERIFY_TMP/node_modules/$PLATFORM_PKG/fallow${ext}.sig"
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
  assert_contains "$MISSING" "fallow" "install verify: missing .sig names the offending binary"

  # Restore a valid-length .sig so the skip-env test sees an otherwise
  # intact-but-wrong setup.
  node -e "
const fs = require('node:fs');
fs.writeFileSync('$VERIFY_TMP/node_modules/$PLATFORM_PKG/fallow${ext}.sig', Buffer.alloc(64));
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

cat > "$ANALYZE_TMP/bin/fallow" <<'SH'
#!/usr/bin/env bash
case "$*" in
  *"--help"*)
    printf '%s\n' 'Usage: fallow dead-code --sarif-file <PATH>'
    ;;
  *"--type-aware"*)
    if [ "${MOCK_TYPE_AWARE_INCOMPLETE:-false}" = "true" ]; then
      printf '%s\n' '{"total_issues":0,"_meta":{"type_aware":{"identity":{"completeness":"partial"},"queries":[]}}}'
      exit 1
    fi
    printf '%s\n' '{"total_issues":0,"_meta":{"type_aware":{"identity":{"completeness":"complete"},"queries":[]}}}'
    ;;
  *)
    printf '%s\n' '{"total_issues":0}'
    ;;
esac
SH
chmod +x "$ANALYZE_TMP/bin/fallow"

cat > "$ANALYZE_TMP/bin/git" <<'SH'
#!/usr/bin/env bash
[ -n "${FAKE_GIT_LOG:-}" ] && printf '%s\n' "$*" >> "$FAKE_GIT_LOG"
case "$1" in
  diff)
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
  cat-file)
    exit 0
    ;;
  fetch)
    exit 0
    ;;
  *)
    exit 0
    ;;
esac
SH
chmod +x "$ANALYZE_TMP/bin/git"

run_analyze_input_case() {
  local case_name=$1
  local changed_since=$2
  local diff_file=$3
  local baseline=${4:-}
  local work="$ANALYZE_TMP/input-$case_name"
  local output="$ANALYZE_TMP/input-output-$case_name"
  local env_file="$ANALYZE_TMP/input-env-$case_name"
  local git_log="$ANALYZE_TMP/input-git-$case_name"
  mkdir -p "$work"
  : > "$output"
  : > "$env_file"
  : > "$git_log"

  (
    cd "$work" || exit 1
    PATH="$ANALYZE_TMP/bin:$PATH" \
      FAKE_GIT_LOG="$git_log" \
      GITHUB_OUTPUT="$output" \
      GITHUB_ENV="$env_file" \
      INPUT_ROOT="." \
      INPUT_COMMAND="dead-code" \
      INPUT_FORMAT="json" \
      INPUT_AUTO_CHANGED_SINCE="false" \
      INPUT_CHANGED_SINCE="$changed_since" \
      FALLOW_DIFF_FILE="$diff_file" \
      INPUT_BASELINE="$baseline" \
      bash "$DIR/../scripts/analyze.sh"
  ) 2>&1
}

OUT=$(run_analyze_input_case "option-like-ref" "-main" "")
cmd_status=$?
if [ "$cmd_status" -eq 2 ]; then
  pass "analyze: rejects option-like changed-since"
else
  fail "analyze: rejects option-like changed-since" "expected exit 2, got $cmd_status"
fi
assert_contains "$OUT" "::error::changed-since must not begin with '-'" "analyze: option-like changed-since error is stable"
if [ ! -s "$ANALYZE_TMP/input-git-option-like-ref" ]; then
  pass "analyze: rejects option-like changed-since before Git"
else
  fail "analyze: rejects option-like changed-since before Git" "Git was invoked"
fi
if [ ! -s "$ANALYZE_TMP/input-output-option-like-ref" ] && [ ! -s "$ANALYZE_TMP/input-env-option-like-ref" ]; then
  pass "analyze: rejects option-like changed-since before file-command writes"
else
  fail "analyze: rejects option-like changed-since before file-command writes" "output or env file was modified"
fi

OUT=$(run_analyze_input_case "control-ref" $'main\ninjected=value' "")
cmd_status=$?
if [ "$cmd_status" -eq 2 ]; then
  pass "analyze: rejects control characters in changed-since"
else
  fail "analyze: rejects control characters in changed-since" "expected exit 2, got $cmd_status"
fi
assert_contains "$OUT" "::error::changed-since must not contain ASCII control characters" "analyze: changed-since control-character error is stable"
if [ ! -s "$ANALYZE_TMP/input-output-control-ref" ] && [ ! -s "$ANALYZE_TMP/input-env-control-ref" ]; then
  pass "analyze: rejects changed-since newline before file-command writes"
else
  fail "analyze: rejects changed-since newline before file-command writes" "output or env file was modified"
fi

OUT=$(run_analyze_input_case "control-diff" "" $'reports/diff\nINJECTED=value')
cmd_status=$?
if [ "$cmd_status" -eq 2 ]; then
  pass "analyze: rejects control characters in diff-file"
else
  fail "analyze: rejects control characters in diff-file" "expected exit 2, got $cmd_status"
fi
assert_contains "$OUT" "::error::diff-file must not contain ASCII control characters" "analyze: diff-file control-character error is stable"
if [ ! -s "$ANALYZE_TMP/input-output-control-diff" ] && [ ! -s "$ANALYZE_TMP/input-env-control-diff" ]; then
  pass "analyze: rejects diff-file newline before file-command writes"
else
  fail "analyze: rejects diff-file newline before file-command writes" "output or env file was modified"
fi

# The baseline path reaches `$GITHUB_OUTPUT` and the job summary, so a newline
# in it must stop the run before any file-command write (issue #2756).
OUT=$(run_analyze_input_case "control-baseline" "" "" $'baseline.json\ninjected=value')
cmd_status=$?
if [ "$cmd_status" -eq 2 ]; then
  pass "analyze: rejects control characters in baseline"
else
  fail "analyze: rejects control characters in baseline" "expected exit 2, got $cmd_status"
fi
assert_contains "$OUT" "::error::baseline must not contain ASCII control characters" "analyze: baseline control-character error is stable"
if [ ! -s "$ANALYZE_TMP/input-output-control-baseline" ] && [ ! -s "$ANALYZE_TMP/input-env-control-baseline" ]; then
  pass "analyze: rejects baseline newline before file-command writes"
else
  fail "analyze: rejects baseline newline before file-command writes" "output or env file was modified"
fi

VALID_DIFF="$ANALYZE_TMP/diff files/current change.patch"
mkdir -p "$(dirname "$VALID_DIFF")"
printf '%s\n' 'diff --git a/src/a.ts b/src/a.ts' > "$VALID_DIFF"
OUT=$(run_analyze_input_case "valid-scalars" "refs/remotes/origin/main~1" "$VALID_DIFF")
cmd_status=$?
if [ "$cmd_status" -eq 0 ]; then
  pass "analyze: preserves valid ref and spaced diff-file path"
else
  fail "analyze: preserves valid ref and spaced diff-file path" "exit $cmd_status, output: $OUT"
fi
assert_contains "$(cat "$ANALYZE_TMP/input-output-valid-scalars")" "changed_since=refs/remotes/origin/main~1" "analyze: preserves valid relative ref"
assert_contains "$(cat "$ANALYZE_TMP/input-env-valid-scalars")" "FALLOW_DIFF_FILE=$VALID_DIFF" "analyze: preserves diff-file path containing spaces"

TYPE_AWARE_WORK="$ANALYZE_TMP/type-aware-options"
mkdir -p "$TYPE_AWARE_WORK"
OUT=$(
  cd "$TYPE_AWARE_WORK" || exit 1
  PATH="$ANALYZE_TMP/bin:$PATH" \
    GITHUB_OUTPUT="$ANALYZE_TMP/type-aware-output" \
    GITHUB_ENV="$ANALYZE_TMP/type-aware-env" \
    INPUT_ROOT="." \
    INPUT_COMMAND="dead-code" \
    INPUT_FORMAT="json" \
    INPUT_AUTO_CHANGED_SINCE="false" \
    INPUT_TYPE_AWARE="true" \
    INPUT_TYPE_AWARE_PROJECTS="tsconfig.app.json,tsconfig.test.json" \
    INPUT_TYPE_AWARE_REQUIRE="complete" \
    bash "$DIR/../scripts/analyze.sh"
)
cmd_status=$?
if [ "$cmd_status" -eq 0 ]; then
  pass "analyze: type-aware inputs run successfully"
else
  fail "analyze: type-aware inputs run successfully" "exit $cmd_status, output: $OUT"
fi
ARGS=$(cat "$TYPE_AWARE_WORK/fallow-analysis-args.sh")
assert_contains "$ARGS" "--type-aware" "analyze: forwards type-aware"
assert_contains "$ARGS" "--type-aware-project tsconfig.app.json" "analyze: forwards first type-aware project"
assert_contains "$ARGS" "--type-aware-project tsconfig.test.json" "analyze: forwards second type-aware project"
assert_contains "$ARGS" "--type-aware-require complete" "analyze: forwards type-aware completeness policy"

TYPE_AWARE_INCOMPLETE_WORK="$ANALYZE_TMP/type-aware-incomplete"
mkdir -p "$TYPE_AWARE_INCOMPLETE_WORK"
OUT=$(
  cd "$TYPE_AWARE_INCOMPLETE_WORK" || exit 1
  PATH="$ANALYZE_TMP/bin:$PATH" \
    GITHUB_OUTPUT="$ANALYZE_TMP/type-aware-incomplete-output" \
    INPUT_ROOT="." \
    INPUT_COMMAND="dead-code" \
    INPUT_FORMAT="json" \
    INPUT_AUTO_CHANGED_SINCE="false" \
    INPUT_TYPE_AWARE="true" \
    INPUT_TYPE_AWARE_REQUIRE="complete" \
    MOCK_TYPE_AWARE_INCOMPLETE="true" \
    bash "$DIR/../scripts/analyze.sh" 2>&1
)
cmd_status=$?
if [ "$cmd_status" -eq 1 ]; then
  pass "analyze: complete policy fails partial semantic output with zero findings"
else
  fail "analyze: complete policy fails partial semantic output with zero findings" "expected exit 1, got $cmd_status"
fi
assert_contains "$OUT" "Type-aware completeness gate failed" "analyze: completeness failure is explicit"

run_analyze_scope_case() {
  local case_name=$1
  local changed_files=$2
  local baseline=$3
  local changed_since=$4
  local diff_file=$5
  local root=${6:-.}
  local config=${7:-}
  local work="$ANALYZE_TMP/scope-$case_name"
  mkdir -p "$work/$root"
  rm -f "$ANALYZE_TMP/output-$case_name" "$ANALYZE_TMP/env-$case_name"
  if [ -n "$diff_file" ]; then
    printf '%s\n' 'diff --git a/src/a.ts b/src/a.ts' > "$diff_file"
  fi

  (
    cd "$work" || exit 1
    PATH="$ANALYZE_TMP/bin:$PATH" \
      FAKE_CHANGED_FILES="$changed_files" \
      GITHUB_OUTPUT="$ANALYZE_TMP/output-$case_name" \
      GITHUB_ENV="$ANALYZE_TMP/env-$case_name" \
      INPUT_ROOT="$root" \
      INPUT_CONFIG="$config" \
      INPUT_COMMAND="dead-code" \
      INPUT_FORMAT="json" \
      INPUT_AUTO_CHANGED_SINCE="true" \
      INPUT_CHANGED_SINCE="$changed_since" \
      EVENT_NAME="pull_request" \
      PR_BASE_SHA="base1234" \
      INPUT_BASELINE="$baseline" \
      FALLOW_DIFF_FILE="$diff_file" \
      bash "$DIR/../scripts/analyze.sh"
  ) 2>&1
}

OUT=$(run_analyze_scope_case "config-baseline-auto" ".fallowrc.json" "baseline.json" "" "")
ARGS=$(cat "$ANALYZE_TMP/scope-config-baseline-auto/fallow-analysis-args.sh")
OUTPUTS=$(cat "$ANALYZE_TMP/output-config-baseline-auto")
ENV_OUT=$(cat "$ANALYZE_TMP/env-config-baseline-auto")
assert_not_contains "$ARGS" "--changed-since" "analyze: config baseline auto-scope removes changed-since"
if grep -qx 'changed_since=' "$ANALYZE_TMP/output-config-baseline-auto"; then
  pass "analyze: config baseline auto-scope clears changed_since output"
else
  fail "analyze: config baseline auto-scope clears changed_since output" "outputs were: $OUTPUTS"
fi
assert_not_contains "$ENV_OUT" "FALLOW_DIFF_FILE=" "analyze: config baseline auto-scope skips auto diff file"
assert_contains "$OUT" "dead-code baseline comparison is running unscoped because '.fallowrc.json' changed" "analyze: config baseline auto-scope warns"

OUT=$(run_analyze_scope_case "source-baseline-auto" "src/a.ts" "baseline.json" "" "")
ARGS=$(cat "$ANALYZE_TMP/scope-source-baseline-auto/fallow-analysis-args.sh")
OUTPUTS=$(cat "$ANALYZE_TMP/output-source-baseline-auto")
ENV_OUT=$(cat "$ANALYZE_TMP/env-source-baseline-auto")
assert_contains "$ARGS" "--changed-since base1234" "analyze: source baseline auto-scope keeps changed-since"
assert_contains "$OUTPUTS" "changed_since=base1234" "analyze: source baseline auto-scope keeps changed_since output"
assert_contains "$ENV_OUT" "FALLOW_DIFF_FILE=" "analyze: source baseline auto-scope writes auto diff file"

OUT=$(run_analyze_scope_case "config-no-baseline" ".fallowrc.json" "" "" "")
ARGS=$(cat "$ANALYZE_TMP/scope-config-no-baseline/fallow-analysis-args.sh")
OUTPUTS=$(cat "$ANALYZE_TMP/output-config-no-baseline")
assert_contains "$ARGS" "--changed-since base1234" "analyze: config without baseline keeps changed-since"
assert_contains "$OUTPUTS" "changed_since=base1234" "analyze: config without baseline keeps changed_since output"

OUT=$(run_analyze_scope_case "explicit-config-root-prefix" "packages/app/config/fallow.jsonc" "baseline.json" "" "" "packages/app" "config/fallow.jsonc")
ARGS=$(cat "$ANALYZE_TMP/scope-explicit-config-root-prefix/fallow-analysis-args.sh")
assert_not_contains "$ARGS" "--changed-since" "analyze: explicit config path with root prefix removes auto changed-since"
assert_contains "$OUT" "because 'config/fallow.jsonc' changed" "analyze: explicit config path warning uses root-relative path"

OUT=$(run_analyze_scope_case "config-explicit-changed-since" ".fallowrc.json" "baseline.json" "manual-base" "")
ARGS=$(cat "$ANALYZE_TMP/scope-config-explicit-changed-since/fallow-analysis-args.sh")
OUTPUTS=$(cat "$ANALYZE_TMP/output-config-explicit-changed-since")
assert_contains "$ARGS" "--changed-since manual-base" "analyze: explicit changed-since is preserved"
assert_contains "$OUTPUTS" "changed_since=manual-base" "analyze: explicit changed-since output is preserved"
assert_contains "$OUT" "explicitly scoped" "analyze: explicit changed-since warns about baseline drift"

EXPLICIT_DIFF="$ANALYZE_TMP/user.diff"
OUT=$(run_analyze_scope_case "config-explicit-diff" ".fallowrc.json" "baseline.json" "" "$EXPLICIT_DIFF")
ARGS=$(cat "$ANALYZE_TMP/scope-config-explicit-diff/fallow-analysis-args.sh")
OUTPUTS=$(cat "$ANALYZE_TMP/output-config-explicit-diff")
ENV_OUT=$(cat "$ANALYZE_TMP/env-config-explicit-diff")
assert_not_contains "$ARGS" "--changed-since base1234" "analyze: explicit diff still clears auto changed-since"
if grep -qx 'changed_since=' "$ANALYZE_TMP/output-config-explicit-diff"; then
  pass "analyze: explicit diff clears auto changed_since output"
else
  fail "analyze: explicit diff clears auto changed_since output" "outputs were: $OUTPUTS"
fi
assert_contains "$ENV_OUT" "FALLOW_DIFF_FILE=$EXPLICIT_DIFF" "analyze: explicit diff file is preserved"
assert_contains "$OUT" "explicit diff file remains active" "analyze: explicit diff warns about remaining scope"

OUT=$(run_analyze_scope_case "newline-filename" $'src/line\nbreak.ts' "" "" "")
CHANGED_FILE="$ANALYZE_TMP/scope-newline-filename/fallow-changed-files.json"
if jq -e 'length == 1 and .[0] == "src/line\nbreak.ts"' "$CHANGED_FILE" >/dev/null; then
  pass "analyze: changed-file JSON preserves newline-bearing filename"
else
  fail "analyze: changed-file JSON preserves newline-bearing filename" "got: $(cat "$CHANGED_FILE")"
fi

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

cat > "$ANALYZE_TMP/bin/fallow" <<'SH'
#!/usr/bin/env bash
printf '%s\n' '{"command":"audit","verdict":"fail","attribution":{"gate":"new-only","dead_code_introduced":0,"complexity_introduced":0,"duplication_introduced":0,"styling_introduced":1},"summary":{"dead_code_issues":0,"complexity_findings":0,"duplication_clone_groups":0},"complexity":{"styling_findings":[{"effective_severity":"error","introduced":true}]}}'
SH
chmod +x "$ANALYZE_TMP/bin/fallow"
cd "$ANALYZE_TMP/work" && rm -f "$ANALYZE_TMP/output"
OUT=$(PATH="$ANALYZE_TMP/bin:$PATH" GITHUB_OUTPUT="$ANALYZE_TMP/output" \
  INPUT_ROOT="." INPUT_COMMAND="audit" INPUT_FORMAT="json" \
  bash "$DIR/../scripts/analyze.sh" 2>&1) || true
cd "$DIR"
ISSUES=$(grep '^issues=' "$ANALYZE_TMP/output" | cut -d= -f2)
[ "$ISSUES" = "1" ] && pass "analyze: new-only audit counts introduced styling findings" || fail "analyze: new-only styling issues" "expected 1, got '$ISSUES'"

cat > "$ANALYZE_TMP/bin/fallow" <<'SH'
#!/usr/bin/env bash
printf '%s\n' '{"command":"audit","verdict":"fail","attribution":{"gate":"all"},"summary":{"dead_code_issues":0,"complexity_findings":0,"duplication_clone_groups":0},"complexity":{"styling_findings":[{"effective_severity":"error"},{"effective_severity":"warn"}]}}'
SH
chmod +x "$ANALYZE_TMP/bin/fallow"
cd "$ANALYZE_TMP/work" && rm -f "$ANALYZE_TMP/output"
OUT=$(PATH="$ANALYZE_TMP/bin:$PATH" GITHUB_OUTPUT="$ANALYZE_TMP/output" \
  INPUT_ROOT="." INPUT_COMMAND="audit" INPUT_FORMAT="json" \
  bash "$DIR/../scripts/analyze.sh" 2>&1) || true
cd "$DIR"
ISSUES=$(grep '^issues=' "$ANALYZE_TMP/output" | cut -d= -f2)
[ "$ISSUES" = "2" ] && pass "analyze: all-gate audit counts every styling finding" || fail "analyze: all-gate styling issues" "expected 2, got '$ISSUES'"

cat > "$ANALYZE_TMP/bin/fallow" <<'SH'
#!/usr/bin/env bash
case "$*" in
  *"security"*"--gate newly-reachable"*)
    printf '%s\n' '{"kind":"security","gate":{"mode":"newly-reachable","verdict":"fail","new_count":2},"summary":{"security_findings":5},"security_findings":[]}'
    ;;
  *"security"*)
    printf '%s\n' '{"kind":"security","summary":{"security_findings":3},"security_findings":[]}'
    ;;
  *) printf '{"total_issues":0}\n' ;;
esac
SH
chmod +x "$ANALYZE_TMP/bin/fallow"

cd "$ANALYZE_TMP/work" && rm -f "$ANALYZE_TMP/output"
OUT=$(PATH="$ANALYZE_TMP/bin:$PATH" GITHUB_OUTPUT="$ANALYZE_TMP/output" \
  INPUT_ROOT="." INPUT_COMMAND="security" INPUT_FORMAT="json" INPUT_SECURITY_GATE="newly-reachable" \
  bash "$DIR/../scripts/analyze.sh" 2>&1) || true
cd "$DIR"
GATE=$(grep '^gate=' "$ANALYZE_TMP/output" | cut -d= -f2)
ISSUES=$(grep '^issues=' "$ANALYZE_TMP/output" | cut -d= -f2)
ARGS=$(cat "$ANALYZE_TMP/work/fallow-analysis-args.sh")
[ "$GATE" = "newly-reachable" ] && pass "analyze: emits gate to GITHUB_OUTPUT for security" || fail "analyze: security gate output" "expected newly-reachable, got '$GATE'"
[ "$ISSUES" = "2" ] && pass "analyze: security gate uses new_count for issues" || fail "analyze: security gate issues" "expected 2, got '$ISSUES'"
assert_contains "$ARGS" "--gate newly-reachable" "analyze: forwards security-gate to fallow"

cd "$ANALYZE_TMP/work" && rm -f "$ANALYZE_TMP/output"
OUT=$(PATH="$ANALYZE_TMP/bin:$PATH" GITHUB_OUTPUT="$ANALYZE_TMP/output" \
  INPUT_ROOT="." INPUT_COMMAND="security" INPUT_FORMAT="json" INPUT_SECURITY_GATE="all" \
  bash "$DIR/../scripts/analyze.sh" 2>&1)
cmd_status=$?
cd "$DIR"
if [ "$cmd_status" -eq 2 ]; then
  pass "analyze: rejects invalid security-gate"
else
  fail "analyze: rejects invalid security-gate" "expected exit 2, got $cmd_status"
fi
assert_contains "$OUT" "security-gate must be 'new' or 'newly-reachable'" "analyze: invalid security-gate error is clear"

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

cd "$ANALYZE_TMP/work" && rm -f "$ANALYZE_TMP/output"
OUT=$(PATH="$ANALYZE_TMP/bin:$PATH" GITHUB_OUTPUT="$ANALYZE_TMP/output" \
  INPUT_ROOT="." INPUT_COMMAND="" INPUT_FORMAT="json" \
  INPUT_COVERAGE="coverage/coverage-final.json" INPUT_COVERAGE_ROOT="/ci/workspace" \
  bash "$DIR/../scripts/analyze.sh" 2>&1) || true
cd "$DIR"
ARGS=$(cat "$ANALYZE_TMP/work/fallow-analysis-args.sh")
assert_contains "$ARGS" "--coverage coverage/coverage-final.json" "analyze: forwards coverage to default combined command"
assert_contains "$ARGS" "--coverage-root /ci/workspace" "analyze: forwards coverage-root to default combined command"

cd "$ANALYZE_TMP/work" && rm -f "$ANALYZE_TMP/output" "$ANALYZE_TMP/github-env"
OUT=$(PATH="$ANALYZE_TMP/bin:$PATH" GITHUB_OUTPUT="$ANALYZE_TMP/output" \
  GITHUB_ENV="$ANALYZE_TMP/github-env" INPUT_ROOT="." INPUT_COMMAND="dead-code" \
  INPUT_FORMAT="json" INPUT_ARGS="--report-path-prefix custom/base --unused-files" \
  bash "$DIR/../scripts/analyze.sh" 2>&1) || true
cd "$DIR"
ARGS=$(cat "$ANALYZE_TMP/work/fallow-analysis-args.sh")
RENDER_ENV=$(cat "$ANALYZE_TMP/github-env")
assert_not_contains "$ARGS" "--report-path-prefix" "analyze: keeps renderer prefix out of JSON analysis args"
assert_contains "$ARGS" "--unused-files" "analyze: preserves non-presentation extra args"
assert_contains "$RENDER_ENV" "FALLOW_RENDER_PATH_PREFIX_SET=1" "analyze: marks explicit renderer prefix"
assert_contains "$RENDER_ENV" "FALLOW_RENDER_PATH_PREFIX=custom/base" "analyze: propagates renderer prefix"
assert_contains "$RENDER_ENV" "FALLOW_ANALYSIS_ARGS_JSON=[" "analyze: propagates safe JSON fallback arguments"
assert_contains "$RENDER_ENV" '"--unused-files"' "analyze: JSON fallback arguments preserve non-presentation flags"

cd "$ANALYZE_TMP/work"
OUT=$(PATH="$ANALYZE_TMP/bin:$PATH" GITHUB_OUTPUT="$ANALYZE_TMP/output" \
  GITHUB_ENV="$ANALYZE_TMP/github-env" INPUT_ROOT="." INPUT_COMMAND="dead-code" \
  INPUT_FORMAT="json" bash "$DIR/../scripts/analyze.sh" 2>&1) || true
cd "$DIR"
RENDER_ENV_TAIL=$(tail -3 "$ANALYZE_TMP/github-env")
assert_contains "$RENDER_ENV_TAIL" "FALLOW_RENDER_PATH_PREFIX_SET=0" "analyze: clears a prior renderer prefix on the next invocation"
assert_contains "$RENDER_ENV_TAIL" "FALLOW_RENDER_PATH_PREFIX=" "analyze: clears the prior renderer prefix value"

cat > "$ANALYZE_TMP/bin/fallow" <<'SH'
#!/usr/bin/env bash
case "$*" in
  *"--help"*)
    printf '%s\n' 'Usage: fallow dead-code --sarif-file <PATH>'
    ;;
  *)
    printf '%s\n' '{"check":{"total_issues":0},"dupes":{"clone_groups":[],"clone_families":[],"stats":{"clone_groups":2,"clone_instances":5,"files_with_clones":4,"duplicated_lines":59,"duplication_percentage":0.16}},"health":{"summary":{"functions_above_threshold":0},"runtime_coverage":{"findings":[]}}}'
    ;;
esac
SH
chmod +x "$ANALYZE_TMP/bin/fallow"
cd "$ANALYZE_TMP/work" && rm -f "$ANALYZE_TMP/output"
OUT=$(PATH="$ANALYZE_TMP/bin:$PATH" GITHUB_OUTPUT="$ANALYZE_TMP/output" \
  INPUT_ROOT="." INPUT_COMMAND="" INPUT_FORMAT="json" \
  bash "$DIR/../scripts/analyze.sh" 2>&1) || true
cd "$DIR"
ISSUES=$(grep '^issues=' "$ANALYZE_TMP/output" | cut -d= -f2)
[ "$ISSUES" = "0" ] && pass "analyze: combined empty dupes groups ignore nonzero stats" || fail "analyze: combined empty dupes groups" "expected 0, got '$ISSUES'"
assert_not_contains "$OUT" "Fallow found 2 issues" "analyze: combined empty dupes groups do not fail"

# Issue #735: generated artifacts can be moved out of the workspace root.
cat > "$ANALYZE_TMP/bin/fallow" <<'SH'
#!/usr/bin/env bash
case "$*" in
  *"--help"*)
    printf '%s\n' 'Usage: fallow dead-code --sarif-file <PATH>'
    ;;
  *"--format sarif"*)
    printf '%s\n' '{"version":"2.1.0","runs":[{"results":[{"ruleId":"fallow/test"}]}]}'
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

# Issue #813: the saved-envelope SARIF renderer must validate the produced file,
# not gate on the exit code. The primary analysis may exit 1 when findings exist.
# The renderer must still produce valid SARIF without a second health analysis.
cat > "$ANALYZE_TMP/bin/fallow" <<'SH'
#!/usr/bin/env bash
[ -n "${MOCK_CALL_LOG:-}" ] && printf '%s\n' "$*" >> "$MOCK_CALL_LOG"
if [ "${1:-}" = "report" ] && [ "${2:-}" = "--help" ]; then
  printf '%s\n' 'Usage: fallow report --from <PATH>'
  exit 0
fi
fmt=""
prev=""
for arg in "$@"; do
  [ "$prev" = "--format" ] && fmt="$arg"
  prev="$arg"
done
if [ "$fmt" = "sarif" ]; then
  if [ "${MOCK_SARIF_MODE:-valid}" = "valid" ] || \
     { [ "${MOCK_SARIF_MODE:-valid}" = "report-empty-direct-valid" ] && [ "${1:-}" != "report" ]; }; then
    printf '%s\n' '{"version":"2.1.0","runs":[{"tool":{"driver":{"name":"fallow"}},"results":[{"ruleId":"fallow/test"}]}]}'
  fi
  # exit 1 = issues found (valid mode) OR genuine failure (empty mode writes nothing)
  exit 1
fi
# Primary run is always --format json.
printf '%s\n' '{"summary":{"functions_above_threshold":0}}'
exit 1
SH
chmod +x "$ANALYZE_TMP/bin/fallow"

SARIF_OK_WORK="$ANALYZE_TMP/sarif-exit1-valid"
mkdir -p "$SARIF_OK_WORK"
cd "$SARIF_OK_WORK" && rm -f "$ANALYZE_TMP/output"
rm -f "$ANALYZE_TMP/sarif-calls"
OUT=$(PATH="$ANALYZE_TMP/bin:$PATH" GITHUB_OUTPUT="$ANALYZE_TMP/output" \
  INPUT_ROOT="." INPUT_COMMAND="health" INPUT_FORMAT="sarif" MOCK_SARIF_MODE="valid" \
  MOCK_CALL_LOG="$ANALYZE_TMP/sarif-calls" \
  bash "$DIR/../scripts/analyze.sh" 2>&1) || true
cd "$DIR"
assert_not_contains "$OUT" "produced no SARIF document" "analyze: valid SARIF + exit 1 does not warn (issue #813)"
[ -s "$SARIF_OK_WORK/fallow-results.sarif" ] && pass "analyze: valid SARIF + exit 1 still writes the file" || fail "analyze: valid SARIF + exit 1 still writes the file" "missing sarif file"
HEALTH_ANALYSIS_CALLS=$(grep -c '^health ' "$ANALYZE_TMP/sarif-calls" || true)
[ "$HEALTH_ANALYSIS_CALLS" -eq 1 ] && pass "analyze: SARIF reuses the saved JSON analysis" || fail "analyze: SARIF reuses the saved JSON analysis" "expected one health analysis, got $HEALTH_ANALYSIS_CALLS"

SARIF_BAD_WORK="$ANALYZE_TMP/sarif-empty"
mkdir -p "$SARIF_BAD_WORK"
cd "$SARIF_BAD_WORK" && rm -f "$ANALYZE_TMP/output"
OUT=$(PATH="$ANALYZE_TMP/bin:$PATH" GITHUB_OUTPUT="$ANALYZE_TMP/output" \
  INPUT_ROOT="." INPUT_COMMAND="health" INPUT_FORMAT="sarif" MOCK_SARIF_MODE="empty" \
  bash "$DIR/../scripts/analyze.sh" 2>&1) || true
cd "$DIR"
assert_contains "$OUT" "produced no SARIF document" "analyze: empty/invalid SARIF still warns (issue #813)"
[ ! -e "$SARIF_BAD_WORK/fallow-results.sarif" ] && pass "analyze: empty SARIF is not published" || fail "analyze: empty SARIF is not published" "empty SARIF file remains"
# #2690: the step stays green and uploads nothing, so the warning has to say
# what that costs rather than only that something failed.
assert_contains "$OUT" "code scanning keeps the alerts from the previous upload" \
  "analyze: the missing-SARIF warning names the consequence"
assert_not_contains "$(cat "$ANALYZE_TMP/output" 2>/dev/null || true)" "sarif=fallow-results.sarif" \
  "analyze: a missing SARIF artefact sets no upload output"


SARIF_COMPAT_WORK="$ANALYZE_TMP/sarif-report-empty-direct-valid"
mkdir -p "$SARIF_COMPAT_WORK"
cd "$SARIF_COMPAT_WORK" && rm -f "$ANALYZE_TMP/output"
rm -f "$ANALYZE_TMP/sarif-compat-calls"
OUT=$(PATH="$ANALYZE_TMP/bin:$PATH" GITHUB_OUTPUT="$ANALYZE_TMP/output" \
  INPUT_ROOT="." INPUT_COMMAND="health" INPUT_FORMAT="sarif" \
  MOCK_SARIF_MODE="report-empty-direct-valid" \
  MOCK_CALL_LOG="$ANALYZE_TMP/sarif-compat-calls" \
  bash "$DIR/../scripts/analyze.sh" 2>&1) || true
cd "$DIR"
assert_not_contains "$OUT" "produced no SARIF document" "analyze: report-capable older binary falls back to direct SARIF"
[ -s "$SARIF_COMPAT_WORK/fallow-results.sarif" ] && pass "analyze: compatibility fallback publishes valid SARIF" || fail "analyze: compatibility fallback publishes valid SARIF" "missing sarif file"
HEALTH_COMPAT_CALLS=$(grep -c '^health ' "$ANALYZE_TMP/sarif-compat-calls" || true)
[ "$HEALTH_COMPAT_CALLS" -eq 2 ] && pass "analyze: compatibility fallback reruns only when saved rendering fails" || fail "analyze: compatibility fallback reruns only when saved rendering fails" "expected two health calls, got $HEALTH_COMPAT_CALLS"
# #2690: when the binary recorded why, the warning repeats its sentence rather
# than making the reader turn on step debugging to find it.
SARIF_REASON_WORK="$ANALYZE_TMP/sarif-reason"
mkdir -p "$SARIF_REASON_WORK"
cd "$SARIF_REASON_WORK" && rm -f "$ANALYZE_TMP/output"
cat > "$ANALYZE_TMP/bin/fallow" <<'SH'
#!/usr/bin/env bash
if [ "${1:-}" = "--version" ]; then echo "fallow 9.9.9"; exit 0; fi
if [ "${1:-}" = "--help" ] || [ "${2:-}" = "--help" ]; then echo "--sarif-file"; echo "report"; exit 0; fi
fmt=""
prev=""
for arg in "$@"; do
  [ "$prev" = "--format" ] && fmt="$arg"
  prev="$arg"
done
if [ "$fmt" = "sarif" ]; then exit 1; fi
printf '%s\n' '{"summary":{"functions_above_threshold":0},"request_outcomes":{"sarif-file":{"status":"not-applied","affects":"artifact","requested":"fallow-results.sarif","reason":"write-failed","message":"failed to write SARIF file: Permission denied."}}}'
exit 1
SH
chmod +x "$ANALYZE_TMP/bin/fallow"
OUT=$(PATH="$ANALYZE_TMP/bin:$PATH" GITHUB_OUTPUT="$ANALYZE_TMP/output" \
  INPUT_ROOT="." INPUT_COMMAND="health" INPUT_FORMAT="sarif" \
  bash "$DIR/../scripts/analyze.sh" 2>&1) || true
cd "$DIR"
assert_contains "$OUT" "failed to write SARIF file: Permission denied." \
  "analyze: the warning repeats the reason the envelope recorded"
assert_not_contains "$OUT" "could not apply" \
  "analyze: a failed SARIF write is not reported as a run wider than requested"

# --- Envelope reads keep their cause in the step log (issue #2740) ---
# Every envelope read discarded the stderr of `jq`, so a member of the wrong
# type read as "no findings" and the run went green with no cause in the log.
cat > "$ANALYZE_TMP/bin/fallow" <<'SH'
#!/usr/bin/env bash
case "$*" in
  *"--help"*) printf '%s\n' 'Usage: fallow dead-code' ;;
  *) printf '%s\n' '{"kind":"dead-code","total_issues":0,"gate_outcomes":"truncated","workspace_diagnostics":"truncated"}' ;;
esac
SH
chmod +x "$ANALYZE_TMP/bin/fallow"
cd "$ANALYZE_TMP/work" && rm -f "$ANALYZE_TMP/output"
OUT=$(PATH="$ANALYZE_TMP/bin:$PATH" GITHUB_OUTPUT="$ANALYZE_TMP/output" \
  INPUT_ROOT="." INPUT_COMMAND="dead-code" INPUT_FORMAT="json" \
  bash "$DIR/../scripts/analyze.sh" 2>&1) || true
cd "$DIR"
assert_contains "$OUT" "::debug::jq:" \
  "analyze: a failed envelope read replays its cause as a debug line"
assert_contains "$OUT" "has no keys" \
  "analyze: the replayed line names what jq could not read"

cat > "$ANALYZE_TMP/bin/fallow" <<'SH'
#!/usr/bin/env bash
case "$*" in
  *"--help"*) printf '%s\n' 'Usage: fallow dead-code' ;;
  *) printf '%s\n' '{"kind":"dead-code","total_issues":0,"gate_outcomes":{},"workspace_diagnostics":[],"request_outcomes":{}}' ;;
esac
SH
chmod +x "$ANALYZE_TMP/bin/fallow"
cd "$ANALYZE_TMP/work" && rm -f "$ANALYZE_TMP/output"
OUT=$(PATH="$ANALYZE_TMP/bin:$PATH" GITHUB_OUTPUT="$ANALYZE_TMP/output" \
  INPUT_ROOT="." INPUT_COMMAND="dead-code" INPUT_FORMAT="json" \
  bash "$DIR/../scripts/analyze.sh" 2>&1) || true
cd "$DIR"
assert_not_contains "$OUT" "::debug::jq:" \
  "analyze: a run without a failed envelope read logs no debug line"

# A replayed capture cannot start a workflow command of its own. The replay
# prefixes each line, so a `::error::` sequence stays inside the `::debug::`
# line, and a second captured line keeps the prefix. `jq -s 'last'` collapses
# the raw output before the envelope reads, so each read reports one line; the
# multi-line capture comes from the capability probe.
cat > "$ANALYZE_TMP/bin/fallow" <<'SH'
#!/usr/bin/env bash
case "$*" in
  *"--help"*)
    printf '%s\n' 'probe wrote line one' '::error::probe wrote line two' >&2
    printf '%s\n' 'Usage: fallow dead-code'
    ;;
  *) printf '%s\n' '{"kind":"dead-code","total_issues":0,"gate_outcomes":"::error::x"}' ;;
esac
SH
chmod +x "$ANALYZE_TMP/bin/fallow"
cd "$ANALYZE_TMP/work" && rm -f "$ANALYZE_TMP/output"
OUT=$(PATH="$ANALYZE_TMP/bin:$PATH" GITHUB_OUTPUT="$ANALYZE_TMP/output" \
  INPUT_ROOT="." INPUT_COMMAND="dead-code" INPUT_FORMAT="json" \
  bash "$DIR/../scripts/analyze.sh" 2>&1) || true
cd "$DIR"
assert_contains "$OUT" '::debug::jq: jq: error' \
  "analyze: a replayed jq line keeps the debug prefix"
# jq 1.6 truncates a value in its error text after eleven characters, so the
# forged value is short enough to survive on every runner image.
assert_contains "$OUT" 'string ("::error::x") has no keys' \
  "analyze: the replayed line carries the text jq could not read"
assert_contains "$OUT" '::debug::fallow dead-code --help: ::error::probe wrote line two' \
  "analyze: the second captured line keeps the debug prefix"
LOOSE_SEQUENCE_LINES=$(printf '%s\n' "$OUT" |
  grep -e '::error::x' | grep -cv '^::debug::jq: ' || true)
if [ "$LOOSE_SEQUENCE_LINES" -eq 0 ]; then
  pass "analyze: a workflow command in the jq text stays inside the debug line"
else
  fail "analyze: a workflow command in the jq text stays inside the debug line" \
    "$LOOSE_SEQUENCE_LINES lines carry the sequence without the prefix"
fi
STRAY_REPLAY_LINES=$(printf '%s\n' "$OUT" |
  grep -c -e '^::error::' -e '^jq: error' -e '^probe wrote' || true)
if [ "$STRAY_REPLAY_LINES" -eq 0 ]; then
  pass "analyze: no replayed line starts a line of its own"
else
  fail "analyze: no replayed line starts a line of its own" \
    "found $STRAY_REPLAY_LINES unprefixed lines"
fi

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

OUT_POLICY=$(jq '.policy_violations = [{"path": "src/app.ts", "line": 7, "col": 2, "pack": "team-policy", "rule_id": "no-moment", "kind": "banned-import", "matched": "moment", "severity": "error", "actions": []}] | .total_issues = (.total_issues + 1)' "$FIXTURES/check.json" | jq -r -f "$JQ_DIR/summary-check.jq" 2>&1)
assert_contains "$OUT_POLICY" "Policy violations" "policy: shows summary row and section"
assert_contains "$OUT_POLICY" "team-policy/no-moment" "policy: shows pack/rule identity"

OUT_POLICY_ANNOTATIONS=$(jq '.policy_violations = [{"path": "src/app.ts", "line": 7, "col": 2, "pack": "team-policy", "rule_id": "no-moment", "kind": "banned-import", "matched": "moment", "severity": "error", "message": "Use date-fns.", "actions": []}]' "$FIXTURES/check.json" | jq -r -f "$JQ_DIR/annotations-check.jq" 2>&1)
assert_contains "$OUT_POLICY_ANNOTATIONS" "::error file=src/app.ts,line=7,col=3,title=Policy violation::" "policy: error-severity annotation"
assert_contains "$OUT_POLICY_ANNOTATIONS" "banned by rule 'team-policy/no-moment'" "policy: annotation names the rule"
assert_contains "$OUT_POLICY_ANNOTATIONS" "Use date-fns." "policy: annotation carries the rule message"

# Gate severity: `effective_severity` sets the annotation level. Without the
# field (older binaries), the fallback keeps the historical level.
OUT_GATE_ANN=$(jq '.unused_exports = [
    {"path": "src/error.ts", "line": 2, "col": 13, "export_name": "a", "is_type_only": false, "is_re_export": false, "effective_severity": "error", "actions": []},
    {"path": "src/warn.ts", "line": 2, "col": 13, "export_name": "b", "is_type_only": false, "is_re_export": false, "effective_severity": "warn", "actions": []},
    {"path": "src/legacy.ts", "line": 2, "col": 13, "export_name": "c", "is_type_only": false, "is_re_export": false, "actions": []}
  ]
  | .unused_files = [{"path": "src/orphan.ts", "effective_severity": "error", "actions": []}]
  | .unlisted_dependencies = [{"package_name": "chalk", "effective_severity": "error", "imported_from": [{"path": "src/cli.ts", "line": 3, "col": 0}]}]
  | .duplicate_exports = [{"export_name": "fmt", "effective_severity": "error", "locations": [{"path": "src/a.ts", "line": 1, "col": 0}, {"path": "src/b.ts", "line": 1, "col": 0}]}]
  | .unresolved_catalog_references = [.unresolved_catalog_references[0] + {"effective_severity": "warn"}]' "$FIXTURES/check.json" | jq -r -f "$JQ_DIR/annotations-check.jq" 2>&1)
assert_contains "$OUT_GATE_ANN" "::error file=src/error.ts,line=2,col=14,title=Unused export::" "gate: effective_severity error gives ::error"
assert_contains "$OUT_GATE_ANN" "::warning file=src/warn.ts,line=2,col=14,title=Unused export::" "gate: effective_severity warn gives ::warning"
assert_contains "$OUT_GATE_ANN" "::warning file=src/legacy.ts,line=2,col=14,title=Unused export::" "gate: missing effective_severity keeps ::warning"
assert_contains "$OUT_GATE_ANN" "::error file=src/orphan.ts,title=Unused file::" "gate: file-anchored finding follows effective_severity"
assert_contains "$OUT_GATE_ANN" "::error file=src/cli.ts,line=3,col=1,title=Unlisted dependency::" "gate: per-site annotation reads the finding severity"
assert_contains "$OUT_GATE_ANN" "::error file=src/b.ts,line=1,col=1,title=Duplicate export::" "gate: per-location annotation reads the finding severity"
assert_contains "$OUT_GATE_ANN" "::warning file=packages/app/package.json,line=14,title=Unresolved catalog reference::" "gate: warn lowers a default-error kind"
OUT_GATE_LEGACY_CATALOG=$(jq -r -f "$JQ_DIR/annotations-check.jq" "$FIXTURES/check.json" 2>&1)
assert_contains "$OUT_GATE_LEGACY_CATALOG" "::error file=packages/app/package.json,line=14,title=Unresolved catalog reference::" "gate: missing effective_severity keeps the legacy ::error"
OUT_GATE_UNKNOWN=$(jq '.unresolved_catalog_references = [.unresolved_catalog_references[0] + {"effective_severity": "info"}]
  | .unused_exports = [{"path": "src/unknown.ts", "line": 2, "col": 13, "export_name": "u", "is_type_only": false, "is_re_export": false, "effective_severity": "info", "actions": []}]' "$FIXTURES/check.json" | jq -r -f "$JQ_DIR/annotations-check.jq" 2>&1)
assert_contains "$OUT_GATE_UNKNOWN" "::error file=packages/app/package.json,line=14,title=Unresolved catalog reference::" "gate: unknown effective_severity keeps the legacy ::error"
assert_contains "$OUT_GATE_UNKNOWN" "::warning file=src/unknown.ts,line=2,col=14,title=Unused export::" "gate: unknown effective_severity keeps the legacy ::warning"

OUT_POLICY_FILTERED=$(jq '.policy_violations = [{"path": "src/app.ts", "line": 7, "col": 2, "pack": "team-policy", "rule_id": "no-moment", "kind": "banned-import", "matched": "moment", "severity": "warn", "actions": []}, {"path": "src/other.ts", "line": 1, "col": 0, "pack": "team-policy", "rule_id": "no-moment", "kind": "banned-import", "matched": "moment", "severity": "warn", "actions": []}]' "$FIXTURES/check.json" | jq --argjson changed '["src/app.ts"]' -f "$JQ_DIR/filter-changed.jq" 2>&1)
assert_json_value "$OUT_POLICY_FILTERED" '.policy_violations | length' "1" "policy: filter-changed keeps only changed-file findings"

OUT_ICE=$(jq '.invalid_client_exports = [{"path": "src/app.ts", "line": 5, "col": 0, "export_name": "metadata", "directive": "use client", "actions": []}] | .total_issues = (.total_issues + 1)' "$FIXTURES/check.json" | jq -r -f "$JQ_DIR/summary-check.jq" 2>&1)
assert_contains "$OUT_ICE" "Invalid client exports" "ice: shows summary row and section"
assert_contains "$OUT_ICE" "metadata" "ice: shows export name in section"

OUT_ICE_ANN=$(jq '.invalid_client_exports = [{"path": "src/app.ts", "line": 5, "col": 2, "export_name": "metadata", "directive": "use client", "actions": []}]' "$FIXTURES/check.json" | jq -r -f "$JQ_DIR/annotations-check.jq" 2>&1)
assert_contains "$OUT_ICE_ANN" "::warning file=src/app.ts,line=5,col=3,title=Invalid client export::" "ice: warning-severity annotation"

OUT_ICE_FILTERED=$(jq '.invalid_client_exports = [{"path": "src/app.ts", "line": 5, "col": 0, "export_name": "metadata", "directive": "use client", "actions": []}, {"path": "src/other.ts", "line": 3, "col": 0, "export_name": "generateMetadata", "directive": "use client", "actions": []}]' "$FIXTURES/check.json" | jq --argjson changed '["src/app.ts"]' -f "$JQ_DIR/filter-changed.jq" 2>&1)
assert_json_value "$OUT_ICE_FILTERED" '.invalid_client_exports | length' "1" "ice: filter-changed keeps only changed-file findings"

OUT_MCSB=$(jq '.mixed_client_server_barrels = [{"path": "src/index.ts", "line": 2, "col": 0, "client_origin": "./Button", "server_origin": "./fetchUser", "actions": []}] | .total_issues = (.total_issues + 1)' "$FIXTURES/check.json" | jq -r -f "$JQ_DIR/summary-check.jq" 2>&1)
assert_contains "$OUT_MCSB" "Mixed client/server barrels" "mcsb: shows summary row and section"
assert_contains "$OUT_MCSB" "./fetchUser" "mcsb: shows server origin in section"

OUT_MCSB_ANN=$(jq '.mixed_client_server_barrels = [{"path": "src/index.ts", "line": 2, "col": 2, "client_origin": "./Button", "server_origin": "./fetchUser", "actions": []}]' "$FIXTURES/check.json" | jq -r -f "$JQ_DIR/annotations-check.jq" 2>&1)
assert_contains "$OUT_MCSB_ANN" "::warning file=src/index.ts,line=2,col=3,title=Mixed client/server barrel::" "mcsb: warning-severity annotation"

OUT_MCSB_FILTERED=$(jq '.mixed_client_server_barrels = [{"path": "src/index.ts", "line": 2, "col": 0, "client_origin": "./Button", "server_origin": "./fetchUser", "actions": []}, {"path": "src/other/index.ts", "line": 1, "col": 0, "client_origin": "./Widget", "server_origin": "./loadData", "actions": []}]' "$FIXTURES/check.json" | jq --argjson changed '["src/index.ts"]' -f "$JQ_DIR/filter-changed.jq" 2>&1)
assert_json_value "$OUT_MCSB_FILTERED" '.mixed_client_server_barrels | length' "1" "mcsb: filter-changed keeps only changed-file findings"

OUT_MD=$(jq '.misplaced_directives = [{"path": "src/widget.tsx", "line": 4, "col": 0, "directive": "use client", "actions": []}] | .total_issues = (.total_issues + 1)' "$FIXTURES/check.json" | jq -r -f "$JQ_DIR/summary-check.jq" 2>&1)
assert_contains "$OUT_MD" "Misplaced directives" "md: shows summary row and section"
assert_contains "$OUT_MD" "use client" "md: shows directive in section"

OUT_MD_ANN=$(jq '.misplaced_directives = [{"path": "src/widget.tsx", "line": 4, "col": 2, "directive": "use client", "actions": []}]' "$FIXTURES/check.json" | jq -r -f "$JQ_DIR/annotations-check.jq" 2>&1)
assert_contains "$OUT_MD_ANN" "::warning file=src/widget.tsx,line=4,col=3,title=Misplaced directive::" "md: warning-severity annotation"

OUT_MD_FILTERED=$(jq '.misplaced_directives = [{"path": "src/widget.tsx", "line": 4, "col": 0, "directive": "use client", "actions": []}, {"path": "src/other.tsx", "line": 6, "col": 0, "directive": "use server", "actions": []}]' "$FIXTURES/check.json" | jq --argjson changed '["src/widget.tsx"]' -f "$JQ_DIR/filter-changed.jq" 2>&1)
assert_json_value "$OUT_MD_FILTERED" '.misplaced_directives | length' "1" "md: filter-changed keeps only changed-file findings"

# Directive column renders with the surrounding quotes from the `\"\(.directive)\"` template.
# Asserting the export-cell + directive-cell pair so a regression in quote escaping is caught
# (the bare "use client" string also appears in the section header text).
assert_contains "$OUT_ICE" '`metadata` | `"use client"` |' "ice: directive column renders with surrounding quotes"
# `"use server"` directive path (the section description mentions both, so a use-server-only
# fixture proves the row template, not just the header text).
OUT_MD_SERVER=$(jq '.misplaced_directives = [{"path": "src/action.ts", "line": 3, "col": 0, "directive": "use server", "actions": []}] | .total_issues = (.total_issues + 1)' "$FIXTURES/check.json" | jq -r -f "$JQ_DIR/summary-check.jq" 2>&1)
assert_contains "$OUT_MD_SERVER" '`"use server"` |' "md: use-server directive renders in section row"

# Vue/Next framework IssueKinds: summary row + section + annotation + filter parity.
OUT_USA=$(jq '.unused_server_actions = [{"path": "src/actions.ts", "line": 9, "col": 0, "action_name": "submitForm", "actions": []}] | .total_issues = (.total_issues + 1)' "$FIXTURES/check.json" | jq -r -f "$JQ_DIR/summary-check.jq" 2>&1)
assert_contains "$OUT_USA" "Unused server actions" "usa: shows summary row and section"
assert_contains "$OUT_USA" "submitForm" "usa: shows action name in section"
OUT_USA_ANN=$(jq '.unused_server_actions = [{"path": "src/actions.ts", "line": 9, "col": 2, "action_name": "submitForm", "actions": []}]' "$FIXTURES/check.json" | jq -r -f "$JQ_DIR/annotations-check.jq" 2>&1)
assert_contains "$OUT_USA_ANN" "::warning file=src/actions.ts,line=9,col=3,title=Unused server action::" "usa: warning-severity annotation"
OUT_USA_FILTERED=$(jq '.unused_server_actions = [{"path": "src/actions.ts", "line": 9, "col": 0, "action_name": "submitForm", "actions": []}, {"path": "src/other.ts", "line": 1, "col": 0, "action_name": "delUser", "actions": []}]' "$FIXTURES/check.json" | jq --argjson changed '["src/actions.ts"]' -f "$JQ_DIR/filter-changed.jq" 2>&1)
assert_json_value "$OUT_USA_FILTERED" '.unused_server_actions | length' "1" "usa: filter-changed keeps only changed-file findings"

OUT_URC=$(jq '.unrendered_components = [{"path": "src/Foo.vue", "line": 1, "col": 0, "component_name": "Foo", "framework": "vue", "actions": []}] | .total_issues = (.total_issues + 1)' "$FIXTURES/check.json" | jq -r -f "$JQ_DIR/summary-check.jq" 2>&1)
assert_contains "$OUT_URC" "Unrendered components" "urc: shows summary row and section"
assert_contains "$OUT_URC" "Foo" "urc: shows component name in section"
OUT_URC_ANN=$(jq '.unrendered_components = [{"path": "src/Foo.vue", "line": 1, "col": 0, "component_name": "Foo", "framework": "vue", "actions": []}]' "$FIXTURES/check.json" | jq -r -f "$JQ_DIR/annotations-check.jq" 2>&1)
assert_contains "$OUT_URC_ANN" "::warning file=src/Foo.vue,line=1,col=1,title=Unrendered component::" "urc: warning-severity annotation"
OUT_URC_FILTERED=$(jq '.unrendered_components = [{"path": "src/Foo.vue", "line": 1, "col": 0, "component_name": "Foo", "framework": "vue", "actions": []}, {"path": "src/Bar.vue", "line": 1, "col": 0, "component_name": "Bar", "framework": "vue", "actions": []}]' "$FIXTURES/check.json" | jq --argjson changed '["src/Foo.vue"]' -f "$JQ_DIR/filter-changed.jq" 2>&1)
assert_json_value "$OUT_URC_FILTERED" '.unrendered_components | length' "1" "urc: filter-changed keeps only changed-file findings"

OUT_UCP=$(jq '.unused_component_props = [{"path": "src/Widget.vue", "line": 12, "col": 0, "component_name": "Widget", "prop_name": "variant", "actions": []}] | .total_issues = (.total_issues + 1)' "$FIXTURES/check.json" | jq -r -f "$JQ_DIR/summary-check.jq" 2>&1)
assert_contains "$OUT_UCP" "Unused component props" "ucp: shows summary row and section"
assert_contains "$OUT_UCP" "variant" "ucp: shows prop name in section"
OUT_UCP_ANN=$(jq '.unused_component_props = [{"path": "src/Widget.vue", "line": 12, "col": 4, "component_name": "Widget", "prop_name": "variant", "actions": []}]' "$FIXTURES/check.json" | jq -r -f "$JQ_DIR/annotations-check.jq" 2>&1)
assert_contains "$OUT_UCP_ANN" "::warning file=src/Widget.vue,line=12,col=5,title=Unused component prop::" "ucp: warning-severity annotation"
OUT_UCP_FILTERED=$(jq '.unused_component_props = [{"path": "src/Widget.vue", "line": 12, "col": 0, "component_name": "Widget", "prop_name": "variant", "actions": []}, {"path": "src/Other.vue", "line": 3, "col": 0, "component_name": "Other", "prop_name": "size", "actions": []}]' "$FIXTURES/check.json" | jq --argjson changed '["src/Widget.vue"]' -f "$JQ_DIR/filter-changed.jq" 2>&1)
assert_json_value "$OUT_UCP_FILTERED" '.unused_component_props | length' "1" "ucp: filter-changed keeps only changed-file findings"

OUT_UCE=$(jq '.unused_component_emits = [{"path": "src/Widget.vue", "line": 14, "col": 0, "component_name": "Widget", "emit_name": "submit", "actions": []}] | .total_issues = (.total_issues + 1)' "$FIXTURES/check.json" | jq -r -f "$JQ_DIR/summary-check.jq" 2>&1)
assert_contains "$OUT_UCE" "Unused component emits" "uce: shows summary row and section"
assert_contains "$OUT_UCE" "submit" "uce: shows emit name in section"
OUT_UCE_ANN=$(jq '.unused_component_emits = [{"path": "src/Widget.vue", "line": 14, "col": 4, "component_name": "Widget", "emit_name": "submit", "actions": []}]' "$FIXTURES/check.json" | jq -r -f "$JQ_DIR/annotations-check.jq" 2>&1)
assert_contains "$OUT_UCE_ANN" "::warning file=src/Widget.vue,line=14,col=5,title=Unused component emit::" "uce: warning-severity annotation"
OUT_UCE_FILTERED=$(jq '.unused_component_emits = [{"path": "src/Widget.vue", "line": 14, "col": 0, "component_name": "Widget", "emit_name": "submit", "actions": []}, {"path": "src/Other.vue", "line": 5, "col": 0, "component_name": "Other", "emit_name": "close", "actions": []}]' "$FIXTURES/check.json" | jq --argjson changed '["src/Widget.vue"]' -f "$JQ_DIR/filter-changed.jq" 2>&1)
assert_json_value "$OUT_UCE_FILTERED" '.unused_component_emits | length' "1" "uce: filter-changed keeps only changed-file findings"

OUT_UCI=$(jq '.unused_component_inputs = [{"path": "src/widget.component.ts", "line": 12, "col": 0, "component_name": "Widget", "input_name": "variant", "actions": []}] | .total_issues = (.total_issues + 1)' "$FIXTURES/check.json" | jq -r -f "$JQ_DIR/summary-check.jq" 2>&1)
assert_contains "$OUT_UCI" "Unused component inputs" "uci: shows summary row and section"
assert_contains "$OUT_UCI" "variant" "uci: shows input name in section"
OUT_UCI_ANN=$(jq '.unused_component_inputs = [{"path": "src/widget.component.ts", "line": 12, "col": 4, "component_name": "Widget", "input_name": "variant", "actions": []}]' "$FIXTURES/check.json" | jq -r -f "$JQ_DIR/annotations-check.jq" 2>&1)
assert_contains "$OUT_UCI_ANN" "::warning file=src/widget.component.ts,line=12,col=5,title=Unused component input::" "uci: warning-severity annotation"
OUT_UCI_FILTERED=$(jq '.unused_component_inputs = [{"path": "src/widget.component.ts", "line": 12, "col": 0, "component_name": "Widget", "input_name": "variant", "actions": []}, {"path": "src/other.component.ts", "line": 3, "col": 0, "component_name": "Other", "input_name": "size", "actions": []}]' "$FIXTURES/check.json" | jq --argjson changed '["src/widget.component.ts"]' -f "$JQ_DIR/filter-changed.jq" 2>&1)
assert_json_value "$OUT_UCI_FILTERED" '.unused_component_inputs | length' "1" "uci: filter-changed keeps only changed-file findings"

OUT_UCO=$(jq '.unused_component_outputs = [{"path": "src/widget.component.ts", "line": 14, "col": 0, "component_name": "Widget", "output_name": "submit", "actions": []}] | .total_issues = (.total_issues + 1)' "$FIXTURES/check.json" | jq -r -f "$JQ_DIR/summary-check.jq" 2>&1)
assert_contains "$OUT_UCO" "Unused component outputs" "uco: shows summary row and section"
assert_contains "$OUT_UCO" "submit" "uco: shows output name in section"
OUT_UCO_ANN=$(jq '.unused_component_outputs = [{"path": "src/widget.component.ts", "line": 14, "col": 4, "component_name": "Widget", "output_name": "submit", "actions": []}]' "$FIXTURES/check.json" | jq -r -f "$JQ_DIR/annotations-check.jq" 2>&1)
assert_contains "$OUT_UCO_ANN" "::warning file=src/widget.component.ts,line=14,col=5,title=Unused component output::" "uco: warning-severity annotation"
OUT_UCO_FILTERED=$(jq '.unused_component_outputs = [{"path": "src/widget.component.ts", "line": 14, "col": 0, "component_name": "Widget", "output_name": "submit", "actions": []}, {"path": "src/other.component.ts", "line": 5, "col": 0, "component_name": "Other", "output_name": "close", "actions": []}]' "$FIXTURES/check.json" | jq --argjson changed '["src/widget.component.ts"]' -f "$JQ_DIR/filter-changed.jq" 2>&1)
assert_json_value "$OUT_UCO_FILTERED" '.unused_component_outputs | length' "1" "uco: filter-changed keeps only changed-file findings"

OUT_USE=$(jq '.unused_svelte_events = [{"path": "src/Child.svelte", "line": 6, "col": 0, "component_name": "Child", "event_name": "dead", "actions": []}] | .total_issues = (.total_issues + 1)' "$FIXTURES/check.json" | jq -r -f "$JQ_DIR/summary-check.jq" 2>&1)
assert_contains "$OUT_USE" "Unused Svelte events" "use: shows summary row and section"
assert_contains "$OUT_USE" "dead" "use: shows event name in section"
OUT_USE_ANN=$(jq '.unused_svelte_events = [{"path": "src/Child.svelte", "line": 6, "col": 4, "component_name": "Child", "event_name": "dead", "actions": []}]' "$FIXTURES/check.json" | jq -r -f "$JQ_DIR/annotations-check.jq" 2>&1)
assert_contains "$OUT_USE_ANN" "::warning file=src/Child.svelte,line=6,col=5,title=Unused Svelte event::" "use: warning-severity annotation"
OUT_USE_FILTERED=$(jq '.unused_svelte_events = [{"path": "src/Child.svelte", "line": 6, "col": 0, "component_name": "Child", "event_name": "dead", "actions": []}, {"path": "src/Other.svelte", "line": 5, "col": 0, "component_name": "Other", "event_name": "gone", "actions": []}]' "$FIXTURES/check.json" | jq --argjson changed '["src/Child.svelte"]' -f "$JQ_DIR/filter-changed.jq" 2>&1)
assert_json_value "$OUT_USE_FILTERED" '.unused_svelte_events | length' "1" "use: filter-changed keeps only changed-file findings"

OUT_UPI=$(jq '.unprovided_injects =[{"path": "src/useTheme.ts", "line": 7, "col": 0, "key_name": "themeKey", "framework": "vue", "actions": []}] | .total_issues = (.total_issues + 1)' "$FIXTURES/check.json" | jq -r -f "$JQ_DIR/summary-check.jq" 2>&1)
assert_contains "$OUT_UPI" "Unprovided injects" "upi: shows summary row and section"
assert_contains "$OUT_UPI" "themeKey" "upi: shows inject key in section"
OUT_UPI_ANN=$(jq '.unprovided_injects = [{"path": "src/useTheme.ts", "line": 7, "col": 2, "key_name": "themeKey", "framework": "vue", "actions": []}]' "$FIXTURES/check.json" | jq -r -f "$JQ_DIR/annotations-check.jq" 2>&1)
assert_contains "$OUT_UPI_ANN" "::warning file=src/useTheme.ts,line=7,col=3,title=Unprovided inject::" "upi: warning-severity annotation"
OUT_UPI_FILTERED=$(jq '.unprovided_injects = [{"path": "src/useTheme.ts", "line": 7, "col": 0, "key_name": "themeKey", "framework": "vue", "actions": []}, {"path": "src/other.ts", "line": 2, "col": 0, "key_name": "authKey", "framework": "svelte", "actions": []}]' "$FIXTURES/check.json" | jq --argjson changed '["src/useTheme.ts"]' -f "$JQ_DIR/filter-changed.jq" 2>&1)
assert_json_value "$OUT_UPI_FILTERED" '.unprovided_injects | length' "1" "upi: filter-changed keeps only changed-file findings"

# Missing keys must never crash jq (defensive `// []` / null-safe helpers). Strip every
# framework array and confirm the summary still renders.
OUT_NO_FRAMEWORK_KEYS=$(jq 'del(.unused_server_actions, .unrendered_components, .unused_component_props, .unused_component_emits, .unused_component_inputs, .unused_component_outputs, .unused_svelte_events, .unprovided_injects, .route_collisions, .dynamic_segment_name_conflicts, .invalid_client_exports, .mixed_client_server_barrels, .misplaced_directives)' "$FIXTURES/check.json" | jq -r -f "$JQ_DIR/summary-check.jq" 2>&1)
assert_contains "$OUT_NO_FRAMEWORK_KEYS" "Fallow Analysis" "missing-keys: summary-check survives absent framework keys"

# filter-changed recalculates total_issues from the surviving arrays (synthetic minimal input
# so the assertion does not depend on the base fixture's other findings).
OUT_RSC_RECALC=$(jq -n '{total_issues: 2, invalid_client_exports: [{"path": "src/a.tsx", "line": 1, "col": 0, "export_name": "metadata", "directive": "use client", "actions": []}, {"path": "src/b.tsx", "line": 1, "col": 0, "export_name": "revalidate", "directive": "use client", "actions": []}]}' | jq --argjson changed '["src/a.tsx"]' -f "$JQ_DIR/filter-changed.jq" 2>&1)
assert_json_value "$OUT_RSC_RECALC" '.total_issues' "1" "rsc: filter-changed recalculates total_issues after dropping non-changed finding"

OUT_CLEAN=$(jq -r -f "$JQ_DIR/summary-check.jq" "$FIXTURES/check-clean.json" 2>&1)
assert_contains "$OUT_CLEAN" "No issues found" "clean: shows no issues"
assert_not_contains "$OUT_CLEAN" "WARNING" "clean: no warning"

# Issue #449: kind_known: false renders "unknown kind \`token\`" in the table,
# distinguishing it from a stale-but-known kind which renders just \`token\`.
OUT_UNKNOWN_KIND_SUMMARY=$(jq '.unused_files = [] | .unused_exports = [] | .unused_types = [] | .unused_dependencies = [] | .unused_dev_dependencies = [] | .unused_optional_dependencies = [] | .unused_enum_members = [] | .unused_class_members = [] | .unresolved_imports = [] | .unlisted_dependencies = [] | .duplicate_exports = [] | .circular_dependencies = [] | .boundary_violations = [] | .type_only_dependencies = [] | .test_only_dependencies = [] | .unused_catalog_entries = [] | .empty_catalog_groups = [] | .unresolved_catalog_references = [] | .unused_dependency_overrides = [] | .misconfigured_dependency_overrides = [] | .private_type_leaks = [] | .stale_suppressions = [{"path": "src/utils.ts", "line": 1, "col": 0, "origin": {"type": "comment", "issue_kind": "complexity-typo", "is_file_level": false, "kind_known": false}}] | .total_issues = 1' "$FIXTURES/check.json" | jq -r -f "$JQ_DIR/summary-check.jq" 2>&1)
assert_contains "$OUT_UNKNOWN_KIND_SUMMARY" 'unknown kind' "summary unknown kind: prefix renders"
assert_contains "$OUT_UNKNOWN_KIND_SUMMARY" 'complexity-typo' "summary unknown kind: verbatim token renders"

echo "  summary-security.jq:"
OUT=$(jq -n '{
  kind: "security",
  elapsed_ms: 12,
  gate: {mode: "new", verdict: "fail", new_count: 1},
  security_findings: [{
    path: "src/api.ts",
    line: 10,
    kind: "tainted-sink",
    severity: "high",
    candidate: {sink: {callee: "exec"}}
  }]
}' | jq -r -f "$JQ_DIR/summary-security.jq" 2>&1)
assert_valid_markdown "$OUT" "produces output"
assert_contains "$OUT" "Fallow Security" "security: has title"
assert_contains "$OUT" "Security gate: \`new\`" "security: shows gate"
assert_contains "$OUT" "src/api.ts:10" "security: lists candidate location"

OUT_NO_LINE=$(jq -n '{
  kind: "security",
  elapsed_ms: 1,
  security_findings: [{
    path: "src/a.ts",
    kind: "tainted-sink",
    candidate: {sink: {}}
  }]
}' | jq -r -f "$JQ_DIR/summary-security.jq" 2>&1)
assert_contains "$OUT_NO_LINE" "\`src/a.ts\`" "security: missing line renders path only"
assert_not_contains "$OUT_NO_LINE" "null" "security: missing line does not render null"

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

# Every withholding counter has to reach the headline. A run whose only outcome
# is a withheld dependency or enum member still puts entries in `fixes`, which
# the analyze gate counts, so a summary that reads only the export counter
# prints "No fixable issues found" under a job reporting fixable issues.
FIX_DEP_WITHHELD='{
  "dry_run": false,
  "fixes": [{"type": "remove_dependency", "package": "lodash", "file": "package.json", "location": "dependencies", "skipped": true, "skip_reason": "low_confidence_reachability_caveat"}],
  "total_fixed": 0, "skipped": 0,
  "skipped_content_changed": 0, "skipped_mixed_line_endings": 0,
  "skipped_low_confidence_exports": 0,
  "skipped_low_confidence_dependencies": 1,
  "skipped_low_confidence_members": 0
}'
OUT_DEP_WITHHELD=$(printf '%s' "$FIX_DEP_WITHHELD" | jq -r -f "$JQ_DIR/summary-fix.jq" 2>&1)
assert_not_contains "$OUT_DEP_WITHHELD" "No fixable issues found" "withheld dependency is not reported as clean"
assert_contains "$OUT_DEP_WITHHELD" "kept 1 declared package(s)" "withheld dependency reaches the headline"
# A withheld removal is not a removal: listing it under the count table would
# report a write that never happened.
assert_not_contains "$OUT_DEP_WITHHELD" "Dependency removals | 1" "withheld dependency is not counted as removed"
assert_not_contains "$OUT_DEP_WITHHELD" "lodash" "withheld dependency is not listed as removed"

OUT_MEMBER_WITHHELD=$(printf '%s' "$FIX_DEP_WITHHELD" \
  | jq '.skipped_low_confidence_dependencies = 0 | .skipped_low_confidence_members = 1 | .fixes[0].type = "remove_enum_member"' \
  | jq -r -f "$JQ_DIR/summary-fix.jq" 2>&1)
assert_not_contains "$OUT_MEMBER_WITHHELD" "No fixable issues found" "withheld enum member is not reported as clean"
assert_contains "$OUT_MEMBER_WITHHELD" "kept 1 unused enum member(s)" "withheld enum member reaches the headline"

echo "  summary-dupes.jq:"
OUT=$(jq -r -f "$JQ_DIR/summary-dupes.jq" "$FIXTURES/dupes.json" 2>&1)
assert_valid_markdown "$OUT" "produces output"
assert_contains "$OUT" "clone groups" "mentions clone groups"
assert_contains "$OUT" "Duplicated lines" "shows duplication stats"
assert_contains "$OUT" "content-parser.ts:27-50" "shows clone instance line range"

OUT_FAMILY_RANK=$(jq -n '{
  elapsed_ms: 1,
  stats: {
    total_files: 4, files_with_clones: 4, clone_groups: 2,
    clone_instances: 4, duplicated_lines: 20, total_lines: 100,
    duplication_percentage: 20
  },
  clone_groups: [],
  clone_families: [
    {
      files: ["a-local.ts", "b-local.ts"],
      total_duplicated_lines: 10,
      groups: [{
        line_count: 10, token_count: 110, spread: 0,
        instances: [
          {file: "a-local.ts", start_line: 1, end_line: 10},
          {file: "b-local.ts", start_line: 1, end_line: 10}
        ]
      }],
      suggestions: []
    },
    {
      files: ["z-distant-a.ts", "z-distant-b.ts"],
      total_duplicated_lines: 10,
      groups: [
        {
          line_count: 2, token_count: 5, spread: 0,
          instances: [
            {file: "ignored-first-a.ts", start_line: 1, end_line: 2},
            {file: "ignored-first-b.ts", start_line: 1, end_line: 2}
          ]
        },
        {
          line_count: 10, token_count: 100, spread: 8,
          instances: [
            {file: "z-distant-a.ts", start_line: 1, end_line: 10},
            {file: "z-distant-b.ts", start_line: 1, end_line: 10}
          ]
        }
      ],
      suggestions: []
    }
  ]
}' | jq -r -f "$JQ_DIR/summary-dupes.jq" 2>&1)
FIRST_RANKED_FAMILY=$(printf '%s\n' "$OUT_FAMILY_RANK" | grep '^- \*\*' | head -1)
assert_contains "$FIRST_RANKED_FAMILY" "z-distant-a.ts" "families use their best spread-aware group rank"
FIRST_RANKED_INSTANCE=$(printf '%s\n' "$OUT_FAMILY_RANK" | grep '^  -' | head -1)
assert_contains "$FIRST_RANKED_INSTANCE" "z-distant-a.ts:1-10" "families show their best-ranked group"

OUT_CLEAN=$(jq -r -f "$JQ_DIR/summary-dupes.jq" "$FIXTURES/dupes-clean.json" 2>&1)
assert_contains "$OUT_CLEAN" "No code duplication" "clean: no duplication"

# A `--top`-capped envelope carries a truncated `clone_families[]` while `stats`
# still describes the whole corpus. The families label must report the corpus,
# like the header two lines above it, and must name what it is not showing.
OUT_TOP_CAPPED=$(jq '
  .stats.clone_groups = 9 | .stats.clone_families = 7
  | .clone_families = [.clone_families[0]]
  | .clone_groups_shown = 1 | .clone_groups_omitted = 8
  | .clone_families_shown = 1 | .clone_families_omitted = 6
' "$FIXTURES/dupes.json" | jq -r -f "$JQ_DIR/summary-dupes.jq" 2>&1)
assert_contains "$OUT_TOP_CAPPED" "Clone Families (7)" "top-capped: families label reports the corpus, not the capped array"
assert_not_contains "$OUT_TOP_CAPPED" "Clone Families (1)" "top-capped: capped array length is not the label"
assert_contains "$OUT_TOP_CAPPED" "6 more families" "top-capped: names the families it does not show"
assert_contains "$OUT_TOP_CAPPED" "withheld by a display limit" "top-capped: says the withholding happened before this report"

OUT_TOP_CAPPED_GROUPS=$(jq '
  .stats.clone_groups = 9 | .clone_families = []
  | .clone_groups = [.clone_groups[0]]
  | .clone_groups_shown = 1 | .clone_groups_omitted = 8
' "$FIXTURES/dupes.json" | jq -r -f "$JQ_DIR/summary-dupes.jq" 2>&1)
assert_contains "$OUT_TOP_CAPPED_GROUPS" "8 more groups" "top-capped groups branch: names the groups it does not show"

# An untruncated run stays byte-identical: no omission tail at all.
OUT_UNTRUNCATED=$(jq -r -f "$JQ_DIR/summary-dupes.jq" "$FIXTURES/dupes.json" 2>&1)
assert_not_contains "$OUT_UNTRUNCATED" "withheld by a display limit" "untruncated run carries no omission tail"

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
assert_contains "$OUT_AUDIT" "### Styling" "audit: has styling details"
assert_contains "$OUT_AUDIT" "css-selector-complexity" "audit: lists styling rule"
assert_contains "$OUT_AUDIT" "src/styles.css:4" "audit: lists styling location"
assert_contains "$OUT_AUDIT" "1 introduced clone group demoted to inherited" "audit: shows demotion footnote"

OUT_AUDIT_STYLE_NEW=$(jq -n '{
  command: "audit", verdict: "fail", changed_files_count: 1, elapsed_ms: 4,
  summary: {dead_code_issues: 0, complexity_findings: 0, duplication_clone_groups: 0},
  attribution: {gate: "new-only", styling_introduced: 1, styling_inherited: 0},
  complexity: {styling_findings: [{code: "css-important", path: "src/styles.css", line: 2, value: "!important", effective_severity: "error", introduced: true}]}
}' | jq -r -f "$JQ_DIR/summary-audit.jq" 2>&1)
assert_contains "$OUT_AUDIT_STYLE_NEW" "| Styling | 1 | 1 | 0 |" "audit: styling-only new-only totals are visible"
assert_contains "$OUT_AUDIT_STYLE_NEW" "| new |" "audit: styling-only new-only status is visible"
assert_not_contains "$OUT_AUDIT_STYLE_NEW" "demoted to inherited" "audit: no demotion footnote without demotions"

OUT_AUDIT_STYLE_ALL=$(jq -n '{
  command: "audit", verdict: "fail", changed_files_count: 1, elapsed_ms: 4,
  summary: {dead_code_issues: 0, complexity_findings: 0, duplication_clone_groups: 0},
  attribution: {gate: "all"},
  complexity: {styling_findings: [{code: "css-important", path: null, line: null, value: "!important", effective_severity: "error"}]}
}' | jq -r -f "$JQ_DIR/summary-audit.jq" 2>&1)
assert_contains "$OUT_AUDIT_STYLE_ALL" "| Styling | 1 | 0 | 0 |" "audit: styling-only all totals are visible"
assert_contains "$OUT_AUDIT_STYLE_ALL" '| - | `css-important`' "audit: null styling path uses a safe placeholder"
assert_contains "$OUT_AUDIT_STYLE_ALL" "Audit gate: all" "audit: styling-only all gate is visible"

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

OUT_EMPTY_DUPES=$(jq '.dupes.clone_groups = [] | .dupes.clone_families = [] | .dupes.stats.clone_groups = 2 | .dupes.stats.clone_instances = 5 | .dupes.stats.files_with_clones = 4 | .dupes.stats.duplicated_lines = 59 | .dupes.stats.duplication_percentage = 0.16' "$FIXTURES/combined-clean.json" | jq -r -f "$JQ_DIR/summary-combined.jq" 2>&1)
assert_contains "$OUT_EMPTY_DUPES" "Quality gate passed" "combined: empty dupes groups keep clean summary"
assert_contains "$OUT_EMPTY_DUPES" "No duplication" "combined: empty dupes groups render no duplication"
assert_not_contains "$OUT_EMPTY_DUPES" "2 groups" "combined: nonzero dupes stats do not render actionable groups"

# The other half of that invariant: an array a presentation cap truncated is
# NOT the corpus. `clone_groups_omitted` counts only what a cap withheld (never
# what a filter removed), so adding it keeps issue #1250 intact while a capped
# combined envelope, if the bare command ever grows `--top`, still reports the
# whole measurement instead of the visible slice.
OUT_CAPPED_DUPES=$(jq '.dupes.clone_groups_omitted = 4' "$FIXTURES/combined.json" | jq -r -f "$JQ_DIR/summary-combined.jq" 2>&1)
assert_contains "$OUT_CAPPED_DUPES" "clone group" "combined: capped dupes still render"
OUT_CAPPED_ISSUES=$(jq -r '((.check.total_issues // 0) + (((.dupes.clone_groups // []) | length) + (.dupes.clone_groups_omitted // 0)) + (.health.summary.functions_above_threshold // 0))' <(jq '.dupes.clone_groups_omitted = 4' "$FIXTURES/combined.json"))
OUT_UNCAPPED_ISSUES=$(jq -r '((.check.total_issues // 0) + (((.dupes.clone_groups // []) | length) + (.dupes.clone_groups_omitted // 0)) + (.health.summary.functions_above_threshold // 0))' "$FIXTURES/combined.json")
[ "$OUT_CAPPED_ISSUES" = "$((OUT_UNCAPPED_ISSUES + 4))" ] \
  && pass "combined gate: withheld clone groups reach the issue count" \
  || fail "combined gate: withheld clone groups reach the issue count" "expected $((OUT_UNCAPPED_ISSUES + 4)), got '$OUT_CAPPED_ISSUES'"

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

# RSC findings appear in the combined-mode Code issues breakdown table (not just
# summary-check.jq standalone). All three RSC types injected into .check at once.
OUT_RSC=$(jq '.check.invalid_client_exports = [{"path": "src/app.tsx", "line": 5, "col": 0, "export_name": "metadata", "directive": "use client", "actions": []}] | .check.mixed_client_server_barrels = [{"path": "src/index.ts", "line": 2, "col": 0, "client_origin": "./Button", "server_origin": "./fetchUser", "actions": []}] | .check.misplaced_directives = [{"path": "src/widget.tsx", "line": 4, "col": 0, "directive": "use server", "actions": []}] | .check.total_issues = (.check.total_issues + 3)' "$FIXTURES/combined.json" | jq -r -f "$JQ_DIR/summary-combined.jq" 2>&1)
assert_contains "$OUT_RSC" "| [Invalid client exports](" "combined: RSC invalid-client-exports row in breakdown"
assert_contains "$OUT_RSC" "| [Mixed client/server barrels](" "combined: RSC mixed-barrel row in breakdown"
assert_contains "$OUT_RSC" "| [Misplaced directives](" "combined: RSC misplaced-directives row in breakdown"

# Next.js routing keys (route_collisions + dynamic_segment_name_conflicts) were previously
# absent from the combined-mode Code issues breakdown; assert they now render.
OUT_ROUTING=$(jq '.check.route_collisions = [{"path": "src/app/(a)/p/page.tsx", "url": "/p", "conflicting_paths": ["src/app/(b)/p/page.tsx"], "actions": []}] | .check.dynamic_segment_name_conflicts = [{"path": "src/app/[id]/page.tsx", "position": "0", "conflicting_segments": ["id", "slug"], "actions": []}] | .check.total_issues = (.check.total_issues + 2)' "$FIXTURES/combined.json" | jq -r -f "$JQ_DIR/summary-combined.jq" 2>&1)
assert_contains "$OUT_ROUTING" "| [Route collisions](" "combined: route-collisions row in breakdown"
assert_contains "$OUT_ROUTING" "| [Dynamic segment conflicts](" "combined: dynamic-segment-conflicts row in breakdown"

# Vue/Next framework keys appear in the combined-mode Code issues breakdown table.
OUT_FRAMEWORK=$(jq '.check.unused_server_actions = [{"path": "src/actions.ts", "line": 9, "col": 0, "action_name": "submitForm", "actions": []}] | .check.unrendered_components = [{"path": "src/Foo.vue", "line": 1, "col": 0, "component_name": "Foo", "framework": "vue", "actions": []}] | .check.unused_component_props = [{"path": "src/Widget.vue", "line": 12, "col": 0, "component_name": "Widget", "prop_name": "variant", "actions": []}] | .check.unused_component_emits = [{"path": "src/Widget.vue", "line": 14, "col": 0, "component_name": "Widget", "emit_name": "submit", "actions": []}] | .check.unused_component_inputs = [{"path": "src/widget.component.ts", "line": 12, "col": 0, "component_name": "Widget", "input_name": "variant", "actions": []}] | .check.unused_component_outputs = [{"path": "src/widget.component.ts", "line": 14, "col": 0, "component_name": "Widget", "output_name": "submit", "actions": []}] | .check.unused_svelte_events = [{"path": "src/Child.svelte", "line": 6, "col": 0, "component_name": "Child", "event_name": "dead", "actions": []}] | .check.unprovided_injects = [{"path": "src/useTheme.ts", "line": 7, "col": 0, "key_name": "themeKey", "framework": "vue", "actions": []}] | .check.total_issues = (.check.total_issues + 8)' "$FIXTURES/combined.json" | jq -r -f "$JQ_DIR/summary-combined.jq" 2>&1)
assert_contains "$OUT_FRAMEWORK" "| [Unused server actions](" "combined: unused-server-actions row in breakdown"
assert_contains "$OUT_FRAMEWORK" "| [Unrendered components](" "combined: unrendered-components row in breakdown"
assert_contains "$OUT_FRAMEWORK" "| [Unused component props](" "combined: unused-component-props row in breakdown"
assert_contains "$OUT_FRAMEWORK" "| [Unused component emits](" "combined: unused-component-emits row in breakdown"
assert_contains "$OUT_FRAMEWORK" "| [Unused component inputs](" "combined: unused-component-inputs row in breakdown"
assert_contains "$OUT_FRAMEWORK" "| [Unused component outputs](" "combined: unused-component-outputs row in breakdown"
assert_contains "$OUT_FRAMEWORK" "| [Unused Svelte events](" "combined: unused-svelte-events row in breakdown"
assert_contains "$OUT_FRAMEWORK" "| [Unprovided injects](" "combined: unprovided-injects row in breakdown"

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
assert_contains "$OUT_CLEAN" "Quality gate passed" "clean: no issues"
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

OUT_ESCAPED_PATH=$(jq '.unused_files[0].path = "src/a%,b:c\r\nd.ts"' "$FIXTURES/check.json" | jq -r -f "$JQ_DIR/annotations-check.jq" 2>&1)
assert_contains "$OUT_ESCAPED_PATH" "file=src/a%25%2Cb%3Ac%0D%0Ad.ts" "check annotation escapes workflow-command properties"

OUT_CLEAN=$(jq -r -f "$JQ_DIR/annotations-check.jq" "$FIXTURES/check-clean.json" 2>&1)
[ -z "$OUT_CLEAN" ] && pass "clean: no annotations" || fail "clean: no annotations" "got output"

# An annotation is the surface that suggests the mutation, so a finding whose
# reachability verdict rests on a file the run never fully read must carry the
# caveat here too. The token set is open: an unrecognised value is still a
# caveat and must not be dropped.
OUT_CAVEAT=$(jq '
  .unused_files = [{"path": "src/orphan.ts", "actions": [], "reachability_caveats": ["incomplete-file-analysis", "incomplete-import-graph"]}]
  | .unused_exports[0].reachability_caveats = ["incomplete-import-graph"]
  | .unused_dependencies[0].reachability_caveats = ["incomplete-import-graph"]
  | .unused_dev_dependencies = [{"path": "package.json", "line": 30, "package_name": "vitest", "actions": [], "reachability_caveats": ["some-future-caveat"]}]
' "$FIXTURES/check.json" | jq -r -f "$JQ_DIR/annotations-check.jq" 2>&1)
assert_contains "$OUT_CAVEAT" "Caveat: incomplete file analysis, incomplete import graph" "caveated unused file names both caveats"
assert_contains "$OUT_CAVEAT" "verify before removing" "caveat hedges the suggested removal"
assert_contains "$OUT_CAVEAT" "Caveat: some future caveat" "unrecognised caveat token is rendered, not dropped"
CAVEAT_LINES=$(printf '%s\n' "$OUT_CAVEAT" | grep -c "Caveat: ")
[ "$CAVEAT_LINES" = "4" ] && pass "caveat reaches exactly the caveated findings" \
  || fail "caveat reaches exactly the caveated findings" "got ${CAVEAT_LINES} annotations with a caveat"
assert_not_contains "$OUT" "Caveat:" "uncaveated run carries no caveat text"

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

OUT_ESCAPED_PATH=$(jq '.clone_groups[0].instances[0].file = "src/a%,b:c\r\nd.ts"' "$FIXTURES/dupes.json" | jq -r -f "$JQ_DIR/annotations-dupes.jq" 2>&1)
assert_contains "$OUT_ESCAPED_PATH" "file=src/a%25%2Cb%3Ac%0D%0Ad.ts" "dupes annotation escapes workflow-command properties"

echo "  annotations-health.jq:"
OUT=$(jq -r -f "$JQ_DIR/annotations-health.jq" "$FIXTURES/health.json" 2>&1)
assert_contains "$OUT" "::error" "critical finding emits ::error annotation"
assert_contains "$OUT" "::warning" "high/moderate findings emit ::warning annotation"
assert_contains "$OUT" "(critical)" "critical severity in annotation title"
assert_contains "$OUT" "(high)" "high severity in annotation title"
assert_contains "$OUT" "parseContentBlocks" "includes function name"

OUT_ESCAPED_PATH=$(jq '.findings[0].path = "src/a%,b:c\r\nd.ts"' "$FIXTURES/health.json" | jq -r -f "$JQ_DIR/annotations-health.jq" 2>&1)
assert_contains "$OUT_ESCAPED_PATH" "file=src/a%25%2Cb%3Ac%0D%0Ad.ts" "health annotation escapes workflow-command properties"

# An override-affected finding is described against the ceiling it was measured
# with, not the run-global summary ceiling. Mirrors the native renderer test
# `annotations_use_the_finding_effective_threshold` (issue #2163).
OUT_OVERRIDE=$(jq '.findings = [{"path":"src/Board.astro","name":"<template>","line":6,"col":3,"cyclomatic":11,"cognitive":4,"line_count":20,"param_count":0,"exceeded":"crap","severity":"critical","crap":132.0,"threshold_source":"override","effective_thresholds":{"max_cyclomatic":20,"max_cognitive":15,"max_crap":100,"max_unit_size":60}}]' "$FIXTURES/health.json" | jq -r -f "$JQ_DIR/annotations-health.jq" 2>&1)
assert_contains "$OUT_OVERRIDE" "threshold: 100" "health annotation uses the finding's override ceiling"
assert_not_contains "$OUT_OVERRIDE" "threshold: 30" "health annotation drops the global ceiling under an override"

OUT_PROD_ANN=$(jq '.runtime_coverage = {"verdict":"cold-code-detected","summary":{"functions_tracked":2,"functions_hit":1,"functions_unhit":1,"functions_untracked":0,"coverage_percent":50,"trace_count":1200,"period_days":7,"deployments_seen":2},"findings":[{"path":"src/cold.ts","function":"coldPath","line":14,"verdict":"review_required","invocations":0,"confidence":"medium","evidence":{"static_status":"used","test_coverage":"not_covered","v8_tracking":"tracked"},"actions":[{"description":"Review before deleting."}]},{"path":"src/lazy.ts","function":"lateBound","line":8,"verdict":"coverage_unavailable","confidence":"none","evidence":{"static_status":"used","test_coverage":"not_covered","v8_tracking":"untracked","untracked_reason":"lazy_parsed"}}]}' "$FIXTURES/health-clean.json" | jq -r -f "$JQ_DIR/annotations-health.jq" 2>&1)
assert_contains "$OUT_PROD_ANN" "Runtime coverage" "prod annotation: title present"
assert_contains "$OUT_PROD_ANN" "coldPath" "prod annotation: function name present"

OUT_TEST_ONLY_ANN=$(jq '.runtime_coverage = {"verdict":"cold-code-detected","summary":{"functions_tracked":1,"functions_hit":0,"functions_unhit":1,"functions_untracked":0,"coverage_percent":0,"trace_count":1200,"period_days":7,"deployments_seen":2},"findings":[{"path":"src/helpers.ts","function":"resetForTests","line":4,"verdict":"review_required","invocations":0,"confidence":"high","evidence":{"static_status":"unused","test_coverage":"not_covered","test_only_reference":true,"v8_tracking":"tracked"},"actions":[{"description":"Only tests reference this export; delete the test usage together with the function or keep it."}]}]}' "$FIXTURES/health-clean.json" | jq -r -f "$JQ_DIR/annotations-health.jq" 2>&1)
assert_contains "$OUT_TEST_ONLY_ANN" "Static: unused (referenced only from tests)" "prod annotation: a test-only reference is named next to the static verdict"

render_direct_annotations() {
  local kind="$1" input="$2"
  case "$kind" in
    dead-code) jq -r -f "$JQ_DIR/annotations-check.jq" "$input" ;;
    dupes) jq -r -f "$JQ_DIR/annotations-dupes.jq" "$input" ;;
    health) jq -r -f "$JQ_DIR/annotations-health.jq" "$input" ;;
    audit)
      {
        jq '.dead_code // empty' "$input" | jq -r -f "$JQ_DIR/annotations-check.jq"
        jq '.complexity // empty' "$input" | jq -r -f "$JQ_DIR/annotations-health.jq"
        jq '.duplication // empty' "$input" | jq -r -f "$JQ_DIR/annotations-dupes.jq"
      }
      ;;
    combined)
      {
        jq '.check // empty' "$input" | jq -r -f "$JQ_DIR/annotations-check.jq"
        jq '.health // empty' "$input" | jq -r -f "$JQ_DIR/annotations-health.jq"
        jq '.dupes // empty' "$input" | jq -r -f "$JQ_DIR/annotations-dupes.jq"
      }
      ;;
  esac
}

render_forced_fallback_annotations() {
  local kind="$1" input="$2" command="$1"
  [ "$kind" = "combined" ] && command=""
  HAS_NATIVE_REPORT=false \
    FALLOW_PR_DECISION_FILE="" \
    FALLOW_COMMAND="$command" \
    MAX_ANNOTATIONS="50" \
    ACTION_JQ_DIR="$JQ_DIR" \
    FALLOW_RESULTS_FILE="$input" \
    bash "$DIR/../scripts/annotate.sh" 2>/dev/null | grep -v '^::notice::.*legacy renderer'
}

ANNOTATION_SAFETY_DIR=$(mktemp -d)
ANNOTATION_ATTACK=$'value%\r\n::error::injected'
ANNOTATION_PATH=$'src/a%,b:c\r\nd.ts'
ANNOTATION_EXPECTED_PATH="file=src/a%25%2Cb%3Ac%0D%0Ad.ts"

jq -n --arg path "$ANNOTATION_PATH" --arg attack "$ANNOTATION_ATTACK" '{
  unused_exports: [
    {path: $path, line: 4, col: 1, export_name: $attack, is_re_export: false, is_type_only: false},
    {path: null, line: 5, col: 1, export_name: $attack, is_re_export: false, is_type_only: false},
    {path: 42, line: 6, col: 1, export_name: $attack, is_re_export: false, is_type_only: false}
  ]
}' > "$ANNOTATION_SAFETY_DIR/dead-code.json"

jq -n --arg path "$ANNOTATION_PATH" --arg attack "$ANNOTATION_ATTACK" '{
  clone_groups: [{
    line_count: $attack,
    token_count: $attack,
    instances: [
      {file: $path, start_line: 1, end_line: 3, start_col: 0},
      {file: null, start_line: 4, end_line: 6, start_col: 0},
      {file: 42, start_line: 7, end_line: 9, start_col: 0}
    ]
  }]
}' > "$ANNOTATION_SAFETY_DIR/dupes.json"

jq -n --arg path "$ANNOTATION_PATH" --arg attack "$ANNOTATION_ATTACK" '{
  summary: {max_cyclomatic_threshold: 20, max_cognitive_threshold: 15, max_crap_threshold: 30},
  findings: [
    {path: $path, name: $attack, line: 2, col: 0, cyclomatic: 21, cognitive: 16, line_count: 8, severity: "moderate", exceeded: "both"},
    {path: null, name: $attack, line: 3, col: 0, cyclomatic: 21, cognitive: 16, line_count: 8, severity: "moderate", exceeded: "both"},
    {path: 42, name: $attack, line: 4, col: 0, cyclomatic: 21, cognitive: 16, line_count: 8, severity: "moderate", exceeded: "both"}
  ]
}' > "$ANNOTATION_SAFETY_DIR/health.json"

jq -n \
  --slurpfile dead "$ANNOTATION_SAFETY_DIR/dead-code.json" \
  --slurpfile health "$ANNOTATION_SAFETY_DIR/health.json" \
  --slurpfile dupes "$ANNOTATION_SAFETY_DIR/dupes.json" \
  '{dead_code: $dead[0], complexity: $health[0], duplication: $dupes[0]}' \
  > "$ANNOTATION_SAFETY_DIR/audit.json"

jq -n \
  --slurpfile dead "$ANNOTATION_SAFETY_DIR/dead-code.json" \
  --slurpfile health "$ANNOTATION_SAFETY_DIR/health.json" \
  --slurpfile dupes "$ANNOTATION_SAFETY_DIR/dupes.json" \
  '{check: $dead[0], health: $health[0], dupes: $dupes[0]}' \
  > "$ANNOTATION_SAFETY_DIR/combined.json"

for kind in dead-code dupes health audit combined; do
  expected_lines=1
  [ "$kind" = "audit" ] || [ "$kind" = "combined" ] && expected_lines=3
  DIRECT_SAFE=$(render_direct_annotations "$kind" "$ANNOTATION_SAFETY_DIR/$kind.json")
  assert_contains "$DIRECT_SAFE" "$ANNOTATION_EXPECTED_PATH" "$kind direct renderer preserves property encoding"
  assert_contains "$DIRECT_SAFE" "value%25%0D%0A::error::injected" "$kind direct renderer escapes message data"
  assert_safe_workflow_output "$DIRECT_SAFE" "$expected_lines" "$kind direct renderer skips malformed paths without command injection"

  FALLBACK_SAFE=$(render_forced_fallback_annotations "$kind" "$ANNOTATION_SAFETY_DIR/$kind.json")
  assert_contains "$FALLBACK_SAFE" "$ANNOTATION_EXPECTED_PATH" "$kind forced fallback preserves property encoding"
  assert_contains "$FALLBACK_SAFE" "value%25%0D%0A::error::injected" "$kind forced fallback escapes message data"
  assert_safe_workflow_output "$FALLBACK_SAFE" "$expected_lines" "$kind forced fallback skips malformed paths without command injection"
done

rm -rf "$ANNOTATION_SAFETY_DIR"

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
  if [ "${2:-}" = "post-pr-comment" ]; then
    printf '{"action":"create","marker_id":"fallow-results","body":"smoke"}\n'
    exit 0
  fi
  if [ "${2:-}" = "post-review" ]; then
    case "${MOCK_POST_REVIEW_ERRORS:-}" in
      apply)
        printf '{"action":"post_review","comments_posted":1,"apply_errors":["resolve failed"],"post_errors":[],"apply_hint":"refresh provider state","failed_fingerprints":["a"],"unapplied_fingerprints":["a"]}\n'
        ;;
      *)
        printf '{"action":"post_review","comments_posted":1,"apply_errors":[],"post_errors":[]}\n'
        ;;
    esac
    exit 0
  fi
  printf '{"schema":"fallow-review-reconcile/v1","stale":[]}\n'
  exit 0
fi
if [ "${1:-}" = "report" ] && [ "${MOCK_LEGACY_REPORT_TARGETS:-}" = "1" ]; then
  echo 'fallow report supports --format github-annotations, github-summary, codeclimate, or sarif only' >&2
  exit 2
fi
if [ "${MOCK_RENDER_FAILURE:-}" = "1" ]; then
  echo 'saved audit envelope is missing required field `version`' >&2
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
  pr-comment-github)
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
  github-annotations)
    printf '::notice file=src/a.ts,line=1::smoke\n'
    ;;
  github-summary)
    printf '# Fallow smoke\n'
    ;;
  *)
    printf '{}\n'
    ;;
esac
if [ "${MOCK_LEGACY_FINDINGS_EXIT:-}" = "1" ] && [ "${1:-}" != "report" ]; then
  exit 1
fi
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

printf '{"kind":"dead-code","schema_version":9}\n' > "$ACTION_TYPED_WORK/fallow-results.json"
printf 'FALLOW_ANALYSIS_ARGS=(check --format json --root .)\n' > "$ACTION_TYPED_WORK/fallow-analysis-args.sh"
(
  cd "$ACTION_TYPED_WORK"
  PATH="$ACTION_TYPED_BIN:$PATH" \
    MOCK_LOG="$ACTION_TYPED_LOG" \
    GH_TOKEN="test" \
    PR_NUMBER="123" \
    GH_REPO="owner/repo" \
    GITHUB_SHA="abc123" \
    PR_HEAD_SHA="head456" \
    FALLOW_COMMAND="check" \
    FALLOW_RENDER_PATH_PREFIX_SET="1" \
    FALLOW_RENDER_PATH_PREFIX="custom/base" \
    FALLOW_SUMMARY_SCOPE="diff" \
    bash "$SCRIPTS_DIR/comment.sh" > /dev/null
  PATH="$ACTION_TYPED_BIN:$PATH" \
    MOCK_LOG="$ACTION_TYPED_LOG" \
    GH_TOKEN="test" \
    PR_NUMBER="123" \
    GH_REPO="owner/repo" \
    FALLOW_COMMAND="check" \
    FALLOW_RENDER_PATH_PREFIX_SET="1" \
    FALLOW_RENDER_PATH_PREFIX="custom/base" \
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
    bash "$SCRIPTS_DIR/review.sh" > "$ACTION_TYPED_WORK/review-clean.out"
  PATH="$ACTION_TYPED_BIN:$PATH" \
    MOCK_LOG="$ACTION_TYPED_LOG" \
    MOCK_POST_REVIEW_ERRORS="apply" \
    GH_TOKEN="test" \
    PR_NUMBER="123" \
    GH_REPO="owner/repo" \
    FALLOW_COMMAND="check" \
    FALLOW_ROOT="." \
    MAX_COMMENTS="5" \
    bash "$SCRIPTS_DIR/review.sh" > "$ACTION_TYPED_WORK/review-apply-error.out"
  PATH="$ACTION_TYPED_BIN:$PATH" \
    MOCK_LOG="$ACTION_TYPED_LOG" \
    MOCK_RENDER_FAILURE="1" \
    GH_TOKEN="test" \
    PR_NUMBER="123" \
    GH_REPO="owner/repo" \
    FALLOW_COMMAND="check" \
    bash "$SCRIPTS_DIR/comment.sh" > "$ACTION_TYPED_WORK/comment-render-failure.out"
  PATH="$ACTION_TYPED_BIN:$PATH" \
    MOCK_LOG="$ACTION_TYPED_LOG" \
    MOCK_RENDER_FAILURE="1" \
    GH_TOKEN="test" \
    PR_NUMBER="123" \
    GH_REPO="owner/repo" \
    FALLOW_COMMAND="check" \
    FALLOW_DIFF_FILE="$ACTION_TYPED_WORK/fallow-pr.diff" \
    bash "$SCRIPTS_DIR/review.sh" > "$ACTION_TYPED_WORK/review-render-failure.out"
)
ACTION_TYPED_OUT=$(cat "$ACTION_TYPED_LOG")
assert_contains "$ACTION_TYPED_OUT" "--format pr-comment-github" "comment.sh invokes typed PR comment format"
assert_contains "$ACTION_TYPED_OUT" "--format review-github" "review.sh invokes typed GitHub review format"
assert_contains "$ACTION_TYPED_OUT" "report --from fallow-results.json" "GitHub renderers reuse the saved analysis envelope"
assert_contains "$ACTION_TYPED_OUT" "--report-path-prefix custom/base" "GitHub renderers preserve presentation path prefixes"
assert_contains "$ACTION_TYPED_OUT" "summary_scope=diff" "comment.sh passes FALLOW_SUMMARY_SCOPE to typed PR comment render"
ACTION_BLANK_SUMMARY_SCOPE_COUNT=$(printf '%s\n' "$ACTION_TYPED_OUT" | grep -c '^summary_scope=$' || true)
if [ "$ACTION_BLANK_SUMMARY_SCOPE_COUNT" -ge 1 ]; then
  pass "review.sh does not receive FALLOW_SUMMARY_SCOPE by default"
else
  fail "review.sh does not receive FALLOW_SUMMARY_SCOPE by default" "$ACTION_TYPED_OUT"
fi
assert_contains "$ACTION_TYPED_OUT" "fallow ci post-review --provider github" "review.sh invokes GitHub review post command"
assert_contains "$(cat "$ACTION_TYPED_WORK/review-clean.out")" \
  "0 resolution replies posted, 0 threads resolved" \
  "review.sh exposes successful reconciliation counters"
assert_not_contains "$(cat "$ACTION_TYPED_WORK/review-clean.out")" "fallow post-review incomplete" \
  "review.sh stays quiet when reconciliation fully succeeds"
assert_contains "$(cat "$ACTION_TYPED_WORK/review-apply-error.out")" \
  "::warning::fallow post-review incomplete: refresh provider state" \
  "review.sh warns when applying reconciliation is incomplete"
assert_contains "$(cat "$ACTION_TYPED_WORK/review-apply-error.out")" \
  "(unapplied fingerprints: a)" \
  "review.sh names the fingerprints reconciliation did not apply"
assert_contains "$ACTION_TYPED_OUT" "fallow ci post-pr-comment --provider github" "comment.sh invokes GitHub PR comment post command"
assert_contains "$ACTION_TYPED_OUT" "fallow ci post-check-run --provider github" "comment.sh invokes GitHub Check Run post command"
assert_contains "$ACTION_TYPED_OUT" "--head-sha head456" "comment.sh posts Check Run against the PR head SHA"
assert_contains "$(cat "$ACTION_TYPED_WORK/comment-render-failure.out")" \
  'saved audit envelope is missing required field `version`' \
  "comment.sh surfaces saved-render stderr"
assert_contains "$(cat "$ACTION_TYPED_WORK/review-render-failure.out")" \
  'saved audit envelope is missing required field `version`' \
  "review.sh surfaces saved-render stderr"
assert_not_contains "$ACTION_TYPED_OUT" "fallow check " "malformed saved artifacts do not trigger direct-analysis fallback"

# #2675: the advisory and the gate row are rendered by the CLI, so the action's
# job is to carry them to the surfaces people read. The body it posts and the
# decision it hands to `ci post-check-run` are the two files that do that.
ACTION_BASELINE_LOG="$ACTION_TYPED_WORK/baseline-comment.log"
: > "$ACTION_BASELINE_LOG"
(
  cd "$ACTION_TYPED_WORK"
  PATH="$ACTION_TYPED_BIN:$PATH" \
    MOCK_LOG="$ACTION_BASELINE_LOG" \
    MOCK_BASELINE_ADVISORY="1" \
    GH_TOKEN="test" \
    PR_NUMBER="123" \
    GH_REPO="owner/repo" \
    PR_HEAD_SHA="head456" \
    FALLOW_COMMAND="check" \
    bash "$SCRIPTS_DIR/comment.sh" > /dev/null
)
ACTION_BASELINE_BODY=$(cat "$ACTION_TYPED_WORK/fallow-pr-comment.md")
ACTION_BASELINE_DECISION=$(cat "$ACTION_TYPED_WORK/fallow-pr-decision.json")
assert_contains "$ACTION_BASELINE_BODY" "**Baseline matched nothing.**" \
  "the posted PR comment body carries the baseline advisory"
assert_contains "$ACTION_BASELINE_BODY" "Gate outcomes: failed stale-baseline." \
  "the posted PR comment body keeps the gate inventory beside the advisory"
assert_contains "$ACTION_BASELINE_DECISION" '"id":"stale-baseline"' \
  "the decision sidecar carries the stale-baseline gate row"
assert_contains "$(cat "$ACTION_BASELINE_LOG")" \
  "ci post-check-run --provider github --decision fallow-pr-decision.json" \
  "the sidecar carrying the gate row is the one posted as the Check Run"

: > "$ACTION_TYPED_LOG"
(
  cd "$ACTION_TYPED_WORK"
  PATH="$ACTION_TYPED_BIN:$PATH" \
    MOCK_LOG="$ACTION_TYPED_LOG" \
    MOCK_LEGACY_REPORT_TARGETS="1" \
    MOCK_LEGACY_FINDINGS_EXIT="1" \
    HAS_NATIVE_REPORT="true" \
    FALLOW_ANALYSIS_ARGS_JSON='["check","--root","packages/app one","--quiet","--format","json"]' \
    FALLOW_RENDER_PATH_PREFIX_SET="1" \
    FALLOW_RENDER_PATH_PREFIX="custom/base" \
    GH_TOKEN="test" \
    PR_NUMBER="123" \
    GH_REPO="owner/repo" \
    FALLOW_COMMAND="check" \
    bash "$SCRIPTS_DIR/comment.sh" > /dev/null
  PATH="$ACTION_TYPED_BIN:$PATH" \
    MOCK_LOG="$ACTION_TYPED_LOG" \
    MOCK_LEGACY_REPORT_TARGETS="1" \
    MOCK_LEGACY_FINDINGS_EXIT="1" \
    HAS_NATIVE_REPORT="true" \
    FALLOW_ANALYSIS_ARGS_JSON='["check","--root","packages/app one","--quiet","--format","json"]' \
    FALLOW_RENDER_PATH_PREFIX_SET="1" \
    FALLOW_RENDER_PATH_PREFIX="custom/base" \
    GH_TOKEN="test" \
    PR_NUMBER="123" \
    GH_REPO="owner/repo" \
    FALLOW_COMMAND="check" \
    FALLOW_DIFF_FILE="$ACTION_TYPED_WORK/fallow-pr.diff" \
    MAX_COMMENTS="5" \
    bash "$SCRIPTS_DIR/review.sh" > /dev/null
)
ACTION_LEGACY_OUT=$(cat "$ACTION_TYPED_LOG")
assert_contains "$ACTION_LEGACY_OUT" "fallow report --from fallow-results.json" "pinned CLI compatibility probes saved rendering first"
assert_contains "$ACTION_LEGACY_OUT" "fallow check --root packages/app one --quiet --format pr-comment-github" "comment.sh falls back to direct rendering for pinned CLIs"
assert_contains "$ACTION_LEGACY_OUT" "fallow check --root packages/app one --quiet --format review-github" "review.sh falls back to direct rendering for pinned CLIs"
assert_contains "$ACTION_LEGACY_OUT" "--report-path-prefix custom/base" "pinned CLI fallback preserves presentation path prefixes"
assert_contains "$ACTION_LEGACY_OUT" "fallow ci post-pr-comment --provider github" "comment.sh posts valid pinned-CLI findings output"
assert_contains "$ACTION_LEGACY_OUT" "fallow ci post-review --provider github" "review.sh posts valid pinned-CLI findings output"
assert_not_contains "$(cat "$SCRIPTS_DIR/comment.sh")$(cat "$SCRIPTS_DIR/review.sh")" "source \"\$FALLOW_ANALYSIS_ARGS_FILE\"" "pinned CLI fallback does not source workspace shell"

: > "$ACTION_TYPED_LOG"
(
  cd "$ACTION_TYPED_WORK"
  PATH="$ACTION_TYPED_BIN:$PATH" \
    MOCK_LOG="$ACTION_TYPED_LOG" \
    HAS_NATIVE_REPORT="true" \
    FALLOW_BIN="$ACTION_TYPED_BIN/fallow" \
    FALLOW_COMMAND="dead-code" \
    INPUT_ROOT="packages/app" \
    MAX_ANNOTATIONS="5" \
    ACTION_JQ_DIR="$JQ_DIR" \
    FALLOW_RESULTS_FILE="$ACTION_TYPED_WORK/fallow-results.json" \
    FALLOW_RENDER_PATH_PREFIX_SET="1" \
    FALLOW_RENDER_PATH_PREFIX="custom/base" \
    bash "$SCRIPTS_DIR/annotate.sh" > /dev/null
  HAS_NATIVE_REPORT="true" \
    MOCK_LOG="$ACTION_TYPED_LOG" \
    FALLOW_BIN="$ACTION_TYPED_BIN/fallow" \
    FALLOW_COMMAND="dead-code" \
    INPUT_ROOT="packages/app" \
    ACTION_JQ_DIR="$JQ_DIR" \
    GITHUB_STEP_SUMMARY="$ACTION_TYPED_WORK/summary.md" \
    FALLOW_RESULTS_FILE="$ACTION_TYPED_WORK/fallow-results.json" \
    FALLOW_RENDER_PATH_PREFIX_SET="1" \
    FALLOW_RENDER_PATH_PREFIX="custom/base" \
    bash "$SCRIPTS_DIR/summary.sh" > /dev/null
)
ACTION_NATIVE_OUT=$(cat "$ACTION_TYPED_LOG")
assert_contains "$ACTION_NATIVE_OUT" "--root packages/app --format github-annotations --report-path-prefix custom/base" "annotation saved renderer preserves root and path prefix"
assert_contains "$ACTION_NATIVE_OUT" "--root packages/app --format github-summary --report-path-prefix custom/base" "summary saved renderer preserves root and path prefix"
assert_contains "$(cat "$SCRIPTS_DIR/comment.sh")" "FALLOW_PR_COMMENT_ENVELOPE_FILE" "comment.sh asks fallow for typed PR comment envelope"
assert_contains "$(cat "$SCRIPTS_DIR/comment.sh")" "FALLOW_PR_DECISION_FILE" "comment.sh asks fallow for typed PR decision sidecar"
assert_contains "$(cat "$SCRIPTS_DIR/comment.sh")" "FALLOW_PR_DETAILS_FILE" "comment.sh asks fallow for typed PR details artifact"
assert_contains "$(cat "$SCRIPTS_DIR/comment.sh")" "--envelope" "comment.sh passes typed PR comment envelope when present"
assert_contains "$(cat "$SCRIPTS_DIR/review.sh")" "apply_errors" "review.sh checks reconcile apply errors"
assert_contains "$(cat "$SCRIPTS_DIR/review.sh")" "apply_hint" "review.sh emits reconcile apply hint"
# The runner rejects a manifest that is not valid YAML before any step runs, and the
# substring assertions below cannot see that: an apostrophe inside a single-quoted
# description is enough. Ruby ships with macOS and with the hosted Linux runners.
if command -v ruby > /dev/null 2>&1; then
  if ruby -ryaml -e 'YAML.load_file(ARGV[0]).fetch("outputs")' "$DIR/../../action.yml" > /dev/null 2>&1; then
    pass "action.yml parses as YAML"
  else
    fail "action.yml parses as YAML" "the manifest does not load; check the quoting of the last edited description"
  fi
else
  echo "  - action.yml parse check skipped: ruby is not installed"
fi
assert_contains "$(cat "$DIR/../../action.yml")" "review-guidance:" "action.yml exposes review-guidance input"
assert_contains "$(cat "$DIR/../../action.yml")" "FALLOW_REVIEW_GUIDANCE: \${{ inputs.review-guidance }}" "action.yml maps review-guidance to env"
assert_contains "$(cat "$DIR/../../action.yml")" "review-id:" "action.yml exposes review-id input"
assert_contains "$(cat "$DIR/../../action.yml")" "FALLOW_REVIEW_ID: \${{ inputs.review-id }}" "action.yml maps review-id to env"
assert_contains "$(cat "$DIR/../../action.yml")" "summary-scope:" "action.yml exposes summary-scope input"
assert_contains "$(cat "$DIR/../../action.yml")" "FALLOW_SUMMARY_SCOPE: \${{ inputs.summary-scope }}" "action.yml maps summary-scope to comment env"
assert_contains "$(cat "$DIR/../../action.yml")" "comment-layout:" "action.yml exposes comment-layout input"
assert_contains "$(cat "$DIR/../../action.yml")" "FALLOW_PR_COMMENT_LAYOUT: \${{ inputs.comment-layout }}" "action.yml maps comment-layout to comment env"
assert_contains "$(cat "$DIR/../../action.yml")" "FALLOW_PR_COMMENT_ENVELOPE_FILE: \${{ inputs.artifacts-dir }}/fallow-pr-comment-envelope.json" "action.yml maps typed envelope to job summary"
assert_contains "$(cat "$DIR/../../action.yml")" "FALLOW_PR_DECISION_FILE: \${{ inputs.artifacts-dir }}/fallow-pr-decision.json" "action.yml maps typed decision to annotations"
assert_contains "$(cat "$DIR/../../action.yml")" "FALLOW_ARTIFACTS_DIR: \${{ inputs.artifacts-dir }}" "action.yml maps artifacts dir to typed scripts"
assert_contains "$(cat "$DIR/../../action.yml")" "PR_HEAD_SHA: \${{ github.event.pull_request.head.sha }}" "action.yml maps PR head SHA for native Check Runs"
assert_contains "$(cat "$DIR/../../crates/cli/src/ci.rs")" "with_rate_limit_retry(\"GitHub\"" "Rust CI adapter wraps GitHub API calls with retry"
assert_contains "$(cat "$DIR/../../crates/cli/src/ci.rs")" "rate-limited" "Rust CI adapter reports GitHub rate-limit retries"
assert_contains "$(cat "$SCRIPTS_DIR/comment.sh")" "Unsupported FALLOW_SUMMARY_SCOPE" "comment.sh warns on invalid summary scope"
assert_not_contains "$(cat "$SCRIPTS_DIR/review.sh")" "FALLOW_SUMMARY_SCOPE" "review.sh does not consume summary scope"
if sed -n '/name: Post review comments/,/run: bash/p' "$DIR/../../action.yml" | /usr/bin/grep -q "FALLOW_SUMMARY_SCOPE"; then
  fail "Post review comments action env excludes FALLOW_SUMMARY_SCOPE" "summary scope must not affect inline review comments"
else
  pass "Post review comments action env excludes FALLOW_SUMMARY_SCOPE"
fi
assert_contains "$(cat "$SCRIPTS_DIR/review.sh")" "fallow ci post-review" "review.sh delegates retryable provider work to Rust"
assert_contains "$(cat "$SCRIPTS_DIR/review.sh")" "RESOLUTION_NOUN" "review.sh logs and pluralizes reconciliation reply counts"
assert_contains "$(cat "$SCRIPTS_DIR/review.sh")" "THREAD_NOUN" "review.sh logs and pluralizes resolved-thread counts"
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
API_CHANGED_FILE="$API_FAIL_WORK/fallow-changed-files.json"
if jq -e '
  length == 4
  and .[0] == "src/a.ts"
  and .[1] == "src/line\nbreak.ts"
  and .[2] == "src/quote\"file.ts"
  and .[3] == "src/back\\slash.ts"
' "$API_CHANGED_FILE" >/dev/null; then
  pass "analyze: API fallback preserves JSON-escaped filenames"
else
  fail "analyze: API fallback preserves JSON-escaped filenames" \
    "got: $(cat "$API_CHANGED_FILE" 2>/dev/null || echo absent)"
fi

# --- Test 2a: API fallback scopes repo-relative paths for an absolute root ---

mkdir -p "$API_FAIL_WORK/packages/app"
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

: > "$API_FAIL_OUTPUT"
(cd "$API_FAIL_WORK" \
  && PATH="$API_FAIL_BIN:$PATH" \
  GITHUB_WORKSPACE="$API_FAIL_WORK" \
  GITHUB_OUTPUT="$API_FAIL_OUTPUT" \
  INPUT_ROOT="$API_FAIL_WORK/packages/app" \
  INPUT_COMMAND="check" \
  INPUT_FORMAT="json" \
  INPUT_CHANGED_SINCE="0000000000000000000000000000000000000000" \
  PR_NUMBER="123" \
  GH_REPO="owner/repo" \
  GH_TOKEN="test" \
  FALLOW_API_RETRIES=1 \
  FALLOW_API_RETRY_DELAY=0 \
  bash "$SCRIPTS_DIR/analyze.sh" >/dev/null 2>&1) || true

if jq -e 'length == 1 and .[0] == "src/a.ts"' "$API_CHANGED_FILE" >/dev/null; then
  pass "analyze: API fallback scopes changed files for an absolute root"
else
  fail "analyze: API fallback scopes changed files for an absolute root" \
    "got: $(cat "$API_CHANGED_FILE" 2>/dev/null || echo absent)"
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
  if [ "${2:-}" = "post-review" ]; then
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
  printf '{"kind":"dead-code","schema_version":9}\n' > "$API_FAIL_WORK/fallow-results.json"
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

# Test 3: review.sh delegates provider posting and dedup to Rust.
api_fail_review_run "multi-5xx" R3_STATUS R3_OUTPUT R3_STDERR "" "5xx"
[ "$R3_STATUS" -eq 0 ] && pass "review.sh: Rust-delegated review post exits 0" \
  || fail "review.sh: Rust-delegated review post exits 0" "got $R3_STATUS"
assert_contains "$R3_OUTPUT" "post_skipped_reason=none" \
  "review.sh: initializes post skip marker while Rust owns dedup policy"
assert_contains "$R3_OUTPUT" "dedup_lookup_failed=false" \
  "review.sh: initializes dedup lookup marker while Rust owns dedup policy"
assert_contains "$(cat "$API_FAIL_WORK/mock.log")" "fallow ci post-review --provider github" \
  "review.sh: delegates provider review posting to Rust"
if cat "$API_FAIL_WORK/mock.log" 2>/dev/null | /usr/bin/grep -q "gh api"; then
  fail "review.sh: does not call gh api for review posting" "$(cat "$API_FAIL_WORK/mock.log")"
else
  pass "review.sh: does not call gh api for review posting"
fi

# Test 4: retry-exhausted 429 behavior now lives in the Rust post-review
# command; the shell wrapper should still delegate and stay non-fatal.
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
  if [ "${2:-}" = "post-review" ]; then
    printf '{"action":"post_review","comments_posted":1,"apply_errors":[],"post_errors":[]}\n'
  else
    printf '{"schema":"fallow-review-reconcile/v1","stale":[]}\n'
  fi
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
  && pass "review.sh: retry-exhausted 429 remains non-fatal in shell wrapper" \
  || fail "review.sh: retry-exhausted 429 remains non-fatal in shell wrapper" "got $R5B_STATUS"
assert_contains "$(cat "$API_FAIL_WORK/mock.log")" "fallow ci post-review --provider github" \
  "review.sh: 429 path still delegates review posting to Rust"

# Test 6: comment.sh delegates sticky summary posting to the Rust adapter
cat > "$API_FAIL_BIN/gh" <<'SH'
#!/usr/bin/env bash
printf 'gh %s\n' "$*" >> "$MOCK_LOG"
if [ "${1:-}" = "pr" ] && [ "${2:-}" = "diff" ]; then
  printf 'diff --git a/src/a.ts b/src/a.ts\n--- a/src/a.ts\n+++ b/src/a.ts\n@@ -0,0 +1 @@\n+export const a = 1;\n'
  exit 0
fi
if [ "${1:-}" = "api" ]; then
  echo "gh api should not be called by comment.sh summary posting" >&2
  exit 1
fi
SH
chmod +x "$API_FAIL_BIN/gh"

cat > "$API_FAIL_BIN/fallow" <<'SH'
#!/usr/bin/env bash
printf 'fallow %s\n' "$*" >> "$MOCK_LOG"
if [ "${1:-}" = "ci" ] && [ "${2:-}" = "post-pr-comment" ]; then
  printf '{"action":"create","marker_id":"fallow-results","body":"smoke"}\n'
  exit 0
fi
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

assert_contains "$(cat "$API_FAIL_OUTPUT")" "dedup_lookup_failed=false" \
  "comment.sh: initializes dedup lookup marker"
assert_contains "$(cat "$API_FAIL_WORK/mock.log")" "fallow ci post-pr-comment --provider github" \
  "comment.sh: delegates summary posting to Rust"
if /usr/bin/grep -q "gh api" "$API_FAIL_WORK/mock.log"; then
  fail "comment.sh: does not call gh api for summary posting" \
    "$(cat "$API_FAIL_WORK/mock.log")"
else
  pass "comment.sh: does not call gh api for summary posting"
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

TYPED_SUMMARY_FILE="$WORK_DIR/typed-summary.md"
printf '{"body":"# Fallow typed summary\\n\\nGenerated by fallow."}\n' > "$CUSTOM_ARTIFACTS/fallow-pr-comment-envelope.json"
OUT=$(cd "$WORK_DIR" && \
  GITHUB_STEP_SUMMARY="$TYPED_SUMMARY_FILE" \
  FALLOW_COMMAND="dead-code" \
  ACTION_JQ_DIR="$JQ_DIR" \
  FALLOW_RESULTS_FILE="missing-results.json" \
  FALLOW_PR_COMMENT_ENVELOPE_FILE=".var/fallow/fallow-pr-comment-envelope.json" \
  bash "$SCRIPTS_DIR/summary.sh" 2>&1)
cmd_status=$?
if [ "$cmd_status" -eq 0 ]; then
  pass "summary.sh: typed envelope path succeeds without jq results"
else
  fail "summary.sh: typed envelope path succeeds without jq results" "exit $cmd_status, output: $OUT"
fi
assert_contains "$(cat "$TYPED_SUMMARY_FILE")" "# Fallow typed summary" "summary.sh: typed envelope body wins"

# #2736: the degraded note is read by a human who then looks for the cause, so
# it must not claim files were skipped when the degrading kind is a config a
# plugin could not read.
DEGRADED_SUMMARY_FILE="$WORK_DIR/degraded-summary.md"
OUT=$(cd "$WORK_DIR" && \
  GITHUB_STEP_SUMMARY="$DEGRADED_SUMMARY_FILE" \
  FALLOW_COMMAND="dead-code" \
  ACTION_JQ_DIR="$JQ_DIR" \
  FALLOW_ANALYSIS_DEGRADED="true" \
  FALLOW_RESULTS_FILE=".var/fallow/fallow-results.json" \
  FALLOW_SCOPED_RESULTS_FILE=".var/fallow/fallow-results-degraded.json" \
  bash "$SCRIPTS_DIR/summary.sh" 2>&1)
assert_contains "$(cat "$DEGRADED_SUMMARY_FILE")" "Analysis was degraded." \
  "summary.sh: the degraded flag still writes its note"
assert_contains "$(cat "$DEGRADED_SUMMARY_FILE")" "or from an input that did not load" \
  "summary.sh: the note covers a degrading kind that is not about files"
assert_not_contains "$(cat "$DEGRADED_SUMMARY_FILE")" "Some files never reached the analysis" \
  "summary.sh: the note does not claim files were skipped"

# summary.sh writes a `Gates:` line from `FALLOW_GATES_FAILED` alone. The
# analyze.sh cases cover how a failing default rule reaches that list when
# `fail-on-issues` is false.
GATES_SUMMARY_FILE="$WORK_DIR/gates-summary.md"
: > "$GATES_SUMMARY_FILE"
OUT=$(cd "$WORK_DIR" && \
  GITHUB_STEP_SUMMARY="$GATES_SUMMARY_FILE" \
  FALLOW_COMMAND="dead-code" \
  ACTION_JQ_DIR="$JQ_DIR" \
  FALLOW_GATES_FAILED="error-severity-findings" \
  FALLOW_RESULTS_FILE="missing-results.json" \
  bash "$SCRIPTS_DIR/summary.sh" 2>&1)
assert_contains "$(cat "$GATES_SUMMARY_FILE")" "> **Gates:** failed error-severity-findings." \
  "summary.sh: the Gates line lists an unenforced default rule with status fail"

printf '{"annotations":[{"path":"src/a.ts","line":0,"level":"failure","title":"fallow/high-crap-score","message":"Needs work","raw_details":null},{"path":"src/b.ts","line":12,"level":"notice","title":"fallow/info","message":"FYI","raw_details":null}]}\n' > "$CUSTOM_ARTIFACTS/fallow-pr-decision.json"
OUT=$(cd "$WORK_DIR" && \
  FALLOW_COMMAND="dead-code" \
  MAX_ANNOTATIONS="1" \
  ACTION_JQ_DIR="$JQ_DIR" \
  FALLOW_RESULTS_FILE="missing-results.json" \
  FALLOW_PR_DECISION_FILE=".var/fallow/fallow-pr-decision.json" \
  bash "$SCRIPTS_DIR/annotate.sh" 2>&1)
cmd_status=$?
if [ "$cmd_status" -eq 0 ]; then
  pass "annotate.sh: typed decision path succeeds without jq results"
else
  fail "annotate.sh: typed decision path succeeds without jq results" "exit $cmd_status, output: $OUT"
fi
assert_contains "$OUT" "::error file=src/a.ts,line=1,title=fallow/high-crap-score::Needs work" "annotate.sh: typed decision emits workflow command"
assert_contains "$OUT" "Showing 1 of 2 annotations" "annotate.sh: typed decision honors max annotations"

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

# --- Native report fastpath (annotate.sh / summary.sh) ---
# Exercise the native github-annotations / github-summary fastpath. These need a
# report-capable fallow binary (on PATH or at $FALLOW_BIN); without one the whole
# section skips, and older binaries keep the jq path covered by the tests above.

echo ""
echo "=== Native report fastpath ==="

FASTPATH_SCRIPTS="$DIR/../scripts"
FASTPATH_ENVELOPE="$FIXTURES/dead-code-envelope.json"

# Resolve a report-capable binary the same way issuekind-drift-guard.sh does:
# explicit $FALLOW_BIN, else a locally built target/{release,debug}/fallow, else
# a PATH lookup. Absolute so the check survives the cwd changes above.
FASTPATH_REPO_ROOT="$(cd "$DIR/../.." && pwd)"
FASTPATH_BIN="${FALLOW_BIN:-}"
if [ -z "$FASTPATH_BIN" ]; then
  for FASTPATH_CAND in "$FASTPATH_REPO_ROOT/target/release/fallow" "$FASTPATH_REPO_ROOT/target/debug/fallow"; do
    if [ -x "$FASTPATH_CAND" ]; then FASTPATH_BIN="$FASTPATH_CAND"; break; fi
  done
fi
[ -n "$FASTPATH_BIN" ] || FASTPATH_BIN="fallow"

if ! command -v "$FASTPATH_BIN" > /dev/null 2>&1 || ! "$FASTPATH_BIN" report --help > /dev/null 2>&1; then
  echo "  (skipped: no fallow binary with 'report' support on PATH or at \$FALLOW_BIN)"
else
  FASTPATH_WORK=$(mktemp -d)

  # (a) byte-equality: the action layers only the cap on the native stream.
  FASTPATH_EXPECTED=$("$FASTPATH_BIN" report --from "$FASTPATH_ENVELOPE" --format github-annotations | head -n 999)
  FASTPATH_ACTUAL=$(
    HAS_NATIVE_REPORT=true \
      FALLOW_BIN="$FASTPATH_BIN" \
      FALLOW_COMMAND="dead-code" \
      MAX_ANNOTATIONS="999" \
      ACTION_JQ_DIR="$JQ_DIR" \
      FALLOW_RESULTS_FILE="$FASTPATH_ENVELOPE" \
      bash "$FASTPATH_SCRIPTS/annotate.sh" 2>/dev/null
  )
  if [ "$FASTPATH_ACTUAL" = "$FASTPATH_EXPECTED" ]; then
    pass "annotate.sh native fastpath is byte-identical to report --from | head"
  else
    fail "annotate.sh native fastpath is byte-identical to report --from | head" "diverged from the native render"
  fi

  # (b) truncation notice fires with a small cap.
  FASTPATH_TRUNC=$(
    HAS_NATIVE_REPORT=true \
      FALLOW_BIN="$FASTPATH_BIN" \
      FALLOW_COMMAND="dead-code" \
      MAX_ANNOTATIONS="2" \
      ACTION_JQ_DIR="$JQ_DIR" \
      FALLOW_RESULTS_FILE="$FASTPATH_ENVELOPE" \
      bash "$FASTPATH_SCRIPTS/annotate.sh" 2>/dev/null
  )
  assert_contains "$FASTPATH_TRUNC" "Showing 2 of " "annotate.sh native fastpath honors max-annotations"

  # (c) summary fastpath writes the native heading.
  FASTPATH_SUMMARY="$FASTPATH_WORK/summary.md"
  HAS_NATIVE_REPORT=true \
    FALLOW_BIN="$FASTPATH_BIN" \
    FALLOW_COMMAND="dead-code" \
    ACTION_JQ_DIR="$JQ_DIR" \
    GITHUB_STEP_SUMMARY="$FASTPATH_SUMMARY" \
    FALLOW_RESULTS_FILE="$FASTPATH_ENVELOPE" \
    bash "$FASTPATH_SCRIPTS/summary.sh" > /dev/null 2>&1
  assert_contains "$(cat "$FASTPATH_SUMMARY")" "# Fallow Analysis" "summary.sh native fastpath writes the native heading"

  # (d) fix has no report kind: the fastpath is bypassed for the jq summary.
  FASTPATH_FIX_SUMMARY="$FASTPATH_WORK/fix-summary.md"
  FASTPATH_FIX_LOG=$(
    HAS_NATIVE_REPORT=true \
      FALLOW_BIN="$FASTPATH_BIN" \
      FALLOW_COMMAND="fix" \
      ACTION_JQ_DIR="$JQ_DIR" \
      GITHUB_STEP_SUMMARY="$FASTPATH_FIX_SUMMARY" \
      FALLOW_RESULTS_FILE="$FIXTURES/fix.json" \
      bash "$FASTPATH_SCRIPTS/summary.sh" 2>&1
  )
  assert_contains "$FASTPATH_FIX_LOG" "summary rendered via summary-fix.jq" "summary.sh routes fix to jq even with native support"
  assert_not_contains "$FASTPATH_FIX_LOG" "legacy renderer" "summary.sh prints no legacy notice for fix"
  assert_not_contains "$FASTPATH_FIX_LOG" "rendered via native" "summary.sh never renders fix natively"

  # (e) probe-false pins the exact jq behavior for older binaries.
  FASTPATH_PROBE_FALSE=$(
    HAS_NATIVE_REPORT=false \
      FALLOW_COMMAND="dead-code" \
      MAX_ANNOTATIONS="50" \
      ACTION_JQ_DIR="$JQ_DIR" \
      FALLOW_RESULTS_FILE="$FIXTURES/check.json" \
      bash "$FASTPATH_SCRIPTS/annotate.sh" 2>/dev/null
  )
  FASTPATH_JQ_ONLY=$(jq -r -f "$JQ_DIR/annotations-check.jq" "$FIXTURES/check.json" 2>/dev/null | head -n 50)
  assert_contains "$FASTPATH_PROBE_FALSE" "::notice::" "annotate.sh probe-false prints the legacy notice"
  FASTPATH_PROBE_FALSE=$(printf '%s\n' "$FASTPATH_PROBE_FALSE" | grep -v '^::notice::.*legacy renderer')
  if [ "$FASTPATH_PROBE_FALSE" = "$FASTPATH_JQ_ONLY" ]; then
    pass "annotate.sh probe-false keeps the exact jq annotation output"
  else
    fail "annotate.sh probe-false keeps the exact jq annotation output" "diverged from the jq render"
  fi

  FASTPATH_ESCAPE_ENVELOPE="$FASTPATH_WORK/escape.json"
  jq '.unused_files[0].path = "src/a%,b:c\r\nd.ts"' "$FIXTURES/check.json" > "$FASTPATH_ESCAPE_ENVELOPE"
  FASTPATH_ESCAPED=$(
    HAS_NATIVE_REPORT=false \
      FALLOW_COMMAND="dead-code" \
      MAX_ANNOTATIONS="50" \
      ACTION_JQ_DIR="$JQ_DIR" \
      FALLOW_RESULTS_FILE="$FASTPATH_ESCAPE_ENVELOPE" \
      bash "$FASTPATH_SCRIPTS/annotate.sh" 2>/dev/null
  )
  assert_contains "$FASTPATH_ESCAPED" "file=src/a%25%2Cb%3Ac%0D%0Ad.ts" "annotate.sh legacy renderer escapes workflow-command properties"

  rm -rf "$FASTPATH_WORK"
fi

# --- Legacy jq renderers and the native render gate (summary.sh / annotate.sh) ---
# The jq renderers are frozen legacy renderers for fallow before 3.4.2. A
# report-capable binary must never fall back to them, and a legacy run must
# tell the user how to get the native renderer. Stub binaries keep these tests
# independent of a local fallow build.

echo ""
echo "=== Legacy renderer gate ==="

LEGACY_WORK=$(mktemp -d)
# make_fallow_stub <name> <version line> <report mode: fail|empty>
make_fallow_stub() {
  local path="$LEGACY_WORK/$1"
  cat > "$path" <<STUB
#!/usr/bin/env bash
if [ "\$1" = "--version" ]; then echo "$2"; exit 0; fi
if [ "\$1" = "report" ]; then
  case "$3" in
    fail) echo "stub report error" >&2; exit 2 ;;
    empty) exit 0 ;;
  esac
fi
exit 0
STUB
  chmod +x "$path"
  printf '%s\n' "$path"
}
LEGACY_OLD_BIN=$(make_fallow_stub fallow-old "fallow 3.3.0" fail)
LEGACY_NOVERSION_BIN=$(make_fallow_stub fallow-noversion "fallow (unknown build)" fail)
LEGACY_NEW_BIN=$(make_fallow_stub fallow-new "fallow 3.30.0" fail)
LEGACY_EMPTY_BIN=$(make_fallow_stub fallow-empty "fallow 3.30.0" empty)

# run_legacy_summary <summary file> <env assignments...>: print stdout + stderr.
run_legacy_summary() {
  local summary_file="$1"
  shift
  env GITHUB_STEP_SUMMARY="$summary_file" \
    FALLOW_COMMAND="dead-code" \
    ACTION_JQ_DIR="$JQ_DIR" \
    FALLOW_RESULTS_FILE="$FIXTURES/check.json" \
    "$@" bash "$SCRIPTS_DIR/summary.sh" 2>&1
}

# run_legacy_annotate <env assignments...>: print stdout only.
run_legacy_annotate() {
  env FALLOW_COMMAND="dead-code" \
    MAX_ANNOTATIONS="50" \
    ACTION_JQ_DIR="$JQ_DIR" \
    FALLOW_RESULTS_FILE="$FIXTURES/check.json" \
    "$@" bash "$SCRIPTS_DIR/annotate.sh" 2>"$LEGACY_WORK/annotate.err"
}

# (a) An old binary gets the legacy summary, a notice, and a footnote.
LEGACY_SUMMARY="$LEGACY_WORK/legacy-summary.md"
LEGACY_OUT=$(run_legacy_summary "$LEGACY_SUMMARY" HAS_NATIVE_REPORT=false FALLOW_BIN="$LEGACY_OLD_BIN")
assert_contains "$LEGACY_OUT" "::notice::fallow 3.3.0 predates native GitHub rendering (3.4.2)." \
  "summary.sh legacy path names the binary version in the notice"
assert_contains "$LEGACY_OUT" "Set the action 'version' input, or the fallow version in package.json, to 3.4.2 or later." \
  "summary.sh legacy notice names the upgrade setting"
assert_contains "$LEGACY_OUT" "summary rendered via legacy jq renderer" "summary.sh legacy path logs the renderer"
assert_contains "$(cat "$LEGACY_SUMMARY")" "| [Unused exports](" "summary.sh legacy path still renders the jq table"
assert_contains "$(cat "$LEGACY_SUMMARY")" "*Rendered by the legacy renderer for fallow before 3.4.2." \
  "summary.sh legacy path appends the footnote"

# (b) A version the notice cannot parse gives version-free text.
LEGACY_OUT=$(run_legacy_summary "$LEGACY_WORK/noversion.md" HAS_NATIVE_REPORT=false FALLOW_BIN="$LEGACY_NOVERSION_BIN")
assert_contains "$LEGACY_OUT" "::notice::This fallow version predates native GitHub rendering (3.4.2)." \
  "summary.sh legacy notice omits an unknown version"

# (c) A new binary that failed the probe never gets a false "predates" claim.
LEGACY_OUT=$(run_legacy_summary "$LEGACY_WORK/probe-failed.md" HAS_NATIVE_REPORT=false FALLOW_BIN="$LEGACY_NEW_BIN")
assert_contains "$LEGACY_OUT" "::notice::fallow 3.30.0 has native GitHub rendering, but the probe" \
  "summary.sh legacy notice reports a failed probe on a new binary"
assert_not_contains "$LEGACY_OUT" "predates" "summary.sh legacy notice does not call a new binary old"

# (d) An old binary gets the legacy annotations after one notice line.
LEGACY_ANNOTATIONS=$(run_legacy_annotate HAS_NATIVE_REPORT=false FALLOW_BIN="$LEGACY_OLD_BIN")
assert_contains "$LEGACY_ANNOTATIONS" "::notice::fallow 3.3.0 predates native GitHub rendering (3.4.2). fallow renders the inline annotations with the legacy renderer" \
  "annotate.sh legacy path prints the notice"
assert_contains "$LEGACY_ANNOTATIONS" "::warning file=src/helpers/api.ts" "annotate.sh legacy path still emits the jq annotations"
assert_contains "$(cat "$LEGACY_WORK/annotate.err")" "annotations rendered via legacy jq renderer" "annotate.sh legacy path logs the renderer"

# (e) A native render failure on a report-capable binary warns and skips jq.
NATIVE_FAIL_SUMMARY="$LEGACY_WORK/native-fail.md"
LEGACY_OUT=$(run_legacy_summary "$NATIVE_FAIL_SUMMARY" HAS_NATIVE_REPORT=true FALLOW_BIN="$LEGACY_NEW_BIN")
assert_contains "$LEGACY_OUT" "::warning::fallow could not render the job summary." "summary.sh native failure writes a warning"
assert_contains "$LEGACY_OUT" "stub report error" "summary.sh native failure keeps the binary error in the step log"
assert_not_contains "$LEGACY_OUT" "legacy" "summary.sh native failure does not run the legacy renderer"
assert_contains "$(cat "$NATIVE_FAIL_SUMMARY")" "The job summary could not be rendered." "summary.sh native failure writes a summary line"
assert_not_contains "$(cat "$NATIVE_FAIL_SUMMARY")" "| Category | Count |" "summary.sh native failure writes no legacy table"

# (f) The typed body still wins over the warning when the native render fails.
printf '{"body":"# Typed fallback body"}\n' > "$LEGACY_WORK/envelope.json"
TYPED_AFTER_FAIL="$LEGACY_WORK/typed-after-fail.md"
LEGACY_OUT=$(run_legacy_summary "$TYPED_AFTER_FAIL" HAS_NATIVE_REPORT=true FALLOW_BIN="$LEGACY_NEW_BIN" \
  FALLOW_PR_COMMENT_ENVELOPE_FILE="$LEGACY_WORK/envelope.json")
assert_contains "$(cat "$TYPED_AFTER_FAIL")" "# Typed fallback body" "summary.sh uses the typed body after a native failure"
assert_not_contains "$LEGACY_OUT" "could not render the job summary" "summary.sh does not warn when the typed body renders"

# (g) The same gate for annotations.
LEGACY_ANNOTATIONS=$(run_legacy_annotate HAS_NATIVE_REPORT=true FALLOW_BIN="$LEGACY_NEW_BIN")
assert_contains "$LEGACY_ANNOTATIONS" "::warning::fallow could not render the inline annotations." "annotate.sh native failure writes a warning"
assert_not_contains "$LEGACY_ANNOTATIONS" "file=" "annotate.sh native failure emits no legacy annotations"
assert_contains "$(cat "$LEGACY_WORK/annotate.err")" "stub report error" "annotate.sh native failure keeps the binary error in the step log"

# (h) A native render with zero lines is a success, not a failure.
LEGACY_ANNOTATIONS=$(run_legacy_annotate HAS_NATIVE_REPORT=true FALLOW_BIN="$LEGACY_EMPTY_BIN")
if [ -z "$LEGACY_ANNOTATIONS" ]; then
  pass "annotate.sh empty native render emits nothing"
else
  fail "annotate.sh empty native render emits nothing" "got: $LEGACY_ANNOTATIONS"
fi
assert_contains "$(cat "$LEGACY_WORK/annotate.err")" "annotations rendered via native github-annotations" \
  "annotate.sh empty native render counts as the native path"

# (i) fix renders through summary-fix.jq on a report-capable binary, with no notice.
FIX_SUMMARY="$LEGACY_WORK/fix.md"
LEGACY_OUT=$(run_legacy_summary "$FIX_SUMMARY" HAS_NATIVE_REPORT=true FALLOW_BIN="$LEGACY_NEW_BIN" \
  FALLOW_COMMAND="fix" FALLOW_RESULTS_FILE="$FIXTURES/fix.json")
assert_contains "$LEGACY_OUT" "summary rendered via summary-fix.jq" "summary.sh renders fix through summary-fix.jq"
assert_not_contains "$LEGACY_OUT" "::notice::" "summary.sh prints no legacy notice for fix"
assert_not_contains "$LEGACY_OUT" "::warning::" "summary.sh prints no warning for fix"
assert_not_contains "$(cat "$FIX_SUMMARY")" "legacy renderer" "summary.sh adds no legacy footnote for fix"

rm -rf "$LEGACY_WORK"

# --- Code Scanning availability gate (check-code-scanning.sh) ---

echo ""
echo "=== Code Scanning gate ==="

CSCRIPT="$DIR/../scripts/check-code-scanning.sh"
GATE_DIR=$(mktemp -d)
GATE_BIN="$GATE_DIR/bin"
mkdir -p "$GATE_BIN"

# Parameterized gh mock. MOCK_VISIBILITY sets the repos/{repo} --jq .visibility
# response; empty simulates a failed metadata read (gh exits non-zero).
# MOCK_ALERTS_EXIT sets the code-scanning/alerts probe exit code (0 = available).
cat > "$GATE_BIN/gh" <<'SH'
#!/usr/bin/env bash
case "$*" in
  *"code-scanning/alerts"*)
    exit "${MOCK_ALERTS_EXIT:-1}"
    ;;
  *"--jq"*".visibility"*)
    if [ -n "${MOCK_VISIBILITY:-}" ]; then
      printf '%s\n' "$MOCK_VISIBILITY"
      exit 0
    fi
    exit 1
    ;;
esac
exit 1
SH
chmod +x "$GATE_BIN/gh"

GATE_OUT=""
GATE_LOG=""
run_gate() {
  # $1 = visibility value (empty simulates read failure), $2 = alerts probe exit code
  local out_file
  out_file=$(mktemp)
  GATE_LOG=$(PATH="$GATE_BIN:$PATH" GH_REPO="acme/web" GITHUB_OUTPUT="$out_file" \
    MOCK_VISIBILITY="$1" MOCK_ALERTS_EXIT="$2" \
    bash "$CSCRIPT" 2>&1)
  GATE_OUT=$(cat "$out_file")
  rm -f "$out_file"
}

# Case 1: public repo is available even when the alerts probe would fail
# (the first upload initializes Code Scanning; this is the issue #817 fix).
run_gate "public" 1
assert_contains "$GATE_OUT" "available=true" "gate: public repo is available"
assert_not_contains "$GATE_LOG" "::warning::" "gate: public repo emits no skip warning"

# Case 2: private repo with GHAS (alerts probe succeeds) is available.
run_gate "private" 0
assert_contains "$GATE_OUT" "available=true" "gate: private repo with GHAS is available"
assert_not_contains "$GATE_LOG" "::warning::" "gate: private repo with GHAS emits no warning"

# Case 3: private repo without GHAS (alerts probe fails) skips with a warning.
run_gate "private" 1
assert_contains "$GATE_OUT" "available=false" "gate: private repo without GHAS is unavailable"
assert_contains "$GATE_LOG" "::warning::" "gate: private repo without GHAS warns"
assert_contains "$GATE_LOG" "private or internal repository" "gate: warning names private or internal repos"

# Case 4: internal (enterprise) repo with GHAS is available via the probe.
# This is the row the rejected `.private == false` approach would have broken.
run_gate "internal" 0
assert_contains "$GATE_OUT" "available=true" "gate: internal repo with GHAS is available"
assert_not_contains "$GATE_LOG" "::warning::" "gate: internal repo with GHAS emits no warning"

# Case 5: internal repo without GHAS skips with a warning; no fallback debug note
# (visibility was read successfully, so this is the intended probe, not a fallback).
run_gate "internal" 1
assert_contains "$GATE_OUT" "available=false" "gate: internal repo without GHAS is unavailable"
assert_contains "$GATE_LOG" "::warning::" "gate: internal repo without GHAS warns"
assert_contains "$GATE_LOG" "private or internal repository" "gate: internal warning names private or internal repos"
assert_not_contains "$GATE_LOG" "::debug::" "gate: internal repo emits no fallback debug note"

# Case 6: visibility read fails (empty) but the probe succeeds -> available via fallback.
run_gate "" 0
assert_contains "$GATE_OUT" "available=true" "gate: unreadable visibility falls back to probe (available)"
assert_contains "$GATE_LOG" "::debug::" "gate: unreadable visibility emits a fallback debug note"
assert_not_contains "$GATE_LOG" "::warning::" "gate: unreadable visibility with probe success emits no warning"

# Case 7: visibility read fails (empty) and the probe fails -> skip with warning + debug note.
run_gate "" 1
assert_contains "$GATE_OUT" "available=false" "gate: unreadable visibility with probe failure is unavailable"
assert_contains "$GATE_LOG" "::warning::" "gate: unreadable visibility with probe failure warns"
assert_contains "$GATE_LOG" "::debug::" "gate: unreadable visibility with probe failure emits a fallback debug note"

rm -rf "$GATE_DIR"

# --- IssueKind summary drift guard ---
#
# A new fallow dead-code IssueKind must be wired into every GitHub jq surface
# that serves every fallow version, or it vanishes silently from PR output.
# This guard derives the canonical dead-code id set (from `fallow schema`,
# falling back to issue_meta.rs) and asserts each one's JSON key is referenced
# by every gated surface.
#
# Surface expectations:
#   filter-changed.jq     "all"    per-changed-file filter + total_issues recount
#
# The summary-*.jq and annotations-*.jq files are frozen legacy renderers for
# fallow before 3.4.2, which do not emit newer kinds, so this guard does not
# gate them. The native renderers carry the kind coverage, and the Rust tests
# in crates/cli/tests/github_format_tests.rs and
# crates/cli/src/report/github_summary.rs check it.
#
# History: filter-changed.jq once omitted `test-only-dependency` (the key was
# absent from the total_issues recount so a --changed-since count undercounted
# it). That omission is now closed, so the surface gates "all" with no
# allow-list. The `allow:<ids>` machinery in the guard remains available for any
# future surface that legitimately carries only a documented subset.

echo ""
echo "=== IssueKind summary drift guard ==="

GUARD_DIR="$DIR"
# shellcheck source=action/tests/issuekind-drift-guard.sh
. "$DIR/issuekind-drift-guard.sh"
assert_issuekind_summary_coverage "github filter-changed"   "$JQ_DIR/filter-changed.jq"

# VS Code DIAGNOSTIC_CATEGORIES is the LSP diagnostic-code catalog the extension
# uses to filter, count, and render editor findings. It is provider-agnostic
# (not GitHub- or GitLab-specific), so it is checked once here. A new LSP-visible
# dead-code kind missing from it leaves the kind uncounted and unfilterable in
# the editor sidebar even though the server emits a squiggle for it.
assert_issuekind_vscode_category_coverage "vscode DIAGNOSTIC_CATEGORIES" \
  "$DIR/../../editors/vscode/src/generated/issue-types.ts"

# --- Baseline staleness gate (issue #2673) ---

echo ""
echo "=== Baseline staleness gate ==="

STALE_WORK=$(mktemp -d)
STALE_BIN="$STALE_WORK/bin"
mkdir -p "$STALE_BIN"

# The mock behaves like the real binary on the three things this gate reads:
# it answers the capability probes, it logs every analysis argv, and it emits an
# envelope whose `baseline_staleness` reflects whether the run was narrowed.
cat > "$STALE_BIN/fallow" <<'SH'
#!/usr/bin/env bash
for arg in "$@"; do
  if [ "$arg" = "--help" ]; then
    printf 'usage\n--no-type-aware\n'
    exit 0
  fi
done
if [ "${1:-}" = "report" ]; then
  printf '{}\n'
  exit 0
fi
printf 'analysis %s\n' "$*" >> "$MOCK_ANALYSIS_LOG"
if [ -n "${FALLOW_DIFF_FILE:-}" ]; then
  printf 'diff_file=set\n' >> "$MOCK_ANALYSIS_LOG"
fi
# Report the channels that narrowed this argv, the way the real binary
# derives them from the flags it was given. MOCK_SCOPE_REASONS overrides the
# list, which is how a run narrowed through the 'args' input is simulated: the
# envelope names a reason no INPUT_* variable would reveal.
# The real binary serializes the array in its own declaration order, never in
# argv order, so the mock sorts into that order too: the script's rule reads the
# list and a mock that emitted argv order would test a shape no run produces.
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
if [ "${MOCK_NO_STALENESS:-}" = "1" ]; then
  printf '{"schema_version":9,"total_issues":0,"baseline":{"entries":8,"matched":3}}\n'
  exit 0
fi
# An audit-shaped envelope: one staleness object per section, which is what
# makes the first-match `//` chain the wrong reader for this command.
if [ "${MOCK_AUDIT_BASELINES:-}" = "1" ]; then
  printf '{"kind":"audit","schema_version":6,"total_issues":0,"verdict":"pass","dead_code":{"baseline_staleness":{"baseline_entries":12,"matched_entries":4,"stale_entries":8,"current_findings":4,"change_scoped":true,"stale":false,"warning":"none","gate_trips":false,"scope_reasons":["changed-since"]}},"duplication":{"baseline_staleness":{"baseline_entries":3,"matched_entries":0,"stale_entries":3,"current_findings":0,"change_scoped":true,"stale":false,"warning":"none","gate_trips":false,"scope_reasons":["changed-files"]}},"complexity":{"summary":{"baseline_staleness":{"baseline_entries":0,"matched_entries":0,"stale_entries":0,"current_findings":0,"change_scoped":true,"stale":false,"warning":"none","gate_trips":false,"unrecognised_format":true,"scope_reasons":["changed-files"]}}},"gate_outcomes":{"stale-baseline":{"status":"skipped","enforced":false},"audit-verdict":{"status":"pass","enforced":true}}}\n'
  exit 0
fi
# The same shape with every section stating its recognition verdict outright,
# including a literal `false`, which the shared reader keeps distinct from an
# absent member.
if [ "${MOCK_AUDIT_BASELINES:-}" = "2" ]; then
  printf '{"kind":"audit","schema_version":6,"total_issues":0,"verdict":"pass","dead_code":{"baseline_staleness":{"baseline_entries":12,"matched_entries":4,"stale_entries":8,"current_findings":4,"change_scoped":true,"stale":false,"warning":"none","gate_trips":false,"unrecognised_format":false,"scope_reasons":["changed-since"]}},"complexity":{"summary":{"baseline_staleness":{"baseline_entries":0,"matched_entries":0,"stale_entries":0,"current_findings":0,"change_scoped":true,"stale":false,"warning":"none","gate_trips":true,"unrecognised_format":true,"scope_reasons":["changed-files"]}}},"gate_outcomes":{"stale-baseline":{"status":"skipped","enforced":false},"audit-verdict":{"status":"pass","enforced":true}}}\n'
  exit 0
fi
if [ "${MOCK_GATE_RUN_BROKEN:-}" = "1" ] && [ "$scoped" = "false" ]; then
  printf 'not json at all\n'
  exit 2
fi
advisory=${MOCK_ADVISORY:-partial}
entries=${MOCK_ENTRIES:-8}
matched=${MOCK_MATCHED:-3}
stale=$((entries - matched))
findings=${MOCK_FINDINGS:-3}
if [ "$scoped" = "true" ]; then
  if [ "${MOCK_NO_SCOPE_REASONS:-}" = "1" ]; then
    printf '{"schema_version":9,"total_issues":0,"baseline_staleness":{"baseline_entries":%s,"matched_entries":0,"stale_entries":%s,"current_findings":0,"change_scoped":true,"stale":false,"warning":"none","gate_trips":false}}\n' "$entries" "$entries"
  else
    printf '{"schema_version":9,"total_issues":0,"baseline_staleness":{"baseline_entries":%s,"matched_entries":0,"stale_entries":%s,"current_findings":0,"change_scoped":true,"stale":false,"warning":"none","gate_trips":false,"scope_reasons":[%s]}}\n' "$entries" "$entries" "$reasons"
  fi
  exit 0
fi
gate_trips=false
if [ "$stale" -gt 0 ]; then gate_trips=true; fi
stale_flag=false
if [ "$advisory" != "none" ]; then stale_flag=true; fi
unrecognised=""
if [ "${MOCK_UNRECOGNISED:-}" = "1" ]; then
  # The binary trips the gate on a file it cannot read as its own baseline, so
  # the mock carries the same pairing; a fixture that reported the old
  # gate_trips: false would test an envelope fallow no longer produces.
  unrecognised=',"unrecognised_format":true'
  gate_trips=true
fi
printf '{"schema_version":9,"total_issues":%s,"baseline_staleness":{"baseline_entries":%s,"matched_entries":%s,"stale_entries":%s,"current_findings":%s,"change_scoped":false,"stale":%s,"warning":"%s","gate_trips":%s%s}}\n' \
  "${MOCK_TOTAL_ISSUES:-0}" "$entries" "$matched" "$stale" "$findings" "$stale_flag" "$advisory" "$gate_trips" "$unrecognised"
if [ "${MOCK_EXIT_ONE:-}" = "1" ]; then
  exit 1
fi
SH
chmod +x "$STALE_BIN/fallow"

# Run analyze.sh with the action's environment. Prints stderr plus the workflow
# commands the script wrote, and records the exit status in STALE_EXIT.
run_stale_analyze() {
  local run_dir
  run_dir=$(mktemp -d "$STALE_WORK/run.XXXXXX")
  STALE_OUTPUT_FILE="$run_dir/github_output"
  STALE_ENV_FILE="$run_dir/github_env"
  STALE_SUMMARY_FILE="$run_dir/step_summary"
  MOCK_ANALYSIS_LOG="$run_dir/analysis.log"
  : > "$STALE_OUTPUT_FILE"
  : > "$STALE_ENV_FILE"
  : > "$STALE_SUMMARY_FILE"
  : > "$MOCK_ANALYSIS_LOG"
  set +e
  STALE_STDOUT=$(
    cd "$run_dir" \
      && PATH="$STALE_BIN:$PATH" \
      MOCK_ANALYSIS_LOG="$MOCK_ANALYSIS_LOG" \
      GITHUB_OUTPUT="$STALE_OUTPUT_FILE" \
      GITHUB_ENV="$STALE_ENV_FILE" \
      GITHUB_STEP_SUMMARY="$STALE_SUMMARY_FILE" \
      INPUT_ROOT="." \
      INPUT_FORMAT="json" \
      INPUT_ARTIFACTS_DIR="." \
      env "$@" bash "$SCRIPTS_DIR/analyze.sh" 2>&1
  )
  STALE_EXIT=$?
  set -e
  STALE_ANALYSIS_LOG=$(cat "$MOCK_ANALYSIS_LOG")
}

# 1. A partially stale baseline warns even with the gate off.
run_stale_analyze INPUT_COMMAND="dead-code" INPUT_BASELINE="baseline.json"
assert_contains "$STALE_STDOUT" "::warning::fallow: baseline is partially stale: 5 of 8 entries" \
  "stale gate: a partially stale baseline warns without the gate"
if [ "$STALE_EXIT" -eq 0 ]; then
  pass "stale gate: the advisory alone does not fail the run"
else
  fail "stale gate: the advisory alone does not fail the run" "exit ${STALE_EXIT}"
fi
assert_contains "$(cat "$STALE_OUTPUT_FILE")" "baseline_gate_trips=true" \
  "stale gate: the verdict reaches the step outputs"

# 2. Zero overlap has its own wording.
run_stale_analyze INPUT_COMMAND="dead-code" INPUT_BASELINE="baseline.json" \
  MOCK_ADVISORY="zero-overlap" MOCK_MATCHED="0"
assert_contains "$STALE_STDOUT" "::warning::fallow: baseline has 8 entries but matched nothing this run" \
  "stale gate: zero overlap keeps its own wording"

# 3. The issue's headline case: nothing left to report, every entry dead, the
# advisory silent by design, and the gate the only thing that can speak.
run_stale_analyze INPUT_COMMAND="dead-code" INPUT_BASELINE="baseline.json" \
  MOCK_ADVISORY="none" MOCK_MATCHED="0" MOCK_FINDINGS="0" \
  INPUT_FAIL_ON_STALE_BASELINE="true"
assert_contains "$STALE_STDOUT" "8 of 8 baseline entries matched nothing this run" \
  "stale gate: a rotted baseline on a clean project still warns"
assert_contains "$STALE_STDOUT" "::error::Fallow baseline gate failed: 8 of 8 entries" \
  "stale gate: the gate fails the run on a rotted baseline with no findings"
if [ "$STALE_EXIT" -eq 1 ]; then
  pass "stale gate: a tripped gate exits 1"
else
  fail "stale gate: a tripped gate exits 1" "exit ${STALE_EXIT}"
fi
assert_contains "$(cat "$STALE_OUTPUT_FILE")" "issues=0" \
  "stale gate: outputs are published before the gate fails"

# 4. `fail-on-issues` is a different gate and does not switch this one off.
run_stale_analyze INPUT_COMMAND="dead-code" INPUT_BASELINE="baseline.json" \
  INPUT_FAIL_ON_ISSUES="false" INPUT_FAIL_ON_STALE_BASELINE="true"
if [ "$STALE_EXIT" -eq 1 ]; then
  pass "stale gate: fail-on-issues false does not disable the stale-baseline gate"
else
  fail "stale gate: fail-on-issues false does not disable the stale-baseline gate" "exit ${STALE_EXIT}"
fi

# 5. A pull-request run is narrowed, so the gate re-runs the comparison unscoped
# and the second argv carries no narrowing and no writing flag.
run_stale_analyze INPUT_COMMAND="dead-code" INPUT_BASELINE="baseline.json" \
  INPUT_SAVE_BASELINE="baseline.json" INPUT_CHANGED_SINCE="abc123" \
  INPUT_ISSUE_TYPES="unused-files" INPUT_FAIL_ON_STALE_BASELINE="true"
GATE_ARGV=$(printf '%s\n' "$STALE_ANALYSIS_LOG" | grep '^analysis ' | sed -n '2p')
assert_contains "$STALE_ANALYSIS_LOG" "--changed-since abc123" \
  "stale gate: the primary run keeps its PR scoping"
assert_not_contains "$GATE_ARGV" "--changed-since" \
  "stale gate: the unscoped re-run drops --changed-since"
assert_not_contains "$GATE_ARGV" "--save-baseline" \
  "stale gate: the unscoped re-run never rewrites the baseline"
assert_not_contains "$GATE_ARGV" "--unused-files" \
  "stale gate: the unscoped re-run drops the issue-type filter"
assert_contains "$GATE_ARGV" "--baseline baseline.json" \
  "stale gate: the unscoped re-run still loads the baseline"
assert_contains "$STALE_STDOUT" "::error::Fallow baseline gate failed" \
  "stale gate: the re-run's verdict fails the pull-request job"

# 6. The advisory is not gated behind the input: a pull-request run re-reads the
# baseline unscoped so the warning reaches a job that asked for no gate. This is
# the #2627 complaint, which a gate-conditional re-read would have left open for
# every repository that runs the action on pull requests only.
run_stale_analyze INPUT_COMMAND="dead-code" INPUT_BASELINE="baseline.json" \
  INPUT_CHANGED_SINCE="abc123"
STALE_RUN_COUNT=$(printf '%s\n' "$STALE_ANALYSIS_LOG" | grep -c '^analysis ' || true)
if [ "$STALE_RUN_COUNT" = "2" ]; then
  pass "stale gate: a scoped run re-reads the baseline even with the gate off"
else
  fail "stale gate: a scoped run re-reads the baseline even with the gate off" "ran ${STALE_RUN_COUNT} times"
fi
assert_contains "$STALE_STDOUT" "::warning::fallow: baseline is partially stale: 5 of 8 entries" \
  "stale gate: the advisory reaches a pull-request run with no gate"
assert_not_contains "$STALE_STDOUT" "::error::" \
  "stale gate: with the gate off the advisory never fails the job"

# 6b. An unscoped run has nothing to re-read, so it still analyzes once.
run_stale_analyze INPUT_COMMAND="dead-code" INPUT_BASELINE="baseline.json"
STALE_RUN_COUNT=$(printf '%s\n' "$STALE_ANALYSIS_LOG" | grep -c '^analysis ' || true)
if [ "$STALE_RUN_COUNT" = "1" ]; then
  pass "stale gate: an unscoped run analyzes exactly once"
else
  fail "stale gate: an unscoped run analyzes exactly once" "ran ${STALE_RUN_COUNT} times"
fi

# 7. Production mode narrows discovery itself, so no re-run can fix it.
run_stale_analyze INPUT_COMMAND="dead-code" INPUT_BASELINE="baseline.json" \
  INPUT_CHANGED_SINCE="abc123" INPUT_PRODUCTION="true" \
  INPUT_FAIL_ON_STALE_BASELINE="true"
STALE_RUN_COUNT=$(printf '%s\n' "$STALE_ANALYSIS_LOG" | grep -c '^analysis ' || true)
if [ "$STALE_RUN_COUNT" = "1" ]; then
  pass "stale gate: production mode skips the re-run instead of paying for it"
else
  fail "stale gate: production mode skips the re-run instead of paying for it" "ran ${STALE_RUN_COUNT} times"
fi
assert_contains "$STALE_STDOUT" "::warning::fallow: baseline staleness could not be judged" \
  "stale gate: a stand-down is a warning, never a debug line"
assert_contains "$STALE_STDOUT" "fail-on-stale-baseline stood down." \
  "stale gate: the stand-down names the gate when the gate was asked for"
assert_not_contains "$STALE_STDOUT" "::debug::fallow: baseline staleness" \
  "stale gate: the stand-down never hides in ::debug::"
if [ "$STALE_EXIT" -eq 0 ]; then
  pass "stale gate: a stand-down does not fail the run"
else
  fail "stale gate: a stand-down does not fail the run" "exit ${STALE_EXIT}"
fi

# 7b. The same stand-down with no gate asked for is a notice, not a warning.
# Production mode plus a baseline is an ordinary configuration, and a warning
# nobody can turn off on every pull request is noise.
run_stale_analyze INPUT_COMMAND="dead-code" INPUT_BASELINE="baseline.json" \
  INPUT_CHANGED_SINCE="abc123" INPUT_PRODUCTION="true"
assert_contains "$STALE_STDOUT" "::notice::fallow: baseline staleness could not be judged" \
  "stale gate: a stand-down with no gate asked for is a notice"
assert_not_contains "$STALE_STDOUT" "::warning::fallow: baseline staleness could not be judged" \
  "stale gate: a run that asked for nothing is not warned at"
assert_not_contains "$STALE_STDOUT" "stood down" \
  "stale gate: the notice does not name a gate that was never requested"

# 7c. The stand-down names the channels the run reported rather than guessing
# from this script's own inputs, and the reasons reach the step outputs.
run_stale_analyze INPUT_COMMAND="dead-code" INPUT_BASELINE="baseline.json" \
  INPUT_CHANGED_SINCE="abc123" INPUT_PRODUCTION="true"
assert_contains "$STALE_STDOUT" "only part of the project (changed-since, production)" \
  "stale gate: the stand-down names the channels that narrowed the run"
assert_contains "$(cat "$STALE_OUTPUT_FILE")" "baseline_scope_reasons=changed-since, production" \
  "stale gate: the channels reach the step outputs"

# 7d. Scoping smuggled through the 'args' input is invisible to every INPUT_*
# variable, so the input-based guess would send the script into an unscoped
# re-read that comes back narrowed anyway. Reading the run's own reasons is what
# makes it stand down the first time.
run_stale_analyze INPUT_COMMAND="dead-code" INPUT_BASELINE="baseline.json" \
  MOCK_SCOPE_REASONS="production"
STALE_RUN_COUNT=$(printf '%s\n' "$STALE_ANALYSIS_LOG" | grep -c '^analysis ' || true)
if [ "$STALE_RUN_COUNT" = "1" ]; then
  pass "stale gate: an unremovable reason skips the re-read even with no matching input"
else
  fail "stale gate: an unremovable reason skips the re-read even with no matching input" "ran ${STALE_RUN_COUNT} times"
fi
assert_contains "$STALE_STDOUT" "only part of the project (production)" \
  "stale gate: the stand-down names the smuggled channel"

# 7e. A run narrowed only by channels this script can remove still pays for the
# re-read, which is the case the re-read exists for.
run_stale_analyze INPUT_COMMAND="dead-code" INPUT_BASELINE="baseline.json" \
  MOCK_SCOPE_REASONS="changed-files,scope"
STALE_RUN_COUNT=$(printf '%s\n' "$STALE_ANALYSIS_LOG" | grep -c '^analysis ' || true)
if [ "$STALE_RUN_COUNT" = "2" ]; then
  pass "stale gate: removable reasons still earn the unscoped re-read"
else
  fail "stale gate: removable reasons still earn the unscoped re-read" "ran ${STALE_RUN_COUNT} times"
fi

# 7f. A binary that predates the member keeps today's behaviour: the guess from
# the inputs, and the wording that names the two inputs it is built from.
run_stale_analyze INPUT_COMMAND="dead-code" INPUT_BASELINE="baseline.json" \
  INPUT_CHANGED_SINCE="abc123" INPUT_PRODUCTION="true" MOCK_NO_SCOPE_REASONS="1"
assert_contains "$STALE_STDOUT" "only part of the project (production mode or workspace scoping)" \
  "stale gate: a binary without the member falls back to the input-based reason"
assert_contains "$(cat "$STALE_OUTPUT_FILE")" "baseline_scope_reasons=" \
  "stale gate: the output is published empty rather than omitted"

# 8. The re-run exits 1 on findings, which is not an error here.
run_stale_analyze INPUT_COMMAND="dead-code" INPUT_BASELINE="baseline.json" \
  INPUT_CHANGED_SINCE="abc123" INPUT_FAIL_ON_STALE_BASELINE="true" \
  MOCK_EXIT_ONE="1" MOCK_TOTAL_ISSUES="4"
assert_contains "$STALE_STDOUT" "::error::Fallow baseline gate failed" \
  "stale gate: a re-run that exits 1 on findings still yields its verdict"

# 9. A re-run that produces nothing readable warns and leaves the job green.
run_stale_analyze INPUT_COMMAND="dead-code" INPUT_BASELINE="baseline.json" \
  INPUT_CHANGED_SINCE="abc123" INPUT_FAIL_ON_STALE_BASELINE="true" \
  MOCK_GATE_RUN_BROKEN="1"
assert_contains "$STALE_STDOUT" "produced no readable result" \
  "stale gate: a broken re-read warns"
if [ "$STALE_EXIT" -eq 0 ]; then
  pass "stale gate: a broken re-run fails open"
else
  fail "stale gate: a broken re-run fails open" "exit ${STALE_EXIT}"
fi

# 10. A pinned binary older than the envelope field must not fail the job.
run_stale_analyze INPUT_COMMAND="dead-code" INPUT_BASELINE="baseline.json" \
  INPUT_FAIL_ON_STALE_BASELINE="true" MOCK_NO_STALENESS="1"
assert_contains "$STALE_STDOUT" "A fallow that predates this feature cannot report it" \
  "stale gate: an old binary warns instead of failing silently"
if [ "$STALE_EXIT" -eq 0 ]; then
  pass "stale gate: an old binary fails open"
else
  fail "stale gate: an old binary fails open" "exit ${STALE_EXIT}"
fi

# 11. Commands that report no staleness are rejected at validation time.
run_stale_analyze INPUT_COMMAND="fix" INPUT_BASELINE="baseline.json" \
  INPUT_FAIL_ON_STALE_BASELINE="true"
assert_contains "$STALE_STDOUT" "reports no baseline staleness" \
  "stale gate: fix plus the gate is rejected with a reason"
if [ "$STALE_EXIT" -eq 2 ]; then
  pass "stale gate: an invalid combination exits 2"
else
  fail "stale gate: an invalid combination exits 2" "exit ${STALE_EXIT}"
fi

# 12. The gate with no baseline to judge is rejected too.
run_stale_analyze INPUT_COMMAND="dead-code" INPUT_FAIL_ON_STALE_BASELINE="true"
assert_contains "$STALE_STDOUT" "has no baseline to judge" \
  "stale gate: the gate without a baseline is rejected"

# 13. `fix` with a baseline and the gate off stays green and silent.
run_stale_analyze INPUT_COMMAND="fix" INPUT_BASELINE="baseline.json" \
  MOCK_NO_STALENESS="1"
assert_not_contains "$STALE_STDOUT" "baseline" \
  "stale gate: a command without staleness says nothing about baselines"

# 13b. Diff scoping reaches the CLI through FALLOW_DIFF_FILE, not argv, so the
# re-read must clear it from the child environment. Without this pin every other
# assertion would still pass if `env -u` were dropped.
run_stale_analyze INPUT_COMMAND="dead-code" INPUT_BASELINE="baseline.json" \
  INPUT_FAIL_ON_STALE_BASELINE="true" FALLOW_DIFF_FILE="/tmp/does-not-matter.diff"
STALE_DIFF_LINES=$(printf '%s\n' "$STALE_ANALYSIS_LOG" | grep -c '^diff_file=set' || true)
assert_contains "$STALE_ANALYSIS_LOG" "diff_file=set" \
  "stale gate: the primary run keeps the diff scoping it was given"
if [ "$STALE_DIFF_LINES" = "1" ]; then
  pass "stale gate: the unscoped re-read runs with FALLOW_DIFF_FILE cleared"
else
  fail "stale gate: the unscoped re-read runs with FALLOW_DIFF_FILE cleared" \
    "saw ${STALE_DIFF_LINES} invocations with the variable set"
fi
assert_contains "$STALE_STDOUT" "::error::Fallow baseline gate failed" \
  "stale gate: clearing the diff file lets the re-read judge the baseline"

# 13c. `--save-snapshot` takes an optional value, so the strip must not swallow
# the following flag when the bare form is used.
run_stale_analyze INPUT_COMMAND="health" INPUT_BASELINE="baseline.json" \
  INPUT_CHANGED_SINCE="abc123" INPUT_FAIL_ON_STALE_BASELINE="true" \
  INPUT_SAVE_SNAPSHOT="true" INPUT_TREND="true"
GATE_ARGV=$(printf '%s\n' "$STALE_ANALYSIS_LOG" | grep '^analysis ' | sed -n '2p')
assert_not_contains "$GATE_ARGV" "--save-snapshot" \
  "stale gate: the re-read never writes a snapshot"
assert_contains "$GATE_ARGV" "--trend" \
  "stale gate: a bare --save-snapshot does not swallow the next flag"
assert_contains "$STALE_STDOUT" "::error::Fallow baseline gate failed" \
  "stale gate: the re-read still judges the baseline after the strip"

# 13d. A baseline re-saved to the path it is read from can never be stale.
run_stale_analyze INPUT_COMMAND="dead-code" INPUT_BASELINE="baseline.json" \
  INPUT_SAVE_BASELINE="baseline.json"
assert_contains "$STALE_STDOUT" "baseline and save-baseline name the same file" \
  "stale gate: a self-healing baseline is called out"

# 14. The step summary carries the advisory on every render path, because both
# preferred paths return early and would otherwise drop it.
run_stale_summary() {
  local label=$1 expected=${STALE_SUMMARY_EXPECTED-"Baseline is partially stale"}
  shift
  local run_dir
  run_dir=$(mktemp -d "$STALE_WORK/summary.XXXXXX")
  printf '{"schema_version":9,"total_issues":0}\n' > "$run_dir/fallow-results.json"
  printf '{"body":"### typed body"}\n' > "$run_dir/envelope.json"
  set +e
  (
    cd "$run_dir" \
      && PATH="$STALE_BIN:$PATH" \
      MOCK_ANALYSIS_LOG="$run_dir/analysis.log" \
      GITHUB_STEP_SUMMARY="$run_dir/step_summary" \
      ACTION_JQ_DIR="$JQ_DIR" \
      FALLOW_COMMAND="dead-code" \
      FALLOW_RESULTS_FILE="$run_dir/fallow-results.json" \
      FALLOW_BASELINE_ENTRIES="8" \
      FALLOW_BASELINE_STALE_ENTRIES="5" \
      FALLOW_BASELINE_ADVISORY="partial" \
      FALLOW_BASELINE_GATE_TRIPS="true" \
      env "$@" bash "$SCRIPTS_DIR/summary.sh" > /dev/null 2>&1
  )
  set -e
  if [ -n "$expected" ]; then
    assert_contains "$(cat "$run_dir/step_summary")" "$expected" \
      "stale gate: the step summary carries the advisory on the ${label} path"
  else
    assert_not_contains "$(cat "$run_dir/step_summary")" "Baseline recognises nothing" \
      "stale gate: the step summary says nothing about the ${label} path"
  fi
}

run_stale_summary "native" HAS_NATIVE_REPORT="true"
run_stale_summary "typed" HAS_NATIVE_REPORT="false" \
  FALLOW_PR_COMMENT_ENVELOPE_FILE="envelope.json"
run_stale_summary "legacy renderer" HAS_NATIVE_REPORT="false"

# 15. A baseline written by another command suppresses nothing, so the run says
# so and an armed gate fails on it. The branch reads the binary's own verdict
# rather than the entry count, so the command here is only the one this mock's
# envelope shape models; the wrong-kind case that motivates it is pinned per
# command in the Rust integration tests.
run_stale_analyze INPUT_COMMAND="dead-code" INPUT_BASELINE="wrong-kind.json" \
  MOCK_ENTRIES="0" MOCK_MATCHED="0" MOCK_ADVISORY="none" MOCK_FINDINGS="0" \
  MOCK_UNRECOGNISED="1" INPUT_FAIL_ON_STALE_BASELINE="true"
assert_contains "$STALE_STDOUT" "::warning::fallow: the baseline at wrong-kind.json has no entries this command recognises" \
  "stale gate: a baseline that recognises nothing is called out"
assert_not_contains "$STALE_STDOUT" "0 of 0 baseline entries matched nothing" \
  "stale gate: the count advisory stands aside for the recognition warning"
assert_contains "$STALE_STDOUT" "::error::Fallow baseline gate failed: the baseline wrong-kind.json has no entries this command recognises" \
  "stale gate: the armed gate names the recognition failure, not a count"
if [ "$STALE_EXIT" -eq 1 ]; then
  pass "stale gate: an armed gate fails on a baseline nothing recognises"
else
  fail "stale gate: an armed gate fails on a baseline nothing recognises" "exit ${STALE_EXIT}"
fi

# Without the input the verdict is published and nothing fails, which is the
# contract every gate keeps.
run_stale_analyze INPUT_COMMAND="dead-code" INPUT_BASELINE="wrong-kind.json" \
  MOCK_ENTRIES="0" MOCK_MATCHED="0" MOCK_ADVISORY="none" MOCK_FINDINGS="0" \
  MOCK_UNRECOGNISED="1"
assert_contains "$(cat "$STALE_OUTPUT_FILE")" "baseline_unrecognised=true" \
  "stale gate: the recognition verdict reaches the step outputs"
assert_contains "$(cat "$STALE_OUTPUT_FILE")" "baseline_path<<" \
  "stale gate: the path is written in the delimiter form"
assert_not_contains "$(cat "$STALE_OUTPUT_FILE")" "baseline_path=" \
  "stale gate: the path is never written as a plain key=value line"
STALE_BASELINE_PATH_VALUE=$(awk '
  index($0, "baseline_path<<") == 1 { delim = substr($0, 16); reading = 1; next }
  reading && $0 == delim { exit }
  reading { print }
' "$STALE_OUTPUT_FILE")
if [ "$STALE_BASELINE_PATH_VALUE" = "wrong-kind.json" ]; then
  pass "stale gate: the path reaches the step outputs for the job summary"
else
  fail "stale gate: the path reaches the step outputs for the job summary" "got '${STALE_BASELINE_PATH_VALUE}'"
fi
if [ "$STALE_EXIT" -eq 0 ]; then
  pass "stale gate: a baseline nothing recognises does not fail a job that armed no gate"
else
  fail "stale gate: a baseline nothing recognises does not fail a job that armed no gate" "exit ${STALE_EXIT}"
fi

# A baseline passed through the `args` input never reaches INPUT_BASELINE, so
# the line degrades to the subject instead of going missing.
run_stale_analyze INPUT_COMMAND="dead-code" INPUT_ARGS="--baseline wrong-kind.json" \
  MOCK_ENTRIES="0" MOCK_MATCHED="0" MOCK_ADVISORY="none" MOCK_FINDINGS="0" \
  MOCK_UNRECOGNISED="1"
assert_contains "$STALE_STDOUT" "::warning::fallow: the loaded baseline has no entries this command recognises" \
  "stale gate: a baseline passed through args is called out without a path"

# A baseline saved on a project with nothing to record carries zero entries and
# is not a mistake, so the warning the repository cannot turn off must not fire
# on the documented save-on-green-main workflow.
run_stale_analyze INPUT_COMMAND="dead-code" INPUT_BASELINE="own-empty.json" \
  MOCK_ENTRIES="0" MOCK_MATCHED="0" MOCK_ADVISORY="none" MOCK_FINDINGS="0"
assert_not_contains "$STALE_STDOUT" "has no entries this command recognises" \
  "stale gate: a baseline this command saved itself is never called the wrong file"

# A populated baseline never earns that warning, whatever the advisory says.
run_stale_analyze INPUT_COMMAND="dead-code" INPUT_BASELINE="baseline.json"
assert_not_contains "$STALE_STDOUT" "has no entries this command recognises" \
  "stale gate: a populated baseline says nothing about recognition"

STALE_SUMMARY_EXPECTED="Baseline recognises nothing" \
  run_stale_summary "unrecognised" HAS_NATIVE_REPORT="true" \
  FALLOW_BASELINE_ENTRIES="0" FALLOW_BASELINE_UNRECOGNISED="true"

# The job summary is the surface people read, so it names the file rather than
# leaving the path to the step log (issue #2735).
STALE_SUMMARY_EXPECTED='The baseline at `baselines/dead-code.json` has no entries' \
  run_stale_summary "unrecognised with a path" HAS_NATIVE_REPORT="true" \
  FALLOW_BASELINE_ENTRIES="0" FALLOW_BASELINE_UNRECOGNISED="true" \
  FALLOW_BASELINE_PATH="baselines/dead-code.json"

# A backtick in the path must not close the code span early (issue #2756).
# The span uses a fence one backtick longer than the longest run in the path.
STALE_SUMMARY_EXPECTED='The baseline at ``baselines/a`b.json`` has no entries' \
  run_stale_summary "unrecognised with a backtick" HAS_NATIVE_REPORT="true" \
  FALLOW_BASELINE_ENTRIES="0" FALLOW_BASELINE_UNRECOGNISED="true" \
  FALLOW_BASELINE_PATH='baselines/a`b.json'

STALE_SUMMARY_EXPECTED='The baseline at `` `edge.json `` has no entries' \
  run_stale_summary "unrecognised with a leading backtick" HAS_NATIVE_REPORT="true" \
  FALLOW_BASELINE_ENTRIES="0" FALLOW_BASELINE_UNRECOGNISED="true" \
  FALLOW_BASELINE_PATH='`edge.json'

STALE_SUMMARY_EXPECTED="" \
  run_stale_summary "own empty baseline" HAS_NATIVE_REPORT="true" \
  FALLOW_BASELINE_ENTRIES="0" FALLOW_BASELINE_ADVISORY="none" \
  FALLOW_BASELINE_GATE_TRIPS="false"

# 16. `fallow audit` loads up to three baselines and judges none of them, and
# the single-analysis `//` chain is first-match, so it would report one and hide
# the other two. One line per section instead, naming the command a reader has
# to run: `duplication` is served by `fallow dupes` and `complexity` by
# `fallow health`, so the section label and the command deliberately differ.
run_stale_analyze INPUT_COMMAND="audit" MOCK_AUDIT_BASELINES="1" \
  INPUT_DEAD_CODE_BASELINE="audit/dc.json" \
  INPUT_DUPES_BASELINE="audit/du.json" \
  INPUT_HEALTH_BASELINE="audit/he.json"
assert_contains "$STALE_STDOUT" "::notice::fallow: the dead-code baseline (audit/dc.json) has 12 entries and was not judged" \
  "audit baselines: the dead-code baseline is reported with its path"
assert_contains "$STALE_STDOUT" "Run 'fallow dead-code --baseline audit/dc.json' over the whole project" \
  "audit baselines: the pointer names the unscoped command"
assert_contains "$STALE_STDOUT" "::notice::fallow: the duplication baseline (audit/du.json) has 3 entries and was not judged" \
  "audit baselines: the duplication baseline is reported too"
assert_contains "$STALE_STDOUT" "Run 'fallow dupes --baseline audit/du.json' over the whole project" \
  "audit baselines: duplication points at fallow dupes, not at the section name"
assert_contains "$STALE_STDOUT" "::warning::fallow: the complexity baseline at audit/he.json has no entries this command recognises" \
  "audit baselines: an unrecognised audit baseline gets the recognition warning"
if [ "$STALE_EXIT" -eq 0 ]; then
  pass "audit baselines: naming an inert baseline does not fail the job"
else
  fail "audit baselines: naming an inert baseline does not fail the job" "exit ${STALE_EXIT}"
fi

# Each section's own recognition verdict decides its line, including a section
# that states `false` outright: the loop reads it through the shared reader,
# whose `has` guard keeps a literal `false` from reading as an absent member.
run_stale_analyze INPUT_COMMAND="audit" MOCK_AUDIT_BASELINES="2" \
  INPUT_DEAD_CODE_BASELINE="audit/dc.json" \
  INPUT_HEALTH_BASELINE="audit/he.json"
assert_contains "$STALE_STDOUT" "::notice::fallow: the dead-code baseline (audit/dc.json) has 12 entries and was not judged" \
  "audit baselines: a section that reports recognition false keeps the inert-baseline notice"
assert_contains "$STALE_STDOUT" "::warning::fallow: the complexity baseline at audit/he.json has no entries this command recognises" \
  "audit baselines: and the section beside it still earns the recognition warning"
assert_not_contains "$STALE_STDOUT" "the dead-code baseline at audit/dc.json has no entries" \
  "audit baselines: a recognised baseline is never called the wrong file"

# Audit resolves all three from project config as well as from inputs, so there
# is not always a path to echo back.
run_stale_analyze INPUT_COMMAND="audit" MOCK_AUDIT_BASELINES="1"
assert_contains "$STALE_STDOUT" "::notice::fallow: the dead-code baseline has 12 entries and was not judged" \
  "audit baselines: a config-resolved baseline is reported without a path"
assert_contains "$STALE_STDOUT" "Run 'fallow dead-code' with that baseline over the whole project" \
  "audit baselines: the pointer degrades when there is no path to name"

# A single-analysis command keeps the first-match chain and gains no audit line.
run_stale_analyze INPUT_COMMAND="dead-code" INPUT_BASELINE="baseline.json"
assert_not_contains "$STALE_STDOUT" "was not judged on this run: fallow audit" \
  "audit baselines: a single-analysis run gains no audit pointer"

# 17. The gate cannot apply to audit and the `baseline` input is already
# rejected for it, so the pair is only reachable through `args`.
run_stale_analyze INPUT_COMMAND="audit" INPUT_ARGS="--fail-on-stale-baseline"
if [ "$STALE_EXIT" -eq 2 ]; then
  pass "audit baselines: --fail-on-stale-baseline smuggled through args is rejected"
else
  fail "audit baselines: --fail-on-stale-baseline smuggled through args is rejected" "exit ${STALE_EXIT}"
fi
assert_contains "$STALE_STDOUT" "cannot apply to command: audit" \
  "audit baselines: the rejection says why"

# The same flag in args on a command that CAN judge a baseline is untouched.
run_stale_analyze INPUT_COMMAND="dead-code" INPUT_BASELINE="baseline.json" \
  INPUT_ARGS="--fail-on-stale-baseline"
if [ "$STALE_EXIT" -ne 2 ]; then
  pass "audit baselines: the rejection is scoped to audit"
else
  fail "audit baselines: the rejection is scoped to audit" "exit ${STALE_EXIT}"
fi

rm -rf "$STALE_WORK"

# --- Gate verdicts (issues #2680, #2681, #2683, #2685, #2686) ---
#
# The mock emits a caller-supplied envelope, so each case pins one gate shape.
# The gate decision lives in analyze.sh rather than in an inline action.yml
# `run:` block precisely so it can be driven here.

echo ""
echo "Gate verdicts"

GATE_WORK=$(mktemp -d)
GATE_BIN="$GATE_WORK/bin"
mkdir -p "$GATE_BIN"
cat > "$GATE_BIN/fallow" <<'GATE_MOCK'
#!/usr/bin/env bash
printf '%s\n' "$*" >> "${MOCK_ANALYSIS_LOG:-/dev/null}"
case "$*" in
  *--help*) echo "--sarif-file --format json"; exit 0 ;;
  *report*) exit 0 ;;
esac
cat "$MOCK_GATE_ENVELOPE"
exit "${MOCK_GATE_EXIT:-0}"
GATE_MOCK
chmod +x "$GATE_BIN/fallow"

# Every fixture is a minimally valid dead-code envelope plus the gate shape
# under test, so the surrounding script behaves exactly as it does in production.
gate_envelope() {
  local gates=$1 extra=${2:-} body
  body='"kind":"dead-code","schema_version":9,"version":"3.27.0","total_issues":0,"summary":{"total_issues":0},"unused_files":[],"unused_exports":[]'
  [ -n "$gates" ] && body="${body},\"gate_outcomes\":${gates}"
  [ -n "$extra" ] && body="${body},${extra}"
  printf '{%s}\n' "$body"
}

run_gate_analyze() {
  local envelope=$1; shift
  local run_dir
  run_dir=$(mktemp -d "$GATE_WORK/run.XXXXXX")
  printf '%s' "$envelope" > "$run_dir/envelope.json"
  GATE_OUTPUT_FILE="$run_dir/github_output"
  : > "$GATE_OUTPUT_FILE"
  : > "$run_dir/analysis.log"
  set +e
  GATE_STDOUT=$(
    cd "$run_dir" \
      && PATH="$GATE_BIN:$PATH" \
      MOCK_GATE_ENVELOPE="$run_dir/envelope.json" \
      MOCK_ANALYSIS_LOG="$run_dir/analysis.log" \
      GITHUB_OUTPUT="$GATE_OUTPUT_FILE" \
      GITHUB_ENV="$run_dir/github_env" \
      GITHUB_STEP_SUMMARY="$run_dir/step_summary" \
      INPUT_ROOT="." \
      INPUT_FORMAT="json" \
      INPUT_ARTIFACTS_DIR="." \
      env "$@" bash "$SCRIPTS_DIR/analyze.sh" 2>&1
  )
  GATE_EXIT=$?
  set -e
  GATE_OUTPUTS=$(cat "$GATE_OUTPUT_FILE")
  GATE_ARGV=$(cat "$run_dir/analysis.log")
}

# The output is declared as a comma-separated list, so "present and empty" is a
# line with nothing after the `=`. Matching the key alone also matches a
# populated value, which is how a test named for the empty case can never fail.
assert_requests_unapplied_empty() {
  local name="$1"
  if grep -qx 'requests_unapplied=' <<< "$GATE_OUTPUTS"; then
    pass "$name"
  else
    fail "$name" "expected an empty requests_unapplied line, got: $GATE_OUTPUTS"
  fi
}

# The headline of every issue in this batch: the gate fails the job even though
# fail-on-issues is false, because the two are independent.
for gate_case in \
  'regression|INPUT_FAIL_ON_REGRESSION=true|Fallow regression gate failed' \
  'duplication-threshold|INPUT_THRESHOLD=5|Fallow duplication-threshold gate failed' \
  'health-min-score|INPUT_MIN_SCORE=90|Fallow health-min-score gate failed' \
  'health-min-severity|INPUT_MIN_SEVERITY=critical|Fallow health-min-severity gate failed' \
  ; do
  IFS='|' read -r gate_name gate_input expected <<< "$gate_case"
  command_for_gate="dead-code"
  extra_for_gate=""
  case "$gate_name" in
    health-*) command_for_gate="health"; extra_for_gate='"summary":{"functions_above_threshold":0}' ;;
    duplication-*) command_for_gate="dupes"; extra_for_gate='"stats":{"clone_groups":0}' ;;
  esac
  printf -v gate_outcome '{"%s":{"status":"fail","enforced":true}}' "$gate_name"
  run_gate_analyze "$(gate_envelope "$gate_outcome" "$extra_for_gate")" \
    INPUT_COMMAND="$command_for_gate" INPUT_FAIL_ON_ISSUES="false" "$gate_input"
  assert_contains "$GATE_STDOUT" "::error::$expected" \
    "gate: $gate_name fails the job with fail-on-issues false"
  if [ "$GATE_EXIT" = "1" ]; then
    pass "gate: $gate_name exits 1"
  else
    fail "gate: $gate_name exits 1" "got $GATE_EXIT: $GATE_STDOUT"
  fi
done

# #2685: the security gate keeps its documented exit 8, and it used to be
# unreachable because the whole branch sat inside the fail-on-issues conditional.
run_gate_analyze "$(gate_envelope '{"security":{"status":"fail","enforced":true}}' '"gate":{"mode":"new","verdict":"fail","new_count":2}')" \
  INPUT_COMMAND="security" INPUT_FAIL_ON_ISSUES="false" INPUT_SECURITY_GATE="new"
assert_contains "$GATE_STDOUT" "::error::Fallow security gate failed" \
  "gate: security fails with fail-on-issues false"
if [ "$GATE_EXIT" = "8" ]; then
  pass "gate: security keeps exit 8"
else
  fail "gate: security keeps exit 8" "got $GATE_EXIT"
fi

# #2685's second criterion: the verdict wins over the count.
run_gate_analyze "$(gate_envelope '{"security":{"status":"fail","enforced":true}}' '"gate":{"mode":"new","verdict":"fail","new_count":0}')" \
  INPUT_COMMAND="security" INPUT_FAIL_ON_ISSUES="false" INPUT_SECURITY_GATE="new"
if [ "$GATE_EXIT" = "8" ]; then
  pass "gate: a fail verdict with new_count 0 still exits 8"
else
  fail "gate: a fail verdict with new_count 0 still exits 8" "got $GATE_EXIT"
fi

# A gate the repository did not ask for reports and never fails, so
# fail-on-issues: false stays authoritative for a flag passed through args:.
run_gate_analyze "$(gate_envelope '{"regression":{"status":"fail","enforced":true}}')" \
  INPUT_COMMAND="dead-code" INPUT_FAIL_ON_ISSUES="false"
assert_contains "$GATE_STDOUT" "::warning::Fallow regression gate reports a failure" \
  "gate: an unowned failure warns"
assert_not_contains "$GATE_STDOUT" "::error::Fallow regression gate failed" \
  "gate: an unowned failure prints no error"
if [ "$GATE_EXIT" = "0" ]; then
  pass "gate: an unowned failure leaves the job green"
else
  fail "gate: an unowned failure leaves the job green" "got $GATE_EXIT"
fi

# Every envelope carries its default exit rule, also when no gate was armed.
# A failing default rule belongs to the count gate: with fail-on-issues false
# it prints nothing and leaves the job green, whether the CLI enforced it or
# (combined mode) did not.
# The outputs still name the failing default rule, so a step that reads
# `gates-failed != ''` sees it on every run with findings.
for default_case in \
  'dead-code|{"error-severity-findings":{"status":"fail","enforced":true}}||error-severity-findings' \
  'health|{"health-findings":{"status":"fail","enforced":true}}|"summary":{"functions_above_threshold":2}|health-findings' \
  'dead-code|{"error-severity-findings":{"status":"fail","enforced":false},"health-findings":{"status":"fail","enforced":false}}||error-severity-findings,health-findings' \
  ; do
  IFS='|' read -r default_command default_gates default_extra default_failed <<< "$default_case"
  run_gate_analyze "$(gate_envelope "$default_gates" "$default_extra")" \
    INPUT_COMMAND="$default_command" INPUT_FAIL_ON_ISSUES="false"
  if grep -qx "gates_failed=${default_failed}" <<< "$GATE_OUTPUTS"; then
    pass "gate: a default rule on $default_command is named in gates_failed"
  else
    fail "gate: a default rule on $default_command is named in gates_failed" \
      "expected gates_failed=${default_failed}, got: $GATE_OUTPUTS"
  fi
  assert_not_contains "$GATE_STDOUT" "gate reports a failure" \
    "gate: a default rule on $default_command $default_gates prints no gate line"
  assert_not_contains "$GATE_STDOUT" "::error::" \
    "gate: a default rule on $default_command $default_gates prints no error"
  if [ "$GATE_EXIT" = "0" ]; then
    pass "gate: a default rule on $default_command leaves the job green"
  else
    fail "gate: a default rule on $default_command leaves the job green" "got $GATE_EXIT: $GATE_STDOUT"
  fi
done

# A gate the CLI reports as unenforced (combined mode, --report-only) is
# honoured rather than overridden, and says which it was.
run_gate_analyze "$(gate_envelope '{"duplication-threshold":{"status":"fail","enforced":false,"observed":100.0,"threshold":5.0}}' '"stats":{"clone_groups":1}')" \
  INPUT_COMMAND="dupes" INPUT_FAIL_ON_ISSUES="false" INPUT_THRESHOLD="5"
assert_contains "$GATE_STDOUT" "this run does not enforce that gate" \
  "gate: an unenforced verdict names the reason"
if [ "$GATE_EXIT" = "0" ]; then
  pass "gate: an unenforced verdict leaves the job green"
else
  fail "gate: an unenforced verdict leaves the job green" "got $GATE_EXIT"
fi

# skipped is neither a pass nor a failure, and a gate the repository asked for
# and did not get is worth a warning (the #2674 stand-down rule).
run_gate_analyze "$(gate_envelope '{"regression":{"status":"skipped","enforced":false}}')" \
  INPUT_COMMAND="dead-code" INPUT_FAIL_ON_ISSUES="false" INPUT_FAIL_ON_REGRESSION="true"
assert_contains "$GATE_STDOUT" "::warning::Fallow regression gate stood down" \
  "gate: a requested gate that stood down warns"
run_gate_analyze "$(gate_envelope '{"regression":{"status":"skipped","enforced":false}}')" \
  INPUT_COMMAND="dead-code" INPUT_FAIL_ON_ISSUES="false"
assert_contains "$GATE_STDOUT" "::notice::Fallow regression gate stood down" \
  "gate: an unrequested gate that stood down is a notice"

# Three gates at once: every line prints and the step exits once.
run_gate_analyze "$(gate_envelope '{"regression":{"status":"fail","enforced":true},"security":{"status":"fail","enforced":true},"health-min-score":{"status":"fail","enforced":true}}')" \
  INPUT_COMMAND="security" INPUT_FAIL_ON_ISSUES="false" INPUT_FAIL_ON_REGRESSION="true" INPUT_SECURITY_GATE="new"
assert_contains "$GATE_STDOUT" "::error::Fallow regression gate failed" "gate: multi-failure prints regression"
assert_contains "$GATE_STDOUT" "::error::Fallow security gate failed" "gate: multi-failure prints security"
if [ "$GATE_EXIT" = "8" ]; then
  pass "gate: security outranks the generic exit 1"
else
  fail "gate: security outranks the generic exit 1" "got $GATE_EXIT"
fi

# Outputs and the step summary are written before the failing exit, so the
# downstream steps still have something to read.
assert_contains "$GATE_OUTPUTS" "gates_failed=" "gate: outputs are written before the exit"
assert_contains "$GATE_OUTPUTS" "results=" "gate: the results output survives a failing gate"

# Audit stays governed by fail-on-issues, so a reporting configuration keeps
# reporting.
run_gate_analyze "$(gate_envelope '{"audit-verdict":{"status":"fail","enforced":true}}' '"verdict":"fail","attribution":{"gate":"all"}')" \
  INPUT_COMMAND="audit" INPUT_FAIL_ON_ISSUES="false" INPUT_GATE="all"
if [ "$GATE_EXIT" = "0" ]; then
  pass "gate: audit with fail-on-issues false stays a reporting configuration"
else
  fail "gate: audit with fail-on-issues false stays a reporting configuration" "got $GATE_EXIT: $GATE_STDOUT"
fi

# A pinned binary older than the index: the gates that already published a
# feature-local field still work, and only the three that never had one warn.
run_gate_analyze "$(gate_envelope '' '"regression":{"exceeded":true,"delta":4,"baseline_total":1,"current_total":5}')" \
  INPUT_COMMAND="dead-code" INPUT_FAIL_ON_ISSUES="false" INPUT_FAIL_ON_REGRESSION="true"
assert_contains "$GATE_STDOUT" "::error::Fallow regression gate failed" \
  "gate: the regression fallback reads .regression.exceeded"
run_gate_analyze "$(gate_envelope '' '"stats":{"clone_groups":0}')" \
  INPUT_COMMAND="dupes" INPUT_FAIL_ON_ISSUES="false" INPUT_THRESHOLD="5"
assert_contains "$GATE_STDOUT" "could not be checked" \
  "gate: a gate with no fallback fails open with one warning"
run_gate_analyze "$(gate_envelope '')" INPUT_COMMAND="dead-code" INPUT_FAIL_ON_ISSUES="false"
assert_not_contains "$GATE_STDOUT" "could not be checked" \
  "gate: a pinned binary warns about nothing the repository did not configure"

# #2686: one aggregated warning, not one per kind, and the empty case is its own
# sentence behind its own input.
DEGRADED='"workspace_diagnostics":[{"path":"a","kind":"skipped-large-file","message":"m","degrades_analysis":true},{"path":"b","kind":"skipped-large-file","message":"m","degrades_analysis":true},{"path":"c","kind":"node-modules-missing","message":"m","degrades_analysis":true},{"path":".","kind":"boundaries-not-configured","message":"m"}]'
run_gate_analyze "$(gate_envelope '' "$DEGRADED")" INPUT_COMMAND="dead-code" INPUT_FAIL_ON_ISSUES="false"
assert_contains "$GATE_STDOUT" "node-modules-missing (1), skipped-large-file (2)" \
  "degraded: kinds and counts are aggregated into one warning"
assert_contains "$GATE_STDOUT" "Fallow ran with degraded inputs" \
  "degraded: the sentence covers a degraded input as well as a narrower file set"
assert_not_contains "$GATE_STDOUT" "boundaries-not-configured" \
  "degraded: the unconfigured-check kinds are not reported"
assert_contains "$GATE_OUTPUTS" "analysis_degraded=true" "degraded: the output is set"

# #2689: the health pipeline's own degraded inputs reach the same aggregated
# warning through the same selector, with no change to this script's jq.
HEALTH_DEGRADED='"workspace_diagnostics":[{"path":".","kind":"hotspots-skipped","message":"m","degrades_analysis":true},{"path":".","kind":"shallow-clone","message":"m","degrades_analysis":true},{"path":"coverage/coverage-final.json","kind":"coverage-auto-detected","message":"m"}]'
run_gate_analyze "$(gate_envelope '' "$HEALTH_DEGRADED")" INPUT_COMMAND="health" INPUT_FAIL_ON_ISSUES="false"
assert_contains "$GATE_STDOUT" "hotspots-skipped (1), shallow-clone (1)" \
  "degraded: the health kinds are reported without a script change"
assert_not_contains "$GATE_STDOUT" "coverage-auto-detected" \
  "degraded: auto-detected coverage is provenance and not a degraded run"

# #2736: a framework plugin that could not read a build config reaches the same
# aggregated warning through the same selector, and the quiet sibling kind stays
# out of it.
PLUGIN_DEGRADED='"workspace_diagnostics":[{"path":"module-federation.config.ts","kind":"plugin-config-unreadable","plugin":"module-federation","key":"exposes","reason":"not-object-literal","message":"m","degrades_analysis":true},{"path":"module-federation.config.ts","kind":"plugin-config-unreadable","plugin":"module-federation","key":"remotes","reason":"spread","message":"m","degrades_analysis":true},{"path":"nuxt.config.ts","kind":"plugin-effect-not-modeled","plugin":"nuxt","key":"components","reason":"key-effect-not-modeled","message":"m"}]'
run_gate_analyze "$(gate_envelope '' "$PLUGIN_DEGRADED")" INPUT_COMMAND="dead-code" INPUT_FAIL_ON_ISSUES="false"
assert_contains "$GATE_STDOUT" "plugin-config-unreadable (2)" \
  "degraded: two unreadable keys in one config are counted separately"
assert_not_contains "$GATE_STDOUT" "plugin-effect-not-modeled" \
  "degraded: a config whose effect is not modeled lost nothing measurable"
assert_contains "$GATE_OUTPUTS" "analysis_degraded=true" \
  "degraded: a plugin config nobody could read sets the output"

# #2757: the reason set is open, so the call and import reasons reach the same
# line with no script change.
PLUGIN_NEW_REASONS='"workspace_diagnostics":[{"path":"webpack.config.js","kind":"plugin-config-unreadable","plugin":"webpack","key":"exposes","reason":"unrecognized-call","message":"m","degrades_analysis":true},{"path":"rspack.config.js","kind":"plugin-config-unreadable","plugin":"rspack","key":"remotes","reason":"import-target-unreadable","message":"m","degrades_analysis":true}]'
run_gate_analyze "$(gate_envelope '' "$PLUGIN_NEW_REASONS")" INPUT_COMMAND="dead-code" INPUT_FAIL_ON_ISSUES="false"
assert_contains "$GATE_STDOUT" "plugin-config-unreadable (2)" \
  "degraded: the call and import reasons are counted like every other reason"
assert_contains "$GATE_OUTPUTS" "analysis_degraded=true" \
  "degraded: the call and import reasons set the output"

# #2687, #2688: the fact the CLI can only report on the wire, because this step
# always runs it with --quiet and a machine format.
REQUESTS_UNAPPLIED_FIXTURE='"request_outcomes":{"changed-since":{"status":"not-applied","affects":"scope","requested":"origin/main","reason":"git-failed","message":"m"},"diff-filter":{"status":"applied","affects":"scope","requested":"$FALLOW_DIFF_FILE pr.diff"}}'
run_gate_analyze "$(gate_envelope '' "$REQUESTS_UNAPPLIED_FIXTURE")" \
  INPUT_COMMAND="dead-code" INPUT_FAIL_ON_ISSUES="false"
assert_contains "$GATE_STDOUT" "::warning::Fallow could not apply: changed-since (git-failed)" \
  "requests: an unapplied request warns once with its reason"
assert_not_contains "$GATE_STDOUT" "diff-filter" \
  "requests: an honoured request is not named in the warning"
assert_contains "$GATE_OUTPUTS" "requests_unapplied=changed-since (git-failed)" \
  "requests: the output carries the unapplied names"
if [ "$GATE_EXIT" = "0" ]; then
  pass "requests: an unapplied request does not fail the job"
else
  fail "requests: an unapplied request does not fail the job" "got $GATE_EXIT: $GATE_STDOUT"
fi

# Applied-only, and absent: neither may produce a warning or a populated
# output, or every scoped run in CI would carry a false alarm.
REQUESTS_APPLIED_FIXTURE='"request_outcomes":{"diff-filter":{"status":"applied","affects":"scope","requested":"--diff-stdin"}}'
run_gate_analyze "$(gate_envelope '' "$REQUESTS_APPLIED_FIXTURE")" \
  INPUT_COMMAND="dead-code" INPUT_FAIL_ON_ISSUES="false"
assert_not_contains "$GATE_STDOUT" "could not apply" \
  "requests: a run that applied everything it was asked stays silent"
assert_not_contains "$GATE_STDOUT" "empty scope" \
  "requests: an applied request that measured nothing is not called empty"
assert_requests_unapplied_empty "requests: the output is present and empty when everything applied"

# #2734: the applied-but-empty scope. The unapplied selector must stay clear of
# it (the request DID apply) while the advisory names it, because the clean
# report underneath covered nothing.
REQUESTS_EMPTY_SCOPE_FIXTURE='"request_outcomes":{"diff-filter":{"status":"applied","affects":"scope","requested":"--diff-file pr.diff","scope_size":0}}'
run_gate_analyze "$(gate_envelope '' "$REQUESTS_EMPTY_SCOPE_FIXTURE")" \
  INPUT_COMMAND="dead-code" INPUT_FAIL_ON_ISSUES="false"
assert_contains "$GATE_STDOUT" "::warning::Fallow applied diff-filter over an empty scope" \
  "requests: an applied request over an empty scope is advised"
assert_not_contains "$GATE_STDOUT" "could not apply" \
  "requests: an empty scope is not reported as an unapplied request"
assert_requests_unapplied_empty "requests: the unapplied output stays empty for an empty scope"
if [ "$GATE_EXIT" = "0" ]; then
  pass "requests: an empty scope does not fail the job"
else
  fail "requests: an empty scope does not fail the job" "got $GATE_EXIT: $GATE_STDOUT"
fi

REQUESTS_FULL_SCOPE_FIXTURE='"request_outcomes":{"diff-filter":{"status":"applied","affects":"scope","requested":"--diff-file pr.diff","scope_size":12}}'
run_gate_analyze "$(gate_envelope '' "$REQUESTS_FULL_SCOPE_FIXTURE")" \
  INPUT_COMMAND="dead-code" INPUT_FAIL_ON_ISSUES="false"
assert_not_contains "$GATE_STDOUT" "empty scope" \
  "requests: a measured non-empty scope stays silent"

run_gate_analyze "$(gate_envelope '')" INPUT_COMMAND="dead-code" INPUT_FAIL_ON_ISSUES="false"
assert_not_contains "$GATE_STDOUT" "could not apply" \
  "requests: a pinned binary that publishes no object warns about nothing"

# A request that writes a file BESIDE the report narrows nothing, so the scope
# warning and the scope-shaped output must both stay clear of it. The
# SARIF-absence warning owns that case and says the right thing about it.
REQUESTS_ARTIFACT_FIXTURE='"request_outcomes":{"sarif-file":{"status":"not-applied","affects":"artifact","requested":"fallow-results.sarif","reason":"write-failed","message":"m"}}'
run_gate_analyze "$(gate_envelope '' "$REQUESTS_ARTIFACT_FIXTURE")" \
  INPUT_COMMAND="dead-code" INPUT_FAIL_ON_ISSUES="false"
assert_not_contains "$GATE_STDOUT" "could not apply" \
  "requests: an unwritten output file is not reported as an unscoped run"
assert_requests_unapplied_empty "requests: the scope output stays empty when only an output file failed"

EMPTY='"workspace_diagnostics":[{"path":".","kind":"no-source-files-analyzed","message":"m","excluded_file_count":3,"degrades_analysis":true}]'
run_gate_analyze "$(gate_envelope '' "$EMPTY")" INPUT_COMMAND="dead-code" INPUT_FAIL_ON_ISSUES="true"
assert_contains "$GATE_STDOUT" "::warning::Fallow analyzed no source file at all" \
  "empty analysis: warns by default"
if [ "$GATE_EXIT" = "0" ]; then
  pass "empty analysis: passes by default"
else
  fail "empty analysis: passes by default" "got $GATE_EXIT"
fi
run_gate_analyze "$(gate_envelope '' "$EMPTY")" INPUT_COMMAND="dead-code" INPUT_FAIL_ON_ISSUES="true" INPUT_FAIL_ON_EMPTY_ANALYSIS="true"
assert_contains "$GATE_STDOUT" "::error::Fallow analyzed no source file at all" \
  "empty analysis: fails behind the input"
if [ "$GATE_EXIT" = "1" ]; then
  pass "empty analysis: exits 1 behind the input"
else
  fail "empty analysis: exits 1 behind the input" "got $GATE_EXIT"
fi

# A gate name that is not a plain identifier never reaches an output or a
# workflow command.
run_gate_analyze "$(gate_envelope '{"evil\ninjected=1":{"status":"fail","enforced":true}}')" \
  INPUT_COMMAND="dead-code" INPUT_FAIL_ON_ISSUES="false"
assert_not_contains "$GATE_OUTPUTS" "injected=1" "gate: a malformed gate name is dropped"

# #2681 C11: the bare command never forwarded the threshold, so the gate could
# not appear in its envelope at all.
run_gate_analyze "$(gate_envelope '')" INPUT_COMMAND="" INPUT_THRESHOLD="7" INPUT_FAIL_ON_ISSUES="false"
assert_contains "$GATE_ARGV" "--dupes-threshold 7" "gate: the bare command forwards the threshold"

# #2682: --min-score implies --score, so the action keeps the reporting
# surfaces populated unless the caller selected a section itself.
run_gate_analyze "$(gate_envelope '')" INPUT_COMMAND="health" INPUT_MIN_SCORE="90" INPUT_FAIL_ON_ISSUES="false"
assert_contains "$GATE_ARGV" "--min-score 90" "gate: min-score reaches the CLI"
assert_contains "$GATE_ARGV" "--complexity" "gate: min-score adds --complexity"
run_gate_analyze "$(gate_envelope '')" INPUT_COMMAND="health" INPUT_MIN_SCORE="90" INPUT_COMPLEXITY="true" INPUT_FAIL_ON_ISSUES="false"
assert_contains "$GATE_ARGV" "--complexity" "gate: an explicit section input is not doubled"

# Health-only inputs are rejected on other commands rather than arming nothing.
run_gate_analyze "$(gate_envelope '' '"stats":{"clone_groups":0}')" INPUT_COMMAND="dupes" INPUT_MIN_SCORE="90"
assert_contains "$GATE_STDOUT" "applies to command: health only" "gate: min-score is rejected off health"
if [ "$GATE_EXIT" = "2" ]; then
  pass "gate: min-score off health exits 2"
else
  fail "gate: min-score off health exits 2" "got $GATE_EXIT"
fi
run_gate_analyze "$(gate_envelope '')" INPUT_COMMAND="health" INPUT_MIN_SCORE="90" INPUT_ARGS="--report-only"
assert_contains "$GATE_STDOUT" "cannot be combined with the min-score" "gate: report-only plus min-score is rejected"

# N8: two paths record no error for the count, so the advisory must still
# print. Keying it on `fail-on-issues` alone left both silent about the count.
run_gate_analyze "$(gate_envelope '{"health-min-score":{"status":"pass","enforced":true},"health-findings":{"status":"skipped","enforced":false}}' '"summary":{"functions_above_threshold":3}')" \
  INPUT_COMMAND="health" INPUT_FAIL_ON_ISSUES="true" INPUT_MIN_SCORE="1"
assert_contains "$GATE_STDOUT" "::warning::Fallow found 3 high complexity functions" \
  "count: a health run whose count gate stood down still reports the count"
if [ "$GATE_EXIT" = "0" ]; then
  pass "count: that run still passes"
else
  fail "count: that run still passes" "got $GATE_EXIT: $GATE_STDOUT"
fi

run_gate_analyze "$(gate_envelope '{"audit-verdict":{"status":"warn","enforced":true}}' '"verdict":"warn","attribution":{"gate":"new-only","dead_code_introduced":2,"complexity_introduced":0,"duplication_introduced":0,"styling_introduced":0}')" \
  INPUT_COMMAND="audit" INPUT_FAIL_ON_ISSUES="true" INPUT_GATE="new-only"
assert_contains "$GATE_STDOUT" "::warning::Fallow audit found 2 introduced issues in changed files" \
  "count: an audit warn verdict still reports the count"
if [ "$GATE_EXIT" = "0" ]; then
  pass "count: an audit warn verdict still passes"
else
  fail "count: an audit warn verdict still passes" "got $GATE_EXIT: $GATE_STDOUT"
fi

# And the duplicate the advisory was moved to avoid stays avoided.
run_gate_analyze "$(gate_envelope '')" INPUT_COMMAND="dead-code" INPUT_FAIL_ON_ISSUES="true" \
  MOCK_GATE_EXIT=1
assert_not_contains "$GATE_STDOUT" "::warning::Fallow found" \
  "count: a failing count gate still prints the fact once"

# T1: the #2674 unscoped re-read must not inherit the health gate flags.
# `--min-score` is a section selector, so a score-only envelope may carry no
# baseline_staleness at all and the gate would go silent with no message.
STRIP_ARGS=$(
  eval "$(sed -n '/^build_stale_gate_args()/,/^}/p' "$SCRIPTS_DIR/analyze.sh")"
  ARGS=(dead-code --root . --quiet --format json --baseline baseline.json --min-score 90 --complexity --changed-since abc123)
  EXTRA_ARGS=()
  build_stale_gate_args
  printf '%s ' "${GATE_ARGS[@]}"
)
assert_not_contains "$STRIP_ARGS" "--min-score" "re-read: --min-score is stripped"
assert_not_contains "$STRIP_ARGS" "--complexity" "re-read: --complexity is stripped"
assert_not_contains "$STRIP_ARGS" "--changed-since" "re-read: the narrowing flag is still stripped"
assert_contains "$STRIP_ARGS" "--baseline" "re-read: the baseline is still passed"

rm -rf "$GATE_WORK"

# --- Branded token outcome (issue #2756) ---
#
# The smoke test in test-action.yml reads the broker outcome to decide which
# comment author to expect. The outcome reaches later steps through
# $GITHUB_ENV, because a composite action exposes only its declared outputs.

echo ""
echo "=== Branded token outcome ==="

BROKER_WORK=$(mktemp -d)
mkdir -p "$BROKER_WORK/bin"
cat > "$BROKER_WORK/bin/curl" <<'CURL'
#!/usr/bin/env bash
case "$*" in *"--data @-"*) cat > /dev/null ;; esac
case "$*" in
  *"/v1/ci/github-token"*)
    [ "${FAKE_BROKER:-ok}" = "ok" ] || exit 28
    printf '%s' '{"data":{"token":"branded-token"}}' ;;
  *) printf '%s' '{"value":"oidc-token"}' ;;
esac
CURL
chmod +x "$BROKER_WORK/bin/curl"

run_broker() {
  BROKER_OUTPUT="$BROKER_WORK/output"
  BROKER_ENV="$BROKER_WORK/env"
  : > "$BROKER_OUTPUT"
  : > "$BROKER_ENV"
  BROKER_STDERR=$(
    PATH="$BROKER_WORK/bin:$PATH" \
      GITHUB_OUTPUT="$BROKER_OUTPUT" \
      GITHUB_ENV="$BROKER_ENV" \
      ACTIONS_ID_TOKEN_REQUEST_URL="https://token.example/?x=1" \
      ACTIONS_ID_TOKEN_REQUEST_TOKEN="request-token" \
      env "$@" bash "$SCRIPTS_DIR/broker-token.sh" 2>&1 > /dev/null
  )
  BROKER_EXIT=$?
}

run_broker FAKE_BROKER="ok"
assert_contains "$(cat "$BROKER_OUTPUT")" "branded=true" "broker: a minted token sets the step output"
assert_contains "$(cat "$BROKER_ENV")" "FALLOW_TOKEN_BRANDED=true" \
  "broker: a minted token reaches later steps"
# The reason line is always written, so a branded run clears the cause that an
# earlier run of the action in the same job left behind.
if grep -qx "FALLOW_TOKEN_FALLBACK_REASON=" "$BROKER_ENV"; then
  pass "broker: a minted token clears the fallback cause"
else
  fail "broker: a minted token clears the fallback cause" "no empty FALLOW_TOKEN_FALLBACK_REASON line"
fi

run_broker FAKE_BROKER="timeout"
if [ "$BROKER_EXIT" -eq 0 ]; then
  pass "broker: a broker timeout does not fail the step"
else
  fail "broker: a broker timeout does not fail the step" "exit ${BROKER_EXIT}"
fi
assert_contains "$(cat "$BROKER_OUTPUT")" "branded=false" "broker: a timeout sets the step output"
assert_contains "$(cat "$BROKER_ENV")" "FALLOW_TOKEN_BRANDED=false" \
  "broker: a timeout reaches later steps"
assert_contains "$(cat "$BROKER_ENV")" "FALLOW_TOKEN_FALLBACK_REASON=broker unavailable or declined" \
  "broker: a timeout records the fallback cause"

run_broker BRANDED_TOKEN="false"
assert_contains "$(cat "$BROKER_ENV")" "FALLOW_TOKEN_FALLBACK_REASON=branded token disabled" \
  "broker: an opt-out records the fallback cause"

rm -rf "$BROKER_WORK"

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
