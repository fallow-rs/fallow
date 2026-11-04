use super::*;

use fallow_types::duplicates::{CloneFamily, CloneGroup, CloneInstance, DuplicationStats};

use super::editor_results::{
    BoundaryViolation, BoundaryViolationFinding, CircularDependency, CircularDependencyFinding,
    DevDependencyInProduction, DevDependencyInProductionFinding, ExportUsage, SecuritySeverity,
    TestOnlyDependency, TestOnlyDependencyFinding, TypeOnlyDependency, UnlistedDependency,
    UnlistedDependencyFinding, UnusedClassMemberFinding, UnusedDependency, UnusedDependencyFinding,
    UnusedDevDependencyFinding, UnusedEnumMemberFinding, UnusedExport, UnusedExportFinding,
    UnusedFile, UnusedFileFinding, UnusedMember, UnusedOptionalDependencyFinding,
    UnusedStoreMemberFinding, UnusedTypeFinding,
};

#[test]
fn merges_duplication_stats_and_recomputes_percentage() {
    let mut output = EditorAnalysisOutput {
        duplication: EditorDuplicationReport {
            clone_groups: vec![CloneGroup {
                instances: vec![CloneInstance {
                    is_symlink: false,
                    file: PathBuf::from("src/a.ts"),
                    start_line: 1,
                    end_line: 4,
                    start_col: 0,
                    end_col: 10,
                    fragment: "const a = 1;".to_string(),
                }],
                token_count: 8,
                line_count: 4,
                similarity: None,
            }],
            clone_families: Vec::new(),
            mirrored_directories: Vec::new(),
            stats: DuplicationStats {
                clone_groups: 1,
                clone_families: 0,
                clone_instances: 1,
                total_files: 1,
                files_with_clones: 1,
                total_lines: 20,
                duplicated_lines: 4,
                total_tokens: 80,
                duplicated_tokens: 8,
                duplication_percentage: 20.0,
                clone_groups_below_min_occurrences: 1,
                clone_groups_ignored: 1,
                near_candidates_skipped: 2,
            },
        },
        ..Default::default()
    };

    output.merge_duplication(EditorDuplicationReport {
        clone_groups: Vec::new(),
        clone_families: Vec::new(),
        mirrored_directories: Vec::new(),
        stats: DuplicationStats {
            clone_groups: 0,
            clone_families: 0,
            clone_instances: 0,
            total_files: 1,
            files_with_clones: 0,
            total_lines: 30,
            duplicated_lines: 6,
            total_tokens: 120,
            duplicated_tokens: 12,
            duplication_percentage: 20.0,
            clone_groups_below_min_occurrences: 2,
            clone_groups_ignored: 3,
            near_candidates_skipped: 4,
        },
    });

    assert_eq!(output.duplication.stats.total_lines, 50);
    assert_eq!(output.duplication.stats.duplicated_lines, 10);
    assert_eq!(
        output.duplication.stats.clone_groups_below_min_occurrences,
        3
    );
    assert_eq!(output.duplication.stats.clone_groups_ignored, 4);
    assert_eq!(output.duplication.stats.near_candidates_skipped, 6);
    assert!((output.duplication.stats.duplication_percentage - 20.0).abs() < f64::EPSILON);
}

#[test]
fn merging_duplication_keeps_the_family_corpus_count_aligned() {
    let family = |path: &str| CloneFamily {
        files: vec![PathBuf::from(path)],
        groups: Vec::new(),
        total_duplicated_lines: 4,
        total_duplicated_tokens: 8,
        suggestions: Vec::new(),
    };
    let report = |path: &str, families: usize| EditorDuplicationReport {
        clone_groups: Vec::new(),
        clone_families: vec![family(path)],
        mirrored_directories: Vec::new(),
        stats: DuplicationStats {
            clone_families: families,
            ..DuplicationStats::default()
        },
    };

    let mut output = EditorAnalysisOutput {
        duplication: report("src/a.ts", 3),
        ..Default::default()
    };
    output.merge_duplication(report("src/b.ts", 2));

    assert_eq!(output.duplication.stats.clone_families, 5);
    assert_eq!(output.duplication.clone_families_shown(), 2);
    assert_eq!(output.duplication.clone_families_omitted(), 3);
    assert_eq!(
        output.duplication.clone_families_total(),
        output.duplication.stats.clone_families
    );
}

#[test]
fn editor_session_returns_api_owned_project_output() {
    let temp = tempfile::tempdir().expect("temp project");
    let root = temp.path();
    std::fs::create_dir_all(root.join("src")).expect("src dir");
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"editor-api-session","main":"src/index.ts"}"#,
    )
    .expect("package.json");
    std::fs::write(
        root.join("src/index.ts"),
        "export const used = 1;\nconsole.log(used);\n",
    )
    .expect("source");

    let session = EditorAnalysisSession::load(root, None).expect("session loads");
    let output = session
        .analyze_project_with(&fallow_config::DuplicatesConfig::default(), true)
        .expect("analysis runs");

    assert!(output.dead_code.modules.is_some());
    assert!(
        output
            .dead_code
            .files
            .as_ref()
            .is_some_and(|files| !files.is_empty())
    );
}

