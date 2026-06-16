use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};
use std::path::Path;

use rustc_hash::FxHashSet;

fn relative_key_path(path: &Path, root: &Path) -> String {
    let simple_path = dunce::simplified(path);
    let simple_root = dunce::simplified(root);
    simple_path
        .strip_prefix(simple_root)
        .unwrap_or(simple_path)
        .to_string_lossy()
        .replace('\\', "/")
}

fn dependency_location_key(location: &fallow_core::results::DependencyLocation) -> &'static str {
    match location {
        fallow_core::results::DependencyLocation::Dependencies => "unused-dependency",
        fallow_core::results::DependencyLocation::DevDependencies => "unused-dev-dependency",
        fallow_core::results::DependencyLocation::OptionalDependencies => {
            "unused-optional-dependency"
        }
    }
}

fn unused_dependency_key(item: &fallow_core::results::UnusedDependency, root: &Path) -> String {
    format!(
        "{}:{}:{}",
        dependency_location_key(&item.location),
        relative_key_path(&item.path, root),
        item.package_name
    )
}

fn invalid_client_export_key(
    item: &fallow_core::results::InvalidClientExport,
    root: &Path,
) -> String {
    format!(
        "invalid-client-export:{}:{}",
        relative_key_path(&item.path, root),
        item.export_name
    )
}

fn mixed_client_server_barrel_key(
    item: &fallow_core::results::MixedClientServerBarrel,
    root: &Path,
) -> String {
    format!(
        "mixed-client-server-barrel:{}:{}:{}",
        relative_key_path(&item.path, root),
        item.client_origin,
        item.server_origin
    )
}

fn misplaced_directive_key(item: &fallow_core::results::MisplacedDirective, root: &Path) -> String {
    format!(
        "misplaced-directive:{}:{}:{}",
        relative_key_path(&item.path, root),
        item.line,
        item.directive
    )
}

fn unprovided_inject_key(item: &fallow_core::results::UnprovidedInject, root: &Path) -> String {
    format!(
        "unprovided-inject:{}:{}",
        relative_key_path(&item.path, root),
        item.key_name
    )
}

fn unrendered_component_key(
    item: &fallow_core::results::UnrenderedComponent,
    root: &Path,
) -> String {
    format!(
        "unrendered-component:{}:{}",
        relative_key_path(&item.path, root),
        item.component_name
    )
}

fn unused_component_prop_key(
    item: &fallow_core::results::UnusedComponentProp,
    root: &Path,
) -> String {
    format!(
        "unused-component-prop:{}:{}",
        relative_key_path(&item.path, root),
        item.prop_name
    )
}

fn unused_component_emit_key(
    item: &fallow_core::results::UnusedComponentEmit,
    root: &Path,
) -> String {
    format!(
        "unused-component-emit:{}:{}",
        relative_key_path(&item.path, root),
        item.emit_name
    )
}

fn unused_component_input_key(
    item: &fallow_core::results::UnusedComponentInput,
    root: &Path,
) -> String {
    format!(
        "unused-component-input:{}:{}",
        relative_key_path(&item.path, root),
        item.input_name
    )
}

fn unused_component_output_key(
    item: &fallow_core::results::UnusedComponentOutput,
    root: &Path,
) -> String {
    format!(
        "unused-component-output:{}:{}",
        relative_key_path(&item.path, root),
        item.output_name
    )
}

fn unused_server_action_key(
    item: &fallow_core::results::UnusedServerAction,
    root: &Path,
) -> String {
    format!(
        "unused-server-action:{}:{}",
        relative_key_path(&item.path, root),
        item.action_name
    )
}

fn unused_load_data_key_key(item: &fallow_core::results::UnusedLoadDataKey, root: &Path) -> String {
    format!(
        "unused-load-data-key:{}:{}",
        relative_key_path(&item.path, root),
        item.key_name
    )
}

fn route_collision_key(item: &fallow_core::results::RouteCollision, root: &Path) -> String {
    format!(
        "route-collision:{}:{}",
        relative_key_path(&item.path, root),
        item.url
    )
}

fn dynamic_segment_name_conflict_key(
    item: &fallow_core::results::DynamicSegmentNameConflict,
    root: &Path,
) -> String {
    format!(
        "dynamic-segment-name-conflict:{}:{}",
        relative_key_path(&item.path, root),
        item.position
    )
}

fn unlisted_dependency_key(item: &fallow_core::results::UnlistedDependency, root: &Path) -> String {
    let mut sites = item
        .imported_from
        .iter()
        .map(|site| {
            format!(
                "{}:{}:{}",
                relative_key_path(&site.path, root),
                site.line,
                site.col
            )
        })
        .collect::<Vec<_>>();
    sites.sort();
    sites.dedup();
    format!(
        "unlisted-dependency:{}:{}",
        item.package_name,
        sites.join("|")
    )
}

fn unused_member_key(
    rule_id: &str,
    item: &fallow_core::results::UnusedMember,
    root: &Path,
) -> String {
    format!(
        "{}:{}:{}:{}",
        rule_id,
        relative_key_path(&item.path, root),
        item.parent_name,
        item.member_name
    )
}

fn unused_catalog_entry_key(
    item: &fallow_core::results::UnusedCatalogEntry,
    root: &Path,
) -> String {
    format!(
        "unused-catalog-entry:{}:{}:{}:{}",
        relative_key_path(&item.path, root),
        item.line,
        item.catalog_name,
        item.entry_name
    )
}

fn empty_catalog_group_key(item: &fallow_core::results::EmptyCatalogGroup, root: &Path) -> String {
    format!(
        "empty-catalog-group:{}:{}:{}",
        relative_key_path(&item.path, root),
        item.line,
        item.catalog_name
    )
}

fn sorted_relative_path_keys<'a>(
    paths: impl Iterator<Item = &'a Path>,
    root: &Path,
) -> Vec<String> {
    let mut keys = paths
        .map(|path| relative_key_path(path, root))
        .collect::<Vec<_>>();
    keys.sort();
    keys
}

fn duplicate_export_key(
    item: &fallow_core::results::DuplicateExportFinding,
    root: &Path,
) -> String {
    let mut locations = sorted_relative_path_keys(
        item.export.locations.iter().map(|loc| loc.path.as_path()),
        root,
    );
    locations.dedup();
    format!(
        "duplicate-export:{}:{}",
        item.export.export_name,
        locations.join("|")
    )
}

fn circular_dependency_key(
    item: &fallow_core::results::CircularDependencyFinding,
    root: &Path,
) -> String {
    let files = sorted_relative_path_keys(
        item.cycle.files.iter().map(std::path::PathBuf::as_path),
        root,
    );
    format!("circular-dependency:{}", files.join("|"))
}

fn re_export_cycle_key(item: &fallow_core::results::ReExportCycleFinding, root: &Path) -> String {
    let kind = match item.cycle.kind {
        fallow_core::results::ReExportCycleKind::MultiNode => "multi-node",
        fallow_core::results::ReExportCycleKind::SelfLoop => "self-loop",
    };
    let files = sorted_relative_path_keys(
        item.cycle.files.iter().map(std::path::PathBuf::as_path),
        root,
    );
    format!("re-export-cycle:{kind}:{}", files.join("|"))
}

fn boundary_violation_key(
    item: &fallow_core::results::BoundaryViolationFinding,
    root: &Path,
) -> String {
    format!(
        "boundary-violation:{}:{}:{}",
        relative_key_path(&item.violation.from_path, root),
        relative_key_path(&item.violation.to_path, root),
        item.violation.import_specifier
    )
}

fn boundary_coverage_key(
    item: &fallow_core::results::BoundaryCoverageViolationFinding,
    root: &Path,
) -> String {
    format!(
        "boundary-coverage:{}",
        relative_key_path(&item.violation.path, root)
    )
}

fn boundary_call_key(
    item: &fallow_core::results::BoundaryCallViolationFinding,
    root: &Path,
) -> String {
    format!(
        "boundary-call:{}:{}",
        relative_key_path(&item.violation.path, root),
        item.violation.callee
    )
}

fn policy_violation_key(
    item: &fallow_core::results::PolicyViolationFinding,
    root: &Path,
) -> String {
    format!(
        "policy-violation:{}:{}/{}:{}",
        relative_key_path(&item.violation.path, root),
        item.violation.pack,
        item.violation.rule_id,
        item.violation.matched
    )
}

fn stale_suppression_key(item: &fallow_core::results::StaleSuppression, root: &Path) -> String {
    format!(
        "stale-suppression:{}:{}",
        relative_key_path(&item.path, root),
        item.description()
    )
}

fn unresolved_catalog_reference_key(
    item: &fallow_core::results::UnresolvedCatalogReferenceFinding,
    root: &Path,
) -> String {
    format!(
        "unresolved-catalog-reference:{}:{}:{}:{}",
        relative_key_path(&item.reference.path, root),
        item.reference.line,
        item.reference.catalog_name,
        item.reference.entry_name
    )
}

fn unused_dependency_override_key(
    item: &fallow_core::results::UnusedDependencyOverrideFinding,
    root: &Path,
) -> String {
    format!(
        "unused-dependency-override:{}:{}:{}",
        relative_key_path(&item.entry.path, root),
        item.entry.line,
        item.entry.raw_key
    )
}

