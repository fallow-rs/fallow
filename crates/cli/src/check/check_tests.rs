use super::*;
use fallow_types::extract::MemberKind;
use fallow_types::output_dead_code::*;
use fallow_types::results::*;
use std::path::PathBuf;

fn resolved_config(type_aware_enabled: bool) -> ResolvedConfig {
    let config: fallow_config::FallowConfig = serde_json::from_value(serde_json::json!({
        "typeAware": { "enabled": type_aware_enabled }
    }))
    .expect("config");
    config.resolve(
        PathBuf::from("/repo"),
        fallow_config::OutputFormat::Json,
        1,
        false,
        true,
        None,
    )
}

/// First match wins: CLI flag, then command-scoped config override, then
/// the top-level `typeAware.enabled` opt-in. The environment layer is not
/// exercised here because env vars are process-global across tests.
#[test]
fn type_aware_override_precedence_is_cli_then_scoped_config_then_config() {
    let cases: [(Option<bool>, Option<bool>, bool, bool); 6] = [
        // --no-type-aware beats every config layer.
        (Some(false), Some(true), true, false),
        // --type-aware beats a scoped opt-out.
        (Some(true), Some(false), false, true),
        // audit.typeAware=false keeps audit syntactic under typeAware.enabled=true.
        (None, Some(false), true, false),
        // audit.typeAware=true opts audit in alone.
        (None, Some(true), false, true),
        // Unset layers inherit typeAware.enabled.
        (None, None, true, true),
        (None, None, false, false),
    ];
    for (cli, scoped, config_enabled, expected) in cases {
        let mut config = resolved_config(config_enabled);
        apply_type_aware_overrides_from(
            fallow_config::OutputFormat::Json,
            cli,
            scoped,
            &[],
            None,
            &mut config,
        )
        .expect("overrides should apply");
        assert_eq!(
            config.type_aware.enabled, expected,
            "cli={cli:?} scoped={scoped:?} config={config_enabled}"
        );
    }
}

#[test]
fn dependency_findings_are_reported_without_filters_or_with_a_dependency_filter() {
    assert!(no_filters().reports_dependency_findings());
    let mut files_only = no_filters();
    files_only.unused_files = true;
    assert!(!files_only.reports_dependency_findings());
    let mut unlisted = files_only;
    unlisted.unlisted_deps = true;
    assert!(unlisted.reports_dependency_findings());
    let mut unused = no_filters();
    unused.unused_deps = true;
    assert!(unused.reports_dependency_findings());
}

fn no_filters() -> IssueFilters {
    IssueFilters {
        unused_files: false,
        unused_exports: false,
        unused_deps: false,
        unused_types: false,
        private_type_leaks: false,
        deprecated_exports_in_use: false,
        unused_enum_members: false,
        unused_class_members: false,
        unused_namespace_members: false,
        unused_store_members: false,
        unprovided_injects: false,
        unrendered_components: false,
        unused_component_props: false,
        absent_component_props: false,
        unused_component_emits: false,
        unused_component_inputs: false,
        unused_component_outputs: false,
        unused_svelte_events: false,
        unused_effect_layers: false,
        unused_tagged_errors: false,
        unproduced_catch_tags: false,
        unused_server_actions: false,
        unused_load_data_keys: false,
        unresolved_imports: false,
        unlisted_deps: false,
        duplicate_exports: false,
        duplicate_export_aliases: false,
        unlisted_binaries: false,
        circular_deps: false,
        re_export_cycles: false,
        package_cycles: false,
        boundary_violations: false,
        policy_violations: false,
        stale_suppressions: false,
        unused_catalog_entries: false,
        empty_catalog_groups: false,
        unresolved_catalog_references: false,
        unused_dependency_overrides: false,
        misconfigured_dependency_overrides: false,
        invalid_client_exports: false,
        mixed_client_server_barrels: false,
        misplaced_directives: false,
        route_collisions: false,
        dynamic_segment_name_conflicts: false,
        architecture_selection: ArchitectureSelection::All,
    }
}

