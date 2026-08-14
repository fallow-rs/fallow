//! Type-aware command output envelopes.

use crate::root_envelopes::{RootEnvelopeMode, serialize_named_json_output};
use fallow_types::envelope::{SchemaVersion, ToolVersion};
use serde::Serialize;

/// Current schema version for type-aware status JSON output.
pub const TYPE_AWARE_STATUS_SCHEMA_VERSION: u32 = 8;

/// Schema projection for the type-aware status envelope's exact version.
#[cfg(feature = "schema")]
#[allow(dead_code, reason = "schema-only type used by the field projection")]
#[derive(schemars::JsonSchema)]
#[schemars(extend("const" = TYPE_AWARE_STATUS_SCHEMA_VERSION))]
struct TypeAwareStatusSchemaVersion(u32);

/// Envelope emitted by `fallow type-aware status --format json`.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(
    feature = "schema",
    schemars(title = "fallow type-aware status --format json")
)]
pub struct TypeAwareStatusOutput {
    /// Type-aware status schema version.
    #[cfg_attr(feature = "schema", schemars(with = "TypeAwareStatusSchemaVersion"))]
    pub schema_version: SchemaVersion,
    /// Fallow CLI version that produced this output.
    pub version: ToolVersion,
    /// Whether a usable type-aware companion was found.
    pub available: bool,
    /// How the companion was discovered, e.g. `installed-sibling`.
    pub discovery_source: Option<String>,
    /// Root-relative companion path, or only the executable name when the
    /// companion lives outside the analyzed project.
    pub companion_path: Option<String>,
    /// npm package version of the companion, when known.
    pub package_version: Option<String>,
    /// Type-aware protocol version fallow speaks.
    pub protocol_version: u32,
    /// Checker backend family, e.g. `typescript-go`.
    pub backend_family: Option<String>,
    /// Version of the checker backend, when known.
    pub backend_version: Option<String>,
    /// How to make the companion available, when it is not.
    pub remediation: Option<String>,
}

/// Serialize the type-aware status envelope with its root discriminator.
///
/// # Errors
///
/// Returns a serde error when the status output cannot be converted to JSON.
pub fn serialize_type_aware_status_json_output(
    output: TypeAwareStatusOutput,
    mode: RootEnvelopeMode,
) -> Result<serde_json::Value, serde_json::Error> {
    serialize_named_json_output(output, "type-aware-status", mode)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn status_json_uses_tagged_root_contract() {
        let value = serialize_type_aware_status_json_output(
            TypeAwareStatusOutput {
                schema_version: SchemaVersion(7),
                version: ToolVersion("3.8.1".to_string()),
                available: true,
                discovery_source: Some("installed-sibling".to_string()),
                companion_path: Some("node_modules/.bin/fallow-type-aware".to_string()),
                package_version: Some("3.8.1".to_string()),
                protocol_version: 7,
                backend_family: Some("typescript-go".to_string()),
                backend_version: Some("7.0.2".to_string()),
                remediation: None,
            },
            RootEnvelopeMode::Tagged,
        )
        .expect("type-aware status should serialize");

        assert_eq!(value["kind"], "type-aware-status");
        assert_eq!(
            value["companion_path"],
            "node_modules/.bin/fallow-type-aware"
        );
    }
}
