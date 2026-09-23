//! The error-severity rule: whether a dead-code result holds a finding whose
//! effective severity is `error`.
//!
//! This rule decides the exit code of `dead-code` and `check`, the
//! `error-severity-findings` gate entry, the combined verdict and the audit
//! summary. Every caller reads it from here, so a finding cannot fail one
//! command and pass another.

use fallow_config::{ResolvedConfig, RulesConfig, Severity};

use crate::effective_severity::{SeveritySource, any_finding_with_severity};

/// Check whether any issue type with `Severity::Error` has remaining issues.
///
/// The severity of each finding comes from the rule table in
/// `crate::effective_severity`, the same table that writes the
/// `effective_severity` of each finding for the CI formats and that the audit
/// ledger reads. When overrides are configured, file-scoped findings resolve
/// the rules for their own path. Circular dependencies resolve against every
/// file in the cycle. Project-level findings read `rules`.
///
/// `promote_warns` mirrors `--fail-on-issues`: `rules` is expected to arrive
/// already promoted, and the per-file override path promotes each resolved
/// severity after override resolution so an explicit per-path `warn` fails
/// the run just like the base rules would.
pub fn has_error_severity_issues(
    results: &crate::dead_code::AnalysisResults,
    rules: &RulesConfig,
    config: Option<&ResolvedConfig>,
    promote_warns: bool,
) -> bool {
    let source = SeveritySource::new(rules, config, promote_warns);
    any_finding_with_severity(results, &source, Severity::Error)
}

/// Promote all `Warn` severities to `Error` for a single run.
pub fn promote_warns_to_errors(rules: &mut RulesConfig) {
    for rule in [
        &mut rules.unused_files,
        &mut rules.unused_exports,
        &mut rules.unused_types,
        &mut rules.private_type_leaks,
        &mut rules.deprecated_exports_in_use,
        &mut rules.unused_dependencies,
        &mut rules.unused_dev_dependencies,
        &mut rules.unused_optional_dependencies,
        &mut rules.unused_enum_members,
        &mut rules.unused_class_members,
        &mut rules.unused_store_members,
        &mut rules.unprovided_injects,
        &mut rules.unrendered_components,
        &mut rules.unused_component_props,
        &mut rules.unused_component_emits,
        &mut rules.unused_component_inputs,
        &mut rules.unused_component_outputs,
        &mut rules.unused_svelte_events,
        &mut rules.unused_server_actions,
        &mut rules.unused_load_data_keys,
        &mut rules.unresolved_imports,
        &mut rules.unlisted_dependencies,
        &mut rules.duplicate_exports,
        &mut rules.type_only_dependencies,
        &mut rules.test_only_dependencies,
        &mut rules.dev_dependencies_in_production,
        &mut rules.circular_dependencies,
        &mut rules.re_export_cycle,
        &mut rules.boundary_violation,
        &mut rules.coverage_gaps,
        &mut rules.stale_suppressions,
        &mut rules.require_suppression_reason,
        &mut rules.unused_catalog_entries,
        &mut rules.empty_catalog_groups,
        &mut rules.unresolved_catalog_references,
        &mut rules.unused_dependency_overrides,
        &mut rules.misconfigured_dependency_overrides,
        &mut rules.policy_violation,
        &mut rules.invalid_client_export,
        &mut rules.mixed_client_server_barrel,
        &mut rules.misplaced_directive,
        &mut rules.route_collision,
        &mut rules.dynamic_segment_name_conflict,
    ] {
        promote_warn_to_error(rule);
    }
}

fn promote_warn_to_error(rule: &mut Severity) {
    if *rule == Severity::Warn {
        *rule = Severity::Error;
    }
}

