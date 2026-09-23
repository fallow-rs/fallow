//! The error-severity rule: whether a dead-code result holds a finding whose
//! effective severity is `error`.
//!
//! This rule decides the exit code of `dead-code` and `check`, the
//! `error-severity-findings` gate entry, the combined verdict and the audit
//! summary. Every caller reads it from here, so a finding cannot fail one
//! command and pass another.

use std::path::Path;

use fallow_config::{ResolvedConfig, RulesConfig, Severity};

/// Check whether any issue type with `Severity::Error` has remaining issues.
///
/// When overrides are configured, per-file rule resolution is used for
/// file-scoped issue types to determine if any individual issue has Error
/// severity. Circular dependencies resolve against every file in the cycle.
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
    let has_overrides = config.is_some_and(|c| !c.overrides.is_empty());

    let file_scoped_errors = if let Some(config) = config.filter(|c| !c.overrides.is_empty()) {
        let resolver = OverrideSeverities {
            config,
            promote_warns,
        };
        has_override_file_scoped_error(results, &resolver)
    } else {
        has_default_file_scoped_error(results, rules)
    };

    file_scoped_errors || has_project_level_error(results, rules, has_overrides)
}

/// Per-file severity source for the override path of the exit-code check.
///
/// Warn-to-error promotion happens after override resolution so it reaches
/// severities set by a per-path override, not only the base rules.
struct OverrideSeverities<'a> {
    config: &'a ResolvedConfig,
    promote_warns: bool,
}

impl OverrideSeverities<'_> {
    fn effective_rules_for(&self, path: &Path) -> RulesConfig {
        let mut rules = self.config.resolve_rules_for_path(path);
        if self.promote_warns {
            promote_warns_to_errors(&mut rules);
        }
        rules
    }
}

fn has_override_file_scoped_error(
    results: &crate::dead_code::AnalysisResults,
    resolver: &OverrideSeverities<'_>,
) -> bool {
    has_override_dead_code_error(results, resolver)
        || has_override_catalog_boundary_error(results, resolver)
        || has_override_framework_error(results, resolver)
}

fn has_override_dead_code_error(
    results: &crate::dead_code::AnalysisResults,
    resolver: &OverrideSeverities<'_>,
) -> bool {
    has_override_core_dead_code_error(results, resolver)
        || has_override_component_dead_code_error(results, resolver)
}

/// Per-file Error check for the core (non-component) dead-code issue types.
fn has_override_core_dead_code_error(
    results: &crate::dead_code::AnalysisResults,
    resolver: &OverrideSeverities<'_>,
) -> bool {
    results
        .unused_files
        .iter()
        .any(|f| resolver.effective_rules_for(&f.file.path).unused_files == Severity::Error)
        || results
            .unused_exports
            .iter()
            .any(|e| resolver.effective_rules_for(&e.export.path).unused_exports == Severity::Error)
        || results
            .unused_types
            .iter()
            .any(|e| resolver.effective_rules_for(&e.export.path).unused_types == Severity::Error)
        || results.private_type_leaks.iter().any(|e| {
            resolver
                .effective_rules_for(&e.leak.path)
                .private_type_leaks
                == Severity::Error
        })
        || results.unused_enum_members.iter().any(|m| {
            resolver
                .effective_rules_for(&m.member.path)
                .unused_enum_members
                == Severity::Error
        })
        || results.unused_class_members.iter().any(|m| {
            resolver
                .effective_rules_for(&m.member.path)
                .unused_class_members
                == Severity::Error
        })
        || results.unused_store_members.iter().any(|m| {
            resolver
                .effective_rules_for(&m.member.path)
                .unused_store_members
                == Severity::Error
        })
        || results.unprovided_injects.iter().any(|f| {
            resolver
                .effective_rules_for(&f.inject.path)
                .unprovided_injects
                == Severity::Error
        })
        || results.unresolved_imports.iter().any(|i| {
            resolver
                .effective_rules_for(&i.import.path)
                .unresolved_imports
                == Severity::Error
        })
}

