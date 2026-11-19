//! The runner options of a mutation run that the runner reads: the selected
//! runner config file and the selected Vitest projects.

use std::path::{Path, PathBuf};

use fallow_engine::session::AnalysisSession;

use super::mutation::{ProgrammaticResult, mutation_error};
use crate::mutation::MutationRunOptions;
use crate::mutation::runner_select::Runner;
use crate::mutation::worktree::canonical_path;

/// The selected runner config: the caller option, else the
/// `mutation.runnerConfig` config key. The result is relative to the project
/// root, with `/` separators, as the companion resolves it against the root.
///
/// # Errors
///
/// Returns an exit-2 error when the file does not exist, is not a file, is
/// outside the project root, or when the runner reads no config file.
pub(super) fn runner_config(
    options: &MutationRunOptions,
    session: &AnalysisSession,
    root: &Path,
    runner: Runner,
) -> ProgrammaticResult<Option<String>> {
    let Some(raw) = options.runner_config.clone().or_else(|| {
        session
            .config()
            .mutation
            .runner_config
            .as_ref()
            .map(PathBuf::from)
    }) else {
        return Ok(None);
    };
    let invalid = |message: String| {
        mutation_error("FALLOW_INVALID_MUTATION_OPTIONS", message)
            .with_context("mutation.runnerConfig")
    };
    let shown = raw.display();
    if runner != Runner::Vitest {
        return Err(invalid(format!(
            "the runner config `{shown}` needs the Vitest runner, but the run uses `{}`; only Vitest reads a config file",
            runner.name()
        )));
    }
    let candidate = if raw.is_absolute() {
        raw.clone()
    } else {
        root.join(&raw)
    };
    if !candidate.is_file() {
        return Err(invalid(format!(
            "the runner config `{shown}` is not a file; a relative path is relative to the project root"
        )));
    }
    let root = canonical_path(root);
    let file = canonical_path(&candidate);
    let Ok(relative) = file.strip_prefix(&root) else {
        return Err(invalid(format!(
            "the runner config `{shown}` is outside the project root `{}`",
            root.display()
        )));
    };
    Ok(Some(
        relative
            .components()
            .map(|part| part.as_os_str().to_string_lossy())
            .collect::<Vec<_>>()
            .join("/"),
    ))
}

/// The selected Vitest projects: the caller option, else the
/// `mutation.runnerProjects` config key, sorted and without duplicates, so
/// that the order of the flags does not make a resume mismatch. Empty uses
/// each project.
///
/// # Errors
///
/// Returns an exit-2 error for an empty name, or when the runner is not
/// Vitest.
pub(super) fn runner_projects(
    options: &MutationRunOptions,
    session: &AnalysisSession,
    runner: Runner,
) -> ProgrammaticResult<Vec<String>> {
    let mut names = if options.runner_projects.is_empty() {
        session
            .config()
            .mutation
            .runner_projects
            .clone()
            .unwrap_or_default()
    } else {
        options.runner_projects.clone()
    };
    for name in &mut names {
        *name = name.trim().to_owned();
    }
    names.sort();
    names.dedup();
    if names.is_empty() {
        return Ok(names);
    }
    let invalid = |message: String| {
        mutation_error("FALLOW_INVALID_MUTATION_OPTIONS", message)
            .with_context("mutation.runnerProjects")
    };
    if names.iter().any(String::is_empty) {
        return Err(invalid("a runner project name is empty".to_owned()));
    }
    if runner != Runner::Vitest {
        return Err(invalid(format!(
            "the runner projects `{}` need the Vitest runner, but the run uses `{}`; only Vitest has projects",
            names.join(", "),
            runner.name()
        )));
    }
    Ok(names)
}
