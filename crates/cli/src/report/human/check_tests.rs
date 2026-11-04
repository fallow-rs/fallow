use super::super::{plain, strip_ansi};
use super::*;
use fallow_config::{RulesConfig, Severity};
use fallow_types::extract::MemberKind;
use fallow_types::results::*;
use std::path::PathBuf;

/// Build sample results including optional deps (extends the shared helper).
fn sample_results(root: &Path) -> AnalysisResults {
    crate::report::test_helpers::sample_results(root)
}

#[test]
fn cascade_footer_names_the_hidden_count_and_the_flag() {
    let mut results = AnalysisResults::default();
    assert_eq!(cascade_footer(&results), None);

    results.cascade_hidden = 1;
    assert_eq!(
        cascade_footer(&results).as_deref(),
        Some("1 finding in unused files is hidden \u{00b7} --show-cascade shows it")
    );

    results.cascade_hidden = 7;
    assert_eq!(
        cascade_footer(&results).as_deref(),
        Some("7 findings in unused files are hidden \u{00b7} --show-cascade shows them")
    );
}

#[test]
fn empty_results_produce_no_lines() {
    let root = PathBuf::from("/project");
    let results = AnalysisResults::default();
    let rules = RulesConfig::default();
    let lines = build_human_lines(&results, &root, &rules, None);
    assert!(lines.is_empty());
}

#[test]
fn truncation_hint_suggests_scoping_for_large_sections_or_totals() {
    assert!(truncation_hint(10, 10).contains("--format json"));
    assert!(truncation_hint(SCOPING_HINT_THRESHOLD + 1, 10).contains("--workspace"));
    assert!(truncation_hint(10, SCOPING_HINT_THRESHOLD + 1).contains("--changed-since"));
}

#[test]
fn insert_test_src_split_only_reports_meaningful_mixed_sections() {
    let src_a = PathBuf::from("src/a.ts");
    let src_b = PathBuf::from("src/b.ts");
    let src_c = PathBuf::from("src/c.ts");
    let src_d = PathBuf::from("src/d.ts");
    let test_a = PathBuf::from("tests/a.test.ts");
    let test_b = PathBuf::from("tests/b.test.ts");
    let fixture = PathBuf::from("__fixtures__/sample.ts");

    let mut small = vec!["section".to_string(), String::new()];
    insert_test_src_split(
        &mut small,
        &[src_a.clone(), test_a.clone()],
        Path::new("/project"),
        PathBuf::as_path,
    );
    assert_eq!(small, vec!["section".to_string(), String::new()]);

    let mut mostly_src = vec!["section".to_string(), String::new()];
    insert_test_src_split(
        &mut mostly_src,
        &[src_a.clone(), src_b.clone(), src_c, src_d, test_a.clone()],
        Path::new("/project"),
        PathBuf::as_path,
    );
    assert_eq!(mostly_src, vec!["section".to_string(), String::new()]);

    let mut all_tests = vec!["section".to_string(), String::new()];
    insert_test_src_split(
        &mut all_tests,
        &[
            test_a.clone(),
            test_b.clone(),
            fixture.clone(),
            test_a.clone(),
            test_b.clone(),
        ],
        Path::new("/project"),
        PathBuf::as_path,
    );
    assert_eq!(all_tests, vec!["section".to_string(), String::new()]);

    let mut mixed = vec!["section".to_string()];
    insert_test_src_split(
        &mut mixed,
        &[src_a, src_b, test_a, test_b, fixture],
        Path::new("/project"),
        PathBuf::as_path,
    );
    assert!(plain(&mixed).contains("2 in src, 3 in test files"));
}

#[test]
fn insert_test_src_split_ignores_directories_above_the_root() {
    let root = PathBuf::from("/home/ci/tests/app");
    let items = [
        root.join("src/a.ts"),
        root.join("src/b.ts"),
        root.join("src/c.ts"),
        root.join("src/a.test.ts"),
        root.join("tests/b.ts"),
    ];

    let mut lines = vec!["section".to_string()];
    insert_test_src_split(&mut lines, &items, &root, PathBuf::as_path);

    assert!(plain(&lines).contains("3 in src, 2 in test files"));
}

#[test]
fn collect_matching_rules_covers_dependency_and_component_health_findings() {
    // `--group-by owner` puts these findings in an owner group, so the
    // "matched by" header must name the rule that put them there.
    let root = PathBuf::from("/project");
    let resolver = OwnershipResolver::Owner(
        crate::codeowners::CodeOwners::parse("/packages/app/ @app\n/packages/ui/ @ui\n").unwrap(),
    );

    let mut deps = AnalysisResults::default();
    deps.unused_dependencies
        .push(UnusedDependencyFinding::with_actions(UnusedDependency {
            package_name: "lodash".to_string(),
            location: fallow_types::results::DependencyLocation::Dependencies,
            path: root.join("packages/app/package.json"),
            line: 5,
            used_in_workspaces: Vec::new(),
            declared_and_imported_in: Vec::new(),
        }));
    assert_eq!(
        collect_matching_rules(&deps, &root, &resolver),
        vec!["/packages/app/".to_string()]
    );

    let mut health = AnalysisResults::default();
    health
        .thin_wrappers
        .push(ThinWrapperFinding::with_actions(ThinWrapper {
            file: root.join("packages/ui/Wrapper.tsx"),
            line: 3,
            component: "Wrapper".to_string(),
            child_component: "Child".to_string(),
        }));
    assert_eq!(
        collect_matching_rules(&health, &root, &resolver),
        vec!["/packages/ui/".to_string()]
    );
}

#[test]
fn collect_matching_rules_routes_mixed_client_server_barrels() {
    // A file whose ONLY finding is a mixed-client-server-barrel must still
    // surface its CODEOWNERS rule in the `--group-by owner` "matched by"
    // header. Reverting the `mixed_client_server_barrels` loop in
    // `collect_matching_rules` makes this assertion fail (empty rules),
    // pinning the fix that was previously missing alongside the sibling
    // invalid-client-export / misplaced-directive loops.
    let root = PathBuf::from("/project");
    let mut results = AnalysisResults::default();
    results
        .mixed_client_server_barrels
        .push(MixedClientServerBarrelFinding::with_actions(
            MixedClientServerBarrel {
                path: root.join("src/index.ts"),
                client_origin: "./Button".to_string(),
                server_origin: "./fetchUser".to_string(),
                line: 2,
                col: 0,
            },
        ));
    let resolver = OwnershipResolver::Owner(
        crate::codeowners::CodeOwners::parse("/src/ @frontend\n").unwrap(),
    );
    let matched = collect_matching_rules(&results, &root, &resolver);
    assert!(
        matched.iter().any(|r| r.contains("src")),
        "mixed-barrel path must route through the ownership resolver, got: {matched:?}"
    );
}

#[test]
fn unused_file_lines_normalize_backslash_separators() {
    // A Windows path reaches the renderer with native separators. The human
    // report is read by people and matched by tooling, so it must show the
    // same forward-slash form on every platform.
    let root = PathBuf::from("/project");
    let mut results = AnalysisResults::default();
    results
        .unused_files
        .push(UnusedFileFinding::with_actions(UnusedFile {
            path: root.join("src\\dead.ts"),
        }));
    let rules = RulesConfig::default();
    let text = super::super::plain(&build_human_lines(&results, &root, &rules, None));

    assert!(
        text.contains("src/dead.ts"),
        "unused-file lines must use forward slashes: {text}"
    );
    assert!(
        !text.contains("src\\dead.ts"),
        "unused-file lines must not leak native separators: {text}"
    );
}

#[test]
fn section_headers_contain_title_and_count() {
    let root = PathBuf::from("/project");
    let results = sample_results(&root);
    let rules = RulesConfig::default();
    let lines = build_human_lines(&results, &root, &rules, None);
    let text = plain(&lines);

    assert!(text.contains("Unused files (1)"));
    assert!(text.contains("Unused exports (1)"));
    assert!(text.contains("Unused type exports (1)"));
    assert!(text.contains("Unused dependencies (1)"));
    assert!(text.contains("Unused devDependencies (1)"));
    assert!(text.contains("Unused optionalDependencies (1)"));
    assert!(text.contains("Unused enum members (1)"));
    assert!(text.contains("Unused class members (1)"));
    assert!(text.contains("Unresolved imports (1)"));
    assert!(text.contains("Unlisted dependencies (1)"));
    assert!(text.contains("Duplicate exports (1)"));
    assert!(text.contains("Type-only dependencies (consider moving to devDependencies) (1)"));
    assert!(text.contains("Circular dependencies (1)"));
}

#[test]
fn section_header_shows_correct_count_for_multiple_items() {
    let root = PathBuf::from("/project");
    let mut results = AnalysisResults::default();
    for i in 0..5 {
        results
            .unused_files
            .push(UnusedFileFinding::with_actions(UnusedFile {
                path: root.join(format!("src/dead{i}.ts")),
            }));
    }
    let rules = RulesConfig::default();
    let lines = build_human_lines(&results, &root, &rules, None);
    let text = plain(&lines);
    assert!(text.contains("Unused files (5)"));
}

