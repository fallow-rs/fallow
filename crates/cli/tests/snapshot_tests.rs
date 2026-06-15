#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

use std::path::{Path, PathBuf};
use std::time::Duration;

use fallow_cli::health_types::*;
use fallow_cli::report::{
    build_codeclimate, build_compact_lines, build_duplication_codeclimate,
    build_duplication_markdown, build_grouped_duplication_json, build_health_codeclimate,
    build_health_json, build_health_markdown, build_health_sarif, build_json, build_markdown,
    build_sarif,
    ci::{
        pr_comment::{Provider, issues_from_codeclimate, render_pr_comment},
        review::render_review_envelope,
    },
    codeclimate_issues_to_value,
};
use fallow_config::RulesConfig;
use fallow_core::duplicates::{CloneGroup, CloneInstance, DuplicationReport, DuplicationStats};
use fallow_core::extract::MemberKind;
use fallow_core::results::*;

/// Build sample `AnalysisResults` with one issue of each type for consistent snapshots.
#[expect(
    clippy::too_many_lines,
    reason = "One block per issue type used across the snapshot suite; the line count grows with new issue types and the structure is intentional."
)]
fn sample_results(root: &Path) -> AnalysisResults {
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
            path: root.join("src/stores/counter.ts"),
            parent_name: "useCounterStore".to_string(),
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
    r.unused_optional_dependencies
        .push(UnusedOptionalDependencyFinding::with_actions(
            UnusedDependency {
                package_name: "fsevents".to_string(),
                location: DependencyLocation::OptionalDependencies,
                path: root.join("package.json"),
                line: 5,
                used_in_workspaces: Vec::new(),
            },
        ));
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
    r.re_export_cycles
        .push(ReExportCycleFinding::with_actions(ReExportCycle {
            files: vec![
                root.join("src/api/index.ts"),
                root.join("src/api/internal/index.ts"),
            ],
            kind: ReExportCycleKind::MultiNode,
        }));
    r.unused_catalog_entries.push(
        fallow_core::results::UnusedCatalogEntryFinding::with_actions(
            fallow_core::results::UnusedCatalogEntry {
                entry_name: "is-even".to_string(),
                catalog_name: "default".to_string(),
                path: PathBuf::from("pnpm-workspace.yaml"),
                line: 6,
                hardcoded_consumers: vec![],
            },
        ),
    );
    r.unused_catalog_entries.push(
        fallow_core::results::UnusedCatalogEntryFinding::with_actions(
            fallow_core::results::UnusedCatalogEntry {
                entry_name: "old-thing".to_string(),
                catalog_name: "legacy".to_string(),
                path: PathBuf::from("pnpm-workspace.yaml"),
                line: 12,
                hardcoded_consumers: vec![PathBuf::from("apps/web/package.json")],
            },
        ),
    );
    r.empty_catalog_groups.push(
        fallow_core::results::EmptyCatalogGroupFinding::with_actions(
            fallow_core::results::EmptyCatalogGroup {
                catalog_name: "react17".to_string(),
                path: PathBuf::from("pnpm-workspace.yaml"),
                line: 10,
            },
        ),
    );
    r.unresolved_catalog_references.push(
        fallow_core::results::UnresolvedCatalogReferenceFinding::with_actions(
            fallow_core::results::UnresolvedCatalogReference {
                entry_name: "old-react".to_string(),
                catalog_name: "react17".to_string(),
                path: root.join("packages/app/package.json"),
                line: 14,
                available_in_catalogs: vec!["react18".to_string()],
            },
        ),
    );
    r.unused_dependency_overrides.push(
        fallow_core::results::UnusedDependencyOverrideFinding::with_actions(
            fallow_core::results::UnusedDependencyOverride {
                raw_key: "axios".to_string(),
                target_package: "axios".to_string(),
                parent_package: None,
                version_constraint: None,
                version_range: "^1.6.0".to_string(),
                source: fallow_core::results::DependencyOverrideSource::PnpmWorkspaceYaml,
                path: root.join("pnpm-workspace.yaml"),
                line: 9,
                hint: Some(
                    "may target a transitive dependency; pnpm install --frozen-lockfile is the ground truth"
                        .to_string(),
                ),
            },
        ),
    );
    r.misconfigured_dependency_overrides.push(
        fallow_core::results::MisconfiguredDependencyOverrideFinding::with_actions(
            fallow_core::results::MisconfiguredDependencyOverride {
                raw_key: "@types/react@<<18".to_string(),
                target_package: None,
                raw_value: "18.0.0".to_string(),
                reason: fallow_core::results::DependencyOverrideMisconfigReason::UnparsableKey,
                source: fallow_core::results::DependencyOverrideSource::PnpmPackageJson,
                path: root.join("package.json"),
                line: 3,
            },
        ),
    );
    r.invalid_client_exports.push(
        fallow_core::results::InvalidClientExportFinding::with_actions(
            fallow_core::results::InvalidClientExport {
                path: root.join("app/page.tsx"),
                export_name: "metadata".to_string(),
                directive: "use client".to_string(),
                line: 3,
                col: 0,
            },
        ),
    );
    r.mixed_client_server_barrels.push(
        fallow_core::results::MixedClientServerBarrelFinding::with_actions(
            fallow_core::results::MixedClientServerBarrel {
                path: root.join("app/components/index.ts"),
                client_origin: "./Button".to_string(),
                server_origin: "./fetchUser".to_string(),
                line: 2,
                col: 0,
            },
        ),
    );
    r.unprovided_injects
        .push(fallow_core::results::UnprovidedInjectFinding::with_actions(
            fallow_core::results::UnprovidedInject {
                path: root.join("src/useTheme.ts"),
                key_name: "THEME_KEY".to_string(),
                framework: "vue".to_string(),
                line: 5,
                col: 2,
            },
        ));

    r
}

#[test]
fn json_output_snapshot() {
    let root = PathBuf::from("/project");
    let results = sample_results(&root);
    let elapsed = Duration::from_millis(42);
    let value = build_json(&results, &root, elapsed).expect("JSON build should succeed");
    let json_str = serde_json::to_string_pretty(&value).expect("should serialize");

    insta::assert_snapshot!(
        "json_output",
        json_str.replace(
            &format!("\"version\": \"{}\"", env!("CARGO_PKG_VERSION")),
            "\"version\": \"[VERSION]\"",
        )
    );
}

#[test]
fn next_steps_are_read_only_and_placeholder_free() {
    // The dead-code JSON path emits `next_steps` when findings exist. Every
    // emitted command must be runnable as-is (no `<...>` placeholder) and never
    // mutating (`fix`/`init`/`hooks`/`migrate`), per the NextStep contract.
    let root = PathBuf::from("/project");
    let results = sample_results(&root);
    let value = build_json(&results, &root, Duration::ZERO).expect("JSON build should succeed");
    let steps = value
        .get("next_steps")
        .and_then(serde_json::Value::as_array)
        .expect("dead-code output carries next_steps when findings exist");
    assert!(!steps.is_empty(), "expected at least one next-step");
    assert_eq!(
        steps[0]["id"], "trace-unused-export",
        "the verification path leads the priority order"
    );
    for step in steps {
        let command = step["command"].as_str().expect("command is a string");
        assert!(
            !command.contains('<') && !command.contains('>'),
            "command must be placeholder-free: {command}"
        );
        for verb in ["fix", "init", "hooks", "migrate", "setup-hooks"] {
            assert!(
                !command.split_whitespace().any(|token| token == verb),
                "command must be read-only: {command}"
            );
        }
        assert!(
            step["reason"].as_str().is_some_and(|r| !r.is_empty()),
            "each next-step carries a non-empty reason"
        );
    }
}

#[test]
fn json_empty_results_snapshot() {
    let root = PathBuf::from("/project");
    let results = AnalysisResults::default();
    let elapsed = Duration::from_millis(0);
    let value = build_json(&results, &root, elapsed).expect("JSON build should succeed");
    let json_str = serde_json::to_string_pretty(&value).expect("should serialize");

    insta::assert_snapshot!(
        "json_empty",
        json_str.replace(
            &format!("\"version\": \"{}\"", env!("CARGO_PKG_VERSION")),
            "\"version\": \"[VERSION]\"",
        )
    );
}

#[test]
fn sarif_output_snapshot() {
    let root = PathBuf::from("/project");
    let results = sample_results(&root);
    let rules = RulesConfig::default();
    let sarif = build_sarif(&results, &root, &rules);
    let json_str = serde_json::to_string_pretty(&sarif).expect("should serialize");

    insta::assert_snapshot!("sarif_output", redact_sarif_version(&json_str));
}

#[test]
fn sarif_empty_results_snapshot() {
    let root = PathBuf::from("/project");
    let results = AnalysisResults::default();
    let rules = RulesConfig::default();
    let sarif = build_sarif(&results, &root, &rules);
    let json_str = serde_json::to_string_pretty(&sarif).expect("should serialize");

    insta::assert_snapshot!("sarif_empty", redact_sarif_version(&json_str));
}

#[test]
fn compact_output_snapshot() {
    let root = PathBuf::from("/project");
    let results = sample_results(&root);
    let lines = build_compact_lines(&results, &root);
    let output = lines.join("\n");

    insta::assert_snapshot!("compact_output", output);
}

#[test]
fn compact_empty_results_snapshot() {
    let root = PathBuf::from("/project");
    let results = AnalysisResults::default();
    let lines = build_compact_lines(&results, &root);
    let output = lines.join("\n");

    insta::assert_snapshot!("compact_empty", output);
}

#[test]
fn compact_unused_files_only_snapshot() {
    let root = PathBuf::from("/project");
    let mut results = AnalysisResults::default();
    results
        .unused_files
        .push(UnusedFileFinding::with_actions(UnusedFile {
            path: root.join("src/dead.ts"),
        }));
    results
        .unused_files
        .push(UnusedFileFinding::with_actions(UnusedFile {
            path: root.join("src/orphan.ts"),
        }));
    let lines = build_compact_lines(&results, &root);
    insta::assert_snapshot!("compact_unused_files_only", lines.join("\n"));
}

#[test]
fn compact_unused_exports_only_snapshot() {
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
        }));
    results
        .unused_exports
        .push(UnusedExportFinding::with_actions(UnusedExport {
            path: root.join("src/utils.ts"),
            export_name: "formatDate".to_string(),
            is_type_only: false,
            line: 25,
            col: 0,
            span_start: 300,
            is_re_export: false,
        }));
    let lines = build_compact_lines(&results, &root);
    insta::assert_snapshot!("compact_unused_exports_only", lines.join("\n"));
}

#[test]
fn compact_unused_types_only_snapshot() {
    let root = PathBuf::from("/project");
    let mut results = AnalysisResults::default();
    results
        .unused_types
        .push(UnusedTypeFinding::with_actions(UnusedExport {
            path: root.join("src/types.ts"),
            export_name: "OldType".to_string(),
            is_type_only: true,
            line: 5,
            col: 0,
            span_start: 60,
            is_re_export: false,
        }));
    let lines = build_compact_lines(&results, &root);
    insta::assert_snapshot!("compact_unused_types_only", lines.join("\n"));
}

#[test]
fn compact_unused_deps_only_snapshot() {
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
        }));
    results
        .unused_dependencies
        .push(UnusedDependencyFinding::with_actions(UnusedDependency {
            package_name: "moment".to_string(),
            location: DependencyLocation::Dependencies,
            path: root.join("package.json"),
            line: 5,
            used_in_workspaces: Vec::new(),
        }));
    let lines = build_compact_lines(&results, &root);
    insta::assert_snapshot!("compact_unused_deps_only", lines.join("\n"));
}

