use std::collections::BTreeSet;

use fallow_output::issue_output_contracts;

fn codeclimate_check_name_literals() -> BTreeSet<String> {
    let source = [
        include_str!("dead_code_codeclimate.rs")
            .split("#[cfg(test)]")
            .next()
            .expect("source before tests"),
        include_str!("dead_code_codeclimate_component_health.rs"),
        include_str!("dead_code_codeclimate_catalog.rs"),
        include_str!("dead_code_codeclimate_component_io.rs"),
    ]
    .concat();
    let mut literals = BTreeSet::new();
    let mut rest = source.as_str();
    while let Some(start) = rest.find("\"fallow/") {
        let after_quote = &rest[start + 1..];
        let Some(end) = after_quote.find('"') else {
            break;
        };
        literals.insert(after_quote[..end].to_owned());
        rest = &after_quote[end + 1..];
    }
    literals
}

#[test]
fn codeclimate_check_names_match_issue_contracts() {
    let from_emitter = codeclimate_check_name_literals();
    let from_contracts = issue_output_contracts()
        .flat_map(|contract| contract.codeclimate_check_names)
        .collect::<BTreeSet<_>>();

    assert_eq!(from_emitter, from_contracts);
}

mod caveats {
    use std::path::{Path, PathBuf};

    use fallow_config::RulesConfig;
    use fallow_types::extract::MemberKind;
    use fallow_types::output_dead_code::{
        ReachabilityCaveat, UnusedClassMemberFinding, UnusedDependencyFinding,
        UnusedEnumMemberFinding, UnusedExportFinding, UnusedFileFinding, UnusedStoreMemberFinding,
    };
    use fallow_types::results::{
        AnalysisResults, DependencyLocation, UnusedDependency, UnusedExport, UnusedFile,
        UnusedMember,
    };

    use crate::dead_code_codeclimate::build_codeclimate;

