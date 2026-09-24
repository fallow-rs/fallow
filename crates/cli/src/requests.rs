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
use std::sync::{Mutex, OnceLock};

use fallow_output::{RequestName, RequestOutcome, RequestOutcomes, RequestStatus};
use rustc_hash::FxHashSet;

/// What became of this run's `--changed-since` request.
///
/// Set by the first command that resolves the ref. Later commands in a
/// combined run resolve the same ref against the same root and observe the
/// original value, which is the same value they would have computed.
static CHANGED_SINCE_OUTCOME: OnceLock<RequestOutcome> = OnceLock::new();

/// The changed files of an applied `--changed-since` request, normalized the
/// way the result filter normalizes them, so the scope count and the filter
/// agree on which file is "changed".
static CHANGED_SINCE_FILES: OnceLock<FxHashSet<PathBuf>> = OnceLock::new();

/// The changed files the run analyzed, over every analysis that measured: the
/// `scope_size` of an applied `changed-since` entry is its length.
///
/// A union, not the first measurement. The analyses of one combined run can
/// discover different files: a per-analysis `production` setting drops test
/// files from dead code but keeps them for health and duplication. The unit is
/// "changed files that the run analyzed", so a file any analysis kept counts,
/// and the value does not depend on which section measures first. `None`
/// means no analysis measured.
static CHANGED_SINCE_ANALYZED: Mutex<Option<FxHashSet<PathBuf>>> = Mutex::new(None);

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
            let _ = CHANGED_SINCE_FILES.set(
                files
                    .iter()
                    .map(|path| dunce::simplified(path).to_path_buf())
                    .collect(),
            );
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

/// Add the changed files that stay in this analysis's set to the run's
/// analyzed changed files, whose count is the `scope_size` of the applied
/// `changed-since` entry.
///
/// Call it with the files discovery kept, after the ref was resolved. A changed
/// file that discovery dropped (ignored, outside the project, not a source
/// file) does not count: a commit that touches only a README narrows the run
/// to nothing, and `0` is how the envelope says so (issue #2800). Does nothing
/// when no ref was resolved, so a command can call it without a guard.
pub fn measure_changed_since_scope(analyzed: &[fallow_types::discover::DiscoveredFile]) {
    let Some(changed) = CHANGED_SINCE_FILES.get() else {
        return;
    };
    let Ok(mut union) = CHANGED_SINCE_ANALYZED.lock() else {
        return;
    };
    let union = union.get_or_insert_with(FxHashSet::default);
    union.extend(
        analyzed
            .iter()
            .map(|file| dunce::simplified(&file.path))
            .filter(|path| changed.contains(*path))
            .map(Path::to_path_buf),
    );
}

/// The recorded `changed-since` entry, with the measured scope when the request
/// applied and a command measured it.
fn changed_since_outcome() -> Option<RequestOutcome> {
    let outcome = CHANGED_SINCE_OUTCOME.get()?.clone();
    let size = CHANGED_SINCE_ANALYZED
        .lock()
        .ok()
        .and_then(|union| union.as_ref().map(|files| files.len() as u64));
    Some(match size {
        Some(size) if outcome.status == RequestStatus::Applied => RequestOutcome {
            scope_size: Some(size),
            ..outcome
        },
        _ => outcome,
    })
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
    requests.insert_if(RequestName::ChangedSince, changed_since_outcome());
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
    requests.insert_if(RequestName::ChangedSince, changed_since_outcome());
    requests.insert_if(
        RequestName::DiffFilter,
        crate::report::ci::diff_filter::shared_diff_request_outcome().cloned(),
    );
    requests.insert_if(RequestName::SarifFile, SARIF_FILE_OUTCOME.get().cloned());
    requests.into_option()
}
