use std::path::PathBuf;

use fallow_types::duplicates::{CloneGroup, CloneInstance, DuplicationStats};
use fallow_types::output_dead_code::UnusedFileFinding;
use fallow_types::results::{AnalysisResults, UnusedFile};

use super::*;

fn diff_risk_finding(
    rule: fallow_output::DiffRiskRule,
    kind: Option<fallow_output::WeakenedTestsKind>,
    path: &str,
    line: Option<u32>,
    evidence: &str,
    severity: fallow_output::DiffRiskSeverity,
) -> fallow_output::DiffRiskFinding {
    fallow_output::DiffRiskFinding {
        rule,
        kind,
        path: path.to_owned(),
        line,
        message: "m".to_owned(),
        evidence: evidence.to_owned(),
        severity,
        introduced: true,
        related: Vec::new(),
        actions: Vec::new(),
        suppression: None,
    }
}

#[test]
fn diff_risk_compact_lines_follow_the_tag_path_line_subject_grammar() {
    let report = fallow_output::DiffRiskReport {
        findings: vec![
            diff_risk_finding(
                fallow_output::DiffRiskRule::WeakenedTests,
                Some(fallow_output::WeakenedTestsKind::SkipOnly),
                "src/a.test.ts",
                Some(4),
                "it.skip",
                fallow_output::DiffRiskSeverity::Warn,
            ),
            diff_risk_finding(
                fallow_output::DiffRiskRule::WeakenedTests,
                Some(fallow_output::WeakenedTestsKind::RemovedTests),
                "src/b.test.ts",
                None,
                "test cases: 3 -> 1",
                fallow_output::DiffRiskSeverity::Error,
            ),
            diff_risk_finding(
                fallow_output::DiffRiskRule::AddedDependency,
                None,
                "package.json",
                Some(7),
                "left-pad@^1.3.0 (dependencies)",
                fallow_output::DiffRiskSeverity::Warn,
            ),
        ],
    };
    assert_eq!(
        build_diff_risk_compact_lines(&report),
        vec![
            "weakened-tests:src/a.test.ts:4:it.skip,severity=warn,kind=skip-only".to_owned(),
            "weakened-tests:src/b.test.ts:0:test cases 3 -> 1,severity=error,kind=removed-tests"
                .to_owned(),
            "added-dependency:package.json:7:left-pad@^1.3.0 (dependencies),severity=warn"
                .to_owned(),
        ]
    );
}

#[test]
fn compact_cycles_preserve_empty_self_loop_and_cross_package_output() {
    use fallow_types::output_dead_code::{CircularDependencyFinding, ReExportCycleFinding};
    use fallow_types::results::{CircularDependency, ReExportCycle, ReExportCycleKind};

    let root = Path::new("/project");
    for (files, cross_package, expected) in [
        (vec![], false, "circular-dependency::7:"),
        (
            vec![root.join("src/a.ts")],
            false,
            "circular-dependency:src/a.ts:7:src/a.ts → src/a.ts",
        ),
        (
            vec![root.join("src/a.ts"), root.join("src/b.ts")],
            true,
            "circular-dependency:src/a.ts:7:src/a.ts → src/b.ts → src/a.ts (cross-package)",
        ),
    ] {
        let mut results = AnalysisResults::default();
        results
            .circular_dependencies
            .push(CircularDependencyFinding {
                finding_id: None,
                cycle: CircularDependency {
                    length: files.len(),
                    files,
                    line: 7,
                    col: 0,
                    edges: vec![],
                    is_cross_package: cross_package,
                },
                actions: vec![],
                introduced: None,
                effective_severity: None,
            });
        assert_eq!(build_compact_lines(&results, root), vec![expected]);
    }
    for (files, kind, line, expected) in [
        (
            vec![],
            ReExportCycleKind::MultiNode,
            0,
            "re-export-cycle::0:",
        ),
        (
            vec![root.join("src/a.ts")],
            ReExportCycleKind::SelfLoop,
            1,
            "re-export-cycle:src/a.ts:1:src/a.ts (self-loop)",
        ),
        (
            vec![root.join("src/a.ts"), root.join("src/b.ts")],
            ReExportCycleKind::MultiNode,
            4,
            "re-export-cycle:src/a.ts:4:src/a.ts <-> src/b.ts",
        ),
    ] {
        let mut results = AnalysisResults::default();
        results.re_export_cycles.push(ReExportCycleFinding {
            finding_id: None,
            cycle: ReExportCycle {
                files,
                kind,
                line,
                col: 0,
            },
            actions: vec![],
            introduced: None,
            effective_severity: None,
        });
        assert_eq!(build_compact_lines(&results, root), vec![expected]);
    }
}

