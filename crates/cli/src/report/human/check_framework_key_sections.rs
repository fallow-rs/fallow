//! Human report sections for framework key findings.

use super::*;

/// Render the framework-key policy sections (Svelte events, server actions,
/// load data keys).
pub(super) fn push_framework_key_sections(
    lines: &mut Vec<String>,
    results: &AnalysisResults,
    root: &Path,
    rules: &RulesConfig,
) {
    build_human_grouped_section(GroupedSectionInput {
        lines,
        items: &results.unused_svelte_events,
        title: "Unused Svelte events",
        level: severity_to_level(rules.unused_svelte_events),
        root,
        max_files: MAX_FLAT_ITEMS,
        get_path: |e: &fallow_types::output_dead_code::UnusedSvelteEventFinding| {
            e.event.path.as_path()
        },
        format_detail: &format_unused_svelte_event,
    });

    build_human_grouped_section(GroupedSectionInput {
        lines,
        items: &results.unused_effect_layers,
        title: "Unused Effect layers",
        level: severity_to_level(rules.unused_effect_layers),
        root,
        max_files: MAX_FLAT_ITEMS,
        get_path: |e: &fallow_types::output_dead_code::UnusedEffectLayerFinding| {
            e.layer.path.as_path()
        },
        format_detail: &format_unused_effect_layer,
    });

    build_human_grouped_section(GroupedSectionInput {
        lines,
        items: &results.unused_tagged_errors,
        title: "Unused tagged errors",
        level: severity_to_level(rules.unused_tagged_errors),
        root,
        max_files: MAX_FLAT_ITEMS,
        get_path: |e: &fallow_types::output_dead_code::UnusedTaggedErrorFinding| {
            e.error.path.as_path()
        },
        format_detail: &format_unused_tagged_error,
    });

    build_human_grouped_section(GroupedSectionInput {
        lines,
        items: &results.unproduced_catch_tags,
        title: "Unproduced catch tags",
        level: severity_to_level(rules.unproduced_catch_tags),
        root,
        max_files: MAX_FLAT_ITEMS,
        get_path: |e: &fallow_types::output_dead_code::UnproducedCatchTagFinding| {
            e.catch_tag.path.as_path()
        },
        format_detail: &format_unproduced_catch_tag,
    });

    build_human_grouped_section(GroupedSectionInput {
        lines,
        items: &results.unused_server_actions,
        title: "Unused server actions",
        level: severity_to_level(rules.unused_server_actions),
        root,
        max_files: MAX_FLAT_ITEMS,
        get_path: |a: &fallow_types::output_dead_code::UnusedServerActionFinding| {
            a.action.path.as_path()
        },
        format_detail: &format_unused_server_action,
    });

    build_human_grouped_section(GroupedSectionInput {
        lines,
        items: &results.unused_load_data_keys,
        title: "Unused load data keys",
        level: severity_to_level(rules.unused_load_data_keys),
        root,
        max_files: MAX_FLAT_ITEMS,
        get_path: |k: &fallow_types::output_dead_code::UnusedLoadDataKeyFinding| {
            k.key.path.as_path()
        },
        format_detail: &format_unused_load_data_key,
    });
}