#[test]
fn boundary_coverage_alone_renders_structure_section() {
    let root = PathBuf::from("/project");
    let mut results = AnalysisResults::default();
    results
        .boundary_coverage_violations
        .push(BoundaryCoverageViolationFinding::with_actions(
            BoundaryCoverageViolation {
                path: root.join("src/middleware/error.ts"),
                line: 1,
                col: 0,
            },
        ));

    let lines = build_human_lines(&results, &root, &RulesConfig::default(), None);
    let text = plain(&lines);

    assert!(text.contains("Structure"));
    assert!(text.contains("Boundary coverage (1)"));
    assert!(text.contains("src/middleware/error.ts:1"));
}

#[test]
fn boundary_violation_through_a_barrel_names_the_barrel() {
    let root = PathBuf::from("/project");
    let mut results = AnalysisResults::default();
    results
        .boundary_violations
        .push(BoundaryViolationFinding::with_actions(BoundaryViolation {
            from_path: root.join("src/ui/Button.ts"),
            to_path: root.join("src/core/named.ts"),
            from_zone: "ui".to_string(),
            to_zone: "core".to_string(),
            import_specifier: "src/core/named.ts".to_string(),
            line: 1,
            col: 9,
            via_path: Some(root.join("src/shared/index.ts")),
        }));

    let lines = build_human_lines(&results, &root, &RulesConfig::default(), None);
    let text = plain(&lines);

    assert!(
            text.contains(
                "src/ui/Button.ts:1 \u{2192} src/core/named.ts (ui \u{2192} core) via src/shared/index.ts"
            ),
            "got: {text}"
        );
}

#[test]
fn boundary_calls_alone_render_structure_section() {
    let root = PathBuf::from("/project");
    let mut results = AnalysisResults::default();
    results
        .boundary_call_violations
        .push(BoundaryCallViolationFinding::with_actions(
            BoundaryCallViolation {
                path: root.join("src/domain/policy.ts"),
                line: 5,
                col: 2,
                zone: "domain".to_string(),
                callee: "execSync".to_string(),
                pattern: "child_process.*".to_string(),
            },
        ));

    let lines = build_human_lines(&results, &root, &RulesConfig::default(), None);
    let text = plain(&lines);

    assert!(text.contains("Structure"));
    assert!(text.contains("Boundary calls (1)"));
    assert!(text.contains("src/domain/policy.ts:5"));
    assert!(text.contains("execSync"));
    assert!(text.contains("child_process.*"));
    assert!(text.contains("zone 'domain'"));
    // The rule id is boundary-call-violation but the working token is the
    // family token; the section must teach the literal token.
    assert!(text.contains("// fallow-ignore-next-line boundary-violation"));
}

#[test]
fn policy_violations_render_policy_section_with_message() {
    let root = PathBuf::from("/project");
    let mut results = AnalysisResults::default();
    results
        .policy_violations
        .push(PolicyViolationFinding::with_actions(PolicyViolation {
            path: root.join("src/app.ts"),
            line: 7,
            col: 2,
            pack: "team-policy".to_string(),
            rule_id: "no-moment".to_string(),
            kind: fallow_types::results::PolicyRuleKind::BannedImport,
            matched: "moment/locale/nl".to_string(),
            severity: fallow_types::results::PolicyViolationSeverity::Error,
            message: Some("Use date-fns.".to_string()),
        }));

    let lines = build_human_lines(&results, &root, &RulesConfig::default(), None);
    let text = plain(&lines);

    assert!(text.contains("Policy"));
    assert!(text.contains("Policy violations (1)"));
    assert!(text.contains("src/app.ts:7"));
    assert!(text.contains("moment/locale/nl"));
    assert!(text.contains("team-policy/no-moment"));
    assert!(text.contains("Use date-fns."));
    assert!(text.contains("fallow-ignore-next-line policy-violation:<pack>/<rule-id>"));
}

#[test]
fn unused_files_show_relative_paths() {
    let root = PathBuf::from("/project");
    let mut results = AnalysisResults::default();
    results
        .unused_files
        .push(UnusedFileFinding::with_actions(UnusedFile {
            path: root.join("src/components/Button.tsx"),
        }));
    let rules = RulesConfig::default();
    let lines = build_human_lines(&results, &root, &rules, None);
    let text = plain(&lines);
    assert!(text.contains("src/components/Button.tsx"));
    assert!(!text.contains("/project/"));
}

#[test]
fn unused_files_show_src_test_split_when_mixed() {
    let root = PathBuf::from("/project");
    let mut results = AnalysisResults::default();
    for path in [
        "src/dead-a.ts",
        "src/dead-b.ts",
        "tests/dead-a.test.ts",
        "tests/dead-b.test.ts",
        "__fixtures__/dead-fixture.ts",
    ] {
        results
            .unused_files
            .push(UnusedFileFinding::with_actions(UnusedFile {
                path: root.join(path),
            }));
    }
    let rules = RulesConfig::default();
    let lines = build_human_lines(&results, &root, &rules, None);
    let text = plain(&lines);

    assert!(text.contains("2 in src, 3 in test files"));
}

/// The `source-parse-degraded` diagnostic sits at the top of the JSON
/// envelope, and the human report never showed it next to the finding it
/// distorts. A person reading `src/helper.ts` under a `delete-file`
/// heading has to see that the verdict rests on an incomplete parse.
#[test]
fn a_caveated_finding_carries_a_compact_suffix_in_the_human_report() {
    let root = PathBuf::from("/project");
    let mut results = AnalysisResults::default();
    let mut file = UnusedFileFinding::with_actions(UnusedFile {
        path: root.join("src/helper.ts"),
    });
    file.reachability_caveats = vec![ReachabilityCaveat::IncompleteImportGraph];
    results.unused_files.push(file);

    let mut export = UnusedExportFinding::with_actions(UnusedExport {
        path: root.join("src/lib.ts"),
        export_name: "needed".to_string(),
        is_type_only: false,
        line: 1,
        col: 13,
        span_start: 13,
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
        package_name: "lodash".to_string(),
        location: fallow_types::results::DependencyLocation::Dependencies,
        path: root.join("package.json"),
        line: 5,
        used_in_workspaces: Vec::new(),
        declared_and_imported_in: Vec::new(),
    });
    dep.reachability_caveats = vec![ReachabilityCaveat::IncompleteImportGraph];
    results.unused_dependencies.push(dep);

    // All three member arrays are stamped by the same reachability-free
    // access walk, so all three lines have to hedge.
    let member = |parent: &str, name: &str, kind| UnusedMember {
        path: root.join("src/lib.ts"),
        parent_name: parent.to_string(),
        member_name: name.to_string(),
        kind,
        line: 7,
        col: 2,
    };
    let mut enum_member =
        UnusedEnumMemberFinding::with_actions(member("Mode", "Legacy", MemberKind::EnumMember));
    enum_member.reachability_caveats = vec![ReachabilityCaveat::IncompleteImportGraph];
    results.unused_enum_members.push(enum_member);

    let mut class_member =
        UnusedClassMemberFinding::with_actions(member("Widget", "render", MemberKind::ClassMethod));
    class_member.reachability_caveats = vec![ReachabilityCaveat::IncompleteImportGraph];
    results.unused_class_members.push(class_member);

    let mut store_member = UnusedStoreMemberFinding::with_actions(member(
        "useCart",
        "subtotal",
        MemberKind::StoreMember,
    ));
    store_member.reachability_caveats = vec![ReachabilityCaveat::IncompleteImportGraph];
    results.unused_store_members.push(store_member);

    let rules = RulesConfig::default();
    let text = plain(&build_human_lines(&results, &root, &rules, None));

    assert!(
        text.contains("src/helper.ts (caveat: incomplete import graph)"),
        "the unused-file line must name the caveat: {text}"
    );
    assert!(
        text.contains(":1 needed (caveat: incomplete file analysis, incomplete import graph)"),
        "the unused-export line must name both caveats: {text}"
    );
    assert!(
        text.contains("lodash (caveat: incomplete import graph)"),
        "the dependency line must name the caveat: {text}"
    );
    for expected in [
        "Mode.Legacy (caveat: incomplete import graph)",
        "Widget.render (caveat: incomplete import graph)",
        "useCart.subtotal (caveat: incomplete import graph)",
    ] {
        assert!(
            text.contains(expected),
            "every member line must name the caveat, missing {expected}: {text}"
        );
    }
}

/// `--summary` prints category counts with no per-finding lines, so a CI
/// job that captures only this output would otherwise lose the caveat the
/// full report carries. It is stated once under the totals instead.
#[test]
fn the_summary_view_states_the_caveat_under_the_totals() {
    let root = PathBuf::from("/project");
    let mut results = AnalysisResults::default();
    let mut file = UnusedFileFinding::with_actions(UnusedFile {
        path: root.join("src/helper.ts"),
    });
    file.reachability_caveats = vec![ReachabilityCaveat::IncompleteImportGraph];
    results.unused_files.push(file);
    let mut dep = UnusedDependencyFinding::with_actions(UnusedDependency {
        package_name: "lodash".to_string(),
        location: fallow_types::results::DependencyLocation::Dependencies,
        path: root.join("package.json"),
        line: 5,
        used_in_workspaces: Vec::new(),
        declared_and_imported_in: Vec::new(),
    });
    dep.reachability_caveats = vec![ReachabilityCaveat::IncompleteImportGraph];
    results.unused_dependencies.push(dep);

    assert_eq!(
        check_summary_caveat_note(&results).as_deref(),
        Some("(2 of these carry a caveat: incomplete import graph)")
    );
}

