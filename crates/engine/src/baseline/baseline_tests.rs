#[test]
fn prop_baselines_match_repeated_occurrences_and_advisory_staleness() {
    use fallow_types::output_dead_code::{EffectiveSeverity, PropDrillingChainFinding};
    use fallow_types::results::{PropDrillHop, PropDrillingChain};
    let root = Path::new("/project");
    let identity = fallow_types::semantic::SemanticAnalysisIdentity::syntactic();
    let mut chain = PropDrillingChainFinding::with_actions(PropDrillingChain {
        prop: "user".to_owned(),
        depth: 3,
        hops: vec![PropDrillHop {
            file: root.join("src/Page.tsx"),
            line: 4,
            component: "Page".to_owned(),
        }],
    });
    chain.effective_severity = Some(EffectiveSeverity::Error);
    let original = AnalysisResults {
        prop_drilling_chains: vec![chain.clone()],
        ..Default::default()
    };
    let baseline = BaselineData::from_results(&original, root);
    let saved = serde_json::to_string(&baseline).unwrap();
    let mut repeated = original.clone();
    repeated.prop_drilling_chains.push(chain.clone());
    let outcome = apply_dead_code_baseline(&mut repeated, &saved, root, &identity, false).unwrap();
    let DeadCodeBaselineOutcome::Applied { staleness, .. } = outcome else {
        panic!("baseline must apply");
    };
    assert_eq!(
        (
            staleness.entries,
            staleness.matched,
            staleness.current_findings
        ),
        (1, 1, 2)
    );
    assert_eq!(repeated.enforced_prop_drilling_count(), 1);
    let doubled = AnalysisResults {
        prop_drilling_chains: vec![chain.clone(), chain.clone()],
        ..Default::default()
    };
    let doubled_baseline =
        serde_json::to_string(&BaselineData::from_results(&doubled, root)).unwrap();
    let base_value = serde_json::from_str(&saved).unwrap();
    let doubled_value = serde_json::from_str(&doubled_baseline).unwrap();
    let growth = crate::baseline_growth::baseline_growth(
        BaselineKind::DeadCode,
        &base_value,
        &doubled_value,
    );
    assert_eq!(growth.added_entries(), 1);
    assert_eq!(growth.categories[0].category, "prop_drilling_chains");
    assert!(
        crate::baseline_growth::baseline_growth(
            BaselineKind::DeadCode,
            &doubled_value,
            &base_value
        )
        .is_empty()
    );
    let pruned = prune_dead_code_baseline(&doubled_baseline, &original, root, &identity).unwrap();
    assert_eq!((pruned.entries_before, pruned.entries_after), (2, 1));
    let mut old: serde_json::Value = serde_json::from_str(&saved).unwrap();
    old.as_object_mut().unwrap().remove("prop_drilling_chains");
    let mut unreviewed = original;
    apply_dead_code_baseline(&mut unreviewed, &old.to_string(), root, &identity, false).unwrap();
    assert_eq!(unreviewed.enforced_prop_drilling_count(), 1);

    chain.effective_severity = Some(EffectiveSeverity::Warn);
    let mut advisory = AnalysisResults {
        prop_drilling_chains: vec![chain],
        ..Default::default()
    };
    let saved = serde_json::to_string(&BaselineData::from_results(&advisory, root)).unwrap();
    assert_eq!(advisory.total_issues(), 0);
    let outcome = apply_dead_code_baseline(&mut advisory, &saved, root, &identity, false).unwrap();
    let DeadCodeBaselineOutcome::Applied { staleness, .. } = outcome else {
        panic!("baseline must apply");
    };
    assert_eq!(
        (
            staleness.entries,
            staleness.matched,
            staleness.current_findings
        ),
        (1, 1, 1)
    );
    assert_eq!(advisory.prop_drilling_chains.len(), 0);
}

use super::*;

use crate::duplicates::{CloneGroup, CloneInstance, DuplicationReport, DuplicationStats};
use crate::results::{
    AnalysisResults, BoundaryViolationFinding, CircularDependencyFinding, DependencyLocation,
    UnusedDependency, UnusedDependencyFinding, UnusedDevDependencyFinding, UnusedExport,
    UnusedFile,
};
use fallow_types::output_dead_code::{UnusedExportFinding, UnusedFileFinding, UnusedTypeFinding};
use std::path::PathBuf;

#[test]
fn stale_share_threshold_matches_the_documented_quarter() {
    for (baseline_entries, stale_entries, expected) in [
        (100, 24, false),
        (100, 25, true),
        (4, 2, true),
        (29, 2, false),
        (5, 1, false),
        (0, 0, false),
    ] {
        assert_eq!(
            stale_share_warrants_warning(baseline_entries, stale_entries),
            expected,
            "{stale_entries} of {baseline_entries} entries"
        );
    }
}

const fn staleness(entries: usize, matched: usize, current_findings: usize) -> BaselineStaleness {
    BaselineStaleness {
        entries,
        matched,
        current_findings,
        change_scoped: false,
    }
}

#[test]
fn warning_is_silent_when_the_run_found_nothing() {
    let staleness = staleness(4, 0, 0);
    assert_eq!(staleness.stale_entries(), 4);
    assert_eq!(staleness.warning(), BaselineStalenessWarning::None);
}

#[test]
fn zero_overlap_warns_when_the_run_has_findings() {
    assert_eq!(
        staleness(4, 0, 4).warning(),
        BaselineStalenessWarning::ZeroOverlap
    );
}

#[test]
fn partial_warning_needs_the_documented_quarter() {
    assert_eq!(
        staleness(100, 76, 100).warning(),
        BaselineStalenessWarning::None
    );
    assert_eq!(
        staleness(100, 75, 100).warning(),
        BaselineStalenessWarning::Partial
    );
}

#[test]
fn empty_baseline_never_warns() {
    assert_eq!(staleness(0, 0, 3).warning(), BaselineStalenessWarning::None);
}

#[test]
fn change_scoped_run_never_warns_and_never_trips_the_gate() {
    let scoped = BaselineStaleness {
        change_scoped: true,
        ..staleness(8, 2, 8)
    };
    assert_eq!(scoped.warning(), BaselineStalenessWarning::None);
    assert!(!scoped.trips_gate());
}

/// The whole point of the opt-in gate: it fires where the advisory
/// warning deliberately stays quiet, because the repository asked for
/// strictness rather than calibration.
#[test]
fn gate_trips_on_one_stale_entry_the_warning_ignores() {
    let staleness = staleness(20, 19, 19);
    assert_eq!(staleness.warning(), BaselineStalenessWarning::None);
    assert!(staleness.trips_gate());
}

#[test]
fn gate_trips_on_a_cleaned_project_the_warning_stays_silent_about() {
    let staleness = staleness(4, 0, 0);
    assert_eq!(staleness.warning(), BaselineStalenessWarning::None);
    assert!(staleness.trips_gate());
}

#[test]
fn gate_is_inert_on_an_empty_baseline() {
    assert!(!staleness(0, 0, 0).trips_gate());
}

#[test]
fn gate_is_inert_when_every_entry_matched() {
    assert!(!staleness(4, 4, 4).trips_gate());
}

pub(super) fn make_results() -> AnalysisResults {
    AnalysisResults {
        unused_files: vec![
            UnusedFileFinding::with_actions(UnusedFile {
                path: PathBuf::from("src/old.ts"),
            }),
            UnusedFileFinding::with_actions(UnusedFile {
                path: PathBuf::from("src/dead.ts"),
            }),
        ],
        unused_exports: vec![UnusedExportFinding::with_actions(UnusedExport {
            path: PathBuf::from("src/utils.ts"),
            export_name: "helperA".to_string(),
            is_type_only: false,
            line: 5,
            col: 0,
            span_start: 40,
            is_re_export: false,
            deprecated: false,
            deprecated_reason: None,
        })],
        unused_types: vec![UnusedTypeFinding::with_actions(UnusedExport {
            path: PathBuf::from("src/types.ts"),
            export_name: "OldType".to_string(),
            is_type_only: true,
            line: 10,
            col: 0,
            span_start: 100,
            is_re_export: false,
            deprecated: false,
            deprecated_reason: None,
        })],
        unused_dependencies: vec![UnusedDependencyFinding::with_actions(UnusedDependency {
            package_name: "lodash".to_string(),
            location: DependencyLocation::Dependencies,
            path: PathBuf::from("package.json"),
            line: 5,
            used_in_workspaces: Vec::new(),
            declared_and_imported_in: Vec::new(),
        })],
        unused_dev_dependencies: vec![UnusedDevDependencyFinding::with_actions(UnusedDependency {
            package_name: "jest".to_string(),
            location: DependencyLocation::DevDependencies,
            path: PathBuf::from("package.json"),
            line: 5,
            used_in_workspaces: Vec::new(),
            declared_and_imported_in: Vec::new(),
        })],
        ..Default::default()
    }
}

#[test]
fn baseline_from_results_captures_all_fields() {
    let results = make_results();
    let baseline = BaselineData::from_results(&results, Path::new(""));
    assert_eq!(
        baseline.unused_files,
        vec!["unused-file:src/dead.ts", "unused-file:src/old.ts"]
    );
    assert_eq!(
        baseline.unused_exports,
        vec!["unused-export:src/utils.ts:helperA"]
    );
    assert_eq!(
        baseline.unused_types,
        vec!["unused-type:src/types.ts:OldType"]
    );
    assert_eq!(
        baseline.unused_dependencies,
        vec!["unused-dependency:package.json:lodash"]
    );
    assert_eq!(
        baseline.unused_dev_dependencies,
        vec!["unused-dev-dependency:package.json:jest"]
    );
}

#[test]
fn dependency_baseline_keys_include_package_json_path() {
    let root = Path::new("/repo");
    let results = AnalysisResults {
        unused_dependencies: vec![
            UnusedDependencyFinding::with_actions(UnusedDependency {
                package_name: "lodash-es".to_string(),
                location: DependencyLocation::Dependencies,
                path: PathBuf::from("/repo/packages/app-a/package.json"),
                line: 5,
                used_in_workspaces: Vec::new(),
                declared_and_imported_in: Vec::new(),
            }),
            UnusedDependencyFinding::with_actions(UnusedDependency {
                package_name: "lodash-es".to_string(),
                location: DependencyLocation::Dependencies,
                path: PathBuf::from("/repo/packages/app-b/package.json"),
                line: 5,
                used_in_workspaces: Vec::new(),
                declared_and_imported_in: Vec::new(),
            }),
        ],
        ..Default::default()
    };

    let baseline = BaselineData::from_results(&results, root);

    assert_eq!(
        baseline.unused_dependencies,
        vec![
            "unused-dependency:packages/app-a/package.json:lodash-es",
            "unused-dependency:packages/app-b/package.json:lodash-es"
        ]
    );
}

#[test]
fn dependency_baseline_filter_matches_path_before_package_name() {
    let root = Path::new("/repo");
    let results = AnalysisResults {
        unused_dependencies: vec![
            UnusedDependencyFinding::with_actions(UnusedDependency {
                package_name: "lodash-es".to_string(),
                location: DependencyLocation::Dependencies,
                path: PathBuf::from("/repo/packages/app-a/package.json"),
                line: 5,
                used_in_workspaces: Vec::new(),
                declared_and_imported_in: Vec::new(),
            }),
            UnusedDependencyFinding::with_actions(UnusedDependency {
                package_name: "lodash-es".to_string(),
                location: DependencyLocation::Dependencies,
                path: PathBuf::from("/repo/packages/app-b/package.json"),
                line: 5,
                used_in_workspaces: Vec::new(),
                declared_and_imported_in: Vec::new(),
            }),
        ],
        ..Default::default()
    };
    let baseline = BaselineData {
        unused_dependencies: vec!["packages/app-a/package.json:lodash-es".to_string()],
        ..BaselineData::legacy_from_results(&AnalysisResults::default(), root)
    };

    let filtered = filter_new_issues(results, &baseline, root);

    assert_eq!(filtered.unused_dependencies.len(), 1);
    assert_eq!(
        filtered.unused_dependencies[0].dep.path,
        PathBuf::from("/repo/packages/app-b/package.json")
    );
}

