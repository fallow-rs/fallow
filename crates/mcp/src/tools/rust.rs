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
//! root with both files keeps the JavaScript route, and the result gets a
//! `warnings` entry that points to `cargo fallow`.
//!
//! The mutation tools never compile and never run tests: `mutation_plan` runs
//! `mutants --list` and `mutation_inspect` runs `mutants --inspect <id>
//! --no-build`, which reads the coverage map from the cache only.
//!
//! The route fails closed: a parameter that `cargo-fallow` cannot honor gives
//! a typed error, not a silent drop.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use rmcp::ErrorData as McpError;
use rmcp::model::{CallToolResult, ContentBlock};
use serde::Serialize;

use crate::params::{
    AnalyzeParams, ArchitectureParams, FindDupesParams, HealthParams, MutationInspectParams,
    MutationPlanParams,
};
use fallow_api::MutationRunScope;

use super::mutation::plan_scope;
use super::{run_tool, typed_validation_error_body, validation_error_body};

/// The environment variable that names the `cargo-fallow` binary.
pub const CARGO_FALLOW_BIN_ENV: &str = "CARGO_FALLOW_BIN";

const BINARY_NAME: &str = "cargo-fallow";
const NOT_FOUND_CODE: &str = "FALLOW_RUST_BINARY_NOT_FOUND";
const UNSUPPORTED_PARAMS_CODE: &str = "FALLOW_RUST_UNSUPPORTED_PARAMS";
pub(super) const UNSUPPORTED_TOOL_CODE: &str = "FALLOW_RUST_UNSUPPORTED_TOOL";
const NO_CACHED_MAP_CODE: &str = "CARGO_FALLOW_NO_CACHED_MAP";
/// Added to the `help` of a missing coverage map: the command that builds the
/// map compiles the workspace and runs its tests.
const NO_CACHED_MAP_HELP: &str = "The command in the first sentence compiles the workspace and runs the tests, \
     so ask your host first. The MCP tools never compile and never run tests.";
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
    is_rust_dir(&dir).then(|| dir.join("Cargo.toml"))
}

/// Whether `dir` has a `Cargo.toml` and no `package.json`.
pub fn is_rust_dir(dir: &Path) -> bool {
    dir.join("Cargo.toml").is_file() && !dir.join("package.json").is_file()
}

/// The `warnings` entry for a root with both `Cargo.toml` and `package.json`.
/// The call keeps the JavaScript route, so the entry points to `cargo fallow`.
pub fn mixed_root_warning(root: Option<&str>) -> Option<String> {
    let dir = root.map_or_else(|| PathBuf::from("."), PathBuf::from);
    (dir.join("Cargo.toml").is_file() && dir.join("package.json").is_file()).then(|| {
        "The root also has a Cargo.toml. This result covers TypeScript and JavaScript only. \
         Run `cargo fallow` to analyze the Rust workspace."
            .to_string()
    })
}

/// Add the [`mixed_root_warning`] to the root `warnings` array of a
/// successful JSON result. An error result, a text result, a result with a
/// `warnings` value that is not an array, and a result on another root stay
/// as they are.
pub fn with_mixed_root_warning(
    root: Option<&str>,
    result: Result<CallToolResult, McpError>,
) -> Result<CallToolResult, McpError> {
    let Some(warning) = mixed_root_warning(root) else {
        return result;
    };
    result.map(|result| append_warning(result, warning))
}

fn append_warning(result: CallToolResult, warning: String) -> CallToolResult {
    if result.is_error == Some(true) || result.content.len() != 1 {
        return result;
    }
    let Some(text) = result.content[0].as_text() else {
        return result;
    };
    let Ok(serde_json::Value::Object(mut root)) = serde_json::from_str(&text.text) else {
        return result;
    };
    let warnings = root
        .entry("warnings")
        .or_insert_with(|| serde_json::Value::Array(Vec::new()));
    let Some(warnings) = warnings.as_array_mut() else {
        return result;
    };
    warnings.push(serde_json::Value::String(warning));
    let text = serde_json::Value::Object(root).to_string();
    CallToolResult::success(vec![ContentBlock::text(text)])
}

