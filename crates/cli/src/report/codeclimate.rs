use std::path::Path;
use std::process::ExitCode;

#[cfg(test)]
use fallow_config::Severity;
use fallow_config::{OutputFormat, RulesConfig};
use fallow_output::{
    CodeClimateIssue, CodeClimateIssueKind, CodeClimateLines, CodeClimateLocation,
    CodeClimateSeverity, HealthReport, codeclimate_fingerprint_hash, codeclimate_issues_to_value,
};
use fallow_types::duplicates::DuplicationReport;
use fallow_types::results::AnalysisResults;

use super::github::report_prefix;
use super::github::{AnnotationLevel, resolve_render_options};
use super::github_annotations::{EnvelopeKind, collect_annotations};
use super::grouping::OwnershipResolver;
use super::{emit_json, normalize_uri, relative_path};
#[path = "codeclimate_saved_grouping.rs"]
mod saved_grouping;
use saved_grouping::{
    annotate_dead_code_owner, annotate_duplication_groups, annotate_health_groups,
    annotate_saved_dead_code, annotate_saved_fallback_grouping, saved_clone_group_flags,
};

/// Rebase every issue path onto the repository root.
///
/// CI platforms address files by repo-root-relative path: GitLab's Code
/// Quality widget matches `location.path` against the MR diff, and the review
/// APIs reject a `new_path` that names no file in the diff. The CodeClimate
/// builders emit analysis-root-relative paths, which only coincide with the
/// repo root for a single-package repo.
///
/// The review and sticky-summary formats derive their paths from these issues,
/// so rebasing here covers every CI surface but `github-annotations`, which
/// renders from JSON and applies the same rebase itself.
pub fn rebase_codeclimate_paths(issues: &mut [CodeClimateIssue]) {
    let prefix = report_prefix();
    if prefix.is_empty() {
        return;
    }
    for issue in issues {
        issue.location.path = fallow_output::apply_path_prefix(prefix, &issue.location.path);
        for location in &mut issue.other_locations {
            location.path = fallow_output::apply_path_prefix(prefix, &location.path);
        }
    }
}

/// Rebase, then serialize. The single point where CodeClimate issues leave as
/// the CodeClimate wire format; everything upstream keeps analysis-root-relative
/// paths so diff lookups and CODEOWNERS resolution stay in one namespace.
fn emit_codeclimate(mut issues: Vec<CodeClimateIssue>) -> ExitCode {
    rebase_codeclimate_paths(&mut issues);
    let value = codeclimate_issues_to_value(&issues);
    emit_json(&value, "CodeClimate")
}

/// Re-render a stored JSON envelope as a CodeClimate issue array without
/// repeating analysis. Semantic provenance stays in the paired JSON artifact.
///
/// `survivors` are the survivor findings of a mutation report, already
/// filtered to the diff. They follow the analysis issues.
pub fn print_envelope_codeclimate_with_config(
    kind: EnvelopeKind,
    envelope: &serde_json::Value,
    root: &Path,
    config_path: Option<&Path>,
    resolver: Option<&OwnershipResolver>,
    survivors: &[CodeClimateIssue],
) -> ExitCode {
    let mut issues = match envelope_codeclimate_issues_with_config(
        kind,
        envelope,
        root,
        config_path,
        resolver,
    ) {
        Ok(issues) => issues,
        Err(error) => {
            return crate::emit_known_failure(
                &error,
                crate::exit_codes::INVALID_INPUT,
                OutputFormat::CodeClimate,
                crate::telemetry::FailureReason::Validation,
            );
        }
    };
    issues.extend_from_slice(survivors);
    emit_codeclimate(issues)
}

#[cfg(test)]
fn envelope_codeclimate_issues(
    kind: EnvelopeKind,
    envelope: &serde_json::Value,
    root: &Path,
) -> Vec<CodeClimateIssue> {
    envelope_codeclimate_issues_with_config(kind, envelope, root, None, None)
        .expect("test envelope should reconstruct as CodeClimate issues")
}

