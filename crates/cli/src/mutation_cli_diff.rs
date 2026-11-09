//! Preserve the startup diff source when mutation crosses into the API.

use std::io::Write;

use crate::report::ci::diff_filter::{self, DiffSource};
use fallow_api::{MutationRunOptions, MutationRunScope, ProgrammaticError};

pub(super) fn prepare(
    options: &mut MutationRunOptions,
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
    let mut patch = tempfile::NamedTempFile::new().map_err(stdin_error)?;
    patch.write_all(raw.as_bytes()).map_err(stdin_error)?;
    patch.flush().map_err(stdin_error)?;
    options.analysis.diff_file = Some(patch.path().to_path_buf());
    Ok(Some(patch))
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
