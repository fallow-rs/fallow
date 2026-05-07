#!/usr/bin/env bash
set -eo pipefail

# Emit inline PR annotations via workflow commands
# Required env: FALLOW_COMMAND, MAX_ANNOTATIONS, ACTION_JQ_DIR
# Optional env: CHANGED_SINCE, INPUT_ROOT (for scoping results to changed files)

MAX="${MAX_ANNOTATIONS:-50}"
if ! [[ "$MAX" =~ ^[0-9]+$ ]]; then
  echo "::warning::max-annotations must be a positive integer, got: ${MAX_ANNOTATIONS}. Using default: 50"
  MAX=50
fi

# Detect package manager from lock files
PKG_MANAGER="npm"
ROOT="${FALLOW_ROOT:-.}"
if [ -f "${ROOT}/pnpm-lock.yaml" ] || [ -f "pnpm-lock.yaml" ]; then
  PKG_MANAGER="pnpm"
elif [ -f "${ROOT}/yarn.lock" ] || [ -f "yarn.lock" ]; then
  PKG_MANAGER="yarn"
fi
export PKG_MANAGER

# Scope results to changed files when --changed-since is active
RESULTS_FILE="fallow-results.json"
if [ -n "${CHANGED_SINCE:-}" ]; then
  CHANGED_JSON=""

  # Prefer pre-computed list from analyze step (handles shallow clones via API fallback)
  if [ -f fallow-changed-files.json ]; then
    CHANGED_JSON=$(cat fallow-changed-files.json)
  else
    # Fallback: compute locally (for standalone usage outside the action)
    _ROOT="${INPUT_ROOT:-.}"
    CHANGED_FILES=$(cd "$_ROOT" && git diff --name-only --relative "${CHANGED_SINCE}...HEAD" -- . 2>/dev/null || true)
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

ANNOTATIONS_FILE=$(mktemp)
: > "$ANNOTATIONS_FILE"

case "$FALLOW_COMMAND" in
  dead-code|check)
    jq -r -f "${ACTION_JQ_DIR}/annotations-check.jq" "$RESULTS_FILE" > "$ANNOTATIONS_FILE" 2>/dev/null || true ;;
  dupes)
    jq -r -f "${ACTION_JQ_DIR}/annotations-dupes.jq" "$RESULTS_FILE" > "$ANNOTATIONS_FILE" 2>/dev/null || true ;;
  health)
    jq -r -f "${ACTION_JQ_DIR}/annotations-health.jq" "$RESULTS_FILE" > "$ANNOTATIONS_FILE" 2>/dev/null || true ;;
  fix) ;;
  audit|"")
    # Audit JSON is re-keyed in analyze.sh to use combined-mode keys (check/health/dupes).
    {
      jq '.check // empty' "$RESULTS_FILE" | jq -r -f "${ACTION_JQ_DIR}/annotations-check.jq" 2>/dev/null || true
      jq '.health // empty' "$RESULTS_FILE" | jq -r -f "${ACTION_JQ_DIR}/annotations-health.jq" 2>/dev/null || true
      jq '.dupes // empty' "$RESULTS_FILE" | jq -r -f "${ACTION_JQ_DIR}/annotations-dupes.jq" 2>/dev/null || true
    } > "$ANNOTATIONS_FILE" ;;
esac

TOTAL=$(wc -l < "$ANNOTATIONS_FILE" | tr -d ' ')
if [ "$TOTAL" -gt 0 ]; then
  head -n "$MAX" "$ANNOTATIONS_FILE"
  if [ "$TOTAL" -gt "$MAX" ]; then
    echo "::notice::Showing ${MAX} of ${TOTAL} annotations. Increase max-annotations to see more."
  fi
fi

rm -f "$ANNOTATIONS_FILE"
