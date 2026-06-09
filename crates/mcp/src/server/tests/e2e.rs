//! End-to-end tests that exercise the full param → arg-builder → real fallow binary → JSON parse chain.
//!
//! These tests require the `fallow` binary at `target/debug/fallow`. When running
//! `cargo test --workspace`, Cargo builds it automatically. If running `cargo test -p fallow-mcp`
//! alone, build the binary first: `cargo build -p fallow-cli`.

use std::path::PathBuf;

use rmcp::model::RawContent;

use crate::tools::{
    build_analyze_args, build_health_args, build_project_info_args, build_security_candidates_args,
    build_trace_clone_args, build_trace_dependency_args, build_trace_export_args,
    build_trace_file_args, execute_code_mode, run_fallow,
};

/// Resolve the fallow binary from `FALLOW_BIN`, or the workspace target dir.
fn fallow_binary() -> String {
    if let Ok(bin) = std::env::var("FALLOW_BIN") {
        return bin;
    }
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.pop(); // crates/
    path.pop(); // project root
    path.push("target/debug/fallow");
    if cfg!(windows) {
        path.set_extension("exe");
    }
    assert!(
        path.is_file(),
        "fallow binary not found at {path:?}. Build it first: cargo build -p fallow-cli"
    );
    path.to_string_lossy().to_string()
}

/// Resolve a fixture path relative to the workspace root.
fn fixture_path(name: &str) -> PathBuf {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.pop();
    path.pop();
    path.push("tests/fixtures");
    path.push(name);
    path
}

/// Extract the text content from a `CallToolResult`.
fn extract_text(result: &rmcp::model::CallToolResult) -> &str {
    match &result.content[0].raw {
        RawContent::Text(t) => &t.text,
        _ => panic!("expected text content"),
    }
}

#[tokio::test]
async fn e2e_analyze_returns_json_on_basic_project() {
    let bin = fallow_binary();
    let root = fixture_path("basic-project");
    let params = crate::params::AnalyzeParams {
        root: Some(root.to_string_lossy().to_string()),
        ..Default::default()
    };
    let args = build_analyze_args(&params).unwrap();
    let result = run_fallow(&bin, &args).await.unwrap();

    assert_eq!(result.is_error, Some(false));

    let text = extract_text(&result);
    let json: serde_json::Value = serde_json::from_str(text)
        .unwrap_or_else(|e| panic!("should parse as JSON: {e}\ntext: {text}"));
    assert!(
        json.get("schema_version").is_some(),
        "analyze output should have schema_version"
    );
    assert!(
        json.get("total_issues").is_some(),
        "analyze output should have total_issues"
    );
}

#[tokio::test]
async fn e2e_project_info_returns_files() {
    let bin = fallow_binary();
    let root = fixture_path("basic-project");
    let params = crate::params::ProjectInfoParams {
        root: Some(root.to_string_lossy().to_string()),
        ..Default::default()
    };
    let args = build_project_info_args(&params);
    let result = run_fallow(&bin, &args).await.unwrap();

    assert_eq!(result.is_error, Some(false));

    let text = extract_text(&result);
    let json: serde_json::Value = serde_json::from_str(text)
        .unwrap_or_else(|e| panic!("should parse as JSON: {e}\ntext: {text}"));
    let file_count = json["file_count"].as_u64().unwrap_or(0);
    assert!(
        file_count > 0,
        "project_info should report files, got file_count={file_count}"
    );
}

#[test]
fn e2e_code_execute_runs_project_info_on_basic_project() {
    let bin = fallow_binary();
    let root = fixture_path("basic-project");
    let output = execute_code_mode(
        bin,
        crate::params::CodeExecuteParams {
            code: "return { fileCount: fallow.projectInfo({ files: true }).file_count, root };"
                .to_string(),
            root: Some(root.to_string_lossy().to_string()),
            timeout_ms: Some(10_000),
            max_output_bytes: Some(1_000_000),
        },
    )
    .unwrap_or_else(|err| panic!("code mode should succeed: {err}"));

    let json: serde_json::Value = serde_json::from_str(&output)
        .unwrap_or_else(|e| panic!("should parse as JSON: {e}\ntext: {output}"));
    assert_eq!(json["ok"].as_bool(), Some(true));
    assert!(json["result"]["fileCount"].as_u64().unwrap_or(0) > 0);
    assert_eq!(json["calls"][0]["tool"].as_str(), Some("project_info"));
}

#[test]
fn e2e_code_execute_enforces_host_output_limit() {
    let bin = fallow_binary();
    let root = fixture_path("basic-project");
    let output = execute_code_mode(
        bin,
        crate::params::CodeExecuteParams {
            code: "return fallow.projectInfo({ files: true });".to_string(),
            root: Some(root.to_string_lossy().to_string()),
            timeout_ms: Some(10_000),
            max_output_bytes: Some(1),
        },
    )
    .expect_err("code mode should cap host output");

    let json: serde_json::Value = serde_json::from_str(&output)
        .unwrap_or_else(|e| panic!("should parse as JSON: {e}\ntext: {output}"));
    assert_eq!(json["ok"].as_bool(), Some(false));
    assert!(
        json["error"]
            .as_str()
            .is_some_and(|error| error.contains("host output exceeded 1 bytes")),
        "output cap rejection should be explicit: {output}"
    );
}