/// Reconstruct the typed CodeClimate issues stored in a saved native envelope.
///
/// Paths remain analysis-root-relative here. Presentation renderers apply the
/// repository prefix once at their existing output boundary.
pub fn envelope_codeclimate_issues_with_config(
    kind: EnvelopeKind,
    envelope: &serde_json::Value,
    root: &Path,
    config_path: Option<&Path>,
    resolver: Option<&OwnershipResolver>,
) -> Result<Vec<CodeClimateIssue>, String> {
    validate_saved_schema(kind, envelope)?;
    match saved_native_codeclimate_issues(kind, envelope, root, config_path, resolver) {
        Ok(issues) => Ok(issues),
        Err(native_error)
            // `architecture` has no legacy schema: it first shipped with its
            // own envelope, so a saved architecture file is never legacy.
            if matches!(
                kind,
                EnvelopeKind::DeadCode
                    | EnvelopeKind::Dupes
                    | EnvelopeKind::Health
                    | EnvelopeKind::Audit
                    | EnvelopeKind::Combined
            ) && saved_schema_is_legacy(kind, envelope) =>
        {
            let issues =
                saved_annotation_codeclimate_issues(kind, envelope, root, config_path, resolver);
            if issues.is_empty() {
                Err(native_error)
            } else {
                Ok(issues)
            }
        }
        Err(error) => Err(error),
    }
}

fn saved_annotation_codeclimate_issues(
    kind: EnvelopeKind,
    envelope: &serde_json::Value,
    root: &Path,
    config_path: Option<&Path>,
    resolver: Option<&OwnershipResolver>,
) -> Vec<CodeClimateIssue> {
    let options = resolve_render_options(root);
    let mut issues = collect_annotations(kind, envelope, options.pm)
        .into_iter()
        .map(|annotation| {
            let path = annotation.path;
            let line = annotation.line.unwrap_or(1).clamp(1, u64::from(u32::MAX)) as u32;
            let rule_id = super::sarif::native_rule_id(&annotation.title);
            let severity = if rule_id == "fallow/runtime-safe-to-delete" {
                CodeClimateSeverity::Critical
            } else if rule_id == "fallow/runtime-review-required" {
                CodeClimateSeverity::Major
            } else if super::sarif::is_dead_code_rule_id(&rule_id) {
                super::sarif::envelope_rule_level_with_config(
                    kind,
                    &annotation.title,
                    root,
                    config_path,
                )
                .as_deref()
                .map_or_else(
                    || annotation_codeclimate_severity(annotation.level),
                    sarif_level_to_codeclimate,
                )
            } else {
                annotation_codeclimate_severity(annotation.level)
            };
            let fingerprint = envelope_fingerprint(&rule_id, &path, line, &annotation.message);
            CodeClimateIssue {
                kind: CodeClimateIssueKind::Issue,
                check_name: rule_id.clone(),
                description: format!("{}: {}", annotation.title, annotation.message),
                categories: vec![native_category(&rule_id).to_owned()],
                severity,
                fingerprint,
                location: CodeClimateLocation {
                    path,
                    lines: CodeClimateLines {
                        begin: line,
                        end: None,
                    },
                },
                other_locations: Vec::new(),
                owner: None,
                group: None,
                legacy_fingerprint: None,
                audit: None,
            }
        })
        .collect::<Vec<_>>();
    annotate_saved_fallback_grouping(&mut issues, kind, resolver);
    issues
}