#[test]
fn compact_unused_dev_deps_only_snapshot() {
    let root = PathBuf::from("/project");
    let mut results = AnalysisResults::default();
    results
        .unused_dev_dependencies
        .push(UnusedDevDependencyFinding::with_actions(UnusedDependency {
            package_name: "jest".to_string(),
            location: DependencyLocation::DevDependencies,
            path: root.join("package.json"),
            line: 5,
            used_in_workspaces: Vec::new(),
        }));
    let lines = build_compact_lines(&results, &root);
    insta::assert_snapshot!("compact_unused_dev_deps_only", lines.join("\n"));
}

#[test]
fn compact_unused_optional_deps_only_snapshot() {
    let root = PathBuf::from("/project");
    let mut results = AnalysisResults::default();
    results
        .unused_optional_dependencies
        .push(UnusedOptionalDependencyFinding::with_actions(
            UnusedDependency {
                package_name: "fsevents".to_string(),
                location: DependencyLocation::OptionalDependencies,
                path: root.join("package.json"),
                line: 5,
                used_in_workspaces: Vec::new(),
            },
        ));
    let lines = build_compact_lines(&results, &root);
    insta::assert_snapshot!("compact_unused_optional_deps_only", lines.join("\n"));
}

#[test]
fn compact_unresolved_imports_only_snapshot() {
    let root = PathBuf::from("/project");
    let mut results = AnalysisResults::default();
    results
        .unresolved_imports
        .push(UnresolvedImportFinding::with_actions(UnresolvedImport {
            path: root.join("src/app.ts"),
            specifier: "./missing-module".to_string(),
            line: 3,
            col: 0,
            specifier_col: 0,
        }));
    results
        .unresolved_imports
        .push(UnresolvedImportFinding::with_actions(UnresolvedImport {
            path: root.join("src/app.ts"),
            specifier: "@org/nonexistent".to_string(),
            line: 4,
            col: 0,
            specifier_col: 0,
        }));
    let lines = build_compact_lines(&results, &root);
    insta::assert_snapshot!("compact_unresolved_imports_only", lines.join("\n"));
}

#[test]
fn compact_unlisted_deps_only_snapshot() {
    let root = PathBuf::from("/project");
    let mut results = AnalysisResults::default();
    results
        .unlisted_dependencies
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
    let lines = build_compact_lines(&results, &root);
    insta::assert_snapshot!("compact_unlisted_deps_only", lines.join("\n"));
}

#[test]
fn compact_unused_enum_members_only_snapshot() {
    let root = PathBuf::from("/project");
    let mut results = AnalysisResults::default();
    results
        .unused_enum_members
        .push(UnusedEnumMemberFinding::with_actions(UnusedMember {
            path: root.join("src/enums.ts"),
            parent_name: "Status".to_string(),
            member_name: "Deprecated".to_string(),
            kind: MemberKind::EnumMember,
            line: 8,
            col: 2,
        }));
    let lines = build_compact_lines(&results, &root);
    insta::assert_snapshot!("compact_unused_enum_members_only", lines.join("\n"));
}

#[test]
fn compact_unused_class_members_only_snapshot() {
    let root = PathBuf::from("/project");
    let mut results = AnalysisResults::default();
    results
        .unused_class_members
        .push(UnusedClassMemberFinding::with_actions(UnusedMember {
            path: root.join("src/service.ts"),
            parent_name: "UserService".to_string(),
            member_name: "legacyMethod".to_string(),
            kind: MemberKind::ClassMethod,
            line: 42,
            col: 4,
        }));
    let lines = build_compact_lines(&results, &root);
    insta::assert_snapshot!("compact_unused_class_members_only", lines.join("\n"));
}

#[test]
fn compact_duplicate_exports_only_snapshot() {
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
    let lines = build_compact_lines(&results, &root);
    insta::assert_snapshot!("compact_duplicate_exports_only", lines.join("\n"));
}

#[test]
fn compact_re_export_variant_snapshot() {
    let root = PathBuf::from("/project");
    let mut results = AnalysisResults::default();
    results
        .unused_exports
        .push(UnusedExportFinding::with_actions(UnusedExport {
            path: root.join("src/index.ts"),
            export_name: "reExportedFn".to_string(),
            is_type_only: false,
            line: 1,
            col: 0,
            span_start: 0,
            is_re_export: true,
        }));
    results
        .unused_types
        .push(UnusedTypeFinding::with_actions(UnusedExport {
            path: root.join("src/index.ts"),
            export_name: "ReExportedType".to_string(),
            is_type_only: true,
            line: 2,
            col: 0,
            span_start: 30,
            is_re_export: true,
        }));
    let lines = build_compact_lines(&results, &root);
    insta::assert_snapshot!("compact_re_export_variants", lines.join("\n"));
}

#[test]
fn json_re_export_variant_snapshot() {
    let root = PathBuf::from("/project");
    let mut results = AnalysisResults::default();
    results
        .unused_exports
        .push(UnusedExportFinding::with_actions(UnusedExport {
            path: root.join("src/index.ts"),
            export_name: "reExportedFn".to_string(),
            is_type_only: false,
            line: 1,
            col: 0,
            span_start: 0,
            is_re_export: true,
        }));
    let elapsed = Duration::from_millis(0);
    let value = build_json(&results, &root, elapsed).expect("JSON build should succeed");
    let json_str = serde_json::to_string_pretty(&value).expect("should serialize");
    insta::assert_snapshot!(
        "json_re_export_variant",
        json_str.replace(
            &format!("\"version\": \"{}\"", env!("CARGO_PKG_VERSION")),
            "\"version\": \"[VERSION]\"",
        )
    );
}

#[test]
fn sarif_re_export_variant_snapshot() {
    let root = PathBuf::from("/project");
    let mut results = AnalysisResults::default();
    results
        .unused_exports
        .push(UnusedExportFinding::with_actions(UnusedExport {
            path: root.join("src/index.ts"),
            export_name: "reExportedFn".to_string(),
            is_type_only: false,
            line: 1,
            col: 0,
            span_start: 0,
            is_re_export: true,
        }));
    let rules = RulesConfig::default();
    let sarif = build_sarif(&results, &root, &rules);
    let json_str = serde_json::to_string_pretty(&sarif).expect("should serialize");
    insta::assert_snapshot!("sarif_re_export_variant", redact_sarif_version(&json_str));
}

#[test]
fn sarif_mixed_severity_snapshot() {
    let root = PathBuf::from("/project");
    let results = sample_results(&root);
    let rules = RulesConfig {
        unused_files: fallow_config::Severity::Error,
        unused_exports: fallow_config::Severity::Warn,
        unused_types: fallow_config::Severity::Warn,
        private_type_leaks: fallow_config::Severity::Warn,
        unused_dependencies: fallow_config::Severity::Error,
        unused_dev_dependencies: fallow_config::Severity::Warn,
        unused_optional_dependencies: fallow_config::Severity::Warn,
        unused_enum_members: fallow_config::Severity::Warn,
        unused_class_members: fallow_config::Severity::Warn,
        unused_store_members: fallow_config::Severity::Warn,
        unprovided_injects: fallow_config::Severity::Warn,
        unrendered_components: fallow_config::Severity::Warn,
        unused_component_props: fallow_config::Severity::Warn,
        unused_component_emits: fallow_config::Severity::Warn,
        unused_server_actions: fallow_config::Severity::Warn,
        unused_load_data_keys: fallow_config::Severity::Warn,
        unresolved_imports: fallow_config::Severity::Error,
        unlisted_dependencies: fallow_config::Severity::Error,
        duplicate_exports: fallow_config::Severity::Warn,
        type_only_dependencies: fallow_config::Severity::Warn,
        circular_dependencies: fallow_config::Severity::Warn,
        re_export_cycle: fallow_config::Severity::Warn,
        test_only_dependencies: fallow_config::Severity::Warn,
        boundary_violation: fallow_config::Severity::Warn,
        coverage_gaps: fallow_config::Severity::Warn,
        feature_flags: fallow_config::Severity::Off,
        stale_suppressions: fallow_config::Severity::Warn,
        unused_catalog_entries: fallow_config::Severity::Warn,
        empty_catalog_groups: fallow_config::Severity::Warn,
        unresolved_catalog_references: fallow_config::Severity::Error,
        unused_dependency_overrides: fallow_config::Severity::Warn,
        misconfigured_dependency_overrides: fallow_config::Severity::Error,
        security_client_server_leak: fallow_config::Severity::Off,
        security_sink: fallow_config::Severity::Off,
        policy_violation: fallow_config::Severity::Warn,
        invalid_client_export: fallow_config::Severity::Warn,
        mixed_client_server_barrel: fallow_config::Severity::Warn,
        misplaced_directive: fallow_config::Severity::Warn,
        route_collision: fallow_config::Severity::Warn,
        dynamic_segment_name_conflict: fallow_config::Severity::Warn,
    };
    let sarif = build_sarif(&results, &root, &rules);
    let json_str = serde_json::to_string_pretty(&sarif).expect("should serialize");
    insta::assert_snapshot!("sarif_mixed_severity", redact_sarif_version(&json_str));
}

#[test]
fn json_type_only_deps_snapshot() {
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
    results
        .type_only_dependencies
        .push(TypeOnlyDependencyFinding::with_actions(
            TypeOnlyDependency {
                package_name: "@types/react".to_string(),
                path: root.join("package.json"),
                line: 8,
            },
        ));
    let elapsed = Duration::from_millis(10);
    let value = build_json(&results, &root, elapsed).expect("JSON build should succeed");
    let json_str = serde_json::to_string_pretty(&value).expect("should serialize");
    insta::assert_snapshot!(
        "json_type_only_deps",
        json_str.replace(
            &format!("\"version\": \"{}\"", env!("CARGO_PKG_VERSION")),
            "\"version\": \"[VERSION]\"",
        )
    );
}

fn redact_version(json_str: &str) -> String {
    json_str.replace(
        &format!("\"version\": \"{}\"", env!("CARGO_PKG_VERSION")),
        "\"version\": \"[VERSION]\"",
    )
}

#[test]
fn json_unused_files_only_snapshot() {
    let root = PathBuf::from("/project");
    let mut results = AnalysisResults::default();
    results
        .unused_files
        .push(UnusedFileFinding::with_actions(UnusedFile {
            path: root.join("src/dead.ts"),
        }));
    let value = build_json(&results, &root, Duration::ZERO).expect("JSON build should succeed");
    let json_str = serde_json::to_string_pretty(&value).expect("should serialize");
    insta::assert_snapshot!("json_unused_files_only", redact_version(&json_str));
}

#[test]
fn json_unused_exports_only_snapshot() {
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
        }));
    let value = build_json(&results, &root, Duration::ZERO).expect("JSON build should succeed");
    let json_str = serde_json::to_string_pretty(&value).expect("should serialize");
    insta::assert_snapshot!("json_unused_exports_only", redact_version(&json_str));
}

#[test]
fn json_unused_types_only_snapshot() {
    let root = PathBuf::from("/project");
    let mut results = AnalysisResults::default();
    results
        .unused_types
        .push(UnusedTypeFinding::with_actions(UnusedExport {
            path: root.join("src/types.ts"),
            export_name: "OldType".to_string(),
            is_type_only: true,
            line: 5,
            col: 0,
            span_start: 60,
            is_re_export: false,
        }));
    let value = build_json(&results, &root, Duration::ZERO).expect("JSON build should succeed");
    let json_str = serde_json::to_string_pretty(&value).expect("should serialize");
    insta::assert_snapshot!("json_unused_types_only", redact_version(&json_str));
}

#[test]
fn json_unused_deps_only_snapshot() {
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
        }));
    let value = build_json(&results, &root, Duration::ZERO).expect("JSON build should succeed");
    let json_str = serde_json::to_string_pretty(&value).expect("should serialize");
    insta::assert_snapshot!("json_unused_deps_only", redact_version(&json_str));
}

