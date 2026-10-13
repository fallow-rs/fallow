//! A warm churn cache must produce the same result as a cold build.
//!
//! `crates/core/tests/integration_test/graph_cache_transparency.rs` states the
//! invariant for the analysis graph: a cache hit has to produce identical
//! results to a cold build. Churn is the one subsystem where nothing enforced
//! it. The cache only ever appended, so an entry minted while a commit was
//! still inside the window kept reporting that commit long after a cold
//! `git log --after` stopped returning it.
//!
//! These tests drive the real `analyze_churn_cached` entry point over a real
//! git repository with commits placed deliberately on the window edge.

#![allow(
    clippy::expect_used,
    reason = "integration tests use expect to keep fixture setup concise"
)]

use std::path::{Path, PathBuf};
use std::process::Command;

use fallow_engine::churn::{ChurnResult, ChurnWindowUnit, SinceDuration, analyze_churn_cached};

/// Window used by every test here: 90 days, so the cutoff is exactly
/// `head_committer_epoch - 90 * 86400` with no calendar clamping to reason
/// about.
const WINDOW_DAYS: u64 = 90;

const SECS_PER_DAY: u64 = 86_400;

/// Fixed base instant (2023-11-14T22:13:20Z) so every commit timestamp in
/// these tests is an exact, reproducible epoch rather than an offset from the
/// wall clock.
const BASE_EPOCH: u64 = 1_700_000_000;

fn window() -> SinceDuration {
    SinceDuration::relative(WINDOW_DAYS, ChurnWindowUnit::Days, "90 days")
}

fn git(root: &Path, args: &[&str], epoch: Option<u64>) {
    let mut command = Command::new("git");
    fallow_engine::changed_files::clear_ambient_git_env(&mut command);
    command.current_dir(root).args(args);
    if let Some(epoch) = epoch {
        let stamp = format!("{epoch} +0000");
        command
            .env("GIT_AUTHOR_DATE", &stamp)
            .env("GIT_COMMITTER_DATE", &stamp);
    }
    let status = command.status().expect("run git");
    assert!(status.success(), "git {args:?} failed");
}

fn init_repo(root: &Path) {
    git(root, &["init", "--quiet", "--initial-branch=main"], None);
    git(root, &["config", "user.name", "Churn Fixture"], None);
    git(root, &["config", "user.email", "fixture@example.com"], None);
    git(root, &["config", "commit.gpgsign", "false"], None);
}

/// Write `path` and commit it with both author and committer date pinned to
/// `epoch`. `git log --after` filters on the committer date, and the run clock
/// reads HEAD's committer date, so pinning both makes the window boundary
/// exact.
fn commit_file(root: &Path, path: &str, contents: &str, epoch: u64) {
    let file = root.join(path);
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent).expect("create source directory");
    }
    std::fs::write(&file, contents).expect("write source file");
    git(root, &["add", path], None);
    git(root, &["commit", "--quiet", "-m", path], Some(epoch));
}

fn churn(root: &Path, cache_dir: &Path, no_cache: bool) -> (ChurnResult, bool) {
    analyze_churn_cached(root, &window(), cache_dir, no_cache).expect("churn analysis")
}

/// Files with retained churn, sorted, as project-relative strings.
fn changed_files(result: &ChurnResult, root: &Path) -> Vec<String> {
    let mut paths: Vec<String> = result
        .files
        .keys()
        .map(|path| {
            path.strip_prefix(root)
                .unwrap_or(path)
                .to_string_lossy()
                .replace('\\', "/")
        })
        .collect();
    paths.sort();
    paths
}

/// Every comparable field of a churn row, so a warm/cold comparison catches a
/// retained event even when the file set already matches.
fn churn_rows(result: &ChurnResult, root: &Path) -> Vec<(String, u32, u64, u64)> {
    let mut rows: Vec<(String, u32, u64, u64)> = result
        .files
        .iter()
        .map(|(path, file)| {
            (
                path.strip_prefix(root)
                    .unwrap_or(path)
                    .to_string_lossy()
                    .replace('\\', "/"),
                file.commits,
                u64::from(file.lines_added),
                u64::from(file.lines_deleted),
            )
        })
        .collect();
    rows.sort();
    rows
}