/// The rollup walks every stamped array, and two of them arrived through
/// separate merges. Counting a caveated type or enum member once per
/// chained iterator made the footnote over-report: on a fixture with one
/// caveated entry per array it claimed more than it printed. "N of these"
/// is only checkable against the full report while N is the number of
/// findings the report prints, so this pins one count per finding, with a
/// caveated entry in every array the rollup walks.
#[test]
fn the_summary_rollup_counts_each_caveated_finding_exactly_once() {
    let root = PathBuf::from("/project");
    let mut results = AnalysisResults::default();
    let caveat = vec![ReachabilityCaveat::IncompleteImportGraph];

    let mut file = UnusedFileFinding::with_actions(UnusedFile {
        path: root.join("src/helper.ts"),
    });
    file.reachability_caveats.clone_from(&caveat);
    results.unused_files.push(file);

    let export = |name: &str| UnusedExport {
        path: root.join("src/lib.ts"),
        export_name: name.to_string(),
        is_type_only: false,
        line: 1,
        col: 13,
        span_start: 13,
        is_re_export: false,
        deprecated: false,
        deprecated_reason: None,
    };

    let mut unused_export = UnusedExportFinding::with_actions(export("needed"));
    unused_export.reachability_caveats.clone_from(&caveat);
    results.unused_exports.push(unused_export);

    let mut unused_type = UnusedTypeFinding::with_actions(export("Shape"));
    unused_type.reachability_caveats.clone_from(&caveat);
    results.unused_types.push(unused_type);

    let mut member = UnusedEnumMemberFinding::with_actions(UnusedMember {
        path: root.join("src/enums.ts"),
        parent_name: "Color".to_string(),
        member_name: "Purple".to_string(),
        kind: MemberKind::EnumMember,
        line: 5,
        col: 2,
    });
    member.reachability_caveats.clone_from(&caveat);
    results.unused_enum_members.push(member);

    let mut class_member = UnusedClassMemberFinding::with_actions(UnusedMember {
        path: root.join("src/widget.ts"),
        parent_name: "Widget".to_string(),
        member_name: "render".to_string(),
        kind: MemberKind::ClassMethod,
        line: 4,
        col: 2,
    });
    class_member.reachability_caveats.clone_from(&caveat);
    results.unused_class_members.push(class_member);

    let mut store_member = UnusedStoreMemberFinding::with_actions(UnusedMember {
        path: root.join("src/cart.ts"),
        parent_name: "useCart".to_string(),
        member_name: "subtotal".to_string(),
        kind: MemberKind::StoreMember,
        line: 6,
        col: 2,
    });
    store_member.reachability_caveats.clone_from(&caveat);
    results.unused_store_members.push(store_member);

    let dependency = |name: &str, location| UnusedDependency {
        package_name: name.to_string(),
        location,
        path: root.join("package.json"),
        line: 5,
        used_in_workspaces: Vec::new(),
        declared_and_imported_in: Vec::new(),
    };

    let mut prod = UnusedDependencyFinding::with_actions(dependency(
        "lodash",
        fallow_types::results::DependencyLocation::Dependencies,
    ));
    prod.reachability_caveats.clone_from(&caveat);
    results.unused_dependencies.push(prod);

    let mut dev = UnusedDevDependencyFinding::with_actions(dependency(
        "chalk",
        fallow_types::results::DependencyLocation::DevDependencies,
    ));
    dev.reachability_caveats.clone_from(&caveat);
    results.unused_dev_dependencies.push(dev);

    let mut optional = UnusedOptionalDependencyFinding::with_actions(dependency(
        "fsevents",
        fallow_types::results::DependencyLocation::OptionalDependencies,
    ));
    optional.reachability_caveats.clone_from(&caveat);
    results.unused_optional_dependencies.push(optional);

    assert_eq!(
        check_summary_caveat_note(&results).as_deref(),
        Some("(9 of these carry a caveat: incomplete import graph)")
    );

    // Every finding above carries exactly one caveat, which makes "one per
    // finding" and "one per caveat token" indistinguishable. A real run
    // produces both tokens on one finding whenever the finding's own file
    // is the degraded one, so stamp that shape and hold the same total:
    // the count is of findings, and a finding with two caveats is still
    // one line in the report the reader checks it against.
    results.unused_files[0].reachability_caveats = vec![
        ReachabilityCaveat::IncompleteFileAnalysis,
        ReachabilityCaveat::IncompleteImportGraph,
    ];
    assert_eq!(
        check_summary_caveat_note(&results).as_deref(),
        Some("(9 of these carry a caveat: incomplete file analysis, incomplete import graph)"),
        "a finding carrying both caveats must still be counted once"
    );
}

/// Silent on a run that read every file it discovered, so the default
/// `--summary` output stays byte-identical.
#[test]
fn the_summary_view_stays_silent_without_a_caveat() {
    let root = PathBuf::from("/project");
    let mut results = AnalysisResults::default();
    results
        .unused_files
        .push(UnusedFileFinding::with_actions(UnusedFile {
            path: root.join("src/helper.ts"),
        }));

    assert_eq!(check_summary_caveat_note(&results), None);
}

/// A project that parses cleanly must render exactly as before.
#[test]
fn an_uncaveated_finding_renders_without_a_suffix() {
    let root = PathBuf::from("/project");
    let mut results = AnalysisResults::default();
    results
        .unused_files
        .push(UnusedFileFinding::with_actions(UnusedFile {
            path: root.join("src/helper.ts"),
        }));
    results
        .unused_dependencies
        .push(UnusedDependencyFinding::with_actions(UnusedDependency {
            package_name: "lodash".to_string(),
            location: fallow_types::results::DependencyLocation::Dependencies,
            path: root.join("package.json"),
            line: 5,
            used_in_workspaces: Vec::new(),
            declared_and_imported_in: Vec::new(),
        }));

    let rules = RulesConfig::default();
    let text = plain(&build_human_lines(&results, &root, &rules, None));

    assert!(
        !text.contains("caveat"),
        "clean output must not change: {text}"
    );
}

/// Above the rollup threshold the per-file lines collapse into directory
/// counts, so the caveat is stated once for the section instead of being
/// silently dropped.
#[test]
fn the_directory_rollup_states_the_caveat_once() {
    let root = PathBuf::from("/project");
    let mut results = AnalysisResults::default();
    for index in 0..=DIR_ROLLUP_THRESHOLD {
        let mut file = UnusedFileFinding::with_actions(UnusedFile {
            path: root.join(format!("src/dead{index}.ts")),
        });
        if index % 2 == 0 {
            file.reachability_caveats = vec![ReachabilityCaveat::IncompleteImportGraph];
        }
        results.unused_files.push(file);
    }
    let expected = results
        .unused_files
        .iter()
        .filter(|finding| !finding.reachability_caveats.is_empty())
        .count();

    let rules = RulesConfig::default();
    let text = plain(&build_human_lines(&results, &root, &rules, None));

    assert!(
        text.contains(&format!(
            "({expected} of these carry a caveat: incomplete import graph)"
        )),
        "the rollup must still surface the caveat: {text}"
    );
}

/// The rollup labels a directory entry with a trailing slash. A file that
/// sits directly in the dominant directory has no subdirectory to group
/// under, and the split read its second path component regardless, so a
/// file name was labelled as though it were a directory: `src/orphan.ts/`.
/// It belongs to the dominant directory itself.
#[test]
fn the_dominant_directory_split_never_labels_a_file_as_a_directory() {
    let root = PathBuf::from("/project");
    let mut results = AnalysisResults::default();
    for index in 0..DIR_ROLLUP_THRESHOLD {
        results
            .unused_files
            .push(UnusedFileFinding::with_actions(UnusedFile {
                path: root.join(format!("src/components/dead{index}.ts")),
            }));
    }
    results
        .unused_files
        .push(UnusedFileFinding::with_actions(UnusedFile {
            path: root.join("src/orphan.ts"),
        }));

    let rules = RulesConfig::default();
    let text = plain(&build_human_lines(&results, &root, &rules, None));

    assert!(
        !text.contains("src/orphan.ts/"),
        "a file must never be rendered as a directory: {text}"
    );
    assert!(
        text.contains("src/  1 file"),
        "the loose file counts under the directory it lives in: {text}"
    );
}

#[test]
fn unused_exports_grouped_by_file_with_line_and_name() {
    let root = PathBuf::from("/project");
    let mut results = AnalysisResults::default();
    results
        .unused_exports
        .push(UnusedExportFinding::with_actions(UnusedExport {
            path: root.join("src/utils.ts"),
            export_name: "helperFn".to_string(),
            is_type_only: false,
            line: 10,
            col: 4,
            span_start: 120,
            is_re_export: false,
            deprecated: false,
            deprecated_reason: None,
        }));
    results
        .unused_exports
        .push(UnusedExportFinding::with_actions(UnusedExport {
            path: root.join("src/utils.ts"),
            export_name: "anotherFn".to_string(),
            is_type_only: false,
            line: 25,
            col: 0,
            span_start: 300,
            is_re_export: false,
            deprecated: false,
            deprecated_reason: None,
        }));
    let rules = RulesConfig::default();
    let lines = build_human_lines(&results, &root, &rules, None);
    let text = plain(&lines);

    assert!(text.contains("Unused exports (2)"));
    assert!(text.contains("src/utils.ts"));
    assert!(text.contains(":10 helperFn"));
    assert!(text.contains(":25 anotherFn"));
}