#[test]
fn json_unresolved_imports_only_snapshot() {
    let root = PathBuf::from("/project");
    let mut results = AnalysisResults::default();
    results
        .unresolved_imports
        .push(UnresolvedImportFinding::with_actions(UnresolvedImport {
            path: root.join("src/app.ts"),
            specifier: "./missing-module".to_string(),
            line: 3,
            col: 0,
            specifier_col: 0,
        }));
    let value = build_json(&results, &root, Duration::ZERO).expect("JSON build should succeed");
    let json_str = serde_json::to_string_pretty(&value).expect("should serialize");
    insta::assert_snapshot!("json_unresolved_imports_only", redact_version(&json_str));
}

#[test]
fn json_unlisted_deps_only_snapshot() {
    let root = PathBuf::from("/project");
    let mut results = AnalysisResults::default();
    results
        .unlisted_dependencies
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
    let value = build_json(&results, &root, Duration::ZERO).expect("JSON build should succeed");
    let json_str = serde_json::to_string_pretty(&value).expect("should serialize");
    insta::assert_snapshot!("json_unlisted_deps_only", redact_version(&json_str));
}

#[test]
fn json_unused_enum_members_only_snapshot() {
    let root = PathBuf::from("/project");
    let mut results = AnalysisResults::default();
    results
        .unused_enum_members
        .push(UnusedEnumMemberFinding::with_actions(UnusedMember {
            path: root.join("src/enums.ts"),
            parent_name: "Status".to_string(),
            member_name: "Deprecated".to_string(),
            kind: MemberKind::EnumMember,
            line: 8,
            col: 2,
        }));
    let value = build_json(&results, &root, Duration::ZERO).expect("JSON build should succeed");
    let json_str = serde_json::to_string_pretty(&value).expect("should serialize");
    insta::assert_snapshot!("json_unused_enum_members_only", redact_version(&json_str));
}

#[test]
fn json_unused_class_members_only_snapshot() {
    let root = PathBuf::from("/project");
    let mut results = AnalysisResults::default();
    results
        .unused_class_members
        .push(UnusedClassMemberFinding::with_actions(UnusedMember {
            path: root.join("src/service.ts"),
            parent_name: "UserService".to_string(),
            member_name: "legacyMethod".to_string(),
            kind: MemberKind::ClassMethod,
            line: 42,
            col: 4,
        }));
    let value = build_json(&results, &root, Duration::ZERO).expect("JSON build should succeed");
    let json_str = serde_json::to_string_pretty(&value).expect("should serialize");
    insta::assert_snapshot!("json_unused_class_members_only", redact_version(&json_str));
}

#[test]
fn json_duplicate_exports_only_snapshot() {
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
    let value = build_json(&results, &root, Duration::ZERO).expect("JSON build should succeed");
    let json_str = serde_json::to_string_pretty(&value).expect("should serialize");
    insta::assert_snapshot!("json_duplicate_exports_only", redact_version(&json_str));
}

#[test]
fn json_stale_suppression_unknown_kind_snapshot() {
    let root = PathBuf::from("/project");
    let mut results = AnalysisResults::default();
    results.stale_suppressions.push(StaleSuppression {
        path: root.join("src/utils.ts"),
        line: 1,
        col: 0,
        origin: SuppressionOrigin::Comment {
            issue_kind: Some("complexity-typo".to_string()),
            is_file_level: false,
            kind_known: false,
        },
    });
    results.stale_suppressions.push(StaleSuppression {
        path: root.join("src/utils.ts"),
        line: 10,
        col: 0,
        origin: SuppressionOrigin::Comment {
            issue_kind: Some("unused-export".to_string()),
            is_file_level: false,
            kind_known: true,
        },
    });
    let value = build_json(&results, &root, Duration::ZERO).expect("JSON build should succeed");
    let json_str = serde_json::to_string_pretty(&value).expect("should serialize");
    insta::assert_snapshot!(
        "json_stale_suppression_unknown_kind",
        redact_version(&json_str)
    );
}

fn redact_sarif_version(json_str: &str) -> String {
    json_str.replace(
        &format!(
            "\"name\": \"fallow\",\n          \"version\": \"{}\"",
            env!("CARGO_PKG_VERSION")
        ),
        "\"name\": \"fallow\",\n          \"version\": \"[VERSION]\"",
    )
}

#[test]
fn sarif_unused_files_only_snapshot() {
    let root = PathBuf::from("/project");
    let mut results = AnalysisResults::default();
    results
        .unused_files
        .push(UnusedFileFinding::with_actions(UnusedFile {
            path: root.join("src/dead.ts"),
        }));
    let sarif = build_sarif(&results, &root, &RulesConfig::default());
    let json_str = serde_json::to_string_pretty(&sarif).expect("should serialize");
    insta::assert_snapshot!("sarif_unused_files_only", redact_sarif_version(&json_str));
}

#[test]
fn sarif_unused_exports_only_snapshot() {
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
        }));
    let sarif = build_sarif(&results, &root, &RulesConfig::default());
    let json_str = serde_json::to_string_pretty(&sarif).expect("should serialize");
    insta::assert_snapshot!("sarif_unused_exports_only", redact_sarif_version(&json_str));
}

#[test]
fn sarif_unused_types_only_snapshot() {
    let root = PathBuf::from("/project");
    let mut results = AnalysisResults::default();
    results
        .unused_types
        .push(UnusedTypeFinding::with_actions(UnusedExport {
            path: root.join("src/types.ts"),
            export_name: "OldType".to_string(),
            is_type_only: true,
            line: 5,
            col: 0,
            span_start: 60,
            is_re_export: false,
        }));
    let sarif = build_sarif(&results, &root, &RulesConfig::default());
    let json_str = serde_json::to_string_pretty(&sarif).expect("should serialize");
    insta::assert_snapshot!("sarif_unused_types_only", redact_sarif_version(&json_str));
}

#[test]
fn sarif_unused_deps_only_snapshot() {
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
        }));
    let sarif = build_sarif(&results, &root, &RulesConfig::default());
    let json_str = serde_json::to_string_pretty(&sarif).expect("should serialize");
    insta::assert_snapshot!("sarif_unused_deps_only", redact_sarif_version(&json_str));
}

#[test]
fn sarif_unresolved_imports_only_snapshot() {
    let root = PathBuf::from("/project");
    let mut results = AnalysisResults::default();
    results
        .unresolved_imports
        .push(UnresolvedImportFinding::with_actions(UnresolvedImport {
            path: root.join("src/app.ts"),
            specifier: "./missing-module".to_string(),
            line: 3,
            col: 0,
            specifier_col: 0,
        }));
    let sarif = build_sarif(&results, &root, &RulesConfig::default());
    let json_str = serde_json::to_string_pretty(&sarif).expect("should serialize");
    insta::assert_snapshot!(
        "sarif_unresolved_imports_only",
        redact_sarif_version(&json_str)
    );
}

#[test]
fn sarif_unlisted_deps_only_snapshot() {
    let root = PathBuf::from("/project");
    let mut results = AnalysisResults::default();
    results
        .unlisted_dependencies
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
    let sarif = build_sarif(&results, &root, &RulesConfig::default());
    let json_str = serde_json::to_string_pretty(&sarif).expect("should serialize");
    insta::assert_snapshot!("sarif_unlisted_deps_only", redact_sarif_version(&json_str));
}

#[test]
fn sarif_unused_enum_members_only_snapshot() {
    let root = PathBuf::from("/project");
    let mut results = AnalysisResults::default();
    results
        .unused_enum_members
        .push(UnusedEnumMemberFinding::with_actions(UnusedMember {
            path: root.join("src/enums.ts"),
            parent_name: "Status".to_string(),
            member_name: "Deprecated".to_string(),
            kind: MemberKind::EnumMember,
            line: 8,
            col: 2,
        }));
    let sarif = build_sarif(&results, &root, &RulesConfig::default());
    let json_str = serde_json::to_string_pretty(&sarif).expect("should serialize");
    insta::assert_snapshot!(
        "sarif_unused_enum_members_only",
        redact_sarif_version(&json_str)
    );
}

#[test]
fn sarif_unused_class_members_only_snapshot() {
    let root = PathBuf::from("/project");
    let mut results = AnalysisResults::default();
    results
        .unused_class_members
        .push(UnusedClassMemberFinding::with_actions(UnusedMember {
            path: root.join("src/service.ts"),
            parent_name: "UserService".to_string(),
            member_name: "legacyMethod".to_string(),
            kind: MemberKind::ClassMethod,
            line: 42,
            col: 4,
        }));
    let sarif = build_sarif(&results, &root, &RulesConfig::default());
    let json_str = serde_json::to_string_pretty(&sarif).expect("should serialize");
    insta::assert_snapshot!(
        "sarif_unused_class_members_only",
        redact_sarif_version(&json_str)
    );
}

#[test]
fn sarif_duplicate_exports_only_snapshot() {
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
    let sarif = build_sarif(&results, &root, &RulesConfig::default());
    let json_str = serde_json::to_string_pretty(&sarif).expect("should serialize");
    insta::assert_snapshot!(
        "sarif_duplicate_exports_only",
        redact_sarif_version(&json_str)
    );
}

#[test]
fn json_multiple_exports_same_file_snapshot() {
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
        }));
    results
        .unused_exports
        .push(UnusedExportFinding::with_actions(UnusedExport {
            path: root.join("src/utils.ts"),
            export_name: "formatDate".to_string(),
            is_type_only: false,
            line: 25,
            col: 0,
            span_start: 300,
            is_re_export: false,
        }));
    results
        .unused_exports
        .push(UnusedExportFinding::with_actions(UnusedExport {
            path: root.join("src/helpers.ts"),
            export_name: "capitalize".to_string(),
            is_type_only: false,
            line: 1,
            col: 0,
            span_start: 0,
            is_re_export: false,
        }));
    let value = build_json(&results, &root, Duration::ZERO).expect("JSON build should succeed");
    let json_str = serde_json::to_string_pretty(&value).expect("should serialize");
    insta::assert_snapshot!("json_multiple_exports_same_file", redact_version(&json_str));
}

#[test]
fn sarif_multiple_exports_same_file_snapshot() {
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
        }));
    results
        .unused_exports
        .push(UnusedExportFinding::with_actions(UnusedExport {
            path: root.join("src/utils.ts"),
            export_name: "formatDate".to_string(),
            is_type_only: false,
            line: 25,
            col: 0,
            span_start: 300,
            is_re_export: false,
        }));
    let rules = RulesConfig::default();
    let sarif = build_sarif(&results, &root, &rules);
    let json_str = serde_json::to_string_pretty(&sarif).expect("should serialize");
    insta::assert_snapshot!(
        "sarif_multiple_exports_same_file",
        redact_sarif_version(&json_str)
    );
}

#[test]
fn compact_multiple_exports_same_file_snapshot() {
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
        }));
    results
        .unused_exports
        .push(UnusedExportFinding::with_actions(UnusedExport {
            path: root.join("src/utils.ts"),
            export_name: "formatDate".to_string(),
            is_type_only: false,
            line: 25,
            col: 0,
            span_start: 300,
            is_re_export: false,
        }));
    let lines = build_compact_lines(&results, &root);
    insta::assert_snapshot!("compact_multiple_exports_same_file", lines.join("\n"));
}

#[test]
fn json_workspace_dep_snapshot() {
    let root = PathBuf::from("/project");
    let mut results = AnalysisResults::default();
    results
        .unused_dependencies
        .push(UnusedDependencyFinding::with_actions(UnusedDependency {
            package_name: "lodash".to_string(),
            location: DependencyLocation::Dependencies,
            path: root.join("packages/ui/package.json"),
            line: 5,
            used_in_workspaces: Vec::new(),
        }));
    results
        .unused_dev_dependencies
        .push(UnusedDevDependencyFinding::with_actions(UnusedDependency {
            package_name: "jest".to_string(),
            location: DependencyLocation::DevDependencies,
            path: root.join("packages/ui/package.json"),
            line: 5,
            used_in_workspaces: Vec::new(),
        }));
    let value = build_json(&results, &root, Duration::ZERO).expect("JSON build should succeed");
    let json_str = serde_json::to_string_pretty(&value).expect("should serialize");
    insta::assert_snapshot!("json_workspace_deps", redact_version(&json_str));
}

