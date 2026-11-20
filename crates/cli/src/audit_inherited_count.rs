//! The inherited findings that the audit gate leaves out, and the noun of
//! the pass status line that follows from them.

use fallow_config::{AuditGate, AuditWorsened};

use super::super::AuditResult;

/// The inherited findings that the audit gate left out. Zero under
/// `--gate all`, because that gate judges every finding.
pub(super) fn gate_excluded_inherited(result: &AuditResult) -> usize {
    if matches!(result.attribution.gate, AuditGate::All) {
        return 0;
    }
    // The same classification gives `complexity_worsened` and
    // `complexity_inherited`, and every worsened finding is also counted as
    // inherited. Under `--worsened error` the worsened findings take part in
    // the gate, so they are not excluded.
    let gated_worsened = if result.attribution.worsened == AuditWorsened::Error {
        result.attribution.complexity_worsened
    } else {
        0
    };
    (result.attribution.dead_code_inherited
        + result.attribution.complexity_inherited
        + result.attribution.duplication_inherited)
        .saturating_sub(gated_worsened)
}

/// `issues` for the pass status line, or `introduced issues` when the gate
/// left out inherited findings that the output lists above the line.
pub(super) fn pass_issue_noun(result: &AuditResult) -> &'static str {
    if gate_excluded_inherited(result) > 0 {
        "introduced issues"
    } else {
        "issues"
    }
}