/// A compact record is what a CI script greps to decide what to delete, so
/// a verdict resting on a file the run never read has to say so on the same
/// line. The field rides in the trailing `,key=value` slot duplication
/// records already use, so the leading colon-separated fields a parser
/// splits on are untouched.
#[test]
fn a_caveated_finding_carries_a_trailing_caveat_field() {
    use fallow_types::extract::MemberKind;
    use fallow_types::output_dead_code::{ReachabilityCaveat, UnusedDependencyFinding};
    use fallow_types::results::{UnusedDependency, UnusedExport};

    let root = PathBuf::from("/project");
    let mut results = AnalysisResults::default();

    let mut file = UnusedFileFinding::with_actions(UnusedFile {
        path: root.join("src/dead.ts"),
    });
    file.reachability_caveats = vec![ReachabilityCaveat::IncompleteImportGraph];
    results.unused_files.push(file);

    let mut export =
        fallow_types::output_dead_code::UnusedExportFinding::with_actions(UnusedExport {
            path: root.join("src/lib.ts"),
            export_name: "needed".to_owned(),
            is_type_only: false,
            line: 3,
            col: 0,
            span_start: 0,
            is_re_export: false,
            deprecated: false,
            deprecated_reason: None,
        });
    export.reachability_caveats = vec![
        ReachabilityCaveat::IncompleteFileAnalysis,
        ReachabilityCaveat::IncompleteImportGraph,
    ];
    results.unused_exports.push(export);

    let mut dep = UnusedDependencyFinding::with_actions(UnusedDependency {
        package_name: "left-pad".to_owned(),
        location: fallow_types::results::DependencyLocation::Dependencies,
        path: root.join("package.json"),
        line: 5,
        used_in_workspaces: Vec::new(),
        declared_and_imported_in: Vec::new(),
    });
    dep.reachability_caveats = vec![ReachabilityCaveat::IncompleteImportGraph];
    results.unused_dependencies.push(dep);

    let member = |parent: &str, name: &str, kind| fallow_types::results::UnusedMember {
        path: root.join("src/lib.ts"),
        parent_name: parent.to_owned(),
        member_name: name.to_owned(),
        kind,
        line: 7,
        col: 2,
    };
    let mut enum_member = fallow_types::output_dead_code::UnusedEnumMemberFinding::with_actions(
        member("Mode", "Legacy", MemberKind::EnumMember),
    );
    enum_member.reachability_caveats = vec![ReachabilityCaveat::IncompleteImportGraph];
    results.unused_enum_members.push(enum_member);

    let mut class_member =
        fallow_types::output_dead_code::UnusedClassMemberFinding::with_actions(member(
            "Widget",
            "render",
            MemberKind::ClassMethod,
        ));
    class_member.reachability_caveats = vec![ReachabilityCaveat::IncompleteImportGraph];
    results.unused_class_members.push(class_member);

    let mut store_member =
        fallow_types::output_dead_code::UnusedStoreMemberFinding::with_actions(member(
            "useCart",
            "subtotal",
            MemberKind::StoreMember,
        ));
    store_member.reachability_caveats = vec![ReachabilityCaveat::IncompleteImportGraph];
    results.unused_store_members.push(store_member);

    let lines = build_compact_lines(&results, &root);

    assert_eq!(
        lines,
        vec![
            "unused-file:src/dead.ts,caveat=incomplete-import-graph",
            "unused-export:src/lib.ts:3:needed,caveat=incomplete-file-analysis+incomplete-import-graph",
            "unused-dep:package.json:5:left-pad,caveat=incomplete-import-graph",
            "unused-enum-member:src/lib.ts:7:Mode.Legacy,caveat=incomplete-import-graph",
            "unused-class-member:src/lib.ts:7:Widget.render,caveat=incomplete-import-graph",
            "unused-store-member:src/lib.ts:7:useCart.subtotal,caveat=incomplete-import-graph",
        ]
    );
}

