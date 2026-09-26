//! Feature flag output contracts.

use std::path::Path;
use std::time::Duration;

use fallow_types::envelope::{ElapsedMs, SchemaVersion, TelemetryMeta, ToolVersion};
use fallow_types::flag_retirement::FlagRetirementReport;
use fallow_types::results::{FeatureFlag, FlagConfidence, FlagKind};
use fallow_types::workspace::WorkspaceDiagnostic;
use serde::Serialize;

use crate::root_envelopes::{attach_telemetry_meta, serialize_named_json_output};

/// Current schema version for feature-flag JSON output.
///
/// Unmoved by the additive `workspace_diagnostics[]` field: it carries
/// `skip_serializing_if`, so a run that records no diagnostic emits the same
/// bytes as before. This is the rule `docs/backwards-compatibility.md` states
/// for additive optional fields, and the precedent the bare combined envelope
/// set when it gained the same array.
pub const FEATURE_FLAGS_SCHEMA_VERSION: u32 = 8;

/// Schema projection for the feature-flags envelope's exact version.
#[cfg(feature = "schema")]
#[allow(dead_code, reason = "schema-only type used by the field projection")]
#[derive(schemars::JsonSchema)]
#[schemars(extend("const" = FEATURE_FLAGS_SCHEMA_VERSION))]
struct FeatureFlagsSchemaVersion(u32);

/// Inputs for building `fallow flags --format json`.
pub struct FeatureFlagsOutputInput<'a> {
    /// Flags output schema version to report.
    pub schema_version: u32,
    /// Fallow CLI version to report.
    pub version: String,
    /// Wall-clock analysis duration; serialized as whole milliseconds.
    pub elapsed: Duration,
    /// Detected flags from the engine.
    pub flags: &'a [FeatureFlag],
    /// Analysis root paths are relativized against.
    pub root: &'a Path,
    /// Workspace- and source-discovery diagnostics the run recorded. Passed
    /// absolute; the builder relativizes them against `root`.
    pub workspace_diagnostics: Vec<WorkspaceDiagnostic>,
    /// What became of the narrowing requests the run received, or `None` when
    /// it received none.
    pub request_outcomes: Option<crate::RequestOutcomes>,
    /// `_meta` block to attach when `--explain` was passed.
    pub meta: Option<FeatureFlagsMeta>,
    /// Retirement report, present only with `--retirement`.
    pub retirement: Option<FlagRetirementReport>,
}

/// Envelope emitted by `fallow flags --format json`.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "schema", schemars(title = "fallow flags --format json"))]
pub struct FeatureFlagsOutput {
    /// Flags output schema version.
    #[cfg_attr(feature = "schema", schemars(with = "FeatureFlagsSchemaVersion"))]
    pub schema_version: SchemaVersion,
    /// Fallow CLI version that produced this output.
    pub version: ToolVersion,
    /// Wall-clock analysis duration in milliseconds.
    pub elapsed_ms: ElapsedMs,
    /// What the run was asked to narrow and whether it did. See
    /// [`crate::RequestOutcomes`] for the full contract.
    ///
    /// `fallow flags` accepts `--changed-since`, and an unresolvable ref widens
    /// the scan to the whole project rather than failing the run. Until this
    /// member existed the only account of that was a stderr line, which `--quiet`
    /// removes, so a flag inventory read as scoped to the change could silently
    /// be the whole project's (issue #2734).
    ///
    /// The command applies no diff filter, so the object carries the
    /// `changed-since` entry only. Omitted when the run was asked for nothing,
    /// which keeps a scan that passed no narrowing flag byte-identical and moves
    /// no `schema_version`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_outcomes: Option<crate::RequestOutcomes>,
    /// Detected feature-flag findings.
    pub feature_flags: Vec<FeatureFlagFinding>,
    /// Number of entries in `feature_flags`.
    pub total_flags: usize,
    /// Workspace-discovery and source-discovery diagnostics for the run. See
    /// `CheckOutput::workspace_diagnostics` for the full contract.
    ///
    /// A flags run walks and parses the project like every other analysis, so
    /// it records the same discovery kinds: a `skipped-large-file`,
    /// `skipped-minified-file`, or `source-read-failure` file was never
    /// scanned for flags, and a `source-parse-degraded` file was scanned from
    /// a partial module. Each is a reason a flag can be missing from
    /// `feature_flags[]`, which is exactly what a consumer reading a
    /// zero-result run needs to know. The analysis-stage kinds appear here
    /// too: the scan correlates flags with dead exports, so it runs the
    /// dead-code analyze pass that records them.
    ///
    /// Omitted when empty, so a project with no discovery noise sees no
    /// change.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub workspace_diagnostics: Vec<WorkspaceDiagnostic>,
    /// One row per flag with the reasons the flag can be retired.
    ///
    /// Present only with `--retirement`. Without that option the key is
    /// omitted, so the envelope stays byte-identical and `schema_version`
    /// does not move. The per-site `feature_flags[]` array is the same with
    /// and without the option.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub retirement: Option<FlagRetirementReport>,
    /// `_meta` block; see [`FeatureFlagsMeta`].
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<FeatureFlagsMeta>,
}

