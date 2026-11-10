//! GitHub summary sections for framework and hygiene finding kinds.

use super::*;

#[expect(
    clippy::too_many_lines,
    reason = "a flat data table of 14 per-kind row templates ported 1:1 from summary-check.jq; splitting would obscure the correspondence"
)]
pub(super) fn check_sections_frameworks_and_hygiene() -> Vec<SectionSpec> {
    vec![
        SectionSpec {
            name: "Unrendered components",
            key: "unrendered_components",
            header: "Vue/Svelte components reachable in the module graph but rendered nowhere: no tag, no dynamic binding, no registration. A barrel re-export keeps them alive even though nothing instantiates them.\n\n| File | Component | Framework |\n|------|-----------|-----------|\n",
            row: |it| {
                format!(
                    "| {} | {} | {} |",
                    path_line_cell(it),
                    code_cell(it, "component_name"),
                    markdown_table_text(s(it, "framework")),
                )
            },
        },
        SectionSpec {
            name: "Absent optional component props",
            key: "absent_component_props",
            header: "Known reachable callers omit this used optional prop. Review defaults and API intent manually; static analysis does not prove runtime unreachability.\n\n| File | Component prop | Default | Inspected callers |\n|------|----------------|---------|-------------------|\n",
            row: |it| {
                let callers = arr(it, "inspected_call_sites")
                    .map(path_line_cell)
                    .collect::<Vec<_>>()
                    .join(", ");
                format!(
                    "| {} | {} | {} | {} |",
                    path_line_cell(it),
                    component_member_item(it, "prop_name"),
                    if it.get("has_default").and_then(Value::as_bool) == Some(true) {
                        "present"
                    } else {
                        "absent"
                    },
                    callers
                )
            },
        },
        SectionSpec {
            name: "Constant parameters",
            key: "constant_parameters",
            header: "Each visible call passes this parameter in one way. Review the parameter and its callers manually; static analysis does not prove that no caller outside the project exists.\n\n| File | Parameter | Reason | Value | Calls |\n|------|-----------|--------|-------|-------|\n",
            row: |it| {
                format!(
                    "| {} | {} | {} | {} | {} |",
                    path_line_cell(it),
                    markdown_table_code_span(&format!(
                        "{}({})",
                        s(it, "function_name"),
                        s(it, "parameter_name")
                    )),
                    markdown_table_text(s(it, "reason")),
                    it.get("value")
                        .and_then(|value| value.get("text"))
                        .and_then(Value::as_str)
                        .map_or_else(String::new, markdown_table_code_span),
                    num(it, "call_site_count")
                )
            },
        },
        SectionSpec {
            name: "Unused component props",
            key: "unused_component_props",
            header: "Vue `defineProps` props referenced nowhere inside their own single-file component (neither script nor template).\n\n| File | Component | Prop |\n|------|-----------|------|\n",
            row: |it| component_detail_row(it, "prop_name"),
        },
        SectionSpec {
            name: "Unused component emits",
            key: "unused_component_emits",
            header: "Vue `defineEmits` events emitted nowhere inside their own single-file component (no matching `emit()` call).\n\n| File | Component | Event |\n|------|-----------|-------|\n",
            row: |it| component_detail_row(it, "emit_name"),
        },
        SectionSpec {
            name: "Unused component inputs",
            key: "unused_component_inputs",
            header: "Angular `@Input()` / signal `input()` declarations read nowhere inside their own component (neither class body nor template).\n\n| File | Component | Input |\n|------|-----------|-------|\n",
            row: |it| component_detail_row(it, "input_name"),
        },
        SectionSpec {
            name: "Unused component outputs",
            key: "unused_component_outputs",
            header: "Angular `@Output()` / signal `output()` declarations emitted nowhere inside their own component (no matching `emit()` call).\n\n| File | Component | Output |\n|------|-----------|--------|\n",
            row: |it| component_detail_row(it, "output_name"),
        },
        SectionSpec {
            name: "Unused Svelte events",
            key: "unused_svelte_events",
            header: "Svelte components dispatching a `createEventDispatcher` event listened to nowhere in the project (cross-file dead-output direction).\n\n| File | Component | Event |\n|------|-----------|-------|\n",
            row: |it| component_detail_row(it, "event_name"),
        },
        SectionSpec {
            name: "Duplicate export aliases",
            key: "duplicate_export_aliases",
            header: "One binding exported under two or more names in one file.\n\n| File | Binding | Names |\n|------|---------|-------|\n",
            row: |it| {
                let names = arr(it, "export_names")
                    .filter_map(Value::as_str)
                    .map(markdown_table_code_span)
                    .collect::<Vec<_>>()
                    .join(", ");
                format!(
                    "| {} | {} | {names} |",
                    path_line_cell(it),
                    code_cell(it, "local_name")
                )
            },
        },
        SectionSpec {
            name: "Unused Effect layers",
            key: "unused_effect_layers",
            header: "Effect layers composed only into layers that nothing uses.\n\n| File | Layer |\n|------|-------|\n",
            row: |it| format!("| {} | {} |", path_line_cell(it), code_cell(it, "name")),
        },
        SectionSpec {
            name: "Unused tagged errors",
            key: "unused_tagged_errors",
            header: "Effect tagged errors that no code constructs or fails with.\n\n| File | Class | Tag |\n|------|-------|-----|\n",
            row: |it| {
                format!(
                    "| {} | {} | {} |",
                    path_line_cell(it),
                    code_cell(it, "name"),
                    code_cell(it, "tag")
                )
            },
        },
        SectionSpec {
            name: "Unproduced catch tags",
            key: "unproduced_catch_tags",
            header: "`catchTag` handlers for a project tag that no code produces.\n\n| File | Tag |\n|------|-----|\n",
            row: |it| format!("| {} | {} |", path_line_cell(it), code_cell(it, "tag")),
        },
        SectionSpec {
            name: "Unprovided injects",
            key: "unprovided_injects",
            header: "Vue `inject` / Svelte `getContext` calls for a key that no ancestor `provide` / `setContext` supplies.\n\n| File | Key | Framework |\n|------|-----|-----------|\n",
            row: |it| {
                format!(
                    "| {} | {} | {} |",
                    path_line_cell(it),
                    code_cell(it, "key_name"),
                    markdown_table_text(s(it, "framework")),
                )
            },
        },
        SectionSpec {
            name: "Unused load data keys",
            key: "unused_load_data_keys",
            header: "SvelteKit `load()` return-object keys read by no consumer (neither the sibling `+page.svelte` nor `$page.data`). The key runs a real server fetch / DB cost per request for data nothing renders.\n\n| File | Route | Key |\n|------|-------|-----|\n",
            row: |it| {
                format!(
                    "| {} | {} | {} |",
                    path_line_cell(it),
                    code_cell(it, "route_dir"),
                    code_cell(it, "key_name"),
                )
            },
        },
        SectionSpec {
            name: "Type-only dependencies",
            key: "type_only_dependencies",
            header: "Dependencies only used for type imports - consider moving to `devDependencies`.\n\n| Package |\n|---------|\n",
            row: package_row,
        },
        SectionSpec {
            name: "Test-only dependencies",
            key: "test_only_dependencies",
            header: "Production dependencies only imported by test files - consider moving to `devDependencies`.\n\n| Package |\n|---------|\n",
            row: package_row,
        },
        SectionSpec {
            name: "Dev dependencies used in production",
            key: "dev_dependencies_in_production",
            header: "`devDependencies` imported by production code at runtime - consider moving to `dependencies` so a production-only install does not break.\n\n| Package |\n|---------|\n",
            row: package_row,
        },
        SectionSpec {
            name: "Stale suppressions",
            key: "stale_suppressions",
            header: "Suppression comments or JSDoc tags that no longer match any active issue.\n\n| File | Line | Description |\n|------|-----:|-------------|\n",
            row: |it| {
                format!(
                    "| {} | {} | {} |",
                    code_cell(it, "path"),
                    num(it, "line"),
                    stale_suppression_description(it),
                )
            },
        },
    ]
}
