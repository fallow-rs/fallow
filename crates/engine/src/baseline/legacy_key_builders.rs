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
        unused_namespace_members: namespace_member_baseline_keys(
            &results.unused_namespace_members,
            root,
        ),
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

fn unused_export_baseline_keys(
    items: &[crate::results::UnusedExportFinding],
    root: &Path,
) -> Vec<String> {
    items
        .iter()
        .map(|e| {
            format!(
                "{}:{}",
                relative_path(&e.export.path, root),
                e.export.export_name
            )
        })
        .collect()
}

fn unused_type_baseline_keys(
    items: &[crate::results::UnusedTypeFinding],
    root: &Path,
) -> Vec<String> {
    items
        .iter()
        .map(|e| {
            format!(
                "{}:{}",
                relative_path(&e.export.path, root),
                e.export.export_name
            )
        })
        .collect()
}

fn invalid_client_export_baseline_keys(
    items: &[crate::results::InvalidClientExportFinding],
    root: &Path,
) -> Vec<String> {
    items
        .iter()
        .map(|e| {
            format!(
                "{}:{}",
                relative_path(&e.export.path, root),
                e.export.export_name
            )
        })
        .collect()
}

fn private_type_leak_baseline_keys(
    items: &[crate::results::PrivateTypeLeakFinding],
    root: &Path,
) -> Vec<String> {
    items
        .iter()
        .map(|e| {
            format!(
                "{}:{}->{}",
                relative_path(&e.leak.path, root),
                e.leak.export_name,
                e.leak.type_name
            )
        })
        .collect()
}

fn barrel_baseline_keys(
    items: &[crate::results::MixedClientServerBarrelFinding],
    root: &Path,
) -> Vec<String> {
    items
        .iter()
        .map(|b| {
            format!(
                "{}:{}:{}",
                relative_path(&b.barrel.path, root),
                b.barrel.client_origin,
                b.barrel.server_origin
            )
        })
        .collect()
}

fn directive_baseline_keys(
    items: &[crate::results::MisplacedDirectiveFinding],
    root: &Path,
) -> Vec<String> {
    items
        .iter()
        .map(|d| {
            format!(
                "{}:{}:{}",
                relative_path(&d.directive_site.path, root),
                d.directive_site.line,
                d.directive_site.directive
            )
        })
        .collect()
}

fn route_collision_baseline_keys(
    items: &[crate::results::RouteCollisionFinding],
    root: &Path,
) -> Vec<String> {
    items
        .iter()
        .map(|c| {
            format!(
                "{}:{}",
                relative_path(&c.collision.path, root),
                c.collision.url
            )
        })
        .collect()
}

fn enum_member_baseline_keys(
    items: &[crate::results::UnusedEnumMemberFinding],
    root: &Path,
) -> Vec<String> {
    items
        .iter()
        .map(|m| unused_member_baseline_key(&m.member, root))
        .collect()
}

fn class_member_baseline_keys(
    items: &[crate::results::UnusedClassMemberFinding],
    root: &Path,
) -> Vec<String> {
    items
        .iter()
        .map(|m| unused_member_baseline_key(&m.member, root))
        .collect()
}

fn store_member_baseline_keys(
    items: &[crate::results::UnusedStoreMemberFinding],
    root: &Path,
) -> Vec<String> {
    items
        .iter()
        .map(|m| unused_member_baseline_key(&m.member, root))
        .collect()
}

fn namespace_member_baseline_keys(
    items: &[crate::results::UnusedNamespaceMemberFinding],
    root: &Path,
) -> Vec<String> {
    items
        .iter()
        .map(|m| unused_member_baseline_key(&m.member, root))
        .collect()
}

fn unused_member_baseline_key(member: &crate::results::UnusedMember, root: &Path) -> String {
    format!(
        "{}:{}.{}",
        relative_path(&member.path, root),
        member.parent_name,
        member.member_name
    )
}

fn server_action_baseline_keys(
    items: &[crate::results::UnusedServerActionFinding],
    root: &Path,
) -> Vec<String> {
    items
        .iter()
        .map(|a| {
            format!(
                "{}:{}",
                relative_path(&a.action.path, root),
                a.action.action_name
            )
        })
        .collect()
}

fn load_data_key_baseline_keys(
    items: &[crate::results::UnusedLoadDataKeyFinding],
    root: &Path,
) -> Vec<String> {
    items
        .iter()
        .map(|k| format!("{}:{}", relative_path(&k.key.path, root), k.key.key_name))
        .collect()
}

