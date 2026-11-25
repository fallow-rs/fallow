//! The dead-code sections of the GitHub check summary.

use super::*;

#[expect(
    clippy::too_many_lines,
    reason = "a flat data table of 14 per-kind row templates ported 1:1 from summary-check.jq; splitting would obscure the correspondence"
)]
pub(super) fn check_sections_core() -> Vec<SectionSpec> {
    vec![
        SectionSpec {
            name: "Unused files",
            key: "unused_files",
            header: "Files not reachable from any entry point.\n\n| File |\n|------|\n",
            row: |it| format!("| {}{} |", code_cell(it, "path"), caveat_cell_suffix(it)),
        },
        SectionSpec {
            name: "Unused exports",
            key: "unused_exports",
            header: "Exported symbols with no known consumers.\n\n| File | Line | Export |\n|------|-----:|--------|\n",
            row: |it| {
                format!(
                    "| {} | {} | {}{}{} |",
                    code_cell(it, "path"),
                    num(it, "line"),
                    code_cell(it, "export_name"),
                    if b(it, "is_re_export") {
                        " *(re-export)*"
                    } else {
                        ""
                    },
                    caveat_cell_suffix(it),
                )
            },
        },
        SectionSpec {
            name: "Unused types",
            key: "unused_types",
            header: "Type exports with no known consumers.\n\n| File | Line | Type |\n|------|-----:|------|\n",
            row: |it| {
                format!(
                    "| {} | {} | {}{} |",
                    code_cell(it, "path"),
                    num(it, "line"),
                    code_cell(it, "export_name"),
                    caveat_cell_suffix(it),
                )
            },
        },
        SectionSpec {
            name: "Private type leaks",
            key: "private_type_leaks",
            header: "Exported signatures that reference same-file private types.\n\n| File | Line | Export | Private type |\n|------|-----:|--------|--------------|\n",
            row: |it| {
                format!(
                    "| {} | {} | {} | {} |",
                    code_cell(it, "path"),
                    num(it, "line"),
                    code_cell(it, "export_name"),
                    code_cell(it, "type_name"),
                )
            },
        },
        SectionSpec {
            name: "Deprecated exports in use",
            key: "deprecated_exports_in_use",
            header: "Exports marked `@deprecated` that still have consumers.\n\n| File | Line | Export | Consumers |\n|------|-----:|--------|----------:|\n",
            row: |it| {
                format!(
                    "| {} | {} | {} | {} |",
                    code_cell(it, "path"),
                    num(it, "line"),
                    code_cell(it, "export_name"),
                    num(it, "consumer_count"),
                )
            },
        },
        SectionSpec {
            name: "Unused dependencies",
            key: "unused_dependencies",
            header: "Listed in `dependencies` but never imported by the declaring workspace.\n\n| Package | Imported elsewhere |\n|---------|--------------------|\n",
            row: |it| {
                format!(
                    "| {}{} | {} |",
                    code_cell(it, "package_name"),
                    caveat_cell_suffix(it),
                    check_workspace_context(it),
                )
            },
        },
        SectionSpec {
            name: "Unused devDependencies",
            key: "unused_dev_dependencies",
            header: "Listed in `devDependencies` but never imported or referenced by the declaring workspace.\n\n| Package | Imported elsewhere |\n|---------|--------------------|\n",
            row: |it| {
                format!(
                    "| {}{} | {} |",
                    code_cell(it, "package_name"),
                    caveat_cell_suffix(it),
                    check_workspace_context(it),
                )
            },
        },
        SectionSpec {
            name: "Unused optionalDependencies",
            key: "unused_optional_dependencies",
            header: "Listed in `optionalDependencies` but never imported by the declaring workspace.\n\n| Package | Imported elsewhere |\n|---------|--------------------|\n",
            row: |it| {
                format!(
                    "| {}{} | {} |",
                    code_cell(it, "package_name"),
                    caveat_cell_suffix(it),
                    check_workspace_context(it),
                )
            },
        },
        SectionSpec {
            name: "Unused enum members",
            key: "unused_enum_members",
            header: "Enum members never referenced outside their declaration.\n\n| File | Line | Enum | Member |\n|------|-----:|------|--------|\n",
            row: member_row,
        },
        SectionSpec {
            name: "Unused class members",
            key: "unused_class_members",
            header: "Class methods or properties that no code uses, also not through `this`.\n\n| File | Line | Class | Member |\n|------|-----:|-------|--------|\n",
            row: member_row,
        },
        SectionSpec {
            name: "Unused store members",
            key: "unused_store_members",
            header: "Pinia store members (state, getter, action) never accessed by any consumer.\n\n| File | Line | Store | Member |\n|------|-----:|-------|--------|\n",
            row: member_row,
        },
        SectionSpec {
            name: "Unused protected class members",
            key: "unused_protected_class_members",
            header: "Protected class members that neither the class nor a subclass reads.\n\n| File | Line | Class | Member |\n|------|-----:|-------|--------|\n",
            row: member_row,
        },
        SectionSpec {
            name: "Namespace escapes",
            key: "namespace_escapes",
            header: "Namespace imports used as a whole value, so fallow does not check the exports of the module.\n\n| File | Line | Binding | Module | Hidden exports |\n|------|-----:|---------|--------|---------------:|\n",
            row: |it| {
                format!(
                    "| {} | {} | {} | {} | {} |",
                    code_cell(it, "path"),
                    num(it, "line"),
                    code_cell(it, "local_name"),
                    code_cell(it, "module"),
                    num(it, "hidden_exports"),
                )
            },
        },
        SectionSpec {
            name: "Test-only exports",
            key: "test_only_exports",
            header: "Exported values that only test-side files use. Production code does not use them.\n\n| File | Line | Export | Colocated |\n|------|-----:|--------|-----------|\n",
            row: |it| {
                format!(
                    "| {} | {} | {} | {} |",
                    code_cell(it, "path"),
                    num(it, "line"),
                    code_cell(it, "export_name"),
                    if it["colocated_only"].as_bool() == Some(true) {
                        "yes"
                    } else {
                        "no"
                    },
                )
            },
        },
        SectionSpec {
            name: "Unused namespace members",
            key: "unused_namespace_members",
            header: "Exported members of an exported TypeScript namespace that no code reads.\n\n| File | Line | Namespace | Member |\n|------|-----:|-----------|--------|\n",
            row: member_row,
        },
        SectionSpec {
            name: "Unresolved imports",
            key: "unresolved_imports",
            header: "Import paths that could not be resolved - check for missing packages or broken paths.\n\n| File | Line | Import |\n|------|-----:|--------|\n",
            row: |it| {
                format!(
                    "| {} | {} | {} |",
                    code_cell(it, "path"),
                    num(it, "line"),
                    code_cell(it, "specifier"),
                )
            },
        },
        SectionSpec {
            name: "Unlisted dependencies",
            key: "unlisted_dependencies",
            header: "Packages imported in code but missing from `package.json`.\n\n| Package | Used in |\n|---------|--------|\n",
            row: |it| {
                let sites: Vec<&Value> = arr(it, "imported_from").collect();
                let cell = if sites.is_empty() {
                    String::new()
                } else {
                    let shown = sites
                        .iter()
                        .take(3)
                        .map(|site| {
                            markdown_table_code_span(&format!(
                                "{}:{}",
                                s(site, "path"),
                                num(site, "line")
                            ))
                        })
                        .collect::<Vec<_>>()
                        .join(", ");
                    let more = if sites.len() > 3 {
                        format!(" *+{} more*", sites.len() - 3)
                    } else {
                        String::new()
                    };
                    format!("{shown}{more}")
                };
                format!("| {} | {cell} |", code_cell(it, "package_name"))
            },
        },
        SectionSpec {
            name: "Unlisted binaries",
            key: "unlisted_binaries",
            header: "Binaries that scripts or process calls run, but that no installed package provides.\n\n| Binary | Used in |\n|--------|--------|\n",
            row: |it| {
                let sites: Vec<&Value> = arr(it, "used_in").collect();
                let shown = sites
                    .iter()
                    .take(3)
                    .map(|site| {
                        markdown_table_code_span(&format!(
                            "{}:{}",
                            s(site, "path"),
                            num(site, "line")
                        ))
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                let more = if sites.len() > 3 {
                    format!(" *+{} more*", sites.len() - 3)
                } else {
                    String::new()
                };
                format!("| {} | {shown}{more} |", code_cell(it, "binary_name"))
            },
        },
        SectionSpec {
            name: "Duplicate exports",
            key: "duplicate_exports",
            header: "Same export name defined in multiple files - barrel re-exports may resolve ambiguously.\n\n| Export | Locations |\n|--------|-----------|\n",
            row: |it| {
                let locations: Vec<&Value> = arr(it, "locations").collect();
                let shown = locations
                    .iter()
                    .take(3)
                    .map(|location| {
                        markdown_table_code_span(&format!(
                            "{}:{}",
                            s(location, "path"),
                            num(location, "line")
                        ))
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                let more = if locations.len() > 3 {
                    format!(" *+{} more*", locations.len() - 3)
                } else {
                    String::new()
                };
                format!("| {} | {shown}{more} |", code_cell(it, "export_name"))
            },
        },
        SectionSpec {
            name: "Circular dependencies",
            key: "circular_dependencies",
            header: "Import cycles that can cause initialization failures and prevent tree-shaking.\n\n| Cycle | Length |\n|-------|-------:|\n",
            row: |it| {
                let cycle = arr(it, "files")
                    .filter_map(Value::as_str)
                    .map(markdown_table_code_span)
                    .collect::<Vec<_>>()
                    .join(" \u{2192} ");
                format!("| {cycle} | {} |", num(it, "length"))
            },
        },
    ]
}
