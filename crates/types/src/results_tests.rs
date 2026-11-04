use super::*;
use crate::output_dead_code::{
    BoundaryViolationFinding, CircularDependencyFinding, UnresolvedImportFinding,
    UnusedClassMemberFinding, UnusedEnumMemberFinding, UnusedExportFinding, UnusedFileFinding,
    UnusedTypeFinding,
};

#[test]
fn empty_results_no_issues() {
    let results = AnalysisResults::default();
    assert_eq!(results.total_issues(), 0);
    assert!(!results.has_issues());
}

#[test]
fn results_with_unused_file() {
    let mut results = AnalysisResults::default();
    results
        .unused_files
        .push(UnusedFileFinding::with_actions(UnusedFile {
            path: PathBuf::from("test.ts"),
        }));
    assert_eq!(results.total_issues(), 1);
    assert!(results.has_issues());
}

#[test]
fn results_with_unused_export() {
    let mut results = AnalysisResults::default();
    results
        .unused_exports
        .push(UnusedExportFinding::with_actions(UnusedExport {
            path: PathBuf::from("test.ts"),
            export_name: "foo".to_string(),
            is_type_only: false,
            line: 1,
            col: 0,
            span_start: 0,
            is_re_export: false,
            deprecated: false,
            deprecated_reason: None,
        }));
    assert_eq!(results.total_issues(), 1);
    assert!(results.has_issues());
}

#[test]
fn merge_into_appends_counts_and_preserves_existing_optional_metadata() {
    let framework_contract = crate::semantic::SemanticFrameworkContract {
        framework: "lit".to_string(),
        package: "lit".to_string(),
        heritage_symbol: "LitElement".to_string(),
        heritage_names: vec!["LitElement".to_string()],
        relation: crate::semantic::SemanticFrameworkRelation::Extends,
        members: vec!["render".to_string()],
    };
    let mut target = AnalysisResults {
        unused_files: vec![UnusedFileFinding::with_actions(UnusedFile {
            path: PathBuf::from("a.ts"),
        })],
        suppression_count: 2,
        security_unresolved_edge_files: 1,
        security_unresolved_callee_sites: 3,
        entry_point_summary: Some(EntryPointSummary {
            total: 1,
            by_source: vec![("existing".to_string(), 1)],
        }),
        semantic_framework_contracts: vec![framework_contract.clone()],
        ..AnalysisResults::default()
    };
    let source = AnalysisResults {
        unused_files: vec![UnusedFileFinding::with_actions(UnusedFile {
            path: PathBuf::from("b.ts"),
        })],
        suppression_count: 4,
        security_unresolved_edge_files: 5,
        security_unresolved_callee_sites: 6,
        unused_load_data_keys_global_abstain: true,
        entry_point_summary: Some(EntryPointSummary {
            total: 1,
            by_source: vec![("incoming".to_string(), 1)],
        }),
        render_fan_in: Some(RenderFanInMetric::default()),
        semantic_framework_contracts: vec![framework_contract],
        ..AnalysisResults::default()
    };

    target.merge_into(source);

    assert_eq!(target.unused_files.len(), 2);
    assert_eq!(target.suppression_count, 6);
    assert_eq!(target.security_unresolved_edge_files, 6);
    assert_eq!(target.security_unresolved_callee_sites, 9);
    assert!(target.unused_load_data_keys_global_abstain);
    assert_eq!(
        target
            .entry_point_summary
            .as_ref()
            .map(|summary| summary.total),
        Some(1)
    );
    assert_eq!(
        target
            .entry_point_summary
            .as_ref()
            .and_then(|summary| summary.by_source.first())
            .map(|(name, _)| name.as_str()),
        Some("existing")
    );
    assert!(target.render_fan_in.is_some());
    assert_eq!(target.semantic_framework_contracts.len(), 1);
}

fn test_unused_export(path: &str, export_name: &str, is_type_only: bool) -> UnusedExport {
    UnusedExport {
        path: PathBuf::from(path),
        export_name: export_name.to_string(),
        is_type_only,
        line: 1,
        col: 0,
        span_start: 0,
        is_re_export: false,
        deprecated: false,
        deprecated_reason: None,
    }
}

fn test_unused_dependency(package_name: &str, location: DependencyLocation) -> UnusedDependency {
    UnusedDependency {
        package_name: package_name.to_string(),
        location,
        path: PathBuf::from("package.json"),
        line: 5,
        used_in_workspaces: Vec::new(),
        declared_and_imported_in: Vec::new(),
    }
}

fn test_unused_member(member_name: &str, kind: MemberKind) -> UnusedMember {
    UnusedMember {
        path: PathBuf::from("members.ts"),
        parent_name: "Parent".to_string(),
        member_name: member_name.to_string(),
        kind,
        line: 1,
        col: 0,
    }
}