fn unresolved_import_baseline_keys(
    items: &[crate::results::UnresolvedImportFinding],
    root: &Path,
) -> Vec<String> {
    items
        .iter()
        .map(|i| {
            format!(
                "{}:{}",
                relative_path(&i.import.path, root),
                i.import.specifier
            )
        })
        .collect()
}

pub(super) fn baseline_dependency_keys(
    results: &crate::results::AnalysisResults,
    root: &Path,
) -> BaselineDependencyKeys {
    BaselineDependencyKeys {
        unused: results
            .unused_dependencies
            .iter()
            .map(|d| package_json_dependency_key(&d.dep.package_name, &d.dep.path, root))
            .collect(),
        unused_dev: results
            .unused_dev_dependencies
            .iter()
            .map(|d| package_json_dependency_key(&d.dep.package_name, &d.dep.path, root))
            .collect(),
        unused_optional: results
            .unused_optional_dependencies
            .iter()
            .map(|d| package_json_dependency_key(&d.dep.package_name, &d.dep.path, root))
            .collect(),
        unlisted: results
            .unlisted_dependencies
            .iter()
            .map(|d| d.dep.package_name.clone())
            .collect(),
        type_only: results
            .type_only_dependencies
            .iter()
            .map(|d| package_json_dependency_key(&d.dep.package_name, &d.dep.path, root))
            .collect(),
        test_only: results
            .test_only_dependencies
            .iter()
            .map(|d| package_json_dependency_key(&d.dep.package_name, &d.dep.path, root))
            .collect(),
        dev_in_prod: results
            .dev_dependencies_in_production
            .iter()
            .map(|d| package_json_dependency_key(&d.dep.package_name, &d.dep.path, root))
            .collect(),
    }
}

pub(super) fn baseline_graph_keys(
    results: &crate::results::AnalysisResults,
    root: &Path,
) -> BaselineGraphKeys {
    BaselineGraphKeys {
        circular_dependencies: results
            .circular_dependencies
            .iter()
            .map(|c| circular_dep_key(&c.cycle, root))
            .collect(),
        re_export_cycles: results
            .re_export_cycles
            .iter()
            .map(|c| re_export_cycle_key(&c.cycle, root))
            .collect(),
        package_cycles: results
            .package_cycles
            .iter()
            .map(|c| package_cycle_key(&c.cycle))
            .collect(),
        boundary_violations: results
            .boundary_violations
            .iter()
            .map(|v| boundary_violation_key(&v.violation, root))
            .collect(),
        boundary_coverage_violations: results
            .boundary_coverage_violations
            .iter()
            .map(|v| relative_path(&v.violation.path, root))
            .collect(),
        boundary_call_violations: results
            .boundary_call_violations
            .iter()
            .map(|v| boundary_call_violation_key(&v.violation, root))
            .collect(),
        policy_violations: results
            .policy_violations
            .iter()
            .map(|v| policy_violation_key(&v.violation, root))
            .collect(),
    }
}

pub(super) fn baseline_catalog_keys(
    results: &crate::results::AnalysisResults,
    root: &Path,
) -> BaselineCatalogKeys {
    BaselineCatalogKeys {
        unused_catalog_entries: results
            .unused_catalog_entries
            .iter()
            .map(|e| format!("{}:{}", e.entry.catalog_name, e.entry.entry_name))
            .collect(),
        empty_catalog_groups: results
            .empty_catalog_groups
            .iter()
            .map(|g| g.group.catalog_name.clone())
            .collect(),
        unresolved_catalog_references: results
            .unresolved_catalog_references
            .iter()
            .map(|r| {
                format!(
                    "{}:{}:{}:{}",
                    relative_path(&r.reference.path, root),
                    r.reference.line,
                    r.reference.catalog_name,
                    r.reference.entry_name,
                )
            })
            .collect(),
        unused_dependency_overrides: results
            .unused_dependency_overrides
            .iter()
            .map(|o| format!("{}:{}", o.entry.source, o.entry.raw_key))
            .collect(),
        misconfigured_dependency_overrides: results
            .misconfigured_dependency_overrides
            .iter()
            .map(|o| format!("{}:{}", o.entry.source, o.entry.raw_key))
            .collect(),
    }
}

/// Generate a stable key for a rule-pack policy violation:
/// `path:pack/rule_id:matched`. Line numbers are deliberately excluded so a
/// baselined finding survives unrelated edits above it.
fn policy_violation_key(v: &crate::results::PolicyViolation, root: &Path) -> String {
    format!(
        "{}:{}/{}:{}",
        relative_path(&v.path, root),
        v.pack,
        v.rule_id,
        v.matched
    )
}
