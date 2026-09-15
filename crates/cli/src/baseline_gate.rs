//! The opt-in `--fail-on-stale-baseline` gate, shared by every command that
//! accepts `--baseline`.
//!
//! The advisory warning and this gate answer different questions. The warning
//! is calibrated so an unasked-for line stays worth reading; the gate is what a
//! repository asks for when it wants any stale entry to break the build. Both
//! read the same [`fallow_engine::baseline::BaselineStaleness`], so the two can
//! disagree about whether to speak but never about the facts.

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

/// Evaluate the gate for a loaded baseline and print its line when it fires.
///
/// The line prints regardless of `--quiet`. Unlike the score and findings
/// gates, whose condition is visible in the report itself, a stale baseline
/// appears nowhere in human or JSON output, so suppressing the line would leave
/// a bare exit 1 with nothing to act on. `--ci` implies `--quiet`, which is
/// exactly the configuration where that matters.
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

fn report_gate(
    entries: usize,
    matched: usize,
    change_scoped: bool,
    path: &Path,
    noun: &str,
) -> bool {
    if !stale_baseline_gate_trips(entries, matched, change_scoped) {
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
