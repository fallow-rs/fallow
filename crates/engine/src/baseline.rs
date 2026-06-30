use rustc_hash::{FxHashMap, FxHashSet};
use std::collections::BTreeMap;
use std::path::Path;

use crate::duplicates::DuplicationReport;

/// Strip the project root from a path to produce a portable relative key.
///
/// Both `path` and `root` must be in the same form (both canonicalized or both
/// not) for `strip_prefix` to succeed. The analysis pipeline keeps all paths
/// non-canonicalized, so this invariant holds in practice.
fn relative_path(path: &Path, root: &Path) -> String {
    match path.strip_prefix(root) {
        Ok(relative) => relative.to_string_lossy().replace('\\', "/"),
        Err(_) => {
            tracing::debug!(
                path = %path.display(),
                root = %root.display(),
                "baseline key: path is not under project root, using absolute path as key"
            );
            path.to_string_lossy().replace('\\', "/")
        }
    }
}

fn package_json_dependency_key(package_name: &str, path: &Path, root: &Path) -> String {
    format!("{}:{package_name}", relative_path(path, root))
}

fn baseline_contains_dependency(
    baseline_keys: &FxHashSet<&str>,
    package_name: &str,
    path_key: &str,
) -> bool {
    baseline_keys.contains(path_key) || baseline_keys.contains(package_name)
}

fn retain_new_by_keys<T>(
    items: &mut Vec<T>,
    baseline_keys: &[String],
    root: &Path,
    key_builder: fn(&[T], &Path) -> Vec<String>,
) {
    let baseline_keys: FxHashSet<&str> = baseline_keys.iter().map(String::as_str).collect();
    let item_keys = key_builder(items, root);
    let mut key_iter = item_keys.into_iter();
    items.retain(|_| match key_iter.next() {
        Some(key) => !baseline_keys.contains(key.as_str()),
        None => true,
    });
}

/// Baseline data for comparison.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct BaselineData {
    pub unused_files: Vec<String>,
    pub unused_exports: Vec<String>,
    pub unused_types: Vec<String>,
    #[serde(default)]
    pub private_type_leaks: Vec<String>,
    /// Unused dependencies, keyed by `package.json:package_name`. Legacy
    /// bare `package_name` keys are still matched for back-compat with
    /// baselines saved by older fallow versions.
    pub unused_dependencies: Vec<String>,
    /// Unused dev dependencies, keyed by `package.json:package_name`. Legacy
    /// bare `package_name` keys are still matched for back-compat with
    /// baselines saved by older fallow versions.
    pub unused_dev_dependencies: Vec<String>,
    /// Circular dependency chains, keyed by sorted file paths joined with `->`.
    #[serde(default)]
    pub circular_dependencies: Vec<String>,
    /// Re-export cycles, keyed by `kind:sorted_file_paths_joined_with_<->`
    /// (where `kind` is `multi-node` or `self-loop`). The kind prefix keeps
    /// self-loops from keyspace-colliding with future single-file multi-node
    /// shapes.
    #[serde(default)]
    pub re_export_cycles: Vec<String>,
    /// Unused optional dependencies, keyed by `package.json:package_name`.
    /// Legacy bare `package_name` keys are still matched for back-compat
    /// with baselines saved by older fallow versions.
    #[serde(default)]
    pub unused_optional_dependencies: Vec<String>,
    /// Unused enum members, keyed by `file:parent.member`.
    #[serde(default)]
    pub unused_enum_members: Vec<String>,
    /// Unused class members, keyed by `file:parent.member`.
    #[serde(default)]
    pub unused_class_members: Vec<String>,
    /// Unused store members, keyed by `file:parent.member`.
    #[serde(default)]
    pub unused_store_members: Vec<String>,
    /// Unprovided injects, keyed by `file:key_name`.
    #[serde(default)]
    pub unprovided_injects: Vec<String>,
    /// Unrendered components, keyed by `file:component_name`.
    #[serde(default)]
    pub unrendered_components: Vec<String>,
    /// Unused component props, keyed by `file:prop_name`.
    #[serde(default)]
    pub unused_component_props: Vec<String>,
    /// Unused component emits, keyed by `file:emit_name`.
    #[serde(default)]
    pub unused_component_emits: Vec<String>,
    /// Unused component inputs, keyed by `file:input_name`.
    #[serde(default)]
    pub unused_component_inputs: Vec<String>,
    /// Unused component outputs, keyed by `file:output_name`.
    #[serde(default)]
    pub unused_component_outputs: Vec<String>,
    /// Unused Svelte dispatched events, keyed by `file:event_name`.
    #[serde(default)]
    pub unused_svelte_events: Vec<String>,
    /// Unused server actions, keyed by `file:action_name`.
    #[serde(default)]
    pub unused_server_actions: Vec<String>,
    /// Unused SvelteKit load() data keys, keyed by `file:key_name`.
    #[serde(default)]
    pub unused_load_data_keys: Vec<String>,
    /// Unresolved imports, keyed by `file:specifier`.
    #[serde(default)]
    pub unresolved_imports: Vec<String>,
    /// Unlisted dependencies, keyed by package name.
    #[serde(default)]
    pub unlisted_dependencies: Vec<String>,
    /// Duplicate exports, keyed by export name.
    #[serde(default)]
    pub duplicate_exports: Vec<String>,
    /// Type-only dependencies, keyed by `package.json:package_name`. Legacy
    /// bare `package_name` keys are still matched for back-compat with
    /// baselines saved by older fallow versions.
    #[serde(default)]
    pub type_only_dependencies: Vec<String>,
    /// Test-only dependencies, keyed by `package.json:package_name`. Legacy
    /// bare `package_name` keys are still matched for back-compat with
    /// baselines saved by older fallow versions.
    #[serde(default)]
    pub test_only_dependencies: Vec<String>,
    /// Boundary violations, keyed by `from_path->to_path`.
    #[serde(default)]
    pub boundary_violations: Vec<String>,
    /// Boundary coverage violations, keyed by `path`.
    #[serde(default)]
    pub boundary_coverage_violations: Vec<String>,
    /// Boundary call violations, keyed by `path:callee`.
    #[serde(default)]
    pub boundary_call_violations: Vec<String>,
    /// Rule-pack policy violations, keyed by `path:pack/rule_id:matched`.
    #[serde(default)]
    pub policy_violations: Vec<String>,
    /// Stale suppressions, keyed by `file:line`.
    #[serde(default)]
    pub stale_suppressions: Vec<String>,
    /// Unused pnpm catalog entries, keyed by `catalog_name:entry_name`.
    #[serde(default)]
    pub unused_catalog_entries: Vec<String>,
    /// Empty pnpm catalog groups, keyed by `catalog_name`.
    #[serde(default)]
    pub empty_catalog_groups: Vec<String>,
    /// Unresolved catalog references, keyed by `path:line:catalog_name:entry_name`.
    #[serde(default)]
    pub unresolved_catalog_references: Vec<String>,
    /// Unused pnpm dependency overrides, keyed by `source:raw_key`.
    #[serde(default)]
    pub unused_dependency_overrides: Vec<String>,
    /// Misconfigured pnpm dependency overrides, keyed by `source:raw_key`.
    #[serde(default)]
    pub misconfigured_dependency_overrides: Vec<String>,
    /// Invalid `"use client"` exports, keyed by `path:export_name`.
    #[serde(default)]
    pub invalid_client_exports: Vec<String>,
    /// Mixed client/server barrels, keyed by `path:client_origin:server_origin`.
    #[serde(default)]
    pub mixed_client_server_barrels: Vec<String>,
    /// Misplaced `"use client"` / `"use server"` directives, keyed by
    /// `path:line:directive`.
    #[serde(default)]
    pub misplaced_directives: Vec<String>,
    /// Next.js route collisions, keyed by `path:url`.
    #[serde(default)]
    pub route_collisions: Vec<String>,
    /// Next.js dynamic-segment name conflicts, keyed by `path:position`.
    #[serde(default)]
    pub dynamic_segment_name_conflicts: Vec<String>,
}

impl BaselineData {
    pub fn from_results(results: &crate::results::AnalysisResults, root: &Path) -> Self {
        let file_exports = baseline_file_export_keys(results, root);
        let member_imports = baseline_member_import_keys(results, root);
        let dependencies = baseline_dependency_keys(results, root);
        let graph = baseline_graph_keys(results, root);
        let catalog = baseline_catalog_keys(results, root);

        Self {
            unused_files: file_exports.unused_files,
            unused_exports: file_exports.unused_exports,
            unused_types: file_exports.unused_types,
            private_type_leaks: file_exports.private_type_leaks,
            unused_dependencies: dependencies.unused,
            unused_dev_dependencies: dependencies.unused_dev,
            circular_dependencies: graph.circular_dependencies,
            re_export_cycles: graph.re_export_cycles,
            unused_optional_dependencies: dependencies.unused_optional,
            unused_enum_members: member_imports.unused_enum_members,
            unused_class_members: member_imports.unused_class_members,
            unused_store_members: member_imports.unused_store_members,
            unprovided_injects: member_imports.unprovided_injects,
            unrendered_components: member_imports.unrendered_components,
            unused_component_props: member_imports.unused_component_props,
            unused_component_emits: member_imports.unused_component_emits,
            unused_component_inputs: member_imports.unused_component_inputs,
            unused_component_outputs: member_imports.unused_component_outputs,
            unused_svelte_events: member_imports.unused_svelte_events,
            unused_server_actions: member_imports.unused_server_actions,
            unused_load_data_keys: member_imports.unused_load_data_keys,
            unresolved_imports: member_imports.unresolved_imports,
            unlisted_dependencies: dependencies.unlisted,
            duplicate_exports: member_imports.duplicate_exports,
            type_only_dependencies: dependencies.type_only,
            test_only_dependencies: dependencies.test_only,
            boundary_violations: graph.boundary_violations,
            boundary_coverage_violations: graph.boundary_coverage_violations,
            boundary_call_violations: graph.boundary_call_violations,
            policy_violations: graph.policy_violations,
            stale_suppressions: member_imports.stale_suppressions,
            unused_catalog_entries: catalog.unused_catalog_entries,
            empty_catalog_groups: catalog.empty_catalog_groups,
            unresolved_catalog_references: catalog.unresolved_catalog_references,
            unused_dependency_overrides: catalog.unused_dependency_overrides,
            misconfigured_dependency_overrides: catalog.misconfigured_dependency_overrides,
            invalid_client_exports: file_exports.invalid_client_exports,
            mixed_client_server_barrels: file_exports.mixed_client_server_barrels,
            misplaced_directives: file_exports.misplaced_directives,
            route_collisions: file_exports.route_collisions,
            dynamic_segment_name_conflicts: file_exports.dynamic_segment_name_conflicts,
        }
    }

    /// Total number of entries across all categories.
    pub fn total_entries(&self) -> usize {
        self.unused_files.len()
            + self.unused_exports.len()
            + self.unused_types.len()
            + self.private_type_leaks.len()
            + self.unused_dependencies.len()
            + self.unused_dev_dependencies.len()
            + self.circular_dependencies.len()
            + self.re_export_cycles.len()
            + self.unused_optional_dependencies.len()
            + self.unused_enum_members.len()
            + self.unused_class_members.len()
            + self.unused_store_members.len()
            + self.unprovided_injects.len()
            + self.unrendered_components.len()
            + self.unused_component_props.len()
            + self.unused_component_emits.len()
            + self.unused_component_inputs.len()
            + self.unused_component_outputs.len()
            + self.unused_svelte_events.len()
            + self.unused_server_actions.len()
            + self.unused_load_data_keys.len()
            + self.unresolved_imports.len()
            + self.unlisted_dependencies.len()
            + self.duplicate_exports.len()
            + self.type_only_dependencies.len()
            + self.test_only_dependencies.len()
            + self.boundary_violations.len()
            + self.boundary_coverage_violations.len()
            + self.boundary_call_violations.len()
            + self.policy_violations.len()
            + self.stale_suppressions.len()
            + self.unused_catalog_entries.len()
            + self.empty_catalog_groups.len()
            + self.unresolved_catalog_references.len()
            + self.unused_dependency_overrides.len()
            + self.misconfigured_dependency_overrides.len()
            + self.invalid_client_exports.len()
            + self.mixed_client_server_barrels.len()
            + self.misplaced_directives.len()
            + self.route_collisions.len()
            + self.dynamic_segment_name_conflicts.len()
    }
}

struct BaselineFileExportKeys {
    unused_files: Vec<String>,
    unused_exports: Vec<String>,
    unused_types: Vec<String>,
    private_type_leaks: Vec<String>,
    invalid_client_exports: Vec<String>,
    mixed_client_server_barrels: Vec<String>,
    misplaced_directives: Vec<String>,
    route_collisions: Vec<String>,
    dynamic_segment_name_conflicts: Vec<String>,
}

