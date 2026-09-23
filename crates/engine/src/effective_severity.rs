//! Per-finding gate severity for dead-code results.
//!
//! The exit-code check in the CLI decides whether a run fails. CI formats
//! (SARIF, CodeClimate, GitHub annotations) state a level for each finding.
//! Both must agree, so this module writes the severity that the exit-code
//! check uses onto each finding one time, after rule resolution. The rules
//! here mirror `has_error_severity_issues` in `crates/engine/src/error_severity.rs`:
//!
//! - a file-scoped finding resolves `overrides[].rules` for its own path;
//! - a circular dependency is `error` when any file in the cycle resolves to
//!   `error`;
//! - a project-level finding (dependencies, catalog entries, duplicate
//!   exports, re-export cycles) uses the base rules.
//!
//! Empty catalog groups and dependency overrides are file-scoped: they sit on
//! the file that declares them (`pnpm-workspace.yaml` or a `package.json`), so
//! an override for that file decides. The audit ledger in
//! `crates/api/src/audit_keys.rs` resolves them the same way.
//!
//! Policy violations carry their own `severity`. Prop-drilling, thin-wrapper
//! and duplicate-prop-shape records are health signals that never gate the
//! run, so they carry no gate severity.

use std::path::Path;

use fallow_config::{ResolvedConfig, RulesConfig, Severity};
use fallow_types::output_dead_code::{EffectiveSeverity, GatedFinding};
use fallow_types::results::AnalysisResults;

fn gate(severity: Severity) -> Option<EffectiveSeverity> {
    match severity {
        Severity::Error => Some(EffectiveSeverity::Error),
        Severity::Warn => Some(EffectiveSeverity::Warn),
        Severity::Off => None,
    }
}

/// Resolves the rules for one path, with a fast path when no override exists.
struct PathRules<'a> {
    config: &'a ResolvedConfig,
}

impl PathRules<'_> {
    fn severity(&self, path: &Path, rule: fn(&RulesConfig) -> Severity) -> Severity {
        if self.config.overrides.is_empty() {
            rule(&self.config.rules)
        } else {
            rule(&self.config.resolve_rules_for_path(path))
        }
    }

    fn stamp<T: GatedFinding>(
        &self,
        findings: &mut [T],
        path: fn(&T) -> &Path,
        rule: fn(&RulesConfig) -> Severity,
    ) {
        for finding in findings {
            let severity = self.severity(path(finding), rule);
            finding.set_effective_severity(gate(severity));
        }
    }
}

fn stamp_base<T: GatedFinding>(findings: &mut [T], severity: Severity) {
    let severity = gate(severity);
    for finding in findings {
        finding.set_effective_severity(severity);
    }
}