/// Analysis retains the full project until the caller applies change scope.
#[test]
fn changed_files_scope_runs_after_the_analysis_keeps_all_unused_files() {
    let temp = tempfile::tempdir().expect("temp project");
    let root = temp.path().canonicalize().expect("canonical root");
    let src = root.join("src");
    std::fs::create_dir_all(&src).expect("src dir");
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"editor-scope-order","main":"src/a.ts"}"#,
    )
    .expect("package.json");
    std::fs::write(src.join("a.ts"), "export const a = 1;\n").expect("source a");
    std::fs::write(src.join("orphan.ts"), "export const orphan = 1;\n").expect("orphan");

    let session = EditorAnalysisSession::load(&root, None).expect("session loads");
    let mut changed_files = FxHashSet::default();
    changed_files.insert(src.join("a.ts"));
    let mut output = session
        .analyze_project_with_changed_files(
            &fallow_config::DuplicatesConfig::default(),
            false,
            Some(&changed_files),
        )
        .expect("analysis runs");
    let unused_files = |output: &EditorProjectAnalysisOutput| {
        output
            .dead_code
            .results
            .unused_files
            .iter()
            .map(|finding| finding.file.path.clone())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        unused_files(&output),
        vec![src.join("orphan.ts")],
        "the analysis keeps the unused file outside the changed set"
    );

    session.apply_change_scope(&mut output, &ChangeScope::changed_files(&changed_files));
    assert!(
        unused_files(&output).is_empty(),
        "the scope removes the unused file outside the changed set: {:?}",
        unused_files(&output)
    );
}

#[test]
fn editor_session_scopes_duplication_to_changed_files() {
    let temp = tempfile::tempdir().expect("temp project");
    let root = temp.path();
    let src = root.join("src");
    std::fs::create_dir_all(&src).expect("src dir");
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"editor-api-session","main":"src/a.ts"}"#,
    )
    .expect("package.json");
    let repeated =
        "export function repeated() {\n  return ['alpha', 'beta', 'gamma'].join(',');\n}\n";
    std::fs::write(src.join("a.ts"), repeated).expect("source a");
    std::fs::write(src.join("b.ts"), repeated).expect("source b");

    let session = EditorAnalysisSession::load(root, None).expect("session loads");
    let mut config = session.config().duplicates.clone();
    config.min_tokens = 1;
    config.min_lines = 1;
    let full = session
        .analyze_project_with(&config, false)
        .expect("analysis runs");
    assert!(!full.duplication.clone_groups.is_empty());

    let mut changed_files = FxHashSet::default();
    changed_files.insert(src.join("unrelated.ts"));
    let scoped = session
        .analyze_project_with_changed_files(&config, false, Some(&changed_files))
        .expect("analysis runs");
    assert!(scoped.duplication.clone_groups.is_empty());
}

/// A function under a `health.thresholdOverrides` entry that raises the
/// ceilings is not a health finding, so it gets no code lens either.
#[test]
fn inline_complexity_applies_health_threshold_overrides() {
    let temp = tempfile::tempdir().expect("temp project");
    let root = temp.path();
    std::fs::create_dir_all(root.join("src")).expect("src dir");
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"editor-inline-overrides","main":"src/index.ts"}"#,
    )
    .expect("package.json");
    std::fs::write(
            root.join(".fallowrc.json"),
            r#"{"health":{"maxCyclomatic":2,"maxCognitive":2,"thresholdOverrides":[{"files":["src/legacy.ts"],"maxCyclomatic":50,"maxCognitive":50}]}}"#,
        )
        .expect("config");
    let branchy = |name: &str| {
        format!(
            "export function {name}(value: number): number {{\n  if (value > 1) {{ return 1; }}\n  \
                 if (value > 2) {{ return 2; }}\n  if (value > 3) {{ return 3; }}\n  return 0;\n}}\n"
        )
    };
    std::fs::write(root.join("src/app.ts"), branchy("appBranchy")).expect("app");
    std::fs::write(root.join("src/legacy.ts"), branchy("legacyBranchy")).expect("legacy");
    std::fs::write(
        root.join("src/index.ts"),
        "export { appBranchy } from \"./app\";\nexport { legacyBranchy } from \"./legacy\";\n",
    )
    .expect("index");

    let session = EditorAnalysisSession::load(root, None).expect("session loads");
    let output = session
        .analyze_project_with(&session.config().duplicates.clone(), true)
        .expect("analysis runs");
    let names = collect_inline_complexity(session.config(), &output.dead_code)
        .into_iter()
        .map(|finding| finding.name)
        .collect::<Vec<_>>();

    assert_eq!(
        names,
        vec!["appBranchy".to_string()],
        "the override raises the ceilings for src/legacy.ts, as in `fallow health`"
    );
}

fn make_inline_finding(path: PathBuf) -> EditorInlineComplexityFinding {
    EditorInlineComplexityFinding {
        path,
        name: "myFn".to_string(),
        line: 1,
        col: 0,
        cyclomatic: 5,
        cognitive: 4,
        exceeded: EditorInlineComplexityExceeded::Cyclomatic,
    }
}

#[test]
fn filter_inline_complexity_keeps_findings_in_changed_set() {
    let changed: FxHashSet<PathBuf> = [PathBuf::from("/src/a.ts"), PathBuf::from("/src/b.ts")]
        .into_iter()
        .collect();
    let mut findings = vec![
        make_inline_finding(PathBuf::from("/src/a.ts")),
        make_inline_finding(PathBuf::from("/src/c.ts")),
    ];

    filter_inline_complexity_by_changed_files(&mut findings, &changed);

    assert_eq!(findings.len(), 1);
    assert_eq!(
        findings[0].path.to_string_lossy().replace('\\', "/"),
        "/src/a.ts"
    );
}

#[test]
fn filter_inline_complexity_removes_all_when_changed_set_empty() {
    let changed: FxHashSet<PathBuf> = FxHashSet::default();
    let mut findings = vec![make_inline_finding(PathBuf::from("/src/a.ts"))];

    filter_inline_complexity_by_changed_files(&mut findings, &changed);

    assert!(
        findings.is_empty(),
        "empty changed-files set must drop all inline complexity findings"
    );
}

