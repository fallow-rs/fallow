//! Dead-code result helpers exposed through the engine boundary.

use std::path::{Path, PathBuf};

use rustc_hash::FxHashSet;

use fallow_config::{ResolvedConfig, RulesConfig, Severity};
use fallow_types::discover::StableFileKey;

pub use crate::results::{
    AnalysisResults, DeadCodeAnalysis, DeadCodeAnalysisArtifacts, DeadCodeAnalysisOutput,
    DeadCodeAnalysisWithHashes, derive_security_severity, enable_security_rules,
    security_catalogue_title, security_finding_id, security_rule_id,
};

pub use crate::effective_severity::{
    RuleSeverity, SeveritySource, apply_effective_severities, promote_effective_warns,
};

use crate::{
    EngineResult, session::analyze_dead_code_with_parse_result_from_config, source::ModuleInfo,
};

/// Run dead-code analysis from pre-parsed modules.
///
/// # Errors
///
/// Returns an error if discovery, graph construction, or analysis fails.
pub(crate) fn analyze_with_parse_result(
    config: &ResolvedConfig,
    modules: &[ModuleInfo],
) -> EngineResult<DeadCodeAnalysisArtifacts> {
    analyze_dead_code_with_parse_result_from_config(config, modules)
}

/// Scope dead-code results to the union of the given workspace roots.
///
/// The full cross-workspace graph is still built before this helper runs, so
/// cross-package imports are resolved. Only reported findings are narrowed.
pub fn filter_to_workspaces(results: &mut AnalysisResults, ws_roots: &[PathBuf]) {
    let any_under = |path: &Path| ws_roots.iter().any(|root| path.starts_with(root));
    let pkg_jsons = ws_roots
        .iter()
        .map(|root| root.join("package.json"))
        .collect::<Vec<_>>();
    let in_pkg_jsons = |path: &Path| pkg_jsons.iter().any(|pkg| path == pkg);

    filter_workspace_source_findings(results, &any_under);
    filter_workspace_dependency_findings(results, &any_under, &in_pkg_jsons);
    filter_workspace_graph_findings(results, &any_under);
    filter_workspace_policy_findings(results, &any_under);
}

/// The scope of one dead-code run, as the surface resolved it.
///
/// Every field is optional. A field that is `None` does not narrow the run.
#[derive(Debug, Clone, Copy)]
pub struct DeadCodeScope<'a> {
    /// `--workspace`, `--changed-workspaces` and a positional path: the union
    /// of these roots.
    pub workspace_roots: Option<&'a [PathBuf]>,
    /// `--changed-since`: the files that changed since the ref.
    pub changed_files: Option<&'a FxHashSet<PathBuf>>,
    /// A unified diff, with the root that finding paths resolve against.
    pub diff: Option<(&'a fallow_output::DiffIndex, &'a Path)>,
    /// `--file`: the only files to report. Dependency findings are dropped,
    /// because a file list does not own a manifest.
    pub files: Option<&'a FxHashSet<PathBuf>>,
}

/// Narrow dead-code results to the scope of the run.
///
/// The CLI, the programmatic API and the MCP typed path call this one function,
/// so a scope narrows the same way on every surface. The filters run in this
/// order: workspace roots, changed files, the diff, the file list. Then the
/// configured `ignoreFindings` patterns run again, because the scope filters
/// remove owners from a finding with several owners (`duplicate_exports`). A
/// finding that only ignored owners hold after the scope is hidden, as the
/// "hidden only when every owner matches" rule says.
pub fn apply_scope(
    results: &mut AnalysisResults,
    scope: &DeadCodeScope<'_>,
    config: &ResolvedConfig,
) {
    if let Some(roots) = scope.workspace_roots {
        filter_to_workspaces(results, roots);
    }
    if let Some(changed_files) = scope.changed_files {
        filter_by_changed_files(results, changed_files);
    }
    if let Some((diff, root)) = scope.diff {
        crate::diff_scope::filter_dead_code_by_diff(results, diff, root);
    }
    if let Some(files) = scope.files {
        filter_by_changed_files(results, files);
        clear_dependency_findings(results);
    }
    filter_configured_ignored_findings(results, config);
}

