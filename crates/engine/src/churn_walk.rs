//! The `git log` walk that reads churn events.

use std::path::Path;
use std::process::Command;

use super::{ChurnEventState, git_command, parse_git_log_events_z, spawn_output};

/// Applies the cutoff to each commit and walks the full history. Git
/// accepts this flag from version 2.37.
const CUTOFF_FILTER_FLAG: &str = "--since-as-filter";
/// Stops the walk at the first commit that is older than the cutoff.
const CUTOFF_STOP_FLAG: &str = "--after";

/// Run `git log --numstat` over `revisions` and return event-level churn
/// state. `revisions` is a commit id, or an `<old>..<new>` range of commit
/// ids.
///
/// The cutoff is an absolute epoch and not a phrase that git resolves
/// against the wall clock, so two runs over one commit see the same window
/// boundary. Git applies it to each commit, as
/// [`super::ChurnEventState::retain_window`] does for cached events.
pub(super) fn analyze_churn_events(
    root: &Path,
    cutoff_secs: Option<u64>,
    revisions: &str,
    clock: &crate::clock::AnalysisClock,
) -> Option<ChurnEventState> {
    let mut output = run_churn_log(root, cutoff_secs, revisions, CUTOFF_FILTER_FLAG)?;
    if !output.status.success()
        && is_unknown_cutoff_filter(&String::from_utf8_lossy(&output.stderr))
    {
        output = run_churn_log(root, cutoff_secs, revisions, CUTOFF_STOP_FLAG)?;
    }
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        tracing::warn!("hotspot analysis skipped: git log failed: {stderr}");
        return None;
    }

    let mut state = parse_git_log_events_z(&output.stdout, root, clock.epoch_secs());
    state.git_log_bytes = output.stdout.len() as u64;
    Some(state)
}

/// True when `stderr` is the refusal of a Git that does not know
/// [`CUTOFF_FILTER_FLAG`].
fn is_unknown_cutoff_filter(stderr: &str) -> bool {
    stderr.contains(CUTOFF_FILTER_FLAG)
}

/// Run one churn `git log`. `None` when the process does not start.
fn run_churn_log(
    root: &Path,
    cutoff_secs: Option<u64>,
    revisions: &str,
    cutoff_flag: &str,
) -> Option<std::process::Output> {
    let mut command = churn_log_command(root, cutoff_secs, revisions, cutoff_flag);
    match spawn_output(&mut command) {
        Ok(output) => Some(output),
        Err(e) => {
            tracing::warn!("hotspot analysis skipped: failed to run git: {e}");
            None
        }
    }
}

/// The `git log --numstat` command over `revisions` with `cutoff_flag`.
fn churn_log_command(
    root: &Path,
    cutoff_secs: Option<u64>,
    revisions: &str,
    cutoff_flag: &str,
) -> Command {
    let mut command = git_command(root);
    command.arg("log").arg(revisions);
    command
        .args([
            "--numstat",
            "--no-merges",
            "--no-renames",
            "--use-mailmap",
            "-z",
            "--format=format:%at|%ct|%H|%aE%x00",
        ])
        .current_dir(root);
    if let Some(cutoff) = cutoff_secs {
        command.arg(format!("{cutoff_flag}=@{cutoff}"));
    }
    // numstat paths are relative to the git toplevel. When the project root
    // is a subdirectory, `--relative` rewrites them relative to the root, and
    // the `.` pathspec skips commits and files outside it. Both are no-ops at
    // the toplevel.
    command.args(["--relative", "--", "."]);
    command
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::super::{
        ChurnCoverage, analyze_churn_cached, analyze_fresh_churn, extend_churn_cache, get_head_sha,
        git_command, parse_since, read_churn_cache,
    };
    use super::is_unknown_cutoff_filter;

    fn git(root: &Path, args: &[&str]) {
        let status = git_command(root)
            .args(args)
            .current_dir(root)
            .status()
            .expect("run git");
        assert!(status.success(), "git {args:?} failed");
    }

    /// Commit one new file and return the commit id.
    fn commit(root: &Path, name: &str) -> String {
        std::fs::write(root.join(name), "export const value = 1;\n").expect("write file");
        git(root, &["add", "."]);
        git(root, &["commit", "--quiet", "-m", name]);
        get_head_sha(root).expect("head sha")
    }

    fn init_repo(root: &Path) {
        git(root, &["init", "--quiet"]);
        git(root, &["config", "user.email", "churn@example.test"]);
        git(root, &["config", "user.name", "Churn Test"]);
        git(root, &["config", "commit.gpgsign", "false"]);
    }

    #[test]
    fn fresh_walk_reads_the_recorded_revision() {
        let repo = tempfile::tempdir().expect("create repo");
        let root = repo.path();
        init_repo(root);
        let first = commit(root, "a.ts");
        commit(root, "b.ts");

        let since = parse_since("1y").expect("window");
        let clock = crate::clock::AnalysisClock::for_repo(root);
        let coverage = ChurnCoverage {
            head_sha: first,
            earliest_cutoff_secs: since.window.cutoff_secs(&clock),
            ..ChurnCoverage::default()
        };
        let cache = tempfile::tempdir().expect("create cache dir");
        let state = analyze_fresh_churn(root, &since, cache.path(), true, &coverage, &clock)
            .expect("fresh walk");

        assert!(state.files.contains_key(&root.join("a.ts")));
        assert!(
            !state.files.contains_key(&root.join("b.ts")),
            "the walk must stop at the recorded revision"
        );
    }

    #[test]
    fn extension_walk_ends_at_the_recorded_revision() {
        let repo = tempfile::tempdir().expect("create repo");
        let root = repo.path();
        init_repo(root);
        commit(root, "a.ts");
        let since = parse_since("1y").expect("window");
        let cache_dir = tempfile::tempdir().expect("create cache dir");
        analyze_churn_cached(root, &since, cache_dir.path(), false).expect("cold run");

        let second = commit(root, "b.ts");
        commit(root, "c.ts");
        let clock = crate::clock::AnalysisClock::for_repo(root);
        let cache = read_churn_cache(cache_dir.path()).expect("saved entry");
        let extended = cache
            .coverage()
            .extended_to(&second, since.window.cutoff_secs(&clock));
        let state = extend_churn_cache(root, &since, cache_dir.path(), &extended, cache, &clock)
            .expect("extension");

        assert!(state.files.contains_key(&root.join("b.ts")));
        assert!(
            !state.files.contains_key(&root.join("c.ts")),
            "the extension must end at the recorded revision"
        );
    }

    #[test]
    fn unknown_cutoff_filter_is_detected() {
        assert!(is_unknown_cutoff_filter(
            "fatal: unrecognized argument: --since-as-filter=@1"
        ));
        assert!(!is_unknown_cutoff_filter("fatal: bad revision 'abc'"));
    }
}