#[test]
fn type_aware_summary_reports_clean_and_retained_outcomes() {
    let clean = fallow_types::envelope::TypeAwareMeta {
        candidate_count: 1,
        confirmed_used_count: 1,
        ..fallow_types::envelope::TypeAwareMeta::default()
    };
    assert_eq!(
        format_type_aware_summary(&clean),
        "Type-aware refinement: 1 candidate, 1 confirmed use, 0 preserved contracts, \
             0 candidates without static references (0 guarded fixes), 0 unresolved findings, \
             0 abstained findings"
    );

    let retained = fallow_types::envelope::TypeAwareMeta {
        candidate_count: 3,
        confirmed_used_count: 1,
        no_static_references_count: 1,
        fix_eligible_count: 1,
        unresolved_count: 1,
        ..fallow_types::envelope::TypeAwareMeta::default()
    };
    assert_eq!(
        format_type_aware_summary(&retained),
        "Type-aware refinement: 3 candidates, 1 confirmed use, 0 preserved contracts, \
             1 candidate without static references (1 guarded fix), 1 unresolved finding, \
             0 abstained findings"
    );

    let abstained = fallow_types::envelope::TypeAwareMeta {
        candidate_count: 2,
        contract_preserved_count: 1,
        abstained_count: 1,
        ..fallow_types::envelope::TypeAwareMeta::default()
    };
    assert_eq!(
        format_type_aware_summary(&abstained),
        "Type-aware refinement: 2 candidates, 0 confirmed uses, 1 preserved contract, \
             0 candidates without static references (0 guarded fixes), 0 unresolved findings, \
             1 abstained finding"
    );
}

#[test]
fn private_type_leaks_filter_opts_in_off_by_default_rule() {
    let mut rules = fallow_config::RulesConfig::default();
    assert_eq!(rules.private_type_leaks, fallow_config::Severity::Off);

    let mut filters = no_filters();
    filters.private_type_leaks = true;
    filters.activate_explicit_opt_ins(&mut rules);

    assert_eq!(rules.private_type_leaks, fallow_config::Severity::Warn);
}

#[test]
fn deprecated_exports_filter_opts_in_off_by_default_rule() {
    let mut rules = fallow_config::RulesConfig::default();
    assert_eq!(
        rules.deprecated_exports_in_use,
        fallow_config::Severity::Off
    );

    let mut filters = no_filters();
    filters.deprecated_exports_in_use = true;
    filters.activate_explicit_opt_ins(&mut rules);

    assert_eq!(
        rules.deprecated_exports_in_use,
        fallow_config::Severity::Warn
    );
    assert_eq!(rules.private_type_leaks, fallow_config::Severity::Off);
}

fn type_aware_resolved_config() -> ResolvedConfig {
    let mut config = fallow_config::FallowConfig::default().resolve(
        PathBuf::from("/project"),
        OutputFormat::Json,
        1,
        true,
        true,
        None,
    );
    config.type_aware.enabled = true;
    config
}

#[test]
fn type_aware_defaults_private_type_leaks_on_when_unconfigured() {
    let mut config = type_aware_resolved_config();
    assert_eq!(config.rules.private_type_leaks, Severity::Off);
    assert!(!config.rules.private_type_leaks_configured);

    apply_type_aware_private_leak_default(&mut config, false);

    assert_eq!(config.rules.private_type_leaks, Severity::Warn);
}

#[test]
fn type_aware_respects_explicit_private_type_leaks_off() {
    let mut config = type_aware_resolved_config();
    config.rules.private_type_leaks_configured = true;

    apply_type_aware_private_leak_default(&mut config, false);

    assert_eq!(config.rules.private_type_leaks, Severity::Off);
}

fn load_and_resolve_config_file(json: &str) -> ResolvedConfig {
    let dir = tempfile::tempdir().expect("tempdir");
    let config_path = dir.path().join(".fallowrc.json");
    std::fs::write(&config_path, json).expect("config file is written");
    fallow_config::FallowConfig::load(&config_path)
        .expect("config file loads")
        .resolve(
            dir.path().to_path_buf(),
            OutputFormat::Json,
            1,
            true,
            true,
            None,
        )
}

#[test]
fn loaded_explicit_private_type_leaks_off_survives_resolve_and_type_aware_default() {
    let mut config = load_and_resolve_config_file(
        r#"{"typeAware": {"enabled": true}, "rules": {"private-type-leaks": "off"}}"#,
    );
    assert!(config.type_aware.enabled);
    assert!(config.rules.private_type_leaks_configured);

    apply_type_aware_private_leak_default(&mut config, false);

    assert_eq!(config.rules.private_type_leaks, Severity::Off);
}

