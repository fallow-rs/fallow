//! The saved report of the last mutation run, which `fallow mutation
//! inspect` reads when no `--report` is given.
//!
//! Each run saves its full JSON report to
//! `<cache dir>/mutation/last-report.json`. The cache directory has a
//! `.gitignore` with `*`, so the saved report never makes the worktree dirty.
//! `fallow mutation inspect` reads this file when the caller names no report.

use std::path::{Path, PathBuf};

use crate::analysis_context::resolve_programmatic_analysis_context;
use crate::{AnalysisOptions, ProgrammaticError};

/// The file name of the saved report below `<cache dir>/mutation`.
const LAST_REPORT_FILE: &str = "last-report.json";

/// Writes `*` to `<cache>/.gitignore` when the cache has no ignore file, so
/// that the run directory and the saved report never show in `git status`.
pub fn ignore_cache_dir(cache_dir: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(cache_dir)?;
    let ignore = cache_dir.join(".gitignore");
    if !ignore.exists() {
        std::fs::write(ignore, "*\n")?;
    }
    Ok(())
}

fn cache_error(message: String) -> ProgrammaticError {
    ProgrammaticError::new(message, 2).with_code("FALLOW_MUTATION_CACHE_WRITE_FAILED")
}

/// The cache directory of the project config: the directory that a run uses.
fn cache_dir(analysis: &AnalysisOptions) -> Result<PathBuf, ProgrammaticError> {
    let resolved = resolve_programmatic_analysis_context(analysis)?;
    let project = fallow_engine::project_config::config_for_project_with_load_options(
        resolved.root(),
        resolved.config_path().as_deref(),
        fallow_config::ConfigLoadOptions {
            allow_remote_extends: resolved.allow_remote_extends(),
        },
    )
    .map_err(|error| {
        ProgrammaticError::new(format!("failed to load config: {error}"), 2)
            .with_code("FALLOW_CONFIG_LOAD_FAILED")
            .with_context("analysis.configPath")
    })?;
    Ok(project.config.cache_dir)
}

/// The path of the saved report of the last run of the project.
///
/// # Errors
///
/// Returns an error when the root or the config does not resolve.
pub fn last_mutation_report_path(analysis: &AnalysisOptions) -> Result<PathBuf, ProgrammaticError> {
    Ok(cache_dir(analysis)?.join("mutation").join(LAST_REPORT_FILE))
}

/// Removes the outcome journal of the project when `report` is complete,
/// and returns true when it removed a journal. Call it after the report is
/// saved: the saved report then holds each result. A partial or failed run
/// keeps the journal, so `--resume` can continue it.
///
/// # Errors
///
/// Returns an error when the config does not resolve or the journal cannot
/// be removed.
pub fn remove_finished_mutation_journal(
    analysis: &AnalysisOptions,
    report: &fallow_output::MutationReport,
) -> Result<bool, ProgrammaticError> {
    if report.completion.status != fallow_output::MutationCompletionStatus::Complete {
        return Ok(false);
    }
    let path = super::journal::Journal::path(&cache_dir(analysis)?.join("mutation"));
    match std::fs::remove_file(&path) {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(cache_error(format!(
            "failed to remove {}: {error}",
            path.display()
        ))),
    }
}

/// Saves the JSON `report` of a run as the last report of the project, and
/// returns its path. The write goes to a temporary file first, so a reader
/// never sees half a report.
///
/// # Errors
///
/// Returns an error when the config does not resolve or the cache directory
/// cannot be written.
pub fn save_last_mutation_report(
    analysis: &AnalysisOptions,
    report: &serde_json::Value,
) -> Result<PathBuf, ProgrammaticError> {
    let cache_dir = cache_dir(analysis)?;
    ignore_cache_dir(&cache_dir).map_err(|error| cache_error(error.to_string()))?;
    let dir = cache_dir.join("mutation");
    std::fs::create_dir_all(&dir).map_err(|error| cache_error(error.to_string()))?;
    let bytes = serde_json::to_vec(report)
        .map_err(|error| cache_error(format!("failed to serialize the report: {error}")))?;
    let path = dir.join(LAST_REPORT_FILE);
    let temporary = dir.join(format!("{LAST_REPORT_FILE}.{}.tmp", std::process::id()));
    std::fs::write(&temporary, bytes)
        .and_then(|()| std::fs::rename(&temporary, &path))
        .map_err(|error| {
            let _ = std::fs::remove_file(&temporary);
            cache_error(format!("failed to save {}: {error}", path.display()))
        })?;
    Ok(path)
}
