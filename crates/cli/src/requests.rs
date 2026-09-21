//! What became of the narrowing requests one run received.
//!
//! The sibling of [`crate::gates`]: that module projects what the run
//! concluded, this one projects whether the run did what it was asked. Both
//! assemble a keyed root object from values the run already computed, and
//! neither decides anything of its own.
//!
//! # Why a process-wide record rather than a threaded value
//!
//! `--changed-since`, the diff source and `--sarif-file` are global CLI inputs
//! resolved once per process against one root. A combined run resolves
//! `--changed-since` separately for dead-code, duplication and health, and the
//! three answers are the same answer, so recording the first is recording all
//! of them. The diff source is already cached this way, for a stronger reason:
//! stdin can be drained exactly once. And `--sarif-file` is written before the
//! envelope that reports it is assembled, so the record is what carries the
//! fate forward.
//!
//! # Why an honoured request is recorded too
//!
//! Without the `applied` entry a consumer cannot tell "the report is scoped to
//! the change" from "nothing was asked for", and that distinction is the
//! reviewer question behind issues #2687 and #2688. `gate_outcomes` publishes
//! gates that passed for the same reason.

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use fallow_output::{RequestName, RequestOutcome, RequestOutcomes};
use rustc_hash::FxHashSet;

/// What became of this run's `--changed-since` request.
///
/// Set by the first command that resolves the ref. Later commands in a
/// combined run resolve the same ref against the same root and observe the
/// original value, which is the same value they would have computed.
static CHANGED_SINCE_OUTCOME: OnceLock<RequestOutcome> = OnceLock::new();

/// Resolve `--changed-since` to a file set, warn when git cannot, and record
/// what became of the request either way.
///
/// `None` means the analysis runs at FULL scope: the report that follows is
/// valid, complete, and wider than what was asked for. That is the whole
/// defect behind issue #2687, and the recorded outcome is what carries the
/// fact past a `--quiet --format json` invocation, which is how both shipped
/// CI integrations run fallow.
pub fn resolve_changed_since(root: &Path, git_ref: &str) -> Option<FxHashSet<PathBuf>> {
    match fallow_engine::changed_files::changed_files(root, git_ref) {
        Ok(files) => {
            record_changed_since(RequestOutcome::applied(RequestName::ChangedSince, git_ref));
            Some(files)
        }
        Err(err) => {
            let message = err.changed_since_message(git_ref);
            eprintln!("Warning: {message}");
            record_changed_since(RequestOutcome::not_applied(
                RequestName::ChangedSince,
                git_ref,
                err.reason(),
                message,
            ));
            None
        }
    }
}

fn record_changed_since(outcome: RequestOutcome) {
    let _ = CHANGED_SINCE_OUTCOME.set(outcome);
}

/// What became of this run's `--sarif-file` request.
///
/// One global path per process, written once by the single site that produces
/// the file.
static SARIF_FILE_OUTCOME: OnceLock<RequestOutcome> = OnceLock::new();

/// Record a `--sarif-file` document that was written.
pub fn record_sarif_file_applied(path: &Path) {
    let _ = SARIF_FILE_OUTCOME.set(RequestOutcome::applied(
        RequestName::SarifFile,
        path.display().to_string(),
    ));
}

/// Record a `--sarif-file` document that was not written, with the reason token
/// and the sentence the CLI also printed.
///
/// The exit code does not move: the primary report is complete and the run
/// still exits on its findings, so this is the only channel that says the
/// secondary artefact is missing (issue #2690).
pub fn record_sarif_file_failure(path: &Path, reason: &str, message: String) {
    let _ = SARIF_FILE_OUTCOME.set(RequestOutcome::not_applied(
        RequestName::SarifFile,
        path.display().to_string(),
        reason,
        message,
    ));
}

/// This run's `request_outcomes` limited to the `changed-since` channel, or
/// `None` when no ref was resolved.
///
/// For the commands that resolve a ref and apply no diff filter of their own.
/// `init_cli_diff_filter` runs for EVERY command, so `--diff-file` populates the
/// diff record before dispatch; a command that never consults that index would
/// publish `diff-filter: applied` from [`request_outcomes`] and claim a
/// narrowing it did not perform. Named after what it publishes rather than
/// after the command that needs it, so a third caller reads the guarantee off
/// the name (issue #2734).
#[must_use]
pub fn changed_since_request_outcomes() -> Option<RequestOutcomes> {
    let mut requests = RequestOutcomes::new();
    requests.insert_if(
        RequestName::ChangedSince,
        CHANGED_SINCE_OUTCOME.get().cloned(),
    );
    requests.into_option()
}

/// This run's `request_outcomes` object, or `None` when it was asked for
/// nothing.
///
/// Reads each channel where it is produced rather than taking them as
/// parameters, so a command that grows another request cannot publish a
/// half-filled object by forgetting to thread one through.
#[must_use]
pub fn request_outcomes() -> Option<RequestOutcomes> {
    let mut requests = RequestOutcomes::new();
    requests.insert_if(
        RequestName::ChangedSince,
        CHANGED_SINCE_OUTCOME.get().cloned(),
    );
    requests.insert_if(
        RequestName::DiffFilter,
        crate::report::ci::diff_filter::shared_diff_request_outcome().cloned(),
    );
    requests.insert_if(RequestName::SarifFile, SARIF_FILE_OUTCOME.get().cloned());
    requests.into_option()
}