    /// One finding of each caveated kind, caveated or not.
    fn results_with(root: &Path, caveated: bool) -> AnalysisResults {
        let caveats = if caveated {
            vec![ReachabilityCaveat::IncompleteImportGraph]
        } else {
            Vec::new()
        };
        let mut results = AnalysisResults::default();

        let mut file = UnusedFileFinding::with_actions(UnusedFile {
            path: root.join("src/lib.ts"),
        });
        file.reachability_caveats.clone_from(&caveats);
        results.unused_files.push(file);

        let mut export = UnusedExportFinding::with_actions(UnusedExport {
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
        export.reachability_caveats.clone_from(&caveats);
        results.unused_exports.push(export);

        let mut dep = UnusedDependencyFinding::with_actions(UnusedDependency {
            package_name: "left-pad".to_owned(),
            location: DependencyLocation::Dependencies,
            path: root.join("package.json"),
            line: 5,
            used_in_workspaces: Vec::new(),
            declared_and_imported_in: Vec::new(),
        });
        dep.reachability_caveats.clone_from(&caveats);
        results.unused_dependencies.push(dep);

        let member = |parent: &str, name: &str, kind| UnusedMember {
            path: root.join("src/lib.ts"),
            parent_name: parent.to_owned(),
            member_name: name.to_owned(),
            kind,
            line: 7,
            col: 2,
        };

        let mut enum_member =
            UnusedEnumMemberFinding::with_actions(member("Mode", "Legacy", MemberKind::EnumMember));
        enum_member.reachability_caveats.clone_from(&caveats);
        results.unused_enum_members.push(enum_member);

        let mut class_member = UnusedClassMemberFinding::with_actions(member(
            "Widget",
            "render",
            MemberKind::ClassMethod,
        ));
        class_member.reachability_caveats.clone_from(&caveats);
        results.unused_class_members.push(class_member);

        let mut store_member = UnusedStoreMemberFinding::with_actions(member(
            "useCart",
            "subtotal",
            MemberKind::StoreMember,
        ));
        store_member.reachability_caveats.clone_from(&caveats);
        results.unused_store_members.push(store_member);

        results
    }

    /// `description` is the field GitLab renders inline on the MR diff, and
    /// the field `CiIssue` carries into the PR-comment and review-comment
    /// bodies that offer the mutation. A verdict resting on a file the run
    /// never read has to say so there.
    #[test]
    fn descriptions_name_the_caveat() {
        let root = PathBuf::from("/project");

        let issues = build_codeclimate(&results_with(&root, true), &root, &RulesConfig::default());

        let descriptions: Vec<&str> = issues
            .iter()
            .map(|issue| issue.description.as_str())
            .collect();
        assert!(
            descriptions
                .iter()
                .all(|description| description.ends_with(" (caveat: incomplete import graph)")),
            "every caveated finding hedges its description: {descriptions:?}"
        );
        assert!(
            descriptions.contains(
                &"File is not reachable from any entry point (caveat: incomplete import graph)"
            ),
            "{descriptions:?}"
        );
    }

    /// A clean run must stay byte-identical, so an integrator diffing
    /// reports across versions sees no churn from a mechanism that did not
    /// fire.
    #[test]
    fn a_clean_run_carries_no_caveat_text() {
        let root = PathBuf::from("/project");

        let issues = build_codeclimate(&results_with(&root, false), &root, &RulesConfig::default());

        assert!(
            issues
                .iter()
                .all(|issue| !issue.description.contains("caveat")),
            "{:?}",
            issues
                .iter()
                .map(|issue| issue.description.as_str())
                .collect::<Vec<_>>()
        );
    }

    /// The fingerprint is GitLab's and the review layer's comment identity.
    /// It is computed from rule id plus location, never from the message,
    /// so gaining a caveat must not reopen a resolved comment thread.
    #[test]
    fn the_caveat_does_not_move_the_fingerprint() {
        let root = PathBuf::from("/project");

        let clean = build_codeclimate(&results_with(&root, false), &root, &RulesConfig::default());
        let caveated =
            build_codeclimate(&results_with(&root, true), &root, &RulesConfig::default());

        let fingerprints = |issues: &[fallow_output::CodeClimateIssue]| {
            issues
                .iter()
                .map(|issue| issue.fingerprint.clone())
                .collect::<Vec<_>>()
        };
        assert_eq!(fingerprints(&clean), fingerprints(&caveated));
        assert_ne!(
            clean[0].description, caveated[0].description,
            "the guard is only meaningful while the description actually changed"
        );
    }
}

mod stable_fingerprints {
    use std::path::{Path, PathBuf};

    use fallow_config::RulesConfig;
    use fallow_output::{CodeClimateIssue, codeclimate_fingerprint_hash};
    use fallow_types::identity::stamp_dead_code_finding_ids;
    use fallow_types::output_dead_code::{
        DuplicateExportFinding, UnlistedDependencyFinding, UnusedDependencyFinding,
        UnusedExportFinding,
    };
    use fallow_types::results::{
        AnalysisResults, DependencyLocation, DuplicateExport, DuplicateLocation, ImportSite,
        UnlistedDependency, UnusedDependency, UnusedExport,
    };

    use crate::dead_code_codeclimate::build_codeclimate;

    fn export(root: &Path, name: &str, line: u32) -> UnusedExportFinding {
        UnusedExportFinding::with_actions(UnusedExport {
            path: root.join("src/lib.ts"),
            export_name: name.to_owned(),
            is_type_only: false,
            line,
            col: 0,
            span_start: 0,
            is_re_export: false,
            deprecated: false,
            deprecated_reason: None,
        })
    }

    fn dependency(manifest: PathBuf, line: u32) -> UnusedDependencyFinding {
        UnusedDependencyFinding::with_actions(UnusedDependency {
            package_name: "left-pad".to_owned(),
            location: DependencyLocation::Dependencies,
            path: manifest,
            line,
            used_in_workspaces: Vec::new(),
            declared_and_imported_in: Vec::new(),
        })
    }

    /// Findings of the kinds whose old fingerprint held a line, with
    /// every line moved down by `shift`.
    fn results(root: &Path, shift: u32) -> AnalysisResults {
        let mut results = AnalysisResults::default();
        results
            .unused_exports
            .push(export(root, "first", 3 + shift));
        results
            .unused_exports
            .push(export(root, "second", 9 + shift));
        results
            .unused_dependencies
            .push(dependency(root.join("package.json"), 5 + shift));
        results
            .unlisted_dependencies
            .push(UnlistedDependencyFinding::with_actions(
                UnlistedDependency {
                    package_name: "chalk".to_owned(),
                    imported_from: vec![
                        ImportSite {
                            path: root.join("src/cli.ts"),
                            line: 2 + shift,
                            col: 0,
                        },
                        ImportSite {
                            path: root.join("src/cli.ts"),
                            line: 8 + shift,
                            col: 0,
                        },
                    ],
                },
            ));
        results
            .duplicate_exports
            .push(DuplicateExportFinding::with_actions(DuplicateExport {
                export_name: "Config".to_owned(),
                locations: vec![
                    DuplicateLocation {
                        path: root.join("src/a.ts"),
                        line: 4 + shift,
                        col: 0,
                    },
                    DuplicateLocation {
                        path: root.join("src/b.ts"),
                        line: 6 + shift,
                        col: 0,
                    },
                ],
            }));
        stamp_dead_code_finding_ids(&mut results, root);
        results
    }

    fn fingerprints(issues: &[CodeClimateIssue]) -> Vec<String> {
        issues
            .iter()
            .map(|issue| issue.fingerprint.clone())
            .collect()
    }

    /// A line shift above a finding keeps its fingerprint, so GitLab and
    /// the review layer keep the same issue and the same thread.
    #[test]
    fn a_line_shift_keeps_every_dead_code_fingerprint() {
        let root = PathBuf::from("/project");
        let rules = RulesConfig::default();

        let before = build_codeclimate(&results(&root, 0), &root, &rules);
        let after = build_codeclimate(&results(&root, 40), &root, &rules);

        assert_eq!(before.len(), 7);
        assert_eq!(fingerprints(&before), fingerprints(&after));
        assert_ne!(
            before[0].location.lines.begin, after[0].location.lines.begin,
            "the guard is only meaningful while the lines actually moved"
        );
        let mut unique = fingerprints(&before);
        unique.sort();
        unique.dedup();
        assert_eq!(
            unique.len(),
            before.len(),
            "every issue keeps its own fingerprint"
        );
        assert_eq!(
            before[0].legacy_fingerprint.as_deref(),
            Some(
                codeclimate_fingerprint_hash(&["fallow/unused-export", "src/lib.ts", "3", "first"])
                    .as_str()
            ),
            "the line-based value stays available to match older review threads"
        );
    }

    fn site(root: &Path, file: &str, line: u32) -> ImportSite {
        ImportSite {
            path: root.join(file),
            line,
            col: 0,
        }
    }

    fn location(root: &Path, file: &str, line: u32) -> DuplicateLocation {
        DuplicateLocation {
            path: root.join(file),
            line,
            col: 0,
        }
    }

    /// Fingerprint per location path for one unlisted dependency and one
    /// duplicate export with the given locations.
    fn per_location(
        root: &Path,
        sites: Vec<ImportSite>,
        locations: Vec<DuplicateLocation>,
    ) -> Vec<(String, u32, String)> {
        let mut results = AnalysisResults::default();
        results
            .unlisted_dependencies
            .push(UnlistedDependencyFinding::with_actions(
                UnlistedDependency {
                    package_name: "chalk".to_owned(),
                    imported_from: sites,
                },
            ));
        results
            .duplicate_exports
            .push(DuplicateExportFinding::with_actions(DuplicateExport {
                export_name: "Config".to_owned(),
                locations,
            }));
        stamp_dead_code_finding_ids(&mut results, root);
        build_codeclimate(&results, root, &RulesConfig::default())
            .into_iter()
            .map(|issue| {
                (
                    format!("{} {}", issue.check_name, issue.location.path),
                    issue.location.lines.begin,
                    issue.fingerprint,
                )
            })
            .collect()
    }

    fn fingerprint_at(issues: &[(String, u32, String)], path: &str, line: u32) -> String {
        issues
            .iter()
            .find(|(p, l, _)| p == path && *l == line)
            .map_or_else(
                || panic!("no issue at {path}:{line}: {issues:?}"),
                |(_, _, fingerprint)| fingerprint.clone(),
            )
    }

    /// A location is named by its content, not by its position among the
    /// other locations. Adding or removing a sibling location, in another
    /// file or later in the same file, keeps the fingerprints of the
    /// others.
    #[test]
    fn a_sibling_location_does_not_move_the_other_fingerprints() {
        let root = PathBuf::from("/project");
        let base = per_location(
            &root,
            vec![site(&root, "src/b.ts", 4), site(&root, "src/c.ts", 7)],
            vec![
                location(&root, "src/b.ts", 4),
                location(&root, "src/c.ts", 7),
            ],
        );
        let added = per_location(
            &root,
            vec![
                site(&root, "src/a.ts", 1),
                site(&root, "src/b.ts", 4),
                site(&root, "src/b.ts", 9),
                site(&root, "src/c.ts", 7),
            ],
            vec![
                location(&root, "src/a.ts", 1),
                location(&root, "src/b.ts", 4),
                location(&root, "src/b.ts", 9),
                location(&root, "src/c.ts", 7),
            ],
        );
        let removed = per_location(
            &root,
            vec![site(&root, "src/c.ts", 7)],
            vec![
                location(&root, "src/c.ts", 7),
                location(&root, "src/d.ts", 2),
            ],
        );

        for rule in ["fallow/unlisted-dependency", "fallow/duplicate-export"] {
            let pick = |issues: &[(String, u32, String)]| {
                issues
                    .iter()
                    .filter_map(|(key, line, fingerprint)| {
                        key.strip_prefix(&format!("{rule} "))
                            .map(|path| (path.to_owned(), *line, fingerprint.clone()))
                    })
                    .collect::<Vec<_>>()
            };
            let (base, added, removed) = (pick(&base), pick(&added), pick(&removed));
            assert_eq!(
                fingerprint_at(&base, "src/b.ts", 4),
                fingerprint_at(&added, "src/b.ts", 4)
            );
            assert_eq!(
                fingerprint_at(&base, "src/c.ts", 7),
                fingerprint_at(&added, "src/c.ts", 7)
            );
            assert_eq!(
                fingerprint_at(&base, "src/c.ts", 7),
                fingerprint_at(&removed, "src/c.ts", 7)
            );
            assert_ne!(
                fingerprint_at(&added, "src/b.ts", 4),
                fingerprint_at(&added, "src/b.ts", 9),
                "two locations with the same content still get two fingerprints"
            );
        }
    }

    /// The same package unused in two workspaces is two findings. The old
    /// fingerprint held only rule and package name, so they collided.
    #[test]
    fn the_same_dependency_in_two_workspaces_gets_two_fingerprints() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results
            .unused_dependencies
            .push(dependency(root.join("packages/a/package.json"), 5));
        results
            .unused_dependencies
            .push(dependency(root.join("packages/b/package.json"), 5));
        stamp_dead_code_finding_ids(&mut results, &root);

        let issues = build_codeclimate(&results, &root, &RulesConfig::default());

        assert_eq!(issues.len(), 2);
        assert_ne!(issues[0].fingerprint, issues[1].fingerprint);
    }

    /// A saved report from before finding ids has no ids. It keeps the old
    /// line-based fingerprint, so its threads stay matched.
    #[test]
    fn a_finding_without_an_id_keeps_the_legacy_fingerprint() {
        let root = PathBuf::from("/project");
        let mut results = AnalysisResults::default();
        results.unused_exports.push(export(&root, "first", 3));

        let issues = build_codeclimate(&results, &root, &RulesConfig::default());

        assert_eq!(
            issues[0].fingerprint,
            codeclimate_fingerprint_hash(&["fallow/unused-export", "src/lib.ts", "3", "first"])
        );
    }
}
