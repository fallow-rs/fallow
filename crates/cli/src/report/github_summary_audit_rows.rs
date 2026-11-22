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

/// jq order positions 20-25: RSC/Next.js kinds, including unused server
/// actions between misplaced directives and route collisions.
pub(super) fn audit_rows_frameworks(dead_code: &Value, rows: &mut Vec<AuditRow>) {
    for it in arr(dead_code, "invalid_client_exports") {
        rows.push(audit_row(
            "Invalid client export",
            rel_path_line_cell(it, "path"),
            format!(
                "{} in {}",
                code_cell(it, "export_name"),
                markdown_table_code_span(&format!("\"{}\"", s(it, "directive")))
            ),
            it,
        ));
    }
    for it in arr(dead_code, "mixed_client_server_barrels") {
        rows.push(audit_row(
            "Mixed client/server barrel",
            rel_path_line_cell(it, "path"),
            format!(
                "{} + {}",
                code_cell(it, "client_origin"),
                code_cell(it, "server_origin")
            ),
            it,
        ));
    }
    for it in arr(dead_code, "misplaced_directives") {
        rows.push(audit_row(
            "Misplaced directive",
            rel_path_line_cell(it, "path"),
            markdown_table_code_span(&format!("\"{}\"", s(it, "directive"))),
            it,
        ));
    }
    for it in arr(dead_code, "unused_server_actions") {
        rows.push(audit_row(
            "Unused server action",
            path_line(it),
            code_cell(it, "action_name"),
            it,
        ));
    }
    for it in arr(dead_code, "route_collisions") {
        rows.push(audit_row(
            "Route collision",
            markdown_table_code_span(&rel_path_absolute_only(s(it, "path"))),
            code_cell(it, "url"),
            it,
        ));
    }
    for it in arr(dead_code, "dynamic_segment_name_conflicts") {
        rows.push(audit_row(
            "Dynamic segment conflict",
            markdown_table_code_span(&rel_path_absolute_only(s(it, "path"))),
            markdown_table_code_span(&plain_join(it, "conflicting_segments", ", ")),
            it,
        ));
    }
}