#[test]
fn results_total_counts_all_types() {
    let results = AnalysisResults {
        unused_files: vec![UnusedFileFinding::with_actions(UnusedFile {
            path: PathBuf::from("a.ts"),
        })],
        unused_exports: vec![UnusedExportFinding::with_actions(test_unused_export(
            "b.ts", "x", false,
        ))],
        unused_types: vec![UnusedTypeFinding::with_actions(test_unused_export(
            "c.ts", "T", true,
        ))],
        unused_dependencies: vec![UnusedDependencyFinding::with_actions(
            test_unused_dependency("dep", DependencyLocation::Dependencies),
        )],
        unused_dev_dependencies: vec![UnusedDevDependencyFinding::with_actions(
            test_unused_dependency("dev", DependencyLocation::DevDependencies),
        )],
        unused_enum_members: vec![UnusedEnumMemberFinding::with_actions(test_unused_member(
            "A",
            MemberKind::EnumMember,
        ))],
        unused_class_members: vec![UnusedClassMemberFinding::with_actions(test_unused_member(
            "m",
            MemberKind::ClassMethod,
        ))],
        unresolved_imports: vec![UnresolvedImportFinding::with_actions(UnresolvedImport {
            path: PathBuf::from("f.ts"),
            specifier: "./missing".to_string(),
            line: 1,
            col: 0,
            specifier_col: 0,
        })],
        unlisted_dependencies: vec![UnlistedDependencyFinding::with_actions(
            UnlistedDependency {
                package_name: "unlisted".to_string(),
                imported_from: vec![ImportSite {
                    path: PathBuf::from("g.ts"),
                    line: 1,
                    col: 0,
                }],
            },
        )],
        duplicate_exports: vec![DuplicateExportFinding::with_actions(DuplicateExport {
            export_name: "dup".to_string(),
            locations: vec![
                DuplicateLocation {
                    path: PathBuf::from("h.ts"),
                    line: 15,
                    col: 0,
                },
                DuplicateLocation {
                    path: PathBuf::from("i.ts"),
                    line: 30,
                    col: 0,
                },
            ],
        })],
        unused_optional_dependencies: vec![UnusedOptionalDependencyFinding::with_actions(
            test_unused_dependency("optional", DependencyLocation::OptionalDependencies),
        )],
        type_only_dependencies: vec![TypeOnlyDependencyFinding::with_actions(
            TypeOnlyDependency {
                package_name: "type-only".to_string(),
                path: PathBuf::from("package.json"),
                line: 8,
            },
        )],
        test_only_dependencies: vec![TestOnlyDependencyFinding::with_actions(
            TestOnlyDependency {
                package_name: "test-only".to_string(),
                path: PathBuf::from("package.json"),
                line: 9,
            },
        )],
        circular_dependencies: vec![CircularDependencyFinding::with_actions(
            CircularDependency {
                files: vec![PathBuf::from("a.ts"), PathBuf::from("b.ts")],
                length: 2,
                line: 3,
                col: 0,
                edges: Vec::new(),
                is_cross_package: false,
            },
        )],
        boundary_violations: vec![BoundaryViolationFinding::with_actions(BoundaryViolation {
            from_path: PathBuf::from("src/ui/Button.tsx"),
            to_path: PathBuf::from("src/db/queries.ts"),
            from_zone: "ui".to_string(),
            to_zone: "database".to_string(),
            import_specifier: "../db/queries".to_string(),
            line: 3,
            col: 0,
            via_path: None,
        })],
        ..Default::default()
    };

    // 15 categories, one of each
    assert_eq!(results.total_issues(), 15);
    assert!(results.has_issues());
}

// ── total_issues counts each category independently ─────────

#[test]
fn total_issues_sums_all_categories_independently() {
    let mut results = AnalysisResults::default();
    results
        .unused_files
        .push(UnusedFileFinding::with_actions(UnusedFile {
            path: PathBuf::from("a.ts"),
        }));
    assert_eq!(results.total_issues(), 1);

    results
        .unused_files
        .push(UnusedFileFinding::with_actions(UnusedFile {
            path: PathBuf::from("b.ts"),
        }));
    assert_eq!(results.total_issues(), 2);

    results
        .unresolved_imports
        .push(UnresolvedImportFinding::with_actions(UnresolvedImport {
            path: PathBuf::from("c.ts"),
            specifier: "./missing".to_string(),
            line: 1,
            col: 0,
            specifier_col: 0,
        }));
    assert_eq!(results.total_issues(), 3);
}

// ── sort: unused_files by path ──────────────────────────────

#[test]
fn sort_unused_files_by_path() {
    let mut r = AnalysisResults::default();
    r.unused_files
        .push(UnusedFileFinding::with_actions(UnusedFile {
            path: PathBuf::from("z.ts"),
        }));
    r.unused_files
        .push(UnusedFileFinding::with_actions(UnusedFile {
            path: PathBuf::from("a.ts"),
        }));
    r.unused_files
        .push(UnusedFileFinding::with_actions(UnusedFile {
            path: PathBuf::from("m.ts"),
        }));
    r.sort();
    let paths: Vec<_> = r
        .unused_files
        .iter()
        .map(|f| f.file.path.to_string_lossy().to_string())
        .collect();
    assert_eq!(paths, vec!["a.ts", "m.ts", "z.ts"]);
}

// ── sort: unused_exports by path, line, name ────────────────

#[test]
fn sort_unused_exports_by_path_line_name() {
    let mut r = AnalysisResults::default();
    let mk = |path: &str, line: u32, name: &str| {
        UnusedExportFinding::with_actions(UnusedExport {
            path: PathBuf::from(path),
            export_name: name.to_string(),
            is_type_only: false,
            line,
            col: 0,
            span_start: 0,
            is_re_export: false,
            deprecated: false,
            deprecated_reason: None,
        })
    };
    r.unused_exports.push(mk("b.ts", 5, "beta"));
    r.unused_exports.push(mk("a.ts", 10, "zeta"));
    r.unused_exports.push(mk("a.ts", 10, "alpha"));
    r.unused_exports.push(mk("a.ts", 1, "gamma"));
    r.sort();
    let keys: Vec<_> = r
        .unused_exports
        .iter()
        .map(|e| {
            format!(
                "{}:{}:{}",
                e.export.path.to_string_lossy(),
                e.export.line,
                e.export.export_name
            )
        })
        .collect();
    assert_eq!(
        keys,
        vec![
            "a.ts:1:gamma",
            "a.ts:10:alpha",
            "a.ts:10:zeta",
            "b.ts:5:beta"
        ]
    );
}

// ── sort: unused_types (same sort as unused_exports) ────────

#[test]
fn sort_unused_types_by_path_line_name() {
    let mut r = AnalysisResults::default();
    let mk = |path: &str, line: u32, name: &str| {
        UnusedTypeFinding::with_actions(UnusedExport {
            path: PathBuf::from(path),
            export_name: name.to_string(),
            is_type_only: true,
            line,
            col: 0,
            span_start: 0,
            is_re_export: false,
            deprecated: false,
            deprecated_reason: None,
        })
    };
    r.unused_types.push(mk("z.ts", 1, "Z"));
    r.unused_types.push(mk("a.ts", 1, "A"));
    r.sort();
    assert_eq!(r.unused_types[0].export.path, PathBuf::from("a.ts"));
    assert_eq!(r.unused_types[1].export.path, PathBuf::from("z.ts"));
}

