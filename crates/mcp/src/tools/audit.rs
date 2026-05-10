use crate::params::AuditParams;

use super::{VALID_AUDIT_GATES, push_global, push_scope, push_str_flag, validation_error_body};

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
    if params.production_dead_code == Some(true) {
        args.push("--production-dead-code".to_string());
    }
    if params.production_health == Some(true) {
        args.push("--production-health".to_string());
    }
    if params.production_dupes == Some(true) {
        args.push("--production-dupes".to_string());
    }
    push_str_flag(&mut args, "--group-by", params.group_by.as_deref());
    push_str_flag(&mut args, "--gate", params.gate.as_deref());
    push_str_flag(
        &mut args,
        "--dead-code-baseline",
        params.dead_code_baseline.as_deref(),
    );
    push_str_flag(
        &mut args,
        "--health-baseline",
        params.health_baseline.as_deref(),
    );
    push_str_flag(
        &mut args,
        "--dupes-baseline",
        params.dupes_baseline.as_deref(),
    );
    if params.explain_skipped == Some(true) {
        args.push("--explain-skipped".to_string());
    }
    if let Some(max_crap) = params.max_crap {
        args.extend(["--max-crap".to_string(), format!("{max_crap}")]);
    }
    push_str_flag(&mut args, "--coverage", params.coverage.as_deref());
    push_str_flag(
        &mut args,
        "--coverage-root",
        params.coverage_root.as_deref(),
    );
    if params.include_entry_exports == Some(true) {
        args.push("--include-entry-exports".to_string());
    }
    push_str_flag(
        &mut args,
        "--runtime-coverage",
        params.runtime_coverage.as_deref(),
    );
    if let Some(min_invocations_hot) = params.min_invocations_hot {
        args.extend([
            "--min-invocations-hot".to_string(),
            format!("{min_invocations_hot}"),
        ]);
    }

    Ok(args)
}
