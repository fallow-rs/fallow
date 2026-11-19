//! `fallow audit --staged` audits the staged index, not the working tree.
//!
//! The changed files are the staged changes against HEAD. The analysis reads
//! each file from the index, so unstaged edits and untracked files do not
//! change the verdict, and a staged file that is deleted from the working
//! tree is still audited.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    reason = "tests and benches use unwrap and expect to keep fixture setup concise"
)]

use std::fs;
use std::path::Path;
use std::process::Command;

use tempfile::TempDir;

use crate::common::{
    CommandOutput, commit_all, fallow_bin, git, git_command, parse_json, scrub_analysis_env,
};

/// A repository with one commit on `main`: `src/index.ts` is the entry and
/// exports `a`. A `node_modules` package is present and not ignored, so the
/// staged worktree links it.
fn fixture() -> TempDir {
    let tmp = TempDir::new().expect("failed to create temp dir");
    let dir = tmp.path();
    fs::create_dir_all(dir.join("src")).unwrap();
    fs::write(
        dir.join("package.json"),
        r#"{"name": "audit-staged", "private": true, "main": "src/index.ts"}"#,
    )
    .unwrap();
    fs::write(dir.join("src/index.ts"), "export const a = 1;\n").unwrap();
    git(dir, &["init", "-b", "main"]);
    git(dir, &["config", "core.autocrlf", "false"]);
    commit_all(dir, "baseline");
    fs::create_dir_all(dir.join("node_modules/left-pad")).unwrap();
    fs::write(
        dir.join("node_modules/left-pad/package.json"),
        r#"{"name": "left-pad", "version": "1.0.0"}"#,
    )
    .unwrap();
    tmp
}

/// Run `fallow audit --staged` in `dir`. `index_file` is the
/// `GIT_INDEX_FILE` of the run. Without it, the run gets no
/// `GIT_INDEX_FILE`, also when the test suite runs inside a Git hook.
fn audit_staged_with_index(dir: &Path, extra: &[&str], index_file: Option<&Path>) -> CommandOutput {
    let mut command = audit_staged_command(dir, extra);
    if let Some(index_file) = index_file {
        command.env("GIT_INDEX_FILE", index_file);
    }
    run(&mut command)
}

/// A `fallow audit --staged` command for `dir` with no `GIT_INDEX_FILE`.
fn audit_staged_command(dir: &Path, extra: &[&str]) -> Command {
    let mut command = Command::new(fallow_bin());
    command.env("RUST_LOG", "").env("NO_COLOR", "1");
    scrub_analysis_env(&mut command);
    command.env_remove("GIT_INDEX_FILE");
    command
        .args(["audit", "--root"])
        .arg(dir)
        .args(["--staged", "--format", "json", "--quiet"])
        .args(extra);
    command
}

fn run(command: &mut Command) -> CommandOutput {
    let output = command.output().expect("failed to run fallow binary");
    CommandOutput {
        stdout: String::from_utf8_lossy(&output.stdout).to_string(),
        stderr: String::from_utf8_lossy(&output.stderr).to_string(),
        code: output.status.code().unwrap_or(-1),
    }
}

fn audit_staged(dir: &Path, extra: &[&str]) -> CommandOutput {
    audit_staged_with_index(dir, extra, None)
}