// ── sort: unused_dependencies by path, line, name ───────────

#[test]
fn sort_unused_dependencies_by_path_line_name() {
    let mut r = AnalysisResults::default();
    let mk = |path: &str, line: u32, name: &str| {
        UnusedDependencyFinding::with_actions(UnusedDependency {
            package_name: name.to_string(),
            location: DependencyLocation::Dependencies,
            path: PathBuf::from(path),
            line,
            used_in_workspaces: Vec::new(),
            declared_and_imported_in: Vec::new(),
        })
    };
    r.unused_dependencies.push(mk("b/package.json", 3, "zlib"));
    r.unused_dependencies.push(mk("a/package.json", 5, "react"));
    r.unused_dependencies.push(mk("a/package.json", 5, "axios"));
    r.sort();
    let names: Vec<_> = r
        .unused_dependencies
        .iter()
        .map(|d| d.dep.package_name.as_str())
        .collect();
    assert_eq!(names, vec!["axios", "react", "zlib"]);
}

// ── sort: unused_dev_dependencies ───────────────────────────

#[test]
fn sort_unused_dev_dependencies() {
    let mut r = AnalysisResults::default();
    r.unused_dev_dependencies
        .push(UnusedDevDependencyFinding::with_actions(UnusedDependency {
            package_name: "vitest".to_string(),
            location: DependencyLocation::DevDependencies,
            path: PathBuf::from("package.json"),
            line: 10,
            used_in_workspaces: Vec::new(),
            declared_and_imported_in: Vec::new(),
        }));
    r.unused_dev_dependencies
        .push(UnusedDevDependencyFinding::with_actions(UnusedDependency {
            package_name: "jest".to_string(),
            location: DependencyLocation::DevDependencies,
            path: PathBuf::from("package.json"),
            line: 5,
            used_in_workspaces: Vec::new(),
            declared_and_imported_in: Vec::new(),
        }));
    r.sort();
    assert_eq!(r.unused_dev_dependencies[0].dep.package_name, "jest");
    assert_eq!(r.unused_dev_dependencies[1].dep.package_name, "vitest");
}

// ── sort: unused_optional_dependencies ──────────────────────

#[test]
fn sort_unused_optional_dependencies() {
    let mut r = AnalysisResults::default();
    r.unused_optional_dependencies
        .push(UnusedOptionalDependencyFinding::with_actions(
            UnusedDependency {
                package_name: "zod".to_string(),
                location: DependencyLocation::OptionalDependencies,
                path: PathBuf::from("package.json"),
                line: 3,
                used_in_workspaces: Vec::new(),
                declared_and_imported_in: Vec::new(),
            },
        ));
    r.unused_optional_dependencies
        .push(UnusedOptionalDependencyFinding::with_actions(
            UnusedDependency {
                package_name: "ajv".to_string(),
                location: DependencyLocation::OptionalDependencies,
                path: PathBuf::from("package.json"),
                line: 2,
                used_in_workspaces: Vec::new(),
                declared_and_imported_in: Vec::new(),
            },
        ));
    r.sort();
    assert_eq!(r.unused_optional_dependencies[0].dep.package_name, "ajv");
    assert_eq!(r.unused_optional_dependencies[1].dep.package_name, "zod");
}

// ── sort: unused_enum_members by path, line, parent, member ─

#[test]
fn sort_unused_enum_members_by_path_line_parent_member() {
    let mut r = AnalysisResults::default();
    let mk = |path: &str, line: u32, parent: &str, member: &str| {
        UnusedEnumMemberFinding::with_actions(UnusedMember {
            path: PathBuf::from(path),
            parent_name: parent.to_string(),
            member_name: member.to_string(),
            kind: MemberKind::EnumMember,
            line,
            col: 0,
        })
    };
    r.unused_enum_members.push(mk("a.ts", 5, "Status", "Z"));
    r.unused_enum_members.push(mk("a.ts", 5, "Status", "A"));
    r.unused_enum_members.push(mk("a.ts", 1, "Direction", "Up"));
    r.sort();
    let keys: Vec<_> = r
        .unused_enum_members
        .iter()
        .map(|m| format!("{}:{}", m.member.parent_name, m.member.member_name))
        .collect();
    assert_eq!(keys, vec!["Direction:Up", "Status:A", "Status:Z"]);
}

// ── sort: unused_class_members by path, line, parent, member

#[test]
fn sort_unused_class_members() {
    let mut r = AnalysisResults::default();
    let mk = |path: &str, line: u32, parent: &str, member: &str| {
        UnusedClassMemberFinding::with_actions(UnusedMember {
            path: PathBuf::from(path),
            parent_name: parent.to_string(),
            member_name: member.to_string(),
            kind: MemberKind::ClassMethod,
            line,
            col: 0,
        })
    };
    r.unused_class_members.push(mk("b.ts", 1, "Foo", "z"));
    r.unused_class_members.push(mk("a.ts", 1, "Bar", "a"));
    r.sort();
    assert_eq!(r.unused_class_members[0].member.path, PathBuf::from("a.ts"));
    assert_eq!(r.unused_class_members[1].member.path, PathBuf::from("b.ts"));
}

// ── sort: unresolved_imports by path, line, col, specifier ──

#[test]
fn sort_unresolved_imports_by_path_line_col_specifier() {
    let mut r = AnalysisResults::default();
    let mk = |path: &str, line: u32, col: u32, spec: &str| {
        UnresolvedImportFinding::with_actions(UnresolvedImport {
            path: PathBuf::from(path),
            specifier: spec.to_string(),
            line,
            col,
            specifier_col: 0,
        })
    };
    r.unresolved_imports.push(mk("a.ts", 5, 0, "./z"));
    r.unresolved_imports.push(mk("a.ts", 5, 0, "./a"));
    r.unresolved_imports.push(mk("a.ts", 1, 0, "./m"));
    r.sort();
    let specs: Vec<_> = r
        .unresolved_imports
        .iter()
        .map(|i| i.import.specifier.as_str())
        .collect();
    assert_eq!(specs, vec!["./m", "./a", "./z"]);
}