fn clear_dependency_findings(results: &mut AnalysisResults) {
    results.unused_dependencies.clear();
    results.unused_dev_dependencies.clear();
    results.unused_optional_dependencies.clear();
    results.type_only_dependencies.clear();
    results.test_only_dependencies.clear();
    results.dev_dependencies_in_production.clear();
}

/// Scope dead-code results to findings affected by changed files.
#[expect(
    clippy::implicit_hasher,
    reason = "fallow standardizes on FxHashSet across the workspace"
)]
pub fn filter_by_changed_files(results: &mut AnalysisResults, changed_files: &FxHashSet<PathBuf>) {
    crate::changed_files::filter_results_by_changed_files(results, changed_files);
}

/// Apply configured source-owned finding exclusions to an analysis result.
///
/// Analysis stages that append findings after the engine pipeline, such as
/// type-aware reconciliation, must call this before exposing their final
/// result.
pub fn filter_configured_ignored_findings(results: &mut AnalysisResults, config: &ResolvedConfig) {
    if config.ignore_findings.is_empty() {
        return;
    }

    results.remove_ignored_dead_code_findings(|path| {
        let key = if path.is_absolute() {
            let Ok(relative) = path.strip_prefix(&config.root) else {
                return false;
            };
            StableFileKey::from_relative(relative)
        } else {
            StableFileKey::from_relative(path)
        };
        config.ignore_findings.is_ignored(key.as_str())
    });
}

fn filter_workspace_source_findings(
    results: &mut AnalysisResults,
    any_under: &dyn Fn(&Path) -> bool,
) {
    results
        .unused_files
        .retain(|finding| any_under(&finding.file.path));
    results
        .unused_exports
        .retain(|finding| any_under(&finding.export.path));
    results
        .unused_types
        .retain(|finding| any_under(&finding.export.path));
    results
        .private_type_leaks
        .retain(|finding| any_under(&finding.leak.path));
    results
        .unused_enum_members
        .retain(|finding| any_under(&finding.member.path));
    results
        .unused_class_members
        .retain(|finding| any_under(&finding.member.path));
    results
        .unused_store_members
        .retain(|finding| any_under(&finding.member.path));
    results
        .unprovided_injects
        .retain(|finding| any_under(&finding.inject.path));
    results
        .unrendered_components
        .retain(|finding| any_under(&finding.component.path));
    results
        .unused_component_props
        .retain(|finding| any_under(&finding.prop.path));
    results
        .unused_component_emits
        .retain(|finding| any_under(&finding.emit.path));
    results
        .unused_component_inputs
        .retain(|finding| any_under(&finding.input.path));
    results
        .unused_component_outputs
        .retain(|finding| any_under(&finding.output.path));
    results
        .unused_svelte_events
        .retain(|finding| any_under(&finding.event.path));
    results
        .unused_server_actions
        .retain(|finding| any_under(&finding.action.path));
    results
        .unused_load_data_keys
        .retain(|finding| any_under(&finding.key.path));
    results
        .unresolved_imports
        .retain(|finding| any_under(&finding.import.path));
}

fn filter_workspace_dependency_findings(
    results: &mut AnalysisResults,
    any_under: &dyn Fn(&Path) -> bool,
    in_pkg_jsons: &dyn Fn(&Path) -> bool,
) {
    results
        .unused_dependencies
        .retain(|finding| in_pkg_jsons(&finding.dep.path));
    results
        .unused_dev_dependencies
        .retain(|finding| in_pkg_jsons(&finding.dep.path));
    results
        .unused_optional_dependencies
        .retain(|finding| in_pkg_jsons(&finding.dep.path));
    results
        .type_only_dependencies
        .retain(|finding| in_pkg_jsons(&finding.dep.path));
    results
        .test_only_dependencies
        .retain(|finding| in_pkg_jsons(&finding.dep.path));
    results
        .dev_dependencies_in_production
        .retain(|finding| in_pkg_jsons(&finding.dep.path));

    results.unlisted_dependencies.retain(|finding| {
        finding
            .dep
            .imported_from
            .iter()
            .any(|source| any_under(&source.path))
    });
    results.unused_dependency_overrides.clear();
    results.misconfigured_dependency_overrides.clear();
}

