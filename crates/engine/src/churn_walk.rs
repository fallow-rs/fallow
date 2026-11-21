//! The `git log` walk that reads churn events.

use std::path::Path;

use super::{ChurnEventState, SinceDuration, git_command, parse_git_log_events_z, spawn_output};

/// Run `git log --numstat` and return event-level churn state.
///
/// The window is passed as an absolute `--after=@<epoch>` resolved against the
/// run clock rather than as a phrase git re-resolves against the wall clock, so
/// two runs over one commit see the same window boundary.
pub(super) fn analyze_churn_events(
    root: &Path,
    since: &SinceDuration,
    revision_range: Option<&str>,
    clock: &crate::clock::AnalysisClock,
) -> Option<ChurnEventState> {
    let mut command = git_command(root);
    command.arg("log");
    if let Some(range) = revision_range {
        command.arg(range);
    }
    command
        .args([
            "--numstat",
            "--no-merges",
            "--no-renames",
            "--use-mailmap",
            "-z",
            "--format=format:%at|%ct|%H|%ae%x00",
        ])
        .current_dir(root);
    if let Some(cutoff) = since.window.cutoff_secs(clock) {
        command.arg(format!("--after=@{cutoff}"));
    }
    // numstat paths are relative to the git toplevel. When the project root
    // is a subdirectory, `--relative` rewrites them relative to the root, and
    // the `.` pathspec skips commits and files outside it. Both are no-ops at
    // the toplevel.
    command.args(["--relative", "--", "."]);

    let output = match spawn_output(&mut command) {
        Ok(o) => o,
        Err(e) => {
            tracing::warn!("hotspot analysis skipped: failed to run git: {e}");
            return None;
        }
    };

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        tracing::warn!("hotspot analysis skipped: git log failed: {stderr}");
        return None;
    }

    let mut state = parse_git_log_events_z(&output.stdout, root, clock.epoch_secs());
    state.git_log_bytes = output.stdout.len() as u64;
    Some(state)
}
