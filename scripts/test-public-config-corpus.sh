#!/usr/bin/env bash
#
# Offline regression test for scripts/public-config-corpus.py.

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

mkdir -p "$TMP_DIR/cache"
cp -R "$ROOT/scripts/fixtures/public-config-corpus/cache/." "$TMP_DIR/cache/"

python3 "$ROOT/scripts/public-config-corpus.py" \
  --from-search-fixture "$ROOT/scripts/fixtures/public-config-corpus/search.json" \
  --cache-dir "$TMP_DIR/cache" \
  --output "$TMP_DIR/report.md" \
  --manifest "$TMP_DIR/manifest.json" \
  --fetched-at "2026-05-27T00:00:00Z" \
  --gh-version "gh version fixture"

if python3 "$ROOT/scripts/public-config-corpus.py" --search-timeout 0 >/dev/null 2>&1; then
  echo "expected --search-timeout 0 to fail" >&2
  exit 1
fi

python3 - "$TMP_DIR/manifest.json" "$TMP_DIR/report.md" <<'PY'
import json
import sys
from pathlib import Path

manifest = json.loads(Path(sys.argv[1]).read_text())
report = Path(sys.argv[2]).read_text()
entries = manifest["entries"]

assert manifest["generated_at"] == "2026-05-27T00:00:00Z"
assert manifest["gh_version"] == "gh version fixture"
assert len(entries) == 4

alpha = next(entry for entry in entries if entry.get("repo") == "alpha/app")
assert alpha["parse_status"] == "ok"
assert alpha["bytes"] > 0
assert len(alpha["sha256"]) == 64
assert alpha["keys"] == ["entry", "ignoreDependencies", "rules", "audit"]
assert alpha["comment_hits"][0]["phrase"] == "framework"

beta = next(entry for entry in entries if entry.get("repo") == "beta/tool")
assert beta["parse_status"] == "ok"
assert beta["keys"] == ["entry", "dynamicallyLoaded", "ignorePatterns"]

bad = next(entry for entry in entries if entry.get("repo") == "bad/invalid")
assert bad["parse_status"].startswith("parse-error:")
assert bad["comment_hits"][0]["phrase"] == "fallow misses"

missing = next(entry for entry in entries if entry.get("repo") == "missing/cache")
assert missing["parse_status"] == "not-fetched"
assert "cache miss in offline mode" in missing["fetch_error"]

assert "Fetched configs: 3" in report
assert "Fetch failures: 1" in report
assert "Parse failures: 1" in report
assert "Candidate workaround comments: 3" in report
assert "Review `entry` usage across 2 config(s)." in report
assert "cache miss in offline mode" in report
assert "Comment hits are candidate evidence only" in report
PY

echo "public config corpus fixture test passed"
