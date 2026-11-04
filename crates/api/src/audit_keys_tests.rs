use std::path::{Path, PathBuf};

use fallow_config::{FallowConfig, Severity};
use fallow_types::duplicates::{CloneGroup, CloneInstance, DuplicationReport};
use fallow_types::envelope::AuditIntroduced;
use fallow_types::extract::MemberKind;
use fallow_types::output_dead_code::*;
use fallow_types::output_format::OutputFormat;
use fallow_types::results::*;
use rustc_hash::FxHashSet;
use serde_json::json;

use fallow_output::{
    ComplexityViolation, ExceededThreshold, FindingSeverity, HealthFinding, HealthReport,
};

use super::{
    AuditDomainLedger, annotate_dead_code_json, annotate_dupes_json, annotate_health_json,
    annotate_stale_suppressions_json, dead_code_audit_ledger, dead_code_keys, dupe_group_key,
    dupes_keys, health_finding_key, health_keys, relative_key_path, remap_keys_for_renames,
    retain_introduced_dead_code,
};

fn root() -> PathBuf {
    PathBuf::from("/repo")
}

#[test]
fn prop_audit_matches_source_occurrences_and_keeps_warnings_out_of_counts() {
    let root = root();
    let mut chain = PropDrillingChainFinding::with_actions(PropDrillingChain {
        prop: "user".to_owned(),
        depth: 3,
        hops: vec![PropDrillHop {
            file: root.join("src/Page.tsx"),
            line: 4,
            component: "Page".to_owned(),
        }],
    });
    let original = AnalysisResults {
        prop_drilling_chains: vec![chain.clone()],
        ..Default::default()
    };
    let base = dead_code_keys(&original, &root);
    chain.chain.hops[0].line = 12;
    let mut repeated = AnalysisResults {
        prop_drilling_chains: vec![chain.clone(), chain],
        ..Default::default()
    };
    let config: FallowConfig =
        serde_json::from_value(json!({"rules": {"prop-drilling": "error"}})).expect("prop policy");
    let config = config.resolve(root.clone(), OutputFormat::Json, 1, false, true, None);
    let ledger = dead_code_audit_ledger(&repeated, &root, &config, Some(&base));
    assert_eq!(
        (ledger.introduced_count(), ledger.inherited_count()),
        (1, 1)
    );
    assert!(ledger.has_introduced_errors());
    ledger.annotate_results(&mut repeated);
    assert_eq!(
        repeated.prop_drilling_chains[0].introduced,
        Some(AuditIntroduced(false))
    );
    assert_eq!(
        repeated.prop_drilling_chains[1].introduced,
        Some(AuditIntroduced(true))
    );
    retain_introduced_dead_code(&mut repeated, &root, Some(&base));
    assert_eq!(repeated.prop_drilling_chains.len(), 1);
    let mut config = config;
    config.rules.prop_drilling = Severity::Warn;
    let ledger = dead_code_audit_ledger(&original, &root, &config, Some(&FxHashSet::default()));
    assert_eq!((ledger.introduced_count(), ledger.visible_count()), (0, 0));
    assert!(!ledger.has_introduced_errors());
    assert!(ledger.records()[0].introduced);
    let mut ledger = ledger;
    ledger.demote_unattributable_introductions(&FxHashSet::default());
    assert_eq!(
        (
            ledger.introduced_count(),
            ledger.inherited_count(),
            ledger.visible_count()
        ),
        (0, 0, 0)
    );
}

fn export(path: &Path, name: &str) -> UnusedExportFinding {
    UnusedExportFinding::with_actions(UnusedExport {
        path: path.to_path_buf(),
        export_name: name.to_string(),
        is_type_only: false,
        line: 1,
        col: 0,
        span_start: 0,
        is_re_export: false,
        deprecated: false,
        deprecated_reason: None,
    })
}

fn unused_file(path: &Path) -> UnusedFileFinding {
    UnusedFileFinding::with_actions(UnusedFile {
        path: path.to_path_buf(),
    })
}

fn dependency(path: &Path, package_name: &str) -> UnusedDependencyFinding {
    UnusedDependencyFinding::with_actions(UnusedDependency {
        package_name: package_name.to_string(),
        location: DependencyLocation::Dependencies,
        path: path.to_path_buf(),
        line: 4,
        used_in_workspaces: Vec::new(),
        declared_and_imported_in: Vec::new(),
    })
}

fn unresolved(path: &Path, specifier: &str) -> UnresolvedImportFinding {
    UnresolvedImportFinding::with_actions(UnresolvedImport {
        path: path.to_path_buf(),
        specifier: specifier.to_string(),
        line: 2,
        col: 1,
        specifier_col: 8,
    })
}

fn unlisted(path: &Path, package_name: &str) -> UnlistedDependencyFinding {
    UnlistedDependencyFinding::with_actions(UnlistedDependency {
        package_name: package_name.to_string(),
        imported_from: vec![
            ImportSite {
                path: path.to_path_buf(),
                line: 9,
                col: 2,
            },
            ImportSite {
                path: path.to_path_buf(),
                line: 9,
                col: 2,
            },
        ],
    })
}

fn duplicate_export(root: &Path) -> DuplicateExportFinding {
    DuplicateExportFinding::with_actions(DuplicateExport {
        export_name: "Button".to_string(),
        locations: vec![
            DuplicateLocation {
                path: root.join("src/b.ts"),
                line: 1,
                col: 0,
            },
            DuplicateLocation {
                path: root.join("src/a.ts"),
                line: 1,
                col: 0,
            },
            DuplicateLocation {
                path: root.join("src/a.ts"),
                line: 2,
                col: 0,
            },
        ],
    })
}

fn deprecated_export(
    path: &Path,
    name: &str,
) -> fallow_types::output_dead_code::DeprecatedExportInUseFinding {
    fallow_types::output_dead_code::DeprecatedExportInUseFinding::with_actions(
        fallow_types::results::DeprecatedExportInUse {
            path: path.to_path_buf(),
            export_name: name.to_string(),
            is_type_only: false,
            line: 9,
            col: 13,
            span_start: 90,
            deprecated_reason: None,
            consumer_count: 1,
            consumers: Vec::new(),
            public_api: false,
        },
    )
}

fn sample_results(root: &Path) -> AnalysisResults {
    let source = root.join("src/page.ts");
    let package_json = root.join("package.json");
    let mut results = AnalysisResults::default();
    results
        .unused_files
        .push(unused_file(&root.join("src/dead.ts")));
    results.unused_exports.push(export(&source, "loader"));
    results
        .unused_dependencies
        .push(dependency(&package_json, "left-pad"));
    results
        .unresolved_imports
        .push(unresolved(&source, "./missing"));
    results.unlisted_dependencies.push(unlisted(&source, "zod"));
    results.unlisted_binaries.push(
        fallow_types::output_dead_code::UnlistedBinaryFinding::with_actions(
            fallow_types::results::UnlistedBinary {
                binary_name: "hyprctl".to_owned(),
                used_in: vec![fallow_types::results::BinaryUseSite {
                    path: source.clone(),
                    line: 2,
                    col: 0,
                    script_name: None,
                }],
            },
        ),
    );
    results
        .deprecated_exports_in_use
        .push(deprecated_export(&source, "legacyLoader"));
    results.duplicate_exports.push(duplicate_export(root));
    results
}

#[test]
fn relative_key_path_strips_root_and_normalizes_separators() {
    let path = Path::new("/repo/src\\feature\\index.ts");
    assert_eq!(
        relative_key_path(path, Path::new("/repo")),
        "src/feature/index.ts"
    );
}

#[test]
fn dead_code_keys_are_stable_for_unsorted_and_duplicate_locations() {
    let root = root();
    let keys = dead_code_keys(&sample_results(&root), &root);

    assert!(keys.contains("unused-file:src/dead.ts"));
    assert!(keys.contains("unused-export:src/page.ts:loader"));
    assert!(keys.contains("unused-dependency:package.json:left-pad"));
    assert!(keys.contains("unresolved-import:src/page.ts:./missing"));
    assert!(keys.contains("unlisted-dependency:zod"));
    assert!(keys.contains("duplicate-export:Button:src/a.ts|src/b.ts"));
}

