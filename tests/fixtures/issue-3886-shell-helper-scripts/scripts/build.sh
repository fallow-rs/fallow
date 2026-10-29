#!/usr/bin/env bash
set -euo pipefail
# unused-cli --help
report-cli --format json
bash scripts/nested.sh
if [ -n "${CI:-}" ]; then
  tsx src/generate.ts
fi
