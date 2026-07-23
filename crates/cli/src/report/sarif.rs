use std::collections::BTreeMap;
use std::path::Path;
use std::process::ExitCode;

use fallow_config::RulesConfig;
#[cfg(test)]
use fallow_config::Severity;
use fallow_output::{
    SarifDocumentInput, SarifResultInput, SarifRuleInput, build_sarif_document, build_sarif_result,
    build_sarif_rule,
};
use fallow_types::duplicates::DuplicationReport;
use fallow_types::results::AnalysisResults;

use super::emit_json;
use super::github::{AnnotationLevel, resolve_render_options};
use super::github_annotations::{EnvelopeKind, collect_annotations};
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

/// Attach semantic provenance to a SARIF run without manufacturing findings.
pub fn annotate_type_aware_sarif(
    sarif: &mut serde_json::Value,
    type_aware: Option<&fallow_types::envelope::TypeAwareMeta>,
) {
    let Some(type_aware) = type_aware else {
        return;
    };
    let value = serde_json::to_value(type_aware).unwrap_or(serde_json::Value::Null);
    annotate_type_aware_sarif_value(sarif, &value);
}

fn annotate_type_aware_sarif_value(sarif: &mut serde_json::Value, type_aware: &serde_json::Value) {
    let Some(run) = sarif
        .get_mut("runs")
        .and_then(serde_json::Value::as_array_mut)
        .and_then(|runs| runs.first_mut())
        .and_then(serde_json::Value::as_object_mut)
    else {
        return;
    };
    let properties = run
        .entry("properties")
        .or_insert_with(|| serde_json::json!({}));
    if let Some(properties) = properties.as_object_mut() {
        properties.insert("typeAware".to_string(), type_aware.clone());
    }
    let notifications = type_aware
        .get("queries")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter(|query| query.get("status").and_then(serde_json::Value::as_str) != Some("complete"))
        .map(|query| {
            let capability = query
                .get("capability")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("unknown");
            let status = query
                .get("status")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("unavailable");
            let assertion = query
                .get("assertion")
                .and_then(serde_json::Value::as_str)
                .unwrap_or("no-safe-assertion");
            serde_json::json!({
                "level": "warning",
                "message": {
                    "text": format!(
                        "Type-aware {capability} was {status}: {assertion}"
                    )
                },
                "properties": {
                    "capability": capability,
                    "status": status,
                    "reasonCode": query.get("reason_code"),
                    "actions": query.get("actions"),
                    "omissions": query.get("omissions")
                }
            })
        })
        .collect::<Vec<_>>();
    if notifications.is_empty() {
        return;
    }
    let invocations = run
        .entry("invocations")
        .or_insert_with(|| serde_json::json!([{"executionSuccessful": true}]));
    if let Some(invocation) = invocations
        .as_array_mut()
        .and_then(|invocations| invocations.first_mut())
        .and_then(serde_json::Value::as_object_mut)
    {
        invocation.insert(
            "toolExecutionNotifications".to_string(),
            serde_json::Value::Array(notifications),
        );
    }
}

/// Re-render a stored JSON envelope as SARIF without repeating analysis.
pub fn print_envelope_sarif(
    kind: EnvelopeKind,
    envelope: &serde_json::Value,
    root: &Path,
) -> ExitCode {
    let sarif = envelope_sarif_document(kind, envelope, root);
    emit_json(&sarif, "SARIF")
}

fn envelope_sarif_document(
    kind: EnvelopeKind,
    envelope: &serde_json::Value,
    root: &Path,
) -> serde_json::Value {
    let options = resolve_render_options(root);
    let annotations = collect_annotations(kind, envelope, options.pm);
    let mut rules = BTreeMap::<String, serde_json::Value>::new();
    let mut results = Vec::with_capacity(annotations.len());

    for annotation in annotations {
        let rule_id = annotation_rule_id(&annotation.title);
        let level = annotation_level(annotation.level);
        rules.entry(rule_id.clone()).or_insert_with(|| {
            build_sarif_rule(SarifRuleInput {
                id: &rule_id,
                short_description: &annotation.title,
                level,
                full_description: None,
                help_uri: None,
            })
        });

        let uri = options.rebase.apply(&annotation.path);
        let region = annotation.line.map(|line| {
            let line = line.clamp(1, u64::from(u32::MAX)) as u32;
            let col = annotation.col.unwrap_or(1).clamp(1, u64::from(u32::MAX)) as u32;
            (line, col)
        });
        let message = format!("{}: {}", annotation.title, annotation.message);
        let mut result = build_sarif_result(SarifResultInput {
            rule_id: &rule_id,
            level,
            message: &message,
            uri: &uri,
            region,
            snippet: None,
        });
        if let Some(end_line) = annotation.end_line
            && let Some(region) = result
                .pointer_mut("/locations/0/physicalLocation/region")
                .and_then(serde_json::Value::as_object_mut)
        {
            region.insert(
                "endLine".to_string(),
                serde_json::json!(end_line.clamp(1, u64::from(u32::MAX))),
            );
        }
        results.push(result);
    }

    let rules = rules.into_values().collect::<Vec<_>>();
    let mut sarif = build_sarif_document(SarifDocumentInput {
        results: &results,
        rules: &rules,
        tool_version: env!("CARGO_PKG_VERSION"),
    });
    if let Some(type_aware) = envelope_type_aware(envelope) {
        annotate_type_aware_sarif_value(&mut sarif, type_aware);
    }
    sarif
}

