use fallow_config::{ResolvedConfig, RulesConfig, Severity};

/// Remove issues whose effective severity is `Off` from the results.
///
/// When overrides are configured, per-file rule resolution is used for
/// file-scoped issue types. Circular dependencies resolve against every file in
/// the cycle. Non-file-scoped issues (unused deps, unlisted deps, duplicate
/// exports) use the base rules only.
pub fn apply_rules(results: &mut fallow_types::results::AnalysisResults, config: &ResolvedConfig) {
    let rules = &config.rules;
    let has_overrides = !config.overrides.is_empty();

    if has_overrides {
        apply_file_override_rules(results, config);
        apply_boundary_override_rules(results, config);
    } else {
        apply_base_file_rules(results, rules);
    }

    apply_base_collection_rules(results, rules);
}

fn apply_base_collection_rules(
    results: &mut fallow_types::results::AnalysisResults,
    rules: &RulesConfig,
) {
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
    results: &mut fallow_types::results::AnalysisResults,
    config: &ResolvedConfig,
) {
    apply_dead_code_override_rules(results, config);
    apply_catalog_override_rules(results, config);
    apply_framework_override_rules(results, config);
    apply_circular_override_rules(results, config);
}

fn apply_dead_code_override_rules(
    results: &mut fallow_types::results::AnalysisResults,
    config: &ResolvedConfig,
) {
    apply_core_dead_code_override_rules(results, config);
    apply_component_dead_code_override_rules(results, config);
}

/// Retain core (non-component) dead-code findings whose per-file rule is not Off.
fn apply_core_dead_code_override_rules(
    results: &mut fallow_types::results::AnalysisResults,
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
    results.unresolved_imports.retain(|i| {
        config
            .resolve_rules_for_path(&i.import.path)
            .unresolved_imports
            != Severity::Off
    });
}

