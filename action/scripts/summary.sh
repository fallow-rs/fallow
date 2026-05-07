#!/usr/bin/env bash
set -eo pipefail

# Write job summary using the appropriate jq script
# Required env: FALLOW_COMMAND, ACTION_JQ_DIR
# Optional env: CHANGED_SINCE, INPUT_ROOT (for scoping results to changed files)

select_summary_script() {
  case "$FALLOW_COMMAND" in
    dead-code|check) echo "${ACTION_JQ_DIR}/summary-check.jq" ;;
    dupes)           echo "${ACTION_JQ_DIR}/summary-dupes.jq" ;;
    health)          echo "${ACTION_JQ_DIR}/summary-health.jq" ;;
    fix)             echo "${ACTION_JQ_DIR}/summary-fix.jq" ;;
    audit|"")        echo "${ACTION_JQ_DIR}/summary-combined.jq" ;;
    *)               echo "::error::Unexpected command: ${FALLOW_COMMAND}"; exit 2 ;;
  esac
}

JQ_FILE=$(select_summary_script)
if [ ! -f "$JQ_FILE" ]; then
  echo "::warning::Summary script not found: ${JQ_FILE}"
  exit 0
fi

# Scope results to changed files when --changed-since is active
RESULTS_FILE="fallow-results.json"
if [ -n "${CHANGED_SINCE:-}" ]; then
  CHANGED_JSON=""

  # Prefer pre-computed list from analyze step (handles shallow clones via API fallback)
  if [ -f fallow-changed-files.json ]; then
    CHANGED_JSON=$(cat fallow-changed-files.json)
  else
    # Fallback: compute locally (for standalone usage outside the action)
    ROOT="${INPUT_ROOT:-.}"
    CHANGED_FILES=$(cd "$ROOT" && git diff --name-only --relative "${CHANGED_SINCE}...HEAD" -- . 2>/dev/null || true)
    if [ -n "$CHANGED_FILES" ]; then
      CHANGED_JSON=$(echo "$CHANGED_FILES" | jq -R -s 'split("\n") | map(select(length > 0))')
    fi
  fi

  if [ -n "$CHANGED_JSON" ] && [ "$CHANGED_JSON" != "[]" ]; then
    if jq --argjson changed "$CHANGED_JSON" -f "${ACTION_JQ_DIR}/filter-changed.jq" fallow-results.json > fallow-results-scoped.json 2>/dev/null; then
      RESULTS_FILE="fallow-results-scoped.json"
    fi
  fi
fi

if ! BODY=$(jq -r -f "$JQ_FILE" "$RESULTS_FILE"); then
  echo "::warning::Failed to generate job summary"
  exit 0
fi

# Add scoping indicator when results were filtered to changed files
if [ "$RESULTS_FILE" != "fallow-results.json" ]; then
  COMMIT_URL="${GITHUB_SERVER_URL:-https://github.com}/${GITHUB_REPOSITORY}/commit/${CHANGED_SINCE}"
  BODY="${BODY}"$'\n\n'"*Issue counts scoped to files changed since [\`${CHANGED_SINCE:0:7}\`](${COMMIT_URL}) · health metrics reflect the full codebase*"
fi

echo "$BODY" >> "$GITHUB_STEP_SUMMARY"
