//! Type-aware command output envelopes.

use crate::root_envelopes::{RootEnvelopeMode, serialize_named_json_output};
use fallow_types::envelope::{SchemaVersion, ToolVersion};
use serde::Serialize;

/// Envelope emitted by `fallow type-aware status --format json`.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(
    feature = "schema",
    schemars(title = "fallow type-aware status --format json")
)]
pub struct TypeAwareStatusOutput {
    pub schema_version: SchemaVersion,
    pub version: ToolVersion,
    pub available: bool,
    pub discovery_source: Option<String>,
    /// Root-relative companion path, or only the executable name when the
    /// companion lives outside the analyzed project.
    pub companion_path: Option<String>,
    pub package_version: Option<String>,
    pub protocol_version: u32,
    pub backend_family: Option<String>,
    pub backend_version: Option<String>,
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
                protocol_version: 5,
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
