#!/usr/bin/env bash
set -eo pipefail

# Post review comments with rich markdown formatting
# Required env: GH_TOKEN, PR_NUMBER, GH_REPO, FALLOW_COMMAND, FALLOW_ROOT,
#   MAX_COMMENTS, ACTION_JQ_DIR
# Optional env: CHANGED_SINCE (for scoping results to changed files)

MAX="${MAX_COMMENTS:-50}"
if ! [[ "$MAX" =~ ^[0-9]+$ ]]; then
  echo "::warning::max-annotations must be a positive integer, got: ${MAX_COMMENTS}. Using default: 50"
  MAX=50
fi

# Reject path traversal in root
if [[ "${FALLOW_ROOT:-}" =~ \.\. ]]; then
  echo "::error::root input contains path traversal sequence"
  exit 2
fi

# Clean up ALL previous review comments from github-actions[bot]
while read -r CID; do
  gh api "repos/${GH_REPO}/pulls/comments/${CID}" --method DELETE > /dev/null 2>&1 || true
done < <(gh api "repos/${GH_REPO}/pulls/${PR_NUMBER}/comments" --paginate \
  --jq '.[] | select(.user.login == "github-actions[bot]") | .id' 2>/dev/null)

# Dismiss previous fallow reviews
gh api "repos/${GH_REPO}/pulls/${PR_NUMBER}/reviews" --paginate \
  --jq '.[] | select(.user.login == "github-actions[bot]" and .state != "DISMISSED") | .id' 2>/dev/null | while read -r RID; do
  gh api "repos/${GH_REPO}/pulls/${PR_NUMBER}/reviews/${RID}" \
    --method PUT --field event=DISMISS \
    --field message="Superseded by new analysis" > /dev/null 2>&1 || true
done

# Clean up body-only fallow comments from previous runs (posted when all findings were outside the diff)
while read -r CID; do
  gh api "repos/${GH_REPO}/issues/comments/${CID}" --method DELETE > /dev/null 2>&1 || true
done < <(gh api "repos/${GH_REPO}/issues/${PR_NUMBER}/comments" --paginate \
  --jq '.[] | select(.user.login == "github-actions[bot]" and (.body | contains("fallow-review"))) | .id' 2>/dev/null)

# Prefix for paths: if root is not ".", prepend it
PREFIX=""
if [ "$FALLOW_ROOT" != "." ]; then
  PREFIX="${FALLOW_ROOT}/"
fi

# Detect package manager from lock files
_ROOT="${FALLOW_ROOT:-.}"
PKG_MANAGER="npm"
if [ -f "${_ROOT}/pnpm-lock.yaml" ] || [ -f "pnpm-lock.yaml" ]; then
  PKG_MANAGER="pnpm"
elif [ -f "${_ROOT}/yarn.lock" ] || [ -f "yarn.lock" ]; then
  PKG_MANAGER="yarn"
fi

# Export env vars for jq access
export PREFIX MAX FALLOW_ROOT GH_REPO PR_NUMBER PR_HEAD_SHA PKG_MANAGER

# Scope results to changed files when --changed-since is active
RESULTS_FILE="fallow-results.json"
if [ -n "${CHANGED_SINCE:-}" ]; then
  CHANGED_JSON=""

  # Prefer pre-computed list from analyze step (handles shallow clones via API fallback)
  if [ -f fallow-changed-files.json ]; then
    CHANGED_JSON=$(cat fallow-changed-files.json)
  else
    # Fallback: compute locally (for standalone usage outside the action)
    ROOT="${FALLOW_ROOT:-.}"
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