fn misconfigured_dependency_override_key(
    item: &fallow_core::results::MisconfiguredDependencyOverrideFinding,
    root: &Path,
) -> String {
    format!(
        "misconfigured-dependency-override:{}:{}:{}",
        relative_key_path(&item.entry.path, root),
        item.entry.line,
        item.entry.raw_key
    )
}

/// Build the set of audit attribution keys for all dead-code findings in
/// `results`.
///
/// Each key is a stable string that uniquely identifies one finding across
/// runs (e.g. `unused-file:src/dead.ts`, `unused-export:src/a.ts:Foo`).
/// `retain_introduced_dead_code` and `annotate_dead_code_json` use the same
/// key format to diff the current run against a base snapshot.
///
/// This destructure is deliberately exhaustive: adding a field to
/// `AnalysisResults` must fail compilation here so the author decides
/// explicitly whether the new finding type needs an audit key (add a loop)
/// or has no key representation today (bind with underscore and document why).
///
/// Sibling exhaustive sites: `fallow_core::changed_files::filter_results_by_changed_files`,
/// `dead_code_keys`, `retain_introduced_dead_code`.
/// Non-exhaustive siblings the compiler will NOT flag (wire manually when a
/// finding type is added): `annotate_dead_code_json` (same key formats, this
/// file) and the per-collection severity branches in
/// `crates/cli/src/check/rules.rs` (`apply_rules`, `has_error_severity_issues`).
/// TypeScript mirror: `editors/vscode/scripts/codegen-types.mjs` (`BARE_DEAD_CODE_ALIASES`).
pub(super) fn dead_code_keys(
    results: &fallow_core::results::AnalysisResults,
    root: &Path,
) -> FxHashSet<String> {
    let fallow_core::results::AnalysisResults {
        unused_files,
        unused_exports,
        unused_types,
        private_type_leaks,
        unused_dependencies,
        unused_dev_dependencies,
        unused_optional_dependencies,
        unused_enum_members,
        unused_class_members,
        unused_store_members,
        unresolved_imports,
        unlisted_dependencies,
        duplicate_exports,
        type_only_dependencies,
        test_only_dependencies,
        circular_dependencies,
        re_export_cycles,
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
        unused_component_emits,
        unused_component_inputs,
        unused_component_outputs,
        unused_server_actions,
        unused_load_data_keys,
        unused_load_data_keys_global_abstain: _unused_load_data_keys_global_abstain,
        route_collisions,
        dynamic_segment_name_conflicts,
        // Non-finding fields: counts and metadata, not attributable to a key.
        suppression_count: _suppression_count,
        active_suppressions: _active_suppressions,
        feature_flags: _feature_flags,
        // Security findings are emitted via `fallow security`, not the audit
        // dead-code gate; they have no dead-code key representation today.
        security_findings: _security_findings,
        security_unresolved_edge_files: _security_unresolved_edge_files,
        security_unresolved_callee_sites: _security_unresolved_callee_sites,
        security_unresolved_callee_diagnostics: _security_unresolved_callee_diagnostics,
        // Prop-drilling is a dormant multi-file health signal (rule defaults to
        // `off`); like security findings it has no dead-code attribution key.
        prop_drilling_chains: _prop_drilling_chains,
        // Thin wrappers are a dormant health signal (rule defaults to `off`); a
        // candidate-for-inlining record, not a dead-code attribution key.
        thin_wrappers: _thin_wrappers,
        // Duplicate prop shapes are a dormant multi-file health signal (rule
        // defaults to `off`); a missing-abstraction record, not a dead-code
        // attribution key.
        duplicate_prop_shapes: _duplicate_prop_shapes,
        // Export usages and entry-point summary are metadata, not issue
        // collections; no key needed.
        export_usages: _export_usages,
        entry_point_summary: _entry_point_summary,
        // Render fan-in is a whole-project descriptive metric, not an issue
        // collection; no attribution key needed.
        render_fan_in: _render_fan_in,
    } = results;

    let mut collector = DeadCodeKeyCollector::new(root);
    collector.add_core_findings(
        unused_files,
        unused_exports,
        unused_types,
        private_type_leaks,
    );
    collector.add_client_directive_findings(
        invalid_client_exports,
        mixed_client_server_barrels,
        misplaced_directives,
    );
    collector.add_dependency_findings(
        unused_dependencies,
        unused_dev_dependencies,
        unused_optional_dependencies,
        unlisted_dependencies,
        type_only_dependencies,
        test_only_dependencies,
    );
    collector.add_dependency_override_findings(
        unused_dependency_overrides,
        misconfigured_dependency_overrides,
    );
    collector.add_member_findings(
        unused_enum_members,
        unused_class_members,
        unused_store_members,
    );
    collector.add_component_contract_findings(
        unused_component_props,
        unused_component_emits,
        unused_component_inputs,
        unused_component_outputs,
    );
    collector.add_graph_findings(
        unresolved_imports,
        duplicate_exports,
        circular_dependencies,
        re_export_cycles,
    );
    collector.add_boundary_findings(
        boundary_violations,
        boundary_coverage_violations,
        boundary_call_violations,
        policy_violations,
        stale_suppressions,
    );
    collector.add_catalog_findings(
        unresolved_catalog_references,
        unused_catalog_entries,
        empty_catalog_groups,
    );
    collector.add_framework_findings(
        unprovided_injects,
        unrendered_components,
        unused_server_actions,
        unused_load_data_keys,
        route_collisions,
        dynamic_segment_name_conflicts,
    );
    collector.into_keys()
}

struct DeadCodeKeyCollector<'a> {
    root: &'a Path,
    keys: FxHashSet<String>,
}

impl<'a> DeadCodeKeyCollector<'a> {
    fn new(root: &'a Path) -> Self {
        Self {
            root,
            keys: FxHashSet::default(),
        }
    }

    fn into_keys(self) -> FxHashSet<String> {
        self.keys
    }

    fn insert(&mut self, key: String) {
        self.keys.insert(key);
    }

    fn add_core_findings(
        &mut self,
        unused_files: &[fallow_core::results::UnusedFileFinding],
        unused_exports: &[fallow_core::results::UnusedExportFinding],
        unused_types: &[fallow_core::results::UnusedTypeFinding],
        private_type_leaks: &[fallow_core::results::PrivateTypeLeakFinding],
    ) {
        self.add_unused_files(unused_files);
        self.add_unused_exports(unused_exports);
        self.add_unused_types(unused_types);
        self.add_private_type_leaks(private_type_leaks);
    }

    fn add_client_directive_findings(
        &mut self,
        invalid_client_exports: &[fallow_core::results::InvalidClientExportFinding],
        mixed_client_server_barrels: &[fallow_core::results::MixedClientServerBarrelFinding],
        misplaced_directives: &[fallow_core::results::MisplacedDirectiveFinding],
    ) {
        self.add_invalid_client_exports(invalid_client_exports);
        self.add_mixed_client_server_barrels(mixed_client_server_barrels);
        self.add_misplaced_directives(misplaced_directives);
    }

    fn add_dependency_findings(
        &mut self,
        unused_dependencies: &[fallow_core::results::UnusedDependencyFinding],
        unused_dev_dependencies: &[fallow_core::results::UnusedDevDependencyFinding],
        unused_optional_dependencies: &[fallow_core::results::UnusedOptionalDependencyFinding],
        unlisted_dependencies: &[fallow_core::results::UnlistedDependencyFinding],
        type_only_dependencies: &[fallow_core::results::TypeOnlyDependencyFinding],
        test_only_dependencies: &[fallow_core::results::TestOnlyDependencyFinding],
    ) {
        self.add_unused_dependencies(unused_dependencies);
        self.add_unused_dev_dependencies(unused_dev_dependencies);
        self.add_unused_optional_dependencies(unused_optional_dependencies);
        self.add_unlisted_dependencies(unlisted_dependencies);
        self.add_type_only_dependencies(type_only_dependencies);
        self.add_test_only_dependencies(test_only_dependencies);
    }

    fn add_dependency_override_findings(
        &mut self,
        unused_dependency_overrides: &[fallow_core::results::UnusedDependencyOverrideFinding],
        misconfigured_dependency_overrides: &[fallow_core::results::MisconfiguredDependencyOverrideFinding],
    ) {
        self.add_unused_dependency_overrides(unused_dependency_overrides);
        self.add_misconfigured_dependency_overrides(misconfigured_dependency_overrides);
    }

    fn add_member_findings(
        &mut self,
        unused_enum_members: &[fallow_core::results::UnusedEnumMemberFinding],
        unused_class_members: &[fallow_core::results::UnusedClassMemberFinding],
        unused_store_members: &[fallow_core::results::UnusedStoreMemberFinding],
    ) {
        self.add_unused_enum_members(unused_enum_members);
        self.add_unused_class_members(unused_class_members);
        self.add_unused_store_members(unused_store_members);
    }

    fn add_component_contract_findings(
        &mut self,
        unused_component_props: &[fallow_core::results::UnusedComponentPropFinding],
        unused_component_emits: &[fallow_core::results::UnusedComponentEmitFinding],
        unused_component_inputs: &[fallow_core::results::UnusedComponentInputFinding],
        unused_component_outputs: &[fallow_core::results::UnusedComponentOutputFinding],
    ) {
        self.add_unused_component_props(unused_component_props);
        self.add_unused_component_emits(unused_component_emits);
        self.add_unused_component_inputs(unused_component_inputs);
        self.add_unused_component_outputs(unused_component_outputs);
    }