fn filter_workspace_graph_findings(
    results: &mut AnalysisResults,
    any_under: &dyn Fn(&Path) -> bool,
) {
    for duplicate in &mut results.duplicate_exports {
        duplicate
            .export
            .locations
            .retain(|location| any_under(&location.path));
    }
    results
        .duplicate_exports
        .retain(|duplicate| duplicate.export.locations.len() >= 2);

    results
        .circular_dependencies
        .retain(|cycle| cycle.cycle.files.iter().any(|path| any_under(path)));

    results
        .re_export_cycles
        .retain(|cycle| cycle.cycle.files.iter().any(|path| any_under(path)));
}

fn filter_workspace_policy_findings(
    results: &mut AnalysisResults,
    any_under: &dyn Fn(&Path) -> bool,
) {
    results
        .boundary_violations
        .retain(|finding| any_under(&finding.violation.from_path));
    results
        .boundary_coverage_violations
        .retain(|finding| any_under(&finding.violation.path));
    results
        .boundary_call_violations
        .retain(|finding| any_under(&finding.violation.path));
    results
        .policy_violations
        .retain(|finding| any_under(&finding.violation.path));

    results
        .stale_suppressions
        .retain(|finding| any_under(&finding.path));

    results
        .security_findings
        .retain(|finding| any_under(&finding.path));
    results
        .security_unresolved_callee_diagnostics
        .retain(|finding| any_under(&finding.path));

    results.unused_catalog_entries.clear();
    results.empty_catalog_groups.clear();
    results
        .unresolved_catalog_references
        .retain(|finding| any_under(&finding.reference.path));

    results
        .invalid_client_exports
        .retain(|finding| any_under(&finding.export.path));

    results
        .mixed_client_server_barrels
        .retain(|finding| any_under(&finding.barrel.path));

    results
        .misplaced_directives
        .retain(|finding| any_under(&finding.directive_site.path));

    results
        .route_collisions
        .retain(|finding| any_under(&finding.collision.path));

    results
        .dynamic_segment_name_conflicts
        .retain(|finding| any_under(&finding.conflict.path));
}

/// Remove findings whose effective severity is `Off` from an analysis result.
///
/// Every surface that reports findings runs this pass: the `check` command
/// (which also serves `dead-code` and the CLI audit), the editor analysis path
/// behind inline diagnostics and the sidebar, and the programmatic runtime
/// behind the MCP tools, the decision surface and the Node bindings. Each of
/// them runs it at the same two points, once over the freshly analyzed set and
/// once after type-aware reconciliation, because reconciliation can append
/// findings. The pass removes findings and writes the gate severity of each
/// finding that stays, so the second run is idempotent when nothing was
/// appended.
///
/// When overrides are configured, per-file rule resolution is used for
/// file-scoped issue types. Circular dependencies resolve against every file in
/// the cycle. Non-file-scoped issues (unused deps, unlisted deps, duplicate
/// exports) use the base rules only.
pub fn apply_rule_severities(results: &mut AnalysisResults, config: &ResolvedConfig) {
    let rules = &config.rules;
    let has_overrides = !config.overrides.is_empty();

    if has_overrides {
        apply_file_override_rules(results, config);
        apply_boundary_override_rules(results, config);
    } else {
        apply_base_file_rules(results, rules);
    }

    apply_base_collection_rules(results, rules);
    apply_effective_severities(results, config);
}

