//! The dead-code pass of each audit side and its changed-file scope.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use fallow_config::PlannedAnalysis;
use fallow_engine::resolved_run::ResolvedRun;
use rustc_hash::FxHashSet;

use super::{AuditOptions, AuditTypeAwareOptions, BaseRunInputs, runs};
use crate::check::{CheckOptions, CheckResult, IssueFilters, TraceOptions};

/// One dead-code pass of an audit side.
#[derive(Clone, Copy)]
pub(super) struct CheckPass<'a> {
    /// The resolved run of the side.
    pub(super) run: &'a ResolvedRun,
    pub(super) changed_since: Option<&'a str>,
    /// `None` when the caller could not express a focus set for this
    /// analysis root.
    pub(super) changed_files: Option<&'a FxHashSet<PathBuf>>,
    pub(super) retain_modules_for_health: bool,
    pub(super) snapshot: fallow_config::AnalysisSnapshot,
}

/// Run dead code analysis for the audit pipeline.
/// Without `changed_files`, results are left unfiltered rather than filtered
/// against an empty set, which would drop every finding. A base pass, and a
/// head pass under `--gate new-only`, keep every unused file and unused export
/// (see [`scope_audit_dead_code`]).
pub(super) fn run_audit_check<'a>(
    opts: &'a AuditOptions<'a>,
    type_aware: AuditTypeAwareOptions<'a>,
    pass: CheckPass<'a>,
) -> Result<Option<CheckResult>, ExitCode> {
    let dead_code = opts.analyses.dead_code;
    let Some(runnable) = runs::sub_analysis(pass.run, PlannedAnalysis::DeadCode, dead_code) else {
        return Ok(None);
    };
    let keep_unused_code = match pass.snapshot {
        fallow_config::AnalysisSnapshot::Base => true,
        fallow_config::AnalysisSnapshot::Current => {
            matches!(opts.gate, fallow_config::AuditGate::NewOnly)
        }
    };
    let filters = IssueFilters {
        architecture_selection: opts.analyses.architecture,
        ..IssueFilters::default()
    };
    // The review brief needs the module graph for the impact closure, which
    // rides the retained-modules path. Force retention on the brief path even
    // when health does not share the dead-code parse (mismatched production
    // modes), so the graph is available before health consumes the shared parse.
    let retain_modules_for_health = pass.retain_modules_for_health || opts.brief;
    let trace_opts = TraceOptions {
        trace_export: None,
        trace_file: None,
        trace_dependency: None,
        impact_closure: None,
        symbol_impact: None,
        performance: opts.performance,
    };
    match crate::check::execute_check(&CheckOptions {
        root: opts.root,
        config_path: opts.config_path,
        output: opts.output,
        json_style: opts.json_style,
        no_cache: opts.no_cache,
        threads: opts.threads,
        quiet: opts.quiet,
        allow_remote_extends: opts.allow_remote_extends,
        fail_on_issues: false,
        filters: &filters,
        changed_since: pass.changed_since,
        // Audit compares head and base against its own changed files. The
        // scope keeps the unused code that the change made unused outside the
        // changed files.
        change_scope_owner: if keep_unused_code {
            fallow_engine::change_scope::ChangeScopeOwner::CallerKeepingUnusedCode
        } else {
            fallow_engine::change_scope::ChangeScopeOwner::Caller
        },
        no_package_baselines: false,
        diff_index: None,
        use_shared_diff_index: true,
        baseline: opts.dead_code_baseline,
        baseline_flag: "--dead-code-baseline",
        bare_run: false,
        save_baseline: None,
        // The gate is answered once by `note_stale_baseline_gate_inert`;
        // this sub-pass is change-scoped and could only stand down again.
        fail_on_stale_baseline: false,
        sarif_file: None,
        production: *runnable.production().value(),
        production_override: Some(*runnable.production().value()),
        run: Some(runnable),
        workspace: opts.workspace,
        changed_workspaces: opts.changed_workspaces,
        group_by: opts.group_by,
        include_dupes: false,
        type_aware: type_aware.enabled,
        type_aware_config_override: type_aware.config_default,
        type_aware_projects: type_aware.projects,
        type_aware_require: type_aware.require,
        trace_opts: &trace_opts,
        explain: opts.explain,
        top: None,
        file: &[],
        // Scope travels with the changed set (already intersected at the
        // audit prelude); the sub-passes stay unscoped.
        scope: None,
        finding_ids: None,
        include_entry_exports: opts.include_entry_exports,
        show_cascade: opts.show_cascade,
        fail_on_parse_error: opts.fail_on_parse_error,
        summary: false,
        regression_opts: crate::regression::RegressionOpts {
            fail_on_regression: false,
            tolerance: crate::regression::Tolerance::Absolute(0),
            args: crate::regression::RegressionArgs::default(),
            regression_baseline_file: None,
            save_target: crate::regression::SaveRegressionTarget::None,
            scoped: true,
            quiet: opts.quiet,
            output: opts.output,
        },
        retain_modules_for_health,
        defer_performance: false,
        analysis_snapshot: pass.snapshot,
        explain_skipped: opts.explain_skipped,
        surface: crate::check::CheckSurface::DeadCode,
    }) {
        Ok(mut result) => {
            if let Some(changed_files) = pass.changed_files {
                scope_audit_dead_code(opts, &mut result.results, changed_files, keep_unused_code);
            }
            Ok(Some(result))
        }
        Err(code) => Err(code),
    }
}

