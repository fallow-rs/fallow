#!/usr/bin/env bash
# Shared drift guard: every canonical dead-code IssueKind must surface in the
# jq summary / annotation / filter tables that are supposed to carry the full
# dead-code set. A new fallow IssueKind that is not wired into one of those
# surfaces would otherwise vanish silently from PR/MR output (the class of gap
# this guard exists to catch). It gates ALL such surfaces, not just
# summary-check.jq:
#
#   action/jq/summary-check.jq      (all)   GitHub dead-code summary table
#   action/jq/summary-combined.jq   (all)   GitHub combined Code-issues breakdown
#   action/jq/summary-audit.jq      (all)   GitHub audit dead_code_rows
#   action/jq/annotations-check.jq  (all)   GitHub ::warning annotations
#   action/jq/filter-changed.jq     (all)   per-changed-file filter + recount
#   ci/jq/summary-check.jq          (all)   GitLab dead-code summary table
#   ci/jq/summary-combined.jq       (all)   GitLab combined Code-issues breakdown
#   ci/jq/summary-audit.jq          (all)   GitLab audit dead_code_rows
#
# Sourced by both action/tests/run.sh and ci/tests/run.sh. Relies on the
# `pass` / `fail` helpers defined by the sourcing runner, plus `$GUARD_DIR`
# (the directory containing this script) being set by the caller.
#
# Canonical set: the dead-code issue-type ids, result keys, and count policy
# from `fallow schema` (issue_types[].command == "dead-code"). When the binary
# is unavailable or too old to expose result metadata, the fallback derives the
# same row contract from crates/types/src/issue_meta.rs `ISSUE_RESULT_META`.
#
# Non-dead-code kinds (security-*, code-duplication, complexity, coverage-gaps,
# feature-flag) are NOT summarised by these dead-code surfaces: they belong to
# the dupes / health / flags / security surfaces. They have no mapping entry
# and are reported as deliberate skips rather than failures.
#
# Per-surface expectation. Each surface declares whether it SHOULD carry every
# dead-code kind ("all") or a documented subset. A surface keyed "all" fails
# the moment any canonical kind is absent. A subset surface lists the kebab ids
# it is permitted to omit; those ids are skipped (reported, never failed) while
# every OTHER kind is still gated. This means a brand-new 38th IssueKind that
# fails to reach a subset surface STILL fails the guard; only the explicitly
# enumerated, documented omissions are tolerated.

FALLOW_DEAD_CODE_SCHEMA_ROWS_CACHE="__unset__"