fn apply_base_collection_rules(results: &mut AnalysisResults, rules: &RulesConfig) {
    if rules.unused_dependencies == Severity::Off {
        results.unused_dependencies.clear();
    }
    if rules.unused_dev_dependencies == Severity::Off {
        results.unused_dev_dependencies.clear();
    }
    if rules.unused_optional_dependencies == Severity::Off {
        results.unused_optional_dependencies.clear();
    }
    if rules.unlisted_dependencies == Severity::Off {
        results.unlisted_dependencies.clear();
    }
    if rules.duplicate_exports == Severity::Off {
        results.duplicate_exports.clear();
    }
    if rules.type_only_dependencies == Severity::Off {
        results.type_only_dependencies.clear();
    }
    if rules.test_only_dependencies == Severity::Off {
        results.test_only_dependencies.clear();
    }
    if rules.dev_dependencies_in_production == Severity::Off {
        results.dev_dependencies_in_production.clear();
    }
    if rules.circular_dependencies == Severity::Off {
        results.circular_dependencies.clear();
    }
    if rules.re_export_cycle == Severity::Off {
        results.re_export_cycles.clear();
    }
    if rules.boundary_violation == Severity::Off {
        results.boundary_violations.clear();
        results.boundary_coverage_violations.clear();
        results.boundary_call_violations.clear();
    }
    if rules.policy_violation == Severity::Off {
        results.policy_violations.clear();
    }
    if rules.unused_catalog_entries == Severity::Off {
        results.unused_catalog_entries.clear();
    }
    if rules.empty_catalog_groups == Severity::Off {
        results.empty_catalog_groups.clear();
    }
    if rules.unresolved_catalog_references == Severity::Off {
        results.unresolved_catalog_references.clear();
    }
    if rules.unused_dependency_overrides == Severity::Off {
        results.unused_dependency_overrides.clear();
    }
    if rules.misconfigured_dependency_overrides == Severity::Off {
        results.misconfigured_dependency_overrides.clear();
    }
}

fn apply_file_override_rules(results: &mut AnalysisResults, config: &ResolvedConfig) {
    apply_dead_code_override_rules(results, config);
    apply_catalog_override_rules(results, config);
    apply_framework_override_rules(results, config);
    apply_circular_override_rules(results, config);
}

fn apply_dead_code_override_rules(results: &mut AnalysisResults, config: &ResolvedConfig) {
    apply_core_dead_code_override_rules(results, config);
    apply_component_dead_code_override_rules(results, config);
}

/// Retain core (non-component) dead-code findings whose per-file rule is not Off.
fn apply_core_dead_code_override_rules(results: &mut AnalysisResults, config: &ResolvedConfig) {
    results
        .unused_files
        .retain(|f| config.resolve_rules_for_path(&f.file.path).unused_files != Severity::Off);
    results
        .unused_exports
        .retain(|e| config.resolve_rules_for_path(&e.export.path).unused_exports != Severity::Off);
    results
        .unused_types
        .retain(|e| config.resolve_rules_for_path(&e.export.path).unused_types != Severity::Off);
    results.private_type_leaks.retain(|e| {
        config
            .resolve_rules_for_path(&e.leak.path)
            .private_type_leaks
            != Severity::Off
    });
    results.unused_enum_members.retain(|m| {
        config
            .resolve_rules_for_path(&m.member.path)
            .unused_enum_members
            != Severity::Off
    });
    results.unused_class_members.retain(|m| {
        config
            .resolve_rules_for_path(&m.member.path)
            .unused_class_members
            != Severity::Off
    });
    results.unused_store_members.retain(|m| {
        config
            .resolve_rules_for_path(&m.member.path)
            .unused_store_members
            != Severity::Off
    });
    results.unprovided_injects.retain(|f| {
        config
            .resolve_rules_for_path(&f.inject.path)
            .unprovided_injects
            != Severity::Off
    });
    results.unresolved_imports.retain(|i| {
        config
            .resolve_rules_for_path(&i.import.path)
            .unresolved_imports
            != Severity::Off
    });
}

