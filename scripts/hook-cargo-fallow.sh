#!/bin/sh
# Run the command of the "Rust Structure" CI job before a push. The CI job
# compares the branch with its base, so `cargo fallow check` also enforces the
# file-growth gate: a .rs file over 2000 lines must not grow. A rebase commits
# resolved conflicts without the pre-commit hook, so this pre-push check sees a
# long file that the rebase made grow. hk.pkl calls this script.
# See docs/development/quality-gates.md#rust-structure.
#
# Usage: sh scripts/hook-cargo-fallow.sh <base-ref>...
# The script uses the first ref that exists. When no ref exists, it runs the
# check without the file-growth gate, because CI still checks it.
set -e

# hk passes GIT_DIR and GIT_WORK_TREE to each step. cargo fallow runs git, so
# clear them.
unset GIT_DIR GIT_WORK_TREE GIT_INDEX_FILE

if ! command -v cargo-fallow >/dev/null 2>&1; then
  exec sh scripts/hook-tool-hint.sh cargo-fallow
fi

# `--base` compares each file with its version at the merge base of HEAD and
# the ref, so the script passes the ref as it is.
for base in "$@"; do
  if git rev-parse --verify --quiet "${base}^{commit}" >/dev/null 2>&1; then
    exec cargo fallow check --baseline cargo-fallow-baseline.json --fail-on-issues --quiet --base "$base"
  fi
done

echo "cargo-fallow: no ref of '$*' exists; the file-growth gate does not run (CI still checks it)." >&2
exec cargo fallow check --baseline cargo-fallow-baseline.json --fail-on-issues --quiet
