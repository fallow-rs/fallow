//! Step summary sections for architecture findings.

use super::*;

#[expect(
    clippy::too_many_lines,
    reason = "a flat data table of 14 per-kind row templates ported 1:1 from summary-check.jq; splitting would obscure the correspondence"
)]
pub(super) fn check_sections_architecture() -> Vec<SectionSpec> {
    vec![
        SectionSpec {
            name: "Re-export cycles",
            key: "re_export_cycles",
            header: "Barrel files that re-export from each other in a loop. Chain propagation through the loop is a no-op, so imports through any member may silently come up empty.\n\n| Cycle | Kind | Members |\n|-------|------|--------:|\n",
            row: |it| {
                let cycle = arr(it, "files")
                    .filter_map(Value::as_str)
                    .map(markdown_table_code_span)
                    .collect::<Vec<_>>()
                    .join(" <-> ");
                format!(
                    "| {cycle} | {} | {} |",
                    markdown_table_text(s(it, "kind")),
                    arr(it, "files").count()
                )
            },
        },
        SectionSpec {
            name: "Package cycles",
            key: "package_cycles",
            header: "Workspace packages that import each other in a loop. Packages in a cycle cannot be built in dependency order.\n\n| Cycle | Example import | Packages |\n|-------|----------------|---------:|\n",
            row: |it| {
                let mut chain: Vec<String> = arr(it, "packages")
                    .filter_map(Value::as_str)
                    .map(markdown_table_code_span)
                    .collect();
                if let Some(first) = chain.first().cloned() {
                    chain.push(first);
                }
                let example = arr(it, "edges")
                    .next()
                    .map_or_else(String::new, path_line_cell);
                let note = if it.get("group_truncated").and_then(Value::as_bool) == Some(true) {
                    format!(
                        " ({})",
                        fallow_types::results::PackageCycle::GROUP_TRUNCATED_NOTE
                    )
                } else {
                    String::new()
                };
                format!(
                    "| {}{note} | {example} | {} |",
                    chain.join(" \u{2192} "),
                    arr(it, "packages").count()
                )
            },
        },
        SectionSpec {
            name: "Boundary violations",
            key: "boundary_violations",
            header: "Imports that cross defined architecture zone boundaries.\n\n| From | To | Zones |\n|------|-----|-------|\n",
            row: |it| {
                format!(
                    "| {} | {} | {} \u{2192} {} |",
                    markdown_table_code_span(&format!(
                        "{}:{}",
                        s(it, "from_path"),
                        num(it, "line")
                    )),
                    code_cell(it, "to_path"),
                    markdown_table_code_span(s(it, "from_zone")),
                    markdown_table_code_span(s(it, "to_zone")),
                )
            },
        },
        SectionSpec {
            name: "Boundary coverage",
            key: "boundary_coverage_violations",
            header: "Files that match no configured architecture boundary zone.\n\n| File |\n|------|\n",
            row: |it| format!("| {} |", path_line_cell(it)),
        },
        SectionSpec {
            name: "Boundary calls",
            key: "boundary_call_violations",
            header: "Calls from zoned files to callees forbidden for that zone.\n\n| File | Callee | Zone | Pattern |\n|------|--------|------|---------|\n",
            row: |it| {
                format!(
                    "| {} | {} | {} | {} |",
                    path_line_cell(it),
                    code_cell(it, "callee"),
                    markdown_table_code_span(s(it, "zone")),
                    code_cell(it, "pattern"),
                )
            },
        },
        SectionSpec {
            name: "Policy violations",
            key: "policy_violations",
            header: "Banned calls, imports, and catalogue-derived effects matched by configured rule packs.\n\n| File | Matched | Rule | Severity |\n|------|---------|------|----------|\n",
            row: |it| {
                format!(
                    "| {} | {} | {} | {} |",
                    path_line_cell(it),
                    code_cell(it, "matched"),
                    markdown_table_code_span(&format!("{}/{}", s(it, "pack"), s(it, "rule_id"))),
                    markdown_table_text(s(it, "severity")),
                )
            },
        },
        SectionSpec {
            name: "Invalid client exports",
            key: "invalid_client_exports",
            header: "`\"use client\"` files exporting a Next.js server-only / route-config name. Next.js rejects this at build time.\n\n| File | Export | Directive |\n|------|--------|-----------|\n",
            row: |it| {
                format!(
                    "| {} | {} | {} |",
                    path_line_cell(it),
                    code_cell(it, "export_name"),
                    markdown_table_code_span(&format!("\"{}\"", s(it, "directive"))),
                )
            },
        },
        SectionSpec {
            name: "Mixed client/server barrels",
            key: "mixed_client_server_barrels",
            header: "Barrels re-exporting both a `\"use client\"` module and a server-only module. One import drags the other's directive across the boundary.\n\n| File | Client origin | Server origin |\n|------|---------------|---------------|\n",
            row: |it| {
                format!(
                    "| {} | {} | {} |",
                    path_line_cell(it),
                    code_cell(it, "client_origin"),
                    code_cell(it, "server_origin"),
                )
            },
        },
        SectionSpec {
            name: "Misplaced directives",
            key: "misplaced_directives",
            header: "`\"use client\"` / `\"use server\"` directives written after a non-directive statement, so the RSC bundler ignores them. Move the directive to the top of the file.\n\n| File | Directive |\n|------|-----------|\n",
            row: |it| {
                format!(
                    "| {} | {} |",
                    path_line_cell(it),
                    markdown_table_code_span(&format!("\"{}\"", s(it, "directive"))),
                )
            },
        },
        SectionSpec {
            name: "Unused server actions",
            key: "unused_server_actions",
            header: "Next.js Server Actions (exports of a `\"use server\"` file) that no project code references. The endpoint stays POST-able, but no code calls it (likely dead).\n\n| File | Action |\n|------|--------|\n",
            row: |it| {
                format!(
                    "| {} | {} |",
                    path_line_cell(it),
                    code_cell(it, "action_name")
                )
            },
        },
        SectionSpec {
            name: "Route collisions",
            key: "route_collisions",
            header: "Next.js App Router route files that resolve to the same URL within one app-root. Next.js fails the build because a URL can have only one owner.\n\n| File | URL |\n|------|-----|\n",
            row: |it| format!("| {} | {} |", code_cell(it, "path"), code_cell(it, "url")),
        },
        SectionSpec {
            name: "Dynamic segment conflicts",
            key: "dynamic_segment_name_conflicts",
            header: "Sibling Next.js dynamic route segments at one position using different slug names. Next.js requires one consistent name per dynamic path.\n\n| File | Position | Segments |\n|------|----------|----------|\n",
            row: |it| {
                format!(
                    "| {} | {} | {} |",
                    code_cell(it, "path"),
                    code_cell(it, "position"),
                    markdown_table_code_span(&plain_join(it, "conflicting_segments", ", ")),
                )
            },
        },
    ]
}
