#!/usr/bin/env bash
# Shared `fallow mutation` helpers for the GitHub Action
# (action/scripts/mutation.sh) and the GitLab template (ci/scripts/mutation.sh).
# The caller sources this file, sets the FALLOW_MUTATION_* variables below,
# and calls fallow_mutation_main.
#
# Input variables:
#   FALLOW_MUTATION_ROOT           project root (default: .)
#   FALLOW_MUTATION_CONFIG         config file path (optional)
#   FALLOW_MUTATION_SCOPE          changed (default), all, or paths
#   FALLOW_MUTATION_TIME_BUDGET    for example 10m; empty means no budget
#   FALLOW_MUTATION_MAX_MUTANTS    optional positive integer
#   FALLOW_MUTATION_MIN_SCORE      optional percent from 0 to 100
#   FALLOW_MUTATION_DIFF_FILE      diff of the changed scope (optional)
#   FALLOW_MUTATION_CHANGED_SINCE  base ref of the changed scope (optional)
#   FALLOW_MUTATION_REFUSE_REASON  non-empty: do not run, and report why
#   FALLOW_MUTATION_OUT_DIR        directory for the result files (default: .)
#
# Result files in FALLOW_MUTATION_OUT_DIR, written after the run:
#   fallow-mutation.json          the mutation report, when the run wrote one
#   fallow-mutation-tasks.json    the open tasks of the report, when available
#   fallow-mutation-status.json   the outcome of the step
#   fallow-mutation-section.md    the section for the sticky comment

FALLOW_MUTATION_TOP_ITEMS=5
FALLOW_MUTATION_MAX_LINE_CHARS=160

# The report fields that the status and the section read. A report of another
# shape counts as a tool failure, so a gate result is never lost.
FALLOW_MUTATION_REPORT_SHAPE='
  (.completion | type == "object")
  and (.completion.status | type == "string")
  and (.summary | type == "object")
  and (.summary.counts | type == "object")
  and ((.mutants // []) | type == "array")
'

fallow_mutation_sha256() {
  if command -v sha256sum > /dev/null 2>&1; then
    sha256sum | cut -d' ' -f1
  else
    shasum -a 256 | cut -d' ' -f1
  fi
}

# Prints a 16-character digest of the lockfiles and the runner config files
# of a project root. The CI cache key of the mutation result cache uses it, so
# a dependency or runner config change starts a new cache.
fallow_mutation_cache_digest() {
  local root=${1:-.}
  local top dir name path
  top=$(git -C "$root" rev-parse --show-toplevel 2>/dev/null || printf '%s' "$root")
  {
    for dir in "$root" "$top"; do
      for name in package-lock.json npm-shrinkwrap.json yarn.lock pnpm-lock.yaml bun.lock bun.lockb; do
        path="$dir/$name"
        [ -f "$path" ] && printf 'lock %s %s\n' "$name" "$(fallow_mutation_sha256 < "$path")"
      done
    done
    for path in "$root"/vitest.config.* "$root"/vite.config.* "$root"/vitest.workspace.* \
        "$root/bunfig.toml" "$root/package.json" "$root/tsconfig.json"; do
      [ -f "$path" ] && printf 'config %s %s\n' "${path##*/}" "$(fallow_mutation_sha256 < "$path")"
    done
    true
  } | fallow_mutation_sha256 | cut -c1-16
}

# Checks the input values before they reach the command line. Fallow
# validates the meaning of each value; this guard only keeps a value from
# being read as a flag. Prints the problem and returns 1 for a bad value.
fallow_mutation_validate_inputs() {
  local budget=${FALLOW_MUTATION_TIME_BUDGET:-}
  local max=${FALLOW_MUTATION_MAX_MUTANTS:-}
  local min=${FALLOW_MUTATION_MIN_SCORE:-}
  if [ -n "$budget" ] && ! [[ "$budget" =~ ^[0-9][0-9a-z.]*$ ]]; then
    printf 'The mutation time budget must be a duration such as 90s, 10m or 1h, got: %s\n' "$budget"
    return 1
  fi
  if [ -n "$max" ] && ! [[ "$max" =~ ^[1-9][0-9]*$ ]]; then
    printf 'The mutation mutant limit must be a positive integer, got: %s\n' "$max"
    return 1
  fi
  if [ -n "$min" ] && ! [[ "$min" =~ ^[0-9]+(\.[0-9]+)?$ ]]; then
    printf 'The mutation minimum score must be a number from 0 to 100, got: %s\n' "$min"
    return 1
  fi
  local token
  for token in $(fallow_mutation_scope_paths); do
    case "$token" in
      -*)
        printf 'A mutation scope path must not start with "-", got: %s\n' "$token"
        return 1
        ;;
    esac
  done
  return 0
}

fallow_mutation_scope() {
  local scope=${FALLOW_MUTATION_SCOPE:-changed}
  printf '%s' "${scope:-changed}"
}

# Prints the paths of a path scope, one for each line. Commas and white space
# separate the paths. Prints nothing for the changed and all scopes.
fallow_mutation_scope_paths() {
  local scope
  scope=$(fallow_mutation_scope)
  case "$scope" in
    changed|all) return 0 ;;
  esac
  printf '%s\n' "$scope" | tr ',' ' ' | tr -s '[:space:]' '\n' | sed '/^$/d'
}