// ── sort: unlisted_dependencies + inner imported_from ───────

#[test]
fn sort_unlisted_dependencies_by_name_and_inner_sites() {
    let mut r = AnalysisResults::default();
    r.unlisted_dependencies
        .push(UnlistedDependencyFinding::with_actions(
            UnlistedDependency {
                package_name: "zod".to_string(),
                imported_from: vec![
                    ImportSite {
                        path: PathBuf::from("b.ts"),
                        line: 10,
                        col: 0,
                    },
                    ImportSite {
                        path: PathBuf::from("a.ts"),
                        line: 1,
                        col: 0,
                    },
                ],
            },
        ));
    r.unlisted_dependencies
        .push(UnlistedDependencyFinding::with_actions(
            UnlistedDependency {
                package_name: "axios".to_string(),
                imported_from: vec![ImportSite {
                    path: PathBuf::from("c.ts"),
                    line: 1,
                    col: 0,
                }],
            },
        ));
    r.sort();

    // Outer sort: by package_name
    assert_eq!(r.unlisted_dependencies[0].dep.package_name, "axios");
    assert_eq!(r.unlisted_dependencies[1].dep.package_name, "zod");

    // Inner sort: imported_from sorted by path, then line
    let zod_sites: Vec<_> = r.unlisted_dependencies[1]
        .dep
        .imported_from
        .iter()
        .map(|s| s.path.to_string_lossy().to_string())
        .collect();
    assert_eq!(zod_sites, vec!["a.ts", "b.ts"]);
}

// ── sort: duplicate_exports + inner locations ───────────────

#[test]
fn sort_duplicate_exports_by_name_and_inner_locations() {
    let mut r = AnalysisResults::default();
    r.duplicate_exports
        .push(DuplicateExportFinding::with_actions(DuplicateExport {
            export_name: "z".to_string(),
            locations: vec![
                DuplicateLocation {
                    path: PathBuf::from("c.ts"),
                    line: 1,
                    col: 0,
                },
                DuplicateLocation {
                    path: PathBuf::from("a.ts"),
                    line: 5,
                    col: 0,
                },
            ],
        }));
    r.duplicate_exports
        .push(DuplicateExportFinding::with_actions(DuplicateExport {
            export_name: "a".to_string(),
            locations: vec![DuplicateLocation {
                path: PathBuf::from("b.ts"),
                line: 1,
                col: 0,
            }],
        }));
    r.sort();

    // Outer sort: by export_name
    assert_eq!(r.duplicate_exports[0].export.export_name, "a");
    assert_eq!(r.duplicate_exports[1].export.export_name, "z");

    // Inner sort: locations sorted by path, then line
    let z_locs: Vec<_> = r.duplicate_exports[1]
        .export
        .locations
        .iter()
        .map(|l| l.path.to_string_lossy().to_string())
        .collect();
    assert_eq!(z_locs, vec!["a.ts", "c.ts"]);
}

// ── sort: type_only_dependencies ────────────────────────────

#[test]
fn sort_type_only_dependencies() {
    let mut r = AnalysisResults::default();
    r.type_only_dependencies
        .push(TypeOnlyDependencyFinding::with_actions(
            TypeOnlyDependency {
                package_name: "zod".to_string(),
                path: PathBuf::from("package.json"),
                line: 10,
            },
        ));
    r.type_only_dependencies
        .push(TypeOnlyDependencyFinding::with_actions(
            TypeOnlyDependency {
                package_name: "ajv".to_string(),
                path: PathBuf::from("package.json"),
                line: 5,
            },
        ));
    r.sort();
    assert_eq!(r.type_only_dependencies[0].dep.package_name, "ajv");
    assert_eq!(r.type_only_dependencies[1].dep.package_name, "zod");
}

// ── sort: test_only_dependencies ────────────────────────────

#[test]
fn sort_test_only_dependencies() {
    let mut r = AnalysisResults::default();
    r.test_only_dependencies
        .push(TestOnlyDependencyFinding::with_actions(
            TestOnlyDependency {
                package_name: "vitest".to_string(),
                path: PathBuf::from("package.json"),
                line: 15,
            },
        ));
    r.test_only_dependencies
        .push(TestOnlyDependencyFinding::with_actions(
            TestOnlyDependency {
                package_name: "jest".to_string(),
                path: PathBuf::from("package.json"),
                line: 10,
            },
        ));
    r.sort();
    assert_eq!(r.test_only_dependencies[0].dep.package_name, "jest");
    assert_eq!(r.test_only_dependencies[1].dep.package_name, "vitest");
}

// ── sort: circular_dependencies by files, then length ───────

#[test]
fn sort_circular_dependencies_by_files_then_length() {
    let mut r = AnalysisResults::default();
    r.circular_dependencies
        .push(CircularDependencyFinding::with_actions(
            CircularDependency {
                files: vec![PathBuf::from("b.ts"), PathBuf::from("c.ts")],
                length: 2,
                line: 1,
                col: 0,
                edges: Vec::new(),
                is_cross_package: false,
            },
        ));
    r.circular_dependencies
        .push(CircularDependencyFinding::with_actions(
            CircularDependency {
                files: vec![PathBuf::from("a.ts"), PathBuf::from("b.ts")],
                length: 2,
                line: 1,
                col: 0,
                edges: Vec::new(),
                is_cross_package: true,
            },
        ));
    r.sort();
    assert_eq!(
        r.circular_dependencies[0].cycle.files[0],
        PathBuf::from("a.ts")
    );
    assert_eq!(
        r.circular_dependencies[1].cycle.files[0],
        PathBuf::from("b.ts")
    );
}

// ── sort: boundary_violations by from_path, line, col, to_path