fn saved_native_codeclimate_issues(
    kind: EnvelopeKind,
    envelope: &serde_json::Value,
    root: &Path,
    config_path: Option<&Path>,
    resolver: Option<&OwnershipResolver>,
) -> Result<Vec<CodeClimateIssue>, String> {
    let rules = super::sarif::saved_report_rules(root, config_path);
    match kind {
        EnvelopeKind::DeadCode | EnvelopeKind::Architecture => {
            let mut results =
                parse_saved_section::<AnalysisResults>(envelope, "dead-code envelope")?;
            if !saved_schema_is_legacy(kind, envelope) {
                let declared_total = envelope
                    .get("total_issues")
                    .and_then(serde_json::Value::as_u64)
                    .ok_or_else(|| {
                        "saved dead-code envelope is missing a non-negative integer `total_issues`"
                            .to_owned()
                    })?;
                let actual_total = u64::try_from(results.total_issues()).unwrap_or(u64::MAX);
                if declared_total != actual_total {
                    return Err(format!(
                        "saved dead-code envelope declares {declared_total} findings but contains {actual_total}"
                    ));
                }
            }
            // Grouped saved envelopes are flattened in group order. Restore the
            // analyzer's canonical order before building any presentation surface
            // so owner/section grouping cannot reorder otherwise identical findings.
            results.sort();
            let mut issues = api_codeclimate_issues(&results, root, &rules);
            annotate_dead_code_owner(&mut issues, resolver);
            Ok(issues)
        }
        EnvelopeKind::Dupes => {
            let report = parse_saved_section::<DuplicationReport>(envelope, "dupes envelope")?;
            let mut issues = api_duplication_codeclimate_issues(&report, root);
            annotate_duplication_groups(&mut issues, &report, root, resolver);
            Ok(issues)
        }
        EnvelopeKind::Health => {
            let report =
                fallow_output::health_report_from_saved_value(envelope).ok_or_else(|| {
                    "saved health envelope is incompatible with this Fallow version".to_owned()
                })?;
            let mut issues = api_health_codeclimate_issues(&report, root);
            annotate_health_groups(&mut issues, resolver);
            Ok(issues)
        }
        EnvelopeKind::Audit => {
            let mut issues = saved_multi_analysis_codeclimate(
                envelope,
                root,
                &rules,
                "/dead_code",
                "/duplication",
                "/complexity",
                resolver,
            )?;
            if let Some(diff_risk) =
                parse_optional_section::<fallow_output::DiffRiskReport>(envelope, "/diff_risk")?
            {
                issues.extend(fallow_api::build_diff_risk_codeclimate(&diff_risk));
            }
            Ok(issues)
        }
        EnvelopeKind::Combined => saved_multi_analysis_codeclimate(
            envelope, root, &rules, "/check", "/dupes", "/health", resolver,
        ),
        EnvelopeKind::Fix => Ok(Vec::new()),
        EnvelopeKind::Security => Err(format!(
            "saved {} envelopes cannot be rendered as CodeClimate issues",
            envelope_kind_label(kind)
        )),
    }
}

pub fn validate_saved_schema(
    kind: EnvelopeKind,
    envelope: &serde_json::Value,
) -> Result<(), String> {
    let Some(value) = envelope.get("schema_version") else {
        // Early Fallow envelopes did not consistently carry a schema version.
        // Their typed shape remains the compatibility check below.
        return Ok(());
    };
    let Some(version) = value.as_u64() else {
        return Err(format!(
            "saved {} envelope has a non-numeric `schema_version`",
            envelope_kind_label(kind)
        ));
    };
    let Some(current) = current_schema_version(kind) else {
        return Ok(());
    };
    if version == 0 || version > u64::from(current) {
        return Err(format!(
            "unsupported saved {} schema version {version}; this Fallow version supports versions 1 through {current}",
            envelope_kind_label(kind)
        ));
    }
    if !saved_schema_is_legacy(kind, envelope) {
        validate_current_saved_envelope(kind, envelope)?;
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum SavedFieldType {
    Array,
    Boolean,
    Object,
    String,
    UnsignedInteger,
}

impl SavedFieldType {
    const fn label(self) -> &'static str {
        match self {
            Self::Array => "an array",
            Self::Boolean => "a boolean",
            Self::Object => "an object",
            Self::String => "a string",
            Self::UnsignedInteger => "a non-negative integer",
        }
    }

    fn matches(self, value: &serde_json::Value) -> bool {
        match self {
            Self::Array => value.is_array(),
            Self::Boolean => value.is_boolean(),
            Self::Object => value.is_object(),
            Self::String => value.is_string(),
            Self::UnsignedInteger => value.as_u64().is_some(),
        }
    }
}

fn require_saved_field<'a>(
    kind: EnvelopeKind,
    object: &'a serde_json::Value,
    field: &str,
    field_type: SavedFieldType,
) -> Result<&'a serde_json::Value, String> {
    let Some(value) = object.get(field) else {
        return Err(format!(
            "saved {} envelope is missing required field `{field}`",
            envelope_kind_label(kind)
        ));
    };
    if !field_type.matches(value) {
        return Err(format!(
            "saved {} envelope field `{field}` must be {}",
            envelope_kind_label(kind),
            field_type.label()
        ));
    }
    Ok(value)
}

