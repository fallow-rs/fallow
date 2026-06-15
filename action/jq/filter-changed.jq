# Filter fallow results to only include issues in changed files.
# Usage: jq --argjson changed '["src/a.ts","src/b.ts"]' -f filter-changed.jq results.json
#
# Handles single-command output (check, dupes, health) and combined output.
# Paths in $changed must be relative to the project root (matching fallow JSON paths).

def in_changed: . as $path | $changed | any(. == $path);

# Filter dead-code (check) results and recalculate total_issues.
# Dependency-level issues (unused_dependencies, unused_dev_dependencies, unused_optional_dependencies,
# type_only_dependencies) are intentionally NOT filtered — they are project-wide concerns not
# attributable to individual changed files. They are still included in total_issues.
def filter_check:
  (if .unused_files         then .unused_files         |= map(select(.path | in_changed))      else . end) |
  (if .unused_exports       then .unused_exports       |= map(select(.path | in_changed))      else . end) |
  (if .unused_types         then .unused_types         |= map(select(.path | in_changed))      else . end) |
  (if .private_type_leaks   then .private_type_leaks   |= map(select(.path | in_changed))      else . end) |
  (if .unused_enum_members  then .unused_enum_members  |= map(select(.path | in_changed))      else . end) |
  (if .unused_class_members then .unused_class_members |= map(select(.path | in_changed))      else . end) |
  (if .unused_store_members then .unused_store_members |= map(select(.path | in_changed))      else . end) |
  (if .unresolved_imports   then .unresolved_imports   |= map(select(.path | in_changed))      else . end) |
  (if .unlisted_dependencies then
    .unlisted_dependencies |= map(select(.imported_from | any(.path | in_changed)))
  else . end) |
  (if .duplicate_exports then
    .duplicate_exports |= (map(.locations |= map(select(.path | in_changed))) | map(select(.locations | length >= 2)))
  else . end) |
  (if .circular_dependencies then
    .circular_dependencies |= map(select(.files | any(in_changed)))
  else . end) |
  (if .re_export_cycles then
    .re_export_cycles |= map(select(.files | any(in_changed)))
  else . end) |
  (if .boundary_violations then
    .boundary_violations |= map(select(.from_path | in_changed))
  else . end) |
  (if .boundary_coverage_violations then
    .boundary_coverage_violations |= map(select(.path | in_changed))
  else . end) |
  (if .boundary_call_violations then
    .boundary_call_violations |= map(select(.path | in_changed))
  else . end) |
  (if .policy_violations then
    .policy_violations |= map(select(.path | in_changed))
  else . end) |
  (if .stale_suppressions then
    .stale_suppressions |= map(select(.path | in_changed))
  else . end) |
  (if .unresolved_catalog_references then
    .unresolved_catalog_references |= map(select(.path | in_changed))
  else . end) |
  (if .empty_catalog_groups then
    .empty_catalog_groups |= map(select(.path | in_changed))
  else . end) |
  (if .unused_dependency_overrides then
    .unused_dependency_overrides |= map(select(.path | in_changed))
  else . end) |
  (if .misconfigured_dependency_overrides then
    .misconfigured_dependency_overrides |= map(select(.path | in_changed))
  else . end) |
  (if .invalid_client_exports then
    .invalid_client_exports |= map(select(.path | in_changed))
  else . end) |
  (if .mixed_client_server_barrels then
    .mixed_client_server_barrels |= map(select(.path | in_changed))
  else . end) |
  (if .misplaced_directives then
    .misplaced_directives |= map(select(.path | in_changed))
  else . end) |
  (if .route_collisions then
    .route_collisions |= map(select(.path | in_changed))
  else . end) |
  (if .dynamic_segment_name_conflicts then
    .dynamic_segment_name_conflicts |= map(select(.path | in_changed))
  else . end) |
  (if .unused_server_actions then
    .unused_server_actions |= map(select(.path | in_changed))
  else . end) |
  (if .unrendered_components then
    .unrendered_components |= map(select(.path | in_changed))
  else . end) |
  (if .unused_component_props then
    .unused_component_props |= map(select(.path | in_changed))
  else . end) |
  (if .unused_component_emits then
    .unused_component_emits |= map(select(.path | in_changed))
  else . end) |
  (if .unprovided_injects then
    .unprovided_injects |= map(select(.path | in_changed))
  else . end) |
  (if .unused_load_data_keys then
    .unused_load_data_keys |= map(select(.path | in_changed))
  else . end) |
  # Recalculate total_issues from filtered arrays
  (if .total_issues != null then
    .total_issues = (
      (.unused_files // [] | length) +
      (.unused_exports // [] | length) +
      (.unused_types // [] | length) +
      (.private_type_leaks // [] | length) +
      (.unused_dependencies // [] | length) +
      (.unused_dev_dependencies // [] | length) +
      (.unused_optional_dependencies // [] | length) +
      (.unused_enum_members // [] | length) +
      (.unused_class_members // [] | length) +
      (.unused_store_members // [] | length) +
      (.unresolved_imports // [] | length) +
      (.unlisted_dependencies // [] | length) +
      (.duplicate_exports // [] | length) +
      (.circular_dependencies // [] | length) +
      (.re_export_cycles // [] | length) +
      (.boundary_violations // [] | length) +
      (.boundary_coverage_violations // [] | length) +
      (.boundary_call_violations // [] | length) +
      (.policy_violations // [] | length) +
      (.type_only_dependencies // [] | length) +
      (.stale_suppressions // [] | length) +
      (.unused_catalog_entries // [] | length) +
      (.empty_catalog_groups // [] | length) +
      (.unresolved_catalog_references // [] | length) +
      (.unused_dependency_overrides // [] | length) +
      (.misconfigured_dependency_overrides // [] | length) +
      (.invalid_client_exports // [] | length) +
      (.mixed_client_server_barrels // [] | length) +
      (.misplaced_directives // [] | length) +
      (.route_collisions // [] | length) +
      (.dynamic_segment_name_conflicts // [] | length) +
      (.unused_server_actions // [] | length) +
      (.unrendered_components // [] | length) +
      (.unused_component_props // [] | length) +
      (.unused_component_emits // [] | length) +
      (.unprovided_injects // [] | length) +
      (.unused_load_data_keys // [] | length)
    )
  else . end);

# Filter duplication results and recompute stats
def filter_dupes:
  (if .clone_groups then
    .clone_groups |= map(select(.instances | any(.file | in_changed)))
  else . end) |
  (if .stats then
    .stats.clone_groups = (.clone_groups // [] | length) |
    .stats.clone_instances = ([(.clone_groups // [])[] | .instances | length] | add // 0) |
    .stats.duplicated_lines = ([(.clone_groups // [])[] | (.line_count // 0)] | add // 0) |
    .stats.files_with_clones = ([(.clone_groups // [])[] | .instances[].file] | unique | length)
  else . end) |
  (if .clone_families then
    .clone_families |= map(.groups |= map(select(.instances | any(.file | in_changed)))) |
    .clone_families |= map(select(.groups | length > 0))
  else . end);

# Filter health results
def filter_health:
  (if .findings then
    .findings |= map(select(.path | in_changed))
  else . end) |
  (if .summary then
    .summary.functions_above_threshold = (.findings // [] | length)
  else . end) |
  (if .file_scores then
    .file_scores |= map(select(.path | in_changed))
  else . end) |
  (if .hotspots then
    .hotspots |= map(select(.path | in_changed))
  else . end) |
  (if .targets then
    .targets |= map(select(.path | in_changed))
  else . end);

# Detect format and apply appropriate filter
if .check or .dupes or .health then
  # Combined format
  (if .check  then .check  |= filter_check  else . end) |
  (if .dupes  then .dupes  |= filter_dupes  else . end) |
  (if .health then .health |= filter_health else . end)
elif .total_issues != null then
  # Check (dead-code) format
  filter_check
elif .clone_groups then
  # Dupes format
  filter_dupes
elif .findings then
  # Health format
  filter_health
else
  # Unknown format — pass through unchanged
  .
end
