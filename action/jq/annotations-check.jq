def san: gsub("\n"; " ") | gsub("\r"; " ") | gsub("%"; "%25");
def nl: "%0A";
def pm: $ENV.PKG_MANAGER // "npm";
def remove_cmd(pkg): if pm == "pnpm" then "pnpm remove \(pkg)" elif pm == "yarn" then "yarn remove \(pkg)" else "npm uninstall \(pkg)" end;
def add_cmd(pkg): if pm == "pnpm" then "pnpm add \(pkg)" elif pm == "yarn" then "yarn add \(pkg)" else "npm install \(pkg)" end;
def workspace_context:
  if ((.used_in_workspaces // []) | length) > 0 then
    "\(nl)\(nl)Imported in other workspaces: " + (.used_in_workspaces | join(", "))
  else
    ""
  end;
def dependency_action(pkg):
  if ((.used_in_workspaces // []) | length) > 0 then
    "Move this dependency to the consuming workspace package.json."
  else
    "Run: \(remove_cmd(pkg))"
  end;
[
  (.unused_files[]? |
    "::warning file=\(.path | san),title=Unused file::This file is not imported by any other module and unreachable from entry points.\(nl)Consider removing it or importing it where needed."),
  (.unused_exports[]? |
    "::warning file=\(.path | san),line=\(.line),col=\(.col + 1),title=Unused export::\(if .is_re_export then "Re-exported" else "Exported" end) \(if .is_type_only then "type" else "value" end) '\(.export_name | san)' is never imported by other modules.\(nl)\(nl)If this export is part of a public API, consider adding it to the entry configuration.\(nl)Otherwise, remove the export keyword or delete the declaration."),
  (.unused_types[]? |
    "::warning file=\(.path | san),line=\(.line),col=\(.col + 1),title=Unused type::\(if .is_re_export then "Re-exported" else "Exported" end) type '\(.export_name | san)' is never imported by other modules.\(nl)\(nl)If only used internally, remove the export keyword."),
  (.private_type_leaks[]? |
    "::warning file=\(.path | san),line=\(.line),col=\(.col + 1),title=Private type leak::Export '\(.export_name | san)' references private type '\(.type_name | san)'.\(nl)\(nl)Export the referenced type or remove it from the public signature."),
  (.unused_dependencies[]? |
    "::warning file=\(.path | san)\(if .line > 0 then ",line=\(.line)" else "" end),title=Unused dependency::Package '\(.package_name | san)' is listed in dependencies but never imported by this package.\(workspace_context)\(nl)\(nl)\(dependency_action(.package_name | san))"),
  (.unused_dev_dependencies[]? |
    "::warning file=\(.path | san)\(if .line > 0 then ",line=\(.line)" else "" end),title=Unused devDependency::Package '\(.package_name | san)' is listed in devDependencies but never imported by this package.\(workspace_context)\(nl)\(nl)\(dependency_action(.package_name | san))"),
  (.unused_optional_dependencies[]? |
    "::warning file=\(.path | san)\(if .line > 0 then ",line=\(.line)" else "" end),title=Unused optionalDependency::Package '\(.package_name | san)' is listed in optionalDependencies but never imported by this package.\(workspace_context)\(nl)\(nl)\(dependency_action(.package_name | san))"),
  (.unused_enum_members[]? |
    "::warning file=\(.path | san),line=\(.line),col=\(.col + 1),title=Unused enum member::Enum member '\(.parent_name | san).\(.member_name | san)' is never referenced in the codebase.\(nl)\(nl)Consider removing it to keep the enum minimal."),
  (.unused_class_members[]? |
    "::warning file=\(.path | san),line=\(.line),col=\(.col + 1),title=Unused class member::Class member '\(.parent_name | san).\(.member_name | san)' is never referenced.\(nl)\(nl)Consider removing it or marking it as private."),
  (.unused_store_members[]? |
    "::warning file=\(.path | san),line=\(.line),col=\(.col + 1),title=Unused store member::Store member '\(.parent_name | san).\(.member_name | san)' is never accessed by any consumer.\(nl)\(nl)Consider removing the unused store state, getter, or action."),
  (.unresolved_imports[]? |
    "::warning file=\(.path | san),line=\(.line),col=\(.col + 1),title=Unresolved import::Import '\(.specifier | san)' could not be resolved to a file or package.\(nl)\(nl)Check for typos, missing dependencies, or incorrect path aliases."),
  (.unlisted_dependencies[]? | (.package_name | san) as $pkg | .imported_from[]? |
    "::warning file=\(.path | san),line=\(.line),col=\(.col + 1),title=Unlisted dependency::Package '\($pkg)' is imported here but not listed in package.json.\(nl)\(nl)Run: \(add_cmd($pkg))"),
  (.duplicate_exports[]? | (.export_name | san) as $name | .locations as $locs | .locations[]? |
    "::warning file=\(.path | san),line=\(.line),col=\(.col + 1),title=Duplicate export::Export '\($name)' is defined in \($locs | length) modules:\(nl)\($locs | map("  \u2022 " + (.path | san) + ":" + (.line | tostring)) | join(nl))\(nl)\(nl)This causes ambiguity for consumers. Keep one canonical location."),
  (.circular_dependencies[]? |
    "::warning file=\(.files[0] | san)\(if .line > 0 then ",line=\(.line),col=\(.col + 1)" else "" end),title=Circular dependency::Circular import chain detected:\(nl)\(.files | map(san) | join(" \u2192 ")) \u2192 \(.files[0] | san)\(nl)\(nl)Circular dependencies can cause initialization bugs and make code harder to reason about.\(nl)Consider extracting shared logic into a separate module."),
  (.re_export_cycles[]? | (.files | length) as $n | .files as $files | .kind as $kind |
    "::warning file=\($files[0] | san),title=Re-export cycle::\(if $kind == "self-loop" then "Self-loop: this file re-exports from itself." else "Re-export cycle (" + ($n | tostring) + " files): " + ($files | map(san) | join(" <-> ")) + "." end)\(nl)\(nl)Chain propagation through the loop is a no-op, so imports through any member may silently come up empty.\(nl)\(if $kind == "self-loop" then "Remove the `export * from './'` (or equivalent) inside this file." else "Remove one `export * from` statement on any one member file to break the cycle." end)"),
  (.boundary_violations[]? |
    "::warning file=\(.from_path | san)\(if .line > 0 then ",line=\(.line),col=\(.col + 1)" else "" end),title=Boundary violation::Import from zone '\(.from_zone | san)' to zone '\(.to_zone | san)' is not allowed.\(nl)\(.from_path | san) -> \(.to_path | san)\(nl)\(nl)Route the import through an allowed zone or restructure the dependency."),
  (.boundary_coverage_violations[]? |
    "::warning file=\(.path | san)\(if .line > 0 then ",line=\(.line),col=\(.col + 1)" else "" end),title=Boundary coverage::File does not match any configured architecture boundary zone.\(nl)\(nl)Add the file to a zone pattern or allow-list it with boundaries.coverage.allowUnmatched."),
  (.boundary_call_violations[]? |
    "::warning file=\(.path | san)\(if .line > 0 then ",line=\(.line),col=\(.col + 1)" else "" end),title=Boundary call violation::Call to '\(.callee | san)' matches forbidden pattern '\(.pattern | san)' in zone '\(.zone | san)'.\(nl)\(nl)Move the call behind an allowed abstraction or adjust boundaries.calls.forbidden."),
  (.policy_violations[]? |
    "::\(if .severity == "error" then "error" else "warning" end) file=\(.path | san)\(if .line > 0 then ",line=\(.line),col=\(.col + 1)" else "" end),title=Policy violation::'\(.matched | san)' is banned by rule '\(.pack | san)/\(.rule_id | san)'.\(if .message then "\(nl)\(nl)\(.message | san)" else "" end)"),
  (.invalid_client_exports[]? |
    "::warning file=\(.path | san),line=\(.line),col=\(.col + 1),title=Invalid client export::Export '\(.export_name | san)' is not allowed in a \"\(.directive | san)\" file (Next.js server-only / route-config name).\(nl)\(nl)Move the server-only export to a non-client module, or remove the \"\(.directive | san)\" directive."),
  (.mixed_client_server_barrels[]? |
    "::warning file=\(.path | san),line=\(.line),col=\(.col + 1),title=Mixed client/server barrel::This barrel re-exports both a \"use client\" module ('\(.client_origin | san)') and a server-only module ('\(.server_origin | san)'); one import drags the other's directive across the boundary.\(nl)\(nl)Split the barrel so client and server-only modules are re-exported from separate entry points."),
  (.misplaced_directives[]? |
    "::warning file=\(.path | san),line=\(.line),col=\(.col + 1),title=Misplaced directive::Directive \"\(.directive | san)\" is not in the leading position, so the RSC bundler ignores it.\(nl)\(nl)Move the directive to the very top of the file, above every import."),
  (.route_collisions[]? |
    "::warning file=\(.path | san),title=Route collision::This route file resolves to '\(.url | san)', also owned by \(.conflicting_paths | length) other file(s). Next.js fails the build because a URL can have only one owner.\(nl)\(nl)Move or merge one of the colliding files; route groups and parallel slots do not change the URL."),
  (.dynamic_segment_name_conflicts[]? |
    "::warning file=\(.path | san),title=Dynamic segment conflict::Dynamic segments at '\(.position | san)' use different slug names (\(.conflicting_segments | join(", ") | san)). Next.js requires one consistent name per dynamic path.\(nl)\(nl)Rename the dynamic segments at this position to a single slug name."),
  (.type_only_dependencies[]? |
    "::warning file=\(.path | san)\(if .line > 0 then ",line=\(.line)" else "" end),title=Type-only dependency::Package '\(.package_name | san)' is only used via type imports.\(nl)\(nl)Move it from dependencies to devDependencies to reduce production bundle size."),
  (.stale_suppressions[]? |
    if .origin.type == "jsdoc_tag" then
      "::warning file=\(.path | san),line=\(.line),col=\(.col + 1),title=Stale @expected-unused::The @expected-unused tag on '\(.origin.export_name | san)' is stale because the export is now used.\(nl)\(nl)Remove the @expected-unused tag."
    elif (.origin.kind_known == false) then
      "::warning file=\(.path | san),line=\(.line),col=\(.col + 1),title=Unknown suppression kind::'\((.origin.issue_kind // "") | san)' is not a recognized fallow issue kind. Other tokens on this '\(if .origin.is_file_level then "fallow-ignore-file" else "fallow-ignore-next-line" end)' line still apply.\(nl)\(nl)Fix the typo or remove the unknown token."
    else
      "::warning file=\(.path | san),line=\(.line),col=\(.col + 1),title=Stale suppression::This '\(if .origin.is_file_level then "fallow-ignore-file" else "fallow-ignore-next-line" end)' comment\(if .origin.issue_kind then " for '\(.origin.issue_kind | san)'" else "" end) no longer matches any active issue.\(nl)\(nl)Remove the suppression comment to keep the codebase clean."
    end),
  (.unused_catalog_entries[]? |
    "::warning file=\(.path | san),line=\(.line),title=Unused catalog entry::Catalog entry '\(.entry_name | san)' (catalog '\(.catalog_name | san)') is not referenced by any workspace package via the catalog: protocol.\(nl)\(nl)\(if ((.hardcoded_consumers // []) | length) > 0 then "Hardcoded consumers: " + (.hardcoded_consumers | map(san) | join(", ")) + ".\(nl)Switch them to catalog: before removing." else "Remove the entry from pnpm-workspace.yaml." end)"),
  (.empty_catalog_groups[]? |
    "::warning file=\(.path | san),line=\(.line),title=Empty catalog group::Catalog group '\(.catalog_name | san)' has no entries.\(nl)\(nl)Remove the empty group header from pnpm-workspace.yaml."),
  (.unresolved_catalog_references[]? |
    "::error file=\(.path | san),line=\(.line),title=Unresolved catalog reference::Package '\(.entry_name | san)' is referenced via `catalog:\(if .catalog_name == "default" then "" else (.catalog_name | san) end)` but \(if .catalog_name == "default" then "the default catalog" else "catalog '" + (.catalog_name | san) + "'" end) does not declare it. `pnpm install` will fail.\(nl)\(nl)\(if ((.available_in_catalogs // []) | length) > 0 then "Available in: " + (.available_in_catalogs | map(san) | join(", ")) + ".\(nl)Switch the reference to a catalog that declares this package, or add it to the named catalog." else "Add this package to the named catalog in pnpm-workspace.yaml, or remove the reference and pin a hardcoded version." end)"),
  (.unused_dependency_overrides[]? |
    "::warning file=\((.path // "") | san),line=\(.line // 0),title=Unused dependency override::Override `\((.raw_key // "") | san)` forces `\((.target_package // "") | san)` to `\((.version_range // "") | san)` but no workspace package depends on `\((.target_package // "") | san)`.\(nl)\(nl)\(if .hint then (.hint | san) + ".\(nl)" else "" end)Delete the entry, or scope it under a real parent (`pkg>\((.target_package // "") | san)`) if it pins a transitive."),
  (.misconfigured_dependency_overrides[]? |
    "::error file=\((.path // "") | san),line=\(.line // 0),title=Misconfigured dependency override::Override `\((.raw_key // "") | san)` -> `\((.raw_value // "") | san)` is malformed (\((.reason // "unparsable") | san)). `pnpm install` will reject this entry.\(nl)\(nl)Fix the key/value to match pnpm's override grammar, or remove the entry.")
] | .[]
