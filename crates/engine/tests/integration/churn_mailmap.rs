//! Churn history reads author emails through the repository mailmap.

#![allow(
    clippy::expect_used,
    reason = "integration tests use expect to keep fixture setup concise"
)]

use std::path::{Path, PathBuf};
use std::process::Command;

use fallow_engine::churn::{ChurnResult, ChurnWindowUnit, SinceDuration, analyze_churn_cached};

const OLD_EMAIL: &str = "old@corp.test";
const NEW_EMAIL: &str = "new@corp.test";

fn window() -> SinceDuration {
    SinceDuration::relative(1, ChurnWindowUnit::Years, "1 year")
}

fn git(root: &Path, args: &[&str]) {
    let mut command = Command::new("git");
    fallow_engine::changed_files::clear_ambient_git_env(&mut command);
    let status = command
        .current_dir(root)
        .args(args)
        .status()
        .expect("run git");
    assert!(status.success(), "git {args:?} failed");
}

/// A repository where one person committed `src/a.ts` with two emails.
fn two_email_repo(dir: &tempfile::TempDir) -> PathBuf {
    let root = dir.path().canonicalize().expect("canonical fixture root");
    git(&root, &["init", "--quiet", "--initial-branch=main"]);
    git(&root, &["config", "user.name", "Churn Fixture"]);
    git(&root, &["config", "commit.gpgsign", "false"]);
    std::fs::create_dir_all(root.join("src")).expect("create source directory");
    for (email, contents) in [
        (OLD_EMAIL, "export const a = 1;\n"),
        (NEW_EMAIL, "export const a = 2;\n"),
    ] {
        std::fs::write(root.join("src/a.ts"), contents).expect("write source file");
        git(&root, &["add", "src/a.ts"]);
        let identity = format!("user.email={email}");
        git(&root, &["-c", &identity, "commit", "--quiet", "-m", email]);
    }
    root
}

fn write_mailmap(root: &Path) {
    let line = format!("Churn Fixture <{NEW_EMAIL}> <{OLD_EMAIL}>\n");
    std::fs::write(root.join(".mailmap"), line).expect("write mailmap");
}

fn churn(root: &Path, cache_dir: &Path, no_cache: bool) -> (ChurnResult, bool) {
    analyze_churn_cached(root, &window(), cache_dir, no_cache).expect("churn analysis")
}

fn authors(result: &ChurnResult) -> Vec<String> {
    let mut pool = result.author_pool.clone();
    pool.sort();
    pool
}

#[test]
fn churn_merges_mailmap_aliases_into_one_author() {
    let dir = tempfile::tempdir().expect("repo tempdir");
    let cache = tempfile::tempdir().expect("cache tempdir");
    let root = two_email_repo(&dir);
    write_mailmap(&root);

    let (result, _) = churn(&root, cache.path(), true);

    assert_eq!(authors(&result), vec![NEW_EMAIL.to_string()]);
}

#[test]
fn churn_cache_is_rejected_after_a_mailmap_edit() {
    let dir = tempfile::tempdir().expect("repo tempdir");
    let cache = tempfile::tempdir().expect("cache tempdir");
    let root = two_email_repo(&dir);

    let (cold, cold_hit) = churn(&root, cache.path(), false);
    assert!(!cold_hit);
    assert_eq!(
        authors(&cold),
        vec![NEW_EMAIL.to_string(), OLD_EMAIL.to_string()]
    );
    let (_, warm_hit) = churn(&root, cache.path(), false);
    assert!(warm_hit);

    write_mailmap(&root);
    let (mapped, mapped_hit) = churn(&root, cache.path(), false);

    assert!(!mapped_hit, "a mailmap edit must reject the cache entry");
    assert_eq!(authors(&mapped), vec![NEW_EMAIL.to_string()]);
}