fn type_member_and_dependency_results(root: &Path) -> AnalysisResults {
    let source = root.join("src/types.ts");
    let package_json = root.join("package.json");
    let mut results = AnalysisResults::default();
    results
        .unused_types
        .push(UnusedTypeFinding::with_actions(UnusedExport {
            path: source.clone(),
            export_name: "UnusedType".to_string(),
            is_type_only: true,
            line: 3,
            col: 0,
            span_start: 12,
            is_re_export: false,
            deprecated: false,
            deprecated_reason: None,
        }));
    results
        .private_type_leaks
        .push(PrivateTypeLeakFinding::with_actions(PrivateTypeLeak {
            path: source.clone(),
            export_name: "makePublic".to_string(),
            type_name: "PrivateShape".to_string(),
            line: 7,
            col: 12,
            span_start: 64,
            semantic: None,
        }));
    results
        .unused_dev_dependencies
        .push(UnusedDevDependencyFinding::with_actions(UnusedDependency {
            package_name: "vite".to_string(),
            location: DependencyLocation::DevDependencies,
            path: package_json.clone(),
            line: 10,
            used_in_workspaces: Vec::new(),
            declared_and_imported_in: Vec::new(),
        }));
    results
        .unused_optional_dependencies
        .push(UnusedOptionalDependencyFinding::with_actions(
            UnusedDependency {
                package_name: "fsevents".to_string(),
                location: DependencyLocation::OptionalDependencies,
                path: package_json.clone(),
                line: 11,
                used_in_workspaces: Vec::new(),
                declared_and_imported_in: Vec::new(),
            },
        ));
    results
        .unused_enum_members
        .push(UnusedEnumMemberFinding::with_actions(UnusedMember {
            path: source.clone(),
            parent_name: "Status".to_string(),
            member_name: "Idle".to_string(),
            kind: MemberKind::EnumMember,
            line: 15,
            col: 2,
        }));
    results
        .unused_class_members
        .push(UnusedClassMemberFinding::with_actions(UnusedMember {
            path: source,
            parent_name: "Controller".to_string(),
            member_name: "legacy".to_string(),
            kind: MemberKind::ClassMethod,
            line: 21,
            col: 2,
        }));
    results
        .type_only_dependencies
        .push(TypeOnlyDependencyFinding::with_actions(
            TypeOnlyDependency {
                package_name: "zod".to_string(),
                path: package_json.clone(),
                line: 12,
            },
        ));
    results
        .test_only_dependencies
        .push(TestOnlyDependencyFinding::with_actions(
            TestOnlyDependency {
                package_name: "vitest".to_string(),
                path: package_json,
                line: 13,
            },
        ));

    results
}

#[test]
fn dead_code_keys_cover_type_member_and_dependency_variants() {
    let root = root();
    let results = type_member_and_dependency_results(&root);

    let keys = dead_code_keys(&results, &root);

    assert!(keys.contains("unused-type:src/types.ts:UnusedType"));
    assert!(keys.contains("private-type-leak:src/types.ts:makePublic:PrivateShape"));
    assert!(keys.contains("unused-dev-dependency:package.json:vite"));
    assert!(keys.contains("unused-optional-dependency:package.json:fsevents"));
    assert!(keys.contains("unused-enum-member:src/types.ts:Status:Idle"));
    assert!(keys.contains("unused-class-member:src/types.ts:Controller:legacy"));
    assert!(keys.contains("type-only-dependency:package.json:zod"));
    assert!(keys.contains("test-only-dependency:package.json:vitest"));
}

#[expect(
    clippy::too_many_lines,
    reason = "test fixture; linear setup/assert, length is not a maintainability concern"
)]
fn graph_boundary_catalog_override_results(root: &std::path::Path) -> AnalysisResults {
    let source = root.join("src/app.ts");
    let other = root.join("src/other.ts");
    let workspace = root.join("pnpm-workspace.yaml");
    let mut results = AnalysisResults::default();
    results
        .circular_dependencies
        .push(CircularDependencyFinding::with_actions(
            CircularDependency {
                files: vec![other.clone(), source.clone()],
                length: 2,
                line: 4,
                col: 0,
                edges: Vec::new(),
                is_cross_package: false,
            },
        ));
    results
        .re_export_cycles
        .push(ReExportCycleFinding::with_actions(ReExportCycle {
            files: vec![source.clone()],
            kind: ReExportCycleKind::SelfLoop,
        }));
    results.package_cycles.push(
        fallow_types::output_dead_code::PackageCycleFinding::with_actions(
            fallow_types::results::PackageCycle {
                packages: vec!["@x/a".to_string(), "@x/b".to_string()],
                package_roots: Vec::new(),
                length: 2,
                edges: vec![fallow_types::results::PackageCycleEdge {
                    from_package: "@x/a".to_string(),
                    to_package: "@x/b".to_string(),
                    path: source.clone(),
                    target_path: other.clone(),
                    line: 4,
                    col: 0,
                    type_only: false,
                }],
                group_truncated: false,
            },
        ),
    );
    results
        .boundary_violations
        .push(BoundaryViolationFinding::with_actions(BoundaryViolation {
            from_path: source.clone(),
            to_path: other,
            from_zone: "ui".to_string(),
            to_zone: "server".to_string(),
            import_specifier: "../other".to_string(),
            line: 1,
            col: 0,
            via_path: None,
        }));
    results
        .boundary_coverage_violations
        .push(BoundaryCoverageViolationFinding::with_actions(
            BoundaryCoverageViolation {
                path: root.join("src/unmatched.ts"),
                line: 1,
                col: 0,
            },
        ));
    results
        .boundary_call_violations
        .push(BoundaryCallViolationFinding::with_actions(
            BoundaryCallViolation {
                path: source.clone(),
                line: 12,
                col: 4,
                zone: "ui".to_string(),
                callee: "child_process.exec".to_string(),
                pattern: "child_process.*".to_string(),
            },
        ));
    results.stale_suppressions.push(StaleSuppression {
        finding_id: None,
        path: source,
        line: 2,
        col: 0,
        origin: SuppressionOrigin::Comment {
            issue_kind: Some("unused-export".to_string()),
            reason: None,
            is_file_level: false,
            kind_known: true,
        },
        missing_reason: false,
        actions: StaleSuppression::actions_for(false),
        effective_severity: None,
    });
    results.stale_suppressions.push(StaleSuppression {
        finding_id: None,
        path: root.join("src/app.ts"),
        line: 2,
        col: 0,
        origin: SuppressionOrigin::Comment {
            issue_kind: Some("unused-export".to_string()),
            reason: None,
            is_file_level: false,
            kind_known: true,
        },
        missing_reason: true,
        actions: StaleSuppression::actions_for(true),
        effective_severity: None,
    });
    results
        .unresolved_catalog_references
        .push(UnresolvedCatalogReferenceFinding::with_actions(
            UnresolvedCatalogReference {
                entry_name: "react".to_string(),
                catalog_name: "default".to_string(),
                path: root.join("packages/app/package.json"),
                line: 9,
                available_in_catalogs: vec!["react18".to_string()],
            },
        ));
    results
        .unused_catalog_entries
        .push(UnusedCatalogEntryFinding::with_actions(
            UnusedCatalogEntry {
                entry_name: "lodash".to_string(),
                catalog_name: "default".to_string(),
                path: workspace.clone(),
                line: 3,
                hardcoded_consumers: Vec::new(),
            },
        ));
    results
        .empty_catalog_groups
        .push(EmptyCatalogGroupFinding::with_actions(EmptyCatalogGroup {
            catalog_name: "react17".to_string(),
            path: workspace.clone(),
            line: 7,
        }));
    results
        .unused_dependency_overrides
        .push(UnusedDependencyOverrideFinding::with_actions(
            UnusedDependencyOverride {
                raw_key: "left-pad".to_string(),
                target_package: "left-pad".to_string(),
                parent_package: None,
                version_constraint: None,
                version_range: "^1.3.0".to_string(),
                source: DependencyOverrideSource::PnpmWorkspaceYaml,
                path: workspace.clone(),
                line: 11,
                hint: None,
            },
        ));
    results.misconfigured_dependency_overrides.push(
        MisconfiguredDependencyOverrideFinding::with_actions(MisconfiguredDependencyOverride {
            raw_key: ">".to_string(),
            target_package: None,
            raw_value: String::new(),
            reason: DependencyOverrideMisconfigReason::UnparsableKey,
            source: DependencyOverrideSource::PnpmWorkspaceYaml,
            path: workspace,
            line: 12,
        }),
    );
    results
}

#[test]
fn dead_code_keys_cover_graph_boundary_catalog_and_override_variants() {
    let root = root();
    let results = graph_boundary_catalog_override_results(&root);

    let keys = dead_code_keys(&results, &root);

    assert!(keys.contains("circular-dependency:src/app.ts|src/other.ts"));
    assert!(keys.contains("re-export-cycle:self-loop:src/app.ts"));
    assert!(keys.contains("boundary-violation:src/app.ts:src/other.ts"));
    assert!(keys.contains("boundary-coverage:src/unmatched.ts"));
    assert!(keys.contains("boundary-call-violation:src/app.ts:child_process.exec"));
    assert!(keys.contains("stale-suppression:src/app.ts:comment:unused-export:line"));
    assert!(keys.contains("missing-suppression-reason:src/app.ts:comment:unused-export:line"));
    assert!(keys.contains("unresolved-catalog-reference:packages/app/package.json:default:react"));
    assert!(keys.contains("unused-catalog-entry:pnpm-workspace.yaml:default:lodash"));
    assert!(keys.contains("empty-catalog-group:pnpm-workspace.yaml:react17"));
    assert!(keys.contains("unused-dependency-override:pnpm-workspace.yaml:left-pad"));
    assert!(keys.contains("misconfigured-dependency-override:pnpm-workspace.yaml:>"));
}

#[test]
fn retain_introduced_dead_code_keeps_only_findings_absent_from_base() {
    let root = root();
    let mut results = sample_results(&root);
    let base = FxHashSet::from_iter([
        "unused-file:src/dead.ts".to_string(),
        "unused-dependency:package.json:left-pad".to_string(),
        "unresolved-import:src/page.ts:./missing".to_string(),
    ]);

    retain_introduced_dead_code(&mut results, &root, Some(&base));

    assert!(results.unused_files.is_empty());
    assert!(results.unused_dependencies.is_empty());
    assert!(results.unresolved_imports.is_empty());
    assert_eq!(results.unused_exports.len(), 1);
    assert_eq!(results.unlisted_dependencies.len(), 1);
    assert_eq!(results.duplicate_exports.len(), 1);
}