#[test]
fn dependency_baseline_filter_supports_legacy_package_only_keys() {
    let root = Path::new("/repo");
    let results = AnalysisResults {
        unused_dependencies: vec![UnusedDependencyFinding::with_actions(UnusedDependency {
            package_name: "lodash-es".to_string(),
            location: DependencyLocation::Dependencies,
            path: PathBuf::from("/repo/packages/app/package.json"),
            line: 5,
            used_in_workspaces: Vec::new(),
            declared_and_imported_in: Vec::new(),
        })],
        ..Default::default()
    };
    let baseline = BaselineData {
        unused_dependencies: vec!["lodash-es".to_string()],
        ..BaselineData::legacy_from_results(&AnalysisResults::default(), root)
    };

    let filtered = filter_new_issues(results, &baseline, root);

    assert!(filtered.unused_dependencies.is_empty());
}

#[test]
fn baseline_serialization_roundtrip() {
    let results = make_results();
    let baseline = BaselineData::from_results(&results, Path::new(""));
    let json = serde_json::to_string(&baseline).unwrap();
    let deserialized: BaselineData = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.unused_files, baseline.unused_files);
    assert_eq!(deserialized.unused_exports, baseline.unused_exports);
    assert_eq!(deserialized.unused_types, baseline.unused_types);
    assert_eq!(
        deserialized.unused_dependencies,
        baseline.unused_dependencies
    );
    assert_eq!(
        deserialized.unused_dev_dependencies,
        baseline.unused_dev_dependencies
    );
}

#[test]
fn filter_removes_baseline_issues() {
    let results = make_results();
    let baseline = BaselineData::from_results(&results, Path::new(""));
    let filtered = filter_new_issues(results, &baseline, Path::new(""));
    assert!(
        filtered.unused_files.is_empty(),
        "all files were in baseline"
    );
    assert!(
        filtered.unused_exports.is_empty(),
        "all exports were in baseline"
    );
    assert!(
        filtered.unused_types.is_empty(),
        "all types were in baseline"
    );
    assert!(
        filtered.unused_dependencies.is_empty(),
        "all deps were in baseline"
    );
    assert!(
        filtered.unused_dev_dependencies.is_empty(),
        "all dev deps were in baseline"
    );
}

#[test]
fn filter_keeps_new_issues_not_in_baseline() {
    let baseline = BaselineData {
        kind: None,
        analysis_identity: install_state::BaselineAnalysisIdentity::default(),
        scope_reasons: Vec::new(),
        identity: None,
        unused_files: vec!["src/old.ts".to_string()],
        unused_exports: vec![],
        unused_types: vec![],
        private_type_leaks: vec![],
        deprecated_exports_in_use: vec![],
        unused_dependencies: vec![],
        unused_dev_dependencies: vec![],
        circular_dependencies: vec![],
        re_export_cycles: vec![],
        package_cycles: vec![],
        unused_optional_dependencies: vec![],
        unused_enum_members: vec![],
        unused_class_members: vec![],
        unused_store_members: vec![],
        unprovided_injects: vec![],
        unrendered_components: vec![],
        unused_component_props: vec![],
        absent_component_props: vec![],
        unused_component_emits: vec![],
        unused_component_inputs: vec![],
        unused_component_outputs: vec![],
        unused_svelte_events: vec![],
        unused_effect_layers: vec![],
        unused_tagged_errors: vec![],
        unproduced_catch_tags: vec![],
        unused_server_actions: vec![],
        unused_load_data_keys: vec![],
        unresolved_imports: vec![],
        unlisted_dependencies: vec![],
        duplicate_exports: vec![],
        type_only_dependencies: vec![],
        test_only_dependencies: vec![],
        dev_dependencies_in_production: vec![],
        boundary_violations: vec![],
        boundary_coverage_violations: vec![],
        boundary_call_violations: vec![],
        policy_violations: vec![],
        stale_suppressions: vec![],
        unused_catalog_entries: vec![],
        empty_catalog_groups: vec![],
        unresolved_catalog_references: vec![],
        unused_dependency_overrides: vec![],
        misconfigured_dependency_overrides: vec![],
        invalid_client_exports: vec![],
        mixed_client_server_barrels: vec![],
        misplaced_directives: vec![],
        route_collisions: vec![],
        dynamic_segment_name_conflicts: vec![],
        prop_drilling_chains: vec![],
        thin_wrappers: vec![],
        duplicate_prop_shapes: vec![],
    };
    let results = AnalysisResults {
        unused_files: vec![
            UnusedFileFinding::with_actions(UnusedFile {
                path: PathBuf::from("src/old.ts"),
            }),
            UnusedFileFinding::with_actions(UnusedFile {
                path: PathBuf::from("src/new-dead.ts"),
            }),
        ],
        ..Default::default()
    };
    let filtered = filter_new_issues(results, &baseline, Path::new(""));
    assert_eq!(filtered.unused_files.len(), 1);
    assert_eq!(
        filtered.unused_files[0].file.path,
        PathBuf::from("src/new-dead.ts")
    );
}

#[test]
fn filter_with_empty_baseline_keeps_all() {
    let baseline = BaselineData {
        kind: None,
        analysis_identity: install_state::BaselineAnalysisIdentity::default(),
        scope_reasons: Vec::new(),
        identity: None,
        unused_files: vec![],
        unused_exports: vec![],
        unused_types: vec![],
        private_type_leaks: vec![],
        deprecated_exports_in_use: vec![],
        unused_dependencies: vec![],
        unused_dev_dependencies: vec![],
        circular_dependencies: vec![],
        re_export_cycles: vec![],
        package_cycles: vec![],
        unused_optional_dependencies: vec![],
        unused_enum_members: vec![],
        unused_class_members: vec![],
        unused_store_members: vec![],
        unprovided_injects: vec![],
        unrendered_components: vec![],
        unused_component_props: vec![],
        absent_component_props: vec![],
        unused_component_emits: vec![],
        unused_component_inputs: vec![],
        unused_component_outputs: vec![],
        unused_svelte_events: vec![],
        unused_effect_layers: vec![],
        unused_tagged_errors: vec![],
        unproduced_catch_tags: vec![],
        unused_server_actions: vec![],
        unused_load_data_keys: vec![],
        unresolved_imports: vec![],
        unlisted_dependencies: vec![],
        duplicate_exports: vec![],
        type_only_dependencies: vec![],
        test_only_dependencies: vec![],
        dev_dependencies_in_production: vec![],
        boundary_violations: vec![],
        boundary_coverage_violations: vec![],
        boundary_call_violations: vec![],
        policy_violations: vec![],
        stale_suppressions: vec![],
        unused_catalog_entries: vec![],
        empty_catalog_groups: vec![],
        unresolved_catalog_references: vec![],
        unused_dependency_overrides: vec![],
        misconfigured_dependency_overrides: vec![],
        invalid_client_exports: vec![],
        mixed_client_server_barrels: vec![],
        misplaced_directives: vec![],
        route_collisions: vec![],
        dynamic_segment_name_conflicts: vec![],
        prop_drilling_chains: vec![],
        thin_wrappers: vec![],
        duplicate_prop_shapes: vec![],
    };
    let results = make_results();
    let filtered = filter_new_issues(results, &baseline, Path::new(""));
    assert_eq!(filtered.unused_files.len(), 2);
    assert_eq!(filtered.unused_exports.len(), 1);
}

#[test]
fn filter_new_exports_by_file_and_name() {
    let baseline = BaselineData {
        kind: None,
        analysis_identity: install_state::BaselineAnalysisIdentity::default(),
        scope_reasons: Vec::new(),
        identity: None,
        unused_files: vec![],
        unused_exports: vec!["src/utils.ts:helperA".to_string()],
        unused_types: vec![],
        private_type_leaks: vec![],
        deprecated_exports_in_use: vec![],
        unused_dependencies: vec![],
        unused_dev_dependencies: vec![],
        circular_dependencies: vec![],
        re_export_cycles: vec![],
        package_cycles: vec![],
        unused_optional_dependencies: vec![],
        unused_enum_members: vec![],
        unused_class_members: vec![],
        unused_store_members: vec![],
        unprovided_injects: vec![],
        unrendered_components: vec![],
        unused_component_props: vec![],
        absent_component_props: vec![],
        unused_component_emits: vec![],
        unused_component_inputs: vec![],
        unused_component_outputs: vec![],
        unused_svelte_events: vec![],
        unused_effect_layers: vec![],
        unused_tagged_errors: vec![],
        unproduced_catch_tags: vec![],
        unused_server_actions: vec![],
        unused_load_data_keys: vec![],
        unresolved_imports: vec![],
        unlisted_dependencies: vec![],
        duplicate_exports: vec![],
        type_only_dependencies: vec![],
        test_only_dependencies: vec![],
        dev_dependencies_in_production: vec![],
        boundary_violations: vec![],
        boundary_coverage_violations: vec![],
        boundary_call_violations: vec![],
        policy_violations: vec![],
        stale_suppressions: vec![],
        unused_catalog_entries: vec![],
        empty_catalog_groups: vec![],
        unresolved_catalog_references: vec![],
        unused_dependency_overrides: vec![],
        misconfigured_dependency_overrides: vec![],
        invalid_client_exports: vec![],
        mixed_client_server_barrels: vec![],
        misplaced_directives: vec![],
        route_collisions: vec![],
        dynamic_segment_name_conflicts: vec![],
        prop_drilling_chains: vec![],
        thin_wrappers: vec![],
        duplicate_prop_shapes: vec![],
    };
    let results = AnalysisResults {
        unused_exports: vec![
            UnusedExportFinding::with_actions(UnusedExport {
                path: PathBuf::from("src/utils.ts"),
                export_name: "helperA".to_string(),
                is_type_only: false,
                line: 5,
                col: 0,
                span_start: 40,
                is_re_export: false,
                deprecated: false,
                deprecated_reason: None,
            }),
            UnusedExportFinding::with_actions(UnusedExport {
                path: PathBuf::from("src/utils.ts"),
                export_name: "helperB".to_string(),
                is_type_only: false,
                line: 10,
                col: 0,
                span_start: 80,
                is_re_export: false,
                deprecated: false,
                deprecated_reason: None,
            }),
        ],
        ..Default::default()
    };
    let filtered = filter_new_issues(results, &baseline, Path::new(""));
    assert_eq!(filtered.unused_exports.len(), 1);
    assert_eq!(filtered.unused_exports[0].export.export_name, "helperB");
}

pub(super) fn make_clone_group(instances: Vec<(&str, usize, usize)>) -> CloneGroup {
    let mut files: Vec<&str> = instances.iter().map(|(file, _, _)| *file).collect();
    files.sort_unstable();
    let fragment = format!("const source = '{}';", files.join(","));
    make_clone_group_with_fragment(&fragment, instances)
}

fn make_clone_group_with_fragment(
    fragment: &str,
    instances: Vec<(&str, usize, usize)>,
) -> CloneGroup {
    CloneGroup {
        instances: instances
            .into_iter()
            .map(|(file, start, end)| CloneInstance {
                is_symlink: false,
                file: PathBuf::from(file),
                start_line: start,
                end_line: end,
                start_col: 0,
                end_col: 0,
                fragment: fragment.to_string(),
            })
            .collect(),
        token_count: 50,
        line_count: 10,
        similarity: None,
    }
}

pub(super) fn make_duplication_report(groups: Vec<CloneGroup>) -> DuplicationReport {
    DuplicationReport {
        clone_groups: groups,
        clone_families: vec![],
        mirrored_directories: vec![],
        stats: DuplicationStats {
            total_files: 10,
            files_with_clones: 2,
            total_lines: 1000,
            duplicated_lines: 100,
            total_tokens: 5000,
            duplicated_tokens: 500,
            clone_groups: 1,
            clone_families: 0,
            clone_instances: 2,
            duplication_percentage: 10.0,
            clone_groups_below_min_occurrences: 0,
            clone_groups_ignored: 0,
            near_candidates_skipped: 0,
        },
    }
}

fn normalized_clone_group_key(group: &CloneGroup) -> String {
    let fingerprints =
        crate::duplicates::CloneFingerprintSet::from_groups(std::slice::from_ref(group));
    clone_group_fingerprint_key(group, &fingerprints)
}

#[test]
fn clone_group_key_is_deterministic() {
    let root = Path::new("/project");
    let group = make_clone_group(vec![
        ("/project/src/a.ts", 1, 10),
        ("/project/src/b.ts", 5, 15),
    ]);
    let key1 = clone_group_key(&group, root);
    let key2 = clone_group_key(&group, root);
    assert_eq!(key1, key2);
}

