#!/bin/sh
# Check that the branch leaves the [Unreleased] section of CHANGELOG.md alone
# and that every changelog.d/ fragment is valid. hk.pkl calls this script.
# The base is origin/main, or upstream/main in a fork clone. A clone without
# either ref skips the check, because CI still runs it.
set -e

for base in origin/main upstream/main; do
  if git rev-parse --verify --quiet "${base}^{commit}" >/dev/null 2>&1; then
    exec node scripts/changelog-fragments.mjs check --base "$base"
  fi
done

echo "pre-commit: no origin/main or upstream/main ref; skipping the changelog fragment check (CI still checks it)." >&2
exit 0
