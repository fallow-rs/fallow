//! The sample results of the snapshot suite.

use super::*;

/// Build sample `AnalysisResults` with one issue of each type for consistent snapshots.
#[expect(
    clippy::too_many_lines,
    reason = "One block per issue type used across the snapshot suite; the line count grows with new issue types and the structure is intentional."
)]
pub(super) fn sample_results(root: &Path) -> AnalysisResults {
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
            deprecated: false,
            deprecated_reason: None,
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
            deprecated: false,
            deprecated_reason: None,
        }));
    r.unused_dependencies
        .push(UnusedDependencyFinding::with_actions(UnusedDependency {
            package_name: "lodash".to_string(),
            location: DependencyLocation::Dependencies,
            path: root.join("package.json"),
            line: 5,
            used_in_workspaces: Vec::new(),
            declared_and_imported_in: Vec::new(),
        }));
    r.unused_dev_dependencies
        .push(UnusedDevDependencyFinding::with_actions(UnusedDependency {
            package_name: "jest".to_string(),
            location: DependencyLocation::DevDependencies,
            path: root.join("package.json"),
            line: 5,
            used_in_workspaces: Vec::new(),
            declared_and_imported_in: Vec::new(),
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
                declared_and_imported_in: Vec::new(),
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
            line: 1,
            col: 0,
        }));
    r.unused_catalog_entries
        .push(UnusedCatalogEntryFinding::with_actions(
            UnusedCatalogEntry {
                entry_name: "is-even".to_string(),
                catalog_name: "default".to_string(),
                path: PathBuf::from("pnpm-workspace.yaml"),
                line: 6,
                hardcoded_consumers: vec![],
            },
        ));
    r.unused_catalog_entries
        .push(UnusedCatalogEntryFinding::with_actions(
            UnusedCatalogEntry {
                entry_name: "old-thing".to_string(),
                catalog_name: "legacy".to_string(),
                path: PathBuf::from("pnpm-workspace.yaml"),
                line: 12,
                hardcoded_consumers: vec![PathBuf::from("apps/web/package.json")],
            },
        ));
    r.empty_catalog_groups
        .push(EmptyCatalogGroupFinding::with_actions(EmptyCatalogGroup {
            catalog_name: "react17".to_string(),
            path: PathBuf::from("pnpm-workspace.yaml"),
            line: 10,
        }));
    r.unresolved_catalog_references
        .push(UnresolvedCatalogReferenceFinding::with_actions(
            UnresolvedCatalogReference {
                entry_name: "old-react".to_string(),
                catalog_name: "react17".to_string(),
                path: root.join("packages/app/package.json"),
                line: 14,
                available_in_catalogs: vec!["react18".to_string()],
            },
        ));
    r.unused_dependency_overrides.push(
        UnusedDependencyOverrideFinding::with_actions(
            UnusedDependencyOverride {
                raw_key: "axios".to_string(),
                target_package: "axios".to_string(),
                parent_package: None,
                version_constraint: None,
                version_range: "^1.6.0".to_string(),
                source: DependencyOverrideSource::PnpmWorkspaceYaml,
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
        MisconfiguredDependencyOverrideFinding::with_actions(MisconfiguredDependencyOverride {
            raw_key: "@types/react@<<18".to_string(),
            target_package: None,
            raw_value: "18.0.0".to_string(),
            reason: DependencyOverrideMisconfigReason::UnparsableKey,
            source: DependencyOverrideSource::PnpmPackageJson,
            path: root.join("package.json"),
            line: 3,
        }),
    );
    r.invalid_client_exports
        .push(InvalidClientExportFinding::with_actions(
            InvalidClientExport {
                path: root.join("app/page.tsx"),
                export_name: "metadata".to_string(),
                directive: "use client".to_string(),
                line: 3,
                col: 0,
            },
        ));
    r.mixed_client_server_barrels
        .push(MixedClientServerBarrelFinding::with_actions(
            MixedClientServerBarrel {
                path: root.join("app/components/index.ts"),
                client_origin: "./Button".to_string(),
                server_origin: "./fetchUser".to_string(),
                line: 2,
                col: 0,
            },
        ));
    r.unprovided_injects
        .push(UnprovidedInjectFinding::with_actions(UnprovidedInject {
            path: root.join("src/useTheme.ts"),
            key_name: "THEME_KEY".to_string(),
            framework: "vue".to_string(),
            line: 5,
            col: 2,
        }));
    r.unused_component_inputs
        .push(UnusedComponentInputFinding::with_actions(
            UnusedComponentInput {
                path: root.join("src/user-card.component.ts"),
                component_name: "UserCardComponent".to_string(),
                input_name: "variant".to_string(),
                line: 12,
                col: 2,
            },
        ));
    r.unused_component_outputs
        .push(UnusedComponentOutputFinding::with_actions(
            UnusedComponentOutput {
                path: root.join("src/user-card.component.ts"),
                component_name: "UserCardComponent".to_string(),
                output_name: "selected".to_string(),
                line: 15,
                col: 2,
            },
        ));
    r.unused_svelte_events
        .push(UnusedSvelteEventFinding::with_actions(UnusedSvelteEvent {
            path: root.join("src/Child.svelte"),
            component_name: "Child".to_string(),
            event_name: "dead".to_string(),
            line: 6,
            col: 2,
        }));

    r
}