# Collect all review comments from the results
COMMENTS="[]"
case "$FALLOW_COMMAND" in
  dead-code|check)
    COMMENTS=$(jq -f "${ACTION_JQ_DIR}/review-comments-check.jq" "$RESULTS_FILE" 2>&1) || { echo "jq check error: $COMMENTS"; COMMENTS="[]"; } ;;
  dupes)
    COMMENTS=$(jq -f "${ACTION_JQ_DIR}/review-comments-dupes.jq" "$RESULTS_FILE" 2>&1) || { echo "jq dupes error: $COMMENTS"; COMMENTS="[]"; } ;;
  health)
    COMMENTS=$(jq -f "${ACTION_JQ_DIR}/review-comments-health.jq" "$RESULTS_FILE" 2>&1) || { echo "jq health error: $COMMENTS"; COMMENTS="[]"; } ;;
  audit|"")
    # Audit JSON is re-keyed in analyze.sh to use combined-mode keys (check/health/dupes).
    WORK_DIR=$(mktemp -d)
    jq '.check // {}' "$RESULTS_FILE" > "$WORK_DIR/check.json" 2>/dev/null
    jq '.dupes // {}' "$RESULTS_FILE" > "$WORK_DIR/dupes.json" 2>/dev/null
    jq '.health // {}' "$RESULTS_FILE" > "$WORK_DIR/health.json" 2>/dev/null
    CHECK=$(jq -f "${ACTION_JQ_DIR}/review-comments-check.jq" "$WORK_DIR/check.json" 2>/dev/null || echo "[]")
    DUPES=$(jq -f "${ACTION_JQ_DIR}/review-comments-dupes.jq" "$WORK_DIR/dupes.json" 2>/dev/null || echo "[]")
    HEALTH=$(jq -f "${ACTION_JQ_DIR}/review-comments-health.jq" "$WORK_DIR/health.json" 2>/dev/null || echo "[]")
    COMMENTS=$(jq -n \
      --argjson a "$CHECK" --argjson b "$DUPES" --argjson c "$HEALTH" \
      --argjson max "$MAX" \
      '$a + $b + $c | .[:$max]')
    rm -rf "$WORK_DIR" ;;
esac

# Post-process: group unused exports, dedup clones, drop refactoring targets, merge same-line
MERGED=$(echo "$COMMENTS" | jq --argjson max "$MAX" -f "${ACTION_JQ_DIR}/merge-comments.jq" 2>&1) && COMMENTS="$MERGED" || echo "Merge warning: $MERGED"

# Filter comments to only lines within PR diff hunks.
# GitHub's review API rejects comments on lines outside the diff — filtering
# up-front avoids the batch-422-then-retry-one-by-one fallback path entirely.
# Fail-open: if PR files can't be fetched or a file has no patch, keep all its comments.
PRE_FILTER_COUNT=$(echo "$COMMENTS" | jq 'length' 2>/dev/null || echo 0)
PR_FILES_TMP=$(mktemp)
gh api "repos/${GH_REPO}/pulls/${PR_NUMBER}/files?per_page=100" --paginate 2>/dev/null \
  | jq -s 'add // []' > "$PR_FILES_TMP" 2>/dev/null || {
  echo "::warning::Hunk filtering disabled — could not fetch PR files; some comments may be rejected by GitHub"
  echo '[]' > "$PR_FILES_TMP"
}
if jq -e 'length > 0' "$PR_FILES_TMP" > /dev/null 2>&1; then
  # Use --slurpfile to avoid ARG_MAX limits on large PRs (--argjson inlines the
  # entire PR files JSON on the command line, which exceeds the limit for 100+ files).
  # --slurpfile wraps contents in an outer array; the jq script normalizes this.
  FILTERED=$(echo "$COMMENTS" | jq --slurpfile pr_files "$PR_FILES_TMP" -f "${ACTION_JQ_DIR}/filter-diff-hunks.jq" 2>&1) \
    && COMMENTS="$FILTERED" \
    || echo "::warning::Hunk filter failed, posting all comments: $FILTERED"
fi
rm -f "$PR_FILES_TMP"
POST_FILTER_COUNT=$(echo "$COMMENTS" | jq 'length' 2>/dev/null || echo 0)
FILTERED_OUT=$((PRE_FILTER_COUNT - POST_FILTER_COUNT))
if [ "$FILTERED_OUT" -gt 0 ]; then
  echo "Filtered to $POST_FILTER_COUNT of $PRE_FILTER_COUNT comments (${FILTERED_OUT} outside diff hunks)"
