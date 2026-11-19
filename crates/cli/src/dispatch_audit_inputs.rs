//! Resolves the inputs of `fallow audit` from the command flags.

use fallow_config::{ConfigKey, RunCommand, RunPlan, RunRequest};

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
    // Plan before `config.audit` moves out below.
    let plan = fallow_config::plan_run(config.plan_sections(), audit_request(args));
    let duplicates_off_in_config = !config.duplicates.runs_by_default();
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
    let analyses = audit_selection(plan, args, duplicates_off_in_config);
    if !(analyses.dead_code || analyses.health || analyses.dupes) {
        return Err(emit_input_error(
            "this audit selects no sub-analysis: --skip, the audit.deadCode, audit.health and audit.dupes config keys, and duplicates.enabled leave nothing to run",
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
            return Err(emit_input_error(
                &format!(
                    "{flag} has no effect: this audit skips that sub-analysis (--only, --skip or the audit config)"
                ),
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
    // `duplicates.enabled: false` alone turns the sub-analysis off here. A
    // `--dupes-baseline` flag is then no mistake of this command line: the
    // flag had an effect before the audit read that key, so it gets a note
    // and the run goes on.
    let dupes_off_by_duplicates_enabled = plan.duplication.off_by(ConfigKey::DuplicatesEnabled);
    if dupes_off_by_duplicates_enabled && args.dupes_baseline.is_some() {
        eprintln!("{}", combined::INERT_DUPES_BASELINE_NOTE);
    }
    let resolved_dupes_baseline = baseline(
        analyses.dupes,
        "--dupes-baseline",
        args.dupes_baseline
            .as_deref()
            .filter(|_| !dupes_off_by_duplicates_enabled),
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

/// The request of this audit. The review brief names every sub-analysis, so
/// `--only`, `--skip` and the config keys do not change it. Otherwise
/// `--only` names the listed sub-analyses, and without `--only` the
/// `audit.deadCode`, `audit.health` and `audit.dupes` keys decide, then
/// `duplicates.enabled`.
fn audit_request(args: &AuditDispatchArgs) -> RunRequest {
    if audit_renders_brief(args) {
        return RunRequest::all_named(RunCommand::Audit);
    }
    let (only, skip) = combined::planned_lists(&args.only, &args.skip);
    RunRequest::from_lists(RunCommand::Audit, &only, &skip)
}

/// The sub-analyses of this audit from its run `plan`.
fn audit_selection(
    plan: RunPlan,
    args: &AuditDispatchArgs,
    duplicates_off_in_config: bool,
) -> audit::AuditAnalysisSelection {
    let architecture = if audit_renders_brief(args) {
        crate::check::ArchitectureSelection::All
    } else {
        combined::resolve_architecture_selection(&args.only, &args.skip)
    };
    audit::AuditAnalysisSelection {
        dead_code: plan.dead_code.runs(),
        health: plan.health.runs(),
        dupes: plan.duplication.runs(),
        architecture,
        duplicates_off_in_config,
    }
}