/// Promote per-finding `warn` policy-violation severities to `error` for a
/// strict (fail-on-issues) run. Policy findings carry their effective
/// severity baked by the evaluator, so the rule-level promotion in
/// [`promote_warns_to_errors`] alone would not flip findings whose rule
/// explicitly opted down to `warn`; under strict mode every warning fails.
pub fn promote_policy_finding_warns(results: &mut crate::dead_code::AnalysisResults) {
    use fallow_types::results::PolicyViolationSeverity;
    for finding in &mut results.policy_violations {
        if finding.violation.severity == PolicyViolationSeverity::Warn {
            finding.violation.severity = PolicyViolationSeverity::Error;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use fallow_config::{ConfigOverride, FallowConfig, PartialRulesConfig, RulesConfig, Severity};
    use fallow_types::output_dead_code::{
        EmptyCatalogGroupFinding, MisconfiguredDependencyOverrideFinding,
        UnusedDependencyOverrideFinding,
    };
    use fallow_types::results::{
        DependencyOverrideMisconfigReason, DependencyOverrideSource, EmptyCatalogGroup,
        MisconfiguredDependencyOverride, UnusedDependencyOverride,
    };

    use super::has_error_severity_issues;
    use crate::dead_code::AnalysisResults;

    /// One finding of each manifest-level kind that per-file `overrides` can
    /// change: an unused and a misconfigured dependency override in
    /// `package.json`, and an empty catalog group in `pnpm-workspace.yaml`.
    fn manifest_findings() -> [(&'static str, AnalysisResults); 3] {
        let mut unused = AnalysisResults::default();
        unused
            .unused_dependency_overrides
            .push(UnusedDependencyOverrideFinding::with_actions(
                UnusedDependencyOverride {
                    raw_key: "old-dep".to_string(),
                    target_package: "old-dep".to_string(),
                    parent_package: None,
                    version_constraint: None,
                    version_range: "^1.0.0".to_string(),
                    source: DependencyOverrideSource::PnpmPackageJson,
                    path: PathBuf::from("/project/package.json"),
                    line: 7,
                    hint: None,
                },
            ));
        let mut misconfigured = AnalysisResults::default();
        misconfigured.misconfigured_dependency_overrides.push(
            MisconfiguredDependencyOverrideFinding::with_actions(MisconfiguredDependencyOverride {
                raw_key: "bad>".to_string(),
                target_package: None,
                raw_value: "1.0.0".to_string(),
                reason: DependencyOverrideMisconfigReason::UnparsableKey,
                source: DependencyOverrideSource::PnpmPackageJson,
                path: PathBuf::from("/project/package.json"),
                line: 4,
            }),
        );
        let mut empty_group = AnalysisResults::default();
        empty_group
            .empty_catalog_groups
            .push(EmptyCatalogGroupFinding::with_actions(EmptyCatalogGroup {
                catalog_name: "legacy".to_string(),
                path: PathBuf::from("/project/pnpm-workspace.yaml"),
                line: 3,
            }));
        [
            ("unused-dependency-overrides", unused),
            ("misconfigured-dependency-overrides", misconfigured),
            ("empty-catalog-groups", empty_group),
        ]
    }

    fn config_with_manifest_override(
        base: Severity,
        manifest: Severity,
    ) -> fallow_config::ResolvedConfig {
        FallowConfig {
            rules: RulesConfig {
                unused_dependency_overrides: base,
                misconfigured_dependency_overrides: base,
                empty_catalog_groups: base,
                ..RulesConfig::default()
            },
            overrides: vec![ConfigOverride {
                files: vec![
                    "package.json".to_string(),
                    "pnpm-workspace.yaml".to_string(),
                ],
                rules: PartialRulesConfig {
                    unused_dependency_overrides: Some(manifest),
                    misconfigured_dependency_overrides: Some(manifest),
                    empty_catalog_groups: Some(manifest),
                    ..PartialRulesConfig::default()
                },
            }],
            ..FallowConfig::default()
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

    #[test]
    fn a_manifest_override_to_warn_clears_a_base_error() {
        let config = config_with_manifest_override(Severity::Error, Severity::Warn);
        for (kind, results) in manifest_findings() {
            assert!(
                !has_error_severity_issues(&results, &config.rules, Some(&config), false),
                "the `warn` override for the manifest must win over the base `error` for {kind}"
            );
        }
    }

    #[test]
    fn a_manifest_override_to_error_raises_a_base_warn() {
        let config = config_with_manifest_override(Severity::Warn, Severity::Error);
        for (kind, results) in manifest_findings() {
            assert!(
                has_error_severity_issues(&results, &config.rules, Some(&config), false),
                "the `error` override for the manifest must win over the base `warn` for {kind}"
            );
        }
    }
}
