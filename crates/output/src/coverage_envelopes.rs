//! Coverage command output envelopes.

use crate::RuntimeCoverageReport;
use fallow_types::envelope::{ElapsedMs, Meta, ToolVersion};
use serde::Serialize;

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
