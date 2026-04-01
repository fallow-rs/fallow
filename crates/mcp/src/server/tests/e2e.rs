//! End-to-end tests that exercise the full param → arg-builder → real fallow binary → JSON parse chain.
//!
//! These tests require the `fallow` binary at `target/debug/fallow`. When running
//! `cargo test --workspace`, Cargo builds it automatically. If running `cargo test -p fallow-mcp`
//! alone, build the binary first: `cargo build -p fallow-cli`.

use std::path::PathBuf;

use rmcp::model::RawContent;

use crate::tools::{build_analyze_args, build_health_args, build_project_info_args, run_fallow};

/// Resolve the fallow binary from the workspace target dir.
fn fallow_binary() -> String {
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

// ── End-to-end: analyze ──────────────────────────────────────────

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

// ── End-to-end: project_info ─────────────────────────────────────

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

// ── End-to-end: analyze with issue type filter ───────────────────

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

// ── End-to-end: health ───────────────────────────────────────────

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