/// Scope the dead-code findings of one audit side to `changed_files`.
///
/// With `keep_unused_code`, every unused file and unused export stays, also
/// outside `changed_files`. The attribution then keeps such a finding only
/// when the base does not report it, because the change made that code unused.
/// A `--scope` path still bounds these findings, and the `--walkthrough-file`
/// response never counts as one.
fn scope_audit_dead_code(
    opts: &AuditOptions<'_>,
    results: &mut fallow_types::results::AnalysisResults,
    changed_files: &FxHashSet<PathBuf>,
    keep_unused_code: bool,
) {
    if !keep_unused_code {
        fallow_engine::changed_files::filter_results_by_changed_files(results, changed_files);
        return;
    }
    fallow_engine::changed_files::filter_results_by_changed_files_keeping_unused_code(
        results,
        changed_files,
    );
    // The `--walkthrough-file` response is command input, not project code,
    // so its file is never orphaned code.
    let walkthrough_file = opts
        .walkthrough_file
        .and_then(|path| dunce::canonicalize(path).ok());
    let in_report = |path: &Path| {
        walkthrough_file.as_deref() != Some(path)
            && opts
                .scope
                .as_deref()
                .is_none_or(|scope| crate::scope_path::scope_covers(scope, path))
    };
    results
        .unused_files
        .retain(|finding| in_report(&finding.file.path));
    results
        .unused_exports
        .retain(|finding| in_report(&finding.export.path));
}

pub(super) struct BaseOwnConfigInput<'a> {
    pub(super) opts: &'a AuditOptions<'a>,
    /// The head run, for the base copy of its config file.
    pub(super) head: &'a ResolvedRun,
    /// The config file of the head run.
    pub(super) current_config_path: Option<&'a Path>,
    pub(super) base_root: &'a Path,
    pub(super) base_cache_dir: &'a Path,
    pub(super) base_run: &'a BaseRunInputs,
    pub(super) focus: Option<&'a FxHashSet<PathBuf>>,
    /// The dead-code pass of the base under the head config.
    pub(super) base_check: Option<&'a CheckResult>,
}

/// The keys of the unused files and unused exports of the base under the
/// config of the base commit (see
/// `fallow_api::audit_run::AuditKeySnapshot::own_config_unused_code`).
///
/// This second dead-code pass runs only when the change touched the config
/// file of the head run, with the dead-code production mode of the head run.
/// It returns `None` when the change did not touch it, when the base config
/// does not resolve, and when the base config turns the `unused-files` or
/// `unused-exports` rule off. Then the base pass with the head config is the
/// only evidence. A finding of that pass in code that the
/// base config ignores also counts as unused under the base config (see
/// `fallow_api::audit_run::own_config_unused_code_keys`).
pub(super) fn base_own_config_unused_code(
    input: &BaseOwnConfigInput<'_>,
) -> Option<FxHashSet<String>> {
    let BaseOwnConfigInput {
        opts,
        head,
        current_config_path,
        base_root,
        base_cache_dir,
        base_run,
        focus,
        base_check,
    } = *input;
    let (focus, base_check) = (focus?, base_check?);
    let relative = fallow_api::audit_run::config_path_under_root(current_config_path?, opts.root)?;
    if !focus.contains(&base_root.join(&relative)) {
        return None;
    }
    // With `--config`, the base config is the base version of that file. A
    // file that the change added has no base version, so discovery decides.
    let run = runs::own_config_run(head, base_root, opts.no_cache)?;
    let base_config_path = run.config_file().value().clone();
    let own_opts = base_run.options(opts, base_root, &base_config_path, base_cache_dir);
    // A syntactic pass reports at least the unused code of a type-aware pass,
    // so it can only make fewer head findings look introduced.
    let syntactic = AuditTypeAwareOptions {
        enabled: Some(false),
        ..AuditTypeAwareOptions::default()
    };
    let pass = CheckPass {
        run: &run,
        changed_since: None,
        changed_files: Some(focus),
        retain_modules_for_health: false,
        snapshot: fallow_config::AnalysisSnapshot::Base,
    };
    let check = run_audit_check(&own_opts, syntactic, pass).ok()??;
    let rules = &check.config.rules;
    if rules.unused_files == fallow_config::Severity::Off
        || rules.unused_exports == fallow_config::Severity::Off
    {
        return None;
    }
    Some(fallow_api::audit_run::own_config_unused_code_keys(
        &check.results,
        &check.config,
        &base_check.results,
        &check.config.root,
    ))
}