#[test]
fn clone_group_key_is_sorted() {
    let root = Path::new("/project");
    let group_ab = make_clone_group(vec![
        ("/project/src/a.ts", 1, 10),
        ("/project/src/b.ts", 5, 15),
    ]);
    let group_ba = make_clone_group(vec![
        ("/project/src/b.ts", 5, 15),
        ("/project/src/a.ts", 1, 10),
    ]);
    assert_eq!(
        clone_group_key(&group_ab, root),
        clone_group_key(&group_ba, root),
        "key should be stable regardless of instance order"
    );
}

#[test]
fn duplication_baseline_roundtrip() {
    let root = Path::new("/project");
    let group = make_clone_group(vec![
        ("/project/src/a.ts", 1, 10),
        ("/project/src/b.ts", 5, 15),
    ]);
    let report = make_duplication_report(vec![group]);
    let baseline = DuplicationBaselineData::from_report(&report, root);
    let json = serde_json::to_string(&baseline).unwrap();
    let deserialized: DuplicationBaselineData = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.clone_groups, baseline.clone_groups);
    assert_eq!(deserialized.clone_fingerprints, baseline.clone_fingerprints);
    assert_eq!(
        deserialized.normalized_clone_fingerprints,
        baseline.normalized_clone_fingerprints
    );
    assert_eq!(
        baseline.normalized_clone_fingerprints.len(),
        1,
        "a saved baseline carries a normalized key per clone group"
    );
}

/// The key list is what separates a foreign file from a legitimately empty
/// baseline, so a field added to the format without a key here would make
/// its own saved baseline read as foreign.
///
/// `kind` is compared out rather than added: it is the one key every format
/// writes, so listing it would make each format declare all three and turn
/// the key fallback into a coin flip for files that carry no `kind`.
#[test]
fn the_declared_keys_are_every_key_each_format_writes() {
    let duplication = DuplicationBaselineData {
        kind: Some(BaselineKind::Dupes),
        clone_groups: vec!["src/a.ts:1-10".to_owned()],
        clone_fingerprints: vec!["abc".to_owned()],
        normalized_clone_fingerprints: vec!["def".to_owned()],
    };
    assert_eq!(
        serialized_keys_without_kind(&duplication),
        DuplicationBaselineData::DECLARED_KEYS
    );

    let counts: HealthFindingCountMap = std::iter::once((
        "src/a.ts".to_owned(),
        std::iter::once(("complexity".to_owned(), HealthBaselineCount { count: 1 })).collect(),
    ))
    .collect();
    let health = HealthBaselineData {
        kind: Some(BaselineKind::Health),
        findings: vec!["src/a.ts:run:1".to_owned()],
        finding_counts: counts.clone(),
        identity_finding_counts: counts,
        runtime_coverage_findings: vec!["src/a.ts:run".to_owned()],
        runtime_coverage_source_hashes: vec!["src/a.ts\0run\0hash".to_owned()],
        target_keys: vec!["src/a.ts:complexity".to_owned()],
    };
    assert_eq!(
        serialized_keys_without_kind(&health),
        HealthBaselineData::DECLARED_KEYS
    );

    for list in [
        DuplicationBaselineData::DECLARED_KEYS,
        HealthBaselineData::DECLARED_KEYS,
        BaselineData::REQUIRED_KEYS,
    ] {
        assert!(
            !list.contains(&"kind"),
            "no format may declare the key every format writes: {list:?}"
        );
    }
}

/// The five keys the dead-code format cannot load without, which is what
/// makes "carries none of them" a safe reading of "not a dead-code
/// baseline". Derived from the format rather than restated, so a field that
/// loses its serde default has to be listed.
#[test]
fn the_required_dead_code_keys_are_the_ones_without_a_serde_default() {
    let json = serde_json::to_string(&BaselineData::from_results(
        &crate::results::AnalysisResults::default(),
        Path::new("/project"),
    ))
    .expect("baseline serializes");
    let serde_json::Value::Object(object) =
        serde_json::from_str::<serde_json::Value>(&json).expect("object")
    else {
        panic!("a baseline serializes as an object");
    };

    for key in object.keys() {
        let mut without = object.clone();
        without.remove(key);
        let loads =
            serde_json::from_value::<BaselineData>(serde_json::Value::Object(without)).is_ok();
        assert_eq!(
            !loads,
            BaselineData::REQUIRED_KEYS.contains(&key.as_str()),
            "REQUIRED_KEYS must list exactly the keys a load cannot do without, and {key} \
                 disagrees"
        );
    }
}

#[test]
fn every_saved_baseline_names_the_command_that_wrote_it() {
    let dead_code = serde_json::to_string(&BaselineData::from_results(
        &crate::results::AnalysisResults::default(),
        Path::new("/project"),
    ))
    .expect("baseline serializes");
    let dupes = serde_json::to_string(&DuplicationBaselineData::from_report(
        &make_duplication_report(Vec::new()),
        Path::new("/project"),
    ))
    .expect("baseline serializes");
    let health = serde_json::to_string(&HealthBaselineData::from_findings(
        &[],
        &[],
        &[],
        Path::new("/project"),
    ))
    .expect("baseline serializes");

    for (json, kind) in [
        (&dead_code, BaselineKind::DeadCode),
        (&dupes, BaselineKind::Dupes),
        (&health, BaselineKind::Health),
    ] {
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(json).expect("object")["kind"],
            serde_json::json!(kind.as_str()),
            "a saved baseline states which command wrote it: {json}"
        );
        assert_eq!(classify_baseline_file(json, kind), BaselineFileKind::Own);
        for other in [
            BaselineKind::DeadCode,
            BaselineKind::Dupes,
            BaselineKind::Health,
        ] {
            if other == kind {
                continue;
            }
            assert_eq!(
                classify_baseline_file(json, other),
                BaselineFileKind::Foreign(kind.as_str().to_owned()),
                "and every other command reads it as that command's: {json}"
            );
        }
    }
}

/// A baseline saved by the previous release carries no `kind`, so the keys
/// decide, which is exactly today's behaviour and must stay it.
#[test]
fn a_baseline_without_a_kind_is_classified_by_its_keys() {
    let dupes = r#"{"clone_groups":[],"clone_fingerprints":[],"normalized_clone_fingerprints":[]}"#;
    let health = r#"{"runtime_coverage_findings":[],"target_keys":[]}"#;
    let dead_code = r#"{"unused_files":[],"unused_exports":[],"unused_types":[],"unused_dependencies":[],"unused_dev_dependencies":[]}"#;

    for (json, own) in [
        (dupes, BaselineKind::Dupes),
        (health, BaselineKind::Health),
        (dead_code, BaselineKind::DeadCode),
    ] {
        assert_eq!(classify_baseline_file(json, own), BaselineFileKind::Own);
        for other in [
            BaselineKind::DeadCode,
            BaselineKind::Dupes,
            BaselineKind::Health,
        ] {
            if other == own {
                continue;
            }
            assert_eq!(
                classify_baseline_file(json, other),
                BaselineFileKind::Unrecognised,
                "a file with no kind and none of this format's keys is unrecognised, not \
                     attributed to a command it never named: {json}"
            );
        }
    }
}