fallow_dead_code_source_rows() {
  local repo_root source_file
  repo_root="$(cd "$GUARD_DIR/../.." && pwd)"
  source_file="$repo_root/crates/types/src/issue_meta.rs"
  if [ ! -f "$source_file" ]; then
    return 1
  fi

  awk '
    /IssueResultMeta[[:space:]]*\{/ {
      in_meta = 1
      code = ""
      result_key = ""
      counts = ""
      next
    }
    in_meta && /code: "/ {
      line = $0
      sub(/^.*code: "/, "", line)
      sub(/".*$/, "", line)
      code = line
      next
    }
    in_meta && /result_key: "/ {
      line = $0
      sub(/^.*result_key: "/, "", line)
      sub(/".*$/, "", line)
      result_key = line
      next
    }
    in_meta && /counts_in_total: / {
      line = $0
      sub(/^.*counts_in_total: /, "", line)
      sub(/,.*/, "", line)
      counts = line
      next
    }
    in_meta && /^[[:space:]]*\},/ {
      if (code != "" && result_key != "" && counts != "") {
        print code "\t" result_key "\t" counts
      }
      in_meta = 0
      next
    }
  ' "$source_file"
}

fallow_dead_code_schema_rows() {
  if [ "$FALLOW_DEAD_CODE_SCHEMA_ROWS_CACHE" != "__unset__" ]; then
    printf '%s\n' "$FALLOW_DEAD_CODE_SCHEMA_ROWS_CACHE"
    [ -n "$FALLOW_DEAD_CODE_SCHEMA_ROWS_CACHE" ]
    return
  fi

  local repo_root bin rows
  repo_root="$(cd "$GUARD_DIR/../.." && pwd)"
  bin="${FALLOW_BIN:-}"
  if [ -z "$bin" ]; then
    for cand in "$repo_root/target/debug/fallow" "$repo_root/target/release/fallow"; do
      if [ -x "$cand" ]; then bin="$cand"; break; fi
    done
  fi

  rows=""
  if [ -n "$bin" ] && [ -x "$bin" ] && command -v jq > /dev/null 2>&1; then
    rows="$("$bin" schema 2>/dev/null \
      | jq -r '
        [.issue_types[] | select(.command == "dead-code")] as $rows
        | if (($rows | length) > 0 and all($rows[]; has("result_key") and has("counts_in_total") and has("summary_label") and has("summary_docs_anchor"))) then
            $rows[] | [
              .id,
              (.result_key // ""),
              (.counts_in_total | tostring),
              (.summary_label // ""),
              (.summary_docs_anchor // "")
            ] | @tsv
          else
            empty
          end
      ' 2>/dev/null)"
  fi

  if [ -z "$rows" ]; then
    rows="$(fallow_dead_code_source_rows 2>/dev/null)"
  fi

  FALLOW_DEAD_CODE_SCHEMA_ROWS_CACHE="$rows"
  printf '%s\n' "$rows"
  [ -n "$rows" ]
}

issuekind_schema_field() {
  local id="$1" field="$2" rows row_id row_key row_counts row_label row_anchor
  rows="$(fallow_dead_code_schema_rows)" || return 1
  while IFS=$'\t' read -r row_id row_key row_counts row_label row_anchor; do
    [ -z "$row_id" ] && continue
    if [ "$row_id" != "$id" ]; then
      continue
    fi
    case "$field" in
      result_key) printf '%s\n' "$row_key" ;;
      counts_in_total) printf '%s\n' "$row_counts" ;;
      summary_label) printf '%s\n' "$row_label" ;;
      summary_docs_anchor) printf '%s\n' "$row_anchor" ;;
      *) return 1 ;;
    esac
    return 0
  done <<< "$rows"
  return 1
}

# Legacy kebab-id -> summary-check.jq JSON key fallback used only when
# `fallow schema` is unavailable. Irregular pluralisation makes mechanical
# conversion unsafe, so this remains explicit for the fallback path.
issuekind_json_key_fallback() {
  case "$1" in
    unused-file) echo "unused_files" ;;
    unused-export) echo "unused_exports" ;;
    unused-type) echo "unused_types" ;;
    private-type-leak) echo "private_type_leaks" ;;
    unused-dependency) echo "unused_dependencies" ;;
    unused-dev-dependency) echo "unused_dev_dependencies" ;;
    unused-optional-dependency) echo "unused_optional_dependencies" ;;
    type-only-dependency) echo "type_only_dependencies" ;;
    test-only-dependency) echo "test_only_dependencies" ;;
    dev-dependency-in-production) echo "dev_dependencies_in_production" ;;
    unused-enum-member) echo "unused_enum_members" ;;
    unused-class-member) echo "unused_class_members" ;;
    unused-store-member) echo "unused_store_members" ;;
    unresolved-import) echo "unresolved_imports" ;;
    unlisted-dependency) echo "unlisted_dependencies" ;;
    duplicate-export) echo "duplicate_exports" ;;
    circular-dependency) echo "circular_dependencies" ;;
    re-export-cycle) echo "re_export_cycles" ;;
    boundary-violation) echo "boundary_violations" ;;
    boundary-coverage) echo "boundary_coverage_violations" ;;
    boundary-call-violation) echo "boundary_call_violations" ;;
    policy-violation) echo "policy_violations" ;;
    stale-suppression) echo "stale_suppressions" ;;
    unused-catalog-entry) echo "unused_catalog_entries" ;;
    empty-catalog-group) echo "empty_catalog_groups" ;;
    unresolved-catalog-reference) echo "unresolved_catalog_references" ;;
    unused-dependency-override) echo "unused_dependency_overrides" ;;
    misconfigured-dependency-override) echo "misconfigured_dependency_overrides" ;;
    invalid-client-export) echo "invalid_client_exports" ;;
    mixed-client-server-barrel) echo "mixed_client_server_barrels" ;;
    misplaced-directive) echo "misplaced_directives" ;;
    unprovided-inject) echo "unprovided_injects" ;;
    unrendered-component) echo "unrendered_components" ;;
    unused-component-prop) echo "unused_component_props" ;;
    unused-component-emit) echo "unused_component_emits" ;;
    unused-component-input) echo "unused_component_inputs" ;;
    unused-component-output) echo "unused_component_outputs" ;;
    unused-svelte-event) echo "unused_svelte_events" ;;
    unused-server-action) echo "unused_server_actions" ;;
    unused-load-data-key) echo "unused_load_data_keys" ;;
    route-collision) echo "route_collisions" ;;
    dynamic-segment-name-conflict) echo "dynamic_segment_name_conflicts" ;;
    *) return 1 ;;
  esac
}