#[test]
fn re_exports_are_tagged() {
    let root = PathBuf::from("/project");
    let mut results = AnalysisResults::default();
    results
        .unused_exports
        .push(UnusedExportFinding::with_actions(UnusedExport {
            path: root.join("src/index.ts"),
            export_name: "reExported".to_string(),
            is_type_only: false,
            line: 1,
            col: 0,
            span_start: 0,
            is_re_export: true,
            deprecated: false,
            deprecated_reason: None,
        }));
    let rules = RulesConfig::default();
    let lines = build_human_lines(&results, &root, &rules, None);
    let text = plain(&lines);
    assert!(text.contains("(re-export)"));
}

#[test]
fn non_re_exports_have_no_tag() {
    let root = PathBuf::from("/project");
    let mut results = AnalysisResults::default();
    results
        .unused_exports
        .push(UnusedExportFinding::with_actions(UnusedExport {
            path: root.join("src/utils.ts"),
            export_name: "helper".to_string(),
            is_type_only: false,
            line: 1,
            col: 0,
            span_start: 0,
            is_re_export: false,
            deprecated: false,
            deprecated_reason: None,
        }));
    let rules = RulesConfig::default();
    let lines = build_human_lines(&results, &root, &rules, None);
    let text = plain(&lines);
    assert!(!text.contains("(re-export)"));
}

#[test]
fn unused_enum_members_show_parent_dot_member() {
    let root = PathBuf::from("/project");
    let mut results = AnalysisResults::default();
    results
        .unused_enum_members
        .push(UnusedEnumMemberFinding::with_actions(UnusedMember {
            path: root.join("src/enums.ts"),
            parent_name: "Color".to_string(),
            member_name: "Purple".to_string(),
            kind: MemberKind::EnumMember,
            line: 5,
            col: 2,
        }));
    let rules = RulesConfig::default();
    let lines = build_human_lines(&results, &root, &rules, None);
    let text = plain(&lines);
    assert!(text.contains("Color.Purple"));
    assert!(text.contains(":5"));
}

#[test]
fn unused_class_members_show_parent_dot_member() {
    let root = PathBuf::from("/project");
    let mut results = AnalysisResults::default();
    results
        .unused_class_members
        .push(UnusedClassMemberFinding::with_actions(UnusedMember {
            path: root.join("src/service.ts"),
            parent_name: "ApiService".to_string(),
            member_name: "disconnect".to_string(),
            kind: MemberKind::ClassMethod,
            line: 99,
            col: 4,
        }));
    let rules = RulesConfig::default();
    let lines = build_human_lines(&results, &root, &rules, None);
    let text = plain(&lines);
    assert!(text.contains("ApiService.disconnect"));
    assert!(text.contains(":99"));
}

#[test]
fn unused_deps_at_root_show_package_name_only() {
    let root = PathBuf::from("/project");
    let mut results = AnalysisResults::default();
    results
        .unused_dependencies
        .push(UnusedDependencyFinding::with_actions(UnusedDependency {
            package_name: "lodash".to_string(),
            location: DependencyLocation::Dependencies,
            path: root.join("package.json"),
            line: 5,
            used_in_workspaces: Vec::new(),
            declared_and_imported_in: Vec::new(),
        }));
    let rules = RulesConfig::default();
    let lines = build_human_lines(&results, &root, &rules, None);
    let text = plain(&lines);
    assert!(text.contains("lodash"));
    assert!(!text.contains("(package.json)"));
}

#[test]
fn unused_deps_in_workspace_show_workspace_path() {
    let root = PathBuf::from("/project");
    let mut results = AnalysisResults::default();
    results
        .unused_dependencies
        .push(UnusedDependencyFinding::with_actions(UnusedDependency {
            package_name: "axios".to_string(),
            location: DependencyLocation::Dependencies,
            path: root.join("packages/web/package.json"),
            line: 8,
            used_in_workspaces: Vec::new(),
            declared_and_imported_in: Vec::new(),
        }));
    let rules = RulesConfig::default();
    let lines = build_human_lines(&results, &root, &rules, None);
    let text = plain(&lines);
    assert!(text.contains("axios"));
    assert!(text.contains("(packages/web/package.json)"));
}

#[test]
fn unused_deps_show_cross_workspace_context() {
    let root = PathBuf::from("/project");
    let mut results = AnalysisResults::default();
    results
        .unused_dependencies
        .push(UnusedDependencyFinding::with_actions(UnusedDependency {
            package_name: "lodash-es".to_string(),
            location: DependencyLocation::Dependencies,
            path: root.join("packages/shared/package.json"),
            line: 8,
            used_in_workspaces: vec![root.join("packages/consumer")],
            declared_and_imported_in: Vec::new(),
        }));
    let rules = RulesConfig::default();
    let lines = build_human_lines(&results, &root, &rules, None);
    let text = plain(&lines);
    assert!(text.contains("lodash-es"));
    assert!(text.contains("packages/shared/package.json; imported in packages/consumer"));
}

#[test]
fn unused_root_dep_with_cross_workspace_context_uses_context_label() {
    let root = PathBuf::from("/project");
    let mut results = AnalysisResults::default();
    results
        .unused_dependencies
        .push(UnusedDependencyFinding::with_actions(UnusedDependency {
            package_name: "lodash-es".to_string(),
            location: DependencyLocation::Dependencies,
            path: root.join("package.json"),
            line: 8,
            used_in_workspaces: vec![root.join("packages/consumer")],
            declared_and_imported_in: Vec::new(),
        }));
    let rules = RulesConfig::default();
    let lines = build_human_lines(&results, &root, &rules, None);
    let text = plain(&lines);
    assert!(text.contains("lodash-es"));
    assert!(text.contains("(imported in packages/consumer)"));
    assert!(!text.contains("(package.json; imported in packages/consumer)"));
}

#[test]
fn unused_root_dep_names_the_workspaces_that_declare_and_import_it() {
    let root = PathBuf::from("/project");
    let mut results = AnalysisResults::default();
    results
        .unused_dependencies
        .push(UnusedDependencyFinding::with_actions(UnusedDependency {
            package_name: "lodash".to_string(),
            location: DependencyLocation::Dependencies,
            path: root.join("package.json"),
            line: 8,
            used_in_workspaces: Vec::new(),
            declared_and_imported_in: vec![root.join("packages/app")],
        }));
    let rules = RulesConfig::default();
    let lines = build_human_lines(&results, &root, &rules, None);
    let text = plain(&lines);
    assert!(
        text.contains("lodash (package.json; declared and imported in packages/app)"),
        "{text}"
    );
}

#[test]
fn unused_dep_with_many_workspaces_holds_80_columns() {
    let root = PathBuf::from("/project");
    let manifest = root.join("packages/design-tokens/package.json");
    let workspaces = vec![
        root.join("packages/web-application"),
        root.join("packages/mobile-application"),
        root.join("packages/documentation-site"),
    ];
    let name = "@acme/tokens";
    let natural: usize = name.chars().count()
        + DEP_LINE_DECORATION
        + relative_path(&manifest, &root)
            .display()
            .to_string()
            .chars()
            .count()
        + IMPORTED_IN.chars().count()
        + workspaces
            .iter()
            .map(|path| {
                relative_path(path, &root)
                    .display()
                    .to_string()
                    .chars()
                    .count()
                    + 2
            })
            .sum::<usize>();
    // Unelided, this line runs well past the ceiling it has to hold.
    // The bound is a literal: written as DEP_LINE_WIDTH + 30 it would
    // travel with the constant under test and guard nothing.
    assert!(natural > 110, "{natural}");
    let mut results = AnalysisResults::default();
    results
        .unused_dependencies
        .push(UnusedDependencyFinding::with_actions(UnusedDependency {
            package_name: name.to_string(),
            location: DependencyLocation::Dependencies,
            path: manifest,
            line: 8,
            used_in_workspaces: workspaces,
            declared_and_imported_in: Vec::new(),
        }));
    let rules = RulesConfig::default();
    let lines = build_human_lines(&results, &root, &rules, None);
    let text = plain(&lines);
    let rendered = text
        .lines()
        .find(|line| line.contains(name))
        .expect("dependency line rendered");
    // Eighty columns is the contract with the terminal, not a restatement
    // of DEP_LINE_WIDTH, so it is asserted as a literal. The fixture is
    // sized to land on the ceiling exactly, which is what makes widening
    // the constant fail here instead of passing while the output overruns.
    assert_eq!(rendered.chars().count(), 80, "{rendered}");
    // The declaring package survives the elision, and the workspaces the
    // clause could not seat are disclosed rather than dropped.
    assert!(rendered.contains("design-tokens/package.json"));
    assert!(rendered.contains("imported in"));
    assert!(rendered.contains("+2 more"));
}

