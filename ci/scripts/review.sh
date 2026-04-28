#!/usr/bin/env bash
set -eo pipefail

# Post inline MR discussions with rich markdown formatting and suggestion blocks
# Required env: GITLAB_TOKEN, CI_API_V4_URL, CI_PROJECT_ID,
#   CI_MERGE_REQUEST_IID, CI_COMMIT_SHA, CI_MERGE_REQUEST_DIFF_BASE_SHA,
#   FALLOW_COMMAND, FALLOW_ROOT, MAX_COMMENTS, FALLOW_JQ_DIR

MAX="${MAX_COMMENTS:-50}"
if ! [[ "$MAX" =~ ^[0-9]+$ ]]; then
  echo "WARNING: max-comments must be a positive integer, got: ${MAX_COMMENTS}. Using default: 50"
  MAX=50
fi

# Reject path traversal in root
if [[ "${FALLOW_ROOT:-}" =~ \.\. ]]; then
  echo "ERROR: root input contains path traversal sequence"
  exit 2
fi

# Auth header
if [ -z "${GITLAB_TOKEN:-}" ]; then
  echo "WARNING: GITLAB_TOKEN is required to create/delete MR discussions; CI_JOB_TOKEN is read-only for MR notes in the official GitLab API. Skipping inline MR review."
  exit 0
fi
AUTH_HEADER="PRIVATE-TOKEN: ${GITLAB_TOKEN}"

NOTES_URL="${CI_API_V4_URL}/projects/${CI_PROJECT_ID}/merge_requests/${CI_MERGE_REQUEST_IID}/notes"
DISCUSSIONS_URL="${CI_API_V4_URL}/projects/${CI_PROJECT_ID}/merge_requests/${CI_MERGE_REQUEST_IID}/discussions"

# --- Cleanup previous fallow comments and discussions ---

echo "Cleaning up previous fallow comments..."

# Delete previous fallow review body (notes with <!-- fallow-review --> marker)
while IFS= read -r NOTE_ID; do
  [ -z "$NOTE_ID" ] && continue
  curl -sf \
    --header "${AUTH_HEADER}" \
    --request DELETE \
    "${NOTES_URL}/${NOTE_ID}" > /dev/null 2>&1 || true
done < <(curl -sf \
  --header "${AUTH_HEADER}" \
  "${NOTES_URL}?per_page=100" \
  | jq -r '.[] | select(.body | contains("<!-- fallow-review -->")) | .id' 2>/dev/null)

# Delete previous fallow inline discussions (discussions with docs.fallow.tools links)
while IFS= read -r DISC_ID; do
  [ -z "$DISC_ID" ] && continue
  # Get the first note ID to delete the discussion
  NOTE_ID=$(curl -sf \
    --header "${AUTH_HEADER}" \
    "${DISCUSSIONS_URL}/${DISC_ID}" \
    | jq -r '.notes[0].id' 2>/dev/null) || continue
  [ -z "$NOTE_ID" ] || [ "$NOTE_ID" = "null" ] && continue
  curl -sf \
    --header "${AUTH_HEADER}" \
    --request DELETE \
    "${NOTES_URL}/${NOTE_ID}" > /dev/null 2>&1 || true
done < <(curl -sf \
  --header "${AUTH_HEADER}" \
  "${DISCUSSIONS_URL}?per_page=100" \
  | jq -r '.[] | select(.notes[0].body | contains("docs.fallow.tools")) | .id' 2>/dev/null)

echo "Cleanup complete"

# --- Prefix for paths ---

PREFIX=""
if [ "$FALLOW_ROOT" != "." ]; then
  PREFIX="${FALLOW_ROOT}/"
fi

# --- Select jq scripts ---

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

# Detect package manager from lock files
_ROOT="${FALLOW_ROOT:-.}"
PKG_MANAGER="npm"
if [ -f "${_ROOT}/pnpm-lock.yaml" ] || [ -f "pnpm-lock.yaml" ]; then
  PKG_MANAGER="pnpm"
elif [ -f "${_ROOT}/yarn.lock" ] || [ -f "yarn.lock" ]; then
  PKG_MANAGER="yarn"
fi

# Export env vars for jq access
export PREFIX MAX FALLOW_ROOT CI_PROJECT_URL CI_COMMIT_SHA PKG_MANAGER

# --- Scope results to changed files ---

RESULTS_FILE="fallow-results.json"
if [ -n "${CHANGED_SINCE:-}" ]; then
  ROOT="${FALLOW_ROOT:-.}"
  FILTER_JQ=$(pick_jq "filter-changed.jq")
  if [ -f "$FILTER_JQ" ]; then
    CHANGED_FILES=$(cd "$ROOT" && git diff --name-only --relative "${CHANGED_SINCE}...HEAD" -- . 2>/dev/null || true)
    if [ -n "$CHANGED_FILES" ]; then
      CHANGED_JSON=$(echo "$CHANGED_FILES" | jq -R -s 'split("\n") | map(select(length > 0))')
      if jq --argjson changed "$CHANGED_JSON" -f "$FILTER_JQ" fallow-results.json > fallow-results-scoped.json 2>/dev/null; then
        RESULTS_FILE="fallow-results-scoped.json"
      fi
    fi
  fi