/// Only the file's own statement is trusted. A `kind` a newer fallow writes
/// must read as another command's file rather than as a parse error, and a
/// `kind` that is not a string is no statement at all.
#[test]
fn an_unreadable_kind_never_becomes_a_parse_error() {
    assert_eq!(
        classify_baseline_file(r#"{"kind":"security"}"#, BaselineKind::Dupes),
        BaselineFileKind::Foreign("security".to_owned())
    );
    assert_eq!(
        classify_baseline_file(r#"{"kind":7,"clone_groups":[]}"#, BaselineKind::Dupes),
        BaselineFileKind::Own
    );
    assert_eq!(
        classify_baseline_file("[]", BaselineKind::Dupes),
        BaselineFileKind::NotAnObject
    );
    assert_eq!(
        classify_baseline_file("not json", BaselineKind::DeadCode),
        BaselineFileKind::NotAnObject
    );
    assert!(
        serde_json::from_str::<DuplicationBaselineData>(r#"{"kind":"security"}"#).is_ok(),
        "a kind only a newer fallow writes must not break a load"
    );
}

#[test]
fn a_save_is_refused_only_over_another_commands_baseline() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("baseline.json");

    assert!(
        refuse_baseline_kind_overwrite(&path, BaselineKind::Dupes).is_none(),
        "there is nothing to destroy yet"
    );

    std::fs::write(
        &path,
        serde_json::to_string(&DuplicationBaselineData::from_report(
            &make_duplication_report(Vec::new()),
            Path::new("/project"),
        ))
        .expect("baseline serializes"),
    )
    .expect("write");
    assert!(
        refuse_baseline_kind_overwrite(&path, BaselineKind::Dupes).is_none(),
        "re-saving over its own file is the documented workflow"
    );
    let message = refuse_baseline_kind_overwrite(&path, BaselineKind::Health)
        .expect("a health save over a duplication baseline is refused");
    assert!(message.contains("`fallow dupes`"), "{message}");
    assert!(message.contains("`fallow health`"), "{message}");
    assert!(message.contains(&path.display().to_string()), "{message}");

    std::fs::write(&path, "{}").expect("write");
    assert!(
        refuse_baseline_kind_overwrite(&path, BaselineKind::Health).is_none(),
        "a file with nothing to identify it carries no claim to protect"
    );
}

fn serialized_keys_without_kind<T: serde::Serialize>(value: &T) -> Vec<String> {
    let serde_json::Value::Object(object) =
        serde_json::to_value(value).expect("baseline serializes")
    else {
        panic!("a baseline serializes as an object");
    };
    object
        .keys()
        .filter(|key| key.as_str() != "kind")
        .cloned()
        .collect()
}

#[test]
fn a_baseline_saved_from_a_clean_project_still_declares_its_format() {
    let empty =
        serde_json::to_string(&DuplicationBaselineData::default()).expect("baseline serializes");

    assert!(
        declares_baseline_format(&empty, DuplicationBaselineData::DECLARED_KEYS),
        "an empty duplication baseline is still a duplication baseline: {empty}"
    );
    assert!(
        !declares_baseline_format(&empty, HealthBaselineData::DECLARED_KEYS),
        "and it is not a health one: {empty}"
    );
    assert!(!declares_baseline_format(
        "{}",
        HealthBaselineData::DECLARED_KEYS
    ));
    assert!(!declares_baseline_format(
        "[]",
        HealthBaselineData::DECLARED_KEYS
    ));
    assert!(!declares_baseline_format(
        "not json",
        HealthBaselineData::DECLARED_KEYS
    ));
}

#[test]
fn filter_new_clone_groups_matches_shifted_clone() {
    let root = Path::new("/project");
    let baseline_report = make_duplication_report(vec![make_clone_group_with_fragment(
        "const total = a + b;",
        vec![("/project/src/a.ts", 10, 20), ("/project/src/b.ts", 30, 40)],
    )]);
    let baseline = DuplicationBaselineData::from_report(&baseline_report, root);

    let shifted = make_duplication_report(vec![make_clone_group_with_fragment(
        "const total = a + b;",
        vec![("/project/src/a.ts", 18, 28), ("/project/src/b.ts", 30, 40)],
    )]);
    let filtered = filter_new_clone_groups(shifted, &baseline, root);
    assert!(
        filtered.clone_groups.is_empty(),
        "an unrelated line shift must not resurface a baselined clone"
    );
}

/// A baseline filter keeps the family order of every other path: the file
/// set. The larger family has the later file set, so an order by
/// duplicated lines puts it first.
#[test]
fn filter_new_clone_groups_orders_families_by_file_set() {
    let root = Path::new("/project");
    let baseline = DuplicationBaselineData::from_report(&make_duplication_report(vec![]), root);
    let mut large = make_clone_group_with_fragment(
        "const large = 1;",
        vec![("/project/src/y.ts", 1, 46), ("/project/src/z.ts", 1, 46)],
    );
    large.line_count = 46;
    let small = make_clone_group_with_fragment(
        "const small = 1;",
        vec![("/project/src/a.ts", 1, 7), ("/project/src/b.ts", 1, 7)],
    );

    let filtered =
        filter_new_clone_groups(make_duplication_report(vec![large, small]), &baseline, root);

    let order: Vec<&Path> = filtered
        .clone_families
        .iter()
        .map(|family| family.files[0].as_path())
        .collect();
    assert_eq!(
        order,
        [
            Path::new("/project/src/a.ts"),
            Path::new("/project/src/y.ts")
        ]
    );
}

#[test]
fn filter_new_clone_groups_reports_extra_copy() {
    let root = Path::new("/project");
    let baseline_report = make_duplication_report(vec![make_clone_group_with_fragment(
        "const total = a + b;",
        vec![("/project/src/a.ts", 10, 20), ("/project/src/b.ts", 30, 40)],
    )]);
    let baseline = DuplicationBaselineData::from_report(&baseline_report, root);

    let with_third_copy = make_duplication_report(vec![make_clone_group_with_fragment(
        "const total = a + b;",
        vec![
            ("/project/src/a.ts", 10, 20),
            ("/project/src/b.ts", 30, 40),
            ("/project/src/c.ts", 5, 15),
        ],
    )]);
    let filtered = filter_new_clone_groups(with_third_copy, &baseline, root);
    assert_eq!(
        filtered.clone_groups.len(),
        1,
        "a fresh copy in a third file is a new finding"
    );
}

#[test]
fn filter_new_clone_groups_reads_legacy_baseline() {
    let root = Path::new("/project");
    let legacy_json = r#"{"clone_groups":["src/a.ts:10-20|src/b.ts:30-40"]}"#;
    let baseline: DuplicationBaselineData = serde_json::from_str(legacy_json).unwrap();
    assert_eq!(baseline.entry_count(), 1);

    let unchanged = make_duplication_report(vec![make_clone_group_with_fragment(
        "const total = a + b;",
        vec![("/project/src/a.ts", 10, 20), ("/project/src/b.ts", 30, 40)],
    )]);
    assert!(
        filter_new_clone_groups(unchanged, &baseline, root)
            .clone_groups
            .is_empty(),
        "a legacy baseline still matches on locations"
    );

    let shifted = make_duplication_report(vec![make_clone_group_with_fragment(
        "const total = a + b;",
        vec![("/project/src/a.ts", 18, 28), ("/project/src/b.ts", 30, 40)],
    )]);
    assert_eq!(
        filter_new_clone_groups(shifted, &baseline, root)
            .clone_groups
            .len(),
        1,
        "legacy behavior is unchanged: a shift stops matching"
    );
}

#[test]
fn clone_group_fingerprint_key_survives_file_rename() {
    let before = make_clone_group_with_fragment(
        "const total = a + b;",
        vec![("/project/src/a.ts", 10, 20), ("/project/src/b.ts", 30, 40)],
    );
    let after = make_clone_group_with_fragment(
        "const total = a + b;",
        vec![
            ("/project/src/renamed.ts", 10, 20),
            ("/project/src/b.ts", 30, 40),
        ],
    );
    assert_eq!(
        normalized_clone_group_key(&before),
        normalized_clone_group_key(&after),
        "renaming a file must not resurface a baselined clone"
    );
}

#[test]
fn clone_group_fingerprint_key_does_not_follow_representative_order() {
    let mut before = make_clone_group_with_fragment(
        "const total = a + b;",
        vec![("/project/src/a.ts", 10, 20), ("/project/src/b.ts", 30, 40)],
    );
    before.instances[1].fragment = "const total = a  +  b;".to_string();

    let mut after = before.clone();
    after.instances[0].file = PathBuf::from("/project/src/z.ts");

    assert_eq!(
        normalized_clone_group_key(&before),
        normalized_clone_group_key(&after),
        "normalized group identity is independent of representative order"
    );
}

#[test]
fn filter_new_clone_groups_matches_legacy_raw_fingerprint_key() {
    let root = Path::new("/project");
    let group = make_clone_group_with_fragment(
        "const total = a + b;",
        vec![("/project/src/a.ts", 10, 20), ("/project/src/b.ts", 30, 40)],
    );
    let legacy_key = legacy_clone_group_fingerprint_key(&group);
    let baseline = DuplicationBaselineData {
        kind: None,
        clone_groups: Vec::new(),
        clone_fingerprints: vec![legacy_key.clone()],
        normalized_clone_fingerprints: Vec::new(),
    };

    let filtered = filter_new_clone_groups(
        make_duplication_report(vec![group.clone()]),
        &baseline,
        root,
    );
    assert!(filtered.clone_groups.is_empty());

    let current = DuplicationBaselineData::from_report(&make_duplication_report(vec![group]), root);
    assert_eq!(current.clone_fingerprints, vec![legacy_key]);
    assert_ne!(
        current.normalized_clone_fingerprints,
        current.clone_fingerprints
    );
}

#[test]
fn normalized_baseline_survives_formatting_only_edits() {
    let root = Path::new("/project");
    let baseline_report = make_duplication_report(vec![make_clone_group_with_fragment(
        "const total = left + right;",
        vec![("/project/src/a.ts", 10, 20), ("/project/src/b.ts", 30, 40)],
    )]);
    let baseline = DuplicationBaselineData::from_report(&baseline_report, root);

    let formatted = make_duplication_report(vec![make_clone_group_with_fragment(
        "/* reviewed */\r\nconst  total=left + right;",
        vec![("/project/src/a.ts", 18, 28), ("/project/src/b.ts", 35, 45)],
    )]);
    assert!(
        filter_new_clone_groups(formatted, &baseline, root)
            .clone_groups
            .is_empty()
    );
}

#[test]
fn filter_new_clone_groups_removes_baseline() {
    let root = Path::new("/project");
    let group = make_clone_group(vec![
        ("/project/src/a.ts", 1, 10),
        ("/project/src/b.ts", 5, 15),
    ]);
    let report = make_duplication_report(vec![group]);
    let baseline = DuplicationBaselineData::from_report(&report, root);
    let filtered = filter_new_clone_groups(report, &baseline, root);
    assert!(
        filtered.clone_groups.is_empty(),
        "baseline group should be filtered out"
    );
}

#[test]
fn filter_new_clone_groups_keeps_new_groups() {
    let root = Path::new("/project");
    let baseline_group = make_clone_group(vec![
        ("/project/src/a.ts", 1, 10),
        ("/project/src/b.ts", 5, 15),
    ]);
    let new_group = make_clone_group(vec![
        ("/project/src/c.ts", 20, 30),
        ("/project/src/d.ts", 25, 35),
    ]);
    let baseline_report = make_duplication_report(vec![baseline_group]);
    let baseline = DuplicationBaselineData::from_report(&baseline_report, root);

    let report = make_duplication_report(vec![
        make_clone_group(vec![
            ("/project/src/a.ts", 1, 10),
            ("/project/src/b.ts", 5, 15),
        ]),
        new_group,
    ]);
    let filtered = filter_new_clone_groups(report, &baseline, root);
    assert_eq!(
        filtered.clone_groups.len(),
        1,
        "only the new group should remain"
    );
}

#[test]
fn recompute_stats_after_filtering() {
    let root = Path::new("/project");
    let group = make_clone_group(vec![
        ("/project/src/a.ts", 1, 10),
        ("/project/src/b.ts", 5, 15),
    ]);
    let report = make_duplication_report(vec![group]);
    let baseline = DuplicationBaselineData::from_report(&report, root);
    let filtered = filter_new_clone_groups(report, &baseline, root);
    assert_eq!(filtered.stats.clone_groups, 0);
    assert_eq!(filtered.stats.clone_instances, 0);
    assert_eq!(filtered.stats.duplicated_lines, 0);
}

#[test]
fn recompute_stats_zero_total_lines() {
    let report = DuplicationReport {
        clone_groups: vec![],
        clone_families: vec![],
        mirrored_directories: vec![],
        stats: DuplicationStats {
            total_files: 0,
            files_with_clones: 0,
            total_lines: 0,
            duplicated_lines: 0,
            total_tokens: 0,
            duplicated_tokens: 0,
            clone_groups: 0,
            clone_families: 0,
            clone_instances: 0,
            duplication_percentage: 0.0,
            clone_groups_below_min_occurrences: 0,
            clone_groups_ignored: 0,
            near_candidates_skipped: 0,
        },
    };
    let stats = super::recompute_stats(&report);
    assert!((stats.duplication_percentage - 0.0).abs() < f64::EPSILON);
}

/// Count-mode wrapper shadowing the mode-aware function, so the existing
/// count-mode expectations stay readable. Identity-mode tests call
/// `super::filter_new_health_findings` directly.
fn filter_new_health_findings(
    findings: Vec<fallow_output::ComplexityViolation>,
    baseline: &HealthBaselineData,
    root: &Path,
) -> Vec<fallow_output::ComplexityViolation> {
    super::filter_new_health_findings(findings, baseline, root, HealthBaselineMode::Count)
}

fn make_health_finding(root: &Path, name: &str, line: u32) -> fallow_output::ComplexityViolation {
    make_health_finding_with(
        root,
        name,
        line,
        fallow_output::ExceededThreshold::Both,
        fallow_output::FindingSeverity::High,
    )
}

pub(super) fn make_health_finding_with(
    root: &Path,
    name: &str,
    line: u32,
    exceeded: fallow_output::ExceededThreshold,
    severity: fallow_output::FindingSeverity,
) -> fallow_output::ComplexityViolation {
    fallow_output::ComplexityViolation {
        path: root.join("src/utils.ts"),
        name: name.to_string(),
        line,
        col: 0,
        cyclomatic: 25,
        cognitive: 30,
        line_count: 80,
        param_count: 0,
        react_hook_count: 0,
        react_jsx_max_depth: 0,
        react_prop_count: 0,
        react_hook_profile: None,
        exceeded,
        severity,
        ..Default::default()
    }
}

#[test]
fn health_baseline_roundtrip() {
    let root = PathBuf::from("/project");
    let findings = vec![make_health_finding(&root, "parseExpression", 42)];
    let baseline = HealthBaselineData::from_findings(&findings, &[], &[], &root);
    let json = serde_json::to_string(&baseline).unwrap();
    let deserialized: HealthBaselineData = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.findings, baseline.findings);
    assert_eq!(baseline.findings, Vec::<String>::new());
    assert_eq!(
        deserialized.finding_counts["src/utils.ts"]["complexity_high"].count,
        1
    );
    assert!(!json.contains("parseExpression"));
}

#[test]
fn health_baseline_filters_known_findings() {
    let root = PathBuf::from("/project");
    let mut findings = vec![
        make_health_finding(&root, "parseExpression", 42),
        make_health_finding(&root, "newFunction", 100),
    ];
    findings[1].path = root.join("src/other.ts");
    let baseline = HealthBaselineData::from_findings(&findings[..1], &[], &[], &root);
    let filtered = filter_new_health_findings(findings, &baseline, &root);
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].name, "newFunction");
}

#[test]
fn health_baseline_filters_shifted_lines_with_same_category_count() {
    let root = PathBuf::from("/project");
    let baseline = HealthBaselineData::from_findings(
        &[make_health_finding(&root, "parseExpression", 42)],
        &[],
        &[],
        &root,
    );
    let filtered = filter_new_health_findings(
        vec![make_health_finding(&root, "parseExpression", 43)],
        &baseline,
        &root,
    );
    assert!(filtered.is_empty());
}

#[test]
fn health_baseline_reports_full_category_when_count_increases() {
    let root = PathBuf::from("/project");
    let baseline = HealthBaselineData::from_findings(
        &[make_health_finding(&root, "parseExpression", 42)],
        &[],
        &[],
        &root,
    );
    let filtered = filter_new_health_findings(
        vec![
            make_health_finding(&root, "parseExpression", 43),
            make_health_finding(&root, "newFunction", 100),
        ],
        &baseline,
        &root,
    );
    assert_eq!(filtered.len(), 2);
}

#[test]
fn health_baseline_legacy_findings_still_load() {
    let root = PathBuf::from("/project");
    let baseline = HealthBaselineData {
        kind: None,
        findings: vec!["src/utils.ts:parseExpression:42".to_owned()],
        finding_counts: BTreeMap::new(),
        identity_finding_counts: BTreeMap::new(),
        target_keys: vec![],
        runtime_coverage_findings: vec![],
        runtime_coverage_source_hashes: vec![],
    };
    let filtered = filter_new_health_findings(
        vec![make_health_finding(&root, "parseExpression", 42)],
        &baseline,
        &root,
    );
    assert!(filtered.is_empty());
}

#[test]
fn health_baseline_keeps_crap_categories_separate_from_complexity() {
    let root = PathBuf::from("/project");
    let baseline = HealthBaselineData::from_findings(
        &[make_health_finding_with(
            &root,
            "parseExpression",
            42,
            fallow_output::ExceededThreshold::Crap,
            fallow_output::FindingSeverity::High,
        )],
        &[],
        &[],
        &root,
    );
    let filtered = filter_new_health_findings(
        vec![
            make_health_finding_with(
                &root,
                "parseExpression",
                43,
                fallow_output::ExceededThreshold::Crap,
                fallow_output::FindingSeverity::High,
            ),
            make_health_finding(&root, "newComplexityOnlyFunction", 100),
        ],
        &baseline,
        &root,
    );
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].name, "newComplexityOnlyFunction");
}

