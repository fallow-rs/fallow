mod quality;
pub mod security;
mod structural;
mod unused;

use rustc_hash::FxHashMap;
use std::path::Path;

use ls_types::{CodeDescription, Diagnostic, Position, Range, Uri};

use fallow_api::{
    EditorAnalysisResults as AnalysisResults, EditorDuplicationReport as DuplicationReport,
};

/// Base URL for diagnostic documentation links.
const DOCS_BASE: &str = "https://docs.fallow.tools/explanations/dead-code#";

/// Build a `CodeDescription` with a documentation URL for the given anchor.
fn doc_link(anchor: &str) -> Option<CodeDescription> {
    let url = format!("{DOCS_BASE}{anchor}");
    url.parse::<Uri>().ok().map(|href| CodeDescription { href })
}

/// LSP range covering the entire first line — used for file-level and package.json diagnostics.
pub const FIRST_LINE_RANGE: Range = Range {
    start: Position {
        line: 0,
        character: 0,
    },
    end: Position {
        line: 0,
        character: u32::MAX,
    },
};

/// Build all LSP diagnostics from analysis results and duplication report, keyed by file URI.
#[derive(Clone, Copy)]
pub struct DiagnosticInput<'a> {
    pub results: &'a AnalysisResults,
    pub duplication: &'a DuplicationReport,
    pub root: &'a Path,
}

impl<'a> DiagnosticInput<'a> {
    #[must_use]
    pub const fn new(
        results: &'a AnalysisResults,
        duplication: &'a DuplicationReport,
        root: &'a Path,
    ) -> Self {
        Self {
            results,
            duplication,
            root,
        }
    }
}

/// Build all LSP diagnostics from a typed editor analysis input, keyed by file URI.
pub fn build_diagnostics(input: DiagnosticInput<'_>) -> FxHashMap<Uri, Vec<Diagnostic>> {
    let DiagnosticInput {
        results,
        duplication,
        root,
    } = input;
    let mut map: FxHashMap<Uri, Vec<Diagnostic>> = FxHashMap::default();
    let package_json_uri = Uri::from_file_path(root.join("package.json"));

    unused::push_export_diagnostics(&mut map, results);
    unused::push_file_diagnostics(&mut map, results);
    unused::push_import_diagnostics(&mut map, results);
    unused::push_dep_diagnostics(&mut map, results, package_json_uri.as_ref(), root);
    unused::push_member_diagnostics(&mut map, results);
    quality::push_duplicate_export_diagnostics(&mut map, results);
    quality::push_duplication_diagnostics(&mut map, duplication);
    structural::push_circular_dep_diagnostics(&mut map, results);
    structural::push_re_export_cycle_diagnostics(&mut map, results);
    structural::push_boundary_violation_diagnostics(&mut map, results);
    structural::push_policy_violation_diagnostics(&mut map, results);
    structural::push_invalid_client_export_diagnostics(&mut map, results);
    structural::push_mixed_client_server_barrel_diagnostics(&mut map, results);
    structural::push_misplaced_directive_diagnostics(&mut map, results);
    structural::push_unprovided_inject_diagnostics(&mut map, results);
    structural::push_route_collision_diagnostics(&mut map, results);
    structural::push_dynamic_segment_name_conflict_diagnostics(&mut map, results);
    quality::push_stale_suppression_diagnostics(&mut map, results);
    security::push_security_diagnostics(&mut map, results);

    map
}

