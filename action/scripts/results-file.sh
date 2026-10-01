#!/usr/bin/env bash
# Shared results-file helper for the GitHub Action render steps.
#
# summary.sh and annotate.sh source this file. It defines functions only and
# changes no shell state. The caller owns the EXIT handler and the temp files.
#
# Optional env: FALLOW_RESULTS_FILE, FALLOW_SCOPED_RESULTS_FILE,
#   FALLOW_CHANGED_FILES_FILE, CHANGED_SINCE, INPUT_ROOT
# Required env when CHANGED_SINCE is set: ACTION_JQ_DIR

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