/// Per-file Error check for the component-shaped dead-code issue types.
fn has_override_component_dead_code_error(
    results: &crate::dead_code::AnalysisResults,
    resolver: &OverrideSeverities<'_>,
) -> bool {
    results.unrendered_components.iter().any(|c| {
        resolver
            .effective_rules_for(&c.component.path)
            .unrendered_components
            == Severity::Error
    }) || results.unused_component_props.iter().any(|p| {
        resolver
            .effective_rules_for(&p.prop.path)
            .unused_component_props
            == Severity::Error
    }) || results.unused_component_emits.iter().any(|e| {
        resolver
            .effective_rules_for(&e.emit.path)
            .unused_component_emits
            == Severity::Error
    }) || results.unused_component_inputs.iter().any(|i| {
        resolver
            .effective_rules_for(&i.input.path)
            .unused_component_inputs
            == Severity::Error
    }) || results.unused_component_outputs.iter().any(|o| {
        resolver
            .effective_rules_for(&o.output.path)
            .unused_component_outputs
            == Severity::Error
    }) || results.unused_svelte_events.iter().any(|e| {
        resolver
            .effective_rules_for(&e.event.path)
            .unused_svelte_events
            == Severity::Error
    }) || results.unused_server_actions.iter().any(|a| {
        resolver
            .effective_rules_for(&a.action.path)
            .unused_server_actions
            == Severity::Error
    }) || results.unused_load_data_keys.iter().any(|k| {
        resolver
            .effective_rules_for(&k.key.path)
            .unused_load_data_keys
            == Severity::Error
    })
}

fn has_override_catalog_boundary_error(
    results: &crate::dead_code::AnalysisResults,
    resolver: &OverrideSeverities<'_>,
) -> bool {
    results.stale_suppressions.iter().any(|s| {
        let rules = resolver.effective_rules_for(&s.path);
        if s.missing_reason {
            rules.require_suppression_reason == Severity::Error
        } else {
            rules.stale_suppressions == Severity::Error
        }
    }) || results.unresolved_catalog_references.iter().any(|r| {
        resolver
            .effective_rules_for(&r.reference.path)
            .unresolved_catalog_references
            == Severity::Error
    }) || results.empty_catalog_groups.iter().any(|g| {
        resolver
            .effective_rules_for(&g.group.path)
            .empty_catalog_groups
            == Severity::Error
    }) || results.unused_dependency_overrides.iter().any(|o| {
        resolver
            .effective_rules_for(&o.entry.path)
            .unused_dependency_overrides
            == Severity::Error
    }) || results.misconfigured_dependency_overrides.iter().any(|o| {
        resolver
            .effective_rules_for(&o.entry.path)
            .misconfigured_dependency_overrides
            == Severity::Error
    }) || results.boundary_violations.iter().any(|v| {
        resolver
            .effective_rules_for(&v.violation.from_path)
            .boundary_violation
            == Severity::Error
    }) || results.boundary_coverage_violations.iter().any(|v| {
        resolver
            .effective_rules_for(&v.violation.path)
            .boundary_violation
            == Severity::Error
    }) || results.boundary_call_violations.iter().any(|v| {
        resolver
            .effective_rules_for(&v.violation.path)
            .boundary_violation
            == Severity::Error
    }) || results.circular_dependencies.iter().any(|c| {
        c.cycle
            .files
            .iter()
            .any(|path| resolver.effective_rules_for(path).circular_dependencies == Severity::Error)
    })
}

fn has_override_framework_error(
    results: &crate::dead_code::AnalysisResults,
    resolver: &OverrideSeverities<'_>,
) -> bool {
    results.invalid_client_exports.iter().any(|e| {
        resolver
            .effective_rules_for(&e.export.path)
            .invalid_client_export
            == Severity::Error
    }) || results.mixed_client_server_barrels.iter().any(|b| {
        resolver
            .effective_rules_for(&b.barrel.path)
            .mixed_client_server_barrel
            == Severity::Error
    }) || results.misplaced_directives.iter().any(|d| {
        resolver
            .effective_rules_for(&d.directive_site.path)
            .misplaced_directive
            == Severity::Error
    }) || results.route_collisions.iter().any(|c| {
        resolver
            .effective_rules_for(&c.collision.path)
            .route_collision
            == Severity::Error
    }) || results.dynamic_segment_name_conflicts.iter().any(|c| {
        resolver
            .effective_rules_for(&c.conflict.path)
            .dynamic_segment_name_conflict
            == Severity::Error
    })
}