#[cfg(test)]
fn build_diagnostics_for_test(
    results: &AnalysisResults,
    duplication: &DuplicationReport,
    root: &Path,
) -> FxHashMap<Uri, Vec<Diagnostic>> {
    build_diagnostics(DiagnosticInput::new(results, duplication, root))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    use fallow_api::editor_duplicates::{DuplicationReport, DuplicationStats};
    use fallow_api::editor_results::{
        AnalysisResults, SecuritySeverity, UnresolvedImport, UnresolvedImportFinding, UnusedExport,
        UnusedExportFinding, UnusedFile, UnusedFileFinding,
    };

    fn test_root() -> PathBuf {
        if cfg!(windows) {
            PathBuf::from("C:\\project")
        } else {
            PathBuf::from("/project")
        }
    }

    fn empty_duplication() -> DuplicationReport {
        DuplicationReport {
            clone_groups: vec![],
            clone_families: vec![],
            mirrored_directories: vec![],
            stats: DuplicationStats {
                total_files: 0,
                files_with_clones: 0,
                total_lines: 0,
                duplicated_lines: 0,
                total_tokens: 0,
                duplicated_tokens: 0,
                clone_groups: 0,
                clone_instances: 0,
                duplication_percentage: 0.0,
                clone_groups_below_min_occurrences: 0,
            },
        }
    }

    #[test]
    fn empty_results_produce_no_diagnostics() {
        let results = AnalysisResults::default();
        let duplication = empty_duplication();
        let root = test_root();

        let diags = build_diagnostics_for_test(&results, &duplication, &root);
        assert!(diags.is_empty());
    }

    #[test]
    fn multiple_issues_same_file_aggregate() {
        let root = test_root();
        let mut results = AnalysisResults::default();
        let path = root.join("src/mod.ts");
        results
            .unused_exports
            .push(UnusedExportFinding::with_actions(UnusedExport {
                path: path.clone(),
                export_name: "foo".to_string(),
                is_type_only: false,
                line: 1,
                col: 0,
                span_start: 0,
                is_re_export: false,
            }));
        results
            .unused_exports
            .push(UnusedExportFinding::with_actions(UnusedExport {
                path: path.clone(),
                export_name: "bar".to_string(),
                is_type_only: false,
                line: 5,
                col: 0,
                span_start: 50,
                is_re_export: false,
            }));
        results
            .unresolved_imports
            .push(UnresolvedImportFinding::with_actions(UnresolvedImport {
                path: path.clone(),
                specifier: "./gone".to_string(),
                line: 10,
                col: 0,
                specifier_col: 0,
            }));

        let duplication = empty_duplication();
        let diags = build_diagnostics_for_test(&results, &duplication, &root);

        let uri = Uri::from_file_path(&path).unwrap();
        let file_diags = &diags[&uri];
        assert_eq!(file_diags.len(), 3);
    }

    #[test]
    fn all_diagnostics_have_fallow_source() {
        let root = test_root();
        let mut results = AnalysisResults::default();
        results
            .unused_files
            .push(UnusedFileFinding::with_actions(UnusedFile {
                path: root.join("src/a.ts"),
            }));
        results
            .unused_exports
            .push(UnusedExportFinding::with_actions(UnusedExport {
                path: root.join("src/b.ts"),
                export_name: "x".to_string(),
                is_type_only: false,
                line: 1,
                col: 0,
                span_start: 0,
                is_re_export: false,
            }));
        results
            .unresolved_imports
            .push(UnresolvedImportFinding::with_actions(UnresolvedImport {
                path: root.join("src/c.ts"),
                specifier: "./nope".to_string(),
                line: 1,
                col: 0,
                specifier_col: 0,
            }));

        let duplication = empty_duplication();
        let diags = build_diagnostics_for_test(&results, &duplication, &root);

        for file_diags in diags.values() {
            for d in file_diags {
                assert_eq!(d.source, Some("fallow".to_string()));
            }
        }
    }

    #[test]
    fn build_diagnostics_wires_security_block() {
        let root = test_root();
        let path = root.join("src/render.ts");
        let mut results = AnalysisResults::default();
        results
            .security_findings
            .push(fallow_api::editor_results::SecurityFinding {
                finding_id: String::new(),
                candidate: fallow_api::editor_results::SecurityCandidate::default(),
                taint_flow: None,
                attack_surface: None,
                kind: fallow_api::editor_results::SecurityFindingKind::TaintedSink,
                category: Some("dangerous-html".to_string()),
                cwe: Some(79),
                path: path.clone(),
                line: 4,
                col: 2,
                evidence: "sink".to_string(),
                source_backed: false,
                source_read: None,
                severity: SecuritySeverity::Low,
                trace: vec![],
                actions: vec![],
                dead_code: None,
                reachability: None,
                runtime: None,
            });

        let duplication = empty_duplication();
        let diags = build_diagnostics_for_test(&results, &duplication, &root);
        let uri = Uri::from_file_path(&path).unwrap();
        let file_diags = diags.get(&uri).expect("security diagnostic present");
        assert!(file_diags.iter().any(|d| matches!(
            &d.code,
            Some(ls_types::NumberOrString::String(c)) if c == "security-sink"
        )));
    }

    #[test]
    fn doc_link_produces_valid_url() {
        let link = doc_link("unused-exports");
        assert!(link.is_some());
        let desc = link.unwrap();
        assert_eq!(
            desc.href.as_str(),
            "https://docs.fallow.tools/explanations/dead-code#unused-exports"
        );
    }

    #[test]
    fn first_line_range_values() {
        assert_eq!(FIRST_LINE_RANGE.start.line, 0);
        assert_eq!(FIRST_LINE_RANGE.start.character, 0);
        assert_eq!(FIRST_LINE_RANGE.end.line, 0);
        assert_eq!(FIRST_LINE_RANGE.end.character, u32::MAX);
    }
}

/// LSP severity drift gate.
///
/// Two guards work together so that adding a new dead-code `IssueKind` (a new
/// `AnalysisResults` field that produces a diagnostic) forces the author to
/// declare its editor severity, AND so that an existing rule emitting the wrong
/// level is caught:
///
/// 1. [`severity_gate_classifies_every_result_field`] exhaustively destructures
///    `AnalysisResults` with NO `..` rest pattern (the same compile-time pin as
///    `merge_results_covers_all_fields` in `main.rs`, issue #444). Adding a new
///    field is a COMPILE error here until the author drops it into one of the
///    two buckets: a diagnostic-emitting dead-code field (which must gain a row
///    in [`severity_gate_emits_expected_severity_per_kind`]) or a
///    destructured-and-ignored non-diagnostic field (metadata / counts /
///    advisory). `AnalysisResults` is NOT `#[non_exhaustive]` (defined in
///    `fallow_types::results`, re-exported via `fallow_api::editor_results`), so the
///    exhaustive destructure compiles cross-crate -- the preferred mechanism.
///
/// 2. [`severity_gate_emits_expected_severity_per_kind`] builds a synthetic
///    one-finding `AnalysisResults` per dead-code kind and asserts
///    `build_diagnostics` emits the EXACT severity the explicit per-kind table
///    declares. A production-severity flip (e.g. `route-collision`
///    ERROR -> WARNING) fails this test. For the two kinds whose LSP severity
///    must agree with the core `RulesConfig` default
///    (`route-collision` + `dynamic-segment-name-conflict`), the expected
///    ERROR is cross-checked against `fallow_config::RulesConfig::default()` so
///    the table cannot silently drift from core.
#[cfg(test)]
mod severity_gate {
    use std::path::PathBuf;