#[test]
fn a_caveated_dep_line_keeps_its_caveat_inside_80_columns() {
    let root = PathBuf::from("/project");
    let name = "react-native-gzip";
    let mut dep = UnusedDependencyFinding::with_actions(UnusedDependency {
        package_name: name.to_string(),
        location: DependencyLocation::Dependencies,
        path: root.join("packages/mobile-application/package.json"),
        line: 8,
        used_in_workspaces: Vec::new(),
        declared_and_imported_in: Vec::new(),
    });
    dep.reachability_caveats = vec![ReachabilityCaveat::IncompleteImportGraph];
    let mut results = AnalysisResults::default();
    results.unused_dependencies.push(dep);
    let lines = build_human_lines(&results, &root, &RulesConfig::default(), None);
    let text = plain(&lines);
    let rendered = text
        .lines()
        .find(|line| line.contains(name))
        .expect("dependency line rendered");
    // The caveat is appended after the label, so the label budget has to
    // leave room for it. Unreserved, this line renders at 95 columns.
    assert!(
        rendered.chars().count() <= 80,
        "{} columns: {rendered}",
        rendered.chars().count()
    );
    assert!(
        rendered.ends_with("(caveat: incomplete import graph)"),
        "the caveat is never cut: {rendered}"
    );
    assert!(
        rendered.contains("package.json"),
        "the manifest still renders: {rendered}"
    );
}

#[test]
fn dep_label_caps_the_workspace_list_and_keeps_whole_paths() {
    let label = dep_label(
        "packages/tsc/package.json",
        WorkspaceClause::ImportedIn,
        &[
            "packages/bench".to_string(),
            "packages/treeshake".to_string(),
        ],
        DEP_LINE_WIDTH - "valibot".len() - DEP_LINE_DECORATION,
    )
    .expect("label rendered");
    assert_eq!(
        label,
        "packages/tsc/package.json; imported in packages/bench +1 more"
    );
}

#[test]
fn dep_label_elides_a_deep_manifest_on_segment_boundaries() {
    let label = dep_label(
        "packages/platform/internal/tooling/generators/package.json",
        WorkspaceClause::ImportedIn,
        &[],
        40,
    )
    .expect("label rendered");
    assert!(label.chars().count() <= 40, "{label}");
    assert_eq!(label, ".../tooling/generators/package.json");
}

#[test]
fn dep_label_drops_the_clause_that_would_erode_the_manifest_path() {
    let label = dep_label(
        "packages/platform/design-system/package.json",
        WorkspaceClause::ImportedIn,
        &["packages/platform/web-application".to_string()],
        DEP_LINE_WIDTH - "@internal/design-system-tokens".len() - DEP_LINE_DECORATION,
    )
    .expect("label rendered");
    // Seating the clause would leave `.../package.json`, which names no
    // package, so the path takes the label instead.
    assert_eq!(label, "packages/platform/design-system/package.json");
}

#[test]
fn root_dep_label_without_room_for_the_declaring_workspaces_stays_bare() {
    let workspaces = ["packages/app".to_string()];
    // The root manifest alone says nothing, so it renders only with the
    // clause that it introduces.
    assert_eq!(
        dep_label(
            ROOT_MANIFEST,
            WorkspaceClause::DeclaredAndImportedIn,
            &workspaces,
            31
        ),
        None
    );
    assert_eq!(
        dep_label(
            ROOT_MANIFEST,
            WorkspaceClause::DeclaredAndImportedIn,
            &workspaces,
            60
        )
        .as_deref(),
        Some("package.json; declared and imported in packages/app")
    );
}

#[test]
fn dep_label_is_dropped_when_the_package_name_consumes_the_line() {
    let manifest = "packages/platform/design-system/package.json";
    let workspaces = ["packages/platform/web-application".to_string()];
    // Sixteen columns is the width of a manifest elided to its file name,
    // the narrowest label that still names something. Both budgets are
    // literals: as MIN_LABEL_WIDTH +/- 1 they would follow the constant
    // and leave the boundary it exists to express unpinned.
    assert_eq!(
        dep_label(manifest, WorkspaceClause::ImportedIn, &workspaces, 15),
        None
    );
    assert_eq!(
        dep_label(manifest, WorkspaceClause::ImportedIn, &workspaces, 16).as_deref(),
        Some(".../package.json")
    );
}

#[test]
fn summarize_workspaces_reserves_room_for_the_more_suffix() {
    let workspaces: Vec<String> = ["a/bb", "c/ddddddddddd", "e/ff", "g/hh", "i/jj"]
        .iter()
        .map(|path| (*path).to_string())
        .collect();
    let list = summarize_workspaces(&workspaces, 30);
    // Seating a third path leaves no room for the `+N more` that the two
    // it displaces still need, so the loop stops one earlier and every
    // path it did seat renders whole. Drop the reservation and the joined
    // list overruns, gets elided from the left, and silently loses entries
    // the count is still promising are shown.
    assert_eq!(list, "a/bb, c/ddddddddddd +3 more");
    assert!(list.chars().count() <= 30, "{list}");
}

#[test]
fn summarize_workspaces_elides_a_single_overlong_path() {
    let list = summarize_workspaces(
        &["packages/platform/internal/generators".to_string()],
        MIN_WORKSPACE_LIST_WIDTH,
    );
    assert!(list.chars().count() <= MIN_WORKSPACE_LIST_WIDTH, "{list}");
    assert_eq!(list, ".../generators");
}

#[test]
fn unresolved_imports_show_specifier_and_line() {
    let root = PathBuf::from("/project");
    let mut results = AnalysisResults::default();
    results
        .unresolved_imports
        .push(UnresolvedImportFinding::with_actions(UnresolvedImport {
            path: root.join("src/app.ts"),
            specifier: "@org/missing-pkg".to_string(),
            line: 7,
            col: 0,
            specifier_col: 0,
        }));
    let rules = RulesConfig::default();
    let lines = build_human_lines(&results, &root, &rules, None);
    let text = plain(&lines);
    assert!(text.contains("src/app.ts"));
    assert!(text.contains(":7"));
    assert!(text.contains("@org/missing-pkg"));
}

fn make_dup(name: &str, paths: &[&str]) -> DuplicateExportFinding {
    DuplicateExportFinding::with_actions(DuplicateExport {
        export_name: name.to_string(),
        locations: paths
            .iter()
            .map(|p| DuplicateLocation {
                path: PathBuf::from(p),
                line: 1,
                col: 0,
            })
            .collect(),
    })
}

#[test]
fn is_namespace_barrel_location_matches_documented_extensions() {
    assert!(is_namespace_barrel_location(Path::new(
        "components/ui/button/index.ts"
    )));
    assert!(is_namespace_barrel_location(Path::new(
        "components/ui/button/index.tsx"
    )));
    assert!(is_namespace_barrel_location(Path::new("src/x/index.mjs")));
    assert!(is_namespace_barrel_location(Path::new("src/x/index.cjs")));
    assert!(is_namespace_barrel_location(Path::new("src/x/index.jsx")));
    assert!(is_namespace_barrel_location(Path::new(
        "components/ui/button/index.TS"
    )));
    assert!(is_namespace_barrel_location(Path::new(
        "components/ui/button/index.Tsx"
    )));
}

#[test]
fn is_namespace_barrel_location_rejects_non_index_files() {
    assert!(!is_namespace_barrel_location(Path::new(
        "components/ui/button/Button.ts"
    )));
    assert!(!is_namespace_barrel_location(Path::new(
        "components/ui/button/Index.ts"
    )));
    assert!(!is_namespace_barrel_location(Path::new(
        "components/ui/button/index.svelte"
    )));
    assert!(!is_namespace_barrel_location(Path::new(
        "components/ui/button/index.vue"
    )));
    assert!(!is_namespace_barrel_location(Path::new(
        "components/ui/button/index"
    )));
}

#[test]
fn namespace_barrel_hint_fires_when_4_of_5_findings_match() {
    let items = vec![
        make_dup(
            "Root",
            &["packages/ui/a/index.ts", "packages/ui/b/index.ts"],
        ),
        make_dup(
            "Content",
            &["packages/ui/c/index.ts", "packages/ui/d/index.ts"],
        ),
        make_dup(
            "Trigger",
            &["packages/ui/e/index.ts", "packages/ui/f/index.ts"],
        ),
        make_dup(
            "Item",
            &["packages/ui/g/index.ts", "packages/ui/h/index.ts"],
        ),
        make_dup("Config", &["src/config.ts", "src/types.ts"]),
    ];
    assert!(should_show_namespace_barrel_hint(&items));
}

#[test]
fn namespace_barrel_hint_does_not_fire_when_2_of_5_findings_match() {
    let items = vec![
        make_dup(
            "Root",
            &["packages/ui/a/index.ts", "packages/ui/b/index.ts"],
        ),
        make_dup("Content", &["packages/ui/c/index.ts", "src/types.ts"]),
        make_dup("Trigger", &["src/a.ts", "src/b.ts"]),
        make_dup("Item", &["src/c.ts", "src/d.ts"]),
        make_dup("Config", &["src/config.ts", "src/types.ts"]),
    ];
    assert!(!should_show_namespace_barrel_hint(&items));
}