#[test]
fn compact_unused_file_format_uses_relative_paths() {
    let root = PathBuf::from("/project");
    let mut results = AnalysisResults::default();
    results
        .unused_files
        .push(UnusedFileFinding::with_actions(UnusedFile {
            path: root.join("src/dead.ts"),
        }));

    let lines = build_compact_lines(&results, &root);

    assert_eq!(lines, vec!["unused-file:src/dead.ts"]);
}

#[test]
fn grouped_compact_prefixes_each_issue_with_group_key() {
    let root = PathBuf::from("/project");
    let mut results = AnalysisResults::default();
    results
        .unused_files
        .push(UnusedFileFinding::with_actions(UnusedFile {
            path: root.join("src/dead.ts"),
        }));
    let groups = vec![ResultGroup {
        key: "team-a".to_owned(),
        owners: Some(vec!["@team-a".to_owned()]),
        results,
    }];

    let lines = build_grouped_compact_lines(&groups, &root);

    assert_eq!(lines, vec!["team-a\tunused-file:src/dead.ts"]);
}

#[test]
fn duplication_compact_lines_include_stable_group_context() {
    let root = PathBuf::from("/project");
    let report = DuplicationReport {
        clone_groups: vec![CloneGroup {
            instances: vec![CloneInstance {
                is_symlink: false,
                file: root.join("src/a.ts"),
                start_line: 2,
                end_line: 6,
                start_col: 0,
                end_col: 10,
                fragment: "const duplicated = true;".to_owned(),
            }],
            token_count: 12,
            line_count: 5,
            similarity: None,
        }],
        clone_families: Vec::new(),
        mirrored_directories: Vec::new(),
        range_marker_problems: Vec::new(),
        stats: DuplicationStats::default(),
    };

    let lines = build_duplication_compact_lines(&report, &root);

    assert_eq!(lines.len(), 1);
    assert!(lines[0].starts_with("code-duplication:src/a.ts:2-6:fingerprint="));
    assert!(lines[0].contains(",group=1,tokens=12,lines=5,instances=1"));
    assert!(!lines[0].contains("symlink"));
}

#[test]
fn duplication_compact_lines_mark_symlinked_instances() {
    let root = PathBuf::from("/project");
    let instance = |path: &str, is_symlink: bool| CloneInstance {
        is_symlink,
        file: root.join(path),
        start_line: 1,
        end_line: 5,
        start_col: 0,
        end_col: 0,
        fragment: String::new(),
    };
    let report = DuplicationReport {
        clone_groups: vec![CloneGroup {
            instances: vec![
                instance("src/link.ts", true),
                instance("src/real.ts", false),
            ],
            token_count: 12,
            line_count: 5,
            similarity: None,
        }],
        clone_families: Vec::new(),
        mirrored_directories: Vec::new(),
        range_marker_problems: Vec::new(),
        stats: DuplicationStats::default(),
    };

    let lines = build_duplication_compact_lines(&report, &root);

    assert!(lines[0].starts_with("code-duplication:src/link.ts:1-5:"));
    assert!(lines[0].ends_with(",instances=2,symlink=true"));
    assert!(lines[1].ends_with(",instances=2"));
}

#[test]
fn health_compact_lines_include_score_and_vital_signs() {
    let root = PathBuf::from("/project");
    let report = fallow_output::HealthReport {
        health_score: Some(fallow_output::HealthScore {
            formula_version: 1,
            score: 91.2,
            grade: "A",
            penalties: fallow_output::HealthScorePenalties {
                dead_files: None,
                dead_exports: None,
                complexity: 0.0,
                p90_complexity: 0.0,
                maintainability: None,
                hotspots: None,
                unused_deps: None,
                circular_deps: None,
                unit_size: None,
                coupling: None,
                duplication: None,
                prop_drilling: None,
            },
        }),
        vital_signs: Some(fallow_output::VitalSigns {
            total_loc: 120,
            avg_cyclomatic: 3.4,
            p90_cyclomatic: 8,
            ..Default::default()
        }),
        ..Default::default()
    };

    let lines = build_health_compact_lines(&report, &root);

    assert_eq!(lines[0], "health-score:91.2:A");
    assert_eq!(
        lines[1],
        "vital-signs:total_loc=120,avg_cyclomatic=3.4,p90_cyclomatic=8"
    );
    assert!(
        lines
            .iter()
            .all(|line| !line.starts_with("cyclomatic-population:"))
    );
}