#[test]
fn sarif_workspace_dep_snapshot() {
    let root = PathBuf::from("/project");
    let mut results = AnalysisResults::default();
    results
        .unused_dependencies
        .push(UnusedDependencyFinding::with_actions(UnusedDependency {
            package_name: "lodash".to_string(),
            location: DependencyLocation::Dependencies,
            path: root.join("packages/ui/package.json"),
            line: 5,
            used_in_workspaces: Vec::new(),
        }));
    let rules = RulesConfig::default();
    let sarif = build_sarif(&results, &root, &rules);
    let json_str = serde_json::to_string_pretty(&sarif).expect("should serialize");
    insta::assert_snapshot!("sarif_workspace_deps", redact_sarif_version(&json_str));
}

#[test]
fn codeclimate_output_snapshot() {
    let root = PathBuf::from("/project");
    let results = sample_results(&root);
    let rules = RulesConfig::default();
    let cc = codeclimate_issues_to_value(&build_codeclimate(&results, &root, &rules));
    let json_str = serde_json::to_string_pretty(&cc).expect("should serialize");
    insta::assert_snapshot!("codeclimate_output", json_str);
}

#[test]
fn codeclimate_empty_results_snapshot() {
    let root = PathBuf::from("/project");
    let results = AnalysisResults::default();
    let rules = RulesConfig::default();
    let cc = codeclimate_issues_to_value(&build_codeclimate(&results, &root, &rules));
    let json_str = serde_json::to_string_pretty(&cc).expect("should serialize");
    insta::assert_snapshot!("codeclimate_empty", json_str);
}

#[test]
fn codeclimate_unused_files_only_snapshot() {
    let root = PathBuf::from("/project");
    let mut results = AnalysisResults::default();
    results
        .unused_files
        .push(UnusedFileFinding::with_actions(UnusedFile {
            path: root.join("src/dead.ts"),
        }));
    let cc =
        codeclimate_issues_to_value(&build_codeclimate(&results, &root, &RulesConfig::default()));
    let json_str = serde_json::to_string_pretty(&cc).expect("should serialize");
    insta::assert_snapshot!("codeclimate_unused_files_only", json_str);
}

#[test]
fn codeclimate_unused_exports_only_snapshot() {
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
        }));
    let cc =
        codeclimate_issues_to_value(&build_codeclimate(&results, &root, &RulesConfig::default()));
    let json_str = serde_json::to_string_pretty(&cc).expect("should serialize");
    insta::assert_snapshot!("codeclimate_unused_exports_only", json_str);
}

#[test]
fn codeclimate_unused_types_only_snapshot() {
    let root = PathBuf::from("/project");
    let mut results = AnalysisResults::default();
    results
        .unused_types
        .push(UnusedTypeFinding::with_actions(UnusedExport {
            path: root.join("src/types.ts"),
            export_name: "OldType".to_string(),
            is_type_only: true,
            line: 5,
            col: 0,
            span_start: 60,
            is_re_export: false,
        }));
    let cc =
        codeclimate_issues_to_value(&build_codeclimate(&results, &root, &RulesConfig::default()));
    let json_str = serde_json::to_string_pretty(&cc).expect("should serialize");
    insta::assert_snapshot!("codeclimate_unused_types_only", json_str);
}

#[test]
fn codeclimate_unused_deps_only_snapshot() {
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
        }));
    let cc =
        codeclimate_issues_to_value(&build_codeclimate(&results, &root, &RulesConfig::default()));
    let json_str = serde_json::to_string_pretty(&cc).expect("should serialize");
    insta::assert_snapshot!("codeclimate_unused_deps_only", json_str);
}

#[test]
fn codeclimate_unresolved_imports_only_snapshot() {
    let root = PathBuf::from("/project");
    let mut results = AnalysisResults::default();
    results
        .unresolved_imports
        .push(UnresolvedImportFinding::with_actions(UnresolvedImport {
            path: root.join("src/index.ts"),
            specifier: "./missing".to_string(),
            line: 3,
            col: 0,
            specifier_col: 0,
        }));
    let cc =
        codeclimate_issues_to_value(&build_codeclimate(&results, &root, &RulesConfig::default()));
    let json_str = serde_json::to_string_pretty(&cc).expect("should serialize");
    insta::assert_snapshot!("codeclimate_unresolved_imports_only", json_str);
}

#[test]
fn codeclimate_unlisted_deps_only_snapshot() {
    let root = PathBuf::from("/project");
    let mut results = AnalysisResults::default();
    results
        .unlisted_dependencies
        .push(UnlistedDependencyFinding::with_actions(
            UnlistedDependency {
                package_name: "chalk".to_string(),
                imported_from: vec![ImportSite {
                    path: root.join("src/logger.ts"),
                    line: 1,
                    col: 0,
                }],
            },
        ));
    let cc =
        codeclimate_issues_to_value(&build_codeclimate(&results, &root, &RulesConfig::default()));
    let json_str = serde_json::to_string_pretty(&cc).expect("should serialize");
    insta::assert_snapshot!("codeclimate_unlisted_deps_only", json_str);
}

#[test]
fn codeclimate_unused_enum_members_only_snapshot() {
    let root = PathBuf::from("/project");
    let mut results = AnalysisResults::default();
    results
        .unused_enum_members
        .push(UnusedEnumMemberFinding::with_actions(UnusedMember {
            path: root.join("src/enums.ts"),
            parent_name: "Status".to_string(),
            member_name: "Deprecated".to_string(),
            kind: MemberKind::EnumMember,
            line: 8,
            col: 2,
        }));
    let cc =
        codeclimate_issues_to_value(&build_codeclimate(&results, &root, &RulesConfig::default()));
    let json_str = serde_json::to_string_pretty(&cc).expect("should serialize");
    insta::assert_snapshot!("codeclimate_unused_enum_members_only", json_str);
}

#[test]
fn codeclimate_unused_class_members_only_snapshot() {
    let root = PathBuf::from("/project");
    let mut results = AnalysisResults::default();
    results
        .unused_class_members
        .push(UnusedClassMemberFinding::with_actions(UnusedMember {
            path: root.join("src/service.ts"),
            parent_name: "UserService".to_string(),
            member_name: "legacyMethod".to_string(),
            kind: MemberKind::ClassMethod,
            line: 42,
            col: 4,
        }));
    let cc =
        codeclimate_issues_to_value(&build_codeclimate(&results, &root, &RulesConfig::default()));
    let json_str = serde_json::to_string_pretty(&cc).expect("should serialize");
    insta::assert_snapshot!("codeclimate_unused_class_members_only", json_str);
}

#[test]
fn codeclimate_duplicate_exports_only_snapshot() {
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
    let cc =
        codeclimate_issues_to_value(&build_codeclimate(&results, &root, &RulesConfig::default()));
    let json_str = serde_json::to_string_pretty(&cc).expect("should serialize");
    insta::assert_snapshot!("codeclimate_duplicate_exports_only", json_str);
}

#[test]
fn codeclimate_re_export_variant_snapshot() {
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
        }));
    let cc =
        codeclimate_issues_to_value(&build_codeclimate(&results, &root, &RulesConfig::default()));
    let json_str = serde_json::to_string_pretty(&cc).expect("should serialize");
    insta::assert_snapshot!("codeclimate_re_export_variant", json_str);
}

#[test]
fn codeclimate_mixed_severity_snapshot() {
    let root = PathBuf::from("/project");
    let results = sample_results(&root);
    let rules = RulesConfig {
        unused_files: fallow_config::Severity::Error,
        unused_exports: fallow_config::Severity::Warn,
        unused_types: fallow_config::Severity::Warn,
        private_type_leaks: fallow_config::Severity::Warn,
        unused_dependencies: fallow_config::Severity::Error,
        unused_dev_dependencies: fallow_config::Severity::Warn,
        unused_optional_dependencies: fallow_config::Severity::Warn,
        unused_enum_members: fallow_config::Severity::Warn,
        unused_class_members: fallow_config::Severity::Warn,
        unused_store_members: fallow_config::Severity::Warn,
        unprovided_injects: fallow_config::Severity::Warn,
        unrendered_components: fallow_config::Severity::Warn,
        unused_component_props: fallow_config::Severity::Warn,
        unused_component_emits: fallow_config::Severity::Warn,
        unused_server_actions: fallow_config::Severity::Warn,
        unused_load_data_keys: fallow_config::Severity::Warn,
        unresolved_imports: fallow_config::Severity::Error,
        unlisted_dependencies: fallow_config::Severity::Error,
        duplicate_exports: fallow_config::Severity::Warn,
        type_only_dependencies: fallow_config::Severity::Warn,
        circular_dependencies: fallow_config::Severity::Warn,
        re_export_cycle: fallow_config::Severity::Warn,
        test_only_dependencies: fallow_config::Severity::Warn,
        boundary_violation: fallow_config::Severity::Warn,
        coverage_gaps: fallow_config::Severity::Warn,
        feature_flags: fallow_config::Severity::Off,
        stale_suppressions: fallow_config::Severity::Warn,
        unused_catalog_entries: fallow_config::Severity::Warn,
        empty_catalog_groups: fallow_config::Severity::Warn,
        unresolved_catalog_references: fallow_config::Severity::Error,
        unused_dependency_overrides: fallow_config::Severity::Warn,
        misconfigured_dependency_overrides: fallow_config::Severity::Error,
        security_client_server_leak: fallow_config::Severity::Off,
        security_sink: fallow_config::Severity::Off,
        policy_violation: fallow_config::Severity::Warn,
        invalid_client_export: fallow_config::Severity::Warn,
        mixed_client_server_barrel: fallow_config::Severity::Warn,
        misplaced_directive: fallow_config::Severity::Warn,
        route_collision: fallow_config::Severity::Warn,
        dynamic_segment_name_conflict: fallow_config::Severity::Warn,
    };
    let cc = codeclimate_issues_to_value(&build_codeclimate(&results, &root, &rules));
    let json_str = serde_json::to_string_pretty(&cc).expect("should serialize");
    insta::assert_snapshot!("codeclimate_mixed_severity", json_str);
}

#[test]
fn codeclimate_type_only_deps_snapshot() {
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
    let cc =
        codeclimate_issues_to_value(&build_codeclimate(&results, &root, &RulesConfig::default()));
    let json_str = serde_json::to_string_pretty(&cc).expect("should serialize");
    insta::assert_snapshot!("codeclimate_type_only_deps", json_str);
}

#[test]
fn codeclimate_unused_dev_deps_only_snapshot() {
    let root = PathBuf::from("/project");
    let mut results = AnalysisResults::default();
    results
        .unused_dev_dependencies
        .push(UnusedDevDependencyFinding::with_actions(UnusedDependency {
            package_name: "jest".to_string(),
            location: DependencyLocation::DevDependencies,
            path: root.join("package.json"),
            line: 12,
            used_in_workspaces: Vec::new(),
        }));
    let cc =
        codeclimate_issues_to_value(&build_codeclimate(&results, &root, &RulesConfig::default()));
    let json_str = serde_json::to_string_pretty(&cc).expect("should serialize");
    insta::assert_snapshot!("codeclimate_unused_dev_deps_only", json_str);
}

#[test]
fn codeclimate_unused_optional_deps_only_snapshot() {
    let root = PathBuf::from("/project");
    let mut results = AnalysisResults::default();
    results
        .unused_optional_dependencies
        .push(UnusedOptionalDependencyFinding::with_actions(
            UnusedDependency {
                package_name: "fsevents".to_string(),
                location: DependencyLocation::OptionalDependencies,
                path: root.join("package.json"),
                line: 5,
                used_in_workspaces: Vec::new(),
            },
        ));
    let cc =
        codeclimate_issues_to_value(&build_codeclimate(&results, &root, &RulesConfig::default()));
    let json_str = serde_json::to_string_pretty(&cc).expect("should serialize");
    insta::assert_snapshot!("codeclimate_unused_optional_deps_only", json_str);
}

