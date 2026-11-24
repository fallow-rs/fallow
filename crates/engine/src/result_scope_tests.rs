use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use fallow_output::DiffIndex;
use fallow_types::extract::{
    MemberKind, SkippedSecurityCalleeExpressionKind, SkippedSecurityCalleeReason,
};
use fallow_types::output_dead_code::*;
use fallow_types::results::*;
use rustc_hash::FxHashSet;

const REAL_LINE: u32 = 5;
const SYNTHETIC_LINE: u32 = 1;

fn inside() -> PathBuf {
    PathBuf::from("/repo/packages/a")
}

fn outside() -> PathBuf {
    PathBuf::from("/repo/packages/b")
}

fn source(root: &Path) -> PathBuf {
    root.join("src/x.ts")
}

fn member(path: &Path, kind: MemberKind) -> UnusedMember {
    UnusedMember {
        path: path.to_path_buf(),
        parent_name: "Parent".to_owned(),
        member_name: "member".to_owned(),
        kind,
        line: REAL_LINE,
        col: 0,
    }
}

fn export(path: &Path) -> UnusedExport {
    UnusedExport {
        path: path.to_path_buf(),
        export_name: "value".to_owned(),
        is_type_only: false,
        line: REAL_LINE,
        col: 0,
        span_start: 0,
        is_re_export: false,
        deprecated: false,
        deprecated_reason: None,
    }
}

fn push_source_findings(results: &mut AnalysisResults, path: &Path) {
    results
        .unused_files
        .push(UnusedFileFinding::with_actions(UnusedFile {
            path: path.to_path_buf(),
        }));
    results
        .unused_exports
        .push(UnusedExportFinding::with_actions(export(path)));
    results
        .unused_types
        .push(UnusedTypeFinding::with_actions(export(path)));
    results
        .private_type_leaks
        .push(PrivateTypeLeakFinding::with_actions(PrivateTypeLeak {
            path: path.to_path_buf(),
            export_name: "value".to_owned(),
            type_name: "Hidden".to_owned(),
            line: REAL_LINE,
            col: 0,
            span_start: 0,
            semantic: None,
        }));
    results
        .deprecated_exports_in_use
        .push(DeprecatedExportInUseFinding::with_actions(
            DeprecatedExportInUse {
                path: path.to_path_buf(),
                export_name: "old".to_owned(),
                is_type_only: false,
                line: REAL_LINE,
                col: 0,
                span_start: 0,
                deprecated_reason: None,
                consumer_count: 0,
                consumers: Vec::new(),
                public_api: false,
            },
        ));
    results
        .unused_enum_members
        .push(UnusedEnumMemberFinding::with_actions(member(
            path,
            MemberKind::EnumMember,
        )));
    results
        .unused_class_members
        .push(UnusedClassMemberFinding::with_actions(member(
            path,
            MemberKind::ClassMethod,
        )));
    results
        .unused_namespace_members
        .push(UnusedNamespaceMemberFinding::with_actions(member(
            path,
            MemberKind::NamespaceMember,
        )));
    results
        .unused_protected_class_members
        .push(UnusedProtectedClassMemberFinding::with_actions(member(
            path,
            MemberKind::ClassProperty,
        )));
    results
        .unused_store_members
        .push(UnusedStoreMemberFinding::with_actions(member(
            path,
            MemberKind::StoreMember,
        )));
    results
        .unresolved_imports
        .push(UnresolvedImportFinding::with_actions(UnresolvedImport {
            path: path.to_path_buf(),
            specifier: "./missing".to_owned(),
            line: REAL_LINE,
            col: 0,
            specifier_col: 0,
        }));
}