#[test]
fn annotate_dead_code_json_marks_introduced_status_by_matching_key_order() {
    let root = root();
    let results = sample_results(&root);
    let base = FxHashSet::from_iter([
        "unused-file:src/dead.ts".to_string(),
        "unlisted-dependency:zod".to_string(),
    ]);
    let mut json = json!({
        "unused_files": [{}],
        "unused_exports": [{}],
        "unused_dependencies": [{}],
        "unresolved_imports": [{}],
        "unlisted_dependencies": [{}],
        "duplicate_exports": [{}],
    });

    annotate_dead_code_json(&mut json, &results, &root, &base);

    assert_eq!(json["unused_files"][0]["introduced"], false);
    assert_eq!(json["unused_exports"][0]["introduced"], true);
    assert_eq!(json["unused_dependencies"][0]["introduced"], true);
    assert_eq!(json["unresolved_imports"][0]["introduced"], true);
    assert_eq!(json["unlisted_dependencies"][0]["introduced"], false);
    assert_eq!(json["duplicate_exports"][0]["introduced"], true);
}

// --- key-building coverage for lines 68-177 (framework-specific key fns) ---

fn effect_results(root: &Path) -> AnalysisResults {
    use fallow_types::output_dead_code::{
        UnproducedCatchTagFinding, UnusedEffectLayerFinding, UnusedTaggedErrorFinding,
    };
    use fallow_types::results::{
        EffectLayerKind, UnproducedCatchTag, UnusedEffectLayer, UnusedTaggedError,
    };
    AnalysisResults {
        unused_effect_layers: vec![UnusedEffectLayerFinding::with_actions(UnusedEffectLayer {
            path: root.join("src/services.ts"),
            name: "CacheLive".to_owned(),
            kind: EffectLayerKind::TopLevel,
            line: 3,
            col: 0,
            dead_consumers: vec!["OrphanLive".to_owned()],
        })],
        unused_tagged_errors: vec![UnusedTaggedErrorFinding::with_actions(UnusedTaggedError {
            path: root.join("src/errors.ts"),
            name: "Expired".to_owned(),
            tag: "Expired".to_owned(),
            line: 4,
            col: 0,
            catch_sites: Vec::new(),
        })],
        unproduced_catch_tags: vec![UnproducedCatchTagFinding::with_actions(
            UnproducedCatchTag {
                path: root.join("src/main.ts"),
                tag: "Expired".to_owned(),
                line: 5,
                col: 0,
                candidate_classes: vec!["Expired".to_owned()],
            },
        )],
        ..Default::default()
    }
}

fn framework_inject_and_render_results(root: &Path) -> AnalysisResults {
    let src = root.join("src/App.vue");
    let mut results = AnalysisResults::default();
    results
        .unprovided_injects
        .push(UnprovidedInjectFinding::with_actions(UnprovidedInject {
            path: src.clone(),
            key_name: "userStore".to_string(),
            framework: "vue".to_string(),
            line: 5,
            col: 0,
        }));
    results
        .unrendered_components
        .push(UnrenderedComponentFinding::with_actions(
            UnrenderedComponent {
                path: src.clone(),
                component_name: "MyModal".to_string(),
                framework: "vue".to_string(),
                reachable_via: None,
                line: 1,
                col: 0,
            },
        ));
    results
        .unused_component_props
        .push(UnusedComponentPropFinding::with_actions(
            UnusedComponentProp {
                path: src.clone(),
                component_name: "MyModal".to_string(),
                prop_name: "title".to_string(),
                line: 3,
                col: 2,
            },
        ));
    results
        .unused_component_emits
        .push(UnusedComponentEmitFinding::with_actions(
            UnusedComponentEmit {
                path: src,
                component_name: "MyModal".to_string(),
                emit_name: "close".to_string(),
                line: 4,
                col: 2,
            },
        ));
    results
        .unused_svelte_events
        .push(UnusedSvelteEventFinding::with_actions(UnusedSvelteEvent {
            path: root.join("src/Counter.svelte"),
            component_name: "Counter".to_string(),
            event_name: "increment".to_string(),
            line: 8,
            col: 0,
        }));

    results
}

#[test]
fn dead_code_keys_cover_framework_inject_and_render_variants() {
    let root = root();
    let results = framework_inject_and_render_results(&root);

    let keys = dead_code_keys(&results, &root);

    assert!(keys.contains("unprovided-inject:src/App.vue:userStore"));
    assert!(keys.contains("unrendered-component:src/App.vue:MyModal"));
    assert!(keys.contains("unused-component-prop:src/App.vue:MyModal:title"));
    assert!(keys.contains("unused-component-emit:src/App.vue:MyModal:close"));
    assert!(keys.contains("unused-svelte-event:src/Counter.svelte:Counter:increment"));
}

fn server_action_load_data_and_route_results(root: &Path) -> AnalysisResults {
    let actions_file = root.join("src/actions/submit.ts");
    let page_file = root.join("src/routes/blog/+page.server.ts");
    let route_file = root.join("app/(auth)/login/page.tsx");
    let route_file2 = root.join("app/login/page.tsx");
    let mut results = AnalysisResults::default();
    results
        .unused_server_actions
        .push(UnusedServerActionFinding::with_actions(
            UnusedServerAction {
                path: actions_file,
                action_name: "submitForm".to_string(),
                line: 2,
                col: 0,
            },
        ));
    results
        .unused_load_data_keys
        .push(UnusedLoadDataKeyFinding::with_actions(UnusedLoadDataKey {
            path: page_file,
            key_name: "posts".to_string(),
            line: 10,
            col: 4,
            route_dir: None,
        }));
    results
        .route_collisions
        .push(RouteCollisionFinding::with_actions(RouteCollision {
            path: route_file.clone(),
            url: "/login".to_string(),
            conflicting_paths: vec![route_file2.clone()],
            line: 1,
            col: 0,
        }));
    results
        .dynamic_segment_name_conflicts
        .push(DynamicSegmentNameConflictFinding::with_actions(
            DynamicSegmentNameConflict {
                path: route_file,
                position: "/shop".to_string(),
                conflicting_segments: vec!["[id]".to_string(), "[slug]".to_string()],
                conflicting_paths: vec![route_file2],
                line: 1,
                col: 0,
            },
        ));

    results
}

#[test]
fn dead_code_keys_cover_server_action_load_data_and_route_variants() {
    let root = root();
    let results = server_action_load_data_and_route_results(&root);

    let keys = dead_code_keys(&results, &root);

    assert!(keys.contains("unused-server-action:src/actions/submit.ts:submitForm"));
    assert!(keys.contains("unused-load-data-key:src/routes/blog/+page.server.ts:posts"));
    assert!(keys.contains("route-collision:app/(auth)/login/page.tsx:/login"));
    assert!(keys.contains("dynamic-segment-name-conflict:app/(auth)/login/page.tsx:/shop"));
}

fn angular_input_output_and_policy_results(root: &Path) -> AnalysisResults {
    let component = root.join("src/app/card.component.ts");
    let src = root.join("src/utils.ts");
    let mut results = AnalysisResults::default();
    results
        .unused_component_inputs
        .push(UnusedComponentInputFinding::with_actions(
            UnusedComponentInput {
                path: component.clone(),
                component_name: "CardComponent".to_string(),
                input_name: "label".to_string(),
                line: 12,
                col: 4,
            },
        ));
    results
        .unused_component_outputs
        .push(UnusedComponentOutputFinding::with_actions(
            UnusedComponentOutput {
                path: component,
                component_name: "CardComponent".to_string(),
                output_name: "clicked".to_string(),
                line: 13,
                col: 4,
            },
        ));
    results
        .policy_violations
        .push(PolicyViolationFinding::with_actions(PolicyViolation {
            path: src,
            line: 7,
            col: 0,
            pack: "security".to_string(),
            rule_id: "no-eval".to_string(),
            kind: PolicyRuleKind::BannedCall,
            matched: "eval".to_string(),
            severity: PolicyViolationSeverity::Error,
            message: None,
        }));

    results
}

#[test]
fn dead_code_keys_cover_angular_input_output_and_policy_variants() {
    let root = root();
    let results = angular_input_output_and_policy_results(&root);

    let keys = dead_code_keys(&results, &root);

    assert!(keys.contains("unused-component-input:src/app/card.component.ts:CardComponent:label"));
    assert!(
        keys.contains("unused-component-output:src/app/card.component.ts:CardComponent:clicked")
    );
    assert!(keys.contains("policy-violation:src/utils.ts:security:no-eval:eval"));
}

#[test]
fn dead_code_keys_cover_re_export_cycle_multi_node_variant() {
    let root = root();
    let a = root.join("src/a.ts");
    let b = root.join("src/b.ts");
    let mut results = AnalysisResults::default();
    results
        .re_export_cycles
        .push(ReExportCycleFinding::with_actions(ReExportCycle {
            files: vec![b, a],
            kind: ReExportCycleKind::MultiNode,
        }));

    let keys = dead_code_keys(&results, &root);

    // multi-node variant hits line 275; files are sorted
    assert!(keys.contains("re-export-cycle:multi-node:src/a.ts|src/b.ts"));
}

