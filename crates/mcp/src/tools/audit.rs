use crate::params::AuditParams;

use fallow_api::{
    AnalysisOptions, AuditGate, AuditOptions, run_audit as run_audit_api,
    serialize_audit_programmatic_json,
};
use rmcp::ErrorData as McpError;
use rmcp::model::{CallToolResult, Content};

use super::{
    VALID_AUDIT_GATES,
    api_runtime::{
        changed_since_from_param, env_diff_file, json_success, non_empty_path, non_empty_string,
        programmatic_error_body, run_api_blocking,
    },
    fallback_policy::{baseline_fallback_reason, filled, grouped_fallback_reason},
    push_global, push_scope, push_str_flag, run_tool, validation_error_body,
};

/// Run the `audit` tool through the typed API when parameters map cleanly to
/// the programmatic contract, falling back to the CLI for CLI-only surfaces.
pub async fn run_audit(binary: &str, params: AuditParams) -> Result<CallToolResult, McpError> {
    if !requires_cli_fallback(&params) {
        let options = match audit_options_from_params(&params) {
            Ok(options) => options,
            Err(msg) => return Ok(CallToolResult::error(vec![Content::text(msg)])),
        };
        let result = run_api_blocking("audit", move || {
            run_audit_api(&options).and_then(serialize_audit_programmatic_json)
        })
        .await?
        .map_or_else(
            |err| CallToolResult::error(vec![Content::text(programmatic_error_body(&err))]),
            |value| json_success(&value),
        );
        return Ok(result);
    }

    match build_audit_args(&params) {
        Ok(args) => run_tool(binary, "audit", &args).await,
        Err(msg) => Ok(CallToolResult::error(vec![Content::text(msg)])),
    }
}

pub fn run_audit_api_value(params: &AuditParams) -> Result<Option<serde_json::Value>, String> {
    if requires_cli_fallback(params) {
        return Ok(None);
    }
    let options = audit_options_from_params(params)?;
    run_audit_api(&options)
        .and_then(serialize_audit_programmatic_json)
        .map(Some)
        .map_err(|err| programmatic_error_body(&err))
}

/// Build CLI arguments for the `audit` tool.
pub fn build_audit_args(params: &AuditParams) -> Result<Vec<String>, String> {
    if let Some(ref gate) = params.gate
        && !VALID_AUDIT_GATES.contains(&gate.as_str())
    {
        return Err(validation_error_body(format!(
            "Invalid gate '{gate}'. Valid values: new-only, all"
        )));
    }

    let mut args = vec![
        "audit".to_string(),
        "--format".to_string(),
        "json".to_string(),
        "--quiet".to_string(),
        "--explain".to_string(),
    ];

    push_global(
        &mut args,
        params.root.as_deref(),
        params.config.as_deref(),
        params.no_cache,
        params.threads,
    );
    push_str_flag(&mut args, "--base", params.base.as_deref());
    push_scope(&mut args, params.production, params.workspace.as_deref());
    push_audit_production_flags(&mut args, params);
    if params.css == Some(false) {
        args.push("--no-css".to_string());
    }
    if params.css_deep == Some(true) {
        args.push("--css-deep".to_string());
    } else if params.css_deep == Some(false) {
        args.push("--no-css-deep".to_string());
    }
    push_str_flag(&mut args, "--group-by", params.group_by.as_deref());
    push_str_flag(&mut args, "--gate", params.gate.as_deref());
    push_audit_baseline_flags(&mut args, params);
    if params.explain_skipped == Some(true) {
        args.push("--explain-skipped".to_string());
    }
    push_audit_coverage_flags(&mut args, params);

    Ok(args)
}

/// Push the per-analysis production-mode flags for the `audit` tool.
fn push_audit_production_flags(args: &mut Vec<String>, params: &AuditParams) {
    if params.production_dead_code == Some(true) {
        args.push("--production-dead-code".to_string());
    }
    if params.production_health == Some(true) {
        args.push("--production-health".to_string());
    }
    if params.production_dupes == Some(true) {
        args.push("--production-dupes".to_string());
    }
}

