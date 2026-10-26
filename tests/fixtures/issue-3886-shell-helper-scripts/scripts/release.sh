#!/usr/bin/env bash
exec-cli publish "$@"
cat <<'NOTES'
unused-cli inside a heredoc
NOTES
cd "$(dirname "$0")/.."
tsx src/release-entry.ts