/// The error body of a tool that has no Rust route.
pub fn unsupported_tool_body(tool: &str, help: &str) -> String {
    typed_validation_error_body(
        format!("{tool} has no route for a Rust workspace yet"),
        UNSUPPORTED_TOOL_CODE,
        help,
        CONTEXT,
    )
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
const ANALYZE_PARAMS: &[&str] = &["root"];
const MUTATION_PLAN_PARAMS: &[&str] = &["root", "package", "changed_since", "all", "paths"];
const MUTATION_INSPECT_PARAMS: &[&str] = &["root", "package", "mutant_id"];

fn check_params(tool: &str, params: &impl Serialize, supported: &[&str]) -> Result<(), String> {
    let unsupported = unsupported_params(params, supported);
    if unsupported.is_empty() {
        Ok(())
    } else {
        Err(unsupported_error(tool, &unsupported, supported))
    }
}

fn check_package(package: Option<&str>) -> Result<(), String> {
    if package.is_some_and(|package| package.trim().is_empty()) {
        return Err(validation_error_body("package must not be empty"));
    }
    Ok(())
}

/// `analyze` runs `cargo fallow check` with the deps and visibility checks.
/// The partial run has no health score.
pub fn analyze_call(params: &AnalyzeParams, manifest: &Path) -> Result<RustCall, String> {
    check_params("analyze", params, ANALYZE_PARAMS)?;
    let mut args = base_args("check", manifest);
    args.extend(["--only", "deps", "--only", "visibility"].map(String::from));
    Ok(RustCall {
        tool: "analyze",
        args,
    })
}

/// `mutation_plan` runs `cargo fallow mutants --list`, which compiles nothing
/// and runs no test. Without `paths` or `changed_since`, the list covers the
/// whole package.
pub fn mutation_plan_call(
    params: &MutationPlanParams,
    manifest: &Path,
) -> Result<RustCall, String> {
    check_params("mutation_plan", params, MUTATION_PLAN_PARAMS)?;
    check_package(params.package.as_deref())?;
    let scope = plan_scope(params)?;
    let mut args = base_args("mutants", manifest);
    args.push("--list".to_string());
    push_value(
        &mut args,
        "--package",
        params.package.as_deref().map(str::trim),
    );
    match scope {
        MutationRunScope::Changed { base } => push_value(&mut args, "--in-diff", base),
        MutationRunScope::Paths(paths) => {
            for path in paths {
                push_value(&mut args, "--file", Some(path.to_string_lossy()));
            }
        }
        MutationRunScope::All => {}
    }
    Ok(RustCall {
        tool: "mutation_plan",
        args,
    })
}

/// `mutation_inspect` runs `cargo fallow mutants --inspect <id> --no-build`,
/// which reads the coverage map from the cache only.
pub fn mutation_inspect_call(
    params: &MutationInspectParams,
    manifest: &Path,
) -> Result<RustCall, String> {
    check_params("mutation_inspect", params, MUTATION_INSPECT_PARAMS)?;
    check_package(params.package.as_deref())?;
    let mutant_id = params.mutant_id.trim();
    if mutant_id.is_empty() {
        return Err(validation_error_body("mutant_id must not be empty"));
    }
    let mut args = base_args("mutants", manifest);
    push_value(
        &mut args,
        "--package",
        params.package.as_deref().map(str::trim),
    );
    args.extend(["--inspect", mutant_id, "--no-build"].map(String::from));
    Ok(RustCall {
        tool: "mutation_inspect",
        args,
    })
}

/// `find_dupes` runs `cargo fallow dupes`.
pub fn dupes_call(params: &FindDupesParams, manifest: &Path) -> Result<RustCall, String> {
    check_params("find_dupes", params, DUPES_PARAMS)?;
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
    check_params("check_health", params, HEALTH_PARAMS)?;
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
    check_params("check_architecture", params, ARCHITECTURE_PARAMS)?;
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
    run_tool(binary, call.tool, &call.args)
        .await
        .map(explain_missing_map)
}

/// A missing coverage map keeps the message and the help of `cargo-fallow`,
/// and the help adds that the command which builds the map runs the tests.
fn explain_missing_map(result: CallToolResult) -> CallToolResult {
    if result.is_error != Some(true) || result.content.len() != 1 {
        return result;
    }
    let Some(text) = result.content[0].as_text() else {
        return result;
    };
    let Ok(serde_json::Value::Object(mut body)) = serde_json::from_str(&text.text) else {
        return result;
    };
    if body.get("code").and_then(serde_json::Value::as_str) != Some(NO_CACHED_MAP_CODE) {
        return result;
    }
    let help = match body.get("help").and_then(serde_json::Value::as_str) {
        Some(help) => format!("{help} {NO_CACHED_MAP_HELP}"),
        None => NO_CACHED_MAP_HELP.to_string(),
    };
    body.insert("help".to_string(), serde_json::Value::String(help));
    body.insert(
        "context".to_string(),
        serde_json::Value::String(CONTEXT.to_string()),
    );
    let text = serde_json::Value::Object(body).to_string();
    CallToolResult::error(vec![ContentBlock::text(text)])
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
        assert_eq!(
            value.get("help"),
            None,
            "only a missing map gets a help: {value}"
        );
        assert_eq!(value.get("context"), None, "{value}");
    }

    fn error_value(body: &str) -> serde_json::Value {
        serde_json::from_str(body).expect("json body")
    }

    #[test]
    fn analyze_runs_the_deps_and_visibility_checks() {
        let params = AnalyzeParams {
            root: Some("/repo".into()),
            ..AnalyzeParams::default()
        };
        let call = analyze_call(&params, Path::new("/repo/Cargo.toml")).expect("supported");
        assert_eq!(call.tool, "analyze");
        assert_eq!(call.args[0], "check");
        assert_eq!(call.args[6..], ["--only", "deps", "--only", "visibility"]);
    }

    #[test]
    fn analyze_rejects_a_javascript_param() {
        let params = AnalyzeParams {
            production: Some(true),
            ..AnalyzeParams::default()
        };
        let body = analyze_call(&params, Path::new("Cargo.toml")).expect_err("unsupported");
        assert_eq!(
            error_value(&body)["message"],
            "analyze on a Rust workspace does not support: production"
        );
    }

    #[test]
    fn mutation_plan_lists_the_mutants_and_compiles_nothing() {
        let params = MutationPlanParams {
            package: Some("core".into()),
            paths: Some(vec!["src/lib.rs".into(), "src/a".into()]),
            ..MutationPlanParams::default()
        };
        let call = mutation_plan_call(&params, Path::new("/repo/Cargo.toml")).expect("supported");
        assert_eq!(call.tool, "mutation_plan");
        assert_eq!(call.args[0], "mutants");
        assert_eq!(
            call.args[6..],
            [
                "--list",
                "--package",
                "core",
                "--file",
                "src/lib.rs",
                "--file",
                "src/a"
            ]
        );
        let changed = MutationPlanParams {
            changed_since: Some("origin/main".into()),
            ..MutationPlanParams::default()
        };
        let call = mutation_plan_call(&changed, Path::new("Cargo.toml")).expect("supported");
        assert_eq!(call.args[6..], ["--list", "--in-diff", "origin/main"]);
        let all = MutationPlanParams {
            all: Some(true),
            ..MutationPlanParams::default()
        };
        let call = mutation_plan_call(&all, Path::new("Cargo.toml")).expect("supported");
        assert_eq!(call.args[6..], ["--list"]);
    }

    #[test]
    fn mutation_plan_rejects_a_limit_and_conflicting_scopes() {
        let limited = MutationPlanParams {
            max_mutants: Some(5),
            ..MutationPlanParams::default()
        };
        let body = mutation_plan_call(&limited, Path::new("Cargo.toml")).expect_err("limit");
        assert_eq!(error_value(&body)["code"], UNSUPPORTED_PARAMS_CODE);
        let both = MutationPlanParams {
            all: Some(true),
            changed_since: Some("main".into()),
            ..MutationPlanParams::default()
        };
        let body = mutation_plan_call(&both, Path::new("Cargo.toml")).expect_err("scopes");
        assert!(body.contains("mutually exclusive"));
    }

    fn inspect_params(mutant_id: &str) -> MutationInspectParams {
        MutationInspectParams {
            mutant_id: mutant_id.into(),
            report: None,
            root: None,
            config: None,
            allow_remote_extends: None,
            package: None,
        }
    }

    #[test]
    fn mutation_inspect_reads_the_cache_only() {
        let params = MutationInspectParams {
            package: Some("core".into()),
            ..inspect_params(" m-12 ")
        };
        let call = mutation_inspect_call(&params, Path::new("Cargo.toml")).expect("supported");
        assert_eq!(call.tool, "mutation_inspect");
        assert_eq!(call.args[0], "mutants");
        assert_eq!(
            call.args[6..],
            ["--package", "core", "--inspect", "m-12", "--no-build"]
        );
    }

    #[test]
    fn mutation_inspect_rejects_a_report_and_an_empty_id() {
        let params = MutationInspectParams {
            report: Some("report.json".into()),
            ..inspect_params("m-1")
        };
        let body = mutation_inspect_call(&params, Path::new("Cargo.toml")).expect_err("report");
        assert_eq!(
            error_value(&body)["message"],
            "mutation_inspect on a Rust workspace does not support: report"
        );
        let body =
            mutation_inspect_call(&inspect_params("  "), Path::new("Cargo.toml")).expect_err("id");
        assert!(body.contains("mutant_id must not be empty"));
    }

    #[test]
    fn a_rust_only_tool_error_names_the_tool_and_the_code() {
        let value = error_value(&unsupported_tool_body("project_info", "Call analyze."));
        assert_eq!(value["code"], UNSUPPORTED_TOOL_CODE);
        assert_eq!(value["context"], CONTEXT);
        assert_eq!(value["exit_code"], 2);
        assert_eq!(
            value["message"],
            "project_info has no route for a Rust workspace yet"
        );
        assert_eq!(value["help"], "Call analyze.");
    }

    #[test]
    fn a_mixed_root_gets_a_warning_and_other_roots_do_not() {
        let mixed = workspace(&["Cargo.toml", "package.json"]);
        let warning = mixed_root_warning(root_of(&mixed).as_deref()).expect("mixed root");
        assert!(warning.contains("cargo fallow"));
        let js = workspace(&["package.json"]);
        assert_eq!(mixed_root_warning(root_of(&js).as_deref()), None);
        let rust = workspace(&["Cargo.toml"]);
        assert_eq!(mixed_root_warning(root_of(&rust).as_deref()), None);
    }

    #[test]
    fn the_mixed_root_warning_lands_in_the_root_warnings_array() {
        let mixed = workspace(&["Cargo.toml", "package.json"]);
        let root = root_of(&mixed);
        let report = CallToolResult::success(vec![ContentBlock::text(
            r#"{"kind":"dupes","warnings":["first"]}"#,
        )]);
        let result = with_mixed_root_warning(root.as_deref(), Ok(report)).expect("result");
        let value: serde_json::Value =
            serde_json::from_str(&result_text(&result)).expect("json body");
        let warnings = value["warnings"].as_array().expect("warnings array");
        assert_eq!(warnings.len(), 2);
        assert_eq!(warnings[0], "first");
        assert!(
            warnings[1]
                .as_str()
                .is_some_and(|w| w.contains("cargo fallow"))
        );

        let bare = CallToolResult::success(vec![ContentBlock::text(r#"{"kind":"health"}"#)]);
        let result = with_mixed_root_warning(root.as_deref(), Ok(bare)).expect("result");
        let value: serde_json::Value =
            serde_json::from_str(&result_text(&result)).expect("json body");
        assert_eq!(value["warnings"].as_array().map(Vec::len), Some(1));

        let failed = CallToolResult::error(vec![ContentBlock::text(r#"{"error":true}"#)]);
        let result = with_mixed_root_warning(root.as_deref(), Ok(failed)).expect("result");
        assert_eq!(result_text(&result), r#"{"error":true}"#);
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_missing_coverage_map_gives_an_error_that_keeps_the_help() {
        let dir = workspace(&[]);
        let report = r#"{"error":true,"message":"the cache has no coverage map for this source of package `core`","exit_code":2,"code":"CARGO_FALLOW_NO_CACHED_MAP","help":"Run `cargo fallow mutants -p core --inspect m-1` once to build the map. Then run this command again."}"#;
        let stub = stub_binary(dir.path(), report, 2);
        let call = mutation_inspect_call(&inspect_params("m-1"), Path::new("Cargo.toml"));
        let result = run_rust_call(&stub, call).await.expect("tool result");
        assert_eq!(result.is_error, Some(true));
        let value: serde_json::Value =
            serde_json::from_str(&result_text(&result)).expect("json body");
        assert_eq!(value["code"], NO_CACHED_MAP_CODE);
        assert_eq!(value["context"], CONTEXT);
        let help = value["help"].as_str().expect("help");
        assert!(help.starts_with("Run `cargo fallow mutants -p core --inspect m-1` once"));
        assert!(
            help.contains(
                "The command in the first sentence compiles the workspace and runs the tests"
            ),
            "{help}"
        );
    }

    #[test]
    fn an_empty_package_or_path_list_is_an_error() {
        let plan = MutationPlanParams {
            package: Some(" ".into()),
            ..MutationPlanParams::default()
        };
        let body = mutation_plan_call(&plan, Path::new("Cargo.toml")).expect_err("package");
        assert_eq!(error_value(&body)["message"], "package must not be empty");
        let inspect = MutationInspectParams {
            package: Some(String::new()),
            ..inspect_params("m-1")
        };
        let body = mutation_inspect_call(&inspect, Path::new("Cargo.toml")).expect_err("package");
        assert_eq!(error_value(&body)["message"], "package must not be empty");
        let paths = MutationPlanParams {
            paths: Some(Vec::new()),
            ..MutationPlanParams::default()
        };
        let body = mutation_plan_call(&paths, Path::new("Cargo.toml")).expect_err("paths");
        assert!(body.contains("paths must contain at least one entry"));
    }
}
