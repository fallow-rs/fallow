//! Coverage command output envelopes.

use crate::RuntimeCoverageReport;
use crate::root_envelopes::{RootEnvelopeMode, attach_telemetry_meta, serialize_named_json_output};
use fallow_types::envelope::{ElapsedMs, Meta, ToolVersion};
use serde::Serialize;
use std::time::Duration;

/// `fallow coverage setup --json` envelope.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "schema", schemars(title = "fallow coverage setup --json"))]
pub struct CoverageSetupOutput {
    pub schema_version: CoverageSetupSchemaVersion,
    pub framework_detected: CoverageSetupFramework,
    pub package_manager: Option<CoverageSetupPackageManager>,
    pub runtime_targets: Vec<CoverageSetupRuntimeTarget>,
    pub members: Vec<CoverageSetupMember>,
    pub config_written: Option<serde_json::Value>,
    pub commands: Vec<String>,
    pub files_to_edit: Vec<CoverageSetupFileToEdit>,
    pub snippets: Vec<CoverageSetupSnippet>,
    pub dockerfile_snippet: Option<String>,
    pub next_steps: Vec<String>,
    pub warnings: Vec<String>,
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum CoverageSetupSchemaVersion {
    #[serde(rename = "1")]
    V1,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum CoverageSetupFramework {
    #[serde(rename = "nextjs")]
    NextJs,
    #[serde(rename = "nestjs")]
    NestJs,
    Nuxt,
    #[serde(rename = "sveltekit")]
    SvelteKit,
    Astro,
    Remix,
    Vite,
    PlainNode,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum CoverageSetupPackageManager {
    Npm,
    Pnpm,
    Yarn,
    Bun,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum CoverageSetupRuntimeTarget {
    Node,
    Browser,
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct CoverageSetupMember {
    pub name: String,
    pub path: String,
    pub framework_detected: CoverageSetupFramework,
    pub package_manager: Option<CoverageSetupPackageManager>,
    pub runtime_targets: Vec<CoverageSetupRuntimeTarget>,
    pub files_to_edit: Vec<CoverageSetupFileToEdit>,
    pub snippets: Vec<CoverageSetupSnippet>,
    pub dockerfile_snippet: Option<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct CoverageSetupFileToEdit {
    pub path: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct CoverageSetupSnippet {
    pub label: String,
    pub path: String,
    pub content: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum CoverageAnalyzeSchemaVersion {
    #[serde(rename = "1")]
    V1,
}

#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(
    feature = "schema",
    schemars(title = "fallow coverage analyze --format json")
)]
pub struct CoverageAnalyzeOutput {
    pub schema_version: CoverageAnalyzeSchemaVersion,
    pub version: ToolVersion,
    pub elapsed_ms: ElapsedMs,
    pub runtime_coverage: RuntimeCoverageReport,
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Meta>,
}

/// Serialize the `fallow coverage setup --json` envelope.
///
/// # Errors
///
/// Returns a serde error when the envelope cannot be converted to JSON.
pub fn serialize_coverage_setup_json_output(
    output: CoverageSetupOutput,
    mode: RootEnvelopeMode,
    analysis_run_id: Option<&str>,
) -> Result<serde_json::Value, serde_json::Error> {
    let mut value = serialize_named_json_output(output, "coverage-setup", mode)?;
    attach_telemetry_meta(&mut value, analysis_run_id);
    Ok(value)
}

/// Build the `fallow coverage analyze --format json` envelope.
#[must_use]
pub fn build_coverage_analyze_output(
    report: &RuntimeCoverageReport,
    elapsed: Duration,
    version: impl Into<String>,
) -> CoverageAnalyzeOutput {
    CoverageAnalyzeOutput {
        schema_version: CoverageAnalyzeSchemaVersion::V1,
        version: ToolVersion(version.into()),
        elapsed_ms: ElapsedMs(u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX)),
        runtime_coverage: report.clone(),
        meta: None,
    }
}

/// Serialize the `fallow coverage analyze --format json` envelope.
///
/// `explain_meta` is inserted after typed-envelope serialization because the
/// existing command metadata is a JSON object shared with docs/schema helpers.
///
/// # Errors
///
/// Returns a serde error when the envelope cannot be converted to JSON.
pub fn serialize_coverage_analyze_json_output(
    output: CoverageAnalyzeOutput,
    mode: RootEnvelopeMode,
    explain_meta: Option<serde_json::Value>,
    analysis_run_id: Option<&str>,
) -> Result<serde_json::Value, serde_json::Error> {
    let mut value = serialize_named_json_output(output, "coverage-analyze", mode)?;
    if let Some(meta) = explain_meta
        && let Some(map) = value.as_object_mut()
    {
        map.insert("_meta".to_owned(), meta);
    }
    attach_telemetry_meta(&mut value, analysis_run_id);
    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn coverage_setup_json_output_uses_named_root_contract() {
        let output = CoverageSetupOutput {
            schema_version: CoverageSetupSchemaVersion::V1,
            framework_detected: CoverageSetupFramework::Unknown,
            package_manager: None,
            runtime_targets: Vec::new(),
            members: Vec::new(),
            config_written: None,
            commands: Vec::new(),
            files_to_edit: Vec::new(),
            snippets: Vec::new(),
            dockerfile_snippet: None,
            next_steps: Vec::new(),
            warnings: Vec::new(),
            meta: None,
        };

        let value =
            serialize_coverage_setup_json_output(output, RootEnvelopeMode::Tagged, Some("run-1"))
                .expect("coverage setup should serialize");

        assert_eq!(value["kind"], "coverage-setup");
        assert_eq!(value["schema_version"], "1");
        assert_eq!(value["_meta"]["telemetry"]["analysis_run_id"], "run-1");
    }

    #[test]
    fn coverage_analyze_json_output_inserts_explain_meta_and_telemetry() {
        let report = RuntimeCoverageReport::default();
        let output = build_coverage_analyze_output(&report, Duration::from_millis(7), "test");

        let value = serialize_coverage_analyze_json_output(
            output,
            RootEnvelopeMode::Tagged,
            Some(json!({"docs": "coverage"})),
            Some("run-2"),
        )
        .expect("coverage analyze should serialize");

        assert_eq!(value["kind"], "coverage-analyze");
        assert_eq!(value["schema_version"], "1");
        assert_eq!(value["elapsed_ms"], 7);
        assert_eq!(value["_meta"]["docs"], "coverage");
        assert_eq!(value["_meta"]["telemetry"]["analysis_run_id"], "run-2");
    }
}
