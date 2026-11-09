#!/bin/sh
# Run oxlint and oxfmt --check only when the commit touches a JS or TS file
# inside one of the lintable scopes, so that a pure Rust commit stays fast.
# The `lint:js` script in package.json holds the scope list. The helper reads
# it from there, and fails when the format scripts hold a different list.
#
# With --all, the hook checks every file in the scopes. The pre-push hook uses
# it, because a rebase applies resolved conflicts without the pre-commit hook.
set -e

if ! command -v node >/dev/null 2>&1; then
  exit 0
fi

if [ "$1" != "--all" ]; then
  changed=$(git diff --cached --name-only --diff-filter=ACMR | node scripts/js-lint-scopes.mjs --filter-staged)
  if [ -z "$changed" ]; then
    exit 0
  fi
fi

if [ ! -d node_modules ]; then
  echo "js-lint: node_modules is missing; skipping oxlint and oxfmt (CI still enforces them). Run: pnpm install" >&2
  exit 0
fi

npm run --silent lint:js
npm run --silent fmt:js:check