#[test]
fn loaded_unset_private_type_leaks_defaults_on_after_resolve_under_type_aware() {
    let mut config = load_and_resolve_config_file(r#"{"typeAware": {"enabled": true}}"#);
    assert!(config.type_aware.enabled);
    assert!(!config.rules.private_type_leaks_configured);

    apply_type_aware_private_leak_default(&mut config, false);

    assert_eq!(config.rules.private_type_leaks, Severity::Warn);
}

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
            deprecated: false,
            deprecated_reason: None,
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
            deprecated: false,
            deprecated_reason: None,
        }));
    r.unused_dependencies
        .push(UnusedDependencyFinding::with_actions(UnusedDependency {
            package_name: "lodash".into(),
            location: DependencyLocation::Dependencies,
            path: PathBuf::from("/project/package.json"),
            line: 5,
            used_in_workspaces: Vec::new(),
            declared_and_imported_in: Vec::new(),
        }));
    r.unused_dev_dependencies
        .push(UnusedDevDependencyFinding::with_actions(UnusedDependency {
            package_name: "jest".into(),
            location: DependencyLocation::DevDependencies,
            path: PathBuf::from("/project/package.json"),
            line: 5,
            used_in_workspaces: Vec::new(),
            declared_and_imported_in: Vec::new(),
        }));
    r.test_only_dependencies
        .push(TestOnlyDependencyFinding::with_actions(
            TestOnlyDependency {
                package_name: "msw".into(),
                path: PathBuf::from("/project/package.json"),
                line: 9,
            },
        ));
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

#[test]
fn save_baseline_writes_trailing_newline() {
    let dir = tempfile::tempdir().expect("tempdir");
    let baseline_path = dir.path().join("baseline.json");
    let mut results = make_results();

    handle_baseline(
        &mut results,
        &BaselineIo {
            save_path: Some(&baseline_path),
            load_path: None,
            load_flag: "--baseline",
            bare_run: false,
            command: "dead-code",
            config: &resolved_config(false),
            quiet: true,
            output: OutputFormat::Json,
            analysis_identity: &fallow_types::semantic::SemanticAnalysisIdentity::default(),
            scope_reasons: fallow_output::BaselineScopeReasons::empty(),
            filters: &IssueFilters::default(),
        },
    )
    .expect("baseline save succeeds");

    let saved = std::fs::read_to_string(&baseline_path).expect("baseline is written");
    assert!(saved.ends_with('\n'));
    assert!(!saved.ends_with("\n\n"));
}

#[test]
fn no_filters_means_none_active() {
    assert!(!no_filters().any_active());
}

#[test]
fn single_filter_is_active() {
    let mut f = no_filters();
    f.unused_files = true;
    assert!(f.any_active());
}

#[test]
fn every_registry_filter_flag_registers_as_active() {
    for flag in fallow_types::issue_meta::DEAD_CODE_FILTER_FLAGS.iter() {
        let mut f = no_filters();
        assert!(
            f.enable_cli_filter_flag(flag),
            "registry filter flag {flag} has no CLI IssueFilters mapping"
        );
        assert!(
            f.any_active(),
            "registry filter flag {flag} stayed inactive"
        );
    }
}

#[test]
fn apply_no_active_filters_preserves_all_results() {
    let mut results = make_results();
    let original_total = results.total_issues();
    no_filters().apply(&mut results);
    assert_eq!(results.total_issues(), original_total);
}

#[test]
fn apply_unused_files_filter_keeps_only_unused_files() {
    let mut results = make_results();
    let mut f = no_filters();
    f.unused_files = true;
    f.apply(&mut results);

    assert_eq!(results.unused_files.len(), 1);
    assert!(results.unused_exports.is_empty());
    assert!(results.unused_types.is_empty());
    assert!(results.unused_dependencies.is_empty());
    assert!(results.unused_dev_dependencies.is_empty());
    assert!(results.test_only_dependencies.is_empty());
    assert!(results.unused_enum_members.is_empty());
    assert!(results.unused_class_members.is_empty());
    assert!(results.unresolved_imports.is_empty());
    assert!(results.unlisted_dependencies.is_empty());
    assert!(results.duplicate_exports.is_empty());
}