/// Write the gate severity onto each dead-code finding in `results`.
///
/// Call this after the findings whose rule is `off` are removed. The function
/// overwrites any earlier value, so a second call with the same config gives
/// the same result.
///
/// The destructure has no `..`, so a new field on [`AnalysisResults`] fails to
/// compile here until it is stamped or listed as not gated.
#[expect(
    clippy::too_many_lines,
    reason = "one exhaustive list of finding collections; splitting it would lose the compile-time guard"
)]
pub fn apply_effective_severities(results: &mut AnalysisResults, config: &ResolvedConfig) {
    let rules = PathRules { config };
    let base = &config.rules;
    let AnalysisResults {
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
        dev_dependencies_in_production,
        circular_dependencies,
        re_export_cycles,
        boundary_violations,
        boundary_coverage_violations,
        boundary_call_violations,
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
        route_collisions,
        dynamic_segment_name_conflicts,
        unused_component_props,
        unused_component_emits,
        unused_component_inputs,
        unused_component_outputs,
        unused_svelte_events,
        unused_server_actions,
        unused_load_data_keys,
        // Policy violations carry their own evaluated `severity`.
        policy_violations: _,
        // Health signals that never gate the run.
        prop_drilling_chains: _,
        thin_wrappers: _,
        duplicate_prop_shapes: _,
        // Not findings: counts, flags and metadata.
        unused_load_data_keys_global_abstain: _,
        suppression_count: _,
        unused_component_props_exempted: _,
        active_suppressions: _,
        feature_flags: _,
        export_usages: _,
        entry_point_summary: _,
        render_fan_in: _,
        react_component_intel: _,
        semantic_framework_contracts: _,
        // Security findings belong to `fallow security` and its own gate.
        security_findings: _,
        security_unresolved_edge_files: _,
        security_unresolved_callee_sites: _,
        security_unresolved_callee_diagnostics: _,
    } = results;

    rules.stamp(unused_files, |f| &f.file.path, |r| r.unused_files);
    rules.stamp(unused_exports, |f| &f.export.path, |r| r.unused_exports);
    rules.stamp(unused_types, |f| &f.export.path, |r| r.unused_types);
    rules.stamp(
        private_type_leaks,
        |f| &f.leak.path,
        |r| r.private_type_leaks,
    );
    rules.stamp(
        unused_enum_members,
        |f| &f.member.path,
        |r| r.unused_enum_members,
    );
    rules.stamp(
        unused_class_members,
        |f| &f.member.path,
        |r| r.unused_class_members,
    );
    rules.stamp(
        unused_store_members,
        |f| &f.member.path,
        |r| r.unused_store_members,
    );
    rules.stamp(
        unprovided_injects,
        |f| &f.inject.path,
        |r| r.unprovided_injects,
    );
    rules.stamp(
        unresolved_imports,
        |f| &f.import.path,
        |r| r.unresolved_imports,
    );

    rules.stamp(
        unrendered_components,
        |f| &f.component.path,
        |r| r.unrendered_components,
    );
    rules.stamp(
        unused_component_props,
        |f| &f.prop.path,
        |r| r.unused_component_props,
    );
    rules.stamp(
        unused_component_emits,
        |f| &f.emit.path,
        |r| r.unused_component_emits,
    );
    rules.stamp(
        unused_component_inputs,
        |f| &f.input.path,
        |r| r.unused_component_inputs,
    );
    rules.stamp(
        unused_component_outputs,
        |f| &f.output.path,
        |r| r.unused_component_outputs,
    );
    rules.stamp(
        unused_svelte_events,
        |f| &f.event.path,
        |r| r.unused_svelte_events,
    );
    rules.stamp(
        unused_server_actions,
        |f| &f.action.path,
        |r| r.unused_server_actions,
    );
    rules.stamp(
        unused_load_data_keys,
        |f| &f.key.path,
        |r| r.unused_load_data_keys,
    );

    rules.stamp(
        invalid_client_exports,
        |f| &f.export.path,
        |r| r.invalid_client_export,
    );
    rules.stamp(
        mixed_client_server_barrels,
        |f| &f.barrel.path,
        |r| r.mixed_client_server_barrel,
    );
    rules.stamp(
        misplaced_directives,
        |f| &f.directive_site.path,
        |r| r.misplaced_directive,
    );
    rules.stamp(
        route_collisions,
        |f| &f.collision.path,
        |r| r.route_collision,
    );
    rules.stamp(
        dynamic_segment_name_conflicts,
        |f| &f.conflict.path,
        |r| r.dynamic_segment_name_conflict,
    );

    rules.stamp(
        boundary_violations,
        |f| &f.violation.from_path,
        |r| r.boundary_violation,
    );
    rules.stamp(
        boundary_coverage_violations,
        |f| &f.violation.path,
        |r| r.boundary_violation,
    );
    rules.stamp(
        boundary_call_violations,
        |f| &f.violation.path,
        |r| r.boundary_violation,
    );
    for finding in circular_dependencies {
        let any_error = finding
            .cycle
            .files
            .iter()
            .any(|path| rules.severity(path, |r| r.circular_dependencies) == Severity::Error);
        finding.set_effective_severity(Some(if any_error {
            EffectiveSeverity::Error
        } else {
            EffectiveSeverity::Warn
        }));
    }

    for finding in stale_suppressions {
        let severity = if finding.missing_reason {
            rules.severity(&finding.path, |r| r.require_suppression_reason)
        } else {
            rules.severity(&finding.path, |r| r.stale_suppressions)
        };
        finding.set_effective_severity(gate(severity));
    }
    rules.stamp(
        unresolved_catalog_references,
        |f| &f.reference.path,
        |r| r.unresolved_catalog_references,
    );
    rules.stamp(
        empty_catalog_groups,
        |f| &f.group.path,
        |r| r.empty_catalog_groups,
    );
    rules.stamp(
        unused_dependency_overrides,
        |f| &f.entry.path,
        |r| r.unused_dependency_overrides,
    );
    rules.stamp(
        misconfigured_dependency_overrides,
        |f| &f.entry.path,
        |r| r.misconfigured_dependency_overrides,
    );

    stamp_base(unused_dependencies, base.unused_dependencies);
    stamp_base(unused_dev_dependencies, base.unused_dev_dependencies);
    stamp_base(
        unused_optional_dependencies,
        base.unused_optional_dependencies,
    );
    stamp_base(unlisted_dependencies, base.unlisted_dependencies);
    stamp_base(duplicate_exports, base.duplicate_exports);
    stamp_base(type_only_dependencies, base.type_only_dependencies);
    stamp_base(test_only_dependencies, base.test_only_dependencies);
    stamp_base(
        dev_dependencies_in_production,
        base.dev_dependencies_in_production,
    );
    stamp_base(re_export_cycles, base.re_export_cycle);
    stamp_base(unused_catalog_entries, base.unused_catalog_entries);
}

