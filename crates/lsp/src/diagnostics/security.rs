//! LSP diagnostics for security CANDIDATES (issue #891).
//!
//! Surfaces `AnalysisResults.security_findings` as opt-in editor squiggles. The
//! findings are CANDIDATES for downstream verification, never proven
//! vulnerabilities, so the LSP diagnostic severity is fixed at `INFORMATION`.
//! The candidate review-priority tier is carried in `Diagnostic.data.security`,
//! not mapped onto LSP severity.
//!
//! Opt-in is automatic: both `security-sink` and `security-client-server-leak`
//! rules default to `off`, the LSP reuses the project config, so
//! `security_findings` is empty (and this block produces zero diagnostics)
//! unless the user raises a rule to `warn`/`error` in their fallow config.

use rustc_hash::FxHashMap;

use ls_types::{
    CodeDescription, Diagnostic, DiagnosticSeverity, NumberOrString, Position, Range, Uri,
};

use fallow_api::EditorAnalysisResults as AnalysisResults;
use fallow_api::editor_results::{SecurityFinding, SecurityFindingKind};

use crate::position::PositionMapper;

/// Documentation page for the security candidate surface. The dead-code
/// `DOCS_BASE` in `super` points at the dead-code explanation; security has its
/// own CLI page, so this block uses a dedicated link.
const SECURITY_DOCS_URL: &str = "https://docs.fallow.tools/cli/security";

/// The `// fallow-ignore-file` / `// fallow-ignore-next-line` suppression token
/// for a security finding's kind. One token per kind covers all catalogue
/// categories (mirrors `IssueKind::SecuritySink` / `SecurityClientServerLeak`).
pub fn security_token(kind: SecurityFindingKind) -> &'static str {
    match kind {
        SecurityFindingKind::TaintedSink => "security-sink",
        SecurityFindingKind::ClientServerLeak => "security-client-server-leak",
    }
}

/// Human-facing label for a security candidate. Mirrors
/// `crates/cli/src/security.rs::security_finding_label`: `client-server-leak`
/// for the leak rule, `<catalogue title> (CWE-N)` for a catalogue sink.
pub fn security_label(finding: &SecurityFinding) -> String {
    match finding.kind {
        SecurityFindingKind::ClientServerLeak => "client-server-leak".to_string(),
        SecurityFindingKind::TaintedSink => {
            let title = finding
                .category
                .as_deref()
                .and_then(fallow_api::editor_security::security_catalogue_title)
                .or(finding.category.as_deref())
                .unwrap_or("tainted-sink");
            match finding.cwe {
                Some(cwe) => format!("{title} (CWE-{cwe})"),
                None => title.to_string(),
            }
        }
    }
}

/// Build a `CodeDescription` linking to the security candidate docs page.
fn security_doc_link() -> Option<CodeDescription> {
    SECURITY_DOCS_URL
        .parse::<Uri>()
        .ok()
        .map(|href| CodeDescription { href })
}

/// Structured `Diagnostic.data` payload so agents reading `getDiagnostics()` can
/// triage a candidate (source-backed? entry-reachable? blast radius?) without a
/// hover round-trip or a CLI re-run. Mirrors the `circularDependency` data
/// precedent in `structural.rs`; `attach_changed_since_data` merges
/// `changedSince` into this object rather than clobbering it.
fn security_data(finding: &SecurityFinding) -> serde_json::Value {
    let kind = match finding.kind {
        SecurityFindingKind::ClientServerLeak => "client-server-leak",
        SecurityFindingKind::TaintedSink => "tainted-sink",
    };
    let reach = finding.reachability.as_ref();
    serde_json::json!({
        "security": {
            "kind": kind,
            "category": finding.category,
            "cwe": finding.cwe,
            "severity": finding.severity,
            "sourceBacked": finding.source_backed,
            "reachableFromEntry": reach.map(|r| r.reachable_from_entry),
            "taintConfidence": reach.and_then(|r| r.taint_confidence),
            "blastRadius": reach.map(|r| r.blast_radius),
            "crossesBoundary": reach.map(|r| r.crosses_boundary),
        }
    })
}

