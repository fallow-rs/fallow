//! The message text of the health-stage kinds: scoring, churn, ownership,
//! trend and coverage input.

use super::WorkspaceDiagnosticKind;

/// The message of one health-stage kind. `display` is the project-relative
/// path of the diagnostic. Any other kind renders as an empty string.
pub(super) fn health_stage_message(display: &str, kind: &WorkspaceDiagnosticKind) -> String {
    match kind {
        WorkspaceDiagnosticKind::FileScoresUnavailable { error } => format!(
            "Could not compute per-file health scores ({error}), so the score list is empty and \
             the scored-file count is 0 because nothing was measured rather than because the \
             project has nothing to score. Rerun with --no-cache, or scope the run to a \
             subdirectory to find the input that fails."
        ),
        WorkspaceDiagnosticKind::HotspotsSkipped { cause } => match cause.as_str() {
            "invalid-since" => "Hotspot analysis was skipped because --since could not be read \
                 as a time window, so the hotspots, churn and ownership sections report nothing \
                 rather than zero. Spell it as a duration such as 6m or 90d, or drop it to use \
                 the default window."
                .to_owned(),
            "no-commits" => "Hotspot analysis was skipped because the current branch has no \
                 commits yet, so the hotspots, churn and ownership sections report nothing \
                 rather than zero. Commit the project to give churn a history, or pass \
                 --churn-file with exported change history."
                .to_owned(),
            "churn-file-unreadable" => format!(
                "Hotspot analysis was skipped because the churn file '{display}' could no longer \
                 be read after it was validated, so the hotspots, churn and ownership sections \
                 report nothing rather than zero. Make sure nothing rewrites the file while \
                 fallow runs, and rerun."
            ),
            // The original single cause, whose wording predates the token and
            // is kept byte-identical: a consumer matching on this sentence is
            // reading the same run it always was.
            _ => "Hotspot analysis was skipped because no git repository was found at the \
                  project root, so the hotspots, churn and ownership sections report nothing \
                  rather than zero. Run fallow inside the repository, or pass --churn-file with \
                  exported change history."
                .to_owned(),
        },
        WorkspaceDiagnosticKind::ChangeCouplingSkipped { cause } => match cause.as_str() {
            "shallow-clone" => "Change coupling was not measured because the repository is a \
                 shallow clone, so no change coupling pairs are reported. Fetch the full \
                 history with `git fetch --unshallow` to measure it."
                .to_owned(),
            _ => "Change coupling found no pairs because the history window has fewer \
                  analyzed commits than the minimum revisions of one file. Widen the window \
                  with --since, or lower health.changeCoupling.minRevisions."
                .to_owned(),
        },
        WorkspaceDiagnosticKind::ShallowClone {
            ownership_requested,
        } => {
            let ownership = if *ownership_requested {
                " Ownership signals are skewed too, because a shallow clone inflates \
                 single-author dominance."
            } else {
                ""
            };
            format!(
                "This is a shallow clone, so churn covers only the fetched history and every \
                 hotspot figure is incomplete.{ownership} Run git fetch --unshallow for the full \
                 history."
            )
        }
        WorkspaceDiagnosticKind::UnpinnedClock => {
            "No commit timestamp was available, so churn recency and ownership staleness were \
             measured against the wall clock and drift between runs over the same commit. Set \
             FALLOW_CLOCK_EPOCH to pin the run clock."
                .to_owned()
        }
        _ => input_message(display, kind),
    }
}

/// The message of the ownership, trend and coverage input kinds.
fn input_message(display: &str, kind: &WorkspaceDiagnosticKind) -> String {
    match kind {
        WorkspaceDiagnosticKind::OwnershipUnavailable { cause, error } => {
            if cause == "codeowners-parse-failed" {
                format!(
                    "Ownership signals are degraded: CODEOWNERS could not be parsed ({error}), \
                     so hotspot entries carry no declared owner. Fix the CODEOWNERS syntax, or \
                     drop --ownership for this run."
                )
            } else {
                format!(
                    "Ownership signals are degraded: health.ownership.botPatterns contains an \
                     invalid glob ({error}), so no author is classified as a bot and bot commits \
                     count towards ownership. Fix the pattern, or remove it from the config."
                )
            }
        }
        WorkspaceDiagnosticKind::TrendSnapshotUnreadable { error } => format!(
            "Skipped health snapshot '{display}' ({error}), so the trend is computed over fewer \
             snapshots than this project has on disk. Delete the unreadable file, or rewrite it \
             with fallow health --save-snapshot."
        ),
        WorkspaceDiagnosticKind::TrendGroupBaselineUnavailable { cause } => {
            if cause == "grouped-by-mismatch" {
                format!(
                    "Health snapshot '{display}' was saved with a different --group-by mode, so \
                     the groups have no trend. The project trend is not affected. Save a new \
                     snapshot with the same --group-by mode."
                )
            } else {
                format!(
                    "Health snapshot '{display}' holds no group data, so the groups have no \
                     trend. The project trend is not affected. Save a new snapshot with \
                     --group-by and --save-snapshot."
                )
            }
        }
        WorkspaceDiagnosticKind::CoverageAutoDetected => format!(
            "Coverage was auto-detected at '{display}' rather than passed with --coverage, so the \
             CRAP scores depend on whichever coverage file is on disk at run time. Pass --coverage \
             '{display}' explicitly for reproducible scores."
        ),
        _ => String::new(),
    }
}
