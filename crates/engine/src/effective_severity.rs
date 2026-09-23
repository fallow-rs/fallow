//! Per-finding rule severity for dead-code results.
//!
//! One table in this module maps each dead-code finding to the rule that
//! decides its severity. Three consumers read it:
//!
//! - [`apply_effective_severities`] writes the severity onto each finding for
//!   the CI formats (SARIF, CodeClimate, GitHub annotations);
//! - `has_error_severity_issues` in `crates/engine/src/error_severity.rs`
//!   decides the exit code, the combined verdict and the audit `all` gate;
//! - the audit ledger in `crates/api/src/audit_keys.rs` decides the audit
//!   `new-only` gate.
//!
//! The rules of the table:
//!
//! - a file-scoped finding resolves `overrides[].rules` for its own path;
//! - a circular dependency takes the highest severity of the files in the
//!   cycle;
//! - a project-level finding (dependencies, catalog entries, duplicate
//!   exports, re-export cycles) uses the base rules.
//!
//! Empty catalog groups and dependency overrides are file-scoped: they sit on
//! the file that declares them (`pnpm-workspace.yaml` or a `package.json`), so
//! an override for that file decides.
//!
//! Policy violations carry their own `severity`. Prop-drilling, thin-wrapper
//! and duplicate-prop-shape records are health signals that never gate the
//! run, so they carry no gate severity.

use std::path::Path;

use fallow_config::{ResolvedConfig, RulesConfig, Severity};
use fallow_types::output_dead_code::{
    BoundaryCallViolationFinding, BoundaryCoverageViolationFinding, BoundaryViolationFinding,
    CircularDependencyFinding, DevDependencyInProductionFinding, DuplicateExportFinding,
    DynamicSegmentNameConflictFinding, EffectiveSeverity, EmptyCatalogGroupFinding, GatedFinding,
    InvalidClientExportFinding, MisconfiguredDependencyOverrideFinding, MisplacedDirectiveFinding,
    MixedClientServerBarrelFinding, PolicyViolationFinding, PrivateTypeLeakFinding,
    ReExportCycleFinding, RouteCollisionFinding, TestOnlyDependencyFinding,
    TypeOnlyDependencyFinding, UnlistedDependencyFinding, UnprovidedInjectFinding,
    UnrenderedComponentFinding, UnresolvedCatalogReferenceFinding, UnresolvedImportFinding,
    UnusedCatalogEntryFinding, UnusedClassMemberFinding, UnusedComponentEmitFinding,
    UnusedComponentInputFinding, UnusedComponentOutputFinding, UnusedComponentPropFinding,
    UnusedDependencyFinding, UnusedDependencyOverrideFinding, UnusedDevDependencyFinding,
    UnusedEnumMemberFinding, UnusedExportFinding, UnusedFileFinding, UnusedLoadDataKeyFinding,
    UnusedOptionalDependencyFinding, UnusedServerActionFinding, UnusedStoreMemberFinding,
    UnusedSvelteEventFinding, UnusedTypeFinding,
};
use fallow_types::results::{AnalysisResults, PolicyViolationSeverity, StaleSuppression};

use crate::error_severity::promote_warns_to_errors;

fn gate(severity: Severity) -> Option<EffectiveSeverity> {
    match severity {
        Severity::Error => Some(EffectiveSeverity::Error),
        Severity::Warn => Some(EffectiveSeverity::Warn),
        Severity::Off => None,
    }
}

/// The rules that give a finding its severity.
#[derive(Clone, Copy)]
pub struct SeveritySource<'a> {
    base: &'a RulesConfig,
    overrides: Option<&'a ResolvedConfig>,
    promote_warns: bool,
}

impl<'a> SeveritySource<'a> {
    /// The rules of `config`, with its `overrides` for file-scoped findings.
    #[must_use]
    pub fn from_config(config: &'a ResolvedConfig) -> Self {
        Self::new(&config.rules, Some(config), false)
    }