fi

# --- Collect review comments ---

COMMENTS="[]"
case "$FALLOW_COMMAND" in
  dead-code|check)
    COMMENTS=$(jq -f "$(pick_jq review-comments-check.jq)" "$RESULTS_FILE" 2>&1) || { echo "jq check error: $COMMENTS"; COMMENTS="[]"; } ;;
  dupes)
    COMMENTS=$(jq -f "$(pick_jq review-comments-dupes.jq)" "$RESULTS_FILE" 2>&1) || { echo "jq dupes error: $COMMENTS"; COMMENTS="[]"; } ;;
  health)
    COMMENTS=$(jq -f "$(pick_jq review-comments-health.jq)" "$RESULTS_FILE" 2>&1) || { echo "jq health error: $COMMENTS"; COMMENTS="[]"; } ;;
  "")
    # Combined: extract each section and run through its jq script
    WORK_DIR=$(mktemp -d)
    jq '.check // {}' "$RESULTS_FILE" > "$WORK_DIR/check.json" 2>/dev/null
    jq '.dupes // {}' "$RESULTS_FILE" > "$WORK_DIR/dupes.json" 2>/dev/null
    jq '.health // {}' "$RESULTS_FILE" > "$WORK_DIR/health.json" 2>/dev/null
    CHECK=$(jq -f "$(pick_jq review-comments-check.jq)" "$WORK_DIR/check.json" 2>/dev/null || echo "[]")
    DUPES=$(jq -f "$(pick_jq review-comments-dupes.jq)" "$WORK_DIR/dupes.json" 2>/dev/null || echo "[]")
    HEALTH=$(jq -f "$(pick_jq review-comments-health.jq)" "$WORK_DIR/health.json" 2>/dev/null || echo "[]")
    COMMENTS=$(jq -n \
      --argjson a "$CHECK" --argjson b "$DUPES" --argjson c "$HEALTH" \
      --argjson max "$MAX" \
      '$a + $b + $c | .[:$max]')
    rm -rf "$WORK_DIR" ;;
esac

# --- Post-process: group, dedup, merge ---

MERGE_JQ=$(pick_jq merge-comments.jq)
MERGED=$(echo "$COMMENTS" | jq --argjson max "$MAX" -f "$MERGE_JQ" 2>&1) && COMMENTS="$MERGED" || echo "Merge warning: $MERGED"

# --- Add suggestion blocks for unused exports ---

ENRICHED=$(echo "$COMMENTS" | jq -c '.[]' | while IFS= read -r comment; do
  TYPE=$(echo "$comment" | jq -r '.type // ""')
  if [ "$TYPE" = "unused-export" ]; then
    FILE_PATH=$(echo "$comment" | jq -r '.path')
    LINE_NUM=$(echo "$comment" | jq -r '.line')
    if [ -f "$FILE_PATH" ] && [ "$LINE_NUM" -gt 0 ] 2>/dev/null; then
      SOURCE_LINE=$(sed -n "${LINE_NUM}p" "$FILE_PATH")
      if [ -n "$SOURCE_LINE" ]; then
        # Strip "export " or "export default " from the line
        FIXED_LINE=$(echo "$SOURCE_LINE" | sed 's/^export default //' | sed 's/^export //')
        if [ "$FIXED_LINE" != "$SOURCE_LINE" ]; then
          SUGGESTION=$'\n\n```suggestion:-0+0\n'"${FIXED_LINE}"$'\n```'
          echo "$comment" | jq --arg sug "$SUGGESTION" '.body = .body + $sug'
          continue
        fi
      fi
    fi
  fi
  echo "$comment"
done | jq -s '.')
if [ -n "$ENRICHED" ] && echo "$ENRICHED" | jq -e '.' > /dev/null 2>&1; then
  COMMENTS="$ENRICHED"
fi

TOTAL=$(echo "$COMMENTS" | jq 'length')
if [ "$TOTAL" -eq 0 ]; then
  echo "No review comments to post"
  exit 0
fi

echo "Posting $TOTAL review comments (after merging)..."

# --- Post review body as MR note ---

REVIEW_BODY=""
REVIEW_BODY_JQ=$(pick_jq review-body.jq)
if [ -f "$REVIEW_BODY_JQ" ]; then
  REVIEW_BODY=$(jq -r -f "$REVIEW_BODY_JQ" "$RESULTS_FILE" 2>&1) || true
fi
if [ -z "$REVIEW_BODY" ] || echo "$REVIEW_BODY" | grep -q "^jq:"; then
  REVIEW_BODY="## :seedling: Fallow Review

Found **${TOTAL}** issues — see inline comments below.

<!-- fallow-review -->"
fi

