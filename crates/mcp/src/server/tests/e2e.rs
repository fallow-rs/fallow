//! End-to-end tests that exercise the full param → arg-builder → real fallow binary → JSON parse chain.
//!
//! These tests require the `fallow` binary at `target/debug/fallow`. When running
//! `cargo test --workspace`, Cargo builds it automatically. If running `cargo test -p fallow-mcp`
//! alone, build the binary first: `cargo build -p fallow-cli`.

use std::path::PathBuf;

use rmcp::model::ContentBlock;

use crate::tools::{
    build_analyze_args, build_health_args, build_impact_closure_args, build_project_info_args,
    build_security_candidates_args, build_trace_clone_args, build_trace_dependency_args,
    build_trace_export_args, build_trace_file_args, execute_code_mode, inspect_target, run_analyze,
    run_fallow, run_find_dupes, run_fix_apply, run_fix_preview, run_trace_clone_tool,
    run_trace_error_tool, run_trace_export_tool,
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
    match &result.content[0] {
        ContentBlock::Text(t) => &t.text,
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

/// A real fan-out over the real binary: one in-process element and two
/// subprocess-backed ones, positionally aligned, with the repeat served from
/// the snippet's memo instead of a third analysis.
///
/// The subprocess elements are deliberately git-independent. `audit` resolves
/// a base branch, which a detached shallow CI checkout cannot detect, so it
/// exits 2 there and would fail this element on CI while passing locally.
#[test]
fn e2e_code_execute_batches_real_analyses_and_reuses_the_memo() {
    let bin = fallow_binary();
    let root = fixture_path("basic-project");
    let output = execute_code_mode(
        bin,
        crate::params::CodeExecuteParams {
            code: r#"
            const batch = fallow.all([
                { tool: "project_info", params: { files: true } },
                { tool: "analyze", params: { issue_types: ["unused-exports"] } },
                { tool: "find_dupes", params: {} },
                { tool: "project_info", params: { files: true } }
            ]);
            return {
                ok: batch.map((element) => element.ok),
                fileCount: batch[0].value.file_count,
                deadCodeKind: batch[1].value.kind,
                dupesKind: batch[2].value.kind,
                repeated: batch[3].value.file_count
            };
            "#
            .to_string(),
            root: Some(root.to_string_lossy().to_string()),
            timeout_ms: Some(30_000),
            max_output_bytes: Some(4_000_000),
        },
    )
    .unwrap_or_else(|err| panic!("batched code mode should succeed: {err}"));

    let json: serde_json::Value = serde_json::from_str(&output)
        .unwrap_or_else(|e| panic!("should parse as JSON: {e}\ntext: {output}"));
    assert_eq!(json["ok"].as_bool(), Some(true));
    assert_eq!(
        json["result"]["ok"],
        serde_json::json!([true, true, true, true])
    );
    assert!(json["result"]["fileCount"].as_u64().unwrap_or(0) > 0);
    assert_eq!(json["result"]["deadCodeKind"], "dead-code");
    assert_eq!(json["result"]["dupesKind"], "dupes");
    assert_eq!(json["result"]["repeated"], json["result"]["fileCount"]);

    let calls = json["calls"].as_array().expect("calls");
    assert_eq!(calls.len(), 4, "every element stays in the trace: {output}");
    assert_eq!(calls[3]["cache_hit"], true);
    assert!(calls[0].get("cache_hit").is_none());
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
        allow_remote_extends: None,
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
    assert_eq!(json["namespace"].as_str(), Some("value"));
    assert_eq!(json["is_used"].as_bool(), Some(true));
}

#[tokio::test]
async fn api_backed_trace_export_tool_returns_json() {
    let root = fixture_path("basic-project");
    let result = run_trace_export_tool(crate::params::TraceExportParams {
        file: "src/utils.ts".to_string(),
        export_name: "usedFunction".to_string(),
        root: Some(root.to_string_lossy().to_string()),
        config: None,
        allow_remote_extends: None,
        production: None,
        workspace: None,
        no_cache: None,
        threads: None,
    })
    .await
    .unwrap();

    assert_eq!(result.is_error, Some(false));

    let text = extract_text(&result);
    let json: serde_json::Value = serde_json::from_str(text)
        .unwrap_or_else(|e| panic!("should parse as JSON: {e}\ntext: {text}"));
    assert_eq!(json["file"].as_str(), Some("src/utils.ts"));
    assert_eq!(json["export_name"].as_str(), Some("usedFunction"));
    assert_eq!(json["namespace"].as_str(), Some("value"));
    assert_eq!(json["is_used"].as_bool(), Some(true));
}

#[tokio::test]
async fn api_backed_trace_error_tool_resolves_frames_against_the_graph() {
    let root = fixture_path("basic-project");
    let result = run_trace_error_tool(crate::params::TraceErrorParams {
        trace: "TypeError: value is not a function\n    at usedFunction (src/utils.ts:1:29)\n    at boot (node:internal/main/run_main_module:23:47)\n"
            .to_string(),
        source: None,
        root: Some(root.to_string_lossy().to_string()),
        config: None,
        allow_remote_extends: None,
        production: None,
        workspace: None,
        no_cache: None,
        threads: None,
    })
    .await
    .unwrap();

    assert_eq!(result.is_error, Some(false));

    let text = extract_text(&result);
    let json: serde_json::Value = serde_json::from_str(text)
        .unwrap_or_else(|e| panic!("should parse as JSON: {e}\ntext: {text}"));
    assert_eq!(json["schema_version"].as_str(), Some("1"));
    assert_eq!(json["source"].as_str(), Some("mcp"));
    assert_eq!(json["frames"][0]["resolution"].as_str(), Some("resolved"));
    assert_eq!(
        json["frames"][0]["candidates"][0]["symbol"].as_str(),
        Some("usedFunction")
    );
    assert_eq!(
        json["frames"][1]["origin"].as_str(),
        Some("out_of_corpus"),
        "a runtime-internal frame keeps its row and is never asked about"
    );
    assert_eq!(json["counts"]["frames"].as_u64(), Some(2));
}

#[tokio::test]
async fn api_backed_trace_error_tool_refuses_an_empty_trace() {
    let root = fixture_path("basic-project");
    let result = run_trace_error_tool(crate::params::TraceErrorParams {
        trace: "   ".to_string(),
        source: None,
        root: Some(root.to_string_lossy().to_string()),
        config: None,
        allow_remote_extends: None,
        production: None,
        workspace: None,
        no_cache: None,
        threads: None,
    })
    .await
    .unwrap();

    assert_eq!(result.is_error, Some(true));
    assert!(
        extract_text(&result).contains("trace"),
        "the refusal must name the field: {}",
        extract_text(&result)
    );
}

#[tokio::test]
async fn e2e_trace_file_returns_json() {
    let bin = fallow_binary();
    let root = fixture_path("basic-project");
    let args = build_trace_file_args(&crate::params::TraceFileParams {
        file: "src/utils.ts".to_string(),
        root: Some(root.to_string_lossy().to_string()),
        config: None,
        allow_remote_extends: None,
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
async fn e2e_impact_closure_returns_json() {
    let bin = fallow_binary();
    let root = fixture_path("basic-project");
    let args = build_impact_closure_args(&crate::params::ImpactClosureParams {
        path: "src/utils.ts".to_string(),
        root: Some(root.to_string_lossy().to_string()),
        config: None,
        allow_remote_extends: None,
        production: None,
        workspace: None,
        no_cache: None,
        threads: None,
        max_output_bytes: None,
    })
    .unwrap();
    let result = run_fallow(&bin, &args).await.unwrap();

    assert_eq!(result.is_error, Some(false));

    let text = extract_text(&result);
    let json: serde_json::Value = serde_json::from_str(text)
        .unwrap_or_else(|e| panic!("should parse as JSON: {e}\ntext: {text}"));
    assert_eq!(json["seed"].as_str(), Some("src/utils.ts"));
    assert!(json["affected_not_shown"].is_array());
    assert!(json["coordination_gap"].is_array());
}

#[tokio::test]
async fn e2e_inspect_target_file_returns_evidence_bundle() {
    let bin = fallow_binary();
    let root = fixture_path("basic-project");
    let result = inspect_target(
        &bin,
        &crate::params::InspectTargetParams {
            target: crate::params::InspectTarget::File {
                file: "src/utils.ts".to_string(),
            },
            root: Some(root.to_string_lossy().to_string()),
            config: None,
            allow_remote_extends: None,
            production: None,
            workspace: None,
            no_cache: None,
            threads: None,
            type_aware: None,
            type_aware_projects: None,
            type_aware_require: None,
            symbol_chain: None,
            include_churn: None,
            max_output_bytes: None,
        },
    )
    .await
    .unwrap();

    assert_eq!(result.is_error, Some(false));

    let text = extract_text(&result);
    let json: serde_json::Value = serde_json::from_str(text)
        .unwrap_or_else(|e| panic!("should parse as JSON: {e}\ntext: {text}"));
    assert_eq!(json["kind"].as_str(), Some("inspect_target"));
    assert_eq!(json["target"]["type"].as_str(), Some("file"));
    assert_eq!(json["identity"]["file"].as_str(), Some("src/utils.ts"));
    assert_eq!(
        json["evidence"]["trace_file"]["status"].as_str(),
        Some("ok")
    );
    assert_eq!(json["evidence"]["dead_code"]["status"].as_str(), Some("ok"));
    assert!(json["evidence"]["trace_export"].is_null());
    assert!(json["evidence"].get("churn").is_none());
}

#[tokio::test]
async fn e2e_inspect_target_symbol_returns_symbol_and_file_evidence() {
    let bin = fallow_binary();
    let root = fixture_path("basic-project");
    let result = inspect_target(
        &bin,
        &crate::params::InspectTargetParams {
            target: crate::params::InspectTarget::Symbol {
                file: "src/utils.ts".to_string(),
                export_name: "usedFunction".to_string(),
            },
            root: Some(root.to_string_lossy().to_string()),
            config: None,
            allow_remote_extends: None,
            production: None,
            workspace: None,
            no_cache: None,
            threads: None,
            type_aware: None,
            type_aware_projects: None,
            type_aware_require: None,
            symbol_chain: None,
            include_churn: None,
            max_output_bytes: None,
        },
    )
    .await
    .unwrap();

    assert_eq!(result.is_error, Some(false));

    let text = extract_text(&result);
    let json: serde_json::Value = serde_json::from_str(text)
        .unwrap_or_else(|e| panic!("should parse as JSON: {e}\ntext: {text}"));
    assert_eq!(json["kind"].as_str(), Some("inspect_target"));
    assert_eq!(json["target"]["type"].as_str(), Some("symbol"));
    assert_eq!(json["identity"]["file"].as_str(), Some("src/utils.ts"));
    assert_eq!(
        json["identity"]["export_name"].as_str(),
        Some("usedFunction")
    );
    assert_eq!(json["identity"]["is_used"].as_bool(), Some(true));
    assert_eq!(
        json["evidence"]["trace_export"]["status"].as_str(),
        Some("ok")
    );
    assert_eq!(
        json["evidence"]["duplication"]["scope"].as_str(),
        Some("project_filtered_to_file")
    );
    assert!(
        json["warnings"]
            .as_array()
            .is_some_and(|warnings| warnings.iter().any(|warning| warning
                .as_str()
                .is_some_and(|warning| warning.contains("file-scoped")))),
        "symbol bundles should make file-scoped evidence explicit"
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
        allow_remote_extends: None,
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
        allow_remote_extends: None,
        workspace: None,
        mode: None,
        near: None,
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
async fn api_backed_trace_clone_tool_returns_json() {
    let root = fixture_path("duplicate-code");
    let result = run_trace_clone_tool(crate::params::TraceCloneParams {
        file: Some("src/original.ts".to_string()),
        line: Some(2),
        fingerprint: None,
        root: Some(root.to_string_lossy().to_string()),
        config: None,
        allow_remote_extends: None,
        workspace: None,
        mode: None,
        near: None,
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
    .await
    .unwrap();

    assert_eq!(result.is_error, Some(false));

    let text = extract_text(&result);
    let json: serde_json::Value = serde_json::from_str(text)
        .unwrap_or_else(|e| panic!("should parse as JSON: {e}\ntext: {text}"));
    assert_eq!(json["file"].as_str(), Some("src/original.ts"));
    assert_eq!(json["line"].as_u64(), Some(2));
    assert!(json["matched_instance"].is_object());
    assert!(json["clone_groups"].is_array());
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

/// Write a project whose only consumer of an export failed to parse, so the
/// run records `source-parse-degraded` and every reachability finding it
/// produces carries a `reachability_caveats` entry.
///
/// `src/lib.ts` exports `needed`, which nothing the run could read still uses:
/// the file that imports it stops at a syntax error before that import is
/// extracted. The export is therefore reported unused, and the removal rests
/// on a file the run did not fully analyze.
fn write_caveated_export_project(root: &std::path::Path) {
    std::fs::create_dir_all(root.join("src")).expect("create src");
    std::fs::write(
        root.join("package.json"),
        r#"{ "name": "mcp-caveat-withholding", "version": "1.0.0", "main": "src/index.ts" }"#,
    )
    .expect("write manifest");
    std::fs::write(
        root.join("src/lib.ts"),
        "export const needed = 1;\nexport const alsoUsed = 2;\n",
    )
    .expect("write library");
    std::fs::write(
        root.join("src/degraded.ts"),
        "import { needed } from \"./lib\";\n\nexport const broken = (): number => {\n  return needed(\n};\n",
    )
    .expect("write unparseable importer");
    std::fs::write(
        root.join("src/index.ts"),
        "import \"./degraded\";\nimport { alsoUsed } from \"./lib\";\n\nexport const run = (): number => alsoUsed;\n",
    )
    .expect("write entry module");
}

fn fix_params(root: &std::path::Path) -> crate::params::FixParams {
    crate::params::FixParams {
        root: Some(root.to_string_lossy().to_string()),
        no_cache: Some(true),
        ..Default::default()
    }
}

/// The caveat withholding is a CLI-side decision, and both MCP fix tools are
/// thin wrappers over that CLI. Nothing pinned that they inherit it, so an
/// agent could have been handed a removal the `fallow fix` command declines.
#[tokio::test]
async fn e2e_fix_preview_withholds_a_removal_the_run_cannot_evidence() {
    let bin = fallow_binary();
    let dir = tempfile::tempdir().expect("temporary project");
    let root = dir.path().join("project");
    write_caveated_export_project(&root);

    let result = run_fix_preview(&bin, fix_params(&root))
        .await
        .expect("fix preview runs");
    assert_eq!(result.is_error, Some(false));

    let text = extract_text(&result);
    let json: serde_json::Value = serde_json::from_str(text)
        .unwrap_or_else(|e| panic!("should parse as JSON: {e}\ntext: {text}"));

    assert_eq!(json["dry_run"], true, "{json}");
    assert_eq!(
        json["fixes"][0]["skip_reason"], "low_confidence_incomplete_analysis",
        "{json}"
    );
    assert_eq!(
        json["fixes"][0]["reachability_caveats"],
        serde_json::json!(["incomplete-import-graph"]),
        "{json}"
    );
    assert_eq!(json["skipped_low_confidence_exports"], 1, "{json}");
    assert_eq!(json["total_fixed"], 0, "{json}");
}

/// The withholding is only worth anything on the tool that writes. `fix_apply`
/// must report the same skip AND leave the file on disk untouched.
#[tokio::test]
async fn e2e_fix_apply_inherits_the_withholding_and_writes_nothing() {
    let bin = fallow_binary();
    let dir = tempfile::tempdir().expect("temporary project");
    let root = dir.path().join("project");
    write_caveated_export_project(&root);
    let library = root.join("src/lib.ts");
    let before = std::fs::read_to_string(&library).expect("read library");

    let result = run_fix_apply(&bin, fix_params(&root))
        .await
        .expect("fix apply runs");
    assert_eq!(result.is_error, Some(false));

    let text = extract_text(&result);
    let json: serde_json::Value = serde_json::from_str(text)
        .unwrap_or_else(|e| panic!("should parse as JSON: {e}\ntext: {text}"));

    assert_eq!(json["dry_run"], false, "{json}");
    assert_eq!(
        json["fixes"][0]["skip_reason"], "low_confidence_incomplete_analysis",
        "{json}"
    );
    assert_eq!(json["total_fixed"], 0, "{json}");
    assert_eq!(
        std::fs::read_to_string(&library).expect("re-read library"),
        before,
        "a withheld removal must not reach the file"
    );
}

/// A baseline whose entries no longer match anything is the run #2676 is
/// about: the `baseline` parameter forces the CLI subprocess, the CLI's
/// advisory goes to stderr where `--quiet` removes it, and its exit 1 becomes
/// a success. Everything the agent could act on then lives in members it was
/// never told to read.
///
/// This drives the real binary rather than a fixture envelope, because the
/// claim is about what a tool call returns end to end, and because the
/// staleness object only exists on a run that actually loaded a baseline.
#[tokio::test]
async fn e2e_analyze_warns_when_the_loaded_baseline_matched_nothing() {
    let bin = fallow_binary();
    let dir = tempfile::tempdir().expect("temporary project");
    let root = dir.path().join("project");
    let source = root.join("src");
    std::fs::create_dir_all(&source).expect("create project");
    std::fs::write(
        root.join("package.json"),
        r#"{ "name": "stale-baseline-probe", "version": "1.0.0", "private": true }"#,
    )
    .expect("write manifest");
    std::fs::write(
        source.join("library.ts"),
        "export const first = (): number => 1;\nexport const second = (): number => 2;\n",
    )
    .expect("write library");

    let baseline = dir.path().join("baseline.json");
    let save = crate::params::AnalyzeParams {
        root: Some(root.to_string_lossy().to_string()),
        save_baseline: Some(baseline.to_string_lossy().to_string()),
        ..Default::default()
    };
    run_analyze(&bin, save).await.expect("baseline is saved");
    assert!(baseline.is_file(), "the run should have written a baseline");

    // Move the file the baseline recorded. Every entry now matches nothing,
    // while the run still produces findings to compare against.
    std::fs::rename(source.join("library.ts"), source.join("renamed.ts"))
        .expect("rename the analyzed file");

    let compare = crate::params::AnalyzeParams {
        root: Some(root.to_string_lossy().to_string()),
        baseline: Some(baseline.to_string_lossy().to_string()),
        ..Default::default()
    };
    let result = run_analyze(&bin, compare).await.expect("analyze runs");

    assert_eq!(
        result.is_error,
        Some(false),
        "a stale baseline is a verdict, not a tool failure"
    );
    let text = extract_text(&result);
    let json: serde_json::Value = serde_json::from_str(text)
        .unwrap_or_else(|e| panic!("the result must stay parseable: {e}\ntext: {text}"));

    assert_eq!(
        json["baseline_staleness"]["gate_trips"], true,
        "fixture precondition: the baseline should have rotted\n{json}"
    );
    let warnings = json["warnings"]
        .as_array()
        .unwrap_or_else(|| panic!("the result should carry warnings\n{json}"));
    let staleness = warnings
        .iter()
        .filter_map(serde_json::Value::as_str)
        .find(|entry| entry.starts_with("Baseline staleness:"))
        .unwrap_or_else(|| panic!("no baseline warning in {warnings:?}"));
    assert!(
        staleness.contains("--fail-on-stale-baseline"),
        "{staleness}"
    );
    assert!(staleness.contains("save_baseline"), "{staleness}");
}

/// A failing duplication threshold is a gate, and a gate an agent cannot see is
/// the defect this change exists to remove. The programmatic duplication route
/// has no threshold comparison, so a direct `find_dupes` call used to answer
/// with no verdict at all while the same call with an unrelated `group_by`
/// beside it reported one. Both surfaces must now say the same thing.
#[tokio::test]
async fn e2e_find_dupes_reports_a_failing_threshold_on_both_routes() {
    let bin = fallow_binary();
    let root = fixture_path("duplicate-code");

    let direct = crate::params::FindDupesParams {
        root: Some(root.to_string_lossy().to_string()),
        threshold: Some(0.1),
        ..Default::default()
    };
    let grouped = crate::params::FindDupesParams {
        root: Some(root.to_string_lossy().to_string()),
        threshold: Some(0.1),
        group_by: Some("directory".to_string()),
        ..Default::default()
    };

    for (label, params) in [("direct", direct), ("grouped", grouped)] {
        let result = run_find_dupes(&bin, params).await.expect("find_dupes runs");
        assert_eq!(result.is_error, Some(false), "{label}");

        let text = extract_text(&result);
        let json: serde_json::Value = serde_json::from_str(text)
            .unwrap_or_else(|e| panic!("{label} must stay parseable: {e}\ntext: {text}"));

        assert_eq!(
            json["gate_outcomes"]["duplication-threshold"]["status"], "fail",
            "{label} should carry the gate it armed\n{json}"
        );
        let warnings = json["warnings"]
            .as_array()
            .unwrap_or_else(|| panic!("{label} should carry warnings\n{json}"));
        let gate = warnings
            .iter()
            .filter_map(serde_json::Value::as_str)
            .find(|entry| entry.starts_with("Gate duplication-threshold failed"))
            .unwrap_or_else(|| panic!("{label}: no gate warning in {warnings:?}"));
        assert!(gate.contains("threshold 0.1"), "{label}: {gate}");
        assert!(gate.contains("enforced"), "{label}: {gate}");
    }
}
