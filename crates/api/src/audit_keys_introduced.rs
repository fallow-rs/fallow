//! Classification of the dead-code fields that an audit run introduced.

pub(super) fn classify_introduced_dead_code_fields(
    results: &fallow_types::results::AnalysisResults,
) {
    let fallow_types::results::AnalysisResults {
        shared_state: _,
        unused_files: _unused_files,
        unused_exports: _unused_exports,
        unused_types: _unused_types,
        private_type_leaks: _private_type_leaks,
        deprecated_exports_in_use: _deprecated_exports_in_use,
        unused_dependencies: _unused_dependencies,
        unused_dev_dependencies: _unused_dev_dependencies,
        unused_optional_dependencies: _unused_optional_dependencies,
        unused_enum_members: _unused_enum_members,
        unused_class_members: _unused_class_members,
        unused_namespace_members: _unused_namespace_members,
        unused_protected_class_members: _unused_protected_class_members,
        unused_store_members: _unused_store_members,
        unresolved_imports: _unresolved_imports,
        unlisted_dependencies: _unlisted_dependencies,
        unlisted_binaries: _unlisted_binaries,
        duplicate_exports: _duplicate_exports,
        duplicate_export_aliases: _duplicate_export_aliases,
        type_only_dependencies: _type_only_dependencies,
        test_only_dependencies: _test_only_dependencies,
        dev_dependencies_in_production: _dev_dependencies_in_production,
        circular_dependencies: _circular_dependencies,
        re_export_cycles: _re_export_cycles,
        package_cycles: _package_cycles,
        boundary_violations: _boundary_violations,
        boundary_coverage_violations: _boundary_coverage_violations,
        boundary_call_violations: _boundary_call_violations,
        policy_violations: _policy_violations,
        stale_suppressions: _stale_suppressions,
        unused_catalog_entries: _unused_catalog_entries,
        empty_catalog_groups: _empty_catalog_groups,
        unresolved_catalog_references: _unresolved_catalog_references,
        unused_dependency_overrides: _unused_dependency_overrides,
        misconfigured_dependency_overrides: _misconfigured_dependency_overrides,
        invalid_client_exports: _invalid_client_exports,
        mixed_client_server_barrels: _mixed_client_server_barrels,
        misplaced_directives: _misplaced_directives,
        unprovided_injects: _unprovided_injects,
        unrendered_components: _unrendered_components,
        unused_component_props: _unused_component_props,
        absent_component_props: _absent_component_props,
        constant_parameters: _constant_parameters,
        unused_component_emits: _unused_component_emits,
        unused_component_inputs: _unused_component_inputs,
        unused_component_outputs: _unused_component_outputs,
        unused_svelte_events: _unused_svelte_events,
        unused_effect_layers: _unused_effect_layers,
        unused_tagged_errors: _unused_tagged_errors,
        unproduced_catch_tags: _unproduced_catch_tags,
        unused_server_actions: _unused_server_actions,
        unused_load_data_keys: _unused_load_data_keys,
        unused_load_data_keys_global_abstain: _unused_load_data_keys_global_abstain,
        route_collisions: _route_collisions,
        dynamic_segment_name_conflicts: _dynamic_segment_name_conflicts,
        // Non-finding fields: counts and metadata, not subject to base-keyed
        // filtering.
        cascade_hidden: _cascade_hidden,
        cascade: _cascade,
        suppression_count: _suppression_count,
        unused_component_props_exempted: _unused_component_props_exempted,
        active_suppressions: _active_suppressions,
        feature_flags: _feature_flags,
        // Security findings are emitted via `fallow security`, not the audit
        // dead-code gate; they have no key representation and are not filtered
        // here.
        security_findings: _security_findings,
        suppressed_security_findings: _suppressed_security_findings,
        security_unresolved_edge_files: _security_unresolved_edge_files,
        security_unresolved_callee_sites: _security_unresolved_callee_sites,
        security_unresolved_callee_diagnostics: _security_unresolved_callee_diagnostics,
        // Prop chains use source identities and occurrence-aware audit filtering.
        prop_drilling_chains: _prop_drilling_chains,
        // Component health findings use source identities and occurrence-aware filtering.
        thin_wrappers: _thin_wrappers,
        duplicate_prop_shapes: _duplicate_prop_shapes,
        // Export usages and entry-point summary are metadata, not issue
        // collections; no key needed.
        export_usages: _export_usages,
        entry_point_summary: _entry_point_summary,
        // Render fan-in is a whole-project descriptive metric, not an issue
        // collection; no key needed.
        render_fan_in: _render_fan_in,
        // Per-component React intel is a descriptive ambient-editor carrier, not
        // an issue collection; no key needed.
        react_component_intel: _react_component_intel,
        semantic_reachable_files: _,
        semantic_framework_contracts: _semantic_framework_contracts,
        constant_parameter_candidates: _constant_parameter_candidates,
    } = results;
}
