//! Preserve the startup diff source when mutation crosses into the API.

use std::io::Write;
use std::path::Path;

use crate::report::ci::diff_filter::{self, DiffSource};
use fallow_api::{MutationRunOptions, MutationRunScope, ProgrammaticError};

pub(super) fn prepare(
    options: &mut MutationRunOptions,
    root: &Path,
) -> Result<Option<tempfile::NamedTempFile>, ProgrammaticError> {
    if !matches!(options.scope, MutationRunScope::Changed { .. }) {
        return Ok(None);
    }
    let source = diff_filter::shared_diff_source();
    if !matches!(source, Some(DiffSource::Stdin)) {
        return Ok(None);
    }
    if let Some(request) = diff_filter::shared_diff_request_outcome()
        && request.status == fallow_output::RequestStatus::NotApplied
    {
        return Err(ProgrammaticError::new(
            format!(
                "cannot use {} for mutation scope ({}); regenerate the patch from the project or repository root",
                request.requested,
                request.reason.as_deref().unwrap_or("not-applied"),
            ),
            2,
        )
        .with_code("FALLOW_MUTATION_DIFF_NOT_APPLIED")
        .with_context("analysis.diffFile"));
    }
    let raw = diff_filter::shared_diff_raw().ok_or_else(|| stdin_error("no cached patch"))?;
    let mut patch = external_patch(root)?;
    patch.write_all(raw.as_bytes()).map_err(stdin_error)?;
    patch.flush().map_err(stdin_error)?;
    options.analysis.diff_file = Some(patch.path().to_path_buf());
    Ok(Some(patch))
}

fn external_patch(root: &Path) -> Result<tempfile::NamedTempFile, ProgrammaticError> {
    use fallow_engine::changed_files::{ChangedFilesError, resolve_git_toplevel};

    let boundary = match resolve_git_toplevel(root) {
        Ok(toplevel) => toplevel,
        Err(ChangedFilesError::NotARepository) => root.to_path_buf(),
        Err(error) => return Err(stdin_error(error.describe())),
    };
    let boundary = dunce::canonicalize(boundary).map_err(stdin_error)?;
    let configured = dunce::canonicalize(std::env::temp_dir()).map_err(stdin_error)?;
    // A bridge inside the worktree would itself trip the dirty-worktree guard.
    // Canonical paths also catch a temporary directory reached through a symlink.
    let directory = if configured.starts_with(&boundary) {
        boundary
            .parent()
            .ok_or_else(|| stdin_error("no temporary directory outside the project is available"))?
    } else {
        configured.as_path()
    };
    tempfile::NamedTempFile::new_in(directory).map_err(|error| {
        stdin_error(format!(
            "cannot create a temporary patch outside the worktree: {}",
            error.kind(),
        ))
        .with_help("set the temporary directory outside the worktree, or supply --diff-file")
    })
}

fn stdin_error(error: impl std::fmt::Display) -> ProgrammaticError {
    ProgrammaticError::new(
        format!("cannot prepare --diff-stdin for mutation: {error}"),
        2,
    )
    .with_code("FALLOW_INVALID_DIFF_FILE")
    .with_context("analysis.diffFile")
}

/// Keep the caller's stdin label in errors from the temporary API input.
pub(super) fn contextualize(
    mut error: ProgrammaticError,
    patch: Option<&tempfile::NamedTempFile>,
) -> ProgrammaticError {
    if let Some(patch) = patch {
        error.message = error
            .message
            .replace(patch.path().to_string_lossy().as_ref(), "--diff-stdin");
    }
    error
}
