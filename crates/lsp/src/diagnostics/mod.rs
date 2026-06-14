mod quality;
pub mod security;
mod structural;
mod unused;

use rustc_hash::FxHashMap;
use std::path::Path;

use ls_types::{CodeDescription, Diagnostic, Position, Range, Uri};

use fallow_core::duplicates::DuplicationReport;
use fallow_core::results::AnalysisResults;

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
pub fn build_diagnostics(
    results: &AnalysisResults,
    duplication: &DuplicationReport,
    root: &Path,
) -> FxHashMap<Uri, Vec<Diagnostic>> {
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
mod tests {
    use super::*;
    use std::path::PathBuf;

    use fallow_core::duplicates::{DuplicationReport, DuplicationStats};
    use fallow_core::results::{
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

        let diags = build_diagnostics(&results, &duplication, &root);
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
        let diags = build_diagnostics(&results, &duplication, &root);

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
        let diags = build_diagnostics(&results, &duplication, &root);

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
            .push(fallow_core::results::SecurityFinding {
                finding_id: String::new(),
                candidate: fallow_core::results::SecurityCandidate::default(),
                taint_flow: None,
                attack_surface: None,
                kind: fallow_core::results::SecurityFindingKind::TaintedSink,
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
        let diags = build_diagnostics(&results, &duplication, &root);
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
