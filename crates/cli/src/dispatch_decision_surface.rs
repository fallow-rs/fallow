//! The `AuditOptions` of the decision-surface path of `fallow audit`.

use super::*;

pub(super) fn decision_surface_audit_options<'a>(
    dispatch: &'a DispatchContext<'a>,
    inputs: &'a ResolvedAuditInputs,
    max_decisions: usize,
) -> audit::AuditOptions<'a> {
    let cli = dispatch.cli;
    audit::AuditOptions {
        root: dispatch.root,
        config_path: &cli.config,
        cache_dir: &inputs.cache_dir,
        output: dispatch.output,
        json_style: dispatch.json_style,
        no_cache: cli.no_cache,
        threads: dispatch.threads,
        quiet: dispatch.quiet,
        allow_remote_extends: cli.allow_remote_extends,
        changed_since: cli.changed_since.as_deref(),
        production: cli.production,
        production_dead_code: Some(inputs.production.dead_code),
        production_health: Some(inputs.production.health),
        production_dupes: Some(inputs.production.dupes),
        workspace: cli.workspace.as_deref(),
        changed_workspaces: cli.changed_workspaces.as_deref(),
        explain: cli.explain,
        explain_skipped: cli.explain_skipped,
        performance: cli.performance,
        group_by: cli.group_by,
        dead_code_baseline: inputs.dead_code_baseline.as_deref(),
        health_baseline: inputs.health_baseline.as_deref(),
        dupes_baseline: inputs.dupes_baseline.as_deref(),
        health_baseline_mode: cli.baseline_mode.unwrap_or_default().into(),
        // The decision surface renders the apex brief through
        // `run_decision_surface`, which owns no exit gates and prints no gate
        // lines, so the flag has nothing to say on this path.
        fail_on_stale_baseline: false,
        max_crap: None,
        file_lines_check: inputs.file_lines_check,
        coverage: None,
        coverage_root: None,
        gate: inputs.audit_cfg.gate,
        worsened: inputs.audit_cfg.worsened,
        include_entry_exports: cli.include_entry_exports,
        show_cascade: cli.show_cascade,
        fail_on_parse_error: false,
        analyses: audit::AuditAnalysisSelection::ALL,
        // Decision-surface (brief apex) does not render styling; keep it lean.
        css: false,
        css_deep: false,
        runtime_coverage: None,
        min_invocations_hot: 0,
        brief: true,
        max_decisions,
        walkthrough_guide: false,
        walkthrough: false,
        mark_viewed: &[],
        show_cleared: false,
        walkthrough_file: None,
        show_deprioritized: false,
        scope: None,
    }
}
