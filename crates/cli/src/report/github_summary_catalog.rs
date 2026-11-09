//! Step summary sections for catalog findings.

use super::*;

pub(super) fn check_sections_catalog() -> Vec<SectionSpec> {
    vec![
        SectionSpec {
            name: "Unused catalog entries",
            key: "unused_catalog_entries",
            header: "pnpm catalog entries not referenced by any workspace package.\n\n| Entry | Catalog | Location | Hardcoded consumers |\n|-------|---------|----------|---------------------|\n",
            row: |it| {
                format!(
                    "| {} | {} | {} | {} |",
                    code_cell(it, "entry_name"),
                    code_cell(it, "catalog_name"),
                    path_line_cell(it),
                    backtick_join(it, "hardcoded_consumers"),
                )
            },
        },
        SectionSpec {
            name: "Empty catalog groups",
            key: "empty_catalog_groups",
            header: "Named pnpm catalog groups with no entries.\n\n| Catalog | Location |\n|---------|----------|\n",
            row: |it| {
                format!(
                    "| {} | {} |",
                    code_cell(it, "catalog_name"),
                    path_line_cell(it)
                )
            },
        },
        SectionSpec {
            name: "Unresolved catalog references",
            key: "unresolved_catalog_references",
            header: "Workspace `package.json` references to catalogs that do not declare the package. `pnpm install` will fail until each entry is added to its named catalog or the reference is switched.\n\n| Entry | Catalog | Location | Available in |\n|-------|---------|----------|--------------|\n",
            row: |it| {
                format!(
                    "| {} | {} | {} | {} |",
                    code_cell(it, "entry_name"),
                    code_cell(it, "catalog_name"),
                    path_line_cell(it),
                    backtick_join(it, "available_in_catalogs"),
                )
            },
        },
        SectionSpec {
            name: "Unused dependency overrides",
            key: "unused_dependency_overrides",
            header: "Package-manager override entries forcing a version no workspace package depends on. Some entries may be intentional pins for transitive CVEs; the hint column flags those.\n\n| Override | Forces | Source | Location | Hint |\n|----------|--------|--------|----------|------|\n",
            row: |it| {
                format!(
                    "| {} | {} -> {} | {} | {} | {} |",
                    code_cell(it, "raw_key"),
                    code_cell(it, "target_package"),
                    code_cell(it, "version_range"),
                    code_cell(it, "source"),
                    path_line_cell(it),
                    markdown_table_text(str_or(it, "hint", "")),
                )
            },
        },
        SectionSpec {
            name: "Misconfigured dependency overrides",
            key: "misconfigured_dependency_overrides",
            header: "Package-manager override entries with an unparsable key or empty value. The active package manager will reject or ignore these.\n\n| Override | Value | Source | Location | Reason |\n|----------|-------|--------|----------|--------|\n",
            row: |it| {
                format!(
                    "| {} | {} | {} | {} | {} |",
                    markdown_table_code_span(str_or(it, "raw_key", "")),
                    markdown_table_code_span(str_or(it, "raw_value", "")),
                    code_cell(it, "source"),
                    path_line_cell(it),
                    markdown_table_text(str_or(it, "reason", "unparsable")),
                )
            },
        },
    ]
}
