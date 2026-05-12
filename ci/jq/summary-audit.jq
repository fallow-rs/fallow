def plural(n; word): "\(n) \(word)\(if n == 1 then "" else "s" end)";
def rel_path: if startswith("/") then (split("/") | if length > 3 then .[-3:] | join("/") else join("/") end) else . end;
def verdict_label:
  if . == "fail" then ":warning: **Audit failed**"
  elif . == "warn" then ":warning: **Audit passed with warnings**"
  else "**Audit passed**"
  end;
def introduced(v): if v == true then "new" elif v == false then "inherited" else "-" end;
def path_line: "`\(.path | rel_path)\(if .line then ":\(.line)" else "" end)`";
def first_import_site:
  if ((.imported_from // []) | length) > 0 then
    (.imported_from[0] | "`\(.path | rel_path):\(.line)`")
  else path_line end;
def dead_code_rows:
  ([ (.dead_code.unused_files // [])[] | {kind:"Unused file", location:("`\(.path | rel_path)`"), item:"-", introduced:.introduced} ] +
   [ (.dead_code.unused_exports // [])[] | {kind:"Unused export", location:path_line, item:("`\(.export_name)`"), introduced:.introduced} ] +
   [ (.dead_code.unused_types // [])[] | {kind:"Unused type", location:path_line, item:("`\(.export_name)`"), introduced:.introduced} ] +
   [ (.dead_code.private_type_leaks // [])[] | {kind:"Private type leak", location:path_line, item:("`\(.export_name)` -> `\(.type_name)`"), introduced:.introduced} ] +
   [ (.dead_code.unused_dependencies // [])[] | {kind:"Unused dependency", location:path_line, item:("`\(.package_name)`"), introduced:.introduced} ] +
   [ (.dead_code.unused_dev_dependencies // [])[] | {kind:"Unused devDependency", location:path_line, item:("`\(.package_name)`"), introduced:.introduced} ] +
   [ (.dead_code.unused_optional_dependencies // [])[] | {kind:"Unused optionalDependency", location:path_line, item:("`\(.package_name)`"), introduced:.introduced} ] +
   [ (.dead_code.unused_enum_members // [])[] | {kind:"Unused enum member", location:path_line, item:("`\(.parent_name).\(.member_name)`"), introduced:.introduced} ] +
   [ (.dead_code.unused_class_members // [])[] | {kind:"Unused class member", location:path_line, item:("`\(.parent_name).\(.member_name)`"), introduced:.introduced} ] +
   [ (.dead_code.unresolved_imports // [])[] | {kind:"Unresolved import", location:path_line, item:("`\(.specifier)`"), introduced:.introduced} ] +
   [ (.dead_code.unlisted_dependencies // [])[] | {kind:"Unlisted dependency", location:first_import_site, item:("`\(.package_name)`"), introduced:.introduced} ] +
   [ (.dead_code.duplicate_exports // [])[] | {kind:"Duplicate export", location:(.locations[:3] | map("`\(.path | rel_path):\(.line)`") | join(", ")), item:("`\(.export_name)`"), introduced:.introduced} ] +
   [ (.dead_code.circular_dependencies // [])[] | {kind:"Circular dependency", location:((.files // []) | map("`\(. | rel_path)`") | join(" -> ")), item:"cycle", introduced:.introduced} ] +
   [ (.dead_code.boundary_violations // [])[] | {kind:"Boundary violation", location:("`\(.from_path | rel_path):\(.line)`"), item:("\(.from_zone) -> \(.to_zone)"), introduced:.introduced} ] +
   [ (.dead_code.type_only_dependencies // [])[] | {kind:"Type-only dependency", location:path_line, item:("`\(.package_name)`"), introduced:.introduced} ] +
   [ (.dead_code.test_only_dependencies // [])[] | {kind:"Test-only dependency", location:path_line, item:("`\(.package_name)`"), introduced:.introduced} ] +
   [ (.dead_code.stale_suppressions // [])[] | {kind:"Stale suppression", location:path_line, item:(.description // "suppression"), introduced:.introduced} ] +
   [ (.dead_code.unused_catalog_entries // [])[] | {kind:"Unused catalog entry", location:path_line, item:("`\(.entry_name)` (`\(.catalog_name)`)"), introduced:.introduced} ] +
   [ (.dead_code.unresolved_catalog_references // [])[] | {kind:"Unresolved catalog reference", location:path_line, item:("`\(.entry_name)` -> `\(.catalog_name)`"), introduced:.introduced} ] +
   [ (.dead_code.unused_dependency_overrides // [])[] | {kind:"Unused dependency override", location:path_line, item:("`\(.raw_key)` (`\(.source)`)"), introduced:.introduced} ] +
   [ (.dead_code.misconfigured_dependency_overrides // [])[] | {kind:"Misconfigured dependency override", location:path_line, item:("`\(.raw_key)` (`\(.source)`)"), introduced:.introduced} ]);
def duplication_rows:
  [(.duplication.clone_groups // [])[] |
    (.instances // []) as $instances |
    ($instances[0] // {}) as $first |
    {
      location: (if ($first.file // "") != "" then "`\($first.file | rel_path):\($first.start_line // 1)`" else "-" end),
      files: ($instances | map(.file | rel_path) | unique | .[:3] | join(", ")),
      size: "\(.line_count // 0) lines / \(.token_count // 0) tokens",
      instances: ($instances | length),
      introduced: .introduced
    }
  ];

(.verdict // "pass") as $verdict |
(.summary // {}) as $summary |
(.attribution // {}) as $attr |
($summary.dead_code_issues // 0) as $dead |
($summary.complexity_findings // 0) as $complex |
($summary.duplication_clone_groups // 0) as $dupes |
(.changed_files_count // 0) as $files |
(.elapsed_ms // 0) as $elapsed |
dead_code_rows as $dead_rows |
(.complexity.findings // []) as $complex_findings |
duplication_rows as $dupe_rows |

"## Fallow Audit\n\n" +
"> \($verdict | verdict_label) · \(plural($files; "changed file")) · \($elapsed)ms\n\n" +
"| Category | Findings | Introduced | Inherited |\n|:---------|---------:|-----------:|----------:|\n" +
"| Dead code | \($dead) | \($attr.dead_code_introduced // 0) | \($attr.dead_code_inherited // 0) |\n" +
"| Complexity | \($complex) | \($attr.complexity_introduced // 0) | \($attr.complexity_inherited // 0) |\n" +
"| Duplication | \($dupes) | \($attr.duplication_introduced // 0) | \($attr.duplication_inherited // 0) |\n\n" +
(if ($dead_rows | length) > 0 then
  "### Dead Code\n\n" +
  "| Type | Location | Item | Status |\n|:-----|:---------|:-----|:-------|\n" +
  ([$dead_rows[:10][] |
    "| \(.kind) | \(.location) | \(.item) | \(introduced(.introduced)) |"
  ] | join("\n")) +
  (if ($dead_rows | length) > 10 then "\n\n> \(($dead_rows | length) - 10) more dead-code findings in the full audit report" else "" end) +
  "\n\n"
else "" end) +
(if ($complex_findings | length) > 0 then
  "### Complexity\n\n" +
  "| File | Function | Status | Severity | Cyclomatic | Cognitive | Coverage | CRAP |\n|:-----|:---------|:-------|:---------|:-----------|:----------|:---------|:-----|\n" +
  ([$complex_findings[:15][] |
    "| `\(.path):\(.line)` | `\(.name)` | \(introduced(.introduced)) | \(.severity // "moderate") | \(.cyclomatic) | \(.cognitive) | \(.coverage_tier // "-") | \(if .crap == null then "-" else (.crap | tostring) end) |"
  ] | join("\n")) +
  (if ($complex_findings | length) > 15 then "\n\n> \(($complex_findings | length) - 15) more complexity findings in the full audit report" else "" end) +
  ((.complexity.summary.coverage_model // null) as $model |
    (.complexity.summary.istanbul_matched // null) as $matched |
    (.complexity.summary.istanbul_total // null) as $total |
    if $model == "istanbul" then
      (if ($matched != null and $total != null and $total > 0) then
        "\n\n*Coverage model: istanbul. Matched \($matched)/\($total) functions"
        + (if (($matched * 100) / $total) < 50 then ". Low match rate; check `--coverage-root` is correct for this checkout." else "." end)
        + "*"
      else "\n\n*Coverage model: istanbul (exact, from `--coverage`).*" end)
    elif ($model == "static_estimated" or $model == "static_binary") then
      "\n\n*Coverage model: static (estimated). Pair with `--coverage <coverage-final.json>` for measured coverage instead of estimates.*"
    else "" end) +
  "\n\n"
else "" end) +
(if ($dupe_rows | length) > 0 then
  "### Duplication\n\n" +
  "| Location | Files | Size | Instances | Status |\n|:---------|:------|:-----|----------:|:-------|\n" +
  ([$dupe_rows[:10][] |
    "| \(.location) | \(.files) | \(.size) | \(.instances) | \(introduced(.introduced)) |"
  ] | join("\n")) +
  (if ($dupe_rows | length) > 10 then "\n\n> \(($dupe_rows | length) - 10) more clone groups in the full audit report" else "" end) +
  "\n\n"
else "" end) +
(if ($attr.gate // "new-only") == "new-only" then
  "*Audit gate: new-only. Inherited findings are reported but do not fail the verdict.*"
else
  "*Audit gate: all. Every finding in changed files affects the verdict.*"
end)
