//! The Rust route: a tool call on a Cargo workspace runs `cargo-fallow`.
//!
//! The route is a thin layer. It finds the workspace, builds the arguments,
//! and runs the binary through [`super::run_tool`], so the timeout, the
//! cancellation, the output cap and the exit-code policy stay the same as for
//! the JavaScript route. `cargo-fallow` writes the shared report contract
//! (`schema_version`, `kind`, `language`, `gate_outcomes`, `actions` and the
//! JSON error document), so the route passes its JSON through unchanged.
//!
//! A root with a `Cargo.toml` and no `package.json` takes the Rust route. A
//! root with both files keeps the JavaScript route, so no existing call
//! changes its result.
//!
//! The route fails closed: a parameter that `cargo-fallow` cannot honor gives
//! a typed error, not a silent drop.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use rmcp::ErrorData as McpError;
use rmcp::model::{CallToolResult, ContentBlock};
use serde::Serialize;

use crate::params::HealthParams;
use crate::params::{ArchitectureParams, FindDupesParams};

use super::{run_tool, typed_validation_error_body};

/// The environment variable that names the `cargo-fallow` binary.
pub const CARGO_FALLOW_BIN_ENV: &str = "CARGO_FALLOW_BIN";

const BINARY_NAME: &str = "cargo-fallow";
const NOT_FOUND_CODE: &str = "FALLOW_RUST_BINARY_NOT_FOUND";
const UNSUPPORTED_PARAMS_CODE: &str = "FALLOW_RUST_UNSUPPORTED_PARAMS";
const CONTEXT: &str = "rust";

/// One tool call that the Rust route runs.
#[derive(Debug, PartialEq, Eq)]
pub struct RustCall {
    /// The MCP tool name, for the telemetry tag of the subprocess.
    pub tool: &'static str,
    /// The arguments of `cargo-fallow`.
    pub args: Vec<String>,
}

/// The `Cargo.toml` of the root when the call takes the Rust route.
pub fn rust_manifest(root: Option<&str>) -> Option<PathBuf> {
    let dir = root.map_or_else(|| PathBuf::from("."), PathBuf::from);
    let manifest = dir.join("Cargo.toml");
    (manifest.is_file() && !dir.join("package.json").is_file()).then_some(manifest)
}

/// Resolve the `cargo-fallow` binary: `CARGO_FALLOW_BIN`, else a sibling of
/// the running executable, else the name for a PATH lookup.
pub fn resolve_cargo_fallow() -> String {
    resolve_cargo_fallow_from(
        std::env::var(CARGO_FALLOW_BIN_ENV).ok(),
        std::env::current_exe().ok(),
    )
}

fn resolve_cargo_fallow_from(override_bin: Option<String>, exe: Option<PathBuf>) -> String {
    if let Some(bin) = override_bin.filter(|bin| !bin.trim().is_empty()) {
        return bin;
    }
    if let Some(exe) = exe {
        let sibling = exe.with_file_name(executable_name(BINARY_NAME));
        if sibling.is_file()
            && let Some(path) = sibling.to_str()
        {
            return path.to_string();
        }
    }
    BINARY_NAME.to_string()
}

fn executable_name(name: &str) -> String {
    if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_string()
    }
}

/// The file that a spawn of `binary` runs, or `None` when no such file exists.
fn locate(binary: &str, path_var: Option<OsString>) -> Option<PathBuf> {
    let candidate = Path::new(binary);
    if candidate.components().count() > 1 {
        return candidate.is_file().then(|| candidate.to_path_buf());
    }
    let name = executable_name(binary);
    std::env::split_paths(&path_var?)
        .map(|dir| dir.join(&name))
        .find(|path| path.is_file())
}

/// The parameters of a call that are set and that the Rust route does not
/// support. `null` and `false` count as not set.
fn unsupported_params(params: &impl Serialize, supported: &[&str]) -> Vec<String> {
    let Ok(serde_json::Value::Object(fields)) = serde_json::to_value(params) else {
        return Vec::new();
    };
    let mut unsupported: Vec<String> = fields
        .into_iter()
        .filter(|(_, value)| {
            !matches!(
                value,
                serde_json::Value::Null | serde_json::Value::Bool(false)
            )
        })
        .map(|(name, _)| name)
        .filter(|name| !supported.contains(&name.as_str()))
        .collect();
    unsupported.sort();
    unsupported
}

