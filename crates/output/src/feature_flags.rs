//! Feature flag output contracts.

use std::path::Path;
use std::time::Duration;

use fallow_types::envelope::{ElapsedMs, SchemaVersion, TelemetryMeta, ToolVersion};
use fallow_types::results::{FeatureFlag, FlagConfidence, FlagKind};
use serde::Serialize;

use crate::root_envelopes::{RootEnvelopeMode, attach_telemetry_meta, serialize_named_json_output};

/// Inputs for building `fallow flags --format json`.
pub struct FeatureFlagsOutputInput<'a> {
    pub schema_version: u32,
    pub version: String,
    pub elapsed: Duration,
    pub flags: &'a [FeatureFlag],
    pub root: &'a Path,
    pub meta: Option<FeatureFlagsMeta>,
}

/// Envelope emitted by `fallow flags --format json`.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "schema", schemars(title = "fallow flags --format json"))]
pub struct FeatureFlagsOutput {
    pub schema_version: SchemaVersion,
    pub version: ToolVersion,
    pub elapsed_ms: ElapsedMs,
    pub feature_flags: Vec<FeatureFlagFinding>,
    pub total_flags: usize,
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<FeatureFlagsMeta>,
}

/// One feature flag finding in JSON output.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct FeatureFlagFinding {
    pub path: String,
    pub flag_name: String,
    pub kind: FeatureFlagKind,
    pub confidence: FeatureFlagConfidence,
    pub line: u32,
    pub col: u32,
    pub actions: Vec<FeatureFlagAction>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sdk_name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dead_code_overlap: Option<FeatureFlagDeadCodeOverlap>,
}

/// Feature flag kind values emitted in JSON.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum FeatureFlagKind {
    EnvironmentVariable,
    SdkCall,
    ConfigObject,
}

/// Feature flag confidence values emitted in JSON.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum FeatureFlagConfidence {
    High,
    Medium,
    Low,
}

/// Per-finding action emitted for feature flag findings.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct FeatureFlagAction {
    #[serde(rename = "type")]
    pub kind: FeatureFlagActionType,
    pub auto_fixable: bool,
    pub description: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
}

/// Feature flag action discriminants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum FeatureFlagActionType {
    InvestigateFlag,
    SuppressLine,
}

/// Dead-code overlap block attached when a flag guards unused exports.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct FeatureFlagDeadCodeOverlap {
    pub guarded_lines: u32,
    pub dead_export_count: usize,
    pub dead_exports: Vec<String>,
}

/// Optional `_meta` block for [`FeatureFlagsOutput`]. Both fields are optional
/// because the two contributors are independent: `feature_flags` details are
/// present only with `--explain`, and `telemetry` is injected post-pass by
/// [`attach_telemetry_meta`] whenever an analysis run id is available (which is
/// the default path). Mirrors `Meta` / `CombinedMeta`, which also model
/// `telemetry` as an optional, never-required property.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct FeatureFlagsMeta {
    /// Feature-flag detection explanations, emitted only with `--explain`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub feature_flags: Option<FeatureFlagsMetaDetails>,
    /// Local telemetry correlation metadata for agent follow-up runs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub telemetry: Option<TelemetryMeta>,
}

/// Feature flag explanatory metadata.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct FeatureFlagsMetaDetails {
    pub description: &'static str,
    pub kinds: FeatureFlagsKindMeta,
    pub confidence: FeatureFlagsConfidenceMeta,
    pub docs: &'static str,
}

/// Feature flag kind explanations.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct FeatureFlagsKindMeta {
    pub environment_variable: &'static str,
    pub sdk_call: &'static str,
    pub config_object: &'static str,
}

/// Feature flag confidence explanations.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct FeatureFlagsConfidenceMeta {
    pub high: &'static str,
    pub medium: &'static str,
    pub low: &'static str,
}

/// Build the typed feature flags output envelope.
#[must_use]
pub fn build_feature_flags_output(input: FeatureFlagsOutputInput<'_>) -> FeatureFlagsOutput {
    let feature_flags = input
        .flags
        .iter()
        .map(|flag| feature_flag_finding(flag, input.root))
        .collect();
    FeatureFlagsOutput {
        schema_version: SchemaVersion(input.schema_version),
        version: ToolVersion(input.version),
        elapsed_ms: ElapsedMs(input.elapsed.as_millis() as u64),
        feature_flags,
        total_flags: input.flags.len(),
        meta: input.meta,
    }
}

/// Serialize `fallow flags --format json`.
///
/// # Errors
///
/// Returns a serde error when the feature flags output cannot be converted to
/// JSON.
pub fn serialize_feature_flags_json_output(
    output: FeatureFlagsOutput,
    mode: RootEnvelopeMode,
    analysis_run_id: Option<&str>,
) -> Result<serde_json::Value, serde_json::Error> {
    let mut value = serialize_named_json_output(output, "feature-flags", mode)?;
    attach_telemetry_meta(&mut value, analysis_run_id);
    Ok(value)
}

/// Metadata emitted when `fallow flags --explain --format json` is requested.
#[must_use]
pub const fn feature_flags_meta() -> FeatureFlagsMeta {
    FeatureFlagsMeta {
        telemetry: None,
        feature_flags: Some(FeatureFlagsMetaDetails {
            description: "Feature flag patterns detected via AST analysis",
            kinds: FeatureFlagsKindMeta {
                environment_variable: "process.env.FEATURE_* pattern (high confidence)",
                sdk_call: "Feature flag SDK function call (high confidence)",
                config_object: "Config object property access matching flag keywords (low confidence, heuristic)",
            },
            confidence: FeatureFlagsConfidenceMeta {
                high: "Unambiguous pattern match (env vars, direct SDK calls)",
                medium: "Pattern match with some ambiguity",
                low: "Heuristic match (config objects), may produce false positives",
            },
            docs: "https://docs.fallow.tools/cli/flags",
        }),
    }
}