#[test]
fn apply_unused_deps_filter_keeps_both_dep_types() {
    let mut results = make_results();
    let mut f = no_filters();
    f.unused_deps = true;
    f.apply(&mut results);

    assert_eq!(results.unused_dependencies.len(), 1);
    assert_eq!(results.unused_dev_dependencies.len(), 1);
    assert_eq!(results.test_only_dependencies.len(), 1);
    assert!(results.unused_files.is_empty());
    assert!(results.unused_exports.is_empty());
}

#[test]
fn apply_single_type_filter_clears_test_only_dependencies() {
    // Regression for #1192: a single-type filter that is not --unused-deps must clear
    // test-only-dependency findings, matching every other dependency kind. Before the fix the
    // --unused-deps clear arm omitted test_only_dependencies, so it leaked into the output of
    // any single-type filter run (e.g. `fallow dead-code --unused-files`).
    let mut results = make_results();
    assert_eq!(results.test_only_dependencies.len(), 1);

    let mut f = no_filters();
    f.unused_files = true;
    f.apply(&mut results);

    assert!(
        results.test_only_dependencies.is_empty(),
        "test-only-dependency findings must be cleared when --unused-deps is not active"
    );
}

#[test]
fn apply_multiple_filters_keeps_selected_types() {
    let mut results = make_results();
    let mut f = no_filters();
    f.unused_files = true;
    f.unresolved_imports = true;
    f.apply(&mut results);

    assert_eq!(results.unused_files.len(), 1);
    assert_eq!(results.unresolved_imports.len(), 1);
    assert!(results.unused_exports.is_empty());
    assert!(results.unused_types.is_empty());
    assert!(results.duplicate_exports.is_empty());
}