#[test]
fn health_compact_population_rows_preserve_vital_signs_and_order() {
    let root = PathBuf::from("/project");
    let vitals = fallow_output::VitalSigns {
        avg_cyclomatic: 16.0,
        p90_cyclomatic: 31,
        ..Default::default()
    };
    let legacy = build_health_compact_lines(
        &fallow_output::HealthReport {
            vital_signs: Some(vitals.clone()),
            ..Default::default()
        },
        &root,
    );
    let report = fallow_output::HealthReport {
        vital_signs: Some(fallow_output::VitalSigns {
            cyclomatic_population: Some(fallow_output::CyclomaticPopulation {
                functions: fallow_output::CyclomaticUnitPopulation {
                    count: 1,
                    sum: 1,
                    max: Some(1),
                },
                modules: fallow_output::CyclomaticUnitPopulation {
                    count: 1,
                    sum: 31,
                    max: Some(31),
                },
                templates: fallow_output::CyclomaticUnitPopulation::default(),
            }),
            ..vitals
        }),
        ..Default::default()
    };
    let lines = build_health_compact_lines(&report, &root);
    assert_eq!(lines[0], legacy[0]);
    assert_eq!(
        &lines[1..],
        [
            "cyclomatic-population:functions:count=1,sum=1,max=1",
            "cyclomatic-population:modules:count=1,sum=31,max=31",
            "cyclomatic-population:templates:count=0,sum=0,max=null",
        ]
    );
}

#[test]
fn health_compact_measured_empty_populations_keep_null_maxima() {
    let report = fallow_output::HealthReport {
        vital_signs: Some(fallow_output::VitalSigns {
            cyclomatic_population: Some(fallow_output::CyclomaticPopulation::default()),
            ..Default::default()
        }),
        ..Default::default()
    };
    let lines = build_health_compact_lines(&report, &PathBuf::from("/project"));
    assert_eq!(
        &lines[1..],
        [
            "cyclomatic-population:functions:count=0,sum=0,max=null",
            "cyclomatic-population:modules:count=0,sum=0,max=null",
            "cyclomatic-population:templates:count=0,sum=0,max=null",
        ]
    );
}

#[test]
fn health_compact_lines_include_styling_findings() {
    let root = PathBuf::from("/project");
    let report = fallow_output::HealthReport {
        styling_findings: vec![fallow_output::StylingFinding {
            internal: fallow_output::StylingFindingInternal::default(),
            code: "css-token-drift".to_string(),
            sub_kind: "tailwind-arbitrary-value".to_string(),
            path: "/project/src/app.css".to_string(),
            line: 6,
            value: "--color-brand: rgb(240, 90, 41)".to_string(),
            effective_severity: fallow_output::StylingFindingSeverity::Warn,
            blast_radius: None,
            confidence: None,
            agent_disposition: None,
            nearest_token: None,
            fix_hint: None,
            actions: Vec::new(),
        }],
        ..Default::default()
    };

    let lines = build_health_compact_lines(&report, &root);

    assert_eq!(
        lines,
        vec![
            "css-token-drift:src/app.css:6:tailwind-arbitrary-value:severity=warn,value=--color-brand rgb(240 90 41)"
        ]
    );
}

#[test]
fn namespace_escape_compact_line_has_four_fields() {
    let root = PathBuf::from("/project");
    let mut results = AnalysisResults::default();
    results.namespace_escapes.push(
        fallow_types::output_dead_code::NamespaceEscapeFinding::with_actions(
            fallow_types::results::NamespaceEscape {
                path: root.join("src/app.ts"),
                line: 3,
                col: 7,
                local_name: "Formats".to_string(),
                module: root.join("src/formats.ts"),
                hidden_exports: 2,
                reason: fallow_types::results::NamespaceEscapeReason::WholeObjectUse,
            },
        ),
    );

    assert_eq!(
        build_compact_lines(&results, &root),
        vec!["namespace-escape:src/app.ts:3:Formats".to_owned()]
    );
}

