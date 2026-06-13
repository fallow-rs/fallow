use std::path::Path;

use fallow_core::extract::MemberKind;
use fallow_core::results::*;
use fallow_types::output_dead_code::{
    BoundaryViolationFinding, CircularDependencyFinding, TestOnlyDependencyFinding,
    TypeOnlyDependencyFinding, UnlistedDependencyFinding, UnresolvedImportFinding,
    UnusedClassMemberFinding, UnusedDependencyFinding, UnusedDevDependencyFinding,
    UnusedEnumMemberFinding, UnusedExportFinding, UnusedFileFinding,
    UnusedOptionalDependencyFinding, UnusedStoreMemberFinding, UnusedTypeFinding,
};

/// Build an `AnalysisResults` populated with one issue of every type.
///
/// Shared across all report format tests for consistency.
#[expect(
    clippy::too_many_lines,
    reason = "flat one-of-each fixture; splitting would harm readability"
)]
pub fn sample_results(root: &Path) -> AnalysisResults {
    let mut r = AnalysisResults::default();

    r.unused_files
        .push(UnusedFileFinding::with_actions(UnusedFile {
            path: root.join("src/dead.ts"),
        }));
    r.unused_exports
        .push(UnusedExportFinding::with_actions(UnusedExport {
            path: root.join("src/utils.ts"),
            export_name: "helperFn".to_string(),
            is_type_only: false,
            line: 10,
            col: 4,
            span_start: 120,
            is_re_export: false,
        }));
    r.unused_types
        .push(UnusedTypeFinding::with_actions(UnusedExport {
            path: root.join("src/types.ts"),
            export_name: "OldType".to_string(),
            is_type_only: true,
            line: 5,
            col: 0,
            span_start: 60,
            is_re_export: false,
        }));
    r.unused_dependencies
        .push(UnusedDependencyFinding::with_actions(UnusedDependency {
            package_name: "lodash".to_string(),
            location: DependencyLocation::Dependencies,
            path: root.join("package.json"),
            line: 5,
            used_in_workspaces: Vec::new(),
        }));
    r.unused_dev_dependencies
        .push(UnusedDevDependencyFinding::with_actions(UnusedDependency {
            package_name: "jest".to_string(),
            location: DependencyLocation::DevDependencies,
            path: root.join("package.json"),
            line: 5,
            used_in_workspaces: Vec::new(),
        }));
    r.unused_optional_dependencies
        .push(UnusedOptionalDependencyFinding::with_actions(
            UnusedDependency {
                package_name: "fsevents".to_string(),
                location: DependencyLocation::OptionalDependencies,
                path: root.join("package.json"),
                line: 15,
                used_in_workspaces: Vec::new(),
            },
        ));
    r.unused_enum_members
        .push(UnusedEnumMemberFinding::with_actions(UnusedMember {
            path: root.join("src/enums.ts"),
            parent_name: "Status".to_string(),
            member_name: "Deprecated".to_string(),
            kind: MemberKind::EnumMember,
            line: 8,
            col: 2,
        }));
    r.unused_class_members
        .push(UnusedClassMemberFinding::with_actions(UnusedMember {
            path: root.join("src/service.ts"),
            parent_name: "UserService".to_string(),
            member_name: "legacyMethod".to_string(),
            kind: MemberKind::ClassMethod,
            line: 42,
            col: 4,
        }));
    r.unused_store_members
        .push(UnusedStoreMemberFinding::with_actions(UnusedMember {
            path: root.join("src/store.ts"),
            parent_name: "useStore".to_string(),
            member_name: "legacyAction".to_string(),
            kind: MemberKind::StoreMember,
            line: 24,
            col: 2,
        }));
    r.unresolved_imports
        .push(UnresolvedImportFinding::with_actions(UnresolvedImport {
            path: root.join("src/app.ts"),
            specifier: "./missing-module".to_string(),
            line: 3,
            col: 0,
            specifier_col: 0,
        }));
    r.unlisted_dependencies
        .push(UnlistedDependencyFinding::with_actions(
            UnlistedDependency {
                package_name: "chalk".to_string(),
                imported_from: vec![ImportSite {
                    path: root.join("src/cli.ts"),
                    line: 2,
                    col: 0,
                }],
            },
        ));
    r.duplicate_exports
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
    r.type_only_dependencies
        .push(TypeOnlyDependencyFinding::with_actions(
            TypeOnlyDependency {
                package_name: "zod".to_string(),
                path: root.join("package.json"),
                line: 8,
            },
        ));
    r.test_only_dependencies
        .push(TestOnlyDependencyFinding::with_actions(
            TestOnlyDependency {
                package_name: "msw".to_string(),
                path: root.join("package.json"),
                line: 12,
            },
        ));
    r.circular_dependencies
        .push(CircularDependencyFinding::with_actions(
            CircularDependency {
                files: vec![root.join("src/a.ts"), root.join("src/b.ts")],
                length: 2,
                line: 3,
                col: 0,
                edges: Vec::new(),
                is_cross_package: false,
            },
        ));
    r.boundary_violations
        .push(BoundaryViolationFinding::with_actions(BoundaryViolation {
            from_path: root.join("src/ui/Button.tsx"),
            to_path: root.join("src/db/query.ts"),
            from_zone: "ui".to_string(),
            to_zone: "db".to_string(),
            import_specifier: "src/db/query.ts".to_string(),
            line: 2,
            col: 0,
        }));
    r.stale_suppressions.push(StaleSuppression {
        path: root.join("src/utils.ts"),
        line: 5,
        col: 0,
        origin: SuppressionOrigin::Comment {
            issue_kind: Some("unused-exports".to_string()),
            is_file_level: false,
            kind_known: false,
        },
    });

    r
}