struct Fixture {
    _dir: tempfile::TempDir,
    root: PathBuf,
    cache: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let dir = tempfile::tempdir().expect("churn fixture tempdir");
        // The macOS tempdir lives behind a /var -> /private/var symlink, and
        // churn keys files by the path git reports under `root`. Canonicalize
        // once so `strip_prefix` in the assertions lines up.
        let root = dir.path().canonicalize().expect("canonical fixture root");
        let cache = root.join(".fallow-cache");
        init_repo(&root);
        Self {
            _dir: dir,
            root,
            cache,
        }
    }
}

#[test]
fn warm_cache_drops_commits_the_window_no_longer_covers() {
    let fixture = Fixture::new();
    let root = &fixture.root;

    // Two commits inside the window as of the second commit's own date.
    commit_file(root, "src/old.ts", "export const old = 1;\n", BASE_EPOCH);
    commit_file(
        root,
        "src/mid.ts",
        "export const mid = 1;\n",
        BASE_EPOCH + 10 * SECS_PER_DAY,
    );

    // Mint the cache while both commits still qualify.
    let (warm_seed, seeded_from_cache) = churn(root, &fixture.cache, false);
    assert!(!seeded_from_cache, "first run must be a cold build");
    assert_eq!(
        changed_files(&warm_seed, root),
        vec!["src/mid.ts".to_owned(), "src/old.ts".to_owned()],
        "both commits are inside the window at the seeding commit"
    );

    // Advance HEAD far enough that the run clock moves the cutoff past both
    // earlier commits. This is the "cache minted months ago" case, expressed
    // as HEAD advancing rather than as the wall clock drifting.
    commit_file(
        root,
        "src/new.ts",
        "export const fresh = 1;\n",
        BASE_EPOCH + 200 * SECS_PER_DAY,
    );

    let (warm, from_cache) = churn(root, &fixture.cache, false);
    assert!(from_cache, "second run must reuse the seeded cache");

    let cold_cache = root.join(".fallow-cold-cache");
    let (cold, _) = churn(root, &cold_cache, true);

    assert_eq!(
        changed_files(&cold, root),
        vec!["src/new.ts".to_owned()],
        "a cold build only sees the commit inside the 90 day window"
    );
    assert_eq!(
        churn_rows(&warm, root),
        churn_rows(&cold, root),
        "a warm churn cache must produce the same rows as a cold build"
    );
    assert_eq!(
        warm.shallow_clone, cold.shallow_clone,
        "shallow-clone detection must survive the cache path"
    );
    assert_eq!(
        warm.clock.epoch_secs(),
        cold.clock.epoch_secs(),
        "both runs resolve the same clock from the same HEAD"
    );
}

#[test]
fn cache_prune_and_git_after_agree_on_the_cutoff_second() {
    let fixture = Fixture::new();
    let root = &fixture.root;

    // Three commits laid out so that a later HEAD puts the cutoff exactly on
    // `edge.ts`'s committer second: `head.ts` is 90 days after `edge.ts`, and
    // the window is 90 days. `before-edge.ts` sits one second earlier.
    // Git's `--after` decides the cold answer and the cache prune decides the
    // warm one; if they disagree by a second, warm and cold differ by a commit.
    let edge_epoch = BASE_EPOCH;
    let head_epoch = edge_epoch + WINDOW_DAYS * SECS_PER_DAY;

    commit_file(
        root,
        "src/before-edge.ts",
        "export const before = 1;\n",
        edge_epoch - 1,
    );
    commit_file(root, "src/edge.ts", "export const edge = 1;\n", edge_epoch);

    // Seed the cache at a HEAD where both boundary commits are comfortably
    // inside the window, so the prune, not `git log`, is what decides their
    // fate on the warm run below.
    commit_file(
        root,
        "src/seed.ts",
        "export const seed = 1;\n",
        edge_epoch + 10 * SECS_PER_DAY,
    );
    let (seeded, seeded_from_cache) = churn(root, &fixture.cache, false);
    assert!(!seeded_from_cache, "first run must be a cold build");
    assert!(
        changed_files(&seeded, root).contains(&"src/before-edge.ts".to_owned()),
        "both boundary commits must reach the cache"
    );

    commit_file(root, "src/head.ts", "export const head = 1;\n", head_epoch);

    let (warm, from_cache) = churn(root, &fixture.cache, false);
    assert!(
        from_cache,
        "advancing HEAD must extend the cache, not miss it"
    );

    let cold_cache = root.join(".fallow-cold-cache");
    let (cold, _) = churn(root, &cold_cache, true);

    assert_eq!(
        changed_files(&cold, root),
        vec![
            "src/edge.ts".to_owned(),
            "src/head.ts".to_owned(),
            "src/seed.ts".to_owned(),
        ],
        "the cutoff second itself is inside the window, the second before it is not"
    );
    assert_eq!(
        churn_rows(&warm, root),
        churn_rows(&cold, root),
        "the cache prune must use the same boundary as git --after"
    );
}