/// Retain component-shaped dead-code findings whose per-file rule is not Off.
fn apply_component_dead_code_override_rules(
    results: &mut fallow_types::results::AnalysisResults,
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

fn apply_catalog_override_rules(
    results: &mut fallow_types::results::AnalysisResults,
    config: &ResolvedConfig,
) {
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

fn apply_framework_override_rules(
    results: &mut fallow_types::results::AnalysisResults,
    config: &ResolvedConfig,
) {
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

fn apply_circular_override_rules(
    results: &mut fallow_types::results::AnalysisResults,
    config: &ResolvedConfig,
) {
    results.circular_dependencies.retain(|c| {
        c.cycle
            .files
            .iter()
            .any(|path| config.resolve_rules_for_path(path).circular_dependencies != Severity::Off)
    });
}

fn apply_base_file_rules(
    results: &mut fallow_types::results::AnalysisResults,
    rules: &RulesConfig,
) {
    clear_base_core_dead_code(results, rules);
    clear_base_component_dead_code(results, rules);
    clear_base_suppression_and_framework(results, rules);
}

/// Clear core (non-component) dead-code findings whose base rule is Off.
fn clear_base_core_dead_code(
    results: &mut fallow_types::results::AnalysisResults,
    rules: &RulesConfig,
) {
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
fn clear_base_component_dead_code(
    results: &mut fallow_types::results::AnalysisResults,
    rules: &RulesConfig,
) {
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
fn clear_base_suppression_and_framework(
    results: &mut fallow_types::results::AnalysisResults,
    rules: &RulesConfig,
) {
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

fn apply_boundary_override_rules(
    results: &mut fallow_types::results::AnalysisResults,
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
    results: &fallow_types::results::AnalysisResults,
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
    results: &fallow_types::results::AnalysisResults,
    config: &ResolvedConfig,
) -> bool {
    has_override_dead_code_error(results, config)
        || has_override_catalog_boundary_error(results, config)
        || has_override_framework_error(results, config)
}

fn has_override_dead_code_error(
    results: &fallow_types::results::AnalysisResults,
    config: &ResolvedConfig,
) -> bool {
    has_override_core_dead_code_error(results, config)
        || has_override_component_dead_code_error(results, config)
}

/// Per-file Error check for the core (non-component) dead-code issue types.
fn has_override_core_dead_code_error(
    results: &fallow_types::results::AnalysisResults,
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
        || results.unresolved_imports.iter().any(|i| {
            config
                .resolve_rules_for_path(&i.import.path)
                .unresolved_imports
                == Severity::Error
        })
}

/// Per-file Error check for the component-shaped dead-code issue types.
fn has_override_component_dead_code_error(
    results: &fallow_types::results::AnalysisResults,
    config: &ResolvedConfig,
) -> bool {
    results.unrendered_components.iter().any(|c| {
        config
            .resolve_rules_for_path(&c.component.path)
            .unrendered_components
            == Severity::Error
    }) || results.unused_component_props.iter().any(|p| {
        config
            .resolve_rules_for_path(&p.prop.path)
            .unused_component_props
            == Severity::Error
    }) || results.unused_component_emits.iter().any(|e| {
        config
            .resolve_rules_for_path(&e.emit.path)
            .unused_component_emits
            == Severity::Error
    }) || results.unused_component_inputs.iter().any(|i| {
        config
            .resolve_rules_for_path(&i.input.path)
            .unused_component_inputs
            == Severity::Error
    }) || results.unused_component_outputs.iter().any(|o| {
        config
            .resolve_rules_for_path(&o.output.path)
            .unused_component_outputs
            == Severity::Error
    }) || results.unused_svelte_events.iter().any(|e| {
        config
            .resolve_rules_for_path(&e.event.path)
            .unused_svelte_events
            == Severity::Error
    }) || results.unused_server_actions.iter().any(|a| {
        config
            .resolve_rules_for_path(&a.action.path)
            .unused_server_actions
            == Severity::Error
    }) || results.unused_load_data_keys.iter().any(|k| {
        config
            .resolve_rules_for_path(&k.key.path)
            .unused_load_data_keys
            == Severity::Error
    })
}

fn has_override_catalog_boundary_error(
    results: &fallow_types::results::AnalysisResults,
    config: &ResolvedConfig,
) -> bool {
    results.stale_suppressions.iter().any(|s| {
        let rules = config.resolve_rules_for_path(&s.path);
        if s.missing_reason {
            rules.require_suppression_reason == Severity::Error
        } else {
            rules.stale_suppressions == Severity::Error
        }
    }) || results.unresolved_catalog_references.iter().any(|r| {
        config
            .resolve_rules_for_path(&r.reference.path)
            .unresolved_catalog_references
            == Severity::Error
    }) || results.empty_catalog_groups.iter().any(|g| {
        config
            .resolve_rules_for_path(&g.group.path)
            .empty_catalog_groups
            == Severity::Error
    }) || results.boundary_coverage_violations.iter().any(|v| {
        config
            .resolve_rules_for_path(&v.violation.path)
            .boundary_violation
            == Severity::Error
    }) || results.boundary_call_violations.iter().any(|v| {
        config
            .resolve_rules_for_path(&v.violation.path)
            .boundary_violation
            == Severity::Error
    }) || results.circular_dependencies.iter().any(|c| {
        c.cycle.files.iter().any(|path| {
            config.resolve_rules_for_path(path).circular_dependencies == Severity::Error
        })
    })
}

fn has_override_framework_error(
    results: &fallow_types::results::AnalysisResults,
    config: &ResolvedConfig,
) -> bool {
    results.invalid_client_exports.iter().any(|e| {
        config
            .resolve_rules_for_path(&e.export.path)
            .invalid_client_export
            == Severity::Error
    }) || results.mixed_client_server_barrels.iter().any(|b| {
        config
            .resolve_rules_for_path(&b.barrel.path)
            .mixed_client_server_barrel
            == Severity::Error
    }) || results.misplaced_directives.iter().any(|d| {
        config
            .resolve_rules_for_path(&d.directive_site.path)
            .misplaced_directive
            == Severity::Error
    }) || results.route_collisions.iter().any(|c| {
        config
            .resolve_rules_for_path(&c.collision.path)
            .route_collision
            == Severity::Error
    }) || results.dynamic_segment_name_conflicts.iter().any(|c| {
        config
            .resolve_rules_for_path(&c.conflict.path)
            .dynamic_segment_name_conflict
            == Severity::Error
    })
}

fn has_default_file_scoped_error(
    results: &fallow_types::results::AnalysisResults,
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
    results: &fallow_types::results::AnalysisResults,
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
pub fn promote_policy_finding_warns(results: &mut fallow_types::results::AnalysisResults) {
    use fallow_types::results::PolicyViolationSeverity;
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
    use fallow_types::extract::MemberKind;
    use fallow_types::output_dead_code::*;
    use fallow_types::results::*;
    use std::path::PathBuf;

    #[expect(
        clippy::too_many_lines,
        reason = "test fixture; linear setup/assert, length is not a maintainability concern"
    )]
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
            unused_component_props: fallow_config::UnusedComponentPropsConfig::default(),
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
            unused_component_inputs: Severity::Off,
            unused_component_outputs: Severity::Off,
            unused_svelte_events: Severity::Off,
            unused_server_actions: Severity::Off,
            unused_load_data_keys: Severity::Off,
            prop_drilling: Severity::Off,
            thin_wrapper: Severity::Off,
            duplicate_prop_shape: Severity::Off,
            css_token_drift: Severity::Off,
            css_duplicate_block: Severity::Off,
            css_selector_complexity: Severity::Off,
            css_dead_surface: Severity::Off,
            css_broken_reference: Severity::Off,
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
            require_suppression_reason: Severity::Off,
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
            unused_component_inputs: Severity::Warn,
            unused_component_outputs: Severity::Warn,
            unused_svelte_events: Severity::Warn,
            unused_server_actions: Severity::Warn,
            unused_load_data_keys: Severity::Warn,
            prop_drilling: Severity::Off,
            thin_wrapper: Severity::Off,
            duplicate_prop_shape: Severity::Off,
            css_token_drift: Severity::Off,
            css_duplicate_block: Severity::Off,
            css_selector_complexity: Severity::Off,
            css_dead_surface: Severity::Off,
            css_broken_reference: Severity::Off,
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
            require_suppression_reason: Severity::Warn,
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
            unused_component_inputs: Severity::Warn,
            unused_component_outputs: Severity::Warn,
            unused_svelte_events: Severity::Warn,
            unused_server_actions: Severity::Warn,
            unused_load_data_keys: Severity::Warn,
            prop_drilling: Severity::Off,
            thin_wrapper: Severity::Off,
            duplicate_prop_shape: Severity::Off,
            css_token_drift: Severity::Off,
            css_duplicate_block: Severity::Off,
            css_selector_complexity: Severity::Off,
            css_dead_surface: Severity::Off,
            css_broken_reference: Severity::Off,
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
            require_suppression_reason: Severity::Warn,
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
            unused_component_props: fallow_config::UnusedComponentPropsConfig::default(),
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
            unused_component_props: fallow_config::UnusedComponentPropsConfig::default(),
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
            unused_component_props: fallow_config::UnusedComponentPropsConfig::default(),
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
            unused_component_inputs: Severity::Warn,
            unused_component_outputs: Severity::Warn,
            unused_svelte_events: Severity::Warn,
            unused_server_actions: Severity::Warn,
            unused_load_data_keys: Severity::Warn,
            prop_drilling: Severity::Off,
            thin_wrapper: Severity::Off,
            duplicate_prop_shape: Severity::Off,
            css_token_drift: Severity::Off,
            css_duplicate_block: Severity::Off,
            css_selector_complexity: Severity::Off,
            css_dead_surface: Severity::Off,
            css_broken_reference: Severity::Off,
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
            require_suppression_reason: Severity::Warn,
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
            unused_component_inputs: Severity::Off,
            unused_component_outputs: Severity::Off,
            unused_svelte_events: Severity::Off,
            unused_server_actions: Severity::Off,
            unused_load_data_keys: Severity::Off,
            prop_drilling: Severity::Off,
            thin_wrapper: Severity::Off,
            duplicate_prop_shape: Severity::Off,
            css_token_drift: Severity::Off,
            css_duplicate_block: Severity::Off,
            css_selector_complexity: Severity::Off,
            css_dead_surface: Severity::Off,
            css_broken_reference: Severity::Off,
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
            require_suppression_reason: Severity::Off,
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

    // -------------------------------------------------------------------------
    // Helpers for the extended tests
    // -------------------------------------------------------------------------

    fn config_with_override_for_rule(
        pattern: &str,
        configure: impl FnOnce(&mut fallow_config::PartialRulesConfig),
    ) -> ResolvedConfig {
        let mut partial = fallow_config::PartialRulesConfig::default();
        configure(&mut partial);
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
            unused_component_props: fallow_config::UnusedComponentPropsConfig::default(),
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
                rules: partial,
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

    fn stale_suppression(path: &str, missing_reason: bool) -> StaleSuppression {
        StaleSuppression {
            path: PathBuf::from(path),
            line: 1,
            col: 0,
            origin: SuppressionOrigin::Comment {
                issue_kind: Some("unused-exports".to_string()),
                reason: if missing_reason {
                    None
                } else {
                    Some("no longer needed".to_string())
                },
                is_file_level: false,
                kind_known: true,
            },
            missing_reason,
            actions: StaleSuppression::actions_for(missing_reason),
        }
    }

    // -------------------------------------------------------------------------
    // Lines 51-61: apply_base_collection_rules - re_export_cycle / boundary_violation
    // / policy_violation / catalog rules / dep override rules
    // -------------------------------------------------------------------------

    #[test]
    fn base_collection_re_export_cycle_off_clears_findings() {
        let mut results = AnalysisResults::default();
        results
            .re_export_cycles
            .push(ReExportCycleFinding::with_actions(ReExportCycle {
                files: vec![
                    PathBuf::from("/project/src/a.ts"),
                    PathBuf::from("/project/src/b.ts"),
                ],
                kind: ReExportCycleKind::MultiNode,
            }));
        let rules = RulesConfig {
            re_export_cycle: Severity::Off,
            ..RulesConfig::default()
        };
        let config = config_with_rules(rules);
        apply_rules(&mut results, &config);
        assert!(results.re_export_cycles.is_empty());
    }

    #[test]
    fn base_collection_boundary_violation_off_clears_all_three_vecs() {
        let mut results = AnalysisResults::default();
        results
            .boundary_violations
            .push(boundary_violation("/project/src/a.ts"));
        results
            .boundary_coverage_violations
            .push(boundary_coverage_violation("/project/src/a.ts"));
        results
            .boundary_call_violations
            .push(BoundaryCallViolationFinding::with_actions(
                BoundaryCallViolation {
                    path: PathBuf::from("/project/src/a.ts"),
                    line: 1,
                    col: 0,
                    zone: "ui".to_string(),
                    callee: "fs.readFileSync".to_string(),
                    pattern: "fs.*".to_string(),
                },
            ));
        let rules = RulesConfig {
            boundary_violation: Severity::Off,
            ..RulesConfig::default()
        };
        let config = config_with_rules(rules);
        apply_rules(&mut results, &config);
        assert!(results.boundary_violations.is_empty());
        assert!(results.boundary_coverage_violations.is_empty());
        assert!(results.boundary_call_violations.is_empty());
    }

    #[test]
    fn base_collection_policy_violation_off_clears_findings() {
        let mut results = AnalysisResults::default();
        results
            .policy_violations
            .push(PolicyViolationFinding::with_actions(PolicyViolation {
                path: PathBuf::from("/project/src/a.ts"),
                line: 1,
                col: 0,
                pack: "my-pack".to_string(),
                rule_id: "no-exec".to_string(),
                kind: PolicyRuleKind::BannedCall,
                matched: "cp.exec".to_string(),
                severity: PolicyViolationSeverity::Error,
                message: None,
            }));
        let rules = RulesConfig {
            policy_violation: Severity::Off,
            ..RulesConfig::default()
        };
        let config = config_with_rules(rules);
        apply_rules(&mut results, &config);
        assert!(results.policy_violations.is_empty());
    }

    #[test]
    fn base_collection_unused_catalog_entries_off_clears_findings() {
        let mut results = AnalysisResults::default();
        results
            .unused_catalog_entries
            .push(UnusedCatalogEntryFinding::with_actions(
                UnusedCatalogEntry {
                    entry_name: "lodash".to_string(),
                    catalog_name: "default".to_string(),
                    path: PathBuf::from("/project/pnpm-workspace.yaml"),
                    line: 5,
                    hardcoded_consumers: vec![],
                },
            ));
        let rules = RulesConfig {
            unused_catalog_entries: Severity::Off,
            ..RulesConfig::default()
        };
        let config = config_with_rules(rules);
        apply_rules(&mut results, &config);
        assert!(results.unused_catalog_entries.is_empty());
    }

    #[test]
    fn base_collection_empty_catalog_groups_off_clears_findings() {
        let mut results = AnalysisResults::default();
        results
            .empty_catalog_groups
            .push(EmptyCatalogGroupFinding::with_actions(EmptyCatalogGroup {
                catalog_name: "tools".to_string(),
                path: PathBuf::from("/project/pnpm-workspace.yaml"),
                line: 10,
            }));
        let rules = RulesConfig {
            empty_catalog_groups: Severity::Off,
            ..RulesConfig::default()
        };
        let config = config_with_rules(rules);
        apply_rules(&mut results, &config);
        assert!(results.empty_catalog_groups.is_empty());
    }

    #[test]
    fn base_collection_unresolved_catalog_references_off_clears_findings() {
        let mut results = AnalysisResults::default();
        results.unresolved_catalog_references.push(
            UnresolvedCatalogReferenceFinding::with_actions(UnresolvedCatalogReference {
                entry_name: "missing-pkg".to_string(),
                catalog_name: "default".to_string(),
                path: PathBuf::from("/project/package.json"),
                line: 3,
                available_in_catalogs: vec![],
            }),
        );
        let rules = RulesConfig {
            unresolved_catalog_references: Severity::Off,
            ..RulesConfig::default()
        };
        let config = config_with_rules(rules);
        apply_rules(&mut results, &config);
        assert!(results.unresolved_catalog_references.is_empty());
    }

    #[test]
    fn base_collection_unused_dependency_overrides_off_clears_findings() {
        let mut results = AnalysisResults::default();
        results
            .unused_dependency_overrides
            .push(UnusedDependencyOverrideFinding::with_actions(
                UnusedDependencyOverride {
                    raw_key: "old-dep".to_string(),
                    target_package: "old-dep".to_string(),
                    parent_package: None,
                    version_constraint: None,
                    version_range: "^1.0.0".to_string(),
                    source: DependencyOverrideSource::PnpmWorkspaceYaml,
                    path: PathBuf::from("/project/pnpm-workspace.yaml"),
                    line: 7,
                    hint: None,
                },
            ));
        let rules = RulesConfig {
            unused_dependency_overrides: Severity::Off,
            ..RulesConfig::default()
        };
        let config = config_with_rules(rules);
        apply_rules(&mut results, &config);
        assert!(results.unused_dependency_overrides.is_empty());
    }

    #[test]
    fn base_collection_misconfigured_dependency_overrides_off_clears_findings() {
        let mut results = AnalysisResults::default();
        results.misconfigured_dependency_overrides.push(
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
        let rules = RulesConfig {
            misconfigured_dependency_overrides: Severity::Off,
            ..RulesConfig::default()
        };
        let config = config_with_rules(rules);
        apply_rules(&mut results, &config);
        assert!(results.misconfigured_dependency_overrides.is_empty());
    }

    // -------------------------------------------------------------------------
    // Lines 110-147: apply_core_dead_code_override_rules (with overrides active)
    // -------------------------------------------------------------------------

    #[test]
    fn override_drops_unused_types_for_matching_file() {
        let mut results = AnalysisResults::default();
        results
            .unused_types
            .push(UnusedTypeFinding::with_actions(UnusedExport {
                path: PathBuf::from("/project/src/generated/types.ts"),
                export_name: "GenType".to_string(),
                is_type_only: true,
                line: 1,
                col: 0,
                span_start: 0,
                is_re_export: false,
            }));
        let config = config_with_override_for_rule("src/generated/**", |p| {
            p.unused_types = Some(Severity::Off);
        });
        apply_rules(&mut results, &config);
        assert!(results.unused_types.is_empty());
    }

    #[test]
    fn override_keeps_unused_types_for_non_matching_file() {
        let mut results = AnalysisResults::default();
        results
            .unused_types
            .push(UnusedTypeFinding::with_actions(UnusedExport {
                path: PathBuf::from("/project/src/core/types.ts"),
                export_name: "CoreType".to_string(),
                is_type_only: true,
                line: 1,
                col: 0,
                span_start: 0,
                is_re_export: false,
            }));
        let config = config_with_override_for_rule("src/generated/**", |p| {
            p.unused_types = Some(Severity::Off);
        });
        apply_rules(&mut results, &config);
        assert_eq!(results.unused_types.len(), 1);
    }

    #[test]
    fn override_drops_private_type_leaks_for_matching_file() {
        let mut results = AnalysisResults::default();
        results
            .private_type_leaks
            .push(PrivateTypeLeakFinding::with_actions(PrivateTypeLeak {
                path: PathBuf::from("/project/src/generated/api.ts"),
                export_name: "publicFn".to_string(),
                type_name: "_PrivateHelper".to_string(),
                line: 5,
                col: 0,
                span_start: 0,
            }));
        let config = config_with_override_for_rule("src/generated/**", |p| {
            p.private_type_leaks = Some(Severity::Off);
        });
        apply_rules(&mut results, &config);
        assert!(results.private_type_leaks.is_empty());
    }

    #[test]
    fn override_drops_unused_enum_members_for_matching_file() {
        let mut results = AnalysisResults::default();
        results
            .unused_enum_members
            .push(UnusedEnumMemberFinding::with_actions(UnusedMember {
                path: PathBuf::from("/project/src/generated/enums.ts"),
                parent_name: "Status".to_string(),
                member_name: "Legacy".to_string(),
                kind: MemberKind::EnumMember,
                line: 8,
                col: 0,
            }));
        let config = config_with_override_for_rule("src/generated/**", |p| {
            p.unused_enum_members = Some(Severity::Off);
        });
        apply_rules(&mut results, &config);
        assert!(results.unused_enum_members.is_empty());
    }

    #[test]
    fn override_drops_unused_class_members_for_matching_file() {
        let mut results = AnalysisResults::default();
        results
            .unused_class_members
            .push(UnusedClassMemberFinding::with_actions(UnusedMember {
                path: PathBuf::from("/project/src/generated/service.ts"),
                parent_name: "MyService".to_string(),
                member_name: "_legacyHelper".to_string(),
                kind: MemberKind::ClassMethod,
                line: 20,
                col: 0,
            }));
        let config = config_with_override_for_rule("src/generated/**", |p| {
            p.unused_class_members = Some(Severity::Off);
        });
        apply_rules(&mut results, &config);
        assert!(results.unused_class_members.is_empty());
    }

    #[test]
    fn override_drops_unused_store_members_for_matching_file() {
        let mut results = AnalysisResults::default();
        results
            .unused_store_members
            .push(UnusedStoreMemberFinding::with_actions(UnusedMember {
                path: PathBuf::from("/project/src/generated/store.ts"),
                parent_name: "useGenStore".to_string(),
                member_name: "unusedAction".to_string(),
                kind: MemberKind::StoreMember,
                line: 12,
                col: 0,
            }));
        let config = config_with_override_for_rule("src/generated/**", |p| {
            p.unused_store_members = Some(Severity::Off);
        });
        apply_rules(&mut results, &config);
        assert!(results.unused_store_members.is_empty());
    }

    #[test]
    fn override_drops_unprovided_injects_for_matching_file() {
        let mut results = AnalysisResults::default();
        results
            .unprovided_injects
            .push(UnprovidedInjectFinding::with_actions(UnprovidedInject {
                path: PathBuf::from("/project/src/generated/child.vue"),
                key_name: "INJECT_KEY".to_string(),
                framework: "vue".to_string(),
                line: 3,
                col: 0,
            }));
        let config = config_with_override_for_rule("src/generated/**", |p| {
            p.unprovided_injects = Some(Severity::Off);
        });
        apply_rules(&mut results, &config);
        assert!(results.unprovided_injects.is_empty());
    }

    #[test]
    fn override_drops_unresolved_imports_for_matching_file() {
        let mut results = AnalysisResults::default();
        results
            .unresolved_imports
            .push(UnresolvedImportFinding::with_actions(UnresolvedImport {
                path: PathBuf::from("/project/src/generated/mod.ts"),
                specifier: "./missing-gen".to_string(),
                line: 1,
                col: 0,
                specifier_col: 0,
            }));
        let config = config_with_override_for_rule("src/generated/**", |p| {
            p.unresolved_imports = Some(Severity::Off);
        });
        apply_rules(&mut results, &config);
        assert!(results.unresolved_imports.is_empty());
    }

    // -------------------------------------------------------------------------
    // Lines 154-202: apply_component_dead_code_override_rules
    // -------------------------------------------------------------------------

    #[test]
    fn override_drops_unrendered_components_for_matching_file() {
        let mut results = AnalysisResults::default();
        results
            .unrendered_components
            .push(UnrenderedComponentFinding::with_actions(
                UnrenderedComponent {
                    path: PathBuf::from("/project/src/legacy/LegacyWidget.vue"),
                    component_name: "LegacyWidget".to_string(),
                    framework: "vue".to_string(),
                    reachable_via: None,
                    line: 1,
                    col: 0,
                },
            ));
        let config = config_with_override_for_rule("src/legacy/**", |p| {
            p.unrendered_components = Some(Severity::Off);
        });
        apply_rules(&mut results, &config);
        assert!(results.unrendered_components.is_empty());
    }

    #[test]
    fn override_drops_unused_component_props_for_matching_file() {
        let mut results = AnalysisResults::default();
        results
            .unused_component_props
            .push(UnusedComponentPropFinding::with_actions(
                UnusedComponentProp {
                    path: PathBuf::from("/project/src/legacy/Card.vue"),
                    component_name: "Card".to_string(),
                    prop_name: "unusedColor".to_string(),
                    line: 5,
                    col: 0,
                },
            ));
        let config = config_with_override_for_rule("src/legacy/**", |p| {
            p.unused_component_props = Some(Severity::Off);
        });
        apply_rules(&mut results, &config);
        assert!(results.unused_component_props.is_empty());
    }

    #[test]
    fn override_drops_unused_component_emits_for_matching_file() {
        let mut results = AnalysisResults::default();
        results
            .unused_component_emits
            .push(UnusedComponentEmitFinding::with_actions(
                UnusedComponentEmit {
                    path: PathBuf::from("/project/src/legacy/Button.vue"),
                    component_name: "Button".to_string(),
                    emit_name: "legacy-click".to_string(),
                    line: 7,
                    col: 0,
                },
            ));
        let config = config_with_override_for_rule("src/legacy/**", |p| {
            p.unused_component_emits = Some(Severity::Off);
        });
        apply_rules(&mut results, &config);
        assert!(results.unused_component_emits.is_empty());
    }

    #[test]
    fn override_drops_unused_component_inputs_for_matching_file() {
        let mut results = AnalysisResults::default();
        results
            .unused_component_inputs
            .push(UnusedComponentInputFinding::with_actions(
                UnusedComponentInput {
                    path: PathBuf::from("/project/src/legacy/table.component.ts"),
                    component_name: "table".to_string(),
                    input_name: "deprecated".to_string(),
                    line: 12,
                    col: 0,
                },
            ));
        let config = config_with_override_for_rule("src/legacy/**", |p| {
            p.unused_component_inputs = Some(Severity::Off);
        });
        apply_rules(&mut results, &config);
        assert!(results.unused_component_inputs.is_empty());
    }

    #[test]
    fn override_drops_unused_component_outputs_for_matching_file() {
        let mut results = AnalysisResults::default();
        results
            .unused_component_outputs
            .push(UnusedComponentOutputFinding::with_actions(
                UnusedComponentOutput {
                    path: PathBuf::from("/project/src/legacy/table.component.ts"),
                    component_name: "table".to_string(),
                    output_name: "deprecatedChange".to_string(),
                    line: 15,
                    col: 0,
                },
            ));
        let config = config_with_override_for_rule("src/legacy/**", |p| {
            p.unused_component_outputs = Some(Severity::Off);
        });
        apply_rules(&mut results, &config);
        assert!(results.unused_component_outputs.is_empty());
    }

    #[test]
    fn override_drops_unused_svelte_events_for_matching_file() {
        let mut results = AnalysisResults::default();
        results
            .unused_svelte_events
            .push(UnusedSvelteEventFinding::with_actions(UnusedSvelteEvent {
                path: PathBuf::from("/project/src/legacy/Widget.svelte"),
                component_name: "Widget".to_string(),
                event_name: "legacy".to_string(),
                line: 10,
                col: 0,
            }));
        let config = config_with_override_for_rule("src/legacy/**", |p| {
            p.unused_svelte_events = Some(Severity::Off);
        });
        apply_rules(&mut results, &config);
        assert!(results.unused_svelte_events.is_empty());
    }

    #[test]
    fn override_drops_unused_server_actions_for_matching_file() {
        let mut results = AnalysisResults::default();
        results
            .unused_server_actions
            .push(UnusedServerActionFinding::with_actions(
                UnusedServerAction {
                    path: PathBuf::from("/project/src/legacy/actions.ts"),
                    action_name: "deprecatedAction".to_string(),
                    line: 3,
                    col: 0,
                },
            ));
        let config = config_with_override_for_rule("src/legacy/**", |p| {
            p.unused_server_actions = Some(Severity::Off);
        });
        apply_rules(&mut results, &config);
        assert!(results.unused_server_actions.is_empty());
    }

    #[test]
    fn override_drops_unused_load_data_keys_for_matching_file() {
        let mut results = AnalysisResults::default();
        results
            .unused_load_data_keys
            .push(UnusedLoadDataKeyFinding::with_actions(UnusedLoadDataKey {
                path: PathBuf::from("/project/src/legacy/+page.server.ts"),
                key_name: "oldKey".to_string(),
                line: 4,
                col: 0,
                route_dir: None,
            }));
        let config = config_with_override_for_rule("src/legacy/**", |p| {
            p.unused_load_data_keys = Some(Severity::Off);
        });
        apply_rules(&mut results, &config);
        assert!(results.unused_load_data_keys.is_empty());
    }

    // -------------------------------------------------------------------------
    // Lines 208-240: apply_catalog_override_rules
    // -------------------------------------------------------------------------

    #[test]
    fn override_drops_stale_suppression_when_stale_rule_off() {
        let mut results = AnalysisResults::default();
        results
            .stale_suppressions
            .push(stale_suppression("/project/src/generated/a.ts", false));
        let config = config_with_override_for_rule("src/generated/**", |p| {
            p.stale_suppressions = Some(Severity::Off);
        });
        apply_rules(&mut results, &config);
        assert!(results.stale_suppressions.is_empty());
    }

    #[test]
    fn override_drops_missing_reason_suppression_when_require_reason_off() {
        let mut results = AnalysisResults::default();
        results
            .stale_suppressions
            .push(stale_suppression("/project/src/generated/b.ts", true));
        let config = config_with_override_for_rule("src/generated/**", |p| {
            p.require_suppression_reason = Some(Severity::Off);
        });
        apply_rules(&mut results, &config);
        assert!(results.stale_suppressions.is_empty());
    }

    #[test]
    fn override_keeps_stale_suppression_for_non_matching_file() {
        let mut results = AnalysisResults::default();
        results
            .stale_suppressions
            .push(stale_suppression("/project/src/core/a.ts", false));
        let config = config_with_override_for_rule("src/generated/**", |p| {
            p.stale_suppressions = Some(Severity::Off);
        });
        apply_rules(&mut results, &config);
        assert_eq!(results.stale_suppressions.len(), 1);
    }

    #[test]
    fn override_drops_unresolved_catalog_references_for_matching_file() {
        let mut results = AnalysisResults::default();
        results.unresolved_catalog_references.push(
            UnresolvedCatalogReferenceFinding::with_actions(UnresolvedCatalogReference {
                entry_name: "react".to_string(),
                catalog_name: "default".to_string(),
                path: PathBuf::from("/project/packages/app/package.json"),
                line: 5,
                available_in_catalogs: vec![],
            }),
        );
        let config = config_with_override_for_rule("packages/app/**", |p| {
            p.unresolved_catalog_references = Some(Severity::Off);
        });
        apply_rules(&mut results, &config);
        assert!(results.unresolved_catalog_references.is_empty());
    }

    #[test]
    fn override_drops_empty_catalog_groups_for_matching_file() {
        let mut results = AnalysisResults::default();
        results
            .empty_catalog_groups
            .push(EmptyCatalogGroupFinding::with_actions(EmptyCatalogGroup {
                catalog_name: "legacy".to_string(),
                path: PathBuf::from("/project/pnpm-workspace.yaml"),
                line: 20,
            }));
        let config = config_with_override_for_rule("pnpm-workspace.yaml", |p| {
            p.empty_catalog_groups = Some(Severity::Off);
        });
        apply_rules(&mut results, &config);
        assert!(results.empty_catalog_groups.is_empty());
    }

    #[test]
    fn override_drops_unused_dependency_overrides_for_matching_file() {
        let mut results = AnalysisResults::default();
        results
            .unused_dependency_overrides
            .push(UnusedDependencyOverrideFinding::with_actions(
                UnusedDependencyOverride {
                    raw_key: "old-pkg".to_string(),
                    target_package: "old-pkg".to_string(),
                    parent_package: None,
                    version_constraint: None,
                    version_range: "^2.0.0".to_string(),
                    source: DependencyOverrideSource::PnpmWorkspaceYaml,
                    path: PathBuf::from("/project/pnpm-workspace.yaml"),
                    line: 8,
                    hint: None,
                },
            ));
        let config = config_with_override_for_rule("pnpm-workspace.yaml", |p| {
            p.unused_dependency_overrides = Some(Severity::Off);
        });
        apply_rules(&mut results, &config);
        assert!(results.unused_dependency_overrides.is_empty());
    }

    #[test]
    fn override_drops_misconfigured_dependency_overrides_for_matching_file() {
        let mut results = AnalysisResults::default();
        results.misconfigured_dependency_overrides.push(
            MisconfiguredDependencyOverrideFinding::with_actions(MisconfiguredDependencyOverride {
                raw_key: "bad>".to_string(),
                target_package: None,
                raw_value: "1.0.0".to_string(),
                reason: DependencyOverrideMisconfigReason::UnparsableKey,
                source: DependencyOverrideSource::PnpmWorkspaceYaml,
                path: PathBuf::from("/project/pnpm-workspace.yaml"),
                line: 12,
            }),
        );
        let config = config_with_override_for_rule("pnpm-workspace.yaml", |p| {
            p.misconfigured_dependency_overrides = Some(Severity::Off);
        });
        apply_rules(&mut results, &config);
        assert!(results.misconfigured_dependency_overrides.is_empty());
    }

    // -------------------------------------------------------------------------
    // Lines 246-276: apply_framework_override_rules
    // -------------------------------------------------------------------------

    #[test]
    fn override_drops_invalid_client_exports_for_matching_file() {
        let mut results = AnalysisResults::default();
        results
            .invalid_client_exports
            .push(InvalidClientExportFinding::with_actions(
                InvalidClientExport {
                    path: PathBuf::from("/project/src/app/page.ts"),
                    export_name: "metadata".to_string(),
                    directive: "use client".to_string(),
                    line: 3,
                    col: 0,
                },
            ));
        let config = config_with_override_for_rule("src/app/**", |p| {
            p.invalid_client_export = Some(Severity::Off);
        });
        apply_rules(&mut results, &config);
        assert!(results.invalid_client_exports.is_empty());
    }

    #[test]
    fn override_drops_mixed_client_server_barrels_for_matching_file() {
        let mut results = AnalysisResults::default();
        results
            .mixed_client_server_barrels
            .push(MixedClientServerBarrelFinding::with_actions(
                MixedClientServerBarrel {
                    path: PathBuf::from("/project/src/app/index.ts"),
                    client_origin: "./client".to_string(),
                    server_origin: "./server".to_string(),
                    line: 2,
                    col: 0,
                },
            ));
        let config = config_with_override_for_rule("src/app/**", |p| {
            p.mixed_client_server_barrel = Some(Severity::Off);
        });
        apply_rules(&mut results, &config);
        assert!(results.mixed_client_server_barrels.is_empty());
    }

    #[test]
    fn override_drops_misplaced_directives_for_matching_file() {
        let mut results = AnalysisResults::default();
        results
            .misplaced_directives
            .push(MisplacedDirectiveFinding::with_actions(
                MisplacedDirective {
                    path: PathBuf::from("/project/src/app/widget.ts"),
                    directive: "use client".to_string(),
                    line: 5,
                    col: 0,
                },
            ));
        let config = config_with_override_for_rule("src/app/**", |p| {
            p.misplaced_directive = Some(Severity::Off);
        });
        apply_rules(&mut results, &config);
        assert!(results.misplaced_directives.is_empty());
    }

    #[test]
    fn override_drops_route_collisions_for_matching_file() {
        let mut results = AnalysisResults::default();
        results
            .route_collisions
            .push(RouteCollisionFinding::with_actions(RouteCollision {
                path: PathBuf::from("/project/src/app/(a)/about/page.tsx"),
                url: "/about".to_string(),
                conflicting_paths: vec![PathBuf::from("/project/src/app/(b)/about/page.tsx")],
                line: 1,
                col: 0,
            }));
        let config = config_with_override_for_rule("src/app/(a)/**", |p| {
            p.route_collision = Some(Severity::Off);
        });
        apply_rules(&mut results, &config);
        assert!(results.route_collisions.is_empty());
    }

    #[test]
    fn override_drops_dynamic_segment_name_conflicts_for_matching_file() {
        let mut results = AnalysisResults::default();
        results.dynamic_segment_name_conflicts.push(
            DynamicSegmentNameConflictFinding::with_actions(DynamicSegmentNameConflict {
                path: PathBuf::from("/project/src/app/routes/page.tsx"),
                position: "/".to_string(),
                conflicting_segments: vec!["[id]".to_string(), "[slug]".to_string()],
                conflicting_paths: vec![PathBuf::from("/project/src/app/other/page.tsx")],
                line: 1,
                col: 0,
            }),
        );
        let config = config_with_override_for_rule("src/app/**", |p| {
            p.dynamic_segment_name_conflict = Some(Severity::Off);
        });
        apply_rules(&mut results, &config);
        assert!(results.dynamic_segment_name_conflicts.is_empty());
    }

    // -------------------------------------------------------------------------
    // Lines 312-313: clear_base_core_dead_code - private_type_leaks Off
    // -------------------------------------------------------------------------

    #[test]
    fn base_private_type_leaks_off_clears_findings() {
        let mut results = AnalysisResults::default();
        results
            .private_type_leaks
            .push(PrivateTypeLeakFinding::with_actions(PrivateTypeLeak {
                path: PathBuf::from("/project/src/api.ts"),
                export_name: "publicFn".to_string(),
                type_name: "_Internal".to_string(),
                line: 3,
                col: 0,
                span_start: 0,
            }));
        let rules = RulesConfig {
            private_type_leaks: Severity::Off,
            ..RulesConfig::default()
        };
        let config = config_with_rules(rules);
        apply_rules(&mut results, &config);
        assert!(results.private_type_leaks.is_empty());
    }

    // -------------------------------------------------------------------------
    // Lines 367-388: clear_base_suppression_and_framework - stale_suppressions retain
    // and base framework Off clears
    // -------------------------------------------------------------------------

    #[test]
    fn base_stale_suppression_off_clears_non_missing_reason_suppressions() {
        let mut results = AnalysisResults::default();
        results
            .stale_suppressions
            .push(stale_suppression("/project/src/a.ts", false));
        let rules = RulesConfig {
            stale_suppressions: Severity::Off,
            ..RulesConfig::default()
        };
        let config = config_with_rules(rules);
        apply_rules(&mut results, &config);
        assert!(results.stale_suppressions.is_empty());
    }

    #[test]
    fn base_require_suppression_reason_off_clears_missing_reason_suppressions() {
        let mut results = AnalysisResults::default();
        results
            .stale_suppressions
            .push(stale_suppression("/project/src/a.ts", true));
        let rules = RulesConfig {
            require_suppression_reason: Severity::Off,
            ..RulesConfig::default()
        };
        let config = config_with_rules(rules);
        apply_rules(&mut results, &config);
        assert!(results.stale_suppressions.is_empty());
    }

    #[test]
    fn base_require_suppression_reason_off_keeps_normal_stale_suppressions() {
        let mut results = AnalysisResults::default();
        results
            .stale_suppressions
            .push(stale_suppression("/project/src/a.ts", false));
        let rules = RulesConfig {
            require_suppression_reason: Severity::Off,
            ..RulesConfig::default()
        };
        let config = config_with_rules(rules);
        apply_rules(&mut results, &config);
        // stale_suppressions rule is still Error (default), so non-missing-reason stays
        assert_eq!(results.stale_suppressions.len(), 1);
    }

    #[test]
    fn base_invalid_client_export_off_clears_findings() {
        let mut results = AnalysisResults::default();
        results
            .invalid_client_exports
            .push(InvalidClientExportFinding::with_actions(
                InvalidClientExport {
                    path: PathBuf::from("/project/src/page.ts"),
                    export_name: "metadata".to_string(),
                    directive: "use client".to_string(),
                    line: 1,
                    col: 0,
                },
            ));
        let rules = RulesConfig {
            invalid_client_export: Severity::Off,
            ..RulesConfig::default()
        };
        let config = config_with_rules(rules);
        apply_rules(&mut results, &config);
        assert!(results.invalid_client_exports.is_empty());
    }

    #[test]
    fn base_mixed_client_server_barrel_off_clears_findings() {
        let mut results = AnalysisResults::default();
        results
            .mixed_client_server_barrels
            .push(MixedClientServerBarrelFinding::with_actions(
                MixedClientServerBarrel {
                    path: PathBuf::from("/project/src/index.ts"),
                    client_origin: "./client".to_string(),
                    server_origin: "./server".to_string(),
                    line: 1,
                    col: 0,
                },
            ));
        let rules = RulesConfig {
            mixed_client_server_barrel: Severity::Off,
            ..RulesConfig::default()
        };
        let config = config_with_rules(rules);
        apply_rules(&mut results, &config);
        assert!(results.mixed_client_server_barrels.is_empty());
    }

    #[test]
    fn base_misplaced_directive_off_clears_findings() {
        let mut results = AnalysisResults::default();
        results
            .misplaced_directives
            .push(MisplacedDirectiveFinding::with_actions(
                MisplacedDirective {
                    path: PathBuf::from("/project/src/widget.ts"),
                    directive: "use client".to_string(),
                    line: 5,
                    col: 0,
                },
            ));
        let rules = RulesConfig {
            misplaced_directive: Severity::Off,
            ..RulesConfig::default()
        };
        let config = config_with_rules(rules);
        apply_rules(&mut results, &config);
        assert!(results.misplaced_directives.is_empty());
    }

    #[test]
    fn base_route_collision_off_clears_findings() {
        let mut results = AnalysisResults::default();
        results
            .route_collisions
            .push(RouteCollisionFinding::with_actions(RouteCollision {
                path: PathBuf::from("/project/src/app/(a)/about/page.tsx"),
                url: "/about".to_string(),
                conflicting_paths: vec![PathBuf::from("/project/src/app/(b)/about/page.tsx")],
                line: 1,
                col: 0,
            }));
        let rules = RulesConfig {
            route_collision: Severity::Off,
            ..RulesConfig::default()
        };
        let config = config_with_rules(rules);
        apply_rules(&mut results, &config);
        assert!(results.route_collisions.is_empty());
    }

    #[test]
    fn base_dynamic_segment_name_conflict_off_clears_findings() {
        let mut results = AnalysisResults::default();
        results.dynamic_segment_name_conflicts.push(
            DynamicSegmentNameConflictFinding::with_actions(DynamicSegmentNameConflict {
                path: PathBuf::from("/project/src/app/[id]/page.tsx"),
                position: "/".to_string(),
                conflicting_segments: vec!["[id]".to_string(), "[slug]".to_string()],
                conflicting_paths: vec![PathBuf::from("/project/src/app/[slug]/page.tsx")],
                line: 1,
                col: 0,
            }),
        );
        let rules = RulesConfig {
            dynamic_segment_name_conflict: Severity::Off,
            ..RulesConfig::default()
        };
        let config = config_with_rules(rules);
        apply_rules(&mut results, &config);
        assert!(results.dynamic_segment_name_conflicts.is_empty());
    }

    // -------------------------------------------------------------------------
    // Lines 407-419: apply_boundary_override_rules - boundary_call_violations
    // and policy_violations with override
    // -------------------------------------------------------------------------

    #[test]
    fn override_drops_boundary_call_violations_for_matching_file() {
        let mut results = AnalysisResults::default();
        results
            .boundary_call_violations
            .push(BoundaryCallViolationFinding::with_actions(
                BoundaryCallViolation {
                    path: PathBuf::from("/project/src/ui/Component.ts"),
                    line: 10,
                    col: 0,
                    zone: "ui".to_string(),
                    callee: "cp.exec".to_string(),
                    pattern: "child_process.*".to_string(),
                },
            ));
        let config = config_with_override_for_rule("src/ui/**", |p| {
            p.boundary_violation = Some(Severity::Off);
        });
        apply_rules(&mut results, &config);
        assert!(results.boundary_call_violations.is_empty());
    }

    #[test]
    fn override_keeps_boundary_call_violations_for_non_matching_file() {
        let mut results = AnalysisResults::default();
        results
            .boundary_call_violations
            .push(BoundaryCallViolationFinding::with_actions(
                BoundaryCallViolation {
                    path: PathBuf::from("/project/src/core/Service.ts"),
                    line: 10,
                    col: 0,
                    zone: "core".to_string(),
                    callee: "cp.exec".to_string(),
                    pattern: "child_process.*".to_string(),
                },
            ));
        let config = config_with_override_for_rule("src/ui/**", |p| {
            p.boundary_violation = Some(Severity::Off);
        });
        apply_rules(&mut results, &config);
        assert_eq!(results.boundary_call_violations.len(), 1);
    }

    #[test]
    fn override_drops_policy_violations_for_matching_file() {
        let mut results = AnalysisResults::default();
        results
            .policy_violations
            .push(PolicyViolationFinding::with_actions(PolicyViolation {
                path: PathBuf::from("/project/src/ui/Component.ts"),
                line: 5,
                col: 0,
                pack: "no-exec".to_string(),
                rule_id: "exec-banned".to_string(),
                kind: PolicyRuleKind::BannedCall,
                matched: "cp.exec".to_string(),
                severity: PolicyViolationSeverity::Error,
                message: None,
            }));
        let config = config_with_override_for_rule("src/ui/**", |p| {
            p.policy_violation = Some(Severity::Off);
        });
        apply_rules(&mut results, &config);
        assert!(results.policy_violations.is_empty());
    }

    // -------------------------------------------------------------------------
    // Lines 467-511: has_override_core_dead_code_error - per-file Error check
    // -------------------------------------------------------------------------

    #[test]
    fn has_error_override_unused_types_error_for_matching_file() {
        let mut results = AnalysisResults::default();
        results
            .unused_types
            .push(UnusedTypeFinding::with_actions(UnusedExport {
                path: PathBuf::from("/project/src/core/types.ts"),
                export_name: "CoreType".to_string(),
                is_type_only: true,
                line: 1,
                col: 0,
                span_start: 0,
                is_re_export: false,
            }));
        let config = config_with_override_for_rule("src/generated/**", |p| {
            p.unused_types = Some(Severity::Off);
        });
        let rules = &config.rules;
        assert!(has_error_severity_issues(&results, rules, Some(&config)));
    }

    #[test]
    fn has_error_override_unused_types_off_for_matching_file() {
        let mut results = AnalysisResults::default();
        results
            .unused_types
            .push(UnusedTypeFinding::with_actions(UnusedExport {
                path: PathBuf::from("/project/src/generated/types.ts"),
                export_name: "GenType".to_string(),
                is_type_only: true,
                line: 1,
                col: 0,
                span_start: 0,
                is_re_export: false,
            }));
        let config = config_with_override_for_rule("src/generated/**", |p| {
            p.unused_types = Some(Severity::Off);
        });
        let rules = &config.rules;
        assert!(!has_error_severity_issues(&results, rules, Some(&config)));
    }

    #[test]
    fn has_error_override_private_type_leaks_off_for_matching_file() {
        // private_type_leaks base default is Off; override sets it to Warn for generated.
        // A non-generated file keeps Off; the matching file with Off still produces no error.
        let mut results = AnalysisResults::default();
        results
            .private_type_leaks
            .push(PrivateTypeLeakFinding::with_actions(PrivateTypeLeak {
                path: PathBuf::from("/project/src/generated/api.ts"),
                export_name: "publicFn".to_string(),
                type_name: "_Internal".to_string(),
                line: 3,
                col: 0,
                span_start: 0,
            }));
        // Override generated files to Warn (not Error); should not produce an error.
        let config = config_with_override_for_rule("src/generated/**", |p| {
            p.private_type_leaks = Some(Severity::Warn);
        });
        let rules = &config.rules;
        assert!(!has_error_severity_issues(&results, rules, Some(&config)));
    }

    #[test]
    fn has_error_override_unused_enum_members_error_for_non_matching_file() {
        let mut results = AnalysisResults::default();
        results
            .unused_enum_members
            .push(UnusedEnumMemberFinding::with_actions(UnusedMember {
                path: PathBuf::from("/project/src/core/enums.ts"),
                parent_name: "Status".to_string(),
                member_name: "Legacy".to_string(),
                kind: MemberKind::EnumMember,
                line: 8,
                col: 0,
            }));
        let config = config_with_override_for_rule("src/generated/**", |p| {
            p.unused_enum_members = Some(Severity::Off);
        });
        let rules = &config.rules;
        assert!(has_error_severity_issues(&results, rules, Some(&config)));
    }

    #[test]
    fn has_error_override_unused_class_members_off_for_matching_file() {
        let mut results = AnalysisResults::default();
        results
            .unused_class_members
            .push(UnusedClassMemberFinding::with_actions(UnusedMember {
                path: PathBuf::from("/project/src/generated/service.ts"),
                parent_name: "Service".to_string(),
                member_name: "_hidden".to_string(),
                kind: MemberKind::ClassMethod,
                line: 20,
                col: 0,
            }));
        let config = config_with_override_for_rule("src/generated/**", |p| {
            p.unused_class_members = Some(Severity::Off);
        });
        let rules = &config.rules;
        assert!(!has_error_severity_issues(&results, rules, Some(&config)));
    }

    #[test]
    fn has_error_override_unused_store_members_off_for_matching_file() {
        let mut results = AnalysisResults::default();
        results
            .unused_store_members
            .push(UnusedStoreMemberFinding::with_actions(UnusedMember {
                path: PathBuf::from("/project/src/generated/store.ts"),
                parent_name: "useGenStore".to_string(),
                member_name: "unused".to_string(),
                kind: MemberKind::StoreMember,
                line: 5,
                col: 0,
            }));
        let config = config_with_override_for_rule("src/generated/**", |p| {
            p.unused_store_members = Some(Severity::Off);
        });
        let rules = &config.rules;
        assert!(!has_error_severity_issues(&results, rules, Some(&config)));
    }

    #[test]
    fn has_error_override_unprovided_injects_off_for_matching_file() {
        let mut results = AnalysisResults::default();
        results
            .unprovided_injects
            .push(UnprovidedInjectFinding::with_actions(UnprovidedInject {
                path: PathBuf::from("/project/src/generated/child.vue"),
                key_name: "KEY".to_string(),
                framework: "vue".to_string(),
                line: 2,
                col: 0,
            }));
        let config = config_with_override_for_rule("src/generated/**", |p| {
            p.unprovided_injects = Some(Severity::Off);
        });
        let rules = &config.rules;
        assert!(!has_error_severity_issues(&results, rules, Some(&config)));
    }

    #[test]
    fn has_error_override_unresolved_imports_off_for_matching_file() {
        let mut results = AnalysisResults::default();
        results
            .unresolved_imports
            .push(UnresolvedImportFinding::with_actions(UnresolvedImport {
                path: PathBuf::from("/project/src/generated/mod.ts"),
                specifier: "./missing".to_string(),
                line: 1,
                col: 0,
                specifier_col: 0,
            }));
        let config = config_with_override_for_rule("src/generated/**", |p| {
            p.unresolved_imports = Some(Severity::Off);
        });
        let rules = &config.rules;
        assert!(!has_error_severity_issues(&results, rules, Some(&config)));
    }

    // -------------------------------------------------------------------------
    // Lines 518-559: has_override_component_dead_code_error
    // -------------------------------------------------------------------------

    #[test]
    fn has_error_override_unrendered_components_warn_for_non_matching_file() {
        // unrendered_components base default is Warn (not Error), so non-matching
        // files do not produce an error finding even when an override turns legacy files Off.
        let mut results = AnalysisResults::default();
        results
            .unrendered_components
            .push(UnrenderedComponentFinding::with_actions(
                UnrenderedComponent {
                    path: PathBuf::from("/project/src/core/Widget.vue"),
                    component_name: "Widget".to_string(),
                    framework: "vue".to_string(),
                    reachable_via: None,
                    line: 1,
                    col: 0,
                },
            ));
        let config = config_with_override_for_rule("src/legacy/**", |p| {
            p.unrendered_components = Some(Severity::Off);
        });
        let rules = &config.rules;
        // Base default is Warn, so a non-matched file is also Warn: no error.
        assert!(!has_error_severity_issues(&results, rules, Some(&config)));
    }

    #[test]
    fn has_error_override_unused_component_props_off_for_matching_file() {
        let mut results = AnalysisResults::default();
        results
            .unused_component_props
            .push(UnusedComponentPropFinding::with_actions(
                UnusedComponentProp {
                    path: PathBuf::from("/project/src/legacy/Card.vue"),
                    component_name: "Card".to_string(),
                    prop_name: "oldProp".to_string(),
                    line: 3,
                    col: 0,
                },
            ));
        let config = config_with_override_for_rule("src/legacy/**", |p| {
            p.unused_component_props = Some(Severity::Off);
        });
        let rules = &config.rules;
        assert!(!has_error_severity_issues(&results, rules, Some(&config)));
    }

    #[test]
    fn has_error_override_unused_component_emits_off_for_matching_file() {
        let mut results = AnalysisResults::default();
        results
            .unused_component_emits
            .push(UnusedComponentEmitFinding::with_actions(
                UnusedComponentEmit {
                    path: PathBuf::from("/project/src/legacy/Button.vue"),
                    component_name: "Button".to_string(),
                    emit_name: "old-click".to_string(),
                    line: 6,
                    col: 0,
                },
            ));
        let config = config_with_override_for_rule("src/legacy/**", |p| {
            p.unused_component_emits = Some(Severity::Off);
        });
        let rules = &config.rules;
        assert!(!has_error_severity_issues(&results, rules, Some(&config)));
    }

    #[test]
    fn has_error_override_unused_component_inputs_off_for_matching_file() {
        let mut results = AnalysisResults::default();
        results
            .unused_component_inputs
            .push(UnusedComponentInputFinding::with_actions(
                UnusedComponentInput {
                    path: PathBuf::from("/project/src/legacy/table.component.ts"),
                    component_name: "table".to_string(),
                    input_name: "deprecated".to_string(),
                    line: 10,
                    col: 0,
                },
            ));
        let config = config_with_override_for_rule("src/legacy/**", |p| {
            p.unused_component_inputs = Some(Severity::Off);
        });
        let rules = &config.rules;
        assert!(!has_error_severity_issues(&results, rules, Some(&config)));
    }

    #[test]
    fn has_error_override_unused_component_outputs_off_for_matching_file() {
        let mut results = AnalysisResults::default();
        results
            .unused_component_outputs
            .push(UnusedComponentOutputFinding::with_actions(
                UnusedComponentOutput {
                    path: PathBuf::from("/project/src/legacy/table.component.ts"),
                    component_name: "table".to_string(),
                    output_name: "legacyChange".to_string(),
                    line: 14,
                    col: 0,
                },
            ));
        let config = config_with_override_for_rule("src/legacy/**", |p| {
            p.unused_component_outputs = Some(Severity::Off);
        });
        let rules = &config.rules;
        assert!(!has_error_severity_issues(&results, rules, Some(&config)));
    }

    #[test]
    fn has_error_override_unused_svelte_events_off_for_matching_file() {
        let mut results = AnalysisResults::default();
        results
            .unused_svelte_events
            .push(UnusedSvelteEventFinding::with_actions(UnusedSvelteEvent {
                path: PathBuf::from("/project/src/legacy/Widget.svelte"),
                component_name: "Widget".to_string(),
                event_name: "oldEvent".to_string(),
                line: 8,
                col: 0,
            }));
        let config = config_with_override_for_rule("src/legacy/**", |p| {
            p.unused_svelte_events = Some(Severity::Off);
        });
        let rules = &config.rules;
        assert!(!has_error_severity_issues(&results, rules, Some(&config)));
    }

    #[test]
    fn has_error_override_unused_server_actions_off_for_matching_file() {
        let mut results = AnalysisResults::default();
        results
            .unused_server_actions
            .push(UnusedServerActionFinding::with_actions(
                UnusedServerAction {
                    path: PathBuf::from("/project/src/legacy/actions.ts"),
                    action_name: "oldAction".to_string(),
                    line: 3,
                    col: 0,
                },
            ));
        let config = config_with_override_for_rule("src/legacy/**", |p| {
            p.unused_server_actions = Some(Severity::Off);
        });
        let rules = &config.rules;
        assert!(!has_error_severity_issues(&results, rules, Some(&config)));
    }

    #[test]
    fn has_error_override_unused_load_data_keys_off_for_matching_file() {
        let mut results = AnalysisResults::default();
        results
            .unused_load_data_keys
            .push(UnusedLoadDataKeyFinding::with_actions(UnusedLoadDataKey {
                path: PathBuf::from("/project/src/legacy/+page.server.ts"),
                key_name: "oldKey".to_string(),
                line: 2,
                col: 0,
                route_dir: None,
            }));
        let config = config_with_override_for_rule("src/legacy/**", |p| {
            p.unused_load_data_keys = Some(Severity::Off);
        });
        let rules = &config.rules;
        assert!(!has_error_severity_issues(&results, rules, Some(&config)));
    }

    // -------------------------------------------------------------------------
    // Lines 565-592: has_override_catalog_boundary_error
    // -------------------------------------------------------------------------

    #[test]
    fn has_error_override_stale_suppression_missing_reason_not_error_for_non_matching_file() {
        // require_suppression_reason base default is Off, so even a non-matching file
        // does not produce an error for missing-reason suppressions.
        let mut results = AnalysisResults::default();
        results
            .stale_suppressions
            .push(stale_suppression("/project/src/core/a.ts", true));
        let config = config_with_override_for_rule("src/generated/**", |p| {
            p.require_suppression_reason = Some(Severity::Off);
        });
        let rules = &config.rules;
        assert!(!has_error_severity_issues(&results, rules, Some(&config)));
    }

    #[test]
    fn has_error_override_stale_suppression_missing_reason_off_for_matching_file() {
        let mut results = AnalysisResults::default();
        results
            .stale_suppressions
            .push(stale_suppression("/project/src/generated/b.ts", true));
        let config = config_with_override_for_rule("src/generated/**", |p| {
            p.require_suppression_reason = Some(Severity::Off);
        });
        let rules = &config.rules;
        assert!(!has_error_severity_issues(&results, rules, Some(&config)));
    }

    #[test]
    fn has_error_override_stale_suppression_off_for_matching_file() {
        let mut results = AnalysisResults::default();
        results
            .stale_suppressions
            .push(stale_suppression("/project/src/generated/a.ts", false));
        let config = config_with_override_for_rule("src/generated/**", |p| {
            p.stale_suppressions = Some(Severity::Off);
        });
        let rules = &config.rules;
        assert!(!has_error_severity_issues(&results, rules, Some(&config)));
    }

    #[test]
    fn has_error_override_unresolved_catalog_references_off_for_matching_file() {
        let mut results = AnalysisResults::default();
        results.unresolved_catalog_references.push(
            UnresolvedCatalogReferenceFinding::with_actions(UnresolvedCatalogReference {
                entry_name: "react".to_string(),
                catalog_name: "default".to_string(),
                path: PathBuf::from("/project/packages/app/package.json"),
                line: 5,
                available_in_catalogs: vec![],
            }),
        );
        let config = config_with_override_for_rule("packages/app/**", |p| {
            p.unresolved_catalog_references = Some(Severity::Off);
        });
        let rules = &config.rules;
        assert!(!has_error_severity_issues(&results, rules, Some(&config)));
    }

    #[test]
    fn has_error_override_empty_catalog_groups_off_for_matching_file() {
        let mut results = AnalysisResults::default();
        results
            .empty_catalog_groups
            .push(EmptyCatalogGroupFinding::with_actions(EmptyCatalogGroup {
                catalog_name: "tools".to_string(),
                path: PathBuf::from("/project/pnpm-workspace.yaml"),
                line: 10,
            }));
        let config = config_with_override_for_rule("pnpm-workspace.yaml", |p| {
            p.empty_catalog_groups = Some(Severity::Off);
        });
        let rules = &config.rules;
        assert!(!has_error_severity_issues(&results, rules, Some(&config)));
    }

    // -------------------------------------------------------------------------
    // Lines 603-629: has_override_framework_error
    // -------------------------------------------------------------------------

    #[test]
    fn has_error_override_invalid_client_exports_off_for_matching_file() {
        let mut results = AnalysisResults::default();
        results
            .invalid_client_exports
            .push(InvalidClientExportFinding::with_actions(
                InvalidClientExport {
                    path: PathBuf::from("/project/src/app/page.ts"),
                    export_name: "metadata".to_string(),
                    directive: "use client".to_string(),
                    line: 1,
                    col: 0,
                },
            ));
        let config = config_with_override_for_rule("src/app/**", |p| {
            p.invalid_client_export = Some(Severity::Off);
        });
        let rules = &config.rules;
        assert!(!has_error_severity_issues(&results, rules, Some(&config)));
    }

    #[test]
    fn has_error_override_mixed_client_server_barrel_off_for_matching_file() {
        let mut results = AnalysisResults::default();
        results
            .mixed_client_server_barrels
            .push(MixedClientServerBarrelFinding::with_actions(
                MixedClientServerBarrel {
                    path: PathBuf::from("/project/src/app/index.ts"),
                    client_origin: "./client".to_string(),
                    server_origin: "./server".to_string(),
                    line: 1,
                    col: 0,
                },
            ));
        let config = config_with_override_for_rule("src/app/**", |p| {
            p.mixed_client_server_barrel = Some(Severity::Off);
        });
        let rules = &config.rules;
        assert!(!has_error_severity_issues(&results, rules, Some(&config)));
    }

    #[test]
    fn has_error_override_misplaced_directive_off_for_matching_file() {
        let mut results = AnalysisResults::default();
        results
            .misplaced_directives
            .push(MisplacedDirectiveFinding::with_actions(
                MisplacedDirective {
                    path: PathBuf::from("/project/src/app/widget.ts"),
                    directive: "use client".to_string(),
                    line: 5,
                    col: 0,
                },
            ));
        let config = config_with_override_for_rule("src/app/**", |p| {
            p.misplaced_directive = Some(Severity::Off);
        });
        let rules = &config.rules;
        assert!(!has_error_severity_issues(&results, rules, Some(&config)));
    }

    #[test]
    fn has_error_override_route_collision_off_for_matching_file() {
        let mut results = AnalysisResults::default();
        results
            .route_collisions
            .push(RouteCollisionFinding::with_actions(RouteCollision {
                path: PathBuf::from("/project/src/app/(a)/about/page.tsx"),
                url: "/about".to_string(),
                conflicting_paths: vec![PathBuf::from("/project/src/app/(b)/about/page.tsx")],
                line: 1,
                col: 0,
            }));
        let config = config_with_override_for_rule("src/app/(a)/**", |p| {
            p.route_collision = Some(Severity::Off);
        });
        let rules = &config.rules;
        assert!(!has_error_severity_issues(&results, rules, Some(&config)));
    }

    #[test]
    fn has_error_override_dynamic_segment_name_conflict_off_for_matching_file() {
        let mut results = AnalysisResults::default();
        results.dynamic_segment_name_conflicts.push(
            DynamicSegmentNameConflictFinding::with_actions(DynamicSegmentNameConflict {
                path: PathBuf::from("/project/src/app/routes/page.tsx"),
                position: "/".to_string(),
                conflicting_segments: vec!["[id]".to_string(), "[slug]".to_string()],
                conflicting_paths: vec![PathBuf::from("/project/src/app/other/page.tsx")],
                line: 1,
                col: 0,
            }),
        );
        let config = config_with_override_for_rule("src/app/**", |p| {
            p.dynamic_segment_name_conflict = Some(Severity::Off);
        });
        let rules = &config.rules;
        assert!(!has_error_severity_issues(&results, rules, Some(&config)));
    }

    // -------------------------------------------------------------------------
    // Lines 638-679 / 691-720: has_default_file_scoped_error and
    // has_project_level_error - stale-suppression and dep-override error arms
    // -------------------------------------------------------------------------

    #[test]
    fn default_file_scoped_stale_suppression_missing_reason_error() {
        let mut results = AnalysisResults::default();
        results
            .stale_suppressions
            .push(stale_suppression("/project/src/a.ts", true));
        let rules = RulesConfig {
            require_suppression_reason: Severity::Error,
            ..RulesConfig::default()
        };
        assert!(has_error_severity_issues(&results, &rules, None));
    }

    #[test]
    fn default_file_scoped_stale_suppression_missing_reason_warn_not_error() {
        let mut results = AnalysisResults::default();
        results
            .stale_suppressions
            .push(stale_suppression("/project/src/a.ts", true));
        // The missing_reason branch checks require_suppression_reason; stale branch checks stale_suppressions.
        // Both are Warn here so no error.
        let all_warn = RulesConfig {
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
            unused_component_inputs: Severity::Warn,
            unused_component_outputs: Severity::Warn,
            unused_svelte_events: Severity::Warn,
            unused_server_actions: Severity::Warn,
            unused_load_data_keys: Severity::Warn,
            prop_drilling: Severity::Off,
            thin_wrapper: Severity::Off,
            duplicate_prop_shape: Severity::Off,
            css_token_drift: Severity::Off,
            css_duplicate_block: Severity::Off,
            css_selector_complexity: Severity::Off,
            css_dead_surface: Severity::Off,
            css_broken_reference: Severity::Off,
            unresolved_imports: Severity::Warn,
            unlisted_dependencies: Severity::Warn,
            duplicate_exports: Severity::Warn,
            type_only_dependencies: Severity::Warn,
            test_only_dependencies: Severity::Warn,
            boundary_violation: Severity::Warn,
            circular_dependencies: Severity::Warn,
            re_export_cycle: Severity::Warn,
            coverage_gaps: Severity::Warn,
            feature_flags: Severity::Warn,
            stale_suppressions: Severity::Warn,
            require_suppression_reason: Severity::Warn,
            unused_catalog_entries: Severity::Warn,
            empty_catalog_groups: Severity::Warn,
            unresolved_catalog_references: Severity::Warn,
            unused_dependency_overrides: Severity::Warn,
            misconfigured_dependency_overrides: Severity::Warn,
            security_client_server_leak: Severity::Off,
            security_sink: Severity::Off,
            policy_violation: Severity::Warn,
            invalid_client_export: Severity::Warn,
            mixed_client_server_barrel: Severity::Warn,
            misplaced_directive: Severity::Warn,
            route_collision: Severity::Warn,
            dynamic_segment_name_conflict: Severity::Warn,
        };
        assert!(!has_error_severity_issues(&results, &all_warn, None));
    }

    #[test]
    fn project_level_unused_dep_overrides_error() {
        let mut results = AnalysisResults::default();
        results
            .unused_dependency_overrides
            .push(UnusedDependencyOverrideFinding::with_actions(
                UnusedDependencyOverride {
                    raw_key: "old-pkg".to_string(),
                    target_package: "old-pkg".to_string(),
                    parent_package: None,
                    version_constraint: None,
                    version_range: "^1.0.0".to_string(),
                    source: DependencyOverrideSource::PnpmWorkspaceYaml,
                    path: PathBuf::from("/project/pnpm-workspace.yaml"),
                    line: 5,
                    hint: None,
                },
            ));
        let rules = RulesConfig {
            unused_dependency_overrides: Severity::Error,
            ..RulesConfig::default()
        };
        assert!(has_error_severity_issues(&results, &rules, None));
    }

    #[test]
    fn project_level_misconfigured_dep_overrides_error() {
        let mut results = AnalysisResults::default();
        results.misconfigured_dependency_overrides.push(
            MisconfiguredDependencyOverrideFinding::with_actions(MisconfiguredDependencyOverride {
                raw_key: "bad>".to_string(),
                target_package: None,
                raw_value: "1.0.0".to_string(),
                reason: DependencyOverrideMisconfigReason::UnparsableKey,
                source: DependencyOverrideSource::PnpmPackageJson,
                path: PathBuf::from("/project/package.json"),
                line: 3,
            }),
        );
        let rules = RulesConfig {
            misconfigured_dependency_overrides: Severity::Error,
            ..RulesConfig::default()
        };
        assert!(has_error_severity_issues(&results, &rules, None));
    }

    #[test]
    fn project_level_re_export_cycle_error() {
        let mut results = AnalysisResults::default();
        results
            .re_export_cycles
            .push(ReExportCycleFinding::with_actions(ReExportCycle {
                files: vec![
                    PathBuf::from("/project/src/a.ts"),
                    PathBuf::from("/project/src/b.ts"),
                ],
                kind: ReExportCycleKind::MultiNode,
            }));
        let rules = RulesConfig {
            re_export_cycle: Severity::Error,
            ..RulesConfig::default()
        };
        assert!(has_error_severity_issues(&results, &rules, None));
    }

    #[test]
    fn project_level_unused_catalog_entries_error() {
        let mut results = AnalysisResults::default();
        results
            .unused_catalog_entries
            .push(UnusedCatalogEntryFinding::with_actions(
                UnusedCatalogEntry {
                    entry_name: "lodash".to_string(),
                    catalog_name: "default".to_string(),
                    path: PathBuf::from("/project/pnpm-workspace.yaml"),
                    line: 5,
                    hardcoded_consumers: vec![],
                },
            ));
        let rules = RulesConfig {
            unused_catalog_entries: Severity::Error,
            ..RulesConfig::default()
        };
        assert!(has_error_severity_issues(&results, &rules, None));
    }

    #[test]
    fn project_level_boundary_violation_no_override_error_for_call_violation() {
        let mut results = AnalysisResults::default();
        results
            .boundary_call_violations
            .push(BoundaryCallViolationFinding::with_actions(
                BoundaryCallViolation {
                    path: PathBuf::from("/project/src/ui/a.ts"),
                    line: 1,
                    col: 0,
                    zone: "ui".to_string(),
                    callee: "cp.exec".to_string(),
                    pattern: "child_process.*".to_string(),
                },
            ));
        let rules = RulesConfig::default(); // boundary_violation is Error
        assert!(has_error_severity_issues(&results, &rules, None));
    }

    // -------------------------------------------------------------------------
    // Lines 729-730: promote_policy_finding_warns
    // -------------------------------------------------------------------------

    #[test]
    fn promote_policy_finding_warns_flips_warn_to_error() {
        let mut results = AnalysisResults::default();
        results
            .policy_violations
            .push(PolicyViolationFinding::with_actions(PolicyViolation {
                path: PathBuf::from("/project/src/a.ts"),
                line: 1,
                col: 0,
                pack: "pack".to_string(),
                rule_id: "rule".to_string(),
                kind: PolicyRuleKind::BannedCall,
                matched: "exec".to_string(),
                severity: PolicyViolationSeverity::Warn,
                message: None,
            }));
        promote_policy_finding_warns(&mut results);
        assert_eq!(
            results.policy_violations[0].violation.severity,
            PolicyViolationSeverity::Error
        );
    }

    #[test]
    fn promote_policy_finding_warns_preserves_error_severity() {
        let mut results = AnalysisResults::default();
        results
            .policy_violations
            .push(PolicyViolationFinding::with_actions(PolicyViolation {
                path: PathBuf::from("/project/src/a.ts"),
                line: 1,
                col: 0,
                pack: "pack".to_string(),
                rule_id: "rule".to_string(),
                kind: PolicyRuleKind::BannedCall,
                matched: "exec".to_string(),
                severity: PolicyViolationSeverity::Error,
                message: None,
            }));
        promote_policy_finding_warns(&mut results);
        assert_eq!(
            results.policy_violations[0].violation.severity,
            PolicyViolationSeverity::Error
        );
    }

    // -------------------------------------------------------------------------
    // Lines 794-799: promote_policy_finding_warns on empty results
    // -------------------------------------------------------------------------

    #[test]
    fn promote_policy_finding_warns_noop_on_empty() {
        let mut results = AnalysisResults::default();
        promote_policy_finding_warns(&mut results);
        assert!(results.policy_violations.is_empty());
    }

    // -------------------------------------------------------------------------
    // Additional base-component clearing (clear_base_component_dead_code)
    // -------------------------------------------------------------------------

    #[test]
    fn base_unrendered_components_off_clears_findings() {
        let mut results = AnalysisResults::default();
        results
            .unrendered_components
            .push(UnrenderedComponentFinding::with_actions(
                UnrenderedComponent {
                    path: PathBuf::from("/project/src/Widget.vue"),
                    component_name: "Widget".to_string(),
                    framework: "vue".to_string(),
                    reachable_via: None,
                    line: 1,
                    col: 0,
                },
            ));
        let rules = RulesConfig {
            unrendered_components: Severity::Off,
            ..RulesConfig::default()
        };
        let config = config_with_rules(rules);
        apply_rules(&mut results, &config);
        assert!(results.unrendered_components.is_empty());
    }

    #[test]
    fn base_unused_component_props_off_clears_findings() {
        let mut results = AnalysisResults::default();
        results
            .unused_component_props
            .push(UnusedComponentPropFinding::with_actions(
                UnusedComponentProp {
                    path: PathBuf::from("/project/src/Card.vue"),
                    component_name: "Card".to_string(),
                    prop_name: "color".to_string(),
                    line: 4,
                    col: 0,
                },
            ));
        let rules = RulesConfig {
            unused_component_props: Severity::Off,
            ..RulesConfig::default()
        };
        let config = config_with_rules(rules);
        apply_rules(&mut results, &config);
        assert!(results.unused_component_props.is_empty());
    }

    #[test]
    fn base_unused_component_emits_off_clears_findings() {
        let mut results = AnalysisResults::default();
        results
            .unused_component_emits
            .push(UnusedComponentEmitFinding::with_actions(
                UnusedComponentEmit {
                    path: PathBuf::from("/project/src/Button.vue"),
                    component_name: "Button".to_string(),
                    emit_name: "click".to_string(),
                    line: 6,
                    col: 0,
                },
            ));
        let rules = RulesConfig {
            unused_component_emits: Severity::Off,
            ..RulesConfig::default()
        };
        let config = config_with_rules(rules);
        apply_rules(&mut results, &config);
        assert!(results.unused_component_emits.is_empty());
    }

    #[test]
    fn base_unused_component_inputs_off_clears_findings() {
        let mut results = AnalysisResults::default();
        results
            .unused_component_inputs
            .push(UnusedComponentInputFinding::with_actions(
                UnusedComponentInput {
                    path: PathBuf::from("/project/src/table.component.ts"),
                    component_name: "table".to_string(),
                    input_name: "rows".to_string(),
                    line: 10,
                    col: 0,
                },
            ));
        let rules = RulesConfig {
            unused_component_inputs: Severity::Off,
            ..RulesConfig::default()
        };
        let config = config_with_rules(rules);
        apply_rules(&mut results, &config);
        assert!(results.unused_component_inputs.is_empty());
    }

    #[test]
    fn base_unused_component_outputs_off_clears_findings() {
        let mut results = AnalysisResults::default();
        results
            .unused_component_outputs
            .push(UnusedComponentOutputFinding::with_actions(
                UnusedComponentOutput {
                    path: PathBuf::from("/project/src/table.component.ts"),
                    component_name: "table".to_string(),
                    output_name: "rowClick".to_string(),
                    line: 14,
                    col: 0,
                },
            ));
        let rules = RulesConfig {
            unused_component_outputs: Severity::Off,
            ..RulesConfig::default()
        };
        let config = config_with_rules(rules);
        apply_rules(&mut results, &config);
        assert!(results.unused_component_outputs.is_empty());
    }

    #[test]
    fn base_unused_svelte_events_off_clears_findings() {
        let mut results = AnalysisResults::default();
        results
            .unused_svelte_events
            .push(UnusedSvelteEventFinding::with_actions(UnusedSvelteEvent {
                path: PathBuf::from("/project/src/Widget.svelte"),
                component_name: "Widget".to_string(),
                event_name: "toggle".to_string(),
                line: 7,
                col: 0,
            }));
        let rules = RulesConfig {
            unused_svelte_events: Severity::Off,
            ..RulesConfig::default()
        };
        let config = config_with_rules(rules);
        apply_rules(&mut results, &config);
        assert!(results.unused_svelte_events.is_empty());
    }

    #[test]
    fn base_unused_server_actions_off_clears_findings() {
        let mut results = AnalysisResults::default();
        results
            .unused_server_actions
            .push(UnusedServerActionFinding::with_actions(
                UnusedServerAction {
                    path: PathBuf::from("/project/src/actions.ts"),
                    action_name: "submitForm".to_string(),
                    line: 2,
                    col: 0,
                },
            ));
        let rules = RulesConfig {
            unused_server_actions: Severity::Off,
            ..RulesConfig::default()
        };
        let config = config_with_rules(rules);
        apply_rules(&mut results, &config);
        assert!(results.unused_server_actions.is_empty());
    }

    #[test]
    fn base_unused_load_data_keys_off_clears_findings() {
        let mut results = AnalysisResults::default();
        results
            .unused_load_data_keys
            .push(UnusedLoadDataKeyFinding::with_actions(UnusedLoadDataKey {
                path: PathBuf::from("/project/src/routes/+page.server.ts"),
                key_name: "userData".to_string(),
                line: 3,
                col: 0,
                route_dir: Some("src/routes".to_string()),
            }));
        let rules = RulesConfig {
            unused_load_data_keys: Severity::Off,
            ..RulesConfig::default()
        };
        let config = config_with_rules(rules);
        apply_rules(&mut results, &config);
        assert!(results.unused_load_data_keys.is_empty());
    }

    #[test]
    fn base_unprovided_injects_off_clears_findings() {
        let mut results = AnalysisResults::default();
        results
            .unprovided_injects
            .push(UnprovidedInjectFinding::with_actions(UnprovidedInject {
                path: PathBuf::from("/project/src/Child.vue"),
                key_name: "MY_KEY".to_string(),
                framework: "vue".to_string(),
                line: 2,
                col: 0,
            }));
        let rules = RulesConfig {
            unprovided_injects: Severity::Off,
            ..RulesConfig::default()
        };
        let config = config_with_rules(rules);
        apply_rules(&mut results, &config);
        assert!(results.unprovided_injects.is_empty());
    }
}