issuekind_json_key() {
  local key counts
  if key="$(issuekind_schema_field "$1" result_key)"; then
    counts="$(issuekind_schema_field "$1" counts_in_total || true)"
    if [ "$counts" = "true" ] && [ -n "$key" ]; then
      printf '%s\n' "$key"
      return 0
    fi
    # `missing-suppression-reason` is a distinct rule identity for SARIF,
    # CodeClimate, severity, and audit attribution, but the JSON payload keeps
    # those rows in the shared stale_suppressions result array.
    if [ "$1" = "missing-suppression-reason" ]; then
      printf '%s\n' "stale_suppressions"
      return 0
    fi
    return 1
  fi
  issuekind_json_key_fallback "$1"
}

# Map a canonical kebab id to the VS Code diagnostic CODE that filters it in
# DIAGNOSTIC_CATEGORIES. Mostly identity (the diagnostic code equals the rule
# id), except the boundary family: the LSP deliberately emits boundary-coverage
# and boundary-call-violation findings under the single `boundary-violation`
# diagnostic code, so one catalog entry filters all three and the two sub-kinds
# do not get their own. Used only by the VS Code catalog check.
issuekind_diagnostic_code() {
  case "$1" in
    boundary-coverage | boundary-call-violation) echo "boundary-violation" ;;
    *) echo "$1" ;;
  esac
}

# Resolve the canonical dead-code id list. Prefer `fallow schema` so the set is
# command-tagged; fall back to the checked-in result metadata table so shell-only
# guard runs still gate every counted serialized result key.
fallow_dead_code_ids() {
  local rows row_id _row_key _row_counts _row_label _row_anchor
  if rows="$(fallow_dead_code_schema_rows)" && [ -n "$rows" ]; then
    echo "__SOURCE__ fallow schema or issue_meta.rs result metadata" >&2
    while IFS=$'\t' read -r row_id _row_key _row_counts _row_label _row_anchor; do
      [ -n "$row_id" ] && printf '%s\n' "$row_id"
    done <<< "$rows"
    return 0
  fi
  return 1
}

# Does the JSON result key appear in this jq source? Surfaces reference keys in
# two distinct ways and the guard must recognise BOTH:
#   * quoted string token   -> "unused_files"      (summary-check.jq table_row)
#   * jq member access       -> .unused_files,
#                               .check.unused_files,
#                               .dead_code.unused_files[]?   (combined/audit/
#                               annotations/filter surfaces)
# The member-access form is matched as a literal `.` immediately followed by the
# key, bounded so `.unused_file` never matches `.unused_files`. The trailing
# bound also accepts end-of-line.
issuekind_strip_jq_comments() {
  awk '
    {
      out = ""
      in_string = 0
      escaped = 0
      for (i = 1; i <= length($0); i++) {
        char = substr($0, i, 1)
        if (in_string) {
          out = out char
          if (escaped) {
            escaped = 0
          } else if (char == "\\") {
            escaped = 1
          } else if (char == "\"") {
            in_string = 0
          }
          continue
        }
        if (char == "\"") {
          in_string = 1
          out = out char
          continue
        }
        if (char == "#") {
          break
        }
        out = out char
      }
      print out
    }
  '
}

