//! Code Mode has no Rust route yet. A snippet on a Rust root, or a host call
//! on one, fails closed with `FALLOW_RUST_UNSUPPORTED_TOOL`.

use std::path::Path;

use serde_json::json;

use super::error_envelope;

/// Code Mode has no Rust route yet, so it fails closed on a Rust root instead
/// of running the JavaScript analysis on a Cargo workspace.
const RUST_ROOT_ERROR: &str = "Code Mode has no Rust route yet";
const RUST_ROOT_HELP: &str = "Call the standalone MCP tools. On a Rust workspace, analyze, \
     find_dupes, check_health, check_architecture, mutation_plan and mutation_inspect run cargo fallow.";

/// The refusal of a snippet on a Rust root: the Code Mode error envelope,
/// plus the typed `code`, `help`, `context` and `exit_code` fields.
pub(super) fn rust_root_envelope(limits: &serde_json::Value, max_error_bytes: usize) -> String {
    let envelope = error_envelope(RUST_ROOT_ERROR, &[], 0, limits, max_error_bytes);
    let Ok(serde_json::Value::Object(mut fields)) = serde_json::from_str(&envelope) else {
        return envelope;
    };
    fields.insert("exit_code".to_string(), json!(2));
    fields.insert(
        "code".to_string(),
        json!(super::super::rust::UNSUPPORTED_TOOL_CODE),
    );
    fields.insert("help".to_string(), json!(RUST_ROOT_HELP));
    fields.insert("context".to_string(), json!("rust"));
    serde_json::Value::Object(fields).to_string()
}

pub(super) fn is_rust_root(root: Option<&str>, cwd: &Path) -> bool {
    super::super::rust::is_rust_dir(root.map_or(cwd, Path::new))
}

pub(super) fn reject_rust_root(params: &serde_json::Value, cwd: &Path) -> Result<(), String> {
    let root = params.get("root").and_then(serde_json::Value::as_str);
    if !is_rust_root(root, cwd) {
        return Ok(());
    }
    Err(format!(
        "{RUST_ROOT_ERROR} ({}): the root of the host call is a Rust workspace. {RUST_ROOT_HELP}",
        super::super::rust::UNSUPPORTED_TOOL_CODE
    ))
}

#[cfg(test)]
/// The test process runs in `crates/mcp`, a Cargo package with no
/// `package.json`, so a snippet without a root would take the Rust refusal.
/// The helper gives the snippet a server folder that is not a Rust root.
pub(super) fn execute_code_mode_off_rust(
    binary: String,
    params: crate::params::CodeExecuteParams,
    request: Option<tokio_util::sync::CancellationToken>,
) -> Result<String, String> {
    let cwd = tempfile::tempdir().map_err(|err| format!("tempdir: {err}"))?;
    super::execute_code_mode_in(binary, params, request, cwd.path())
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::super::execute_code_mode_in;
    use super::execute_code_mode_off_rust;
    use crate::params::CodeExecuteParams;

    fn rust_workspace() -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("tempdir");
        fs::write(dir.path().join("Cargo.toml"), "[workspace]\n").expect("manifest");
        dir
    }

    #[test]
    fn code_mode_fails_closed_on_a_rust_root() {
        let rust = rust_workspace();
        let output = execute_code_mode_off_rust(
            "/definitely/not/fallow".to_string(),
            CodeExecuteParams {
                code: "return fallow.projectInfo({});".to_string(),
                root: Some(rust.path().display().to_string()),
                timeout_ms: Some(5_000),
                max_output_bytes: Some(10_000),
            },
            None,
        )
        .expect_err("Code Mode must refuse a Rust root");
        let json: serde_json::Value = serde_json::from_str(&output).expect("code mode JSON");
        assert_eq!(json["ok"], false);
        assert_eq!(json["code"], "FALLOW_RUST_UNSUPPORTED_TOOL");
        assert_eq!(json["context"], "rust");
        assert_eq!(json["exit_code"], 2);
        assert!(
            json["error"]
                .as_str()
                .is_some_and(|error| error.contains("Code Mode has no Rust route")),
            "{json}"
        );
        assert_eq!(json["calls"], serde_json::json!([]), "{json}");
        assert_eq!(json["limits"]["timeout_ms"], 5_000, "{json}");
    }

    #[test]
    fn code_mode_fails_closed_when_the_server_folder_is_a_rust_root() {
        let rust = rust_workspace();
        let output = execute_code_mode_in(
            "/definitely/not/fallow".to_string(),
            CodeExecuteParams {
                code: "return 1;".to_string(),
                root: None,
                timeout_ms: Some(5_000),
                max_output_bytes: Some(10_000),
            },
            None,
            rust.path(),
        )
        .expect_err("Code Mode must refuse a Rust server folder");
        let json: serde_json::Value = serde_json::from_str(&output).expect("code mode JSON");
        assert_eq!(json["code"], "FALLOW_RUST_UNSUPPORTED_TOOL");
    }

    #[test]
    fn a_host_call_on_a_rust_root_is_refused_before_dispatch() {
        let rust = rust_workspace();
        let code = format!(
            "return fallow.run('project_info', {{ root: {root:?} }});",
            root = rust.path().to_str().expect("utf8"),
        );
        let output = execute_code_mode_off_rust(
            "/definitely/not/fallow".to_string(),
            CodeExecuteParams {
                code,
                root: None,
                timeout_ms: Some(5_000),
                max_output_bytes: Some(10_000),
            },
            None,
        )
        .expect_err("Code Mode must refuse a Rust root in a host call");
        let json: serde_json::Value = serde_json::from_str(&output).expect("code mode JSON");
        assert!(
            json["error"]
                .as_str()
                .is_some_and(|error| error.contains("FALLOW_RUST_UNSUPPORTED_TOOL")),
            "{json}"
        );
        assert_eq!(json["calls"][0]["error_kind"], "unsupported_tool");
    }
}
