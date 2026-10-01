#!/bin/sh
# Fail when a line starts with a git merge-conflict marker. A committed marker
# is an unresolved conflict, or test data that trips conflict scanners in
# every later checkout; build test markers at runtime instead.
# `=======` alone is not checked, because Markdown and reStructuredText use it.
#
# Usage: sh scripts/check-conflict-markers.sh [--staged]
#   --staged  check the lines that the index adds (pre-commit hook)
#   default   check every tracked file (CI)
pattern='^(<{7}|>{7}|\|{7})( |$)'

if [ "${1:-}" = "--staged" ]; then
  hits=$(git diff --cached -U0 --no-color --no-ext-diff --diff-filter=ACMR \
    | grep -E '^(\+\+\+ |\+)' \
    | awk '/^\+\+\+ /{file=substr($0, 7); next} {print file ": " substr($0, 2)}' \
    | grep -E ": (<{7}|>{7}|\|{7})( |$)")
else
  hits=$(git grep -n -I -E "$pattern" -- . 2>/dev/null)
fi

if [ -n "$hits" ]; then
  printf '%s\n' "$hits" >&2
  echo "A line starts with a merge-conflict marker. Resolve the conflict, or build test markers at runtime (for example \"<\".repeat(7))." >&2
  exit 1
fi
