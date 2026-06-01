#!/usr/bin/env bash
set -euo pipefail

# Decide whether GitHub Code Scanning is available for a SARIF upload and write
# `available=true|false` to $GITHUB_OUTPUT (consumed by the Upload SARIF step's
# `if:` condition in action.yml).
#
# Required env: GH_REPO (owner/repo)
# Optional env: GH_TOKEN (read by gh implicitly), GITHUB_OUTPUT (defaults to stdout)
#
# Public repositories get GitHub Code Scanning for free, without GitHub Advanced
# Security (GHAS). The FIRST SARIF upload is what initializes Code Scanning, so
# probing the code-scanning/alerts endpoint beforehand returns 404 on a public
# repo that has never been set up. Gating on that probe wrongly skips the upload
# on public repos (issue #817). Internal (enterprise) repos report
# `private: false` + `visibility: "internal"` yet still require GHAS, so only
# `visibility == "public"` short-circuits; private and internal repos fall back
# to the alerts probe as a GHAS-availability proxy. If the visibility metadata
# read fails (restricted token, transient error), fall back to the probe too,
# preserving the historical behavior.

GH_REPO="${GH_REPO:?GH_REPO required}"
OUTPUT="${GITHUB_OUTPUT:-/dev/stdout}"

visibility=$(gh api "repos/${GH_REPO}" --jq '.visibility' 2>/dev/null || echo "")

if [ "$visibility" = "public" ]; then
  echo "available=true" >> "$OUTPUT"
else
  if [ -z "$visibility" ]; then
    echo "::debug::could not read repository visibility; falling back to the Code Scanning alerts probe"
  fi
  if gh api "repos/${GH_REPO}/code-scanning/alerts?per_page=1" > /dev/null 2>&1; then
    echo "available=true" >> "$OUTPUT"
  else
    echo "available=false" >> "$OUTPUT"
    echo "::warning::SARIF upload skipped. Code Scanning on a private or internal repository requires GitHub Advanced Security. The job summary and JSON output are still available. To suppress this warning, use format: json instead of sarif."
  fi
fi