/// Raise every `warn` gate severity to `error`, for `--fail-on-issues`.
///
/// Under that flag every reported finding fails the run, so every CI format
/// must state `error` too.
pub fn promote_effective_warns(results: &mut AnalysisResults) {
    for_each_gated_finding(results, &mut |finding| {
        if finding.effective_severity() == Some(EffectiveSeverity::Warn) {
            finding.set_effective_severity(Some(EffectiveSeverity::Error));
        }
    });
}

fn visit<T: GatedFinding>(findings: &mut [T], f: &mut dyn FnMut(&mut dyn GatedFinding)) {
    for finding in findings {
        f(finding);
    }
}

/// Visit every finding that carries a gate severity.
///
/// Exhaustive like [`apply_effective_severities`]: a new field on
/// [`AnalysisResults`] fails to compile here until it is listed.
#[expect(
    clippy::too_many_lines,
    reason = "one exhaustive list of finding collections; splitting it would lose the compile-time guard"
)]
fn for_each_gated_finding(results: &mut AnalysisResults, f: &mut dyn FnMut(&mut dyn GatedFinding)) {
    let AnalysisResults {
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
        dev_dependencies_in_production,
        circular_dependencies,
        re_export_cycles,
        boundary_violations,
        boundary_coverage_violations,
        boundary_call_violations,
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
        route_collisions,
        dynamic_segment_name_conflicts,
        unused_component_props,
        unused_component_emits,
        unused_component_inputs,
        unused_component_outputs,
        unused_svelte_events,
        unused_server_actions,
        unused_load_data_keys,
        policy_violations: _,
        prop_drilling_chains: _,
        thin_wrappers: _,
        duplicate_prop_shapes: _,
        unused_load_data_keys_global_abstain: _,
        suppression_count: _,
        unused_component_props_exempted: _,
        active_suppressions: _,
        feature_flags: _,
        export_usages: _,
        entry_point_summary: _,
        render_fan_in: _,
        react_component_intel: _,
        semantic_framework_contracts: _,
        security_findings: _,
        security_unresolved_edge_files: _,
        security_unresolved_callee_sites: _,
        security_unresolved_callee_diagnostics: _,
    } = results;
    visit(unused_files, f);
    visit(unused_exports, f);
    visit(unused_types, f);
    visit(private_type_leaks, f);
    visit(unused_dependencies, f);
    visit(unused_dev_dependencies, f);
    visit(unused_optional_dependencies, f);
    visit(unused_enum_members, f);
    visit(unused_class_members, f);
    visit(unused_store_members, f);
    visit(unresolved_imports, f);
    visit(unlisted_dependencies, f);
    visit(duplicate_exports, f);
    visit(type_only_dependencies, f);
    visit(test_only_dependencies, f);
    visit(dev_dependencies_in_production, f);
    visit(circular_dependencies, f);
    visit(re_export_cycles, f);
    visit(boundary_violations, f);
    visit(boundary_coverage_violations, f);
    visit(boundary_call_violations, f);
    visit(stale_suppressions, f);
    visit(unused_catalog_entries, f);
    visit(empty_catalog_groups, f);
    visit(unresolved_catalog_references, f);
    visit(unused_dependency_overrides, f);
    visit(misconfigured_dependency_overrides, f);
    visit(invalid_client_exports, f);
    visit(mixed_client_server_barrels, f);
    visit(misplaced_directives, f);
    visit(unprovided_injects, f);
    visit(unrendered_components, f);
    visit(route_collisions, f);
    visit(dynamic_segment_name_conflicts, f);
    visit(unused_component_props, f);
    visit(unused_component_emits, f);
    visit(unused_component_inputs, f);
    visit(unused_component_outputs, f);
    visit(unused_svelte_events, f);
    visit(unused_server_actions, f);
    visit(unused_load_data_keys, f);
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use fallow_types::output_dead_code::{
        CircularDependencyFinding, MisconfiguredDependencyOverrideFinding, UnusedDependencyFinding,
        UnusedExportFinding,
    };
    use fallow_types::results::StaleSuppression;
    use serde_json::json;

    use super::*;

    const ROOT: &str = "/project";

    fn config(json: &str) -> ResolvedConfig {
        serde_json::from_str::<fallow_config::FallowConfig>(json)
            .expect("config parses")
            .resolve(
                PathBuf::from(ROOT),
                fallow_config::OutputFormat::Human,
                1,
                true,
                true,
                None,
            )
    }

    fn legacy_warn_config() -> ResolvedConfig {
        config(
            r#"{
                "rules": {
                    "unused-exports": "error",
                    "circular-dependencies": "error",
                    "unused-dependencies": "error",
                    "stale-suppressions": "warn",
                    "require-suppression-reason": "error"
                },
                "overrides": [{
                    "files": ["src/legacy/**", "package.json"],
                    "rules": {
                        "unused-exports": "warn",
                        "circular-dependencies": "warn",
                        "unused-dependencies": "warn",
                        "require-suppression-reason": "warn"
                    }
                }]
            }"#,
        )
    }

    fn export(path: &str) -> UnusedExportFinding {
        serde_json::from_value(json!({
            "path": format!("{ROOT}/{path}"),
            "export_name": "unused",
            "is_type_only": false,
            "line": 1,
            "col": 0,
            "span_start": 0,
            "is_re_export": false,
            "actions": [],
        }))
        .expect("export finding")
    }

    fn cycle(files: &[&str]) -> CircularDependencyFinding {
        serde_json::from_value(json!({
            "files": files.iter().map(|file| format!("{ROOT}/{file}")).collect::<Vec<_>>(),
            "length": files.len(),
            "line": 1,
            "col": 0,
            "actions": [],
        }))
        .expect("cycle finding")
    }

    fn stale(path: &str, missing_reason: bool) -> StaleSuppression {
        serde_json::from_value(json!({
            "path": format!("{ROOT}/{path}"),
            "line": 1,
            "col": 0,
            "origin": { "type": "comment", "is_file_level": false },
            "missing_reason": missing_reason,
            "actions": [],
        }))
        .expect("stale suppression")
    }

    #[test]
    fn file_scoped_findings_follow_the_override_for_their_path() {
        let mut results = AnalysisResults::default();
        results.unused_exports.push(export("src/app.ts"));
        results.unused_exports.push(export("src/legacy/old.ts"));

        apply_effective_severities(&mut results, &legacy_warn_config());

        let severities: Vec<_> = results
            .unused_exports
            .iter()
            .map(|finding| finding.effective_severity)
            .collect();
        assert_eq!(
            severities,
            vec![
                Some(EffectiveSeverity::Error),
                Some(EffectiveSeverity::Warn)
            ]
        );
    }

    #[test]
    fn a_cycle_is_error_when_any_file_in_it_resolves_to_error() {
        let mut results = AnalysisResults::default();
        results
            .circular_dependencies
            .push(cycle(&["src/legacy/a.ts", "src/b.ts"]));
        results
            .circular_dependencies
            .push(cycle(&["src/legacy/a.ts", "src/legacy/b.ts"]));

        apply_effective_severities(&mut results, &legacy_warn_config());

        assert_eq!(
            results.circular_dependencies[0].effective_severity,
            Some(EffectiveSeverity::Error)
        );
        assert_eq!(
            results.circular_dependencies[1].effective_severity,
            Some(EffectiveSeverity::Warn)
        );
    }

    #[test]
    fn project_level_findings_use_the_base_rules() {
        let mut results = AnalysisResults::default();
        results.unused_dependencies.push(
            serde_json::from_value::<UnusedDependencyFinding>(json!({
                "package_name": "left-pad",
                "location": "dependencies",
                "path": format!("{ROOT}/package.json"),
                "line": 3,
                "actions": [],
            }))
            .expect("dependency finding"),
        );

        apply_effective_severities(&mut results, &legacy_warn_config());

        assert_eq!(
            results.unused_dependencies[0].effective_severity,
            Some(EffectiveSeverity::Error)
        );
    }

    #[test]
    fn a_dependency_override_follows_the_override_for_its_file() {
        let config = config(
            r#"{
                "rules": { "misconfigured-dependency-overrides": "error" },
                "overrides": [{
                    "files": ["pnpm-workspace.yaml"],
                    "rules": { "misconfigured-dependency-overrides": "warn" }
                }]
            }"#,
        );
        let mut results = AnalysisResults::default();
        for file in ["pnpm-workspace.yaml", "package.json"] {
            results.misconfigured_dependency_overrides.push(
                serde_json::from_value::<MisconfiguredDependencyOverrideFinding>(json!({
                    "raw_key": "",
                    "raw_value": "^1.0.0",
                    "reason": "empty-value",
                    "source": file,
                    "path": format!("{ROOT}/{file}"),
                    "line": 2,
                    "actions": [],
                }))
                .expect("override finding"),
            );
        }

        apply_effective_severities(&mut results, &config);

        let severities: Vec<_> = results
            .misconfigured_dependency_overrides
            .iter()
            .map(|finding| finding.effective_severity)
            .collect();
        assert_eq!(
            severities,
            vec![
                Some(EffectiveSeverity::Warn),
                Some(EffectiveSeverity::Error)
            ]
        );
    }

    #[test]
    fn a_stale_suppression_reads_the_rule_for_its_kind() {
        let mut results = AnalysisResults::default();
        results.stale_suppressions.push(stale("src/app.ts", false));
        results.stale_suppressions.push(stale("src/app.ts", true));
        results
            .stale_suppressions
            .push(stale("src/legacy/old.ts", true));

        apply_effective_severities(&mut results, &legacy_warn_config());

        let severities: Vec<_> = results
            .stale_suppressions
            .iter()
            .map(|finding| finding.effective_severity)
            .collect();
        assert_eq!(
            severities,
            vec![
                Some(EffectiveSeverity::Warn),
                Some(EffectiveSeverity::Error),
                Some(EffectiveSeverity::Warn),
            ]
        );
    }

    #[test]
    fn fail_on_issues_promotion_raises_warn_and_keeps_error() {
        let mut results = AnalysisResults::default();
        results.unused_exports.push(export("src/app.ts"));
        results.unused_exports.push(export("src/legacy/old.ts"));
        apply_effective_severities(&mut results, &legacy_warn_config());

        promote_effective_warns(&mut results);

        assert!(
            results
                .unused_exports
                .iter()
                .all(|finding| finding.effective_severity == Some(EffectiveSeverity::Error))
        );
    }
}