#[test]
fn sort_boundary_violations() {
    let mut r = AnalysisResults::default();
    let mk = |from: &str, line: u32, col: u32, to: &str| {
        BoundaryViolationFinding::with_actions(BoundaryViolation {
            from_path: PathBuf::from(from),
            to_path: PathBuf::from(to),
            from_zone: "a".to_string(),
            to_zone: "b".to_string(),
            import_specifier: to.to_string(),
            line,
            col,
            via_path: None,
        })
    };
    r.boundary_violations.push(mk("z.ts", 1, 0, "a.ts"));
    r.boundary_violations.push(mk("a.ts", 5, 0, "b.ts"));
    r.boundary_violations.push(mk("a.ts", 1, 0, "c.ts"));
    r.sort();
    let from_paths: Vec<_> = r
        .boundary_violations
        .iter()
        .map(|v| {
            format!(
                "{}:{}",
                v.violation.from_path.to_string_lossy(),
                v.violation.line
            )
        })
        .collect();
    assert_eq!(from_paths, vec!["a.ts:1", "a.ts:5", "z.ts:1"]);
}

// ── sort: export_usages + inner reference_locations ─────────

#[test]
fn sort_export_usages_and_inner_reference_locations() {
    let mut r = AnalysisResults::default();
    r.export_usages.push(ExportUsage {
        path: PathBuf::from("z.ts"),
        export_name: "foo".to_string(),
        line: 1,
        col: 0,
        reference_count: 2,
        reference_locations: vec![
            ReferenceLocation {
                path: PathBuf::from("c.ts"),
                line: 10,
                col: 0,
            },
            ReferenceLocation {
                path: PathBuf::from("a.ts"),
                line: 5,
                col: 0,
            },
        ],
    });
    r.export_usages.push(ExportUsage {
        path: PathBuf::from("a.ts"),
        export_name: "bar".to_string(),
        line: 1,
        col: 0,
        reference_count: 1,
        reference_locations: vec![ReferenceLocation {
            path: PathBuf::from("b.ts"),
            line: 1,
            col: 0,
        }],
    });
    r.sort();

    // Outer sort: by path, then line, then export_name
    assert_eq!(r.export_usages[0].path, PathBuf::from("a.ts"));
    assert_eq!(r.export_usages[1].path, PathBuf::from("z.ts"));

    // Inner sort: reference_locations sorted by path, line, col
    let refs: Vec<_> = r.export_usages[1]
        .reference_locations
        .iter()
        .map(|l| l.path.to_string_lossy().to_string())
        .collect();
    assert_eq!(refs, vec!["a.ts", "c.ts"]);
}

// ── serialization ──────────────────────────────────────────

#[test]
fn serialize_empty_results() {
    let r = AnalysisResults::default();
    let json = serde_json::to_value(&r).unwrap();

    // All arrays should be present and empty
    assert!(json["unused_files"].as_array().unwrap().is_empty());
    assert!(json["unused_exports"].as_array().unwrap().is_empty());
    assert!(json["circular_dependencies"].as_array().unwrap().is_empty());

    // Skipped fields should be absent
    assert!(json.get("export_usages").is_none());
    assert!(json.get("entry_point_summary").is_none());
}

#[test]
fn serialize_unused_file_path() {
    let r = UnusedFile {
        path: PathBuf::from("src/utils/index.ts"),
    };
    let json = serde_json::to_value(&r).unwrap();
    assert_eq!(json["path"], "src/utils/index.ts");
}

#[test]
fn serialize_dependency_location_camel_case() {
    let dep = UnusedDependency {
        package_name: "react".to_string(),
        location: DependencyLocation::DevDependencies,
        path: PathBuf::from("package.json"),
        line: 5,
        used_in_workspaces: Vec::new(),
        declared_and_imported_in: Vec::new(),
    };
    let json = serde_json::to_value(&dep).unwrap();
    assert_eq!(json["location"], "devDependencies");

    let dep2 = UnusedDependency {
        package_name: "react".to_string(),
        location: DependencyLocation::Dependencies,
        path: PathBuf::from("package.json"),
        line: 3,
        used_in_workspaces: Vec::new(),
        declared_and_imported_in: Vec::new(),
    };
    let json2 = serde_json::to_value(&dep2).unwrap();
    assert_eq!(json2["location"], "dependencies");

    let dep3 = UnusedDependency {
        package_name: "fsevents".to_string(),
        location: DependencyLocation::OptionalDependencies,
        path: PathBuf::from("package.json"),
        line: 7,
        used_in_workspaces: Vec::new(),
        declared_and_imported_in: Vec::new(),
    };
    let json3 = serde_json::to_value(&dep3).unwrap();
    assert_eq!(json3["location"], "optionalDependencies");
}

#[test]
fn serialize_circular_dependency_skips_false_cross_package() {
    let cd = CircularDependency {
        files: vec![PathBuf::from("a.ts"), PathBuf::from("b.ts")],
        length: 2,
        line: 1,
        col: 0,
        edges: Vec::new(),
        is_cross_package: false,
    };
    let json = serde_json::to_value(&cd).unwrap();
    // skip_serializing_if = "std::ops::Not::not" means false is skipped
    assert!(json.get("is_cross_package").is_none());
}

#[test]
fn serialize_circular_dependency_includes_true_cross_package() {
    let cd = CircularDependency {
        files: vec![PathBuf::from("a.ts"), PathBuf::from("b.ts")],
        length: 2,
        line: 1,
        col: 0,
        edges: Vec::new(),
        is_cross_package: true,
    };
    let json = serde_json::to_value(&cd).unwrap();
    assert_eq!(json["is_cross_package"], true);
}

#[test]
fn serialize_unused_export_fields() {
    let e = UnusedExport {
        path: PathBuf::from("src/mod.ts"),
        export_name: "helper".to_string(),
        is_type_only: true,
        line: 42,
        col: 7,
        span_start: 100,
        is_re_export: true,
        deprecated: false,
        deprecated_reason: None,
    };
    let json = serde_json::to_value(&e).unwrap();
    assert_eq!(json["path"], "src/mod.ts");
    assert_eq!(json["export_name"], "helper");
    assert_eq!(json["is_type_only"], true);
    assert_eq!(json["line"], 42);
    assert_eq!(json["col"], 7);
    assert_eq!(json["span_start"], 100);
    assert_eq!(json["is_re_export"], true);
}