#[test]
fn codeclimate_circular_deps_only_snapshot() {
    let root = PathBuf::from("/project");
    let mut results = AnalysisResults::default();
    results
        .circular_dependencies
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
    let cc =
        codeclimate_issues_to_value(&build_codeclimate(&results, &root, &RulesConfig::default()));
    let json_str = serde_json::to_string_pretty(&cc).expect("should serialize");
    insta::assert_snapshot!("codeclimate_circular_deps_only", json_str);
}

#[test]
fn codeclimate_multiple_exports_same_file_snapshot() {
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
        }));
    results
        .unused_exports
        .push(UnusedExportFinding::with_actions(UnusedExport {
            path: root.join("src/utils.ts"),
            export_name: "formatDate".to_string(),
            is_type_only: false,
            line: 25,
            col: 0,
            span_start: 300,
            is_re_export: false,
        }));
    results
        .unused_exports
        .push(UnusedExportFinding::with_actions(UnusedExport {
            path: root.join("src/helpers.ts"),
            export_name: "capitalize".to_string(),
            is_type_only: false,
            line: 1,
            col: 0,
            span_start: 0,
            is_re_export: false,
        }));
    let cc =
        codeclimate_issues_to_value(&build_codeclimate(&results, &root, &RulesConfig::default()));
    let json_str = serde_json::to_string_pretty(&cc).expect("should serialize");
    insta::assert_snapshot!("codeclimate_multiple_exports_same_file", json_str);
}

#[test]
fn codeclimate_workspace_dep_snapshot() {
    let root = PathBuf::from("/project");
    let mut results = AnalysisResults::default();
    results
        .unused_dependencies
        .push(UnusedDependencyFinding::with_actions(UnusedDependency {
            package_name: "lodash".to_string(),
            location: DependencyLocation::Dependencies,
            path: root.join("packages/ui/package.json"),
            line: 5,
            used_in_workspaces: Vec::new(),
        }));
    let cc =
        codeclimate_issues_to_value(&build_codeclimate(&results, &root, &RulesConfig::default()));
    let json_str = serde_json::to_string_pretty(&cc).expect("should serialize");
    insta::assert_snapshot!("codeclimate_workspace_deps", json_str);
}

#[test]
fn pr_comment_github_snapshot() {
    let root = PathBuf::from("/project");
    let results = sample_results(&root);
    let codeclimate =
        codeclimate_issues_to_value(&build_codeclimate(&results, &root, &RulesConfig::default()));
    let issues = issues_from_codeclimate(&codeclimate);
    let output = render_pr_comment("check", Provider::Github, &issues);

    insta::assert_snapshot!("pr_comment_github", output);
}

#[test]
fn pr_comment_gitlab_snapshot() {
    let root = PathBuf::from("/project");
    let results = sample_results(&root);
    let codeclimate =
        codeclimate_issues_to_value(&build_codeclimate(&results, &root, &RulesConfig::default()));
    let issues = issues_from_codeclimate(&codeclimate);
    let output = render_pr_comment("check", Provider::Gitlab, &issues);

    insta::assert_snapshot!("pr_comment_gitlab", output);
}

#[test]
fn review_github_envelope_snapshot() {
    let root = PathBuf::from("/project");
    let results = sample_results(&root);
    let codeclimate =
        codeclimate_issues_to_value(&build_codeclimate(&results, &root, &RulesConfig::default()));
    let issues = issues_from_codeclimate(&codeclimate);
    let envelope = render_review_envelope("check", Provider::Github, &issues);
    let json_str = serde_json::to_string_pretty(&envelope).expect("should serialize");

    insta::assert_snapshot!("review_github_envelope", json_str);
}

#[test]
fn review_gitlab_envelope_snapshot() {
    let root = PathBuf::from("/project");
    let results = sample_results(&root);
    let codeclimate =
        codeclimate_issues_to_value(&build_codeclimate(&results, &root, &RulesConfig::default()));
    let issues = issues_from_codeclimate(&codeclimate);
    let envelope = render_review_envelope("check", Provider::Gitlab, &issues);
    let json_str = serde_json::to_string_pretty(&envelope).expect("should serialize");

    insta::assert_snapshot!("review_gitlab_envelope", json_str);
}

#[test]
fn json_circular_deps_only_snapshot() {
    let root = PathBuf::from("/project");
    let mut results = AnalysisResults::default();
    results
        .circular_dependencies
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
    let value = build_json(&results, &root, Duration::ZERO).expect("JSON build should succeed");
    let json_str = serde_json::to_string_pretty(&value).expect("should serialize");
    insta::assert_snapshot!("json_circular_deps_only", redact_version(&json_str));
}

#[test]
fn sarif_circular_deps_only_snapshot() {
    let root = PathBuf::from("/project");
    let mut results = AnalysisResults::default();
    results
        .circular_dependencies
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
    let sarif = build_sarif(&results, &root, &RulesConfig::default());
    let json_str = serde_json::to_string_pretty(&sarif).expect("should serialize");
    insta::assert_snapshot!("sarif_circular_deps_only", redact_sarif_version(&json_str));
}

#[test]
fn compact_circular_deps_only_snapshot() {
    let root = PathBuf::from("/project");
    let mut results = AnalysisResults::default();
    results
        .circular_dependencies
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
    let lines = build_compact_lines(&results, &root);
    insta::assert_snapshot!("compact_circular_deps_only", lines.join("\n"));
}

fn re_export_cycles_results(root: &Path) -> AnalysisResults {
    let mut results = AnalysisResults::default();
    results
        .re_export_cycles
        .push(ReExportCycleFinding::with_actions(ReExportCycle {
            files: vec![
                root.join("src/api/index.ts"),
                root.join("src/api/internal/index.ts"),
            ],
            kind: ReExportCycleKind::MultiNode,
        }));
    results
        .re_export_cycles
        .push(ReExportCycleFinding::with_actions(ReExportCycle {
            files: vec![root.join("src/utils/index.ts")],
            kind: ReExportCycleKind::SelfLoop,
        }));
    results
}

#[test]
fn json_re_export_cycles_only_snapshot() {
    let root = PathBuf::from("/project");
    let results = re_export_cycles_results(&root);
    let value = build_json(&results, &root, Duration::ZERO).expect("JSON build should succeed");
    let json_str = serde_json::to_string_pretty(&value).expect("should serialize");
    insta::assert_snapshot!("json_re_export_cycles_only", redact_version(&json_str));
}

#[test]
fn sarif_re_export_cycles_only_snapshot() {
    let root = PathBuf::from("/project");
    let results = re_export_cycles_results(&root);
    let sarif = build_sarif(&results, &root, &RulesConfig::default());
    let json_str = serde_json::to_string_pretty(&sarif).expect("should serialize");
    insta::assert_snapshot!(
        "sarif_re_export_cycles_only",
        redact_sarif_version(&json_str)
    );
}

#[test]
fn compact_re_export_cycles_only_snapshot() {
    let root = PathBuf::from("/project");
    let results = re_export_cycles_results(&root);
    let lines = build_compact_lines(&results, &root);
    insta::assert_snapshot!("compact_re_export_cycles_only", lines.join("\n"));
}

#[test]
fn markdown_re_export_cycles_only_snapshot() {
    let root = PathBuf::from("/project");
    let results = re_export_cycles_results(&root);
    let md = build_markdown(&results, &root);
    insta::assert_snapshot!("markdown_re_export_cycles_only", md);
}

#[test]
fn codeclimate_re_export_cycles_only_snapshot() {
    let root = PathBuf::from("/project");
    let results = re_export_cycles_results(&root);
    let cc =
        codeclimate_issues_to_value(&build_codeclimate(&results, &root, &RulesConfig::default()));
    let json_str = serde_json::to_string_pretty(&cc).expect("should serialize");
    insta::assert_snapshot!("codeclimate_re_export_cycles_only", json_str);
}

#[test]
fn sarif_type_only_deps_snapshot() {
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
    let sarif = build_sarif(&results, &root, &RulesConfig::default());
    let json_str = serde_json::to_string_pretty(&sarif).expect("should serialize");
    insta::assert_snapshot!("sarif_type_only_deps", redact_sarif_version(&json_str));
}

#[test]
fn compact_type_only_deps_snapshot() {
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
    let lines = build_compact_lines(&results, &root);
    insta::assert_snapshot!("compact_type_only_deps", lines.join("\n"));
}

#[test]
fn json_unused_dev_deps_only_snapshot() {
    let root = PathBuf::from("/project");
    let mut results = AnalysisResults::default();
    results
        .unused_dev_dependencies
        .push(UnusedDevDependencyFinding::with_actions(UnusedDependency {
            package_name: "jest".to_string(),
            location: DependencyLocation::DevDependencies,
            path: root.join("package.json"),
            line: 12,
            used_in_workspaces: Vec::new(),
        }));
    let value = build_json(&results, &root, Duration::ZERO).expect("JSON build should succeed");
    let json_str = serde_json::to_string_pretty(&value).expect("should serialize");
    insta::assert_snapshot!("json_unused_dev_deps_only", redact_version(&json_str));
}

#[test]
fn sarif_unused_dev_deps_only_snapshot() {
    let root = PathBuf::from("/project");
    let mut results = AnalysisResults::default();
    results
        .unused_dev_dependencies
        .push(UnusedDevDependencyFinding::with_actions(UnusedDependency {
            package_name: "jest".to_string(),
            location: DependencyLocation::DevDependencies,
            path: root.join("package.json"),
            line: 12,
            used_in_workspaces: Vec::new(),
        }));
    let sarif = build_sarif(&results, &root, &RulesConfig::default());
    let json_str = serde_json::to_string_pretty(&sarif).expect("should serialize");
    insta::assert_snapshot!(
        "sarif_unused_dev_deps_only",
        redact_sarif_version(&json_str)
    );
}

#[test]
fn json_unused_optional_deps_only_snapshot() {
    let root = PathBuf::from("/project");
    let mut results = AnalysisResults::default();
    results
        .unused_optional_dependencies
        .push(UnusedOptionalDependencyFinding::with_actions(
            UnusedDependency {
                package_name: "fsevents".to_string(),
                location: DependencyLocation::OptionalDependencies,
                path: root.join("package.json"),
                line: 5,
                used_in_workspaces: Vec::new(),
            },
        ));
    let value = build_json(&results, &root, Duration::ZERO).expect("JSON build should succeed");
    let json_str = serde_json::to_string_pretty(&value).expect("should serialize");
    insta::assert_snapshot!("json_unused_optional_deps_only", redact_version(&json_str));
}

#[test]
fn sarif_unused_optional_deps_only_snapshot() {
    let root = PathBuf::from("/project");
    let mut results = AnalysisResults::default();
    results
        .unused_optional_dependencies
        .push(UnusedOptionalDependencyFinding::with_actions(
            UnusedDependency {
                package_name: "fsevents".to_string(),
                location: DependencyLocation::OptionalDependencies,
                path: root.join("package.json"),
                line: 5,
                used_in_workspaces: Vec::new(),
            },
        ));
    let sarif = build_sarif(&results, &root, &RulesConfig::default());
    let json_str = serde_json::to_string_pretty(&sarif).expect("should serialize");
    insta::assert_snapshot!(
        "sarif_unused_optional_deps_only",
        redact_sarif_version(&json_str)
    );
}

#[test]
fn compact_workspace_dep_snapshot() {
    let root = PathBuf::from("/project");
    let mut results = AnalysisResults::default();
    results
        .unused_dependencies
        .push(UnusedDependencyFinding::with_actions(UnusedDependency {
            package_name: "lodash".to_string(),
            location: DependencyLocation::Dependencies,
            path: root.join("packages/ui/package.json"),
            line: 5,
            used_in_workspaces: Vec::new(),
        }));
    let lines = build_compact_lines(&results, &root);
    insta::assert_snapshot!("compact_workspace_deps", lines.join("\n"));
}

