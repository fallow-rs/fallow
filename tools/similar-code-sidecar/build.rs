#![allow(
    clippy::expect_used,
    reason = "build-time protocol generation must fail immediately on an invalid source manifest"
)]

use std::env;
use std::fmt::Write as _;
use std::fs;
use std::path::PathBuf;

fn main() {
    let manifest_dir = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("manifest dir"));
    let protocol_path = manifest_dir.join("../../crates/api/similar-code-protocol.json");
    println!("cargo:rerun-if-changed={}", protocol_path.display());

    let raw = fs::read_to_string(&protocol_path).expect("read similar-code protocol manifest");
    let value: serde_json::Value =
        serde_json::from_str(&raw).expect("parse similar-code protocol manifest");
    let wire = value["wire_protocol_version"]
        .as_u64()
        .expect("wire protocol version");
    let embedding_semantics = value["embedding_semantics_version"]
        .as_u64()
        .expect("embedding semantics version");
    let model = &value["model"];
    let model_id = model["id"].as_str().expect("model id");
    let revision = model["revision"].as_str().expect("model revision");
    let license = model["license"].as_str().expect("model license");
    let dimensions = model["dimensions"].as_u64().expect("model dimensions");
    let max_tokens = model["max_tokens"].as_u64().expect("model max tokens");
    let operation = value["analysis_operation"]
        .as_str()
        .expect("analysis operation");

    let artifacts = model["artifacts"].as_array().expect("model artifacts");
    let mut artifact_rows = String::new();
    for artifact in artifacts {
        let path = artifact["path"].as_str().expect("artifact path");
        let size = readable_u64(artifact["size"].as_u64().expect("artifact size"));
        let sha256 = artifact["sha256"].as_str().expect("artifact sha256");
        writeln!(
            artifact_rows,
            "    ArtifactSpec {{ path: {path:?}, size: {size}, sha256: {sha256:?} }},"
        )
        .expect("write generated artifact row");
    }

    let generated = format!(
        "pub const PROTOCOL_VERSION: u32 = {wire};\n\
         pub const EMBEDDING_SEMANTICS_VERSION: u32 = {embedding_semantics};\n\
         pub const ANALYSIS_OPERATION: &str = {operation:?};\n\
         pub const MODEL_ID: &str = {model_id:?};\n\
         pub const MODEL_REVISION: &str = {revision:?};\n\
         pub const MODEL_LICENSE: &str = {license:?};\n\
         pub const MODEL_DIMENSIONS: usize = {dimensions};\n\
         pub const MODEL_MAX_TOKENS: usize = {max_tokens};\n\
         pub const ARTIFACTS: &[ArtifactSpec] = &[\n{artifact_rows}];\n"
    );
    let out = PathBuf::from(env::var_os("OUT_DIR").expect("out dir")).join("protocol.rs");
    fs::write(out, generated).expect("write generated protocol constants");
}

fn readable_u64(value: u64) -> String {
    let digits = value.to_string();
    let mut rendered = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, character) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            rendered.push('_');
        }
        rendered.push(character);
    }
    rendered
}