fn baseline_file_export_keys(
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

struct BaselineMemberImportKeys {
    unused_enum_members: Vec<String>,
    unused_class_members: Vec<String>,
    unused_store_members: Vec<String>,
    unprovided_injects: Vec<String>,
    unrendered_components: Vec<String>,
    unused_component_props: Vec<String>,
    unused_component_emits: Vec<String>,
    unused_component_inputs: Vec<String>,
    unused_component_outputs: Vec<String>,
    unused_svelte_events: Vec<String>,
    unused_server_actions: Vec<String>,
    unused_load_data_keys: Vec<String>,
    unresolved_imports: Vec<String>,
    duplicate_exports: Vec<String>,
    stale_suppressions: Vec<String>,
}

fn baseline_member_import_keys(
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

fn stale_suppression_baseline_key(
    suppression: &crate::results::StaleSuppression,
    root: &Path,
) -> String {
    let rule_id = if suppression.missing_reason {
        "missing-suppression-reason"
    } else {
        "stale-suppression"
    };
    format!(
        "{rule_id}:{}:{}",
        relative_path(&suppression.path, root),
        suppression.line
    )
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

fn unused_member_baseline_key(member: &crate::results::UnusedMember, root: &Path) -> String {
    format!(
        "{}:{}.{}",
        relative_path(&member.path, root),
        member.parent_name,
        member.member_name
    )
}

fn inject_baseline_keys(
    items: &[crate::results::UnprovidedInjectFinding],
    root: &Path,
) -> Vec<String> {
    items
        .iter()
        .map(|f| {
            format!(
                "{}:{}",
                relative_path(&f.inject.path, root),
                f.inject.key_name
            )
        })
        .collect()
}

fn component_baseline_keys(
    items: &[crate::results::UnrenderedComponentFinding],
    root: &Path,
) -> Vec<String> {
    items
        .iter()
        .map(|c| {
            format!(
                "{}:{}",
                relative_path(&c.component.path, root),
                c.component.component_name
            )
        })
        .collect()
}

fn component_prop_baseline_keys(
    items: &[crate::results::UnusedComponentPropFinding],
    root: &Path,
) -> Vec<String> {
    items
        .iter()
        .map(|p| format!("{}:{}", relative_path(&p.prop.path, root), p.prop.prop_name))
        .collect()
}

fn component_emit_baseline_keys(
    items: &[crate::results::UnusedComponentEmitFinding],
    root: &Path,
) -> Vec<String> {
    items
        .iter()
        .map(|e| format!("{}:{}", relative_path(&e.emit.path, root), e.emit.emit_name))
        .collect()
}

fn component_input_baseline_keys(
    items: &[crate::results::UnusedComponentInputFinding],
    root: &Path,
) -> Vec<String> {
    items
        .iter()
        .map(|i| {
            format!(
                "{}:{}",
                relative_path(&i.input.path, root),
                i.input.input_name
            )
        })
        .collect()
}

fn component_output_baseline_keys(
    items: &[crate::results::UnusedComponentOutputFinding],
    root: &Path,
) -> Vec<String> {
    items
        .iter()
        .map(|o| {
            format!(
                "{}:{}",
                relative_path(&o.output.path, root),
                o.output.output_name
            )
        })
        .collect()
}

fn svelte_event_baseline_keys(
    items: &[crate::results::UnusedSvelteEventFinding],
    root: &Path,
) -> Vec<String> {
    items
        .iter()
        .map(|e| {
            format!(
                "{}:{}",
                relative_path(&e.event.path, root),
                e.event.event_name
            )
        })
        .collect()
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

struct BaselineDependencyKeys {
    unused: Vec<String>,
    unused_dev: Vec<String>,
    unused_optional: Vec<String>,
    unlisted: Vec<String>,
    type_only: Vec<String>,
    test_only: Vec<String>,
}

fn baseline_dependency_keys(
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
    }
}

struct BaselineGraphKeys {
    circular_dependencies: Vec<String>,
    re_export_cycles: Vec<String>,
    boundary_violations: Vec<String>,
    boundary_coverage_violations: Vec<String>,
    boundary_call_violations: Vec<String>,
    policy_violations: Vec<String>,
}

fn baseline_graph_keys(
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

struct BaselineCatalogKeys {
    unused_catalog_entries: Vec<String>,
    empty_catalog_groups: Vec<String>,
    unresolved_catalog_references: Vec<String>,
    unused_dependency_overrides: Vec<String>,
    misconfigured_dependency_overrides: Vec<String>,
}

fn baseline_catalog_keys(
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

/// Generate a stable key for a boundary violation: `from_path->to_path`.
fn boundary_violation_key(v: &crate::results::BoundaryViolation, root: &Path) -> String {
    format!(
        "{}->{}",
        relative_path(&v.from_path, root),
        relative_path(&v.to_path, root),
    )
}

/// Generate a stable key for a boundary call violation: `path:callee`.
fn boundary_call_violation_key(v: &crate::results::BoundaryCallViolation, root: &Path) -> String {
    format!("{}:{}", relative_path(&v.path, root), v.callee)
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

/// Generate a stable key for a duplicate export: `name|sorted_paths`.
fn duplicate_export_key(dup: &crate::results::DuplicateExport, root: &Path) -> String {
    let mut locs: Vec<String> = dup
        .locations
        .iter()
        .map(|l| relative_path(&l.path, root))
        .collect();
    locs.sort();
    format!("{}|{}", dup.export_name, locs.join("|"))
}

/// Generate a stable key for a circular dependency based on sorted file paths.
fn circular_dep_key(dep: &crate::results::CircularDependency, root: &Path) -> String {
    let mut paths: Vec<String> = dep.files.iter().map(|f| relative_path(f, root)).collect();
    paths.sort();
    paths.join("->")
}

/// Generate a stable key for a re-export cycle based on its discriminator
/// kind plus sorted member paths. The `kind` prefix is mandatory: without
/// it a self-loop on `src/foo.ts` would keyspace-collide with any future
/// single-file multi-node shape, and the `--baseline new` filter would
/// silently drop the new one as already-seen (panel catch #7).
fn re_export_cycle_key(cycle: &crate::results::ReExportCycle, root: &Path) -> String {
    let kind = match cycle.kind {
        crate::results::ReExportCycleKind::MultiNode => "multi-node",
        crate::results::ReExportCycleKind::SelfLoop => "self-loop",
    };
    let mut paths: Vec<String> = cycle.files.iter().map(|f| relative_path(f, root)).collect();
    paths.sort();
    format!("{kind}:{}", paths.join("<->"))
}

fn private_type_leak_key(leak: &crate::results::PrivateTypeLeak, root: &Path) -> String {
    format!(
        "{}:{}->{}",
        relative_path(&leak.path, root),
        leak.export_name,
        leak.type_name
    )
}

fn filter_private_type_leaks(
    leaks: &mut Vec<fallow_types::output_dead_code::PrivateTypeLeakFinding>,
    baseline_keys: &[String],
    root: &Path,
) {
    let baseline_private_type_leaks: FxHashSet<&str> =
        baseline_keys.iter().map(String::as_str).collect();
    leaks.retain(|entry| {
        let key = private_type_leak_key(&entry.leak, root);
        !baseline_private_type_leaks.contains(key.as_str())
    });
}

struct BaselineFilterContext<'a> {
    baseline: &'a BaselineData,
    root: &'a Path,
}

impl BaselineFilterContext<'_> {
    fn filter_cycles_and_members(&self, results: &mut crate::results::AnalysisResults) {
        let baseline_circular: FxHashSet<&str> = self
            .baseline
            .circular_dependencies
            .iter()
            .map(String::as_str)
            .collect();
        results.circular_dependencies.retain(|cycle| {
            let key = circular_dep_key(&cycle.cycle, self.root);
            !baseline_circular.contains(key.as_str())
        });

        let baseline_re_export_cycles: FxHashSet<&str> = self
            .baseline
            .re_export_cycles
            .iter()
            .map(String::as_str)
            .collect();
        results.re_export_cycles.retain(|cycle| {
            let key = re_export_cycle_key(&cycle.cycle, self.root);
            !baseline_re_export_cycles.contains(key.as_str())
        });

        self.filter_unused_members(results);
        self.filter_unresolved_and_exports(results);
    }

    fn filter_unused_members(&self, results: &mut crate::results::AnalysisResults) {
        self.filter_enum_class_store_members(results);
        self.filter_component_surface_members(results);
        self.filter_route_action_members(results);
    }

    fn filter_enum_class_store_members(&self, results: &mut crate::results::AnalysisResults) {
        let baseline_enum_members: FxHashSet<&str> = self
            .baseline
            .unused_enum_members
            .iter()
            .map(String::as_str)
            .collect();
        results.unused_enum_members.retain(|member| {
            let key = format!(
                "{}:{}.{}",
                relative_path(&member.member.path, self.root),
                member.member.parent_name,
                member.member.member_name
            );
            !baseline_enum_members.contains(key.as_str())
        });

        let baseline_class_members: FxHashSet<&str> = self
            .baseline
            .unused_class_members
            .iter()
            .map(String::as_str)
            .collect();
        results.unused_class_members.retain(|member| {
            let key = format!(
                "{}:{}.{}",
                relative_path(&member.member.path, self.root),
                member.member.parent_name,
                member.member.member_name
            );
            !baseline_class_members.contains(key.as_str())
        });

        let baseline_store_members: FxHashSet<&str> = self
            .baseline
            .unused_store_members
            .iter()
            .map(String::as_str)
            .collect();
        results.unused_store_members.retain(|member| {
            let key = format!(
                "{}:{}.{}",
                relative_path(&member.member.path, self.root),
                member.member.parent_name,
                member.member.member_name
            );
            !baseline_store_members.contains(key.as_str())
        });
    }

    fn filter_component_surface_members(&self, results: &mut crate::results::AnalysisResults) {
        retain_new_by_keys(
            &mut results.unprovided_injects,
            &self.baseline.unprovided_injects,
            self.root,
            inject_baseline_keys,
        );
        retain_new_by_keys(
            &mut results.unrendered_components,
            &self.baseline.unrendered_components,
            self.root,
            component_baseline_keys,
        );
        retain_new_by_keys(
            &mut results.unused_component_props,
            &self.baseline.unused_component_props,
            self.root,
            component_prop_baseline_keys,
        );
        retain_new_by_keys(
            &mut results.unused_component_emits,
            &self.baseline.unused_component_emits,
            self.root,
            component_emit_baseline_keys,
        );
        retain_new_by_keys(
            &mut results.unused_component_inputs,
            &self.baseline.unused_component_inputs,
            self.root,
            component_input_baseline_keys,
        );
        retain_new_by_keys(
            &mut results.unused_component_outputs,
            &self.baseline.unused_component_outputs,
            self.root,
            component_output_baseline_keys,
        );
        retain_new_by_keys(
            &mut results.unused_svelte_events,
            &self.baseline.unused_svelte_events,
            self.root,
            svelte_event_baseline_keys,
        );
    }

    fn filter_route_action_members(&self, results: &mut crate::results::AnalysisResults) {
        let baseline_unused_server_actions: FxHashSet<&str> = self
            .baseline
            .unused_server_actions
            .iter()
            .map(String::as_str)
            .collect();
        results.unused_server_actions.retain(|finding| {
            let key = format!(
                "{}:{}",
                relative_path(&finding.action.path, self.root),
                finding.action.action_name
            );
            !baseline_unused_server_actions.contains(key.as_str())
        });

        let baseline_unused_load_data_keys: FxHashSet<&str> = self
            .baseline
            .unused_load_data_keys
            .iter()
            .map(String::as_str)
            .collect();
        results.unused_load_data_keys.retain(|finding| {
            let key = format!(
                "{}:{}",
                relative_path(&finding.key.path, self.root),
                finding.key.key_name
            );
            !baseline_unused_load_data_keys.contains(key.as_str())
        });
    }

    fn filter_unresolved_and_exports(&self, results: &mut crate::results::AnalysisResults) {
        let baseline_unresolved: FxHashSet<&str> = self
            .baseline
            .unresolved_imports
            .iter()
            .map(String::as_str)
            .collect();
        results.unresolved_imports.retain(|import| {
            let key = format!(
                "{}:{}",
                relative_path(&import.import.path, self.root),
                import.import.specifier
            );
            !baseline_unresolved.contains(key.as_str())
        });

        let baseline_unlisted: FxHashSet<&str> = self
            .baseline
            .unlisted_dependencies
            .iter()
            .map(String::as_str)
            .collect();
        results
            .unlisted_dependencies
            .retain(|dep| !baseline_unlisted.contains(dep.dep.package_name.as_str()));

        let baseline_dup_exports: FxHashSet<&str> = self
            .baseline
            .duplicate_exports
            .iter()
            .map(String::as_str)
            .collect();
        results.duplicate_exports.retain(|duplicate| {
            let key = duplicate_export_key(&duplicate.export, self.root);
            !baseline_dup_exports.contains(key.as_str())
        });
    }

    fn filter_dependency_variants(&self, results: &mut crate::results::AnalysisResults) {
        let baseline_optional_deps: FxHashSet<&str> = self
            .baseline
            .unused_optional_dependencies
            .iter()
            .map(String::as_str)
            .collect();
        results.unused_optional_dependencies.retain(|dep| {
            let key = package_json_dependency_key(&dep.dep.package_name, &dep.dep.path, self.root);
            !baseline_contains_dependency(
                &baseline_optional_deps,
                &dep.dep.package_name,
                key.as_str(),
            )
        });

        self.filter_type_and_test_only_dependencies(results);
    }

    fn filter_type_and_test_only_dependencies(
        &self,
        results: &mut crate::results::AnalysisResults,
    ) {
        let baseline_type_only: FxHashSet<&str> = self
            .baseline
            .type_only_dependencies
            .iter()
            .map(String::as_str)
            .collect();
        results.type_only_dependencies.retain(|dep| {
            let key = package_json_dependency_key(&dep.dep.package_name, &dep.dep.path, self.root);
            !baseline_contains_dependency(&baseline_type_only, &dep.dep.package_name, key.as_str())
        });

        let baseline_test_only: FxHashSet<&str> = self
            .baseline
            .test_only_dependencies
            .iter()
            .map(String::as_str)
            .collect();
        results.test_only_dependencies.retain(|dep| {
            let key = package_json_dependency_key(&dep.dep.package_name, &dep.dep.path, self.root);
            !baseline_contains_dependency(&baseline_test_only, &dep.dep.package_name, key.as_str())
        });
    }

    fn filter_boundaries_and_suppressions(&self, results: &mut crate::results::AnalysisResults) {
        let baseline_boundary: FxHashSet<&str> = self
            .baseline
            .boundary_violations
            .iter()
            .map(String::as_str)
            .collect();
        results.boundary_violations.retain(|violation| {
            let key = boundary_violation_key(&violation.violation, self.root);
            !baseline_boundary.contains(key.as_str())
        });

        self.filter_boundary_details(results);
        self.filter_stale_suppressions(results);
        self.filter_invalid_client_exports(results);
        self.filter_mixed_client_server_barrels(results);
        self.filter_misplaced_directives(results);
        self.filter_route_collisions(results);
        self.filter_dynamic_segment_name_conflicts(results);
    }

    fn filter_invalid_client_exports(&self, results: &mut crate::results::AnalysisResults) {
        let baseline_invalid: FxHashSet<&str> = self
            .baseline
            .invalid_client_exports
            .iter()
            .map(String::as_str)
            .collect();
        results.invalid_client_exports.retain(|finding| {
            let key = format!(
                "{}:{}",
                relative_path(&finding.export.path, self.root),
                finding.export.export_name
            );
            !baseline_invalid.contains(key.as_str())
        });
    }

    fn filter_mixed_client_server_barrels(&self, results: &mut crate::results::AnalysisResults) {
        let baseline_barrels: FxHashSet<&str> = self
            .baseline
            .mixed_client_server_barrels
            .iter()
            .map(String::as_str)
            .collect();
        results.mixed_client_server_barrels.retain(|finding| {
            let key = format!(
                "{}:{}:{}",
                relative_path(&finding.barrel.path, self.root),
                finding.barrel.client_origin,
                finding.barrel.server_origin
            );
            !baseline_barrels.contains(key.as_str())
        });
    }

    fn filter_misplaced_directives(&self, results: &mut crate::results::AnalysisResults) {
        let baseline_directives: FxHashSet<&str> = self
            .baseline
            .misplaced_directives
            .iter()
            .map(String::as_str)
            .collect();
        results.misplaced_directives.retain(|finding| {
            let key = format!(
                "{}:{}:{}",
                relative_path(&finding.directive_site.path, self.root),
                finding.directive_site.line,
                finding.directive_site.directive
            );
            !baseline_directives.contains(key.as_str())
        });
    }

    fn filter_route_collisions(&self, results: &mut crate::results::AnalysisResults) {
        let baseline_collisions: FxHashSet<&str> = self
            .baseline
            .route_collisions
            .iter()
            .map(String::as_str)
            .collect();
        results.route_collisions.retain(|finding| {
            let key = format!(
                "{}:{}",
                relative_path(&finding.collision.path, self.root),
                finding.collision.url
            );
            !baseline_collisions.contains(key.as_str())
        });
    }

    fn filter_dynamic_segment_name_conflicts(&self, results: &mut crate::results::AnalysisResults) {
        let baseline_conflicts: FxHashSet<&str> = self
            .baseline
            .dynamic_segment_name_conflicts
            .iter()
            .map(String::as_str)
            .collect();
        results.dynamic_segment_name_conflicts.retain(|finding| {
            let key = format!(
                "{}:{}",
                relative_path(&finding.conflict.path, self.root),
                finding.conflict.position
            );
            !baseline_conflicts.contains(key.as_str())
        });
    }

    fn filter_boundary_details(&self, results: &mut crate::results::AnalysisResults) {
        let baseline_boundary_coverage: FxHashSet<&str> = self
            .baseline
            .boundary_coverage_violations
            .iter()
            .map(String::as_str)
            .collect();
        results.boundary_coverage_violations.retain(|violation| {
            let key = relative_path(&violation.violation.path, self.root);
            !baseline_boundary_coverage.contains(key.as_str())
        });

        let baseline_boundary_calls: FxHashSet<&str> = self
            .baseline
            .boundary_call_violations
            .iter()
            .map(String::as_str)
            .collect();
        results.boundary_call_violations.retain(|violation| {
            let key = boundary_call_violation_key(&violation.violation, self.root);
            !baseline_boundary_calls.contains(key.as_str())
        });
    }

    fn filter_stale_suppressions(&self, results: &mut crate::results::AnalysisResults) {
        let baseline_stale: FxHashSet<&str> = self
            .baseline
            .stale_suppressions
            .iter()
            .map(String::as_str)
            .collect();
        results.stale_suppressions.retain(|suppression| {
            let key = stale_suppression_baseline_key(suppression, self.root);
            let legacy_key = format!(
                "{}:{}",
                relative_path(&suppression.path, self.root),
                suppression.line
            );
            !baseline_stale.contains(key.as_str()) && !baseline_stale.contains(legacy_key.as_str())
        });
    }

    fn filter_pnpm_entries(&self, results: &mut crate::results::AnalysisResults) {
        let baseline_catalog: FxHashSet<&str> = self
            .baseline
            .unused_catalog_entries
            .iter()
            .map(String::as_str)
            .collect();
        results.unused_catalog_entries.retain(|entry| {
            let key = format!("{}:{}", entry.entry.catalog_name, entry.entry.entry_name);
            !baseline_catalog.contains(key.as_str())
        });

        let baseline_empty_catalog_groups: FxHashSet<&str> = self
            .baseline
            .empty_catalog_groups
            .iter()
            .map(String::as_str)
            .collect();
        results.empty_catalog_groups.retain(|group| {
            !baseline_empty_catalog_groups.contains(group.group.catalog_name.as_str())
        });

        self.filter_pnpm_references_and_overrides(results);
    }

    fn filter_pnpm_references_and_overrides(&self, results: &mut crate::results::AnalysisResults) {
        let baseline_unresolved: FxHashSet<&str> = self
            .baseline
            .unresolved_catalog_references
            .iter()
            .map(String::as_str)
            .collect();
        results.unresolved_catalog_references.retain(|reference| {
            let key = format!(
                "{}:{}:{}:{}",
                relative_path(&reference.reference.path, self.root),
                reference.reference.line,
                reference.reference.catalog_name,
                reference.reference.entry_name,
            );
            !baseline_unresolved.contains(key.as_str())
        });

        self.filter_pnpm_overrides(results);
    }

    fn filter_pnpm_overrides(&self, results: &mut crate::results::AnalysisResults) {
        let baseline_unused_overrides: FxHashSet<&str> = self
            .baseline
            .unused_dependency_overrides
            .iter()
            .map(String::as_str)
            .collect();
        results
            .unused_dependency_overrides
            .retain(|override_entry| {
                let key = format!(
                    "{}:{}",
                    override_entry.entry.source, override_entry.entry.raw_key
                );
                !baseline_unused_overrides.contains(key.as_str())
            });

        let baseline_misconfigured_overrides: FxHashSet<&str> = self
            .baseline
            .misconfigured_dependency_overrides
            .iter()
            .map(String::as_str)
            .collect();
        results
            .misconfigured_dependency_overrides
            .retain(|override_entry| {
                let key = format!(
                    "{}:{}",
                    override_entry.entry.source, override_entry.entry.raw_key
                );
                !baseline_misconfigured_overrides.contains(key.as_str())
            });
    }
}

/// Filter results to only include issues not present in the baseline.
pub fn filter_new_issues(
    mut results: crate::results::AnalysisResults,
    baseline: &BaselineData,
    root: &Path,
) -> crate::results::AnalysisResults {
    let baseline_files: FxHashSet<&str> =
        baseline.unused_files.iter().map(String::as_str).collect();
    let baseline_exports: FxHashSet<&str> =
        baseline.unused_exports.iter().map(String::as_str).collect();
    let baseline_types: FxHashSet<&str> =
        baseline.unused_types.iter().map(String::as_str).collect();
    let baseline_deps: FxHashSet<&str> = baseline
        .unused_dependencies
        .iter()
        .map(String::as_str)
        .collect();
    let baseline_dev_deps: FxHashSet<&str> = baseline
        .unused_dev_dependencies
        .iter()
        .map(String::as_str)
        .collect();

    results
        .unused_files
        .retain(|f| !baseline_files.contains(relative_path(&f.file.path, root).as_str()));
    results.unused_exports.retain(|e| {
        let key = format!(
            "{}:{}",
            relative_path(&e.export.path, root),
            e.export.export_name
        );
        !baseline_exports.contains(key.as_str())
    });
    results.unused_types.retain(|e| {
        let key = format!(
            "{}:{}",
            relative_path(&e.export.path, root),
            e.export.export_name
        );
        !baseline_types.contains(key.as_str())
    });
    filter_private_type_leaks(
        &mut results.private_type_leaks,
        &baseline.private_type_leaks,
        root,
    );
    results.unused_dependencies.retain(|d| {
        let key = package_json_dependency_key(&d.dep.package_name, &d.dep.path, root);
        !baseline_contains_dependency(&baseline_deps, &d.dep.package_name, key.as_str())
    });
    results.unused_dev_dependencies.retain(|d| {
        let key = package_json_dependency_key(&d.dep.package_name, &d.dep.path, root);
        !baseline_contains_dependency(&baseline_dev_deps, &d.dep.package_name, key.as_str())
    });

    let filter = BaselineFilterContext { baseline, root };
    filter.filter_cycles_and_members(&mut results);
    filter.filter_dependency_variants(&mut results);
    filter.filter_boundaries_and_suppressions(&mut results);
    filter.filter_pnpm_entries(&mut results);

    results
}

/// Baseline data for duplication comparison.
///
/// Each clone group is keyed by a canonical string derived from its sorted
/// (`file:start_line-end_line`) instance locations. This allows stable comparison
/// across runs even if group ordering changes.
#[derive(serde::Serialize, serde::Deserialize)]
pub struct DuplicationBaselineData {
    /// Clone group keys: sorted list of `file:start-end` per group.
    pub clone_groups: Vec<String>,
}

impl DuplicationBaselineData {
    /// Build a duplication baseline from the current report.
    pub fn from_report(report: &DuplicationReport, root: &Path) -> Self {
        Self {
            clone_groups: report
                .clone_groups
                .iter()
                .map(|g| clone_group_key(g, root))
                .collect(),
        }
    }
}

/// Generate a stable key for a clone group based on its instance locations.
fn clone_group_key(group: &crate::duplicates::CloneGroup, root: &Path) -> String {
    let mut parts: Vec<String> = group
        .instances
        .iter()
        .map(|i| {
            format!(
                "{}:{}-{}",
                relative_path(&i.file, root),
                i.start_line,
                i.end_line
            )
        })
        .collect();
    parts.sort();
    parts.join("|")
}

/// Filter a duplication report to only include clone groups not present in the baseline.
pub fn filter_new_clone_groups(
    mut report: DuplicationReport,
    baseline: &DuplicationBaselineData,
    root: &Path,
) -> DuplicationReport {
    let baseline_keys: FxHashSet<&str> = baseline.clone_groups.iter().map(String::as_str).collect();

    report.clone_groups.retain(|g| {
        let key = clone_group_key(g, root);
        !baseline_keys.contains(key.as_str())
    });

    crate::duplicates::refresh_clone_families(&mut report, root);
    report.stats = recompute_stats(&report);

    report
}

/// Recompute duplication statistics after filtering (baseline or `--changed-since`).
///
/// Uses per-file line deduplication (matching `compute_stats` in `detect.rs`)
/// so overlapping clone instances don't inflate the duplicated line count.
pub fn recompute_stats(report: &DuplicationReport) -> crate::duplicates::DuplicationStats {
    crate::duplicates::recompute_stats(report)
}

/// Baseline data for health (complexity) comparison.
///
/// New baselines store count-per-category-per-file data in `finding_counts` so
/// line shifts do not leak pre-existing findings. Legacy baselines with
/// `findings: ["path:name:line"]` still load so users can refresh them in
/// place with `--save-baseline`.
#[derive(Default, serde::Serialize, serde::Deserialize)]
pub struct HealthBaselineData {
    /// Legacy health baseline keys: `relative_path:function_name:line`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub findings: Vec<String>,
    /// Count-per-category-per-file baseline buckets.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub finding_counts: HealthFindingCountMap,
    /// Stable runtime-coverage finding IDs from the sidecar.
    #[serde(default)]
    pub runtime_coverage_findings: Vec<String>,
    /// Line-move-tolerant runtime-coverage suppression keys of the form
    /// `path\0name\0source_hash`. Unlike `runtime_coverage_findings` (whose
    /// keys hash the start line and so churn when a function moves), the
    /// `source_hash` component is the content digest of the function body, so a
    /// moved-but-unedited function keeps the same key and stays suppressed.
    /// Only findings whose `source_hash` is present contribute an entry.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub runtime_coverage_source_hashes: Vec<String>,
    /// Refactoring target keys: `relative_path:category`.
    #[serde(default)]
    pub target_keys: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct HealthBaselineCount {
    pub count: usize,
}

type HealthFindingCountMap = BTreeMap<String, BTreeMap<String, HealthBaselineCount>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum HealthFindingDimension {
    Complexity,
    Crap,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct HealthFindingCategory {
    dimension: HealthFindingDimension,
    severity: fallow_output::FindingSeverity,
}

impl HealthFindingCategory {
    const fn key(self) -> &'static str {
        match (self.dimension, self.severity) {
            (HealthFindingDimension::Complexity, fallow_output::FindingSeverity::Moderate) => {
                "complexity_moderate"
            }
            (HealthFindingDimension::Complexity, fallow_output::FindingSeverity::High) => {
                "complexity_high"
            }
            (HealthFindingDimension::Complexity, fallow_output::FindingSeverity::Critical) => {
                "complexity_critical"
            }
            (HealthFindingDimension::Crap, fallow_output::FindingSeverity::Moderate) => {
                "crap_moderate"
            }
            (HealthFindingDimension::Crap, fallow_output::FindingSeverity::High) => "crap_high",
            (HealthFindingDimension::Crap, fallow_output::FindingSeverity::Critical) => {
                "crap_critical"
            }
        }
    }
}

const HEALTH_FINDING_DIMENSIONS: [HealthFindingDimension; 2] = [
    HealthFindingDimension::Complexity,
    HealthFindingDimension::Crap,
];

impl HealthBaselineData {
    /// Build a health baseline from findings and targets.
    pub fn from_findings(
        findings: &[fallow_output::ComplexityViolation],
        runtime_coverage_findings: &[fallow_output::RuntimeCoverageFinding],
        targets: &[fallow_output::RefactoringTarget],
        root: &Path,
    ) -> Self {
        Self {
            findings: Vec::new(),
            finding_counts: health_finding_counts(findings, root),
            runtime_coverage_findings: runtime_coverage_findings
                .iter()
                .map(|f| runtime_coverage_finding_key(f, root))
                .collect(),
            runtime_coverage_source_hashes: runtime_coverage_findings
                .iter()
                .filter_map(|f| runtime_coverage_source_hash_key(f, root))
                .collect(),
            target_keys: targets
                .iter()
                .map(|t| target_baseline_key(t, root))
                .collect(),
        }
    }

    pub fn finding_entry_count(&self) -> usize {
        if !self.finding_counts.is_empty() {
            self.finding_counts
                .values()
                .flat_map(BTreeMap::values)
                .map(|entry| entry.count)
                .sum()
        } else {
            self.findings.len()
        }
    }

    pub fn overlap_entry_count(
        &self,
        findings: &[fallow_output::ComplexityViolation],
        root: &Path,
    ) -> usize {
        if !self.finding_counts.is_empty() {
            let current_counts = health_finding_counts(findings, root);
            health_overlap_entry_count(&current_counts, &self.finding_counts)
        } else {
            let baseline_keys: FxHashSet<&str> = self.findings.iter().map(String::as_str).collect();
            findings
                .iter()
                .filter(|finding| {
                    baseline_keys.contains(health_finding_key(finding, root).as_str())
                })
                .count()
        }
    }
}

/// Generate a stable key for a refactoring target: `relative_path:category`.
fn target_baseline_key(target: &fallow_output::RefactoringTarget, root: &Path) -> String {
    format!(
        "{}:{}",
        relative_path(&target.path, root),
        target.category.label()
    )
}

/// Generate a stable key for a health finding.
fn health_finding_key(finding: &fallow_output::ComplexityViolation, root: &Path) -> String {
    format!(
        "{}:{}:{}",
        relative_path(&finding.path, root),
        finding.name,
        finding.line
    )
}

fn health_finding_counts(
    findings: &[fallow_output::ComplexityViolation],
    root: &Path,
) -> HealthFindingCountMap {
    let mut counts = BTreeMap::new();
    for finding in findings {
        let path = relative_path(&finding.path, root);
        let file_counts = counts.entry(path).or_insert_with(BTreeMap::new);
        for category in health_finding_categories(finding).into_iter().flatten() {
            file_counts
                .entry(category.key().to_string())
                .and_modify(|entry: &mut HealthBaselineCount| entry.count += 1)
                .or_insert(HealthBaselineCount { count: 1 });
        }
    }
    counts
}

fn health_finding_categories(
    finding: &fallow_output::ComplexityViolation,
) -> [Option<HealthFindingCategory>; 2] {
    let complexity_category = HealthFindingCategory {
        dimension: HealthFindingDimension::Complexity,
        severity: finding.severity,
    };
    let crap_category = HealthFindingCategory {
        dimension: HealthFindingDimension::Crap,
        severity: finding.severity,
    };
    let has_complexity =
        finding.exceeded.includes_cyclomatic() || finding.exceeded.includes_cognitive();
    let has_crap = finding.exceeded.includes_crap();
    [
        has_complexity.then_some(complexity_category),
        has_crap.then_some(crap_category),
    ]
}

fn severity_index(severity: fallow_output::FindingSeverity) -> usize {
    match severity {
        fallow_output::FindingSeverity::Moderate => 0,
        fallow_output::FindingSeverity::High => 1,
        fallow_output::FindingSeverity::Critical => 2,
    }
}

fn severity_counts_for_dimension(
    file_counts: Option<&BTreeMap<String, HealthBaselineCount>>,
    dimension: HealthFindingDimension,
) -> [usize; 3] {
    let mut counts = [0; 3];
    for severity in [
        fallow_output::FindingSeverity::Moderate,
        fallow_output::FindingSeverity::High,
        fallow_output::FindingSeverity::Critical,
    ] {
        let category = HealthFindingCategory {
            dimension,
            severity,
        };
        counts[severity_index(severity)] = file_counts
            .and_then(|entries| entries.get(category.key()))
            .map_or(0, |entry| entry.count);
    }
    counts
}

fn overflowing_severities(current: [usize; 3], baseline: [usize; 3]) -> [bool; 3] {
    let mut available = baseline;
    let mut overflow = [false; 3];

    for severity_idx in 0..3 {
        let compatible = available[severity_idx..].iter().sum::<usize>();
        overflow[severity_idx] = compatible < current[severity_idx];

        let mut matched = current[severity_idx].min(compatible);
        for slot in available.iter_mut().skip(severity_idx) {
            let taken = matched.min(*slot);
            *slot -= taken;
            matched -= taken;
            if matched == 0 {
                break;
            }
        }
    }

    overflow
}

fn health_overflow_categories(
    current_counts: &HealthFindingCountMap,
    baseline_counts: &HealthFindingCountMap,
) -> FxHashMap<String, FxHashSet<&'static str>> {
    let mut overflow_by_path = FxHashMap::default();

    for (path, current_file_counts) in current_counts {
        let mut overflow_categories: FxHashSet<&'static str> = FxHashSet::default();
        let baseline_file_counts = baseline_counts.get(path);

        for dimension in HEALTH_FINDING_DIMENSIONS {
            let current = severity_counts_for_dimension(Some(current_file_counts), dimension);
            let baseline = severity_counts_for_dimension(baseline_file_counts, dimension);
            let overflow = overflowing_severities(current, baseline);

            for severity in [
                fallow_output::FindingSeverity::Moderate,
                fallow_output::FindingSeverity::High,
                fallow_output::FindingSeverity::Critical,
            ] {
                if overflow[severity_index(severity)] {
                    overflow_categories.insert(
                        HealthFindingCategory {
                            dimension,
                            severity,
                        }
                        .key(),
                    );
                }
            }
        }

        if !overflow_categories.is_empty() {
            overflow_by_path.insert(path.clone(), overflow_categories);
        }
    }

    overflow_by_path
}

fn health_overlap_entry_count(
    current_counts: &HealthFindingCountMap,
    baseline_counts: &HealthFindingCountMap,
) -> usize {
    let mut overlap = 0;

    for (path, baseline_file_counts) in baseline_counts {
        let current_file_counts = current_counts.get(path);

        for dimension in HEALTH_FINDING_DIMENSIONS {
            let current_total: usize =
                severity_counts_for_dimension(current_file_counts, dimension)
                    .into_iter()
                    .sum();
            let baseline_total: usize =
                severity_counts_for_dimension(Some(baseline_file_counts), dimension)
                    .into_iter()
                    .sum();
            overlap += current_total.min(baseline_total);
        }
    }

    overlap
}

fn runtime_coverage_finding_key(
    finding: &fallow_output::RuntimeCoverageFinding,
    _root: &Path,
) -> String {
    finding
        .stable_id
        .clone()
        .unwrap_or_else(|| finding.id.clone())
}

/// Line-move-tolerant writer key: `path\0name\0source_hash`.
///
/// Returns `None` when the finding carries no `source_hash` (e.g. a 0.5-shape
/// sidecar or an un-migrated producer); such findings fall back to the
/// line-sensitive `runtime_coverage_finding_key` for suppression. The NUL
/// separator avoids collisions with paths/names that contain `:`.
fn runtime_coverage_source_hash_key(
    finding: &fallow_output::RuntimeCoverageFinding,
    root: &Path,
) -> Option<String> {
    finding.source_hash.as_deref().map(|hash| {
        format!(
            "{}\0{}\0{}",
            relative_path(&finding.path, root),
            finding.function,
            hash
        )
    })
}

/// Filter health findings to only include those not present in the baseline.
pub fn filter_new_health_findings(
    mut findings: Vec<fallow_output::ComplexityViolation>,
    baseline: &HealthBaselineData,
    root: &Path,
) -> Vec<fallow_output::ComplexityViolation> {
    if !baseline.finding_counts.is_empty() {
        let current_counts = health_finding_counts(&findings, root);
        let overflow_categories =
            health_overflow_categories(&current_counts, &baseline.finding_counts);
        findings.retain(|finding| {
            let path = relative_path(&finding.path, root);
            overflow_categories.get(&path).is_some_and(|categories| {
                health_finding_categories(finding)
                    .into_iter()
                    .flatten()
                    .any(|category| categories.contains(category.key()))
            })
        });
        return findings;
    }

    let baseline_keys: FxHashSet<&str> = baseline.findings.iter().map(String::as_str).collect();
    findings.retain(|f| {
        let key = health_finding_key(f, root);
        !baseline_keys.contains(key.as_str())
    });
    findings
}

pub fn filter_new_runtime_coverage_findings(
    mut findings: Vec<fallow_output::RuntimeCoverageFinding>,
    baseline: &HealthBaselineData,
    root: &Path,
) -> Vec<fallow_output::RuntimeCoverageFinding> {
    let baseline_keys: FxHashSet<&str> = baseline
        .runtime_coverage_findings
        .iter()
        .map(String::as_str)
        .collect();
    let baseline_source_hash_keys: FxHashSet<&str> = baseline
        .runtime_coverage_source_hashes
        .iter()
        .map(String::as_str)
        .collect();
    findings.retain(|finding| {
        let suppressed_by_stable_id = finding
            .stable_id
            .as_deref()
            .is_some_and(|id| baseline_keys.contains(id));
        let suppressed_by_legacy_id = baseline_keys.contains(finding.id.as_str());
        let suppressed_by_source_hash = runtime_coverage_source_hash_key(finding, root)
            .is_some_and(|key| baseline_source_hash_keys.contains(key.as_str()));
        !(suppressed_by_stable_id || suppressed_by_legacy_id || suppressed_by_source_hash)
    });
    findings
}

/// Filter refactoring targets to only include those not present in the baseline.
pub fn filter_new_health_targets(
    mut targets: Vec<fallow_output::RefactoringTarget>,
    baseline: &HealthBaselineData,
    root: &Path,
) -> Vec<fallow_output::RefactoringTarget> {
    let baseline_keys: FxHashSet<&str> = baseline.target_keys.iter().map(String::as_str).collect();
    targets.retain(|t| {
        let key = target_baseline_key(t, root);
        !baseline_keys.contains(key.as_str())
    });
    targets
}

/// Per-category delta between current results and a baseline.
#[derive(Debug, Clone, serde::Serialize)]
pub struct CategoryDelta {
    pub current: usize,
    pub baseline: usize,
    pub delta: i64,
}

/// Deltas between current analysis results and a saved baseline.
///
/// Used in combined mode to show +/- counts in the failure summary and
/// to emit `baseline_deltas` in JSON output.
#[derive(Debug, Clone)]
pub struct BaselineDeltas {
    /// Net change in total issue count (positive = more issues).
    pub total_delta: i64,
    /// Per-category deltas keyed by category name.
    pub per_category: Vec<(String, CategoryDelta)>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::duplicates::{CloneGroup, CloneInstance, DuplicationReport, DuplicationStats};
    use crate::results::{
        AnalysisResults, BoundaryViolationFinding, CircularDependencyFinding, DependencyLocation,
        UnusedDependency, UnusedDependencyFinding, UnusedDevDependencyFinding, UnusedExport,
        UnusedFile,
    };
    use fallow_types::output_dead_code::{
        UnusedExportFinding, UnusedFileFinding, UnusedTypeFinding,
    };
    use std::path::PathBuf;

    fn make_results() -> AnalysisResults {
        AnalysisResults {
            unused_files: vec![
                UnusedFileFinding::with_actions(UnusedFile {
                    path: PathBuf::from("src/old.ts"),
                }),
                UnusedFileFinding::with_actions(UnusedFile {
                    path: PathBuf::from("src/dead.ts"),
                }),
            ],
            unused_exports: vec![UnusedExportFinding::with_actions(UnusedExport {
                path: PathBuf::from("src/utils.ts"),
                export_name: "helperA".to_string(),
                is_type_only: false,
                line: 5,
                col: 0,
                span_start: 40,
                is_re_export: false,
            })],
            unused_types: vec![UnusedTypeFinding::with_actions(UnusedExport {
                path: PathBuf::from("src/types.ts"),
                export_name: "OldType".to_string(),
                is_type_only: true,
                line: 10,
                col: 0,
                span_start: 100,
                is_re_export: false,
            })],
            unused_dependencies: vec![UnusedDependencyFinding::with_actions(UnusedDependency {
                package_name: "lodash".to_string(),
                location: DependencyLocation::Dependencies,
                path: PathBuf::from("package.json"),
                line: 5,
                used_in_workspaces: Vec::new(),
            })],
            unused_dev_dependencies: vec![UnusedDevDependencyFinding::with_actions(
                UnusedDependency {
                    package_name: "jest".to_string(),
                    location: DependencyLocation::DevDependencies,
                    path: PathBuf::from("package.json"),
                    line: 5,
                    used_in_workspaces: Vec::new(),
                },
            )],
            ..Default::default()
        }
    }

    #[test]
    fn baseline_from_results_captures_all_fields() {
        let results = make_results();
        let baseline = BaselineData::from_results(&results, Path::new(""));
        assert_eq!(baseline.unused_files.len(), 2);
        assert!(baseline.unused_files.contains(&"src/old.ts".to_string()));
        assert!(baseline.unused_files.contains(&"src/dead.ts".to_string()));
        assert_eq!(baseline.unused_exports, vec!["src/utils.ts:helperA"]);
        assert_eq!(baseline.unused_types, vec!["src/types.ts:OldType"]);
        assert_eq!(baseline.unused_dependencies, vec!["package.json:lodash"]);
        assert_eq!(baseline.unused_dev_dependencies, vec!["package.json:jest"]);
    }

    #[test]
    fn dependency_baseline_keys_include_package_json_path() {
        let root = Path::new("/repo");
        let results = AnalysisResults {
            unused_dependencies: vec![
                UnusedDependencyFinding::with_actions(UnusedDependency {
                    package_name: "lodash-es".to_string(),
                    location: DependencyLocation::Dependencies,
                    path: PathBuf::from("/repo/packages/app-a/package.json"),
                    line: 5,
                    used_in_workspaces: Vec::new(),
                }),
                UnusedDependencyFinding::with_actions(UnusedDependency {
                    package_name: "lodash-es".to_string(),
                    location: DependencyLocation::Dependencies,
                    path: PathBuf::from("/repo/packages/app-b/package.json"),
                    line: 5,
                    used_in_workspaces: Vec::new(),
                }),
            ],
            ..Default::default()
        };

        let baseline = BaselineData::from_results(&results, root);

        assert_eq!(
            baseline.unused_dependencies,
            vec![
                "packages/app-a/package.json:lodash-es",
                "packages/app-b/package.json:lodash-es"
            ]
        );
    }

    #[test]
    fn dependency_baseline_filter_matches_path_before_package_name() {
        let root = Path::new("/repo");
        let results = AnalysisResults {
            unused_dependencies: vec![
                UnusedDependencyFinding::with_actions(UnusedDependency {
                    package_name: "lodash-es".to_string(),
                    location: DependencyLocation::Dependencies,
                    path: PathBuf::from("/repo/packages/app-a/package.json"),
                    line: 5,
                    used_in_workspaces: Vec::new(),
                }),
                UnusedDependencyFinding::with_actions(UnusedDependency {
                    package_name: "lodash-es".to_string(),
                    location: DependencyLocation::Dependencies,
                    path: PathBuf::from("/repo/packages/app-b/package.json"),
                    line: 5,
                    used_in_workspaces: Vec::new(),
                }),
            ],
            ..Default::default()
        };
        let baseline = BaselineData {
            unused_dependencies: vec!["packages/app-a/package.json:lodash-es".to_string()],
            ..BaselineData::from_results(&AnalysisResults::default(), root)
        };

        let filtered = filter_new_issues(results, &baseline, root);

        assert_eq!(filtered.unused_dependencies.len(), 1);
        assert_eq!(
            filtered.unused_dependencies[0].dep.path,
            PathBuf::from("/repo/packages/app-b/package.json")
        );
    }

    #[test]
    fn dependency_baseline_filter_supports_legacy_package_only_keys() {
        let root = Path::new("/repo");
        let results = AnalysisResults {
            unused_dependencies: vec![UnusedDependencyFinding::with_actions(UnusedDependency {
                package_name: "lodash-es".to_string(),
                location: DependencyLocation::Dependencies,
                path: PathBuf::from("/repo/packages/app/package.json"),
                line: 5,
                used_in_workspaces: Vec::new(),
            })],
            ..Default::default()
        };
        let baseline = BaselineData {
            unused_dependencies: vec!["lodash-es".to_string()],
            ..BaselineData::from_results(&AnalysisResults::default(), root)
        };

        let filtered = filter_new_issues(results, &baseline, root);

        assert!(filtered.unused_dependencies.is_empty());
    }

    #[test]
    fn baseline_serialization_roundtrip() {
        let results = make_results();
        let baseline = BaselineData::from_results(&results, Path::new(""));
        let json = serde_json::to_string(&baseline).unwrap();
        let deserialized: BaselineData = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.unused_files, baseline.unused_files);
        assert_eq!(deserialized.unused_exports, baseline.unused_exports);
        assert_eq!(deserialized.unused_types, baseline.unused_types);
        assert_eq!(
            deserialized.unused_dependencies,
            baseline.unused_dependencies
        );
        assert_eq!(
            deserialized.unused_dev_dependencies,
            baseline.unused_dev_dependencies
        );
    }

    #[test]
    fn filter_removes_baseline_issues() {
        let results = make_results();
        let baseline = BaselineData::from_results(&results, Path::new(""));
        let filtered = filter_new_issues(results, &baseline, Path::new(""));
        assert!(
            filtered.unused_files.is_empty(),
            "all files were in baseline"
        );
        assert!(
            filtered.unused_exports.is_empty(),
            "all exports were in baseline"
        );
        assert!(
            filtered.unused_types.is_empty(),
            "all types were in baseline"
        );
        assert!(
            filtered.unused_dependencies.is_empty(),
            "all deps were in baseline"
        );
        assert!(
            filtered.unused_dev_dependencies.is_empty(),
            "all dev deps were in baseline"
        );
    }

    #[test]
    fn filter_keeps_new_issues_not_in_baseline() {
        let baseline = BaselineData {
            unused_files: vec!["src/old.ts".to_string()],
            unused_exports: vec![],
            unused_types: vec![],
            private_type_leaks: vec![],
            unused_dependencies: vec![],
            unused_dev_dependencies: vec![],
            circular_dependencies: vec![],
            re_export_cycles: vec![],
            unused_optional_dependencies: vec![],
            unused_enum_members: vec![],
            unused_class_members: vec![],
            unused_store_members: vec![],
            unprovided_injects: vec![],
            unrendered_components: vec![],
            unused_component_props: vec![],
            unused_component_emits: vec![],
            unused_component_inputs: vec![],
            unused_component_outputs: vec![],
            unused_svelte_events: vec![],
            unused_server_actions: vec![],
            unused_load_data_keys: vec![],
            unresolved_imports: vec![],
            unlisted_dependencies: vec![],
            duplicate_exports: vec![],
            type_only_dependencies: vec![],
            test_only_dependencies: vec![],
            boundary_violations: vec![],
            boundary_coverage_violations: vec![],
            boundary_call_violations: vec![],
            policy_violations: vec![],
            stale_suppressions: vec![],
            unused_catalog_entries: vec![],
            empty_catalog_groups: vec![],
            unresolved_catalog_references: vec![],
            unused_dependency_overrides: vec![],
            misconfigured_dependency_overrides: vec![],
            invalid_client_exports: vec![],
            mixed_client_server_barrels: vec![],
            misplaced_directives: vec![],
            route_collisions: vec![],
            dynamic_segment_name_conflicts: vec![],
        };
        let results = AnalysisResults {
            unused_files: vec![
                UnusedFileFinding::with_actions(UnusedFile {
                    path: PathBuf::from("src/old.ts"),
                }),
                UnusedFileFinding::with_actions(UnusedFile {
                    path: PathBuf::from("src/new-dead.ts"),
                }),
            ],
            ..Default::default()
        };
        let filtered = filter_new_issues(results, &baseline, Path::new(""));
        assert_eq!(filtered.unused_files.len(), 1);
        assert_eq!(
            filtered.unused_files[0].file.path,
            PathBuf::from("src/new-dead.ts")
        );
    }

    #[test]
    fn filter_with_empty_baseline_keeps_all() {
        let baseline = BaselineData {
            unused_files: vec![],
            unused_exports: vec![],
            unused_types: vec![],
            private_type_leaks: vec![],
            unused_dependencies: vec![],
            unused_dev_dependencies: vec![],
            circular_dependencies: vec![],
            re_export_cycles: vec![],
            unused_optional_dependencies: vec![],
            unused_enum_members: vec![],
            unused_class_members: vec![],
            unused_store_members: vec![],
            unprovided_injects: vec![],
            unrendered_components: vec![],
            unused_component_props: vec![],
            unused_component_emits: vec![],
            unused_component_inputs: vec![],
            unused_component_outputs: vec![],
            unused_svelte_events: vec![],
            unused_server_actions: vec![],
            unused_load_data_keys: vec![],
            unresolved_imports: vec![],
            unlisted_dependencies: vec![],
            duplicate_exports: vec![],
            type_only_dependencies: vec![],
            test_only_dependencies: vec![],
            boundary_violations: vec![],
            boundary_coverage_violations: vec![],
            boundary_call_violations: vec![],
            policy_violations: vec![],
            stale_suppressions: vec![],
            unused_catalog_entries: vec![],
            empty_catalog_groups: vec![],
            unresolved_catalog_references: vec![],
            unused_dependency_overrides: vec![],
            misconfigured_dependency_overrides: vec![],
            invalid_client_exports: vec![],
            mixed_client_server_barrels: vec![],
            misplaced_directives: vec![],
            route_collisions: vec![],
            dynamic_segment_name_conflicts: vec![],
        };
        let results = make_results();
        let filtered = filter_new_issues(results, &baseline, Path::new(""));
        assert_eq!(filtered.unused_files.len(), 2);
        assert_eq!(filtered.unused_exports.len(), 1);
    }

    #[test]
    fn filter_new_exports_by_file_and_name() {
        let baseline = BaselineData {
            unused_files: vec![],
            unused_exports: vec!["src/utils.ts:helperA".to_string()],
            unused_types: vec![],
            private_type_leaks: vec![],
            unused_dependencies: vec![],
            unused_dev_dependencies: vec![],
            circular_dependencies: vec![],
            re_export_cycles: vec![],
            unused_optional_dependencies: vec![],
            unused_enum_members: vec![],
            unused_class_members: vec![],
            unused_store_members: vec![],
            unprovided_injects: vec![],
            unrendered_components: vec![],
            unused_component_props: vec![],
            unused_component_emits: vec![],
            unused_component_inputs: vec![],
            unused_component_outputs: vec![],
            unused_svelte_events: vec![],
            unused_server_actions: vec![],
            unused_load_data_keys: vec![],
            unresolved_imports: vec![],
            unlisted_dependencies: vec![],
            duplicate_exports: vec![],
            type_only_dependencies: vec![],
            test_only_dependencies: vec![],
            boundary_violations: vec![],
            boundary_coverage_violations: vec![],
            boundary_call_violations: vec![],
            policy_violations: vec![],
            stale_suppressions: vec![],
            unused_catalog_entries: vec![],
            empty_catalog_groups: vec![],
            unresolved_catalog_references: vec![],
            unused_dependency_overrides: vec![],
            misconfigured_dependency_overrides: vec![],
            invalid_client_exports: vec![],
            mixed_client_server_barrels: vec![],
            misplaced_directives: vec![],
            route_collisions: vec![],
            dynamic_segment_name_conflicts: vec![],
        };
        let results = AnalysisResults {
            unused_exports: vec![
                UnusedExportFinding::with_actions(UnusedExport {
                    path: PathBuf::from("src/utils.ts"),
                    export_name: "helperA".to_string(),
                    is_type_only: false,
                    line: 5,
                    col: 0,
                    span_start: 40,
                    is_re_export: false,
                }),
                UnusedExportFinding::with_actions(UnusedExport {
                    path: PathBuf::from("src/utils.ts"),
                    export_name: "helperB".to_string(),
                    is_type_only: false,
                    line: 10,
                    col: 0,
                    span_start: 80,
                    is_re_export: false,
                }),
            ],
            ..Default::default()
        };
        let filtered = filter_new_issues(results, &baseline, Path::new(""));
        assert_eq!(filtered.unused_exports.len(), 1);
        assert_eq!(filtered.unused_exports[0].export.export_name, "helperB");
    }

    fn make_clone_group(instances: Vec<(&str, usize, usize)>) -> CloneGroup {
        CloneGroup {
            instances: instances
                .into_iter()
                .map(|(file, start, end)| CloneInstance {
                    file: PathBuf::from(file),
                    start_line: start,
                    end_line: end,
                    start_col: 0,
                    end_col: 0,
                    fragment: String::new(),
                })
                .collect(),
            token_count: 50,
            line_count: 10,
        }
    }

    fn make_duplication_report(groups: Vec<CloneGroup>) -> DuplicationReport {
        DuplicationReport {
            clone_groups: groups,
            clone_families: vec![],
            mirrored_directories: vec![],
            stats: DuplicationStats {
                total_files: 10,
                files_with_clones: 2,
                total_lines: 1000,
                duplicated_lines: 100,
                total_tokens: 5000,
                duplicated_tokens: 500,
                clone_groups: 1,
                clone_instances: 2,
                duplication_percentage: 10.0,
                clone_groups_below_min_occurrences: 0,
            },
        }
    }

    #[test]
    fn clone_group_key_is_deterministic() {
        let root = Path::new("/project");
        let group = make_clone_group(vec![
            ("/project/src/a.ts", 1, 10),
            ("/project/src/b.ts", 5, 15),
        ]);
        let key1 = clone_group_key(&group, root);
        let key2 = clone_group_key(&group, root);
        assert_eq!(key1, key2);
    }

    #[test]
    fn clone_group_key_is_sorted() {
        let root = Path::new("/project");
        let group_ab = make_clone_group(vec![
            ("/project/src/a.ts", 1, 10),
            ("/project/src/b.ts", 5, 15),
        ]);
        let group_ba = make_clone_group(vec![
            ("/project/src/b.ts", 5, 15),
            ("/project/src/a.ts", 1, 10),
        ]);
        assert_eq!(
            clone_group_key(&group_ab, root),
            clone_group_key(&group_ba, root),
            "key should be stable regardless of instance order"
        );
    }

    #[test]
    fn duplication_baseline_roundtrip() {
        let root = Path::new("/project");
        let group = make_clone_group(vec![
            ("/project/src/a.ts", 1, 10),
            ("/project/src/b.ts", 5, 15),
        ]);
        let report = make_duplication_report(vec![group]);
        let baseline = DuplicationBaselineData::from_report(&report, root);
        let json = serde_json::to_string(&baseline).unwrap();
        let deserialized: DuplicationBaselineData = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.clone_groups, baseline.clone_groups);
    }

    #[test]
    fn filter_new_clone_groups_removes_baseline() {
        let root = Path::new("/project");
        let group = make_clone_group(vec![
            ("/project/src/a.ts", 1, 10),
            ("/project/src/b.ts", 5, 15),
        ]);
        let report = make_duplication_report(vec![group]);
        let baseline = DuplicationBaselineData::from_report(&report, root);
        let filtered = filter_new_clone_groups(report, &baseline, root);
        assert!(
            filtered.clone_groups.is_empty(),
            "baseline group should be filtered out"
        );
    }

    #[test]
    fn filter_new_clone_groups_keeps_new_groups() {
        let root = Path::new("/project");
        let baseline_group = make_clone_group(vec![
            ("/project/src/a.ts", 1, 10),
            ("/project/src/b.ts", 5, 15),
        ]);
        let new_group = make_clone_group(vec![
            ("/project/src/c.ts", 20, 30),
            ("/project/src/d.ts", 25, 35),
        ]);
        let baseline_report = make_duplication_report(vec![baseline_group]);
        let baseline = DuplicationBaselineData::from_report(&baseline_report, root);

        let report = make_duplication_report(vec![
            make_clone_group(vec![
                ("/project/src/a.ts", 1, 10),
                ("/project/src/b.ts", 5, 15),
            ]),
            new_group,
        ]);
        let filtered = filter_new_clone_groups(report, &baseline, root);
        assert_eq!(
            filtered.clone_groups.len(),
            1,
            "only the new group should remain"
        );
    }

    #[test]
    fn recompute_stats_after_filtering() {
        let root = Path::new("/project");
        let group = make_clone_group(vec![
            ("/project/src/a.ts", 1, 10),
            ("/project/src/b.ts", 5, 15),
        ]);
        let report = make_duplication_report(vec![group]);
        let baseline = DuplicationBaselineData::from_report(&report, root);
        let filtered = filter_new_clone_groups(report, &baseline, root);
        assert_eq!(filtered.stats.clone_groups, 0);
        assert_eq!(filtered.stats.clone_instances, 0);
        assert_eq!(filtered.stats.duplicated_lines, 0);
    }

    #[test]
    fn recompute_stats_zero_total_lines() {
        let report = DuplicationReport {
            clone_groups: vec![],
            clone_families: vec![],
            mirrored_directories: vec![],
            stats: DuplicationStats {
                total_files: 0,
                files_with_clones: 0,
                total_lines: 0,
                duplicated_lines: 0,
                total_tokens: 0,
                duplicated_tokens: 0,
                clone_groups: 0,
                clone_instances: 0,
                duplication_percentage: 0.0,
                clone_groups_below_min_occurrences: 0,
            },
        };
        let stats = super::recompute_stats(&report);
        assert!((stats.duplication_percentage - 0.0).abs() < f64::EPSILON);
    }

    fn make_health_finding(
        root: &Path,
        name: &str,
        line: u32,
    ) -> fallow_output::ComplexityViolation {
        make_health_finding_with(
            root,
            name,
            line,
            fallow_output::ExceededThreshold::Both,
            fallow_output::FindingSeverity::High,
        )
    }

    fn make_health_finding_with(
        root: &Path,
        name: &str,
        line: u32,
        exceeded: fallow_output::ExceededThreshold,
        severity: fallow_output::FindingSeverity,
    ) -> fallow_output::ComplexityViolation {
        fallow_output::ComplexityViolation {
            path: root.join("src/utils.ts"),
            name: name.to_string(),
            line,
            col: 0,
            cyclomatic: 25,
            cognitive: 30,
            line_count: 80,
            param_count: 0,
            react_hook_count: 0,
            react_jsx_max_depth: 0,
            react_prop_count: 0,
            react_hook_profile: None,
            exceeded,
            severity,
            crap: None,
            coverage_pct: None,
            coverage_tier: None,
            coverage_source: None,
            inherited_from: None,
            component_rollup: None,
            contributions: Vec::new(),
            effective_thresholds: None,
            threshold_source: None,
        }
    }

    #[test]
    fn health_baseline_roundtrip() {
        let root = PathBuf::from("/project");
        let findings = vec![make_health_finding(&root, "parseExpression", 42)];
        let baseline = HealthBaselineData::from_findings(&findings, &[], &[], &root);
        let json = serde_json::to_string(&baseline).unwrap();
        let deserialized: HealthBaselineData = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized.findings, baseline.findings);
        assert_eq!(baseline.findings, Vec::<String>::new());
        assert_eq!(
            deserialized.finding_counts["src/utils.ts"]["complexity_high"].count,
            1
        );
        assert!(!json.contains("parseExpression"));
    }

    #[test]
    fn health_baseline_filters_known_findings() {
        let root = PathBuf::from("/project");
        let mut findings = vec![
            make_health_finding(&root, "parseExpression", 42),
            make_health_finding(&root, "newFunction", 100),
        ];
        findings[1].path = root.join("src/other.ts");
        let baseline = HealthBaselineData::from_findings(&findings[..1], &[], &[], &root);
        let filtered = filter_new_health_findings(findings, &baseline, &root);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].name, "newFunction");
    }

    #[test]
    fn health_baseline_filters_shifted_lines_with_same_category_count() {
        let root = PathBuf::from("/project");
        let baseline = HealthBaselineData::from_findings(
            &[make_health_finding(&root, "parseExpression", 42)],
            &[],
            &[],
            &root,
        );
        let filtered = filter_new_health_findings(
            vec![make_health_finding(&root, "parseExpression", 43)],
            &baseline,
            &root,
        );
        assert!(filtered.is_empty());
    }

    #[test]
    fn health_baseline_reports_full_category_when_count_increases() {
        let root = PathBuf::from("/project");
        let baseline = HealthBaselineData::from_findings(
            &[make_health_finding(&root, "parseExpression", 42)],
            &[],
            &[],
            &root,
        );
        let filtered = filter_new_health_findings(
            vec![
                make_health_finding(&root, "parseExpression", 43),
                make_health_finding(&root, "newFunction", 100),
            ],
            &baseline,
            &root,
        );
        assert_eq!(filtered.len(), 2);
    }

    #[test]
    fn health_baseline_legacy_findings_still_load() {
        let root = PathBuf::from("/project");
        let baseline = HealthBaselineData {
            findings: vec!["src/utils.ts:parseExpression:42".to_owned()],
            finding_counts: BTreeMap::new(),
            target_keys: vec![],
            runtime_coverage_findings: vec![],
            runtime_coverage_source_hashes: vec![],
        };
        let filtered = filter_new_health_findings(
            vec![make_health_finding(&root, "parseExpression", 42)],
            &baseline,
            &root,
        );
        assert!(filtered.is_empty());
    }

    #[test]
    fn health_baseline_keeps_crap_categories_separate_from_complexity() {
        let root = PathBuf::from("/project");
        let baseline = HealthBaselineData::from_findings(
            &[make_health_finding_with(
                &root,
                "parseExpression",
                42,
                fallow_output::ExceededThreshold::Crap,
                fallow_output::FindingSeverity::High,
            )],
            &[],
            &[],
            &root,
        );
        let filtered = filter_new_health_findings(
            vec![
                make_health_finding_with(
                    &root,
                    "parseExpression",
                    43,
                    fallow_output::ExceededThreshold::Crap,
                    fallow_output::FindingSeverity::High,
                ),
                make_health_finding(&root, "newComplexityOnlyFunction", 100),
            ],
            &baseline,
            &root,
        );
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].name, "newComplexityOnlyFunction");
    }

    #[test]
    fn health_baseline_suppresses_findings_that_only_improve_in_severity() {
        let root = PathBuf::from("/project");
        let baseline = HealthBaselineData::from_findings(
            &[make_health_finding_with(
                &root,
                "parseExpression",
                42,
                fallow_output::ExceededThreshold::Both,
                fallow_output::FindingSeverity::Critical,
            )],
            &[],
            &[],
            &root,
        );
        let filtered = filter_new_health_findings(
            vec![make_health_finding_with(
                &root,
                "parseExpression",
                42,
                fallow_output::ExceededThreshold::Both,
                fallow_output::FindingSeverity::High,
            )],
            &baseline,
            &root,
        );
        assert!(filtered.is_empty());
    }

    #[test]
    fn health_baseline_still_reports_worse_current_severity_as_new() {
        let root = PathBuf::from("/project");
        let baseline = HealthBaselineData::from_findings(
            &[make_health_finding_with(
                &root,
                "parseExpression",
                42,
                fallow_output::ExceededThreshold::Both,
                fallow_output::FindingSeverity::High,
            )],
            &[],
            &[],
            &root,
        );
        let filtered = filter_new_health_findings(
            vec![make_health_finding_with(
                &root,
                "parseExpression",
                42,
                fallow_output::ExceededThreshold::Both,
                fallow_output::FindingSeverity::Critical,
            )],
            &baseline,
            &root,
        );
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].name, "parseExpression");
        assert!(matches!(
            filtered[0].severity,
            fallow_output::FindingSeverity::Critical
        ));
    }

    #[test]
    fn health_baseline_overlap_counts_partial_category_overflow() {
        let root = PathBuf::from("/project");
        let baseline = HealthBaselineData::from_findings(
            &[make_health_finding(&root, "parseExpression", 42)],
            &[],
            &[],
            &root,
        );
        let overlap = baseline.overlap_entry_count(
            &[
                make_health_finding(&root, "parseExpression", 42),
                make_health_finding(&root, "newFunction", 100),
            ],
            &root,
        );
        assert_eq!(overlap, 1);
    }

    #[test]
    fn health_baseline_empty_keeps_all() {
        let root = PathBuf::from("/project");
        let findings = vec![make_health_finding(&root, "parseExpression", 42)];
        let baseline = HealthBaselineData {
            findings: vec![],
            finding_counts: BTreeMap::new(),
            target_keys: vec![],
            runtime_coverage_findings: vec![],
            runtime_coverage_source_hashes: vec![],
        };
        let filtered = filter_new_health_findings(findings, &baseline, &root);
        assert_eq!(filtered.len(), 1);
    }

    #[test]
    fn circular_dep_key_is_order_independent() {
        use crate::results::CircularDependency;

        let dep_ab = CircularDependencyFinding::with_actions(CircularDependency {
            files: vec![PathBuf::from("src/a.ts"), PathBuf::from("src/b.ts")],
            length: 2,
            line: 1,
            col: 0,
            edges: Vec::new(),
            is_cross_package: false,
        });
        let dep_ba = CircularDependencyFinding::with_actions(CircularDependency {
            files: vec![PathBuf::from("src/b.ts"), PathBuf::from("src/a.ts")],
            length: 2,
            line: 1,
            col: 0,
            edges: Vec::new(),
            is_cross_package: false,
        });
        assert_eq!(
            super::circular_dep_key(&dep_ab.cycle, Path::new("")),
            super::circular_dep_key(&dep_ba.cycle, Path::new("")),
            "same files in different order should produce identical keys"
        );
    }

    #[test]
    fn circular_dep_key_different_files_different_keys() {
        use crate::results::CircularDependency;

        let dep1 = CircularDependencyFinding::with_actions(CircularDependency {
            files: vec![PathBuf::from("src/a.ts"), PathBuf::from("src/b.ts")],
            length: 2,
            line: 1,
            col: 0,
            edges: Vec::new(),
            is_cross_package: false,
        });
        let dep2 = CircularDependencyFinding::with_actions(CircularDependency {
            files: vec![PathBuf::from("src/a.ts"), PathBuf::from("src/c.ts")],
            length: 2,
            line: 1,
            col: 0,
            edges: Vec::new(),
            is_cross_package: false,
        });
        assert_ne!(
            super::circular_dep_key(&dep1.cycle, Path::new("")),
            super::circular_dep_key(&dep2.cycle, Path::new("")),
        );
    }

    #[test]
    fn circular_dep_key_three_files_order_independent() {
        use crate::results::CircularDependency;

        let dep_abc = CircularDependencyFinding::with_actions(CircularDependency {
            files: vec![
                PathBuf::from("src/a.ts"),
                PathBuf::from("src/b.ts"),
                PathBuf::from("src/c.ts"),
            ],
            length: 3,
            line: 1,
            col: 0,
            edges: Vec::new(),
            is_cross_package: false,
        });
        let dep_cab = CircularDependencyFinding::with_actions(CircularDependency {
            files: vec![
                PathBuf::from("src/c.ts"),
                PathBuf::from("src/a.ts"),
                PathBuf::from("src/b.ts"),
            ],
            length: 3,
            line: 1,
            col: 0,
            edges: Vec::new(),
            is_cross_package: false,
        });
        assert_eq!(
            super::circular_dep_key(&dep_abc.cycle, Path::new("")),
            super::circular_dep_key(&dep_cab.cycle, Path::new("")),
        );
    }

    #[expect(
        clippy::too_many_lines,
        reason = "test fixture; linear setup/assert, length is not a maintainability concern"
    )]
    fn make_full_results() -> AnalysisResults {
        use crate::results::*;
        use crate::source::MemberKind;

        let mut r = make_results();
        r.circular_dependencies
            .push(CircularDependencyFinding::with_actions(
                CircularDependency {
                    files: vec![PathBuf::from("src/a.ts"), PathBuf::from("src/b.ts")],
                    length: 2,
                    line: 1,
                    col: 0,
                    edges: Vec::new(),
                    is_cross_package: false,
                },
            ));
        r.unused_optional_dependencies
            .push(UnusedOptionalDependencyFinding::with_actions(
                UnusedDependency {
                    package_name: "fsevents".to_string(),
                    location: DependencyLocation::OptionalDependencies,
                    path: PathBuf::from("package.json"),
                    line: 15,
                    used_in_workspaces: Vec::new(),
                },
            ));
        r.unused_enum_members
            .push(UnusedEnumMemberFinding::with_actions(UnusedMember {
                path: PathBuf::from("src/enums.ts"),
                parent_name: "Status".to_string(),
                member_name: "Deprecated".to_string(),
                kind: MemberKind::EnumMember,
                line: 8,
                col: 0,
            }));
        r.unused_class_members
            .push(UnusedClassMemberFinding::with_actions(UnusedMember {
                path: PathBuf::from("src/service.ts"),
                parent_name: "UserService".to_string(),
                member_name: "legacy".to_string(),
                kind: MemberKind::ClassMethod,
                line: 42,
                col: 0,
            }));
        r.unused_store_members
            .push(UnusedStoreMemberFinding::with_actions(UnusedMember {
                path: PathBuf::from("src/store.ts"),
                parent_name: "useStore".to_string(),
                member_name: "legacyAction".to_string(),
                kind: MemberKind::StoreMember,
                line: 17,
                col: 0,
            }));
        r.unresolved_imports.push(
            fallow_types::output_dead_code::UnresolvedImportFinding::with_actions(
                crate::results::UnresolvedImport {
                    path: PathBuf::from("src/app.ts"),
                    specifier: "./missing".to_string(),
                    line: 3,
                    col: 0,
                    specifier_col: 0,
                },
            ),
        );
        r.unlisted_dependencies
            .push(crate::results::UnlistedDependencyFinding::with_actions(
                UnlistedDependency {
                    package_name: "chalk".to_string(),
                    imported_from: vec![],
                },
            ));
        r.duplicate_exports
            .push(crate::results::DuplicateExportFinding::with_actions(
                crate::results::DuplicateExport {
                    export_name: "Config".to_string(),
                    locations: vec![
                        crate::results::DuplicateLocation {
                            path: PathBuf::from("src/a.ts"),
                            line: 1,
                            col: 0,
                        },
                        crate::results::DuplicateLocation {
                            path: PathBuf::from("src/b.ts"),
                            line: 5,
                            col: 0,
                        },
                    ],
                },
            ));
        r.type_only_dependencies
            .push(crate::results::TypeOnlyDependencyFinding::with_actions(
                TypeOnlyDependency {
                    package_name: "zod".to_string(),
                    path: PathBuf::from("package.json"),
                    line: 8,
                },
            ));
        r.test_only_dependencies
            .push(crate::results::TestOnlyDependencyFinding::with_actions(
                TestOnlyDependency {
                    package_name: "vitest".to_string(),
                    path: PathBuf::from("package.json"),
                    line: 10,
                },
            ));
        r.boundary_violations.push(
            fallow_types::output_dead_code::BoundaryViolationFinding::with_actions(
                crate::results::BoundaryViolation {
                    from_path: PathBuf::from("src/ui/btn.ts"),
                    to_path: PathBuf::from("src/db/query.ts"),
                    from_zone: "ui".to_string(),
                    to_zone: "db".to_string(),
                    import_specifier: "../db/query".to_string(),
                    line: 1,
                    col: 0,
                },
            ),
        );
        r
    }

    #[test]
    fn baseline_from_results_captures_all_extended_fields() {
        let results = make_full_results();
        let baseline = BaselineData::from_results(&results, Path::new(""));
        assert_eq!(baseline.circular_dependencies.len(), 1);
        assert_eq!(
            baseline.unused_optional_dependencies,
            vec!["package.json:fsevents"]
        );
        assert_eq!(baseline.unused_enum_members.len(), 1);
        assert!(baseline.unused_enum_members[0].contains("Status.Deprecated"));
        assert_eq!(baseline.unused_class_members.len(), 1);
        assert!(baseline.unused_class_members[0].contains("UserService.legacy"));
        assert_eq!(baseline.unused_store_members.len(), 1);
        assert!(baseline.unused_store_members[0].contains("useStore.legacyAction"));
        assert_eq!(baseline.unresolved_imports.len(), 1);
        assert!(baseline.unresolved_imports[0].contains("./missing"));
        assert_eq!(baseline.unlisted_dependencies, vec!["chalk"]);
        assert_eq!(baseline.duplicate_exports.len(), 1);
        assert!(baseline.duplicate_exports[0].starts_with("Config|"));
        assert_eq!(baseline.type_only_dependencies, vec!["package.json:zod"]);
        assert_eq!(baseline.test_only_dependencies, vec!["package.json:vitest"]);
        assert_eq!(baseline.boundary_violations.len(), 1);
        assert!(baseline.boundary_violations[0].contains("->"));
    }

    #[test]
    fn filter_removes_all_extended_baseline_issues() {
        let results = make_full_results();
        let baseline = BaselineData::from_results(&results, Path::new(""));
        let filtered = filter_new_issues(results, &baseline, Path::new(""));
        assert!(filtered.circular_dependencies.is_empty());
        assert!(filtered.unused_optional_dependencies.is_empty());
        assert!(filtered.unused_enum_members.is_empty());
        assert!(filtered.unused_class_members.is_empty());
        assert!(filtered.unused_store_members.is_empty());
        assert!(filtered.unresolved_imports.is_empty());
        assert!(filtered.unlisted_dependencies.is_empty());
        assert!(filtered.duplicate_exports.is_empty());
        assert!(filtered.type_only_dependencies.is_empty());
        assert!(filtered.test_only_dependencies.is_empty());
        assert!(filtered.boundary_violations.is_empty());
    }

    #[test]
    fn filter_keeps_new_circular_deps() {
        use crate::results::CircularDependency;
        let baseline = BaselineData {
            circular_dependencies: vec!["src/a.ts->src/b.ts".to_string()],
            ..BaselineData::from_results(&AnalysisResults::default(), Path::new(""))
        };
        let mut results = AnalysisResults::default();
        results
            .circular_dependencies
            .push(CircularDependencyFinding::with_actions(
                CircularDependency {
                    files: vec![PathBuf::from("src/a.ts"), PathBuf::from("src/b.ts")],
                    length: 2,
                    line: 1,
                    col: 0,
                    edges: Vec::new(),
                    is_cross_package: false,
                },
            ));
        results
            .circular_dependencies
            .push(CircularDependencyFinding::with_actions(
                CircularDependency {
                    files: vec![PathBuf::from("src/x.ts"), PathBuf::from("src/y.ts")],
                    length: 2,
                    line: 5,
                    col: 0,
                    edges: Vec::new(),
                    is_cross_package: false,
                },
            ));
        let filtered = filter_new_issues(results, &baseline, Path::new(""));
        assert_eq!(filtered.circular_dependencies.len(), 1);
    }

    #[test]
    fn filter_keeps_new_boundary_violations() {
        use crate::results::BoundaryViolation;
        let baseline = BaselineData {
            boundary_violations: vec!["src/a.ts->src/b.ts".to_string()],
            boundary_coverage_violations: vec![],
            boundary_call_violations: vec![],
            policy_violations: vec![],
            ..BaselineData::from_results(&AnalysisResults::default(), Path::new(""))
        };
        let mut results = AnalysisResults::default();
        results
            .boundary_violations
            .push(BoundaryViolationFinding::with_actions(BoundaryViolation {
                from_path: PathBuf::from("src/a.ts"),
                to_path: PathBuf::from("src/b.ts"),
                from_zone: "a".to_string(),
                to_zone: "b".to_string(),
                import_specifier: "../b".to_string(),
                line: 1,
                col: 0,
            }));
        results
            .boundary_violations
            .push(BoundaryViolationFinding::with_actions(BoundaryViolation {
                from_path: PathBuf::from("src/new.ts"),
                to_path: PathBuf::from("src/secret.ts"),
                from_zone: "new".to_string(),
                to_zone: "secret".to_string(),
                import_specifier: "../secret".to_string(),
                line: 1,
                col: 0,
            }));
        let filtered = filter_new_issues(results, &baseline, Path::new(""));
        assert_eq!(filtered.boundary_violations.len(), 1);
    }

    #[test]
    fn health_targets_baseline_filters_known() {
        let root = PathBuf::from("/project");
        let targets = vec![
            fallow_output::RefactoringTarget {
                path: root.join("src/complex.ts"),
                priority: 80.0,
                efficiency: 40.0,
                recommendation: "Split file".to_string(),
                category: fallow_output::RecommendationCategory::SplitHighImpact,
                effort: fallow_output::EffortEstimate::Medium,
                confidence: fallow_output::Confidence::Medium,
                factors: vec![],
                evidence: None,
            },
            fallow_output::RefactoringTarget {
                path: root.join("src/new-issue.ts"),
                priority: 60.0,
                efficiency: 30.0,
                recommendation: "Extract function".to_string(),
                category: fallow_output::RecommendationCategory::ExtractComplexFunctions,
                effort: fallow_output::EffortEstimate::Low,
                confidence: fallow_output::Confidence::High,
                factors: vec![],
                evidence: None,
            },
        ];
        let baseline = HealthBaselineData::from_findings(&[], &[], &targets[..1], &root);
        let filtered = filter_new_health_targets(targets, &baseline, &root);
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].path, root.join("src/new-issue.ts"));
    }

    #[test]
    fn duplicate_export_key_is_sorted() {
        use crate::results::{DuplicateExport, DuplicateLocation};
        let dup_ab = DuplicateExport {
            export_name: "foo".to_string(),
            locations: vec![
                DuplicateLocation {
                    path: PathBuf::from("src/a.ts"),
                    line: 1,
                    col: 0,
                },
                DuplicateLocation {
                    path: PathBuf::from("src/b.ts"),
                    line: 5,
                    col: 0,
                },
            ],
        };
        let dup_ba = DuplicateExport {
            export_name: "foo".to_string(),
            locations: vec![
                DuplicateLocation {
                    path: PathBuf::from("src/b.ts"),
                    line: 5,
                    col: 0,
                },
                DuplicateLocation {
                    path: PathBuf::from("src/a.ts"),
                    line: 1,
                    col: 0,
                },
            ],
        };
        assert_eq!(
            super::duplicate_export_key(&dup_ab, Path::new("")),
            super::duplicate_export_key(&dup_ba, Path::new("")),
        );
    }

    #[test]
    fn boundary_violation_key_format() {
        use crate::results::BoundaryViolation;
        let v = BoundaryViolation {
            from_path: PathBuf::from("src/ui/btn.ts"),
            to_path: PathBuf::from("src/db/query.ts"),
            from_zone: "ui".to_string(),
            to_zone: "db".to_string(),
            import_specifier: "../db/query".to_string(),
            line: 1,
            col: 0,
        };
        let key = super::boundary_violation_key(&v, Path::new(""));
        assert_eq!(key, "src/ui/btn.ts->src/db/query.ts");
    }

    /// Build results with absolute paths rooted at the given prefix.
    fn make_absolute_results(root: &str) -> AnalysisResults {
        use crate::results::*;
        use crate::source::MemberKind;

        let p = |rel: &str| PathBuf::from(format!("{root}/{rel}"));

        AnalysisResults {
            unused_files: vec![UnusedFileFinding::with_actions(UnusedFile {
                path: p("src/old.ts"),
            })],
            unused_exports: vec![UnusedExportFinding::with_actions(UnusedExport {
                path: p("src/utils.ts"),
                export_name: "helper".to_string(),
                is_type_only: false,
                line: 5,
                col: 0,
                span_start: 40,
                is_re_export: false,
            })],
            unused_dependencies: vec![UnusedDependencyFinding::with_actions(UnusedDependency {
                package_name: "lodash-es".to_string(),
                location: DependencyLocation::Dependencies,
                path: p("packages/app/package.json"),
                line: 5,
                used_in_workspaces: Vec::new(),
            })],
            circular_dependencies: vec![CircularDependencyFinding::with_actions(
                CircularDependency {
                    files: vec![p("src/a.ts"), p("src/b.ts")],
                    length: 2,
                    line: 1,
                    col: 0,
                    edges: Vec::new(),
                    is_cross_package: false,
                },
            )],
            unused_enum_members: vec![UnusedEnumMemberFinding::with_actions(UnusedMember {
                path: p("src/enums.ts"),
                parent_name: "Status".to_string(),
                member_name: "Deprecated".to_string(),
                kind: MemberKind::EnumMember,
                line: 8,
                col: 0,
            })],
            unused_class_members: vec![UnusedClassMemberFinding::with_actions(UnusedMember {
                path: p("src/service.ts"),
                parent_name: "UserService".to_string(),
                member_name: "legacy".to_string(),
                kind: MemberKind::ClassMethod,
                line: 42,
                col: 0,
            })],
            unused_store_members: vec![UnusedStoreMemberFinding::with_actions(UnusedMember {
                path: p("src/store.ts"),
                parent_name: "useStore".to_string(),
                member_name: "legacyAction".to_string(),
                kind: MemberKind::StoreMember,
                line: 17,
                col: 0,
            })],
            unresolved_imports: vec![UnresolvedImportFinding::with_actions(UnresolvedImport {
                path: p("src/app.ts"),
                specifier: "./missing".to_string(),
                line: 3,
                col: 0,
                specifier_col: 0,
            })],
            duplicate_exports: vec![DuplicateExportFinding::with_actions(DuplicateExport {
                export_name: "Config".to_string(),
                locations: vec![
                    DuplicateLocation {
                        path: p("src/a.ts"),
                        line: 1,
                        col: 0,
                    },
                    DuplicateLocation {
                        path: p("src/b.ts"),
                        line: 5,
                        col: 0,
                    },
                ],
            })],
            boundary_violations: vec![BoundaryViolationFinding::with_actions(BoundaryViolation {
                from_path: p("src/ui/btn.ts"),
                to_path: p("src/db/query.ts"),
                from_zone: "ui".to_string(),
                to_zone: "db".to_string(),
                import_specifier: "../db/query".to_string(),
                line: 1,
                col: 0,
            })],
            ..Default::default()
        }
    }

    /// Regression test: baseline saved on one machine (different absolute root)
    /// must match issues found on another machine across all path-based types.
    #[test]
    fn baseline_keys_are_relative_to_root() {
        let local_root = Path::new("/Users/dev/project");
        let results = make_absolute_results("/Users/dev/project");
        let baseline = BaselineData::from_results(&results, local_root);

        assert_eq!(baseline.unused_files, vec!["src/old.ts"]);
        assert_eq!(baseline.unused_exports, vec!["src/utils.ts:helper"]);
        assert_eq!(
            baseline.unused_dependencies,
            vec!["packages/app/package.json:lodash-es"]
        );
        assert_eq!(
            baseline.boundary_violations,
            vec!["src/ui/btn.ts->src/db/query.ts"]
        );
        assert_eq!(baseline.circular_dependencies, vec!["src/a.ts->src/b.ts"]);
        assert_eq!(
            baseline.unused_enum_members,
            vec!["src/enums.ts:Status.Deprecated"]
        );
        assert_eq!(
            baseline.unused_class_members,
            vec!["src/service.ts:UserService.legacy"]
        );
        assert_eq!(
            baseline.unused_store_members,
            vec!["src/store.ts:useStore.legacyAction"]
        );
        assert_eq!(baseline.unresolved_imports, vec!["src/app.ts:./missing"]);
        assert_eq!(baseline.duplicate_exports, vec!["Config|src/a.ts|src/b.ts"]);

        let ci_root = Path::new("/home/runner/work/project/project");
        let ci_results = make_absolute_results("/home/runner/work/project/project");

        let filtered = filter_new_issues(ci_results, &baseline, ci_root);
        assert!(filtered.unused_files.is_empty(), "unused files");
        assert!(filtered.unused_exports.is_empty(), "unused exports");
        assert!(filtered.unused_dependencies.is_empty(), "unused deps");
        assert!(
            filtered.boundary_violations.is_empty(),
            "boundary violations"
        );
        assert!(filtered.circular_dependencies.is_empty(), "circular deps");
        assert!(filtered.unused_enum_members.is_empty(), "enum members");
        assert!(filtered.unused_class_members.is_empty(), "class members");
        assert!(filtered.unused_store_members.is_empty(), "store members");
        assert!(filtered.unresolved_imports.is_empty(), "unresolved imports");
        assert!(filtered.duplicate_exports.is_empty(), "duplicate exports");
    }

    #[test]
    fn stale_suppression_baseline_keys_include_missing_reason_state() {
        let root = Path::new("/project");
        let stale = crate::results::StaleSuppression {
            path: root.join("src/file.ts"),
            line: 1,
            col: 0,
            origin: crate::results::SuppressionOrigin::Comment {
                issue_kind: Some("unused-export".to_string()),
                reason: None,
                is_file_level: false,
                kind_known: true,
            },
            missing_reason: false,
            actions: crate::results::StaleSuppression::actions_for(false),
        };
        let missing = crate::results::StaleSuppression {
            missing_reason: true,
            actions: crate::results::StaleSuppression::actions_for(true),
            ..stale.clone()
        };
        let results = AnalysisResults {
            stale_suppressions: vec![stale, missing],
            ..Default::default()
        };
        let baseline = BaselineData::from_results(&results, root);

        assert_eq!(
            baseline.stale_suppressions,
            vec![
                "stale-suppression:src/file.ts:1",
                "missing-suppression-reason:src/file.ts:1",
            ]
        );

        let mut legacy_baseline = BaselineData::from_results(&AnalysisResults::default(), root);
        legacy_baseline.stale_suppressions = vec!["src/file.ts:1".to_string()];
        let filtered = filter_new_issues(results, &legacy_baseline, root);
        assert!(filtered.stale_suppressions.is_empty());
    }

    fn runtime_finding(
        id: &str,
        stable_id: Option<&str>,
        line: u32,
        source_hash: Option<&str>,
    ) -> fallow_output::RuntimeCoverageFinding {
        fallow_output::RuntimeCoverageFinding {
            id: id.to_owned(),
            stable_id: stable_id.map(str::to_owned),
            source_hash: source_hash.map(str::to_owned),
            path: PathBuf::from("src/a.ts"),
            function: "alpha".to_owned(),
            line,
            verdict: fallow_output::RuntimeCoverageVerdict::ReviewRequired,
            invocations: Some(0),
            confidence: fallow_output::RuntimeCoverageConfidence::Medium,
            evidence: fallow_output::RuntimeCoverageEvidence {
                static_status: "used".to_owned(),
                test_coverage: "not_covered".to_owned(),
                v8_tracking: "tracked".to_owned(),
                untracked_reason: None,
                observation_days: 1,
                deployments_observed: 1,
            },
            actions: vec![],
            discriminators: None,
        }
    }

    #[test]
    fn legacy_prod_baseline_still_suppresses_finding() {
        let baseline = HealthBaselineData {
            runtime_coverage_findings: vec!["fallow:prod:deadbeef".to_owned()],
            ..HealthBaselineData::default()
        };
        let findings = vec![runtime_finding(
            "fallow:prod:deadbeef",
            Some("fallow:fn:00000001"),
            14,
            None,
        )];
        let filtered =
            filter_new_runtime_coverage_findings(findings, &baseline, Path::new("/repo"));
        assert!(filtered.is_empty(), "legacy prod id must still suppress");
    }

    #[test]
    fn source_hash_baseline_survives_line_move() {
        let root = Path::new("/repo");
        let baselined = runtime_finding(
            "fallow:prod:deadbeef",
            Some("fallow:fn:00000001"),
            14,
            Some("0123456789abcdef"),
        );
        let baseline = HealthBaselineData::from_findings(&[], &[baselined], &[], root);
        assert_eq!(baseline.runtime_coverage_source_hashes.len(), 1);

        let findings = vec![runtime_finding(
            "fallow:prod:99999999",
            Some("fallow:fn:cafe0002"),
            40,
            Some("0123456789abcdef"),
        )];
        let filtered = filter_new_runtime_coverage_findings(findings, &baseline, root);
        assert!(
            filtered.is_empty(),
            "source_hash baseline must survive a line move despite a changed stable_id and id"
        );
    }

    #[test]
    fn unbaselined_finding_is_reported() {
        let baseline = HealthBaselineData {
            runtime_coverage_findings: vec!["fallow:fn:00000001".to_owned()],
            ..HealthBaselineData::default()
        };
        let findings = vec![runtime_finding(
            "fallow:prod:abc1234d",
            Some("fallow:fn:beefcafe"),
            7,
            None,
        )];
        let filtered =
            filter_new_runtime_coverage_findings(findings, &baseline, Path::new("/repo"));
        assert_eq!(filtered.len(), 1, "a brand-new finding must be reported");
    }
}