fn push_component_contract_findings(results: &mut AnalysisResults, path: &Path) {
    let path = path.to_path_buf();
    results
        .unprovided_injects
        .push(UnprovidedInjectFinding::with_actions(UnprovidedInject {
            path: path.clone(),
            key_name: "key".to_owned(),
            framework: "vue".to_owned(),
            line: REAL_LINE,
            col: 0,
        }));
    results
        .unrendered_components
        .push(UnrenderedComponentFinding::with_actions(
            UnrenderedComponent {
                path: path.clone(),
                component_name: "Card".to_owned(),
                framework: "vue".to_owned(),
                reachable_via: None,
                line: REAL_LINE,
                col: 0,
            },
        ));
    results
        .unused_component_props
        .push(UnusedComponentPropFinding::with_actions(
            UnusedComponentProp {
                path: path.clone(),
                component_name: "Card".to_owned(),
                prop_name: "title".to_owned(),
                line: REAL_LINE,
                col: 0,
            },
        ));
    results
        .absent_component_props
        .push(AbsentComponentPropFinding::with_actions(
            AbsentComponentProp {
                path: path.clone(),
                component_name: "Card".to_owned(),
                framework: "react".to_owned(),
                prop_name: "title".to_owned(),
                line: REAL_LINE,
                col: 0,
                has_default: false,
                inspected_call_sites: Vec::new(),
                explanation: String::new(),
            },
        ));
    results
        .constant_parameters
        .push(ConstantParameterFinding::with_actions(ConstantParameter {
            path,
            function_name: "run".to_owned(),
            parameter_name: "flag".to_owned(),
            parameter_index: 0,
            option_key: None,
            line: REAL_LINE,
            col: 0,
            reason: ConstantParameterReason::NeverPassed,
            evidence: None,
            value: None,
            optional: true,
            has_default: false,
            condition_reads: 0,
            call_site_count: 0,
            inspected_call_sites: Vec::new(),
            explanation: String::new(),
        }));
}

fn push_component_io_findings(results: &mut AnalysisResults, path: &Path) {
    let path = path.to_path_buf();
    results
        .unused_component_emits
        .push(UnusedComponentEmitFinding::with_actions(
            UnusedComponentEmit {
                path: path.clone(),
                component_name: "Card".to_owned(),
                emit_name: "close".to_owned(),
                line: REAL_LINE,
                col: 0,
            },
        ));
    results
        .unused_component_inputs
        .push(UnusedComponentInputFinding::with_actions(
            UnusedComponentInput {
                path: path.clone(),
                component_name: "Card".to_owned(),
                input_name: "title".to_owned(),
                line: REAL_LINE,
                col: 0,
            },
        ));
    results
        .unused_component_outputs
        .push(UnusedComponentOutputFinding::with_actions(
            UnusedComponentOutput {
                path: path.clone(),
                component_name: "Card".to_owned(),
                output_name: "closed".to_owned(),
                line: REAL_LINE,
                col: 0,
            },
        ));
    results
        .unused_svelte_events
        .push(UnusedSvelteEventFinding::with_actions(UnusedSvelteEvent {
            path,
            component_name: "Card".to_owned(),
            event_name: "close".to_owned(),
            line: REAL_LINE,
            col: 0,
        }));
}

fn push_effect_and_server_findings(results: &mut AnalysisResults, path: &Path) {
    let path = path.to_path_buf();
    results
        .unused_effect_layers
        .push(UnusedEffectLayerFinding::with_actions(UnusedEffectLayer {
            path: path.clone(),
            name: "Live".to_owned(),
            kind: EffectLayerKind::TopLevel,
            line: REAL_LINE,
            col: 0,
            dead_consumers: Vec::new(),
        }));
    results
        .unused_tagged_errors
        .push(UnusedTaggedErrorFinding::with_actions(UnusedTaggedError {
            path: path.clone(),
            name: "Failure".to_owned(),
            tag: "Failure".to_owned(),
            line: REAL_LINE,
            col: 0,
            catch_sites: Vec::new(),
        }));
    results
        .unproduced_catch_tags
        .push(UnproducedCatchTagFinding::with_actions(
            UnproducedCatchTag {
                path: path.clone(),
                tag: "Failure".to_owned(),
                line: REAL_LINE,
                col: 0,
                candidate_classes: Vec::new(),
            },
        ));
    results
        .unused_server_actions
        .push(UnusedServerActionFinding::with_actions(
            UnusedServerAction {
                path: path.clone(),
                action_name: "save".to_owned(),
                line: REAL_LINE,
                col: 0,
            },
        ));
    results
        .unused_load_data_keys
        .push(UnusedLoadDataKeyFinding::with_actions(UnusedLoadDataKey {
            path: path.clone(),
            key_name: "user".to_owned(),
            line: REAL_LINE,
            col: 0,
            route_dir: None,
        }));
    results
        .invalid_client_exports
        .push(InvalidClientExportFinding::with_actions(
            InvalidClientExport {
                path: path.clone(),
                export_name: "value".to_owned(),
                directive: "use client".to_owned(),
                line: REAL_LINE,
                col: 0,
            },
        ));
    results
        .mixed_client_server_barrels
        .push(MixedClientServerBarrelFinding::with_actions(
            MixedClientServerBarrel {
                path: path.clone(),
                client_origin: "./client".to_owned(),
                server_origin: "./server".to_owned(),
                line: REAL_LINE,
                col: 0,
            },
        ));
    results
        .misplaced_directives
        .push(MisplacedDirectiveFinding::with_actions(
            MisplacedDirective {
                path,
                directive: "use server".to_owned(),
                line: REAL_LINE,
                col: 0,
            },
        ));
}