#[test]
fn health_baseline_suppresses_findings_that_only_improve_in_severity() {
    let root = PathBuf::from("/project");
    let baseline = HealthBaselineData::from_findings(
        &[make_health_finding_with(
            &root,
            "parseExpression",
            42,
            fallow_output::ExceededThreshold::Both,
            fallow_output::FindingSeverity::Critical,
        )],
        &[],
        &[],
        &root,
    );
    let filtered = filter_new_health_findings(
        vec![make_health_finding_with(
            &root,
            "parseExpression",
            42,
            fallow_output::ExceededThreshold::Both,
            fallow_output::FindingSeverity::High,
        )],
        &baseline,
        &root,
    );
    assert!(filtered.is_empty());
}

#[test]
fn health_baseline_still_reports_worse_current_severity_as_new() {
    let root = PathBuf::from("/project");
    let baseline = HealthBaselineData::from_findings(
        &[make_health_finding_with(
            &root,
            "parseExpression",
            42,
            fallow_output::ExceededThreshold::Both,
            fallow_output::FindingSeverity::High,
        )],
        &[],
        &[],
        &root,
    );
    let filtered = filter_new_health_findings(
        vec![make_health_finding_with(
            &root,
            "parseExpression",
            42,
            fallow_output::ExceededThreshold::Both,
            fallow_output::FindingSeverity::Critical,
        )],
        &baseline,
        &root,
    );
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].name, "parseExpression");
    assert!(matches!(
        filtered[0].severity,
        fallow_output::FindingSeverity::Critical
    ));
}

#[test]
fn health_baseline_overlap_counts_partial_category_overflow() {
    let root = PathBuf::from("/project");
    let baseline = HealthBaselineData::from_findings(
        &[make_health_finding(&root, "parseExpression", 42)],
        &[],
        &[],
        &root,
    );
    let overlap = baseline.overlap_entries(
        &[
            make_health_finding(&root, "parseExpression", 42),
            make_health_finding(&root, "newFunction", 100),
        ],
        &root,
        HealthBaselineMode::Count,
    );
    assert_eq!(overlap.matched_entries, 1);
    assert_eq!(overlap.moved_entries, 0);
}

/// Baseline saved the way `--baseline-mode identity` saves it.
fn identity_baseline(
    findings: &[fallow_output::ComplexityViolation],
    root: &Path,
) -> HealthBaselineData {
    HealthBaselineData::from_findings(findings, &[], &[], root).with_identity(findings, root)
}

#[test]
fn health_identity_baseline_reports_replacement_hotspot() {
    let root = PathBuf::from("/project");
    let baseline = identity_baseline(&[make_health_finding(&root, "firstHotspot", 3)], &root);
    let replacement = vec![make_health_finding(&root, "replacementHotspot", 3)];

    assert!(
        filter_new_health_findings(replacement.clone(), &baseline, &root).is_empty(),
        "count mode keeps the per-file allowance and suppresses the replacement"
    );

    let filtered = super::filter_new_health_findings(
        replacement,
        &baseline,
        &root,
        HealthBaselineMode::Identity,
    );
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].name, "replacementHotspot");
}

#[test]
fn health_identity_baseline_survives_line_moves() {
    let root = PathBuf::from("/project");
    let baseline = identity_baseline(&[make_health_finding(&root, "parseExpression", 42)], &root);
    let filtered = super::filter_new_health_findings(
        vec![make_health_finding(&root, "parseExpression", 512)],
        &baseline,
        &root,
        HealthBaselineMode::Identity,
    );
    assert!(filtered.is_empty());
}

#[test]
fn health_identity_baseline_suppresses_severity_improvement() {
    let root = PathBuf::from("/project");
    let baseline = identity_baseline(
        &[make_health_finding_with(
            &root,
            "parseExpression",
            42,
            fallow_output::ExceededThreshold::Both,
            fallow_output::FindingSeverity::Critical,
        )],
        &root,
    );
    let filtered = super::filter_new_health_findings(
        vec![make_health_finding_with(
            &root,
            "parseExpression",
            42,
            fallow_output::ExceededThreshold::Both,
            fallow_output::FindingSeverity::Moderate,
        )],
        &baseline,
        &root,
        HealthBaselineMode::Identity,
    );
    assert!(filtered.is_empty());
}

#[test]
fn health_identity_baseline_reports_added_finding_for_known_function() {
    let root = PathBuf::from("/project");
    let baseline = identity_baseline(&[make_health_finding(&root, "parseExpression", 42)], &root);
    let filtered = super::filter_new_health_findings(
        vec![
            make_health_finding(&root, "parseExpression", 42),
            make_health_finding(&root, "parseStatement", 90),
        ],
        &baseline,
        &root,
        HealthBaselineMode::Identity,
    );
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].name, "parseStatement");
}

#[test]
fn health_identity_buckets_are_written_only_in_identity_mode() {
    let root = PathBuf::from("/project");
    let findings = [make_health_finding(&root, "parseExpression", 42)];
    let count_only = HealthBaselineData::from_findings(&findings, &[], &[], &root);
    let json = serde_json::to_string(&count_only).unwrap();
    assert!(!json.contains("identity_finding_counts"));
    assert!(count_only.lacks_identity_data());

    let identity = identity_baseline(&findings, &root);
    assert!(!identity.lacks_identity_data());
    assert_eq!(
        identity.identity_finding_counts["src/utils.ts\0parseExpression"]["complexity_high"].count,
        1
    );
    assert!(
        !identity.finding_counts.is_empty(),
        "an identity baseline stays readable in count mode"
    );
}

fn moved_finding(root: &Path, path: &str, name: &str) -> fallow_output::ComplexityViolation {
    let mut finding = make_health_finding(root, name, 42);
    finding.path = root.join(path);
    finding
}

#[test]
fn health_identity_baseline_follows_file_move() {
    let root = PathBuf::from("/project");
    let baseline = identity_baseline(&[make_health_finding(&root, "parseExpression", 42)], &root);
    let moved = vec![moved_finding(
        &root,
        "src/parser/utils.ts",
        "parseExpression",
    )];

    let overlap = baseline.overlap_entries(&moved, &root, HealthBaselineMode::Identity);
    assert_eq!(
        overlap.matched_entries, 1,
        "a followed move counts as matched, not stale"
    );
    assert_eq!(
        overlap.moved_entries, 1,
        "the followed move stays observable as a moved entry"
    );
    let filtered =
        super::filter_new_health_findings(moved, &baseline, &root, HealthBaselineMode::Identity);
    assert!(filtered.is_empty());
}

#[test]
fn health_identity_move_is_not_followed_when_candidates_are_ambiguous() {
    let root = PathBuf::from("/project");
    let baseline = identity_baseline(&[make_health_finding(&root, "parseExpression", 42)], &root);
    let filtered = super::filter_new_health_findings(
        vec![
            moved_finding(&root, "src/a.ts", "parseExpression"),
            moved_finding(&root, "src/b.ts", "parseExpression"),
        ],
        &baseline,
        &root,
        HealthBaselineMode::Identity,
    );
    assert_eq!(filtered.len(), 2);
}

#[test]
fn health_identity_move_is_not_followed_when_candidate_is_claimed_twice() {
    let root = PathBuf::from("/project");
    let baseline = identity_baseline(
        &[
            moved_finding(&root, "src/a.ts", "parseExpression"),
            moved_finding(&root, "src/b.ts", "parseExpression"),
        ],
        &root,
    );
    let filtered = super::filter_new_health_findings(
        vec![moved_finding(&root, "src/c.ts", "parseExpression")],
        &baseline,
        &root,
        HealthBaselineMode::Identity,
    );
    assert_eq!(filtered.len(), 1);
}

#[test]
fn health_identity_move_is_not_followed_when_old_path_still_exists() {
    // The manifest dir makes the saved path a file that really exists, so
    // the function was fixed or deleted in place rather than moved.
    let root = std::path::PathBuf::from(
        std::env::var_os("CARGO_MANIFEST_DIR").expect("cargo sets CARGO_MANIFEST_DIR"),
    );
    let baseline = identity_baseline(
        &[moved_finding(
            &root,
            "src/baseline/mod.rs",
            "parseExpression",
        )],
        &root,
    );
    let filtered = super::filter_new_health_findings(
        vec![moved_finding(&root, "src/moved.ts", "parseExpression")],
        &baseline,
        &root,
        HealthBaselineMode::Identity,
    );
    assert_eq!(filtered.len(), 1);
}

#[test]
fn health_identity_move_is_not_followed_for_anonymous_functions() {
    let root = PathBuf::from("/project");
    let baseline = identity_baseline(&[make_health_finding(&root, "<anonymous>", 42)], &root);
    let filtered = super::filter_new_health_findings(
        vec![moved_finding(&root, "src/moved.ts", "<anonymous>")],
        &baseline,
        &root,
        HealthBaselineMode::Identity,
    );
    assert_eq!(filtered.len(), 1);
}

#[test]
fn health_identity_move_is_not_followed_for_arrow_placeholders() {
    let root = PathBuf::from("/project");
    let baseline = identity_baseline(&[make_health_finding(&root, "<arrow>", 42)], &root);
    let filtered = super::filter_new_health_findings(
        vec![moved_finding(&root, "src/moved.ts", "<arrow>")],
        &baseline,
        &root,
        HealthBaselineMode::Identity,
    );
    assert_eq!(filtered.len(), 1);
}

#[test]
fn health_identity_placeholder_allowance_covers_new_context_names() {
    let root = PathBuf::from("/project");
    let baseline = identity_baseline(
        &[
            make_health_finding(&root, "<anonymous>", 10),
            make_health_finding(&root, "<arrow>", 20),
        ],
        &root,
    );
    let current = vec![
        make_health_finding(&root, "fetchIssue", 10),
        make_health_finding(&root, "Command.make(\"setup\")", 20),
    ];
    let filtered = super::filter_new_health_findings(
        current.clone(),
        &baseline,
        &root,
        HealthBaselineMode::Identity,
    );
    assert!(filtered.is_empty(), "{filtered:?}");
    let overlap = baseline.overlap_entries(&current, &root, HealthBaselineMode::Identity);
    assert_eq!(overlap.matched_entries, 2);
    assert_eq!(overlap.moved_entries, 0);
}

#[test]
fn health_identity_placeholder_allowance_gives_only_unused_count() {
    let root = PathBuf::from("/project");
    let baseline = identity_baseline(&[make_health_finding(&root, "<anonymous>", 10)], &root);
    let filtered = super::filter_new_health_findings(
        vec![
            make_health_finding(&root, "<anonymous>", 10),
            make_health_finding(&root, "newHotspot", 30),
        ],
        &baseline,
        &root,
        HealthBaselineMode::Identity,
    );
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].name, "newHotspot");
}

#[test]
fn health_identity_placeholder_allowance_stays_in_its_file() {
    let root = PathBuf::from("/project");
    let baseline = identity_baseline(&[make_health_finding(&root, "<anonymous>", 10)], &root);
    let filtered = super::filter_new_health_findings(
        vec![moved_finding(&root, "src/other.ts", "fetchIssue")],
        &baseline,
        &root,
        HealthBaselineMode::Identity,
    );
    assert_eq!(filtered.len(), 1);
}

#[test]
fn health_identity_placeholder_allowance_tops_up_a_known_name() {
    let root = PathBuf::from("/project");
    // The wrapper `load` was a finding, and the generator that it returns
    // was `<anonymous>`. Both now carry `load`.
    let baseline = identity_baseline(
        &[
            make_health_finding(&root, "<anonymous>", 10),
            make_health_finding(&root, "load", 30),
        ],
        &root,
    );
    let current = vec![
        make_health_finding(&root, "load", 10),
        make_health_finding(&root, "load", 30),
    ];
    let filtered = super::filter_new_health_findings(
        current.clone(),
        &baseline,
        &root,
        HealthBaselineMode::Identity,
    );
    assert!(filtered.is_empty(), "{filtered:?}");
    let overlap = baseline.overlap_entries(&current, &root, HealthBaselineMode::Identity);
    assert_eq!(overlap.matched_entries, 2);
}