    fn add_graph_findings(
        &mut self,
        unresolved_imports: &[fallow_core::results::UnresolvedImportFinding],
        duplicate_exports: &[fallow_core::results::DuplicateExportFinding],
        circular_dependencies: &[fallow_core::results::CircularDependencyFinding],
        re_export_cycles: &[fallow_core::results::ReExportCycleFinding],
    ) {
        self.add_unresolved_imports(unresolved_imports);
        self.add_duplicate_exports(duplicate_exports);
        self.add_circular_dependencies(circular_dependencies);
        self.add_re_export_cycles(re_export_cycles);
    }

    fn add_boundary_findings(
        &mut self,
        boundary_violations: &[fallow_core::results::BoundaryViolationFinding],
        boundary_coverage_violations: &[fallow_core::results::BoundaryCoverageViolationFinding],
        boundary_call_violations: &[fallow_core::results::BoundaryCallViolationFinding],
        policy_violations: &[fallow_core::results::PolicyViolationFinding],
        stale_suppressions: &[fallow_core::results::StaleSuppression],
    ) {
        self.add_boundary_violations(boundary_violations);
        self.add_boundary_coverage_violations(boundary_coverage_violations);
        self.add_boundary_call_violations(boundary_call_violations);
        self.add_policy_violations(policy_violations);
        self.add_stale_suppressions(stale_suppressions);
    }

    fn add_catalog_findings(
        &mut self,
        unresolved_catalog_references: &[fallow_core::results::UnresolvedCatalogReferenceFinding],
        unused_catalog_entries: &[fallow_core::results::UnusedCatalogEntryFinding],
        empty_catalog_groups: &[fallow_core::results::EmptyCatalogGroupFinding],
    ) {
        self.add_unresolved_catalog_references(unresolved_catalog_references);
        self.add_unused_catalog_entries(unused_catalog_entries);
        self.add_empty_catalog_groups(empty_catalog_groups);
    }

    fn add_framework_findings(
        &mut self,
        unprovided_injects: &[fallow_core::results::UnprovidedInjectFinding],
        unrendered_components: &[fallow_core::results::UnrenderedComponentFinding],
        unused_server_actions: &[fallow_core::results::UnusedServerActionFinding],
        unused_load_data_keys: &[fallow_core::results::UnusedLoadDataKeyFinding],
        route_collisions: &[fallow_core::results::RouteCollisionFinding],
        dynamic_segment_name_conflicts: &[fallow_core::results::DynamicSegmentNameConflictFinding],
    ) {
        self.add_unprovided_injects(unprovided_injects);
        self.add_unrendered_components(unrendered_components);
        self.add_unused_server_actions(unused_server_actions);
        self.add_unused_load_data_keys(unused_load_data_keys);
        self.add_route_collisions(route_collisions);
        self.add_dynamic_segment_name_conflicts(dynamic_segment_name_conflicts);
    }

    fn add_unused_files(&mut self, items: &[fallow_core::results::UnusedFileFinding]) {
        for item in items {
            self.insert(format!(
                "unused-file:{}",
                relative_key_path(&item.file.path, self.root)
            ));
        }
    }

    fn add_unused_exports(&mut self, items: &[fallow_core::results::UnusedExportFinding]) {
        for item in items {
            self.insert(format!(
                "unused-export:{}:{}",
                relative_key_path(&item.export.path, self.root),
                item.export.export_name
            ));
        }
    }

    fn add_unused_types(&mut self, items: &[fallow_core::results::UnusedTypeFinding]) {
        for item in items {
            self.insert(format!(
                "unused-type:{}:{}",
                relative_key_path(&item.export.path, self.root),
                item.export.export_name
            ));
        }
    }

    fn add_private_type_leaks(&mut self, items: &[fallow_core::results::PrivateTypeLeakFinding]) {
        for item in items {
            self.insert(format!(
                "private-type-leak:{}:{}:{}",
                relative_key_path(&item.leak.path, self.root),
                item.leak.export_name,
                item.leak.type_name
            ));
        }
    }

    fn add_invalid_client_exports(
        &mut self,
        items: &[fallow_core::results::InvalidClientExportFinding],
    ) {
        for item in items {
            self.insert(invalid_client_export_key(&item.export, self.root));
        }
    }

    fn add_mixed_client_server_barrels(
        &mut self,
        items: &[fallow_core::results::MixedClientServerBarrelFinding],
    ) {
        for item in items {
            self.insert(mixed_client_server_barrel_key(&item.barrel, self.root));
        }
    }

    fn add_misplaced_directives(
        &mut self,
        items: &[fallow_core::results::MisplacedDirectiveFinding],
    ) {
        for item in items {
            self.insert(misplaced_directive_key(&item.directive_site, self.root));
        }
    }

    fn add_unprovided_injects(&mut self, items: &[fallow_core::results::UnprovidedInjectFinding]) {
        for item in items {
            self.insert(unprovided_inject_key(&item.inject, self.root));
        }
    }

    fn add_unrendered_components(
        &mut self,
        items: &[fallow_core::results::UnrenderedComponentFinding],
    ) {
        for item in items {
            self.insert(unrendered_component_key(&item.component, self.root));
        }
    }

    fn add_unused_component_props(
        &mut self,
        items: &[fallow_core::results::UnusedComponentPropFinding],
    ) {
        for item in items {
            self.insert(unused_component_prop_key(&item.prop, self.root));
        }
    }

    fn add_unused_component_emits(
        &mut self,
        items: &[fallow_core::results::UnusedComponentEmitFinding],
    ) {
        for item in items {
            self.insert(unused_component_emit_key(&item.emit, self.root));
        }
    }

    fn add_unused_component_inputs(
        &mut self,
        items: &[fallow_core::results::UnusedComponentInputFinding],
    ) {
        for item in items {
            self.insert(unused_component_input_key(&item.input, self.root));
        }
    }

    fn add_unused_component_outputs(
        &mut self,
        items: &[fallow_core::results::UnusedComponentOutputFinding],
    ) {
        for item in items {
            self.insert(unused_component_output_key(&item.output, self.root));
        }
    }

    fn add_unused_server_actions(
        &mut self,
        items: &[fallow_core::results::UnusedServerActionFinding],
    ) {
        for item in items {
            self.insert(unused_server_action_key(&item.action, self.root));
        }
    }

    fn add_unused_load_data_keys(
        &mut self,
        items: &[fallow_core::results::UnusedLoadDataKeyFinding],
    ) {
        for item in items {
            self.insert(unused_load_data_key_key(&item.key, self.root));
        }
    }

    fn add_route_collisions(&mut self, items: &[fallow_core::results::RouteCollisionFinding]) {
        for item in items {
            self.insert(route_collision_key(&item.collision, self.root));
        }
    }

    fn add_dynamic_segment_name_conflicts(
        &mut self,
        items: &[fallow_core::results::DynamicSegmentNameConflictFinding],
    ) {
        for item in items {
            self.insert(dynamic_segment_name_conflict_key(&item.conflict, self.root));
        }
    }

    fn add_unused_dependencies(&mut self, items: &[fallow_core::results::UnusedDependencyFinding]) {
        for item in items {
            self.insert(unused_dependency_key(&item.dep, self.root));
        }
    }

    fn add_unused_dev_dependencies(
        &mut self,
        items: &[fallow_core::results::UnusedDevDependencyFinding],
    ) {
        for item in items {
            self.insert(unused_dependency_key(&item.dep, self.root));
        }
    }

    fn add_unused_optional_dependencies(
        &mut self,
        items: &[fallow_core::results::UnusedOptionalDependencyFinding],
    ) {
        for item in items {
            self.insert(unused_dependency_key(&item.dep, self.root));
        }
    }

    fn add_unused_enum_members(&mut self, items: &[fallow_core::results::UnusedEnumMemberFinding]) {
        for item in items {
            self.insert(unused_member_key(
                "unused-enum-member",
                &item.member,
                self.root,
            ));
        }
    }

    fn add_unused_class_members(
        &mut self,
        items: &[fallow_core::results::UnusedClassMemberFinding],
    ) {
        for item in items {
            self.insert(unused_member_key(
                "unused-class-member",
                &item.member,
                self.root,
            ));
        }
    }

    fn add_unused_store_members(
        &mut self,
        items: &[fallow_core::results::UnusedStoreMemberFinding],
    ) {
        for item in items {
            self.insert(unused_member_key(
                "unused-store-member",
                &item.member,
                self.root,
            ));
        }
    }

    fn add_unresolved_imports(&mut self, items: &[fallow_core::results::UnresolvedImportFinding]) {
        for item in items {
            self.insert(format!(
                "unresolved-import:{}:{}",
                relative_key_path(&item.import.path, self.root),
                item.import.specifier
            ));
        }
    }

    fn add_unlisted_dependencies(
        &mut self,
        items: &[fallow_core::results::UnlistedDependencyFinding],
    ) {
        for item in items {
            self.insert(unlisted_dependency_key(&item.dep, self.root));
        }
    }

    fn add_duplicate_exports(&mut self, items: &[fallow_core::results::DuplicateExportFinding]) {
        for item in items {
            self.insert(duplicate_export_key(item, self.root));
        }
    }