#[test]
fn dead_code_keys_cover_package_cycles_by_package_roots() {
    let root = root();
    let mut results = AnalysisResults::default();
    results.package_cycles.push(
        fallow_types::output_dead_code::PackageCycleFinding::with_actions(
            fallow_types::results::PackageCycle {
                packages: vec!["@x/b".to_string(), "@x/a".to_string()],
                package_roots: vec![root.join("packages/b"), root.join("packages/a")],
                length: 2,
                edges: vec![fallow_types::results::PackageCycleEdge {
                    from_package: "@x/a".to_string(),
                    to_package: "@x/b".to_string(),
                    path: root.join("packages/a/src/index.ts"),
                    target_path: root.join("packages/b/src/index.ts"),
                    line: 3,
                    col: 0,
                    type_only: false,
                }],
                group_truncated: false,
            },
        ),
    );

    let keys = dead_code_keys(&results, &root);

    assert!(
        keys.contains("package-cycle:packages/a|packages/b"),
        "{keys:?}"
    );
}

fn unused_store_member_results(root: &Path) -> AnalysisResults {
    let src = root.join("src/store.ts");
    let mut results = AnalysisResults::default();
    results
        .unused_store_members
        .push(UnusedStoreMemberFinding::with_actions(UnusedMember {
            path: src,
            parent_name: "useAuthStore".to_string(),
            member_name: "resetPassword".to_string(),
            kind: MemberKind::ClassMethod,
            line: 42,
            col: 2,
        }));

    results
}

#[test]
fn dead_code_keys_cover_unused_store_member() {
    let root = root();
    let results = unused_store_member_results(&root);

    let keys = dead_code_keys(&results, &root);

    assert!(keys.contains("unused-store-member:src/store.ts:useAuthStore:resetPassword"));
}

// --- annotate_dead_code_json for framework / component / render / policy arrays ---

#[test]
fn annotate_dead_code_json_marks_framework_keys_correctly() {
    let root = root();
    let src = root.join("src/App.vue");
    let mut results = AnalysisResults::default();
    results
        .unprovided_injects
        .push(UnprovidedInjectFinding::with_actions(UnprovidedInject {
            path: src.clone(),
            key_name: "theme".to_string(),
            framework: "vue".to_string(),
            line: 3,
            col: 0,
        }));
    results
        .unrendered_components
        .push(UnrenderedComponentFinding::with_actions(
            UnrenderedComponent {
                path: src.clone(),
                component_name: "Dialog".to_string(),
                framework: "vue".to_string(),
                reachable_via: None,
                line: 1,
                col: 0,
            },
        ));
    results
        .unused_component_props
        .push(UnusedComponentPropFinding::with_actions(
            UnusedComponentProp {
                path: src.clone(),
                component_name: "Dialog".to_string(),
                prop_name: "open".to_string(),
                line: 5,
                col: 2,
            },
        ));
    results
        .unused_component_emits
        .push(UnusedComponentEmitFinding::with_actions(
            UnusedComponentEmit {
                path: src,
                component_name: "Dialog".to_string(),
                emit_name: "dismiss".to_string(),
                line: 6,
                col: 2,
            },
        ));

    // Only the inject is in the base; the rest are new.
    let base = FxHashSet::from_iter(["unprovided-inject:src/App.vue:theme".to_string()]);
    let mut json_val = json!({
        "unprovided_injects": [{}],
        "unrendered_components": [{}],
        "unused_component_props": [{}],
        "unused_component_emits": [{}],
    });

    annotate_dead_code_json(&mut json_val, &results, &root, &base);

    assert_eq!(json_val["unprovided_injects"][0]["introduced"], false);
    assert_eq!(json_val["unrendered_components"][0]["introduced"], true);
    assert_eq!(json_val["unused_component_props"][0]["introduced"], true);
    assert_eq!(json_val["unused_component_emits"][0]["introduced"], true);
}

#[test]
fn annotate_dead_code_json_marks_component_io_and_route_keys_correctly() {
    let root = root();
    let component = root.join("src/card.component.ts");
    let svelte_file = root.join("src/Counter.svelte");
    let page_file = root.join("src/routes/+page.server.ts");
    let route_file = root.join("app/about/page.tsx");
    let route_file2 = root.join("app/(info)/about/page.tsx");
    let mut results = AnalysisResults::default();
    results
        .unused_component_inputs
        .push(UnusedComponentInputFinding::with_actions(
            UnusedComponentInput {
                path: component.clone(),
                component_name: "CardComponent".to_string(),
                input_name: "size".to_string(),
                line: 8,
                col: 2,
            },
        ));
    results
        .unused_component_outputs
        .push(UnusedComponentOutputFinding::with_actions(
            UnusedComponentOutput {
                path: component,
                component_name: "CardComponent".to_string(),
                output_name: "hovered".to_string(),
                line: 9,
                col: 2,
            },
        ));
    results
        .unused_svelte_events
        .push(UnusedSvelteEventFinding::with_actions(UnusedSvelteEvent {
            path: svelte_file,
            component_name: "Counter".to_string(),
            event_name: "reset".to_string(),
            line: 12,
            col: 0,
        }));
    results
        .unused_server_actions
        .push(UnusedServerActionFinding::with_actions(
            UnusedServerAction {
                path: page_file,
                action_name: "deletePost".to_string(),
                line: 3,
                col: 0,
            },
        ));
    results
        .route_collisions
        .push(RouteCollisionFinding::with_actions(RouteCollision {
            path: route_file.clone(),
            url: "/about".to_string(),
            conflicting_paths: vec![route_file2.clone()],
            line: 1,
            col: 0,
        }));
    results
        .dynamic_segment_name_conflicts
        .push(DynamicSegmentNameConflictFinding::with_actions(
            DynamicSegmentNameConflict {
                path: route_file,
                position: "/".to_string(),
                conflicting_segments: vec!["[id]".to_string()],
                conflicting_paths: vec![route_file2],
                line: 1,
                col: 0,
            },
        ));

    // Nothing is in base: all are introduced.
    let base = FxHashSet::default();
    let mut json_val = json!({
        "unused_component_inputs": [{}],
        "unused_component_outputs": [{}],
        "unused_svelte_events": [{}],
        "unused_server_actions": [{}],
        "route_collisions": [{}],
        "dynamic_segment_name_conflicts": [{}],
    });

    annotate_dead_code_json(&mut json_val, &results, &root, &base);

    assert_eq!(json_val["unused_component_inputs"][0]["introduced"], true);
    assert_eq!(json_val["unused_component_outputs"][0]["introduced"], true);
    assert_eq!(json_val["unused_svelte_events"][0]["introduced"], true);
    assert_eq!(json_val["unused_server_actions"][0]["introduced"], true);
    assert_eq!(json_val["route_collisions"][0]["introduced"], true);
    assert_eq!(
        json_val["dynamic_segment_name_conflicts"][0]["introduced"],
        true
    );
}

#[test]
fn annotate_dead_code_json_marks_members_and_dependencies_correctly() {
    let root = root();
    let src = root.join("src/types.ts");
    let pkg = root.join("package.json");
    let mut results = AnalysisResults::default();
    results
        .unused_enum_members
        .push(UnusedEnumMemberFinding::with_actions(UnusedMember {
            path: src.clone(),
            parent_name: "Color".to_string(),
            member_name: "Blue".to_string(),
            kind: MemberKind::EnumMember,
            line: 5,
            col: 2,
        }));
    results
        .unused_class_members
        .push(UnusedClassMemberFinding::with_actions(UnusedMember {
            path: src.clone(),
            parent_name: "Service".to_string(),
            member_name: "reset".to_string(),
            kind: MemberKind::ClassMethod,
            line: 20,
            col: 2,
        }));
    results
        .unused_store_members
        .push(UnusedStoreMemberFinding::with_actions(UnusedMember {
            path: src,
            parent_name: "useStore".to_string(),
            member_name: "logout".to_string(),
            kind: MemberKind::ClassMethod,
            line: 30,
            col: 2,
        }));
    results
        .unused_dev_dependencies
        .push(UnusedDevDependencyFinding::with_actions(UnusedDependency {
            package_name: "typescript".to_string(),
            location: DependencyLocation::DevDependencies,
            path: pkg.clone(),
            line: 8,
            used_in_workspaces: Vec::new(),
            declared_and_imported_in: Vec::new(),
        }));
    results
        .type_only_dependencies
        .push(TypeOnlyDependencyFinding::with_actions(
            TypeOnlyDependency {
                package_name: "zod".to_string(),
                path: pkg.clone(),
                line: 9,
            },
        ));
    results
        .test_only_dependencies
        .push(TestOnlyDependencyFinding::with_actions(
            TestOnlyDependency {
                package_name: "vitest".to_string(),
                path: pkg,
                line: 10,
            },
        ));

    // Enum member and dev-dep are in base; class member, store member, type-only
    // dep, and test-only dep are new.
    let base = FxHashSet::from_iter([
        "unused-enum-member:src/types.ts:Color:Blue".to_string(),
        "unused-dev-dependency:package.json:typescript".to_string(),
    ]);
    let mut json_val = json!({
        "unused_enum_members": [{}],
        "unused_class_members": [{}],
        "unused_store_members": [{}],
        "unused_dev_dependencies": [{}],
        "type_only_dependencies": [{}],
        "test_only_dependencies": [{}],
    });

    annotate_dead_code_json(&mut json_val, &results, &root, &base);

    assert_eq!(json_val["unused_enum_members"][0]["introduced"], false);
    assert_eq!(json_val["unused_class_members"][0]["introduced"], true);
    assert_eq!(json_val["unused_store_members"][0]["introduced"], true);
    assert_eq!(json_val["unused_dev_dependencies"][0]["introduced"], false);
    assert_eq!(json_val["type_only_dependencies"][0]["introduced"], true);
    assert_eq!(json_val["test_only_dependencies"][0]["introduced"], true);
}