fn require_saved_nested_field(
    kind: EnvelopeKind,
    object: &serde_json::Value,
    object_name: &str,
    field: &str,
    field_type: SavedFieldType,
) -> Result<(), String> {
    let Some(value) = object.get(field) else {
        return Err(format!(
            "saved {} envelope is missing required field `{object_name}.{field}`",
            envelope_kind_label(kind)
        ));
    };
    if !field_type.matches(value) {
        return Err(format!(
            "saved {} envelope field `{object_name}.{field}` must be {}",
            envelope_kind_label(kind),
            field_type.label()
        ));
    }
    Ok(())
}

fn validate_current_saved_envelope(
    kind: EnvelopeKind,
    envelope: &serde_json::Value,
) -> Result<(), String> {
    crate::report::ci::saved_type_aware_metadata(envelope)?;
    require_saved_field(kind, envelope, "version", SavedFieldType::String)?;
    require_saved_field(
        kind,
        envelope,
        "elapsed_ms",
        SavedFieldType::UnsignedInteger,
    )?;

    match kind {
        EnvelopeKind::DeadCode | EnvelopeKind::Architecture => {
            validate_current_dead_code_envelope(kind, envelope)?;
        }
        EnvelopeKind::Dupes => validate_current_duplication_payload(envelope)?,
        EnvelopeKind::Health => validate_current_health_payload(envelope)?,
        EnvelopeKind::Audit => {
            validate_current_audit_envelope(kind, envelope)?;
            validate_current_analysis_sections(
                kind,
                envelope,
                &[
                    ("/dead_code", EnvelopeKind::DeadCode),
                    ("/duplication", EnvelopeKind::Dupes),
                    ("/complexity", EnvelopeKind::Health),
                ],
            )?;
        }
        EnvelopeKind::Combined => validate_current_analysis_sections(
            kind,
            envelope,
            &[
                ("/check", EnvelopeKind::DeadCode),
                ("/dupes", EnvelopeKind::Dupes),
                ("/health", EnvelopeKind::Health),
            ],
        )?,
        EnvelopeKind::Security | EnvelopeKind::Fix => {}
    }
    Ok(())
}

fn validate_current_dead_code_envelope(
    kind: EnvelopeKind,
    envelope: &serde_json::Value,
) -> Result<(), String> {
    let declared_total_value = require_saved_field(
        kind,
        envelope,
        "total_issues",
        SavedFieldType::UnsignedInteger,
    )?;
    let declared_total = declared_total_value.as_u64().ok_or_else(|| {
        "saved dead-code envelope field `total_issues` must be a non-negative integer".to_owned()
    })?;
    if envelope
        .get(crate::cli_report::NORMALIZED_GROUPED_DEAD_CODE_MARKER)
        .and_then(serde_json::Value::as_bool)
        != Some(true)
    {
        require_saved_field(kind, envelope, "summary", SavedFieldType::Object)?;
    }
    let results = parse_saved_section::<AnalysisResults>(envelope, "dead-code envelope")?;
    let actual_total = u64::try_from(results.total_issues()).unwrap_or(u64::MAX);
    if declared_total != actual_total {
        return Err(format!(
            "saved dead-code envelope declares {declared_total} findings but contains {actual_total}"
        ));
    }
    Ok(())
}

fn validate_current_analysis_sections(
    parent_kind: EnvelopeKind,
    envelope: &serde_json::Value,
    sections: &[(&str, EnvelopeKind)],
) -> Result<(), String> {
    for &(pointer, section_kind) in sections {
        let Some(section) = envelope.pointer(pointer) else {
            continue;
        };
        if section.is_null() {
            continue;
        }
        let result = match section_kind {
            EnvelopeKind::DeadCode => validate_current_dead_code_section(section),
            EnvelopeKind::Dupes => validate_current_duplication_payload(section),
            EnvelopeKind::Health => validate_current_health_payload(section),
            EnvelopeKind::Architecture
            | EnvelopeKind::Audit
            | EnvelopeKind::Combined
            | EnvelopeKind::Security
            | EnvelopeKind::Fix => {
                return Err("saved analysis section uses an unsupported envelope kind".to_owned());
            }
        };
        result.map_err(|error| {
            format!(
                "saved {} section `{pointer}` is incompatible with this Fallow version: {error}",
                envelope_kind_label(parent_kind)
            )
        })?;
    }
    Ok(())
}