    fn add_type_only_dependencies(
        &mut self,
        items: &[fallow_core::results::TypeOnlyDependencyFinding],
    ) {
        for item in items {
            self.insert(format!(
                "type-only-dependency:{}:{}",
                relative_key_path(&item.dep.path, self.root),
                item.dep.package_name
            ));
        }
    }

    fn add_test_only_dependencies(
        &mut self,
        items: &[fallow_core::results::TestOnlyDependencyFinding],
    ) {
        for item in items {
            self.insert(format!(
                "test-only-dependency:{}:{}",
                relative_key_path(&item.dep.path, self.root),
                item.dep.package_name
            ));
        }
    }

    fn add_circular_dependencies(
        &mut self,
        items: &[fallow_core::results::CircularDependencyFinding],
    ) {
        for item in items {
            self.insert(circular_dependency_key(item, self.root));
        }
    }

    fn add_re_export_cycles(&mut self, items: &[fallow_core::results::ReExportCycleFinding]) {
        for item in items {
            self.insert(re_export_cycle_key(item, self.root));
        }
    }

    fn add_boundary_violations(
        &mut self,
        items: &[fallow_core::results::BoundaryViolationFinding],
    ) {
        for item in items {
            self.insert(boundary_violation_key(item, self.root));
        }
    }

    fn add_boundary_coverage_violations(
        &mut self,
        items: &[fallow_core::results::BoundaryCoverageViolationFinding],
    ) {
        for item in items {
            self.insert(boundary_coverage_key(item, self.root));
        }
    }

    fn add_boundary_call_violations(
        &mut self,
        items: &[fallow_core::results::BoundaryCallViolationFinding],
    ) {
        for item in items {
            self.insert(boundary_call_key(item, self.root));
        }
    }

    fn add_policy_violations(&mut self, items: &[fallow_core::results::PolicyViolationFinding]) {
        for item in items {
            self.insert(policy_violation_key(item, self.root));
        }
    }

    fn add_stale_suppressions(&mut self, items: &[fallow_core::results::StaleSuppression]) {
        for item in items {
            self.insert(stale_suppression_key(item, self.root));
        }
    }

    fn add_unresolved_catalog_references(
        &mut self,
        items: &[fallow_core::results::UnresolvedCatalogReferenceFinding],
    ) {
        for item in items {
            self.insert(unresolved_catalog_reference_key(item, self.root));
        }
    }

    fn add_unused_catalog_entries(
        &mut self,
        items: &[fallow_core::results::UnusedCatalogEntryFinding],
    ) {
        for item in items {
            self.insert(unused_catalog_entry_key(&item.entry, self.root));
        }
    }

    fn add_empty_catalog_groups(
        &mut self,
        items: &[fallow_core::results::EmptyCatalogGroupFinding],
    ) {
        for item in items {
            self.insert(empty_catalog_group_key(&item.group, self.root));
        }
    }

    fn add_unused_dependency_overrides(
        &mut self,
        items: &[fallow_core::results::UnusedDependencyOverrideFinding],
    ) {
        for item in items {
            self.insert(unused_dependency_override_key(item, self.root));
        }
    }

    fn add_misconfigured_dependency_overrides(
        &mut self,
        items: &[fallow_core::results::MisconfiguredDependencyOverrideFinding],
    ) {
        for item in items {
            self.insert(misconfigured_dependency_override_key(item, self.root));
        }
    }
}

/// Retain only findings whose audit key was NOT present in `base` (i.e. was
/// introduced on the current branch).
///
/// When `base` is `None` (no baseline), all findings are kept.
///
/// This destructure is deliberately exhaustive: adding a field to
/// `AnalysisResults` must fail compilation here so the author decides
/// explicitly whether the new finding type needs an introduced-retain (add a
/// retain block) or has no key representation today (bind with underscore and
/// document why).
///
/// Sibling exhaustive sites: `fallow_core::changed_files::filter_results_by_changed_files`,
/// `dead_code_keys`, `retain_introduced_dead_code`.
/// Non-exhaustive siblings the compiler will NOT flag (wire manually when a
/// finding type is added): `annotate_dead_code_json` (same key formats, this
/// file) and the per-collection severity branches in
/// `crates/cli/src/check/rules.rs` (`apply_rules`, `has_error_severity_issues`).
/// TypeScript mirror: `editors/vscode/scripts/codegen-types.mjs` (`BARE_DEAD_CODE_ALIASES`).
pub(super) fn retain_introduced_dead_code(
    results: &mut fallow_core::results::AnalysisResults,
    root: &Path,
    base: Option<&FxHashSet<String>>,
) {
    let Some(base) = base else {
        return;
    };

    // Compute the introduced set before taking any mutable borrows. Note the
    // order differs from the pre-destructure code, which narrowed
    // unused_files/exports/types first and computed keys from the narrowed
    // results. Computing from the un-narrowed results is equivalent: those
    // retains keep exactly the items whose key is NOT in `base`, and the
    // `!base.contains(key)` filter below removes the same base-member keys
    // from the full key set, so `introduced` is identical either way.
    let introduced = introduced_dead_code_keys(results, root, base);
    let keep = |key: String| introduced.contains(&key);

    let fallow_core::results::AnalysisResults {
        unused_files,
        unused_exports,
        unused_types,
        private_type_leaks,
        unused_dependencies,
        unused_dev_dependencies,
        unused_optional_dependencies,
        unused_enum_members,
        unused_class_members,
        unused_store_members,
        unresolved_imports,
        unlisted_dependencies,
        duplicate_exports,
        type_only_dependencies,
        test_only_dependencies,
        circular_dependencies,
        re_export_cycles,
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
        unused_component_emits,
        unused_component_inputs,
        unused_component_outputs,
        unused_server_actions,
        unused_load_data_keys,
        unused_load_data_keys_global_abstain: _unused_load_data_keys_global_abstain,
        route_collisions,
        dynamic_segment_name_conflicts,
        // Non-finding fields: counts and metadata, not subject to base-keyed
        // filtering.
        suppression_count: _suppression_count,
        active_suppressions: _active_suppressions,
        feature_flags: _feature_flags,
        // Security findings are emitted via `fallow security`, not the audit
        // dead-code gate; they have no key representation and are not filtered
        // here.
        security_findings: _security_findings,
        security_unresolved_edge_files: _security_unresolved_edge_files,
        security_unresolved_callee_sites: _security_unresolved_callee_sites,
        security_unresolved_callee_diagnostics: _security_unresolved_callee_diagnostics,
        // Prop-drilling is a dormant multi-file health signal (rule defaults to
        // `off`); it carries no dead-code key and is not base-filtered here.
        prop_drilling_chains: _prop_drilling_chains,
        // Thin wrappers are a dormant health signal (rule defaults to `off`);
        // no dead-code key and not base-filtered here.
        thin_wrappers: _thin_wrappers,
        // Duplicate prop shapes are a dormant multi-file health signal (rule
        // defaults to `off`); no dead-code key and not base-filtered here.
        duplicate_prop_shapes: _duplicate_prop_shapes,
        // Export usages and entry-point summary are metadata, not issue
        // collections; no key needed.
        export_usages: _export_usages,
        entry_point_summary: _entry_point_summary,
        // Render fan-in is a whole-project descriptive metric, not an issue
        // collection; no key needed.
        render_fan_in: _render_fan_in,
    } = results;

    // The three "fast path" retains use a direct base-lookup rather than the
    // introduced set; both predicates are equivalent for these collections
    // (see the `introduced` comment above), so this preserves the original
    // behavior.
    retain_introduced_fast_paths(unused_files, unused_exports, unused_types, root, base);
    private_type_leaks.retain(|item| {
        keep(format!(
            "private-type-leak:{}:{}:{}",
            relative_key_path(&item.leak.path, root),
            item.leak.export_name,
            item.leak.type_name
        ))
    });
    unused_dependencies.retain(|item| keep(unused_dependency_key(&item.dep, root)));
    unused_dev_dependencies.retain(|item| keep(unused_dependency_key(&item.dep, root)));
    unused_optional_dependencies.retain(|item| keep(unused_dependency_key(&item.dep, root)));
    unused_enum_members
        .retain(|item| keep(unused_member_key("unused-enum-member", &item.member, root)));
    unused_class_members
        .retain(|item| keep(unused_member_key("unused-class-member", &item.member, root)));
    unused_store_members
        .retain(|item| keep(unused_member_key("unused-store-member", &item.member, root)));
    unresolved_imports.retain(|item| {
        keep(format!(
            "unresolved-import:{}:{}",
            relative_key_path(&item.import.path, root),
            item.import.specifier
        ))
    });
    unlisted_dependencies.retain(|item| keep(unlisted_dependency_key(&item.dep, root)));
    duplicate_exports.retain(|item| keep(duplicate_export_key(item, root)));
    type_only_dependencies.retain(|item| {
        keep(format!(
            "type-only-dependency:{}:{}",
            relative_key_path(&item.dep.path, root),
            item.dep.package_name
        ))
    });
    test_only_dependencies.retain(|item| {
        keep(format!(
            "test-only-dependency:{}:{}",
            relative_key_path(&item.dep.path, root),
            item.dep.package_name
        ))
    });
    circular_dependencies.retain(|item| keep(circular_dependency_key(item, root)));
    re_export_cycles.retain(|item| keep(re_export_cycle_key(item, root)));
    boundary_violations.retain(|item| keep(boundary_violation_key(item, root)));
    boundary_coverage_violations.retain(|item| keep(boundary_coverage_key(item, root)));
    boundary_call_violations.retain(|item| keep(boundary_call_key(item, root)));
    policy_violations.retain(|item| keep(policy_violation_key(item, root)));
    stale_suppressions.retain(|item| keep(stale_suppression_key(item, root)));
    unresolved_catalog_references.retain(|item| keep(unresolved_catalog_reference_key(item, root)));
    unused_catalog_entries.retain(|item| keep(unused_catalog_entry_key(&item.entry, root)));
    empty_catalog_groups.retain(|item| keep(empty_catalog_group_key(&item.group, root)));
    unused_dependency_overrides.retain(|item| keep(unused_dependency_override_key(item, root)));
    misconfigured_dependency_overrides
        .retain(|item| keep(misconfigured_dependency_override_key(item, root)));
    invalid_client_exports.retain(|item| keep(invalid_client_export_key(&item.export, root)));
    mixed_client_server_barrels
        .retain(|item| keep(mixed_client_server_barrel_key(&item.barrel, root)));
    misplaced_directives.retain(|item| keep(misplaced_directive_key(&item.directive_site, root)));
    unprovided_injects.retain(|item| keep(unprovided_inject_key(&item.inject, root)));
    unrendered_components.retain(|item| keep(unrendered_component_key(&item.component, root)));
    unused_component_props.retain(|item| keep(unused_component_prop_key(&item.prop, root)));
    unused_component_emits.retain(|item| keep(unused_component_emit_key(&item.emit, root)));
    unused_component_inputs.retain(|item| keep(unused_component_input_key(&item.input, root)));
    unused_component_outputs.retain(|item| keep(unused_component_output_key(&item.output, root)));
    unused_server_actions.retain(|item| keep(unused_server_action_key(&item.action, root)));
    unused_load_data_keys.retain(|item| keep(unused_load_data_key_key(&item.key, root)));
    route_collisions.retain(|item| keep(route_collision_key(&item.collision, root)));
    dynamic_segment_name_conflicts
        .retain(|item| keep(dynamic_segment_name_conflict_key(&item.conflict, root)));
}

