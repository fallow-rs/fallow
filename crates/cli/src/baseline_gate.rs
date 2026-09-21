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

use fallow_engine::baseline::{BaselineKind, BaselineStaleness, stale_baseline_gate_trips};
use std::path::{Path, PathBuf};

/// A loaded baseline's staleness together with the file it was read from, so
/// the gate can name the path to re-save.
#[derive(Clone, Debug)]
pub struct LoadedBaselineStaleness {
    pub staleness: BaselineStaleness,
    pub path: PathBuf,
    /// Which channels narrowed the run, from the same predicate that produced
    /// `staleness.change_scoped`. Carried here rather than on the engine
    /// struct so the analysis keeps the one boolean it needs.
    pub scope_reasons: fallow_output::BaselineScopeReasons,
    /// True when the file carried no key this command's baseline format
    /// writes, so it is another command's baseline rather than an empty one of
    /// this command's.
    pub unrecognised_format: bool,
}

impl LoadedBaselineStaleness {
    /// This run's view of the baseline, for the JSON envelope.
    #[must_use]
    pub fn to_envelope(&self, moved_entries: usize) -> fallow_output::BaselineStaleness {
        self.staleness
            .to_envelope(moved_entries, self.scope_reasons, self.unrecognised_format)
    }
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
        loaded.unrecognised_format,
        &loaded.path,
        noun,
    )
}

/// [`gate_failed`] for a caller that holds the published envelope object rather
/// than a [`LoadedBaselineStaleness`], which is `health`: its load happens in the
/// engine and the report is what comes back.
pub fn gate_failed_from_envelope(
    staleness: &fallow_output::BaselineStaleness,
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
    report_gate(
        staleness.baseline_entries,
        staleness.matched_entries,
        staleness.change_scoped,
        staleness.unrecognised_format,
        path,
        noun,
    )
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

/// Say that a loaded baseline is not written in this command's format.
///
/// Such a file suppresses nothing, so the run names the mistake instead of
/// gating on a file it never read: the advisory has nothing to judge and the
/// counts are all zero, which without this line reads exactly like a project
/// that had nothing to record.
///
/// A baseline saved from this version onward names the command that wrote it,
/// which is what lets the note say so. `unrecognised_format` is the run's own
/// verdict and decides whether anything is said at all; the file is read again
/// only to name that command, because health classifies inside the engine and
/// the bytes are gone by the time the CLI can print under `--quiet`. A second
/// read that no longer agrees falls back to the wording that needs no second
/// fact, which is also the wording a baseline saved before `kind` existed earns.
///
/// Prints regardless of `--quiet`, for the reason [`report_gate`] documents:
/// the fact appears in no human report, and `--ci` implies `--quiet`, which is
/// exactly the configuration where a silently green gate matters.
pub fn note_unrecognised_baseline(
    path: Option<&Path>,
    unrecognised_format: bool,
    expected: BaselineKind,
) {
    if !unrecognised_format {
        return;
    }
    let Some(path) = path else {
        return;
    };
    if let Some(found) = saved_by_another_command(path, expected) {
        eprintln!(
            "Note: the baseline at {} was saved by `fallow {found}` and this is a `fallow {}` \
             run, so it suppresses nothing. Save each command's baseline to its own path.",
            path.display(),
            expected.as_str(),
        );
        return;
    }
    eprintln!(
        "Note: the baseline at {} has no entries this command recognises. It may be a baseline \
         saved by another command, or an empty file. Either way it suppresses nothing.",
        path.display(),
    );
}

/// The `kind` a baseline file names, when it names a command other than
/// `expected`. `None` for a file that names none, which is every baseline saved
/// before the member existed.
fn saved_by_another_command(path: &Path, expected: BaselineKind) -> Option<String> {
    let content = std::fs::read_to_string(path).ok()?;
    match fallow_engine::baseline::classify_baseline_file(&content, expected) {
        fallow_engine::baseline::BaselineFileKind::Foreign(found) => Some(found),
        _ => None,
    }
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
    unrecognised_format: bool,
    path: &Path,
    noun: &str,
) -> bool {
    // A file this command cannot read as its own suppresses nothing, which the
    // counts cannot say: they are all zero, exactly as they are for a baseline
    // saved from a project that had nothing to record. A repository that armed
    // the gate asked to hear about a baseline that protects nothing, so this
    // fires ahead of the count rule and is not suppressed by a narrowed scope,
    // which changes nothing about which command wrote the file.
    if unrecognised_format {
        eprintln!(
            "Baseline gate failed: the baseline {} has no entries this command recognises, so it \
             suppresses nothing. Point --baseline at this command's own baseline.",
            path.display(),
        );
        return true;
    }
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
