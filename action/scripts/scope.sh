#!/usr/bin/env bash
# Resolve the change scope of the job once, before the mutation run and the
# analysis: the effective changed-since ref, a merge base on a shallow clone,
# the changed files and the unified diff. The step also validates
# artifacts-dir and probes the `fallow report` capabilities.
#
# The step writes only to the scope directory ($RUNNER_TEMP/fallow-scope, or
# ${TMPDIR:-/tmp}/fallow-scope) and to .git (fetch). It never writes to the
# worktree, because the mutation run refuses a dirty worktree. The paths in the
# diff and in the changed-files list are relative to INPUT_ROOT.
#
# Exports through $GITHUB_ENV:
#   FALLOW_SCOPE_CHANGED_SINCE  effective changed-since ref (empty: unscoped)
#   FALLOW_DIFF_FILE            user diff-file, or the scope diff (when made)
#   FALLOW_CHANGED_FILES_FILE   scope changed-files list (when made)
#   FALLOW_ARTIFACTS_DIR        validated artifacts-dir
#   FALLOW_SCOPE_CHANGED_FILES_UNAVAILABLE  true when the GitHub API failed
#   HAS_NATIVE_REPORT, HAS_MUTATION_REPORT  capability probes
#
# analyze.sh sources this file for the shared helpers. When the scope step did
# not run (FALLOW_SCOPE_CHANGED_SINCE is unset), analyze.sh calls
# fallow_scope_main in its own process.
#
# Env: INPUT_ROOT, INPUT_CHANGED_SINCE, INPUT_AUTO_CHANGED_SINCE,
# INPUT_DIFF_FILE (or FALLOW_DIFF_FILE), INPUT_ARTIFACTS_DIR, INPUT_COMMAND,
# INPUT_BASELINE, INPUT_CONFIG, PR_BASE_SHA, PR_NUMBER, EVENT_NAME, GH_REPO,
# GH_TOKEN, GITHUB_WORKSPACE.

# Replay a captured stderr file into the step log as ::debug:: lines, then
# remove the file. A discarded stderr is why a degraded change scope, a
# truncated envelope or a failed capability probe reaches the log with no
# cause (issues #2673, #2704, #2740). ::debug:: keeps a green run quiet: the
# lines appear only when the command wrote something, and only for a run with
# ACTIONS_STEP_DEBUG.
replay_stderr_as_debug() {
  local file=$1 label=$2
  if [ -s "$file" ]; then
    while IFS= read -r line; do
      echo "::debug::${label}: ${line}" >&2
    done < "$file"
  fi
  rm -f "$file"
}

contains_ascii_control() {
  local LC_ALL=C
  local value=$1
  [[ "$value" =~ [[:cntrl:]] ]]
}

fallow_scope_dir() {
  printf '%s/fallow-scope\n' "${RUNNER_TEMP:-${TMPDIR:-/tmp}}"
}