#[test]
fn health_identity_placeholder_allowance_is_not_shared_twice() {
    let root = PathBuf::from("/project");
    let baseline = identity_baseline(&[make_health_finding(&root, "<anonymous>", 10)], &root);
    let filtered = super::filter_new_health_findings(
        vec![
            make_health_finding(&root, "alpha", 10),
            make_health_finding(&root, "beta", 30),
        ],
        &baseline,
        &root,
        HealthBaselineMode::Identity,
    );
    assert_eq!(filtered.len(), 1, "one placeholder covers one finding");
}

#[test]
fn health_count_baseline_does_not_follow_file_moves() {
    let root = PathBuf::from("/project");
    let baseline = HealthBaselineData::from_findings(
        &[make_health_finding(&root, "parseExpression", 42)],
        &[],
        &[],
        &root,
    );
    let filtered = filter_new_health_findings(
        vec![moved_finding(
            &root,
            "src/parser/utils.ts",
            "parseExpression",
        )],
        &baseline,
        &root,
    );
    assert_eq!(filtered.len(), 1);
}

#[test]
fn health_identity_data_is_absent_for_legacy_and_empty_baselines() {
    let legacy = HealthBaselineData {
        findings: vec!["src/utils.ts:parseExpression:42".to_string()],
        ..HealthBaselineData::default()
    };
    assert!(legacy.lacks_identity_data());
    assert!(!HealthBaselineData::default().lacks_identity_data());
}

#[test]
fn health_baseline_empty_keeps_all() {
    let root = PathBuf::from("/project");
    let findings = vec![make_health_finding(&root, "parseExpression", 42)];
    let baseline = HealthBaselineData {
        kind: None,
        findings: vec![],
        finding_counts: BTreeMap::new(),
        identity_finding_counts: BTreeMap::new(),
        target_keys: vec![],
        runtime_coverage_findings: vec![],
        runtime_coverage_source_hashes: vec![],
    };
    let filtered = filter_new_health_findings(findings, &baseline, &root);
    assert_eq!(filtered.len(), 1);
}

#[test]
fn circular_dep_key_is_order_independent() {
    use crate::results::CircularDependency;

    let dep_ab = CircularDependencyFinding::with_actions(CircularDependency {
        files: vec![PathBuf::from("src/a.ts"), PathBuf::from("src/b.ts")],
        length: 2,
        line: 1,
        col: 0,
        edges: Vec::new(),
        is_cross_package: false,
    });
    let dep_ba = CircularDependencyFinding::with_actions(CircularDependency {
        files: vec![PathBuf::from("src/b.ts"), PathBuf::from("src/a.ts")],
        length: 2,
        line: 1,
        col: 0,
        edges: Vec::new(),
        is_cross_package: false,
    });
    assert_eq!(
        super::circular_dep_key(&dep_ab.cycle, Path::new("")),
        super::circular_dep_key(&dep_ba.cycle, Path::new("")),
        "same files in different order should produce identical keys"
    );
}

#[test]
fn circular_dep_key_different_files_different_keys() {
    use crate::results::CircularDependency;

    let dep1 = CircularDependencyFinding::with_actions(CircularDependency {
        files: vec![PathBuf::from("src/a.ts"), PathBuf::from("src/b.ts")],
        length: 2,
        line: 1,
        col: 0,
        edges: Vec::new(),
        is_cross_package: false,
    });
    let dep2 = CircularDependencyFinding::with_actions(CircularDependency {
        files: vec![PathBuf::from("src/a.ts"), PathBuf::from("src/c.ts")],
        length: 2,
        line: 1,
        col: 0,
        edges: Vec::new(),
        is_cross_package: false,
    });
    assert_ne!(
        super::circular_dep_key(&dep1.cycle, Path::new("")),
        super::circular_dep_key(&dep2.cycle, Path::new("")),
    );
}

#[test]
fn circular_dep_key_three_files_order_independent() {
    use crate::results::CircularDependency;

    let dep_abc = CircularDependencyFinding::with_actions(CircularDependency {
        files: vec![
            PathBuf::from("src/a.ts"),
            PathBuf::from("src/b.ts"),
            PathBuf::from("src/c.ts"),
        ],
        length: 3,
        line: 1,
        col: 0,
        edges: Vec::new(),
        is_cross_package: false,
    });
    let dep_cab = CircularDependencyFinding::with_actions(CircularDependency {
        files: vec![
            PathBuf::from("src/c.ts"),
            PathBuf::from("src/a.ts"),
            PathBuf::from("src/b.ts"),
        ],
        length: 3,
        line: 1,
        col: 0,
        edges: Vec::new(),
        is_cross_package: false,
    });
    assert_eq!(
        super::circular_dep_key(&dep_abc.cycle, Path::new("")),
        super::circular_dep_key(&dep_cab.cycle, Path::new("")),
    );
}

#[expect(
    clippy::too_many_lines,
    reason = "test fixture; linear setup/assert, length is not a maintainability concern"
)]
fn make_full_results() -> AnalysisResults {
    use crate::results::*;
    use crate::source::MemberKind;

    let mut r = make_results();
    r.circular_dependencies
        .push(CircularDependencyFinding::with_actions(
            CircularDependency {
                files: vec![PathBuf::from("src/a.ts"), PathBuf::from("src/b.ts")],
                length: 2,
                line: 1,
                col: 0,
                edges: Vec::new(),
                is_cross_package: false,
            },
        ));
    r.unused_optional_dependencies
        .push(UnusedOptionalDependencyFinding::with_actions(
            UnusedDependency {
                package_name: "fsevents".to_string(),
                location: DependencyLocation::OptionalDependencies,
                path: PathBuf::from("package.json"),
                line: 15,
                used_in_workspaces: Vec::new(),
                declared_and_imported_in: Vec::new(),
            },
        ));
    r.unused_enum_members
        .push(UnusedEnumMemberFinding::with_actions(UnusedMember {
            path: PathBuf::from("src/enums.ts"),
            parent_name: "Status".to_string(),
            member_name: "Deprecated".to_string(),
            kind: MemberKind::EnumMember,
            line: 8,
            col: 0,
        }));
    r.unused_class_members
        .push(UnusedClassMemberFinding::with_actions(UnusedMember {
            path: PathBuf::from("src/service.ts"),
            parent_name: "UserService".to_string(),
            member_name: "legacy".to_string(),
            kind: MemberKind::ClassMethod,
            line: 42,
            col: 0,
        }));
    r.unused_store_members
        .push(UnusedStoreMemberFinding::with_actions(UnusedMember {
            path: PathBuf::from("src/store.ts"),
            parent_name: "useStore".to_string(),
            member_name: "legacyAction".to_string(),
            kind: MemberKind::StoreMember,
            line: 17,
            col: 0,
        }));
    r.unresolved_imports.push(
        fallow_types::output_dead_code::UnresolvedImportFinding::with_actions(
            crate::results::UnresolvedImport {
                path: PathBuf::from("src/app.ts"),
                specifier: "./missing".to_string(),
                line: 3,
                col: 0,
                specifier_col: 0,
            },
        ),
    );
    r.unlisted_dependencies
        .push(crate::results::UnlistedDependencyFinding::with_actions(
            UnlistedDependency {
                package_name: "chalk".to_string(),
                imported_from: vec![],
            },
        ));
    r.duplicate_exports
        .push(crate::results::DuplicateExportFinding::with_actions(
            crate::results::DuplicateExport {
                export_name: "Config".to_string(),
                locations: vec![
                    crate::results::DuplicateLocation {
                        path: PathBuf::from("src/a.ts"),
                        line: 1,
                        col: 0,
                    },
                    crate::results::DuplicateLocation {
                        path: PathBuf::from("src/b.ts"),
                        line: 5,
                        col: 0,
                    },
                ],
            },
        ));
    r.type_only_dependencies
        .push(crate::results::TypeOnlyDependencyFinding::with_actions(
            TypeOnlyDependency {
                package_name: "zod".to_string(),
                path: PathBuf::from("package.json"),
                line: 8,
            },
        ));
    r.test_only_dependencies
        .push(crate::results::TestOnlyDependencyFinding::with_actions(
            TestOnlyDependency {
                package_name: "vitest".to_string(),
                path: PathBuf::from("package.json"),
                line: 10,
            },
        ));
    r.boundary_violations.push(
        fallow_types::output_dead_code::BoundaryViolationFinding::with_actions(
            crate::results::BoundaryViolation {
                from_path: PathBuf::from("src/ui/btn.ts"),
                to_path: PathBuf::from("src/db/query.ts"),
                from_zone: "ui".to_string(),
                to_zone: "db".to_string(),
                import_specifier: "../db/query".to_string(),
                line: 1,
                col: 0,
                via_path: None,
            },
        ),
    );
    r
}

#[test]
fn baseline_from_results_captures_all_extended_fields() {
    let results = make_full_results();
    let baseline = BaselineData::from_results(&results, Path::new(""));
    assert_eq!(baseline.circular_dependencies.len(), 1);
    assert_eq!(
        baseline.unused_optional_dependencies,
        vec!["unused-optional-dependency:package.json:fsevents"]
    );
    assert_eq!(baseline.unused_enum_members.len(), 1);
    assert!(baseline.unused_enum_members[0].ends_with(":Status:Deprecated"));
    assert_eq!(baseline.unused_class_members.len(), 1);
    assert!(baseline.unused_class_members[0].ends_with(":UserService:legacy"));
    assert_eq!(baseline.unused_store_members.len(), 1);
    assert!(baseline.unused_store_members[0].ends_with(":useStore:legacyAction"));
    assert_eq!(baseline.unresolved_imports.len(), 1);
    assert!(baseline.unresolved_imports[0].ends_with(":./missing"));
    assert_eq!(
        baseline.unlisted_dependencies,
        vec!["unlisted-dependency:chalk"]
    );
    assert_eq!(baseline.duplicate_exports.len(), 1);
    assert!(baseline.duplicate_exports[0].starts_with("duplicate-export:Config:"));
    assert_eq!(
        baseline.type_only_dependencies,
        vec!["type-only-dependency:package.json:zod"]
    );
    assert_eq!(
        baseline.test_only_dependencies,
        vec!["test-only-dependency:package.json:vitest"]
    );
    assert_eq!(baseline.boundary_violations.len(), 1);
    assert!(baseline.boundary_violations[0].starts_with("boundary-violation:"));
}

#[test]
fn filter_removes_all_extended_baseline_issues() {
    let results = make_full_results();
    let baseline = BaselineData::from_results(&results, Path::new(""));
    let filtered = filter_new_issues(results, &baseline, Path::new(""));
    assert!(filtered.circular_dependencies.is_empty());
    assert!(filtered.unused_optional_dependencies.is_empty());
    assert!(filtered.unused_enum_members.is_empty());
    assert!(filtered.unused_class_members.is_empty());
    assert!(filtered.unused_store_members.is_empty());
    assert!(filtered.unresolved_imports.is_empty());
    assert!(filtered.unlisted_dependencies.is_empty());
    assert!(filtered.duplicate_exports.is_empty());
    assert!(filtered.type_only_dependencies.is_empty());
    assert!(filtered.test_only_dependencies.is_empty());
    assert!(filtered.boundary_violations.is_empty());
}

#[test]
fn filter_keeps_new_circular_deps() {
    use crate::results::CircularDependency;
    let baseline = BaselineData {
        circular_dependencies: vec!["src/a.ts->src/b.ts".to_string()],
        ..BaselineData::legacy_from_results(&AnalysisResults::default(), Path::new(""))
    };
    let mut results = AnalysisResults::default();
    results
        .circular_dependencies
        .push(CircularDependencyFinding::with_actions(
            CircularDependency {
                files: vec![PathBuf::from("src/a.ts"), PathBuf::from("src/b.ts")],
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
                files: vec![PathBuf::from("src/x.ts"), PathBuf::from("src/y.ts")],
                length: 2,
                line: 5,
                col: 0,
                edges: Vec::new(),
                is_cross_package: false,
            },
        ));
    let filtered = filter_new_issues(results, &baseline, Path::new(""));
    assert_eq!(filtered.circular_dependencies.len(), 1);
}

