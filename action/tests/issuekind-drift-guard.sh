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
# Canonical set: the dead-code issue-type ids from `fallow schema`
# (issue_types[].command == "dead-code"). When the binary is unavailable the
# fallback derives the kebab ids from crates/types/src/suppress.rs
# `issue_kind_to_kebab` instead. Either source is mapped to the snake_case
# plural JSON result key the surfaces reference.
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

# Deterministic kebab-id -> summary-check.jq JSON key. Irregular pluralisation
# (catalog-entry -> catalog_entries, boundary-coverage -> *_violations) makes a
# mechanical s/-/_/+pluralise unsafe, so the mapping is explicit. A dead-code id
# with no entry here FAILS the guard, forcing this table to grow in lockstep
# with the IssueKind enum.
issuekind_json_key() {
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
    unused-server-action) echo "unused_server_actions" ;;
    unused-load-data-key) echo "unused_load_data_keys" ;;
    route-collision) echo "route_collisions" ;;
    dynamic-segment-name-conflict) echo "dynamic_segment_name_conflicts" ;;
    *) return 1 ;;
  esac
}

# Resolve the canonical dead-code id list. Prefer `fallow schema` so the set is
# command-tagged; fall back to suppress.rs kebab ids (non-dead-code kinds drop
# out at the mapping step, which is the desired conservative behaviour).
fallow_dead_code_ids() {
  local repo_root bin
  repo_root="$(cd "$GUARD_DIR/../.." && pwd)"
  bin="${FALLOW_BIN:-}"
  if [ -z "$bin" ]; then
    for cand in "$repo_root/target/debug/fallow" "$repo_root/target/release/fallow"; do
      if [ -x "$cand" ]; then bin="$cand"; break; fi
    done
  fi
  if [ -n "$bin" ] && [ -x "$bin" ] && command -v jq > /dev/null 2>&1; then
    local ids
    ids="$("$bin" schema 2>/dev/null \
      | jq -r '.issue_types[] | select(.command == "dead-code") | .id' 2>/dev/null)"
    if [ -n "$ids" ]; then
      echo "__SOURCE__ fallow schema ($bin)" >&2
      printf '%s\n' "$ids"
      return 0
    fi
  fi
  # Fallback: kebab ids from issue_kind_to_kebab in suppress.rs.
  echo "__SOURCE__ suppress.rs issue_kind_to_kebab (binary unavailable)" >&2
  grep -oE '=> "[a-z-]+",' "$repo_root/crates/types/src/suppress.rs" \
    | sed -E 's/=> "//; s/",//' | sort -u
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
issuekind_key_present() {
  local jq_src="$1" key="$2"
  printf '%s' "$jq_src" | grep -qE "\"${key}\"|\.${key}([^A-Za-z0-9_]|$)"
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
      case "$id" in
        security-*|code-duplication|complexity|coverage-gaps|feature-flag)
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
