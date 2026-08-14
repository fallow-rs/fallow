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
    /// Setup output schema version; serialized as the string `"1"`.
    pub schema_version: CoverageSetupSchemaVersion,
    /// Framework detected at the project root.
    pub framework_detected: CoverageSetupFramework,
    /// Package manager detected from lockfiles, when one was found.
    pub package_manager: Option<CoverageSetupPackageManager>,
    /// Runtimes the instrumentation must cover at the project root.
    pub runtime_targets: Vec<CoverageSetupRuntimeTarget>,
    /// Per-member setup guidance for workspace projects.
    pub members: Vec<CoverageSetupMember>,
    /// Coverage config that was written to disk, when setup wrote one.
    pub config_written: Option<serde_json::Value>,
    /// Shell commands the user should run to complete setup.
    pub commands: Vec<String>,
    /// Files the user must edit by hand, with reasons.
    pub files_to_edit: Vec<CoverageSetupFileToEdit>,
    /// Ready-to-paste code snippets for the files to edit.
    pub snippets: Vec<CoverageSetupSnippet>,
    /// Dockerfile additions needed for containerized capture, when relevant.
    pub dockerfile_snippet: Option<String>,
    /// Ordered human-readable follow-up instructions.
    pub next_steps: Vec<String>,
    /// Non-fatal problems encountered during detection.
    pub warnings: Vec<String>,
    /// `_meta` block with docs and field definitions, when requested.
    #[serde(rename = "_meta", default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<serde_json::Value>,
}

/// Schema-version discriminator for [`CoverageSetupOutput`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum CoverageSetupSchemaVersion {
    /// First release of the coverage setup format.
    #[serde(rename = "1")]
    V1,
}

/// Framework detected during coverage setup; drives which instrumentation
/// guidance is emitted.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "snake_case")]
pub enum CoverageSetupFramework {
    /// Next.js application.
    #[serde(rename = "nextjs")]
    NextJs,
    /// NestJS application.
    #[serde(rename = "nestjs")]
    NestJs,
    /// Nuxt application.
    Nuxt,
    /// SvelteKit application.
    #[serde(rename = "sveltekit")]
    SvelteKit,
    /// Astro application.
    Astro,
    /// Remix application.
    Remix,
    /// Vite-built application without a detected meta-framework.
    Vite,
    /// Node project without a detected framework or bundler.
    PlainNode,
    /// No framework signal was found.
    Unknown,
}

/// Package manager detected from the project's lockfile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum CoverageSetupPackageManager {
    /// npm (`package-lock.json`).
    Npm,
    /// pnpm (`pnpm-lock.yaml`).
    Pnpm,
    /// Yarn (`yarn.lock`).
    Yarn,
    /// Bun (`bun.lock` / `bun.lockb`).
    Bun,
}

/// Runtime environment coverage capture must instrument.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "lowercase")]
pub enum CoverageSetupRuntimeTarget {
    /// Server-side Node.js execution.
    Node,
    /// Client-side browser execution.
    Browser,
}

/// Per-workspace-member setup guidance inside [`CoverageSetupOutput::members`].
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct CoverageSetupMember {
    /// Package name of the workspace member.
    pub name: String,
    /// Member path relative to the workspace root.
    pub path: String,
    /// Framework detected for this member.
    pub framework_detected: CoverageSetupFramework,
    /// Package manager detected for this member, when one was found.
    pub package_manager: Option<CoverageSetupPackageManager>,
    /// Runtimes the instrumentation must cover for this member.
    pub runtime_targets: Vec<CoverageSetupRuntimeTarget>,
    /// Files the user must edit by hand, with reasons.
    pub files_to_edit: Vec<CoverageSetupFileToEdit>,
    /// Ready-to-paste code snippets for the files to edit.
    pub snippets: Vec<CoverageSetupSnippet>,
    /// Dockerfile additions needed for containerized capture, when relevant.
    pub dockerfile_snippet: Option<String>,
    /// Non-fatal problems encountered during detection.
    pub warnings: Vec<String>,
}

/// One manual edit the user must make to wire up coverage capture.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct CoverageSetupFileToEdit {
    /// File path relative to the project root.
    pub path: String,
    /// Why the file needs editing.
    pub reason: String,
}

/// Ready-to-paste code snippet accompanying a file edit.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct CoverageSetupSnippet {
    /// Short description of what the snippet does.
    pub label: String,
    /// File path the snippet belongs in.
    pub path: String,
    /// The snippet source text.
    pub content: String,
}

/// Schema-version discriminator for [`CoverageAnalyzeOutput`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum CoverageAnalyzeSchemaVersion {
    /// First release of the coverage analyze format.
    #[serde(rename = "1")]
    V1,
    /// Expands the required semantic omission reason-code enum.
    #[serde(rename = "2")]
    V2,
}

/// Envelope emitted by `fallow coverage analyze --format json`.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(
    feature = "schema",
    schemars(title = "fallow coverage analyze --format json")
)]
pub struct CoverageAnalyzeOutput {
    /// Analyze output schema version; currently serialized as the string `"2"`.
    pub schema_version: CoverageAnalyzeSchemaVersion,
    /// Fallow CLI version that produced this output.
    pub version: ToolVersion,
    /// Wall-clock analysis duration in milliseconds.
    pub elapsed_ms: ElapsedMs,
    /// Runtime-coverage report body.
    pub runtime_coverage: RuntimeCoverageReport,
    /// `_meta` block with docs and metric definitions, when `--explain` was
    /// passed.
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
        schema_version: CoverageAnalyzeSchemaVersion::V2,
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
        assert_eq!(value["schema_version"], "2");
        assert_eq!(value["elapsed_ms"], 7);
        assert_eq!(value["_meta"]["docs"], "coverage");
        assert_eq!(value["_meta"]["telemetry"]["analysis_run_id"], "run-2");
    }
}