issuekind_key_present() {
  local jq_src="$1" key="$2" stripped
  stripped="$(printf '%s' "$jq_src" | issuekind_strip_jq_comments)"
  grep -qE "\"${key}\"|\.${key}([^A-Za-z0-9_]|$)" <<< "$stripped"
}

issuekind_summary_table_row_present() {
  local jq_src="$1" key="$2" label="$3" anchor="$4" stripped expected
  stripped="$(printf '%s' "$jq_src" | issuekind_strip_jq_comments)"
  expected="table_row(\"$label\"; \"$key\"; \"$anchor\")"
  grep -qF "$expected" <<< "$stripped"
}

# Is <kebab-id> in the space-separated allowed-omission list <allow>? Used to
# tolerate the documented per-surface subset exceptions.
issuekind_in_allowlist() {
  local id="$1" allow="$2" entry
  for entry in $allow; do
    [ "$entry" = "$id" ] && return 0
  done
  return 1
}

# Run the guard against one dead-code surface.
#   $1 label        human label for pass/fail output
#   $2 jq_file      path to the jq surface to scan
#   $3 expectation  "all" (default) to require every canonical dead-code kind,
#                   or "allow:<kebab-id> <kebab-id> ..." to permit a documented
#                   subset to be omitted while still gating every other kind.
assert_issuekind_summary_coverage() {
  local label="$1" jq_file="$2" expectation="${3:-all}"
  local jq_src ids id key allow="" allowed=() skipped=() missing=() unmapped=()

  case "$expectation" in
    all) ;;
    allow:*) allow="${expectation#allow:}" ;;
    *)
      fail "$label: guard expectation is valid" \
        "unknown expectation '$expectation' (use 'all' or 'allow:<ids>')"
      return
      ;;
  esac

  if [ ! -f "$jq_file" ]; then
    fail "$label: surface file present" "missing file: $jq_file"
    return
  fi
  jq_src="$(cat "$jq_file")"
  ids="$(fallow_dead_code_ids 2>/dev/null)"

  if [ -z "$ids" ]; then
    fail "$label: canonical IssueKind set resolved" "no dead-code ids derived"
    return
  fi

  while IFS= read -r id; do
    [ -z "$id" ] && continue
    if ! key="$(issuekind_json_key "$id")"; then
      # Non-dead-code kinds (security, dupes, health, flags) live on other
      # surfaces; only the suppress.rs fallback yields them. Skip, don't fail.
      #
      # prop-drilling / thin-wrapper / duplicate-prop-shape are command-tagged
      # dead-code, but they are opt-in (default-off) React/Preact advisory
      # health signals surfaced ONLY in the CLI human report + raw JSON. They
      # are deliberately NOT emitted by the LSP (no DIAGNOSTIC_ISSUE_TYPES
      # entry) and not carried by the PR-summary jq surfaces, so they are
      # classified here like complexity / coverage-gaps rather than gated for
      # surface presence.
      case "$id" in
        security-*|code-duplication|complexity|coverage-gaps|feature-flag|prop-drilling|thin-wrapper|duplicate-prop-shape)
          skipped+=("$id") ;;
        *)
          # A dead-code id with no mapping is a guard gap: the mapping table
          # must grow with the enum.
          unmapped+=("$id") ;;
      esac
      continue
    fi
    if issuekind_key_present "$jq_src" "$key"; then
      continue
    fi
    # Key absent. If this surface is permitted to omit it, record it as an
    # allowed-and-documented exception; otherwise it is a real drift miss.
    if [ -n "$allow" ] && issuekind_in_allowlist "$id" "$allow"; then
      allowed+=("$key")
    else
      missing+=("$id -> $key")
    fi
  done <<< "$ids"

  if [ "${#skipped[@]}" -gt 0 ]; then
    echo "    (skipped non-dead-code kinds, not carried by this surface: ${skipped[*]})"
  fi
  if [ "${#allowed[@]}" -gt 0 ]; then
    echo "    (documented omissions tolerated for this surface: ${allowed[*]})"
  fi

  if [ "${#unmapped[@]}" -gt 0 ]; then
    fail "$label: every dead-code IssueKind has a JSON key mapping" \
      "no mapping for: ${unmapped[*]} (add to issuekind_json_key)"
    return
  fi

  if [ "${#missing[@]}" -gt 0 ]; then
    fail "$label: every gated dead-code IssueKind appears in the surface" \
      "absent JSON key(s): ${missing[*]}"
    return
  fi

  pass "$label: every gated dead-code IssueKind appears in the surface"
}