    use fallow_api::editor_duplicates::{DuplicationReport, DuplicationStats};
    use fallow_api::editor_results::AnalysisResults;
    use fallow_config::{RulesConfig, Severity};
    use ls_types::DiagnosticSeverity;

    use crate::diagnostics::build_diagnostics_for_test;

    fn test_root() -> PathBuf {
        if cfg!(windows) {
            PathBuf::from("C:\\project")
        } else {
            PathBuf::from("/project")
        }
    }

    fn empty_duplication() -> DuplicationReport {
        DuplicationReport {
            clone_groups: vec![],
            clone_families: vec![],
            mirrored_directories: vec![],
            stats: DuplicationStats {
                total_files: 0,
                files_with_clones: 0,
                total_lines: 0,
                duplicated_lines: 0,
                total_tokens: 0,
                duplicated_tokens: 0,
                clone_groups: 0,
                clone_instances: 0,
                duplication_percentage: 0.0,
                clone_groups_below_min_occurrences: 0,
            },
        }
    }

    /// COMPILE-TIME drift gate: exhaustively destructure `AnalysisResults` with
    /// NO `..` so a NEW field is a compile error here, forcing the author to
    /// classify it. Each binding is sorted into one of two buckets:
    ///
    /// - DEAD-CODE DIAGNOSTIC fields: listed in the first block. A new
    ///   diagnostic-emitting field added here MUST also gain a row in
    ///   [`expected_severity_table`] (the runtime severity assertion), or the
    ///   drift gate is incomplete. (The compiler does not know which bucket a
    ///   field belongs in; the explicit two-block split plus this doc-comment is
    ///   the hand-off.)
    /// - NON-DIAGNOSTIC fields: destructured-and-ignored below with a reason
    ///   (metadata for Code Lens / entry points, suppression bookkeeping,
    ///   advisory feature flags, or security candidates handled by `security.rs`
    ///   with a FIXED `INFORMATION` severity that is not rule-mapped).
    #[test]
    fn severity_gate_classifies_every_result_field() {
        let AnalysisResults {
            // ---- dead-code diagnostic fields (each has a row in the table) ----
            unused_files: _,
            unused_exports: _,
            unused_types: _,
            private_type_leaks: _,
            unused_dependencies: _,
            unused_dev_dependencies: _,
            unused_optional_dependencies: _,
            unused_enum_members: _,
            unused_class_members: _,
            unused_store_members: _,
            unresolved_imports: _,
            unlisted_dependencies: _,
            duplicate_exports: _,
            type_only_dependencies: _,
            test_only_dependencies: _,
            circular_dependencies: _,
            re_export_cycles: _,
            boundary_violations: _,
            boundary_coverage_violations: _,
            boundary_call_violations: _,
            policy_violations: _,
            stale_suppressions: _,
            unused_catalog_entries: _,
            empty_catalog_groups: _,
            unresolved_catalog_references: _,
            unused_dependency_overrides: _,
            misconfigured_dependency_overrides: _,
            invalid_client_exports: _,
            mixed_client_server_barrels: _,
            misplaced_directives: _,
            unprovided_injects: _,
            unrendered_components: _,
            unused_component_props: _,
            unused_component_emits: _,
            unused_component_inputs: _,
            unused_component_outputs: _,
            unused_svelte_events: _,
            unused_server_actions: _,
            unused_load_data_keys: _,
            prop_drilling_chains: _,
            thin_wrappers: _,
            duplicate_prop_shapes: _,
            route_collisions: _,
            dynamic_segment_name_conflicts: _,
            // ---- non-diagnostic fields (destructured-and-ignored) ----
            // Security candidates are surfaced by `security.rs` at a FIXED
            // `INFORMATION` severity (the LSP `[I]` advisory glyph), not mapped
            // from rule severity, so they are intentionally outside the
            // dead-code severity table.
            security_findings: _,
            security_unresolved_edge_files: _,
            security_unresolved_callee_sites: _,
            security_unresolved_callee_diagnostics: _,
            // Suppression bookkeeping: counts, not diagnostics.
            suppression_count: _,
            unused_component_props_exempted: _,
            active_suppressions: _,
            // Advisory metadata: feature flags are not an issue type; export
            // usages drive Code Lens; entry points are informational.
            feature_flags: _,
            export_usages: _,
            entry_point_summary: _,
            // Whole-project descriptive render fan-in metric (component-graph
            // analogue of module fan-in); surfaced via health vital signs, not a
            // per-finding LSP diagnostic.
            render_fan_in: _,
            // Per-component React render/prop/hook intelligence; surfaced as
            // ambient code-lens + hover context, not a per-finding diagnostic.
            react_component_intel: _,
            // Project-wide abstain flag for the `unused-load-data-key` detector;
            // an observability bool, not a per-finding diagnostic.
            unused_load_data_keys_global_abstain: _,
        } = AnalysisResults::default();
    }

