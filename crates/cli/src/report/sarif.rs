use std::collections::BTreeMap;
use std::path::Path;
use std::process::ExitCode;

#[cfg(test)]
use fallow_config::Severity;
use fallow_config::{FallowConfig, RulesConfig};
use fallow_output::{
    SarifDocumentInput, SarifResultInput, SarifRuleInput, build_sarif_document, build_sarif_result,
    build_sarif_rule,
};
use fallow_types::duplicates::DuplicationReport;
use fallow_types::results::{AnalysisResults, SecurityFinding};

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
    let mut grouped_queries = std::collections::BTreeMap::<String, Vec<&serde_json::Value>>::new();
    for query in type_aware
        .get("queries")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter(|query| query.get("status").and_then(serde_json::Value::as_str) != Some("complete"))
    {
        let key = serde_json::json!([
            query.get("capability"),
            query.get("status"),
            query.get("assertion"),
            query.get("reason_code"),
            query.get("actions"),
            query.get("omissions"),
        ])
        .to_string();
        grouped_queries.entry(key).or_default().push(query);
    }
    let notifications = grouped_queries
        .into_values()
        .map(|queries| {
            let query = queries[0];
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
            let query_ids = queries
                .iter()
                .filter_map(|query| query.get("query_id").and_then(serde_json::Value::as_u64))
                .collect::<Vec<_>>();
            let query_count = queries.len();
            let suffix = if query_count > 1 {
                format!(" ({query_count} queries)")
            } else {
                String::new()
            };
            serde_json::json!({
                "level": "warning",
                "message": {
                    "text": format!(
                        "Type-aware {capability} was {status}: {assertion}{suffix}"
                    )
                },
                "properties": {
                    "capability": capability,
                    "status": status,
                    "queryIds": query_ids,
                    "queryCount": query_count,
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
pub fn print_envelope_sarif_with_config(
    kind: EnvelopeKind,
    envelope: &serde_json::Value,
    root: &Path,
    config_path: Option<&Path>,
    resolver: Option<&OwnershipResolver>,
) -> ExitCode {
    let sarif = envelope_sarif_document_with_context(kind, envelope, root, config_path, resolver);
    emit_json(&sarif, "SARIF")
}

#[cfg(test)]
fn envelope_sarif_document(
    kind: EnvelopeKind,
    envelope: &serde_json::Value,
    root: &Path,
) -> serde_json::Value {
    envelope_sarif_document_with_config(kind, envelope, root, None)
}

#[cfg(test)]
fn envelope_sarif_document_with_config(
    kind: EnvelopeKind,
    envelope: &serde_json::Value,
    root: &Path,
    config_path: Option<&Path>,
) -> serde_json::Value {
    envelope_sarif_document_with_context(kind, envelope, root, config_path, None)
}

fn envelope_sarif_document_with_context(
    kind: EnvelopeKind,
    envelope: &serde_json::Value,
    root: &Path,
    config_path: Option<&Path>,
    resolver: Option<&OwnershipResolver>,
) -> serde_json::Value {
    if let Some(results) = saved_dead_code_results(kind, envelope) {
        let rules = saved_report_rules(root, config_path);
        let mut sarif = api_sarif_document(&results, root, &rules);
        if let Some(type_aware) = envelope_type_aware(envelope) {
            annotate_type_aware_sarif_value(&mut sarif, type_aware);
        }
        annotate_saved_sarif_grouping(&mut sarif, kind, resolver);
        return sarif;
    }
    if kind == EnvelopeKind::Dupes
        && let Ok(report) = serde_json::from_value::<DuplicationReport>(envelope.clone())
    {
        let sarif = resolver.map_or_else(
            || fallow_api::build_duplication_sarif(&report, root, &sarif_rule),
            |resolver| {
                fallow_api::build_grouped_duplication_sarif(&report, root, &sarif_rule, |group| {
                    super::dupes_grouping::largest_owner(group, root, resolver)
                })
            },
        );
        return sarif;
    }
    if kind == EnvelopeKind::Health
        && let Some(report) = fallow_output::health_report_from_saved_value(envelope)
    {
        let mut sarif = api_health_sarif_document(&report, root);
        if let Some(type_aware) = envelope_type_aware(envelope) {
            annotate_type_aware_sarif_value(&mut sarif, type_aware);
        }
        annotate_saved_sarif_grouping(&mut sarif, kind, resolver);
        return sarif;
    }
    if kind == EnvelopeKind::Audit
        && let Some(mut sarif) = saved_audit_sarif(envelope, root, config_path)
    {
        annotate_saved_sarif_grouping(&mut sarif, kind, resolver);
        return sarif;
    }
    if kind == EnvelopeKind::Combined
        && let Some(mut sarif) = saved_combined_sarif(envelope, root, config_path)
    {
        annotate_saved_sarif_grouping(&mut sarif, kind, resolver);
        return sarif;
    }
    if kind == EnvelopeKind::Security
        && let Some(sarif) = saved_security_sarif(envelope)
    {
        return sarif;
    }

    let mut sarif = annotation_sarif_document(kind, envelope, root, config_path);
    annotate_saved_sarif_grouping(&mut sarif, kind, resolver);
    sarif
}

fn saved_security_sarif(envelope: &serde_json::Value) -> Option<serde_json::Value> {
    let findings =
        serde_json::from_value::<Vec<SecurityFinding>>(envelope.get("security_findings")?.clone())
            .ok()?;
    Some(crate::security::build_security_sarif(
        &findings,
        envelope.get("gate"),
    ))
}

fn annotation_sarif_document(
    kind: EnvelopeKind,
    envelope: &serde_json::Value,
    root: &Path,
    config_path: Option<&Path>,
) -> serde_json::Value {
    let options = resolve_render_options(root);
    let annotations = collect_annotations(kind, envelope, options.pm);
    let mut sarif = envelope_sarif_base_with_config(kind, root, config_path);
    let rule_order = sarif_rule_ids(&sarif);
    let mut rules = sarif_rules_by_id(&sarif);
    let mut results = Vec::with_capacity(annotations.len());

    for annotation in annotations {
        let rule_id = native_rule_id(&annotation.title);
        let fallback_level = annotation_level(annotation.level);
        let level = if is_dead_code_rule_id(&rule_id) {
            rules
                .get(&rule_id)
                .and_then(|rule| {
                    rule.pointer("/defaultConfiguration/level")
                        .and_then(serde_json::Value::as_str)
                })
                .unwrap_or(fallback_level)
        } else {
            fallback_level
        }
        .to_string();
        rules
            .entry(rule_id.clone())
            .or_insert_with(|| sarif_rule(&rule_id, &annotation.title, &level));

        let uri = options.rebase.apply(&annotation.path);
        let region = annotation.line.map(|line| {
            let line = line.clamp(1, u64::from(u32::MAX)) as u32;
            let col = annotation.col.unwrap_or(1).clamp(1, u64::from(u32::MAX)) as u32;
            (line, col)
        });
        let message = format!("{}: {}", annotation.title, annotation.message);
        let mut result = build_sarif_result(SarifResultInput {
            rule_id: &rule_id,
            level: &level,
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
            let start_line = region
                .get("startLine")
                .and_then(serde_json::Value::as_u64)
                .unwrap_or(1);
            region.insert(
                "endLine".to_string(),
                serde_json::json!(end_line.clamp(start_line, u64::from(u32::MAX)) as u32),
            );
        }
        results.push(result);
    }

    if let Some(driver) = sarif.pointer_mut("/runs/0/tool/driver") {
        let mut ordered_rules = Vec::with_capacity(rules.len());
        for id in rule_order {
            if let Some(rule) = rules.remove(&id) {
                ordered_rules.push(rule);
            }
        }
        ordered_rules.extend(rules.into_values());
        driver["rules"] = serde_json::Value::Array(ordered_rules);
    }
    if let Some(run_results) = sarif.pointer_mut("/runs/0/results") {
        *run_results = serde_json::Value::Array(results);
    }
    if let Some(type_aware) = envelope_type_aware(envelope) {
        annotate_type_aware_sarif_value(&mut sarif, type_aware);
    }
    sarif
}

fn saved_audit_sarif(
    envelope: &serde_json::Value,
    root: &Path,
    config_path: Option<&Path>,
) -> Option<serde_json::Value> {
    let rules = saved_report_rules(root, config_path);
    let mut dead_code = parse_optional_section::<AnalysisResults>(envelope, "/dead_code")
        .ok()?
        .map(|results| api_sarif_document(&results, root, &rules));
    if let Some(sarif) = dead_code.as_mut()
        && let Some(type_aware) = envelope_type_aware(envelope)
    {
        annotate_type_aware_sarif_value(sarif, type_aware);
    }
    let duplication = parse_optional_section::<DuplicationReport>(envelope, "/duplication").ok()?;
    let health = match envelope.pointer("/complexity") {
        Some(value) => Some(api_health_sarif_document(
            &fallow_output::health_report_from_saved_value(value)?,
            root,
        )),
        None => None,
    };
    Some(fallow_api::build_audit_sarif(
        fallow_api::AuditSarifOutputInput {
            dead_code: dead_code.as_ref(),
            duplication: duplication.as_ref(),
            health: health.as_ref(),
        },
    ))
}

fn saved_combined_sarif(
    envelope: &serde_json::Value,
    root: &Path,
    config_path: Option<&Path>,
) -> Option<serde_json::Value> {
    let rules = saved_report_rules(root, config_path);
    let dead_code = parse_optional_section::<AnalysisResults>(envelope, "/check").ok()?;
    let duplication = parse_optional_section::<DuplicationReport>(envelope, "/dupes").ok()?;
    let health = match envelope.pointer("/health") {
        Some(value) => Some(fallow_output::health_report_from_saved_value(value)?),
        None => None,
    };

    let mut runs = Vec::new();
    if let Some(results) = dead_code {
        let mut sarif = api_sarif_document(&results, root, &rules);
        if let Some(type_aware) = envelope_type_aware(envelope) {
            annotate_type_aware_sarif_value(&mut sarif, type_aware);
        }
        extend_sarif_runs(&mut runs, &sarif);
    }
    if let Some(report) = duplication.filter(|report| !report.clone_groups.is_empty()) {
        runs.push(combined_duplication_sarif_run(&report));
    }
    if let Some(report) = health {
        extend_sarif_runs(&mut runs, &api_health_sarif_document(&report, root));
    }
    Some(serde_json::json!({
        "$schema": "https://json.schemastore.org/sarif-2.1.0.json",
        "version": "2.1.0",
        "runs": runs,
    }))
}

fn parse_optional_section<T: serde::de::DeserializeOwned>(
    envelope: &serde_json::Value,
    pointer: &str,
) -> Result<Option<T>, serde_json::Error> {
    let Some(value) = envelope.pointer(pointer) else {
        return Ok(None);
    };
    serde_json::from_value(value.clone()).map(Some)
}

fn extend_sarif_runs(runs: &mut Vec<serde_json::Value>, sarif: &serde_json::Value) {
    if let Some(source_runs) = sarif.get("runs").and_then(serde_json::Value::as_array) {
        runs.extend(source_runs.iter().cloned());
    }
}

fn combined_duplication_sarif_run(report: &DuplicationReport) -> serde_json::Value {
    serde_json::json!({
        "tool": {
            "driver": {
                "name": "fallow",
                "version": env!("CARGO_PKG_VERSION"),
                "informationUri": "https://github.com/fallow-rs/fallow",
            }
        },
        "automationDetails": { "id": "fallow/dupes" },
        "results": report.clone_groups.iter().enumerate().map(|(index, group)| {
            serde_json::json!({
                "ruleId": "fallow/code-duplication",
                "level": "warning",
                "message": {
                    "text": format!(
                        "Clone group {} ({} lines, {} instances)",
                        index + 1,
                        group.line_count,
                        group.instances.len(),
                    ),
                },
            })
        }).collect::<Vec<_>>(),
    })
}

fn annotate_saved_sarif_grouping(
    sarif: &mut serde_json::Value,
    kind: EnvelopeKind,
    resolver: Option<&OwnershipResolver>,
) {
    let Some(resolver) = resolver else {
        return;
    };
    let property = match kind {
        EnvelopeKind::Health | EnvelopeKind::Dupes => "group",
        _ => "owner",
    };
    fallow_api::annotate_sarif_results(sarif, property, |uri| {
        let decoded = uri.replace("%5B", "[").replace("%5D", "]");
        grouping::resolve_owner(Path::new(&decoded), Path::new(""), resolver)
    });
}

pub(super) fn envelope_rule_level_with_config(
    kind: EnvelopeKind,
    title: &str,
    root: &Path,
    config_path: Option<&Path>,
) -> Option<String> {
    let rule_id = native_rule_id(title);
    envelope_sarif_base_with_config(kind, root, config_path)
        .pointer("/runs/0/tool/driver/rules")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .find(|rule| rule.get("id").and_then(serde_json::Value::as_str) == Some(rule_id.as_str()))
        .and_then(|rule| {
            rule.pointer("/defaultConfiguration/level")
                .and_then(serde_json::Value::as_str)
        })
        .map(str::to_owned)
}

fn saved_dead_code_results(
    kind: EnvelopeKind,
    envelope: &serde_json::Value,
) -> Option<AnalysisResults> {
    if kind != EnvelopeKind::DeadCode {
        return None;
    }
    serde_json::from_value(envelope.clone()).ok()
}

pub(super) fn saved_report_rules(root: &Path, config_path: Option<&Path>) -> RulesConfig {
    config_path
        .and_then(|path| FallowConfig::load(path).ok())
        .map(|config| config.rules)
        .or_else(|| {
            FallowConfig::find_and_load(root)
                .ok()
                .flatten()
                .map(|(config, _)| config.rules)
        })
        .unwrap_or_default()
}

fn envelope_sarif_base_with_config(
    kind: EnvelopeKind,
    root: &Path,
    config_path: Option<&Path>,
) -> serde_json::Value {
    let rules = saved_report_rules(root, config_path);
    match kind {
        EnvelopeKind::DeadCode => api_sarif_document(&AnalysisResults::default(), root, &rules),
        EnvelopeKind::Dupes => {
            fallow_api::build_duplication_sarif(&DuplicationReport::default(), root, &sarif_rule)
        }
        EnvelopeKind::Health => {
            api_health_sarif_document(&fallow_output::HealthReport::default(), root)
        }
        EnvelopeKind::Audit | EnvelopeKind::Combined => {
            let mut document = api_sarif_document(&AnalysisResults::default(), root, &rules);
            merge_sarif_rules(
                &mut document,
                &fallow_api::build_duplication_sarif(
                    &DuplicationReport::default(),
                    root,
                    &sarif_rule,
                ),
            );
            merge_sarif_rules(
                &mut document,
                &api_health_sarif_document(&fallow_output::HealthReport::default(), root),
            );
            document
        }
        EnvelopeKind::Security | EnvelopeKind::Fix => build_sarif_document(SarifDocumentInput {
            results: &[],
            rules: &[],
            tool_version: env!("CARGO_PKG_VERSION"),
        }),
    }
}

fn merge_sarif_rules(target: &mut serde_json::Value, source: &serde_json::Value) {
    let mut rules = sarif_rules_by_id(target);
    for (id, rule) in sarif_rules_by_id(source) {
        rules.entry(id).or_insert(rule);
    }
    if let Some(driver) = target.pointer_mut("/runs/0/tool/driver") {
        driver["rules"] = serde_json::Value::Array(rules.into_values().collect());
    }
}

fn sarif_rules_by_id(document: &serde_json::Value) -> BTreeMap<String, serde_json::Value> {
    document
        .pointer("/runs/0/tool/driver/rules")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|rule| {
            rule.get("id")
                .and_then(serde_json::Value::as_str)
                .map(|id| (id.to_string(), rule.clone()))
        })
        .collect()
}

fn sarif_rule_ids(document: &serde_json::Value) -> Vec<String> {
    document
        .pointer("/runs/0/tool/driver/rules")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(|rule| {
            rule.get("id")
                .and_then(serde_json::Value::as_str)
                .map(str::to_owned)
        })
        .collect()
}

pub(super) fn native_rule_id(title: &str) -> String {
    let fixed = match title {
        "Dynamic segment conflict" => Some("fallow/dynamic-segment-name-conflict"),
        "Unused devDependency" => Some("fallow/unused-dev-dependency"),
        "Unused optionalDependency" => Some("fallow/unused-optional-dependency"),
        "Stale @expected-unused" | "Unknown suppression kind" => Some("fallow/stale-suppression"),
        _ if title.starts_with("High cyclomatic complexity") => {
            Some("fallow/high-cyclomatic-complexity")
        }
        _ if title.starts_with("High cognitive complexity") => {
            Some("fallow/high-cognitive-complexity")
        }
        _ if title.starts_with("High complexity") => Some("fallow/high-complexity"),
        _ if title.starts_with("High CRAP score") => Some("fallow/high-crap-score"),
        _ if title.starts_with("Refactoring target") => Some("fallow/refactoring-target"),
        _ if title.starts_with("Runtime coverage") => {
            let normalized = title.replace('_', "-");
            if normalized.contains("safe-to-delete") {
                Some("fallow/runtime-safe-to-delete")
            } else if normalized.contains("review-required") {
                Some("fallow/runtime-review-required")
            } else if normalized.contains("low-traffic") {
                Some("fallow/runtime-low-traffic")
            } else if normalized.contains("coverage-unavailable") {
                Some("fallow/runtime-coverage-unavailable")
            } else {
                Some("fallow/runtime-coverage")
            }
        }
        _ if title.starts_with("Coverage intelligence") => {
            if title.contains("add-test-or-split-before-merge") {
                Some("fallow/coverage-intelligence-risky-change")
            } else if title.contains("delete-after-confirming-owner") {
                Some("fallow/coverage-intelligence-delete")
            } else if title.contains("refactor-carefully-keep-behavior") {
                Some("fallow/coverage-intelligence-refactor")
            } else {
                Some("fallow/coverage-intelligence-review")
            }
        }
        _ if title.starts_with("Security candidate") => Some("fallow/security-candidate"),
        _ => None,
    };
    if let Some(rule_id) = fixed {
        return rule_id.to_string();
    }
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

pub(super) fn is_dead_code_rule_id(rule_id: &str) -> bool {
    fallow_output::issue_output_contracts()
        .any(|contract| contract.sarif_rule_ids.iter().any(|id| id == rule_id))
        || rule_id == "fallow/missing-suppression-reason"
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
    use fallow_types::output_dead_code::UnusedFileFinding;
    use fallow_types::results::UnusedFile;
    use std::path::PathBuf;

    fn rule_by_id<'a>(sarif: &'a serde_json::Value, id: &str) -> &'a serde_json::Value {
        sarif["runs"][0]["tool"]["driver"]["rules"]
            .as_array()
            .expect("SARIF rules")
            .iter()
            .find(|rule| rule["id"] == id)
            .expect("rule id")
    }

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
    fn saved_renderer_uses_native_rule_ids_for_non_slug_titles() {
        assert_eq!(
            native_rule_id("Unused devDependency"),
            "fallow/unused-dev-dependency"
        );
        assert_eq!(
            native_rule_id("Dynamic segment conflict"),
            "fallow/dynamic-segment-name-conflict"
        );
        assert_eq!(
            native_rule_id("Runtime coverage (safe_to_delete)"),
            "fallow/runtime-safe-to-delete"
        );
        assert_eq!(
            native_rule_id("High cyclomatic complexity (high)"),
            "fallow/high-cyclomatic-complexity"
        );
        assert_eq!(
            native_rule_id("High cognitive complexity (moderate)"),
            "fallow/high-cognitive-complexity"
        );
        assert_eq!(
            native_rule_id("High CRAP score (critical)"),
            "fallow/high-crap-score"
        );
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
                        "query_id": 1,
                        "capability": "symbol-use",
                        "status": "partial",
                        "assertion": "evidence-bounded",
                        "reason_code": "evidence-limit"
                    }, {
                        "query_id": 2,
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
        assert_eq!(
            sarif["runs"][0]["invocations"][0]["toolExecutionNotifications"]
                .as_array()
                .map(Vec::len),
            Some(1)
        );
        assert_eq!(
            sarif["runs"][0]["invocations"][0]["toolExecutionNotifications"][0]["properties"]["queryCount"],
            2
        );
        assert_eq!(
            sarif["runs"][0]["invocations"][0]["toolExecutionNotifications"][0]["properties"]["queryIds"],
            serde_json::json!([1, 2])
        );
    }

    #[test]
    fn stored_dead_code_sarif_preserves_native_rule_contract() {
        let root = tempfile::tempdir().expect("tempdir");
        std::fs::write(
            root.path().join(".fallowrc.json"),
            r#"{"rules":{"unused-files":"warn"}}"#,
        )
        .expect("write config");
        let mut results = AnalysisResults::default();
        results
            .unused_files
            .push(UnusedFileFinding::with_actions(UnusedFile {
                path: root.path().join("src/dead.ts"),
            }));
        let rules = RulesConfig {
            unused_files: Severity::Warn,
            ..RulesConfig::default()
        };
        let direct = api_sarif_document(&results, root.path(), &rules);
        let mut envelope = serde_json::to_value(&results).expect("serialize results");
        envelope["kind"] = serde_json::json!("dead-code");
        fallow_output::strip_root_prefix(&mut envelope, &format!("{}/", root.path().display()));

        let saved = envelope_sarif_document(EnvelopeKind::DeadCode, &envelope, root.path());

        assert_eq!(saved["runs"][0]["results"], direct["runs"][0]["results"]);
        assert_eq!(
            saved["runs"][0]["results"][0]["ruleId"],
            direct["runs"][0]["results"][0]["ruleId"]
        );
        assert_eq!(
            saved["runs"][0]["results"][0]["level"],
            direct["runs"][0]["results"][0]["level"]
        );
        assert_eq!(
            sarif_rule_ids(&saved),
            sarif_rule_ids(&direct),
            "saved rendering must preserve the native rule registry order"
        );
        let direct_rule = rule_by_id(&direct, "fallow/unused-file");
        let saved_rule = rule_by_id(&saved, "fallow/unused-file");
        assert_eq!(
            saved_rule["defaultConfiguration"],
            direct_rule["defaultConfiguration"]
        );
        assert_eq!(
            saved_rule["fullDescription"],
            direct_rule["fullDescription"]
        );
        assert_eq!(saved_rule["helpUri"], direct_rule["helpUri"]);
    }

    #[test]
    fn stored_sarif_uses_explicit_report_config() {
        let root = tempfile::tempdir().expect("root");
        std::fs::write(
            root.path().join(".fallowrc.json"),
            r#"{"rules":{"unused-files":"warn"}}"#,
        )
        .expect("write discovered config");
        let explicit = root.path().join("ci.json");
        std::fs::write(&explicit, r#"{"rules":{"unused-files":"error"}}"#)
            .expect("write explicit config");
        let envelope = serde_json::json!({
            "kind": "dead-code",
            "unused_files": [{"path": "src/dead.ts"}]
        });

        let saved = envelope_sarif_document_with_config(
            EnvelopeKind::DeadCode,
            &envelope,
            root.path(),
            Some(&explicit),
        );

        assert_eq!(saved["runs"][0]["results"][0]["level"], "error");
    }

    #[test]
    fn stored_sarif_clamps_invalid_regions() {
        let envelope = serde_json::json!({
            "kind": "dead-code",
            "unused_exports": [{
                "path": "src/dead.ts",
                "line": 0,
                "col": 0,
                "export_name": "dead",
                "is_type_only": false,
                "is_re_export": false
            }]
        });
        let root = PathBuf::from("/project");

        let saved = envelope_sarif_document(EnvelopeKind::DeadCode, &envelope, &root);
        let region = &saved["runs"][0]["results"][0]["locations"][0]["physicalLocation"]["region"];

        assert_eq!(region["startLine"], 1);
        assert_eq!(region["startColumn"], 1);
    }
}
