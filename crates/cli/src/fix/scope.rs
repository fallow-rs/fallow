//! The scope of a fix run.
//!
//! `fix` plans only the findings that `dead-code` reports with the same scope
//! flags, so both commands narrow the results with
//! [`fallow_engine::dead_code::apply_scope`].

use std::path::PathBuf;
use std::process::ExitCode;

use fallow_config::{ResolvedConfig, WorkspaceInfo};
use fallow_engine::change_scope::{ChangeScope, ChangeScopeOwner, ChangeScopeRequest};
use fallow_engine::dead_code::DeadCodeScope;
use fallow_engine::workspace_scope::WorkspaceScope;
use fallow_types::results::AnalysisResults;

use super::FixOptions;

/// The scope of a fix run. The diff of `--diff-file`, `--diff-stdin` or
/// `FALLOW_DIFF_FILE` comes from the shared diff that the CLI start reads.
#[derive(Debug, Clone, Default)]
pub struct FixScope<'a> {
    /// Positional `[PATH]`: root-joined absolute file or directory inside
    /// the root. `None` means whole-project scope.
    pub path: Option<PathBuf>,
    /// `--workspace`.
    pub workspace: Option<&'a [String]>,
    /// `--changed-workspaces`.
    pub changed_workspaces: Option<&'a str>,
    /// `--changed-since`.
    pub changed_since: Option<&'a str>,
    /// `--no-package-baselines`.
    pub no_package_baselines: bool,
}

impl<'a> FixScope<'a> {
    /// The scope of the command line, with the resolved positional path.
    pub(crate) fn of_cli(cli: &'a crate::cli_args::Cli, path: Option<PathBuf>) -> Self {
        Self {
            path,
            workspace: cli.workspace.as_deref(),
            changed_workspaces: cli.changed_workspaces.as_deref(),
            changed_since: cli.changed_since.as_deref(),
            no_package_baselines: cli.no_package_baselines,
        }
    }
}

/// Keep only the findings that the report shows. An unknown workspace or a
/// `--changed-workspaces` ref that does not resolve is an input error.
pub(super) fn narrow_to_report(
    opts: &FixOptions<'_>,
    config: &ResolvedConfig,
    workspaces: &[WorkspaceInfo],
    results: &mut AnalysisResults,
) -> Result<(), ExitCode> {
    // The rule pass removes each finding whose rule an `overrides` entry turns
    // `off` for its file, and hides the findings of unused files.
    fallow_engine::dead_code::apply_rule_severities(results, config);
    let flags = &opts.scope;
    let roots = crate::check::filtering::resolve_workspace_scope(
        opts.root,
        flags.workspace,
        flags.changed_workspaces,
        &config.workspace_patterns,
        opts.output,
    )?;
    let roots = WorkspaceScope::with_root(roots, flags.path.as_ref());
    let changed_files = flags
        .changed_since
        .and_then(|git_ref| crate::requests::resolve_changed_since(opts.root, git_ref));
    let change_scope = ChangeScope::resolve(
        ChangeScopeRequest {
            owner: ChangeScopeOwner::Run,
            global_ref: flags.changed_since.is_some(),
            files: changed_files.as_ref(),
            cache: Some(crate::requests::package_baseline_cache()),
            no_package_baselines: flags.no_package_baselines,
        },
        config,
        workspaces,
    )
    .map_err(|err| {
        crate::error::emit_input_error(&format!("Workspace baseline error: {err}"), opts.output)
    })?;
    crate::requests::warn_if_package_baselines_stood_down(&change_scope);
    let diff = crate::report::ci::diff_filter::shared_diff_index();
    let scope = DeadCodeScope {
        workspace_roots: roots.as_ref(),
        changes: Some(&change_scope),
        diff: diff.map(|index| (index, opts.root)),
        files: None,
    };
    fallow_engine::dead_code::apply_scope(results, &scope, config);
    Ok(())
}
