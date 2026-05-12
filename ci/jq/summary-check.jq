# GitLab variant of summary-check.jq
# Differences from GitHub: no > [!NOTE] / > [!WARNING] / > [!TIP] callouts

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
  "> **No issues found** \u00b7 \(.elapsed_ms)ms\n\n" +
  "All exports are used, all dependencies are declared, and no issues were detected."
else
  "# Fallow Analysis\n\n" +
  "> :warning: **\(.total_issues) issues** found \u00b7 \(.elapsed_ms)ms\n\n" +
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
    table_row("Unresolved imports"; "unresolved_imports"; "unresolved-imports"),
    table_row("Unlisted dependencies"; "unlisted_dependencies"; "unlisted-dependencies"),
    table_row("Duplicate exports"; "duplicate_exports"; "duplicate-exports"),
    table_row("Circular dependencies"; "circular_dependencies"; "circular-dependencies"),
    table_row("Boundary violations"; "boundary_violations"; "boundary-violations"),
    table_row("Type-only dependencies"; "type_only_dependencies"; "type-only-dependencies"),
    table_row("Test-only dependencies"; "test_only_dependencies"; "test-only-dependencies"),
    table_row("Stale suppressions"; "stale_suppressions"; "stale-suppressions"),
    table_row("Unused catalog entries"; "unused_catalog_entries"; "unused-catalog-entries"),
    table_row("Unresolved catalog references"; "unresolved_catalog_references"; "unresolved-catalog-references")
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
  section("Boundary violations"; "boundary_violations";
    "Imports that cross defined architecture zone boundaries.\n\n| From | To | Zones |\n|------|-----|-------|\n";
    "| `\(.from_path):\(.line)` | `\(.to_path)` | \(.from_zone) \u2192 \(.to_zone) |") +
  section("Type-only dependencies"; "type_only_dependencies";
    "Dependencies only used for type imports \u2014 consider moving to `devDependencies`.\n\n| Package |\n|---------|\n";
    "| `\(.package_name)` |") +
  section("Test-only dependencies"; "test_only_dependencies";
    "Production dependencies only imported by test files \u2014 consider moving to `devDependencies`.\n\n| Package |\n|---------|\n";
    "| `\(.package_name)` |") +
  section("Stale suppressions"; "stale_suppressions";
    "Suppression comments or JSDoc tags that no longer match any active issue.\n\n| File | Line | Description |\n|------|-----:|-------------|\n";
    "| `\(.path)` | \(.line) | \(if .origin.type == "jsdoc_tag" then "`@expected-unused` on `\(.origin.export_name)`" elif .origin.issue_kind then "`\(.origin.issue_kind)`" else "blanket" end) |") +
  section("Unused catalog entries"; "unused_catalog_entries";
    "pnpm catalog entries not referenced by any workspace package.\n\n| Entry | Catalog | Location | Hardcoded consumers |\n|-------|---------|----------|---------------------|\n";
    "| `\(.entry_name)` | `\(.catalog_name)` | `\(.path):\(.line)` | \(if ((.hardcoded_consumers // []) | length) > 0 then (.hardcoded_consumers | map("`\(.)`") | join(", ")) else "" end) |") +
  section("Unresolved catalog references"; "unresolved_catalog_references";
    "Workspace `package.json` references to catalogs that do not declare the package. `pnpm install` will fail until each entry is added to its named catalog or the reference is switched.\n\n| Entry | Catalog | Location | Available in |\n|-------|---------|----------|--------------|\n";
    "| `\(.entry_name)` | `\(.catalog_name)` | `\(.path):\(.line)` | \(if ((.available_in_catalogs // []) | length) > 0 then (.available_in_catalogs | map("`\(.)`") | join(", ")) else "" end) |") +
  "\n\n" +
  (if ((.unused_exports // []) + (.unused_dependencies // []) + (.unused_enum_members // [])) | length > 0 then
    "> :bulb: Run `fallow fix --dry-run` to preview safe auto-fixes.\n"
  else "" end) +
  (if (.unused_exports // []) | length > 0 then
    "> :bulb: Intentionally public? Add [`/** @public */`](https://docs.fallow.tools/configuration/suppression) above exports to preserve them.\n"
  else "" end) +
  "> :bulb: Add [`// fallow-ignore-next-line`](https://docs.fallow.tools/configuration/suppression) above a line to suppress a specific finding."
end
