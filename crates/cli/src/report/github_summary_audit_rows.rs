//! Graph rows of the audit section in the GitHub summary.

use super::*;

/// jq order positions 12-15: unlisted dependencies, duplicate exports,
/// circular dependencies, re-export cycles.
pub(super) fn audit_rows_graph(dead_code: &Value, rows: &mut Vec<AuditRow>) {
    for it in arr(dead_code, "unlisted_dependencies") {
        rows.push(audit_row(
            "Unlisted dependency",
            first_import_site(it),
            code_cell(it, "package_name"),
            it,
        ));
    }
    for it in arr(dead_code, "unlisted_binaries") {
        let site = arr(it, "used_in").next().map_or_else(String::new, |site| {
            markdown_table_code_span(&format!(
                "{}:{}",
                rel_path_absolute_only(s(site, "path")),
                num(site, "line"),
            ))
        });
        rows.push(audit_row(
            "Unlisted binary",
            site,
            code_cell(it, "binary_name"),
            it,
        ));
    }
    for it in arr(dead_code, "duplicate_exports") {
        let location = arr(it, "locations")
            .take(3)
            .map(|loc| {
                markdown_table_code_span(&format!(
                    "{}:{}",
                    rel_path_absolute_only(s(loc, "path")),
                    num(loc, "line")
                ))
            })
            .collect::<Vec<_>>()
            .join(", ");
        rows.push(audit_row(
            "Duplicate export",
            location,
            code_cell(it, "export_name"),
            it,
        ));
    }
    for it in arr(dead_code, "circular_dependencies") {
        let location = arr(it, "files")
            .filter_map(Value::as_str)
            .map(|file| markdown_table_code_span(&rel_path_absolute_only(file)))
            .collect::<Vec<_>>()
            .join(" -> ");
        rows.push(audit_row(
            "Circular dependency",
            location,
            "cycle".to_owned(),
            it,
        ));
    }
    for it in arr(dead_code, "re_export_cycles") {
        let location = arr(it, "files")
            .filter_map(Value::as_str)
            .map(|file| markdown_table_code_span(&rel_path_absolute_only(file)))
            .collect::<Vec<_>>()
            .join(" <-> ");
        rows.push(audit_row(
            "Re-export cycle",
            location,
            markdown_table_text(str_or(it, "kind", "cycle")),
            it,
        ));
    }
    for it in arr(dead_code, "package_cycles") {
        let location = arr(it, "packages")
            .filter_map(Value::as_str)
            .map(markdown_table_code_span)
            .collect::<Vec<_>>()
            .join(" -> ");
        let detail = if it.get("group_truncated").and_then(Value::as_bool) == Some(true) {
            format!(
                "cycle ({})",
                fallow_types::results::PackageCycle::GROUP_TRUNCATED_NOTE
            )
        } else {
            "cycle".to_owned()
        };
        rows.push(audit_row("Package cycle", location, detail, it));
    }
}
