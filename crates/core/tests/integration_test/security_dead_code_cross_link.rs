//! Integration tests for security sink dead-code cross-links (#884).

use fallow_config::Severity;
use fallow_core::results::{
    AnalysisResults, SecurityDeadCodeKind, SecurityFinding, SecurityFindingKind,
};
use fallow_types::output::{FixActionType, IssueAction};

use super::common::{create_config_with_rules, fixture_path};

fn analyze_with_security_and_dead_code() -> AnalysisResults {
    let root = fixture_path("security-dead-code-cross-link");
    let config = create_config_with_rules(root, |rules| {
        rules.security_sink = Severity::Warn;
        rules.unused_files = Severity::Warn;
        rules.unused_exports = Severity::Warn;
    });
    fallow_core::analyze(&config).expect("analysis should succeed")
}

fn sink_for<'a>(results: &'a AnalysisResults, suffix: &str) -> &'a SecurityFinding {
    results
        .security_findings
        .iter()
        .find(|finding| {
            matches!(finding.kind, SecurityFindingKind::TaintedSink)
                && finding
                    .path
                    .to_string_lossy()
                    .replace('\\', "/")
                    .ends_with(suffix)
        })
        .unwrap_or_else(|| panic!("{suffix} should produce a security sink finding"))
}

#[test]
fn unused_file_sink_gets_delete_context() {
    let results = analyze_with_security_and_dead_code();
    let finding = sink_for(&results, "src/dead-file.ts");
    let dead_code = finding.dead_code.as_ref().expect("dead-code context");

    assert_eq!(dead_code.kind, SecurityDeadCodeKind::UnusedFile);
    assert_eq!(dead_code.export_name, None);
    assert!(finding.actions.first().is_some_and(|action| {
        matches!(action, IssueAction::Fix(fix) if fix.kind == FixActionType::DeleteFile)
    }));
}

#[test]
fn unused_export_sink_gets_remove_export_context() {
    let results = analyze_with_security_and_dead_code();
    let finding = sink_for(&results, "src/unused-export.ts");
    let dead_code = finding.dead_code.as_ref().expect("dead-code context");

    assert_eq!(dead_code.kind, SecurityDeadCodeKind::UnusedExport);
    assert_eq!(dead_code.export_name.as_deref(), Some("dangerous"));
    assert!(finding.actions.first().is_some_and(|action| {
        matches!(action, IssueAction::Fix(fix) if fix.kind == FixActionType::RemoveExport)
    }));
}

#[test]
fn active_sink_has_no_dead_code_context() {
    let results = analyze_with_security_and_dead_code();
    let finding = sink_for(&results, "src/active.ts");

    assert_eq!(finding.dead_code, None);
}

#[test]
fn active_sink_sorts_before_dead_code_sinks() {
    let results = analyze_with_security_and_dead_code();
    let paths = results
        .security_findings
        .iter()
        .filter(|finding| matches!(finding.kind, SecurityFindingKind::TaintedSink))
        .map(|finding| finding.path.to_string_lossy().replace('\\', "/"))
        .collect::<Vec<_>>();

    let active_index = paths
        .iter()
        .position(|path| path.ends_with("src/active.ts"))
        .expect("active sink should be present");
    let first_dead_index = paths
        .iter()
        .position(|path| {
            path.ends_with("src/dead-file.ts") || path.ends_with("src/unused-export.ts")
        })
        .expect("dead-code sink should be present");

    assert!(active_index < first_dead_index);
}
