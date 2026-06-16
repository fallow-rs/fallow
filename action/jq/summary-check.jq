def docs(anchor): "https://docs.fallow.tools/explanations/dead-code#" + anchor;
def workspace_context:
  if ((.used_in_workspaces // []) | length) > 0 then
    (.used_in_workspaces | map("`\(.)`") | join(", "))
  else
    ""
  end;

def table_row(name; key; anchor):
  (.[key] | length) as $n |
  if $n > 0 then "| [\(name)](\(docs(anchor))) | \($n) |" else empty end;

def section(name; key; header; fmt):
  (.[key] | length) as $n |
  if $n > 0 then
    "\n<details><summary><strong>\(name) (\($n))</strong></summary>\n\n" +
    header +
    ([.[key][:25][] | fmt] | join("\n")) +
    (if $n > 25 then "\n\n> \($n - 25) more \u2014 run `fallow` locally for the full list" else "" end) +
    "\n\n</details>\n"
  else "" end;

if .total_issues == 0 then
  "# Fallow Analysis\n\n" +
  "> [!NOTE]\n> **No issues found** \u00b7 \(.elapsed_ms)ms\n\n" +
  "All exports are used, all dependencies are declared, and no issues were detected."
else
  "# Fallow Analysis\n\n" +
  "> [!WARNING]\n> **\(.total_issues) issues** found \u00b7 \(.elapsed_ms)ms\n\n" +
  "| Category | Count |\n|----------|------:|\n" +
  ([
    table_row("Unused files"; "unused_files"; "unused-files"),
    table_row("Unused exports"; "unused_exports"; "unused-exports"),
    table_row("Unused types"; "unused_types"; "unused-types"),
    table_row("Private type leaks"; "private_type_leaks"; "private-type-leaks"),
    table_row("Unused dependencies"; "unused_dependencies"; "unused-dependencies"),
    table_row("Unused devDependencies"; "unused_dev_dependencies"; "unused-dependencies"),
    table_row("Unused optionalDependencies"; "unused_optional_dependencies"; "unused-dependencies"),
    table_row("Unused enum members"; "unused_enum_members"; "unused-enum-members"),
    table_row("Unused class members"; "unused_class_members"; "unused-class-members"),
    table_row("Unused store members"; "unused_store_members"; "unused-store-members"),
    table_row("Unresolved imports"; "unresolved_imports"; "unresolved-imports"),
    table_row("Unlisted dependencies"; "unlisted_dependencies"; "unlisted-dependencies"),
    table_row("Duplicate exports"; "duplicate_exports"; "duplicate-exports"),
    table_row("Circular dependencies"; "circular_dependencies"; "circular-dependencies"),
    table_row("Re-export cycles"; "re_export_cycles"; "re-export-cycles"),
    table_row("Boundary violations"; "boundary_violations"; "boundary-violations"),
    table_row("Boundary coverage"; "boundary_coverage_violations"; "boundary-violations"),
    table_row("Boundary calls"; "boundary_call_violations"; "boundary-violations"),
    table_row("Policy violations"; "policy_violations"; "policy-violations"),
    table_row("Invalid client exports"; "invalid_client_exports"; "invalid-client-exports"),
    table_row("Mixed client/server barrels"; "mixed_client_server_barrels"; "mixed-client-server-barrels"),
    table_row("Misplaced directives"; "misplaced_directives"; "misplaced-directives"),
    table_row("Unused server actions"; "unused_server_actions"; "unused-server-action"),
    table_row("Route collisions"; "route_collisions"; "route-collisions"),
    table_row("Dynamic segment conflicts"; "dynamic_segment_name_conflicts"; "dynamic-segment-name-conflicts"),
    table_row("Unrendered components"; "unrendered_components"; "unrendered-component"),
    table_row("Unused component props"; "unused_component_props"; "unused-component-prop"),
    table_row("Unused component emits"; "unused_component_emits"; "unused-component-emit"),
    table_row("Unused component inputs"; "unused_component_inputs"; "unused-component-input"),
    table_row("Unused component outputs"; "unused_component_outputs"; "unused-component-output"),
    table_row("Unprovided injects"; "unprovided_injects"; "unprovided-inject"),
    table_row("Unused load data keys"; "unused_load_data_keys"; "unused-load-data-key"),
    table_row("Type-only dependencies"; "type_only_dependencies"; "type-only-dependencies"),
    table_row("Test-only dependencies"; "test_only_dependencies"; "test-only-dependencies"),
    table_row("Stale suppressions"; "stale_suppressions"; "stale-suppressions"),
    table_row("Unused catalog entries"; "unused_catalog_entries"; "unused-catalog-entries"),
    table_row("Empty catalog groups"; "empty_catalog_groups"; "empty-catalog-groups"),
    table_row("Unresolved catalog references"; "unresolved_catalog_references"; "unresolved-catalog-references"),
    table_row("Unused dependency overrides"; "unused_dependency_overrides"; "unused-dependency-overrides"),
    table_row("Misconfigured dependency overrides"; "misconfigured_dependency_overrides"; "misconfigured-dependency-overrides")
  ] | join("\n")) +
  "\n\n---\n" +
  section("Unused files"; "unused_files";
    "Files not reachable from any entry point.\n\n| File |\n|------|\n";
    "| `\(.path)` |") +
  section("Unused exports"; "unused_exports";
    "Exported symbols with no known consumers.\n\n| File | Line | Export |\n|------|-----:|--------|\n";
    "| `\(.path)` | \(.line) | `\(.export_name)`\(if .is_re_export then " *(re-export)*" else "" end) |") +
  section("Unused types"; "unused_types";
    "Type exports with no known consumers.\n\n| File | Line | Type |\n|------|-----:|------|\n";
    "| `\(.path)` | \(.line) | `\(.export_name)` |") +
  section("Private type leaks"; "private_type_leaks";
    "Exported signatures that reference same-file private types.\n\n| File | Line | Export | Private type |\n|------|-----:|--------|--------------|\n";
    "| `\(.path)` | \(.line) | `\(.export_name)` | `\(.type_name)` |") +
  section("Unused dependencies"; "unused_dependencies";
    "Listed in `dependencies` but never imported by the declaring workspace.\n\n| Package | Imported elsewhere |\n|---------|--------------------|\n";
    "| `\(.package_name)` | \(workspace_context) |") +
  section("Unused devDependencies"; "unused_dev_dependencies";
    "Listed in `devDependencies` but never imported or referenced by the declaring workspace.\n\n| Package | Imported elsewhere |\n|---------|--------------------|\n";
    "| `\(.package_name)` | \(workspace_context) |") +
  section("Unused optionalDependencies"; "unused_optional_dependencies";
    "Listed in `optionalDependencies` but never imported by the declaring workspace.\n\n| Package | Imported elsewhere |\n|---------|--------------------|\n";
    "| `\(.package_name)` | \(workspace_context) |") +
  section("Unused enum members"; "unused_enum_members";
    "Enum members never referenced outside their declaration.\n\n| File | Line | Enum | Member |\n|------|-----:|------|--------|\n";
    "| `\(.path)` | \(.line) | `\(.parent_name)` | `\(.member_name)` |") +
  section("Unused class members"; "unused_class_members";
    "Class methods or properties never referenced outside their class.\n\n| File | Line | Class | Member |\n|------|-----:|-------|--------|\n";
    "| `\(.path)` | \(.line) | `\(.parent_name)` | `\(.member_name)` |") +
  section("Unused store members"; "unused_store_members";
    "Pinia store members (state, getter, action) never accessed by any consumer.\n\n| File | Line | Store | Member |\n|------|-----:|-------|--------|\n";
    "| `\(.path)` | \(.line) | `\(.parent_name)` | `\(.member_name)` |") +
  section("Unresolved imports"; "unresolved_imports";
    "Import paths that could not be resolved \u2014 check for missing packages or broken paths.\n\n| File | Line | Import |\n|------|-----:|--------|\n";
    "| `\(.path)` | \(.line) | `\(.specifier)` |") +
  section("Unlisted dependencies"; "unlisted_dependencies";
    "Packages imported in code but missing from `package.json`.\n\n| Package | Used in |\n|---------|--------|\n";
    "| `\(.package_name)` | \(if (.imported_from | length) > 0 then (.imported_from[:3] | map("`\(.path):\(.line)`") | join(", ")) + (if (.imported_from | length) > 3 then " *+\((.imported_from | length) - 3) more*" else "" end) else "" end) |") +
  section("Duplicate exports"; "duplicate_exports";
    "Same export name defined in multiple files \u2014 barrel re-exports may resolve ambiguously.\n\n| Export | Locations |\n|--------|-----------|\n";
    "| `\(.export_name)` | \(.locations[:3] | map("`\(.path):\(.line)`") | join(", "))\(if (.locations | length) > 3 then " *+\((.locations | length) - 3) more*" else "" end) |") +
  section("Circular dependencies"; "circular_dependencies";
    "Import cycles that can cause initialization failures and prevent tree-shaking.\n\n| Cycle | Length |\n|-------|-------:|\n";
    "| \(.files | join(" \u2192 ")) | \(.length) |") +
  section("Re-export cycles"; "re_export_cycles";
    "Barrel files that re-export from each other in a loop. Chain propagation through the loop is a no-op, so imports through any member may silently come up empty.\n\n| Cycle | Kind | Members |\n|-------|------|--------:|\n";
    "| \(.files | map("`\(.)`") | join(" <-> ")) | \(.kind) | \(.files | length) |") +
  section("Boundary violations"; "boundary_violations";
    "Imports that cross defined architecture zone boundaries.\n\n| From | To | Zones |\n|------|-----|-------|\n";
    "| `\(.from_path):\(.line)` | `\(.to_path)` | \(.from_zone) \u2192 \(.to_zone) |") +
  section("Boundary coverage"; "boundary_coverage_violations";
    "Files that match no configured architecture boundary zone.\n\n| File |\n|------|\n";
    "| `\(.path):\(.line)` |") +
  section("Boundary calls"; "boundary_call_violations";
    "Calls from zoned files to callees forbidden for that zone.\n\n| File | Callee | Zone | Pattern |\n|------|--------|------|---------|\n";
    "| `\(.path):\(.line)` | `\(.callee)` | \(.zone) | `\(.pattern)` |") +
  section("Policy violations"; "policy_violations";
    "Banned calls and banned imports matched by configured rule packs.\n\n| File | Matched | Rule | Severity |\n|------|---------|------|----------|\n";
    "| `\(.path):\(.line)` | `\(.matched)` | `\(.pack)/\(.rule_id)` | \(.severity) |") +
  section("Invalid client exports"; "invalid_client_exports";
    "`\"use client\"` files exporting a Next.js server-only / route-config name. Next.js rejects this at build time.\n\n| File | Export | Directive |\n|------|--------|-----------|\n";
    "| `\(.path):\(.line)` | `\(.export_name)` | `\"\(.directive)\"` |") +
  section("Mixed client/server barrels"; "mixed_client_server_barrels";
    "Barrels re-exporting both a `\"use client\"` module and a server-only module. One import drags the other's directive across the boundary.\n\n| File | Client origin | Server origin |\n|------|---------------|---------------|\n";
    "| `\(.path):\(.line)` | `\(.client_origin)` | `\(.server_origin)` |") +
  section("Misplaced directives"; "misplaced_directives";
    "`\"use client\"` / `\"use server\"` directives written after a non-directive statement, so the RSC bundler ignores them. Move the directive to the top of the file.\n\n| File | Directive |\n|------|-----------|\n";
    "| `\(.path):\(.line)` | `\"\(.directive)\"` |") +
  section("Unused server actions"; "unused_server_actions";
    "Next.js Server Actions (exports of a `\"use server\"` file) that no project code references. The endpoint stays POST-able, but no code calls it (likely dead).\n\n| File | Action |\n|------|--------|\n";
    "| `\(.path):\(.line)` | `\(.action_name)` |") +
  section("Route collisions"; "route_collisions";
    "Next.js App Router route files that resolve to the same URL within one app-root. Next.js fails the build because a URL can have only one owner.\n\n| File | URL |\n|------|-----|\n";
    "| `\(.path)` | `\(.url)` |") +
  section("Dynamic segment conflicts"; "dynamic_segment_name_conflicts";
    "Sibling Next.js dynamic route segments at one position using different slug names. Next.js requires one consistent name per dynamic path.\n\n| File | Position | Segments |\n|------|----------|----------|\n";
    "| `\(.path)` | `\(.position)` | `\(.conflicting_segments | join(", "))` |") +
  section("Unrendered components"; "unrendered_components";
    "Vue/Svelte components reachable in the module graph but rendered nowhere: no tag, no dynamic binding, no registration. A barrel re-export keeps them alive even though nothing instantiates them.\n\n| File | Component | Framework |\n|------|-----------|-----------|\n";
    "| `\(.path):\(.line)` | `\(.component_name)` | \(.framework) |") +
  section("Unused component props"; "unused_component_props";
    "Vue `defineProps` props referenced nowhere inside their own single-file component (neither script nor template).\n\n| File | Component | Prop |\n|------|-----------|------|\n";
    "| `\(.path):\(.line)` | `\(.component_name)` | `\(.prop_name)` |") +
  section("Unused component emits"; "unused_component_emits";
    "Vue `defineEmits` events emitted nowhere inside their own single-file component (no matching `emit()` call).\n\n| File | Component | Event |\n|------|-----------|-------|\n";
    "| `\(.path):\(.line)` | `\(.component_name)` | `\(.emit_name)` |") +
  section("Unused component inputs"; "unused_component_inputs";
    "Angular `@Input()` / signal `input()` declarations read nowhere inside their own component (neither class body nor template).\n\n| File | Component | Input |\n|------|-----------|-------|\n";
    "| `\(.path):\(.line)` | `\(.component_name)` | `\(.input_name)` |") +
  section("Unused component outputs"; "unused_component_outputs";
    "Angular `@Output()` / signal `output()` declarations emitted nowhere inside their own component (no matching `emit()` call).\n\n| File | Component | Output |\n|------|-----------|--------|\n";
    "| `\(.path):\(.line)` | `\(.component_name)` | `\(.output_name)` |") +
  section("Unprovided injects"; "unprovided_injects";
    "Vue `inject` / Svelte `getContext` calls for a key that no ancestor `provide` / `setContext` supplies.\n\n| File | Key | Framework |\n|------|-----|-----------|\n";
    "| `\(.path):\(.line)` | `\(.key_name)` | \(.framework) |") +
  section("Unused load data keys"; "unused_load_data_keys";
    "SvelteKit `load()` return-object keys read by no consumer (neither the sibling `+page.svelte` nor `$page.data`). The key runs a real server fetch / DB cost per request for data nothing renders.\n\n| File | Route | Key |\n|------|-------|-----|\n";
    "| `\(.path):\(.line)` | `\(.route_dir)` | `\(.key_name)` |") +
  section("Type-only dependencies"; "type_only_dependencies";
    "Dependencies only used for type imports \u2014 consider moving to `devDependencies`.\n\n| Package |\n|---------|\n";
    "| `\(.package_name)` |") +
  section("Test-only dependencies"; "test_only_dependencies";
    "Production dependencies only imported by test files \u2014 consider moving to `devDependencies`.\n\n| Package |\n|---------|\n";
    "| `\(.package_name)` |") +
  section("Stale suppressions"; "stale_suppressions";
    "Suppression comments or JSDoc tags that no longer match any active issue.\n\n| File | Line | Description |\n|------|-----:|-------------|\n";
    "| `\(.path)` | \(.line) | \(if .origin.type == "jsdoc_tag" then "`@expected-unused` on `\(.origin.export_name)`" elif (.origin.kind_known == false) then "unknown kind `\(.origin.issue_kind)`" elif .origin.issue_kind then "`\(.origin.issue_kind)`" else "blanket" end) |") +
  section("Unused catalog entries"; "unused_catalog_entries";
    "pnpm catalog entries not referenced by any workspace package.\n\n| Entry | Catalog | Location | Hardcoded consumers |\n|-------|---------|----------|---------------------|\n";
    "| `\(.entry_name)` | `\(.catalog_name)` | `\(.path):\(.line)` | \(if ((.hardcoded_consumers // []) | length) > 0 then (.hardcoded_consumers | map("`\(.)`") | join(", ")) else "" end) |") +
  section("Empty catalog groups"; "empty_catalog_groups";
    "Named pnpm catalog groups with no entries.\n\n| Catalog | Location |\n|---------|----------|\n";
    "| `\(.catalog_name)` | `\(.path):\(.line)` |") +
  section("Unresolved catalog references"; "unresolved_catalog_references";
    "Workspace `package.json` references to catalogs that do not declare the package. `pnpm install` will fail until each entry is added to its named catalog or the reference is switched.\n\n| Entry | Catalog | Location | Available in |\n|-------|---------|----------|--------------|\n";
    "| `\(.entry_name)` | `\(.catalog_name)` | `\(.path):\(.line)` | \(if ((.available_in_catalogs // []) | length) > 0 then (.available_in_catalogs | map("`\(.)`") | join(", ")) else "" end) |") +
  section("Unused dependency overrides"; "unused_dependency_overrides";
    "`pnpm.overrides` entries forcing a version no workspace package depends on. Some entries may be intentional pins for transitive CVEs; the hint column flags those.\n\n| Override | Forces | Source | Location | Hint |\n|----------|--------|--------|----------|------|\n";
    "| `\(.raw_key)` | `\(.target_package)` -> `\(.version_range)` | `\(.source)` | `\(.path):\(.line)` | \(.hint // "") |") +
  section("Misconfigured dependency overrides"; "misconfigured_dependency_overrides";
    "`pnpm.overrides` entries with an unparsable key or empty value. `pnpm install` will reject these.\n\n| Override | Value | Source | Location | Reason |\n|----------|-------|--------|----------|--------|\n";
    "| `\(.raw_key // "")` | `\(.raw_value // "")` | `\(.source)` | `\(.path):\(.line)` | \(.reason // "unparsable") |") +
  "\n\n> [!TIP]\n" +
  (if ((.unused_exports // []) + (.unused_dependencies // []) + (.unused_enum_members // [])) | length > 0 then
    "> Run `fallow fix --dry-run` to preview safe auto-fixes.\n"
  else "" end) +
  (if (.unused_exports // []) | length > 0 then
    "> Intentionally public? Add [`/** @public */`](https://docs.fallow.tools/configuration/suppression) above exports to preserve them.\n"
  else "" end) +
  "> Add [`// fallow-ignore-next-line`](https://docs.fallow.tools/configuration/suppression) above a line to suppress a specific finding."
end