/// Retain component-shaped dead-code findings whose per-file rule is not Off.
fn apply_component_dead_code_override_rules(
    results: &mut AnalysisResults,
    config: &ResolvedConfig,
) {
    results.unrendered_components.retain(|c| {
        config
            .resolve_rules_for_path(&c.component.path)
            .unrendered_components
            != Severity::Off
    });
    results.unused_component_props.retain(|p| {
        config
            .resolve_rules_for_path(&p.prop.path)
            .unused_component_props
            != Severity::Off
    });
    results.unused_component_emits.retain(|e| {
        config
            .resolve_rules_for_path(&e.emit.path)
            .unused_component_emits
            != Severity::Off
    });
    results.unused_component_inputs.retain(|i| {
        config
            .resolve_rules_for_path(&i.input.path)
            .unused_component_inputs
            != Severity::Off
    });
    results.unused_component_outputs.retain(|o| {
        config
            .resolve_rules_for_path(&o.output.path)
            .unused_component_outputs
            != Severity::Off
    });
    results.unused_svelte_events.retain(|e| {
        config
            .resolve_rules_for_path(&e.event.path)
            .unused_svelte_events
            != Severity::Off
    });
    results.unused_server_actions.retain(|a| {
        config
            .resolve_rules_for_path(&a.action.path)
            .unused_server_actions
            != Severity::Off
    });
    results.unused_load_data_keys.retain(|k| {
        config
            .resolve_rules_for_path(&k.key.path)
            .unused_load_data_keys
            != Severity::Off
    });
}

fn apply_catalog_override_rules(results: &mut AnalysisResults, config: &ResolvedConfig) {
    results.stale_suppressions.retain(|s| {
        let rules = config.resolve_rules_for_path(&s.path);
        if s.missing_reason {
            rules.require_suppression_reason != Severity::Off
        } else {
            rules.stale_suppressions != Severity::Off
        }
    });
    results.unresolved_catalog_references.retain(|r| {
        config
            .resolve_rules_for_path(&r.reference.path)
            .unresolved_catalog_references
            != Severity::Off
    });
    results.empty_catalog_groups.retain(|g| {
        config
            .resolve_rules_for_path(&g.group.path)
            .empty_catalog_groups
            != Severity::Off
    });
    results.unused_dependency_overrides.retain(|o| {
        config
            .resolve_rules_for_path(&o.entry.path)
            .unused_dependency_overrides
            != Severity::Off
    });
    results.misconfigured_dependency_overrides.retain(|o| {
        config
            .resolve_rules_for_path(&o.entry.path)
            .misconfigured_dependency_overrides
            != Severity::Off
    });
}

fn apply_framework_override_rules(results: &mut AnalysisResults, config: &ResolvedConfig) {
    results.invalid_client_exports.retain(|e| {
        config
            .resolve_rules_for_path(&e.export.path)
            .invalid_client_export
            != Severity::Off
    });
    results.mixed_client_server_barrels.retain(|b| {
        config
            .resolve_rules_for_path(&b.barrel.path)
            .mixed_client_server_barrel
            != Severity::Off
    });
    results.misplaced_directives.retain(|d| {
        config
            .resolve_rules_for_path(&d.directive_site.path)
            .misplaced_directive
            != Severity::Off
    });
    results.route_collisions.retain(|c| {
        config
            .resolve_rules_for_path(&c.collision.path)
            .route_collision
            != Severity::Off
    });
    results.dynamic_segment_name_conflicts.retain(|c| {
        config
            .resolve_rules_for_path(&c.conflict.path)
            .dynamic_segment_name_conflict
            != Severity::Off
    });
}

fn apply_circular_override_rules(results: &mut AnalysisResults, config: &ResolvedConfig) {
    results.circular_dependencies.retain(|c| {
        c.cycle
            .files
            .iter()
            .any(|path| config.resolve_rules_for_path(path).circular_dependencies != Severity::Off)
    });
}

fn apply_base_file_rules(results: &mut AnalysisResults, rules: &RulesConfig) {
    clear_base_core_dead_code(results, rules);
    clear_base_component_dead_code(results, rules);
    clear_base_suppression_and_framework(results, rules);
}