/// One feature flag finding in JSON output.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct FeatureFlagFinding {
    /// File path relative to the analysed root.
    pub path: String,
    /// Detected flag identifier, e.g. the env var or SDK key name.
    pub flag_name: String,
    /// Detection pattern the flag matched.
    pub kind: FeatureFlagKind,
    /// How confident the detector is that this is a real feature flag.
    pub confidence: FeatureFlagConfidence,
    /// 1-based line of the flag usage.
    pub line: u32,
    /// 1-based column of the flag usage.
    pub col: u32,
    /// Suggested follow-up actions (investigate / suppress).
    pub actions: Vec<FeatureFlagAction>,
    /// Flag SDK the call belongs to, for SDK-call findings.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sdk_name: Option<String>,
    /// Overlap with dead-code findings when the flag guards unused exports.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dead_code_overlap: Option<FeatureFlagDeadCodeOverlap>,
}

/// Feature flag kind values emitted in JSON.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum FeatureFlagKind {
    /// Environment-variable read used as a toggle.
    EnvironmentVariable,
    /// Feature-flag SDK evaluation call.
    SdkCall,
    /// Flag key in a configuration object literal.
    ConfigObject,
}

/// Feature flag confidence values emitted in JSON.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum FeatureFlagConfidence {
    /// Strong flag signal, e.g. a known SDK call.
    High,
    /// Plausible flag signal with some ambiguity, e.g. a generic SDK name
    /// such as `isEnabled` in a file that imports no flag SDK or flag module.
    Medium,
    /// Weak signal; likely needs human confirmation.
    Low,
}

/// Per-finding action emitted for feature flag findings.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct FeatureFlagAction {
    /// Action discriminator, serialized as `type`.
    #[serde(rename = "type")]
    pub kind: FeatureFlagActionType,
    /// Whether `fallow fix` can apply the action automatically.
    pub auto_fixable: bool,
    /// Human-readable action description.
    pub description: String,
    /// Suppression comment to insert, for suppress actions.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub comment: Option<String>,
}

/// Feature flag action discriminants.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum FeatureFlagActionType {
    /// Check whether the flag is still needed.
    InvestigateFlag,
    /// Suppress the finding with a `fallow-ignore` line comment.
    SuppressLine,
}

/// Dead-code overlap block attached when a flag guards unused exports.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct FeatureFlagDeadCodeOverlap {
    /// Lines inside the flag-guarded region.
    pub guarded_lines: u32,
    /// Number of unused exports the flag guards.
    pub dead_export_count: usize,
    /// Names of the unused exports the flag guards.
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
    /// What the flags command reports.
    pub description: &'static str,
    /// Explanation of each `kind` value.
    pub kinds: FeatureFlagsKindMeta,
    /// Explanation of each `confidence` value.
    pub confidence: FeatureFlagsConfidenceMeta,
    /// Public documentation URL for the flags command.
    pub docs: &'static str,
}

/// Feature flag kind explanations.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct FeatureFlagsKindMeta {
    /// Explanation of the `environment_variable` kind.
    pub environment_variable: &'static str,
    /// Explanation of the `sdk_call` kind.
    pub sdk_call: &'static str,
    /// Explanation of the `config_object` kind.
    pub config_object: &'static str,
}