/// Push the per-sub-analysis baseline flags for the `audit` tool.
fn push_audit_baseline_flags(args: &mut Vec<String>, params: &AuditParams) {
    push_str_flag(
        args,
        "--dead-code-baseline",
        params.dead_code_baseline.as_deref(),
    );
    push_str_flag(args, "--health-baseline", params.health_baseline.as_deref());
    push_str_flag(args, "--dupes-baseline", params.dupes_baseline.as_deref());
}

/// Push the coverage, entry-export, and runtime-coverage flags for `audit`.
fn push_audit_coverage_flags(args: &mut Vec<String>, params: &AuditParams) {
    if let Some(max_crap) = params.max_crap {
        args.extend(["--max-crap".to_string(), format!("{max_crap}")]);
    }
    push_str_flag(args, "--coverage", params.coverage.as_deref());
    push_str_flag(args, "--coverage-root", params.coverage_root.as_deref());
    if params.include_entry_exports == Some(true) {
        args.push("--include-entry-exports".to_string());
    }
    push_str_flag(
        args,
        "--runtime-coverage",
        params.runtime_coverage.as_deref(),
    );
    if let Some(min_invocations_hot) = params.min_invocations_hot {
        args.extend([
            "--min-invocations-hot".to_string(),
            format!("{min_invocations_hot}"),
        ]);
    }
}

fn requires_cli_fallback(params: &AuditParams) -> bool {
    cli_fallback_reason(params).is_some()
}

fn cli_fallback_reason(params: &AuditParams) -> Option<&'static str> {
    let gate = params.gate.as_deref().unwrap_or("new-only");
    if !VALID_AUDIT_GATES.contains(&gate) {
        return Some("invalid gate");
    }
    baseline_fallback_reason(params.dead_code_baseline.as_deref(), None)
        .or_else(|| baseline_fallback_reason(params.health_baseline.as_deref(), None))
        .or_else(|| baseline_fallback_reason(params.dupes_baseline.as_deref(), None))
        .or_else(|| grouped_fallback_reason(params.group_by.as_deref()))
        .map(|_| "baseline or grouped output")
        .or_else(|| (params.explain_skipped == Some(true)).then_some("duplication skipped notes"))
        .or_else(|| filled(params.runtime_coverage.as_deref()).then_some("runtime coverage"))
}

fn audit_options_from_params(params: &AuditParams) -> Result<AuditOptions, String> {
    let gate = audit_gate_from_param(params.gate.as_deref())?;
    Ok(AuditOptions {
        analysis: AnalysisOptions {
            root: non_empty_path(params.root.as_deref()),
            config_path: non_empty_path(params.config.as_deref()),
            no_cache: params.no_cache.unwrap_or(false),
            threads: params.threads,
            diff_file: env_diff_file(),
            production: params.production.unwrap_or(false),
            production_override: params.production,
            changed_since: changed_since_from_param(None),
            workspace: non_empty_string(params.workspace.as_deref()).map(|value| vec![value]),
            changed_workspaces: None,
            explain: true,
        },
        base: non_empty_string(params.base.as_deref()),
        production: params.production.unwrap_or(false),
        production_dead_code: params.production_dead_code,
        production_health: params.production_health,
        production_dupes: params.production_dupes,
        css: params.css,
        css_deep: params.css_deep,
        gate,
        max_crap: params.max_crap,
        coverage: non_empty_path(params.coverage.as_deref()),
        coverage_root: non_empty_path(params.coverage_root.as_deref()),
        include_entry_exports: params.include_entry_exports.unwrap_or(false),
        runtime_coverage: non_empty_path(params.runtime_coverage.as_deref()),
        min_invocations_hot: params.min_invocations_hot.unwrap_or(100),
    })
}