fn feature_flag_finding(flag: &FeatureFlag, root: &Path) -> FeatureFlagFinding {
    let path = flag
        .path
        .strip_prefix(root)
        .unwrap_or(&flag.path)
        .to_string_lossy()
        .replace('\\', "/");
    FeatureFlagFinding {
        path,
        flag_name: flag.flag_name.clone(),
        kind: feature_flag_kind(flag.kind),
        confidence: feature_flag_confidence(flag.confidence),
        line: flag.line,
        col: flag.col,
        actions: feature_flag_actions(&flag.flag_name),
        sdk_name: flag.sdk_name.clone(),
        dead_code_overlap: feature_flag_dead_code_overlap(flag),
    }
}

const fn feature_flag_kind(kind: FlagKind) -> FeatureFlagKind {
    match kind {
        FlagKind::EnvironmentVariable => FeatureFlagKind::EnvironmentVariable,
        FlagKind::SdkCall => FeatureFlagKind::SdkCall,
        FlagKind::ConfigObject => FeatureFlagKind::ConfigObject,
    }
}

const fn feature_flag_confidence(confidence: FlagConfidence) -> FeatureFlagConfidence {
    match confidence {
        FlagConfidence::High => FeatureFlagConfidence::High,
        FlagConfidence::Medium => FeatureFlagConfidence::Medium,
        FlagConfidence::Low => FeatureFlagConfidence::Low,
    }
}

fn feature_flag_actions(flag_name: &str) -> Vec<FeatureFlagAction> {
    vec![
        FeatureFlagAction {
            kind: FeatureFlagActionType::InvestigateFlag,
            auto_fixable: false,
            description: format!("Verify whether feature flag '{flag_name}' is still active"),
            comment: None,
        },
        FeatureFlagAction {
            kind: FeatureFlagActionType::SuppressLine,
            auto_fixable: false,
            description: "Suppress with an inline comment".to_string(),
            comment: Some("// fallow-ignore-next-line feature-flag".to_string()),
        },
    ]
}

fn feature_flag_dead_code_overlap(flag: &FeatureFlag) -> Option<FeatureFlagDeadCodeOverlap> {
    if flag.guarded_dead_exports.is_empty() {
        return None;
    }
    let guarded_lines = flag
        .guard_line_start
        .and_then(|start| flag.guard_line_end.map(|end| end.saturating_sub(start) + 1))
        .unwrap_or(0);
    Some(FeatureFlagDeadCodeOverlap {
        guarded_lines,
        dead_export_count: flag.guarded_dead_exports.len(),
        dead_exports: flag.guarded_dead_exports.clone(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn flag() -> FeatureFlag {
        FeatureFlag {
            path: PathBuf::from("/repo/src/app.ts"),
            flag_name: "FEATURE_CHECKOUT".to_string(),
            kind: FlagKind::EnvironmentVariable,
            confidence: FlagConfidence::High,
            line: 10,
            col: 4,
            guard_span_start: None,
            guard_span_end: None,
            sdk_name: None,
            guard_line_start: Some(10),
            guard_line_end: Some(12),
            guarded_dead_exports: vec!["legacyCheckout".to_string()],
        }
    }

    #[test]
    fn feature_flags_json_output_uses_output_owned_root_contract() {
        let output = build_feature_flags_output(FeatureFlagsOutputInput {
            schema_version: 7,
            version: "0.0.0".to_string(),
            elapsed: Duration::from_millis(4),
            flags: &[flag()],
            root: Path::new("/repo"),
            meta: Some(feature_flags_meta()),
        });

        let value = serialize_feature_flags_json_output(
            output,
            RootEnvelopeMode::Tagged,
            Some("run-flags"),
        )
        .expect("feature flags output should serialize");

        assert_eq!(value["kind"], "feature-flags");
        assert_eq!(value["feature_flags"][0]["path"], "src/app.ts");
        assert_eq!(
            value["feature_flags"][0]["dead_code_overlap"]["guarded_lines"],
            3
        );
        assert_eq!(
            value["_meta"]["feature_flags"]["docs"],
            "https://docs.fallow.tools/cli/flags"
        );
        assert_eq!(value["_meta"]["telemetry"]["analysis_run_id"], "run-flags");
    }

    #[test]
    fn feature_flags_json_output_without_explain_emits_telemetry_only_meta() {
        // The default path (no --explain) leaves `meta` as None, so the only
        // `_meta` contributor is the post-pass telemetry injection. The typed
        // `FeatureFlagsMeta` must model this telemetry-only shape (both fields
        // optional) so the emitted document conforms to the published schema.
        let output = build_feature_flags_output(FeatureFlagsOutputInput {
            schema_version: 7,
            version: "0.0.0".to_string(),
            elapsed: Duration::from_millis(4),
            flags: &[flag()],
            root: Path::new("/repo"),
            meta: None,
        });

        let value = serialize_feature_flags_json_output(
            output,
            RootEnvelopeMode::Tagged,
            Some("run-flags"),
        )
        .expect("feature flags output should serialize");

        assert_eq!(value["_meta"]["telemetry"]["analysis_run_id"], "run-flags");
        assert!(
            value["_meta"].get("feature_flags").is_none(),
            "feature_flags details are absent without --explain"
        );
    }
}