/// Feature flag confidence explanations.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct FeatureFlagsConfidenceMeta {
    /// Explanation of the `high` confidence level.
    pub high: &'static str,
    /// Explanation of the `medium` confidence level.
    pub medium: &'static str,
    /// Explanation of the `low` confidence level.
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
    // This envelope has no post-serialisation root-prefix strip, so the
    // diagnostics are relativized here or they reach the wire as host paths.
    let root = input.root;
    let workspace_diagnostics = input
        .workspace_diagnostics
        .into_iter()
        .map(|diagnostic| diagnostic.into_root_relative(root))
        .collect();
    FeatureFlagsOutput {
        schema_version: SchemaVersion(input.schema_version),
        version: ToolVersion(input.version),
        elapsed_ms: ElapsedMs(input.elapsed.as_millis() as u64),
        request_outcomes: input.request_outcomes,
        feature_flags,
        total_flags: input.flags.len(),
        workspace_diagnostics,
        retirement: input.retirement,
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
    analysis_run_id: Option<&str>,
) -> Result<serde_json::Value, serde_json::Error> {
    let mut value = serialize_named_json_output(output, "feature-flags")?;
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
            workspace_diagnostics: Vec::new(),
            request_outcomes: None,
            meta: Some(feature_flags_meta()),
            retirement: None,
        });

        let value = serialize_feature_flags_json_output(output, Some("run-flags"))
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
            workspace_diagnostics: Vec::new(),
            request_outcomes: None,
            meta: None,
            retirement: None,
        });

        let value = serialize_feature_flags_json_output(output, Some("run-flags"))
            .expect("feature flags output should serialize");

        assert_eq!(value["_meta"]["telemetry"]["analysis_run_id"], "run-flags");
        assert!(
            value["_meta"].get("feature_flags").is_none(),
            "feature_flags details are absent without --explain"
        );
    }

    #[test]
    fn recorded_diagnostics_reach_the_flags_envelope_root_relative() {
        // A flags run that skipped a file scanned it for nothing. Without this
        // array the skip was unreachable from both channels on this command:
        // stderr no longer carries every kind, and the envelope had no key.
        let output = build_feature_flags_output(FeatureFlagsOutputInput {
            schema_version: FEATURE_FLAGS_SCHEMA_VERSION,
            version: "0.0.0".to_string(),
            elapsed: Duration::from_millis(4),
            flags: &[flag()],
            root: Path::new("/repo"),
            workspace_diagnostics: vec![WorkspaceDiagnostic::new(
                Path::new("/repo"),
                PathBuf::from("/repo/src/generated.ts"),
                fallow_types::workspace::WorkspaceDiagnosticKind::SkippedLargeFile {
                    size_bytes: 9_000_000,
                },
            )],
            request_outcomes: None,
            meta: None,
            retirement: None,
        });

        let value = serialize_feature_flags_json_output(output, None)
            .expect("feature flags output should serialize");

        assert_eq!(
            value["workspace_diagnostics"][0]["path"], "src/generated.ts",
            "the envelope has no post-serialisation strip, so the builder must relativize"
        );
        assert_eq!(
            value["workspace_diagnostics"][0]["kind"], "skipped-large-file",
            "the typed kind reaches the wire, envelope was {value}"
        );
    }

    #[test]
    fn a_clean_run_omits_the_diagnostics_key_entirely() {
        let output = build_feature_flags_output(FeatureFlagsOutputInput {
            schema_version: FEATURE_FLAGS_SCHEMA_VERSION,
            version: "0.0.0".to_string(),
            elapsed: Duration::from_millis(4),
            flags: &[flag()],
            root: Path::new("/repo"),
            workspace_diagnostics: Vec::new(),
            request_outcomes: None,
            meta: None,
            retirement: None,
        });

        let value = serialize_feature_flags_json_output(output, None)
            .expect("feature flags output should serialize");

        assert!(
            value.get("workspace_diagnostics").is_none(),
            "an empty array is omitted so a quiet project sees no wire change"
        );
    }
}
