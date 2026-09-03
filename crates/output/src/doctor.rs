//! Typed readiness report emitted by `fallow doctor`.

use crate::root_envelopes::{RootEnvelopeMode, serialize_named_json_output};
use fallow_types::envelope::{SchemaVersion, ToolVersion};
use serde::Serialize;

/// Current schema version for `fallow doctor --format json`.
pub const DOCTOR_SCHEMA_VERSION: u32 = 1;

/// Schema projection for the exact doctor envelope version.
#[cfg(feature = "schema")]
#[allow(dead_code, reason = "schema-only type used by the field projection")]
#[derive(schemars::JsonSchema)]
#[schemars(extend("const" = DOCTOR_SCHEMA_VERSION))]
struct DoctorSchemaVersion(u32);

/// Schema projection for `.` as the privacy-safe diagnosed project root.
#[cfg(feature = "schema")]
#[allow(dead_code, reason = "schema-only type used by the field projection")]
#[derive(schemars::JsonSchema)]
#[schemars(extend("const" = "."))]
struct DoctorProjectRoot(String);

/// Aggregate readiness outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum DoctorStatus {
    /// Every applicable check passed.
    Pass,
    /// Readiness is usable, with one or more advisory warnings.
    Warn,
    /// At least one required readiness check failed.
    Fail,
}

/// Per-check readiness outcome.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum DoctorCheckStatus {
    /// The check completed successfully.
    Pass,
    /// The check completed with an advisory concern.
    Warn,
    /// The check found a blocking readiness problem.
    Fail,
    /// The check does not apply or an earlier prerequisite failed.
    Skipped,
}

/// Stable identifier for a doctor check. Declaration order is output order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum DoctorCheckId {
    /// Project root availability.
    Root,
    /// Fallow configuration resolution.
    Config,
    /// Workspace declaration discovery.
    Workspaces,
    /// External plugin configuration and activation.
    Plugins,
    /// Optional type-aware companion discovery.
    TypeAware,
}

/// Stable category for a doctor check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[serde(rename_all = "kebab-case")]
pub enum DoctorCheckCategory {
    /// Project filesystem readiness.
    Project,
    /// Fallow configuration readiness.
    Configuration,
    /// Workspace topology readiness.
    Workspace,
    /// Plugin readiness.
    Plugin,
    /// Optional companion readiness.
    Companion,
}

/// Actionable remediation attached to a doctor check.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct DoctorRemediation {
    /// Command the user or agent can choose to run.
    pub command: String,
    /// Working directory for the command, relative to the diagnosed root.
    #[cfg_attr(feature = "schema", schemars(with = "DoctorProjectRoot"))]
    pub cwd: String,
    /// Whether running the command can modify project files or dependencies.
    pub mutating: bool,
}

/// One deterministic doctor check result.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct DoctorCheck {
    /// Stable check identifier.
    pub id: DoctorCheckId,
    /// Readiness area this check covers.
    pub category: DoctorCheckCategory,
    /// Check outcome.
    pub status: DoctorCheckStatus,
    /// Whether failure makes the project not ready.
    pub required: bool,
    /// Human-readable result with no host-specific absolute paths.
    pub message: String,
    /// Optional actionable next command.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remediation: Option<DoctorRemediation>,
}

/// Counts for every per-check status.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct DoctorSummary {
    /// Successful checks.
    pub pass: usize,
    /// Advisory checks.
    pub warn: usize,
    /// Failed checks.
    pub fail: usize,
    /// Inapplicable or prerequisite-blocked checks.
    pub skipped: usize,
}

/// Versioned readiness envelope emitted by `fallow doctor --format json`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "schema", schemars(title = "fallow doctor --format json"))]
pub struct DoctorOutput {
    /// Independent doctor contract version.
    #[cfg_attr(feature = "schema", schemars(with = "DoctorSchemaVersion"))]
    pub schema_version: SchemaVersion,
    /// Fallow version that produced the report.
    pub version: ToolVersion,
    /// Stable project-root identifier. Always `.` to avoid exposing host paths.
    #[cfg_attr(feature = "schema", schemars(with = "DoctorProjectRoot"))]
    pub root: String,
    /// Aggregate readiness outcome.
    pub status: DoctorStatus,
    /// Counts derived from `checks`.
    pub summary: DoctorSummary,
    /// Checks in stable contract order.
    pub checks: Vec<DoctorCheck>,
}

/// Serialize a doctor report with its root discriminator.
///
/// # Errors
///
/// Returns a serde error if the typed report cannot be converted to JSON.
pub fn serialize_doctor_json_output(
    output: DoctorOutput,
    mode: RootEnvelopeMode,
) -> Result<serde_json::Value, serde_json::Error> {
    serialize_named_json_output(output, "doctor", mode)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tagged_json_uses_the_stable_privacy_safe_contract() {
        let value = serialize_doctor_json_output(
            DoctorOutput {
                schema_version: SchemaVersion(DOCTOR_SCHEMA_VERSION),
                version: ToolVersion("1.2.3".to_string()),
                root: ".".to_string(),
                status: DoctorStatus::Warn,
                summary: DoctorSummary {
                    pass: 4,
                    warn: 1,
                    fail: 0,
                    skipped: 0,
                },
                checks: vec![DoctorCheck {
                    id: DoctorCheckId::TypeAware,
                    category: DoctorCheckCategory::Companion,
                    status: DoctorCheckStatus::Warn,
                    required: false,
                    message: "Companion is unavailable.".to_string(),
                    remediation: Some(DoctorRemediation {
                        command: "npm install --save-dev fallow-type-aware@1.2.3".to_string(),
                        cwd: ".".to_string(),
                        mutating: true,
                    }),
                }],
            },
            RootEnvelopeMode::Tagged,
        )
        .expect("serialize doctor output");

        assert_eq!(value["kind"], "doctor");
        assert_eq!(value["schema_version"], DOCTOR_SCHEMA_VERSION);
        assert_eq!(value["root"], ".");
        assert_eq!(value["checks"][0]["remediation"]["cwd"], ".");
        assert_eq!(value["checks"][0]["remediation"]["mutating"], true);
        assert!(
            value["checks"][0]["remediation"]
                .get("mutates_project")
                .is_none()
        );
    }
}