fn push_route_and_policy_findings(results: &mut AnalysisResults, path: &Path) {
    let path = path.to_path_buf();
    results
        .route_collisions
        .push(RouteCollisionFinding::with_actions(RouteCollision {
            path: path.clone(),
            url: "/x".to_owned(),
            conflicting_paths: Vec::new(),
            line: SYNTHETIC_LINE,
            col: 0,
        }));
    results
        .dynamic_segment_name_conflicts
        .push(DynamicSegmentNameConflictFinding::with_actions(
            DynamicSegmentNameConflict {
                path: path.clone(),
                position: "/[id]".to_owned(),
                conflicting_segments: vec!["id".to_owned(), "slug".to_owned()],
                conflicting_paths: Vec::new(),
                line: SYNTHETIC_LINE,
                col: 0,
            },
        ));
    results
        .boundary_violations
        .push(BoundaryViolationFinding::with_actions(BoundaryViolation {
            from_path: path.clone(),
            to_path: PathBuf::from("/repo/target.ts"),
            from_zone: "ui".to_owned(),
            to_zone: "data".to_owned(),
            import_specifier: "../target".to_owned(),
            line: REAL_LINE,
            col: 0,
            via_path: None,
            reason: BoundaryViolationReason::DisallowedZone,
            public_files: Vec::new(),
        }));
    results
        .boundary_coverage_violations
        .push(BoundaryCoverageViolationFinding::with_actions(
            BoundaryCoverageViolation {
                path: path.clone(),
                line: SYNTHETIC_LINE,
                col: 0,
            },
        ));
    results
        .boundary_call_violations
        .push(BoundaryCallViolationFinding::with_actions(
            BoundaryCallViolation {
                path: path.clone(),
                line: REAL_LINE,
                col: 0,
                zone: "ui".to_owned(),
                callee: "cp.exec".to_owned(),
                pattern: "child_process.*".to_owned(),
            },
        ));
    results
        .policy_violations
        .push(PolicyViolationFinding::with_actions(PolicyViolation {
            path: path.clone(),
            line: REAL_LINE,
            col: 0,
            pack: "security".to_owned(),
            rule_id: "no-eval".to_owned(),
            kind: PolicyRuleKind::BannedCall,
            matched: "eval".to_owned(),
            severity: PolicyViolationSeverity::Error,
            message: None,
        }));
    results.stale_suppressions.push(StaleSuppression {
        path: path.clone(),
        line: REAL_LINE,
        col: 0,
        origin: SuppressionOrigin::JsdocTag {
            export_name: "value".to_owned(),
            reason: None,
        },
        missing_reason: false,
        finding_id: None,
        actions: Vec::new(),
        effective_severity: None,
    });
    results
        .security_unresolved_callee_diagnostics
        .push(SecurityUnresolvedCalleeDiagnostic {
            path,
            line: REAL_LINE,
            col: 0,
            reason: SkippedSecurityCalleeReason::DynamicDispatch,
            expression_kind: SkippedSecurityCalleeExpressionKind::Identifier,
        });
}