fn unused_file_paths(json: &serde_json::Value) -> Vec<String> {
    json["dead_code"]["unused_files"]
        .as_array()
        .map(|files| {
            files
                .iter()
                .filter_map(|file| file["path"].as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

#[test]
fn untracked_and_unstaged_files_do_not_affect_the_verdict() {
    let tmp = fixture();
    let dir = tmp.path();
    fs::write(
        dir.join("src/index.ts"),
        "export const a = 1;\nexport const b = a;\n",
    )
    .unwrap();
    git(dir, &["add", "src/index.ts"]);
    fs::write(dir.join("src/untracked-orphan.ts"), "export const z = 3;\n").unwrap();
    fs::write(
        dir.join("src/index.ts"),
        "import './unstaged-orphan';\nexport const a = 1;\nexport const b = a;\n",
    )
    .unwrap();

    let output = audit_staged(dir, &[]);
    let json = parse_json(&output);

    assert_eq!(output.code, 0, "stderr: {}\njson: {json}", output.stderr);
    assert_eq!(json["verdict"], "pass", "{json}");
    assert_eq!(json["changed_files_count"], 1, "{json}");
    assert!(unused_file_paths(&json).is_empty(), "{json}");
}

#[test]
fn a_staged_file_deleted_from_the_working_tree_is_audited() {
    let tmp = fixture();
    let dir = tmp.path();
    fs::write(dir.join("src/staged-orphan.ts"), "export const q = 9;\n").unwrap();
    git(dir, &["add", "src/staged-orphan.ts"]);
    fs::remove_file(dir.join("src/staged-orphan.ts")).unwrap();

    let output = audit_staged(dir, &[]);
    let json = parse_json(&output);

    assert_eq!(output.code, 1, "stderr: {}\njson: {json}", output.stderr);
    assert_eq!(json["verdict"], "fail", "{json}");
    assert_eq!(
        unused_file_paths(&json),
        vec!["src/staged-orphan.ts".to_owned()],
        "{json}"
    );
}

/// `git commit -a` and `git commit <path>` give the hook another index file
/// through `GIT_INDEX_FILE`. The staged audit reads that index.
#[test]
fn the_index_file_that_git_gives_the_hook_is_audited() {
    let tmp = fixture();
    let dir = tmp.path();
    let index_file = dir.join(".git/next-index-test");
    fs::copy(dir.join(".git/index"), &index_file).unwrap();
    fs::write(dir.join("src/staged-orphan.ts"), "export const q = 9;\n").unwrap();
    let status = git_command(dir)
        .env("GIT_INDEX_FILE", &index_file)
        .args(["add", "src/staged-orphan.ts"])
        .status()
        .unwrap();
    assert!(status.success());
    fs::remove_file(dir.join("src/staged-orphan.ts")).unwrap();

    let default_index = audit_staged(dir, &[]);
    assert_eq!(default_index.code, 0, "stderr: {}", default_index.stderr);

    let output = audit_staged_with_index(dir, &[], Some(&index_file));
    let json = parse_json(&output);
    assert_eq!(output.code, 1, "stderr: {}\njson: {json}", output.stderr);
    assert_eq!(
        unused_file_paths(&json),
        vec!["src/staged-orphan.ts".to_owned()],
        "{json}"
    );
}

/// A plain `git commit` gives the hook the relative value `.git/index`. Git
/// resolves it against the top level of the work tree, also when the hook
/// runs `fallow` from a subdirectory.
#[test]
fn a_relative_index_file_resolves_against_the_top_level() {
    let tmp = fixture();
    let dir = tmp.path();
    fs::write(dir.join("src/staged-orphan.ts"), "export const q = 9;\n").unwrap();
    git(dir, &["add", "src/staged-orphan.ts"]);

    let output = run(audit_staged_command(dir, &[])
        .current_dir(dir.join("src"))
        .env("GIT_INDEX_FILE", ".git/index"));
    let json = parse_json(&output);

    assert_eq!(output.code, 1, "stderr: {}\njson: {json}", output.stderr);
    assert_eq!(
        unused_file_paths(&json),
        vec!["src/staged-orphan.ts".to_owned()],
        "{json}"
    );
}

#[test]
fn the_staged_worktree_is_removed_after_the_run() {
    let tmp = fixture();
    let dir = tmp.path();
    fs::write(dir.join("src/index.ts"), "export const a = 2;\n").unwrap();
    git(dir, &["add", "src/index.ts"]);

    let output = audit_staged(dir, &[]);
    assert_eq!(output.code, 0, "stderr: {}", output.stderr);

    let worktrees = git_command(dir)
        .args(["worktree", "list", "--porcelain"])
        .output()
        .unwrap();
    let listing = String::from_utf8_lossy(&worktrees.stdout);
    assert_eq!(
        listing.matches("worktree ").count(),
        1,
        "only the main worktree stays registered: {listing}"
    );
}

#[test]
fn staged_rejects_an_explicit_base() {
    let tmp = fixture();
    let output = audit_staged(tmp.path(), &["--base", "main"]);
    assert_eq!(output.code, 2, "stderr: {}", output.stderr);
    assert!(
        output.stdout.contains("does not take --base")
            || output.stderr.contains("does not take --base"),
        "stdout: {}\nstderr: {}",
        output.stdout,
        output.stderr
    );
}

/// Run `git commit` in `dir` with a `PATH` that holds only the built fallow
/// binary and the system tools, the way a Git client starts the hook.
#[cfg(unix)]
fn commit_through_hook(dir: &Path, bin: &Path, message: &str) -> std::process::Output {
    let path = format!("{}:/usr/bin:/bin", bin.display());
    git_command(dir)
        .env("PATH", path)
        .args(["commit", "-q", "-m", message])
        .output()
        .expect("git commit runs")
}

/// The generated hook gates the commit on the staged index. An untracked
/// orphan does not block a clean commit, and a staged orphan that is deleted
/// from the working tree blocks the commit.
#[cfg(unix)]
#[test]
fn the_generated_hook_audits_the_staged_index() {
    let tmp = fixture();
    let dir = tmp.path();
    let install = crate::common::run_fallow_raw(&[
        "--root",
        dir.to_str().unwrap(),
        "hooks",
        "install",
        "--target",
        "git",
    ]);
    assert_eq!(install.code, 0, "stderr: {}", install.stderr);
    let bin = TempDir::new().unwrap();
    std::os::unix::fs::symlink(fallow_bin(), bin.path().join("fallow")).unwrap();

    fs::write(
        dir.join("src/index.ts"),
        "export const a = 1;\nexport const b = a;\n",
    )
    .unwrap();
    git(dir, &["add", "src/index.ts"]);
    fs::write(dir.join("src/untracked-orphan.ts"), "export const z = 3;\n").unwrap();
    let clean = commit_through_hook(dir, bin.path(), "edit index");
    assert!(
        clean.status.success(),
        "an untracked file must not block the commit: {}",
        String::from_utf8_lossy(&clean.stderr)
    );

    fs::remove_file(dir.join("src/untracked-orphan.ts")).unwrap();
    fs::write(dir.join("src/staged-orphan.ts"), "export const q = 9;\n").unwrap();
    git(dir, &["add", "src/staged-orphan.ts"]);
    fs::remove_file(dir.join("src/staged-orphan.ts")).unwrap();
    let blocked = commit_through_hook(dir, bin.path(), "add staged orphan");
    assert_eq!(
        blocked.status.code(),
        Some(1),
        "a staged orphan must block the commit: {}",
        String::from_utf8_lossy(&blocked.stderr)
    );
}

/// The `cache_hits` and `cache_misses` of the head pass, from the
/// `--performance` JSON on stderr.
fn parse_cache_counts(stderr: &str) -> (u64, u64) {
    let perf = stderr
        .lines()
        .find_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .unwrap_or_else(|| panic!("no performance JSON on stderr: {stderr}"));
    let count = |key: &str| {
        perf[key]
            .as_u64()
            .unwrap_or_else(|| panic!("no {key} in {perf}"))
    };
    (count("cache_hits"), count("cache_misses"))
}

/// The unused files of a `fallow dead-code` JSON report.
fn working_tree_unused_files(json: &serde_json::Value) -> Vec<String> {
    unused_file_paths(&serde_json::json!({ "dead_code": json }))
}

/// Run `fallow dead-code` on the working tree of `dir`. `cache_dir` is the
/// `FALLOW_CACHE_DIR` of the run.
fn dead_code(dir: &Path, extra: &[&str], cache_dir: Option<&Path>) -> CommandOutput {
    let mut command = Command::new(fallow_bin());
    command.env("RUST_LOG", "").env("NO_COLOR", "1");
    scrub_analysis_env(&mut command);
    command.env_remove("FALLOW_CACHE_DIR");
    if let Some(cache_dir) = cache_dir {
        command.env("FALLOW_CACHE_DIR", cache_dir);
    }
    command
        .args(["dead-code", "--root"])
        .arg(dir)
        .args(["--format", "json", "--quiet"])
        .args(extra);
    run(&mut command)
}

/// The staged worktree is new on each run, so its own cache directory is
/// always empty. A staged run reads and updates the parse cache of the
/// checkout, so a second run reparses no file (issue #4763).
#[test]
fn a_second_staged_run_reads_the_parse_cache_of_the_checkout() {
    let tmp = fixture();
    let dir = tmp.path();
    fs::write(dir.join("src/b.ts"), "export const b = 1;\n").unwrap();
    commit_all(dir, "add b");
    fs::write(dir.join("src/index.ts"), "export const a = 2;\n").unwrap();
    git(dir, &["add", "src/index.ts"]);

    let first = run(audit_staged_command(dir, &["--performance"]).env_remove("FALLOW_CACHE_DIR"));
    assert!(first.code <= 1, "stderr: {}", first.stderr);
    assert!(
        dir.join(".fallow/cache.bin").is_file(),
        "the staged run writes the parse cache of the checkout"
    );

    let second = run(audit_staged_command(dir, &["--performance"]).env_remove("FALLOW_CACHE_DIR"));
    assert!(second.code <= 1, "stderr: {}", second.stderr);
    let (hits, misses) = parse_cache_counts(&second.stderr);
    assert_eq!(misses, 0, "stderr: {}", second.stderr);
    assert!(hits > 0, "stderr: {}", second.stderr);
}

/// The parse cache is keyed by content. A staged file whose content differs
/// from the working tree is parsed from the index, and the next working-tree
/// run parses the working-tree content again.
#[test]
fn the_parse_cache_follows_the_staged_content_not_the_working_tree() {
    let tmp = fixture();
    let dir = tmp.path();
    fs::write(dir.join("src/b.ts"), "export const b = 1;\n").unwrap();
    fs::write(
        dir.join("src/index.ts"),
        "import { b } from './b';\nexport const a = b;\n",
    )
    .unwrap();
    git(dir, &["add", "src/index.ts", "src/b.ts"]);
    fs::write(dir.join("src/index.ts"), "export const a = 1;\n").unwrap();

    let warm = dead_code(dir, &[], None);
    assert_eq!(
        working_tree_unused_files(&parse_json(&warm)),
        vec!["src/b.ts".to_owned()],
        "the working tree does not import b: {}",
        warm.stdout
    );

    for _ in 0..2 {
        let staged = run(audit_staged_command(dir, &[]).env_remove("FALLOW_CACHE_DIR"));
        let json = parse_json(&staged);
        assert!(
            unused_file_paths(&json).is_empty(),
            "the staged index imports b: {json}"
        );
    }

    let after = dead_code(dir, &[], None);
    assert_eq!(
        working_tree_unused_files(&parse_json(&after)),
        vec!["src/b.ts".to_owned()],
        "the staged entry must not replace the working-tree analysis: {}",
        after.stdout
    );
}

/// The module graph cache records the project root, and the staged worktree
/// has a new root on each run. A staged run with a shared `FALLOW_CACHE_DIR`
/// must not replace the graph cache of the checkout.
#[test]
fn a_staged_run_keeps_the_graph_cache_of_the_checkout() {
    let tmp = fixture();
    let dir = tmp.path();
    let cache = TempDir::new().unwrap();
    fs::write(dir.join("src/index.ts"), "export const a = 2;\n").unwrap();
    git(dir, &["add", "src/index.ts"]);

    let warm = dead_code(dir, &[], Some(cache.path()));
    assert!(warm.code <= 1, "stderr: {}", warm.stderr);

    let staged = run(audit_staged_command(dir, &[]).env("FALLOW_CACHE_DIR", cache.path()));
    assert!(staged.code <= 1, "stderr: {}", staged.stderr);

    let after = dead_code(dir, &["--performance"], Some(cache.path()));
    let perf = after
        .stderr
        .lines()
        .find_map(|line| serde_json::from_str::<serde_json::Value>(line).ok())
        .unwrap_or_else(|| panic!("no performance JSON: {}", after.stderr));
    assert!(
        perf.get("graph_cache_rejection")
            .is_none_or(serde_json::Value::is_null),
        "the working-tree graph cache must stay valid: {perf}"
    );
    assert_eq!(perf["cache_misses"], 0, "{perf}");
}

/// A staged file and its working-tree version with the same length but
/// another import. The cached entry of the working tree has the same size, so
/// only the content hash tells the two apart.
#[test]
fn an_equal_length_staged_edit_is_parsed_from_the_index() {
    let tmp = fixture();
    let dir = tmp.path();
    fs::write(dir.join("src/b.ts"), "export const b = 1;\n").unwrap();
    fs::write(dir.join("src/c.ts"), "export const b = 2;\n").unwrap();
    fs::write(
        dir.join("src/index.ts"),
        "import { b } from './b';\nexport const a = b;\n",
    )
    .unwrap();
    git(dir, &["add", "src/index.ts", "src/b.ts", "src/c.ts"]);
    fs::write(
        dir.join("src/index.ts"),
        "import { b } from './c';\nexport const a = b;\n",
    )
    .unwrap();

    let warm = dead_code(dir, &[], None);
    assert_eq!(
        working_tree_unused_files(&parse_json(&warm)),
        vec!["src/b.ts".to_owned()],
        "{}",
        warm.stdout
    );

    for _ in 0..2 {
        let staged = run(audit_staged_command(dir, &[]).env_remove("FALLOW_CACHE_DIR"));
        let json = parse_json(&staged);
        assert_eq!(
            unused_file_paths(&json),
            vec!["src/c.ts".to_owned()],
            "the staged index imports b: {json}"
        );
    }

    let after = dead_code(dir, &[], None);
    assert_eq!(
        working_tree_unused_files(&parse_json(&after)),
        vec!["src/b.ts".to_owned()],
        "{}",
        after.stdout
    );
}

#[test]
fn a_staged_run_with_no_cache_writes_no_parse_cache() {
    let tmp = fixture();
    let dir = tmp.path();
    fs::write(dir.join("src/index.ts"), "export const a = 2;\n").unwrap();
    git(dir, &["add", "src/index.ts"]);

    let output = run(audit_staged_command(dir, &["--no-cache"]).env_remove("FALLOW_CACHE_DIR"));
    assert_eq!(output.code, 0, "stderr: {}", output.stderr);
    assert!(!dir.join(".fallow/cache.bin").exists());
}

/// The staged worktree holds committed files. A committed `.fallow/cache.bin`
/// symlink to a path outside the worktree must not receive the parse cache of
/// the checkout.
#[cfg(unix)]
#[test]
fn a_committed_cache_symlink_does_not_receive_the_parse_cache() {
    let tmp = fixture();
    let dir = tmp.path();
    let outside = TempDir::new().unwrap();
    let target = outside.path().join("written-through-link");
    fs::create_dir_all(dir.join(".fallow")).unwrap();
    std::os::unix::fs::symlink(&target, dir.join(".fallow/cache.bin")).unwrap();
    commit_all(dir, "commit a cache symlink");

    // The checkout gets a real parse cache. The index still holds the link.
    fs::remove_file(dir.join(".fallow/cache.bin")).unwrap();
    let warm = dead_code(dir, &[], None);
    assert!(warm.code <= 1, "stderr: {}", warm.stderr);
    assert!(
        fs::symlink_metadata(dir.join(".fallow/cache.bin"))
            .unwrap()
            .is_file()
    );
    fs::write(dir.join("src/index.ts"), "export const a = 2;\n").unwrap();
    git(dir, &["add", "src/index.ts"]);

    let staged = run(audit_staged_command(dir, &[]).env_remove("FALLOW_CACHE_DIR"));
    assert!(staged.code <= 1, "stderr: {}", staged.stderr);
    assert!(
        !target.exists(),
        "the staged run wrote through the committed symlink"
    );
}