# Run the summary-table contract guard against standalone dead-code summary
# renderers. Unlike the broad surface coverage guard above, this checks the
# human-visible table label and docs anchor for each counted result row.
assert_issuekind_summary_table_contract() {
  local label="$1" jq_file="$2"
  local jq_src rows ids id key counts summary_label summary_anchor missing=() skipped=()

  if [ ! -f "$jq_file" ]; then
    fail "$label: surface file present" "missing file: $jq_file"
    return
  fi
  jq_src="$(cat "$jq_file")"
  rows="$(fallow_dead_code_schema_rows 2>/dev/null || true)"
  if ! grep -q $'\t.*\t.*\t' <<< "$rows"; then
    echo "    (skipped summary label contract: schema metadata unavailable)"
    return
  fi
  ids="$(fallow_dead_code_ids 2>/dev/null)"

  if [ -z "$ids" ]; then
    fail "$label: canonical IssueKind set resolved" "no dead-code ids derived"
    return
  fi

  while IFS= read -r id; do
    [ -z "$id" ] && continue
    if ! key="$(issuekind_schema_field "$id" result_key)"; then
      skipped+=("$id")
      continue
    fi
    counts="$(issuekind_schema_field "$id" counts_in_total || true)"
    if [ "$counts" != "true" ] || [ -z "$key" ]; then
      skipped+=("$id")
      continue
    fi
    summary_label="$(issuekind_schema_field "$id" summary_label || true)"
    summary_anchor="$(issuekind_schema_field "$id" summary_docs_anchor || true)"
    if [ -z "$summary_label" ] || [ -z "$summary_anchor" ]; then
      missing+=("$id -> missing summary metadata")
      continue
    fi
    if ! issuekind_summary_table_row_present "$jq_src" "$key" "$summary_label" "$summary_anchor"; then
      missing+=("$id -> table_row(\"$summary_label\"; \"$key\"; \"$summary_anchor\")")
    fi
  done <<< "$ids"

  if [ "${#skipped[@]}" -gt 0 ]; then
    echo "    (skipped rows not counted by standalone summaries: ${skipped[*]})"
  fi

  if [ "${#missing[@]}" -gt 0 ]; then
    fail "$label: summary labels and docs anchors match registry" \
      "missing or mismatched row(s): ${missing[*]}"
    return
  fi

  pass "$label: summary labels and docs anchors match registry"
}