fn push_graph_findings(results: &mut AnalysisResults, path: &Path) {
    results
        .duplicate_export_aliases
        .push(DuplicateExportAliasFinding::with_actions(
            DuplicateExportAlias {
                path: path.to_path_buf(),
                local_name: "value".to_owned(),
                export_names: vec!["a".to_owned(), "b".to_owned()],
                line: REAL_LINE,
                col: 0,
                locations: vec![DuplicateExportAliasLocation {
                    export_name: "a".to_owned(),
                    line: REAL_LINE,
                    col: 0,
                }],
            },
        ));
    results
        .circular_dependencies
        .push(CircularDependencyFinding::with_actions(
            CircularDependency {
                files: vec![path.to_path_buf()],
                length: 1,
                line: REAL_LINE,
                col: 0,
                edges: Vec::new(),
                is_cross_package: false,
            },
        ));
    results
        .re_export_cycles
        .push(ReExportCycleFinding::with_actions(ReExportCycle {
            files: vec![path.to_path_buf()],
            kind: ReExportCycleKind::SelfLoop,
            line: 0,
            col: 0,
        }));
    results
        .package_cycles
        .push(PackageCycleFinding::with_actions(PackageCycle {
            packages: vec!["a".to_owned(), "b".to_owned()],
            package_roots: Vec::new(),
            length: 2,
            edges: vec![PackageCycleEdge {
                from_package: "a".to_owned(),
                to_package: "b".to_owned(),
                path: path.to_path_buf(),
                target_path: PathBuf::from("/repo/target.ts"),
                line: REAL_LINE,
                col: 0,
                type_only: false,
            }],
            group_truncated: false,
        }));
    results
        .duplicate_exports
        .push(DuplicateExportFinding::with_actions(DuplicateExport {
            export_name: "value".to_owned(),
            locations: [REAL_LINE, REAL_LINE + 1]
                .into_iter()
                .map(|line| DuplicateLocation {
                    path: path.to_path_buf(),
                    line,
                    col: 0,
                })
                .collect(),
        }));
    results
        .unlisted_dependencies
        .push(UnlistedDependencyFinding::with_actions(
            UnlistedDependency {
                package_name: "lodash".to_owned(),
                imported_from: vec![ImportSite {
                    path: path.to_path_buf(),
                    line: REAL_LINE,
                    col: 0,
                }],
            },
        ));
    results
        .unlisted_binaries
        .push(UnlistedBinaryFinding::with_actions(UnlistedBinary {
            binary_name: "tsc".to_owned(),
            used_in: vec![BinaryUseSite {
                path: path.to_path_buf(),
                line: REAL_LINE,
                col: 0,
                script_name: None,
            }],
        }));
}