#[test]
fn namespace_barrel_hint_does_not_fire_below_findings_floor() {
    let items = vec![
        make_dup(
            "Root",
            &["packages/ui/a/index.ts", "packages/ui/b/index.ts"],
        ),
        make_dup(
            "Content",
            &["packages/ui/c/index.ts", "packages/ui/d/index.ts"],
        ),
    ];
    assert!(!should_show_namespace_barrel_hint(&items));
}

#[test]
fn namespace_barrel_hint_fires_when_47_of_47_findings_match() {
    let items: Vec<DuplicateExportFinding> = (0..47)
        .map(|i| {
            let path_a = format!("packages/ui/dir_{i}/index.ts");
            let path_b = format!("packages/ui/other_{i}/index.tsx");
            make_dup(&format!("Sym{i}"), &[path_a.as_str(), path_b.as_str()])
        })
        .collect();
    assert!(should_show_namespace_barrel_hint(&items));
}

#[test]
fn namespace_barrel_hint_skips_single_location_findings_when_computing_ratio() {
    let items = vec![
        make_dup(
            "Root",
            &["packages/ui/a/index.ts", "packages/ui/b/index.ts"],
        ),
        make_dup(
            "Content",
            &["packages/ui/c/index.ts", "packages/ui/d/index.ts"],
        ),
        make_dup(
            "Trigger",
            &["packages/ui/e/index.ts", "packages/ui/f/index.ts"],
        ),
        make_dup("Lonely", &["src/lonely.ts"]),
    ];
    assert!(should_show_namespace_barrel_hint(&items));
}

#[test]
fn duplicate_exports_section_emits_hint_when_gate_passes() {
    let root = PathBuf::from("/project");
    let mut results = AnalysisResults::default();
    for i in 0..4 {
        results.duplicate_exports.push(make_dup(
            &format!("Sym{i}"),
            &[
                &format!("/project/packages/ui/dir_{i}/index.ts"),
                &format!("/project/packages/ui/other_{i}/index.tsx"),
            ],
        ));
    }
    let rules = RulesConfig::default();
    let lines = build_human_lines(&results, &root, &rules, None);
    let text = plain(&lines);
    assert!(
        text.contains("namespace-barrel"),
        "expected hint substring in output: {text}"
    );
}

#[test]
fn duplicate_exports_section_omits_hint_when_gate_fails() {
    let root = PathBuf::from("/project");
    let mut results = AnalysisResults::default();
    results.duplicate_exports.push(make_dup(
        "Sym",
        &[
            "/project/packages/ui/a/index.ts",
            "/project/packages/ui/b/index.ts",
        ],
    ));
    let rules = RulesConfig::default();
    let lines = build_human_lines(&results, &root, &rules, None);
    let text = plain(&lines);
    assert!(
        !text.contains("namespace-barrel"),
        "hint must not fire below the 3-finding floor: {text}"
    );
}

#[test]
fn duplicate_exports_show_name_and_locations() {
    let root = PathBuf::from("/project");
    let mut results = AnalysisResults::default();
    results
        .duplicate_exports
        .push(DuplicateExportFinding::with_actions(DuplicateExport {
            export_name: "Config".to_string(),
            locations: vec![
                DuplicateLocation {
                    path: root.join("src/config.ts"),
                    line: 15,
                    col: 0,
                },
                DuplicateLocation {
                    path: root.join("src/types.ts"),
                    line: 30,
                    col: 0,
                },
            ],
        }));
    let rules = RulesConfig::default();
    let lines = build_human_lines(&results, &root, &rules, None);
    let text = plain(&lines);
    assert!(text.contains("Config"));
    assert!(text.contains("src/config.ts"));
    assert!(text.contains("types.ts"));
}

#[test]
fn circular_dependencies_show_cycle_with_arrow_and_repeat() {
    let root = PathBuf::from("/project");
    let mut results = AnalysisResults::default();
    results
        .circular_dependencies
        .push(CircularDependencyFinding::with_actions(
            CircularDependency {
                files: vec![
                    root.join("src/a.ts"),
                    root.join("src/b.ts"),
                    root.join("src/c.ts"),
                ],
                length: 3,
                line: 1,
                col: 0,
                edges: Vec::new(),
                is_cross_package: false,
            },
        ));
    let rules = RulesConfig::default();
    let lines = build_human_lines(&results, &root, &rules, None);
    let text = plain(&lines);
    assert!(text.contains("a.ts"));
    assert!(text.contains("b.ts"));
    assert!(text.contains("c.ts"));
    assert!(text.contains("\u{2192}"));
}

#[test]
fn package_cycle_in_a_truncated_group_shows_a_note() {
    let root = PathBuf::from("/project");
    let hop = |from: &str, to: &str, file: &str| PackageCycleEdge {
        from_package: from.to_string(),
        to_package: to.to_string(),
        path: root.join(file),
        target_path: root.join(file),
        line: 1,
        col: 0,
        type_only: false,
    };
    let cycle = |group_truncated: bool| {
        PackageCycleFinding::with_actions(PackageCycle {
            packages: vec!["a".to_string(), "b".to_string()],
            package_roots: vec![root.join("packages/a"), root.join("packages/b")],
            length: 2,
            edges: vec![
                hop("a", "b", "packages/a/x.ts"),
                hop("b", "a", "packages/b/y.ts"),
            ],
            group_truncated,
        })
    };
    let mut results = AnalysisResults::default();
    results.package_cycles.push(cycle(true));
    results.package_cycles.push(cycle(false));
    let rules = RulesConfig::default();
    let text = plain(&build_human_lines(&results, &root, &rules, None));
    assert_eq!(
        text.matches("(this package group has more cycles than listed)")
            .count(),
        1,
        "{text}"
    );
}

#[test]
fn empty_sections_are_omitted() {
    let root = PathBuf::from("/project");
    let mut results = AnalysisResults::default();
    results
        .unused_files
        .push(UnusedFileFinding::with_actions(UnusedFile {
            path: root.join("src/dead.ts"),
        }));
    let rules = RulesConfig::default();
    let lines = build_human_lines(&results, &root, &rules, None);
    let text = plain(&lines);
    assert!(text.contains("Unused files (1)"));
    assert!(!text.contains("Unused exports"));
    assert!(!text.contains("Unused dependencies"));
    assert!(!text.contains("Unresolved imports"));
}

#[test]
fn section_header_uses_bullet_indicator() {
    let header = build_section_header("Test section", 3, Level::Error);
    let text = strip_ansi(&header);
    assert!(text.contains("\u{25cf}"));
    assert!(text.contains("Test section (3)"));
}

#[test]
fn section_header_formats_for_all_levels() {
    for level in [Level::Error, Level::Warn, Level::Info] {
        let header = build_section_header("Items", 7, level);
        let text = strip_ansi(&header);
        assert!(
            text.contains("Items (7)"),
            "Missing title for level {level:?}"
        );
    }
}

#[test]
fn grouped_exports_from_different_files_sorted_by_path() {
    let root = PathBuf::from("/project");
    let mut results = AnalysisResults::default();
    results
        .unused_exports
        .push(UnusedExportFinding::with_actions(UnusedExport {
            path: root.join("src/z-file.ts"),
            export_name: "zExport".to_string(),
            is_type_only: false,
            line: 1,
            col: 0,
            span_start: 0,
            is_re_export: false,
            deprecated: false,
            deprecated_reason: None,
        }));
    results
        .unused_exports
        .push(UnusedExportFinding::with_actions(UnusedExport {
            path: root.join("src/a-file.ts"),
            export_name: "aExport".to_string(),
            is_type_only: false,
            line: 1,
            col: 0,
            span_start: 0,
            is_re_export: false,
            deprecated: false,
            deprecated_reason: None,
        }));
    let rules = RulesConfig::default();
    let lines = build_human_lines(&results, &root, &rules, None);
    let text = plain(&lines);
    let a_pos = text.find("src/a-file.ts").unwrap();
    let z_pos = text.find("src/z-file.ts").unwrap();
    assert!(a_pos < z_pos, "Files should be sorted alphabetically");
}

#[test]
fn grouped_items_from_same_file_share_one_file_header() {
    let root = PathBuf::from("/project");
    let mut results = AnalysisResults::default();
    for i in 0..3_u32 {
        results
            .unused_exports
            .push(UnusedExportFinding::with_actions(UnusedExport {
                path: root.join("src/utils.ts"),
                export_name: format!("fn{i}"),
                is_type_only: false,
                line: i + 1,
                col: 0,
                span_start: 0,
                is_re_export: false,
                deprecated: false,
                deprecated_reason: None,
            }));
    }
    let rules = RulesConfig::default();
    let lines = build_human_lines(&results, &root, &rules, None);
    let text = plain(&lines);
    let count = text.matches("src/utils.ts").count();
    assert_eq!(count, 1, "File header should appear once, found {count}");
}

#[test]
fn off_severity_still_shows_section_when_items_present() {
    let root = PathBuf::from("/project");
    let mut results = AnalysisResults::default();
    results
        .unused_files
        .push(UnusedFileFinding::with_actions(UnusedFile {
            path: root.join("src/dead.ts"),
        }));
    let rules = RulesConfig {
        unused_files: Severity::Off,
        ..RulesConfig::default()
    };
    let lines = build_human_lines(&results, &root, &rules, None);
    let text = plain(&lines);
    assert!(text.contains("Unused files (1)"));
}

