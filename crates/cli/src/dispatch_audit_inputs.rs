//! Resolves the inputs of `fallow audit` from the command flags.

use fallow_config::{ConfigKey, RunPlan};
use fallow_engine::resolved_run::ResolvedRun;

use super::*;

pub(super) struct ResolvedAuditInputs {
    /// The resolved run of the head side.
    pub(super) run: ResolvedRun,
    pub(super) audit_cfg: fallow_config::AuditConfig,
    pub(super) analyses: audit::AuditAnalysisSelection,
    pub(super) cache_dir: PathBuf,
    pub(super) production: ProductionModes,
    pub(super) dead_code_baseline: Option<PathBuf>,
    pub(super) health_baseline: Option<PathBuf>,
    pub(super) dupes_baseline: Option<PathBuf>,
    /// Istanbul coverage inputs resolved with the health precedence (flag,
    /// env, then `health.coverage` / `health.coverageRoot`), so the head and
    /// base passes score from the same map the standalone health run uses.
    pub(super) coverage: Option<PathBuf>,
    pub(super) coverage_root: Option<PathBuf>,
    pub(super) file_lines_check: bool,
}

pub(super) fn resolve_audit_inputs(
    dispatch: &DispatchContext<'_>,
    args: &AuditDispatchArgs,
) -> Result<ResolvedAuditInputs, ExitCode> {
    let root = dispatch.root;
    let output = dispatch.output;
    let run = dispatch.resolve_run()?;
    let config = run.config();
    let cache_dir = run.settings().cache_dir.value().clone();
    let plan = crate::cli_production::run_plan(&run);
    let duplicates_off_in_config = !config.duplicates.runs_by_default();
    let audit_cfg = config.audit.clone();
    let file_lines_check = config.health.file_lines_check_enabled();
    let production = run.production_modes();
    let analyses = audit_selection(plan, args, duplicates_off_in_config);
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
        combined::note_inert_dupes_baseline(run.root().value(), output, dispatch.quiet);
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
        || Ok(config.health.clone()),
    )?;

    Ok(ResolvedAuditInputs {
        run,
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
