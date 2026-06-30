use std::path::Path;
use std::process::ExitCode;

use fallow_config::RulesConfig;
#[cfg(test)]
use fallow_config::Severity;
use fallow_output::{SarifRuleInput, build_sarif_rule};
use fallow_types::duplicates::DuplicationReport;
use fallow_types::results::AnalysisResults;

use super::emit_json;
use super::grouping::{self, OwnershipResolver};
use crate::explain;

#[cfg(test)]
fn configured_sarif_level(s: Severity) -> &'static str {
    match s {
        Severity::Error => "error",
        Severity::Warn => "warning",
        Severity::Off => "none",
    }
}

/// Build a SARIF rule definition with optional `fullDescription` and `helpUri`
/// sourced from the centralized explain module.
fn sarif_rule(id: &str, fallback_short: &str, level: &str) -> serde_json::Value {
    let def = explain::rule_by_id(id);
    let short_description = def.map_or(fallback_short, |def| def.short);
    let full_description = def.map(|def| def.full);
    let help_uri = def.map(explain::rule_docs_url);
    build_sarif_rule(SarifRuleInput {
        id,
        short_description,
        level,
        full_description,
        help_uri: help_uri.as_deref(),
    })
}

#[must_use]
pub fn api_sarif_document(
    results: &AnalysisResults,
    root: &Path,
    rules: &RulesConfig,
) -> serde_json::Value {
    fallow_api::build_sarif(results, root, rules, &sarif_rule)
}

pub(super) fn print_sarif(results: &AnalysisResults, root: &Path, rules: &RulesConfig) -> ExitCode {
    let sarif = api_sarif_document(results, root, rules);
    emit_json(&sarif, "SARIF")
}

/// Print SARIF output with owner properties added to each result.
pub(super) fn print_grouped_sarif(
    results: &AnalysisResults,
    root: &Path,
    rules: &RulesConfig,
    resolver: &OwnershipResolver,
) -> ExitCode {
    let mut sarif = api_sarif_document(results, root, rules);
    fallow_api::annotate_sarif_results(&mut sarif, "owner", |uri| {
        let decoded = uri.replace("%5B", "[").replace("%5D", "]");
        grouping::resolve_owner(Path::new(&decoded), Path::new(""), resolver)
    });

    emit_json(&sarif, "SARIF")
}

pub(super) fn print_duplication_sarif(report: &DuplicationReport, root: &Path) -> ExitCode {
    let sarif = fallow_api::build_duplication_sarif(report, root, &sarif_rule);
    emit_json(&sarif, "SARIF")
}

pub(super) fn print_grouped_duplication_sarif(
    report: &DuplicationReport,
    root: &Path,
    resolver: &OwnershipResolver,
) -> ExitCode {
    let sarif = fallow_api::build_grouped_duplication_sarif(report, root, &sarif_rule, |group| {
        super::dupes_grouping::largest_owner(group, root, resolver)
    });
    emit_json(&sarif, "SARIF")
}

#[must_use]
pub fn api_health_sarif_document(
    report: &fallow_output::HealthReport,
    root: &Path,
) -> serde_json::Value {
    fallow_api::build_health_sarif(report, root, &sarif_rule)
}

pub(super) fn print_health_sarif(report: &fallow_output::HealthReport, root: &Path) -> ExitCode {
    let sarif = api_health_sarif_document(report, root);
    emit_json(&sarif, "SARIF")
}

pub(super) fn print_grouped_health_sarif(
    report: &fallow_output::HealthReport,
    root: &Path,
    resolver: &OwnershipResolver,
) -> ExitCode {
    let mut sarif = api_health_sarif_document(report, root);
    fallow_api::annotate_sarif_results(&mut sarif, "group", |uri| {
        let decoded = uri.replace("%5B", "[").replace("%5D", "]");
        grouping::resolve_owner(Path::new(&decoded), Path::new(""), resolver)
    });

    emit_json(&sarif, "SARIF")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn configured_sarif_level_keeps_off_rules_in_rule_table() {
        assert_eq!(configured_sarif_level(Severity::Error), "error");
        assert_eq!(configured_sarif_level(Severity::Warn), "warning");
        assert_eq!(configured_sarif_level(Severity::Off), "none");
    }

    #[test]
    fn sarif_rule_uses_fallback_for_unknown_rule() {
        let rule = sarif_rule("fallow/nonexistent", "fallback text", "warning");
        assert_eq!(rule["id"], "fallow/nonexistent");
        assert_eq!(rule["shortDescription"]["text"], "fallback text");
        assert!(rule.get("fullDescription").is_none());
        assert!(rule.get("helpUri").is_none());
    }
}