# Sets FALLOW_MUTATION_ARGS. Returns 3 when the changed scope has no diff
# and no base ref, so the run cannot know the changed lines.
fallow_mutation_build_args() {
  local report_file=$1
  local scope path
  scope=$(fallow_mutation_scope)
  FALLOW_MUTATION_ARGS=(mutation --root "${FALLOW_MUTATION_ROOT:-.}" --format json --quiet -o "$report_file")
  [ -n "${FALLOW_MUTATION_CONFIG:-}" ] && FALLOW_MUTATION_ARGS+=(--config "$FALLOW_MUTATION_CONFIG")
  case "$scope" in
    changed)
      if [ -n "${FALLOW_MUTATION_DIFF_FILE:-}" ]; then
        FALLOW_MUTATION_ARGS+=(--diff-file "$FALLOW_MUTATION_DIFF_FILE")
      elif [ -n "${FALLOW_MUTATION_CHANGED_SINCE:-}" ]; then
        FALLOW_MUTATION_ARGS+=(--changed-since "$FALLOW_MUTATION_CHANGED_SINCE")
      else
        return 3
      fi
      ;;
    all)
      FALLOW_MUTATION_ARGS+=(--all)
      ;;
    *)
      while IFS= read -r path; do
        FALLOW_MUTATION_ARGS+=("$path")
      done < <(fallow_mutation_scope_paths)
      ;;
  esac
  [ -n "${FALLOW_MUTATION_TIME_BUDGET:-}" ] && FALLOW_MUTATION_ARGS+=(--time-budget "$FALLOW_MUTATION_TIME_BUDGET")
  [ -n "${FALLOW_MUTATION_MAX_MUTANTS:-}" ] && FALLOW_MUTATION_ARGS+=(--max-mutants "$FALLOW_MUTATION_MAX_MUTANTS")
  [ -n "${FALLOW_MUTATION_MIN_SCORE:-}" ] && FALLOW_MUTATION_ARGS+=(--min-score "$FALLOW_MUTATION_MIN_SCORE")
  return 0
}

# Maps the exit code of `fallow mutation` to the outcome of the step.
# 0: the run finished (complete or partial). 1: the --min-score gate failed.
# 2: invalid input or an execution error. 128 + n: a signal stopped the run.
fallow_mutation_outcome_for_exit() {
  local code=$1
  if [ "$code" -eq 0 ]; then
    printf 'ok'
  elif [ "$code" -eq 1 ]; then
    printf 'gate-failed'
  elif [ "$code" -ge 128 ]; then
    printf 'interrupted'
  else
    printf 'tool-failed'
  fi
}