#[test]
fn e2e_code_execute_rejects_fix_apply() {
    let bin = fallow_binary();
    let root = fixture_path("basic-project");
    let output = execute_code_mode(
        bin,
        crate::params::CodeExecuteParams {
            code: "return fallow.run('fix_apply', {});".to_string(),
            root: Some(root.to_string_lossy().to_string()),
            timeout_ms: Some(1_000),
            max_output_bytes: Some(10_000),
        },
    )
    .expect_err("code mode should reject fix_apply");

    let json: serde_json::Value = serde_json::from_str(&output)
        .unwrap_or_else(|e| panic!("should parse as JSON: {e}\ntext: {output}"));
    assert_eq!(json["ok"].as_bool(), Some(false));
    assert!(
        json["error"]
            .as_str()
            .is_some_and(|error| error.contains("does not expose fix tools")),
        "fix_apply rejection should be explicit: {output}"
    );
    assert_eq!(json["calls"].as_array().map(Vec::len), Some(1));
    assert_eq!(json["calls"][0]["tool"].as_str(), Some("fix_apply"));
    assert_eq!(
        json["calls"][0]["error_kind"].as_str(),
        Some("unsupported_tool")
    );
}

#[tokio::test]
async fn e2e_analyze_with_issue_type_filter() {
    let bin = fallow_binary();
    let root = fixture_path("basic-project");
    let params = crate::params::AnalyzeParams {
        root: Some(root.to_string_lossy().to_string()),
        issue_types: Some(vec!["unused-files".to_string()]),
        ..Default::default()
    };
    let args = build_analyze_args(&params).unwrap();
    let result = run_fallow(&bin, &args).await.unwrap();

    assert_eq!(result.is_error, Some(false));

    let text = extract_text(&result);
    let json: serde_json::Value = serde_json::from_str(text)
        .unwrap_or_else(|e| panic!("should parse as JSON: {e}\ntext: {text}"));

    assert!(
        json.get("unused_files").is_some(),
        "filtered output should have unused_files"
    );
    let exports = json["unused_exports"].as_array();
    assert!(
        exports.is_none() || exports.unwrap().is_empty(),
        "filtered output should not have unused_exports"
    );
}

#[tokio::test]
async fn e2e_security_candidates_returns_security_json() {
    let bin = fallow_binary();
    let root = fixture_path("security-client-server-leak");
    let params = crate::params::SecurityCandidatesParams {
        root: Some(root.to_string_lossy().to_string()),
        ..Default::default()
    };
    let args = build_security_candidates_args(&params).unwrap();
    let result = run_fallow(&bin, &args).await.unwrap();

    assert_eq!(result.is_error, Some(false));

    let text = extract_text(&result);
    let json: serde_json::Value = serde_json::from_str(text)
        .unwrap_or_else(|e| panic!("should parse as JSON: {e}\ntext: {text}"));
    assert_eq!(json["kind"].as_str(), Some("security"));
    assert!(
        json["security_findings"].is_array(),
        "security output should include security_findings"
    );
}

#[tokio::test]
async fn e2e_security_candidates_paths_scope_real_cli_output() {
    let bin = fallow_binary();
    let root = fixture_path("security-client-server-leak");
    let params = crate::params::SecurityCandidatesParams {
        root: Some(root.to_string_lossy().to_string()),
        paths: Some(vec!["src/export-browser.ts".to_string()]),
        ..Default::default()
    };
    let args = build_security_candidates_args(&params).unwrap();
    let result = run_fallow(&bin, &args).await.unwrap();

    assert_eq!(result.is_error, Some(false));

    let text = extract_text(&result);
    let json: serde_json::Value = serde_json::from_str(text)
        .unwrap_or_else(|e| panic!("should parse as JSON: {e}\ntext: {text}"));
    assert_eq!(json["kind"].as_str(), Some("security"));
    assert_eq!(
        json["security_findings"].as_array().map(Vec::len),
        Some(0),
        "unrelated path scope should filter the fixture candidate"
    );
}

#[tokio::test]
async fn e2e_trace_export_returns_json() {
    let bin = fallow_binary();
    let root = fixture_path("basic-project");
    let args = build_trace_export_args(&crate::params::TraceExportParams {
        file: "src/utils.ts".to_string(),
        export_name: "usedFunction".to_string(),
        root: Some(root.to_string_lossy().to_string()),
        config: None,
        production: None,
        workspace: None,
        no_cache: None,
        threads: None,
    })
    .unwrap();
    let result = run_fallow(&bin, &args).await.unwrap();

    assert_eq!(result.is_error, Some(false));

    let text = extract_text(&result);
    let json: serde_json::Value = serde_json::from_str(text)
        .unwrap_or_else(|e| panic!("should parse as JSON: {e}\ntext: {text}"));
    assert_eq!(json["file"].as_str(), Some("src/utils.ts"));
    assert_eq!(json["export_name"].as_str(), Some("usedFunction"));
    assert_eq!(json["is_used"].as_bool(), Some(true));
}