/// Clear core (non-component) dead-code findings whose base rule is Off.
fn clear_base_core_dead_code(results: &mut AnalysisResults, rules: &RulesConfig) {
    if rules.unused_files == Severity::Off {
        results.unused_files.clear();
    }
    if rules.unused_exports == Severity::Off {
        results.unused_exports.clear();
    }
    if rules.unused_types == Severity::Off {
        results.unused_types.clear();
    }
    if rules.private_type_leaks == Severity::Off {
        results.private_type_leaks.clear();
    }
    if rules.unused_enum_members == Severity::Off {
        results.unused_enum_members.clear();
    }
    if rules.unused_class_members == Severity::Off {
        results.unused_class_members.clear();
    }
    if rules.unused_store_members == Severity::Off {
        results.unused_store_members.clear();
    }
    if rules.unprovided_injects == Severity::Off {
        results.unprovided_injects.clear();
    }
    if rules.unresolved_imports == Severity::Off {
        results.unresolved_imports.clear();
    }
}

/// Clear component-shaped dead-code findings whose base rule is Off.
fn clear_base_component_dead_code(results: &mut AnalysisResults, rules: &RulesConfig) {
    if rules.unrendered_components == Severity::Off {
        results.unrendered_components.clear();
    }
    if rules.unused_component_props == Severity::Off {
        results.unused_component_props.clear();
    }
    if rules.unused_component_emits == Severity::Off {
        results.unused_component_emits.clear();
    }
    if rules.unused_component_inputs == Severity::Off {
        results.unused_component_inputs.clear();
    }
    if rules.unused_component_outputs == Severity::Off {
        results.unused_component_outputs.clear();
    }
    if rules.unused_svelte_events == Severity::Off {
        results.unused_svelte_events.clear();
    }
    if rules.unused_server_actions == Severity::Off {
        results.unused_server_actions.clear();
    }
    if rules.unused_load_data_keys == Severity::Off {
        results.unused_load_data_keys.clear();
    }
}

/// Apply base stale-suppression retention and clear framework findings whose
/// base rule is Off.
fn clear_base_suppression_and_framework(results: &mut AnalysisResults, rules: &RulesConfig) {
    results.stale_suppressions.retain(|s| {
        if s.missing_reason {
            rules.require_suppression_reason != Severity::Off
        } else {
            rules.stale_suppressions != Severity::Off
        }
    });
    if rules.invalid_client_export == Severity::Off {
        results.invalid_client_exports.clear();
    }
    if rules.mixed_client_server_barrel == Severity::Off {
        results.mixed_client_server_barrels.clear();
    }
    if rules.misplaced_directive == Severity::Off {
        results.misplaced_directives.clear();
    }
    if rules.route_collision == Severity::Off {
        results.route_collisions.clear();
    }
    if rules.dynamic_segment_name_conflict == Severity::Off {
        results.dynamic_segment_name_conflicts.clear();
    }
}