fn security_finding(path: &Path) -> SecurityFinding {
    SecurityFinding {
        finding_id: "id".to_owned(),
        stable_finding_id: None,
        stable_id_input: None,
        kind: SecurityFindingKind::TaintedSink,
        category: Some("dangerous-html".to_owned()),
        cwe: Some(79),
        path: path.to_path_buf(),
        line: REAL_LINE,
        col: 0,
        evidence: "candidate".to_owned(),
        source_backed: false,
        source_read: None,
        severity: SecuritySeverity::Low,
        trace: vec![TraceHop::new(
            path.to_path_buf(),
            REAL_LINE,
            0,
            TraceHopRole::Sink,
        )],
        actions: Vec::new(),
        dead_code: None,
        reachability: None,
        candidate: SecurityCandidate {
            source_kind: None,
            sink: SecurityCandidateSink {
                path: path.to_path_buf(),
                line: REAL_LINE,
                col: 0,
                category: Some("dangerous-html".to_owned()),
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
    }
}

fn dependency(manifest: &Path) -> UnusedDependency {
    UnusedDependency {
        package_name: "lodash".to_owned(),
        location: DependencyLocation::Dependencies,
        path: manifest.to_path_buf(),
        line: REAL_LINE,
        used_in_workspaces: Vec::new(),
        declared_and_imported_in: Vec::new(),
    }
}

fn push_manifest_findings(results: &mut AnalysisResults, root: &Path) {
    let manifest = root.join("package.json");
    results
        .unused_dependencies
        .push(UnusedDependencyFinding::with_actions(dependency(&manifest)));
    results
        .unused_dev_dependencies
        .push(UnusedDevDependencyFinding::with_actions(dependency(
            &manifest,
        )));
    results
        .unused_optional_dependencies
        .push(UnusedOptionalDependencyFinding::with_actions(dependency(
            &manifest,
        )));
    results
        .type_only_dependencies
        .push(TypeOnlyDependencyFinding::with_actions(
            TypeOnlyDependency {
                package_name: "zod".to_owned(),
                path: manifest.clone(),
                line: REAL_LINE,
            },
        ));
    results
        .test_only_dependencies
        .push(TestOnlyDependencyFinding::with_actions(
            TestOnlyDependency {
                package_name: "vitest".to_owned(),
                path: manifest.clone(),
                line: REAL_LINE,
            },
        ));
    results
        .dev_dependencies_in_production
        .push(DevDependencyInProductionFinding::with_actions(
            DevDependencyInProduction {
                package_name: "chalk".to_owned(),
                path: manifest,
                line: REAL_LINE,
            },
        ));
}

fn push_catalog_findings(results: &mut AnalysisResults, path: &Path) {
    let path = path.to_path_buf();
    results
        .unused_catalog_entries
        .push(UnusedCatalogEntryFinding::with_actions(
            UnusedCatalogEntry {
                entry_name: "react".to_owned(),
                catalog_name: "default".to_owned(),
                path: path.clone(),
                line: REAL_LINE,
                hardcoded_consumers: Vec::new(),
            },
        ));
    results
        .empty_catalog_groups
        .push(EmptyCatalogGroupFinding::with_actions(EmptyCatalogGroup {
            catalog_name: "legacy".to_owned(),
            path: path.clone(),
            line: REAL_LINE,
        }));
    results
        .unresolved_catalog_references
        .push(UnresolvedCatalogReferenceFinding::with_actions(
            UnresolvedCatalogReference {
                entry_name: "react".to_owned(),
                catalog_name: "default".to_owned(),
                path: path.clone(),
                line: REAL_LINE,
                available_in_catalogs: Vec::new(),
            },
        ));
    results
        .unused_dependency_overrides
        .push(UnusedDependencyOverrideFinding::with_actions(
            UnusedDependencyOverride {
                raw_key: "react".to_owned(),
                target_package: "react".to_owned(),
                parent_package: None,
                version_constraint: None,
                version_range: "18".to_owned(),
                source: DependencyOverrideSource::PnpmWorkspaceYaml,
                path: path.clone(),
                line: REAL_LINE,
                hint: None,
            },
        ));
    results.misconfigured_dependency_overrides.push(
        MisconfiguredDependencyOverrideFinding::with_actions(MisconfiguredDependencyOverride {
            raw_key: "react".to_owned(),
            target_package: None,
            raw_value: String::new(),
            reason: DependencyOverrideMisconfigReason::EmptyValue,
            source: DependencyOverrideSource::PnpmWorkspaceYaml,
            path,
            line: REAL_LINE,
        }),
    );
}

/// Push one prop-drilling chain, thin wrapper and duplicate prop shape for
/// each severity.
fn push_component_health_findings(results: &mut AnalysisResults, path: &Path) {
    for severity in [EffectiveSeverity::Error, EffectiveSeverity::Warn] {
        let mut chain = PropDrillingChainFinding::with_actions(PropDrillingChain {
            prop: "user".to_owned(),
            depth: 1,
            hops: vec![PropDrillHop {
                file: path.to_path_buf(),
                line: REAL_LINE,
                component: "Hop".to_owned(),
            }],
        });
        chain.effective_severity = Some(severity);
        results.prop_drilling_chains.push(chain);

        let mut wrapper = ThinWrapperFinding::with_actions(ThinWrapper {
            file: path.to_path_buf(),
            line: REAL_LINE,
            component: "Wrapper".to_owned(),
            child_component: "Child".to_owned(),
        });
        wrapper.effective_severity = Some(severity);
        results.thin_wrappers.push(wrapper);

        let mut shape = DuplicatePropShapeFinding::with_actions(DuplicatePropShape {
            file: path.to_path_buf(),
            line: REAL_LINE,
            component: "Card".to_owned(),
            shape: vec!["title".to_owned(), "subtitle".to_owned()],
            group_size: 2,
            sharing_components: Vec::new(),
        });
        shape.effective_severity = Some(severity);
        results.duplicate_prop_shapes.push(shape);
    }
}

/// Results with findings of every scoped array in the `inside` and the
/// `outside` workspace.
fn populated(inside: &Path, outside: &Path) -> AnalysisResults {
    let mut results = AnalysisResults::default();
    for root in [inside, outside] {
        let path = source(root);
        push_source_findings(&mut results, &path);
        push_component_contract_findings(&mut results, &path);
        push_component_io_findings(&mut results, &path);
        push_effect_and_server_findings(&mut results, &path);
        push_route_and_policy_findings(&mut results, &path);
        push_graph_findings(&mut results, &path);
        push_catalog_findings(&mut results, &path);
        push_component_health_findings(&mut results, &path);
        push_manifest_findings(&mut results, root);
        results.security_findings.push(security_finding(&path));
    }
    results
}

/// The length of each serialized finding array, plus the security arrays,
/// which the serializer skips.
fn lengths(results: &AnalysisResults) -> BTreeMap<String, usize> {
    let value = serde_json::to_value(results).expect("results serialize");
    let mut lengths: BTreeMap<String, usize> = value
        .as_object()
        .expect("results are an object")
        .iter()
        .filter_map(|(key, value)| Some((key.clone(), value.as_array()?.len())))
        .collect();
    lengths.insert(
        "security_findings".to_owned(),
        results.security_findings.len(),
    );
    lengths.insert(
        "security_unresolved_callee_diagnostics".to_owned(),
        results.security_unresolved_callee_diagnostics.len(),
    );
    lengths
}

fn expected(entries: &[(&str, usize)]) -> BTreeMap<String, usize> {
    entries
        .iter()
        .map(|(key, len)| ((*key).to_owned(), *len))
        .collect()
}

#[test]
fn changed_file_scope_keeps_the_expected_arrays() {
    let mut results = populated(&inside(), &outside());
    let changed: FxHashSet<PathBuf> = std::iter::once(source(&inside())).collect();

    crate::changed_files::filter_results_by_changed_files(&mut results, &changed);

    assert_eq!(
        lengths(&results),
        expected(&[
            ("absent_component_props", 1),
            ("boundary_call_violations", 1),
            ("boundary_coverage_violations", 1),
            ("boundary_violations", 1),
            ("circular_dependencies", 1),
            ("constant_parameters", 1),
            ("deprecated_exports_in_use", 1),
            ("dev_dependencies_in_production", 2),
            ("duplicate_export_aliases", 1),
            ("duplicate_exports", 1),
            ("duplicate_prop_shapes", 2),
            ("dynamic_segment_name_conflicts", 1),
            ("empty_catalog_groups", 1),
            ("invalid_client_exports", 1),
            ("misconfigured_dependency_overrides", 1),
            ("misplaced_directives", 1),
            ("mixed_client_server_barrels", 1),
            ("package_cycles", 1),
            ("policy_violations", 1),
            ("private_type_leaks", 1),
            ("prop_drilling_chains", 2),
            ("re_export_cycles", 1),
            ("route_collisions", 1),
            ("security_findings", 1),
            ("security_unresolved_callee_diagnostics", 1),
            ("stale_suppressions", 1),
            ("test_only_dependencies", 2),
            ("thin_wrappers", 2),
            ("type_only_dependencies", 2),
            ("unlisted_binaries", 1),
            ("unlisted_dependencies", 1),
            ("unproduced_catch_tags", 1),
            ("unprovided_injects", 1),
            ("unrendered_components", 1),
            ("unresolved_catalog_references", 1),
            ("unresolved_imports", 1),
            ("unused_catalog_entries", 2),
            ("unused_class_members", 1),
            ("unused_component_emits", 1),
            ("unused_component_inputs", 1),
            ("unused_component_outputs", 1),
            ("unused_component_props", 1),
            ("unused_dependencies", 2),
            ("unused_dependency_overrides", 1),
            ("unused_dev_dependencies", 2),
            ("unused_effect_layers", 1),
            ("unused_enum_members", 1),
            ("unused_exports", 1),
            ("unused_files", 1),
            ("unused_load_data_keys", 1),
            ("unused_namespace_members", 1),
            ("unused_optional_dependencies", 2),
            ("unused_protected_class_members", 1),
            ("unused_server_actions", 1),
            ("unused_store_members", 1),
            ("unused_svelte_events", 1),
            ("unused_tagged_errors", 1),
            ("unused_types", 1),
        ])
    );
}

#[test]
fn diff_scope_keeps_the_expected_arrays() {
    let mut results = populated(&inside(), &outside());
    let diff = DiffIndex::from_unified_diff(
        "diff --git a/packages/a/src/x.ts b/packages/a/src/x.ts\n\
         --- a/packages/a/src/x.ts\n\
         +++ b/packages/a/src/x.ts\n\
         @@ -4,1 +4,3 @@\n\
         \x20ctx\n\
         +added five\n\
         +added six\n",
    );

    crate::diff_scope::filter_dead_code_by_diff(&mut results, &diff, Path::new("/repo"));

    assert_eq!(
        lengths(&results),
        expected(&[
            ("absent_component_props", 1),
            ("boundary_call_violations", 1),
            ("boundary_coverage_violations", 1),
            ("boundary_violations", 1),
            ("circular_dependencies", 1),
            ("constant_parameters", 1),
            ("deprecated_exports_in_use", 1),
            ("dev_dependencies_in_production", 2),
            ("duplicate_export_aliases", 1),
            ("duplicate_exports", 1),
            ("duplicate_prop_shapes", 3),
            ("dynamic_segment_name_conflicts", 1),
            ("empty_catalog_groups", 2),
            ("invalid_client_exports", 1),
            ("misconfigured_dependency_overrides", 2),
            ("misplaced_directives", 1),
            ("mixed_client_server_barrels", 1),
            ("package_cycles", 1),
            ("policy_violations", 1),
            ("private_type_leaks", 1),
            ("prop_drilling_chains", 3),
            ("re_export_cycles", 1),
            ("route_collisions", 1),
            ("security_findings", 1),
            ("security_unresolved_callee_diagnostics", 1),
            ("stale_suppressions", 1),
            ("test_only_dependencies", 2),
            ("thin_wrappers", 3),
            ("type_only_dependencies", 2),
            ("unlisted_binaries", 1),
            ("unlisted_dependencies", 1),
            ("unproduced_catch_tags", 1),
            ("unprovided_injects", 1),
            ("unrendered_components", 1),
            ("unresolved_catalog_references", 2),
            ("unresolved_imports", 1),
            ("unused_catalog_entries", 2),
            ("unused_class_members", 1),
            ("unused_component_emits", 1),
            ("unused_component_inputs", 1),
            ("unused_component_outputs", 1),
            ("unused_component_props", 1),
            ("unused_dependencies", 2),
            ("unused_dependency_overrides", 2),
            ("unused_dev_dependencies", 2),
            ("unused_effect_layers", 1),
            ("unused_enum_members", 1),
            ("unused_exports", 1),
            ("unused_files", 1),
            ("unused_load_data_keys", 1),
            ("unused_namespace_members", 1),
            ("unused_optional_dependencies", 2),
            ("unused_protected_class_members", 1),
            ("unused_server_actions", 1),
            ("unused_store_members", 1),
            ("unused_svelte_events", 1),
            ("unused_tagged_errors", 1),
            ("unused_types", 1),
        ])
    );
}

#[test]
fn workspace_scope_keeps_the_expected_arrays() {
    let mut results = populated(&inside(), &outside());

    crate::dead_code::filter_to_workspaces(&mut results, &vec![inside()].into());

    assert_eq!(
        lengths(&results),
        expected(&[
            ("absent_component_props", 1),
            ("boundary_call_violations", 1),
            ("boundary_coverage_violations", 1),
            ("boundary_violations", 1),
            ("circular_dependencies", 1),
            ("constant_parameters", 1),
            ("deprecated_exports_in_use", 1),
            ("dev_dependencies_in_production", 1),
            ("duplicate_export_aliases", 1),
            ("duplicate_exports", 1),
            ("duplicate_prop_shapes", 2),
            ("dynamic_segment_name_conflicts", 1),
            ("empty_catalog_groups", 0),
            ("invalid_client_exports", 1),
            ("misconfigured_dependency_overrides", 0),
            ("misplaced_directives", 1),
            ("mixed_client_server_barrels", 1),
            ("package_cycles", 1),
            ("policy_violations", 1),
            ("private_type_leaks", 1),
            ("prop_drilling_chains", 2),
            ("re_export_cycles", 1),
            ("route_collisions", 1),
            ("security_findings", 1),
            ("security_unresolved_callee_diagnostics", 1),
            ("stale_suppressions", 1),
            ("test_only_dependencies", 1),
            ("thin_wrappers", 2),
            ("type_only_dependencies", 1),
            ("unlisted_binaries", 1),
            ("unlisted_dependencies", 1),
            ("unproduced_catch_tags", 1),
            ("unprovided_injects", 1),
            ("unrendered_components", 1),
            ("unresolved_catalog_references", 1),
            ("unresolved_imports", 1),
            ("unused_catalog_entries", 0),
            ("unused_class_members", 1),
            ("unused_component_emits", 1),
            ("unused_component_inputs", 1),
            ("unused_component_outputs", 1),
            ("unused_component_props", 1),
            ("unused_dependencies", 1),
            ("unused_dependency_overrides", 0),
            ("unused_dev_dependencies", 1),
            ("unused_effect_layers", 1),
            ("unused_enum_members", 1),
            ("unused_exports", 1),
            ("unused_files", 1),
            ("unused_load_data_keys", 1),
            ("unused_namespace_members", 1),
            ("unused_optional_dependencies", 1),
            ("unused_protected_class_members", 1),
            ("unused_server_actions", 1),
            ("unused_store_members", 1),
            ("unused_svelte_events", 1),
            ("unused_tagged_errors", 1),
            ("unused_types", 1),
        ])
    );
}

/// Findings whose sites, locations or hops are split across the inside and the
/// outside workspace, so each scope rule gives a different result.
fn split_findings() -> AnalysisResults {
    let inside = source(&inside());
    let outside = source(&outside());
    let mut results = AnalysisResults::default();
    results
        .duplicate_exports
        .push(DuplicateExportFinding::with_actions(DuplicateExport {
            export_name: "value".to_owned(),
            locations: [&inside, &outside]
                .into_iter()
                .map(|path| DuplicateLocation {
                    path: path.clone(),
                    line: REAL_LINE,
                    col: 0,
                })
                .collect(),
        }));
    results
        .unlisted_dependencies
        .push(UnlistedDependencyFinding::with_actions(
            UnlistedDependency {
                package_name: "lodash".to_owned(),
                imported_from: [&inside, &outside]
                    .into_iter()
                    .map(|path| ImportSite {
                        path: path.clone(),
                        line: REAL_LINE,
                        col: 0,
                    })
                    .collect(),
            },
        ));
    for (severity, hops) in [
        (EffectiveSeverity::Warn, vec![&outside, &inside]),
        (EffectiveSeverity::Error, Vec::new()),
    ] {
        let mut chain = PropDrillingChainFinding::with_actions(PropDrillingChain {
            prop: "user".to_owned(),
            depth: 2,
            hops: hops
                .into_iter()
                .map(|path| PropDrillHop {
                    file: path.clone(),
                    line: REAL_LINE,
                    component: "Hop".to_owned(),
                })
                .collect(),
        });
        chain.effective_severity = Some(severity);
        results.prop_drilling_chains.push(chain);
    }
    results
}

/// The duplicate export locations, the unlisted dependency sites and the
/// severities of the prop-drilling chains that stay.
fn split_outcome(
    results: &AnalysisResults,
) -> (Vec<usize>, Vec<usize>, Vec<Option<EffectiveSeverity>>) {
    (
        results
            .duplicate_exports
            .iter()
            .map(|f| f.export.locations.len())
            .collect(),
        results
            .unlisted_dependencies
            .iter()
            .map(|f| f.dep.imported_from.len())
            .collect(),
        results
            .prop_drilling_chains
            .iter()
            .map(|f| f.effective_severity)
            .collect(),
    )
}

#[test]
fn each_scope_applies_its_site_owner_and_component_rules() {
    let path_scope_outcome = (Vec::new(), vec![2], vec![Some(EffectiveSeverity::Error)]);

    let mut changed = split_findings();
    let files: FxHashSet<PathBuf> = std::iter::once(source(&inside())).collect();
    crate::changed_files::filter_results_by_changed_files(&mut changed, &files);
    assert_eq!(split_outcome(&changed), path_scope_outcome);

    let mut workspace = split_findings();
    crate::dead_code::filter_to_workspaces(&mut workspace, &vec![inside()].into());
    assert_eq!(split_outcome(&workspace), path_scope_outcome);

    let mut diff = split_findings();
    let index = DiffIndex::from_unified_diff(
        "diff --git a/packages/a/src/x.ts b/packages/a/src/x.ts\n\
         --- a/packages/a/src/x.ts\n\
         +++ b/packages/a/src/x.ts\n\
         @@ -4,1 +4,2 @@\n\
         \x20ctx\n\
         +added five\n",
    );
    crate::diff_scope::filter_dead_code_by_diff(&mut diff, &index, Path::new("/repo"));
    assert_eq!(
        split_outcome(&diff),
        (
            vec![2],
            vec![1],
            vec![
                Some(EffectiveSeverity::Warn),
                Some(EffectiveSeverity::Error)
            ],
        )
    );
}
