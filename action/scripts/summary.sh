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
#   3. jq     - the bundled summary-*.jq renderers. These are frozen legacy
#               renderers for fallow before 3.4.2, and run only when the probe
#               found no `fallow report`. The fix command always uses
#               summary-fix.jq, because fix has no report kind.
#
# A binary with `fallow report` never uses the legacy renderers. The legacy
# renderers do not know the issue kinds that later versions added, so a
# fallback would hide findings. When the native and typed paths both give
# nothing, the step writes a warning and one summary line instead.
#
# Required env: FALLOW_COMMAND, ACTION_JQ_DIR
# Optional env: CHANGED_SINCE, INPUT_ROOT, FALLOW_RESULTS_FILE,
#   FALLOW_SCOPED_RESULTS_FILE, FALLOW_CHANGED_FILES_FILE,
#   FALLOW_PR_COMMENT_ENVELOPE_FILE, HAS_NATIVE_REPORT, FALLOW_BIN,
#   FALLOW_RENDER_PATH_PREFIX_SET, FALLOW_RENDER_PATH_PREFIX,
#   FALLOW_BASELINE_ENTRIES, FALLOW_BASELINE_STALE_ENTRIES,
#   FALLOW_BASELINE_ADVISORY, FALLOW_BASELINE_GATE_TRIPS

# shellcheck source=action/scripts/legacy-render.sh
. "$(dirname "${BASH_SOURCE[0]}")/legacy-render.sh"

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

  local args=(report --from "$input_file" --root "${INPUT_ROOT:-.}" --format github-summary)
  [ "${FALLOW_RENDER_PATH_PREFIX_SET:-0}" = "1" ] \
    && args+=(--report-path-prefix "${FALLOW_RENDER_PATH_PREFIX:-}")
  local rendered err_file
  err_file=$(mktemp)
  if ! rendered=$("${FALLOW_BIN:-fallow}" "${args[@]}" 2>"$err_file"); then
    cat "$err_file" >&2
    rm -f "$err_file"
    echo "::warning::fallow native summary render failed (fallow report --format github-summary)"
    return 1
  fi
  rm -f "$err_file"
  # Empty render: the binary succeeded but had nothing to say. Do not write a
  # blank section.
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

# The baseline advisory is written before the render dispatch below, because
# all three render paths return early and the line must appear on every one of
# them. Driven by the analyze step's outputs, not by re-reading the envelope,
# so the two surfaces cannot disagree.
append_baseline_advisory() {
  [ -n "${GITHUB_STEP_SUMMARY:-}" ] || return 0
  [ -n "${FALLOW_BASELINE_ENTRIES:-}" ] || return 0
  local line=""
  # Checked before the advisory: such a file carries zero entries, so the
  # advisory arms below would read "0 of 0 saved entries matched nothing this
  # run" next to the line that says what is actually wrong. Keyed on the
  # binary's verdict rather than on a zero entry count, which a baseline saved
  # on a project with nothing to record carries too. The path comes from the
  # analyze step, and is empty for a baseline that reached the run through the
  # `args` input, where the action never sees it.
  if [ "${FALLOW_BASELINE_UNRECOGNISED:-}" = "true" ]; then
    if [ -n "${FALLOW_BASELINE_PATH:-}" ]; then
      line="> **Baseline recognises nothing.** The baseline at \`${FALLOW_BASELINE_PATH}\` has no entries this command recognises. It may be a baseline saved by another command, or an empty file. Either way it suppresses nothing."
    else
      line="> **Baseline recognises nothing.** The baseline has no entries this command recognises. It may be a baseline saved by another command, or an empty file. Either way it suppresses nothing."
    fi
    printf '%s\n\n' "$line" >> "$GITHUB_STEP_SUMMARY"
    return 0
  fi
  case "${FALLOW_BASELINE_ADVISORY:-}" in
    partial)
      line="> **Baseline is partially stale.** ${FALLOW_BASELINE_STALE_ENTRIES} of ${FALLOW_BASELINE_ENTRIES} saved entries matched nothing this run, so the baseline protects less than what was saved. Re-save it with the \`save-baseline\` input."
      ;;
    zero-overlap)
      line="> **Baseline matched nothing.** All ${FALLOW_BASELINE_ENTRIES} saved entries went unmatched. Paths may have changed, or the baseline was saved elsewhere. Re-save it with the \`save-baseline\` input."
      ;;
    *)
      if [ "${FALLOW_BASELINE_GATE_TRIPS:-}" = "true" ]; then
        line="> **Baseline has stale entries.** ${FALLOW_BASELINE_STALE_ENTRIES} of ${FALLOW_BASELINE_ENTRIES} saved entries matched nothing this run. The project may be clean, or the baseline may no longer describe it."
      fi
      ;;
  esac
  [ -n "$line" ] || return 0
  printf '%s\n\n' "$line" >> "$GITHUB_STEP_SUMMARY"
}