fn introduced_dead_code_keys(
    results: &fallow_core::results::AnalysisResults,
    root: &Path,
    base: &FxHashSet<String>,
) -> FxHashSet<String> {
    dead_code_keys(results, root)
        .into_iter()
        .filter(|key| !base.contains(key))
        .collect()
}

fn retain_introduced_fast_paths(
    unused_files: &mut Vec<fallow_core::results::UnusedFileFinding>,
    unused_exports: &mut Vec<fallow_core::results::UnusedExportFinding>,
    unused_types: &mut Vec<fallow_core::results::UnusedTypeFinding>,
    root: &Path,
    base: &FxHashSet<String>,
) {
    unused_files.retain(|item| {
        !base.contains(&format!(
            "unused-file:{}",
            relative_key_path(&item.file.path, root)
        ))
    });
    unused_exports.retain(|item| {
        !base.contains(&format!(
            "unused-export:{}:{}",
            relative_key_path(&item.export.path, root),
            item.export.export_name
        ))
    });
    unused_types.retain(|item| {
        !base.contains(&format!(
            "unused-type:{}:{}",
            relative_key_path(&item.export.path, root),
            item.export.export_name
        ))
    });
}

fn issue_was_introduced(key: &str, base: &FxHashSet<String>) -> bool {
    !base.contains(key)
}

fn annotate_issue_array<I>(json: &mut serde_json::Value, key: &str, introduced: I)
where
    I: IntoIterator<Item = bool>,
{
    let Some(items) = json.get_mut(key).and_then(serde_json::Value::as_array_mut) else {
        return;
    };
    for (item, introduced) in items.iter_mut().zip(introduced) {
        if let serde_json::Value::Object(map) = item {
            map.insert("introduced".to_string(), serde_json::json!(introduced));
        }
    }
}

pub(super) fn annotate_dead_code_json(
    json: &mut serde_json::Value,
    results: &fallow_core::results::AnalysisResults,
    root: &Path,
    base: &FxHashSet<String>,
) {
    let mut annotator = DeadCodeJsonAnnotator {
        json,
        results,
        root,
        base,
    };
    annotator.annotate_file_symbols();
    annotator.annotate_dependencies();
    annotator.annotate_members();
    annotator.annotate_imports_and_exports();
    annotator.annotate_graph();
    annotator.annotate_catalog();
}

struct DeadCodeJsonAnnotator<'a> {
    json: &'a mut serde_json::Value,
    results: &'a fallow_core::results::AnalysisResults,
    root: &'a Path,
    base: &'a FxHashSet<String>,
}

impl DeadCodeJsonAnnotator<'_> {
    fn annotate_file_symbols(&mut self) {
        annotate_issue_array(
            self.json,
            "unused_files",
            self.results.unused_files.iter().map(|item| {
                issue_was_introduced(
                    &format!(
                        "unused-file:{}",
                        relative_key_path(&item.file.path, self.root)
                    ),
                    self.base,
                )
            }),
        );
        annotate_issue_array(
            self.json,
            "unused_exports",
            self.results.unused_exports.iter().map(|item| {
                issue_was_introduced(
                    &format!(
                        "unused-export:{}:{}",
                        relative_key_path(&item.export.path, self.root),
                        item.export.export_name
                    ),
                    self.base,
                )
            }),
        );
        annotate_issue_array(
            self.json,
            "unused_types",
            self.results.unused_types.iter().map(|item| {
                issue_was_introduced(
                    &format!(
                        "unused-type:{}:{}",
                        relative_key_path(&item.export.path, self.root),
                        item.export.export_name
                    ),
                    self.base,
                )
            }),
        );
        annotate_issue_array(
            self.json,
            "private_type_leaks",
            self.results.private_type_leaks.iter().map(|item| {
                issue_was_introduced(
                    &format!(
                        "private-type-leak:{}:{}:{}",
                        relative_key_path(&item.leak.path, self.root),
                        item.leak.export_name,
                        item.leak.type_name
                    ),
                    self.base,
                )
            }),
        );
    }

    fn annotate_dependencies(&mut self) {
        annotate_dependency_json(self.json, self.results, self.root, self.base);
        annotate_issue_array(
            self.json,
            "type_only_dependencies",
            self.results.type_only_dependencies.iter().map(|item| {
                issue_was_introduced(
                    &format!(
                        "type-only-dependency:{}:{}",
                        relative_key_path(&item.dep.path, self.root),
                        item.dep.package_name
                    ),
                    self.base,
                )
            }),
        );
        annotate_issue_array(
            self.json,
            "test_only_dependencies",
            self.results.test_only_dependencies.iter().map(|item| {
                issue_was_introduced(
                    &format!(
                        "test-only-dependency:{}:{}",
                        relative_key_path(&item.dep.path, self.root),
                        item.dep.package_name
                    ),
                    self.base,
                )
            }),
        );
    }

    fn annotate_members(&mut self) {
        annotate_member_json(self.json, self.results, self.root, self.base);
    }

    fn annotate_imports_and_exports(&mut self) {
        self.annotate_import_dependency_keys();
        self.annotate_framework_keys();
        self.annotate_component_keys();
        self.annotate_route_keys();
    }

    fn annotate_import_dependency_keys(&mut self) {
        annotate_issue_array(
            self.json,
            "unresolved_imports",
            self.results.unresolved_imports.iter().map(|item| {
                issue_was_introduced(
                    &format!(
                        "unresolved-import:{}:{}",
                        relative_key_path(&item.import.path, self.root),
                        item.import.specifier
                    ),
                    self.base,
                )
            }),
        );
        annotate_issue_array(
            self.json,
            "unlisted_dependencies",
            self.results.unlisted_dependencies.iter().map(|item| {
                issue_was_introduced(&unlisted_dependency_key(&item.dep, self.root), self.base)
            }),
        );
        annotate_issue_array(
            self.json,
            "duplicate_exports",
            self.results.duplicate_exports.iter().map(|item| {
                let mut locations: Vec<String> = item
                    .export
                    .locations
                    .iter()
                    .map(|loc| relative_key_path(&loc.path, self.root))
                    .collect();
                locations.sort();
                locations.dedup();
                issue_was_introduced(
                    &format!(
                        "duplicate-export:{}:{}",
                        item.export.export_name,
                        locations.join("|")
                    ),
                    self.base,
                )
            }),
        );
    }

    fn annotate_framework_keys(&mut self) {
        annotate_issue_array(
            self.json,
            "invalid_client_exports",
            self.results.invalid_client_exports.iter().map(|item| {
                issue_was_introduced(
                    &invalid_client_export_key(&item.export, self.root),
                    self.base,
                )
            }),
        );
        annotate_issue_array(
            self.json,
            "mixed_client_server_barrels",
            self.results.mixed_client_server_barrels.iter().map(|item| {
                issue_was_introduced(
                    &mixed_client_server_barrel_key(&item.barrel, self.root),
                    self.base,
                )
            }),
        );
        annotate_issue_array(
            self.json,
            "misplaced_directives",
            self.results.misplaced_directives.iter().map(|item| {
                issue_was_introduced(
                    &misplaced_directive_key(&item.directive_site, self.root),
                    self.base,
                )
            }),
        );
        annotate_issue_array(
            self.json,
            "unprovided_injects",
            self.results.unprovided_injects.iter().map(|item| {
                issue_was_introduced(&unprovided_inject_key(&item.inject, self.root), self.base)
            }),
        );
    }

    fn annotate_component_keys(&mut self) {
        annotate_issue_array(
            self.json,
            "unrendered_components",
            self.results.unrendered_components.iter().map(|item| {
                issue_was_introduced(
                    &unrendered_component_key(&item.component, self.root),
                    self.base,
                )
            }),
        );
        annotate_issue_array(
            self.json,
            "unused_component_props",
            self.results.unused_component_props.iter().map(|item| {
                issue_was_introduced(&unused_component_prop_key(&item.prop, self.root), self.base)
            }),
        );
        annotate_issue_array(
            self.json,
            "unused_component_emits",
            self.results.unused_component_emits.iter().map(|item| {
                issue_was_introduced(&unused_component_emit_key(&item.emit, self.root), self.base)
            }),
        );
        annotate_issue_array(
            self.json,
            "unused_component_inputs",
            self.results.unused_component_inputs.iter().map(|item| {
                issue_was_introduced(
                    &unused_component_input_key(&item.input, self.root),
                    self.base,
                )
            }),
        );
        annotate_issue_array(
            self.json,
            "unused_component_outputs",
            self.results.unused_component_outputs.iter().map(|item| {
                issue_was_introduced(
                    &unused_component_output_key(&item.output, self.root),
                    self.base,
                )
            }),
        );
        annotate_issue_array(
            self.json,
            "unused_server_actions",
            self.results.unused_server_actions.iter().map(|item| {
                issue_was_introduced(
                    &unused_server_action_key(&item.action, self.root),
                    self.base,
                )
            }),
        );
    }

    fn annotate_route_keys(&mut self) {
        annotate_issue_array(
            self.json,
            "route_collisions",
            self.results.route_collisions.iter().map(|item| {
                issue_was_introduced(&route_collision_key(&item.collision, self.root), self.base)
            }),
        );
        annotate_issue_array(
            self.json,
            "dynamic_segment_name_conflicts",
            self.results
                .dynamic_segment_name_conflicts
                .iter()
                .map(|item| {
                    issue_was_introduced(
                        &dynamic_segment_name_conflict_key(&item.conflict, self.root),
                        self.base,
                    )
                }),
        );
    }

    fn annotate_graph(&mut self) {
        annotate_graph_json(self.json, self.results, self.root, self.base);
    }

    fn annotate_catalog(&mut self) {
        annotate_catalog_json(self.json, self.results, self.root, self.base);
    }
}

