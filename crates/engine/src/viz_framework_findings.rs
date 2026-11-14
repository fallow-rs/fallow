//! Visualizer findings of framework and component rules.

use super::*;

pub(super) fn build_framework_findings(
    results: &AnalysisResults,
    index: &FileIndex<'_>,
    root: &Path,
) -> VizFindingAnalysis {
    let mut findings = Vec::new();
    let mut count = 0;
    macro_rules! add {
        ($field:ident, $kind:literal, $title:literal) => {
            count += results.$field.len();
            push_findings(&mut findings, $kind, $title, &results.$field, root, index);
        };
    }
    add!(
        invalid_client_exports,
        "invalid-client-export",
        "Invalid client export"
    );
    add!(
        mixed_client_server_barrels,
        "mixed-client-server-barrel",
        "Mixed client/server barrel"
    );
    add!(
        misplaced_directives,
        "misplaced-directive",
        "Misplaced framework directive"
    );
    add!(
        unprovided_injects,
        "unprovided-inject",
        "Injected value is never provided"
    );
    add!(
        unrendered_components,
        "unrendered-component",
        "Component is never rendered"
    );
    add!(route_collisions, "route-collision", "Route collision");
    add!(
        dynamic_segment_name_conflicts,
        "dynamic-segment-conflict",
        "Dynamic segment conflict"
    );
    add!(
        unused_component_props,
        "unused-component-prop",
        "Unused component prop"
    );
    add!(
        absent_component_props,
        "absent-component-prop",
        "Optional prop review candidate"
    );
    add!(
        constant_parameters,
        "constant-parameter",
        "Constant parameter review candidate"
    );
    add!(
        unused_component_emits,
        "unused-component-emit",
        "Unused component event"
    );
    add!(
        unused_component_inputs,
        "unused-component-input",
        "Unused component input"
    );
    add!(
        unused_component_outputs,
        "unused-component-output",
        "Unused component output"
    );
    add!(
        unused_svelte_events,
        "unused-svelte-event",
        "Unused Svelte event"
    );
    count += push_effect_findings(&mut findings, results, index, root);
    add!(
        unused_server_actions,
        "unused-server-action",
        "Unused server action"
    );
    add!(
        unused_load_data_keys,
        "unused-load-data-key",
        "Unused load-data key"
    );
    add!(prop_drilling_chains, "prop-drilling", "Prop-drilling chain");
    add!(thin_wrappers, "thin-wrapper", "Thin component wrapper");
    add!(
        duplicate_prop_shapes,
        "duplicate-prop-shape",
        "Duplicate prop shape"
    );
    let mut analysis = analysis_from_records(findings, count, count, "findings");
    if results.unused_load_data_keys_global_abstain {
        analysis.availability.reason = Some(
            "Load-data-key analysis abstained because whole-object page data usage was detected"
                .to_string(),
        );
    }
    analysis
}