#[test]
fn deeply_nested_paths_display_correctly() {
    let root = PathBuf::from("/project");
    let mut results = AnalysisResults::default();
    results
        .unused_files
        .push(UnusedFileFinding::with_actions(UnusedFile {
            path: root.join("packages/ui/src/components/forms/inputs/TextInput.tsx"),
        }));
    let rules = RulesConfig::default();
    let lines = build_human_lines(&results, &root, &rules, None);
    let text = plain(&lines);
    assert!(text.contains("packages/ui/src/components/forms/inputs/TextInput.tsx"));
}

#[test]
fn all_issue_types_produce_output_lines() {
    let root = PathBuf::from("/project");
    let results = sample_results(&root);
    let rules = RulesConfig::default();
    let lines = build_human_lines(&results, &root, &rules, None);
    let text = plain(&lines);
    assert!(text.contains("Unused files (1)"));
    assert!(text.contains("Unused exports (1)"));
    assert!(text.contains("Unused type exports (1)"));
    assert!(text.contains("Unused dependencies (1)"));
    assert!(text.contains("Unused devDependencies (1)"));
    assert!(text.contains("Unused optionalDependencies (1)"));
    assert!(text.contains("Unused enum members (1)"));
    assert!(text.contains("Unused class members (1)"));
    assert!(text.contains("Unresolved imports (1)"));
    assert!(text.contains("Unlisted dependencies (1)"));
    assert!(text.contains("Duplicate exports (1)"));
    assert!(text.contains("Type-only dependencies (consider moving to devDependencies) (1)"));
    assert!(
        text.contains("Test-only production dependencies (consider moving to devDependencies) (1)")
    );
    assert!(text.contains("Circular dependencies (1)"));
}

#[test]
fn each_section_ends_with_empty_line_separator() {
    let root = PathBuf::from("/project");
    let mut results = AnalysisResults::default();
    results
        .unused_files
        .push(UnusedFileFinding::with_actions(UnusedFile {
            path: root.join("src/a.ts"),
        }));
    results
        .unused_dependencies
        .push(UnusedDependencyFinding::with_actions(UnusedDependency {
            package_name: "pkg".to_string(),
            location: DependencyLocation::Dependencies,
            path: root.join("package.json"),
            line: 1,
            used_in_workspaces: Vec::new(),
            declared_and_imported_in: Vec::new(),
        }));
    let rules = RulesConfig::default();
    let lines = build_human_lines(&results, &root, &rules, None);
    let empty_count = lines.iter().filter(|l| l.is_empty()).count();
    assert_eq!(
        empty_count, 4,
        "Expected 4 empty separators (2 category headers + 2 sections), got {empty_count}"
    );
}

#[test]
fn type_only_deps_section_title_includes_suggestion() {
    let root = PathBuf::from("/project");
    let mut results = AnalysisResults::default();
    results
        .type_only_dependencies
        .push(TypeOnlyDependencyFinding::with_actions(
            TypeOnlyDependency {
                package_name: "zod".to_string(),
                path: root.join("package.json"),
                line: 8,
            },
        ));
    let rules = RulesConfig::default();
    let lines = build_human_lines(&results, &root, &rules, None);
    let text = plain(&lines);
    assert!(text.contains("Type-only dependencies (consider moving to devDependencies)"));
}

#[test]
fn warn_severity_produces_header_with_bullet() {
    let root = PathBuf::from("/project");
    let mut results = AnalysisResults::default();
    results
        .type_only_dependencies
        .push(TypeOnlyDependencyFinding::with_actions(
            TypeOnlyDependency {
                package_name: "zod".to_string(),
                path: root.join("package.json"),
                line: 8,
            },
        ));
    let rules = RulesConfig::default();
    let lines = build_human_lines(&results, &root, &rules, None);
    let text = plain(&lines);
    assert!(text.contains("\u{25cf}"));
    assert!(text.contains("Type-only dependencies"));
}

#[test]
fn unlisted_deps_show_package_name() {
    let root = PathBuf::from("/project");
    let mut results = AnalysisResults::default();
    results
        .unlisted_dependencies
        .push(UnlistedDependencyFinding::with_actions(
            UnlistedDependency {
                package_name: "@scope/unknown-pkg".to_string(),
                imported_from: vec![],
            },
        ));
    let rules = RulesConfig::default();
    let lines = build_human_lines(&results, &root, &rules, None);
    let text = plain(&lines);
    assert!(text.contains("@scope/unknown-pkg"));
}

#[test]
fn circular_deps_grouped_by_hub() {
    let root = PathBuf::from("/project");
    let mut results = AnalysisResults::default();
    results
        .circular_dependencies
        .push(CircularDependencyFinding::with_actions(
            CircularDependency {
                files: vec![root.join("src/hub.ts"), root.join("src/a.ts")],
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
                files: vec![root.join("src/hub.ts"), root.join("src/b.ts")],
                length: 2,
                line: 5,
                col: 0,
                edges: Vec::new(),
                is_cross_package: false,
            },
        ));
    let rules = RulesConfig::default();
    let lines = build_human_lines(&results, &root, &rules, None);
    let text = plain(&lines);
    assert!(text.contains("(2 cycles)"));
    assert_eq!(text.matches("hub.ts").count(), 3); // header + 2 chain endings
}

#[test]
fn summary_footer_uses_short_labels() {
    let root = PathBuf::from("/project");
    let results = sample_results(&root);
    let footer = build_summary_footer(&results, 0, 0);
    assert!(footer.contains("1 file"));
    assert!(footer.contains("1 export"));
    assert!(footer.contains("1 circular"));
    assert!(!footer.contains("unused file"));
}

#[test]
fn summary_footer_counts_misconfigured_dependency_overrides() {
    let mut results = AnalysisResults::default();
    results.misconfigured_dependency_overrides.push(
        MisconfiguredDependencyOverrideFinding::with_actions(MisconfiguredDependencyOverride {
            raw_key: "bad>".to_string(),
            target_package: None,
            raw_value: "^1".to_string(),
            reason: DependencyOverrideMisconfigReason::UnparsableKey,
            source: DependencyOverrideSource::PnpmPackageJson,
            path: PathBuf::from("/project/package.json"),
            line: 3,
        }),
    );
    assert!(build_summary_footer(&results, 0, 0).contains("1 misconfigured dependency override"));
}

#[test]
fn summary_footer_singularizes_pre_pluralized_labels_for_count_1() {
    let root = PathBuf::from("/project");
    let mut results = AnalysisResults::default();
    results.unused_enum_members.push(
        fallow_types::output_dead_code::UnusedEnumMemberFinding::with_actions(UnusedMember {
            path: root.join("src/types.ts"),
            parent_name: "Status".to_string(),
            member_name: "Unused".to_string(),
            line: 10,
            col: 0,
            kind: MemberKind::EnumMember,
        }),
    );
    results.unused_class_members.push(
        fallow_types::output_dead_code::UnusedClassMemberFinding::with_actions(UnusedMember {
            path: root.join("src/foo.ts"),
            parent_name: "Foo".to_string(),
            member_name: "bar".to_string(),
            line: 5,
            col: 0,
            kind: MemberKind::ClassMethod,
        }),
    );
    let footer = build_summary_footer(&results, 0, 0);
    assert!(
        footer.contains("1 enum member"),
        "Expected '1 enum member' but got: {footer}"
    );
    assert!(
        !footer.contains("1 enum members"),
        "Should not contain '1 enum members': {footer}"
    );
    assert!(
        footer.contains("1 class member"),
        "Expected '1 class member' but got: {footer}"
    );
    assert!(
        !footer.contains("1 class members"),
        "Should not contain '1 class members': {footer}"
    );
}

fn push_dev_dependency_in_production(results: &mut AnalysisResults, package_name: &str) {
    results.dev_dependencies_in_production.push(
        fallow_types::output_dead_code::DevDependencyInProductionFinding::with_actions(
            DevDependencyInProduction {
                package_name: package_name.to_string(),
                path: PathBuf::from("/project/package.json"),
                line: 5,
            },
        ),
    );
}

#[test]
fn summary_footer_pluralizes_noun_before_trailing_qualifier() {
    let mut results = AnalysisResults::default();
    push_dev_dependency_in_production(&mut results, "vitest");
    assert_eq!(
        build_summary_footer(&results, 0, 0),
        "1 dev dependency in production"
    );

    push_dev_dependency_in_production(&mut results, "tinyspy");
    assert_eq!(
        build_summary_footer(&results, 0, 0),
        "2 dev dependencies in production"
    );
}

#[test]
fn suppression_footer_names_a_stale_count_only_above_zero() {
    let mut results = AnalysisResults::default();
    assert_eq!(suppression_footer(&results), None);

    results.suppression_count = 2;
    assert_eq!(
        suppression_footer(&results).as_deref(),
        Some("0 issues \u{00b7} 2 suppressed")
    );

    results.stale_suppressions.push(StaleSuppression {
        path: PathBuf::from("/project/src/a.ts"),
        line: 1,
        col: 0,
        origin: SuppressionOrigin::Comment {
            issue_kind: Some("unused-export".to_owned()),
            reason: None,
            is_file_level: false,
            kind_known: true,
        },
        missing_reason: false,
        finding_id: None,
        actions: Vec::new(),
        effective_severity: None,
    });
    assert_eq!(
        suppression_footer(&results).as_deref(),
        Some("1 issue \u{00b7} 2 suppressed \u{00b7} 1 stale suppression")
    );

    results.suppression_count = 0;
    assert_eq!(
        suppression_footer(&results).as_deref(),
        Some("1 issue \u{00b7} 0 suppressed \u{00b7} 1 stale suppression")
    );
}

