#!/usr/bin/env bash
exec-cli publish "$@"
cat <<'NOTES'
unused-cli inside a heredoc
NOTES
