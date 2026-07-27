use std::env;
use std::fs;
use std::io;
use std::path::PathBuf;

use serde::Deserialize;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProtocolManifest {
    schema_version: u32,
    wire_protocol_version: u32,
    semantic_schema_version: u32,
    analysis_operation: String,
    status_operation: String,
    query_operations: Vec<String>,
    session_envelope_types: Vec<String>,
    backend: Backend,
    sidecar: Sidecar,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Backend {
    family: String,
    version: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Sidecar {
    package: String,
    version_source: String,
}

fn rust_string(value: &str) -> String {
    format!("{value:?}")
}

fn rust_string_slice(values: &[String]) -> String {
    values
        .iter()
        .map(|value| rust_string(value))
        .collect::<Vec<_>>()
        .join(", ")
}

fn invalid_manifest(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    const MANIFEST_PATH: &str = "type-aware-protocol.json";
    println!("cargo:rerun-if-changed={MANIFEST_PATH}");

    let source = fs::read_to_string(MANIFEST_PATH)?;
    let manifest: ProtocolManifest = serde_json::from_str(&source)?;
    if manifest.schema_version != 1 {
        return Err(invalid_manifest("unsupported manifest schema").into());
    }
    if manifest.wire_protocol_version == 0 || manifest.semantic_schema_version == 0 {
        return Err(
            invalid_manifest("protocol and semantic schema versions must be positive").into(),
        );
    }
    if manifest.query_operations.is_empty() {
        return Err(invalid_manifest("query operations must not be empty").into());
    }
    if manifest.sidecar.version_source != "workspace-package" {
        return Err(invalid_manifest("unsupported sidecar version source").into());
    }

    let generated = format!(
        "// Generated from crates/api/type-aware-protocol.json. Do not edit.\n\
         pub(super) const WIRE_PROTOCOL_VERSION: u32 = {};\n\
         pub(super) const SEMANTIC_SCHEMA_VERSION: u32 = {};\n\
         pub(super) const ANALYSIS_OPERATION: &str = {};\n\
         pub(super) const STATUS_OPERATION: &str = {};\n\
         pub(super) const QUERY_OPERATIONS: &[&str] = &[{}];\n\
         pub(super) const SESSION_ENVELOPE_TYPES: &[&str] = &[{}];\n\
         pub(super) const BACKEND_FAMILY: &str = {};\n\
         pub(super) const BACKEND_VERSION: &str = {};\n\
         pub(super) const SIDECAR_PACKAGE: &str = {};\n",
        manifest.wire_protocol_version,
        manifest.semantic_schema_version,
        rust_string(&manifest.analysis_operation),
        rust_string(&manifest.status_operation),
        rust_string_slice(&manifest.query_operations),
        rust_string_slice(&manifest.session_envelope_types),
        rust_string(&manifest.backend.family),
        rust_string(&manifest.backend.version),
        rust_string(&manifest.sidecar.package),
    );
    let output = PathBuf::from(
        env::var_os("OUT_DIR")
            .ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "OUT_DIR is not set"))?,
    );
    fs::write(output.join("type_aware_protocol.rs"), generated)?;
    Ok(())
}
