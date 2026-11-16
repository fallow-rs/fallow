//! Reasons why a saved baseline does not cover the scope of the current run.

use super::*;

/// Which channels narrowed the run before the baseline comparison, so the
/// current results cover less than a whole-project baseline ever described. A
/// save records this set, and [`baseline_notes::compare_with_saved_baseline`]
/// turns it into the set that stops this run from judging a loaded file.
///
/// Mirrors the health side's `baseline_scope_reasons` and adds the
/// dead-code-only filter channel, because `--unused-*` flags drop whole
/// baseline categories. The diff channel is resolved exactly as
/// `apply_scope_filters` resolves it: on these commands `--diff-file` and
/// `--diff-stdin` never reach `opts.diff_index` and arrive through the shared
/// index instead, so reading the field alone would miss every diff-scoped run.
/// Production mode counts as narrowing too: it drops test, story and dev files
/// before analysis, and the resolved config carries the effective flag whether
/// it came from the CLI or from the project config. `includeEntryExports`
/// counts for the same reason: it changes which exports the run judges, from
/// the flag or from the config.
///
/// This command can name every channel, because it reads the flags rather than
/// a set already resolved from them.
pub(super) fn baseline_scope_reasons(
    opts: &CheckOptions<'_>,
    config: &ResolvedConfig,
    change_scope: &fallow_engine::change_scope::ChangeScope,
) -> fallow_output::BaselineScopeReasons {
    use fallow_output::ScopeReason;

    let diff_scoped = opts.diff_index.is_some()
        || (opts.use_shared_diff_index
            && crate::report::ci::diff_filter::shared_diff_index().is_some());
    fallow_output::BaselineScopeReasons::empty()
        .insert_if(diff_scoped, ScopeReason::Diff)
        .insert_if(
            change_scope.scope_reason() == Some(ScopeReason::ChangedSince),
            ScopeReason::ChangedSince,
        )
        .insert_if(
            change_scope.scope_reason() == Some(ScopeReason::PackageBaselines),
            ScopeReason::PackageBaselines,
        )
        .insert_if(opts.workspace.is_some(), ScopeReason::Workspace)
        .insert_if(
            opts.changed_workspaces.is_some(),
            ScopeReason::ChangedWorkspaces,
        )
        .insert_if(opts.scope.is_some(), ScopeReason::Scope)
        .insert_if(!opts.file.is_empty(), ScopeReason::File)
        .insert_if(opts.filters.any_active(), ScopeReason::IssueTypeFilter)
        .insert_if(config.production, ScopeReason::Production)
        .insert_if(
            config.include_entry_exports,
            ScopeReason::IncludeEntryExports,
        )
}
