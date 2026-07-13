#!/usr/bin/env bash
set -eo pipefail

# Emit inline PR annotations via workflow commands.
#
# Render precedence (fail-open, most preferred first):
#   1. native - `fallow report --from <results> --format github-annotations`
#               when the analyze step probed HAS_NATIVE_REPORT=true and the
#               command carries a report kind (i.e. not fix)
#   2. typed  - the pr-comment decision JSON, present only when the comment
#               step ran; strict escaping the native format is byte-compatible with
#   3. jq     - the bundled annotations-*.jq renderers (older binaries)
#
# Required env: FALLOW_COMMAND, MAX_ANNOTATIONS, ACTION_JQ_DIR
# Optional env: CHANGED_SINCE, INPUT_ROOT, FALLOW_RESULTS_FILE,
#   FALLOW_SCOPED_RESULTS_FILE, FALLOW_CHANGED_FILES_FILE,
#   FALLOW_PR_DECISION_FILE, HAS_NATIVE_REPORT, FALLOW_BIN

MAX="${MAX_ANNOTATIONS:-50}"
if ! [[ "$MAX" =~ ^[0-9]+$ ]]; then
  echo "::warning::max-annotations must be a positive integer, got: ${MAX_ANNOTATIONS}. Using default: 50"
  MAX=50
fi

_FALLOW_TMPS=()
trap 'rm -f "${_FALLOW_TMPS[@]:-}"' EXIT

# Resolve the results file the render paths consume, scoping it to the changed
# files when --changed-since is active. Native and jq select the same input.
resolve_results_file() {
  local results_file="${FALLOW_RESULTS_FILE:-fallow-results.json}"
  local scoped_file="${FALLOW_SCOPED_RESULTS_FILE:-fallow-results-scoped.json}"
  local changed_files_file="${FALLOW_CHANGED_FILES_FILE:-fallow-changed-files.json}"
  if [ -n "${CHANGED_SINCE:-}" ]; then
    local changed_json=""

    # Prefer pre-computed list from analyze step (handles shallow clones via API fallback)
    if [ -f "$changed_files_file" ]; then
      changed_json=$(cat "$changed_files_file")
    else
      # Fallback: compute locally (for standalone usage outside the action)
      local root="${INPUT_ROOT:-.}"
      local changed_files
      changed_files=$(cd "$root" && git diff --name-only --relative "${CHANGED_SINCE}...HEAD" -- . 2>/dev/null || true)
      if [ -n "$changed_files" ]; then
        changed_json=$(echo "$changed_files" | jq -R -s 'split("\n") | map(select(length > 0))')
      fi
    fi

    if [ -n "$changed_json" ] && [ "$changed_json" != "[]" ]; then
      if jq --argjson changed "$changed_json" -f "${ACTION_JQ_DIR}/filter-changed.jq" "$results_file" > "$scoped_file" 2>/dev/null; then
        results_file="$scoped_file"
      fi
    fi
  fi
  printf '%s\n' "$results_file"
}

# 1. Native renderer. The action adds only the MAX cap and its truncation
# notice on top of the native stream, so the emitted annotations stay
# byte-identical to `fallow report --from ... | head -n MAX`.
emit_native_annotations_if_available() {
  [ "${HAS_NATIVE_REPORT:-false}" = "true" ] || return 1
  # fix has no report kind; the step gate already excludes it, belt and braces.
  [ "$FALLOW_COMMAND" = "fix" ] && return 1

  local input_file
  input_file=$(resolve_results_file)

  local native_file
  native_file=$(mktemp)
  _FALLOW_TMPS+=("$native_file")

  if ! "${FALLOW_BIN:-fallow}" report --from "$input_file" --format github-annotations > "$native_file" 2>/dev/null; then
    echo "::warning::fallow native annotation render failed; falling back to jq"
    return 1
  fi

  local total
  total=$(wc -l < "$native_file" | tr -d ' ')
  if [ "$total" -gt 0 ]; then
    head -n "$MAX" "$native_file"
    if [ "$total" -gt "$MAX" ]; then
      echo "::notice::Showing ${MAX} of ${total} annotations. Increase max-annotations to see more."
    fi
  fi
  echo "fallow: annotations rendered via native github-annotations" >&2
  return 0
}

# 2. Typed renderer: the pr-comment decision JSON, present only when the
# comment step produced it. Same strict escaping as the native format.
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

if emit_native_annotations_if_available; then
  exit 0
fi

if emit_typed_annotations_if_available; then
  exit 0
fi

# 3. jq fallback for binaries without `fallow report`.

# Detect package manager from lock files
PKG_MANAGER="npm"
ROOT="${FALLOW_ROOT:-.}"
if [ -f "${ROOT}/pnpm-lock.yaml" ] || [ -f "pnpm-lock.yaml" ]; then
  PKG_MANAGER="pnpm"
elif [ -f "${ROOT}/yarn.lock" ] || [ -f "yarn.lock" ]; then
  PKG_MANAGER="yarn"
fi
export PKG_MANAGER

RESULTS_FILE=$(resolve_results_file)

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
echo "fallow: annotations rendered via jq fallback" >&2
