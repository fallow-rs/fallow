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

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SimilarCodeProtocolManifest {
    schema_version: u32,
    wire_protocol_version: u32,
    extraction_semantics_version: u32,
    embedding_semantics_version: u32,
    analysis_operation: String,
    status_operation: String,
    setup_operation: String,
    session_envelope_types: Vec<String>,
    model: SimilarCodeModel,
    sidecar: SimilarCodeSidecar,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SimilarCodeModel {
    id: String,
    revision: String,
    license: String,
    dimensions: usize,
    max_tokens: usize,
    normalization: String,
    artifacts: Vec<SimilarCodeArtifact>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SimilarCodeArtifact {
    path: String,
    size: u64,
    sha256: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SimilarCodeSidecar {
    binary: String,
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
    generate_similar_code_protocol(&output)?;
    Ok(())
}

fn generate_similar_code_protocol(
    output: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error>> {
    const MANIFEST_PATH: &str = "similar-code-protocol.json";
    println!("cargo:rerun-if-changed={MANIFEST_PATH}");

    let source = fs::read_to_string(MANIFEST_PATH)?;
    let manifest: SimilarCodeProtocolManifest = serde_json::from_str(&source)?;
    if manifest.schema_version != 1 {
        return Err(invalid_manifest("unsupported similar-code manifest schema").into());
    }
    if manifest.wire_protocol_version == 0
        || manifest.extraction_semantics_version == 0
        || manifest.embedding_semantics_version == 0
        || manifest.model.dimensions == 0
        || manifest.model.max_tokens == 0
        || manifest.model.artifacts.is_empty()
    {
        return Err(invalid_manifest("similar-code protocol values must be positive").into());
    }
    if manifest.model.revision.len() != 40
        || !manifest
            .model
            .revision
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
        || manifest.model.artifacts.iter().any(|artifact| {
            artifact.path.is_empty()
                || artifact.size == 0
                || artifact.sha256.len() != 64
                || !artifact.sha256.bytes().all(|byte| byte.is_ascii_hexdigit())
        })
    {
        return Err(invalid_manifest("invalid similar-code model provenance").into());
    }
    if manifest.sidecar.version_source != "workspace-package" {
        return Err(invalid_manifest("unsupported similar-code sidecar version source").into());
    }

    let artifacts = manifest
        .model
        .artifacts
        .iter()
        .map(|artifact| {
            format!(
                "SimilarCodeArtifact {{ path: {}, size: {}, sha256: {} }}",
                rust_string(&artifact.path),
                artifact.size,
                rust_string(&artifact.sha256),
            )
        })
        .collect::<Vec<_>>()
        .join(", ");
    let generated = format!(
        "// Generated from crates/api/similar-code-protocol.json. Do not edit.\n\
         pub(super) const WIRE_PROTOCOL_VERSION: u32 = {};\n\
         pub(super) const EXTRACTION_SEMANTICS_VERSION: u32 = {};\n\
         pub(super) const EMBEDDING_SEMANTICS_VERSION: u32 = {};\n\
         pub(super) const ANALYSIS_OPERATION: &str = {};\n\
         pub(super) const STATUS_OPERATION: &str = {};\n\
         pub(super) const SETUP_OPERATION: &str = {};\n\
         pub(super) const SESSION_ENVELOPE_TYPES: &[&str] = &[{}];\n\
         pub(super) const MODEL_ID: &str = {};\n\
         pub(super) const MODEL_REVISION: &str = {};\n\
         pub(super) const MODEL_LICENSE: &str = {};\n\
         pub(super) const MODEL_DIMENSIONS: usize = {};\n\
         pub(super) const MODEL_MAX_TOKENS: usize = {};\n\
         pub(super) const MODEL_NORMALIZATION: &str = {};\n\
         pub(super) const MODEL_ARTIFACTS: &[SimilarCodeArtifact] = &[{}];\n\
         pub(super) const SIDECAR_BINARY: &str = {};\n\
         pub(super) const SIDECAR_PACKAGE: &str = {};\n",
        manifest.wire_protocol_version,
        manifest.extraction_semantics_version,
        manifest.embedding_semantics_version,
        rust_string(&manifest.analysis_operation),
        rust_string(&manifest.status_operation),
        rust_string(&manifest.setup_operation),
        rust_string_slice(&manifest.session_envelope_types),
        rust_string(&manifest.model.id),
        rust_string(&manifest.model.revision),
        rust_string(&manifest.model.license),
        manifest.model.dimensions,
        manifest.model.max_tokens,
        rust_string(&manifest.model.normalization),
        artifacts,
        rust_string(&manifest.sidecar.binary),
        rust_string(&manifest.sidecar.package),
    );
    fs::write(output.join("similar_code_protocol.rs"), generated)?;
    Ok(())
}