fn validate_current_dead_code_section(section: &serde_json::Value) -> Result<(), String> {
    let version = section
        .get("schema_version")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| "missing a numeric `schema_version`".to_owned())?;
    let expected = u64::from(fallow_output::CHECK_SCHEMA_VERSION);
    if !(u64::from(FIRST_STRICT_CHECK_SCHEMA_VERSION)..=expected).contains(&version) {
        return Err(format!(
            "schema version is {version}; expected {FIRST_STRICT_CHECK_SCHEMA_VERSION} through {expected}"
        ));
    }
    validate_current_saved_envelope(EnvelopeKind::DeadCode, section)
}

fn validate_current_duplication_payload(payload: &serde_json::Value) -> Result<(), String> {
    require_saved_field(
        EnvelopeKind::Dupes,
        payload,
        "clone_groups",
        SavedFieldType::Array,
    )?;
    require_saved_field(
        EnvelopeKind::Dupes,
        payload,
        "clone_families",
        SavedFieldType::Array,
    )?;
    require_saved_field(
        EnvelopeKind::Dupes,
        payload,
        "stats",
        SavedFieldType::Object,
    )?;
    parse_saved_section::<DuplicationReport>(payload, "duplication report")?;
    Ok(())
}

fn validate_current_health_payload(payload: &serde_json::Value) -> Result<(), String> {
    require_saved_field(
        EnvelopeKind::Health,
        payload,
        "findings",
        SavedFieldType::Array,
    )?;
    require_saved_field(
        EnvelopeKind::Health,
        payload,
        "summary",
        SavedFieldType::Object,
    )?;
    fallow_output::health_report_from_saved_value(payload)
        .ok_or_else(|| "saved health report is incompatible with this Fallow version".to_owned())?;
    Ok(())
}

fn validate_current_audit_envelope(
    kind: EnvelopeKind,
    envelope: &serde_json::Value,
) -> Result<(), String> {
    let command = require_saved_field(kind, envelope, "command", SavedFieldType::String)?;
    if command.as_str() != Some("audit") {
        return Err("saved audit envelope field `command` must be `audit`".to_owned());
    }
    let verdict = require_saved_field(kind, envelope, "verdict", SavedFieldType::String)?;
    if !matches!(verdict.as_str(), Some("pass" | "warn" | "fail")) {
        return Err(
            "saved audit envelope field `verdict` must be `pass`, `warn`, or `fail`".to_owned(),
        );
    }
    require_saved_field(
        kind,
        envelope,
        "changed_files_count",
        SavedFieldType::UnsignedInteger,
    )?;
    require_saved_field(kind, envelope, "base_ref", SavedFieldType::String)?;

    let summary = require_saved_field(kind, envelope, "summary", SavedFieldType::Object)?;
    for field in [
        "dead_code_issues",
        "complexity_findings",
        "duplication_clone_groups",
    ] {
        require_saved_nested_field(
            kind,
            summary,
            "summary",
            field,
            SavedFieldType::UnsignedInteger,
        )?;
    }
    require_saved_nested_field(
        kind,
        summary,
        "summary",
        "dead_code_has_errors",
        SavedFieldType::Boolean,
    )?;

    let attribution = require_saved_field(kind, envelope, "attribution", SavedFieldType::Object)?;
    let gate = attribution.get("gate").ok_or_else(|| {
        "saved audit envelope is missing required field `attribution.gate`".to_owned()
    })?;
    if !matches!(gate.as_str(), Some("new-only" | "all")) {
        return Err(
            "saved audit envelope field `attribution.gate` must be `new-only` or `all`".to_owned(),
        );
    }
    for field in [
        "dead_code_introduced",
        "dead_code_inherited",
        "complexity_introduced",
        "complexity_inherited",
        "duplication_introduced",
        "duplication_inherited",
        "styling_introduced",
        "styling_inherited",
        "duplication_demoted",
    ] {
        require_saved_nested_field(
            kind,
            attribution,
            "attribution",
            field,
            SavedFieldType::UnsignedInteger,
        )?;
    }
    Ok(())
}