fn annotate_dependency_json(
    json: &mut serde_json::Value,
    results: &fallow_core::results::AnalysisResults,
    root: &Path,
    base: &FxHashSet<String>,
) {
    annotate_issue_array(
        json,
        "unused_dependencies",
        results
            .unused_dependencies
            .iter()
            .map(|item| issue_was_introduced(&unused_dependency_key(&item.dep, root), base)),
    );
    annotate_issue_array(
        json,
        "unused_dev_dependencies",
        results
            .unused_dev_dependencies
            .iter()
            .map(|item| issue_was_introduced(&unused_dependency_key(&item.dep, root), base)),
    );
    annotate_issue_array(
        json,
        "unused_optional_dependencies",
        results
            .unused_optional_dependencies
            .iter()
            .map(|item| issue_was_introduced(&unused_dependency_key(&item.dep, root), base)),
    );
}

fn annotate_member_json(
    json: &mut serde_json::Value,
    results: &fallow_core::results::AnalysisResults,
    root: &Path,
    base: &FxHashSet<String>,
) {
    annotate_issue_array(
        json,
        "unused_enum_members",
        results.unused_enum_members.iter().map(|item| {
            issue_was_introduced(
                &unused_member_key("unused-enum-member", &item.member, root),
                base,
            )
        }),
    );
    annotate_issue_array(
        json,
        "unused_class_members",
        results.unused_class_members.iter().map(|item| {
            issue_was_introduced(
                &unused_member_key("unused-class-member", &item.member, root),
                base,
            )
        }),
    );
    annotate_issue_array(
        json,
        "unused_store_members",
        results.unused_store_members.iter().map(|item| {
            issue_was_introduced(
                &unused_member_key("unused-store-member", &item.member, root),
                base,
            )
        }),
    );
}

fn annotate_graph_json(
    json: &mut serde_json::Value,
    results: &fallow_core::results::AnalysisResults,
    root: &Path,
    base: &FxHashSet<String>,
) {
    annotate_cycle_json(json, results, root, base);
    annotate_boundary_json(json, results, root, base);
    annotate_policy_json(json, results, root, base);
}

fn annotate_cycle_json(
    json: &mut serde_json::Value,
    results: &fallow_core::results::AnalysisResults,
    root: &Path,
    base: &FxHashSet<String>,
) {
    annotate_issue_array(
        json,
        "circular_dependencies",
        results.circular_dependencies.iter().map(|item| {
            let mut files: Vec<String> = item
                .cycle
                .files
                .iter()
                .map(|path| relative_key_path(path, root))
                .collect();
            files.sort();
            issue_was_introduced(&format!("circular-dependency:{}", files.join("|")), base)
        }),
    );
    annotate_issue_array(
        json,
        "re_export_cycles",
        results.re_export_cycles.iter().map(|item| {
            let kind = match item.cycle.kind {
                fallow_core::results::ReExportCycleKind::MultiNode => "multi-node",
                fallow_core::results::ReExportCycleKind::SelfLoop => "self-loop",
            };
            let mut files: Vec<String> = item
                .cycle
                .files
                .iter()
                .map(|path| relative_key_path(path, root))
                .collect();
            files.sort();
            issue_was_introduced(&format!("re-export-cycle:{kind}:{}", files.join("|")), base)
        }),
    );
}

fn annotate_boundary_json(
    json: &mut serde_json::Value,
    results: &fallow_core::results::AnalysisResults,
    root: &Path,
    base: &FxHashSet<String>,
) {
    annotate_issue_array(
        json,
        "boundary_violations",
        results.boundary_violations.iter().map(|item| {
            issue_was_introduced(
                &format!(
                    "boundary-violation:{}:{}:{}",
                    relative_key_path(&item.violation.from_path, root),
                    relative_key_path(&item.violation.to_path, root),
                    item.violation.import_specifier
                ),
                base,
            )
        }),
    );
    annotate_issue_array(
        json,
        "boundary_coverage_violations",
        results.boundary_coverage_violations.iter().map(|item| {
            issue_was_introduced(
                &format!(
                    "boundary-coverage:{}",
                    relative_key_path(&item.violation.path, root)
                ),
                base,
            )
        }),
    );
    annotate_issue_array(
        json,
        "boundary_call_violations",
        results.boundary_call_violations.iter().map(|item| {
            issue_was_introduced(
                &format!(
                    "boundary-call:{}:{}",
                    relative_key_path(&item.violation.path, root),
                    item.violation.callee
                ),
                base,
            )
        }),
    );
}

fn annotate_policy_json(
    json: &mut serde_json::Value,
    results: &fallow_core::results::AnalysisResults,
    root: &Path,
    base: &FxHashSet<String>,
) {
    annotate_issue_array(
        json,
        "policy_violations",
        results.policy_violations.iter().map(|item| {
            issue_was_introduced(
                &format!(
                    "policy-violation:{}:{}/{}:{}",
                    relative_key_path(&item.violation.path, root),
                    item.violation.pack,
                    item.violation.rule_id,
                    item.violation.matched
                ),
                base,
            )
        }),
    );
    annotate_issue_array(
        json,
        "stale_suppressions",
        results.stale_suppressions.iter().map(|item| {
            issue_was_introduced(
                &format!(
                    "stale-suppression:{}:{}",
                    relative_key_path(&item.path, root),
                    item.description()
                ),
                base,
            )
        }),
    );
}

fn annotate_catalog_json(
    json: &mut serde_json::Value,
    results: &fallow_core::results::AnalysisResults,
    root: &Path,
    base: &FxHashSet<String>,
) {
    annotate_issue_array(
        json,
        "unresolved_catalog_references",
        results.unresolved_catalog_references.iter().map(|item| {
            issue_was_introduced(
                &format!(
                    "unresolved-catalog-reference:{}:{}:{}:{}",
                    relative_key_path(&item.reference.path, root),
                    item.reference.line,
                    item.reference.catalog_name,
                    item.reference.entry_name
                ),
                base,
            )
        }),
    );
    annotate_issue_array(
        json,
        "unused_catalog_entries",
        results
            .unused_catalog_entries
            .iter()
            .map(|item| issue_was_introduced(&unused_catalog_entry_key(&item.entry, root), base)),
    );
    annotate_issue_array(
        json,
        "empty_catalog_groups",
        results
            .empty_catalog_groups
            .iter()
            .map(|item| issue_was_introduced(&empty_catalog_group_key(&item.group, root), base)),
    );
    annotate_issue_array(
        json,
        "unused_dependency_overrides",
        results.unused_dependency_overrides.iter().map(|item| {
            issue_was_introduced(
                &format!(
                    "unused-dependency-override:{}:{}:{}",
                    relative_key_path(&item.entry.path, root),
                    item.entry.line,
                    item.entry.raw_key
                ),
                base,
            )
        }),
    );
    annotate_issue_array(
        json,
        "misconfigured_dependency_overrides",
        results
            .misconfigured_dependency_overrides
            .iter()
            .map(|item| {
                issue_was_introduced(
                    &format!(
                        "misconfigured-dependency-override:{}:{}:{}",
                        relative_key_path(&item.entry.path, root),
                        item.entry.line,
                        item.entry.raw_key
                    ),
                    base,
                )
            }),
    );
}

