#!/usr/bin/env bash
set -eo pipefail

# Post or update a PR comment with analysis results
# Required env: GH_TOKEN, PR_NUMBER, GH_REPO, FALLOW_COMMAND, ACTION_JQ_DIR
# Optional env: CHANGED_SINCE, INPUT_ROOT (for scoping results to changed files)

# Select jq script
case "$FALLOW_COMMAND" in
  dead-code|check) JQ_FILE="${ACTION_JQ_DIR}/summary-check.jq" ;;
  dupes)           JQ_FILE="${ACTION_JQ_DIR}/summary-dupes.jq" ;;
  health)          JQ_FILE="${ACTION_JQ_DIR}/summary-health.jq" ;;
  fix)             JQ_FILE="${ACTION_JQ_DIR}/summary-fix.jq" ;;
  audit|"")        JQ_FILE="${ACTION_JQ_DIR}/summary-combined.jq" ;;
  *)               echo "::error::Unexpected command: ${FALLOW_COMMAND}"; exit 2 ;;
esac

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

# Generate comment body
BODY=$(jq -r -f "$JQ_FILE" "$RESULTS_FILE") || { echo "::warning::Failed to generate PR comment body"; exit 0; }

# Add scoping indicator when results were filtered to changed files
if [ "$RESULTS_FILE" != "fallow-results.json" ]; then
  COMMIT_URL="${GITHUB_SERVER_URL:-https://github.com}/${GH_REPO}/commit/${CHANGED_SINCE}"
  BODY="${BODY}"$'\n\n'"*Issue counts scoped to files changed since [\`${CHANGED_SINCE:0:7}\`](${COMMIT_URL}) · health metrics reflect the full codebase*"
fi

COMMENT_BODY="${BODY}

<!-- fallow-results -->"

# Find existing fallow comment to update (avoids spam on busy PRs)
COMMENT_ID=$(gh api \
  --paginate \
  "repos/${GH_REPO}/issues/${PR_NUMBER}/comments" \
  --jq '.[] | select(.body | contains("<!-- fallow-results -->")) | .id' \
  2>/dev/null | head -1)

if [ -n "$COMMENT_ID" ]; then
  if ! gh api \
    "repos/${GH_REPO}/issues/comments/${COMMENT_ID}" \
    --method PATCH \
    --field body="$COMMENT_BODY" \
    > /dev/null; then
    echo "::warning::Failed to update PR comment"
  else
    echo "Updated existing PR comment"
  fi
else
  if ! gh api \
    "repos/${GH_REPO}/issues/${PR_NUMBER}/comments" \
    --method POST \
    --field body="$COMMENT_BODY" \
    > /dev/null; then
    echo "::warning::Failed to create PR comment"
  else
    echo "Created new PR comment"
  fi
fi
