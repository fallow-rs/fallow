#!/usr/bin/env bash
set -eo pipefail

# Write the job summary.
#
# Render precedence (fail-open, most preferred first):
#   1. native - `fallow report --from <results> --format github-summary` when
#               the analyze step probed HAS_NATIVE_REPORT=true and the command
#               carries a report kind (i.e. not fix). The purpose-built
#               step-summary rendering, so it wins over the comment-shaped
#               typed body below.
#   2. typed  - the pr-comment envelope's .body, present only when the comment
#               step produced it
#   3. jq     - the bundled summary-*.jq renderers (older binaries)
#
# Required env: FALLOW_COMMAND, ACTION_JQ_DIR
# Optional env: CHANGED_SINCE, INPUT_ROOT, FALLOW_RESULTS_FILE,
#   FALLOW_SCOPED_RESULTS_FILE, FALLOW_CHANGED_FILES_FILE,
#   FALLOW_PR_COMMENT_ENVELOPE_FILE, HAS_NATIVE_REPORT, FALLOW_BIN

select_summary_script() {
  case "$FALLOW_COMMAND" in
    dead-code|check) echo "${ACTION_JQ_DIR}/summary-check.jq" ;;
    dupes)           echo "${ACTION_JQ_DIR}/summary-dupes.jq" ;;
    health)          echo "${ACTION_JQ_DIR}/summary-health.jq" ;;
    audit)           echo "${ACTION_JQ_DIR}/summary-audit.jq" ;;
    security)        echo "${ACTION_JQ_DIR}/summary-security.jq" ;;
    fix)             echo "${ACTION_JQ_DIR}/summary-fix.jq" ;;
    "")              echo "${ACTION_JQ_DIR}/summary-combined.jq" ;;
    *)               echo "::error::Unexpected command: ${FALLOW_COMMAND}"; exit 2 ;;
  esac
}

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

# The changed-files disclaimer appended when results were scoped to a diff.
scoping_footnote() {
  local commit_url="${GITHUB_SERVER_URL:-https://github.com}/${GITHUB_REPOSITORY}/commit/${CHANGED_SINCE}"
  printf '%s' "*Issue counts scoped to files changed since [\`${CHANGED_SINCE:0:7}\`](${commit_url}) · health metrics reflect the full codebase*"
}

# 1. Native renderer: the purpose-built step-summary rendering, source of truth.
emit_native_summary_if_available() {
  [ "${HAS_NATIVE_REPORT:-false}" = "true" ] || return 1
  # fix has no report kind: summary-fix.jq stays on the jq path below.
  [ "$FALLOW_COMMAND" = "fix" ] && return 1

  local input_file
  input_file=$(resolve_results_file)

  local rendered
  if ! rendered=$("${FALLOW_BIN:-fallow}" report --from "$input_file" --format github-summary 2>/dev/null); then
    echo "::warning::fallow native summary render failed; falling back to jq"
    return 1
  fi
  # Empty render: the binary succeeded but had nothing to say. Keep the jq
  # summary rather than writing a blank section.
  [ -n "$rendered" ] || return 1

  # Match the jq path's changed-files disclaimer when results were scoped.
  local scoped_file="${FALLOW_SCOPED_RESULTS_FILE:-fallow-results-scoped.json}"
  if [ "$input_file" = "$scoped_file" ]; then
    rendered="${rendered}"$'\n\n'"$(scoping_footnote)"
  fi

  echo "$rendered" >> "$GITHUB_STEP_SUMMARY"
  echo "fallow: summary rendered via native github-summary" >&2
  return 0
}

# 2. Typed renderer: the pr-comment envelope body, present only when the
# comment step produced it.
append_typed_summary_if_available() {
  local envelope_file="${FALLOW_PR_COMMENT_ENVELOPE_FILE:-}"
  if [ -z "$envelope_file" ] || [ ! -f "$envelope_file" ]; then
    return 1
  fi

  local body
  if ! body=$(jq -r '.body // empty' "$envelope_file" 2>/dev/null); then
    echo "::warning::Failed to read typed job summary envelope"
    return 1
  fi
  if [ -z "$body" ]; then
    return 1
  fi

  echo "$body" >> "$GITHUB_STEP_SUMMARY"
  return 0
}

if emit_native_summary_if_available; then
  exit 0
fi

if append_typed_summary_if_available; then
  exit 0
fi

# 3. jq fallback for binaries without `fallow report`.
JQ_FILE=$(select_summary_script)
if [ ! -f "$JQ_FILE" ]; then
  echo "::warning::Summary script not found: ${JQ_FILE}"
  exit 0
fi

RESULTS_FILE=$(resolve_results_file)
SCOPED_RESULTS_FILE="${FALLOW_SCOPED_RESULTS_FILE:-fallow-results-scoped.json}"

if ! BODY=$(jq -r -f "$JQ_FILE" "$RESULTS_FILE"); then
  echo "::warning::Failed to generate job summary"
  exit 0
fi

# Add scoping indicator when results were filtered to changed files
if [ "$RESULTS_FILE" = "$SCOPED_RESULTS_FILE" ]; then
  BODY="${BODY}"$'\n\n'"$(scoping_footnote)"
fi

echo "$BODY" >> "$GITHUB_STEP_SUMMARY"
echo "fallow: summary rendered via jq fallback" >&2