    /// Explicit base rules, with the `overrides` of `config` when it has any.
    ///
    /// `promote_warns` raises a `warn` that an override resolves to `error`.
    /// The caller promotes `base` itself.
    #[must_use]
    pub fn new(
        base: &'a RulesConfig,
        config: Option<&'a ResolvedConfig>,
        promote_warns: bool,
    ) -> Self {
        Self {
            base,
            overrides: config.filter(|config| !config.overrides.is_empty()),
            promote_warns,
        }
    }

    fn for_path(&self, path: &Path, rule: fn(&RulesConfig) -> Severity) -> Severity {
        let Some(config) = self.overrides else {
            return rule(self.base);
        };
        let mut rules = config.resolve_rules_for_path(path);
        if self.promote_warns {
            promote_warns_to_errors(&mut rules);
        }
        rule(&rules)
    }

    fn project(&self, rule: fn(&RulesConfig) -> Severity) -> Severity {
        rule(self.base)
    }

    /// The base rule when no `overrides` apply, so every finding of a
    /// file-scoped kind has the same severity.
    fn uniform(&self, rule: fn(&RulesConfig) -> Severity) -> Option<Severity> {
        self.overrides.is_none().then(|| rule(self.base))
    }
}

/// A dead-code finding whose severity comes from the configured rules.
pub trait RuleSeverity {
    /// The severity of this finding under `source`.
    fn rule_severity(&self, source: &SeveritySource<'_>) -> Severity;

    /// The severity that every finding of this kind has under `source`, or
    /// `None` when the severity can differ from finding to finding.
    ///
    /// The exit-code check reads this once per collection instead of once
    /// per finding.
    fn uniform_severity(_source: &SeveritySource<'_>) -> Option<Severity>
    where
        Self: Sized,
    {
        None
    }
}

macro_rules! file_scoped {
    ($($finding:ty => $path:ident . $field:ident, $rule:ident;)+) => {
        $(
            impl RuleSeverity for $finding {
                fn rule_severity(&self, source: &SeveritySource<'_>) -> Severity {
                    source.for_path(&self.$path.$field, |rules| rules.$rule)
                }

                fn uniform_severity(source: &SeveritySource<'_>) -> Option<Severity> {
                    source.uniform(|rules| rules.$rule)
                }
            }
        )+
    };
}

macro_rules! project_level {
    ($($finding:ty => $rule:ident;)+) => {
        $(
            impl RuleSeverity for $finding {
                fn rule_severity(&self, source: &SeveritySource<'_>) -> Severity {
                    source.project(|rules| rules.$rule)
                }

                fn uniform_severity(source: &SeveritySource<'_>) -> Option<Severity> {
                    Some(source.project(|rules| rules.$rule))
                }
            }
        )+
    };
}

file_scoped! {
    UnusedFileFinding => file.path, unused_files;
    UnusedExportFinding => export.path, unused_exports;
    UnusedTypeFinding => export.path, unused_types;
    PrivateTypeLeakFinding => leak.path, private_type_leaks;
    UnusedEnumMemberFinding => member.path, unused_enum_members;
    UnusedClassMemberFinding => member.path, unused_class_members;
    UnusedStoreMemberFinding => member.path, unused_store_members;
    UnprovidedInjectFinding => inject.path, unprovided_injects;
    UnresolvedImportFinding => import.path, unresolved_imports;
    UnrenderedComponentFinding => component.path, unrendered_components;
    UnusedComponentPropFinding => prop.path, unused_component_props;
    UnusedComponentEmitFinding => emit.path, unused_component_emits;
    UnusedComponentInputFinding => input.path, unused_component_inputs;
    UnusedComponentOutputFinding => output.path, unused_component_outputs;
    UnusedSvelteEventFinding => event.path, unused_svelte_events;
    UnusedServerActionFinding => action.path, unused_server_actions;
    UnusedLoadDataKeyFinding => key.path, unused_load_data_keys;
    InvalidClientExportFinding => export.path, invalid_client_export;
    MixedClientServerBarrelFinding => barrel.path, mixed_client_server_barrel;
    MisplacedDirectiveFinding => directive_site.path, misplaced_directive;
    RouteCollisionFinding => collision.path, route_collision;
    DynamicSegmentNameConflictFinding => conflict.path, dynamic_segment_name_conflict;
    BoundaryViolationFinding => violation.from_path, boundary_violation;
    BoundaryCoverageViolationFinding => violation.path, boundary_violation;
    BoundaryCallViolationFinding => violation.path, boundary_violation;
    UnresolvedCatalogReferenceFinding => reference.path, unresolved_catalog_references;
    EmptyCatalogGroupFinding => group.path, empty_catalog_groups;
    UnusedDependencyOverrideFinding => entry.path, unused_dependency_overrides;
    MisconfiguredDependencyOverrideFinding => entry.path, misconfigured_dependency_overrides;
}

project_level! {
    UnusedDependencyFinding => unused_dependencies;
    UnusedDevDependencyFinding => unused_dev_dependencies;
    UnusedOptionalDependencyFinding => unused_optional_dependencies;
    UnlistedDependencyFinding => unlisted_dependencies;
    DuplicateExportFinding => duplicate_exports;
    TypeOnlyDependencyFinding => type_only_dependencies;
    TestOnlyDependencyFinding => test_only_dependencies;
    DevDependencyInProductionFinding => dev_dependencies_in_production;
    ReExportCycleFinding => re_export_cycle;
    UnusedCatalogEntryFinding => unused_catalog_entries;
}

impl RuleSeverity for CircularDependencyFinding {
    fn rule_severity(&self, source: &SeveritySource<'_>) -> Severity {
        self.cycle
            .files
            .iter()
            .map(|path| source.for_path(path, |rules| rules.circular_dependencies))
            .max_by_key(|severity| severity_rank(*severity))
            .unwrap_or_else(|| source.project(|rules| rules.circular_dependencies))
    }

    fn uniform_severity(source: &SeveritySource<'_>) -> Option<Severity> {
        source.uniform(|rules| rules.circular_dependencies)
    }
}

impl RuleSeverity for StaleSuppression {
    fn rule_severity(&self, source: &SeveritySource<'_>) -> Severity {
        if self.missing_reason {
            source.for_path(&self.path, |rules| rules.require_suppression_reason)
        } else {
            source.for_path(&self.path, |rules| rules.stale_suppressions)
        }
    }

    fn uniform_severity(source: &SeveritySource<'_>) -> Option<Severity> {
        let stale = source.uniform(|rules| rules.stale_suppressions)?;
        let missing_reason = source.uniform(|rules| rules.require_suppression_reason)?;
        (stale == missing_reason).then_some(stale)
    }
}

impl RuleSeverity for PolicyViolationFinding {
    fn rule_severity(&self, _source: &SeveritySource<'_>) -> Severity {
        match self.violation.severity {
            PolicyViolationSeverity::Error => Severity::Error,
            PolicyViolationSeverity::Warn => Severity::Warn,
        }
    }
}

const fn severity_rank(severity: Severity) -> u8 {
    match severity {
        Severity::Off => 0,
        Severity::Warn => 1,
        Severity::Error => 2,
    }
}

/// A finding that has a rule severity and carries a gate severity.
trait GatedRuleFinding: GatedFinding + RuleSeverity {}

impl<T: GatedFinding + RuleSeverity> GatedRuleFinding for T {}

/// Write the gate severity onto each dead-code finding in `results`.
///
/// Call this after the findings whose rule is `off` are removed. The function
/// overwrites any earlier value, so a second call with the same config gives
/// the same result.
pub fn apply_effective_severities(results: &mut AnalysisResults, config: &ResolvedConfig) {
    let source = SeveritySource::from_config(config);
    for_each_gated_finding(results, &mut |finding| {
        let severity = finding.rule_severity(&source);
        finding.set_effective_severity(gate(severity));
    });
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

/// Whether any dead-code finding in `results` has `severity` under `source`.
///
/// Policy violations count with their own severity.
#[must_use]
pub fn any_finding_with_severity(
    results: &AnalysisResults,
    source: &SeveritySource<'_>,
    severity: Severity,
) -> bool {
    results
        .policy_violations
        .iter()
        .any(|finding| finding.rule_severity(source) == severity)
        || any_gated_finding(results, source, severity)
}

fn visit<T: GatedRuleFinding>(findings: &mut [T], f: &mut dyn FnMut(&mut dyn GatedRuleFinding)) {
    for finding in findings {
        f(finding);
    }
}

/// Whether any finding in `findings` has `severity` under `source`.
///
/// When every finding of the kind has the same severity, one table lookup
/// answers for the whole collection.
fn any<T: RuleSeverity>(findings: &[T], source: &SeveritySource<'_>, severity: Severity) -> bool {
    if findings.is_empty() {
        return false;
    }
    match T::uniform_severity(source) {
        Some(uniform) => uniform == severity,
        None => findings
            .iter()
            .any(|finding| finding.rule_severity(source) == severity),
    }
}

/// Visit every finding that carries a gate severity.
///
/// The destructure has no `..`, so a new field on [`AnalysisResults`] fails to
/// compile here until it is listed.
#[expect(
    clippy::too_many_lines,
    reason = "one exhaustive list of finding collections; splitting it would lose the compile-time guard"
)]
fn for_each_gated_finding(
    results: &mut AnalysisResults,
    f: &mut dyn FnMut(&mut dyn GatedRuleFinding),
) {
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

/// Whether any finding that carries a gate severity has `severity` under
/// `source`.
///
/// Exhaustive like [`for_each_gated_finding`]: a new field on
/// [`AnalysisResults`] fails to compile here until it is listed.
#[expect(
    clippy::too_many_lines,
    reason = "one exhaustive list of finding collections; splitting it would lose the compile-time guard"
)]
fn any_gated_finding(
    results: &AnalysisResults,
    source: &SeveritySource<'_>,
    severity: Severity,
) -> bool {
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
    any(unused_files, source, severity)
        || any(unused_exports, source, severity)
        || any(unused_types, source, severity)
        || any(private_type_leaks, source, severity)
        || any(unused_dependencies, source, severity)
        || any(unused_dev_dependencies, source, severity)
        || any(unused_optional_dependencies, source, severity)
        || any(unused_enum_members, source, severity)
        || any(unused_class_members, source, severity)
        || any(unused_store_members, source, severity)
        || any(unresolved_imports, source, severity)
        || any(unlisted_dependencies, source, severity)
        || any(duplicate_exports, source, severity)
        || any(type_only_dependencies, source, severity)
        || any(test_only_dependencies, source, severity)
        || any(dev_dependencies_in_production, source, severity)
        || any(circular_dependencies, source, severity)
        || any(re_export_cycles, source, severity)
        || any(boundary_violations, source, severity)
        || any(boundary_coverage_violations, source, severity)
        || any(boundary_call_violations, source, severity)
        || any(stale_suppressions, source, severity)
        || any(unused_catalog_entries, source, severity)
        || any(empty_catalog_groups, source, severity)
        || any(unresolved_catalog_references, source, severity)
        || any(unused_dependency_overrides, source, severity)
        || any(misconfigured_dependency_overrides, source, severity)
        || any(invalid_client_exports, source, severity)
        || any(mixed_client_server_barrels, source, severity)
        || any(misplaced_directives, source, severity)
        || any(unprovided_injects, source, severity)
        || any(unrendered_components, source, severity)
        || any(route_collisions, source, severity)
        || any(dynamic_segment_name_conflicts, source, severity)
        || any(unused_component_props, source, severity)
        || any(unused_component_emits, source, severity)
        || any(unused_component_inputs, source, severity)
        || any(unused_component_outputs, source, severity)
        || any(unused_svelte_events, source, severity)
        || any(unused_server_actions, source, severity)
        || any(unused_load_data_keys, source, severity)
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

    /// `apply_rule_severities` removes such a cycle before it writes the
    /// severities. The table must still agree with itself when a caller
    /// skips that filter.
    #[test]
    fn a_cycle_whose_files_all_resolve_to_off_gets_no_severity() {
        let config = config(
            r#"{
                "rules": { "circular-dependencies": "error" },
                "overrides": [{
                    "files": ["src/legacy/**"],
                    "rules": { "circular-dependencies": "off" }
                }]
            }"#,
        );
        let mut results = AnalysisResults::default();
        results
            .circular_dependencies
            .push(cycle(&["src/legacy/a.ts", "src/legacy/b.ts"]));

        apply_effective_severities(&mut results, &config);

        assert_eq!(results.circular_dependencies[0].effective_severity, None);
        assert!(!crate::error_severity::has_error_severity_issues(
            &results,
            &config.rules,
            Some(&config),
            false
        ));
        // The audit ledger reads this value for each finding.
        assert_eq!(
            results.circular_dependencies[0].rule_severity(&SeveritySource::from_config(&config)),
            Severity::Off
        );
    }

    #[test]
    fn the_collection_check_agrees_with_the_per_finding_check_without_overrides() {
        let configs = [
            r#"{ "rules": { "unused-exports": "error", "circular-dependencies": "warn",
                 "unused-dependencies": "off", "stale-suppressions": "warn",
                 "require-suppression-reason": "warn" } }"#,
            r#"{ "rules": { "unused-exports": "warn", "circular-dependencies": "error",
                 "unused-dependencies": "error", "stale-suppressions": "warn",
                 "require-suppression-reason": "error" } }"#,
            r#"{ "rules": { "unused-exports": "off", "circular-dependencies": "off",
                 "unused-dependencies": "warn", "stale-suppressions": "error",
                 "require-suppression-reason": "off" } }"#,
        ];
        let mut results = AnalysisResults::default();
        results.unused_exports.push(export("src/app.ts"));
        results
            .circular_dependencies
            .push(cycle(&["src/a.ts", "src/b.ts"]));
        results.circular_dependencies.push(cycle(&[]));
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
        results.stale_suppressions.push(stale("src/app.ts", false));
        results.stale_suppressions.push(stale("src/app.ts", true));

        for json in configs {
            let config = config(json);
            for promote in [false, true] {
                let mut rules = config.rules.clone();
                if promote {
                    promote_warns_to_errors(&mut rules);
                }
                let source = SeveritySource::new(&rules, Some(&config), promote);
                assert!(source.overrides.is_none());
                for severity in [Severity::Error, Severity::Warn, Severity::Off] {
                    let mut per_finding = false;
                    for_each_gated_finding(&mut results, &mut |finding| {
                        per_finding |= finding.rule_severity(&source) == severity;
                    });
                    assert_eq!(
                        any_gated_finding(&results, &source, severity),
                        per_finding,
                        "{json} promote={promote} {severity:?}"
                    );
                }
            }
        }
    }

    type AddFinding = fn(&mut AnalysisResults);

    #[test]
    fn the_exit_code_rule_fails_exactly_when_a_finding_is_stamped_error() {
        let config = legacy_warn_config();
        let cases: [(&str, AddFinding); 4] = [
            ("legacy export", |r| {
                r.unused_exports.push(export("src/legacy/old.ts"));
            }),
            ("app export", |r| {
                r.unused_exports.push(export("src/app.ts"));
            }),
            ("legacy cycle", |r| {
                r.circular_dependencies
                    .push(cycle(&["src/legacy/a.ts", "src/legacy/b.ts"]));
            }),
            ("stale suppression", |r| {
                r.stale_suppressions.push(stale("src/app.ts", false));
            }),
        ];
        for (name, add) in cases {
            let mut results = AnalysisResults::default();
            add(&mut results);
            apply_effective_severities(&mut results, &config);
            let mut stamped_error = false;
            for_each_gated_finding(&mut results, &mut |finding| {
                stamped_error |= finding.effective_severity() == Some(EffectiveSeverity::Error);
            });
            assert_eq!(
                crate::error_severity::has_error_severity_issues(
                    &results,
                    &config.rules,
                    Some(&config),
                    false
                ),
                stamped_error,
                "{name}"
            );
        }
    }
}
