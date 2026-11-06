//! CodeClimate issues of an audit run. The CI comment and review renderers
//! take the typed issues.

use std::process::ExitCode;

use fallow_api::AuditCodeClimateOutputInput;

use super::AuditResult;
use crate::report;

pub(super) fn print_audit_codeclimate(result: &AuditResult) -> ExitCode {
    let value = build_audit_codeclimate(result);
    report::emit_json(&value, "CodeClimate audit")
}

pub(super) fn build_audit_codeclimate(result: &AuditResult) -> serde_json::Value {
    fallow_output::codeclimate_issues_to_value(&build_audit_codeclimate_issues(result))
}

/// Typed audit CodeClimate issues. The CI comment and review renderers take
/// these, so the legacy fingerprint of a dead-code issue reaches them.
pub(super) fn build_audit_codeclimate_issues(
    result: &AuditResult,
) -> Vec<fallow_output::CodeClimateIssue> {
    fallow_api::build_audit_codeclimate_issues(AuditCodeClimateOutputInput {
        dead_code: result.check.as_ref().map_or_else(Vec::new, |check| {
            fallow_api::build_codeclimate(&check.results, &check.config.root, &check.config.rules)
        }),
        duplication: result.dupes.as_ref().map_or_else(Vec::new, |dupes| {
            let mut issues =
                fallow_api::build_duplication_codeclimate(&dupes.report, &dupes.config.root);
            if result.base_snapshot.is_some()
                && let Some(comparison) = result.comparison.as_ref()
            {
                fallow_api::set_duplication_audit_attribution(
                    &mut issues,
                    &dupes.report,
                    comparison.dupes.introduced(),
                );
            }
            issues
        }),
        health: result.health.as_ref().map_or_else(Vec::new, |health| {
            fallow_api::build_health_codeclimate(&health.report, &health.config.root)
        }),
        diff_risk: fallow_api::build_diff_risk_codeclimate(&result.diff_risk),
    })
}