#[test]
fn json_mixed_severity_snapshot() {
    let root = PathBuf::from("/project");
    let results = sample_results(&root);
    let elapsed = Duration::from_millis(42);
    let value = build_json(&results, &root, elapsed).expect("JSON build should succeed");
    let json_str = serde_json::to_string_pretty(&value).expect("should serialize");
    insta::assert_snapshot!("json_mixed_severity", redact_version(&json_str));
}

#[test]
fn markdown_output_snapshot() {
    let root = PathBuf::from("/project");
    let results = sample_results(&root);
    let output = build_markdown(&results, &root);
    insta::assert_snapshot!("markdown_output", output);
}

#[test]
fn markdown_empty_results_snapshot() {
    let root = PathBuf::from("/project");
    let results = AnalysisResults::default();
    let output = build_markdown(&results, &root);
    insta::assert_snapshot!("markdown_empty", output);
}

#[test]
fn markdown_single_unused_file_snapshot() {
    let root = PathBuf::from("/project");
    let mut results = AnalysisResults::default();
    results
        .unused_files
        .push(UnusedFileFinding::with_actions(UnusedFile {
            path: root.join("src/dead.ts"),
        }));
    let output = build_markdown(&results, &root);
    insta::assert_snapshot!("markdown_single_unused_file", output);
}

#[test]
fn markdown_unused_exports_only_snapshot() {
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
        }));
    results
        .unused_exports
        .push(UnusedExportFinding::with_actions(UnusedExport {
            path: root.join("src/utils.ts"),
            export_name: "formatDate".to_string(),
            is_type_only: false,
            line: 25,
            col: 0,
            span_start: 300,
            is_re_export: false,
        }));
    let output = build_markdown(&results, &root);
    insta::assert_snapshot!("markdown_unused_exports_only", output);
}

#[test]
fn markdown_unused_types_only_snapshot() {
    let root = PathBuf::from("/project");
    let mut results = AnalysisResults::default();
    results
        .unused_types
        .push(UnusedTypeFinding::with_actions(UnusedExport {
            path: root.join("src/types.ts"),
            export_name: "OldType".to_string(),
            is_type_only: true,
            line: 5,
            col: 0,
            span_start: 60,
            is_re_export: false,
        }));
    let output = build_markdown(&results, &root);
    insta::assert_snapshot!("markdown_unused_types_only", output);
}

#[test]
fn markdown_unused_deps_only_snapshot() {
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
        }));
    let output = build_markdown(&results, &root);
    insta::assert_snapshot!("markdown_unused_deps_only", output);
}

#[test]
fn markdown_unresolved_imports_only_snapshot() {
    let root = PathBuf::from("/project");
    let mut results = AnalysisResults::default();
    results
        .unresolved_imports
        .push(UnresolvedImportFinding::with_actions(UnresolvedImport {
            path: root.join("src/app.ts"),
            specifier: "./missing-module".to_string(),
            line: 3,
            col: 0,
            specifier_col: 0,
        }));
    let output = build_markdown(&results, &root);
    insta::assert_snapshot!("markdown_unresolved_imports_only", output);
}

#[test]
fn markdown_unlisted_deps_only_snapshot() {
    let root = PathBuf::from("/project");
    let mut results = AnalysisResults::default();
    results
        .unlisted_dependencies
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
    let output = build_markdown(&results, &root);
    insta::assert_snapshot!("markdown_unlisted_deps_only", output);
}

#[test]
fn markdown_unused_enum_members_only_snapshot() {
    let root = PathBuf::from("/project");
    let mut results = AnalysisResults::default();
    results
        .unused_enum_members
        .push(UnusedEnumMemberFinding::with_actions(UnusedMember {
            path: root.join("src/enums.ts"),
            parent_name: "Status".to_string(),
            member_name: "Deprecated".to_string(),
            kind: MemberKind::EnumMember,
            line: 8,
            col: 2,
        }));
    let output = build_markdown(&results, &root);
    insta::assert_snapshot!("markdown_unused_enum_members_only", output);
}

#[test]
fn markdown_unused_class_members_only_snapshot() {
    let root = PathBuf::from("/project");
    let mut results = AnalysisResults::default();
    results
        .unused_class_members
        .push(UnusedClassMemberFinding::with_actions(UnusedMember {
            path: root.join("src/service.ts"),
            parent_name: "UserService".to_string(),
            member_name: "legacyMethod".to_string(),
            kind: MemberKind::ClassMethod,
            line: 42,
            col: 4,
        }));
    let output = build_markdown(&results, &root);
    insta::assert_snapshot!("markdown_unused_class_members_only", output);
}

#[test]
fn markdown_duplicate_exports_only_snapshot() {
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
    let output = build_markdown(&results, &root);
    insta::assert_snapshot!("markdown_duplicate_exports_only", output);
}

#[test]
fn markdown_circular_deps_only_snapshot() {
    let root = PathBuf::from("/project");
    let mut results = AnalysisResults::default();
    results
        .circular_dependencies
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
    let output = build_markdown(&results, &root);
    insta::assert_snapshot!("markdown_circular_deps_only", output);
}

#[test]
fn markdown_type_only_deps_only_snapshot() {
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
    let output = build_markdown(&results, &root);
    insta::assert_snapshot!("markdown_type_only_deps_only", output);
}

#[test]
fn markdown_re_export_variant_snapshot() {
    let root = PathBuf::from("/project");
    let mut results = AnalysisResults::default();
    results
        .unused_exports
        .push(UnusedExportFinding::with_actions(UnusedExport {
            path: root.join("src/index.ts"),
            export_name: "reExportedFn".to_string(),
            is_type_only: false,
            line: 1,
            col: 0,
            span_start: 0,
            is_re_export: true,
        }));
    let output = build_markdown(&results, &root);
    insta::assert_snapshot!("markdown_re_export_variant", output);
}

#[test]
fn markdown_workspace_dep_snapshot() {
    let root = PathBuf::from("/project");
    let mut results = AnalysisResults::default();
    results
        .unused_dependencies
        .push(UnusedDependencyFinding::with_actions(UnusedDependency {
            package_name: "lodash".to_string(),
            location: DependencyLocation::Dependencies,
            path: root.join("packages/ui/package.json"),
            line: 5,
            used_in_workspaces: Vec::new(),
        }));
    let output = build_markdown(&results, &root);
    insta::assert_snapshot!("markdown_workspace_deps", output);
}

/// Build a minimal health report with one finding for snapshot tests.
fn sample_health_report(root: &Path) -> HealthReport {
    let action_ctx = fallow_cli::health_types::HealthActionContext {
        opts: fallow_cli::health_types::HealthActionOptions::default(),
        max_cyclomatic_threshold: 20,
        max_cognitive_threshold: 15,
        max_crap_threshold: 30.0,
        crap_refactor_band: 5,
    };
    HealthReport {
        findings: vec![fallow_cli::health_types::HealthFinding::with_actions(
            ComplexityViolation {
                path: root.join("src/complex.ts"),
                name: "processData".to_string(),
                line: 42,
                col: 0,
                cyclomatic: 25,
                cognitive: 30,
                line_count: 120,
                param_count: 0,
                exceeded: ExceededThreshold::Both,
                severity: FindingSeverity::High,
                crap: None,
                coverage_pct: None,
                coverage_tier: None,
                coverage_source: None,
                inherited_from: None,
                component_rollup: None,
                contributions: Vec::new(),
                effective_thresholds: None,
                threshold_source: None,
            },
            &action_ctx,
        )],
        summary: HealthSummary {
            files_analyzed: 50,
            functions_analyzed: 200,
            functions_above_threshold: 1,
            max_cyclomatic_threshold: 20,
            max_cognitive_threshold: 15,
            max_crap_threshold: 30.0,
            files_scored: None,
            average_maintainability: None,
            coverage_model: None,
            coverage_source_consistency: None,
            istanbul_matched: None,
            istanbul_total: None,
            severity_critical_count: 0,
            severity_high_count: 1,
            severity_moderate_count: 0,
        },
        vital_signs: None,
        health_score: None,
        file_scores: vec![],
        coverage_gaps: None,
        threshold_overrides: vec![],
        hotspots: vec![],
        hotspot_summary: None,
        large_functions: vec![],
        targets: vec![],
        target_thresholds: None,
        health_trend: None,
        runtime_coverage: None,
        coverage_intelligence: None,
        actions_meta: None,
        css_analytics: None,
    }
}

fn health_report_with_runtime_coverage(root: &Path) -> HealthReport {
    let mut report = sample_health_report(root);
    report.runtime_coverage = Some(RuntimeCoverageReport {
        schema_version: RuntimeCoverageSchemaVersion::V1,
        verdict: RuntimeCoverageReportVerdict::ColdCodeDetected,
        signals: Vec::new(),
        summary: RuntimeCoverageSummary {
            data_source: RuntimeCoverageDataSource::Local,
            last_received_at: None,
            functions_tracked: 6,
            functions_hit: 3,
            functions_unhit: 2,
            functions_untracked: 1,
            coverage_percent: 50.0,
            trace_count: 2_847_291,
            period_days: 30,
            deployments_seen: 14,
            capture_quality: None,
        },
        findings: vec![
            RuntimeCoverageFinding {
                id: "fallow:prod:deadbeef".to_string(),
                stable_id: Some("fallow:fn:00000001".to_string()),
                path: root.join("src/cold.ts"),
                function: "coldPath".to_string(),
                line: 14,
                verdict: RuntimeCoverageVerdict::ReviewRequired,
                invocations: Some(0),
                confidence: RuntimeCoverageConfidence::Medium,
                evidence: RuntimeCoverageEvidence {
                    static_status: "used".to_string(),
                    test_coverage: "not_covered".to_string(),
                    v8_tracking: "tracked".to_string(),
                    untracked_reason: None,
                    observation_days: 30,
                    deployments_observed: 14,
                },
                actions: vec![RuntimeCoverageAction {
                    kind: "review-deletion".to_string(),
                    description: "Tracked in runtime coverage with zero invocations.".to_string(),
                    auto_fixable: false,
                }],
                source_hash: None,
            },
            RuntimeCoverageFinding {
                id: "fallow:prod:feedface".to_string(),
                stable_id: None,
                path: root.join("src/unknown.ts"),
                function: "lateBound".to_string(),
                line: 8,
                verdict: RuntimeCoverageVerdict::CoverageUnavailable,
                invocations: None,
                confidence: RuntimeCoverageConfidence::None,
                evidence: RuntimeCoverageEvidence {
                    static_status: "used".to_string(),
                    test_coverage: "not_covered".to_string(),
                    v8_tracking: "untracked".to_string(),
                    untracked_reason: Some("lazy_parsed".to_string()),
                    observation_days: 30,
                    deployments_observed: 14,
                },
                actions: vec![RuntimeCoverageAction {
                    kind: "collect-runtime-coverage".to_string(),
                    description: "Collect a broader production dump.".to_string(),
                    auto_fixable: false,
                }],
                source_hash: None,
            },
        ],
        hot_paths: vec![RuntimeCoverageHotPath {
            id: "fallow:hot:cafebabe".to_string(),
            stable_id: Some("fallow:fn:00000002".to_string()),
            path: root.join("src/hot.ts"),
            function: "hotPath".to_string(),
            line: 3,
            end_line: 9,
            invocations: 250,
            percentile: 99,
            actions: vec![],
        }],
        blast_radius: vec![],
        importance: vec![],
        watermark: Some(RuntimeCoverageWatermark::LicenseExpiredGrace),
        warnings: vec![RuntimeCoverageMessage {
            code: "partial-input".to_string(),
            message: "One dump was incomplete.".to_string(),
        }],
    });
    report
}