#[test]
fn annotate_dead_code_json_is_noop_when_issue_keys_absent() {
    let root = root();
    let results = sample_results(&root);
    let base = FxHashSet::default();
    let mut json_val = json!({"other_key": []});
    let original = json_val.clone();

    annotate_dead_code_json(&mut json_val, &results, &root, &base);

    assert_eq!(json_val, original);
}

// --- annotate_health_json ---

fn make_violation(path: &Path, name: &str) -> ComplexityViolation {
    ComplexityViolation {
        path: path.to_path_buf(),
        name: name.to_string(),
        line: 1,
        col: 0,
        cyclomatic: 20,
        cognitive: 5,
        line_count: 30,
        param_count: 2,
        react_hook_count: 0,
        react_jsx_max_depth: 0,
        react_prop_count: 0,
        react_hook_profile: None,
        exceeded: ExceededThreshold::Cyclomatic,
        effective_severity: None,
        severity: FindingSeverity::High,
        ..Default::default()
    }
}

fn make_health_report(paths_and_names: &[(&Path, &str)]) -> HealthReport {
    let findings = paths_and_names
        .iter()
        .map(|(path, name)| HealthFinding::from(make_violation(path, name)))
        .collect();
    HealthReport {
        findings,
        ..HealthReport::default()
    }
}

#[test]
fn health_keys_produces_stable_key_per_finding() {
    let root = root();
    let path = root.join("src/heavy.ts");
    let report = make_health_report(&[(&path, "processAll")]);
    let keys = health_keys(&report, &root);
    assert!(keys.contains("complexity:src/heavy.ts:processAll"));
}

#[test]
fn health_finding_key_uses_path_and_name() {
    let root = root();
    let path = root.join("src/heavy.ts");
    let violation = make_violation(&path, "render");
    let key = health_finding_key(&violation, &root);
    assert_eq!(key, "complexity:src/heavy.ts:render");
}

#[test]
fn annotate_health_json_marks_introduced_and_inherited_flags() {
    let root = root();
    let path_a = root.join("src/heavy.ts");
    let path_b = root.join("src/other.ts");
    let report = make_health_report(&[(&path_a, "doWork"), (&path_b, "render")]);

    // Only path_b:render is in the base.
    let base = FxHashSet::from_iter(["complexity:src/other.ts:render".to_string()]);
    let mut json_val = json!({
        "findings": [{}, {}],
    });

    annotate_health_json(&mut json_val, &report, &root, &base);

    assert_eq!(json_val["findings"][0]["introduced"], true);
    assert_eq!(json_val["findings"][1]["introduced"], false);
}

#[test]
fn annotate_health_json_keeps_the_typed_complexity_flag() {
    let root = root();
    let path = root.join("src/other.ts");
    let mut report = make_health_report(&[(&path, "render")]);
    // The metric comparison found a worse value for an inherited key.
    report.findings[0].introduced = Some(true);
    let base = FxHashSet::from_iter(["complexity:src/other.ts:render".to_string()]);
    let mut json_val = json!({ "findings": [{}] });

    annotate_health_json(&mut json_val, &report, &root, &base);

    assert_eq!(json_val["findings"][0]["introduced"], true);
}

#[test]
fn annotate_health_json_is_noop_when_findings_key_absent() {
    let root = root();
    let report = make_health_report(&[]);
    let base = FxHashSet::default();
    let mut json_val = json!({"summary": {}});
    let original = json_val.clone();

    annotate_health_json(&mut json_val, &report, &root, &base);

    assert_eq!(json_val, original);
}

// --- annotate_dupes_json and dupe_group_key ---

fn make_clone_group(files: &[PathBuf], fragment: &str) -> CloneGroup {
    CloneGroup {
        instances: files
            .iter()
            .map(|f| CloneInstance {
                is_symlink: false,
                file: f.clone(),
                start_line: 1,
                end_line: 5,
                start_col: 0,
                end_col: 80,
                fragment: fragment.to_string(),
            })
            .collect(),
        token_count: 10,
        line_count: 5,
        similarity: None,
    }
}

fn make_duplication_report(groups: Vec<CloneGroup>) -> DuplicationReport {
    DuplicationReport {
        clone_groups: groups,
        clone_families: Vec::new(),
        mirrored_directories: Vec::new(),
        stats: fallow_types::duplicates::DuplicationStats::default(),
    }
}

#[test]
fn dupe_group_key_is_stable_for_sorted_deduplicated_files() {
    let root = root();
    let a = root.join("src/a.ts");
    let b = root.join("src/b.ts");
    // Build two groups with same fragment but different file order.
    let group_ab = make_clone_group(&[a.clone(), b.clone()], "const x = 1;");
    let group_ba = make_clone_group(&[b, a], "const x = 1;");
    let key_ab = dupe_group_key(&group_ab, &root);
    let key_ba = dupe_group_key(&group_ba, &root);
    // File list is sorted so both keys share the same file prefix.
    assert!(key_ab.starts_with("dupe:src/a.ts|src/b.ts:"));
    assert!(key_ba.starts_with("dupe:src/a.ts|src/b.ts:"));
    // Same fragment => same hash.
    assert_eq!(key_ab, key_ba);
}

#[test]
fn preexisting_dupe_group_keys_collects_only_groups_outside_added_lines() {
    let root = root();
    let a = root.join("src/a.ts");
    let b = root.join("src/b.ts");
    let c = root.join("src/c.ts");
    // Instances span lines 1..=5. The diff adds line 3 in src/c.ts only.
    let untouched = make_clone_group(&[a.clone(), b], "shared scaffolding");
    let touched = make_clone_group(&[a, c], "pasted block");
    let diff = fallow_output::DiffIndex::from_unified_diff(
        "diff --git a/src/c.ts b/src/c.ts\n--- a/src/c.ts\n+++ b/src/c.ts\n@@ -3,0 +3,1 @@\n+const added = 1;\n",
    );

    let demote = super::preexisting_dupe_group_keys([&untouched, &touched], &root, &diff);

    assert!(demote.contains(&dupe_group_key(&untouched, &root)));
    assert!(!demote.contains(&dupe_group_key(&touched, &root)));
    assert_eq!(demote.len(), 1);
}

#[test]
fn demote_introductions_moves_introduced_keys_to_inherited() {
    let base: FxHashSet<String> = std::iter::once("dupe:old".to_string()).collect();
    let mut ledger = AuditDomainLedger::compare(
        ["dupe:kept".to_string(), "dupe:demoted".to_string()],
        Some(&base),
    );
    assert_eq!(ledger.introduced_count(), 2);

    let demote: FxHashSet<String> = std::iter::once("dupe:demoted".to_string()).collect();
    ledger.demote_introductions(&demote);

    assert_eq!(ledger.introduced_count(), 1);
    assert_eq!(ledger.inherited_count(), 1);
    let introduced: Vec<bool> = ledger.introduced().collect();
    assert_eq!(introduced, vec![true, false]);
    assert_eq!(ledger.demoted_count(), 1);
    assert!(ledger.demoted_count() <= ledger.inherited_count());
    assert!(ledger.demoted_keys().contains("dupe:demoted"));
    let demoted: Vec<bool> = ledger.demoted().collect();
    assert_eq!(demoted, vec![false, true]);
}

#[test]
fn demote_introductions_ignores_keys_that_were_not_introduced() {
    let base: FxHashSet<String> = std::iter::once("dupe:old".to_string()).collect();
    let mut ledger = AuditDomainLedger::compare(
        ["dupe:old".to_string(), "dupe:new".to_string()],
        Some(&base),
    );

    let demote: FxHashSet<String> = std::iter::once("dupe:old".to_string()).collect();
    ledger.demote_introductions(&demote);

    assert_eq!(ledger.demoted_count(), 0);
    let demoted: Vec<bool> = ledger.demoted().collect();
    assert_eq!(demoted, vec![false, false]);
}

#[test]
fn annotate_domain_demotions_json_marks_only_demoted_entries() {
    let mut json_val = json!({
        "clone_groups": [{}, {}],
    });

    super::annotate_domain_demotions_json(
        &mut json_val,
        "clone_groups",
        [false, true],
        crate::CloneDemotionReason::NoAddedLines,
    );

    assert!(json_val["clone_groups"][0].get("demotion_reason").is_none());
    assert_eq!(
        json_val["clone_groups"][1]["demotion_reason"],
        "no-added-lines"
    );
}