/// A project root below the git toplevel (a workspace package, or a repo
/// where the app lives in a subdirectory) must key churn by paths under that
/// root, and must not count files outside it. `git log --numstat` reports
/// paths relative to the toplevel, so joining them to a subdirectory root
/// produced paths that match no source file.
#[test]
fn subdirectory_root_keys_churn_under_the_root() {
    let fixture = Fixture::new();
    let head = BASE_EPOCH + 200 * SECS_PER_DAY;
    commit_file(
        &fixture.root,
        "packages/app/src/a.ts",
        "a1",
        head - 20 * SECS_PER_DAY,
    );
    commit_file(&fixture.root, "other/b.ts", "b1", head - 10 * SECS_PER_DAY);
    commit_file(&fixture.root, "packages/app/src/a.ts", "a2", head);

    let app = fixture.root.join("packages/app");
    let cache = app.join(".fallow-cache");
    let (cold, _) = churn(&app, &cache, false);
    assert_eq!(changed_files(&cold, &app), vec!["src/a.ts".to_string()]);
    assert_eq!(cold.files[&app.join("src/a.ts")].commits, 2);

    // The incremental `<cached>..HEAD` scan must apply the same scope.
    commit_file(&fixture.root, "other/b.ts", "b2", head + SECS_PER_DAY);
    commit_file(
        &fixture.root,
        "packages/app/src/a.ts",
        "a3",
        head + 2 * SECS_PER_DAY,
    );
    let (warm, reused) = churn(&app, &cache, false);
    assert!(reused, "second run must extend the cache");
    let (fresh, _) = churn(&app, &cache, true);
    assert_eq!(changed_files(&warm, &app), vec!["src/a.ts".to_string()]);
    assert_eq!(churn_rows(&warm, &app), churn_rows(&fresh, &app));
    assert_eq!(warm.files[&app.join("src/a.ts")].commits, 3);
}

/// Stage several files and commit them as one commit at `epoch`.
fn commit_files(root: &Path, files: &[(&str, &str)], epoch: u64) {
    for (path, contents) in files {
        let file = root.join(path);
        if let Some(parent) = file.parent() {
            std::fs::create_dir_all(parent).expect("create source directory");
        }
        std::fs::write(&file, contents).expect("write source file");
        git(root, &["add", path], None);
    }
    git(root, &["commit", "--quiet", "-m", "multi"], Some(epoch));
}

fn head_sha(root: &Path) -> String {
    let mut command = Command::new("git");
    fallow_engine::changed_files::clear_ambient_git_env(&mut command);
    let output = command
        .args(["rev-parse", "HEAD"])
        .current_dir(root)
        .output()
        .expect("rev-parse");
    String::from_utf8(output.stdout)
        .expect("utf8 sha")
        .trim()
        .to_owned()
}

/// Commit file sets as `(sha, [relative paths])`, sorted, for comparison.
fn commit_sets(result: &ChurnResult, root: &Path) -> Vec<(String, Vec<String>)> {
    let mut sets: Vec<(String, Vec<String>)> = result
        .commits
        .iter()
        .map(|commit| {
            let mut files: Vec<String> = commit
                .files
                .iter()
                .map(|file| {
                    file.path
                        .strip_prefix(root)
                        .unwrap_or(&file.path)
                        .to_string_lossy()
                        .replace('\\', "/")
                })
                .collect();
            files.sort();
            (commit.sha.clone(), files)
        })
        .collect();
    sets.sort();
    sets
}

