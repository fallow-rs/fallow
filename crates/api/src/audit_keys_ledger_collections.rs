//! The exhaustive list of dead-code collections of the audit ledger.

/// Visit every dead-code collection that an audit ledger record can name.
///
/// The exhaustive destructure stops the build when `AnalysisResults` gains a
/// field, so each new finding collection needs an explicit audit decision.
/// `StaleSuppression` is the sole finding without a typed `introduced` slot,
/// so the callback gets it through the `@no_slot` arm.
macro_rules! for_each_ledger_collection {
    ($results:expr, $apply:ident) => {{
            let fallow_types::results::AnalysisResults {
                shared_state: _,
                unused_files,
                unused_exports,
                unused_types,
                private_type_leaks,
                deprecated_exports_in_use,
                unused_dependencies,
                unused_dev_dependencies,
                unused_optional_dependencies,
                unused_enum_members,
                unused_class_members,
                unused_namespace_members,
                unused_protected_class_members,
                unused_store_members,
                unresolved_imports,
                unlisted_dependencies,
                unlisted_binaries,
                duplicate_exports,
                duplicate_export_aliases,
                type_only_dependencies,
                test_only_dependencies,
                dev_dependencies_in_production,
                circular_dependencies,
                re_export_cycles,
                package_cycles,
                boundary_violations,
                boundary_coverage_violations,
                boundary_call_violations,
                policy_violations,
                stale_suppressions,
                unused_catalog_entries,
                empty_catalog_groups,
                unresolved_catalog_references,
                unused_dependency_overrides,
                misconfigured_dependency_overrides,
                invalid_client_exports,
                mixed_client_server_barrels,
                misplaced_directives,
                unprovided_injects,
                unrendered_components,
                unused_component_props,
                absent_component_props,
                constant_parameters,
                unused_component_emits,
                unused_component_inputs,
                unused_component_outputs,
                unused_svelte_events,
                unused_effect_layers,
                unused_tagged_errors,
                unproduced_catch_tags,
                unused_server_actions,
                unused_load_data_keys,
                unused_load_data_keys_global_abstain: _unused_load_data_keys_global_abstain,
                route_collisions,
                dynamic_segment_name_conflicts,
                cascade_hidden: _cascade_hidden,
                cascade: _cascade,
                suppression_count: _suppression_count,
                unused_component_props_exempted: _unused_component_props_exempted,
                active_suppressions: _active_suppressions,
                feature_flags: _feature_flags,
                security_findings: _security_findings,
                suppressed_security_findings: _suppressed_security_findings,
                security_unresolved_edge_files: _security_unresolved_edge_files,
                security_unresolved_callee_sites: _security_unresolved_callee_sites,
                security_unresolved_callee_diagnostics: _security_unresolved_callee_diagnostics,
                prop_drilling_chains,
                thin_wrappers,
                duplicate_prop_shapes,
                export_usages: _export_usages,
                entry_point_summary: _entry_point_summary,
                render_fan_in: _render_fan_in,
                react_component_intel: _react_component_intel,
                semantic_reachable_files: _,
                semantic_framework_contracts: _semantic_framework_contracts,
                constant_parameter_candidates: _constant_parameter_candidates,
            } = $results;
            $apply!(prop_drilling_chains, "prop_drilling_chains");
            $apply!(thin_wrappers, "thin_wrappers");
            $apply!(duplicate_prop_shapes, "duplicate_prop_shapes");
            $apply!(unused_files, "unused_files");
            $apply!(unused_exports, "unused_exports");
            $apply!(unused_types, "unused_types");
            $apply!(private_type_leaks, "private_type_leaks");
            $apply!(deprecated_exports_in_use, "deprecated_exports_in_use");
            $apply!(unused_dependencies, "unused_dependencies");
            $apply!(unused_dev_dependencies, "unused_dev_dependencies");
            $apply!(unused_optional_dependencies, "unused_optional_dependencies");
            $apply!(unused_enum_members, "unused_enum_members");
            $apply!(unused_class_members, "unused_class_members");
            $apply!(unused_namespace_members, "unused_namespace_members");
            $apply!(
                unused_protected_class_members,
                "unused_protected_class_members"
            );
            $apply!(unused_store_members, "unused_store_members");
            $apply!(unresolved_imports, "unresolved_imports");
            $apply!(unlisted_dependencies, "unlisted_dependencies");
            $apply!(unlisted_binaries, "unlisted_binaries");
            $apply!(duplicate_exports, "duplicate_exports");
            $apply!(duplicate_export_aliases, "duplicate_export_aliases");
            $apply!(type_only_dependencies, "type_only_dependencies");
            $apply!(test_only_dependencies, "test_only_dependencies");
            $apply!(dev_dependencies_in_production, "dev_dependencies_in_production");
            $apply!(circular_dependencies, "circular_dependencies");
            $apply!(re_export_cycles, "re_export_cycles");
            $apply!(package_cycles, "package_cycles");
            $apply!(boundary_violations, "boundary_violations");
            $apply!(boundary_coverage_violations, "boundary_coverage_violations");
            $apply!(boundary_call_violations, "boundary_call_violations");
            $apply!(policy_violations, "policy_violations");
            $apply!(unused_catalog_entries, "unused_catalog_entries");
            $apply!(empty_catalog_groups, "empty_catalog_groups");
            $apply!(unresolved_catalog_references, "unresolved_catalog_references");
            $apply!(unused_dependency_overrides, "unused_dependency_overrides");
            $apply!(misconfigured_dependency_overrides, "misconfigured_dependency_overrides");
            $apply!(invalid_client_exports, "invalid_client_exports");
            $apply!(mixed_client_server_barrels, "mixed_client_server_barrels");
            $apply!(misplaced_directives, "misplaced_directives");
            $apply!(unprovided_injects, "unprovided_injects");
            $apply!(unrendered_components, "unrendered_components");
            $apply!(route_collisions, "route_collisions");
            $apply!(dynamic_segment_name_conflicts, "dynamic_segment_name_conflicts");
            $apply!(unused_component_props, "unused_component_props");
            $apply!(absent_component_props, "absent_component_props");
            $apply!(constant_parameters, "constant_parameters");
            $apply!(unused_component_emits, "unused_component_emits");
            $apply!(unused_component_inputs, "unused_component_inputs");
            $apply!(unused_component_outputs, "unused_component_outputs");
            $apply!(unused_svelte_events, "unused_svelte_events");
            $apply!(unused_effect_layers, "unused_effect_layers");
            $apply!(unused_tagged_errors, "unused_tagged_errors");
            $apply!(unproduced_catch_tags, "unproduced_catch_tags");
            $apply!(unused_server_actions, "unused_server_actions");
            $apply!(unused_load_data_keys, "unused_load_data_keys");
            $apply!(@no_slot stale_suppressions, "stale_suppressions");
    }};
}