pub(super) fn annotate_health_json(
    json: &mut serde_json::Value,
    report: &crate::health_types::HealthReport,
    root: &Path,
    base: &FxHashSet<String>,
) {
    let Some(items) = json
        .get_mut("findings")
        .and_then(serde_json::Value::as_array_mut)
    else {
        return;
    };
    for (item, finding) in items.iter_mut().zip(&report.findings) {
        if let serde_json::Value::Object(map) = item {
            map.insert(
                "introduced".to_string(),
                serde_json::json!(issue_was_introduced(
                    &health_finding_key(finding, root),
                    base
                )),
            );
        }
    }
}

pub(super) fn annotate_dupes_json(
    json: &mut serde_json::Value,
    report: &fallow_core::duplicates::DuplicationReport,
    root: &Path,
    base: &FxHashSet<String>,
) {
    let Some(items) = json
        .get_mut("clone_groups")
        .and_then(serde_json::Value::as_array_mut)
    else {
        return;
    };
    for (item, group) in items.iter_mut().zip(&report.clone_groups) {
        if let serde_json::Value::Object(map) = item {
            map.insert(
                "introduced".to_string(),
                serde_json::json!(issue_was_introduced(&dupe_group_key(group, root), base)),
            );
        }
    }
}

pub(super) fn health_keys(
    report: &crate::health_types::HealthReport,
    root: &Path,
) -> FxHashSet<String> {
    report
        .findings
        .iter()
        .map(|finding| health_finding_key(finding, root))
        .collect()
}

pub(super) fn health_finding_key(
    finding: &crate::health_types::ComplexityViolation,
    root: &Path,
) -> String {
    format!(
        "complexity:{}:{}:{:?}",
        relative_key_path(&finding.path, root),
        finding.name,
        finding.exceeded
    )
}

pub(super) fn dupes_keys(
    report: &fallow_core::duplicates::DuplicationReport,
    root: &Path,
) -> FxHashSet<String> {
    report
        .clone_groups
        .iter()
        .map(|group| dupe_group_key(group, root))
        .collect()
}