# The gate inventory, from the same step outputs. `fallow report` renders its
# own neutral "Gate outcomes:" line into the body below, so this line lists the
# verdicts in the gate outputs. `failed` lists each gate with `status: fail`,
# which is not always a gate that failed the job. A default rule
# (`error-severity-findings`, `health-findings`, `audit-verdict`) that the count
# gate leaves unenforced when `fail-on-issues` is false can show as `failed` on
# a green job. Without this line a repository that armed a gate and passed it
# has no confirmation the input did anything.
append_gate_summary() {
  [ -n "${GITHUB_STEP_SUMMARY:-}" ] || return 0
  local parts=()
  [ -n "${FALLOW_GATES_FAILED:-}" ] && parts+=("failed ${FALLOW_GATES_FAILED}")
  [ -n "${FALLOW_GATES_WARNED:-}" ] && parts+=("warned ${FALLOW_GATES_WARNED}")
  [ -n "${FALLOW_GATES_SKIPPED:-}" ] && parts+=("stood down ${FALLOW_GATES_SKIPPED}")
  [ -n "${FALLOW_GATES_PASSED:-}" ] && parts+=("passed ${FALLOW_GATES_PASSED}")
  if [ ${#parts[@]} -gt 0 ]; then
    local joined=""
    for part in "${parts[@]}"; do
      joined="${joined:+${joined}; }${part}"
    done
    printf '%s\n\n' "> **Gates:** ${joined}." >> "$GITHUB_STEP_SUMMARY"
  fi
  # The degrading kinds are not all about files: a plugin config a reader could
  # not read, a health input that did not load, and a coverage snapshot the run
  # could not use all set this flag. The sentence therefore states what every
  # degrading kind has in common, wording it the way the analyze step already
  # words its own warning, instead of claiming files were skipped.
  if [ "${FALLOW_ANALYSIS_DEGRADED:-}" = "true" ]; then
    printf '%s\n\n' "> **Analysis was degraded.** Some findings or scores were computed over less than the whole project, or from an input that did not load." >> "$GITHUB_STEP_SUMMARY"
  fi
}

append_baseline_advisory
append_gate_summary

if emit_native_summary_if_available; then
  exit 0
fi

if append_typed_summary_if_available; then
  exit 0
fi

# A report-capable binary must not fall back to the legacy renderers: they do
# not know newer issue kinds and would give an incomplete summary.
if [ "${HAS_NATIVE_REPORT:-false}" = "true" ] && [ "$FALLOW_COMMAND" != "fix" ]; then
  echo "::warning::fallow could not render the job summary. The native render gave no output and no typed summary exists. See the earlier lines of this step log."
  printf '%s\n' "> **The job summary could not be rendered.** See the step log of the Job summary step." >> "$GITHUB_STEP_SUMMARY"
  exit 0
fi

# 3. Legacy jq renderers for binaries without `fallow report`, and
# summary-fix.jq for the fix command on every version.
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

if [ "$FALLOW_COMMAND" = "fix" ]; then
  echo "$BODY" >> "$GITHUB_STEP_SUMMARY"
  echo "fallow: summary rendered via summary-fix.jq" >&2
  exit 0
fi

legacy_renderer_notice "job summary"
BODY="${BODY}"$'\n\n'"*Rendered by the legacy renderer for fallow before ${NATIVE_VERSION}. Upgrade fallow to ${NATIVE_VERSION} or later to get the native summary.*"
echo "$BODY" >> "$GITHUB_STEP_SUMMARY"
echo "fallow: summary rendered via legacy jq renderer" >&2
