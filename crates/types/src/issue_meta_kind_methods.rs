//! Accessor methods of `IssueKindMeta`.

use super::*;

impl IssueKindMeta {
    /// Return the filter flag as an MCP selector pair.
    #[must_use]
    pub const fn mcp_pair(self) -> Option<(&'static str, &'static str)> {
        match (self.mcp_issue_type, self.filter_flag) {
            (Some(issue_type), Some(flag)) => Some((issue_type, flag)),
            _ => None,
        }
    }

    /// Whether this row owns a serialized dead-code result contract.
    #[must_use]
    pub fn has_result_contract(self) -> bool {
        issue_result_meta_by_code(self.code).is_some()
    }

    /// SARIF rule ids used by CI formatters for this issue row.
    #[must_use]
    pub fn sarif_rule_ids(self) -> Vec<String> {
        issue_sarif_rule_ids(self.code)
    }

    /// CodeClimate check names used by CI formatters for this issue row.
    #[must_use]
    pub fn codeclimate_check_names(self) -> Vec<String> {
        issue_codeclimate_check_names(self.code)
    }

    /// Documentation anchor under `/explanations/dead-code`.
    #[must_use]
    pub fn docs_anchor(self) -> Option<&'static str> {
        issue_docs_anchor(self.code)
    }

    /// Published TypeScript backwards-compat alias policy.
    #[must_use]
    pub fn ts_alias(self) -> Option<TsAliasMeta> {
        issue_ts_alias(self.code)
    }
}