fn has_default_file_scoped_error(
    results: &crate::dead_code::AnalysisResults,
    rules: &RulesConfig,
) -> bool {
    (rules.unused_files == Severity::Error && !results.unused_files.is_empty())
        || (rules.unused_exports == Severity::Error && !results.unused_exports.is_empty())
        || (rules.unused_types == Severity::Error && !results.unused_types.is_empty())
        || (rules.private_type_leaks == Severity::Error && !results.private_type_leaks.is_empty())
        || (rules.unused_enum_members == Severity::Error && !results.unused_enum_members.is_empty())
        || (rules.unused_class_members == Severity::Error
            && !results.unused_class_members.is_empty())
        || (rules.unused_store_members == Severity::Error
            && !results.unused_store_members.is_empty())
        || (rules.unprovided_injects == Severity::Error && !results.unprovided_injects.is_empty())
        || (rules.unrendered_components == Severity::Error
            && !results.unrendered_components.is_empty())
        || (rules.unused_component_props == Severity::Error
            && !results.unused_component_props.is_empty())
        || (rules.unused_component_emits == Severity::Error
            && !results.unused_component_emits.is_empty())
        || (rules.unused_component_inputs == Severity::Error
            && !results.unused_component_inputs.is_empty())
        || (rules.unused_component_outputs == Severity::Error
            && !results.unused_component_outputs.is_empty())
        || (rules.unused_svelte_events == Severity::Error
            && !results.unused_svelte_events.is_empty())
        || (rules.unused_server_actions == Severity::Error
            && !results.unused_server_actions.is_empty())
        || (rules.unused_load_data_keys == Severity::Error
            && !results.unused_load_data_keys.is_empty())
        || (rules.unresolved_imports == Severity::Error && !results.unresolved_imports.is_empty())
        || results.stale_suppressions.iter().any(|s| {
            if s.missing_reason {
                rules.require_suppression_reason == Severity::Error
            } else {
                rules.stale_suppressions == Severity::Error
            }
        })
        || (rules.unresolved_catalog_references == Severity::Error
            && !results.unresolved_catalog_references.is_empty())
        || (rules.empty_catalog_groups == Severity::Error
            && !results.empty_catalog_groups.is_empty())
        || (rules.unused_dependency_overrides == Severity::Error
            && !results.unused_dependency_overrides.is_empty())
        || (rules.misconfigured_dependency_overrides == Severity::Error
            && !results.misconfigured_dependency_overrides.is_empty())
        || (rules.invalid_client_export == Severity::Error
            && !results.invalid_client_exports.is_empty())
        || (rules.mixed_client_server_barrel == Severity::Error
            && !results.mixed_client_server_barrels.is_empty())
        || (rules.misplaced_directive == Severity::Error
            && !results.misplaced_directives.is_empty())
        || (rules.route_collision == Severity::Error && !results.route_collisions.is_empty())
        || (rules.dynamic_segment_name_conflict == Severity::Error
            && !results.dynamic_segment_name_conflicts.is_empty())
}

fn has_project_level_error(
    results: &crate::dead_code::AnalysisResults,
    rules: &RulesConfig,
    has_overrides: bool,
) -> bool {
    (rules.unused_dependencies == Severity::Error && !results.unused_dependencies.is_empty())
        || (rules.unused_dev_dependencies == Severity::Error
            && !results.unused_dev_dependencies.is_empty())
        || (rules.unused_optional_dependencies == Severity::Error
            && !results.unused_optional_dependencies.is_empty())
        || (rules.unlisted_dependencies == Severity::Error
            && !results.unlisted_dependencies.is_empty())
        || (rules.duplicate_exports == Severity::Error && !results.duplicate_exports.is_empty())
        || (rules.type_only_dependencies == Severity::Error
            && !results.type_only_dependencies.is_empty())
        || (rules.test_only_dependencies == Severity::Error
            && !results.test_only_dependencies.is_empty())
        || (rules.dev_dependencies_in_production == Severity::Error
            && !results.dev_dependencies_in_production.is_empty())
        || (!has_overrides
            && rules.circular_dependencies == Severity::Error
            && !results.circular_dependencies.is_empty())
        || (rules.re_export_cycle == Severity::Error && !results.re_export_cycles.is_empty())
        || (!has_overrides
            && rules.boundary_violation == Severity::Error
            && !results.boundary_violations.is_empty())
        || (!has_overrides
            && rules.boundary_violation == Severity::Error
            && !results.boundary_coverage_violations.is_empty())
        || (!has_overrides
            && rules.boundary_violation == Severity::Error
            && !results.boundary_call_violations.is_empty())
        || (rules.unused_catalog_entries == Severity::Error
            && !results.unused_catalog_entries.is_empty())
        // Empty catalog groups and dependency overrides are file-scoped: the
        // override or default branch above decides them per path, the same
        // way the audit ledger and the per-finding `effective_severity` do.
        // Policy violations gate on the EFFECTIVE per-finding severity baked
        // by the evaluator (per-file override master + per-rule override),
        // not on `rules.policy_violation`: a master of `warn` with one
        // `severity: "error"` rule must still fail the run.
        || results
            .policy_violations
            .iter()
            .any(|v| v.violation.severity == fallow_types::results::PolicyViolationSeverity::Error)
}

/// Promote all `Warn` severities to `Error` for a single run.
pub fn promote_warns_to_errors(rules: &mut RulesConfig) {
    for rule in [
        &mut rules.unused_files,
        &mut rules.unused_exports,
        &mut rules.unused_types,
        &mut rules.private_type_leaks,
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