#[test]
fn compact_dependency_lines_include_package_manifest_locations() {
    use fallow_types::output_dead_code::{
        DevDependencyInProductionFinding, TestOnlyDependencyFinding, TypeOnlyDependencyFinding,
        UnusedDependencyFinding, UnusedDevDependencyFinding, UnusedOptionalDependencyFinding,
    };
    use fallow_types::results::{
        DependencyLocation, DevDependencyInProduction, TestOnlyDependency, TypeOnlyDependency,
        UnusedDependency,
    };

    let root = PathBuf::from("/project");
    let mut results = AnalysisResults::default();

    results.unused_dependencies.push(
        UnusedDependencyFinding::with_actions(UnusedDependency {
            package_name: "lodash".to_string(),
            location: DependencyLocation::Dependencies,
            path: root.join("package.json"),
            line: 5,
            used_in_workspaces: Vec::new(),
            declared_and_imported_in: Vec::new(),
        }),
    );
    results.unused_dev_dependencies.push(
        UnusedDevDependencyFinding::with_actions(UnusedDependency {
            package_name: "jest".to_string(),
            location: DependencyLocation::DevDependencies,
            path: root.join("package.json"),
            line: 6,
            used_in_workspaces: Vec::new(),
            declared_and_imported_in: Vec::new(),
        }),
    );
    results.unused_optional_dependencies.push(
        UnusedOptionalDependencyFinding::with_actions(UnusedDependency {
            package_name: "fsevents".to_string(),
            location: DependencyLocation::OptionalDependencies,
            path: root.join("package.json"),
            line: 7,
            used_in_workspaces: Vec::new(),
            declared_and_imported_in: Vec::new(),
        }),
    );
    results.type_only_dependencies.push(
        TypeOnlyDependencyFinding::with_actions(TypeOnlyDependency {
            package_name: "zod".to_string(),
            path: root.join("package.json"),
            line: 8,
        }),
    );
    results.test_only_dependencies.push(
        TestOnlyDependencyFinding::with_actions(TestOnlyDependency {
            package_name: "msw".to_string(),
            path: root.join("package.json"),
            line: 9,
        }),
    );
    results.dev_dependencies_in_production.push(
        DevDependencyInProductionFinding::with_actions(DevDependencyInProduction {
            package_name: "express".to_string(),
            path: root.join("package.json"),
            line: 10,
        }),
    );

    assert_eq!(
        build_compact_lines(&results, &root),
        vec![
            "unused-dep:package.json:5:lodash".to_owned(),
            "unused-devdep:package.json:6:jest".to_owned(),
            "unused-optionaldep:package.json:7:fsevents".to_owned(),
            "type-only-dep:package.json:8:zod".to_owned(),
            "test-only-dep:package.json:9:msw".to_owned(),
            "dev-dep-in-prod:package.json:10:express".to_owned(),
        ]
    );
}

#[test]
fn compact_unlisted_dependency_and_duplicate_export_lines_fan_out_locations() {
    use fallow_types::output_dead_code::{DuplicateExportFinding, UnlistedDependencyFinding};
    use fallow_types::results::{
        DuplicateExport, DuplicateLocation, ImportSite, UnlistedDependency,
    };

    let root = PathBuf::from("/project");
    let mut results = AnalysisResults::default();

    results.unlisted_dependencies.push(
        UnlistedDependencyFinding::with_actions(UnlistedDependency {
            package_name: "chalk".to_string(),
            imported_from: vec![
                ImportSite {
                    path: root.join("src/a.ts"),
                    line: 2,
                    col: 0,
                },
                ImportSite {
                    path: root.join("src/b.ts"),
                    line: 4,
                    col: 0,
                },
            ],
        }),
    );
    results.duplicate_exports.push(
        DuplicateExportFinding::with_actions(DuplicateExport {
            export_name: "Button".to_string(),
            locations: vec![
                DuplicateLocation {
                    path: root.join("src/a.ts"),
                    line: 10,
                    col: 0,
                },
                DuplicateLocation {
                    path: root.join("src/b.ts"),
                    line: 20,
                    col: 0,
                },
            ],
        }),
    );

    assert_eq!(
        build_compact_lines(&results, &root),
        vec![
            "unlisted-dep:src/a.ts:2:chalk".to_owned(),
            "unlisted-dep:src/b.ts:4:chalk".to_owned(),
            "duplicate-export:src/a.ts:10:Button".to_owned(),
            "duplicate-export:src/b.ts:20:Button".to_owned(),
        ]
    );
}