fn unsupported_error(tool: &str, unsupported: &[String], supported: &[&str]) -> String {
    typed_validation_error_body(
        format!(
            "{tool} on a Rust workspace does not support: {}",
            unsupported.join(", ")
        ),
        UNSUPPORTED_PARAMS_CODE,
        &format!(
            "Remove these parameters. On a Rust workspace, {tool} supports: {}.",
            supported.join(", ")
        ),
        CONTEXT,
    )
}

fn base_args(command: &str, manifest: &Path) -> Vec<String> {
    vec![
        command.to_string(),
        "--manifest-path".to_string(),
        manifest.to_string_lossy().into_owned(),
        "--format".to_string(),
        "json".to_string(),
        "--quiet".to_string(),
    ]
}

fn push_value(args: &mut Vec<String>, flag: &str, value: Option<impl ToString>) {
    if let Some(value) = value {
        args.push(flag.to_string());
        args.push(value.to_string());
    }
}

const DUPES_PARAMS: &[&str] = &["root", "mode", "min_tokens", "min_lines", "threshold"];
const HEALTH_PARAMS: &[&str] = &["root", "max_cyclomatic", "max_cognitive"];
const ARCHITECTURE_PARAMS: &[&str] = &["root", "boundaries"];

/// `find_dupes` runs `cargo fallow dupes`.
pub fn dupes_call(params: &FindDupesParams, manifest: &Path) -> Result<RustCall, String> {
    let unsupported = unsupported_params(params, DUPES_PARAMS);
    if !unsupported.is_empty() {
        return Err(unsupported_error("find_dupes", &unsupported, DUPES_PARAMS));
    }
    let mut args = base_args("dupes", manifest);
    push_value(&mut args, "--mode", params.mode.as_deref());
    push_value(&mut args, "--min-tokens", params.min_tokens);
    push_value(&mut args, "--min-lines", params.min_lines);
    push_value(&mut args, "--threshold", params.threshold);
    Ok(RustCall {
        tool: "find_dupes",
        args,
    })
}

/// `check_health` runs `cargo fallow complexity`. The Rust report has the
/// complexity findings only: no health score, no file scores and no hotspots.
pub fn health_call(params: &HealthParams, manifest: &Path) -> Result<RustCall, String> {
    let unsupported = unsupported_params(params, HEALTH_PARAMS);
    if !unsupported.is_empty() {
        return Err(unsupported_error(
            "check_health",
            &unsupported,
            HEALTH_PARAMS,
        ));
    }
    let mut args = base_args("complexity", manifest);
    push_value(&mut args, "--max-cyclomatic", params.max_cyclomatic);
    push_value(&mut args, "--max-cognitive", params.max_cognitive);
    Ok(RustCall {
        tool: "check_health",
        args,
    })
}

/// `check_architecture` runs `cargo fallow boundaries`. A Rust workspace has
/// no import cycle check and no policy rules yet.
pub fn architecture_call(params: &ArchitectureParams, manifest: &Path) -> Result<RustCall, String> {
    let unsupported = unsupported_params(params, ARCHITECTURE_PARAMS);
    if !unsupported.is_empty() {
        return Err(unsupported_error(
            "check_architecture",
            &unsupported,
            ARCHITECTURE_PARAMS,
        ));
    }
    Ok(RustCall {
        tool: "check_architecture",
        args: base_args("boundaries", manifest),
    })
}

/// Run one Rust call. A missing binary gives a typed error body, because the
/// generic spawn error names the `fallow` binary and `FALLOW_BIN`.
pub async fn run_rust_call(
    binary: &str,
    call: Result<RustCall, String>,
) -> Result<CallToolResult, McpError> {
    let call = match call {
        Ok(call) => call,
        Err(body) => return Ok(CallToolResult::error(vec![ContentBlock::text(body)])),
    };
    if locate(binary, std::env::var_os("PATH")).is_none() {
        return Ok(CallToolResult::error(vec![ContentBlock::text(
            not_found_body(binary),
        )]));
    }
    run_tool(binary, call.tool, &call.args).await
}