fn audit_gate_from_param(value: Option<&str>) -> Result<AuditGate, String> {
    match value.unwrap_or("new-only") {
        "new-only" => Ok(AuditGate::NewOnly),
        "all" => Ok(AuditGate::All),
        other => Err(validation_error_body(format!(
            "Invalid gate '{other}'. Valid values: new-only, all"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use std::process::Command;

    use rmcp::model::RawContent;

    use super::*;

    #[test]
    fn default_new_only_audit_uses_programmatic_api_route() {
        let params = AuditParams::default();
        assert!(!requires_cli_fallback(&params));
        let options = audit_options_from_params(&params).expect("audit options");
        assert_eq!(options.gate, AuditGate::NewOnly);
    }

    #[test]
    fn gate_all_audit_uses_programmatic_api_route() {
        let params = AuditParams {
            gate: Some("all".to_string()),
            ..AuditParams::default()
        };
        assert!(!requires_cli_fallback(&params));
        let options = audit_options_from_params(&params).expect("audit options");
        assert_eq!(options.gate, AuditGate::All);
        assert!(options.analysis.explain);
    }

    #[test]
    fn cli_only_audit_surfaces_keep_fallback() {
        let baseline = AuditParams {
            gate: Some("all".to_string()),
            dead_code_baseline: Some("baseline.json".to_string()),
            ..AuditParams::default()
        };
        let grouped = AuditParams {
            gate: Some("all".to_string()),
            group_by: Some("owner".to_string()),
            ..AuditParams::default()
        };
        let runtime = AuditParams {
            gate: Some("all".to_string()),
            runtime_coverage: Some("coverage".to_string()),
            ..AuditParams::default()
        };

        assert!(requires_cli_fallback(&baseline));
        assert!(requires_cli_fallback(&grouped));
        assert!(requires_cli_fallback(&runtime));
    }

    #[tokio::test]
    async fn run_audit_gate_all_api_path_returns_json_without_cli_binary() {
        let project = audit_fixture();

        let result = run_audit(
            "unused-binary-on-api-path",
            AuditParams {
                root: Some(project.path().display().to_string()),
                base: Some("HEAD".to_string()),
                gate: Some("all".to_string()),
                no_cache: Some(true),
                ..AuditParams::default()
            },
        )
        .await
        .expect("api result");

        assert_eq!(result.is_error, Some(false));
        let text = match &result.content[0].raw {
            RawContent::Text(text) => &text.text,
            _ => panic!("expected text content"),
        };
        let json: serde_json::Value = serde_json::from_str(text).expect("json");
        assert_eq!(json["kind"], "audit");
        assert_eq!(json["command"], "audit");
        assert!(json["dead_code"].is_object());
    }

    #[tokio::test]
    async fn run_audit_default_new_only_api_path_marks_introduced_without_cli_binary() {
        let project = audit_fixture();

        let result = run_audit(
            "unused-binary-on-api-path",
            AuditParams {
                root: Some(project.path().display().to_string()),
                base: Some("HEAD".to_string()),
                no_cache: Some(true),
                ..AuditParams::default()
            },
        )
        .await
        .expect("api result");

        assert_eq!(result.is_error, Some(false));
        let text = match &result.content[0].raw {
            RawContent::Text(text) => &text.text,
            _ => panic!("expected text content"),
        };
        let json: serde_json::Value = serde_json::from_str(text).expect("json");
        assert_eq!(json["kind"], "audit");
        assert_eq!(json["attribution"]["gate"], "new-only");
        assert_eq!(json["attribution"]["dead_code_introduced"], 1);
        assert_eq!(json["dead_code"]["unused_files"][0]["introduced"], true);
    }

    fn audit_fixture() -> tempfile::TempDir {
        let project = tempfile::tempdir().expect("project");
        std::fs::create_dir_all(project.path().join("src")).expect("create src");
        std::fs::write(
            project.path().join("package.json"),
            r#"{"name":"audit-api","type":"module","main":"src/index.ts"}"#,
        )
        .expect("write package");
        std::fs::write(
            project.path().join("src/index.ts"),
            "console.log('entry');\n",
        )
        .expect("write entry");
        git(project.path(), &["init"]);
        git(project.path(), &["add", "."]);
        git(
            project.path(),
            &[
                "-c",
                "user.email=test@example.com",
                "-c",
                "user.name=Test",
                "-c",
                "commit.gpgsign=false",
                "commit",
                "-m",
                "initial",
            ],
        );
        std::fs::write(
            project.path().join("src/feature.ts"),
            "export const unused = 1;\n",
        )
        .expect("write changed source");
        project
    }

    fn git(root: &std::path::Path, args: &[&str]) {
        let status = Command::new("git")
            .args(args)
            .current_dir(root)
            .status()
            .expect("git command");
        assert!(status.success(), "git {args:?} failed");
    }
}
