//! Per-finding gate severity for dead-code results.
//!
//! The exit-code check in the CLI decides whether a run fails. CI formats
//! (SARIF, CodeClimate, GitHub annotations) state a level for each finding.
//! Both must agree, so this module writes the severity that the exit-code
//! check uses onto each finding one time, after rule resolution. The rules
//! here mirror `has_error_severity_issues` in `crates/cli/src/check/rules.rs`:
//!
//! - a file-scoped finding resolves `overrides[].rules` for its own path;
//! - a circular dependency is `error` when any file in the cycle resolves to
//!   `error`;
//! - a project-level finding (dependencies, catalog entries, dependency
//!   overrides, duplicate exports, re-export cycles) uses the base rules;
//! - an empty catalog group is `error` when the base rule or the rule for its
//!   path is `error`.
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
pub fn apply_effective_severities(results: &mut AnalysisResults, config: &ResolvedConfig) {
    let rules = PathRules { config };
    stamp_core_findings(results, &rules);
    stamp_component_findings(results, &rules);
    stamp_framework_findings(results, &rules);
    stamp_graph_findings(results, &rules);
    stamp_suppression_and_catalog_findings(results, &rules);
    stamp_project_findings(results, &config.rules);
}

fn stamp_core_findings(results: &mut AnalysisResults, rules: &PathRules<'_>) {
    rules.stamp(
        &mut results.unused_files,
        |f| &f.file.path,
        |r| r.unused_files,
    );
    rules.stamp(
        &mut results.unused_exports,
        |f| &f.export.path,
        |r| r.unused_exports,
    );
    rules.stamp(
        &mut results.unused_types,
        |f| &f.export.path,
        |r| r.unused_types,
    );
    rules.stamp(
        &mut results.private_type_leaks,
        |f| &f.leak.path,
        |r| r.private_type_leaks,
    );
    rules.stamp(
        &mut results.unused_enum_members,
        |f| &f.member.path,
        |r| r.unused_enum_members,
    );
    rules.stamp(
        &mut results.unused_class_members,
        |f| &f.member.path,
        |r| r.unused_class_members,
    );
    rules.stamp(
        &mut results.unused_store_members,
        |f| &f.member.path,
        |r| r.unused_store_members,
    );
    rules.stamp(
        &mut results.unprovided_injects,
        |f| &f.inject.path,
        |r| r.unprovided_injects,
    );
    rules.stamp(
        &mut results.unresolved_imports,
        |f| &f.import.path,
        |r| r.unresolved_imports,
    );
}

fn stamp_component_findings(results: &mut AnalysisResults, rules: &PathRules<'_>) {
    rules.stamp(
        &mut results.unrendered_components,
        |f| &f.component.path,
        |r| r.unrendered_components,
    );
    rules.stamp(
        &mut results.unused_component_props,
        |f| &f.prop.path,
        |r| r.unused_component_props,
    );
    rules.stamp(
        &mut results.unused_component_emits,
        |f| &f.emit.path,
        |r| r.unused_component_emits,
    );
    rules.stamp(
        &mut results.unused_component_inputs,
        |f| &f.input.path,
        |r| r.unused_component_inputs,
    );
    rules.stamp(
        &mut results.unused_component_outputs,
        |f| &f.output.path,
        |r| r.unused_component_outputs,
    );
    rules.stamp(
        &mut results.unused_svelte_events,
        |f| &f.event.path,
        |r| r.unused_svelte_events,
    );
    rules.stamp(
        &mut results.unused_server_actions,
        |f| &f.action.path,
        |r| r.unused_server_actions,
    );
    rules.stamp(
        &mut results.unused_load_data_keys,
        |f| &f.key.path,
        |r| r.unused_load_data_keys,
    );
}

fn stamp_framework_findings(results: &mut AnalysisResults, rules: &PathRules<'_>) {
    rules.stamp(
        &mut results.invalid_client_exports,
        |f| &f.export.path,
        |r| r.invalid_client_export,
    );
    rules.stamp(
        &mut results.mixed_client_server_barrels,
        |f| &f.barrel.path,
        |r| r.mixed_client_server_barrel,
    );
    rules.stamp(
        &mut results.misplaced_directives,
        |f| &f.directive_site.path,
        |r| r.misplaced_directive,
    );
    rules.stamp(
        &mut results.route_collisions,
        |f| &f.collision.path,
        |r| r.route_collision,
    );
    rules.stamp(
        &mut results.dynamic_segment_name_conflicts,
        |f| &f.conflict.path,
        |r| r.dynamic_segment_name_conflict,
    );
}