#[test]
fn filter_inline_complexity_keeps_all_when_all_in_changed_set() {
    let path_a = PathBuf::from("/src/a.ts");
    let path_b = PathBuf::from("/src/b.ts");
    let changed: FxHashSet<PathBuf> = [path_a.clone(), path_b.clone()].into_iter().collect();
    let mut findings = vec![make_inline_finding(path_a), make_inline_finding(path_b)];

    filter_inline_complexity_by_changed_files(&mut findings, &changed);

    assert_eq!(
        findings.len(),
        2,
        "all findings in the changed set must be retained"
    );
}

#[test]
fn editor_session_applies_per_path_rule_overrides() {
    // The editor analysis path must resolve `overrides[].rules` the same
    // way the CLI does, so inline diagnostics and `fallow dead-code` agree
    // on which findings a project has turned off (issue #2621).
    let temp = tempfile::tempdir().expect("temp project");
    let root = temp.path();
    std::fs::create_dir_all(root.join("src/ui")).expect("ui dir");
    std::fs::create_dir_all(root.join("src/lib")).expect("lib dir");
    std::fs::write(
        root.join("package.json"),
        r#"{"name":"editor-override-rules","private":true,"main":"src/index.ts"}"#,
    )
    .expect("package.json");
    std::fs::write(
        root.join(".fallowrc.json"),
        r#"{
  "rules": { "unused-exports": "warn", "private-type-leaks": "warn" },
  "overrides": [
    {
      "files": ["src/ui/**"],
      "rules": { "unused-exports": "off", "private-type-leaks": "off" }
    }
  ]
}"#,
    )
    .expect("config");
    std::fs::write(
            root.join("src/index.ts"),
            "import { kitUsed } from './ui/kit';\nimport { libUsed } from './lib/util';\n\nexport const app = `${kitUsed}${libUsed}`;\n",
        )
        .expect("index");
    std::fs::write(
            root.join("src/ui/kit.ts"),
            "type Props = { label: string };\n\nexport const kitUsed = 'kit';\n\nexport const Unused = (props: Props) => props.label;\n",
        )
        .expect("kit");
    std::fs::write(
            root.join("src/lib/util.ts"),
            "type Internal = { id: string };\n\nexport const libUsed = 'lib';\n\nexport const alsoUnused = (value: Internal) => value.id;\n",
        )
        .expect("util");

    let session = EditorAnalysisSession::load(root, None).expect("session loads");
    let output = session
        .analyze_project_with_changed_files(
            &fallow_config::DuplicatesConfig::default(),
            false,
            None,
        )
        .expect("analysis runs");
    let results = &output.dead_code.results;

    let unused_export_paths = || {
        results
            .unused_exports
            .iter()
            .map(|finding| finding.export.path.clone())
            .collect::<Vec<_>>()
    };
    let leak_paths = || {
        results
            .private_type_leaks
            .iter()
            .map(|finding| finding.leak.path.clone())
            .collect::<Vec<_>>()
    };

    assert!(
        !unused_export_paths()
            .iter()
            .any(|path| path.ends_with("kit.ts")),
        "the override turns unused-exports off for src/ui/**: {:?}",
        unused_export_paths()
    );
    assert!(
        !leak_paths().iter().any(|path| path.ends_with("kit.ts")),
        "the override turns private-type-leaks off for src/ui/**: {:?}",
        leak_paths()
    );
    assert!(
        unused_export_paths()
            .iter()
            .any(|path| path.ends_with("util.ts")),
        "paths outside the override keep their unused export: {:?}",
        unused_export_paths()
    );
    assert!(
        leak_paths().iter().any(|path| path.ends_with("util.ts")),
        "paths outside the override keep their private type leak: {:?}",
        leak_paths()
    );
}