fn annotation_rule_id(title: &str) -> String {
    let slug = title
        .to_ascii_lowercase()
        .replace(|character: char| !character.is_ascii_alphanumeric(), "-")
        .trim_matches('-')
        .to_string();
    if slug.is_empty() {
        "fallow/finding".to_string()
    } else {
        format!("fallow/{slug}")
    }
}

const fn annotation_level(level: AnnotationLevel) -> &'static str {
    match level {
        AnnotationLevel::Error => "error",
        AnnotationLevel::Warning => "warning",
        AnnotationLevel::Notice => "note",
    }
}

fn envelope_type_aware(envelope: &serde_json::Value) -> Option<&serde_json::Value> {
    envelope
        .pointer("/_meta/type_aware")
        .or_else(|| envelope.pointer("/_meta/check/type_aware"))
}

pub(super) fn print_sarif(
    results: &AnalysisResults,
    root: &Path,
    rules: &RulesConfig,
    type_aware: Option<&fallow_types::envelope::TypeAwareMeta>,
) -> ExitCode {
    let mut sarif = api_sarif_document(results, root, rules);
    annotate_type_aware_sarif(&mut sarif, type_aware);
    emit_json(&sarif, "SARIF")
}

/// Print SARIF output with owner properties added to each result.
pub(super) fn print_grouped_sarif(
    results: &AnalysisResults,
    root: &Path,
    rules: &RulesConfig,
    resolver: &OwnershipResolver,
    type_aware: Option<&fallow_types::envelope::TypeAwareMeta>,
) -> ExitCode {
    let mut sarif = api_sarif_document(results, root, rules);
    annotate_type_aware_sarif(&mut sarif, type_aware);
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

pub(super) fn print_health_sarif(
    report: &fallow_output::HealthReport,
    root: &Path,
    type_aware: Option<&fallow_types::envelope::TypeAwareMeta>,
) -> ExitCode {
    let mut sarif = api_health_sarif_document(report, root);
    annotate_type_aware_sarif(&mut sarif, type_aware);
    emit_json(&sarif, "SARIF")
}

pub(super) fn print_grouped_health_sarif(
    report: &fallow_output::HealthReport,
    root: &Path,
    resolver: &OwnershipResolver,
    type_aware: Option<&fallow_types::envelope::TypeAwareMeta>,
) -> ExitCode {
    let mut sarif = api_health_sarif_document(report, root);
    annotate_type_aware_sarif(&mut sarif, type_aware);
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

    #[test]
    fn stored_envelope_renders_sarif_with_semantic_provenance() {
        let envelope = serde_json::json!({
            "kind": "dead-code",
            "unused_exports": [{
                "path": "src/dead.ts",
                "line": 7,
                "col": 2,
                "export_name": "dead",
                "is_type_only": false,
                "is_re_export": false
            }],
            "_meta": {
                "type_aware": {
                    "backend": "typescript-go",
                    "queries": [{
                        "capability": "symbol-use",
                        "status": "partial",
                        "assertion": "evidence-bounded",
                        "reason_code": "evidence-limit"
                    }]
                }
            }
        });
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .and_then(Path::parent)
            .expect("workspace root");

        let sarif = envelope_sarif_document(EnvelopeKind::DeadCode, &envelope, root);

        assert_eq!(sarif["version"], "2.1.0");
        assert_eq!(
            sarif["runs"][0]["results"][0]["ruleId"],
            "fallow/unused-export"
        );
        assert_eq!(
            sarif["runs"][0]["properties"]["typeAware"]["backend"],
            "typescript-go"
        );
        assert_eq!(
            sarif["runs"][0]["invocations"][0]["toolExecutionNotifications"][0]["properties"]["reasonCode"],
            "evidence-limit"
        );
    }
}