#[test]
fn package_cycles_round_trip_through_the_baseline() {
    use crate::results::{PackageCycle, PackageCycleEdge, PackageCycleFinding};
    let cycle = |packages: &[&str], example: &str| {
        PackageCycleFinding::with_actions(PackageCycle {
            packages: packages.iter().map(ToString::to_string).collect(),
            package_roots: packages
                .iter()
                .map(|name| PathBuf::from(format!("packages/{}", name.trim_start_matches("@x/"))))
                .collect(),
            length: packages.len(),
            edges: vec![PackageCycleEdge {
                from_package: packages[0].to_string(),
                to_package: packages[1].to_string(),
                path: PathBuf::from(example),
                target_path: PathBuf::from("packages/b/src/y.ts"),
                line: 1,
                col: 0,
                type_only: false,
            }],
            group_truncated: false,
        })
    };
    let mut saved = AnalysisResults::default();
    saved
        .package_cycles
        .push(cycle(&["@x/a", "@x/b"], "packages/a/src/x.ts"));
    let baseline = BaselineData::from_results(&saved, Path::new(""));
    assert_eq!(
        baseline.package_cycles,
        vec!["package-cycle:packages/a|packages/b"]
    );

    // A new example import for the same cycle is still a known cycle.
    let mut results = AnalysisResults::default();
    results
        .package_cycles
        .push(cycle(&["@x/a", "@x/b"], "packages/a/src/other.ts"));
    results
        .package_cycles
        .push(cycle(&["@x/a", "@x/c"], "packages/a/src/x.ts"));
    let filtered = filter_new_issues(results, &baseline, Path::new(""));
    assert_eq!(filtered.package_cycles.len(), 1);
    assert_eq!(filtered.package_cycles[0].cycle.packages, ["@x/a", "@x/c"]);

    // Packages that share a name carry their root in the label, so two
    // such cycles have two keys.
    let mut saved = AnalysisResults::default();
    saved.package_cycles.push(cycle(
        &["@x/a", "example (examples/one)"],
        "packages/a/src/x.ts",
    ));
    let baseline = BaselineData::from_results(&saved, Path::new(""));
    let mut results = AnalysisResults::default();
    results.package_cycles.push(cycle(
        &["@x/a", "example (examples/two)"],
        "packages/a/src/x.ts",
    ));
    let filtered = filter_new_issues(results, &baseline, Path::new(""));
    assert_eq!(filtered.package_cycles.len(), 1);

    // A legacy baseline keeps its package-name key form and still matches.
    let mut saved = AnalysisResults::default();
    saved
        .package_cycles
        .push(cycle(&["@x/a", "@x/b"], "packages/a/src/x.ts"));
    let legacy = BaselineData::legacy_from_results(&saved, Path::new(""));
    assert_eq!(legacy.package_cycles, vec!["@x/a->@x/b"]);
    let filtered = filter_new_issues(saved, &legacy, Path::new(""));
    assert!(filtered.package_cycles.is_empty());
}

#[test]
fn filter_keeps_new_boundary_violations() {
    use crate::results::BoundaryViolation;
    let baseline = BaselineData {
        boundary_violations: vec!["src/a.ts->src/b.ts".to_string()],
        boundary_coverage_violations: vec![],
        boundary_call_violations: vec![],
        policy_violations: vec![],
        ..BaselineData::legacy_from_results(&AnalysisResults::default(), Path::new(""))
    };
    let mut results = AnalysisResults::default();
    results
        .boundary_violations
        .push(BoundaryViolationFinding::with_actions(BoundaryViolation {
            from_path: PathBuf::from("src/a.ts"),
            to_path: PathBuf::from("src/b.ts"),
            from_zone: "a".to_string(),
            to_zone: "b".to_string(),
            import_specifier: "../b".to_string(),
            line: 1,
            col: 0,
            via_path: None,
        }));
    results
        .boundary_violations
        .push(BoundaryViolationFinding::with_actions(BoundaryViolation {
            from_path: PathBuf::from("src/new.ts"),
            to_path: PathBuf::from("src/secret.ts"),
            from_zone: "new".to_string(),
            to_zone: "secret".to_string(),
            import_specifier: "../secret".to_string(),
            line: 1,
            col: 0,
            via_path: None,
        }));
    let filtered = filter_new_issues(results, &baseline, Path::new(""));
    assert_eq!(filtered.boundary_violations.len(), 1);
}

#[test]
fn health_targets_baseline_filters_known() {
    let root = PathBuf::from("/project");
    let targets = vec![
        fallow_output::RefactoringTarget {
            path: root.join("src/complex.ts"),
            priority: 80.0,
            efficiency: 40.0,
            recommendation: "Split file".to_string(),
            category: fallow_output::RecommendationCategory::SplitHighImpact,
            effort: fallow_output::EffortEstimate::Medium,
            confidence: fallow_output::Confidence::Medium,
            factors: vec![],
            evidence: None,
        },
        fallow_output::RefactoringTarget {
            path: root.join("src/new-issue.ts"),
            priority: 60.0,
            efficiency: 30.0,
            recommendation: "Extract function".to_string(),
            category: fallow_output::RecommendationCategory::ExtractComplexFunctions,
            effort: fallow_output::EffortEstimate::Low,
            confidence: fallow_output::Confidence::High,
            factors: vec![],
            evidence: None,
        },
    ];
    let baseline = HealthBaselineData::from_findings(&[], &[], &targets[..1], &root);
    let filtered = filter_new_health_targets(targets, &baseline, &root);
    assert_eq!(filtered.len(), 1);
    assert_eq!(filtered[0].path, root.join("src/new-issue.ts"));
}

#[test]
fn duplicate_export_key_is_sorted() {
    use crate::results::{DuplicateExport, DuplicateLocation};
    let dup_ab = DuplicateExport {
        export_name: "foo".to_string(),
        locations: vec![
            DuplicateLocation {
                path: PathBuf::from("src/a.ts"),
                line: 1,
                col: 0,
            },
            DuplicateLocation {
                path: PathBuf::from("src/b.ts"),
                line: 5,
                col: 0,
            },
        ],
    };
    let dup_ba = DuplicateExport {
        export_name: "foo".to_string(),
        locations: vec![
            DuplicateLocation {
                path: PathBuf::from("src/b.ts"),
                line: 5,
                col: 0,
            },
            DuplicateLocation {
                path: PathBuf::from("src/a.ts"),
                line: 1,
                col: 0,
            },
        ],
    };
    assert_eq!(
        super::duplicate_export_key(&dup_ab, Path::new("")),
        super::duplicate_export_key(&dup_ba, Path::new("")),
    );
}

#[test]
fn boundary_violation_key_format() {
    use crate::results::BoundaryViolation;
    let v = BoundaryViolation {
        from_path: PathBuf::from("src/ui/btn.ts"),
        to_path: PathBuf::from("src/db/query.ts"),
        from_zone: "ui".to_string(),
        to_zone: "db".to_string(),
        import_specifier: "../db/query".to_string(),
        line: 1,
        col: 0,
        via_path: None,
    };
    let key = super::boundary_violation_key(&v, Path::new(""));
    assert_eq!(key, "src/ui/btn.ts->src/db/query.ts");
}

/// Build results with absolute paths rooted at the given prefix.
fn make_absolute_results(root: &str) -> AnalysisResults {
    use crate::results::*;
    use crate::source::MemberKind;

    let p = |rel: &str| PathBuf::from(format!("{root}/{rel}"));

    AnalysisResults {
        unused_files: vec![UnusedFileFinding::with_actions(UnusedFile {
            path: p("src/old.ts"),
        })],
        unused_exports: vec![UnusedExportFinding::with_actions(UnusedExport {
            path: p("src/utils.ts"),
            export_name: "helper".to_string(),
            is_type_only: false,
            line: 5,
            col: 0,
            span_start: 40,
            is_re_export: false,
            deprecated: false,
            deprecated_reason: None,
        })],
        unused_dependencies: vec![UnusedDependencyFinding::with_actions(UnusedDependency {
            package_name: "lodash-es".to_string(),
            location: DependencyLocation::Dependencies,
            path: p("packages/app/package.json"),
            line: 5,
            used_in_workspaces: Vec::new(),
            declared_and_imported_in: Vec::new(),
        })],
        circular_dependencies: vec![CircularDependencyFinding::with_actions(
            CircularDependency {
                files: vec![p("src/a.ts"), p("src/b.ts")],
                length: 2,
                line: 1,
                col: 0,
                edges: Vec::new(),
                is_cross_package: false,
            },
        )],
        unused_enum_members: vec![UnusedEnumMemberFinding::with_actions(UnusedMember {
            path: p("src/enums.ts"),
            parent_name: "Status".to_string(),
            member_name: "Deprecated".to_string(),
            kind: MemberKind::EnumMember,
            line: 8,
            col: 0,
        })],
        unused_class_members: vec![UnusedClassMemberFinding::with_actions(UnusedMember {
            path: p("src/service.ts"),
            parent_name: "UserService".to_string(),
            member_name: "legacy".to_string(),
            kind: MemberKind::ClassMethod,
            line: 42,
            col: 0,
        })],
        unused_store_members: vec![UnusedStoreMemberFinding::with_actions(UnusedMember {
            path: p("src/store.ts"),
            parent_name: "useStore".to_string(),
            member_name: "legacyAction".to_string(),
            kind: MemberKind::StoreMember,
            line: 17,
            col: 0,
        })],
        unresolved_imports: vec![UnresolvedImportFinding::with_actions(UnresolvedImport {
            path: p("src/app.ts"),
            specifier: "./missing".to_string(),
            line: 3,
            col: 0,
            specifier_col: 0,
        })],
        duplicate_exports: vec![DuplicateExportFinding::with_actions(DuplicateExport {
            export_name: "Config".to_string(),
            locations: vec![
                DuplicateLocation {
                    path: p("src/a.ts"),
                    line: 1,
                    col: 0,
                },
                DuplicateLocation {
                    path: p("src/b.ts"),
                    line: 5,
                    col: 0,
                },
            ],
        })],
        boundary_violations: vec![BoundaryViolationFinding::with_actions(BoundaryViolation {
            from_path: p("src/ui/btn.ts"),
            to_path: p("src/db/query.ts"),
            from_zone: "ui".to_string(),
            to_zone: "db".to_string(),
            import_specifier: "../db/query".to_string(),
            line: 1,
            col: 0,
            via_path: None,
        })],
        ..Default::default()
    }
}

/// Regression test: baseline saved on one machine (different absolute root)
/// must match issues found on another machine across all path-based types.
#[test]
fn baseline_keys_are_relative_to_root() {
    let local_root = Path::new("/Users/dev/project");
    let results = make_absolute_results("/Users/dev/project");
    let baseline = BaselineData::from_results(&results, local_root);

    assert_eq!(baseline.unused_files, vec!["unused-file:src/old.ts"]);
    assert_eq!(
        baseline.unused_exports,
        vec!["unused-export:src/utils.ts:helper"]
    );
    assert_eq!(
        baseline.unused_dependencies,
        vec!["unused-dependency:packages/app/package.json:lodash-es"]
    );
    assert_eq!(
        baseline.boundary_violations,
        vec!["boundary-violation:src/ui/btn.ts:src/db/query.ts"]
    );
    assert_eq!(
        baseline.circular_dependencies,
        vec!["circular-dependency:src/a.ts|src/b.ts"]
    );
    assert_eq!(
        baseline.unused_enum_members,
        vec!["unused-enum-member:src/enums.ts:Status:Deprecated"]
    );
    assert_eq!(
        baseline.unused_class_members,
        vec!["unused-class-member:src/service.ts:UserService:legacy"]
    );
    assert_eq!(
        baseline.unused_store_members,
        vec!["unused-store-member:src/store.ts:useStore:legacyAction"]
    );
    assert_eq!(
        baseline.unresolved_imports,
        vec!["unresolved-import:src/app.ts:./missing"]
    );
    assert_eq!(
        baseline.duplicate_exports,
        vec!["duplicate-export:Config:src/a.ts|src/b.ts"]
    );

    let ci_root = Path::new("/home/runner/work/project/project");
    let ci_results = make_absolute_results("/home/runner/work/project/project");

    let filtered = filter_new_issues(ci_results, &baseline, ci_root);
    assert!(filtered.unused_files.is_empty(), "unused files");
    assert!(filtered.unused_exports.is_empty(), "unused exports");
    assert!(filtered.unused_dependencies.is_empty(), "unused deps");
    assert!(
        filtered.boundary_violations.is_empty(),
        "boundary violations"
    );
    assert!(filtered.circular_dependencies.is_empty(), "circular deps");
    assert!(filtered.unused_enum_members.is_empty(), "enum members");
    assert!(filtered.unused_class_members.is_empty(), "class members");
    assert!(filtered.unused_store_members.is_empty(), "store members");
    assert!(filtered.unresolved_imports.is_empty(), "unresolved imports");
    assert!(filtered.duplicate_exports.is_empty(), "duplicate exports");
}

