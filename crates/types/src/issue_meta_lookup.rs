//! Lookups over the issue metadata tables.

use super::{
    CODECLIMATE_RESULT_CODES, ISSUE_KIND_META, ISSUE_RESULT_META, IssueKind, IssueKindMeta,
    IssueResultMeta,
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

/// Lookup metadata by canonical code or alias.
#[must_use]
pub fn issue_meta_for_token(token: &str) -> Option<&'static IssueKindMeta> {
    ISSUE_KIND_META
        .iter()
        .find(|meta| meta.code == token || meta.aliases.contains(&token))
}

/// Lookup metadata by any shared contract token: canonical code, alias,
/// config key, MCP selector, suppression token, or CLI filter flag.
#[must_use]
pub fn issue_meta_for_contract_token(token: &str) -> Option<&'static IssueKindMeta> {
    let normalized = token.trim().trim_start_matches("--").replace('_', "-");
    ISSUE_KIND_META
        .iter()
        .find(|meta| issue_meta_matches_contract_token(meta, &normalized))
}

/// Whether a metadata row owns the provided shared contract token.
#[must_use]
pub fn issue_meta_matches_contract_token(meta: &IssueKindMeta, token: &str) -> bool {
    let normalized = token.trim().trim_start_matches("--").replace('_', "-");
    meta.code == normalized
        || meta.aliases.contains(&normalized.as_str())
        || meta.config_key == Some(normalized.as_str())
        || meta.mcp_issue_type == Some(normalized.as_str())
        || meta.suppress_token == Some(normalized.as_str())
        || meta.filter_flag.map(|flag| flag.trim_start_matches("--")) == Some(normalized.as_str())
}

/// Lookup metadata by backing issue kind.
#[must_use]
pub fn issue_meta_by_kind(kind: IssueKind) -> Option<&'static IssueKindMeta> {
    ISSUE_KIND_META.iter().find(|meta| meta.kind == Some(kind))
}

/// Lookup serialized result metadata by canonical issue code.
#[must_use]
pub fn issue_result_meta_by_code(code: &str) -> Option<&'static IssueResultMeta> {
    ISSUE_RESULT_META.iter().find(|meta| meta.code == code)
}