fn not_found_body(binary: &str) -> String {
    typed_validation_error_body(
        format!("the root is a Rust workspace, but the binary `{binary}` was not found"),
        NOT_FOUND_CODE,
        &format!(
            "Install cargo-fallow and put it on the PATH, or set {CARGO_FALLOW_BIN_ENV} to its path."
        ),
        CONTEXT,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn workspace(files: &[&str]) -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("temp dir");
        for file in files {
            std::fs::write(dir.path().join(file), "").expect("write file");
        }
        dir
    }

    fn root_of(dir: &tempfile::TempDir) -> Option<String> {
        dir.path().to_str().map(str::to_string)
    }

    #[test]
    fn a_cargo_root_without_package_json_takes_the_rust_route() {
        let dir = workspace(&["Cargo.toml"]);
        assert_eq!(
            rust_manifest(root_of(&dir).as_deref()),
            Some(dir.path().join("Cargo.toml"))
        );
    }

    #[test]
    fn a_root_with_package_json_keeps_the_javascript_route() {
        let mixed = workspace(&["Cargo.toml", "package.json"]);
        assert_eq!(rust_manifest(root_of(&mixed).as_deref()), None);
        let js = workspace(&["package.json"]);
        assert_eq!(rust_manifest(root_of(&js).as_deref()), None);
        let empty = workspace(&[]);
        assert_eq!(rust_manifest(root_of(&empty).as_deref()), None);
    }

    #[test]
    fn the_binary_override_wins_and_an_empty_override_is_ignored() {
        assert_eq!(
            resolve_cargo_fallow_from(Some("/opt/cargo-fallow".into()), None),
            "/opt/cargo-fallow"
        );
        assert_eq!(
            resolve_cargo_fallow_from(Some("  ".into()), None),
            BINARY_NAME
        );
        assert_eq!(resolve_cargo_fallow_from(None, None), BINARY_NAME);
    }

    #[test]
    fn a_sibling_binary_wins_over_the_path_lookup() {
        let dir = workspace(&[]);
        let sibling = dir.path().join(executable_name(BINARY_NAME));
        std::fs::write(&sibling, "").expect("write sibling");
        let exe = dir.path().join("fallow-mcp");
        assert_eq!(
            resolve_cargo_fallow_from(None, Some(exe)),
            sibling.to_string_lossy()
        );
    }

    #[test]
    fn locate_reads_the_path_and_explicit_paths() {
        let dir = workspace(&[]);
        let binary = dir.path().join(executable_name(BINARY_NAME));
        std::fs::write(&binary, "").expect("write binary");
        let path_var = std::env::join_paths([dir.path()]).expect("join paths");
        assert_eq!(
            locate(BINARY_NAME, Some(path_var.clone())),
            Some(binary.clone())
        );
        assert_eq!(locate("not-a-binary", Some(path_var)), None);
        assert_eq!(locate(BINARY_NAME, None), None);
        let explicit = binary.to_string_lossy().into_owned();
        assert_eq!(locate(&explicit, None), Some(binary));
        let missing = dir.path().join("missing").to_string_lossy().into_owned();
        assert_eq!(locate(&missing, None), None);
    }

    #[test]
    fn dupes_maps_the_supported_params_to_flags() {
        let params = FindDupesParams {
            root: Some("/repo".into()),
            mode: Some("semantic".into()),
            min_tokens: Some(40),
            min_lines: Some(4),
            threshold: Some(2.5),
            ..FindDupesParams::default()
        };
        let call = dupes_call(&params, Path::new("/repo/Cargo.toml")).expect("supported");
        assert_eq!(call.tool, "find_dupes");
        assert_eq!(
            call.args,
            [
                "dupes",
                "--manifest-path",
                "/repo/Cargo.toml",
                "--format",
                "json",
                "--quiet",
                "--mode",
                "semantic",
                "--min-tokens",
                "40",
                "--min-lines",
                "4",
                "--threshold",
                "2.5"
            ]
        );
    }

    #[test]
    fn an_unsupported_param_gives_a_typed_error_that_names_it() {
        let params = FindDupesParams {
            workspace: Some("app".into()),
            skip_local: Some(true),
            ..FindDupesParams::default()
        };
        let body = dupes_call(&params, Path::new("Cargo.toml")).expect_err("unsupported");
        let value: serde_json::Value = serde_json::from_str(&body).expect("json body");
        assert_eq!(value["error"], true);
        assert_eq!(value["exit_code"], 2);
        assert_eq!(value["code"], UNSUPPORTED_PARAMS_CODE);
        assert_eq!(value["context"], CONTEXT);
        assert_eq!(
            value["message"],
            "find_dupes on a Rust workspace does not support: skip_local, workspace"
        );
    }

    #[test]
    fn a_false_flag_counts_as_not_set() {
        let params = FindDupesParams {
            skip_local: Some(false),
            ..FindDupesParams::default()
        };
        assert!(dupes_call(&params, Path::new("Cargo.toml")).is_ok());
    }

    #[test]
    fn health_maps_the_thresholds_and_rejects_the_rest() {
        let params = HealthParams {
            max_cyclomatic: Some(25),
            max_cognitive: Some(18),
            ..HealthParams::default()
        };
        let call = health_call(&params, Path::new("Cargo.toml")).expect("supported");
        assert_eq!(call.tool, "check_health");
        assert_eq!(
            call.args[6..],
            ["--max-cyclomatic", "25", "--max-cognitive", "18"]
        );
        assert_eq!(call.args[0], "complexity");
        let scored = HealthParams {
            score: Some(true),
            ..HealthParams::default()
        };
        let body = health_call(&scored, Path::new("Cargo.toml")).expect_err("unsupported");
        assert!(body.contains("check_health on a Rust workspace does not support: score"));
    }

    #[test]
    fn architecture_runs_boundaries_and_rejects_cycles() {
        let params = ArchitectureParams {
            boundaries: Some(true),
            ..ArchitectureParams::default()
        };
        let call = architecture_call(&params, Path::new("Cargo.toml")).expect("supported");
        assert_eq!(call.args[0], "boundaries");
        let cycles = ArchitectureParams {
            cycles: Some(true),
            ..ArchitectureParams::default()
        };
        let body = architecture_call(&cycles, Path::new("Cargo.toml")).expect_err("unsupported");
        assert!(body.contains("does not support: cycles"));
    }

    #[tokio::test]
    async fn a_missing_binary_gives_a_typed_error_and_spawns_nothing() {
        let dir = workspace(&[]);
        let missing = dir
            .path()
            .join("cargo-fallow")
            .to_string_lossy()
            .into_owned();
        let call = dupes_call(&FindDupesParams::default(), Path::new("Cargo.toml"));
        let result = run_rust_call(&missing, call).await.expect("tool result");
        assert_eq!(result.is_error, Some(true));
        let text = result_text(&result);
        let value: serde_json::Value = serde_json::from_str(&text).expect("json body");
        assert_eq!(value["code"], NOT_FOUND_CODE);
        assert!(
            value["help"]
                .as_str()
                .is_some_and(|help| help.contains(CARGO_FALLOW_BIN_ENV))
        );
    }

    fn result_text(result: &CallToolResult) -> String {
        result
            .content
            .iter()
            .filter_map(|block| block.as_text().map(|text| text.text.clone()))
            .collect()
    }

    /// A stub binary that prints a fixed report and exits with a fixed code.
    #[cfg(unix)]
    fn stub_binary(dir: &Path, stdout: &str, exit_code: i32) -> String {
        use std::os::unix::fs::PermissionsExt;
        let report = dir.join("report.json");
        std::fs::write(&report, stdout).expect("write report");
        let stub = dir.join("cargo-fallow");
        std::fs::write(
            &stub,
            format!(
                "#!/bin/sh\ncat '{}'\nexit {exit_code}\n",
                report.to_string_lossy()
            ),
        )
        .expect("write stub");
        std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o755))
            .expect("chmod stub");
        stub.to_string_lossy().into_owned()
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_report_with_findings_passes_through_as_a_success() {
        let dir = workspace(&[]);
        let report = r#"{"schema_version":1,"kind":"dupes","language":"rust","total_issues":1,"gate_outcomes":{},"findings":[]}"#;
        let stub = stub_binary(dir.path(), report, 1);
        let call = dupes_call(&FindDupesParams::default(), Path::new("Cargo.toml"));
        let result = run_rust_call(&stub, call).await.expect("tool result");
        assert_ne!(result.is_error, Some(true));
        let value: serde_json::Value =
            serde_json::from_str(&result_text(&result)).expect("json body");
        assert_eq!(value["language"], "rust");
        assert_eq!(value["kind"], "dupes");
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn the_json_error_document_passes_through_as_an_error() {
        let dir = workspace(&[]);
        let report = r#"{"error":true,"message":"the manifest does not exist","exit_code":2,"code":"CARGO_FALLOW_MANIFEST_NOT_FOUND"}"#;
        let stub = stub_binary(dir.path(), report, 2);
        let call = health_call(&HealthParams::default(), Path::new("Cargo.toml"));
        let result = run_rust_call(&stub, call).await.expect("tool result");
        assert_eq!(result.is_error, Some(true));
        let value: serde_json::Value =
            serde_json::from_str(&result_text(&result)).expect("json body");
        assert_eq!(value["code"], "CARGO_FALLOW_MANIFEST_NOT_FOUND");
    }
}