/// Build the `Diagnostic` for a single security candidate. Shared by
/// `push_security_diagnostics` and the suppress code action so the published
/// diagnostic and the action's linked diagnostic correlate exactly (range +
/// message + code). `message` is plain text per the LSP spec, NOT
/// markdown-escaped.
pub fn security_diagnostic(finding: &SecurityFinding, mapper: &mut PositionMapper) -> Diagnostic {
    let line = finding.line.saturating_sub(1);
    let col = mapper.utf16_col(&finding.path, line, finding.col);
    let label = security_label(finding);
    Diagnostic {
        range: Range {
            start: Position {
                line,
                character: col,
            },
            end: Position {
                line,
                character: u32::MAX,
            },
        },
        severity: Some(DiagnosticSeverity::INFORMATION),
        source: Some("fallow".to_string()),
        code: Some(NumberOrString::String(
            security_token(finding.kind).to_string(),
        )),
        code_description: security_doc_link(),
        message: format!("Security candidate ({label}): {}", finding.evidence),
        data: Some(security_data(finding)),
        ..Default::default()
    }
}

/// Push one INFORMATION diagnostic per security candidate, keyed by the
/// finding's absolute-path URI (paths are absolute internally; no `root.join`).
pub fn push_security_diagnostics(
    map: &mut FxHashMap<Uri, Vec<Diagnostic>>,
    results: &AnalysisResults,
    mapper: &mut PositionMapper,
) {
    for finding in &results.security_findings {
        let Some(uri) = Uri::from_file_path(&finding.path) else {
            continue;
        };
        map.entry(uri)
            .or_default()
            .push(security_diagnostic(finding, mapper));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    use fallow_api::editor_results::{
        SecurityFinding, SecurityFindingKind, SecurityReachability, SecuritySeverity,
        TaintConfidence,
    };

    fn test_root() -> PathBuf {
        if cfg!(windows) {
            PathBuf::from("C:\\project")
        } else {
            PathBuf::from("/project")
        }
    }

    fn tainted_sink(path: PathBuf) -> SecurityFinding {
        SecurityFinding {
            finding_id: String::new(),
            candidate: fallow_api::editor_results::SecurityCandidate::default(),
            taint_flow: None,
            attack_surface: None,
            kind: SecurityFindingKind::TaintedSink,
            category: Some("dangerous-html".to_string()),
            cwe: Some(79),
            path,
            line: 12,
            col: 4,
            evidence: "user input flows into dangerouslySetInnerHTML".to_string(),
            source_backed: true,
            source_read: None,
            severity: SecuritySeverity::High,
            trace: vec![],
            actions: vec![],
            dead_code: None,
            reachability: Some(SecurityReachability {
                reachable_from_entry: true,
                reachable_from_untrusted_source: false,
                taint_confidence: Some(TaintConfidence::ArgLevel),
                untrusted_source_hop_count: None,
                untrusted_source_trace: vec![],
                blast_radius: 3,
                crosses_boundary: true,
            }),
            runtime: None,
        }
    }

    fn client_server_leak(path: PathBuf) -> SecurityFinding {
        SecurityFinding {
            finding_id: String::new(),
            candidate: fallow_api::editor_results::SecurityCandidate::default(),
            taint_flow: None,
            attack_surface: None,
            kind: SecurityFindingKind::ClientServerLeak,
            category: None,
            cwe: None,
            path,
            line: 1,
            col: 0,
            evidence: "client boundary reaches process.env.SECRET_KEY".to_string(),
            source_backed: false,
            source_read: None,
            severity: SecuritySeverity::Low,
            trace: vec![],
            actions: vec![],
            dead_code: None,
            reachability: None,
            runtime: None,
        }
    }

    #[test]
    fn tainted_sink_produces_information_diagnostic() {
        let root = test_root();
        let path = root.join("src/render.ts");
        let mut results = AnalysisResults::default();
        results.security_findings.push(tainted_sink(path.clone()));

        let mut map = FxHashMap::default();
        let mut mapper = PositionMapper::default();
        push_security_diagnostics(&mut map, &results, &mut mapper);

        let uri = Uri::from_file_path(&path).unwrap();
        let diags = map.get(&uri).expect("security diagnostic present");
        assert_eq!(diags.len(), 1);
        let d = &diags[0];
        assert_eq!(d.severity, Some(DiagnosticSeverity::INFORMATION));
        assert_eq!(d.source, Some("fallow".to_string()));
        assert_eq!(
            d.code,
            Some(NumberOrString::String("security-sink".to_string()))
        );
        assert_eq!(d.range.start.line, 11); // 1-based 12 -> 0-based 11
        assert_eq!(d.range.start.character, 4);
        assert!(d.message.contains("Security candidate"));
        assert!(d.message.contains("CWE-79"));
        assert!(d.message.contains("dangerouslySetInnerHTML"));
    }

    #[test]
    fn client_server_leak_uses_its_own_token() {
        let root = test_root();
        let path = root.join("src/leak.ts");
        let mut results = AnalysisResults::default();
        results
            .security_findings
            .push(client_server_leak(path.clone()));

        let mut map = FxHashMap::default();
        let mut mapper = PositionMapper::default();
        push_security_diagnostics(&mut map, &results, &mut mapper);

        let uri = Uri::from_file_path(&path).unwrap();
        let d = &map.get(&uri).expect("diagnostic present")[0];
        assert_eq!(
            d.code,
            Some(NumberOrString::String(
                "security-client-server-leak".to_string()
            ))
        );
    }

    #[test]
    fn empty_findings_produce_no_diagnostics() {
        let results = AnalysisResults::default();
        let mut map = FxHashMap::default();
        let mut mapper = PositionMapper::default();
        push_security_diagnostics(&mut map, &results, &mut mapper);
        assert!(map.is_empty());
    }

    #[test]
    fn diagnostic_data_carries_triage_facts() {
        let root = test_root();
        let path = root.join("src/render.ts");
        let finding = tainted_sink(path);
        let mut mapper = PositionMapper::default();
        let d = security_diagnostic(&finding, &mut mapper);
        let data = d.data.expect("data present");
        let sec = &data["security"];
        assert_eq!(sec["kind"], serde_json::json!("tainted-sink"));
        assert_eq!(sec["category"], serde_json::json!("dangerous-html"));
        assert_eq!(sec["cwe"], serde_json::json!(79));
        assert_eq!(sec["severity"], serde_json::json!("high"));
        assert_eq!(sec["sourceBacked"], serde_json::json!(true));
        assert_eq!(sec["reachableFromEntry"], serde_json::json!(true));
        assert_eq!(sec["taintConfidence"], serde_json::json!("arg-level"));
        assert_eq!(sec["blastRadius"], serde_json::json!(3));
        assert_eq!(sec["crossesBoundary"], serde_json::json!(true));
    }

    #[test]
    fn diagnostic_data_null_reachability_when_absent() {
        let root = test_root();
        let finding = client_server_leak(root.join("src/leak.ts"));
        let mut mapper = PositionMapper::default();
        let d = security_diagnostic(&finding, &mut mapper);
        let sec = &d.data.expect("data present")["security"];
        assert_eq!(sec["reachableFromEntry"], serde_json::Value::Null);
        assert_eq!(sec["taintConfidence"], serde_json::Value::Null);
        assert_eq!(sec["blastRadius"], serde_json::Value::Null);
        assert_eq!(sec["sourceBacked"], serde_json::json!(false));
    }

    #[test]
    fn token_matches_suppression_kinds() {
        assert_eq!(
            security_token(SecurityFindingKind::TaintedSink),
            "security-sink"
        );
        assert_eq!(
            security_token(SecurityFindingKind::ClientServerLeak),
            "security-client-server-leak"
        );
    }
}