#[test]
fn dupes_keys_produces_one_key_per_clone_group() {
    let root = root();
    let a = root.join("src/a.ts");
    let b = root.join("src/b.ts");
    let groups = vec![
        make_clone_group(&[a.clone(), b.clone()], "block one"),
        make_clone_group(&[a, b], "block two"),
    ];
    let report = make_duplication_report(groups);
    let keys = dupes_keys(&report, &root);
    assert_eq!(keys.len(), 2);
}

#[test]
fn annotate_dupes_json_marks_introduced_and_inherited_flags() {
    let root = root();
    let a = root.join("src/a.ts");
    let b = root.join("src/b.ts");
    let group_new = make_clone_group(&[a.clone(), b.clone()], "new block");
    let group_old = make_clone_group(&[a, b], "old block");
    let old_key = dupe_group_key(&group_old, &root);
    let base = FxHashSet::from_iter([old_key]);
    let report = make_duplication_report(vec![group_new, group_old]);
    let mut json_val = json!({
        "clone_groups": [{}, {}],
    });

    annotate_dupes_json(&mut json_val, &report, &root, &base);

    assert_eq!(json_val["clone_groups"][0]["introduced"], true);
    assert_eq!(json_val["clone_groups"][1]["introduced"], false);
}

#[test]
fn annotate_dupes_json_is_noop_when_clone_groups_key_absent() {
    let root = root();
    let report = make_duplication_report(Vec::new());
    let base = FxHashSet::default();
    let mut json_val = json!({"stats": {}});
    let original = json_val.clone();

    annotate_dupes_json(&mut json_val, &report, &root, &base);

    assert_eq!(json_val, original);
}

// --- retain_introduced_dead_code with None base (no-op path, line 1127) ---

#[test]
fn retain_introduced_dead_code_is_noop_when_base_is_none() {
    let root = root();
    let mut results = sample_results(&root);
    let original_file_count = results.unused_files.len();
    let original_export_count = results.unused_exports.len();

    retain_introduced_dead_code(&mut results, &root, None);

    // Nothing should be filtered when base is None.
    assert_eq!(results.unused_files.len(), original_file_count);
    assert_eq!(results.unused_exports.len(), original_export_count);
}

// --- retain_introduced_dead_code covers framework / component / graph findings ---

#[test]
fn retain_introduced_dead_code_filters_framework_findings() {
    let root = root();
    let src = root.join("src/App.vue");
    let mut results = AnalysisResults::default();
    results
        .unprovided_injects
        .push(UnprovidedInjectFinding::with_actions(UnprovidedInject {
            path: src.clone(),
            key_name: "existing".to_string(),
            framework: "vue".to_string(),
            line: 1,
            col: 0,
        }));
    results
        .unprovided_injects
        .push(UnprovidedInjectFinding::with_actions(UnprovidedInject {
            path: src.clone(),
            key_name: "new".to_string(),
            framework: "vue".to_string(),
            line: 2,
            col: 0,
        }));
    results
        .unrendered_components
        .push(UnrenderedComponentFinding::with_actions(
            UnrenderedComponent {
                path: src,
                component_name: "OldWidget".to_string(),
                framework: "vue".to_string(),
                reachable_via: None,
                line: 1,
                col: 0,
            },
        ));

    let base = FxHashSet::from_iter([
        "unprovided-inject:src/App.vue:existing".to_string(),
        "unrendered-component:src/App.vue:OldWidget".to_string(),
    ]);

    retain_introduced_dead_code(&mut results, &root, Some(&base));

    // Only "new" inject survives; OldWidget is filtered.
    assert_eq!(results.unprovided_injects.len(), 1);
    assert_eq!(results.unprovided_injects[0].inject.key_name, "new");
    assert!(results.unrendered_components.is_empty());
}

#[test]
fn retain_introduced_dead_code_filters_graph_findings() {
    let root = root();
    let a = root.join("src/a.ts");
    let b = root.join("src/b.ts");
    let mut results = AnalysisResults::default();
    results
        .circular_dependencies
        .push(CircularDependencyFinding::with_actions(
            CircularDependency {
                files: vec![a.clone(), b],
                length: 2,
                line: 1,
                col: 0,
                edges: Vec::new(),
                is_cross_package: false,
            },
        ));
    results
        .re_export_cycles
        .push(ReExportCycleFinding::with_actions(ReExportCycle {
            files: vec![a],
            kind: ReExportCycleKind::SelfLoop,
        }));

    // The circular dep is in the base; the re-export cycle is new.
    let base = FxHashSet::from_iter(["circular-dependency:src/a.ts|src/b.ts".to_string()]);

    retain_introduced_dead_code(&mut results, &root, Some(&base));

    assert!(results.circular_dependencies.is_empty());
    assert_eq!(results.re_export_cycles.len(), 1);
}

#[test]
fn audit_ledger_routes_override_severity_and_persists_introduced_flags() {
    let root = root();
    let config: FallowConfig = serde_json::from_value(json!({
        "rules": { "unused-exports": "warn" },
        "overrides": [{
            "files": ["src/generated/**"],
            "rules": { "unused-exports": "error" }
        }]
    }))
    .expect("config");
    let config = config.resolve(root.clone(), OutputFormat::Json, 1, false, true, None);
    let mut results = AnalysisResults::default();
    results
        .unused_exports
        .push(export(&root.join("src/base.ts"), "baseExport"));
    results
        .unused_exports
        .push(export(&root.join("src/generated/new.ts"), "newExport"));
    let base = FxHashSet::from_iter(["unused-export:src/base.ts:baseExport".to_string()]);

    let ledger = dead_code_audit_ledger(&results, &root, &config, Some(&base));

    assert_eq!(ledger.classification_count(), 2);
    assert_eq!(ledger.introduced_count(), 1);
    assert_eq!(ledger.inherited_count(), 1);
    assert!(ledger.has_introduced_errors());
    assert!(!ledger.has_introduced_warnings());
    assert_eq!(ledger.records()[0].effective_severity, Severity::Warn);
    assert_eq!(ledger.records()[1].effective_severity, Severity::Error);

    ledger.annotate_results(&mut results);
    assert_eq!(ledger.classification_count(), 2);
    assert_eq!(
        results.unused_exports[0].introduced,
        Some(AuditIntroduced(false))
    );
    assert_eq!(
        results.unused_exports[1].introduced,
        Some(AuditIntroduced(true))
    );
}

#[test]
fn audit_ledger_demotes_semantic_only_introductions_but_keeps_syntactic_new_findings() {
    let root = root();
    let config: FallowConfig = serde_json::from_value(json!({
        "rules": { "unused-exports": "error" }
    }))
    .expect("config");
    let config = config.resolve(root.clone(), OutputFormat::Json, 1, false, true, None);
    let mut results = AnalysisResults::default();
    // A pre-existing finding, a genuinely new syntactic finding, and a
    // semantic-only finding with no syntactic counterpart on head.
    results
        .unused_exports
        .push(export(&root.join("src/base.ts"), "baseExport"));
    results
        .unused_exports
        .push(export(&root.join("src/new.ts"), "newExport"));
    results
        .unused_exports
        .push(export(&root.join("src/semantic.ts"), "semanticOnly"));
    let base = FxHashSet::from_iter(["unused-export:src/base.ts:baseExport".to_string()]);
    let head_syntactic = FxHashSet::from_iter([
        "unused-export:src/base.ts:baseExport".to_string(),
        "unused-export:src/new.ts:newExport".to_string(),
    ]);

    let mut ledger = dead_code_audit_ledger(&results, &root, &config, Some(&base));
    assert_eq!(ledger.introduced_count(), 2);

    ledger.demote_unattributable_introductions(&head_syntactic);

    // Control: the genuinely new syntactic finding still gates.
    assert_eq!(ledger.introduced_count(), 1);
    assert!(ledger.has_introduced_errors());
    assert_eq!(ledger.inherited_count(), 2);
    ledger.annotate_results(&mut results);
    assert_eq!(
        results.unused_exports[0].introduced,
        Some(AuditIntroduced(false))
    );
    assert_eq!(
        results.unused_exports[1].introduced,
        Some(AuditIntroduced(true))
    );
    assert_eq!(
        results.unused_exports[2].introduced,
        Some(AuditIntroduced(false))
    );
}

#[test]
fn audit_ledger_counts_each_occurrence_of_a_repeated_key() {
    let root = root();
    let config: FallowConfig = serde_json::from_value(json!({
        "rules": { "unused-exports": "error" }
    }))
    .expect("config");
    let config = config.resolve(root.clone(), OutputFormat::Json, 1, false, true, None);
    let mut results = AnalysisResults::default();
    results
        .unused_exports
        .push(export(&root.join("src/collision.ts"), "sameExport"));
    results
        .unused_exports
        .push(export(&root.join("src/collision.ts"), "sameExport"));

    let ledger = dead_code_audit_ledger(&results, &root, &config, Some(&FxHashSet::default()));

    assert_eq!(ledger.classification_count(), 2);
    assert_eq!(ledger.records().len(), 2);
    assert_eq!(ledger.introduced_count(), 2);
    assert_eq!(ledger.inherited_count(), 0);

    ledger.annotate_results(&mut results);
    assert!(
        results
            .unused_exports
            .iter()
            .all(|finding| { finding.introduced == Some(AuditIntroduced(true)) })
    );
}