#[test]
fn stale_suppression_baseline_keys_include_missing_reason_state() {
    let root = Path::new("/project");
    let stale = crate::results::StaleSuppression {
        finding_id: None,
        path: root.join("src/file.ts"),
        line: 1,
        col: 0,
        origin: crate::results::SuppressionOrigin::Comment {
            issue_kind: Some("unused-export".to_string()),
            reason: None,
            is_file_level: false,
            kind_known: true,
        },
        missing_reason: false,
        actions: crate::results::StaleSuppression::actions_for(false),
        effective_severity: None,
    };
    let missing = crate::results::StaleSuppression {
        missing_reason: true,
        actions: crate::results::StaleSuppression::actions_for(true),
        ..stale.clone()
    };
    let results = AnalysisResults {
        stale_suppressions: vec![stale, missing],
        ..Default::default()
    };
    let baseline = BaselineData::from_results(&results, root);

    assert_eq!(
        baseline.stale_suppressions,
        vec![
            "missing-suppression-reason:src/file.ts:comment:unused-export:line",
            "stale-suppression:src/file.ts:comment:unused-export:line",
        ]
    );

    let mut legacy_baseline = BaselineData::legacy_from_results(&AnalysisResults::default(), root);
    legacy_baseline.stale_suppressions = vec!["src/file.ts:1".to_string()];
    let filtered = filter_new_issues(results, &legacy_baseline, root);
    assert!(filtered.stale_suppressions.is_empty());
}

fn runtime_finding(
    id: &str,
    stable_id: Option<&str>,
    line: u32,
    source_hash: Option<&str>,
) -> fallow_output::RuntimeCoverageFinding {
    fallow_output::RuntimeCoverageFinding {
        id: id.to_owned(),
        stable_id: stable_id.map(str::to_owned),
        source_hash: source_hash.map(str::to_owned),
        path: PathBuf::from("src/a.ts"),
        function: "alpha".to_owned(),
        line,
        verdict: fallow_output::RuntimeCoverageVerdict::ReviewRequired,
        invocations: Some(0),
        confidence: fallow_output::RuntimeCoverageConfidence::Medium,
        evidence: fallow_output::RuntimeCoverageEvidence {
            static_status: "used".to_owned(),
            test_coverage: "not_covered".to_owned(),
            test_only_reference: None,
            v8_tracking: "tracked".to_owned(),
            untracked_reason: None,
            observation_days: 1,
            deployments_observed: 1,
        },
        actions: vec![],
        discriminators: None,
    }
}

#[test]
fn legacy_prod_baseline_still_suppresses_finding() {
    let baseline = HealthBaselineData {
        runtime_coverage_findings: vec!["fallow:prod:deadbeef".to_owned()],
        ..HealthBaselineData::default()
    };
    let findings = vec![runtime_finding(
        "fallow:prod:deadbeef",
        Some("fallow:fn:00000001"),
        14,
        None,
    )];
    let filtered = filter_new_runtime_coverage_findings(findings, &baseline, Path::new("/repo"));
    assert!(filtered.is_empty(), "legacy prod id must still suppress");
}

#[test]
fn source_hash_baseline_survives_line_move() {
    let root = Path::new("/repo");
    let baselined = runtime_finding(
        "fallow:prod:deadbeef",
        Some("fallow:fn:00000001"),
        14,
        Some("0123456789abcdef"),
    );
    let baseline = HealthBaselineData::from_findings(&[], &[baselined], &[], root);
    assert_eq!(baseline.runtime_coverage_source_hashes.len(), 1);

    let findings = vec![runtime_finding(
        "fallow:prod:99999999",
        Some("fallow:fn:cafe0002"),
        40,
        Some("0123456789abcdef"),
    )];
    let filtered = filter_new_runtime_coverage_findings(findings, &baseline, root);
    assert!(
        filtered.is_empty(),
        "source_hash baseline must survive a line move despite a changed stable_id and id"
    );
}

#[test]
fn unbaselined_finding_is_reported() {
    let baseline = HealthBaselineData {
        runtime_coverage_findings: vec!["fallow:fn:00000001".to_owned()],
        ..HealthBaselineData::default()
    };
    let findings = vec![runtime_finding(
        "fallow:prod:abc1234d",
        Some("fallow:fn:beefcafe"),
        7,
        None,
    )];
    let filtered = filter_new_runtime_coverage_findings(findings, &baseline, Path::new("/repo"));
    assert_eq!(filtered.len(), 1, "a brand-new finding must be reported");
}

// Line-free, count-matched dead-code baselines.

fn line_free_member(
    root: &Path,
    member: &str,
    line: u32,
) -> crate::results::UnusedClassMemberFinding {
    crate::results::UnusedClassMemberFinding::with_actions(crate::results::UnusedMember {
        path: root.join("src/service.ts"),
        parent_name: "Service".to_owned(),
        member_name: member.to_owned(),
        kind: fallow_types::extract::MemberKind::ClassMethod,
        line,
        col: 2,
    })
}

fn line_free_suppression(
    root: &Path,
    line: u32,
    reason: Option<&str>,
) -> crate::results::StaleSuppression {
    crate::results::StaleSuppression {
        path: root.join("src/flags.ts"),
        line,
        col: 0,
        origin: crate::results::SuppressionOrigin::Comment {
            issue_kind: Some("unused-export".to_owned()),
            reason: reason.map(str::to_owned),
            is_file_level: false,
            kind_known: true,
        },
        missing_reason: false,
        finding_id: None,
        actions: Vec::new(),
        effective_severity: None,
    }
}

fn line_free_directive(root: &Path, line: u32) -> crate::results::MisplacedDirectiveFinding {
    crate::results::MisplacedDirectiveFinding::with_actions(crate::results::MisplacedDirective {
        path: root.join("src/action.ts"),
        directive: "use server".to_owned(),
        line,
        col: 0,
    })
}

fn line_free_results(root: &Path, shift: u32, reason: Option<&str>) -> AnalysisResults {
    AnalysisResults {
        unused_exports: vec![UnusedExportFinding::with_actions(UnusedExport {
            path: root.join("src/utils.ts"),
            export_name: "helper".to_owned(),
            is_type_only: false,
            line: 10 + shift,
            col: 0,
            span_start: 100 + shift * 10,
            is_re_export: false,
            deprecated: false,
            deprecated_reason: None,
        })],
        stale_suppressions: vec![line_free_suppression(root, 3 + shift, reason)],
        misplaced_directives: vec![line_free_directive(root, 5 + shift)],
        ..AnalysisResults::default()
    }
}

fn reload(baseline: &BaselineData) -> BaselineData {
    serde_json::from_str(&serde_json::to_string(baseline).expect("serialize baseline"))
        .expect("parse baseline")
}

#[test]
fn saved_baseline_hides_findings_after_a_line_shift() {
    let root = PathBuf::from("/project");
    let baseline = reload(&BaselineData::from_results(
        &line_free_results(&root, 0, None),
        &root,
    ));

    let filtered = filter_new_issues(line_free_results(&root, 20, None), &baseline, &root);

    assert_eq!(
        filtered.total_issues(),
        0,
        "a line shift must not make findings new"
    );
}

#[test]
fn saved_baseline_ignores_the_suppression_reason_text() {
    let root = PathBuf::from("/project");
    let baseline = reload(&BaselineData::from_results(
        &line_free_results(&root, 0, Some("old reason")),
        &root,
    ));

    let filtered = filter_new_issues(
        line_free_results(&root, 7, Some("a new reason")),
        &baseline,
        &root,
    );

    assert!(filtered.stale_suppressions.is_empty());
}

#[test]
fn saved_baseline_hides_at_most_the_saved_number_of_occurrences() {
    let root = PathBuf::from("/project");
    let saved = AnalysisResults {
        unused_class_members: vec![line_free_member(&root, "value", 4)],
        ..AnalysisResults::default()
    };
    let baseline = reload(&BaselineData::from_results(&saved, &root));
    let current = AnalysisResults {
        unused_class_members: vec![
            line_free_member(&root, "value", 14),
            line_free_member(&root, "value", 18),
        ],
        ..AnalysisResults::default()
    };

    let filtered = filter_new_issues(current, &baseline, &root);

    assert_eq!(
        filtered.unused_class_members.len(),
        1,
        "one saved occurrence hides one current finding, not two"
    );
}

#[test]
fn saved_baseline_declares_the_dc1_identity_and_canonical_keys() {
    let root = PathBuf::from("/project");
    let mut results = line_free_results(&root, 0, None);
    results.unused_class_members = vec![
        line_free_member(&root, "value", 4),
        line_free_member(&root, "value", 8),
    ];

    let json: serde_json::Value =
        serde_json::to_value(BaselineData::from_results(&results, &root)).unwrap();

    assert_eq!(json["identity"], "dc1");
    assert_eq!(
        json["unused_exports"],
        serde_json::json!(["unused-export:src/utils.ts:helper"])
    );
    assert_eq!(
        json["unused_class_members"],
        serde_json::json!([
            "unused-class-member:src/service.ts:Service:value",
            "unused-class-member:src/service.ts:Service:value"
        ])
    );
    assert_eq!(
        json["stale_suppressions"],
        serde_json::json!(["stale-suppression:src/flags.ts:comment:unused-export:line"])
    );
    assert_eq!(
        json["misplaced_directives"],
        serde_json::json!(["misplaced-directive:src/action.ts:use server"])
    );
}

#[test]
fn legacy_baseline_still_matches_its_exact_keys() {
    let root = PathBuf::from("/project");
    let legacy: BaselineData = serde_json::from_value(serde_json::json!({
        "unused_files": [],
        "unused_exports": ["src/utils.ts:helper"],
        "unused_types": [],
        "unused_dependencies": [],
        "unused_dev_dependencies": [],
        "stale_suppressions": ["stale-suppression:src/flags.ts:3"],
        "misplaced_directives": ["src/action.ts:5:use server"]
    }))
    .unwrap();

    let unshifted = filter_new_issues(line_free_results(&root, 0, None), &legacy, &root);
    let shifted = filter_new_issues(line_free_results(&root, 1, None), &legacy, &root);

    assert_eq!(
        unshifted.total_issues(),
        0,
        "a legacy key matches its old form"
    );
    assert!(shifted.unused_exports.is_empty());
    assert_eq!(
        shifted.stale_suppressions.len() + shifted.misplaced_directives.len(),
        2,
        "a legacy key that holds a line matches only that line"
    );
}

#[test]
fn a_baseline_with_an_unknown_identity_is_refused() {
    let root = PathBuf::from("/project");
    let mut results = line_free_results(&root, 0, None);
    let content = serde_json::json!({
        "identity": "dc9",
        "unused_files": [],
        "unused_exports": [],
        "unused_types": [],
        "unused_dependencies": [],
        "unused_dev_dependencies": []
    })
    .to_string();

    let outcome = apply_dead_code_baseline(
        &mut results,
        &content,
        &root,
        &fallow_types::semantic::SemanticAnalysisIdentity::syntactic(),
        false,
    );

    assert!(
        matches!(outcome, Err(DeadCodeBaselineError::Parse(ref message)) if message.contains("dc9")),
        "{outcome:?}"
    );
}

#[test]
fn saved_scope_reasons_reads_only_a_string_array() {
    assert_eq!(
        saved_scope_reasons(r#"{"scope_reasons":["package-baselines","production"]}"#),
        ["package-baselines", "production"]
    );
    assert!(saved_scope_reasons(r#"{"unused_files":[]}"#).is_empty());
    assert!(saved_scope_reasons(r#"{"scope_reasons":"package-baselines"}"#).is_empty());
    assert_eq!(
        saved_scope_reasons(r#"{"scope_reasons":["diff",7,null]}"#),
        ["diff"]
    );
    assert!(saved_scope_reasons("not json").is_empty());
}

#[test]
fn a_whole_project_save_records_no_scope() {
    let root = Path::new("/project");
    let saved = serde_json::to_value(BaselineData::from_results(
        &AnalysisResults::default(),
        root,
    ))
    .expect("baseline JSON");
    assert!(saved.get("scope_reasons").is_none(), "{saved}");
    let scoped = serde_json::to_value(
        BaselineData::from_results(&AnalysisResults::default(), root).with_scope_reasons(
            fallow_output::BaselineScopeReasons::empty()
                .with(fallow_output::ScopeReason::PackageBaselines),
        ),
    )
    .expect("baseline JSON");
    assert_eq!(
        scoped["scope_reasons"],
        serde_json::json!(["package-baselines"])
    );
}