fi
export INLINE_COUNT="$POST_FILTER_COUNT" FILTERED_COUNT="$FILTERED_OUT"

# Add suggestion blocks for unused exports by reading source files
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
          SUGGESTION=$'\n\n```suggestion\n'"${FIXED_LINE}"$'\n```'
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
if [ "$TOTAL" -eq 0 ] && [ "${PRE_FILTER_COUNT:-0}" -eq 0 ]; then
  echo "No review comments to post"
  exit 0
fi

if [ "$TOTAL" -eq 0 ]; then
  echo "All ${PRE_FILTER_COUNT} findings are outside the diff — posting summary-only review"
else
  echo "Posting $TOTAL review comments (after merging)..."
fi

# Generate rich review body from the analysis results
REVIEW_BODY=""
if [ -f "${ACTION_JQ_DIR}/review-body.jq" ]; then
  REVIEW_BODY=$(jq -r -f "${ACTION_JQ_DIR}/review-body.jq" "$RESULTS_FILE" 2>&1) || true
fi
# Fallback if jq failed or produced empty output
if [ -z "$REVIEW_BODY" ] || echo "$REVIEW_BODY" | /usr/bin/grep -q "^jq:"; then
  REVIEW_BODY=$'## \xf0\x9f\x8c\xbf Fallow Review\n\nFound **'"$TOTAL"$'** issues \xe2\x80\x94 see inline comments below.\n\n<!-- fallow-review -->'
fi

# Add scoping indicator when results were filtered to changed files
if [ "$RESULTS_FILE" != "fallow-results.json" ]; then
  COMMIT_URL="${GITHUB_SERVER_URL:-https://github.com}/${GH_REPO}/commit/${CHANGED_SINCE}"
  REVIEW_BODY="${REVIEW_BODY}"$'\n\n'"*Issue counts scoped to files changed since [\`${CHANGED_SINCE:0:7}\`](${COMMIT_URL}) · health metrics reflect the full codebase*"
fi

# Post the review
if [ "$TOTAL" -eq 0 ]; then
  # Body-only review: all findings were outside the diff.
  # GitHub rejects COMMENT reviews with an empty comments array,
  # so post a standalone PR comment instead.
  gh api "repos/${GH_REPO}/issues/${PR_NUMBER}/comments" \
    --method POST \
    --field body="$REVIEW_BODY" > /dev/null 2>&1 \
    && echo "Posted summary comment (no inline comments)" \
    || echo "::warning::Failed to post summary comment"
else
  PAYLOAD=$(echo "$COMMENTS" | jq --arg body "$REVIEW_BODY" '{
    event: "COMMENT",
    body: $body,
    comments: [.[] | {path: .path, line: .line, body: .body}]
  }')

  if ! echo "$PAYLOAD" | gh api \
    "repos/${GH_REPO}/pulls/${PR_NUMBER}/reviews" \
    --method POST \
    --input - > /dev/null 2>&1; then
    echo "::warning::Failed to post review comments. Some findings may be on lines not in the PR diff."

    # Fallback: post comments one by one, skipping failures
    POSTED=0
    for i in $(seq 0 $((TOTAL - 1))); do
      SINGLE=$(echo "$COMMENTS" | jq --arg body "$REVIEW_BODY" --argjson first "$POSTED" '{
        event: "COMMENT",
        body: (if $first == 0 then $body else "" end),
        comments: [.['"$i"'] | {path, line, body}]
      }')
      RESULT=$(echo "$SINGLE" | gh api \
        "repos/${GH_REPO}/pulls/${PR_NUMBER}/reviews" \
        --method POST \
        --input - 2>&1) && POSTED=$((POSTED + 1)) || \
        echo "  Skip: $(echo "$COMMENTS" | jq -r ".[${i}].path"):$(echo "$COMMENTS" | jq -r ".[${i}].line")"
    done
    echo "Posted $POSTED of $TOTAL comments individually"
  else
    echo "Posted review with $TOTAL inline comments"
  fi
fi