// These shapes already enforce complete typed payloads and consistent totals.
// Additive schema bumps must not restore permissive legacy reconstruction.
const FIRST_STRICT_CHECK_SCHEMA_VERSION: u32 = 10;
const FIRST_STRICT_ARCHITECTURE_SCHEMA_VERSION: u32 = 1;

pub fn saved_schema_is_legacy(kind: EnvelopeKind, envelope: &serde_json::Value) -> bool {
    let Some(version) = envelope
        .get("schema_version")
        .and_then(serde_json::Value::as_u64)
    else {
        return true;
    };
    let strict_floor = match kind {
        EnvelopeKind::DeadCode => Some(FIRST_STRICT_CHECK_SCHEMA_VERSION),
        EnvelopeKind::Architecture => Some(FIRST_STRICT_ARCHITECTURE_SCHEMA_VERSION),
        _ => current_schema_version(kind),
    };
    strict_floor.is_some_and(|floor| version < u64::from(floor))
}

fn current_schema_version(kind: EnvelopeKind) -> Option<u32> {
    match kind {
        EnvelopeKind::DeadCode => Some(fallow_output::CHECK_SCHEMA_VERSION),
        EnvelopeKind::Architecture => Some(fallow_output::ARCHITECTURE_SCHEMA_VERSION),
        EnvelopeKind::Dupes => Some(fallow_output::DUPES_SCHEMA_VERSION),
        EnvelopeKind::Health => Some(fallow_output::HEALTH_SCHEMA_VERSION),
        EnvelopeKind::Audit => Some(fallow_output::AUDIT_SCHEMA_VERSION),
        EnvelopeKind::Combined => Some(fallow_output::COMBINED_SCHEMA_VERSION),
        EnvelopeKind::Security | EnvelopeKind::Fix => None,
    }
}

const fn envelope_kind_label(kind: EnvelopeKind) -> &'static str {
    match kind {
        EnvelopeKind::DeadCode => "dead-code",
        EnvelopeKind::Architecture => "architecture",
        EnvelopeKind::Dupes => "dupes",
        EnvelopeKind::Health => "health",
        EnvelopeKind::Audit => "audit",
        EnvelopeKind::Security => "security",
        EnvelopeKind::Combined => "combined",
        EnvelopeKind::Fix => "fix",
    }
}

fn parse_saved_section<T: serde::de::DeserializeOwned>(
    value: &serde_json::Value,
    label: &str,
) -> Result<T, String> {
    serde_json::from_value(value.clone())
        .map_err(|error| format!("saved {label} is incompatible with this Fallow version: {error}"))
}

#[expect(
    clippy::too_many_arguments,
    reason = "audit and combined envelopes supply three named native section pointers"
)]
fn saved_multi_analysis_codeclimate(
    envelope: &serde_json::Value,
    root: &Path,
    rules: &RulesConfig,
    dead_code_pointer: &str,
    duplication_pointer: &str,
    health_pointer: &str,
    resolver: Option<&OwnershipResolver>,
) -> Result<Vec<CodeClimateIssue>, String> {
    let dead_code = parse_optional_section::<AnalysisResults>(envelope, dead_code_pointer)?;
    let duplication = parse_optional_section::<DuplicationReport>(envelope, duplication_pointer)?;
    let health = match envelope.pointer(health_pointer) {
        Some(value) if !value.is_null() => Some(
            fallow_output::health_report_from_saved_value(value).ok_or_else(|| {
                format!("saved section `{health_pointer}` is incompatible with this Fallow version")
            })?,
        ),
        Some(_) | None => None,
    };

    let mut issues = Vec::new();
    if let Some(results) = dead_code {
        let mut dead_code_issues = api_codeclimate_issues(&results, root, rules);
        annotate_saved_dead_code(&mut dead_code_issues, envelope, dead_code_pointer, resolver);
        issues.extend(dead_code_issues);
    }
    if let Some(report) = duplication {
        let mut duplication_issues = api_duplication_codeclimate_issues(&report, root);
        if let Some(introduced) = saved_clone_group_flags(envelope, duplication_pointer) {
            fallow_api::set_duplication_audit_attribution(
                &mut duplication_issues,
                &report,
                introduced,
            );
        }
        annotate_duplication_groups(&mut duplication_issues, &report, root, resolver);
        issues.extend(duplication_issues);
    }
    if let Some(report) = health {
        let mut health_issues = api_health_codeclimate_issues(&report, root);
        annotate_health_groups(&mut health_issues, resolver);
        issues.extend(health_issues);
    }
    Ok(issues)
}