#[test]
fn merge_results_covers_all_fields() {
    let mut output = EditorAnalysisOutput::default();
    output.merge_results(merge_test_source_with_all_fields());
    let target = &output.results;

    assert_eq!(target.unused_files.len(), 1);
    assert_eq!(target.unused_exports.len(), 1);
    assert_eq!(target.unused_types.len(), 1);
    assert_eq!(target.private_type_leaks.len(), 1);
    assert_eq!(target.deprecated_exports_in_use.len(), 1);
    assert_eq!(target.unused_dependencies.len(), 1);
    assert_eq!(target.unused_dev_dependencies.len(), 1);
    assert_eq!(target.unused_optional_dependencies.len(), 1);
    assert_eq!(target.unused_enum_members.len(), 1);
    assert_eq!(target.unused_class_members.len(), 1);
    assert_eq!(target.unused_store_members.len(), 1);
    assert_eq!(target.unresolved_imports.len(), 1);
    assert_eq!(target.unlisted_dependencies.len(), 1);
    assert_eq!(target.duplicate_exports.len(), 1);
    assert_eq!(target.duplicate_export_aliases.len(), 1);
    assert_eq!(target.type_only_dependencies.len(), 1);
    assert_eq!(target.test_only_dependencies.len(), 1);
    assert_eq!(target.circular_dependencies.len(), 1);
    assert_eq!(target.re_export_cycles.len(), 1);
    assert_eq!(target.package_cycles.len(), 1);
    assert_eq!(target.boundary_violations.len(), 1);
    assert_eq!(target.boundary_call_violations.len(), 1);
    assert_eq!(target.policy_violations.len(), 1);
    assert_eq!(target.stale_suppressions.len(), 1);
    assert_eq!(target.unused_catalog_entries.len(), 1);
    assert_eq!(target.empty_catalog_groups.len(), 1);
    assert_eq!(target.unresolved_catalog_references.len(), 1);
    assert_eq!(target.unused_dependency_overrides.len(), 1);
    assert_eq!(target.misconfigured_dependency_overrides.len(), 1);
    assert_eq!(target.invalid_client_exports.len(), 1);
    assert_eq!(target.mixed_client_server_barrels.len(), 1);
    assert_eq!(target.misplaced_directives.len(), 1);
    assert_eq!(target.export_usages.len(), 1);
    assert_eq!(target.feature_flags.len(), 1);
    assert_eq!(target.security_findings.len(), 1);
    assert_eq!(target.security_unresolved_edge_files, 2);
    assert_eq!(target.security_unresolved_callee_diagnostics.len(), 1);
    assert_eq!(target.suppression_count, 1);
    assert_eq!(target.cascade_hidden, 1);
    assert_eq!(target.cascade.suppressed_unused_files.len(), 1);
    assert!(target.entry_point_summary.is_some());
    assert_eq!(
        target
            .render_fan_in
            .as_ref()
            .and_then(|m| m.max_distinct_parents),
        Some(3)
    );
    assert_eq!(target.react_component_intel.len(), 1);
    assert_eq!(target.dev_dependencies_in_production.len(), 1);
    assert_eq!(target.boundary_coverage_violations.len(), 1);
    assert_eq!(target.route_collisions.len(), 1);
    assert_eq!(target.dynamic_segment_name_conflicts.len(), 1);
    assert_eq!(target.unprovided_injects.len(), 1);
    assert_eq!(target.unrendered_components.len(), 1);
    assert_eq!(target.unused_component_props.len(), 1);
    let candidate = &target.absent_component_props[0];
    assert_eq!(candidate.prop.component_name, "Widget");
    assert_eq!(candidate.prop.prop_name, "highlight");
    assert!(candidate.prop.has_default);
    assert_eq!(candidate.prop.framework, "vue");
    assert_eq!(
        candidate.prop.inspected_call_sites[0].path,
        PathBuf::from("/main.vue")
    );
    assert_eq!(candidate.prop.explanation, "Review optional prop contract");

    assert_eq!(target.unused_component_emits.len(), 1);
    assert_eq!(target.unused_component_inputs.len(), 1);
    assert_eq!(target.unused_component_outputs.len(), 1);
    assert_eq!(target.unused_svelte_events.len(), 1);
    assert_eq!(target.unused_effect_layers.len(), 1);
    assert_eq!(target.unused_tagged_errors.len(), 1);
    assert_eq!(target.unproduced_catch_tags.len(), 1);
    assert_eq!(target.unused_server_actions.len(), 1);
    assert_eq!(target.unused_load_data_keys.len(), 1);
    assert!(target.unused_load_data_keys_global_abstain);
    assert_eq!(target.prop_drilling_chains.len(), 1);
    assert_eq!(target.thin_wrappers.len(), 1);
    assert_eq!(target.duplicate_prop_shapes.len(), 1);
    assert_eq!(target.active_suppressions.len(), 1);
    assert_eq!(target.semantic_framework_contracts.len(), 1);
    assert!(target.semantic_reachable_files.contains(Path::new("/f.ts")));
    assert_eq!(target.security_unresolved_callee_sites, 3);
    assert_eq!(target.unused_component_props_exempted, 1);
}

#[test]
fn merge_duplication_recomputes_percentage() {
    let target = EditorDuplicationReport {
        clone_groups: vec![],
        clone_families: vec![],
        mirrored_directories: vec![],
        stats: DuplicationStats {
            total_files: 5,
            files_with_clones: 1,
            total_lines: 200,
            duplicated_lines: 20,
            total_tokens: 1000,
            duplicated_tokens: 100,
            clone_groups: 1,
            clone_families: 0,
            clone_instances: 2,
            duplication_percentage: 10.0, // 20/200 * 100
            clone_groups_below_min_occurrences: 0,
            clone_groups_ignored: 0,
            near_candidates_skipped: 0,
        },
    };
    let source = EditorDuplicationReport {
        clone_groups: vec![],
        clone_families: vec![],
        mirrored_directories: vec![],
        stats: DuplicationStats {
            total_files: 3,
            files_with_clones: 1,
            total_lines: 300,
            duplicated_lines: 60,
            total_tokens: 1500,
            duplicated_tokens: 300,
            clone_groups: 2,
            clone_families: 0,
            clone_instances: 4,
            duplication_percentage: 20.0, // 60/300 * 100
            clone_groups_below_min_occurrences: 0,
            clone_groups_ignored: 0,
            near_candidates_skipped: 0,
        },
    };

    let mut output = EditorAnalysisOutput::new(EditorAnalysisResults::default(), target);
    output.merge_duplication(source);

    let target = &output.duplication;
    assert_eq!(target.stats.total_files, 8);
    assert_eq!(target.stats.files_with_clones, 2);
    assert_eq!(target.stats.total_lines, 500);
    assert_eq!(target.stats.duplicated_lines, 80);
    assert_eq!(target.stats.total_tokens, 2500);
    assert_eq!(target.stats.duplicated_tokens, 400);
    assert_eq!(target.stats.clone_groups, 3);
    assert_eq!(target.stats.clone_instances, 6);
    assert!((target.stats.duplication_percentage - 16.0).abs() < f64::EPSILON);
}

#[test]
fn merge_duplication_zero_total_lines_yields_zero_percentage() {
    let mut output = EditorAnalysisOutput::default();

    output.merge_duplication(EditorDuplicationReport::default());

    let target = &output.duplication;

    assert_eq!(target.stats.total_lines, 0);
    assert!((target.stats.duplication_percentage - 0.0).abs() < f64::EPSILON);
}

fn merge_test_unused_export(
    path: &str,
    export_name: &str,
    is_type_only: bool,
    line: u32,
) -> UnusedExport {
    UnusedExport {
        path: path.into(),
        export_name: export_name.to_string(),
        is_type_only,
        line,
        col: 0,
        span_start: 0,
        is_re_export: false,
        deprecated: false,
        deprecated_reason: None,
    }
}