# Add scoping indicator when results were filtered to changed files
if [ "$RESULTS_FILE" != "fallow-results.json" ]; then
  COMMIT_URL="${CI_PROJECT_URL:-}/-/commit/${CHANGED_SINCE}"
  REVIEW_BODY="${REVIEW_BODY}"$'\n\n'"*Issue counts scoped to files changed since [\`${CHANGED_SINCE:0:7}\`](${COMMIT_URL}) · health metrics reflect the full codebase*"
fi

curl -sf \
  --header "${AUTH_HEADER}" \
  --header "Content-Type: application/json" \
  --request POST \
  --data "$(jq -n --arg body "$REVIEW_BODY" '{body: $body}')" \
  "${NOTES_URL}" > /dev/null 2>&1 \
  && echo "Posted review body" \
  || echo "WARNING: Failed to post review body"

# --- Fetch diff_refs from MR API (more reliable than CI env vars) ---

DIFF_REFS=$(curl -sf \
  --header "${AUTH_HEADER}" \
  "${CI_API_V4_URL}/projects/${CI_PROJECT_ID}/merge_requests/${CI_MERGE_REQUEST_IID}" \
  | jq -r '.diff_refs // empty')

if [ -n "$DIFF_REFS" ] && echo "$DIFF_REFS" | jq -e '.base_sha' > /dev/null 2>&1; then
  BASE_SHA=$(echo "$DIFF_REFS" | jq -r '.base_sha')
  START_SHA=$(echo "$DIFF_REFS" | jq -r '.start_sha')
  HEAD_SHA=$(echo "$DIFF_REFS" | jq -r '.head_sha')
  echo "Using diff_refs from MR API (base: ${BASE_SHA:0:12}, start: ${START_SHA:0:12}, head: ${HEAD_SHA:0:12})"
else
  # Fallback to CI env vars
  BASE_SHA="${CI_MERGE_REQUEST_DIFF_BASE_SHA:-}"
  START_SHA="$BASE_SHA"
  HEAD_SHA="${CI_COMMIT_SHA:-}"
  echo "Using CI env vars for SHAs (diff_refs not available)"
fi

POSTED=0
SKIPPED=0

while IFS= read -r comment; do
  [ -z "$comment" ] && continue
  PATH_VAL=$(echo "$comment" | jq -r '.path')
  LINE_VAL=$(echo "$comment" | jq -r '.line')
  BODY_VAL=$(echo "$comment" | jq -r '.body')

  if [ -n "$BASE_SHA" ] && [ -n "$HEAD_SHA" ]; then
    # Post as inline discussion with position
    PAYLOAD=$(jq -n \
      --arg body "$BODY_VAL" \
      --arg base "$BASE_SHA" \
      --arg start "$START_SHA" \
      --arg head "$HEAD_SHA" \
      --arg path "$PATH_VAL" \
      --argjson line "$LINE_VAL" \
      '{
        body: $body,
        position: {
          base_sha: $base,
          start_sha: $start,
          head_sha: $head,
          position_type: "text",
          old_path: $path,
          new_path: $path,
          new_line: $line
        }
      }')

    if curl -sf \
      --header "${AUTH_HEADER}" \
      --header "Content-Type: application/json" \
      --request POST \
      --data "$PAYLOAD" \
      "${DISCUSSIONS_URL}" > /dev/null 2>&1; then
      POSTED=$((POSTED + 1))
    else
      # Fallback: post as regular note if inline fails (line not in diff)
      # Strip suggestion blocks — they only render in positioned discussions
      CLEAN_BODY=$(echo "$BODY_VAL" | sed '/^```suggestion/,/^```$/d')
      FALLBACK_BODY=$(printf ":warning: **%s:%s**\n\n%s" "$PATH_VAL" "$LINE_VAL" "$CLEAN_BODY")
      if curl -sf \
        --header "${AUTH_HEADER}" \
        --header "Content-Type: application/json" \
        --request POST \
        --data "$(jq -n --arg body "$FALLBACK_BODY" '{body: $body}')" \
        "${NOTES_URL}" > /dev/null 2>&1; then
        POSTED=$((POSTED + 1))
      else
        SKIPPED=$((SKIPPED + 1))
      fi
    fi
  else
    # No SHAs available: post as regular note with file reference
    # Strip suggestion blocks — they only render in positioned discussions
    CLEAN_BODY=$(echo "$BODY_VAL" | sed '/^```suggestion/,/^```$/d')
    FALLBACK_BODY=$(printf ":warning: **%s:%s**\n\n%s" "$PATH_VAL" "$LINE_VAL" "$CLEAN_BODY")
    if curl -sf \
      --header "${AUTH_HEADER}" \
      --header "Content-Type: application/json" \
      --request POST \
      --data "$(jq -n --arg body "$FALLBACK_BODY" '{body: $body}')" \
      "${NOTES_URL}" > /dev/null 2>&1; then
      POSTED=$((POSTED + 1))
    else
      SKIPPED=$((SKIPPED + 1))
    fi
  fi
done < <(echo "$COMMENTS" | jq -c '.[]')

echo "Posted ${POSTED} inline comments, skipped ${SKIPPED}"
