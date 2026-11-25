//! The options of the base pass of `fallow security --gate newly-reachable`.

use std::path::Path;

use super::SecurityOptions;

/// Build the quiet, non-gating `SecurityOptions` used to re-analyze the base
/// worktree for the `--gate newly-reachable` snapshot.
pub(super) fn base_snapshot_security_options<'a>(
    opts: &SecurityOptions<'a>,
    base_root: &'a Path,
    run: &'a fallow_engine::resolved_run::ResolvedRun,
) -> SecurityOptions<'a> {
    SecurityOptions {
        root: base_root,
        run,
        output: opts.output,
        json_style: opts.json_style,
        no_cache: opts.no_cache,
        threads: opts.threads,
        quiet: true,
        allow_remote_extends: opts.allow_remote_extends,
        fail_on_issues: false,
        sarif_file: None,
        summary: false,
        changed_since: None,
        use_shared_diff_index: false,
        workspace: opts.workspace,
        changed_workspaces: None,
        file: &[],
        surface: false,
        // Deliberately unscoped: the base pass runs in another worktree whose
        // path spelling differs, so a head-root scope would empty the base
        // snapshot. The head pass is already scope-narrowed; a full base
        // snapshot joins correctly against it.
        scope: None,
        gate: None,
        runtime_coverage: None,
        min_invocations_hot: opts.min_invocations_hot,
        explain: false,
    }
}