#[test]
fn apply_circular_deps_filter_keeps_only_circular_deps() {
    let mut results = make_results();
    results.circular_dependencies.push(
        fallow_types::output_dead_code::CircularDependencyFinding::with_actions(
            fallow_types::results::CircularDependency {
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
        ),
    );
    let mut f = no_filters();
    f.circular_deps = true;
    f.apply(&mut results);

    assert_eq!(results.circular_dependencies.len(), 1);
    assert!(results.unused_files.is_empty());
    assert!(results.unused_exports.is_empty());
    assert!(results.unused_dependencies.is_empty());
}

#[test]
fn no_trace_options_means_none_active() {
    let t = TraceOptions {
        trace_export: None,
        trace_file: None,
        trace_dependency: None,
        impact_closure: None,
        symbol_impact: None,
        performance: false,
    };
    assert!(!t.any_active());
}

#[test]
fn trace_export_is_active() {
    let t = TraceOptions {
        trace_export: Some("src/foo.ts:bar".into()),
        trace_file: None,
        trace_dependency: None,
        impact_closure: None,
        symbol_impact: None,
        performance: false,
    };
    assert!(t.any_active());
}

#[test]
fn trace_file_is_active() {
    let t = TraceOptions {
        trace_export: None,
        trace_file: Some("src/foo.ts".into()),
        trace_dependency: None,
        impact_closure: None,
        symbol_impact: None,
        performance: false,
    };
    assert!(t.any_active());
}

#[test]
fn trace_dependency_is_active() {
    let t = TraceOptions {
        trace_export: None,
        trace_file: None,
        trace_dependency: Some("lodash".into()),
        impact_closure: None,
        symbol_impact: None,
        performance: false,
    };
    assert!(t.any_active());

    let t = TraceOptions {
        trace_export: None,
        trace_file: None,
        trace_dependency: None,
        impact_closure: Some("src/foo.ts".into()),
        symbol_impact: None,
        performance: false,
    };
    assert!(t.any_active());
}

#[test]
fn performance_flag_is_active() {
    let t = TraceOptions {
        trace_export: None,
        trace_file: None,
        trace_dependency: None,
        impact_closure: None,
        symbol_impact: None,
        performance: true,
    };
    assert!(t.any_active());
}

#[test]
fn apply_boundary_violations_filter() {
    let mut results = make_results();
    results.boundary_violations.push(
        fallow_types::output_dead_code::BoundaryViolationFinding::with_actions(
            fallow_types::results::BoundaryViolation {
                from_path: PathBuf::from("/project/src/bad.ts"),
                to_path: PathBuf::from("/project/lib/secret.ts"),
                from_zone: "src".to_string(),
                to_zone: "lib".to_string(),
                import_specifier: "../lib/secret".to_string(),
                line: 1,
                col: 0,
                via_path: None,
            },
        ),
    );
    let mut f = no_filters();
    f.boundary_violations = true;
    f.apply(&mut results);

    assert_eq!(results.boundary_violations.len(), 1);
    assert!(results.unused_files.is_empty());
    assert!(results.unused_exports.is_empty());
    assert!(results.unused_dependencies.is_empty());
    assert!(results.circular_dependencies.is_empty());
}

#[test]
fn apply_all_filter_types_simultaneously() {
    let mut results = make_results();
    results.circular_dependencies.push(
        fallow_types::output_dead_code::CircularDependencyFinding::with_actions(
            fallow_types::results::CircularDependency {
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
        ),
    );
    results.boundary_violations.push(
        fallow_types::output_dead_code::BoundaryViolationFinding::with_actions(
            fallow_types::results::BoundaryViolation {
                from_path: PathBuf::from("/project/src/x.ts"),
                to_path: PathBuf::from("/project/lib/y.ts"),
                from_zone: "src".to_string(),
                to_zone: "lib".to_string(),
                import_specifier: "../lib/y".to_string(),
                line: 1,
                col: 0,
                via_path: None,
            },
        ),
    );

    let f = IssueFilters {
        unused_files: true,
        unused_exports: true,
        unused_deps: true,
        unused_types: true,
        private_type_leaks: true,
        deprecated_exports_in_use: true,
        unused_enum_members: true,
        unused_class_members: true,
        unused_namespace_members: true,
        unused_store_members: true,
        unprovided_injects: true,
        unrendered_components: true,
        unused_component_props: true,
        absent_component_props: true,
        unused_component_emits: true,
        unused_component_inputs: true,
        unused_component_outputs: true,
        unused_svelte_events: true,
        unused_effect_layers: true,
        unused_tagged_errors: true,
        unproduced_catch_tags: true,
        unused_server_actions: true,
        unused_load_data_keys: true,
        unresolved_imports: true,
        unlisted_deps: true,
        duplicate_exports: true,
        duplicate_export_aliases: true,
        unlisted_binaries: false,
        circular_deps: true,
        re_export_cycles: true,
        package_cycles: true,
        boundary_violations: true,
        policy_violations: true,
        stale_suppressions: true,
        unused_catalog_entries: true,
        empty_catalog_groups: true,
        unresolved_catalog_references: true,
        unused_dependency_overrides: true,
        misconfigured_dependency_overrides: true,
        invalid_client_exports: true,
        mixed_client_server_barrels: true,
        misplaced_directives: true,
        route_collisions: true,
        dynamic_segment_name_conflicts: true,
        architecture_selection: ArchitectureSelection::All,
    };
    let total_before = results.total_issues();
    f.apply(&mut results);
    assert_eq!(results.total_issues(), total_before);
}

#[test]
fn apply_unused_deps_clears_optional_and_type_only() {
    let mut results = make_results();
    results
        .unused_optional_dependencies
        .push(UnusedOptionalDependencyFinding::with_actions(
            UnusedDependency {
                package_name: "fsevents".into(),
                location: DependencyLocation::OptionalDependencies,
                path: PathBuf::from("/project/package.json"),
                line: 5,
                used_in_workspaces: Vec::new(),
                declared_and_imported_in: Vec::new(),
            },
        ));
    results.type_only_dependencies.push(
        fallow_types::output_dead_code::TypeOnlyDependencyFinding::with_actions(
            TypeOnlyDependency {
                package_name: "zod".into(),
                path: PathBuf::from("/project/package.json"),
                line: 8,
            },
        ),
    );

    let mut f = no_filters();
    f.unused_exports = true; // Only keep unused exports
    f.apply(&mut results);

    assert!(results.unused_dependencies.is_empty());
    assert!(results.unused_dev_dependencies.is_empty());
    assert!(results.unused_optional_dependencies.is_empty());
    assert!(results.type_only_dependencies.is_empty());
    assert_eq!(results.unused_exports.len(), 1);
}