#[test]
fn serialize_boundary_violation_fields() {
    let v = BoundaryViolation {
        from_path: PathBuf::from("src/ui/button.tsx"),
        to_path: PathBuf::from("src/db/queries.ts"),
        from_zone: "ui".to_string(),
        to_zone: "db".to_string(),
        import_specifier: "../db/queries".to_string(),
        line: 3,
        col: 0,
        via_path: None,
    };
    let json = serde_json::to_value(&v).unwrap();
    assert_eq!(json["from_path"], "src/ui/button.tsx");
    assert_eq!(json["to_path"], "src/db/queries.ts");
    assert_eq!(json["from_zone"], "ui");
    assert_eq!(json["to_zone"], "db");
    assert_eq!(json["import_specifier"], "../db/queries");
}

#[test]
fn serialize_unlisted_dependency_with_import_sites() {
    let d = UnlistedDependency {
        package_name: "chalk".to_string(),
        imported_from: vec![
            ImportSite {
                path: PathBuf::from("a.ts"),
                line: 1,
                col: 0,
            },
            ImportSite {
                path: PathBuf::from("b.ts"),
                line: 5,
                col: 3,
            },
        ],
    };
    let json = serde_json::to_value(&d).unwrap();
    assert_eq!(json["package_name"], "chalk");
    let sites = json["imported_from"].as_array().unwrap();
    assert_eq!(sites.len(), 2);
    assert_eq!(sites[0]["path"], "a.ts");
    assert_eq!(sites[1]["line"], 5);
}

#[test]
fn serialize_duplicate_export_with_locations() {
    let d = DuplicateExport {
        export_name: "Button".to_string(),
        locations: vec![
            DuplicateLocation {
                path: PathBuf::from("src/a.ts"),
                line: 10,
                col: 0,
            },
            DuplicateLocation {
                path: PathBuf::from("src/b.ts"),
                line: 20,
                col: 5,
            },
        ],
    };
    let json = serde_json::to_value(&d).unwrap();
    assert_eq!(json["export_name"], "Button");
    let locs = json["locations"].as_array().unwrap();
    assert_eq!(locs.len(), 2);
    assert_eq!(locs[0]["line"], 10);
    assert_eq!(locs[1]["col"], 5);
}

#[test]
fn serialize_type_only_dependency() {
    let d = TypeOnlyDependency {
        package_name: "@types/react".to_string(),
        path: PathBuf::from("package.json"),
        line: 12,
    };
    let json = serde_json::to_value(&d).unwrap();
    assert_eq!(json["package_name"], "@types/react");
    assert_eq!(json["line"], 12);
}

#[test]
fn serialize_test_only_dependency() {
    let d = TestOnlyDependency {
        package_name: "vitest".to_string(),
        path: PathBuf::from("package.json"),
        line: 8,
    };
    let json = serde_json::to_value(&d).unwrap();
    assert_eq!(json["package_name"], "vitest");
    assert_eq!(json["line"], 8);
}

#[test]
fn serialize_unused_member() {
    let m = UnusedMember {
        path: PathBuf::from("enums.ts"),
        parent_name: "Status".to_string(),
        member_name: "Pending".to_string(),
        kind: MemberKind::EnumMember,
        line: 3,
        col: 4,
    };
    let json = serde_json::to_value(&m).unwrap();
    assert_eq!(json["parent_name"], "Status");
    assert_eq!(json["member_name"], "Pending");
    assert_eq!(json["line"], 3);
}

#[test]
fn serialize_unresolved_import() {
    let i = UnresolvedImport {
        path: PathBuf::from("app.ts"),
        specifier: "./missing-module".to_string(),
        line: 7,
        col: 0,
        specifier_col: 21,
    };
    let json = serde_json::to_value(&i).unwrap();
    assert_eq!(json["specifier"], "./missing-module");
    assert_eq!(json["specifier_col"], 21);
}

// ── deserialize: CircularDependency serde(default) fields ──

#[test]
fn deserialize_circular_dependency_with_defaults() {
    // CircularDependency derives Deserialize; line/col/is_cross_package have #[serde(default)]
    let json = r#"{"files":["a.ts","b.ts"],"length":2}"#;
    let cd: CircularDependency = serde_json::from_str(json).unwrap();
    assert_eq!(cd.files.len(), 2);
    assert_eq!(cd.length, 2);
    assert_eq!(cd.line, 0);
    assert_eq!(cd.col, 0);
    assert!(!cd.is_cross_package);
}

#[test]
fn deserialize_circular_dependency_with_all_fields() {
    let json = r#"{"files":["a.ts","b.ts"],"length":2,"line":5,"col":10,"is_cross_package":true}"#;
    let cd: CircularDependency = serde_json::from_str(json).unwrap();
    assert_eq!(cd.line, 5);
    assert_eq!(cd.col, 10);
    assert!(cd.is_cross_package);
}

// ── clone produces independent copies ───────────────────────

