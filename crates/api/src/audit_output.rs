//! Shared audit JSON payload contracts for programmatic consumers.

use fallow_config::AuditGate;
use serde::Serialize;

/// Verdict for the audit command.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum AuditVerdict {
    /// No issues in changed files.
    Pass,
    /// Issues found, but all are warn-severity.
    Warn,
    /// Error-severity issues found in changed files.
    Fail,
}

/// Per-category summary counts for the audit result.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct AuditSummary {
    pub dead_code_issues: usize,
    pub dead_code_has_errors: bool,
    pub complexity_findings: usize,
    pub max_cyclomatic: Option<u16>,
    pub duplication_clone_groups: usize,
}

/// New-vs-inherited issue counts for audit.
#[derive(Debug, Default, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct AuditAttribution {
    pub gate: AuditGate,
    pub dead_code_introduced: usize,
    pub dead_code_inherited: usize,
    pub complexity_introduced: usize,
    pub complexity_inherited: usize,
    pub duplication_introduced: usize,
    pub duplication_inherited: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audit_verdict_uses_snake_case_wire_names() {
        let value = serde_json::to_value(AuditVerdict::Pass).expect("serialize verdict");
        assert_eq!(value, serde_json::json!("pass"));
    }
}