pub(super) fn dupe_group_key(group: &fallow_core::duplicates::CloneGroup, root: &Path) -> String {
    let mut files: Vec<String> = group
        .instances
        .iter()
        .map(|instance| relative_key_path(&instance.file, root))
        .collect();
    files.sort();
    files.dedup();
    let mut hasher = DefaultHasher::new();
    for instance in &group.instances {
        instance.fragment.hash(&mut hasher);
    }
    format!(
        "dupe:{}:{}:{}:{:x}",
        files.join("|"),
        group.token_count,
        group.line_count,
        hasher.finish()
    )
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use fallow_core::extract::MemberKind;
    use fallow_core::results::{
        AnalysisResults, BoundaryCallViolation, BoundaryCallViolationFinding,
        BoundaryCoverageViolation, BoundaryCoverageViolationFinding, BoundaryViolation,
        BoundaryViolationFinding, CircularDependency, CircularDependencyFinding,
        DependencyLocation, DependencyOverrideMisconfigReason, DependencyOverrideSource,
        DuplicateExport, DuplicateExportFinding, DuplicateLocation, EmptyCatalogGroup,
        EmptyCatalogGroupFinding, ImportSite, MisconfiguredDependencyOverride,
        MisconfiguredDependencyOverrideFinding, PrivateTypeLeak, PrivateTypeLeakFinding,
        ReExportCycle, ReExportCycleFinding, ReExportCycleKind, StaleSuppression,
        SuppressionOrigin, TestOnlyDependency, TestOnlyDependencyFinding, TypeOnlyDependency,
        TypeOnlyDependencyFinding, UnlistedDependency, UnlistedDependencyFinding,
        UnresolvedCatalogReference, UnresolvedCatalogReferenceFinding, UnresolvedImport,
        UnresolvedImportFinding, UnusedCatalogEntry, UnusedCatalogEntryFinding,
        UnusedClassMemberFinding, UnusedDependency, UnusedDependencyFinding,
        UnusedDependencyOverride, UnusedDependencyOverrideFinding, UnusedDevDependencyFinding,
        UnusedEnumMemberFinding, UnusedExport, UnusedExportFinding, UnusedFile, UnusedFileFinding,
        UnusedMember, UnusedOptionalDependencyFinding, UnusedTypeFinding,
    };
    use rustc_hash::FxHashSet;
    use serde_json::json;

    use super::{
        annotate_dead_code_json, dead_code_keys, relative_key_path, retain_introduced_dead_code,
    };

    fn root() -> PathBuf {
        PathBuf::from("/repo")
    }

    fn export(path: &Path, name: &str) -> UnusedExportFinding {
        UnusedExportFinding::with_actions(UnusedExport {
            path: path.to_path_buf(),
            export_name: name.to_string(),
            is_type_only: false,
            line: 1,
            col: 0,
            span_start: 0,
            is_re_export: false,
        })
    }

    fn unused_file(path: &Path) -> UnusedFileFinding {
        UnusedFileFinding::with_actions(UnusedFile {
            path: path.to_path_buf(),
        })
    }

    fn dependency(path: &Path, package_name: &str) -> UnusedDependencyFinding {
        UnusedDependencyFinding::with_actions(UnusedDependency {
            package_name: package_name.to_string(),
            location: DependencyLocation::Dependencies,
            path: path.to_path_buf(),
            line: 4,
            used_in_workspaces: Vec::new(),
        })
    }

    fn unresolved(path: &Path, specifier: &str) -> UnresolvedImportFinding {
        UnresolvedImportFinding::with_actions(UnresolvedImport {
            path: path.to_path_buf(),
            specifier: specifier.to_string(),
            line: 2,
            col: 1,
            specifier_col: 8,
        })
    }

    fn unlisted(path: &Path, package_name: &str) -> UnlistedDependencyFinding {
        UnlistedDependencyFinding::with_actions(UnlistedDependency {
            package_name: package_name.to_string(),
            imported_from: vec![
                ImportSite {
                    path: path.to_path_buf(),
                    line: 9,
                    col: 2,
                },
                ImportSite {
                    path: path.to_path_buf(),
                    line: 9,
                    col: 2,
                },
            ],
        })
    }

    fn duplicate_export(root: &Path) -> DuplicateExportFinding {
        DuplicateExportFinding::with_actions(DuplicateExport {
            export_name: "Button".to_string(),
            locations: vec![
                DuplicateLocation {
                    path: root.join("src/b.ts"),
                    line: 1,
                    col: 0,
                },
                DuplicateLocation {
                    path: root.join("src/a.ts"),
                    line: 1,
                    col: 0,
                },
                DuplicateLocation {
                    path: root.join("src/a.ts"),
                    line: 2,
                    col: 0,
                },
            ],
        })
    }

    fn sample_results(root: &Path) -> AnalysisResults {
        let source = root.join("src/page.ts");
        let package_json = root.join("package.json");
        let mut results = AnalysisResults::default();
        results
            .unused_files
            .push(unused_file(&root.join("src/dead.ts")));
        results.unused_exports.push(export(&source, "loader"));
        results
            .unused_dependencies
            .push(dependency(&package_json, "left-pad"));
        results
            .unresolved_imports
            .push(unresolved(&source, "./missing"));
        results.unlisted_dependencies.push(unlisted(&source, "zod"));
        results.duplicate_exports.push(duplicate_export(root));
        results
    }

    #[test]
    fn relative_key_path_strips_root_and_normalizes_separators() {
        let path = Path::new("/repo/src\\feature\\index.ts");
        assert_eq!(
            relative_key_path(path, Path::new("/repo")),
            "src/feature/index.ts"
        );
    }

    #[test]
    fn dead_code_keys_are_stable_for_unsorted_and_duplicate_locations() {
        let root = root();
        let keys = dead_code_keys(&sample_results(&root), &root);

        assert!(keys.contains("unused-file:src/dead.ts"));
        assert!(keys.contains("unused-export:src/page.ts:loader"));
        assert!(keys.contains("unused-dependency:package.json:left-pad"));
        assert!(keys.contains("unresolved-import:src/page.ts:./missing"));
        assert!(keys.contains("unlisted-dependency:zod:src/page.ts:9:2"));
        assert!(keys.contains("duplicate-export:Button:src/a.ts|src/b.ts"));
    }

    #[test]
    fn dead_code_keys_cover_type_member_and_dependency_variants() {
        let root = root();
        let source = root.join("src/types.ts");
        let package_json = root.join("package.json");
        let mut results = AnalysisResults::default();
        results
            .unused_types
            .push(UnusedTypeFinding::with_actions(UnusedExport {
                path: source.clone(),
                export_name: "UnusedType".to_string(),
                is_type_only: true,
                line: 3,
                col: 0,
                span_start: 12,
                is_re_export: false,
            }));
        results
            .private_type_leaks
            .push(PrivateTypeLeakFinding::with_actions(PrivateTypeLeak {
                path: source.clone(),
                export_name: "makePublic".to_string(),
                type_name: "PrivateShape".to_string(),
                line: 7,
                col: 12,
                span_start: 64,
            }));
        results
            .unused_dev_dependencies
            .push(UnusedDevDependencyFinding::with_actions(UnusedDependency {
                package_name: "vite".to_string(),
                location: DependencyLocation::DevDependencies,
                path: package_json.clone(),
                line: 10,
                used_in_workspaces: Vec::new(),
            }));
        results
            .unused_optional_dependencies
            .push(UnusedOptionalDependencyFinding::with_actions(
                UnusedDependency {
                    package_name: "fsevents".to_string(),
                    location: DependencyLocation::OptionalDependencies,
                    path: package_json.clone(),
                    line: 11,
                    used_in_workspaces: Vec::new(),
                },
            ));
        results
            .unused_enum_members
            .push(UnusedEnumMemberFinding::with_actions(UnusedMember {
                path: source.clone(),
                parent_name: "Status".to_string(),
                member_name: "Idle".to_string(),
                kind: MemberKind::EnumMember,
                line: 15,
                col: 2,
            }));
        results
            .unused_class_members
            .push(UnusedClassMemberFinding::with_actions(UnusedMember {
                path: source,
                parent_name: "Controller".to_string(),
                member_name: "legacy".to_string(),
                kind: MemberKind::ClassMethod,
                line: 21,
                col: 2,
            }));
        results
            .type_only_dependencies
            .push(TypeOnlyDependencyFinding::with_actions(
                TypeOnlyDependency {
                    package_name: "zod".to_string(),
                    path: package_json.clone(),
                    line: 12,
                },
            ));
        results
            .test_only_dependencies
            .push(TestOnlyDependencyFinding::with_actions(
                TestOnlyDependency {
                    package_name: "vitest".to_string(),
                    path: package_json,
                    line: 13,
                },
            ));

        let keys = dead_code_keys(&results, &root);

        assert!(keys.contains("unused-type:src/types.ts:UnusedType"));
        assert!(keys.contains("private-type-leak:src/types.ts:makePublic:PrivateShape"));
        assert!(keys.contains("unused-dev-dependency:package.json:vite"));
        assert!(keys.contains("unused-optional-dependency:package.json:fsevents"));
        assert!(keys.contains("unused-enum-member:src/types.ts:Status:Idle"));
        assert!(keys.contains("unused-class-member:src/types.ts:Controller:legacy"));
        assert!(keys.contains("type-only-dependency:package.json:zod"));
        assert!(keys.contains("test-only-dependency:package.json:vitest"));
    }

    #[test]
    fn dead_code_keys_cover_graph_boundary_catalog_and_override_variants() {
        let root = root();
        let source = root.join("src/app.ts");
        let other = root.join("src/other.ts");
        let workspace = root.join("pnpm-workspace.yaml");
        let mut results = AnalysisResults::default();
        results
            .circular_dependencies
            .push(CircularDependencyFinding::with_actions(
                CircularDependency {
                    files: vec![other.clone(), source.clone()],
                    length: 2,
                    line: 4,
                    col: 0,
                    edges: Vec::new(),
                    is_cross_package: false,
                },
            ));
        results
            .re_export_cycles
            .push(ReExportCycleFinding::with_actions(ReExportCycle {
                files: vec![source.clone()],
                kind: ReExportCycleKind::SelfLoop,
            }));
        results
            .boundary_violations
            .push(BoundaryViolationFinding::with_actions(BoundaryViolation {
                from_path: source.clone(),
                to_path: other,
                from_zone: "ui".to_string(),
                to_zone: "server".to_string(),
                import_specifier: "../other".to_string(),
                line: 1,
                col: 0,
            }));
        results
            .boundary_coverage_violations
            .push(BoundaryCoverageViolationFinding::with_actions(
                BoundaryCoverageViolation {
                    path: root.join("src/unmatched.ts"),
                    line: 1,
                    col: 0,
                },
            ));
        results
            .boundary_call_violations
            .push(BoundaryCallViolationFinding::with_actions(
                BoundaryCallViolation {
                    path: source.clone(),
                    line: 12,
                    col: 4,
                    zone: "ui".to_string(),
                    callee: "child_process.exec".to_string(),
                    pattern: "child_process.*".to_string(),
                },
            ));
        results.stale_suppressions.push(StaleSuppression {
            path: source,
            line: 2,
            col: 0,
            origin: SuppressionOrigin::Comment {
                issue_kind: Some("unused-export".to_string()),
                is_file_level: false,
                kind_known: true,
            },
        });
        results.unresolved_catalog_references.push(
            UnresolvedCatalogReferenceFinding::with_actions(UnresolvedCatalogReference {
                entry_name: "react".to_string(),
                catalog_name: "default".to_string(),
                path: root.join("packages/app/package.json"),
                line: 9,
                available_in_catalogs: vec!["react18".to_string()],
            }),
        );
        results
            .unused_catalog_entries
            .push(UnusedCatalogEntryFinding::with_actions(
                UnusedCatalogEntry {
                    entry_name: "lodash".to_string(),
                    catalog_name: "default".to_string(),
                    path: workspace.clone(),
                    line: 3,
                    hardcoded_consumers: Vec::new(),
                },
            ));
        results
            .empty_catalog_groups
            .push(EmptyCatalogGroupFinding::with_actions(EmptyCatalogGroup {
                catalog_name: "react17".to_string(),
                path: workspace.clone(),
                line: 7,
            }));
        results
            .unused_dependency_overrides
            .push(UnusedDependencyOverrideFinding::with_actions(
                UnusedDependencyOverride {
                    raw_key: "left-pad".to_string(),
                    target_package: "left-pad".to_string(),
                    parent_package: None,
                    version_constraint: None,
                    version_range: "^1.3.0".to_string(),
                    source: DependencyOverrideSource::PnpmWorkspaceYaml,
                    path: workspace.clone(),
                    line: 11,
                    hint: None,
                },
            ));
        results.misconfigured_dependency_overrides.push(
            MisconfiguredDependencyOverrideFinding::with_actions(MisconfiguredDependencyOverride {
                raw_key: ">".to_string(),
                target_package: None,
                raw_value: String::new(),
                reason: DependencyOverrideMisconfigReason::UnparsableKey,
                source: DependencyOverrideSource::PnpmWorkspaceYaml,
                path: workspace,
                line: 12,
            }),
        );

        let keys = dead_code_keys(&results, &root);

        assert!(keys.contains("circular-dependency:src/app.ts|src/other.ts"));
        assert!(keys.contains("re-export-cycle:self-loop:src/app.ts"));
        assert!(keys.contains("boundary-violation:src/app.ts:src/other.ts:../other"));
        assert!(keys.contains("boundary-coverage:src/unmatched.ts"));
        assert!(keys.contains("boundary-call:src/app.ts:child_process.exec"));
        assert!(
            keys.contains("stale-suppression:src/app.ts:// fallow-ignore-next-line unused-export")
        );
        assert!(
            keys.contains("unresolved-catalog-reference:packages/app/package.json:9:default:react")
        );
        assert!(keys.contains("unused-catalog-entry:pnpm-workspace.yaml:3:default:lodash"));
        assert!(keys.contains("empty-catalog-group:pnpm-workspace.yaml:7:react17"));
        assert!(keys.contains("unused-dependency-override:pnpm-workspace.yaml:11:left-pad"));
        assert!(keys.contains("misconfigured-dependency-override:pnpm-workspace.yaml:12:>"));
    }

    #[test]
    fn retain_introduced_dead_code_keeps_only_findings_absent_from_base() {
        let root = root();
        let mut results = sample_results(&root);
        let base = FxHashSet::from_iter([
            "unused-file:src/dead.ts".to_string(),
            "unused-dependency:package.json:left-pad".to_string(),
            "unresolved-import:src/page.ts:./missing".to_string(),
        ]);

        retain_introduced_dead_code(&mut results, &root, Some(&base));

        assert!(results.unused_files.is_empty());
        assert!(results.unused_dependencies.is_empty());
        assert!(results.unresolved_imports.is_empty());
        assert_eq!(results.unused_exports.len(), 1);
        assert_eq!(results.unlisted_dependencies.len(), 1);
        assert_eq!(results.duplicate_exports.len(), 1);
    }

    #[test]
    fn annotate_dead_code_json_marks_introduced_status_by_matching_key_order() {
        let root = root();
        let results = sample_results(&root);
        let base = FxHashSet::from_iter([
            "unused-file:src/dead.ts".to_string(),
            "unlisted-dependency:zod:src/page.ts:9:2".to_string(),
        ]);
        let mut json = json!({
            "unused_files": [{}],
            "unused_exports": [{}],
            "unused_dependencies": [{}],
            "unresolved_imports": [{}],
            "unlisted_dependencies": [{}],
            "duplicate_exports": [{}],
        });

        annotate_dead_code_json(&mut json, &results, &root, &base);

        assert_eq!(json["unused_files"][0]["introduced"], false);
        assert_eq!(json["unused_exports"][0]["introduced"], true);
        assert_eq!(json["unused_dependencies"][0]["introduced"], true);
        assert_eq!(json["unresolved_imports"][0]["introduced"], true);
        assert_eq!(json["unlisted_dependencies"][0]["introduced"], false);
        assert_eq!(json["duplicate_exports"][0]["introduced"], true);
    }
}