fn parse_optional_section<T: serde::de::DeserializeOwned>(
    envelope: &serde_json::Value,
    pointer: &str,
) -> Result<Option<T>, String> {
    let Some(value) = envelope.pointer(pointer).filter(|value| !value.is_null()) else {
        return Ok(None);
    };
    serde_json::from_value(value.clone())
        .map(Some)
        .map_err(|error| {
            format!("saved section `{pointer}` is incompatible with this Fallow version: {error}")
        })
}

fn native_category(rule_id: &str) -> &'static str {
    if rule_id == "fallow/code-duplication" {
        "Duplication"
    } else if rule_id.starts_with("fallow/high-") || rule_id == "fallow/complexity" {
        "Complexity"
    } else if rule_id.starts_with("fallow/css-") {
        "Style"
    } else if rule_id.contains("untested") {
        "Coverage"
    } else {
        "Bug Risk"
    }
}

const fn annotation_codeclimate_severity(level: AnnotationLevel) -> CodeClimateSeverity {
    match level {
        AnnotationLevel::Error => CodeClimateSeverity::Major,
        AnnotationLevel::Warning => CodeClimateSeverity::Minor,
        AnnotationLevel::Notice => CodeClimateSeverity::Info,
    }
}

fn sarif_level_to_codeclimate(level: &str) -> CodeClimateSeverity {
    match level {
        "error" => CodeClimateSeverity::Major,
        "warning" => CodeClimateSeverity::Minor,
        _ => CodeClimateSeverity::Info,
    }
}

fn envelope_fingerprint(rule_id: &str, path: &str, line: u32, message: &str) -> String {
    if rule_id == "fallow/unused-file" {
        return codeclimate_fingerprint_hash(&[rule_id, path]);
    }
    if matches!(
        rule_id,
        "fallow/unused-dependency"
            | "fallow/unused-dev-dependency"
            | "fallow/unused-optional-dependency"
            | "fallow/type-only-dependency"
            | "fallow/test-only-dependency"
            | "fallow/dev-dependency-in-production"
    ) && let Some(subject) = first_quoted_subject(message)
    {
        return codeclimate_fingerprint_hash(&[rule_id, subject]);
    }
    let line = line.to_string();
    if rule_id.starts_with("fallow/css-") {
        return codeclimate_fingerprint_hash(&[rule_id, path, &line, message]);
    }
    if let Some(subject) = first_quoted_subject(message) {
        codeclimate_fingerprint_hash(&[rule_id, path, &line, subject])
    } else {
        codeclimate_fingerprint_hash(&[rule_id, path, &line])
    }
}

fn first_quoted_subject(message: &str) -> Option<&str> {
    let (_, remainder) = message.split_once('\'')?;
    let (subject, _) = remainder.split_once('\'')?;
    (!subject.is_empty()).then_some(subject)
}

/// Compute a relative path string with forward-slash normalization.
///
/// Uses `normalize_uri` to ensure forward slashes on all platforms
/// and percent-encode brackets for Next.js dynamic routes.
fn cc_path(path: &Path, root: &Path) -> String {
    normalize_uri(&relative_path(path, root).display().to_string())
}