fn apply_boundary_override_rules(results: &mut AnalysisResults, config: &ResolvedConfig) {
    results.boundary_violations.retain(|v| {
        config
            .resolve_rules_for_path(&v.violation.from_path)
            .boundary_violation
            != Severity::Off
    });
    results.boundary_coverage_violations.retain(|v| {
        config
            .resolve_rules_for_path(&v.violation.path)
            .boundary_violation
            != Severity::Off
    });
    results.boundary_call_violations.retain(|v| {
        config
            .resolve_rules_for_path(&v.violation.path)
            .boundary_violation
            != Severity::Off
    });
    results.policy_violations.retain(|v| {
        config
            .resolve_rules_for_path(&v.violation.path)
            .policy_violation
            != Severity::Off
    });
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use fallow_types::output_dead_code::{
        BoundaryViolationFinding, CircularDependencyFinding, PrivateTypeLeakFinding,
        UnusedExportFinding, UnusedFileFinding,
    };
    use fallow_types::results::{
        BoundaryViolation, CircularDependency, PrivateTypeLeak, UnusedExport, UnusedFile,
    };

    #[test]
    fn workspace_filter_keeps_findings_under_workspace_root() {
        let root = PathBuf::from("/repo/packages/app");
        let mut results = AnalysisResults::default();
        results
            .unused_files
            .push(UnusedFileFinding::with_actions(UnusedFile {
                path: root.join("src/unused.ts"),
            }));
        results
            .unused_files
            .push(UnusedFileFinding::with_actions(UnusedFile {
                path: PathBuf::from("/repo/packages/docs/src/unused.ts"),
            }));

        filter_to_workspaces(&mut results, std::slice::from_ref(&root));

        assert_eq!(results.unused_files.len(), 1);
        assert_eq!(
            results.unused_files[0].file.path,
            root.join("src/unused.ts")
        );
    }

    #[test]
    fn configured_filter_removes_findings_added_after_engine_analysis() {
        let project = tempfile::tempdir().expect("project");
        let config = serde_json::from_str::<fallow_config::FallowConfig>(
            r#"{"ignoreFindings":["src/hidden.ts"]}"#,
        )
        .expect("config parses")
        .resolve(
            project.path().to_path_buf(),
            fallow_config::OutputFormat::Human,
            1,
            true,
            true,
            None,
        );
        let mut results = AnalysisResults::default();
        results
            .private_type_leaks
            .push(PrivateTypeLeakFinding::with_actions(PrivateTypeLeak {
                path: project.path().join("src/hidden.ts"),
                export_name: "publicApi".to_string(),
                type_name: "PrivateShape".to_string(),
                line: 1,
                col: 0,
                span_start: 0,
                semantic: None,
            }));
        results
            .boundary_violations
            .push(BoundaryViolationFinding::with_actions(BoundaryViolation {
                from_path: project.path().join("src/hidden.ts"),
                to_path: project.path().join("src/data.ts"),
                from_zone: "ui".to_string(),
                to_zone: "data".to_string(),
                import_specifier: "./data".to_string(),
                line: 1,
                col: 0,
            }));

        filter_configured_ignored_findings(&mut results, &config);

        assert!(results.private_type_leaks.is_empty());
        assert_eq!(results.boundary_violations.len(), 1);
    }

    fn config_with_override(
        pattern: &str,
        configure: impl FnOnce(&mut fallow_config::PartialRulesConfig),
    ) -> ResolvedConfig {
        let mut partial = fallow_config::PartialRulesConfig::default();
        configure(&mut partial);
        fallow_config::FallowConfig {
            rules: RulesConfig {
                private_type_leaks: Severity::Warn,
                ..RulesConfig::default()
            },
            overrides: vec![fallow_config::ConfigOverride {
                files: vec![pattern.to_string()],
                rules: partial,
            }],
            ..fallow_config::FallowConfig::default()
        }
        .resolve(
            PathBuf::from("/project"),
            fallow_config::OutputFormat::Human,
            1,
            true,
            true,
            None,
        )
    }

    fn unused_export(path: &str) -> UnusedExportFinding {
        UnusedExportFinding::with_actions(UnusedExport {
            path: PathBuf::from(path),
            export_name: "Unused".to_string(),
            is_type_only: false,
            line: 1,
            col: 0,
            span_start: 0,
            is_re_export: false,
        })
    }

    fn private_type_leak(path: &str) -> PrivateTypeLeakFinding {
        PrivateTypeLeakFinding::with_actions(PrivateTypeLeak {
            path: PathBuf::from(path),
            export_name: "Unused".to_string(),
            type_name: "Props".to_string(),
            line: 1,
            col: 0,
            span_start: 0,
            semantic: None,
        })
    }

    fn overridden_fixture() -> AnalysisResults {
        let mut results = AnalysisResults::default();
        results
            .unused_exports
            .push(unused_export("/project/src/ui/kit.ts"));
        results
            .unused_exports
            .push(unused_export("/project/src/lib/util.ts"));
        results
            .private_type_leaks
            .push(private_type_leak("/project/src/ui/kit.ts"));
        results
            .private_type_leaks
            .push(private_type_leak("/project/src/lib/util.ts"));
        results
    }

    #[test]
    fn rule_severities_drop_findings_only_on_overridden_paths() {
        let config = config_with_override("src/ui/**", |rules| {
            rules.unused_exports = Some(Severity::Off);
            rules.private_type_leaks = Some(Severity::Off);
        });
        let mut results = overridden_fixture();

        apply_rule_severities(&mut results, &config);

        assert_eq!(
            results
                .unused_exports
                .iter()
                .map(|finding| finding.export.path.clone())
                .collect::<Vec<_>>(),
            vec![PathBuf::from("/project/src/lib/util.ts")]
        );
        assert_eq!(
            results
                .private_type_leaks
                .iter()
                .map(|finding| finding.leak.path.clone())
                .collect::<Vec<_>>(),
            vec![PathBuf::from("/project/src/lib/util.ts")]
        );
    }

    #[test]
    fn rule_severities_are_idempotent() {
        // The editor path resolves severities once after analysis and again
        // after type-aware reconciliation, so a second pass must not change
        // the result set.
        let config = config_with_override("src/ui/**", |rules| {
            rules.unused_exports = Some(Severity::Off);
            rules.private_type_leaks = Some(Severity::Off);
        });

        let mut once = overridden_fixture();
        apply_rule_severities(&mut once, &config);
        let mut twice = overridden_fixture();
        apply_rule_severities(&mut twice, &config);
        apply_rule_severities(&mut twice, &config);

        assert_eq!(
            once.unused_exports
                .iter()
                .map(|finding| finding.export.path.clone())
                .collect::<Vec<_>>(),
            twice
                .unused_exports
                .iter()
                .map(|finding| finding.export.path.clone())
                .collect::<Vec<_>>()
        );
        assert_eq!(
            once.private_type_leaks
                .iter()
                .map(|finding| finding.leak.path.clone())
                .collect::<Vec<_>>(),
            twice
                .private_type_leaks
                .iter()
                .map(|finding| finding.leak.path.clone())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn rule_severities_keep_a_cycle_when_any_member_file_stays_enabled() {
        let config = config_with_override("src/ui/**", |rules| {
            rules.circular_dependencies = Some(Severity::Off);
        });
        let mut results = AnalysisResults::default();
        results
            .circular_dependencies
            .push(CircularDependencyFinding::with_actions(
                CircularDependency {
                    files: vec![
                        PathBuf::from("/project/src/ui/a.ts"),
                        PathBuf::from("/project/src/lib/b.ts"),
                    ],
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
                    files: vec![
                        PathBuf::from("/project/src/ui/c.ts"),
                        PathBuf::from("/project/src/ui/d.ts"),
                    ],
                    length: 2,
                    line: 1,
                    col: 0,
                    edges: Vec::new(),
                    is_cross_package: false,
                },
            ));

        apply_rule_severities(&mut results, &config);

        assert_eq!(results.circular_dependencies.len(), 1);
        assert_eq!(
            results.circular_dependencies[0].cycle.files[0],
            PathBuf::from("/project/src/ui/a.ts")
        );
    }

    #[test]
    fn rule_severities_clear_base_rules_without_overrides() {
        let config = fallow_config::FallowConfig {
            rules: RulesConfig {
                unused_exports: Severity::Off,
                private_type_leaks: Severity::Warn,
                ..RulesConfig::default()
            },
            ..fallow_config::FallowConfig::default()
        }
        .resolve(
            PathBuf::from("/project"),
            fallow_config::OutputFormat::Human,
            1,
            true,
            true,
            None,
        );
        let mut results = overridden_fixture();

        apply_rule_severities(&mut results, &config);

        assert!(results.unused_exports.is_empty());
        assert_eq!(results.private_type_leaks.len(), 2);
    }
}