#[test]
fn audit_domain_ledger_counts_colliding_keys_once_but_preserves_record_membership() {
    let introduced = AuditDomainLedger::compare(
        ["same-key".to_string(), "same-key".to_string()],
        Some(&FxHashSet::default()),
    );
    assert_eq!(introduced.introduced_count(), 1);
    assert_eq!(introduced.inherited_count(), 0);
    assert_eq!(introduced.introduced().collect::<Vec<_>>(), [true, true]);

    let base = FxHashSet::from_iter(["same-key".to_string()]);
    let inherited = AuditDomainLedger::compare(
        ["same-key".to_string(), "same-key".to_string()],
        Some(&base),
    );
    assert_eq!(inherited.introduced_count(), 0);
    assert_eq!(inherited.inherited_count(), 1);
    assert_eq!(inherited.introduced().collect::<Vec<_>>(), [false, false]);
}

#[test]
fn stale_suppression_fallback_preserves_introduced_annotation() {
    let root = root();
    let mut results = AnalysisResults::default();
    results.stale_suppressions.push(StaleSuppression {
        finding_id: None,
        path: root.join("src/new.ts"),
        line: 2,
        col: 0,
        origin: SuppressionOrigin::Comment {
            issue_kind: Some("unused-export".to_string()),
            reason: None,
            is_file_level: false,
            kind_known: true,
        },
        missing_reason: false,
        actions: StaleSuppression::actions_for(false),
        effective_severity: None,
    });
    let mut json = serde_json::to_value(&results).expect("json");

    annotate_stale_suppressions_json(&mut json, &results, &root, &FxHashSet::default());

    assert_eq!(json["stale_suppressions"][0]["introduced"], true);
}

#[test]
fn remap_keys_for_renames_relocates_path_segments() {
    use rustc_hash::FxHashMap;

    let mut renames = FxHashMap::default();
    renames.insert(
        "src/old/impl.ts".to_string(),
        "src/renamed/impl.ts".to_string(),
    );

    let keys: FxHashSet<String> = [
        "complexity:src/old/impl.ts:complexFn:[Cyclomatic]".to_string(),
        "unused-export:src/old/impl.ts:unusedHelper".to_string(),
        "unused-file:src/old/impl.ts".to_string(),
        "unused-export:src/other.ts:src/old/impl.ts-unrelated".to_string(),
    ]
    .into_iter()
    .collect();

    let remapped = remap_keys_for_renames(&keys, &renames);

    assert!(remapped.contains("complexity:src/renamed/impl.ts:complexFn:[Cyclomatic]"));
    assert!(remapped.contains("unused-export:src/renamed/impl.ts:unusedHelper"));
    assert!(remapped.contains("unused-file:src/renamed/impl.ts"));
    assert!(
        remapped.contains("unused-export:src/other.ts:src/old/impl.ts-unrelated"),
        "non-exact segment matches must pass through untouched"
    );
}

#[test]
fn remap_keys_for_renames_recanonicalizes_sorted_path_lists() {
    use rustc_hash::FxHashMap;

    let mut renames = FxHashMap::default();
    renames.insert("src/a.ts".to_string(), "src/z.ts".to_string());

    let keys: FxHashSet<String> = [
        // The base list was sorted as [src/a.ts, src/m.ts]; after the
        // rename the head list sorts as [src/m.ts, src/z.ts].
        "dupe:src/a.ts|src/m.ts:120:12:deadbeef".to_string(),
        "circular-dependency:src/a.ts|src/m.ts".to_string(),
    ]
    .into_iter()
    .collect();

    let remapped = remap_keys_for_renames(&keys, &renames);

    assert!(
        remapped.contains("dupe:src/m.ts|src/z.ts:120:12:deadbeef"),
        "duplicate-group file lists must be re-sorted: {remapped:#?}"
    );
    assert!(
        remapped.contains("circular-dependency:src/m.ts|src/z.ts"),
        "cycle file lists must be re-sorted: {remapped:#?}"
    );
}

#[test]
fn remap_keys_for_renames_without_matches_is_identity() {
    use rustc_hash::FxHashMap;

    let mut renames = FxHashMap::default();
    renames.insert("src/elsewhere.ts".to_string(), "src/moved.ts".to_string());

    let keys: FxHashSet<String> =
        std::iter::once("unused-export:src/utils.ts:helper".to_string()).collect();

    assert_eq!(remap_keys_for_renames(&keys, &renames), keys);
}

/// One finding in each collection that no other fixture fills.
fn production_and_directive_results(root: &Path) -> AnalysisResults {
    let page = root.join("src/app.ts");
    let mut results = AnalysisResults::default();
    results
        .dev_dependencies_in_production
        .push(DevDependencyInProductionFinding::with_actions(
            DevDependencyInProduction {
                package_name: "vite".to_string(),
                path: root.join("package.json"),
                line: 14,
            },
        ));
    results
        .invalid_client_exports
        .push(InvalidClientExportFinding::with_actions(
            InvalidClientExport {
                path: page.clone(),
                export_name: "config".to_string(),
                directive: "use client".to_string(),
                line: 3,
                col: 0,
            },
        ));
    results
        .mixed_client_server_barrels
        .push(MixedClientServerBarrelFinding::with_actions(
            MixedClientServerBarrel {
                path: page.clone(),
                client_origin: "./client".to_string(),
                server_origin: "./server".to_string(),
                line: 1,
                col: 0,
            },
        ));
    results
        .misplaced_directives
        .push(MisplacedDirectiveFinding::with_actions(
            MisplacedDirective {
                path: page,
                directive: "use server".to_string(),
                line: 5,
                col: 2,
            },
        ));
    results
}

/// Every finding of the fixtures, each as its own result set, so a
/// disagreement names one collection.
fn isolated_findings(fixtures: &[AnalysisResults]) -> Vec<(String, AnalysisResults)> {
    let mut isolated = Vec::new();
    for fixture in fixtures {
        let value = serde_json::to_value(fixture).expect("results serialize");
        let object = value.as_object().expect("results serialize as an object");
        for (collection, items) in object {
            let Some(items) = items.as_array() else {
                continue;
            };
            for item in items {
                let mut single = serde_json::to_value(AnalysisResults::default())
                    .expect("empty results serialize");
                single[collection.as_str()] = serde_json::Value::Array(vec![item.clone()]);
                let results = serde_json::from_value(single)
                    .unwrap_or_else(|error| panic!("{collection} round-trips: {error}"));
                isolated.push((collection.clone(), results));
            }
        }
    }
    isolated
}

fn duplicate_export_alias_findings(
    root: &Path,
) -> Vec<fallow_types::output_dead_code::DuplicateExportAliasFinding> {
    vec![
        fallow_types::output_dead_code::DuplicateExportAliasFinding::with_actions(
            fallow_types::results::DuplicateExportAlias {
                path: root.join("src/worker.ts"),
                local_name: "layer".to_owned(),
                export_names: vec!["layer".to_owned(), "default".to_owned()],
                line: 1,
                col: 13,
                locations: vec![],
            },
        ),
    ]
}

fn absent_component_prop_results(root: &Path) -> AnalysisResults {
    let mut results = AnalysisResults::default();
    results
        .absent_component_props
        .push(AbsentComponentPropFinding::with_actions(
            AbsentComponentProp {
                path: root.join("src/App.vue"),
                component_name: "MyModal".into(),
                framework: "vue".into(),
                prop_name: "highlight".into(),
                line: 3,
                col: 2,
                has_default: true,
                inspected_call_sites: vec![ComponentPropCallSite {
                    path: root.join("src/page.ts"),
                    line: 8,
                    col: 0,
                }],
                explanation: "Review known callers and API intent.".into(),
            },
        ));
    results
}

/// A config that sets every rule by `base` and, for the files of the
/// fixtures that an `overrides` entry matches, by `scoped`. Each pair
/// gives the severity of the rules at an even and at an odd position, so
/// a finding that reads the rule of another kind shows as a difference.
fn every_rule_config(base: [&str; 2], scoped: Option<[&str; 2]>) -> fallow_config::ResolvedConfig {
    let every_rule = |severities: [&str; 2]| {
        let rules =
            serde_json::to_value(fallow_config::RulesConfig::default()).expect("rules serialize");
        let names = rules
            .as_object()
            .expect("rules serialize as an object")
            .keys();
        serde_json::Value::Object(
            names
                .enumerate()
                .map(|(index, name)| (name.clone(), json!(severities[index % 2])))
                .collect(),
        )
    };
    let mut config = json!({ "rules": every_rule(base) });
    if let Some(scoped) = scoped {
        config["overrides"] = json!([{
            "files": [
                "package.json",
                "pnpm-workspace.yaml",
                "src/app.ts",
                "src/page.ts",
                "src/types.ts",
                "src/App.vue",
                "src/actions/**",
                "app/**",
                "src/app/**",
                "src/store.ts"
            ],
            "rules": every_rule(scoped)
        }]);
    }
    let config: FallowConfig = serde_json::from_value(config).expect("config");
    config.resolve(root(), OutputFormat::Json, 1, false, true, None)
}