/// Fetch CodeClimate issues from the API-owned dead-code output builder.
///
/// Returns the typed [`CodeClimateIssue`] vec; callers that emit the wire
/// shape convert via [`fallow_output::codeclimate_issues_to_value`]. The schema
/// drift gate locks the per-issue shape against
/// [`fallow_output::CodeClimateOutput`].
#[must_use]
pub(super) fn api_codeclimate_issues(
    results: &AnalysisResults,
    root: &Path,
    rules: &RulesConfig,
) -> Vec<CodeClimateIssue> {
    fallow_api::build_codeclimate(results, root, rules)
}

/// Print dead-code analysis results in CodeClimate format.
pub(super) fn print_codeclimate(
    results: &AnalysisResults,
    root: &Path,
    rules: &RulesConfig,
) -> ExitCode {
    emit_codeclimate(api_codeclimate_issues(results, root, rules))
}

/// Print CodeClimate output with owner properties added to each issue.
///
/// Calls `api_codeclimate_issues` to produce the standard CodeClimate issues, then
/// annotates each entry with `"owner": "@team"` by resolving the
/// issue's location path through the `OwnershipResolver`.
pub(super) fn print_grouped_codeclimate(
    results: &AnalysisResults,
    root: &Path,
    rules: &RulesConfig,
    resolver: &OwnershipResolver,
) -> ExitCode {
    let mut issues = api_codeclimate_issues(results, root, rules);
    annotate_dead_code_owner(&mut issues, Some(resolver));
    emit_codeclimate(issues)
}

/// Fetch CodeClimate issues from the API-owned health output builder.
#[must_use]
pub(super) fn api_health_codeclimate_issues(
    report: &HealthReport,
    root: &Path,
) -> Vec<CodeClimateIssue> {
    fallow_api::build_health_codeclimate(report, root)
}

/// Print health analysis results in CodeClimate format.
pub(super) fn print_health_codeclimate(report: &HealthReport, root: &Path) -> ExitCode {
    emit_codeclimate(api_health_codeclimate_issues(report, root))
}

/// Print health CodeClimate output with a per-issue `group` field.
///
/// Mirrors the dead-code grouped CodeClimate pattern
/// (`print_grouped_codeclimate`): build the standard payload first, then
/// post-process each issue to attach a `group` key derived from the
/// `OwnershipResolver`. Lets GitLab Code Quality and other CodeClimate
/// consumers partition findings per team / package without re-parsing the
/// project structure.
pub(super) fn print_grouped_health_codeclimate(
    report: &HealthReport,
    root: &Path,
    resolver: &OwnershipResolver,
) -> ExitCode {
    let mut issues = api_health_codeclimate_issues(report, root);
    annotate_health_groups(&mut issues, Some(resolver));
    emit_codeclimate(issues)
}

/// Fetch CodeClimate issues from the API-owned duplication output builder.
#[must_use]
pub(super) fn api_duplication_codeclimate_issues(
    report: &DuplicationReport,
    root: &Path,
) -> Vec<CodeClimateIssue> {
    fallow_api::build_duplication_codeclimate(report, root)
}

/// Print duplication analysis results in CodeClimate format.
pub(super) fn print_duplication_codeclimate(report: &DuplicationReport, root: &Path) -> ExitCode {
    emit_codeclimate(api_duplication_codeclimate_issues(report, root))
}

/// Print duplication CodeClimate output with a per-issue `group` field.
///
/// Mirrors [`print_grouped_health_codeclimate`]: each clone group is attributed
/// to its largest owner ([`super::dupes_grouping::largest_owner`]) and every
/// CodeClimate issue emitted for that clone group's instances carries the same
/// top-level `group` key. Lets GitLab Code Quality and other CodeClimate
/// consumers partition findings per team / package / directory without
/// re-parsing the project structure.
pub(super) fn print_grouped_duplication_codeclimate(
    report: &DuplicationReport,
    root: &Path,
    resolver: &OwnershipResolver,
) -> ExitCode {
    let mut issues = api_duplication_codeclimate_issues(report, root);
    annotate_duplication_groups(&mut issues, report, root, Some(resolver));
    emit_codeclimate(issues)
}

#[cfg(test)]
#[path = "codeclimate_tests.rs"]
mod tests;
