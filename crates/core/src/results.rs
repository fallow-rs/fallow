pub use fallow_types::output_dead_code::{
    BoundaryCallViolationFinding, BoundaryCoverageViolationFinding, BoundaryViolationFinding,
    CircularDependencyFinding, DuplicateExportFinding, DuplicatePropShapeFinding,
    DynamicSegmentNameConflictFinding, EmptyCatalogGroupFinding, InvalidClientExportFinding,
    MisconfiguredDependencyOverrideFinding, MisplacedDirectiveFinding,
    MixedClientServerBarrelFinding, PolicyViolationFinding, PrivateTypeLeakFinding,
    ReExportCycleFinding, RouteCollisionFinding, TestOnlyDependencyFinding,
    TypeOnlyDependencyFinding, UnlistedDependencyFinding, UnprovidedInjectFinding,
    UnrenderedComponentFinding, UnresolvedCatalogReferenceFinding, UnresolvedImportFinding,
    UnusedCatalogEntryFinding, UnusedClassMemberFinding, UnusedComponentEmitFinding,
    UnusedComponentPropFinding, UnusedDependencyFinding, UnusedDependencyOverrideFinding,
    UnusedDevDependencyFinding, UnusedEnumMemberFinding, UnusedExportFinding, UnusedFileFinding,
    UnusedLoadDataKeyFinding, UnusedOptionalDependencyFinding, UnusedServerActionFinding,
    UnusedStoreMemberFinding, UnusedTypeFinding,
};
pub use fallow_types::results::{
    ActiveSuppression, AnalysisResults, BoundaryCallViolation, BoundaryCoverageViolation,
    BoundaryViolation, CircularDependency, CircularDependencyEdge, DependencyLocation,
    DependencyOverrideMisconfigReason, DependencyOverrideSource, DuplicateExport,
    DuplicateLocation, DuplicatePropShape, DuplicatePropShapeMember, DynamicSegmentNameConflict,
    EmptyCatalogGroup, EntryPointSummary, ExportUsage, FeatureFlag, FlagConfidence, FlagKind,
    ImportSite, InvalidClientExport, MisconfiguredDependencyOverride, MisplacedDirective,
    MixedClientServerBarrel, PolicyRuleKind, PolicyViolation, PolicyViolationSeverity,
    PrivateTypeLeak, PropDrillHop, PropDrillingChain, ReExportCycle, ReExportCycleKind,
    ReferenceLocation, RenderFanInComponent, RenderFanInMetric, RouteCollision,
    SecurityAttackSurfaceEntry, SecurityCandidate, SecurityCandidateBoundary,
    SecurityCandidateSink, SecurityDeadCodeContext, SecurityDeadCodeKind,
    SecurityDefensiveBoundary, SecurityDefensiveControl, SecurityFinding, SecurityFindingKind,
    SecurityNetworkContext, SecurityReachability, SecuritySeverity, SecurityTaintFlow,
    SecurityUnresolvedCalleeDiagnostic, SecurityZoneCrossing, StaleSuppression, SuppressionOrigin,
    TaintConfidence, TaintEndpoint, TaintPath, TestOnlyDependency, ThinWrapper, TraceHop,
    TraceHopRole, TypeOnlyDependency, UnlistedDependency, UnprovidedInject, UnrenderedComponent,
    UnresolvedCatalogReference, UnresolvedImport, UnusedCatalogEntry, UnusedComponentEmit,
    UnusedComponentProp, UnusedDependency, UnusedDependencyOverride, UnusedExport, UnusedFile,
    UnusedLoadDataKey, UnusedMember, UnusedServerAction,
};

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::extract::MemberKind;
    use fallow_types::output_dead_code::{UnresolvedImportFinding, UnusedFileFinding};

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
            }));
        assert_eq!(results.total_issues(), 1);
        assert!(results.has_issues());
    }

    #[test]
    fn results_total_counts_all_types() {
        let mut results = AnalysisResults::default();
        results
            .unused_files
            .push(UnusedFileFinding::with_actions(UnusedFile {
                path: PathBuf::from("a.ts"),
            }));
        results
            .unused_exports
            .push(UnusedExportFinding::with_actions(UnusedExport {
                path: PathBuf::from("b.ts"),
                export_name: "x".to_string(),
                is_type_only: false,
                line: 1,
                col: 0,
                span_start: 0,
                is_re_export: false,
            }));
        results
            .unused_types
            .push(UnusedTypeFinding::with_actions(UnusedExport {
                path: PathBuf::from("c.ts"),
                export_name: "T".to_string(),
                is_type_only: true,
                line: 1,
                col: 0,
                span_start: 0,
                is_re_export: false,
            }));
        results
            .unused_dependencies
            .push(UnusedDependencyFinding::with_actions(UnusedDependency {
                package_name: "dep".to_string(),
                location: DependencyLocation::Dependencies,
                path: PathBuf::from("package.json"),
                line: 5,
                used_in_workspaces: Vec::new(),
            }));
        results
            .unused_dev_dependencies
            .push(UnusedDevDependencyFinding::with_actions(UnusedDependency {
                package_name: "dev".to_string(),
                location: DependencyLocation::DevDependencies,
                path: PathBuf::from("package.json"),
                line: 5,
                used_in_workspaces: Vec::new(),
            }));
        results
            .unused_enum_members
            .push(UnusedEnumMemberFinding::with_actions(UnusedMember {
                path: PathBuf::from("d.ts"),
                parent_name: "E".to_string(),
                member_name: "A".to_string(),
                kind: MemberKind::EnumMember,
                line: 1,
                col: 0,
            }));
        results
            .unused_class_members
            .push(UnusedClassMemberFinding::with_actions(UnusedMember {
                path: PathBuf::from("e.ts"),
                parent_name: "C".to_string(),
                member_name: "m".to_string(),
                kind: MemberKind::ClassMethod,
                line: 1,
                col: 0,
            }));
        results
            .unresolved_imports
            .push(UnresolvedImportFinding::with_actions(UnresolvedImport {
                path: PathBuf::from("f.ts"),
                specifier: "./missing".to_string(),
                line: 1,
                col: 0,
                specifier_col: 0,
            }));
        results
            .unlisted_dependencies
            .push(UnlistedDependencyFinding::with_actions(
                UnlistedDependency {
                    package_name: "unlisted".to_string(),
                    imported_from: vec![ImportSite {
                        path: PathBuf::from("g.ts"),
                        line: 1,
                        col: 0,
                    }],
                },
            ));
        results
            .duplicate_exports
            .push(DuplicateExportFinding::with_actions(DuplicateExport {
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
            }));

        assert_eq!(results.total_issues(), 10);
        assert!(results.has_issues());
    }
}
