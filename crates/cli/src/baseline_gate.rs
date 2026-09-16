//! The opt-in `--fail-on-stale-baseline` gate, shared by every command that
//! accepts `--baseline`.
//!
//! The advisory warning and this gate answer different questions. The warning
//! is calibrated so an unasked-for line stays worth reading; the gate is what a
//! repository asks for when it wants any stale entry to break the build. Both
//! read the same [`fallow_engine::baseline::BaselineStaleness`], so the two can
//! disagree about whether to speak but never about the facts.
//!
//! Every outcome of the flag says so on stderr. A gate a repository opted into
//! that then stays quiet is worse than no gate at all: the build goes green and
//! nobody learns the baseline was never judged. So the runs that cannot or will
//! not apply the gate, a run narrowed to part of the project and
//! `health --report-only`, name the reason they are standing down instead of
//! returning silently.
//!
//! The verdict is the exit code plus one stderr line, in every output format.
//! A machine consumer reads it from the envelope instead: `dead-code`, `dupes`
//! and `health` publish `baseline_staleness`, whose `gate_trips` is this gate's
//! rule computed by [`stale_baseline_gate_trips`], so a CI integration that
//! cannot see stderr does not have to restate the condition. That object is
//! emitted whenever a baseline was loaded, with or without this flag, so the
//! flag still changes nothing but the exit code and the stderr line.

#![allow(
    clippy::print_stderr,
    reason = "the gate explains a non-zero exit on stderr, like the other CLI gates"
)]

use fallow_engine::baseline::{BaselineStaleness, stale_baseline_gate_trips};
use std::path::{Path, PathBuf};

/// A loaded baseline's staleness together with the file it was read from, so
/// the gate can name the path to re-save.
#[derive(Clone, Debug)]
pub struct LoadedBaselineStaleness {
    pub staleness: BaselineStaleness,
    pub path: PathBuf,
}

/// What each command calls the things its baseline entries describe.
/// Matches the noun the command's own staleness warning already uses.
pub const DEAD_CODE_NOUN: &str = "issue";
pub const DUPES_NOUN: &str = "clone group";
pub const HEALTH_NOUN: &str = "finding";

/// Evaluate the gate for a loaded baseline, print what it decided, and report
/// whether it fired.
pub fn gate_failed(loaded: Option<&LoadedBaselineStaleness>, enabled: bool, noun: &str) -> bool {
    if !enabled {
        return false;
    }
    let Some(loaded) = loaded else {
        return false;
    };
    report_gate(
        loaded.staleness.entries,
        loaded.staleness.matched,
        loaded.staleness.change_scoped,
        &loaded.path,
        noun,
    )
}

/// [`gate_failed`] for callers that carry the counts in their own output type
/// instead of a [`LoadedBaselineStaleness`].
pub fn gate_failed_from_counts(
    entries: usize,
    matched: usize,
    change_scoped: bool,
    path: Option<&Path>,
    enabled: bool,
    noun: &str,
) -> bool {
    if !enabled {
        return false;
    }
    let Some(path) = path else {
        return false;
    };
    report_gate(entries, matched, change_scoped, path, noun)
}

/// Say that a run which asked for the gate deliberately did not apply it,
/// naming the reason.
///
/// Same contract as the scope note in [`report_gate`], for the paths that
/// suppress the gate before it is ever evaluated: an opted-in gate that stays
/// quiet is worse than no gate at all, so every path that skips it says so.
/// Silent when the flag was not passed or no baseline was loaded, because
/// there is nothing to stand down from.
pub fn note_stood_down(path: Option<&Path>, enabled: bool, reason: &str) {
    if !enabled {
        return;
    }
    let Some(path) = path else {
        return;
    };
    eprintln!(
        "Note: --fail-on-stale-baseline did not run: {reason}, so the baseline {} was not judged.",
        path.display(),
    );
}

/// Print the gate's verdict for one loaded baseline and report whether it
/// fired.
///
/// Both the failure line and the stood-down note print regardless of
/// `--quiet`. Unlike the score and findings gates, whose condition is visible
/// in the report itself, the gate's verdict appears in no *human* output: the
/// human renderers never mention the baseline counts. Suppressing the line
/// would leave either a bare exit 1 or a green run with nothing to act on, and
/// `--ci` implies `--quiet`, which is exactly the configuration where that
/// matters. Machine consumers read `baseline_staleness.gate_trips` from the
/// envelope instead of this line.
fn report_gate(
    entries: usize,
    matched: usize,
    change_scoped: bool,
    path: &Path,
    noun: &str,
) -> bool {
    if !stale_baseline_gate_trips(entries, matched, change_scoped) {
        // A run narrowed to part of the project is the one case where the flag
        // was asked for and still cannot answer. Saying so is the difference
        // between a gate that passed and a gate that never ran: `--production`
        // and `--changed-since` are ordinary CI shapes, and a job that believes
        // it gates would otherwise stay green forever.
        if change_scoped && entries > 0 {
            eprintln!(
                "Note: --fail-on-stale-baseline did not run: this analysis covered only part of \
                 the project, which cannot judge the whole-project baseline {}. Re-run over the \
                 whole project to gate on it.",
                path.display(),
            );
        }
        return false;
    }
    let stale = entries.saturating_sub(matched);
    eprintln!(
        "Baseline gate failed: {stale} of {entries} entries in {} matched no current {noun}. \
         Re-save with: --save-baseline {}",
        path.display(),
        path.display(),
    );
    true
}
