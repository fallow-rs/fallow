use std::fmt::Write as _;

use fallow_output::markdown_table_code_span;
use serde_json::Value;

use super::{AuditRow, arr, audit_row, code_cell, s, u};

#[must_use]
pub(super) fn prop_drilling_section(env: &Value) -> String {
    let rows = arr(env, "prop_drilling_chains")
        .filter(|item| s(item, "effective_severity") == "error")
        .filter_map(|item| {
            let source = arr(item, "hops").next()?;
            Some(format!(
                "| {} | {} | {} |",
                markdown_table_code_span(s(source, "file")),
                code_cell(item, "prop"),
                u(item, "depth")
            ))
        })
        .collect::<Vec<_>>();
    if rows.is_empty() {
        return String::new();
    }
    format!(
        "\n### Prop drilling\n\n| Source | Prop | Components |\n|:-------|:-----|-----------:|\n{}\n\nReview composition, colocation or context, or suppress the source prop declaration.\n",
        rows.join("\n")
    )
}

pub(super) fn component_shape_sections(env: &Value) -> String {
    let mut sections = String::new();
    for (collection, title) in [
        ("thin_wrappers", "Thin wrappers"),
        ("duplicate_prop_shapes", "Duplicate prop shapes"),
    ] {
        let rows = arr(env, collection)
            .filter(|item| s(item, "effective_severity") == "error")
            .map(|item| {
                format!(
                    "| {} | {} |",
                    markdown_table_code_span(s(item, "file")),
                    code_cell(item, "component")
                )
            })
            .collect::<Vec<_>>();
        if !rows.is_empty() {
            let _ = write!(
                sections,
                "\n### {title}\n\n| File | Component |\n|:-----|:----------|\n{}\n",
                rows.join("\n")
            );
        }
    }
    sections
}

pub(super) fn push_audit_component_health_rows(dead_code: &Value, rows: &mut Vec<AuditRow>) {
    for item in arr(dead_code, "prop_drilling_chains")
        .filter(|item| s(item, "effective_severity") == "error")
    {
        let Some(source) = arr(item, "hops").next() else {
            continue;
        };
        rows.push(audit_row(
            "Prop drilling",
            markdown_table_code_span(s(source, "file")),
            code_cell(item, "prop"),
            item,
        ));
    }
    for (collection, title) in [
        ("thin_wrappers", "Thin wrapper"),
        ("duplicate_prop_shapes", "Duplicate prop shape"),
    ] {
        for item in
            arr(dead_code, collection).filter(|item| s(item, "effective_severity") == "error")
        {
            rows.push(audit_row(
                title,
                markdown_table_code_span(s(item, "file")),
                code_cell(item, "component"),
                item,
            ));
        }
    }
}