#[test]
fn section_footer_contains_docs_link() {
    let root = PathBuf::from("/project");
    let mut results = AnalysisResults::default();
    results
        .unused_files
        .push(UnusedFileFinding::with_actions(UnusedFile {
            path: root.join("src/dead.ts"),
        }));
    let rules = RulesConfig::default();
    let lines = build_human_lines(&results, &root, &rules, None);
    let text = plain(&lines);
    assert!(text.contains("fallow.tools/docs/explanations/dead-code/"));
    assert!(text.contains("Files not reachable from any entry point"));
}

#[test]
fn flat_section_truncates_at_max() {
    let root = PathBuf::from("/project");
    let mut results = AnalysisResults::default();
    for i in 0..15 {
        results
            .unused_files
            .push(UnusedFileFinding::with_actions(UnusedFile {
                path: root.join(format!("src/dead{i}.ts")),
            }));
    }
    let rules = RulesConfig::default();
    let lines = build_human_lines(&results, &root, &rules, None);
    let text = plain(&lines);
    assert!(text.contains("... and 5 more"));
}

#[test]
fn grouped_section_truncates_files() {
    let root = PathBuf::from("/project");
    let mut results = AnalysisResults::default();
    for i in 0..15 {
        results
            .unused_exports
            .push(UnusedExportFinding::with_actions(UnusedExport {
                path: root.join(format!("src/file{i:02}.ts")),
                export_name: format!("fn{i}"),
                is_type_only: false,
                line: 1,
                col: 0,
                span_start: 0,
                is_re_export: false,
                deprecated: false,
                deprecated_reason: None,
            }));
    }
    let rules = RulesConfig::default();
    let lines = build_human_lines(&results, &root, &rules, None);
    let text = plain(&lines);
    assert!(text.contains("... and 5 more in 5 files"));
}

#[test]
fn top_flag_limits_unused_files_shown() {
    let root = PathBuf::from("/project");
    let mut results = AnalysisResults::default();
    for i in 0..5 {
        results
            .unused_files
            .push(UnusedFileFinding::with_actions(UnusedFile {
                path: root.join(format!("src/dead{i}.ts")),
            }));
    }
    let rules = RulesConfig::default();
    let lines = build_human_lines(&results, &root, &rules, Some(2));
    let text = plain(&lines);

    assert!(text.contains("Unused files (5)"));

    let file_lines: Vec<&str> = text
        .lines()
        .filter(|l| l.contains("src/dead") && l.contains(".ts"))
        .collect();
    assert_eq!(
        file_lines.len(),
        2,
        "Expected 2 file lines with top=2, got {}: {file_lines:?}",
        file_lines.len()
    );

    assert!(
        text.contains("... and 3 more"),
        "Expected truncation hint, got:\n{text}"
    );
}

fn architecture_sample(root: &Path) -> AnalysisResults {
    let mut results = sample_results(root);
    results
        .policy_violations
        .push(PolicyViolationFinding::with_actions(PolicyViolation {
            path: root.join("src/app.ts"),
            line: 7,
            col: 2,
            pack: "team-policy".to_string(),
            rule_id: "no-moment".to_string(),
            kind: fallow_types::results::PolicyRuleKind::BannedImport,
            matched: "moment".to_string(),
            severity: fallow_types::results::PolicyViolationSeverity::Warn,
            message: None,
        }));
    results
}

fn layout_lines(results: &AnalysisResults, root: &Path, layout: ArchitectureLayout) -> Vec<String> {
    build_layout_lines(&LayoutLinesInput {
        results,
        root,
        rules: &RulesConfig::default(),
        top: None,
        explain: false,
        layout,
    })
}

#[test]
fn embedded_layout_is_the_classic_dead_code_report() {
    let root = PathBuf::from("/project");
    let results = architecture_sample(&root);
    let lines = layout_lines(&results, &root, ArchitectureLayout::Embedded);
    assert_eq!(
        lines,
        build_human_lines(&results, &root, &RulesConfig::default(), None)
    );
    let text = plain(&lines);
    assert!(text.contains("Structure"));
    assert!(!text.contains("\u{2500}\u{2500} Architecture"));
    assert!(text.contains("Circular dependencies (1)"));
    assert!(text.contains("Policy violations (1)"));
}

#[test]
fn split_layout_moves_architecture_findings_and_keeps_component_findings() {
    let root = PathBuf::from("/project");
    let results = architecture_sample(&root);
    let text = plain(&layout_lines(&results, &root, ArchitectureLayout::Split));
    let heading = text
        .find("\u{2500}\u{2500} Architecture ")
        .unwrap_or_else(|| panic!("architecture category: {text}"));
    let (dead_code, architecture) = text.split_at(heading);

    assert!(dead_code.contains("Duplicate exports"));
    assert!(dead_code.contains("Unrendered components"));
    assert!(!dead_code.contains("Circular dependencies"));
    assert!(!dead_code.contains("Boundary violations"));
    assert!(!dead_code.contains("Policy violations"));

    assert!(architecture.contains("Circular dependencies (1)"));
    assert!(architecture.contains("Boundary violations (1)"));
    assert!(architecture.contains("Policy violations (1)"));
    assert_eq!(
        text.matches("\u{2500}\u{2500} Architecture").count(),
        1,
        "{text}"
    );
}

#[test]
fn split_layout_equals_the_grouped_layout() {
    let root = PathBuf::from("/project");
    let results = architecture_sample(&root);
    let rules = RulesConfig::default();
    assert_eq!(
        layout_lines(&results, &root, ArchitectureLayout::Split),
        grouped_lines(&results, &root, &rules, false, ArchitectureLayout::Split)
    );
}

#[test]
fn only_layout_prints_one_architecture_category() {
    let root = PathBuf::from("/project");
    let mut results = AnalysisResults::default();
    let sample = architecture_sample(&root);
    results.circular_dependencies = sample.circular_dependencies;
    results.boundary_violations = sample.boundary_violations;
    results.policy_violations = sample.policy_violations;

    let text = plain(&layout_lines(&results, &root, ArchitectureLayout::Only));
    assert!(text.starts_with("\u{2500}\u{2500} Architecture "), "{text}");
    assert!(!text.contains("Structure"));
    assert!(!text.contains("\u{2500}\u{2500} Policy"));
    assert!(text.contains("Circular dependencies (1)"));
    assert!(text.contains("Policy violations (1)"));
}

#[test]
fn split_layout_without_architecture_findings_has_no_architecture_lines() {
    let root = PathBuf::from("/project");
    let mut results = sample_results(&root);
    results.circular_dependencies.clear();
    results.boundary_violations.clear();
    assert_eq!(
        layout_lines(&results, &root, ArchitectureLayout::Split),
        build_human_lines(&results, &root, &RulesConfig::default(), None)
    );
}

#[test]
fn split_summary_moves_the_architecture_rows() {
    let root = PathBuf::from("/project");
    let results = architecture_sample(&root);
    let (dead_code, architecture) = split_summary_categories(&results, &RulesConfig::default());
    let names = |rows: &[(&str, usize, Level)]| -> Vec<String> {
        rows.iter()
            .map(|(name, _, _)| (*name).to_string())
            .collect()
    };
    assert_eq!(
        names(&architecture),
        ARCHITECTURE_SUMMARY_ROWS.map(String::from)
    );
    assert!(names(&dead_code).contains(&"Duplicate exports".to_string()));
    assert_eq!(
        dead_code.len() + architecture.len(),
        check_summary_categories(&results, &RulesConfig::default()).len()
    );
}

#[test]
fn grouped_split_lines_use_one_architecture_category() {
    let root = PathBuf::from("/project");
    let results = architecture_sample(&root);
    let rules = RulesConfig::default();
    let text = plain(&grouped_lines(
        &results,
        &root,
        &rules,
        false,
        ArchitectureLayout::Split,
    ));
    assert!(text.contains("\u{2500}\u{2500} Architecture "), "{text}");
    assert_eq!(text.matches("Policy violations").count(), 1, "{text}");
    let unused = text.find("Duplicate exports").expect("dead-code lines");
    let architecture = text.find("\u{2500}\u{2500} Architecture ").expect("arch");
    assert!(unused < architecture, "{text}");
    assert_eq!(
        grouped_lines(&results, &root, &rules, false, ArchitectureLayout::Embedded),
        build_human_lines(&results, &root, &rules, None)
    );
}

#[test]
fn summary_counts_policy_violations() {
    let root = PathBuf::from("/project");
    let results = architecture_sample(&root);
    let categories = check_summary_categories(&results, &RulesConfig::default());
    assert!(
        categories
            .iter()
            .any(|(name, count, _)| *name == "Policy violations" && *count == 1)
    );
}
