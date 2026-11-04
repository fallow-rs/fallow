//! Legacy baseline key builders that tests use to write old baseline files.

use super::*;

pub(super) fn baseline_file_export_keys(
    results: &crate::results::AnalysisResults,
    root: &Path,
) -> BaselineFileExportKeys {
    BaselineFileExportKeys {
        unused_files: results
            .unused_files
            .iter()
            .map(|f| relative_path(&f.file.path, root))
            .collect(),
        unused_exports: unused_export_baseline_keys(&results.unused_exports, root),
        unused_types: unused_type_baseline_keys(&results.unused_types, root),
        private_type_leaks: private_type_leak_baseline_keys(&results.private_type_leaks, root),
        deprecated_exports_in_use: results
            .deprecated_exports_in_use
            .iter()
            .map(|e| deprecated_export_key(&e.export, root))
            .collect(),
        invalid_client_exports: invalid_client_export_baseline_keys(
            &results.invalid_client_exports,
            root,
        ),
        mixed_client_server_barrels: barrel_baseline_keys(
            &results.mixed_client_server_barrels,
            root,
        ),
        misplaced_directives: directive_baseline_keys(&results.misplaced_directives, root),
        route_collisions: route_collision_baseline_keys(&results.route_collisions, root),
        dynamic_segment_name_conflicts: results
            .dynamic_segment_name_conflicts
            .iter()
            .map(|c| {
                format!(
                    "{}:{}",
                    relative_path(&c.conflict.path, root),
                    c.conflict.position
                )
            })
            .collect(),
    }
}

pub(super) fn baseline_member_import_keys(
    results: &crate::results::AnalysisResults,
    root: &Path,
) -> BaselineMemberImportKeys {
    BaselineMemberImportKeys {
        unused_enum_members: enum_member_baseline_keys(&results.unused_enum_members, root),
        unused_class_members: class_member_baseline_keys(&results.unused_class_members, root),
        unused_store_members: store_member_baseline_keys(&results.unused_store_members, root),
        unprovided_injects: inject_baseline_keys(&results.unprovided_injects, root),
        unrendered_components: component_baseline_keys(&results.unrendered_components, root),
        unused_component_props: component_prop_baseline_keys(&results.unused_component_props, root),
        absent_component_props: absent_component_prop_baseline_keys(
            &results.absent_component_props,
            root,
        ),
        unused_component_emits: component_emit_baseline_keys(&results.unused_component_emits, root),
        unused_component_inputs: component_input_baseline_keys(
            &results.unused_component_inputs,
            root,
        ),
        unused_component_outputs: component_output_baseline_keys(
            &results.unused_component_outputs,
            root,
        ),
        unused_svelte_events: svelte_event_baseline_keys(&results.unused_svelte_events, root),
        unused_effect_layers: effect_layer_baseline_keys(&results.unused_effect_layers, root),
        unused_tagged_errors: tagged_error_baseline_keys(&results.unused_tagged_errors, root),
        unproduced_catch_tags: catch_tag_baseline_keys(&results.unproduced_catch_tags, root),
        unused_server_actions: server_action_baseline_keys(&results.unused_server_actions, root),
        unused_load_data_keys: load_data_key_baseline_keys(&results.unused_load_data_keys, root),
        unresolved_imports: unresolved_import_baseline_keys(&results.unresolved_imports, root),
        duplicate_exports: results
            .duplicate_exports
            .iter()
            .map(|d| duplicate_export_key(&d.export, root))
            .collect(),
        stale_suppressions: results
            .stale_suppressions
            .iter()
            .map(|s| stale_suppression_baseline_key(s, root))
            .collect(),
    }
}
