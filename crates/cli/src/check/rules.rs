use fallow_config::{ResolvedConfig, RulesConfig, Severity};

/// Remove issues whose effective severity is `Off` from the results.
///
/// When overrides are configured, per-file rule resolution is used for
/// file-scoped issue types. Circular dependencies resolve against every file in
/// the cycle. Non-file-scoped issues (unused deps, unlisted deps, duplicate
/// exports) use the base rules only.
pub fn apply_rules(results: &mut fallow_core::results::AnalysisResults, config: &ResolvedConfig) {
    let rules = &config.rules;
    let has_overrides = !config.overrides.is_empty();

    if has_overrides {
        apply_file_override_rules(results, config);
        apply_boundary_override_rules(results, config);
    } else {
        apply_base_file_rules(results, rules);
    }

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

fn apply_file_override_rules(
    results: &mut fallow_core::results::AnalysisResults,
    config: &ResolvedConfig,
) {
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
    results.unused_server_actions.retain(|a| {
        config
            .resolve_rules_for_path(&a.action.path)
            .unused_server_actions
            != Severity::Off
    });
    results.unresolved_imports.retain(|i| {
        config
            .resolve_rules_for_path(&i.import.path)
            .unresolved_imports
            != Severity::Off
    });
    results
        .stale_suppressions
        .retain(|s| config.resolve_rules_for_path(&s.path).stale_suppressions != Severity::Off);
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
    results.circular_dependencies.retain(|c| {
        c.cycle
            .files
            .iter()
            .any(|path| config.resolve_rules_for_path(path).circular_dependencies != Severity::Off)
    });
}

fn apply_base_file_rules(results: &mut fallow_core::results::AnalysisResults, rules: &RulesConfig) {
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
    if rules.unrendered_components == Severity::Off {
        results.unrendered_components.clear();
    }
    if rules.unused_component_props == Severity::Off {
        results.unused_component_props.clear();
    }
    if rules.unused_component_emits == Severity::Off {
        results.unused_component_emits.clear();
    }
    if rules.unused_server_actions == Severity::Off {
        results.unused_server_actions.clear();
    }
    if rules.unresolved_imports == Severity::Off {
        results.unresolved_imports.clear();
    }
    if rules.stale_suppressions == Severity::Off {
        results.stale_suppressions.clear();
    }
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

fn apply_boundary_override_rules(
    results: &mut fallow_core::results::AnalysisResults,
    config: &ResolvedConfig,
) {
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

/// Check whether any issue type with `Severity::Error` has remaining issues.
///
/// When overrides are configured, per-file rule resolution is used for
/// file-scoped issue types to determine if any individual issue has Error
/// severity. Circular dependencies resolve against every file in the cycle.
pub fn has_error_severity_issues(
    results: &fallow_core::results::AnalysisResults,
    rules: &RulesConfig,
    config: Option<&ResolvedConfig>,
) -> bool {
    let has_overrides = config.is_some_and(|c| !c.overrides.is_empty());

    let file_scoped_errors = if let Some(config) = config.filter(|c| !c.overrides.is_empty()) {
        has_override_file_scoped_error(results, config)
    } else {
        has_default_file_scoped_error(results, rules)
    };

    file_scoped_errors || has_project_level_error(results, rules, has_overrides)
}

fn has_override_file_scoped_error(
    results: &fallow_core::results::AnalysisResults,
    config: &ResolvedConfig,
) -> bool {
    results
        .unused_files
        .iter()
        .any(|f| config.resolve_rules_for_path(&f.file.path).unused_files == Severity::Error)
        || results.unused_exports.iter().any(|e| {
            config.resolve_rules_for_path(&e.export.path).unused_exports == Severity::Error
        })
        || results
            .unused_types
            .iter()
            .any(|e| config.resolve_rules_for_path(&e.export.path).unused_types == Severity::Error)
        || results.private_type_leaks.iter().any(|e| {
            config
                .resolve_rules_for_path(&e.leak.path)
                .private_type_leaks
                == Severity::Error
        })
        || results.unused_enum_members.iter().any(|m| {
            config
                .resolve_rules_for_path(&m.member.path)
                .unused_enum_members
                == Severity::Error
        })
        || results.unused_class_members.iter().any(|m| {
            config
                .resolve_rules_for_path(&m.member.path)
                .unused_class_members
                == Severity::Error
        })
        || results.unused_store_members.iter().any(|m| {
            config
                .resolve_rules_for_path(&m.member.path)
                .unused_store_members
                == Severity::Error
        })
        || results.unprovided_injects.iter().any(|f| {
            config
                .resolve_rules_for_path(&f.inject.path)
                .unprovided_injects
                == Severity::Error
        })
        || results.unrendered_components.iter().any(|c| {
            config
                .resolve_rules_for_path(&c.component.path)
                .unrendered_components
                == Severity::Error
        })
        || results.unused_component_props.iter().any(|p| {
            config
                .resolve_rules_for_path(&p.prop.path)
                .unused_component_props
                == Severity::Error
        })
        || results.unused_component_emits.iter().any(|e| {
            config
                .resolve_rules_for_path(&e.emit.path)
                .unused_component_emits
                == Severity::Error
        })
        || results.unused_server_actions.iter().any(|a| {
            config
                .resolve_rules_for_path(&a.action.path)
                .unused_server_actions
                == Severity::Error
        })
        || results.unresolved_imports.iter().any(|i| {
            config
                .resolve_rules_for_path(&i.import.path)
                .unresolved_imports
                == Severity::Error
        })
        || results
            .stale_suppressions
            .iter()
            .any(|s| config.resolve_rules_for_path(&s.path).stale_suppressions == Severity::Error)
        || results.unresolved_catalog_references.iter().any(|r| {
            config
                .resolve_rules_for_path(&r.reference.path)
                .unresolved_catalog_references
                == Severity::Error
        })
        || results.empty_catalog_groups.iter().any(|g| {
            config
                .resolve_rules_for_path(&g.group.path)
                .empty_catalog_groups
                == Severity::Error
        })
        || results.boundary_coverage_violations.iter().any(|v| {
            config
                .resolve_rules_for_path(&v.violation.path)
                .boundary_violation
                == Severity::Error
        })
        || results.boundary_call_violations.iter().any(|v| {
            config
                .resolve_rules_for_path(&v.violation.path)
                .boundary_violation
                == Severity::Error
        })
        || results.circular_dependencies.iter().any(|c| {
            c.cycle.files.iter().any(|path| {
                config.resolve_rules_for_path(path).circular_dependencies == Severity::Error
            })
        })
        || results.invalid_client_exports.iter().any(|e| {
            config
                .resolve_rules_for_path(&e.export.path)
                .invalid_client_export
                == Severity::Error
        })
        || results.mixed_client_server_barrels.iter().any(|b| {
            config
                .resolve_rules_for_path(&b.barrel.path)
                .mixed_client_server_barrel
                == Severity::Error
        })
        || results.misplaced_directives.iter().any(|d| {
            config
                .resolve_rules_for_path(&d.directive_site.path)
                .misplaced_directive
                == Severity::Error
        })
        || results.route_collisions.iter().any(|c| {
            config
                .resolve_rules_for_path(&c.collision.path)
                .route_collision
                == Severity::Error
        })
        || results.dynamic_segment_name_conflicts.iter().any(|c| {
            config
                .resolve_rules_for_path(&c.conflict.path)
                .dynamic_segment_name_conflict
                == Severity::Error
        })
}

fn has_default_file_scoped_error(
    results: &fallow_core::results::AnalysisResults,
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
        || (rules.unused_server_actions == Severity::Error
            && !results.unused_server_actions.is_empty())
        || (rules.unresolved_imports == Severity::Error && !results.unresolved_imports.is_empty())
        || (rules.stale_suppressions == Severity::Error && !results.stale_suppressions.is_empty())
        || (rules.unresolved_catalog_references == Severity::Error
            && !results.unresolved_catalog_references.is_empty())
        || (rules.empty_catalog_groups == Severity::Error
            && !results.empty_catalog_groups.is_empty())
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
    results: &fallow_core::results::AnalysisResults,
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
        || (rules.empty_catalog_groups == Severity::Error
            && !results.empty_catalog_groups.is_empty())
        || (rules.unused_dependency_overrides == Severity::Error
            && !results.unused_dependency_overrides.is_empty())
        || (rules.misconfigured_dependency_overrides == Severity::Error
            && !results.misconfigured_dependency_overrides.is_empty())
        // Policy violations gate on the EFFECTIVE per-finding severity baked
        // by the evaluator (per-file override master + per-rule override),
        // not on `rules.policy_violation`: a master of `warn` with one
        // `severity: "error"` rule must still fail the run.
        || results
            .policy_violations
            .iter()
            .any(|v| v.violation.severity == fallow_core::results::PolicyViolationSeverity::Error)
}

/// Promote all `Warn` severities to `Error` for a single run.
pub fn promote_warns_to_errors(rules: &mut RulesConfig) {
    if rules.unused_files == Severity::Warn {
        rules.unused_files = Severity::Error;
    }
    if rules.unused_exports == Severity::Warn {
        rules.unused_exports = Severity::Error;
    }
    if rules.unused_types == Severity::Warn {
        rules.unused_types = Severity::Error;
    }
    if rules.private_type_leaks == Severity::Warn {
        rules.private_type_leaks = Severity::Error;
    }
    if rules.unused_dependencies == Severity::Warn {
        rules.unused_dependencies = Severity::Error;
    }
    if rules.unused_dev_dependencies == Severity::Warn {
        rules.unused_dev_dependencies = Severity::Error;
    }
    if rules.unused_optional_dependencies == Severity::Warn {
        rules.unused_optional_dependencies = Severity::Error;
    }
    if rules.unused_enum_members == Severity::Warn {
        rules.unused_enum_members = Severity::Error;
    }
    if rules.unused_class_members == Severity::Warn {
        rules.unused_class_members = Severity::Error;
    }
    if rules.unused_store_members == Severity::Warn {
        rules.unused_store_members = Severity::Error;
    }
    if rules.unprovided_injects == Severity::Warn {
        rules.unprovided_injects = Severity::Error;
    }
    if rules.unrendered_components == Severity::Warn {
        rules.unrendered_components = Severity::Error;
    }
    if rules.unused_component_props == Severity::Warn {
        rules.unused_component_props = Severity::Error;
    }
    if rules.unused_component_emits == Severity::Warn {
        rules.unused_component_emits = Severity::Error;
    }
    if rules.unused_server_actions == Severity::Warn {
        rules.unused_server_actions = Severity::Error;
    }
    if rules.unresolved_imports == Severity::Warn {
        rules.unresolved_imports = Severity::Error;
    }
    if rules.unlisted_dependencies == Severity::Warn {
        rules.unlisted_dependencies = Severity::Error;
    }
    if rules.duplicate_exports == Severity::Warn {
        rules.duplicate_exports = Severity::Error;
    }
    if rules.type_only_dependencies == Severity::Warn {
        rules.type_only_dependencies = Severity::Error;
    }
    if rules.test_only_dependencies == Severity::Warn {
        rules.test_only_dependencies = Severity::Error;
    }
    if rules.circular_dependencies == Severity::Warn {
        rules.circular_dependencies = Severity::Error;
    }
    if rules.re_export_cycle == Severity::Warn {
        rules.re_export_cycle = Severity::Error;
    }
    if rules.boundary_violation == Severity::Warn {
        rules.boundary_violation = Severity::Error;
    }
    if rules.coverage_gaps == Severity::Warn {
        rules.coverage_gaps = Severity::Error;
    }
    if rules.stale_suppressions == Severity::Warn {
        rules.stale_suppressions = Severity::Error;
    }
    if rules.unused_catalog_entries == Severity::Warn {
        rules.unused_catalog_entries = Severity::Error;
    }
    if rules.empty_catalog_groups == Severity::Warn {
        rules.empty_catalog_groups = Severity::Error;
    }
    if rules.unresolved_catalog_references == Severity::Warn {
        rules.unresolved_catalog_references = Severity::Error;
    }
    if rules.unused_dependency_overrides == Severity::Warn {
        rules.unused_dependency_overrides = Severity::Error;
    }
    if rules.misconfigured_dependency_overrides == Severity::Warn {
        rules.misconfigured_dependency_overrides = Severity::Error;
    }
    if rules.policy_violation == Severity::Warn {
        rules.policy_violation = Severity::Error;
    }
    if rules.invalid_client_export == Severity::Warn {
        rules.invalid_client_export = Severity::Error;
    }
    if rules.mixed_client_server_barrel == Severity::Warn {
        rules.mixed_client_server_barrel = Severity::Error;
    }
    if rules.misplaced_directive == Severity::Warn {
        rules.misplaced_directive = Severity::Error;
    }
    if rules.route_collision == Severity::Warn {
        rules.route_collision = Severity::Error;
    }
    if rules.dynamic_segment_name_conflict == Severity::Warn {
        rules.dynamic_segment_name_conflict = Severity::Error;
    }
}

/// Promote per-finding `warn` policy-violation severities to `error` for a
/// strict (fail-on-issues) run. Policy findings carry their effective
/// severity baked by the evaluator, so the rule-level promotion in
/// [`promote_warns_to_errors`] alone would not flip findings whose rule
/// explicitly opted down to `warn`; under strict mode every warning fails.
pub fn promote_policy_finding_warns(results: &mut fallow_core::results::AnalysisResults) {
    use fallow_core::results::PolicyViolationSeverity;
    for finding in &mut results.policy_violations {
        if finding.violation.severity == PolicyViolationSeverity::Warn {
            finding.violation.severity = PolicyViolationSeverity::Error;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    type RuleFieldSetter = fn(&mut RulesConfig);
    type ResultFieldCheck = fn(&AnalysisResults) -> bool;
    use fallow_core::extract::MemberKind;
    use fallow_core::results::*;
    use std::path::PathBuf;

    fn make_results() -> AnalysisResults {
        let mut r = AnalysisResults::default();
        r.unused_files
            .push(UnusedFileFinding::with_actions(UnusedFile {
                path: PathBuf::from("/project/src/a.ts"),
            }));
        r.unused_exports
            .push(UnusedExportFinding::with_actions(UnusedExport {
                path: PathBuf::from("/project/src/b.ts"),
                export_name: "foo".into(),
                is_type_only: false,
                line: 1,
                col: 0,
                span_start: 0,
                is_re_export: false,
            }));
        r.unused_types
            .push(UnusedTypeFinding::with_actions(UnusedExport {
                path: PathBuf::from("/project/src/c.ts"),
                export_name: "MyType".into(),
                is_type_only: true,
                line: 5,
                col: 0,
                span_start: 0,
                is_re_export: false,
            }));
        r.unused_dependencies
            .push(UnusedDependencyFinding::with_actions(UnusedDependency {
                package_name: "lodash".into(),
                location: DependencyLocation::Dependencies,
                path: PathBuf::from("/project/package.json"),
                line: 5,
                used_in_workspaces: Vec::new(),
            }));
        r.unused_dev_dependencies
            .push(UnusedDevDependencyFinding::with_actions(UnusedDependency {
                package_name: "jest".into(),
                location: DependencyLocation::DevDependencies,
                path: PathBuf::from("/project/package.json"),
                line: 5,
                used_in_workspaces: Vec::new(),
            }));
        r.unused_enum_members
            .push(UnusedEnumMemberFinding::with_actions(UnusedMember {
                path: PathBuf::from("/project/src/d.ts"),
                parent_name: "Status".into(),
                member_name: "Pending".into(),
                kind: MemberKind::EnumMember,
                line: 3,
                col: 0,
            }));
        r.unused_class_members
            .push(UnusedClassMemberFinding::with_actions(UnusedMember {
                path: PathBuf::from("/project/src/e.ts"),
                parent_name: "Service".into(),
                member_name: "helper".into(),
                kind: MemberKind::ClassMethod,
                line: 10,
                col: 0,
            }));
        r.unused_store_members
            .push(UnusedStoreMemberFinding::with_actions(UnusedMember {
                path: PathBuf::from("/project/src/store.ts"),
                parent_name: "useStore".into(),
                member_name: "unusedAction".into(),
                kind: MemberKind::StoreMember,
                line: 12,
                col: 0,
            }));
        r.unresolved_imports
            .push(UnresolvedImportFinding::with_actions(UnresolvedImport {
                path: PathBuf::from("/project/src/f.ts"),
                specifier: "./missing".into(),
                line: 1,
                col: 0,
                specifier_col: 0,
            }));
        r.unlisted_dependencies
            .push(UnlistedDependencyFinding::with_actions(
                UnlistedDependency {
                    package_name: "chalk".into(),
                    imported_from: vec![ImportSite {
                        path: PathBuf::from("/project/src/g.ts"),
                        line: 1,
                        col: 0,
                    }],
                },
            ));
        r.duplicate_exports
            .push(DuplicateExportFinding::with_actions(DuplicateExport {
                export_name: "helper".into(),
                locations: vec![
                    DuplicateLocation {
                        path: PathBuf::from("/project/src/h.ts"),
                        line: 15,
                        col: 0,
                    },
                    DuplicateLocation {
                        path: PathBuf::from("/project/src/i.ts"),
                        line: 30,
                        col: 0,
                    },
                ],
            }));
        r
    }

    /// Build a minimal ResolvedConfig from a RulesConfig for testing.
    fn config_with_rules(rules: RulesConfig) -> ResolvedConfig {
        fallow_config::FallowConfig {
            schema: None,
            extends: vec![],
            entry: vec![],
            ignore_patterns: vec![],
            framework: vec![],
            workspaces: None,
            ignore_dependencies: vec![],
            ignore_unresolved_imports: vec![],
            ignore_exports: vec![],
            ignore_catalog_references: vec![],
            ignore_dependency_overrides: vec![],
            ignore_exports_used_in_file: fallow_config::IgnoreExportsUsedInFileConfig::default(),
            used_class_members: vec![],
            ignore_decorators: vec![],
            duplicates: fallow_config::DuplicatesConfig::default(),
            health: fallow_config::HealthConfig::default(),
            rules,
            boundaries: fallow_config::BoundaryConfig::default(),
            production: false.into(),
            plugins: vec![],
            rule_packs: vec![],
            dynamically_loaded: vec![],
            overrides: vec![],
            regression: None,
            audit: fallow_config::AuditConfig::default(),
            codeowners: None,
            public_packages: vec![],
            flags: fallow_config::FlagsConfig::default(),
            security: fallow_config::SecurityConfig::default(),
            fix: fallow_config::FixConfig::default(),
            resolve: fallow_config::ResolveConfig::default(),
            sealed: false,
            include_entry_exports: false,
            auto_imports: false,
            cache: fallow_config::CacheConfig::default(),
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
    fn apply_rules_default_error_preserves_all() {
        let mut results = make_results();
        let config = config_with_rules(RulesConfig::default());
        let original_total = results.total_issues();
        apply_rules(&mut results, &config);
        assert_eq!(results.total_issues(), original_total);
    }

    #[test]
    fn apply_rules_off_clears_that_issue_type() {
        let mut results = make_results();
        let rules = RulesConfig {
            unused_files: Severity::Off,
            ..RulesConfig::default()
        };
        let config = config_with_rules(rules);
        apply_rules(&mut results, &config);
        assert!(results.unused_files.is_empty());
        assert!(!results.unused_exports.is_empty());
    }

    #[test]
    fn apply_rules_warn_preserves_issues() {
        let mut results = make_results();
        let rules = RulesConfig {
            unused_exports: Severity::Warn,
            ..RulesConfig::default()
        };
        let config = config_with_rules(rules);
        apply_rules(&mut results, &config);
        assert_eq!(results.unused_exports.len(), 1);
    }

    #[test]
    fn apply_rules_all_off_clears_everything() {
        let mut results = make_results();
        let rules = RulesConfig {
            unused_files: Severity::Off,
            unused_exports: Severity::Off,
            unused_types: Severity::Off,
            private_type_leaks: Severity::Off,
            unused_dependencies: Severity::Off,
            unused_dev_dependencies: Severity::Off,
            unused_optional_dependencies: Severity::Off,
            unused_enum_members: Severity::Off,
            unused_class_members: Severity::Off,
            unused_store_members: Severity::Off,
            unprovided_injects: Severity::Off,
            unrendered_components: Severity::Off,
            unused_component_props: Severity::Off,
            unused_component_emits: Severity::Off,
            unused_server_actions: Severity::Off,
            unresolved_imports: Severity::Off,
            unlisted_dependencies: Severity::Off,
            duplicate_exports: Severity::Off,
            type_only_dependencies: Severity::Off,
            test_only_dependencies: Severity::Off,
            boundary_violation: Severity::Error,
            circular_dependencies: Severity::Off,
            re_export_cycle: Severity::Warn,
            coverage_gaps: Severity::Off,
            feature_flags: Severity::Off,
            stale_suppressions: Severity::Off,
            unused_catalog_entries: Severity::Off,
            empty_catalog_groups: Severity::Off,
            unresolved_catalog_references: Severity::Off,
            unused_dependency_overrides: Severity::Off,
            misconfigured_dependency_overrides: Severity::Off,
            security_client_server_leak: Severity::Off,
            security_sink: Severity::Off,
            policy_violation: Severity::Warn,
            invalid_client_export: Severity::Warn,
            mixed_client_server_barrel: Severity::Warn,
            misplaced_directive: Severity::Warn,
            route_collision: Severity::Warn,
            dynamic_segment_name_conflict: Severity::Warn,
        };
        let config = config_with_rules(rules);
        apply_rules(&mut results, &config);
        assert_eq!(results.total_issues(), 0);
    }

    #[test]
    fn apply_rules_off_each_type_individually() {
        let field_setters: Vec<(RuleFieldSetter, ResultFieldCheck)> = vec![
            (
                |r| r.unused_files = Severity::Off,
                |res| res.unused_files.is_empty(),
            ),
            (
                |r| r.unused_exports = Severity::Off,
                |res| res.unused_exports.is_empty(),
            ),
            (
                |r| r.unused_types = Severity::Off,
                |res| res.unused_types.is_empty(),
            ),
            (
                |r| r.private_type_leaks = Severity::Off,
                |res| res.private_type_leaks.is_empty(),
            ),
            (
                |r| r.unused_dependencies = Severity::Off,
                |res| res.unused_dependencies.is_empty(),
            ),
            (
                |r| r.unused_dev_dependencies = Severity::Off,
                |res| res.unused_dev_dependencies.is_empty(),
            ),
            (
                |r| r.unused_enum_members = Severity::Off,
                |res| res.unused_enum_members.is_empty(),
            ),
            (
                |r| r.unused_class_members = Severity::Off,
                |res| res.unused_class_members.is_empty(),
            ),
            (
                |r| r.unused_store_members = Severity::Off,
                |res| res.unused_store_members.is_empty(),
            ),
            (
                |r| r.unresolved_imports = Severity::Off,
                |res| res.unresolved_imports.is_empty(),
            ),
            (
                |r| r.unlisted_dependencies = Severity::Off,
                |res| res.unlisted_dependencies.is_empty(),
            ),
            (
                |r| r.duplicate_exports = Severity::Off,
                |res| res.duplicate_exports.is_empty(),
            ),
        ];

        for (set_off, check_empty) in field_setters {
            let mut results = make_results();
            let mut rules = RulesConfig::default();
            set_off(&mut rules);
            let config = config_with_rules(rules);
            apply_rules(&mut results, &config);
            assert!(
                check_empty(&results),
                "Setting a rule to Off should clear the corresponding results"
            );
        }
    }

    #[test]
    fn empty_results_no_error_issues() {
        let results = AnalysisResults::default();
        let rules = RulesConfig::default();
        assert!(!has_error_severity_issues(&results, &rules, None));
    }

    #[test]
    fn error_severity_with_issues_returns_true() {
        let results = make_results();
        let rules = RulesConfig::default(); // all Error
        assert!(has_error_severity_issues(&results, &rules, None));
    }

    #[test]
    fn warn_severity_with_issues_returns_false() {
        let results = make_results();
        let rules = RulesConfig {
            unused_files: Severity::Warn,
            unused_exports: Severity::Warn,
            unused_types: Severity::Warn,
            private_type_leaks: Severity::Warn,
            unused_dependencies: Severity::Warn,
            unused_dev_dependencies: Severity::Warn,
            unused_optional_dependencies: Severity::Warn,
            unused_enum_members: Severity::Warn,
            unused_class_members: Severity::Warn,
            unused_store_members: Severity::Warn,
            unprovided_injects: Severity::Warn,
            unrendered_components: Severity::Warn,
            unused_component_props: Severity::Warn,
            unused_component_emits: Severity::Warn,
            unused_server_actions: Severity::Warn,
            unresolved_imports: Severity::Warn,
            unlisted_dependencies: Severity::Warn,
            duplicate_exports: Severity::Warn,
            type_only_dependencies: Severity::Warn,
            test_only_dependencies: Severity::Warn,
            boundary_violation: Severity::Error,
            circular_dependencies: Severity::Warn,
            re_export_cycle: Severity::Warn,
            coverage_gaps: Severity::Warn,
            feature_flags: Severity::Warn,
            stale_suppressions: Severity::Warn,
            unused_catalog_entries: Severity::Warn,
            empty_catalog_groups: Severity::Warn,
            unresolved_catalog_references: Severity::Error,
            unused_dependency_overrides: Severity::Warn,
            misconfigured_dependency_overrides: Severity::Error,
            security_client_server_leak: Severity::Off,
            security_sink: Severity::Off,
            policy_violation: Severity::Warn,
            invalid_client_export: Severity::Warn,
            mixed_client_server_barrel: Severity::Warn,
            misplaced_directive: Severity::Warn,
            route_collision: Severity::Warn,
            dynamic_segment_name_conflict: Severity::Warn,
        };
        assert!(!has_error_severity_issues(&results, &rules, None));
    }

    #[test]
    fn mixed_severity_returns_true_for_error_with_issues() {
        let mut results = AnalysisResults::default();
        results
            .unused_files
            .push(UnusedFileFinding::with_actions(UnusedFile {
                path: PathBuf::from("/project/src/a.ts"),
            }));
        let mut rules = RulesConfig {
            unused_files: Severity::Warn,
            unused_exports: Severity::Warn,
            unused_types: Severity::Warn,
            private_type_leaks: Severity::Warn,
            unused_dependencies: Severity::Warn,
            unused_dev_dependencies: Severity::Warn,
            unused_optional_dependencies: Severity::Warn,
            unused_enum_members: Severity::Warn,
            unused_class_members: Severity::Warn,
            unused_store_members: Severity::Warn,
            unprovided_injects: Severity::Warn,
            unrendered_components: Severity::Warn,
            unused_component_props: Severity::Warn,
            unused_component_emits: Severity::Warn,
            unused_server_actions: Severity::Warn,
            unresolved_imports: Severity::Warn,
            unlisted_dependencies: Severity::Warn,
            duplicate_exports: Severity::Warn,
            type_only_dependencies: Severity::Warn,
            test_only_dependencies: Severity::Warn,
            boundary_violation: Severity::Error,
            circular_dependencies: Severity::Warn,
            re_export_cycle: Severity::Warn,
            coverage_gaps: Severity::Warn,
            feature_flags: Severity::Warn,
            stale_suppressions: Severity::Warn,
            unused_catalog_entries: Severity::Warn,
            empty_catalog_groups: Severity::Warn,
            unresolved_catalog_references: Severity::Error,
            unused_dependency_overrides: Severity::Warn,
            misconfigured_dependency_overrides: Severity::Error,
            security_client_server_leak: Severity::Off,
            security_sink: Severity::Off,
            policy_violation: Severity::Warn,
            invalid_client_export: Severity::Warn,
            mixed_client_server_barrel: Severity::Warn,
            misplaced_directive: Severity::Warn,
            route_collision: Severity::Warn,
            dynamic_segment_name_conflict: Severity::Warn,
        };
        assert!(!has_error_severity_issues(&results, &rules, None));

        rules.unused_files = Severity::Error;
        assert!(has_error_severity_issues(&results, &rules, None));
    }

    #[test]
    fn off_severity_with_issues_returns_false() {
        let mut results = AnalysisResults::default();
        results
            .unresolved_imports
            .push(UnresolvedImportFinding::with_actions(UnresolvedImport {
                path: PathBuf::from("/project/src/a.ts"),
                specifier: "./missing".into(),
                line: 1,
                col: 0,
                specifier_col: 0,
            }));
        let rules = RulesConfig {
            unresolved_imports: Severity::Off,
            ..RulesConfig::default()
        };
        assert!(!has_error_severity_issues(&results, &rules, None));
    }

    /// Build a ResolvedConfig with overrides that turn off unused_exports for test files.
    fn config_with_test_override() -> ResolvedConfig {
        fallow_config::FallowConfig {
            schema: None,
            extends: vec![],
            entry: vec![],
            ignore_patterns: vec![],
            framework: vec![],
            workspaces: None,
            ignore_dependencies: vec![],
            ignore_unresolved_imports: vec![],
            ignore_exports: vec![],
            ignore_catalog_references: vec![],
            ignore_dependency_overrides: vec![],
            ignore_exports_used_in_file: fallow_config::IgnoreExportsUsedInFileConfig::default(),
            used_class_members: vec![],
            ignore_decorators: vec![],
            duplicates: fallow_config::DuplicatesConfig::default(),
            health: fallow_config::HealthConfig::default(),
            rules: RulesConfig::default(), // all Error
            boundaries: fallow_config::BoundaryConfig::default(),
            production: false.into(),
            plugins: vec![],
            rule_packs: vec![],
            dynamically_loaded: vec![],
            regression: None,
            audit: fallow_config::AuditConfig::default(),
            codeowners: None,
            public_packages: vec![],
            flags: fallow_config::FlagsConfig::default(),
            security: fallow_config::SecurityConfig::default(),
            fix: fallow_config::FixConfig::default(),
            resolve: fallow_config::ResolveConfig::default(),
            sealed: false,
            include_entry_exports: false,
            auto_imports: false,
            cache: fallow_config::CacheConfig::default(),
            overrides: vec![fallow_config::ConfigOverride {
                files: vec!["**/*.test.ts".to_string()],
                rules: fallow_config::PartialRulesConfig {
                    unused_exports: Some(Severity::Off),
                    ..Default::default()
                },
            }],
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

    fn config_with_circular_override(pattern: &str, severity: Severity) -> ResolvedConfig {
        fallow_config::FallowConfig {
            schema: None,
            extends: vec![],
            entry: vec![],
            ignore_patterns: vec![],
            framework: vec![],
            workspaces: None,
            ignore_dependencies: vec![],
            ignore_unresolved_imports: vec![],
            ignore_exports: vec![],
            ignore_catalog_references: vec![],
            ignore_dependency_overrides: vec![],
            ignore_exports_used_in_file: fallow_config::IgnoreExportsUsedInFileConfig::default(),
            used_class_members: vec![],
            ignore_decorators: vec![],
            duplicates: fallow_config::DuplicatesConfig::default(),
            health: fallow_config::HealthConfig::default(),
            rules: RulesConfig::default(),
            boundaries: fallow_config::BoundaryConfig::default(),
            production: false.into(),
            plugins: vec![],
            rule_packs: vec![],
            dynamically_loaded: vec![],
            regression: None,
            audit: fallow_config::AuditConfig::default(),
            codeowners: None,
            public_packages: vec![],
            flags: fallow_config::FlagsConfig::default(),
            security: fallow_config::SecurityConfig::default(),
            fix: fallow_config::FixConfig::default(),
            resolve: fallow_config::ResolveConfig::default(),
            sealed: false,
            include_entry_exports: false,
            auto_imports: false,
            cache: fallow_config::CacheConfig::default(),
            overrides: vec![fallow_config::ConfigOverride {
                files: vec![pattern.to_string()],
                rules: fallow_config::PartialRulesConfig {
                    circular_dependencies: Some(severity),
                    ..Default::default()
                },
            }],
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

    fn config_with_boundary_override(pattern: &str, severity: Severity) -> ResolvedConfig {
        fallow_config::FallowConfig {
            schema: None,
            extends: vec![],
            entry: vec![],
            ignore_patterns: vec![],
            framework: vec![],
            workspaces: None,
            ignore_dependencies: vec![],
            ignore_unresolved_imports: vec![],
            ignore_exports: vec![],
            ignore_catalog_references: vec![],
            ignore_dependency_overrides: vec![],
            ignore_exports_used_in_file: fallow_config::IgnoreExportsUsedInFileConfig::default(),
            used_class_members: vec![],
            ignore_decorators: vec![],
            duplicates: fallow_config::DuplicatesConfig::default(),
            health: fallow_config::HealthConfig::default(),
            rules: RulesConfig::default(),
            boundaries: fallow_config::BoundaryConfig::default(),
            production: false.into(),
            plugins: vec![],
            rule_packs: vec![],
            dynamically_loaded: vec![],
            regression: None,
            audit: fallow_config::AuditConfig::default(),
            codeowners: None,
            public_packages: vec![],
            flags: fallow_config::FlagsConfig::default(),
            security: fallow_config::SecurityConfig::default(),
            fix: fallow_config::FixConfig::default(),
            resolve: fallow_config::ResolveConfig::default(),
            sealed: false,
            include_entry_exports: false,
            auto_imports: false,
            cache: fallow_config::CacheConfig::default(),
            overrides: vec![fallow_config::ConfigOverride {
                files: vec![pattern.to_string()],
                rules: fallow_config::PartialRulesConfig {
                    boundary_violation: Some(severity),
                    ..Default::default()
                },
            }],
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

    fn circular_dependency(files: &[&str]) -> CircularDependencyFinding {
        CircularDependencyFinding::with_actions(CircularDependency {
            files: files.iter().map(PathBuf::from).collect(),
            length: files.len(),
            line: 1,
            col: 0,
            edges: Vec::new(),
            is_cross_package: false,
        })
    }

    fn boundary_violation(path: &str) -> BoundaryViolationFinding {
        BoundaryViolationFinding::with_actions(BoundaryViolation {
            from_path: PathBuf::from(path),
            to_path: PathBuf::from("/project/src/db/query.ts"),
            from_zone: "ui".to_string(),
            to_zone: "db".to_string(),
            import_specifier: "../db/query".to_string(),
            line: 1,
            col: 0,
        })
    }

    fn boundary_coverage_violation(path: &str) -> BoundaryCoverageViolationFinding {
        BoundaryCoverageViolationFinding::with_actions(BoundaryCoverageViolation {
            path: PathBuf::from(path),
            line: 1,
            col: 0,
        })
    }

    #[test]
    fn apply_rules_with_override_filters_matching_files() {
        let mut results = AnalysisResults::default();
        results
            .unused_exports
            .push(UnusedExportFinding::with_actions(UnusedExport {
                path: PathBuf::from("/project/src/utils.test.ts"),
                export_name: "testHelper".into(),
                is_type_only: false,
                line: 1,
                col: 0,
                span_start: 0,
                is_re_export: false,
            }));
        results
            .unused_exports
            .push(UnusedExportFinding::with_actions(UnusedExport {
                path: PathBuf::from("/project/src/utils.ts"),
                export_name: "realExport".into(),
                is_type_only: false,
                line: 5,
                col: 0,
                span_start: 0,
                is_re_export: false,
            }));

        let config = config_with_test_override();
        apply_rules(&mut results, &config);

        assert_eq!(results.unused_exports.len(), 1);
        assert_eq!(results.unused_exports[0].export.export_name, "realExport");
    }

    #[test]
    fn apply_rules_with_override_preserves_non_matching_files() {
        let mut results = AnalysisResults::default();
        results
            .unused_files
            .push(UnusedFileFinding::with_actions(UnusedFile {
                path: PathBuf::from("/project/src/dead.ts"),
            }));

        let config = config_with_test_override();
        apply_rules(&mut results, &config);

        assert_eq!(results.unused_files.len(), 1);
    }

    #[test]
    fn apply_rules_with_override_filters_circular_cycle_when_all_files_off() {
        let mut results = AnalysisResults::default();
        results.circular_dependencies.push(circular_dependency(&[
            "/project/src/generated/a.ts",
            "/project/src/generated/b.ts",
        ]));

        let config = config_with_circular_override("src/generated/**", Severity::Off);
        apply_rules(&mut results, &config);

        assert!(results.circular_dependencies.is_empty());
    }

    #[test]
    fn apply_rules_with_override_preserves_circular_cycle_when_any_file_is_on() {
        let mut results = AnalysisResults::default();
        results.circular_dependencies.push(circular_dependency(&[
            "/project/src/generated/a.ts",
            "/project/src/live/b.ts",
        ]));

        let config = config_with_circular_override("src/generated/**", Severity::Off);
        apply_rules(&mut results, &config);

        assert_eq!(results.circular_dependencies.len(), 1);
    }

    #[test]
    fn apply_rules_with_override_filters_boundary_findings() {
        let mut results = AnalysisResults::default();
        results
            .boundary_violations
            .push(boundary_violation("/project/src/generated/a.ts"));
        results
            .boundary_coverage_violations
            .push(boundary_coverage_violation("/project/src/generated/a.ts"));

        let config = config_with_boundary_override("src/generated/**", Severity::Off);
        apply_rules(&mut results, &config);

        assert!(results.boundary_violations.is_empty());
        assert!(results.boundary_coverage_violations.is_empty());
    }

    #[test]
    fn apply_rules_with_override_preserves_unmatched_boundary_findings() {
        let mut results = AnalysisResults::default();
        results
            .boundary_violations
            .push(boundary_violation("/project/src/live/a.ts"));
        results
            .boundary_coverage_violations
            .push(boundary_coverage_violation("/project/src/live/a.ts"));

        let config = config_with_boundary_override("src/generated/**", Severity::Off);
        apply_rules(&mut results, &config);

        assert_eq!(results.boundary_violations.len(), 1);
        assert_eq!(results.boundary_coverage_violations.len(), 1);
    }

    #[test]
    fn has_error_with_override_per_file_resolution() {
        let mut results = AnalysisResults::default();
        results
            .unused_exports
            .push(UnusedExportFinding::with_actions(UnusedExport {
                path: PathBuf::from("/project/src/utils.test.ts"),
                export_name: "testHelper".into(),
                is_type_only: false,
                line: 1,
                col: 0,
                span_start: 0,
                is_re_export: false,
            }));

        let config = config_with_test_override();
        let rules = &config.rules;

        assert!(
            !has_error_severity_issues(&results, rules, Some(&config)),
            "test file override should suppress error"
        );
    }

    #[test]
    fn has_error_with_override_non_matching_file_still_error() {
        let mut results = AnalysisResults::default();
        results
            .unused_exports
            .push(UnusedExportFinding::with_actions(UnusedExport {
                path: PathBuf::from("/project/src/utils.ts"),
                export_name: "realExport".into(),
                is_type_only: false,
                line: 1,
                col: 0,
                span_start: 0,
                is_re_export: false,
            }));

        let config = config_with_test_override();
        let rules = &config.rules;

        assert!(
            has_error_severity_issues(&results, rules, Some(&config)),
            "non-test file should still have Error severity"
        );
    }

    #[test]
    fn has_error_with_override_circular_cycle_uses_file_severity() {
        let mut results = AnalysisResults::default();
        results.circular_dependencies.push(circular_dependency(&[
            "/project/src/generated/a.ts",
            "/project/src/generated/b.ts",
        ]));

        let config = config_with_circular_override("src/generated/**", Severity::Warn);
        let rules = &config.rules;

        assert!(
            !has_error_severity_issues(&results, rules, Some(&config)),
            "cycle files downgraded to Warn should not produce an Error verdict"
        );
    }

    #[test]
    fn has_error_with_override_circular_cycle_keeps_error_for_unmatched_file() {
        let mut results = AnalysisResults::default();
        results.circular_dependencies.push(circular_dependency(&[
            "/project/src/generated/a.ts",
            "/project/src/live/b.ts",
        ]));

        let config = config_with_circular_override("src/generated/**", Severity::Off);
        let rules = &config.rules;

        assert!(
            has_error_severity_issues(&results, rules, Some(&config)),
            "a cycle touching any Error-severity file should still fail"
        );
    }

    #[test]
    fn has_error_with_override_boundary_findings_use_file_severity() {
        let mut results = AnalysisResults::default();
        results
            .boundary_violations
            .push(boundary_violation("/project/src/generated/a.ts"));
        results
            .boundary_coverage_violations
            .push(boundary_coverage_violation("/project/src/generated/a.ts"));

        let config = config_with_boundary_override("src/generated/**", Severity::Warn);
        let rules = &config.rules;

        assert!(
            !has_error_severity_issues(&results, rules, Some(&config)),
            "boundary findings downgraded to Warn should not produce an Error verdict"
        );
    }

    #[test]
    fn promote_warns_to_errors_promotes_all_warns() {
        let mut rules = RulesConfig {
            unused_files: Severity::Warn,
            unused_exports: Severity::Warn,
            unused_types: Severity::Warn,
            private_type_leaks: Severity::Warn,
            unused_dependencies: Severity::Warn,
            unused_dev_dependencies: Severity::Warn,
            unused_optional_dependencies: Severity::Warn,
            unused_enum_members: Severity::Warn,
            unused_class_members: Severity::Warn,
            unused_store_members: Severity::Warn,
            unprovided_injects: Severity::Warn,
            unrendered_components: Severity::Warn,
            unused_component_props: Severity::Warn,
            unused_component_emits: Severity::Warn,
            unused_server_actions: Severity::Warn,
            unresolved_imports: Severity::Warn,
            unlisted_dependencies: Severity::Warn,
            duplicate_exports: Severity::Warn,
            type_only_dependencies: Severity::Warn,
            test_only_dependencies: Severity::Warn,
            boundary_violation: Severity::Error,
            circular_dependencies: Severity::Warn,
            re_export_cycle: Severity::Warn,
            coverage_gaps: Severity::Warn,
            feature_flags: Severity::Warn,
            stale_suppressions: Severity::Warn,
            unused_catalog_entries: Severity::Warn,
            empty_catalog_groups: Severity::Warn,
            unresolved_catalog_references: Severity::Error,
            unused_dependency_overrides: Severity::Warn,
            misconfigured_dependency_overrides: Severity::Error,
            security_client_server_leak: Severity::Off,
            security_sink: Severity::Off,
            policy_violation: Severity::Warn,
            invalid_client_export: Severity::Warn,
            mixed_client_server_barrel: Severity::Warn,
            misplaced_directive: Severity::Warn,
            route_collision: Severity::Warn,
            dynamic_segment_name_conflict: Severity::Warn,
        };
        promote_warns_to_errors(&mut rules);

        assert_eq!(rules.unused_files, Severity::Error);
        assert_eq!(rules.unused_exports, Severity::Error);
        assert_eq!(rules.unused_types, Severity::Error);
        assert_eq!(rules.private_type_leaks, Severity::Error);
        assert_eq!(rules.unused_dependencies, Severity::Error);
        assert_eq!(rules.unused_dev_dependencies, Severity::Error);
        assert_eq!(rules.unused_optional_dependencies, Severity::Error);
        assert_eq!(rules.unused_enum_members, Severity::Error);
        assert_eq!(rules.unused_class_members, Severity::Error);
        assert_eq!(rules.unused_store_members, Severity::Error);
        assert_eq!(rules.unresolved_imports, Severity::Error);
        assert_eq!(rules.unlisted_dependencies, Severity::Error);
        assert_eq!(rules.duplicate_exports, Severity::Error);
        assert_eq!(rules.type_only_dependencies, Severity::Error);
        assert_eq!(rules.test_only_dependencies, Severity::Error);
        assert_eq!(rules.circular_dependencies, Severity::Error);
        assert_eq!(rules.coverage_gaps, Severity::Error);
        assert_eq!(rules.unused_catalog_entries, Severity::Error);
    }

    #[test]
    fn promote_warns_to_errors_preserves_off() {
        let mut rules = RulesConfig {
            unused_files: Severity::Off,
            unused_exports: Severity::Off,
            unused_types: Severity::Off,
            private_type_leaks: Severity::Off,
            unused_dependencies: Severity::Off,
            unused_dev_dependencies: Severity::Off,
            unused_optional_dependencies: Severity::Off,
            unused_enum_members: Severity::Off,
            unused_class_members: Severity::Off,
            unused_store_members: Severity::Off,
            unprovided_injects: Severity::Off,
            unrendered_components: Severity::Off,
            unused_component_props: Severity::Off,
            unused_component_emits: Severity::Off,
            unused_server_actions: Severity::Off,
            unresolved_imports: Severity::Off,
            unlisted_dependencies: Severity::Off,
            duplicate_exports: Severity::Off,
            type_only_dependencies: Severity::Off,
            test_only_dependencies: Severity::Off,
            boundary_violation: Severity::Error,
            circular_dependencies: Severity::Off,
            re_export_cycle: Severity::Warn,
            coverage_gaps: Severity::Off,
            feature_flags: Severity::Off,
            stale_suppressions: Severity::Off,
            unused_catalog_entries: Severity::Off,
            empty_catalog_groups: Severity::Off,
            unresolved_catalog_references: Severity::Off,
            unused_dependency_overrides: Severity::Off,
            misconfigured_dependency_overrides: Severity::Off,
            security_client_server_leak: Severity::Off,
            security_sink: Severity::Off,
            policy_violation: Severity::Warn,
            invalid_client_export: Severity::Warn,
            mixed_client_server_barrel: Severity::Warn,
            misplaced_directive: Severity::Warn,
            route_collision: Severity::Warn,
            dynamic_segment_name_conflict: Severity::Warn,
        };
        promote_warns_to_errors(&mut rules);

        assert_eq!(rules.unused_files, Severity::Off);
        assert_eq!(rules.unused_exports, Severity::Off);
        assert_eq!(rules.unused_types, Severity::Off);
        assert_eq!(rules.private_type_leaks, Severity::Off);
        assert_eq!(rules.circular_dependencies, Severity::Off);
        assert_eq!(rules.coverage_gaps, Severity::Off);
    }

    #[test]
    fn promote_warns_to_errors_preserves_existing_errors() {
        let mut rules = RulesConfig::default(); // all Error
        promote_warns_to_errors(&mut rules);

        assert_eq!(rules.unused_files, Severity::Error);
        assert_eq!(rules.unused_exports, Severity::Error);
    }

    #[test]
    fn promote_warns_to_errors_mixed_severities() {
        let mut rules = RulesConfig {
            unused_files: Severity::Error,
            unused_exports: Severity::Warn,
            unused_types: Severity::Off,
            ..RulesConfig::default()
        };
        promote_warns_to_errors(&mut rules);

        assert_eq!(rules.unused_files, Severity::Error);
        assert_eq!(rules.unused_exports, Severity::Error);
        assert_eq!(rules.unused_types, Severity::Off);
    }

    #[test]
    fn has_error_circular_deps_detected() {
        let mut results = AnalysisResults::default();
        results
            .circular_dependencies
            .push(CircularDependencyFinding::with_actions(
                CircularDependency {
                    files: vec![
                        PathBuf::from("/project/src/a.ts"),
                        PathBuf::from("/project/src/b.ts"),
                    ],
                    length: 2,
                    line: 1,
                    col: 0,
                    edges: Vec::new(),
                    is_cross_package: false,
                },
            ));
        let rules = RulesConfig::default();
        assert!(has_error_severity_issues(&results, &rules, None));
    }

    #[test]
    fn has_error_circular_deps_warn_not_detected() {
        let mut results = AnalysisResults::default();
        results
            .circular_dependencies
            .push(CircularDependencyFinding::with_actions(
                CircularDependency {
                    files: vec![
                        PathBuf::from("/project/src/a.ts"),
                        PathBuf::from("/project/src/b.ts"),
                    ],
                    length: 2,
                    line: 1,
                    col: 0,
                    edges: Vec::new(),
                    is_cross_package: false,
                },
            ));
        let rules = RulesConfig {
            circular_dependencies: Severity::Warn,
            re_export_cycle: Severity::Warn,
            ..RulesConfig::default()
        };
        assert!(!has_error_severity_issues(&results, &rules, None));
    }

    #[test]
    fn has_error_optional_deps_warn_by_default() {
        let mut results = AnalysisResults::default();
        results
            .unused_optional_dependencies
            .push(UnusedOptionalDependencyFinding::with_actions(
                UnusedDependency {
                    package_name: "optional-pkg".into(),
                    location: DependencyLocation::OptionalDependencies,
                    path: PathBuf::from("/project/package.json"),
                    line: 5,
                    used_in_workspaces: Vec::new(),
                },
            ));
        let rules = RulesConfig::default();
        assert!(!has_error_severity_issues(&results, &rules, None));
    }

    #[test]
    fn has_error_optional_deps_detected_when_error() {
        let mut results = AnalysisResults::default();
        results
            .unused_optional_dependencies
            .push(UnusedOptionalDependencyFinding::with_actions(
                UnusedDependency {
                    package_name: "optional-pkg".into(),
                    location: DependencyLocation::OptionalDependencies,
                    path: PathBuf::from("/project/package.json"),
                    line: 5,
                    used_in_workspaces: Vec::new(),
                },
            ));
        let rules = RulesConfig {
            unused_optional_dependencies: Severity::Error,
            ..RulesConfig::default()
        };
        assert!(has_error_severity_issues(&results, &rules, None));
    }

    #[test]
    fn has_error_type_only_deps_warn_by_default() {
        let mut results = AnalysisResults::default();
        results
            .type_only_dependencies
            .push(TypeOnlyDependencyFinding::with_actions(
                TypeOnlyDependency {
                    package_name: "zod".into(),
                    path: PathBuf::from("/project/package.json"),
                    line: 8,
                },
            ));
        let rules = RulesConfig::default();
        assert!(!has_error_severity_issues(&results, &rules, None));
    }

    #[test]
    fn has_error_type_only_deps_detected_when_error() {
        let mut results = AnalysisResults::default();
        results
            .type_only_dependencies
            .push(TypeOnlyDependencyFinding::with_actions(
                TypeOnlyDependency {
                    package_name: "zod".into(),
                    path: PathBuf::from("/project/package.json"),
                    line: 8,
                },
            ));
        let rules = RulesConfig {
            type_only_dependencies: Severity::Error,
            ..RulesConfig::default()
        };
        assert!(has_error_severity_issues(&results, &rules, None));
    }
}
