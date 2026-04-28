#!/usr/bin/env bash
set -eo pipefail

# Post or update an MR comment with analysis results
# Required env: GITLAB_TOKEN, CI_API_V4_URL, CI_PROJECT_ID,
#   CI_MERGE_REQUEST_IID, FALLOW_COMMAND, FALLOW_JQ_DIR
# Optional env: CHANGED_SINCE, INPUT_ROOT (for scoping results to changed files)

# Auth header
if [ -z "${GITLAB_TOKEN:-}" ]; then
  echo "WARNING: GITLAB_TOKEN is required to create or update MR comments; CI_JOB_TOKEN is read-only for MR notes in the official GitLab API. Skipping MR summary comment."
  exit 0
fi
AUTH_HEADER="PRIVATE-TOKEN: ${GITLAB_TOKEN}"

API_URL="${CI_API_V4_URL}/projects/${CI_PROJECT_ID}/merge_requests/${CI_MERGE_REQUEST_IID}/notes"

# Select jq script — prefer GitLab-specific variants, fall back to shared
pick_jq() {
  local name="$1"
  if [ -f "${FALLOW_JQ_DIR}/${name}" ]; then
    echo "${FALLOW_JQ_DIR}/${name}"
  elif [ -f "${FALLOW_SHARED_JQ_DIR:-}/${name}" ]; then
    echo "${FALLOW_SHARED_JQ_DIR}/${name}"
  else
    echo "${FALLOW_JQ_DIR}/${name}"
  fi
}

case "$FALLOW_COMMAND" in
  dead-code|check) JQ_FILE=$(pick_jq "summary-check.jq") ;;
  dupes)           JQ_FILE=$(pick_jq "summary-dupes.jq") ;;
  health)          JQ_FILE=$(pick_jq "summary-health.jq") ;;
  fix)             JQ_FILE=$(pick_jq "summary-fix.jq") ;;
  "")              JQ_FILE=$(pick_jq "summary-combined.jq") ;;
  *)               echo "ERROR: Unexpected command: ${FALLOW_COMMAND}"; exit 2 ;;
esac

# Scope results to changed files when --changed-since is active
RESULTS_BASE="fallow-results.json"
if [ -n "${CHANGED_SINCE:-}" ]; then
  ROOT="${INPUT_ROOT:-.}"
  FILTER_JQ=$(pick_jq "filter-changed.jq")
  if [ -f "$FILTER_JQ" ]; then
    CHANGED_FILES=$(cd "$ROOT" && git diff --name-only --relative "${CHANGED_SINCE}...HEAD" -- . 2>/dev/null || true)
    if [ -n "$CHANGED_FILES" ]; then
      CHANGED_JSON=$(echo "$CHANGED_FILES" | jq -R -s 'split("\n") | map(select(length > 0))')
      if jq --argjson changed "$CHANGED_JSON" -f "$FILTER_JQ" fallow-results.json > fallow-results-scoped.json 2>/dev/null; then
        RESULTS_BASE="fallow-results-scoped.json"
      fi
    fi
  fi
fi

# For combined mode, pass the full JSON; for specific commands, extract section
INPUT_FILE="$RESULTS_BASE"
if [ -z "$FALLOW_COMMAND" ]; then
  INPUT_FILE="$RESULTS_BASE"
elif [ "$FALLOW_COMMAND" = "dead-code" ] || [ "$FALLOW_COMMAND" = "check" ]; then
  # If running in combined mode but requesting check summary
  if jq -e '.check' "$RESULTS_BASE" > /dev/null 2>&1; then
    jq '.check' "$RESULTS_BASE" > /tmp/fallow-comment-input.json
    INPUT_FILE="/tmp/fallow-comment-input.json"
  fi
fi

# Generate comment body
BODY=$(jq -r -f "$JQ_FILE" "$INPUT_FILE") || { echo "WARNING: Failed to generate MR comment body"; exit 0; }

# Add scoping indicator when results were filtered to changed files
if [ "$RESULTS_BASE" != "fallow-results.json" ]; then
  COMMIT_URL="${CI_PROJECT_URL:-}/-/commit/${CHANGED_SINCE}"
  BODY="${BODY}"$'\n\n'"*Issue counts scoped to files changed since [\`${CHANGED_SINCE:0:7}\`](${COMMIT_URL}) · health metrics reflect the full codebase*"
fi

COMMENT_BODY="${BODY}

<!-- fallow-results -->"

# Find existing fallow comment to update (avoids spam on busy MRs)
EXISTING_NOTE_ID=$(curl -sf \
  --header "${AUTH_HEADER}" \
  "${API_URL}?per_page=100" \
  | jq -r '.[] | select(.body | contains("<!-- fallow-results -->")) | .id' \
  | head -1) || true

if [ -n "$EXISTING_NOTE_ID" ]; then
  curl -sf \
    --header "${AUTH_HEADER}" \
    --header "Content-Type: application/json" \
    --request PUT \
    --data "$(jq -n --arg body "$COMMENT_BODY" '{body: $body}')" \
    "${API_URL}/${EXISTING_NOTE_ID}" > /dev/null \
    && echo "Updated existing MR comment" \
    || echo "WARNING: Failed to update MR comment (check token permissions)"
else
  curl -sf \
    --header "${AUTH_HEADER}" \
    --header "Content-Type: application/json" \
    --request POST \
    --data "$(jq -n --arg body "$COMMENT_BODY" '{body: $body}')" \
    "${API_URL}" > /dev/null \
    && echo "Created new MR comment" \
    || echo "WARNING: Failed to create MR comment (check token permissions)"
fi