#[tokio::test]
async fn e2e_trace_file_returns_json() {
    let bin = fallow_binary();
    let root = fixture_path("basic-project");
    let args = build_trace_file_args(&crate::params::TraceFileParams {
        file: "src/utils.ts".to_string(),
        root: Some(root.to_string_lossy().to_string()),
        config: None,
        production: None,
        workspace: None,
        no_cache: None,
        threads: None,
    })
    .unwrap();
    let result = run_fallow(&bin, &args).await.unwrap();

    assert_eq!(result.is_error, Some(false));

    let text = extract_text(&result);
    let json: serde_json::Value = serde_json::from_str(text)
        .unwrap_or_else(|e| panic!("should parse as JSON: {e}\ntext: {text}"));
    assert_eq!(json["file"].as_str(), Some("src/utils.ts"));
    assert_eq!(json["is_reachable"].as_bool(), Some(true));
    assert!(
        json["exports"].is_array(),
        "trace_file should include exports"
    );
}

#[tokio::test]
async fn e2e_trace_dependency_returns_json() {
    let bin = fallow_binary();
    let root = fixture_path("basic-project");
    let args = build_trace_dependency_args(&crate::params::TraceDependencyParams {
        package_name: "react".to_string(),
        root: Some(root.to_string_lossy().to_string()),
        config: None,
        production: None,
        workspace: None,
        no_cache: None,
        threads: None,
    })
    .unwrap();
    let result = run_fallow(&bin, &args).await.unwrap();

    assert_eq!(result.is_error, Some(false));

    let text = extract_text(&result);
    let json: serde_json::Value = serde_json::from_str(text)
        .unwrap_or_else(|e| panic!("should parse as JSON: {e}\ntext: {text}"));
    assert_eq!(json["package_name"].as_str(), Some("react"));
    assert!(json["imported_by"].is_array());
}

#[tokio::test]
async fn e2e_trace_clone_returns_json() {
    let bin = fallow_binary();
    let root = fixture_path("duplicate-code");
    let args = build_trace_clone_args(&crate::params::TraceCloneParams {
        file: Some("src/original.ts".to_string()),
        line: Some(2),
        fingerprint: None,
        root: Some(root.to_string_lossy().to_string()),
        config: None,
        workspace: None,
        mode: None,
        min_tokens: None,
        min_lines: None,
        threshold: None,
        skip_local: None,
        cross_language: None,
        ignore_imports: None,
        no_cache: None,
        threads: None,
        min_occurrences: None,
    })
    .unwrap();
    let result = run_fallow(&bin, &args).await.unwrap();

    assert_eq!(result.is_error, Some(false));

    let text = extract_text(&result);
    let json: serde_json::Value = serde_json::from_str(text)
        .unwrap_or_else(|e| panic!("should parse as JSON: {e}\ntext: {text}"));
    assert_eq!(json["file"].as_str(), Some("src/original.ts"));
    assert_eq!(json["line"].as_u64(), Some(2));
    assert!(json["matched_instance"].is_object());
    assert!(json["clone_groups"].is_array());

    let matched_file = json["matched_instance"]["file"]
        .as_str()
        .expect("matched_instance.file should be a string");
    assert!(
        !matched_file.starts_with('/')
            && !matched_file.contains(":\\")
            && !matched_file.contains(":/"),
        "matched_instance.file should be relative, got {matched_file}",
    );
    for group in json["clone_groups"].as_array().expect("clone_groups array") {
        for inst in group["instances"].as_array().expect("instances array") {
            let file = inst["file"].as_str().expect("instance.file string");
            assert!(
                !file.starts_with('/') && !file.contains(":\\") && !file.contains(":/"),
                "instance.file should be relative, got {file}",
            );
        }
    }
}

#[tokio::test]
async fn e2e_health_returns_json() {
    let bin = fallow_binary();
    let root = fixture_path("complexity-project");
    let params = crate::params::HealthParams {
        root: Some(root.to_string_lossy().to_string()),
        complexity: Some(true),
        ..Default::default()
    };
    let args = build_health_args(&params);
    let result = run_fallow(&bin, &args).await.unwrap();

    assert_eq!(result.is_error, Some(false));

    let text = extract_text(&result);
    let json: serde_json::Value = serde_json::from_str(text)
        .unwrap_or_else(|e| panic!("should parse as JSON: {e}\ntext: {text}"));
    assert!(json.is_object(), "health output should be a JSON object");
}
