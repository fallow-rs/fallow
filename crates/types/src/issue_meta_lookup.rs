//! Lookups over the issue metadata tables.

use super::{
    CODECLIMATE_RESULT_CODES, ISSUE_KIND_META, ISSUE_RESULT_META, IssueKindMeta, IssueResultMeta,
    issue_result_meta_by_code,
};

/// SARIF rule ids used by CI formatters for a canonical issue code.
#[must_use]
pub fn issue_sarif_rule_ids(code: &str) -> Vec<String> {
    let mut ids = vec![format!("fallow/{code}")];
    if code == "stale-suppression" {
        ids.push("fallow/missing-suppression-reason".to_string());
    }
    ids
}

/// CodeClimate check names used by CI formatters for a canonical issue code.
#[must_use]
pub fn issue_codeclimate_check_names(code: &str) -> Vec<String> {
    if !CODECLIMATE_RESULT_CODES.contains(&code) {
        return Vec::new();
    }
    issue_sarif_rule_ids(code)
}

/// Documentation anchor under `/explanations/dead-code` for a canonical issue
/// code.
#[must_use]
pub fn issue_docs_anchor(code: &str) -> Option<&'static str> {
    issue_result_meta_by_code(code).map(|meta| meta.docs_anchor)
}

/// Rows exposed by the LSP issue-type capability.
pub fn diagnostic_issue_metas() -> impl Iterator<Item = &'static IssueKindMeta> {
    ISSUE_KIND_META.iter().filter(|meta| meta.lsp)
}

/// Rows that map to a serialized `AnalysisResults` array.
pub fn result_issue_metas() -> impl Iterator<Item = &'static IssueResultMeta> {
    ISSUE_RESULT_META.iter()
}

/// Rows whose serialized `AnalysisResults` array contributes to `total_issues`.
pub fn counted_result_issue_metas() -> impl Iterator<Item = &'static IssueResultMeta> {
    result_issue_metas().filter(|meta| meta.counts_in_total)
}
