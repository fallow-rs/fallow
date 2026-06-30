use crate::params::AuditParams;

use rmcp::ErrorData as McpError;
use rmcp::model::{CallToolResult, Content};

use super::{
    VALID_AUDIT_GATES, push_global, push_scope, push_str_flag, run_tool, validation_error_body,
};

/// Run the `audit` tool. This remains CLI-backed until `fallow-api` exposes a
/// command-neutral audit runner; the fallback is owned here so the server
/// handler does not know how audit is executed.
pub async fn run_audit(binary: &str, params: AuditParams) -> Result<CallToolResult, McpError> {
    match build_audit_args(&params) {
        Ok(args) => run_tool(binary, "audit", &args).await,
        Err(msg) => Ok(CallToolResult::error(vec![Content::text(msg)])),
    }
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
