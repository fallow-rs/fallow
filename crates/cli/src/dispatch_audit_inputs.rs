//! Resolves the inputs of `fallow audit` from the command flags.

use super::*;

pub(super) fn resolve_audit_inputs(
    dispatch: &DispatchContext<'_>,
    args: &AuditDispatchArgs,
) -> Result<ResolvedAuditInputs, ExitCode> {
    let cli = dispatch.cli;
    let root = dispatch.root;
    let output = dispatch.output;
    let config = load_config(
        root,
        &cli.config,
        LoadConfigArgs {
            output,
            no_cache: cli.no_cache,
            threads: dispatch.threads,
            production: cli.production,
            quiet: dispatch.quiet,
            allow_remote_extends: cli.allow_remote_extends,
        },
    )?;
    let cache_dir = config.cache_dir.clone();
    let audit_cfg = config.audit;
    let file_lines_check = config.health.file_lines_check_enabled();
    let production = resolve_production_modes(
        cli,
        root,
        output,
        args.production_dead_code,
        args.production_health,
        args.production_dupes,
    )?;
    let analyses = resolve_audit_analyses(&audit_cfg, args);
    if !(analyses.dead_code || analyses.health || analyses.dupes) {
        return Err(emit_error(
            "this audit selects no sub-analysis: --skip and the audit.deadCode, audit.health and audit.dupes config keys leave nothing to run",
            2,
            output,
        ));
    }
    // A skipped sub-analysis reads no baseline. An explicit flag for one is a
    // mistake; a configured one is dropped so the baseline gates do not see it.
    let baseline = |enabled: bool, flag: &str, cli_path: Option<&Path>, cfg_path: Option<&str>| {
        if enabled {
            return Ok(resolve_audit_baseline_path(root, cli_path, cfg_path));
        }
        if cli_path.is_some() {
            return Err(emit_error(
                &format!(
                    "{flag} has no effect: this audit skips that sub-analysis (--only, --skip or the audit config)"
                ),
                2,
                output,
            ));
        }
        Ok(None)
    };
    let resolved_dead_code_baseline = baseline(
        analyses.dead_code,
        "--dead-code-baseline",
        args.dead_code_baseline.as_deref(),
        audit_cfg.dead_code_baseline.as_deref(),
    )?;
    let resolved_health_baseline = baseline(
        analyses.health,
        "--health-baseline",
        args.health_baseline.as_deref(),
        audit_cfg.health_baseline.as_deref(),
    )?;
    let resolved_dupes_baseline = baseline(
        analyses.dupes,
        "--dupes-baseline",
        args.dupes_baseline.as_deref(),
        audit_cfg.dupes_baseline.as_deref(),
    )?;
    let coverage_inputs = resolve_coverage_inputs(
        args.coverage.as_deref(),
        args.coverage_root.as_deref(),
        output,
        || Ok(config.health),
    )?;

    Ok(ResolvedAuditInputs {
        audit_cfg,
        analyses,
        cache_dir,
        production,
        dead_code_baseline: resolved_dead_code_baseline,
        health_baseline: resolved_health_baseline,
        dupes_baseline: resolved_dupes_baseline,
        coverage: coverage_inputs.coverage,
        coverage_root: coverage_inputs.coverage_root,
        file_lines_check,
    })
}