/// Health report carrying a populated `coverage_intelligence` block alongside
/// runtime coverage, so the public format renderers exercise the live combined
/// surface (not just the per-format helper unit tests). Mirrors the shape the
/// `build_coverage_intelligence` builder emits for a risky changed hot path plus
/// a high-confidence delete candidate, with one ambiguous match skipped.
fn health_report_with_coverage_intelligence(root: &Path) -> HealthReport {
    let mut report = health_report_with_runtime_coverage(root);
    report.coverage_intelligence = Some(CoverageIntelligenceReport {
        schema_version: CoverageIntelligenceSchemaVersion::default(),
        verdict: CoverageIntelligenceVerdict::RiskyChangeDetected,
        summary: CoverageIntelligenceSummary {
            findings: 2,
            risky_changes: 1,
            high_confidence_deletes: 1,
            review_required: 0,
            refactor_carefully: 0,
            skipped_ambiguous_matches: 1,
        },
        findings: vec![
            CoverageIntelligenceFinding {
                id: "fallow:coverage-intel:0badc0de".to_string(),
                path: root.join("src/hot.ts"),
                identity: Some("handler".to_string()),
                line: 10,
                verdict: CoverageIntelligenceVerdict::RiskyChangeDetected,
                signals: vec![
                    CoverageIntelligenceSignal::Changed,
                    CoverageIntelligenceSignal::HotPath,
                    CoverageIntelligenceSignal::LowTestCoverage,
                    CoverageIntelligenceSignal::HighCrap,
                ],
                recommendation: CoverageIntelligenceRecommendation::AddTestOrSplitBeforeMerge,
                confidence: CoverageIntelligenceConfidence::High,
                related_ids: vec!["fallow:hot:cafebabe".to_string()],
                evidence: CoverageIntelligenceEvidence {
                    coverage_pct: Some(20.0),
                    crap: Some(45.0),
                    runtime_verdict: Some("hot_path_touched".to_string()),
                    invocations: Some(250),
                    static_status: Some("used".to_string()),
                    test_coverage: Some("partially_covered".to_string()),
                    changed: true,
                    ownership_state: None,
                    match_confidence: CoverageIntelligenceMatchConfidence::PathFunctionLine,
                },
                actions: vec![CoverageIntelligenceAction {
                    kind: "add-tests".to_string(),
                    description:
                        "Add tests or split before merge: this changed hot path is undertested."
                            .to_string(),
                    auto_fixable: false,
                }],
            },
            CoverageIntelligenceFinding {
                id: "fallow:coverage-intel:1deadfa1".to_string(),
                path: root.join("src/dead.ts"),
                identity: Some("deadPath".to_string()),
                line: 4,
                verdict: CoverageIntelligenceVerdict::HighConfidenceDelete,
                signals: vec![
                    CoverageIntelligenceSignal::StaticUnused,
                    CoverageIntelligenceSignal::RuntimeCold,
                    CoverageIntelligenceSignal::NoTestPath,
                ],
                recommendation: CoverageIntelligenceRecommendation::DeleteAfterConfirmingOwner,
                confidence: CoverageIntelligenceConfidence::High,
                related_ids: vec!["fallow:prod:deadbeef".to_string()],
                evidence: CoverageIntelligenceEvidence {
                    coverage_pct: Some(0.0),
                    crap: None,
                    runtime_verdict: Some("safe_to_delete".to_string()),
                    invocations: Some(0),
                    static_status: Some("unused".to_string()),
                    test_coverage: Some("not_covered".to_string()),
                    changed: false,
                    ownership_state: None,
                    match_confidence: CoverageIntelligenceMatchConfidence::PathLine,
                },
                actions: vec![CoverageIntelligenceAction {
                    kind: "delete-after-confirm".to_string(),
                    description:
                        "Delete after confirming with the owner: unused, cold, and untested."
                            .to_string(),
                    auto_fixable: false,
                }],
            },
        ],
    });
    report
}

#[test]
fn json_health_with_coverage_intelligence_snapshot() {
    let root = PathBuf::from("/project");
    let report = health_report_with_coverage_intelligence(&root);
    let value = build_health_json(&report, &root, Duration::ZERO, false)
        .expect("health JSON build should succeed");
    let json_str = serde_json::to_string_pretty(&value).expect("should serialize");
    insta::assert_snapshot!(
        "json_health_with_coverage_intelligence",
        redact_version(&json_str)
    );
}

#[test]
fn markdown_health_with_coverage_intelligence_snapshot() {
    let root = PathBuf::from("/project");
    let report = health_report_with_coverage_intelligence(&root);
    let output = build_health_markdown(&report, &root);
    insta::assert_snapshot!("markdown_health_with_coverage_intelligence", output);
}

#[test]
fn sarif_health_with_coverage_intelligence_snapshot() {
    let root = PathBuf::from("/project");
    let report = health_report_with_coverage_intelligence(&root);
    let json_str = serde_json::to_string_pretty(&build_health_sarif(&report, &root)).unwrap();
    insta::assert_snapshot!(
        "sarif_health_with_coverage_intelligence",
        redact_sarif_version(&json_str)
    );
}

#[test]
fn codeclimate_health_with_coverage_intelligence_snapshot() {
    let root = PathBuf::from("/project");
    let report = health_report_with_coverage_intelligence(&root);
    let cc = codeclimate_issues_to_value(&build_health_codeclimate(&report, &root));
    let json_str = serde_json::to_string_pretty(&cc).expect("should serialize");
    insta::assert_snapshot!("codeclimate_health_with_coverage_intelligence", json_str);
}

/// Build an empty health report (no findings).
const fn empty_health_report() -> HealthReport {
    HealthReport {
        findings: vec![],
        summary: HealthSummary {
            files_analyzed: 50,
            functions_analyzed: 200,
            functions_above_threshold: 0,
            max_cyclomatic_threshold: 20,
            max_cognitive_threshold: 15,
            max_crap_threshold: 30.0,
            files_scored: None,
            average_maintainability: None,
            coverage_model: None,
            coverage_source_consistency: None,
            istanbul_matched: None,
            istanbul_total: None,
            severity_critical_count: 0,
            severity_high_count: 0,
            severity_moderate_count: 0,
        },
        vital_signs: None,
        health_score: None,
        file_scores: vec![],
        coverage_gaps: None,
        threshold_overrides: vec![],
        hotspots: vec![],
        hotspot_summary: None,
        large_functions: vec![],
        targets: vec![],
        target_thresholds: None,
        health_trend: None,
        runtime_coverage: None,
        coverage_intelligence: None,
        actions_meta: None,
        css_analytics: None,
    }
}

#[test]
fn markdown_health_output_snapshot() {
    let root = PathBuf::from("/project");
    let report = sample_health_report(&root);
    let output = build_health_markdown(&report, &root);
    insta::assert_snapshot!("markdown_health_output", output);
}

#[test]
fn markdown_health_empty_snapshot() {
    let root = PathBuf::from("/project");
    let report = empty_health_report();
    let output = build_health_markdown(&report, &root);
    insta::assert_snapshot!("markdown_health_empty", output);
}

#[test]
fn markdown_health_with_vital_signs_snapshot() {
    let root = PathBuf::from("/project");
    let mut report = sample_health_report(&root);
    report.vital_signs = Some(VitalSigns {
        dead_file_pct: Some(3.2),
        dead_export_pct: Some(8.1),
        avg_cyclomatic: 4.7,
        p90_cyclomatic: 12,
        duplication_pct: None,
        hotspot_count: None,
        maintainability_avg: Some(72.4),
        unused_dep_count: Some(3),
        circular_dep_count: Some(1),
        counts: None,
        unit_size_profile: None,
        unit_interfacing_profile: None,
        p95_fan_in: None,
        coupling_high_pct: None,
        total_loc: 42_000,
        ..Default::default()
    });
    let output = build_health_markdown(&report, &root);
    insta::assert_snapshot!("markdown_health_with_vital_signs", output);
}

fn redact_health_sarif_version(json_str: &str) -> String {
    json_str.replace(
        &format!(
            "\"name\": \"fallow\",\n          \"version\": \"{}\"",
            env!("CARGO_PKG_VERSION")
        ),
        "\"name\": \"fallow\",\n          \"version\": \"[VERSION]\"",
    )
}

#[test]
fn sarif_health_output_snapshot() {
    let root = PathBuf::from("/project");
    let report = sample_health_report(&root);
    let sarif = build_health_sarif(&report, &root);
    let json_str = serde_json::to_string_pretty(&sarif).expect("should serialize");
    insta::assert_snapshot!(
        "sarif_health_output",
        redact_health_sarif_version(&json_str)
    );
}

#[test]
fn sarif_health_empty_snapshot() {
    let root = PathBuf::from("/project");
    let report = empty_health_report();
    let sarif = build_health_sarif(&report, &root);
    let json_str = serde_json::to_string_pretty(&sarif).expect("should serialize");
    insta::assert_snapshot!("sarif_health_empty", redact_health_sarif_version(&json_str));
}

#[test]
fn codeclimate_health_output_snapshot() {
    let root = PathBuf::from("/project");
    let report = sample_health_report(&root);
    let cc = codeclimate_issues_to_value(&build_health_codeclimate(&report, &root));
    let json_str = serde_json::to_string_pretty(&cc).expect("should serialize");
    insta::assert_snapshot!("codeclimate_health_output", json_str);
}

#[test]
fn codeclimate_health_empty_snapshot() {
    let root = PathBuf::from("/project");
    let report = empty_health_report();
    let cc = codeclimate_issues_to_value(&build_health_codeclimate(&report, &root));
    let json_str = serde_json::to_string_pretty(&cc).expect("should serialize");
    insta::assert_snapshot!("codeclimate_health_empty", json_str);
}

#[test]
fn markdown_health_with_runtime_coverage_snapshot() {
    let root = PathBuf::from("/project");
    let report = health_report_with_runtime_coverage(&root);
    let output = build_health_markdown(&report, &root);
    insta::assert_snapshot!("markdown_health_with_runtime_coverage", output);
}

#[test]
fn sarif_health_with_runtime_coverage_snapshot() {
    let root = PathBuf::from("/project");
    let report = health_report_with_runtime_coverage(&root);
    let sarif = build_health_sarif(&report, &root);
    let json_str = serde_json::to_string_pretty(&sarif).expect("should serialize");
    insta::assert_snapshot!(
        "sarif_health_with_runtime_coverage",
        redact_health_sarif_version(&json_str)
    );
}

#[test]
fn codeclimate_health_with_runtime_coverage_snapshot() {
    let root = PathBuf::from("/project");
    let report = health_report_with_runtime_coverage(&root);
    let cc = codeclimate_issues_to_value(&build_health_codeclimate(&report, &root));
    let json_str = serde_json::to_string_pretty(&cc).expect("should serialize");
    insta::assert_snapshot!("codeclimate_health_with_runtime_coverage", json_str);
}

#[test]
fn json_health_with_runtime_coverage_snapshot() {
    let root = PathBuf::from("/project");
    let report = health_report_with_runtime_coverage(&root);
    let value = build_health_json(&report, &root, Duration::ZERO, false)
        .expect("health JSON build should succeed");
    let json_str = serde_json::to_string_pretty(&value).expect("should serialize");
    insta::assert_snapshot!(
        "json_health_with_runtime_coverage",
        redact_version(&json_str)
    );
}

/// Build a health report with coverage_gaps populated (untested files +
/// untested exports). Locks down the wire shape so the typed-wrapper refactor
/// stays byte-identical.
fn health_report_with_coverage_gaps(root: &Path) -> HealthReport {
    let mut report = sample_health_report(root);
    report.coverage_gaps = Some(CoverageGaps {
        summary: CoverageGapSummary {
            runtime_files: 8,
            covered_files: 5,
            file_coverage_pct: 62.5,
            untested_files: 2,
            untested_exports: 1,
        },
        files: vec![
            UntestedFileFinding::with_actions(
                UntestedFile {
                    path: root.join("src/untested-one.ts"),
                    value_export_count: 3,
                },
                root,
            ),
            UntestedFileFinding::with_actions(
                UntestedFile {
                    path: root.join("src/untested-two.ts"),
                    value_export_count: 1,
                },
                root,
            ),
        ],
        exports: vec![UntestedExportFinding::with_actions(
            UntestedExport {
                path: root.join("src/partial.ts"),
                export_name: "helper".to_string(),
                line: 12,
                col: 7,
            },
            root,
        )],
    });
    report
}