fn stamp_graph_findings(results: &mut AnalysisResults, rules: &PathRules<'_>) {
    rules.stamp(
        &mut results.boundary_violations,
        |f| &f.violation.from_path,
        |r| r.boundary_violation,
    );
    rules.stamp(
        &mut results.boundary_coverage_violations,
        |f| &f.violation.path,
        |r| r.boundary_violation,
    );
    rules.stamp(
        &mut results.boundary_call_violations,
        |f| &f.violation.path,
        |r| r.boundary_violation,
    );
    for finding in &mut results.circular_dependencies {
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
}

fn stamp_suppression_and_catalog_findings(results: &mut AnalysisResults, rules: &PathRules<'_>) {
    for finding in &mut results.stale_suppressions {
        let severity = if finding.missing_reason {
            rules.severity(&finding.path, |r| r.require_suppression_reason)
        } else {
            rules.severity(&finding.path, |r| r.stale_suppressions)
        };
        finding.set_effective_severity(gate(severity));
    }
    rules.stamp(
        &mut results.unresolved_catalog_references,
        |f| &f.reference.path,
        |r| r.unresolved_catalog_references,
    );
    let base_empty_group = rules.config.rules.empty_catalog_groups;
    for finding in &mut results.empty_catalog_groups {
        let severity = if base_empty_group == Severity::Error {
            Severity::Error
        } else {
            rules.severity(&finding.group.path, |r| r.empty_catalog_groups)
        };
        finding.set_effective_severity(gate(severity));
    }
}

fn stamp_project_findings(results: &mut AnalysisResults, rules: &RulesConfig) {
    stamp_base(&mut results.unused_dependencies, rules.unused_dependencies);
    stamp_base(
        &mut results.unused_dev_dependencies,
        rules.unused_dev_dependencies,
    );
    stamp_base(
        &mut results.unused_optional_dependencies,
        rules.unused_optional_dependencies,
    );
    stamp_base(
        &mut results.unlisted_dependencies,
        rules.unlisted_dependencies,
    );
    stamp_base(&mut results.duplicate_exports, rules.duplicate_exports);
    stamp_base(
        &mut results.type_only_dependencies,
        rules.type_only_dependencies,
    );
    stamp_base(
        &mut results.test_only_dependencies,
        rules.test_only_dependencies,
    );
    stamp_base(
        &mut results.dev_dependencies_in_production,
        rules.dev_dependencies_in_production,
    );
    stamp_base(&mut results.re_export_cycles, rules.re_export_cycle);
    stamp_base(
        &mut results.unused_catalog_entries,
        rules.unused_catalog_entries,
    );
    stamp_base(
        &mut results.unused_dependency_overrides,
        rules.unused_dependency_overrides,
    );
    stamp_base(
        &mut results.misconfigured_dependency_overrides,
        rules.misconfigured_dependency_overrides,
    );
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

fn for_each_gated_finding(results: &mut AnalysisResults, f: &mut dyn FnMut(&mut dyn GatedFinding)) {
    visit(&mut results.unused_files, f);
    visit(&mut results.unused_exports, f);
    visit(&mut results.unused_types, f);
    visit(&mut results.private_type_leaks, f);
    visit(&mut results.unused_dependencies, f);
    visit(&mut results.unused_dev_dependencies, f);
    visit(&mut results.unused_optional_dependencies, f);
    visit(&mut results.unused_enum_members, f);
    visit(&mut results.unused_class_members, f);
    visit(&mut results.unused_store_members, f);
    visit(&mut results.unresolved_imports, f);
    visit(&mut results.unlisted_dependencies, f);
    visit(&mut results.duplicate_exports, f);
    visit(&mut results.type_only_dependencies, f);
    visit(&mut results.test_only_dependencies, f);
    visit(&mut results.dev_dependencies_in_production, f);
    visit(&mut results.circular_dependencies, f);
    visit(&mut results.re_export_cycles, f);
    visit(&mut results.boundary_violations, f);
    visit(&mut results.boundary_coverage_violations, f);
    visit(&mut results.boundary_call_violations, f);
    visit(&mut results.stale_suppressions, f);
    visit(&mut results.unused_catalog_entries, f);
    visit(&mut results.empty_catalog_groups, f);
    visit(&mut results.unresolved_catalog_references, f);
    visit(&mut results.unused_dependency_overrides, f);
    visit(&mut results.misconfigured_dependency_overrides, f);
    visit(&mut results.invalid_client_exports, f);
    visit(&mut results.mixed_client_server_barrels, f);
    visit(&mut results.misplaced_directives, f);
    visit(&mut results.unprovided_injects, f);
    visit(&mut results.unrendered_components, f);
    visit(&mut results.route_collisions, f);
    visit(&mut results.dynamic_segment_name_conflicts, f);
    visit(&mut results.unused_component_props, f);
    visit(&mut results.unused_component_emits, f);
    visit(&mut results.unused_component_inputs, f);
    visit(&mut results.unused_component_outputs, f);
    visit(&mut results.unused_svelte_events, f);
    visit(&mut results.unused_server_actions, f);
    visit(&mut results.unused_load_data_keys, f);
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use fallow_types::output_dead_code::{
        CircularDependencyFinding, UnusedDependencyFinding, UnusedExportFinding,
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
