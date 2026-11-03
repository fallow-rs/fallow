#!/bin/sh
# Stop a .mjs or .ts file over 2000 lines from growing. `fallow audit` compares
# the working tree with a base ref and fails on a long file that is new or that
# grew by 5 lines or more. hk.pkl and scripts/verify-repo.mjs call this script.
# See docs/development/quality-gates.md#file-size.
#
# Usage: sh scripts/hook-file-length.sh <base-ref>...
# The script uses the first ref that exists. When no ref exists, or when no
# fallow binary exists, it prints a note and skips, because CI still checks it.
set -e

# hk passes GIT_DIR and GIT_WORK_TREE to each step, and git passes
# GIT_INDEX_FILE to a pre-commit hook. fallow audit runs git, so clear them.
unset GIT_DIR GIT_WORK_TREE GIT_INDEX_FILE

# Prefer the binary of this checkout. An installed fallow is the fallback.
fallow="${CARGO_TARGET_DIR:-target}/debug/fallow"
if [ ! -x "$fallow" ]; then
  if command -v fallow >/dev/null 2>&1; then
    fallow=fallow
  else
    echo "file-length: no fallow binary; skipping the .mjs and .ts file-length check (CI still checks it). Run: cargo build --bin fallow" >&2
    exit 0
  fi
fi

for base in "$@"; do
  if git rev-parse --verify --quiet "${base}^{commit}" >/dev/null 2>&1; then
    exec "$fallow" audit --config scripts/fallow-file-length.json --base "$base" --no-css --quiet
  fi
done

echo "file-length: no ref of '$*' exists; skipping the .mjs and .ts file-length check (CI still checks it)." >&2
exit 0