fn merge_test_unused_dependency(
    package_name: &str,
    location: super::editor_results::DependencyLocation,
    line: u32,
) -> UnusedDependency {
    UnusedDependency {
        package_name: package_name.to_string(),
        location,
        path: "/pkg.json".into(),
        line,
        used_in_workspaces: Vec::new(),
        declared_and_imported_in: Vec::new(),
    }
}

fn merge_test_unused_member(
    parent_name: &str,
    member_name: &str,
    kind: super::editor_extract::MemberKind,
    line: u32,
) -> UnusedMember {
    UnusedMember {
        path: "/f.ts".into(),
        parent_name: parent_name.to_string(),
        member_name: member_name.to_string(),
        kind,
        line,
        col: 0,
    }
}

#[expect(
    clippy::too_many_lines,
    reason = "intentionally names every EditorAnalysisResults field (no ..Default::default()) so a new field is a compile error here; see #444"
)]
pub fn merge_test_source_with_all_fields() -> EditorAnalysisResults {
    EditorAnalysisResults {
        unused_files: vec![UnusedFileFinding::with_actions(UnusedFile {
            path: "/f.ts".into(),
        })],
        unused_exports: vec![UnusedExportFinding::with_actions(merge_test_unused_export(
            "/f.ts", "e", false, 1,
        ))],
        unused_types: vec![UnusedTypeFinding::with_actions(merge_test_unused_export(
            "/f.ts", "T", true, 2,
        ))],
        unused_dependencies: vec![UnusedDependencyFinding::with_actions(
            merge_test_unused_dependency(
                "dep",
                super::editor_results::DependencyLocation::Dependencies,
                3,
            ),
        )],
        unused_dev_dependencies: vec![UnusedDevDependencyFinding::with_actions(
            merge_test_unused_dependency(
                "dev-dep",
                super::editor_results::DependencyLocation::DevDependencies,
                4,
            ),
        )],
        unused_optional_dependencies: vec![UnusedOptionalDependencyFinding::with_actions(
            merge_test_unused_dependency(
                "opt-dep",
                super::editor_results::DependencyLocation::OptionalDependencies,
                5,
            ),
        )],
        unused_enum_members: vec![UnusedEnumMemberFinding::with_actions(
            merge_test_unused_member("E", "A", super::editor_extract::MemberKind::EnumMember, 6),
        )],
        unused_class_members: vec![UnusedClassMemberFinding::with_actions(
            merge_test_unused_member("C", "m", super::editor_extract::MemberKind::ClassMethod, 7),
        )],
        unused_store_members: vec![UnusedStoreMemberFinding::with_actions(
            merge_test_unused_member("S", "a", super::editor_extract::MemberKind::StoreMember, 7),
        )],
        unresolved_imports: vec![
            super::editor_results::UnresolvedImportFinding::with_actions(
                super::editor_results::UnresolvedImport {
                    path: "/f.ts".into(),
                    specifier: "./gone".to_string(),
                    line: 8,
                    col: 0,
                    specifier_col: 10,
                },
            ),
        ],
        unlisted_dependencies: vec![UnlistedDependencyFinding::with_actions(
            UnlistedDependency {
                package_name: "unlisted".to_string(),
                imported_from: vec![],
            },
        )],
        duplicate_exports: vec![super::editor_results::DuplicateExportFinding::with_actions(
            super::editor_results::DuplicateExport {
                export_name: "dup".to_string(),
                locations: vec![],
            },
        )],
        duplicate_export_aliases: vec![
            super::editor_results::DuplicateExportAliasFinding::with_actions(
                super::editor_results::DuplicateExportAlias {
                    path: "/worker.ts".into(),
                    local_name: "layer".to_string(),
                    export_names: vec!["layer".to_string(), "default".to_string()],
                    line: 1,
                    col: 13,
                    locations: vec![],
                },
            ),
        ],
        type_only_dependencies: vec![
            super::editor_results::TypeOnlyDependencyFinding::with_actions(TypeOnlyDependency {
                package_name: "type-only".to_string(),
                path: "/pkg.json".into(),
                line: 9,
            }),
        ],
        circular_dependencies: vec![CircularDependencyFinding::with_actions(
            CircularDependency {
                files: vec!["/a.ts".into(), "/b.ts".into()],
                length: 2,
                line: 10,
                col: 0,
                edges: Vec::new(),
                is_cross_package: false,
            },
        )],
        test_only_dependencies: vec![TestOnlyDependencyFinding::with_actions(
            TestOnlyDependency {
                package_name: "test-only".to_string(),
                path: "/pkg.json".into(),
                line: 11,
            },
        )],
        dev_dependencies_in_production: vec![DevDependencyInProductionFinding::with_actions(
            DevDependencyInProduction {
                package_name: "dev-in-prod".to_string(),
                path: "/pkg.json".into(),
                line: 12,
            },
        )],
        boundary_violations: vec![BoundaryViolationFinding::with_actions(BoundaryViolation {
            from_path: "/a.ts".into(),
            to_path: "/b.ts".into(),
            from_zone: "ui".to_string(),
            to_zone: "data".to_string(),
            import_specifier: "../data/db".to_string(),
            line: 12,
            col: 0,
            via_path: None,
        })],
        boundary_coverage_violations: vec![
            super::editor_results::BoundaryCoverageViolationFinding::with_actions(
                super::editor_results::BoundaryCoverageViolation {
                    path: "/unzoned.ts".into(),
                    line: 13,
                    col: 0,
                },
            ),
        ],
        boundary_call_violations: vec![
            super::editor_results::BoundaryCallViolationFinding::with_actions(
                super::editor_results::BoundaryCallViolation {
                    path: "/zoned.ts".into(),
                    line: 14,
                    col: 0,
                    zone: "domain".to_string(),
                    callee: "console.log".to_string(),
                    pattern: "console.*".to_string(),
                },
            ),
        ],
        policy_violations: vec![super::editor_results::PolicyViolationFinding::with_actions(
            super::editor_results::PolicyViolation {
                path: "/zoned.ts".into(),
                line: 15,
                col: 0,
                pack: "team-policy".to_string(),
                rule_id: "no-console".to_string(),
                kind: super::editor_results::PolicyRuleKind::BannedCall,
                matched: "console.log".to_string(),
                severity: super::editor_results::PolicyViolationSeverity::Warn,
                message: None,
            },
        )],
        export_usages: vec![ExportUsage {
            path: "/f.ts".into(),
            export_name: "used".to_string(),
            line: 15,
            col: 0,
            reference_count: 3,
            reference_locations: vec![],
        }],
        private_type_leaks: vec![super::editor_results::PrivateTypeLeakFinding::with_actions(
            super::editor_results::PrivateTypeLeak {
                path: "/f.ts".into(),
                export_name: "pub_fn".to_string(),
                type_name: "Secret".to_string(),
                line: 14,
                col: 0,
                span_start: 0,
                semantic: None,
            },
        )],
        deprecated_exports_in_use: vec![
            super::editor_results::DeprecatedExportInUseFinding::with_actions(
                super::editor_results::DeprecatedExportInUse {
                    path: "/f.ts".into(),
                    export_name: "old".to_string(),
                    is_type_only: false,
                    line: 16,
                    col: 0,
                    span_start: 0,
                    deprecated_reason: None,
                    consumer_count: 1,
                    consumers: vec![super::editor_results::DeprecatedExportConsumer {
                        path: "/g.ts".into(),
                        line: 1,
                        col: 0,
                        kind: super::editor_results::DeprecatedConsumerKind::NamedImport,
                    }],
                    public_api: false,
                },
            ),
        ],
        re_export_cycles: vec![super::editor_results::ReExportCycleFinding::with_actions(
            super::editor_results::ReExportCycle {
                files: vec!["/barrel.ts".into()],
                kind: super::editor_results::ReExportCycleKind::SelfLoop,
            },
        )],
        package_cycles: vec![super::editor_results::PackageCycleFinding::with_actions(
            super::editor_results::PackageCycle {
                packages: vec!["a".into(), "b".into()],
                package_roots: Vec::new(),
                length: 2,
                edges: vec![super::editor_results::PackageCycleEdge {
                    from_package: "a".into(),
                    to_package: "b".into(),
                    path: "/a/x.ts".into(),
                    target_path: "/b/y.ts".into(),
                    line: 1,
                    col: 0,
                    type_only: false,
                }],
                group_truncated: false,
            },
        )],
        stale_suppressions: vec![super::editor_results::StaleSuppression {
            finding_id: None,
            path: "/f.ts".into(),
            line: 15,
            col: 0,
            origin: super::editor_results::SuppressionOrigin::Comment {
                issue_kind: None,
                reason: None,
                is_file_level: false,
                kind_known: true,
            },
            missing_reason: false,
            actions: super::editor_results::StaleSuppression::actions_for(false),
            effective_severity: None,
        }],
        unused_catalog_entries: vec![
            super::editor_results::UnusedCatalogEntryFinding::with_actions(
                super::editor_results::UnusedCatalogEntry {
                    entry_name: "react".to_string(),
                    catalog_name: "default".to_string(),
                    path: "/pnpm-workspace.yaml".into(),
                    line: 16,
                    hardcoded_consumers: vec![],
                },
            ),
        ],
        empty_catalog_groups: vec![
            super::editor_results::EmptyCatalogGroupFinding::with_actions(
                super::editor_results::EmptyCatalogGroup {
                    catalog_name: "ui".to_string(),
                    path: "/pnpm-workspace.yaml".into(),
                    line: 17,
                },
            ),
        ],
        unresolved_catalog_references: vec![
            super::editor_results::UnresolvedCatalogReferenceFinding::with_actions(
                super::editor_results::UnresolvedCatalogReference {
                    entry_name: "vue".to_string(),
                    catalog_name: "default".to_string(),
                    path: "/pkg.json".into(),
                    line: 18,
                    available_in_catalogs: vec![],
                },
            ),
        ],
        unused_dependency_overrides: vec![
            super::editor_results::UnusedDependencyOverrideFinding::with_actions(
                super::editor_results::UnusedDependencyOverride {
                    raw_key: "react".to_string(),
                    target_package: "react".to_string(),
                    parent_package: None,
                    version_constraint: None,
                    version_range: "18".to_string(),
                    source: super::editor_results::DependencyOverrideSource::PnpmWorkspaceYaml,
                    path: "/pnpm-workspace.yaml".into(),
                    line: 19,
                    hint: None,
                },
            ),
        ],
        misconfigured_dependency_overrides: vec![
            super::editor_results::MisconfiguredDependencyOverrideFinding::with_actions(
                super::editor_results::MisconfiguredDependencyOverride {
                    raw_key: "bad>".to_string(),
                    target_package: None,
                    raw_value: String::new(),
                    reason: super::editor_results::DependencyOverrideMisconfigReason::EmptyValue,
                    source: super::editor_results::DependencyOverrideSource::PnpmPackageJson,
                    path: "/pkg.json".into(),
                    line: 20,
                },
            ),
        ],
        invalid_client_exports: vec![
            super::editor_results::InvalidClientExportFinding::with_actions(
                super::editor_results::InvalidClientExport {
                    path: "/app/page.tsx".into(),
                    export_name: "metadata".to_string(),
                    directive: "use client".to_string(),
                    line: 22,
                    col: 0,
                },
            ),
        ],
        mixed_client_server_barrels: vec![
            super::editor_results::MixedClientServerBarrelFinding::with_actions(
                super::editor_results::MixedClientServerBarrel {
                    path: "/app/components/index.ts".into(),
                    client_origin: "./Button".to_string(),
                    server_origin: "./fetchUser".to_string(),
                    line: 23,
                    col: 0,
                },
            ),
        ],
        misplaced_directives: vec![
            super::editor_results::MisplacedDirectiveFinding::with_actions(
                super::editor_results::MisplacedDirective {
                    path: "/app/widget.tsx".into(),
                    directive: "use client".to_string(),
                    line: 24,
                    col: 0,
                },
            ),
        ],
        unprovided_injects: vec![
            super::editor_results::UnprovidedInjectFinding::with_actions(
                super::editor_results::UnprovidedInject {
                    path: "/Comp.vue".into(),
                    key_name: "ApiKey".to_string(),
                    framework: "vue".to_string(),
                    line: 25,
                    col: 0,
                },
            ),
        ],
        unrendered_components: vec![
            super::editor_results::UnrenderedComponentFinding::with_actions(
                super::editor_results::UnrenderedComponent {
                    path: "/Widget.vue".into(),
                    component_name: "Widget".to_string(),
                    framework: "vue".to_string(),
                    reachable_via: None,
                    line: 26,
                    col: 0,
                },
            ),
        ],
        unused_component_props: vec![
            super::editor_results::UnusedComponentPropFinding::with_actions(
                super::editor_results::UnusedComponentProp {
                    path: "/Widget.vue".into(),
                    component_name: "Widget".to_string(),
                    prop_name: "size".to_string(),
                    line: 27,
                    col: 0,
                },
            ),
        ],
        absent_component_props: vec![
            super::editor_results::AbsentComponentPropFinding::with_actions(
                super::editor_results::AbsentComponentProp {
                    path: "/Widget.vue".into(),
                    component_name: "Widget".to_string(),
                    framework: "vue".to_string(),
                    prop_name: "highlight".to_string(),
                    line: 27,
                    col: 0,
                    has_default: true,
                    inspected_call_sites: vec![super::editor_results::ComponentPropCallSite {
                        path: "/main.vue".into(),
                        line: 1,
                        col: 0,
                    }],
                    explanation: "Review optional prop contract".to_string(),
                },
            ),
        ],
        unused_component_emits: vec![
            super::editor_results::UnusedComponentEmitFinding::with_actions(
                super::editor_results::UnusedComponentEmit {
                    path: "/Widget.vue".into(),
                    component_name: "Widget".to_string(),
                    emit_name: "change".to_string(),
                    line: 28,
                    col: 0,
                },
            ),
        ],
        unused_component_inputs: vec![
            super::editor_results::UnusedComponentInputFinding::with_actions(
                super::editor_results::UnusedComponentInput {
                    path: "/widget.component.ts".into(),
                    component_name: "WidgetComponent".to_string(),
                    input_name: "size".to_string(),
                    line: 29,
                    col: 0,
                },
            ),
        ],
        unused_component_outputs: vec![
            super::editor_results::UnusedComponentOutputFinding::with_actions(
                super::editor_results::UnusedComponentOutput {
                    path: "/widget.component.ts".into(),
                    component_name: "WidgetComponent".to_string(),
                    output_name: "change".to_string(),
                    line: 30,
                    col: 0,
                },
            ),
        ],
        unused_svelte_events: vec![
            super::editor_results::UnusedSvelteEventFinding::with_actions(
                super::editor_results::UnusedSvelteEvent {
                    path: "/Child.svelte".into(),
                    component_name: "Child".to_string(),
                    event_name: "dead".to_string(),
                    line: 31,
                    col: 0,
                },
            ),
        ],
        unused_effect_layers: vec![
            super::editor_results::UnusedEffectLayerFinding::with_actions(
                super::editor_results::UnusedEffectLayer {
                    path: "/services.ts".into(),
                    name: "CacheLive".to_string(),
                    kind: super::editor_results::EffectLayerKind::TopLevel,
                    line: 33,
                    col: 0,
                    dead_consumers: vec!["OrphanLive".to_string()],
                },
            ),
        ],
        unused_tagged_errors: vec![
            super::editor_results::UnusedTaggedErrorFinding::with_actions(
                super::editor_results::UnusedTaggedError {
                    path: "/errors.ts".into(),
                    name: "Expired".to_string(),
                    tag: "Expired".to_string(),
                    line: 34,
                    col: 0,
                    catch_sites: Vec::new(),
                },
            ),
        ],
        unproduced_catch_tags: vec![
            super::editor_results::UnproducedCatchTagFinding::with_actions(
                super::editor_results::UnproducedCatchTag {
                    path: "/main.ts".into(),
                    tag: "Expired".to_string(),
                    line: 35,
                    col: 0,
                    candidate_classes: vec!["Expired".to_string()],
                },
            ),
        ],
        unused_server_actions: vec![
            super::editor_results::UnusedServerActionFinding::with_actions(
                super::editor_results::UnusedServerAction {
                    path: "/app/actions.ts".into(),
                    action_name: "createUser".to_string(),
                    line: 32,
                    col: 0,
                },
            ),
        ],
        unused_load_data_keys: vec![
            super::editor_results::UnusedLoadDataKeyFinding::with_actions(
                super::editor_results::UnusedLoadDataKey {
                    path: "/src/routes/blog/+page.server.ts".into(),
                    key_name: "posts".to_string(),
                    line: 33,
                    col: 0,
                    route_dir: None,
                },
            ),
        ],
        unused_load_data_keys_global_abstain: true,
        prop_drilling_chains: vec![
            super::editor_results::PropDrillingChainFinding::with_actions(
                super::editor_results::PropDrillingChain {
                    prop: "user".to_string(),
                    depth: 1,
                    hops: vec![super::editor_results::PropDrillHop {
                        file: "/Hop.tsx".into(),
                        line: 34,
                        component: "Hop".to_string(),
                    }],
                },
            ),
        ],
        thin_wrappers: vec![super::editor_results::ThinWrapperFinding::with_actions(
            super::editor_results::ThinWrapper {
                file: "/Wrapper.tsx".into(),
                line: 35,
                component: "Wrapper".to_string(),
                child_component: "Child".to_string(),
            },
        )],
        duplicate_prop_shapes: vec![
            super::editor_results::DuplicatePropShapeFinding::with_actions(
                super::editor_results::DuplicatePropShape {
                    file: "/Card.tsx".into(),
                    line: 36,
                    component: "Card".to_string(),
                    shape: vec!["subtitle".to_string(), "title".to_string()],
                    group_size: 2,
                    sharing_components: vec![],
                },
            ),
        ],
        route_collisions: vec![super::editor_results::RouteCollisionFinding::with_actions(
            super::editor_results::RouteCollision {
                path: "/app/(a)/about/page.tsx".into(),
                url: "/about".to_string(),
                conflicting_paths: vec!["/app/(b)/about/page.tsx".into()],
                line: 1,
                col: 0,
            },
        )],
        dynamic_segment_name_conflicts: vec![
            super::editor_results::DynamicSegmentNameConflictFinding::with_actions(
                super::editor_results::DynamicSegmentNameConflict {
                    path: "/app/shop/[id]/page.tsx".into(),
                    position: "/shop".to_string(),
                    conflicting_segments: vec!["[id]".to_string(), "[slug]".to_string()],
                    conflicting_paths: vec!["/app/shop/[slug]/edit/page.tsx".into()],
                    line: 1,
                    col: 0,
                },
            ),
        ],
        cascade_hidden: 1,
        cascade: fallow_types::results::CascadeState {
            suppressed_unused_files: vec!["/f.ts".into()],
            ..fallow_types::results::CascadeState::default()
        },
        suppression_count: 1,
        unused_component_props_exempted: 1,
        active_suppressions: vec![super::editor_results::ActiveSuppression {
            path: "/f.ts".into(),
            kind: Some("unused-export".to_string()),
            is_file_level: false,
            reason: None,
            comment_line: 37,
        }],
        feature_flags: vec![super::editor_results::FeatureFlag {
            path: "/f.ts".into(),
            flag_name: "ENABLE_X".to_string(),
            kind: super::editor_results::FlagKind::EnvironmentVariable,
            confidence: super::editor_results::FlagConfidence::High,
            line: 21,
            col: 0,
            guard_span_start: None,
            guard_span_end: None,
            sdk_name: None,
            guard_line_start: None,
            guard_line_end: None,
            guarded_dead_exports: vec![],
        }],
        entry_point_summary: Some(super::editor_results::EntryPointSummary {
            total: 0,
            by_source: vec![],
        }),
        security_findings: vec![super::editor_results::SecurityFinding {
            finding_id: String::new(),
            candidate: super::editor_results::SecurityCandidate::default(),
            taint_flow: None,
            attack_surface: None,
            kind: super::editor_results::SecurityFindingKind::ClientServerLeak,
            category: None,
            cwe: None,
            path: "/client.tsx".into(),
            line: 1,
            col: 0,
            evidence: "transitively reaches DATABASE_URL".to_string(),
            source_backed: false,
            source_read: None,
            severity: SecuritySeverity::Low,
            trace: vec![],
            actions: vec![],
            dead_code: None,
            reachability: None,
            runtime: None,
        }],
        security_unresolved_edge_files: 2,
        security_unresolved_callee_sites: 3,
        security_unresolved_callee_diagnostics: vec![
            super::editor_results::SecurityUnresolvedCalleeDiagnostic {
                path: "/client.tsx".into(),
                line: 2,
                col: 0,
                reason: super::editor_extract::SkippedSecurityCalleeReason::DynamicDispatch,
                expression_kind: super::editor_extract::SkippedSecurityCalleeExpressionKind::Other,
            },
        ],
        render_fan_in: Some(super::editor_results::RenderFanInMetric {
            per_component: vec![super::editor_results::RenderFanInComponent {
                file: "/Button.tsx".into(),
                component: "Button".to_string(),
                render_sites: 6,
                distinct_parents: 3,
            }],
            p95_distinct_parents: Some(3),
            high_pct: Some(0.0),
            max_distinct_parents: Some(3),
        }),
        react_component_intel: vec![super::editor_results::ReactComponentIntel {
            path: "/Button.tsx".into(),
            component_name: "Button".to_string(),
            anchor_line: 1,
            anchor_col: 0,
            render_sites: 6,
            distinct_parents: 3,
            prop_count: 1,
            hooks: super::editor_results::ReactHookSummary::default(),
            props: Vec::new(),
        }],
        semantic_reachable_files: std::iter::once("/f.ts".into()).collect(),
        semantic_framework_contracts: vec![fallow_types::semantic::SemanticFrameworkContract {
            framework: "lit".to_string(),
            package: "lit".to_string(),
            heritage_symbol: "LitElement".to_string(),
            heritage_names: vec!["LitElement".to_string()],
            relation: fallow_types::semantic::SemanticFrameworkRelation::Extends,
            members: vec!["render".to_string()],
        }],
    }
}