#[test]
fn churn_groups_files_by_commit_sha() {
    let fixture = Fixture::new();
    let root = &fixture.root;
    commit_files(
        root,
        &[
            ("src/a.ts", "export const a = 1;\n"),
            ("src/b.ts", "export const b = 1;\n"),
        ],
        BASE_EPOCH,
    );
    let first = head_sha(root);
    commit_file(
        root,
        "src/a.ts",
        "export const a = 2;\nexport const c = 3;\n",
        BASE_EPOCH + SECS_PER_DAY,
    );
    let second = head_sha(root);

    let (result, _) = churn(root, &fixture.cache, true);
    let mut expected = vec![
        (first, vec!["src/a.ts".to_owned(), "src/b.ts".to_owned()]),
        (second.clone(), vec!["src/a.ts".to_owned()]),
    ];
    expected.sort();
    assert_eq!(
        commit_sets(&result, root),
        expected,
        "each commit keeps the set of files it changed"
    );
    let latest = result
        .commits
        .iter()
        .find(|commit| commit.sha == second)
        .expect("second commit");
    assert_eq!(
        latest.files[0].lines_changed, 3,
        "1 deleted + 2 added lines"
    );
}

#[test]
fn incremental_cache_keeps_commit_file_sets_identical_to_a_cold_build() {
    let fixture = Fixture::new();
    let root = &fixture.root;
    commit_files(
        root,
        &[
            ("src/a.ts", "export const a = 1;\n"),
            ("src/b.ts", "export const b = 1;\n"),
        ],
        BASE_EPOCH,
    );
    let (_, seeded) = churn(root, &fixture.cache, false);
    assert!(!seeded);
    commit_files(
        root,
        &[
            ("src/a.ts", "export const a = 2;\n"),
            ("src/c.ts", "export const c = 1;\n"),
        ],
        BASE_EPOCH + SECS_PER_DAY,
    );

    let (warm, from_cache) = churn(root, &fixture.cache, false);
    assert!(from_cache, "the second run extends the cache");
    let (cold, _) = churn(root, &root.join(".fallow-cold-cache"), true);
    assert_eq!(commit_sets(&warm, root), commit_sets(&cold, root));
    assert_eq!(commit_sets(&cold, root).len(), 2);
}

#[test]
fn a_wider_cached_window_serves_a_narrower_request() {
    let fixture = Fixture::new();
    let root = &fixture.root;
    commit_file(root, "src/old.ts", "export const old = 1;\n", BASE_EPOCH);
    commit_file(
        root,
        "src/new.ts",
        "export const fresh = 1;\n",
        BASE_EPOCH + 200 * SECS_PER_DAY,
    );
    let wide = SinceDuration::relative(1, ChurnWindowUnit::Years, "1 year");

    let (wide_result, wide_hit) =
        analyze_churn_cached(root, &wide, &fixture.cache, false).expect("wide churn");
    assert!(!wide_hit);
    assert_eq!(
        changed_files(&wide_result, root),
        vec!["src/new.ts".to_owned(), "src/old.ts".to_owned()]
    );

    let (narrow, narrow_hit) =
        analyze_churn_cached(root, &window(), &fixture.cache, false).expect("narrow churn");
    assert!(narrow_hit, "the 1y cache entry covers the 90 day request");
    let (cold, _) = churn(root, &root.join(".fallow-cold-cache"), true);
    assert_eq!(churn_rows(&narrow, root), churn_rows(&cold, root));
    assert_eq!(commit_sets(&narrow, root), commit_sets(&cold, root));

    // The narrow run must not replace the wide entry.
    let (_, wide_again) =
        analyze_churn_cached(root, &wide, &fixture.cache, false).expect("wide churn again");
    assert!(wide_again, "the wide entry survives a narrow run");
}

#[test]
fn history_window_keeps_older_commits_while_files_use_the_narrow_window() {
    let fixture = Fixture::new();
    let root = &fixture.root;
    commit_files(
        root,
        &[
            ("src/a.ts", "export const a = 1;\n"),
            ("src/b.ts", "export const b = 1;\n"),
        ],
        BASE_EPOCH,
    );
    commit_file(
        root,
        "src/c.ts",
        "export const c = 1;\n",
        BASE_EPOCH + 200 * SECS_PER_DAY,
    );
    let history = SinceDuration::relative(1, ChurnWindowUnit::Years, "1 year");

    let (result, _) = fallow_engine::churn::analyze_churn_cached_with_history(
        root,
        &window(),
        &history,
        &fixture.cache,
        true,
    )
    .expect("churn with history");
    assert_eq!(
        changed_files(&result, root),
        vec!["src/c.ts".to_owned()],
        "per-file churn uses the 90 day window"
    );
    assert_eq!(
        result.commits.len(),
        2,
        "the commit list covers the one year history window"
    );
}