# The value reaches `mkdir`, `$GITHUB_ENV` and artifact paths, so it must stay
# a single relative line inside the workspace.
fallow_scope_validate_artifacts_dir() {
  local dir=$1
  if [[ "$dir" = /* ]] || [[ "$dir" = -* ]] || \
     [[ "$dir" == *$'\n'* ]] || [[ "$dir" == *$'\r'* ]] || \
     [[ "$dir" =~ (^|/)\.\.(/|$) ]]; then
    echo "::error::artifacts-dir must be a relative path inside the workspace, got: ${dir}"
    return 1
  fi
}

is_dead_code_baseline_command() {
  [ -n "${INPUT_BASELINE:-}" ] || return 1
  case "${INPUT_COMMAND:-}" in
    ""|dead-code|check|architecture) return 0 ;;
    *) return 1 ;;
  esac
}

repo_relative_root() {
  local root="${INPUT_ROOT:-.}"
  root="${root#./}"
  root="${root%/}"
  # `./` and `.//` become empty after the strips, and name the workspace root.
  [ -n "$root" ] || root=.

  if [ "$root" = "." ]; then
    printf '.\n'
    return 0
  fi

  if [[ "$root" != /* ]]; then
    printf '%s\n' "${root%/}"
    return 0
  fi

  local workspace="${GITHUB_WORKSPACE:-}"
  local abs_root
  local abs_workspace
  [ -n "$workspace" ] || return 1
  # The stderr of `cd` is discarded because it restates the test: the `return 1`
  # is the answer, and the caller warns on it.
  abs_root=$(cd "$root" 2>/dev/null && pwd -P) || return 1
  abs_workspace=$(cd "$workspace" 2>/dev/null && pwd -P) || return 1

  if [ "$abs_root" = "$abs_workspace" ]; then
    printf '.\n'
  elif [[ "$abs_root" == "$abs_workspace/"* ]]; then
    printf '%s\n' "${abs_root#"$abs_workspace/"}"
  else
    return 1
  fi
}

normalize_config_path() {
  local path=$1
  local root="${INPUT_ROOT:-.}"

  path="${path#./}"
  root="${root#./}"

  if [[ "$path" = /* ]]; then
    local abs_root
    # The stderr of `cd` is discarded because a missing root is reported by the
    # command that needs it, not by this path normalizer.
    abs_root=$(cd "${INPUT_ROOT:-.}" 2>/dev/null && pwd -P)
    if [ -n "$abs_root" ] && [[ "$path" == "$abs_root/"* ]]; then
      path="${path#"$abs_root/"}"
    fi
  elif [ "$root" != "." ] && [[ "$path" == "$root/"* ]]; then
    path="${path#"$root/"}"
  fi

  printf '%s\n' "$path"
}

find_changed_fallow_config() {
  local changed_files_json=$1
  local explicit_config=""
  local matched=""
  local root_prefix=""

  if [ -n "${INPUT_CONFIG:-}" ]; then
    explicit_config=$(normalize_config_path "$INPUT_CONFIG")
  fi
  if [ -n "${INPUT_ROOT:-}" ] && [ "$INPUT_ROOT" != "." ]; then
    root_prefix="${INPUT_ROOT#./}/"
  fi

  matched=$(printf '%s' "$changed_files_json" | jq -r \
    --arg explicit "$explicit_config" --arg root "$root_prefix" '
    .[]
    | sub("^\\./"; "")
    | if ($root != "" and startswith($root)) then ltrimstr($root) else . end
    | select(
        . == ".fallowrc.json"
        or . == ".fallowrc.jsonc"
        or . == "fallow.toml"
        or . == ".fallow.toml"
        or ($explicit != "" and . == $explicit)
      )
  ' | head -1)

  if [ -n "$matched" ]; then
    printf '%s\n' "$matched"
    return 0
  fi

  return 1
}

# Rewrite a repository-relative unified diff (the `gh pr diff` output) to paths
# relative to the root <prefix>, and drop each file section outside the root.
# The `git diff --relative` diffs already have this form, so every consumer
# reads one path base. Only the header lines of a section are rewritten: a
# hunk line that looks like `+++ b/...` is content.
rebase_diff_to_root() {
  local prefix=$1
  awk -v p="$prefix" '
    function strip_lead(line, lead) {
      if (index(line, lead p) == 1) {
        return lead substr(line, length(lead p) + 1)
      }
      return ""
    }
    /^diff --git / {
      in_header = 1
      keep = 0
      if (index($0, "diff --git a/" p) == 1) {
        rest = substr($0, length("diff --git a/" p) + 1)
        pos = index(rest, " b/" p)
        if (pos > 0) {
          keep = 1
          print "diff --git a/" substr(rest, 1, pos - 1) " b/" substr(rest, pos + length(" b/" p))
        }
      }
      next
    }
    !keep { next }
    /^@@/ { in_header = 0 }
    in_header {
      n = split("--- a/|+++ b/|rename from |rename to |copy from |copy to ", leads, "|")
      for (i = 1; i <= n; i++) {
        out = strip_lead($0, leads[i])
        if (out != "") { print out; next }
      }
    }
    { print }
  '
}

# Write the PR diff from the GitHub API to $1, rebased onto the root. `gh pr
# diff` returns the unified-diff format of git, with repository-relative
# paths. Leaves $1 empty when the API call fails or the root is outside the
# workspace.
fallow_scope_fetch_pr_diff() {
  local dest=$1 err raw root
  : > "$dest"
  [ -n "${GH_TOKEN:-}" ] && [ -n "${PR_NUMBER:-}" ] && [ -n "${GH_REPO:-}" ] || return 0
  err=$(mktemp)
  raw=$(mktemp)
  gh pr diff "$PR_NUMBER" --repo "$GH_REPO" > "$raw" 2> "$err" || true
  replay_stderr_as_debug "$err" "gh pr diff"
  root=$(repo_relative_root || true)
  if [ -z "$root" ]; then
    echo "::warning::fallow: absolute root is outside GITHUB_WORKSPACE; the GitHub PR diff cannot be scoped safely." >&2
  elif [ "$root" = "." ]; then
    cp "$raw" "$dest"
  else
    rebase_diff_to_root "${root%/}/" < "$raw" > "$dest"
  fi
  rm -f "$raw"
}

# Refuse a ref or a path that Git could read as an option, or that could add a
# line to `$GITHUB_ENV`, before the step runs Git or writes a file command.
fallow_scope_validate_inputs() {
  local changed_since="${INPUT_CHANGED_SINCE:-}"
  local diff_file=$1

  if [ -z "$changed_since" ] && [ "${INPUT_AUTO_CHANGED_SINCE:-}" = "true" ] && \
     { [ "${EVENT_NAME:-}" = "pull_request" ] || [ "${EVENT_NAME:-}" = "pull_request_target" ]; }; then
    changed_since="${PR_BASE_SHA:-}"
  fi
  if [[ "$changed_since" = -* ]]; then
    printf '%s\n' "::error::changed-since must not begin with '-'"
    return 1
  fi
  if contains_ascii_control "$changed_since"; then
    printf '%s\n' "::error::changed-since must not contain ASCII control characters"
    return 1
  fi
  if contains_ascii_control "$diff_file"; then
    printf '%s\n' "::error::diff-file must not contain ASCII control characters"
    return 1
  fi
}

# Probe the capabilities of `fallow report`. HAS_NATIVE_REPORT gates the
# native annotate, summary and SARIF renders, and HAS_MUTATION_REPORT gates the
# `--mutation-report` and `--mutation-not-run` flags. The probe tests the
# capability, not the version.
fallow_scope_probe_report() {
  local help_file
  HAS_NATIVE_REPORT=false
  HAS_MUTATION_REPORT=false
  help_file=$(mktemp)
  if fallow report --help > "$help_file" 2>/dev/null; then
    HAS_NATIVE_REPORT=true
    if grep -q -- '--mutation-report' "$help_file"; then
      HAS_MUTATION_REPORT=true
    fi
  fi
  rm -f "$help_file"
}

# --- Make a merge base available on shallow clones ---
# `fallow --changed-since <base>` diffs against merge-base(<base>, HEAD). The
# default actions/checkout clone has depth 1, so the base commit and the merge
# base are not in the clone and the CLI exits 2 with "no merge base". Fetch the
# smallest history that gives a merge base:
#   1. The base commit itself (depth 1).
#   2. For a merge commit (the pull_request default checkout), its parents.
#      The first parent is the base branch tip, so this is usually enough.
#   3. Otherwise deepen both histories in bounded steps.
# The fetches use the checkout credentials, or none for a public fork PR. A
# failed fetch never fails the job: the changed-files cascade below still runs.
MERGE_BASE_DEEPEN_STEPS="50 100 200 400"

has_merge_base() {
  git merge-base "$1" HEAD >/dev/null 2>&1
}

fetch_for_merge_base() {
  local fetch_err
  fetch_err=$(mktemp)
  git fetch --no-tags "$@" 2> "$fetch_err" || true
  replay_stderr_as_debug "$fetch_err" "git fetch $*"
}

ensure_changed_since_merge_base() {
  local base=$1 base_sha head_sha parent_count step
  [ "$(git rev-parse --is-shallow-repository 2>/dev/null)" = "true" ] || return 0
  head_sha=$(git rev-parse --verify -q 'HEAD^{commit}') || return 0
  if ! git cat-file -e "${base}^{commit}" 2>/dev/null; then
    fetch_for_merge_base --depth=1 origin "$base"
  fi
  base_sha=$(git rev-parse --verify -q "${base}^{commit}") || return 0
  has_merge_base "$base_sha" && return 0
  # `git cat-file -p` lists every parent even when the clone grafts them away.
  parent_count=$(git cat-file -p "$head_sha" 2>/dev/null | awk '/^parent /{n++} /^$/{exit} END{print n+0}')
  if [ "${parent_count:-0}" -gt 1 ]; then
    fetch_for_merge_base --depth=2 origin "$head_sha"
    has_merge_base "$base_sha" && return 0
  fi
  for step in $MERGE_BASE_DEEPEN_STEPS; do
    fetch_for_merge_base --deepen="$step" origin "$head_sha" "$base_sha"
    has_merge_base "$base_sha" && return 0
  done
  echo "::warning::fallow: found no merge base between ${base_sha:0:7} and HEAD in this shallow clone. Use fetch-depth: 0 in actions/checkout."
}

# Resolve the scope, export it to this process and write it to $GITHUB_ENV.
# Exits 2 on invalid input, before any Git command or file-command write.
fallow_scope_main() {
  local user_diff_input="${INPUT_DIFF_FILE:-${FALLOW_DIFF_FILE:-}}"
  local scope_dir

  fallow_scope_validate_inputs "$user_diff_input" || exit 2

  ARTIFACTS_DIR="${INPUT_ARTIFACTS_DIR:-.}"
  if [ -z "$ARTIFACTS_DIR" ]; then
    ARTIFACTS_DIR="."
  fi
  fallow_scope_validate_artifacts_dir "$ARTIFACTS_DIR" || exit 2

  scope_dir=$(fallow_scope_dir)
  if ! mkdir -p "$scope_dir"; then
    echo "::error::Failed to create the scope directory: ${scope_dir}"
    exit 2
  fi
  CHANGED_FILES_FILE="$scope_dir/changed-files.json"
  AUTO_DIFF_FILE="$scope_dir/pr.diff"
  # A self-hosted runner without RUNNER_TEMP keeps the directory between jobs.
  rm -f "$CHANGED_FILES_FILE" "$AUTO_DIFF_FILE"

  fallow_scope_probe_report

  FALLOW_DIFF_FILE="$user_diff_input"
  CHANGED_FILES_UNAVAILABLE=false

  # --- Auto-detect changed-since in PR context ---

  AUTO_CHANGED_SINCE=false
  USER_DIFF_FILE=false
  [ -n "${FALLOW_DIFF_FILE:-}" ] && USER_DIFF_FILE=true

  if [ -z "${INPUT_CHANGED_SINCE:-}" ] && [ "${INPUT_AUTO_CHANGED_SINCE:-}" = "true" ] && \
     { [ "${EVENT_NAME:-}" = "pull_request" ] || [ "${EVENT_NAME:-}" = "pull_request_target" ]; } && \
     [ -n "${PR_BASE_SHA:-}" ]; then
    INPUT_CHANGED_SINCE="$PR_BASE_SHA"
    AUTO_CHANGED_SINCE=true
    echo "::notice::Auto-scoping analysis to files changed since PR base (${PR_BASE_SHA:0:7})"
  fi

  if [ -n "${INPUT_CHANGED_SINCE:-}" ]; then
    ensure_changed_since_merge_base "$INPUT_CHANGED_SINCE"
  fi

  # --- Pre-compute changed files list for downstream filtering ---
  # Downstream steps (mutation, comment, summary, annotations, review) need the
  # list of changed files to scope results to the PR. On shallow clones (the
  # default actions/checkout depth), git diff against the base SHA fails. The
  # list is computed here once, with git first and then the GitHub API.

  _CHANGED_JSON=""

  if [ -n "${INPUT_CHANGED_SINCE:-}" ]; then
    _ROOT="${INPUT_ROOT:-.}"
    _CHANGED_JSON=""

    # Try three-dot diff (precise: changes since merge-base, needs full history)
    _SCOPE_ERR=$(mktemp)
    _CHANGED_JSON=$(cd "$_ROOT" && git diff --name-only -z --relative "${INPUT_CHANGED_SINCE}...HEAD" -- . 2> "$_SCOPE_ERR" | jq -Rs 'split("\u0000") | map(select(length > 0))' || true)
    replay_stderr_as_debug "$_SCOPE_ERR" "git diff --name-only"

    # Shallow clone fallback: fetch the base commit and try two-dot diff
    if ! printf '%s' "$_CHANGED_JSON" | jq -e 'length > 0' >/dev/null 2>&1; then
      # The stderr of `git cat-file -e` is discarded because it is a pure
      # existence test and the fetch below is the answer to a missing commit.
      if ! git cat-file -e "${INPUT_CHANGED_SINCE}^{commit}" 2>/dev/null; then
        _FETCH_ERR=$(mktemp)
        git fetch --depth=1 origin "$INPUT_CHANGED_SINCE" 2> "$_FETCH_ERR" || true
        replay_stderr_as_debug "$_FETCH_ERR" "git fetch"
      fi
      _SCOPE_ERR=$(mktemp)
      _CHANGED_JSON=$(cd "$_ROOT" && git diff --name-only -z --relative "${INPUT_CHANGED_SINCE}" HEAD -- . 2> "$_SCOPE_ERR" | jq -Rs 'split("\u0000") | map(select(length > 0))' || true)
      replay_stderr_as_debug "$_SCOPE_ERR" "git diff --name-only"
    fi

    # Last resort: GitHub API (works regardless of clone depth).
    # Distinguish API failure (rate limit, 5xx, expired token, missing
    # permissions) from "no PR context" (no GH_TOKEN / PR_NUMBER / GH_REPO).
    # On API failure, set `changed_files_unavailable=true` so downstream
    # workflow steps can gate on the degraded state rather than silently
    # running unscoped analysis. The existing shallow-clone warning below
    # keeps its framing for the no-API-credentials case.
    if ! printf '%s' "$_CHANGED_JSON" | jq -e 'length > 0' >/dev/null 2>&1 \
        && [ -n "${GH_TOKEN:-}" ] && [ -n "${PR_NUMBER:-}" ] && [ -n "${GH_REPO:-}" ]; then
      _API_TMP=$(mktemp)
      _API_ERR=$(mktemp)
      trap 'rm -f "$_API_TMP" "$_API_ERR"' EXIT
      if gh api --paginate "repos/${GH_REPO}/pulls/${PR_NUMBER}/files" --jq '.[].filename | @json' \
           > "$_API_TMP" 2> "$_API_ERR"; then
        _CHANGED_JSON=$(jq -s '.' "$_API_TMP")
        if printf '%s' "$_CHANGED_JSON" | jq -e 'length > 0' >/dev/null 2>&1; then
          _API_ROOT=$(repo_relative_root || true)
          if [ -z "$_API_ROOT" ]; then
            echo "::warning::fallow: absolute root is outside GITHUB_WORKSPACE; GitHub API paths cannot be scoped safely." >&2
            CHANGED_FILES_UNAVAILABLE=true
            _CHANGED_JSON='[]'
          elif [ "$_API_ROOT" != "." ]; then
            # Strip root prefix; API returns repo-root-relative paths, fallow JSON uses root-relative.
            _CHANGED_JSON=$(printf '%s' "$_CHANGED_JSON" | jq -c --arg prefix "${_API_ROOT%/}/" \
              'map(select(startswith($prefix)) | ltrimstr($prefix))')
          fi
        fi
      else
        _STDERR_HEAD=$(head -3 "$_API_ERR" | tr '\n' ' ')
        echo "::warning::fallow: GitHub API call to list PR files failed; analysis will run against the full codebase, not just files changed in this PR. stderr: ${_STDERR_HEAD} Re-run the job to retry. If persistent, check 'gh auth status' and repo permissions." >&2
        CHANGED_FILES_UNAVAILABLE=true
      fi
    fi

    if printf '%s' "$_CHANGED_JSON" | jq -e 'length > 0' >/dev/null 2>&1; then
      printf '%s\n' "$_CHANGED_JSON" > "$CHANGED_FILES_FILE"
    else
      echo "::warning::Could not determine changed files for --changed-since scoping. Use fetch-depth: 0 in actions/checkout for best results."
    fi
  fi

  if is_dead_code_baseline_command \
      && printf '%s' "$_CHANGED_JSON" | jq -e 'length > 0' >/dev/null 2>&1; then
    CONFIG_SCOPE_TRIGGER=$(find_changed_fallow_config "$_CHANGED_JSON" || true)
    if [ -n "$CONFIG_SCOPE_TRIGGER" ]; then
      if [ "$AUTO_CHANGED_SINCE" = "true" ]; then
        if [ "$USER_DIFF_FILE" = "true" ]; then
          echo "::warning::fallow: '${CONFIG_SCOPE_TRIGGER}' changed, so auto changed-since scoping is disabled for dead-code baseline comparison. The explicit diff file remains active and may still hide baseline drift until an unscoped run." >&2
        else
          echo "::warning::fallow: dead-code baseline comparison is running unscoped because '${CONFIG_SCOPE_TRIGGER}' changed. Fallow config can change baseline membership; downstream PR filtering is disabled for this run." >&2
        fi
        INPUT_CHANGED_SINCE=""
        rm -f "$CHANGED_FILES_FILE" "$AUTO_DIFF_FILE"
      else
        echo "::warning::fallow: '${CONFIG_SCOPE_TRIGGER}' changed while dead-code baseline comparison is explicitly scoped. Fallow config can change baseline membership, so baseline drift may stay hidden until an unscoped run." >&2
      fi
    fi
  fi

  # --- Pre-compute unified diff for line-level scoping ---
  # `fallow mutation`, `fallow audit` and `fallow health` consume a unified
  # diff for line-overlap matching: a changed-scope mutation run plans mutants
  # only on added lines, and the `hot-path-touched` verdict fires only when an
  # added line falls inside a hot function's body. Mirrors the changed-files
  # cascade above (three-dot diff, shallow-clone fetch fallback, GitHub API
  # last resort) so behavior is consistent across checkout depths.
  #
  # Skip when the user already supplied `inputs.diff-file`: respect their
  # choice. Skip when there is no changed-since, since there is nothing to
  # scope against.
  if [ -n "${INPUT_CHANGED_SINCE:-}" ] && [ -z "${FALLOW_DIFF_FILE:-}" ]; then
    _ROOT="${INPUT_ROOT:-.}"
    _DIFF_PATH="$AUTO_DIFF_FILE"

    # Three-dot diff (precise: changes since merge-base, needs full history).
    _DIFF_ERR=$(mktemp)
    if (cd "$_ROOT" && git diff --unified=0 --relative "${INPUT_CHANGED_SINCE}...HEAD" -- .) > "$_DIFF_PATH" 2> "$_DIFF_ERR"; then
      :
    fi
    replay_stderr_as_debug "$_DIFF_ERR" "git diff --unified=0"

    # Shallow-clone fallback: fetch the base commit, retry two-dot diff.
    if [ ! -s "$_DIFF_PATH" ]; then
      # The stderr of `git cat-file -e` is discarded because it is a pure
      # existence test and the fetch below is the answer to a missing commit.
      if ! git cat-file -e "${INPUT_CHANGED_SINCE}^{commit}" 2>/dev/null; then
        _FETCH_ERR=$(mktemp)
        git fetch --depth=1 origin "$INPUT_CHANGED_SINCE" 2> "$_FETCH_ERR" || true
        replay_stderr_as_debug "$_FETCH_ERR" "git fetch"
      fi
      _DIFF_ERR=$(mktemp)
      (cd "$_ROOT" && git diff --unified=0 --relative "${INPUT_CHANGED_SINCE}" HEAD -- .) > "$_DIFF_PATH" 2> "$_DIFF_ERR" || true
      replay_stderr_as_debug "$_DIFF_ERR" "git diff --unified=0"
    fi

    # Last resort: GitHub API.
    if [ ! -s "$_DIFF_PATH" ]; then
      fallow_scope_fetch_pr_diff "$_DIFF_PATH"
    fi

    if [ -s "$_DIFF_PATH" ]; then
      FALLOW_DIFF_FILE="$_DIFF_PATH"
    else
      rm -f "$_DIFF_PATH"
      # Soft-degrade: line-level filtering disabled, the runtime-coverage
      # filter falls back to file-level via `--changed-since`. Emit a
      # machine-greppable warning so dashboards can alert on it without
      # parsing free-form text.
      echo "::warning::fallow: warning [shallow-clone]: could not produce unified diff for line-level hot-path scoping. Use fetch-depth: 0 in actions/checkout for line-precision."
    fi
  fi

  # A pull request without changed-since (auto-changed-since: false) still
  # gets the PR diff, so the comment and the review keep their line filter and
  # the changed mutation scope has its lines.
  if [ -z "${FALLOW_DIFF_FILE:-}" ] && [ -n "${PR_NUMBER:-}" ]; then
    fallow_scope_fetch_pr_diff "$AUTO_DIFF_FILE"
    if [ -s "$AUTO_DIFF_FILE" ]; then
      FALLOW_DIFF_FILE="$AUTO_DIFF_FILE"
    else
      rm -f "$AUTO_DIFF_FILE"
    fi
  fi

  FALLOW_SCOPE_CHANGED_SINCE="${INPUT_CHANGED_SINCE:-}"
  FALLOW_SCOPE_CHANGED_FILES_UNAVAILABLE="$CHANGED_FILES_UNAVAILABLE"
  FALLOW_ARTIFACTS_DIR="$ARTIFACTS_DIR"
  export FALLOW_SCOPE_CHANGED_SINCE FALLOW_SCOPE_CHANGED_FILES_UNAVAILABLE \
    FALLOW_ARTIFACTS_DIR HAS_NATIVE_REPORT HAS_MUTATION_REPORT
  if [ -n "$FALLOW_DIFF_FILE" ]; then
    export FALLOW_DIFF_FILE
  else
    unset FALLOW_DIFF_FILE
  fi
  if [ -f "$CHANGED_FILES_FILE" ]; then
    export FALLOW_CHANGED_FILES_FILE="$CHANGED_FILES_FILE"
  else
    unset FALLOW_CHANGED_FILES_FILE
  fi

  # Later composite steps see only $GITHUB_ENV, not exported shell variables.
  if [ -n "${GITHUB_ENV:-}" ]; then
    {
      printf '%s\n' \
        "FALLOW_SCOPE_CHANGED_SINCE=${FALLOW_SCOPE_CHANGED_SINCE}" \
        "FALLOW_SCOPE_CHANGED_FILES_UNAVAILABLE=${FALLOW_SCOPE_CHANGED_FILES_UNAVAILABLE}" \
        "FALLOW_ARTIFACTS_DIR=${FALLOW_ARTIFACTS_DIR}" \
        "HAS_NATIVE_REPORT=${HAS_NATIVE_REPORT}" \
        "HAS_MUTATION_REPORT=${HAS_MUTATION_REPORT}"
      [ -n "${FALLOW_DIFF_FILE:-}" ] && printf '%s\n' "FALLOW_DIFF_FILE=${FALLOW_DIFF_FILE}"
      [ -n "${FALLOW_CHANGED_FILES_FILE:-}" ] && printf '%s\n' "FALLOW_CHANGED_FILES_FILE=${FALLOW_CHANGED_FILES_FILE}"
    } >> "$GITHUB_ENV"
  fi
}

if [ "${BASH_SOURCE[0]}" = "$0" ]; then
  # Disable errexit: composite action runners inject -e via the shell
  # invocation, but this script handles errors explicitly with if-guards.
  set +e -o pipefail
  fallow_scope_main
  exit 0
fi