/// The audit ledger and the exit-code rule resolve severity in two
/// places. With every finding introduced, the ledger must report an
/// error exactly when `has_error_severity_issues` does, for every
/// collection, with and without `overrides`.
#[test]
fn audit_ledger_errors_match_the_exit_code_rule_for_every_collection() {
    let root = root();
    // Two arrays keep each stack array under the large-array lint limit.
    let fixtures: Vec<AnalysisResults> = [
        sample_results(&root),
        graph_boundary_catalog_override_results(&root),
        type_member_and_dependency_results(&root),
        framework_inject_and_render_results(&root),
        absent_component_prop_results(&root),
    ]
    .into_iter()
    .chain([
        server_action_load_data_and_route_results(&root),
        angular_input_output_and_policy_results(&root),
        unused_store_member_results(&root),
        production_and_directive_results(&root),
        AnalysisResults {
            prop_drilling_chains: vec![PropDrillingChainFinding::with_actions(PropDrillingChain {
                prop: "user".to_owned(),
                depth: 3,
                hops: vec![PropDrillHop {
                    file: root.join("src/page.ts"),
                    line: 4,
                    component: "Page".to_owned(),
                }],
            })],
            thin_wrappers: vec![ThinWrapperFinding::with_actions(ThinWrapper {
                file: root.join("src/page.ts"),
                line: 4,
                component: "Wrapper".to_owned(),
                child_component: "Child".to_owned(),
            })],
            duplicate_prop_shapes: vec![DuplicatePropShapeFinding::with_actions(
                DuplicatePropShape {
                    file: root.join("src/page.ts"),
                    line: 5,
                    component: "Field".to_owned(),
                    shape: vec![
                        "value".to_owned(),
                        "error".to_owned(),
                        "helpText".to_owned(),
                        "label".to_owned(),
                    ],
                    group_size: 3,
                    sharing_components: vec![],
                },
            )],
            duplicate_export_aliases: duplicate_export_alias_findings(&root),
            ..effect_results(&root)
        },
    ])
    .collect();
    let findings = isolated_findings(&fixtures);
    let empty_base = FxHashSet::default();
    let mut covered = std::collections::BTreeSet::new();
    let mut mismatches = Vec::new();
    for (base, scoped) in [
        (["error", "error"], None),
        (["warn", "warn"], None),
        (["error", "warn"], None),
        (["error", "error"], Some(["warn", "warn"])),
        (["warn", "warn"], Some(["error", "error"])),
        (["off", "off"], Some(["error", "error"])),
        (["error", "warn"], Some(["warn", "error"])),
        (["warn", "error"], Some(["error", "warn"])),
    ] {
        let config = every_rule_config(base, scoped);
        for (collection, results) in &findings {
            let ledger = dead_code_audit_ledger(results, &root, &config, Some(&empty_base));
            covered.extend(ledger.records().iter().map(|record| record.collection));
            let exit_rule = fallow_engine::error_severity::has_error_severity_issues(
                results,
                &config.rules,
                Some(&config),
                false,
            );
            if ledger.has_introduced_errors() != exit_rule {
                mismatches.push(format!(
                        "{collection} (rules {base:?}, overrides {scoped:?}): ledger {}, exit rule {exit_rule}",
                        ledger.has_introduced_errors()
                    ));
            }
        }
    }
    assert!(mismatches.is_empty(), "{}", mismatches.join("\n"));
    let missing: Vec<&str> = super::AuditCollection::ALL
        .iter()
        .map(|collection| collection.json_key())
        .filter(|key| !covered.contains(key))
        .collect();
    assert!(
        missing.is_empty(),
        "the fixtures hold no finding in {missing:?}"
    );
}

// Line-free, count-aware audit keys.

fn audit_suppression(root: &Path, line: u32, reason: Option<&str>) -> StaleSuppression {
    StaleSuppression {
        finding_id: None,
        path: root.join("src/flags.ts"),
        line,
        col: 0,
        origin: SuppressionOrigin::Comment {
            issue_kind: Some("unused-export".to_string()),
            reason: reason.map(str::to_owned),
            is_file_level: false,
            kind_known: true,
        },
        missing_reason: false,
        actions: StaleSuppression::actions_for(false),
        effective_severity: None,
    }
}

/// Add `by` to the line of every finding whose old audit key held a line.
fn shift_lines(results: &mut AnalysisResults, by: u32) {
    for item in &mut results.unlisted_dependencies {
        for site in &mut item.dep.imported_from {
            site.line += by;
        }
    }
    for item in &mut results.stale_suppressions {
        item.line += by;
    }
    for item in &mut results.unused_catalog_entries {
        item.entry.line += by;
    }
    for item in &mut results.empty_catalog_groups {
        item.group.line += by;
    }
    for item in &mut results.unresolved_catalog_references {
        item.reference.line += by;
    }
    for item in &mut results.unused_dependency_overrides {
        item.entry.line += by;
    }
    for item in &mut results.misconfigured_dependency_overrides {
        item.entry.line += by;
    }
    for item in &mut results.misplaced_directives {
        item.directive_site.line += by;
    }
    for item in &mut results.unused_exports {
        item.export.line += by;
    }
}

fn line_sensitive_results(root: &Path) -> AnalysisResults {
    let mut results = graph_boundary_catalog_override_results(root);
    let sample = sample_results(root);
    results.unlisted_dependencies = sample.unlisted_dependencies;
    results.unused_exports = sample.unused_exports;
    results
        .misplaced_directives
        .push(MisplacedDirectiveFinding::with_actions(
            MisplacedDirective {
                path: root.join("src/action.ts"),
                directive: "use server".to_string(),
                line: 4,
                col: 0,
            },
        ));
    results
}

#[test]
fn dead_code_keys_do_not_change_when_lines_shift() {
    let root = root();
    let before = line_sensitive_results(&root);
    let mut after = before.clone();
    shift_lines(&mut after, 20);

    let before_keys = dead_code_keys(&before, &root);
    let after_keys = dead_code_keys(&after, &root);

    let mut changed: Vec<&String> = after_keys.difference(&before_keys).collect();
    changed.sort();
    assert!(changed.is_empty(), "keys that hold a line: {changed:?}");
}

#[test]
fn stale_suppression_keys_ignore_the_reason_text() {
    let root = root();
    let with_reason = |reason: &str| AnalysisResults {
        stale_suppressions: vec![audit_suppression(&root, 3, Some(reason))],
        ..AnalysisResults::default()
    };

    assert_eq!(
        dead_code_keys(&with_reason("kept for the plugin API"), &root),
        dead_code_keys(&with_reason("removed in v3"), &root)
    );
}

#[test]
fn unlisted_dependency_keys_ignore_the_import_sites() {
    let root = root();
    let base = sample_results(&root);
    let mut head = base.clone();
    let item = &mut head.unlisted_dependencies[0].dep;
    item.imported_from[0].line = 40;
    item.imported_from.push(ImportSite {
        path: root.join("src/other.ts"),
        line: 1,
        col: 0,
    });

    assert_eq!(dead_code_keys(&base, &root), dead_code_keys(&head, &root));
}

#[test]
fn a_second_occurrence_of_an_inherited_key_is_introduced() {
    let root = root();
    let config: FallowConfig = serde_json::from_value(json!({
        "rules": { "stale-suppressions": "error" }
    }))
    .expect("config");
    let config = config.resolve(root.clone(), OutputFormat::Json, 1, false, true, None);
    let base_results = AnalysisResults {
        stale_suppressions: vec![audit_suppression(&root, 3, None)],
        ..AnalysisResults::default()
    };
    let base = dead_code_keys(&base_results, &root);
    let mut head = AnalysisResults {
        stale_suppressions: vec![
            audit_suppression(&root, 13, None),
            audit_suppression(&root, 17, None),
        ],
        ..AnalysisResults::default()
    };

    let ledger = dead_code_audit_ledger(&head, &root, &config, Some(&base));
    assert_eq!(ledger.introduced_count(), 1);
    assert_eq!(ledger.inherited_count(), 1);
    assert!(!ledger.records()[0].introduced);
    assert!(ledger.records()[1].introduced);

    retain_introduced_dead_code(&mut head, &root, Some(&base));
    assert_eq!(head.stale_suppressions.len(), 1);
    assert_eq!(head.stale_suppressions[0].line, 17);
}

#[test]
fn a_renamed_file_keeps_its_findings_and_occurrences_inherited() {
    let root = root();
    let finding = |path: &str, line: u32| {
        let mut suppression = audit_suppression(&root, line, None);
        suppression.path = root.join(path);
        suppression
    };
    let base_results = AnalysisResults {
        stale_suppressions: vec![finding("src/old.ts", 3), finding("src/old.ts", 9)],
        ..AnalysisResults::default()
    };
    let head = AnalysisResults {
        stale_suppressions: vec![finding("src/new.ts", 4), finding("src/new.ts", 10)],
        ..AnalysisResults::default()
    };
    let renames =
        rustc_hash::FxHashMap::from_iter([("src/old.ts".to_string(), "src/new.ts".to_string())]);

    let base = remap_keys_for_renames(&dead_code_keys(&base_results, &root), &renames);

    assert_eq!(base, dead_code_keys(&head, &root));
}
