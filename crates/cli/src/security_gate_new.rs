//! The diff of `fallow security --gate new` (issue #886).
//!
//! The gate keeps only the candidates on the changed lines and drives a
//! dedicated exit code.

use std::process::ExitCode;

use fallow_types::results::AnalysisResults;

use super::{SecurityGateMode, SecurityOptions};
use crate::error::emit_input_error;
use crate::report::ci::diff_filter::{DiffIndex, shared_diff_index};

const NEEDS_DIFF_SOURCE: &str = "fallow security --gate requires a diff source: \
     --changed-since <ref>, --diff-file <path>, or --diff-stdin.";

/// The diff that `--gate new` keeps findings against.
pub(super) enum GateNewDiff {
    /// The process-wide diff of `--diff-file` or `--diff-stdin`.
    Shared(&'static DiffIndex),
    /// The diff of `--changed-since`, computed for this run. Boxed, because
    /// the index is much larger than the reference of `Shared`.
    Owned(Box<DiffIndex>),
}

impl GateNewDiff {
    fn index(&self) -> &DiffIndex {
        match self {
            Self::Shared(index) => index,
            Self::Owned(index) => index,
        }
    }
}

/// Get the diff of `--gate new` before the analysis and the scope filter run.
///
/// The gate requires a diff source. A diff it cannot compute is an input error
/// (exit 2), never a green gate, because a silent miss defeats a security gate.
/// The diff resolves first, so a ref that git cannot resolve stops the run
/// before `--changed-since` falls back to the whole project. That fallback
/// warns that the report covers the whole project, and a gate run produces no
/// report (issue #4431).
pub(super) fn resolve(opts: &SecurityOptions<'_>) -> Result<Option<GateNewDiff>, ExitCode> {
    if !matches!(opts.gate, Some(SecurityGateMode::New)) {
        return Ok(None);
    }
    if let Some(shared) = shared_diff_index() {
        return Ok(Some(GateNewDiff::Shared(shared)));
    }
    let Some(git_ref) = opts.changed_since else {
        return Err(emit_input_error(NEEDS_DIFF_SOURCE, opts.output));
    };
    match fallow_engine::changed_files::try_get_changed_diff(opts.root, git_ref) {
        Ok(text) => Ok(Some(GateNewDiff::Owned(Box::new(
            DiffIndex::from_unified_diff(&text),
        )))),
        Err(err) => Err(emit_input_error(
            &format!(
                "fallow security --gate could not compute the diff for '{git_ref}': {}",
                err.describe()
            ),
            opts.output,
        )),
    }
}

/// Keep only the candidates that are new on the changed lines of `diff`.
///
/// `resolve` returns a diff for each `--gate new` run. The `None` arm is a
/// fail-closed guard: a gate without a diff exits 2 and never passes.
pub(super) fn retain(
    opts: &SecurityOptions<'_>,
    results: &mut AnalysisResults,
    diff: Option<GateNewDiff>,
) -> Result<(), ExitCode> {
    let Some(diff) = diff else {
        return Err(emit_input_error(NEEDS_DIFF_SOURCE, opts.output));
    };
    crate::check::filtering::retain_gate_new(results, diff.index(), opts.root);
    Ok(())
}