# Assert the VS Code extension's DIAGNOSTIC_CATEGORIES, the LSP diagnostic-code
# catalog that drives the mute filter, carries every canonical dead-code
# IssueKind that actually has an LSP diagnostic. DIAGNOSTIC_CATEGORIES keys on
# the singular kebab rule-id (e.g. `code: "unused-file"`), which equals the
# `fallow schema` issue-type id for LSP-visible rows. Same single source as the
# jq surfaces, with documented skips for rule identities that are SARIF /
# CodeClimate / audit-only and deliberately have `lsp: false` in IssueKindMeta.
# This closes the last LSP surface a new diagnostic kind could silently miss.
# The catalog is provider-agnostic, so this runs once (from the GitHub runner).
assert_issuekind_vscode_category_coverage() {
  local label="$1" ts_file="$2"
  local ts_src ids id code key wiring_file wiring_src missing=() missing_wiring=() skipped=() unmapped=()

  if [ ! -f "$ts_file" ]; then
    fail "$label: surface file present" "missing file: $ts_file"
    return
  fi
  ts_src="$(cat "$ts_file")"
  wiring_file="$(cd "$GUARD_DIR/../.." && pwd)/editors/vscode/test/deadCodeKindDrift.test.ts"
  if [ ! -f "$wiring_file" ]; then
    fail "$label: VS Code wiring test present" "missing file: $wiring_file"
    return
  fi
  wiring_src="$(cat "$wiring_file")"
  ids="$(fallow_dead_code_ids 2>/dev/null)"

  if [ -z "$ids" ]; then
    fail "$label: canonical IssueKind set resolved" "no dead-code ids derived"
    return
  fi

  while IFS= read -r id; do
    [ -z "$id" ] && continue
    # Reuse the json-key map purely to classify dead-code vs non-dead-code: a
    # successful mapping means a dead-code kind (must be in the catalog); a
    # failed one is a non-dead-code kind carried by other catalogs.
    if ! key="$(issuekind_json_key "$id")"; then
      # prop-drilling / thin-wrapper / duplicate-prop-shape are dead-code-tagged
      # but CLI/JSON-only advisory health signals the LSP does not emit, so they
      # carry no diagnostic CODE in DIAGNOSTIC_CATEGORIES (same treatment as the
      # complexity / coverage-gaps non-catalog kinds).
      case "$id" in
        security-* | code-duplication | complexity | coverage-gaps | feature-flag | prop-drilling | thin-wrapper | duplicate-prop-shape)
          skipped+=("$id") ;;
        *) unmapped+=("$id") ;;
      esac
      continue
    fi
    if ! printf '%s' "$wiring_src" | grep -qE "field: \"${key}\""; then
      missing_wiring+=("$id -> $key")
    fi
    if [ "$id" = "missing-suppression-reason" ]; then
      skipped+=("$id")
      continue
    fi
    # The catalog carries each kind under its diagnostic CODE (the boundary
    # family collapses to boundary-violation); the quotes bound the match so
    # "unused-file" never matches "unused-files".
    code="$(issuekind_diagnostic_code "$id")"
    if printf '%s' "$ts_src" | grep -qE "\"${code}\""; then
      continue
    fi
    missing+=("$id -> code \"$code\"")
  done <<< "$ids"

  if [ "${#skipped[@]}" -gt 0 ]; then
    echo "    (skipped non-dead-code kinds, carried by other catalogs: ${skipped[*]})"
  fi

  if [ "${#unmapped[@]}" -gt 0 ]; then
    fail "$label: every dead-code IssueKind has a JSON key mapping" \
      "no mapping for: ${unmapped[*]} (add to issuekind_json_key)"
    return
  fi

  if [ "${#missing[@]}" -gt 0 ]; then
    fail "$label: every dead-code IssueKind appears in DIAGNOSTIC_CATEGORIES" \
      "absent diagnostic code(s): ${missing[*]} (add to editors/vscode/src/generated/issue-types.ts via codegen)"
    return
  fi

  if [ "${#missing_wiring[@]}" -gt 0 ]; then
    fail "$label: every counted dead-code result key is covered by VS Code wiring tests" \
      "absent result key(s): ${missing_wiring[*]} (add to editors/vscode/test/deadCodeKindDrift.test.ts)"
    return
  fi

  pass "$label: every dead-code IssueKind appears in DIAGNOSTIC_CATEGORIES"
}