fn protected_architecture_findings(path: &Path) -> AnalysisResults {
    AnalysisResults {
        boundary_violations: vec![BoundaryViolationFinding::with_actions(BoundaryViolation {
            from_path: path.to_path_buf(),
            to_path: PathBuf::from("src/target.ts"),
            from_zone: "ui".to_string(),
            to_zone: "data".to_string(),
            import_specifier: "../target".to_string(),
            line: 1,
            col: 0,
            via_path: None,
        })],
        boundary_coverage_violations: vec![BoundaryCoverageViolationFinding::with_actions(
            BoundaryCoverageViolation {
                path: path.to_path_buf(),
                line: 1,
                col: 0,
            },
        )],
        boundary_call_violations: vec![BoundaryCallViolationFinding::with_actions(
            BoundaryCallViolation {
                path: path.to_path_buf(),
                line: 1,
                col: 0,
                zone: "ui".to_string(),
                callee: "cp.exec".to_string(),
                pattern: "child_process.*".to_string(),
            },
        )],
        policy_violations: vec![PolicyViolationFinding::with_actions(PolicyViolation {
            path: path.to_path_buf(),
            line: 1,
            col: 0,
            pack: "security".to_string(),
            rule_id: "no-eval".to_string(),
            kind: PolicyRuleKind::BannedCall,
            matched: "eval".to_string(),
            severity: PolicyViolationSeverity::Error,
            message: None,
        })],
        stale_suppressions: vec![StaleSuppression {
            finding_id: None,
            path: path.to_path_buf(),
            line: 1,
            col: 0,
            origin: SuppressionOrigin::Comment {
                issue_kind: Some("unused-file".to_string()),
                reason: None,
                is_file_level: false,
                kind_known: true,
            },
            missing_reason: false,
            actions: StaleSuppression::actions_for(false),
            effective_severity: None,
        }],
        ..AnalysisResults::default()
    }
}

fn protected_framework_findings() -> AnalysisResults {
    AnalysisResults {
        invalid_client_exports: vec![InvalidClientExportFinding::with_actions(
            InvalidClientExport {
                path: PathBuf::from("ignored/client.ts"),
                export_name: "metadata".to_string(),
                directive: "use client".to_string(),
                line: 1,
                col: 0,
            },
        )],
        mixed_client_server_barrels: vec![MixedClientServerBarrelFinding::with_actions(
            MixedClientServerBarrel {
                path: PathBuf::from("ignored/barrel.ts"),
                client_origin: "./client".to_string(),
                server_origin: "./server".to_string(),
                line: 1,
                col: 0,
            },
        )],
        misplaced_directives: vec![MisplacedDirectiveFinding::with_actions(
            MisplacedDirective {
                path: PathBuf::from("ignored/directive.ts"),
                directive: "use client".to_string(),
                line: 2,
                col: 0,
            },
        )],
        route_collisions: vec![RouteCollisionFinding::with_actions(RouteCollision {
            path: PathBuf::from("ignored/app/about/page.tsx"),
            url: "/about".to_string(),
            conflicting_paths: vec![PathBuf::from("src/app/about/page.tsx")],
            line: 1,
            col: 0,
        })],
        dynamic_segment_name_conflicts: vec![DynamicSegmentNameConflictFinding::with_actions(
            DynamicSegmentNameConflict {
                path: PathBuf::from("ignored/app/shop/[id]/page.tsx"),
                position: "/shop".to_string(),
                conflicting_segments: vec!["[id]".to_string(), "[slug]".to_string()],
                conflicting_paths: vec![PathBuf::from("src/app/shop/[slug]/page.tsx")],
                line: 1,
                col: 0,
            },
        )],
        ..AnalysisResults::default()
    }
}

#[test]
fn finding_ignore_hides_dead_code_but_retains_protected_findings() {
    let ignored_path = PathBuf::from("ignored/dead.ts");
    let mut results = protected_architecture_findings(&ignored_path);
    results.merge_into(protected_framework_findings());
    results.unused_files = vec![
        UnusedFileFinding::with_actions(UnusedFile { path: ignored_path }),
        UnusedFileFinding::with_actions(UnusedFile {
            path: PathBuf::from("src/visible.ts"),
        }),
    ];

    results.remove_ignored_dead_code_findings(|path| path.starts_with("ignored"));

    assert_eq!(results.unused_files.len(), 1);
    assert_eq!(
        results.unused_files[0].file.path,
        PathBuf::from("src/visible.ts")
    );
    assert_eq!(results.boundary_violations.len(), 1);
    assert_eq!(results.boundary_coverage_violations.len(), 1);
    assert_eq!(results.boundary_call_violations.len(), 1);
    assert_eq!(results.policy_violations.len(), 1);
    assert_eq!(results.stale_suppressions.len(), 1);
    assert_eq!(results.invalid_client_exports.len(), 1);
    assert_eq!(results.mixed_client_server_barrels.len(), 1);
    assert_eq!(results.misplaced_directives.len(), 1);
    assert_eq!(results.route_collisions.len(), 1);
    assert_eq!(results.dynamic_segment_name_conflicts.len(), 1);
}

#[test]
fn finding_ignore_requires_every_source_owner_to_match() {
    let duplicate = |paths: &[&str]| {
        DuplicateExportFinding::with_actions(DuplicateExport {
            export_name: "shared".to_string(),
            locations: paths
                .iter()
                .map(|path| DuplicateLocation {
                    path: PathBuf::from(path),
                    line: 1,
                    col: 0,
                })
                .collect(),
        })
    };
    let mut results = AnalysisResults {
        duplicate_exports: vec![
            duplicate(&["ignored/a.ts", "ignored/b.ts"]),
            duplicate(&["ignored/a.ts", "src/b.ts"]),
            duplicate(&[]),
        ],
        ..AnalysisResults::default()
    };

    results.remove_ignored_dead_code_findings(|path| path.starts_with("ignored"));

    assert_eq!(results.duplicate_exports.len(), 2);
    assert_eq!(results.duplicate_exports[0].export.locations.len(), 2);
    assert!(results.duplicate_exports[1].export.locations.is_empty());
}

#[test]
fn finding_ignore_retains_unowned_package_issues() {
    let mut results = AnalysisResults {
        unused_dependencies: vec![UnusedDependencyFinding::with_actions(UnusedDependency {
            package_name: "unused-package".to_string(),
            location: DependencyLocation::Dependencies,
            path: PathBuf::from("ignored/package.json"),
            line: 3,
            used_in_workspaces: vec![],
            declared_and_imported_in: Vec::new(),
        })],
        ..AnalysisResults::default()
    };

    results.remove_ignored_dead_code_findings(|path| path.starts_with("ignored"));

    assert_eq!(results.unused_dependencies.len(), 1);
}

