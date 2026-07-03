#!/usr/bin/env bash
set -eo pipefail

# Emit inline PR annotations via workflow commands
# Required env: FALLOW_COMMAND, MAX_ANNOTATIONS, ACTION_JQ_DIR
# Optional env: CHANGED_SINCE, INPUT_ROOT, FALLOW_RESULTS_FILE,
#   FALLOW_SCOPED_RESULTS_FILE, FALLOW_CHANGED_FILES_FILE,
#   FALLOW_PR_DECISION_FILE

MAX="${MAX_ANNOTATIONS:-50}"
if ! [[ "$MAX" =~ ^[0-9]+$ ]]; then
  echo "::warning::max-annotations must be a positive integer, got: ${MAX_ANNOTATIONS}. Using default: 50"
  MAX=50
fi

emit_typed_annotations_if_available() {
  local decision_file="${FALLOW_PR_DECISION_FILE:-}"
  if [ -z "$decision_file" ] || [ ! -f "$decision_file" ]; then
    return 1
  fi

  local annotations_file
  annotations_file=$(mktemp)
  _FALLOW_TMPS+=("$annotations_file")

  if ! jq -r '
    def esc:
      tostring
      | gsub("%"; "%25")
      | gsub("\r"; "%0D")
      | gsub("\n"; "%0A");
    def prop:
      esc
      | gsub(","; "%2C")
      | gsub(":"; "%3A");
    def workflow_level:
      if . == "failure" then "error"
      elif . == "notice" then "notice"
      else "warning"
      end;
    (.annotations // [])[]
    | (.line // 1) as $line
    | "::\(.level | workflow_level) file=\(.path | prop),line=\(if $line < 1 then 1 else $line end),title=\(.title | prop)::\(.message | esc)"
  ' "$decision_file" > "$annotations_file" 2>/dev/null; then
    echo "::warning::Failed to read typed annotation decision"
    return 1
  fi

  local total
  total=$(wc -l < "$annotations_file" | tr -d ' ')
  if [ "$total" -gt 0 ]; then
    head -n "$MAX" "$annotations_file"
    if [ "$total" -gt "$MAX" ]; then
      echo "::notice::Showing ${MAX} of ${total} annotations. Increase max-annotations to see more."
    fi
  fi
  return 0
}

_FALLOW_TMPS=()
trap 'rm -f "${_FALLOW_TMPS[@]:-}"' EXIT

if emit_typed_annotations_if_available; then
  exit 0
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
RESULTS_FILE="${FALLOW_RESULTS_FILE:-fallow-results.json}"
SCOPED_RESULTS_FILE="${FALLOW_SCOPED_RESULTS_FILE:-fallow-results-scoped.json}"
CHANGED_FILES_FILE="${FALLOW_CHANGED_FILES_FILE:-fallow-changed-files.json}"
if [ -n "${CHANGED_SINCE:-}" ]; then
  CHANGED_JSON=""

  # Prefer pre-computed list from analyze step (handles shallow clones via API fallback)
  if [ -f "$CHANGED_FILES_FILE" ]; then
    CHANGED_JSON=$(cat "$CHANGED_FILES_FILE")
  else
    # Fallback: compute locally (for standalone usage outside the action)
    _ROOT="${INPUT_ROOT:-.}"
    CHANGED_FILES=$(cd "$_ROOT" && git diff --name-only --relative "${CHANGED_SINCE}...HEAD" -- . 2>/dev/null || true)
    if [ -n "$CHANGED_FILES" ]; then
      CHANGED_JSON=$(echo "$CHANGED_FILES" | jq -R -s 'split("\n") | map(select(length > 0))')
    fi
  fi

  if [ -n "$CHANGED_JSON" ] && [ "$CHANGED_JSON" != "[]" ]; then
    if jq --argjson changed "$CHANGED_JSON" -f "${ACTION_JQ_DIR}/filter-changed.jq" "$RESULTS_FILE" > "$SCOPED_RESULTS_FILE" 2>/dev/null; then
      RESULTS_FILE="$SCOPED_RESULTS_FILE"
    fi
  fi
fi

ANNOTATIONS_FILE=$(mktemp)
_FALLOW_TMPS+=("$ANNOTATIONS_FILE")
: > "$ANNOTATIONS_FILE"

case "$FALLOW_COMMAND" in
  dead-code|check)
    jq -r -f "${ACTION_JQ_DIR}/annotations-check.jq" "$RESULTS_FILE" > "$ANNOTATIONS_FILE" 2>/dev/null || true ;;
  dupes)
    jq -r -f "${ACTION_JQ_DIR}/annotations-dupes.jq" "$RESULTS_FILE" > "$ANNOTATIONS_FILE" 2>/dev/null || true ;;
  health)
    jq -r -f "${ACTION_JQ_DIR}/annotations-health.jq" "$RESULTS_FILE" > "$ANNOTATIONS_FILE" 2>/dev/null || true ;;
  audit)
    {
      jq '.dead_code // empty' "$RESULTS_FILE" | jq -r -f "${ACTION_JQ_DIR}/annotations-check.jq" 2>/dev/null || true
      jq '.complexity // empty' "$RESULTS_FILE" | jq -r -f "${ACTION_JQ_DIR}/annotations-health.jq" 2>/dev/null || true
      jq '.duplication // empty' "$RESULTS_FILE" | jq -r -f "${ACTION_JQ_DIR}/annotations-dupes.jq" 2>/dev/null || true
    } > "$ANNOTATIONS_FILE" ;;
  fix) ;;
  "")
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