    /// Build a one-finding `AnalysisResults` for a single dead-code kind, run
    /// `build_diagnostics`, and return the lone emitted severity. Panics if the
    /// kind produced anything other than exactly one diagnostic (a wiring change
    /// that splits or drops the kind should fail loudly, not silently pass).
    fn emitted_severity(
        build: impl FnOnce(&PathBuf, &mut AnalysisResults),
    ) -> Option<DiagnosticSeverity> {
        let root = test_root();
        let mut results = AnalysisResults::default();
        build(&root, &mut results);
        let diags = build_diagnostics_for_test(&results, &empty_duplication(), &root);
        let all: Vec<_> = diags.values().flatten().collect();
        assert_eq!(
            all.len(),
            1,
            "each gate fixture must emit exactly one diagnostic",
        );
        all[0].severity
    }

    /// Severity drift gate: builds one synthetic finding per dead-code kind and
    /// asserts each emits its expected `DiagnosticSeverity` from the table below.
    /// The exhaustive `AnalysisResults` destructure forces a new result field to
    /// be classified here before it can compile.
    ///
    /// Most kinds match their core `RulesConfig` default. The deliberate
    /// divergences are `circular-dependency` and the `boundary-violation` family:
    /// core default ERROR, but the LSP softens them to WARNING. DECIDED
    /// 2026-06-15: keep them editor-softer (WARNING) while CI still gates them at
    /// error, because both can be numerous and appear mid-refactor, so a red
    /// squiggle per occurrence would be noisy (unlike the rare, always-real
    /// `route-collision` / `dynamic-segment-name-conflict`, which DO render
    /// ERROR). Pinned here so the divergence is a deliberate, reviewed value
    /// rather than silent drift.
    #[expect(
        clippy::too_many_lines,
        reason = "intentionally builds one finding per dead-code kind so each emitted severity is asserted against the explicit table; see #444 sibling gate"
    )]
    #[test]
    fn severity_gate_emits_expected_severity_per_kind() {
        use ls_types::DiagnosticSeverity as S;

        // EXPLICIT per-kind severity table. The first element of each tuple is
        // the code token (for failure messages); the second is the EXPECTED
        // severity; the third builds a synthetic one-finding result. A
        // production-severity flip in `diagnostics/{unused,structural,quality}.rs`
        // fails the assertion below.
        #[expect(
            clippy::type_complexity,
            reason = "table of (code, expected severity, fixture builder) is clearer inline than a named struct here"
        )]
        let table: Vec<(&str, S, Box<dyn Fn(&PathBuf, &mut AnalysisResults)>)> = vec![
            (
                "unused-file",
                S::WARNING,
                Box::new(|root, r| {
                    r.unused_files.push(
                        fallow_api::editor_results::UnusedFileFinding::with_actions(
                            fallow_api::editor_results::UnusedFile {
                                path: root.join("a.ts"),
                            },
                        ),
                    );
                }),
            ),
            (
                "unused-export",
                S::HINT,
                Box::new(|root, r| {
                    r.unused_exports.push(
                        fallow_api::editor_results::UnusedExportFinding::with_actions(
                            fallow_api::editor_results::UnusedExport {
                                path: root.join("a.ts"),
                                export_name: "x".to_string(),
                                is_type_only: false,
                                line: 1,
                                col: 0,
                                span_start: 0,
                                is_re_export: false,
                            },
                        ),
                    );
                }),
            ),
            (
                // HINT: type exports share the `push_export_diagnostics` loop
                // with `unused-export`, which emits HINT (an unobtrusive
                // "fade-out" squiggle for the deletable-symbol family), NOT the
                // ERROR the core `unused_types` default would suggest. The table
                // pins the CURRENT emitted value so a regression is caught.
                "unused-type",
                S::HINT,
                Box::new(|root, r| {
                    r.unused_types.push(
                        fallow_api::editor_results::UnusedTypeFinding::with_actions(
                            fallow_api::editor_results::UnusedExport {
                                path: root.join("a.ts"),
                                export_name: "T".to_string(),
                                is_type_only: true,
                                line: 1,
                                col: 0,
                                span_start: 0,
                                is_re_export: false,
                            },
                        ),
                    );
                }),
            ),
            (
                "private-type-leak",
                S::WARNING,
                Box::new(|root, r| {
                    r.private_type_leaks.push(
                        fallow_api::editor_results::PrivateTypeLeakFinding::with_actions(
                            fallow_api::editor_results::PrivateTypeLeak {
                                path: root.join("a.ts"),
                                export_name: "pub_fn".to_string(),
                                type_name: "Secret".to_string(),
                                line: 1,
                                col: 0,
                                span_start: 0,
                            },
                        ),
                    );
                }),
            ),
            (
                "unused-dependency",
                S::WARNING,
                Box::new(|root, r| {
                    r.unused_dependencies.push(
                        fallow_api::editor_results::UnusedDependencyFinding::with_actions(
                            fallow_api::editor_results::UnusedDependency {
                                package_name: "dep".to_string(),
                                location:
                                    fallow_api::editor_results::DependencyLocation::Dependencies,
                                path: root.join("package.json"),
                                line: 3,
                                used_in_workspaces: Vec::new(),
                            },
                        ),
                    );
                }),
            ),
            (
                "unused-dev-dependency",
                S::WARNING,
                Box::new(|root, r| {
                    r.unused_dev_dependencies.push(
                        fallow_api::editor_results::UnusedDevDependencyFinding::with_actions(
                            fallow_api::editor_results::UnusedDependency {
                                package_name: "dev-dep".to_string(),
                                location:
                                    fallow_api::editor_results::DependencyLocation::DevDependencies,
                                path: root.join("package.json"),
                                line: 4,
                                used_in_workspaces: Vec::new(),
                            },
                        ),
                    );
                }),
            ),
            (
                "unused-optional-dependency",
                S::WARNING,
                Box::new(|root, r| {
                    r.unused_optional_dependencies.push(
                        fallow_api::editor_results::UnusedOptionalDependencyFinding::with_actions(
                            fallow_api::editor_results::UnusedDependency {
                                package_name: "opt-dep".to_string(),
                                location:
                                    fallow_api::editor_results::DependencyLocation::OptionalDependencies,
                                path: root.join("package.json"),
                                line: 5,
                                used_in_workspaces: Vec::new(),
                            },
                        ),
                    );
                }),
            ),
            (
                "unused-enum-member",
                S::HINT,
                Box::new(|root, r| {
                    r.unused_enum_members.push(
                        fallow_api::editor_results::UnusedEnumMemberFinding::with_actions(
                            fallow_api::editor_results::UnusedMember {
                                path: root.join("a.ts"),
                                parent_name: "E".to_string(),
                                member_name: "A".to_string(),
                                kind: fallow_api::editor_extract::MemberKind::EnumMember,
                                line: 6,
                                col: 0,
                            },
                        ),
                    );
                }),
            ),
            (
                "unused-class-member",
                S::HINT,
                Box::new(|root, r| {
                    r.unused_class_members.push(
                        fallow_api::editor_results::UnusedClassMemberFinding::with_actions(
                            fallow_api::editor_results::UnusedMember {
                                path: root.join("a.ts"),
                                parent_name: "C".to_string(),
                                member_name: "m".to_string(),
                                kind: fallow_api::editor_extract::MemberKind::ClassMethod,
                                line: 7,
                                col: 0,
                            },
                        ),
                    );
                }),
            ),
            (
                "unused-store-member",
                S::HINT,
                Box::new(|root, r| {
                    r.unused_store_members.push(
                        fallow_api::editor_results::UnusedStoreMemberFinding::with_actions(
                            fallow_api::editor_results::UnusedMember {
                                path: root.join("a.ts"),
                                parent_name: "S".to_string(),
                                member_name: "a".to_string(),
                                kind: fallow_api::editor_extract::MemberKind::StoreMember,
                                line: 8,
                                col: 0,
                            },
                        ),
                    );
                }),
            ),
            (
                "unresolved-import",
                S::ERROR,
                Box::new(|root, r| {
                    r.unresolved_imports.push(
                        fallow_api::editor_results::UnresolvedImportFinding::with_actions(
                            fallow_api::editor_results::UnresolvedImport {
                                path: root.join("a.ts"),
                                specifier: "./gone".to_string(),
                                line: 1,
                                col: 0,
                                specifier_col: 0,
                            },
                        ),
                    );
                }),
            ),
            (
                "unlisted-dependency",
                S::WARNING,
                Box::new(|_root, r| {
                    r.unlisted_dependencies.push(
                        fallow_api::editor_results::UnlistedDependencyFinding::with_actions(
                            fallow_api::editor_results::UnlistedDependency {
                                package_name: "unlisted".to_string(),
                                imported_from: vec![],
                            },
                        ),
                    );
                }),
            ),
            (
                "duplicate-export",
                S::WARNING,
                Box::new(|root, r| {
                    r.duplicate_exports.push(
                        fallow_api::editor_results::DuplicateExportFinding::with_actions(
                            fallow_api::editor_results::DuplicateExport {
                                export_name: "dup".to_string(),
                                locations: vec![fallow_api::editor_results::DuplicateLocation {
                                    path: root.join("a.ts"),
                                    line: 1,
                                    col: 0,
                                }],
                            },
                        ),
                    );
                }),
            ),
            (
                "type-only-dependency",
                S::INFORMATION,
                Box::new(|root, r| {
                    r.type_only_dependencies.push(
                        fallow_api::editor_results::TypeOnlyDependencyFinding::with_actions(
                            fallow_api::editor_results::TypeOnlyDependency {
                                package_name: "type-only".to_string(),
                                path: root.join("package.json"),
                                line: 9,
                            },
                        ),
                    );
                }),
            ),
            (
                "test-only-dependency",
                S::INFORMATION,
                Box::new(|root, r| {
                    r.test_only_dependencies.push(
                        fallow_api::editor_results::TestOnlyDependencyFinding::with_actions(
                            fallow_api::editor_results::TestOnlyDependency {
                                package_name: "test-only".to_string(),
                                path: root.join("package.json"),
                                line: 10,
                            },
                        ),
                    );
                }),
            ),
            (
                // INTENTIONAL editor-softer deviation from the core ERROR
                // default (`RulesConfig::default().circular_dependencies ==
                // Severity::Error`). The LSP softens a cycle to a WARNING
                // squiggle. FLAGGED in the agent report for a follow-up
                // decision; pinned here so the value is deliberate.
                "circular-dependency",
                S::WARNING,
                Box::new(|root, r| {
                    r.circular_dependencies.push(
                        fallow_api::editor_results::CircularDependencyFinding::with_actions(
                            fallow_api::editor_results::CircularDependency {
                                files: vec![root.join("a.ts"), root.join("b.ts")],
                                length: 2,
                                line: 1,
                                col: 0,
                                edges: Vec::new(),
                                is_cross_package: false,
                            },
                        ),
                    );
                }),
            ),
            (
                "re-export-cycle",
                S::WARNING,
                Box::new(|root, r| {
                    r.re_export_cycles.push(
                        fallow_api::editor_results::ReExportCycleFinding::with_actions(
                            fallow_api::editor_results::ReExportCycle {
                                files: vec![root.join("barrel.ts")],
                                kind: fallow_api::editor_results::ReExportCycleKind::SelfLoop,
                            },
                        ),
                    );
                }),
            ),
            (
                // INTENTIONAL editor-softer deviation from the core ERROR
                // default (`RulesConfig::default().boundary_violation ==
                // Severity::Error`). FLAGGED in the agent report for a follow-up
                // decision; pinned here so the value is deliberate.
                "boundary-violation",
                S::WARNING,
                Box::new(|root, r| {
                    r.boundary_violations.push(
                        fallow_api::editor_results::BoundaryViolationFinding::with_actions(
                            fallow_api::editor_results::BoundaryViolation {
                                from_path: root.join("a.ts"),
                                to_path: root.join("b.ts"),
                                from_zone: "ui".to_string(),
                                to_zone: "data".to_string(),
                                import_specifier: "../data/db".to_string(),
                                line: 1,
                                col: 0,
                            },
                        ),
                    );
                }),
            ),
            (
                // Emits a `boundary-violation` code at WARNING too (shares the
                // softer deviation; same flag applies).
                "boundary-coverage-violation",
                S::WARNING,
                Box::new(|root, r| {
                    r.boundary_coverage_violations.push(
                        fallow_api::editor_results::BoundaryCoverageViolationFinding::with_actions(
                            fallow_api::editor_results::BoundaryCoverageViolation {
                                path: root.join("unzoned.ts"),
                                line: 1,
                                col: 0,
                            },
                        ),
                    );
                }),
            ),
            (
                "boundary-call-violation",
                S::WARNING,
                Box::new(|root, r| {
                    r.boundary_call_violations.push(
                        fallow_api::editor_results::BoundaryCallViolationFinding::with_actions(
                            fallow_api::editor_results::BoundaryCallViolation {
                                path: root.join("zoned.ts"),
                                line: 1,
                                col: 0,
                                zone: "domain".to_string(),
                                callee: "console.log".to_string(),
                                pattern: "console.*".to_string(),
                            },
                        ),
                    );
                }),
            ),
            (
                // policy-violation maps from the EFFECTIVE per-finding severity
                // (here `Warn`), so the emitted level mirrors the rule's own
                // severity rather than a fixed per-kind constant.
                "policy-violation",
                S::WARNING,
                Box::new(|root, r| {
                    r.policy_violations.push(
                        fallow_api::editor_results::PolicyViolationFinding::with_actions(
                            fallow_api::editor_results::PolicyViolation {
                                path: root.join("zoned.ts"),
                                line: 1,
                                col: 0,
                                pack: "team-policy".to_string(),
                                rule_id: "no-console".to_string(),
                                kind: fallow_api::editor_results::PolicyRuleKind::BannedCall,
                                matched: "console.log".to_string(),
                                severity: fallow_api::editor_results::PolicyViolationSeverity::Warn,
                                message: None,
                            },
                        ),
                    );
                }),
            ),
            (
                "stale-suppression",
                S::HINT,
                Box::new(|root, r| {
                    r.stale_suppressions
                        .push(fallow_api::editor_results::StaleSuppression {
                            path: root.join("a.ts"),
                            line: 1,
                            col: 0,
                            origin: fallow_api::editor_results::SuppressionOrigin::Comment {
                                issue_kind: None,
                                reason: None,
                                is_file_level: false,
                                kind_known: true,
                            },
                            missing_reason: false,
                            actions: fallow_api::editor_results::StaleSuppression::actions_for(
                                false,
                            ),
                        });
                }),
            ),
            (
                "unused-catalog-entry",
                S::WARNING,
                Box::new(|root, r| {
                    r.unused_catalog_entries.push(
                        fallow_api::editor_results::UnusedCatalogEntryFinding::with_actions(
                            fallow_api::editor_results::UnusedCatalogEntry {
                                entry_name: "react".to_string(),
                                catalog_name: "default".to_string(),
                                path: root.join("pnpm-workspace.yaml"),
                                line: 1,
                                hardcoded_consumers: vec![],
                            },
                        ),
                    );
                }),
            ),
            (
                "empty-catalog-group",
                S::WARNING,
                Box::new(|root, r| {
                    r.empty_catalog_groups.push(
                        fallow_api::editor_results::EmptyCatalogGroupFinding::with_actions(
                            fallow_api::editor_results::EmptyCatalogGroup {
                                catalog_name: "ui".to_string(),
                                path: root.join("pnpm-workspace.yaml"),
                                line: 1,
                            },
                        ),
                    );
                }),
            ),
            (
                "unresolved-catalog-reference",
                S::ERROR,
                Box::new(|root, r| {
                    r.unresolved_catalog_references.push(
                        fallow_api::editor_results::UnresolvedCatalogReferenceFinding::with_actions(
                            fallow_api::editor_results::UnresolvedCatalogReference {
                                entry_name: "vue".to_string(),
                                catalog_name: "default".to_string(),
                                path: root.join("package.json"),
                                line: 1,
                                available_in_catalogs: vec![],
                            },
                        ),
                    );
                }),
            ),
            (
                "unused-dependency-override",
                S::WARNING,
                Box::new(|root, r| {
                    r.unused_dependency_overrides.push(
                        fallow_api::editor_results::UnusedDependencyOverrideFinding::with_actions(
                            fallow_api::editor_results::UnusedDependencyOverride {
                                raw_key: "react".to_string(),
                                target_package: "react".to_string(),
                                parent_package: None,
                                version_constraint: None,
                                version_range: "18".to_string(),
                                source:
                                    fallow_api::editor_results::DependencyOverrideSource::PnpmWorkspaceYaml,
                                path: root.join("pnpm-workspace.yaml"),
                                line: 1,
                                hint: None,
                            },
                        ),
                    );
                }),
            ),
            (
                "misconfigured-dependency-override",
                S::ERROR,
                Box::new(|root, r| {
                    r.misconfigured_dependency_overrides.push(
                        fallow_api::editor_results::MisconfiguredDependencyOverrideFinding::with_actions(
                            fallow_api::editor_results::MisconfiguredDependencyOverride {
                                raw_key: "bad>".to_string(),
                                target_package: None,
                                raw_value: String::new(),
                                reason:
                                    fallow_api::editor_results::DependencyOverrideMisconfigReason::EmptyValue,
                                source:
                                    fallow_api::editor_results::DependencyOverrideSource::PnpmPackageJson,
                                path: root.join("package.json"),
                                line: 1,
                            },
                        ),
                    );
                }),
            ),
            (
                "invalid-client-export",
                S::WARNING,
                Box::new(|root, r| {
                    r.invalid_client_exports.push(
                        fallow_api::editor_results::InvalidClientExportFinding::with_actions(
                            fallow_api::editor_results::InvalidClientExport {
                                path: root.join("app/page.tsx"),
                                export_name: "metadata".to_string(),
                                directive: "use client".to_string(),
                                line: 1,
                                col: 0,
                            },
                        ),
                    );
                }),
            ),
            (
                "mixed-client-server-barrel",
                S::WARNING,
                Box::new(|root, r| {
                    r.mixed_client_server_barrels.push(
                        fallow_api::editor_results::MixedClientServerBarrelFinding::with_actions(
                            fallow_api::editor_results::MixedClientServerBarrel {
                                path: root.join("app/index.ts"),
                                client_origin: "./Button".to_string(),
                                server_origin: "./fetchUser".to_string(),
                                line: 1,
                                col: 0,
                            },
                        ),
                    );
                }),
            ),
            (
                "misplaced-directive",
                S::WARNING,
                Box::new(|root, r| {
                    r.misplaced_directives.push(
                        fallow_api::editor_results::MisplacedDirectiveFinding::with_actions(
                            fallow_api::editor_results::MisplacedDirective {
                                path: root.join("app/widget.tsx"),
                                directive: "use client".to_string(),
                                line: 1,
                                col: 0,
                            },
                        ),
                    );
                }),
            ),
            (
                "unprovided-inject",
                S::WARNING,
                Box::new(|root, r| {
                    r.unprovided_injects.push(
                        fallow_api::editor_results::UnprovidedInjectFinding::with_actions(
                            fallow_api::editor_results::UnprovidedInject {
                                path: root.join("Comp.vue"),
                                key_name: "ApiKey".to_string(),
                                framework: "vue".to_string(),
                                line: 1,
                                col: 0,
                            },
                        ),
                    );
                }),
            ),
            (
                "unrendered-component",
                S::HINT,
                Box::new(|root, r| {
                    r.unrendered_components.push(
                        fallow_api::editor_results::UnrenderedComponentFinding::with_actions(
                            fallow_api::editor_results::UnrenderedComponent {
                                path: root.join("Widget.vue"),
                                component_name: "Widget".to_string(),
                                framework: "vue".to_string(),
                                reachable_via: None,
                                line: 1,
                                col: 0,
                            },
                        ),
                    );
                }),
            ),
            (
                "unused-component-prop",
                S::HINT,
                Box::new(|root, r| {
                    r.unused_component_props.push(
                        fallow_api::editor_results::UnusedComponentPropFinding::with_actions(
                            fallow_api::editor_results::UnusedComponentProp {
                                path: root.join("Widget.vue"),
                                component_name: "Widget".to_string(),
                                prop_name: "size".to_string(),
                                line: 1,
                                col: 0,
                            },
                        ),
                    );
                }),
            ),
            (
                "unused-component-emit",
                S::HINT,
                Box::new(|root, r| {
                    r.unused_component_emits.push(
                        fallow_api::editor_results::UnusedComponentEmitFinding::with_actions(
                            fallow_api::editor_results::UnusedComponentEmit {
                                path: root.join("Widget.vue"),
                                component_name: "Widget".to_string(),
                                emit_name: "change".to_string(),
                                line: 1,
                                col: 0,
                            },
                        ),
                    );
                }),
            ),
            (
                "unused-component-input",
                S::HINT,
                Box::new(|root, r| {
                    r.unused_component_inputs.push(
                        fallow_api::editor_results::UnusedComponentInputFinding::with_actions(
                            fallow_api::editor_results::UnusedComponentInput {
                                path: root.join("widget.component.ts"),
                                component_name: "WidgetComponent".to_string(),
                                input_name: "size".to_string(),
                                line: 1,
                                col: 0,
                            },
                        ),
                    );
                }),
            ),
            (
                "unused-component-output",
                S::HINT,
                Box::new(|root, r| {
                    r.unused_component_outputs.push(
                        fallow_api::editor_results::UnusedComponentOutputFinding::with_actions(
                            fallow_api::editor_results::UnusedComponentOutput {
                                path: root.join("widget.component.ts"),
                                component_name: "WidgetComponent".to_string(),
                                output_name: "change".to_string(),
                                line: 1,
                                col: 0,
                            },
                        ),
                    );
                }),
            ),
            (
                "unused-svelte-event",
                S::HINT,
                Box::new(|root, r| {
                    r.unused_svelte_events.push(
                        fallow_api::editor_results::UnusedSvelteEventFinding::with_actions(
                            fallow_api::editor_results::UnusedSvelteEvent {
                                path: root.join("Child.svelte"),
                                component_name: "Child".to_string(),
                                event_name: "dead".to_string(),
                                line: 1,
                                col: 0,
                            },
                        ),
                    );
                }),
            ),
            (
                "unused-server-action",
                S::HINT,
                Box::new(|root, r| {
                    r.unused_server_actions.push(
                        fallow_api::editor_results::UnusedServerActionFinding::with_actions(
                            fallow_api::editor_results::UnusedServerAction {
                                path: root.join("app/actions.ts"),
                                action_name: "createUser".to_string(),
                                line: 1,
                                col: 0,
                            },
                        ),
                    );
                }),
            ),
            (
                "unused-load-data-key",
                S::HINT,
                Box::new(|root, r| {
                    r.unused_load_data_keys.push(
                        fallow_api::editor_results::UnusedLoadDataKeyFinding::with_actions(
                            fallow_api::editor_results::UnusedLoadDataKey {
                                path: root.join("src/routes/blog/+page.server.ts"),
                                key_name: "posts".to_string(),
                                line: 1,
                                col: 0,
                                route_dir: None,
                            },
                        ),
                    );
                }),
            ),
            (
                // ERROR, cross-checked against core below.
                "route-collision",
                S::ERROR,
                Box::new(|root, r| {
                    r.route_collisions.push(
                        fallow_api::editor_results::RouteCollisionFinding::with_actions(
                            fallow_api::editor_results::RouteCollision {
                                path: root.join("app/(a)/about/page.tsx"),
                                url: "/about".to_string(),
                                conflicting_paths: vec![root.join("app/(b)/about/page.tsx")],
                                line: 1,
                                col: 0,
                            },
                        ),
                    );
                }),
            ),
            (
                // ERROR, cross-checked against core below.
                "dynamic-segment-name-conflict",
                S::ERROR,
                Box::new(|root, r| {
                    r.dynamic_segment_name_conflicts.push(
                        fallow_api::editor_results::DynamicSegmentNameConflictFinding::with_actions(
                            fallow_api::editor_results::DynamicSegmentNameConflict {
                                path: root.join("app/blog/[id]/page.tsx"),
                                position: "/blog".to_string(),
                                conflicting_segments: vec![
                                    "[id]".to_string(),
                                    "[slug]".to_string(),
                                ],
                                conflicting_paths: vec![root.join("app/blog/[slug]/page.tsx")],
                                line: 1,
                                col: 0,
                            },
                        ),
                    );
                }),
            ),
        ];

        for (code, expected, build) in table {
            let got = emitted_severity(build);
            assert_eq!(
                got,
                Some(expected),
                "LSP severity for `{code}` diverged from the gate table",
            );
        }
    }

    /// Cross-check: the two kinds whose LSP severity is required to MATCH the
    /// core `RulesConfig` default both default to `Error` in core, so the gate
    /// table's ERROR expectation for them cannot silently drift from core. If a
    /// future refactor softens either core default, this fails and forces a
    /// re-think of the LSP ERROR mapping at the same time.
    #[test]
    fn route_and_dsc_match_core_error_default() {
        let rules = RulesConfig::default();
        assert_eq!(
            rules.route_collision,
            Severity::Error,
            "core route-collision default changed; the LSP ERROR mapping must be revisited",
        );
        assert_eq!(
            rules.dynamic_segment_name_conflict,
            Severity::Error,
            "core dynamic-segment-name-conflict default changed; the LSP ERROR mapping must be revisited",
        );
    }
}