fn thin_wrapper_finding(path: &str) -> ThinWrapperFinding {
    ThinWrapperFinding::with_actions(ThinWrapper {
        file: PathBuf::from(path),
        line: 1,
        component: "Wrapper".to_string(),
        child_component: "Child".to_string(),
    })
}

fn duplicate_prop_shape_finding(path: &str) -> DuplicatePropShapeFinding {
    DuplicatePropShapeFinding::with_actions(DuplicatePropShape {
        file: PathBuf::from(path),
        line: 1,
        component: "Card".to_string(),
        shape: vec!["title".to_string(), "subtitle".to_string()],
        group_size: 3,
        sharing_components: vec![],
    })
}

fn prop_drilling_chain_finding(paths: &[&str]) -> PropDrillingChainFinding {
    PropDrillingChainFinding::with_actions(PropDrillingChain {
        prop: "user".to_string(),
        depth: paths.len() as u32,
        hops: paths
            .iter()
            .map(|path| PropDrillHop {
                file: PathBuf::from(path),
                line: 1,
                component: "Hop".to_string(),
            })
            .collect(),
    })
}

#[test]
fn finding_ignore_hides_thin_wrappers_by_wrapper_file() {
    let mut results = AnalysisResults {
        thin_wrappers: vec![
            thin_wrapper_finding("ignored/Wrapper.tsx"),
            thin_wrapper_finding("src/Wrapper.tsx"),
        ],
        ..AnalysisResults::default()
    };

    results.remove_ignored_dead_code_findings(|path| path.starts_with("ignored"));

    assert_eq!(results.thin_wrappers.len(), 1);
    assert_eq!(
        results.thin_wrappers[0].wrapper.file,
        PathBuf::from("src/Wrapper.tsx")
    );
}

#[test]
fn finding_ignore_hides_duplicate_prop_shapes_by_component_file() {
    let mut results = AnalysisResults {
        duplicate_prop_shapes: vec![
            duplicate_prop_shape_finding("ignored/Card.tsx"),
            duplicate_prop_shape_finding("src/Card.tsx"),
        ],
        ..AnalysisResults::default()
    };

    results.remove_ignored_dead_code_findings(|path| path.starts_with("ignored"));

    assert_eq!(results.duplicate_prop_shapes.len(), 1);
    assert_eq!(
        results.duplicate_prop_shapes[0].shape.file,
        PathBuf::from("src/Card.tsx")
    );
}

#[test]
fn finding_ignore_hides_prop_drilling_chains_only_when_every_hop_matches() {
    let mut results = AnalysisResults {
        prop_drilling_chains: vec![
            prop_drilling_chain_finding(&["ignored/a.tsx", "ignored/b.tsx"]),
            prop_drilling_chain_finding(&["ignored/a.tsx", "src/b.tsx"]),
            prop_drilling_chain_finding(&[]),
        ],
        ..AnalysisResults::default()
    };

    results.remove_ignored_dead_code_findings(|path| path.starts_with("ignored"));

    assert_eq!(results.prop_drilling_chains.len(), 2);
    assert_eq!(results.prop_drilling_chains[0].chain.hops.len(), 2);
    assert!(results.prop_drilling_chains[1].chain.hops.is_empty());
}

#[test]
fn finding_ignore_retains_security_findings_and_blind_spot_diagnostics() {
    let path = PathBuf::from("ignored/leak.ts");
    let mut results = AnalysisResults {
        security_findings: vec![SecurityFinding {
            finding_id: "id".to_string(),
            kind: SecurityFindingKind::TaintedSink,
            category: Some("dangerous-html".to_string()),
            cwe: Some(79),
            path: path.clone(),
            line: 1,
            col: 0,
            evidence: "candidate".to_string(),
            source_backed: false,
            source_read: None,
            severity: SecuritySeverity::Low,
            trace: vec![TraceHop {
                path: path.clone(),
                line: 1,
                col: 0,
                role: TraceHopRole::Sink,
            }],
            actions: vec![],
            dead_code: None,
            reachability: None,
            candidate: SecurityCandidate {
                source_kind: None,
                sink: SecurityCandidateSink {
                    path: path.clone(),
                    line: 1,
                    col: 0,
                    category: Some("dangerous-html".to_string()),
                    cwe: Some(79),
                    callee: None,
                    url_shape: None,
                },
                boundary: SecurityCandidateBoundary::default(),
                network: None,
            },
            taint_flow: None,
            runtime: None,
            attack_surface: None,
        }],
        security_unresolved_callee_diagnostics: vec![SecurityUnresolvedCalleeDiagnostic {
            path,
            line: 1,
            col: 0,
            reason: SkippedSecurityCalleeReason::DynamicDispatch,
            expression_kind: SkippedSecurityCalleeExpressionKind::ComputedMemberExpression,
        }],
        ..AnalysisResults::default()
    };

    results.remove_ignored_dead_code_findings(|path| path.starts_with("ignored"));

    assert_eq!(results.security_findings.len(), 1);
    assert_eq!(results.security_unresolved_callee_diagnostics.len(), 1);
}

// ── export_usages not counted in total_issues ───────────────

#[test]
fn export_usages_not_counted_in_total_issues() {
    let mut r = AnalysisResults::default();
    r.export_usages.push(ExportUsage {
        path: PathBuf::from("mod.ts"),
        export_name: "foo".to_string(),
        line: 1,
        col: 0,
        reference_count: 3,
        reference_locations: vec![],
    });
    // export_usages is metadata, not an issue type
    assert_eq!(r.total_issues(), 0);
    assert!(!r.has_issues());
}

// ── entry_point_summary not counted in total_issues ─────────

#[test]
fn entry_point_summary_not_counted_in_total_issues() {
    let r = AnalysisResults {
        entry_point_summary: Some(EntryPointSummary {
            total: 10,
            by_source: vec![("config".to_string(), 10)],
        }),
        ..AnalysisResults::default()
    };
    assert_eq!(r.total_issues(), 0);
    assert!(!r.has_issues());
}
