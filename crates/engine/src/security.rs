//! Security metadata helpers owned by the engine boundary.
//!
//! The severity policy, the catalogue-title lookup, the rule and finding
//! identifiers, and the rule-enabling policy are shared with the core analyze
//! ranking pass via fallow-security so the CLI, SARIF, JSON, and viz Security
//! surfaces can never diverge.

use std::path::{Path, PathBuf};

use fallow_types::results::{AnalysisResults, SecurityFinding};
use rustc_hash::FxHashMap;

pub use fallow_security::{
    derive_security_severity, enable_security_rules, resolve_security_finding_severity,
    security_catalogue_title, security_rule_id, security_rules_can_error,
};

/// Set the `finding_id` and the `stable_finding_id` of each security finding
/// in `results`.
///
/// The stable id hashes the normalized text of the exact sink expression. This
/// function reads each anchor file once from disk, and gets the text of each
/// sink span from [`fallow_extract::sink_identity_text`]. A finding whose file
/// cannot be read gets no stable id.
///
/// The candidates in `results.suppressed_security_findings` get no id, but
/// they keep their tiebreak position. This function then clears that list.
pub fn stamp_security_finding_ids(results: &mut AnalysisResults, root: &Path) {
    let suppressed = std::mem::take(&mut results.suppressed_security_findings);
    stamp_findings(&mut results.security_findings, &suppressed, root);
}

fn stamp_findings(findings: &mut [SecurityFinding], suppressed: &[SecurityFinding], root: &Path) {
    let mut sources: FxHashMap<PathBuf, Option<String>> = FxHashMap::default();
    fallow_security::stamp_security_finding_ids(findings, suppressed, root, |path, start, end| {
        let source = sources
            .entry(path.to_path_buf())
            .or_insert_with_key(|path| std::fs::read_to_string(path).ok());
        fallow_extract::sink_identity_text(path, source.as_deref()?, start, end)
    });
}