# Writes the status file. Arguments: file, outcome, exit code (or empty),
# error code, message, help, report file (or empty).
fallow_mutation_write_status() {
  local file=$1 outcome=$2 exit_code=$3 code=$4 message=$5 help=$6 report=$7
  jq -n \
    --arg outcome "$outcome" \
    --arg exit_code "$exit_code" \
    --arg code "$code" \
    --arg message "$message" \
    --arg help "$help" \
    --arg scope "$(fallow_mutation_scope)" \
    --arg root "${FALLOW_MUTATION_ROOT:-.}" \
    --arg time_budget "${FALLOW_MUTATION_TIME_BUDGET:-}" \
    --arg max_mutants "${FALLOW_MUTATION_MAX_MUTANTS:-}" \
    --arg min_score "${FALLOW_MUTATION_MIN_SCORE:-}" \
    --slurpfile report "${report:-/dev/null}" \
    '
    ($report[0] // null) as $r
    | {
        outcome: $outcome,
        exit_code: (if $exit_code == "" then null else ($exit_code | tonumber) end),
        code: (if $code == "" then null else $code end),
        message: (if $message == "" then null else $message end),
        help: (if $help == "" then null else $help end),
        scope: $scope,
        root: $root,
        time_budget: (if $time_budget == "" then null else $time_budget end),
        max_mutants: (if $max_mutants == "" then null else ($max_mutants | tonumber) end),
        min_score: (if $min_score == "" then null else ($min_score | tonumber) end),
        completion: ($r.completion.status // null),
        score: ($r.summary.score // null),
        clean: (
          ($outcome == "ok" or $outcome == "skipped")
          and (($r.completion.status // "complete") == "complete")
          and (($r.summary.counts.survived // 0) == 0)
          and (($r.summary.counts.no_coverage // 0) == 0)
        )
      }
    ' > "$file"
}

# The jq program that renders the comment section from the status, the
# report and the tasks. Paths, names and source lines come from the pull
# request, so they appear only inside code spans and code blocks.
# shellcheck disable=SC2016
FALLOW_MUTATION_SECTION_JQ='
def quote: [39] | implode;
def span: tostring | gsub("[`\r\n]"; quote) | "`" + . + "`";
def pct: if . == null then "n/a" else ((. * 1000 | round) / 10 | tostring) + "%" end;
def duration:
  (. / 1000 | floor) as $s
  | if $s >= 3600 then "\($s / 3600 | floor)h \(($s % 3600) / 60 | floor)m"
    elif $s >= 60 then "\($s / 60 | floor)m \($s % 60)s"
    else "\($s)s" end;
def clip: if length > $max_chars then .[0:$max_chars] + " ..." else . end;
def block($sign): split("\n") | map("   " + $sign + (gsub("\r"; "") | clip)) | join("\n");
def text_block: "```text\n" + (tostring | gsub("```"; quote + quote + quote)) + "\n```";

($status[0]) as $st
| ($report[0] // null) as $r
| ($tasks[0] // null) as $t
| (if $st.root == "." then "" else " --root " + $st.root end) as $root_flag
| ($r.summary.counts // {}) as $c
| (($c.killed // 0) + ($c.timeout // 0)) as $killed
| (($c.pending // 0) + ($c.runtime_error // 0) + ($c.flaky // 0) + ($c.compile_error // 0)) as $not_measured
| (($r.completion.reasons // []) | map(span) | join(", ")) as $reasons

| def counts_table:
    "| Killed | Survived | No coverage | Not measured | Score |\n"
    + "| --- | --- | --- | --- | --- |\n"
    + "| \($killed) | \($c.survived // 0) | \($c.no_coverage // 0) | \($not_measured) | \($r.summary.score | pct) |";

  def scope_text:
    if $r.scope.kind == "changed" then "the changed lines"
    elif $r.scope.kind == "all" then "all source files"
    elif $r.scope.kind == "paths" then "the selected paths"
    else ($st.scope | span) end;

  def budget_text:
    [ (if $st.time_budget != null then "time " + ($st.time_budget | span) else empty end),
      (if $st.max_mutants != null then "\($st.max_mutants) mutants" else empty end) ]
    | if length == 0 then "none" else join(", ") end;

  def run_facts:
    "Scope: \(scope_text). Budget: \(budget_text)."
    + (if $r.elapsed_ms != null then " The run took \($r.elapsed_ms | duration)." else "" end)
    + (if ($r.cache.hits // 0) > 0 then " \($r.cache.hits) results came from the cache." else "" end);

  def task_for($id): ($t.tasks // []) | map(select(.mutant_id == $id)) | first;

  def item($m):
    task_for($m.mutant_id) as $task
    | "1. " + ("\($m.file):\($m.line)" | span)
      + (if $m.function != null then ", function " + ($m.function | span) else "" end)
      + (if $m.status == "survived" then " (survived)" else " (no test reaches it)" end)
      + "\n"
      + (if $task.original_lines != null and $task.mutated_lines != null then
           "   ````diff\n" + ($task.original_lines | block("-")) + "\n"
           + ($task.mutated_lines | block("+")) + "\n   ````\n"
         elif $m.change != null then
           "   Change: " + ($m.change.from | span) + " to " + ($m.change.to | span) + "\n"
         else "" end)
      + "   " + ("fallow mutation inspect \($m.mutant_id)" + $root_flag | span);

  def survivors:
    [ ($r.mutants // [])[] | select(.status == "survived" or .status == "no_coverage") ]
    | sort_by(if .status == "survived" then 0 else 1 end)
    | if length == 0 then ""
      else
        "\n\n<details open>\n<summary>Top survivors (\([length, $top] | min) of \(length))</summary>\n\n"
        + (.[0:$top] | map(item(.)) | join("\n"))
        + "\n\n</details>\n\nEach item is a small code change that made no test fail. Add a test that fails on the change. To inspect an item on your machine, run `fallow mutation` with the same scope on this commit first, or add `--report` with the `fallow-mutation.json` file of this job. `fallow mutation tasks` lists each item."
      end;

  def partial_note:
    if ($r.completion.status // "complete") == "partial" then
      "**Partial run** (\($reasons)). The counts cover only the mutants that ran."
      + (if ($r.completion.reasons // []) | any(. == "time-budget")
         then " The next run with the same budget continues from the result cache."
         else "" end)
      + "\n\n"
    else "" end;

  def headline:
    if $st.outcome == "gate-failed" then
      "**Score below the minimum.** The score is \($r.summary.score | pct)"
      + (if $st.min_score != null then ", and the minimum is \($st.min_score)%." else ", and the minimum score of the config is higher." end)
      + "\n\n"
    elif $st.outcome == "interrupted" then
      "**Mutation testing stopped** because a signal stopped the run (exit code \($st.exit_code)).\n\n"
    elif ($c.total // 0) == 0 then
      "The scope has no mutant.\n\n"
    elif (($c.survived // 0) + ($c.no_coverage // 0)) == 0 then
      "The tests detected each mutant that ran.\n\n"
    else "" end;

  "## Mutation testing\n\n"
  + (
    if $st.outcome == "refused" then
      "**Mutation testing did not run.** " + $st.message
    elif $st.outcome == "skipped" then
      "Mutation testing did not run: " + $st.message
    elif $st.outcome == "invalid-input" then
      "**Mutation testing did not start.** The input is invalid.\n\n" + ($st.message | text_block)
    elif $st.outcome == "tool-failed" then
      "**Mutation testing failed** (exit code \($st.exit_code // "n/a")"
      + (if $st.code != null then ", " + ($st.code | span) else "" end) + ").\n\n"
      + ($st.message // "fallow mutation gave no error message." | text_block)
      + (if $st.help != null then "\n\n" + ($st.help | text_block) else "" end)
    elif $r == null then
      "**Mutation testing failed** (exit code \($st.exit_code // "n/a")). The run wrote no report."
    else
      partial_note + headline + counts_table + "\n\n" + run_facts + survivors
    end
  )
  + "\n"
'

# Renders the comment section to stdout. Arguments: status file, report
# file (or empty), tasks file (or empty).
fallow_mutation_render_section() {
  local status=$1 report=${2:-} tasks=${3:-}
  [ -n "$report" ] && [ -s "$report" ] || report=/dev/null
  [ -n "$tasks" ] && [ -s "$tasks" ] || tasks=/dev/null
  jq -nrj \
    --slurpfile status "$status" \
    --slurpfile report "$report" \
    --slurpfile tasks "$tasks" \
    --argjson top "$FALLOW_MUTATION_TOP_ITEMS" \
    --argjson max_chars "$FALLOW_MUTATION_MAX_LINE_CHARS" \
    "$FALLOW_MUTATION_SECTION_JQ"
}

# shellcheck disable=SC2016
FALLOW_MUTATION_INSERT_JQ='
def insert_section($s):
  split("\nGenerated by fallow.") as $p
  | if ($p | length) < 2 then . + "\n" + $s
    else ($p[:-1] | join("\nGenerated by fallow.")) + "\n" + $s + "\nGenerated by fallow." + $p[-1]
    end;
'

# Puts the mutation section above the footer of the rendered sticky comment,
# in the markdown body and in the envelope that the post command reads. A
# section with open items makes the envelope not clean, so the post command
# creates the first comment. Arguments: body file, envelope file (or empty),
# section file, clean (true or false).
fallow_mutation_insert_section() {
  local body=$1 envelope=${2:-} section=$3 clean=${4:-true}
  [ -s "$section" ] || return 0
  local tmp
  if [ -f "$body" ]; then
    tmp="${body}.mutation.tmp"
    if ! jq -Rsj --rawfile s "$section" "${FALLOW_MUTATION_INSERT_JQ} insert_section(\$s)" "$body" > "$tmp"; then
      rm -f "$tmp"
      return 1
    fi
    mv "$tmp" "$body"
  fi
  if [ -n "$envelope" ] && [ -f "$envelope" ]; then
    tmp="${envelope}.mutation.tmp"
    if ! jq --rawfile s "$section" --argjson clean "$clean" \
        "${FALLOW_MUTATION_INSERT_JQ} .body |= insert_section(\$s) | .is_clean = (.is_clean and \$clean)" \
        "$envelope" > "$tmp"; then
      rm -f "$tmp"
      return 1
    fi
    mv "$tmp" "$envelope"
  fi
  return 0
}

# Prints one field of the error envelope that `fallow mutation` printed.
fallow_mutation_error_field() {
  local stdout_file=$1 field=$2
  jq -rs --arg f "$field" \
    'map(select(type == "object" and .error == true)) | first | .[$f] // empty' \
    "$stdout_file" 2>/dev/null || true
}

# Runs `fallow mutation` and writes the result files. Returns 0 when the run
# finished or was skipped, 1 when the --min-score gate failed, and 2 for a
# refusal, a tool failure or a signal. The caller decides when to apply the
# return code, so the analysis and the comment still run after a failure.
fallow_mutation_main() {
  local out_dir=${FALLOW_MUTATION_OUT_DIR:-.}
  local tmp_parent=${RUNNER_TEMP:-${TMPDIR:-/tmp}}
  local tmp
  tmp=$(mktemp -d "${tmp_parent%/}/fallow-mutation.XXXXXX")
  local report="$tmp/report.json"
  local status_tmp="$tmp/status.json"
  local tasks="$tmp/tasks.json"
  local outcome exit_code="" code="" message="" help="" problem build_status=0
  local name

  # A result file of an earlier run in the same directory must not look like
  # the result of this run.
  for name in fallow-mutation.json fallow-mutation-tasks.json fallow-mutation-status.json \
      fallow-mutation-section.md fallow-mutation-stderr.log; do
    rm -f "${out_dir}/${name}"
  done

  if [ -n "${FALLOW_MUTATION_REFUSE_REASON:-}" ]; then
    outcome=refused
    message=$FALLOW_MUTATION_REFUSE_REASON
  elif ! problem=$(fallow_mutation_validate_inputs); then
    outcome=invalid-input
    message=$problem
  else
    fallow_mutation_build_args "$report" || build_status=$?
    if [ "$build_status" -eq 3 ]; then
      outcome=skipped
      message="the changed scope needs a pull request diff or a base ref, and this job has neither. Run the job in a pull request, set a base ref, or use the all scope."
    else
      # The tests of the project run inside this command. Its stdout and
      # stderr go to the temp directory: a new file in the worktree would
      # make the dirty-worktree check of the run refuse the run.
      exit_code=0
      fallow "${FALLOW_MUTATION_ARGS[@]}" > "$tmp/stdout.json" 2> "$tmp/stderr.log" || exit_code=$?
      outcome=$(fallow_mutation_outcome_for_exit "$exit_code")
      code=$(fallow_mutation_error_field "$tmp/stdout.json" code)
      message=$(fallow_mutation_error_field "$tmp/stdout.json" message)
      help=$(fallow_mutation_error_field "$tmp/stdout.json" help)
      # The stderr can hold test output, so it goes to the log file of the
      # job and never into the comment.
      if [ -z "$message" ] && [ "$outcome" = "tool-failed" ]; then
        message="fallow mutation printed no error message. The file fallow-mutation-stderr.log of this job has its error output."
      fi
      if [ -s "$report" ] && ! jq -e "$FALLOW_MUTATION_REPORT_SHAPE" "$report" > /dev/null 2>&1; then
        rm -f "$report"
        if [ "$outcome" = "ok" ] || [ "$outcome" = "gate-failed" ]; then
          outcome=tool-failed
          message="fallow mutation wrote a report with an unknown shape."
        fi
      fi
    fi
  fi

  [ -s "$report" ] || report=""
  if [ -n "$report" ]; then
    local tasks_args=(mutation tasks --report "$report" --root "${FALLOW_MUTATION_ROOT:-.}" --format json --quiet)
    [ -n "${FALLOW_MUTATION_CONFIG:-}" ] && tasks_args+=(--config "$FALLOW_MUTATION_CONFIG")
    fallow "${tasks_args[@]}" > "$tasks" 2> /dev/null || rm -f "$tasks"
  fi

  mkdir -p "$out_dir"
  if ! fallow_mutation_write_status "$status_tmp" "$outcome" "$exit_code" "$code" "$message" "$help" "$report"; then
    outcome=tool-failed
    report=""
    fallow_mutation_write_status "$status_tmp" "$outcome" "$exit_code" "" \
      "The CI step could not read the result of fallow mutation." "" ""
  fi
  if ! fallow_mutation_render_section "$status_tmp" "$report" "$tasks" > "$out_dir/fallow-mutation-section.md"; then
    printf '## Mutation testing\n\nThe CI step could not render the result. See fallow-mutation-status.json of this job.\n' \
      > "$out_dir/fallow-mutation-section.md"
  fi
  cp "$status_tmp" "$out_dir/fallow-mutation-status.json"
  [ -n "$report" ] && cp "$report" "$out_dir/fallow-mutation.json"
  [ -s "$tasks" ] && cp "$tasks" "$out_dir/fallow-mutation-tasks.json"
  [ -s "$tmp/stderr.log" ] && cp "$tmp/stderr.log" "$out_dir/fallow-mutation-stderr.log"
  rm -rf "$tmp"

  case "$outcome" in
    ok|skipped) return 0 ;;
    gate-failed) return 1 ;;
    *) return 2 ;;
  esac
}
