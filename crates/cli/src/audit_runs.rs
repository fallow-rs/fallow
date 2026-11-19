//! The resolved runs of the head side and the base side of one audit.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use fallow_config::{
    CommandKind, EnvSnapshot, PlannedAnalysis, RunInputs, SelectionInput, Surface,
};
use fallow_engine::resolved_run::{
    BaseConfig, BaseSide, ResolveError, ResolvedRun, Runnable,
    resolve_base_run_with_head_decisions, resolve_run,
};
use fallow_types::sourced::{Given, Origin};
use rustc_hash::FxHashSet;

use super::AuditOptions;
use crate::error::emit_input_error;

/// A value that the flag `name` gives.
const fn flag<T>(name: &'static str) -> impl Fn(T) -> Given<T> {
    move |value| Given {
        value,
        origin: Origin::Flag(name),
    }
}

/// The run inputs of `opts`, for an audit without a run of the command
/// line: a unit test or a benchmark hook.
fn options_inputs(opts: &AuditOptions<'_>) -> RunInputs {
    let mut inputs = RunInputs::new(
        Surface::Cli,
        EnvSnapshot::capture(),
        CommandKind::Audit,
        SelectionInput::Options {
            dead_code: Some(opts.analyses.dead_code),
            duplication: Some(opts.analyses.dupes),
            health: Some(opts.analyses.health),
            origin: Origin::Flag("--only"),
        },
    );
    inputs.root = Some(flag("--root")(opts.root.to_path_buf()));
    inputs.config_path = opts.config_path.clone().map(flag("--config"));
    inputs.threads = Some(flag("--threads")(opts.threads.max(1)));
    inputs.no_cache = opts.no_cache.then(|| flag("--no-cache")(true));
    inputs.allow_remote_extends = opts
        .allow_remote_extends
        .then(|| flag("--allow-remote-extends")(true));
    inputs.include_entry_exports = opts
        .include_entry_exports
        .then(|| flag("--include-entry-exports")(true));
    inputs.production = opts.production.then(|| flag("--production")(true));
    inputs.production_dead_code = opts
        .production_dead_code
        .map(flag("--production-dead-code"));
    inputs.production_health = opts.production_health.map(flag("--production-health"));
    inputs.production_dupes = opts.production_dupes.map(flag("--production-dupes"));
    inputs.output = opts.output;
    inputs.quiet = opts.quiet;
    inputs
}

/// The head run of an audit without a run of the command line.
///
/// # Errors
///
/// Prints a resolve error on the error channel of the output format and
/// gives exit code 2.
pub(super) fn options_run(opts: &AuditOptions<'_>) -> Result<ResolvedRun, ExitCode> {
    resolve_run(&options_inputs(opts)).map_err(|err| {
        emit_input_error(
            &crate::cli_production::resolve_error_message(&err, opts.config_path.as_deref()),
            opts.output,
        )
    })
}

/// The run of an audit side in a temporary worktree: the inputs of `head` on
/// `root`, with the config file of the head run. A head run without a config
/// file discovers the config from `root`. The run keeps the plan and the
/// production modes of the head run, so the config of the worktree cannot
/// turn an analysis off or change a production mode. The run keeps its caches
/// in the cache directory of the worktree that the audit lends the parse
/// cache to, so a shared cache directory never gets the caches of a temporary
/// root. `origin` names the flag that made the worktree: `--base` or
/// `--staged`.
///
/// # Errors
///
/// Prints a resolve error on the error channel of `opts.output` and gives
/// exit code 2.
pub(super) fn worktree_run(
    head: &ResolvedRun,
    root: &Path,
    opts: &AuditOptions<'_>,
    origin: &'static str,
) -> Result<ResolvedRun, ExitCode> {
    worktree_side_run(head, root, opts.no_cache, origin).map_err(|err| {
        emit_input_error(
            &crate::cli_production::resolve_error_message(&err, None),
            opts.output,
        )
    })
}

fn worktree_side_run(
    head: &ResolvedRun,
    root: &Path,
    no_cache: bool,
    origin: &'static str,
) -> Result<ResolvedRun, ResolveError> {
    resolve_base_run_with_head_decisions(
        head,
        worktree_side(root, origin, BaseConfig::Head, no_cache),
    )
}

/// The base side of the own-config pass: the base copy of the config file of
/// `head`, with the plan and the production modes of the head run, as the
/// worktree side has them. `None` when that config does not resolve. The base
/// pass with the head config is then the only evidence.
pub(super) fn own_config_run(
    head: &ResolvedRun,
    base_root: &Path,
    no_cache: bool,
) -> Option<ResolvedRun> {
    let side = worktree_side(base_root, "--base", BaseConfig::BaseCopy, no_cache);
    resolve_base_run_with_head_decisions(head, side).ok()
}

/// The side of a worktree at `root`. Without `no_cache`, the side keeps its
/// caches in the cache directory of the worktree.
fn worktree_side(
    root: &Path,
    origin: &'static str,
    config: BaseConfig,
    no_cache: bool,
) -> BaseSide {
    let cache_dir = (!no_cache).then(|| flag(origin)(super::worktree_cache::cache_dir(root)));
    base_side(root, config, cache_dir)
}

fn base_side(root: &Path, config: BaseConfig, cache_dir: Option<Given<PathBuf>>) -> BaseSide {
    BaseSide {
        root: flag("--base")(root.to_path_buf()),
        config,
        cache_dir,
    }
}

/// The sub-analysis of `run` when `selected` and the run executes it.
pub(super) fn sub_analysis(
    run: &ResolvedRun,
    analysis: PlannedAnalysis,
    selected: bool,
) -> Option<Runnable<'_>> {
    selected
        .then(|| crate::cli_production::runnable(run, analysis))
        .flatten()
}

/// The canonical spelling of `root` and of the changed files under it. A
/// resolved run canonicalizes its root, so its findings name files under the
/// canonical root.
pub(super) fn canonical_changed_files(
    root: &Path,
    files: &[PathBuf],
) -> (PathBuf, FxHashSet<PathBuf>) {
    let canonical = dunce::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let files = files
        .iter()
        .map(|file| {
            file.strip_prefix(root)
                .map_or_else(|_| file.clone(), |relative| canonical.join(relative))
        })
        .collect();
    (canonical, files)
}

#[cfg(test)]
#[path = "audit_runs_tests.rs"]
mod tests;