#[test]
fn json_health_with_coverage_gaps_snapshot() {
    let root = PathBuf::from("/project");
    let report = health_report_with_coverage_gaps(&root);
    let value = build_health_json(&report, &root, Duration::ZERO, false)
        .expect("health JSON build should succeed");
    let json_str = serde_json::to_string_pretty(&value).expect("should serialize");
    insta::assert_snapshot!("json_health_with_coverage_gaps", redact_version(&json_str));
}

/// Build a health report with score populated.
fn health_report_with_score(root: &Path) -> HealthReport {
    let mut report = sample_health_report(root);
    report.vital_signs = Some(VitalSigns {
        dead_file_pct: Some(15.5),
        dead_export_pct: Some(30.2),
        avg_cyclomatic: 1.3,
        p90_cyclomatic: 2,
        duplication_pct: None,
        hotspot_count: Some(0),
        maintainability_avg: Some(85.2),
        unused_dep_count: Some(22),
        circular_dep_count: Some(4),
        counts: None,
        unit_size_profile: None,
        unit_interfacing_profile: None,
        p95_fan_in: None,
        coupling_high_pct: None,
        total_loc: 85_000,
        ..Default::default()
    });
    report.health_score = Some(HealthScore {
        formula_version: HEALTH_SCORE_FORMULA_VERSION,
        score: 76.9,
        grade: "B",
        penalties: HealthScorePenalties {
            dead_files: Some(3.1),
            dead_exports: Some(6.0),
            complexity: 0.0,
            p90_complexity: 0.0,
            maintainability: Some(0.0),
            hotspots: Some(0.0),
            unused_deps: Some(10.0),
            circular_deps: Some(4.0),
            unit_size: None,
            coupling: None,
            duplication: None,
        },
    });
    report
}

#[test]
fn markdown_health_with_score_snapshot() {
    let root = PathBuf::from("/project");
    let report = health_report_with_score(&root);
    let output = build_health_markdown(&report, &root);
    insta::assert_snapshot!("markdown_health_with_score", output);
}

#[test]
fn sarif_health_with_score_snapshot() {
    let root = PathBuf::from("/project");
    let report = health_report_with_score(&root);
    let sarif = build_health_sarif(&report, &root);
    let json_str = serde_json::to_string_pretty(&sarif).expect("should serialize");
    insta::assert_snapshot!(
        "sarif_health_with_score",
        redact_health_sarif_version(&json_str)
    );
}

#[test]
fn codeclimate_health_with_score_snapshot() {
    let root = PathBuf::from("/project");
    let report = health_report_with_score(&root);
    let cc = codeclimate_issues_to_value(&build_health_codeclimate(&report, &root));
    let json_str = serde_json::to_string_pretty(&cc).expect("should serialize");
    insta::assert_snapshot!("codeclimate_health_with_score", json_str);
}

/// Build a health report with trend data populated.
fn health_report_with_trend(root: &Path) -> HealthReport {
    let mut report = health_report_with_score(root);
    report.health_trend = Some(HealthTrend {
        compared_to: TrendPoint {
            timestamp: "2026-03-25T14:30:00Z".into(),
            git_sha: Some("abc1234".into()),
            score: Some(72.0),
            grade: Some("B".into()),
            coverage_model: None,
            snapshot_schema_version: None,
        },
        metrics: vec![
            TrendMetric {
                name: "score",
                label: "Health Score",
                previous: 72.0,
                current: 76.9,
                delta: 4.9,
                direction: TrendDirection::Improving,
                unit: "",
                previous_count: None,
                current_count: None,
            },
            TrendMetric {
                name: "dead_file_pct",
                label: "Dead Files",
                previous: 18.0,
                current: 15.5,
                delta: -2.5,
                direction: TrendDirection::Improving,
                unit: "%",
                previous_count: Some(TrendCount {
                    value: 18,
                    total: 100,
                }),
                current_count: Some(TrendCount {
                    value: 16,
                    total: 100,
                }),
            },
            TrendMetric {
                name: "avg_cyclomatic",
                label: "Avg Cyclomatic",
                previous: 1.3,
                current: 1.3,
                delta: 0.0,
                direction: TrendDirection::Stable,
                unit: "",
                previous_count: None,
                current_count: None,
            },
            TrendMetric {
                name: "unused_dep_count",
                label: "Unused Deps",
                previous: 20.0,
                current: 22.0,
                delta: 2.0,
                direction: TrendDirection::Declining,
                unit: "",
                previous_count: None,
                current_count: None,
            },
        ],
        snapshots_loaded: 3,
        overall_direction: TrendDirection::Improving,
    });
    report
}

#[test]
fn markdown_health_with_trend_snapshot() {
    let root = PathBuf::from("/project");
    let report = health_report_with_trend(&root);
    let output = build_health_markdown(&report, &root);
    insta::assert_snapshot!("markdown_health_with_trend", output);
}

/// Build a sample duplication report for snapshot tests.
fn sample_duplication_report(root: &Path) -> DuplicationReport {
    DuplicationReport {
        clone_groups: vec![CloneGroup {
            instances: vec![
                CloneInstance {
                    file: root.join("src/utils.ts"),
                    start_line: 10,
                    end_line: 20,
                    start_col: 0,
                    end_col: 1,
                    fragment:
                        "function validate(input) {\n  if (!input) return false;\n  return true;\n}"
                            .to_string(),
                },
                CloneInstance {
                    file: root.join("src/helpers.ts"),
                    start_line: 5,
                    end_line: 15,
                    start_col: 0,
                    end_col: 1,
                    fragment:
                        "function validate(input) {\n  if (!input) return false;\n  return true;\n}"
                            .to_string(),
                },
            ],
            token_count: 25,
            line_count: 11,
        }],
        clone_families: vec![],
        mirrored_directories: vec![],
        stats: DuplicationStats {
            total_files: 100,
            files_with_clones: 2,
            total_lines: 5000,
            duplicated_lines: 11,
            total_tokens: 25000,
            duplicated_tokens: 25,
            clone_groups: 1,
            clone_instances: 2,
            duplication_percentage: 0.22,
            clone_groups_below_min_occurrences: 0,
        },
    }
}

#[test]
fn markdown_duplication_output_snapshot() {
    let root = PathBuf::from("/project");
    let report = sample_duplication_report(&root);
    let output = build_duplication_markdown(&report, &root);
    insta::assert_snapshot!("markdown_duplication_output", output);
}

#[test]
fn markdown_duplication_empty_snapshot() {
    let root = PathBuf::from("/project");
    let report = DuplicationReport::default();
    let output = build_duplication_markdown(&report, &root);
    insta::assert_snapshot!("markdown_duplication_empty", output);
}

#[test]
fn codeclimate_duplication_output_snapshot() {
    let root = PathBuf::from("/project");
    let report = sample_duplication_report(&root);
    let cc = codeclimate_issues_to_value(&build_duplication_codeclimate(&report, &root));
    let json_str = serde_json::to_string_pretty(&cc).expect("should serialize");
    insta::assert_snapshot!("codeclimate_duplication_output", json_str);
}

#[test]
fn codeclimate_duplication_empty_snapshot() {
    let root = PathBuf::from("/project");
    let report = DuplicationReport::default();
    let cc = codeclimate_issues_to_value(&build_duplication_codeclimate(&report, &root));
    let json_str = serde_json::to_string_pretty(&cc).expect("should serialize");
    insta::assert_snapshot!("codeclimate_duplication_empty", json_str);
}

/// Build a multi-group duplication report exercising the largest-owner rule.
fn sample_grouped_duplication_report(root: &Path) -> DuplicationReport {
    DuplicationReport {
        clone_groups: vec![
            CloneGroup {
                instances: vec![
                    CloneInstance {
                        file: root.join("src/a.ts"),
                        start_line: 10,
                        end_line: 20,
                        start_col: 0,
                        end_col: 1,
                        fragment: "function a() { return 1; }".to_string(),
                    },
                    CloneInstance {
                        file: root.join("src/b.ts"),
                        start_line: 5,
                        end_line: 15,
                        start_col: 0,
                        end_col: 1,
                        fragment: "function a() { return 1; }".to_string(),
                    },
                    CloneInstance {
                        file: root.join("lib/c.ts"),
                        start_line: 30,
                        end_line: 40,
                        start_col: 0,
                        end_col: 1,
                        fragment: "function a() { return 1; }".to_string(),
                    },
                ],
                token_count: 25,
                line_count: 11,
            },
            CloneGroup {
                instances: vec![
                    CloneInstance {
                        file: root.join("lib/x.ts"),
                        start_line: 1,
                        end_line: 8,
                        start_col: 0,
                        end_col: 1,
                        fragment: "const x = 1;".to_string(),
                    },
                    CloneInstance {
                        file: root.join("lib/y.ts"),
                        start_line: 1,
                        end_line: 8,
                        start_col: 0,
                        end_col: 1,
                        fragment: "const x = 1;".to_string(),
                    },
                ],
                token_count: 18,
                line_count: 8,
            },
        ],
        clone_families: vec![],
        mirrored_directories: vec![],
        stats: DuplicationStats {
            total_files: 100,
            files_with_clones: 5,
            total_lines: 5000,
            duplicated_lines: 35,
            total_tokens: 25000,
            duplicated_tokens: 86,
            clone_groups: 2,
            clone_instances: 5,
            duplication_percentage: 0.7,
            clone_groups_below_min_occurrences: 0,
        },
    }
}

#[test]
fn grouped_duplication_json_directory_snapshot() {
    let root = PathBuf::from("/project");
    let report = sample_grouped_duplication_report(&root);
    let resolver = fallow_cli::report::OwnershipResolver::Directory;
    let grouping =
        fallow_cli::report::dupes_grouping::build_duplication_grouping(&report, &root, &resolver);
    let value =
        build_grouped_duplication_json(&report, &grouping, &root, Duration::from_millis(0), false)
            .expect("should serialize");
    let json_str = serde_json::to_string_pretty(&value).expect("should serialize");
    insta::assert_snapshot!(
        "grouped_duplication_json_directory",
        json_str.replace(
            &format!("\"version\": \"{}\"", env!("CARGO_PKG_VERSION")),
            "\"version\": \"[VERSION]\"",
        )
    );
}

#[test]
fn grouped_duplication_codeclimate_directory_snapshot() {
    let root = PathBuf::from("/project");
    let report = sample_grouped_duplication_report(&root);
    let resolver = fallow_cli::report::OwnershipResolver::Directory;
    let mut value = codeclimate_issues_to_value(&build_duplication_codeclimate(&report, &root));
    let mut path_to_owner = rustc_hash::FxHashMap::<String, String>::default();
    for group in &report.clone_groups {
        let owner = fallow_cli::report::dupes_grouping::largest_owner(group, &root, &resolver);
        for instance in &group.instances {
            let rel = instance
                .file
                .strip_prefix(&root)
                .unwrap_or(&instance.file)
                .to_string_lossy()
                .replace('\\', "/");
            path_to_owner.insert(rel, owner.clone());
        }
    }
    if let Some(arr) = value.as_array_mut() {
        for issue in arr {
            let path = issue
                .pointer("/location/path")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();
            let group = path_to_owner
                .get(&path)
                .cloned()
                .unwrap_or_else(|| "(unowned)".to_string());
            issue
                .as_object_mut()
                .expect("issue object")
                .insert("group".to_string(), serde_json::Value::String(group));
        }
    }
    let json_str = serde_json::to_string_pretty(&value).expect("should serialize");
    insta::assert_snapshot!("grouped_duplication_codeclimate_directory", json_str);
}
