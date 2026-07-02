use super::*;
use std::{fs, process::Command};

fn git(dir: &std::path::Path, args: &[&str]) {
    let output = Command::new("git")
        .args(args)
        .current_dir(dir)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env("GIT_CONFIG_SYSTEM", "/dev/null")
        .env("GIT_AUTHOR_NAME", "test")
        .env("GIT_AUTHOR_EMAIL", "test@test.com")
        .env("GIT_COMMITTER_NAME", "test")
        .env("GIT_COMMITTER_EMAIL", "test@test.com")
        .output()
        .expect("git command failed");
    assert!(
        output.status.success(),
        "git {:?} failed\nstdout:\n{}\nstderr:\n{}",
        args,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn audit_worktree_helpers_filter_to_fallow_temp_prefix() {
    let temp = std::env::temp_dir();
    let audit_path = temp.join("fallow-audit-base-123-456");
    let reusable_path = temp.join("fallow-audit-base-cache-abcd-1234");
    let canonical_audit_path = temp
        .canonicalize()
        .unwrap_or_else(|_| temp.clone())
        .join("fallow-audit-base-456-789");
    let unrelated_temp = temp.join("other-worktree");
    let output = format!(
        "worktree /repo\nHEAD abc\n\nworktree {}\nHEAD def\n\nworktree {}\nHEAD ghi\n\nworktree {}\nHEAD jkl\n",
        audit_path.display(),
        unrelated_temp.display(),
        reusable_path.display()
    );

    assert_eq!(
        parse_worktree_list(&output),
        vec![audit_path, reusable_path.clone()]
    );
    assert!(is_fallow_audit_worktree_path(&canonical_audit_path));
    assert!(is_reusable_audit_worktree_path(&reusable_path));
    assert_eq!(audit_worktree_pid("fallow-audit-base-123-456"), Some(123));
    assert_eq!(
        audit_worktree_pid("fallow-audit-base-cache-abcd-1234"),
        None
    );
    assert_eq!(audit_worktree_pid("not-fallow-audit-base-123"), None);
}

/// Initialize a throwaway git repo with a single commit and return its root.
/// Used by the worktree-lifecycle tests below as a parent repo that can host
/// `git worktree add` invocations.
fn init_throwaway_repo(parent: &std::path::Path, name: &str) -> PathBuf {
    let root = parent.join(name);
    fs::create_dir_all(&root).expect("repo root should be created");
    fs::write(root.join("README.md"), "seed\n").expect("seed file should be written");
    git(&root, &["init", "-b", "main"]);
    git(&root, &["add", "."]);
    git(
        &root,
        &["-c", "commit.gpgsign=false", "commit", "-m", "initial"],
    );
    root
}

/// Add a tracked file and commit it; return the new HEAD SHA.
fn commit_file(repo: &std::path::Path, name: &str, body: &str) -> String {
    fs::write(repo.join(name), body).expect("file should be written");
    git(repo, &["add", "."]);
    git(repo, &["-c", "commit.gpgsign=false", "commit", "-m", name]);
    git_rev_parse(repo, "HEAD").expect("HEAD should resolve")
}

#[test]
fn auto_detect_base_ref_resolves_origin_default_to_merge_base() {
    let tmp = tempfile::TempDir::new().expect("temp dir should be created");
    let repo = init_throwaway_repo(tmp.path(), "repo");
    let head = git_rev_parse(&repo, "HEAD").expect("HEAD should resolve");
    git(&repo, &["branch", "trunk"]);
    git(&repo, &["update-ref", "refs/remotes/origin/trunk", "trunk"]);
    git(
        &repo,
        &[
            "symbolic-ref",
            "refs/remotes/origin/HEAD",
            "refs/remotes/origin/trunk",
        ],
    );

    let detected = auto_detect_base_ref(&repo).expect("base should be detected");
    // trunk == HEAD, so the merge-base is HEAD's own SHA (the bare branch
    // name `trunk` is no longer returned: it would resolve to a local ref).
    assert_eq!(detected.git_ref, head);
    assert_eq!(detected.description, "merge-base with origin/trunk");
}

/// Regression for issue #1168: a worktree checkout whose local `main` is
/// stale relative to a fresh `origin/main`. The base must be the fork point
/// (merge-base with `origin/main`), NOT the stale local-`main` commit that
/// the old bare-name resolution diffed against.
#[test]
fn auto_detect_base_ref_ignores_stale_local_main() {
    let tmp = tempfile::TempDir::new().expect("temp dir should be created");
    let repo = init_throwaway_repo(tmp.path(), "repo");
    let stale = git_rev_parse(&repo, "HEAD").expect("HEAD should resolve");

    // origin/main starts at the first commit, then a teammate advances it.
    git(&repo, &["update-ref", "refs/remotes/origin/main", "main"]);
    git(
        &repo,
        &[
            "symbolic-ref",
            "refs/remotes/origin/HEAD",
            "refs/remotes/origin/main",
        ],
    );
    let fork_point = commit_file(&repo, "teammate.txt", "merged work\n");
    git(&repo, &["update-ref", "refs/remotes/origin/main", "main"]);

    // Cut a feature branch from the fresh origin tip using the raw SHA (no
    // upstream tracking), then leave local `main` behind at the stale commit.
    git(&repo, &["checkout", "-b", "feature", &fork_point]);
    commit_file(&repo, "feature.txt", "my change\n");
    git(&repo, &["branch", "-f", "main", &stale]);

    let detected = auto_detect_base_ref(&repo).expect("base should be detected");
    assert_eq!(
        detected.git_ref, fork_point,
        "base must be the fork point (origin/main), not stale local main"
    );
    assert_ne!(
        detected.git_ref, stale,
        "must not diff against stale local main"
    );
    assert_eq!(detected.description, "merge-base with origin/main");
}

#[test]
fn auto_detect_base_ref_prefers_configured_upstream() {
    let tmp = tempfile::TempDir::new().expect("temp dir should be created");
    let repo = init_throwaway_repo(tmp.path(), "repo");
    let fork_point = git_rev_parse(&repo, "HEAD").expect("HEAD should resolve");
    // Configure `origin` so refs/remotes/origin/* are recognized as tracking
    // refs and `--set-upstream-to` is accepted.
    git(&repo, &["remote", "add", "origin", &repo.to_string_lossy()]);
    git(&repo, &["update-ref", "refs/remotes/origin/main", "main"]);

    git(&repo, &["checkout", "-b", "feature"]);
    git(
        &repo,
        &["branch", "--set-upstream-to=origin/main", "feature"],
    );
    commit_file(&repo, "feature.txt", "my change\n");

    let detected = auto_detect_base_ref(&repo).expect("base should be detected");
    assert_eq!(detected.git_ref, fork_point);
    assert_eq!(detected.description, "merge-base with origin/main");
}

#[test]
fn auto_detect_base_ref_falls_back_to_local_main_without_remote() {
    let tmp = tempfile::TempDir::new().expect("temp dir should be created");
    let repo = init_throwaway_repo(tmp.path(), "repo");

    let detected = auto_detect_base_ref(&repo).expect("base should be detected");
    assert_eq!(detected.git_ref, "main");
    assert_eq!(detected.description, "local main");
}

#[test]
fn auto_detect_base_ref_falls_back_to_local_master_without_remote() {
    let tmp = tempfile::TempDir::new().expect("temp dir should be created");
    let repo = tmp.path().join("repo");
    fs::create_dir_all(&repo).expect("repo root should be created");
    fs::write(repo.join("README.md"), "seed\n").expect("seed file should be written");
    git(&repo, &["init", "-b", "master"]);
    git(&repo, &["add", "."]);
    git(
        &repo,
        &["-c", "commit.gpgsign=false", "commit", "-m", "initial"],
    );

    let detected = auto_detect_base_ref(&repo).expect("base should be detected");
    assert_eq!(detected.git_ref, "master");
    assert_eq!(detected.description, "local master");
}

#[test]
fn auto_detect_base_ref_returns_none_outside_git_repo() {
    let tmp = tempfile::TempDir::new().expect("temp dir should be created");

    assert!(auto_detect_base_ref(tmp.path()).is_none());
}

#[test]
fn parse_audit_base_override_trims_and_rejects_empty() {
    assert_eq!(parse_audit_base_override(None), None);
    assert_eq!(parse_audit_base_override(Some(String::new())), None);
    assert_eq!(parse_audit_base_override(Some("   ".to_string())), None);
    assert_eq!(
        parse_audit_base_override(Some("  origin/main  ".to_string())),
        Some("origin/main".to_string())
    );
}

/// When the remote default shares no history with HEAD (the merge-base
/// failure case a shallow clone also hits), auto-detect falls back to the
/// remote-tracking ref tip rather than failing detection.
#[test]
fn auto_detect_base_ref_falls_back_to_remote_tip_without_common_ancestor() {
    let tmp = tempfile::TempDir::new().expect("temp dir should be created");
    let repo = init_throwaway_repo(tmp.path(), "repo");
    // Build an unrelated-history commit and point origin/main at it, so
    // merge-base(origin/main, HEAD) has no common ancestor.
    git(&repo, &["checkout", "--orphan", "unrelated"]);
    commit_file(&repo, "unrelated.txt", "no shared history\n");
    let unrelated = git_rev_parse(&repo, "HEAD").expect("HEAD should resolve");
    git(
        &repo,
        &["update-ref", "refs/remotes/origin/main", &unrelated],
    );
    git(
        &repo,
        &[
            "symbolic-ref",
            "refs/remotes/origin/HEAD",
            "refs/remotes/origin/main",
        ],
    );
    git(&repo, &["checkout", "main"]);

    let detected = auto_detect_base_ref(&repo).expect("base should be detected");
    assert_eq!(detected.git_ref, "origin/main");
    assert_eq!(detected.description, "origin/main (tip)");
}

#[test]
fn get_head_sha_returns_short_head_for_git_repo() {
    let tmp = tempfile::TempDir::new().expect("temp dir should be created");
    let repo = init_throwaway_repo(tmp.path(), "repo");
    let output = Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .current_dir(&repo)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .output()
        .expect("git rev-parse should run");
    assert!(output.status.success());

    assert_eq!(
        get_head_sha(&repo),
        Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
    );
}

#[test]
fn get_head_sha_returns_none_outside_git_repo() {
    let tmp = tempfile::TempDir::new().expect("temp dir should be created");

    assert_eq!(get_head_sha(tmp.path()), None);
}

fn worktree_is_registered_with_git(repo_root: &std::path::Path, worktree_path: &Path) -> bool {
    list_audit_worktrees(repo_root)
        .is_some_and(|paths| paths.iter().any(|p| paths_equal(p, worktree_path)))
}

/// True when `git worktree list --porcelain` still carries an admin entry
/// whose path ends with `worktree_path`'s basename. Unlike
/// `worktree_is_registered_with_git`, this matches by basename against the
/// raw porcelain output, so it stays correct even when the directory has
/// been deleted (a prunable orphan): `paths_equal` canonicalization cannot
/// match a missing path across the macOS `/var` -> `/private/var` symlink,
/// but the unique nanos-suffixed basename is stable.
fn worktree_admin_entry_present(repo_root: &std::path::Path, worktree_path: &Path) -> bool {
    let basename = worktree_path
        .file_name()
        .and_then(|n| n.to_str())
        .expect("reusable worktree path has a utf-8 basename");
    let output = Command::new("git")
        .args(["worktree", "list", "--porcelain"])
        .current_dir(repo_root)
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .output()
        .expect("git worktree list should run");
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| line.strip_prefix("worktree "))
        .any(|p| p.ends_with(basename))
}

#[test]
fn worktree_cleanup_guard_runs_on_drop() {
    let tmp = tempfile::TempDir::new().expect("temp dir should be created");
    let repo = init_throwaway_repo(tmp.path(), "repo");
    let worktree_path = tmp.path().join("fallow-audit-base-1234-5678");

    git(
        &repo,
        &[
            "worktree",
            "add",
            "--detach",
            "--quiet",
            worktree_path.to_str().expect("path is utf-8"),
            "HEAD",
        ],
    );
    assert!(worktree_path.is_dir());
    assert!(worktree_is_registered_with_git(&repo, &worktree_path));

    {
        let _guard = WorktreeCleanupGuard::new(&repo, &worktree_path);
    }

    assert!(
        !worktree_path.exists(),
        "guard Drop should remove the worktree directory",
    );
    assert!(
        !worktree_is_registered_with_git(&repo, &worktree_path),
        "guard Drop should remove the git worktree registration",
    );
}

#[test]
fn worktree_cleanup_guard_defused_skips_drop() {
    let tmp = tempfile::TempDir::new().expect("temp dir should be created");
    let repo = init_throwaway_repo(tmp.path(), "repo");
    let worktree_path = tmp.path().join("fallow-audit-base-1234-5679");

    git(
        &repo,
        &[
            "worktree",
            "add",
            "--detach",
            "--quiet",
            worktree_path.to_str().expect("path is utf-8"),
            "HEAD",
        ],
    );
    assert!(worktree_path.is_dir());

    {
        let mut guard = WorktreeCleanupGuard::new(&repo, &worktree_path);
        guard.defuse();
        guard.defuse();
    }

    assert!(
        worktree_path.is_dir(),
        "defused guard must not remove the worktree on drop",
    );
    assert!(
        worktree_is_registered_with_git(&repo, &worktree_path),
        "defused guard must not unregister the worktree from git",
    );

    remove_audit_worktree(&repo, &worktree_path);
    let _ = fs::remove_dir_all(&worktree_path);
}

#[test]
fn audit_orphan_sweep_removes_dead_pid_worktree() {
    const DEAD_PID: u32 = 99_999_999;
    assert!(!process_is_alive(DEAD_PID));

    let tmp = tempfile::TempDir::new().expect("temp dir should be created");
    let repo = init_throwaway_repo(tmp.path(), "repo");

    let worktree_path = std::env::temp_dir().join(format!(
        "fallow-audit-base-{}-{}",
        DEAD_PID,
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock should be after epoch")
            .as_nanos()
    ));
    git(
        &repo,
        &[
            "worktree",
            "add",
            "--detach",
            "--quiet",
            worktree_path.to_str().expect("path is utf-8"),
            "HEAD",
        ],
    );
    assert!(worktree_path.is_dir());
    assert!(worktree_is_registered_with_git(&repo, &worktree_path));

    sweep_orphan_audit_worktrees(&repo);

    assert!(
        !worktree_path.exists(),
        "sweep should remove worktree owned by a dead PID",
    );
    assert!(
        !worktree_is_registered_with_git(&repo, &worktree_path),
        "sweep should unregister worktree owned by a dead PID",
    );
}

#[test]
fn audit_orphan_sweep_keeps_live_pid_worktree() {
    let live_pid = std::process::id();
    assert!(process_is_alive(live_pid));

    let tmp = tempfile::TempDir::new().expect("temp dir should be created");
    let repo = init_throwaway_repo(tmp.path(), "repo");

    let worktree_path = std::env::temp_dir().join(format!(
        "fallow-audit-base-{}-{}",
        live_pid,
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock should be after epoch")
            .as_nanos()
    ));
    git(
        &repo,
        &[
            "worktree",
            "add",
            "--detach",
            "--quiet",
            worktree_path.to_str().expect("path is utf-8"),
            "HEAD",
        ],
    );

    sweep_orphan_audit_worktrees(&repo);

    assert!(
        worktree_path.is_dir(),
        "sweep must not remove worktree owned by a live PID",
    );
    assert!(
        worktree_is_registered_with_git(&repo, &worktree_path),
        "sweep must not unregister worktree owned by a live PID",
    );

    remove_audit_worktree(&repo, &worktree_path);
    let _ = fs::remove_dir_all(&worktree_path);
}

/// Build a reusable-shaped worktree path inside the system tempdir
/// (so `is_reusable_audit_worktree_path` and `path_is_inside_temp_dir`
/// both match), uniquified by nanos so parallel tests do not collide.
fn make_reusable_path(label: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("clock should be after epoch")
        .as_nanos();
    std::env::temp_dir().join(format!("fallow-audit-base-cache-{label}-{nanos:032x}"))
}

/// Register a worktree with the parent repo at `path` checked out at HEAD.
/// Mirrors what `BaseWorktree::reuse_or_create` does for the fresh-create
/// path so the GC sweep tests can build real cache entries.
fn register_reusable_worktree(repo: &Path, path: &Path) {
    git(
        repo,
        &[
            "worktree",
            "add",
            "--detach",
            "--quiet",
            path.to_str().expect("path is utf-8"),
            "HEAD",
        ],
    );
}

fn write_sidecar_with_age(path: &Path, age: Duration) {
    let sidecar = reusable_worktree_last_used_path(path);
    let file = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(&sidecar)
        .expect("sidecar should open");
    let when = SystemTime::now()
        .checked_sub(age)
        .expect("backdated time should fit in SystemTime");
    file.set_modified(when)
        .expect("set_modified should succeed");
}

/// Tear down a reusable worktree (git registration + dir + sidecar + lock)
/// regardless of which of those the test created. Idempotent.
fn cleanup_reusable_worktree(repo: &Path, path: &Path) {
    remove_audit_worktree(repo, path);
    let _ = fs::remove_dir_all(path);
    let _ = fs::remove_file(reusable_worktree_last_used_path(path));
    let _ = fs::remove_file(reusable_worktree_lock_path(path));
}

#[test]
fn reusable_cache_gc_removes_old_entry_with_backdated_sidecar() {
    let tmp = tempfile::TempDir::new().expect("temp dir should be created");
    let repo = init_throwaway_repo(tmp.path(), "repo-gc-remove");
    let worktree_path = make_reusable_path("gc-remove");
    register_reusable_worktree(&repo, &worktree_path);
    write_sidecar_with_age(&worktree_path, Duration::from_hours(31 * 24));

    sweep_old_reusable_caches(&repo, Some(Duration::from_hours(30 * 24)), true);

    assert!(
        !worktree_path.exists(),
        "sweep should remove worktree dir whose sidecar is older than the threshold",
    );
    assert!(
        !worktree_is_registered_with_git(&repo, &worktree_path),
        "sweep should unregister the worktree from git",
    );
    assert!(
        !reusable_worktree_last_used_path(&worktree_path).exists(),
        "sweep should remove the sidecar `.last-used` file alongside the worktree",
    );
    cleanup_reusable_worktree(&repo, &worktree_path);
}

#[test]
fn reusable_cache_gc_keeps_fresh_entry() {
    let tmp = tempfile::TempDir::new().expect("temp dir should be created");
    let repo = init_throwaway_repo(tmp.path(), "repo-gc-keep");
    let worktree_path = make_reusable_path("gc-keep");
    register_reusable_worktree(&repo, &worktree_path);
    write_sidecar_with_age(&worktree_path, Duration::from_mins(1));

    sweep_old_reusable_caches(&repo, Some(Duration::from_hours(30 * 24)), true);

    assert!(
        worktree_path.is_dir(),
        "sweep must not remove a worktree whose sidecar is fresher than the threshold",
    );
    assert!(
        worktree_is_registered_with_git(&repo, &worktree_path),
        "sweep must not unregister a fresh worktree",
    );
    cleanup_reusable_worktree(&repo, &worktree_path);
}

#[test]
fn reusable_cache_gc_skips_locked_entry() {
    let tmp = tempfile::TempDir::new().expect("temp dir should be created");
    let repo = init_throwaway_repo(tmp.path(), "repo-gc-locked");
    let worktree_path = make_reusable_path("gc-locked");
    register_reusable_worktree(&repo, &worktree_path);
    write_sidecar_with_age(&worktree_path, Duration::from_hours(31 * 24));

    let lock = ReusableWorktreeLock::try_acquire(&worktree_path)
        .expect("test should acquire the lock first");

    sweep_old_reusable_caches(&repo, Some(Duration::from_hours(30 * 24)), true);

    assert!(
        worktree_path.is_dir(),
        "sweep must skip a locked entry even when its sidecar is stale",
    );
    assert!(
        worktree_is_registered_with_git(&repo, &worktree_path),
        "sweep must not unregister a locked entry",
    );
    drop(lock);
    cleanup_reusable_worktree(&repo, &worktree_path);
}

#[test]
fn reusable_cache_gc_grace_when_sidecar_absent() {
    let tmp = tempfile::TempDir::new().expect("temp dir should be created");
    let repo = init_throwaway_repo(tmp.path(), "repo-gc-grace");
    let worktree_path = make_reusable_path("gc-grace");
    register_reusable_worktree(&repo, &worktree_path);
    let sidecar = reusable_worktree_last_used_path(&worktree_path);
    assert!(
        !sidecar.exists(),
        "test pre-condition: sidecar should not exist",
    );

    sweep_old_reusable_caches(&repo, Some(Duration::from_hours(30 * 24)), true);

    assert!(
        worktree_path.is_dir(),
        "pre-upgrade grace: sidecar-absent entries must NOT be removed on first encounter",
    );
    assert!(
        sidecar.exists(),
        "pre-upgrade grace: sidecar must be seeded so the next run can age from real last-used",
    );
    let mtime = std::fs::metadata(&sidecar)
        .and_then(|m| m.modified())
        .expect("seeded sidecar should have a readable mtime");
    let age = SystemTime::now()
        .duration_since(mtime)
        .unwrap_or(Duration::ZERO);
    assert!(
        age < Duration::from_mins(1),
        "seeded sidecar mtime should be near `now()`, got age {age:?}",
    );
    cleanup_reusable_worktree(&repo, &worktree_path);
}

#[test]
fn reusable_cache_gc_reclaims_prunable_orphan_when_dir_missing() {
    let tmp = tempfile::TempDir::new().expect("temp dir should be created");
    let repo = init_throwaway_repo(tmp.path(), "repo-gc-orphan");
    let worktree_path = make_reusable_path("gc-orphan");
    register_reusable_worktree(&repo, &worktree_path);
    // Fresh sidecar: the age branch alone would KEEP this entry, so a
    // successful reclaim proves the dir-missing branch drove it.
    write_sidecar_with_age(&worktree_path, Duration::from_mins(1));
    let sidecar = reusable_worktree_last_used_path(&worktree_path);

    // Simulate an external temp-reaper: delete only the worktree directory,
    // leaving git's admin entry and the sidecar behind.
    fs::remove_dir_all(&worktree_path).expect("test should remove the cache dir");
    assert!(
        !worktree_path.exists(),
        "test pre-condition: cache dir should be gone",
    );
    assert!(
        worktree_admin_entry_present(&repo, &worktree_path),
        "test pre-condition: git admin entry should still be registered (prunable)",
    );
    assert!(
        sidecar.exists(),
        "test pre-condition: sidecar survives a dir-only reaper",
    );

    sweep_old_reusable_caches(&repo, Some(Duration::from_hours(30 * 24)), true);

    assert!(
        !worktree_admin_entry_present(&repo, &worktree_path),
        "sweep should unregister a prunable orphan whose dir was externally removed",
    );
    assert!(
        !sidecar.exists(),
        "sweep should remove the stale sidecar for a reclaimed orphan",
    );
    cleanup_reusable_worktree(&repo, &worktree_path);
}

#[test]
fn reusable_cache_gc_reclaims_prunable_orphan_even_when_age_gc_disabled() {
    let tmp = tempfile::TempDir::new().expect("temp dir should be created");
    let repo = init_throwaway_repo(tmp.path(), "repo-gc-orphan-nogc");
    let worktree_path = make_reusable_path("gc-orphan-nogc");
    register_reusable_worktree(&repo, &worktree_path);
    write_sidecar_with_age(&worktree_path, Duration::from_mins(1));
    let sidecar = reusable_worktree_last_used_path(&worktree_path);
    fs::remove_dir_all(&worktree_path).expect("test should remove the cache dir");
    assert!(
        worktree_admin_entry_present(&repo, &worktree_path),
        "test pre-condition: git admin entry should still be registered (prunable)",
    );
    assert!(
        sidecar.exists(),
        "test pre-condition: sidecar survives a dir-only reaper",
    );

    // `None` = age-based GC disabled (`cacheMaxAgeDays = 0`). Orphan reclaim
    // must still run so dead admin entries do not accumulate forever.
    sweep_old_reusable_caches(&repo, None, true);

    assert!(
        !worktree_admin_entry_present(&repo, &worktree_path),
        "orphan reclaim must run even when age-based GC is disabled",
    );
    assert!(
        !sidecar.exists(),
        "sweep should remove the stale sidecar even when age-based GC is disabled",
    );
    cleanup_reusable_worktree(&repo, &worktree_path);
}

#[test]
fn reusable_cache_gc_preserves_lock_file_after_removal() {
    let tmp = tempfile::TempDir::new().expect("temp dir should be created");
    let repo = init_throwaway_repo(tmp.path(), "repo-gc-lockfile");
    let worktree_path = make_reusable_path("gc-lockfile");
    register_reusable_worktree(&repo, &worktree_path);
    write_sidecar_with_age(&worktree_path, Duration::from_hours(31 * 24));
    let lock_path = reusable_worktree_lock_path(&worktree_path);
    drop(ReusableWorktreeLock::try_acquire(&worktree_path).expect("test should acquire the lock"));
    assert!(
        lock_path.exists(),
        "test pre-condition: lock file should exist before sweep",
    );

    sweep_old_reusable_caches(&repo, Some(Duration::from_hours(30 * 24)), true);

    assert!(
        !worktree_path.exists(),
        "sweep should still remove the worktree directory",
    );
    assert!(
        lock_path.exists(),
        "sweep MUST NOT delete the `.lock` file (lock-lifecycle invariant)",
    );
    let _ = fs::remove_file(&lock_path);
    cleanup_reusable_worktree(&repo, &worktree_path);
}

#[test]
fn reuse_or_create_stamps_sidecar_on_fresh_create() {
    let tmp = tempfile::TempDir::new().expect("temp dir should be created");
    let repo = init_throwaway_repo(tmp.path(), "repo-fresh-create-stamp");
    let base_sha = git_rev_parse(&repo, "HEAD").expect("HEAD should resolve");

    let worktree = BaseWorktree::reuse_or_create(&repo, &base_sha)
        .expect("fresh reuse_or_create should succeed on a clean repo");
    let cache_path = worktree.path().to_path_buf();
    let sidecar = reusable_worktree_last_used_path(&cache_path);

    assert!(
        sidecar.exists(),
        "fresh-create must write the sidecar so age is measured from now",
    );
    let initial_age = std::fs::metadata(&sidecar)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|mtime| SystemTime::now().duration_since(mtime).ok())
        .expect("sidecar mtime should be readable and not in the future");
    assert!(
        initial_age < Duration::from_mins(1),
        "fresh-create sidecar mtime should be near now(), got age {initial_age:?}",
    );

    drop(worktree);
    cleanup_reusable_worktree(&repo, &cache_path);
}

#[test]
fn days_to_duration_zero_disables() {
    assert!(days_to_duration(0).is_none());
    assert_eq!(days_to_duration(1), Some(Duration::from_hours(24)));
    assert_eq!(days_to_duration(30), Some(Duration::from_hours(30 * 24)));
}

#[test]
fn reusable_worktree_last_used_path_lives_next_to_cache_dir() {
    let cache_dir = std::env::temp_dir().join("fallow-audit-base-cache-abcd-1234");
    let sidecar = reusable_worktree_last_used_path(&cache_dir);
    assert_eq!(sidecar.parent(), cache_dir.parent());
    assert_eq!(
        sidecar.file_name().and_then(|s| s.to_str()),
        Some("fallow-audit-base-cache-abcd-1234.last-used"),
    );
}

#[test]
fn touch_last_used_creates_sidecar_if_missing() {
    let tmp = tempfile::TempDir::new().expect("temp dir should be created");
    let cache_dir = tmp.path().join("fallow-audit-base-cache-touchtest-0000");
    fs::create_dir(&cache_dir).expect("cache dir should be created");
    let sidecar = reusable_worktree_last_used_path(&cache_dir);
    assert!(!sidecar.exists(), "sidecar should not exist before touch");

    touch_last_used(&cache_dir);

    assert!(sidecar.exists(), "touch should create the sidecar");
    let mtime = fs::metadata(&sidecar)
        .and_then(|m| m.modified())
        .expect("sidecar should have an mtime");
    let age = SystemTime::now()
        .duration_since(mtime)
        .unwrap_or(Duration::ZERO);
    assert!(
        age < Duration::from_mins(1),
        "touched sidecar should be near `now()`",
    );
}

#[test]
fn reusable_worktree_lock_excludes_concurrent_acquires() {
    let tmp = tempfile::TempDir::new().expect("temp dir should be created");
    let reusable = tmp.path().join("fallow-audit-base-cache-deadbeef-0000");
    let lock_path = reusable_worktree_lock_path(&reusable);

    let first = ReusableWorktreeLock::try_acquire(&reusable)
        .expect("first acquire on a fresh path should succeed");
    assert!(
        ReusableWorktreeLock::try_acquire(&reusable).is_none(),
        "second acquire must fail while the first is held",
    );
    drop(first);
    assert!(
        lock_path.exists(),
        "lock file must persist after drop (only the kernel lock is released)",
    );
}

#[test]
fn base_analysis_root_preserves_repo_subdirectory_roots() {
    let tmp = tempfile::TempDir::new().expect("temp dir should be created");
    let repo = tmp.path().join("repo");
    let app_root = repo.join("apps/mobile");
    let base_worktree = tmp.path().join("base-worktree");
    fs::create_dir_all(&app_root).expect("app root should be created");
    fs::create_dir_all(&base_worktree).expect("base worktree should be created");
    git(&repo, &["init", "-b", "main"]);

    assert_eq!(
        base_analysis_root(&app_root, &base_worktree),
        base_worktree.join("apps/mobile")
    );
}

#[test]
fn audit_base_worktree_reuses_current_node_modules_context() {
    let tmp = tempfile::TempDir::new().expect("temp dir should be created");
    let root = tmp.path();
    fs::create_dir_all(root.join("src")).expect("src dir should be created");
    fs::write(root.join(".gitignore"), "node_modules\n.fallow\n")
        .expect("gitignore should be written");
    fs::write(
            root.join("package.json"),
            r#"{"name":"audit-rn-alias","main":"src/index.ts","dependencies":{"@react-native/typescript-config":"1.0.0"}}"#,
        )
        .expect("package.json should be written");
    fs::write(
            root.join("tsconfig.json"),
            r#"{"extends":"./node_modules/@react-native/typescript-config/tsconfig.json","compilerOptions":{"baseUrl":".","paths":{"@/*":["src/*"]}},"include":["src"]}"#,
        )
        .expect("tsconfig should be written");
    fs::write(
        root.join("src/index.ts"),
        "import { used } from '@/feature';\nconsole.log(used);\n",
    )
    .expect("index should be written");
    fs::write(root.join("src/feature.ts"), "export const used = 1;\n")
        .expect("feature should be written");

    git(root, &["init", "-b", "main"]);
    git(root, &["add", "."]);
    git(
        root,
        &["-c", "commit.gpgsign=false", "commit", "-m", "initial"],
    );

    let rn_config = root.join("node_modules/@react-native/typescript-config");
    fs::create_dir_all(&rn_config).expect("node_modules config dir should be created");
    fs::write(
        rn_config.join("tsconfig.json"),
        r#"{"compilerOptions":{"jsx":"react-native","moduleResolution":"bundler"}}"#,
    )
    .expect("node_modules tsconfig should be written");

    let worktree =
        BaseWorktree::create(root, "HEAD", None).expect("base worktree should be created");
    assert!(
        worktree.path().join("node_modules").is_dir(),
        "base worktree should reuse ignored node_modules from the current checkout"
    );
    assert!(
        worktree
            .path()
            .join("node_modules/@react-native/typescript-config/tsconfig.json")
            .is_file(),
        "base worktree should preserve tsconfig extends targets installed in node_modules"
    );
}

/// Confirms `materialize_base_dependency_context` symlinks the Nuxt
/// `.nuxt/` generated dir from the host checkout into the audit base
/// worktree. Without this, root `tsconfig.json` `references` entries
/// pointing into `.nuxt/tsconfig.app.json` break in the base pass and
/// emit "Nuxt project missing .nuxt/tsconfig.json" plus "Broken tsconfig
/// chain" warnings. The function is exercised directly here rather than
/// through `BaseWorktree::create` to avoid the `git worktree add`
/// concurrency-flakiness the worktree-level integration tests already
/// exhibit.
#[test]
fn materialize_base_dependency_context_symlinks_nuxt_generated_dir() {
    let host = tempfile::TempDir::new().expect("host tempdir should be created");
    let worktree = tempfile::TempDir::new().expect("worktree tempdir should be created");

    let dot_nuxt = host.path().join(".nuxt");
    fs::create_dir_all(&dot_nuxt).expect(".nuxt dir should be created");
    fs::write(dot_nuxt.join("tsconfig.json"), r#"{"compilerOptions":{}}"#)
        .expect(".nuxt/tsconfig.json should be written");
    fs::write(
        dot_nuxt.join("tsconfig.app.json"),
        r#"{"compilerOptions":{}}"#,
    )
    .expect(".nuxt/tsconfig.app.json should be written");

    materialize_base_dependency_context(host.path(), worktree.path());

    let mirrored = worktree.path().join(".nuxt");
    assert!(
        mirrored.is_dir(),
        "base worktree should reuse the ignored .nuxt dir from the host checkout"
    );
    let link_meta = fs::symlink_metadata(&mirrored)
        .expect(".nuxt entry should exist as a symlink in the worktree");
    assert!(
        link_meta.file_type().is_symlink(),
        "base worktree's .nuxt should be a symlink to the host checkout"
    );
    assert!(
        mirrored.join("tsconfig.json").is_file(),
        "base worktree should expose .nuxt/tsconfig.json so the Nuxt meta-framework \
             prerequisite check stays quiet"
    );
    assert!(
        mirrored.join("tsconfig.app.json").is_file(),
        "base worktree should expose .nuxt/tsconfig.app.json so root tsconfig references \
             resolve without falling back to resolver-less resolution"
    );
}

/// Confirms the same symlink treatment for Astro's `.astro/` generated
/// types directory, which is gitignored by default and would otherwise
/// trip the "Astro project missing .astro/" prerequisite check on the
/// base pass.
#[test]
fn materialize_base_dependency_context_symlinks_astro_generated_dir() {
    let host = tempfile::TempDir::new().expect("host tempdir should be created");
    let worktree = tempfile::TempDir::new().expect("worktree tempdir should be created");

    let dot_astro = host.path().join(".astro");
    fs::create_dir_all(&dot_astro).expect(".astro dir should be created");
    fs::write(dot_astro.join("types.d.ts"), "// generated types\n")
        .expect(".astro/types.d.ts should be written");

    materialize_base_dependency_context(host.path(), worktree.path());

    let mirrored = worktree.path().join(".astro");
    assert!(
        mirrored.is_dir(),
        "base worktree should reuse the ignored .astro dir from the host checkout"
    );
    assert!(
        mirrored.join("types.d.ts").is_file(),
        "base worktree should expose generated Astro types so the Astro meta-framework \
             prerequisite check stays quiet"
    );
}

/// Confirms the symlink step is a no-op when the host checkout has no
/// meta-framework output. We must not fabricate a dangling `.nuxt`
/// symlink: the Nuxt prerequisite check would then pass on the base pass
/// while the actual `.nuxt/tsconfig.json` still doesn't exist, hiding a
/// real "run `nuxt prepare`" warning on the HEAD pass behind a
/// process-wide dedupe key.
#[test]
fn materialize_base_dependency_context_skips_when_host_lacks_meta_framework_dir() {
    let host = tempfile::TempDir::new().expect("host tempdir should be created");
    let worktree = tempfile::TempDir::new().expect("worktree tempdir should be created");

    materialize_base_dependency_context(host.path(), worktree.path());

    assert!(
        !worktree.path().join(".nuxt").exists(),
        "base worktree should not fabricate a .nuxt symlink when the host has no .nuxt dir"
    );
    assert!(
        !worktree.path().join(".astro").exists(),
        "base worktree should not fabricate a .astro symlink when the host has no .astro dir"
    );
    assert!(
        !worktree.path().join("node_modules").exists(),
        "base worktree should not fabricate a node_modules symlink when the host has none"
    );
}

/// Confirms each entry in `MATERIALIZED_CONTEXT_DIRS` is independent: a
/// missing host `.nuxt/` must not prevent `node_modules` from being
/// symlinked when only one of the two is present on the host.
#[test]
fn materialize_base_dependency_context_handles_each_dir_independently() {
    let host = tempfile::TempDir::new().expect("host tempdir should be created");
    let worktree = tempfile::TempDir::new().expect("worktree tempdir should be created");

    fs::create_dir_all(host.path().join("node_modules"))
        .expect("host node_modules should be created");

    materialize_base_dependency_context(host.path(), worktree.path());

    assert!(
        worktree.path().join("node_modules").is_dir(),
        "node_modules should still be symlinked even when host has no .nuxt or .astro"
    );
    assert!(
        !worktree.path().join(".nuxt").exists(),
        "missing host .nuxt should leave the worktree slot empty"
    );
}

/// Confirms a real (non-symlink) generated dir already present in the base
/// worktree is preserved, not clobbered by a host symlink. A base commit
/// that genuinely tracks `.nuxt/` is base-shaped and authoritative; the
/// host-symlink shortcut only fills the gap when the worktree slot is
/// empty (or a stale dangling symlink), so the `destination.is_dir()`
/// early-continue must keep the worktree's own contents.
#[test]
fn materialize_base_dependency_context_preserves_real_worktree_dir() {
    let host = tempfile::TempDir::new().expect("host tempdir should be created");
    let worktree = tempfile::TempDir::new().expect("worktree tempdir should be created");

    let host_nuxt = host.path().join(".nuxt");
    fs::create_dir_all(&host_nuxt).expect("host .nuxt dir should be created");
    fs::write(host_nuxt.join("tsconfig.json"), r#"{"_source":"host"}"#)
        .expect("host .nuxt/tsconfig.json should be written");

    let worktree_nuxt = worktree.path().join(".nuxt");
    fs::create_dir_all(&worktree_nuxt).expect("worktree .nuxt dir should be created");
    fs::write(worktree_nuxt.join("tsconfig.json"), r#"{"_source":"base"}"#)
        .expect("worktree .nuxt/tsconfig.json should be written");

    materialize_base_dependency_context(host.path(), worktree.path());

    let link_meta = fs::symlink_metadata(&worktree_nuxt)
        .expect(".nuxt entry should still exist in the worktree");
    assert!(
        !link_meta.file_type().is_symlink(),
        "a real base-tracked .nuxt dir must not be replaced by a host symlink"
    );
    let contents =
        fs::read_to_string(worktree_nuxt.join("tsconfig.json")).expect("tsconfig should read");
    assert!(
        contents.contains("base"),
        "base worktree's own .nuxt contents must survive, not be overwritten by the host's"
    );
}

#[test]
fn audit_reusable_base_worktree_refreshes_current_node_modules_context() {
    let tmp = tempfile::TempDir::new().expect("temp dir should be created");
    let root = tmp.path();
    fs::write(root.join(".gitignore"), "node_modules\n.fallow\n")
        .expect("gitignore should be written");
    fs::write(root.join("package.json"), r#"{"name":"audit-reusable"}"#)
        .expect("package.json should be written");

    git(root, &["init", "-b", "main"]);
    git(root, &["add", "."]);
    git(
        root,
        &["-c", "commit.gpgsign=false", "commit", "-m", "initial"],
    );

    let rn_config = root.join("node_modules/@react-native/typescript-config");
    fs::create_dir_all(&rn_config).expect("node_modules config dir should be created");
    fs::write(rn_config.join("tsconfig.json"), "{}")
        .expect("node_modules tsconfig should be written");

    let base_sha = git_rev_parse(root, "HEAD").expect("HEAD should resolve");
    let first = BaseWorktree::reuse_or_create(root, &base_sha)
        .expect("persistent base worktree should be created");
    let worktree_path = first.path().to_path_buf();
    assert!(
        worktree_path.join("node_modules").is_dir(),
        "initial persistent worktree should receive node_modules context"
    );
    remove_node_modules_context(&worktree_path);
    assert!(
        !worktree_path.join("node_modules").exists(),
        "test setup should remove the dependency context from the reusable worktree"
    );
    drop(first);

    let reused = BaseWorktree::reuse_or_create(root, &base_sha)
        .expect("ready persistent base worktree should be reused");
    assert_eq!(reused.path(), worktree_path.as_path());
    assert!(
        reused.path().join("node_modules").is_dir(),
        "ready persistent worktree should refresh missing node_modules context"
    );

    cleanup_reusable_worktree(root, reused.path());
}

fn remove_node_modules_context(worktree_path: &Path) {
    let path = worktree_path.join("node_modules");
    let Ok(metadata) = fs::symlink_metadata(&path) else {
        return;
    };
    if metadata.file_type().is_symlink() {
        #[cfg(unix)]
        let _ = fs::remove_file(path);
        #[cfg(windows)]
        let _ = fs::remove_dir(&path).or_else(|_| fs::remove_file(&path));
    } else {
        let _ = fs::remove_dir_all(path);
    }
}

#[test]
fn audit_base_snapshot_cache_payload_roundtrips_sets() {
    let key = AuditBaseSnapshotCacheKey {
        hash: 42,
        base_sha: "abc123".to_string(),
    };
    let snapshot = AuditKeySnapshot {
        dead_code: ["dead:a".to_string(), "dead:b".to_string()]
            .into_iter()
            .collect(),
        health: std::iter::once("health:a".to_string()).collect(),
        styling: std::iter::once("styling:a".to_string()).collect(),
        dupes: ["dupe:a".to_string(), "dupe:b".to_string()]
            .into_iter()
            .collect(),
        boundary_edges: std::iter::once("ui->-db".to_string()).collect(),
        cycles: std::iter::once("a.ts|b.ts".to_string()).collect(),
        public_api: std::iter::once("src/index.ts::foo".to_string()).collect(),
    };

    let cached = cached_from_snapshot(&key, &snapshot);
    assert_eq!(cached.version, AUDIT_BASE_SNAPSHOT_CACHE_VERSION);
    assert_eq!(cached.key_hash, key.hash);
    assert_eq!(cached.base_sha, key.base_sha);
    assert_eq!(cached.dead_code, vec!["dead:a", "dead:b"]);

    let decoded = snapshot_from_cached(cached);
    assert_eq!(decoded.dead_code, snapshot.dead_code);
    assert_eq!(decoded.health, snapshot.health);
    assert_eq!(decoded.styling, snapshot.styling);
    assert_eq!(decoded.dupes, snapshot.dupes);
    assert_eq!(decoded.boundary_edges, snapshot.boundary_edges);
    assert_eq!(decoded.cycles, snapshot.cycles);
    assert_eq!(decoded.public_api, snapshot.public_api);
}

#[test]
fn audit_base_snapshot_cache_dir_writes_gitignore() {
    let tmp = tempfile::TempDir::new().expect("temp dir should be created");
    let cache_root = tmp.path().join(".custom-fallow-cache");
    let cache_dir = audit_base_snapshot_cache_dir(&cache_root);

    ensure_audit_base_snapshot_cache_dir(&cache_dir).expect("cache dir should be created");

    assert_eq!(
        fs::read_to_string(cache_dir.join(".gitignore")).expect("gitignore should read"),
        "*\n"
    );
}

#[test]
fn audit_base_snapshot_cache_roundtrips_from_disk() {
    let tmp = tempfile::TempDir::new().expect("temp dir should be created");
    let config_path = None;
    let cache_root = tmp.path().join(".custom-fallow-cache");
    let opts = AuditOptions {
        root: tmp.path(),
        cache_dir: &cache_root,
        config_path: &config_path,
        output: OutputFormat::Json,
        no_cache: false,
        threads: 1,
        quiet: true,
        changed_since: Some("HEAD"),
        production: false,
        production_dead_code: None,
        production_health: None,
        production_dupes: None,
        workspace: None,
        changed_workspaces: None,
        explain: false,
        explain_skipped: false,
        performance: false,
        group_by: None,
        dead_code_baseline: None,
        health_baseline: None,
        dupes_baseline: None,
        max_crap: None,
        coverage: None,
        coverage_root: None,
        gate: AuditGate::NewOnly,
        include_entry_exports: false,
        css: false,
        css_deep: false,
        runtime_coverage: None,
        min_invocations_hot: 100,
        brief: false,
        max_decisions: 4,
        walkthrough_guide: false,
        walkthrough: false,
        mark_viewed: &[],
        show_cleared: false,
        walkthrough_file: None,
        show_deprioritized: false,
    };
    let key = AuditBaseSnapshotCacheKey {
        hash: 0xfeed,
        base_sha: "abc123".to_string(),
    };
    let snapshot = AuditKeySnapshot {
        dead_code: std::iter::once("dead:a".to_string()).collect(),
        health: std::iter::once("health:a".to_string()).collect(),
        styling: std::iter::once("styling:a".to_string()).collect(),
        dupes: std::iter::once("dupe:a".to_string()).collect(),
        boundary_edges: FxHashSet::default(),
        cycles: FxHashSet::default(),
        public_api: FxHashSet::default(),
    };

    save_cached_base_snapshot(&opts, &key, &snapshot);
    assert!(
        audit_base_snapshot_cache_file(&cache_root, &key).exists(),
        "snapshot should be saved below the configured cache directory"
    );
    let loaded = load_cached_base_snapshot(&opts, &key).expect("snapshot should load");

    assert_eq!(loaded.dead_code, snapshot.dead_code);
    assert_eq!(loaded.health, snapshot.health);
    assert_eq!(loaded.dupes, snapshot.dupes);
}

#[test]
fn audit_base_snapshot_cache_rejects_mismatched_key() {
    let tmp = tempfile::TempDir::new().expect("temp dir should be created");
    let config_path = None;
    let cache_root = tmp.path().join(".custom-fallow-cache");
    let opts = AuditOptions {
        root: tmp.path(),
        cache_dir: &cache_root,
        config_path: &config_path,
        output: OutputFormat::Json,
        no_cache: false,
        threads: 1,
        quiet: true,
        changed_since: Some("HEAD"),
        production: false,
        production_dead_code: None,
        production_health: None,
        production_dupes: None,
        workspace: None,
        changed_workspaces: None,
        explain: false,
        explain_skipped: false,
        performance: false,
        group_by: None,
        dead_code_baseline: None,
        health_baseline: None,
        dupes_baseline: None,
        max_crap: None,
        coverage: None,
        coverage_root: None,
        gate: AuditGate::NewOnly,
        include_entry_exports: false,
        css: false,
        css_deep: false,
        runtime_coverage: None,
        min_invocations_hot: 100,
        brief: false,
        max_decisions: 4,
        walkthrough_guide: false,
        walkthrough: false,
        mark_viewed: &[],
        show_cleared: false,
        walkthrough_file: None,
        show_deprioritized: false,
    };
    let key = AuditBaseSnapshotCacheKey {
        hash: 0xbeef,
        base_sha: "head".to_string(),
    };
    let cached = CachedAuditKeySnapshot {
        version: AUDIT_BASE_SNAPSHOT_CACHE_VERSION,
        cli_version: env!("CARGO_PKG_VERSION").to_string(),
        key_hash: key.hash,
        base_sha: "other".to_string(),
        dead_code: vec!["dead:a".to_string()],
        health: vec![],
        styling: vec![],
        dupes: vec![],
        boundary_edges: vec![],
        cycles: vec![],
        public_api: vec![],
    };
    let cache_dir = audit_base_snapshot_cache_dir(&cache_root);
    ensure_audit_base_snapshot_cache_dir(&cache_dir).expect("cache dir should be created");
    fs::write(
        audit_base_snapshot_cache_file(&cache_root, &key),
        bitcode::encode(&cached),
    )
    .expect("cache file should be written");

    assert!(load_cached_base_snapshot(&opts, &key).is_none());
}

#[test]
fn audit_base_snapshot_cache_key_includes_extended_config() {
    let tmp = tempfile::TempDir::new().expect("temp dir should be created");
    let root = tmp.path();
    fs::write(
        root.join(".fallowrc.json"),
        r#"{"extends":"base.json","entry":["src/index.ts"]}"#,
    )
    .expect("config should be written");
    fs::write(
        root.join("base.json"),
        r#"{"rules":{"unused-exports":"off"}}"#,
    )
    .expect("base config should be written");

    let config_path = None;
    let cache_root = root.join(".fallow");
    let opts = AuditOptions {
        root,
        cache_dir: &cache_root,
        config_path: &config_path,
        output: OutputFormat::Json,
        no_cache: false,
        threads: 1,
        quiet: true,
        changed_since: Some("HEAD"),
        production: false,
        production_dead_code: None,
        production_health: None,
        production_dupes: None,
        workspace: None,
        changed_workspaces: None,
        explain: false,
        explain_skipped: false,
        performance: false,
        group_by: None,
        dead_code_baseline: None,
        health_baseline: None,
        dupes_baseline: None,
        max_crap: None,
        coverage: None,
        coverage_root: None,
        gate: AuditGate::NewOnly,
        include_entry_exports: false,
        css: false,
        css_deep: false,
        runtime_coverage: None,
        min_invocations_hot: 100,
        brief: false,
        max_decisions: 4,
        walkthrough_guide: false,
        walkthrough: false,
        mark_viewed: &[],
        show_cleared: false,
        walkthrough_file: None,
        show_deprioritized: false,
    };

    let first = config_file_fingerprint(&opts).expect("fingerprint should be computed");
    fs::write(
        root.join("base.json"),
        r#"{"rules":{"unused-exports":"error"}}"#,
    )
    .expect("base config should be updated");
    let second = config_file_fingerprint(&opts).expect("fingerprint should be recomputed");

    assert_ne!(
        first.resolved_hash, second.resolved_hash,
        "extended config changes must invalidate cached base snapshots"
    );
}

#[test]
fn audit_gate_all_skips_base_snapshot() {
    let tmp = tempfile::TempDir::new().expect("temp dir should be created");
    let root = tmp.path();
    fs::create_dir_all(root.join("src")).expect("src dir should be created");
    fs::write(
        root.join("package.json"),
        r#"{"name":"audit-gate-all","main":"src/index.ts"}"#,
    )
    .expect("package.json should be written");
    fs::write(root.join("src/index.ts"), "export const legacy = 1;\n")
        .expect("index should be written");

    git(root, &["init", "-b", "main"]);
    git(root, &["add", "."]);
    git(
        root,
        &["-c", "commit.gpgsign=false", "commit", "-m", "initial"],
    );
    fs::write(
        root.join("src/index.ts"),
        "export const legacy = 1;\nexport const changed = 2;\n",
    )
    .expect("changed module should be written");

    let config_path = None;
    let cache_root = root.join(".fallow");
    let opts = AuditOptions {
        root,
        cache_dir: &cache_root,
        config_path: &config_path,
        output: OutputFormat::Json,
        no_cache: true,
        threads: 1,
        quiet: true,
        changed_since: Some("HEAD"),
        production: false,
        production_dead_code: None,
        production_health: None,
        production_dupes: None,
        workspace: None,
        changed_workspaces: None,
        explain: false,
        explain_skipped: false,
        performance: false,
        group_by: None,
        dead_code_baseline: None,
        health_baseline: None,
        dupes_baseline: None,
        max_crap: None,
        coverage: None,
        coverage_root: None,
        gate: AuditGate::All,
        include_entry_exports: false,
        css: false,
        css_deep: false,
        runtime_coverage: None,
        min_invocations_hot: 100,
        brief: false,
        max_decisions: 4,
        walkthrough_guide: false,
        walkthrough: false,
        mark_viewed: &[],
        show_cleared: false,
        walkthrough_file: None,
        show_deprioritized: false,
    };

    let result = execute_audit(&opts).expect("audit should execute");
    assert!(result.base_snapshot.is_none());
    assert_eq!(result.attribution.gate, AuditGate::All);
    assert_eq!(result.attribution.dead_code_introduced, 0);
    assert_eq!(result.attribution.dead_code_inherited, 0);
}

#[test]
fn audit_gate_new_only_skips_base_snapshot_for_docs_only_diff() {
    let tmp = tempfile::TempDir::new().expect("temp dir should be created");
    let root = tmp.path();
    fs::create_dir_all(root.join("src")).expect("src dir should be created");
    fs::write(
        root.join("package.json"),
        r#"{"name":"audit-docs-only","main":"src/index.ts"}"#,
    )
    .expect("package.json should be written");
    fs::write(
        root.join(".fallowrc.json"),
        r#"{"duplicates":{"minTokens":5,"minLines":2,"mode":"strict"}}"#,
    )
    .expect("config should be written");
    let duplicated = "export function same(input: number): number {\n  const doubled = input * 2;\n  const shifted = doubled + 1;\n  return shifted;\n}\n";
    fs::write(root.join("src/index.ts"), duplicated).expect("index should be written");
    fs::write(root.join("src/copy.ts"), duplicated).expect("copy should be written");
    fs::write(root.join("README.md"), "before\n").expect("readme should be written");

    git(root, &["init", "-b", "main"]);
    git(root, &["add", "."]);
    git(
        root,
        &["-c", "commit.gpgsign=false", "commit", "-m", "initial"],
    );
    fs::write(root.join("README.md"), "after\n").expect("readme should be modified");
    fs::create_dir_all(root.join(".fallow/cache/dupes-tokens-v2"))
        .expect("cache dir should be created");
    fs::write(
        root.join(".fallow/cache/dupes-tokens-v2/cache.bin"),
        b"cache",
    )
    .expect("cache artifact should be written");

    let before_worktrees = audit_worktree_names(root);

    let config_path = None;
    let cache_root = root.join(".fallow");
    let opts = AuditOptions {
        root,
        cache_dir: &cache_root,
        config_path: &config_path,
        output: OutputFormat::Json,
        no_cache: true,
        threads: 1,
        quiet: true,
        changed_since: Some("HEAD"),
        production: false,
        production_dead_code: None,
        production_health: None,
        production_dupes: None,
        workspace: None,
        changed_workspaces: None,
        explain: false,
        explain_skipped: false,
        performance: true,
        group_by: None,
        dead_code_baseline: None,
        health_baseline: None,
        dupes_baseline: None,
        max_crap: None,
        coverage: None,
        coverage_root: None,
        gate: AuditGate::NewOnly,
        include_entry_exports: false,
        css: false,
        css_deep: false,
        runtime_coverage: None,
        min_invocations_hot: 100,
        brief: false,
        max_decisions: 4,
        walkthrough_guide: false,
        walkthrough: false,
        mark_viewed: &[],
        show_cleared: false,
        walkthrough_file: None,
        show_deprioritized: false,
    };

    let result = execute_audit(&opts).expect("audit should execute");
    assert_eq!(result.verdict, AuditVerdict::Pass);
    assert_eq!(result.changed_files_count, 2);
    assert!(result.base_snapshot_skipped);
    assert!(result.base_snapshot.is_some());

    let after_worktrees = audit_worktree_names(root);
    assert_eq!(
        before_worktrees, after_worktrees,
        "base snapshot skip must not create a temporary base worktree"
    );
}

fn audit_worktree_names(repo_root: &Path) -> Vec<String> {
    let mut names: Vec<String> = list_audit_worktrees(repo_root)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .map(str::to_owned)
        })
        .collect();
    names.sort();
    names
}

#[test]
fn audit_reuses_dead_code_parse_for_health_when_production_matches() {
    let tmp = tempfile::TempDir::new().expect("temp dir should be created");
    let root = tmp.path();
    fs::create_dir_all(root.join("src")).expect("src dir should be created");
    fs::write(
        root.join("package.json"),
        r#"{"name":"audit-shared-parse","main":"src/index.ts"}"#,
    )
    .expect("package.json should be written");
    fs::write(
        root.join("src/index.ts"),
        "import { used } from './used';\nused();\n",
    )
    .expect("index should be written");
    fs::write(
        root.join("src/used.ts"),
        "export function used() {\n  return 1;\n}\n",
    )
    .expect("used module should be written");

    git(root, &["init", "-b", "main"]);
    git(root, &["add", "."]);
    git(
        root,
        &["-c", "commit.gpgsign=false", "commit", "-m", "initial"],
    );
    fs::write(
        root.join("src/used.ts"),
        "export function used() {\n  return 1;\n}\nexport function changed() {\n  return 2;\n}\n",
    )
    .expect("changed module should be written");

    let config_path = None;
    let cache_root = root.join(".fallow");
    let opts = AuditOptions {
        root,
        cache_dir: &cache_root,
        config_path: &config_path,
        output: OutputFormat::Json,
        no_cache: true,
        threads: 1,
        quiet: true,
        changed_since: Some("HEAD"),
        production: false,
        production_dead_code: None,
        production_health: None,
        production_dupes: None,
        workspace: None,
        changed_workspaces: None,
        explain: false,
        explain_skipped: false,
        performance: true,
        group_by: None,
        dead_code_baseline: None,
        health_baseline: None,
        dupes_baseline: None,
        max_crap: None,
        coverage: None,
        coverage_root: None,
        gate: AuditGate::NewOnly,
        include_entry_exports: false,
        css: false,
        css_deep: false,
        runtime_coverage: None,
        min_invocations_hot: 100,
        brief: false,
        max_decisions: 4,
        walkthrough_guide: false,
        walkthrough: false,
        mark_viewed: &[],
        show_cleared: false,
        walkthrough_file: None,
        show_deprioritized: false,
    };

    let result = execute_audit(&opts).expect("audit should execute");
    let health = result.health.expect("health should run for changed files");
    let timings = health.timings.expect("performance timings should be kept");
    assert!(timings.discover_ms.abs() < f64::EPSILON);
    assert!(timings.parse_ms.abs() < f64::EPSILON);
    assert!(
        result.dupes.is_some(),
        "dupes should run when changed files exist"
    );
}

#[test]
fn audit_dupes_falls_back_to_own_discovery_when_health_off() {
    let tmp = tempfile::TempDir::new().expect("temp dir should be created");
    let root = tmp.path();
    fs::create_dir_all(root.join("src")).expect("src dir should be created");
    fs::write(
        root.join("package.json"),
        r#"{"name":"audit-dupes-fallback","main":"src/index.ts"}"#,
    )
    .expect("package.json should be written");
    fs::write(
        root.join("src/index.ts"),
        "import { used } from './used';\nused();\n",
    )
    .expect("index should be written");
    fs::write(
        root.join("src/used.ts"),
        "export function used() {\n  return 1;\n}\n",
    )
    .expect("used module should be written");

    git(root, &["init", "-b", "main"]);
    git(root, &["add", "."]);
    git(
        root,
        &["-c", "commit.gpgsign=false", "commit", "-m", "initial"],
    );
    fs::write(
        root.join("src/used.ts"),
        "export function used() {\n  return 1;\n}\nexport function changed() {\n  return 2;\n}\n",
    )
    .expect("changed module should be written");

    let config_path = None;
    let cache_root = root.join(".fallow");
    let opts = AuditOptions {
        root,
        cache_dir: &cache_root,
        config_path: &config_path,
        output: OutputFormat::Json,
        no_cache: true,
        threads: 1,
        quiet: true,
        changed_since: Some("HEAD"),
        production: false,
        production_dead_code: Some(true),
        production_health: Some(false),
        production_dupes: Some(false),
        workspace: None,
        changed_workspaces: None,
        explain: false,
        explain_skipped: false,
        performance: true,
        group_by: None,
        dead_code_baseline: None,
        health_baseline: None,
        dupes_baseline: None,
        max_crap: None,
        coverage: None,
        coverage_root: None,
        gate: AuditGate::NewOnly,
        include_entry_exports: false,
        css: false,
        css_deep: false,
        runtime_coverage: None,
        min_invocations_hot: 100,
        brief: false,
        max_decisions: 4,
        walkthrough_guide: false,
        walkthrough: false,
        mark_viewed: &[],
        show_cleared: false,
        walkthrough_file: None,
        show_deprioritized: false,
    };

    let result = execute_audit(&opts).expect("audit should execute");
    assert!(result.dupes.is_some(), "dupes should still run");
}

#[cfg(unix)]
#[test]
fn remap_focus_files_does_not_canonicalize_through_symlinks() {
    let tmp = tempfile::TempDir::new().expect("temp dir");
    let real = tmp.path().join("real");
    let link = tmp.path().join("link");
    fs::create_dir_all(&real).expect("real dir");
    std::os::unix::fs::symlink(&real, &link).expect("symlink");
    let canonical = link.canonicalize().expect("canonicalize symlink");
    assert_ne!(link, canonical, "symlink should not equal its target");

    let from_root = PathBuf::from("/repo");
    let mut focus = FxHashSet::default();
    focus.insert(from_root.join("src/foo.ts"));

    let remapped = remap_focus_files(&focus, &from_root, &link)
        .expect("remap should succeed for in-prefix files");

    let expected = link.join("src/foo.ts");
    assert!(
        remapped.contains(&expected),
        "remapped paths must keep the un-canonical to_root prefix; got {remapped:?}, expected entry {expected:?}"
    );
}

#[test]
fn remap_focus_files_skips_paths_outside_from_root() {
    let from_root = PathBuf::from("/repo/apps/web");
    let to_root = PathBuf::from("/wt/apps/web");
    let mut focus = FxHashSet::default();
    focus.insert(PathBuf::from("/repo/apps/web/src/in.ts"));
    focus.insert(PathBuf::from("/repo/services/api/src/out.ts"));

    let remapped =
        remap_focus_files(&focus, &from_root, &to_root).expect("partial map should succeed");

    assert_eq!(remapped.len(), 1);
    assert!(remapped.contains(&PathBuf::from("/wt/apps/web/src/in.ts")));
}

#[test]
fn remap_focus_files_returns_none_when_no_paths_map() {
    let from_root = PathBuf::from("/repo/apps/web");
    let to_root = PathBuf::from("/wt/apps/web");
    let mut focus = FxHashSet::default();
    focus.insert(PathBuf::from("/elsewhere/foo.ts"));

    let remapped = remap_focus_files(&focus, &from_root, &to_root);
    assert!(
        remapped.is_none(),
        "remap should return None when no paths can be mapped, falling caller back to full corpus"
    );
}

#[test]
fn remap_cache_dir_moves_project_local_cache_to_base_worktree() {
    let tmp = tempfile::TempDir::new().expect("temp dir should be created");
    let current_root = tmp.path().join("repo");
    let base_root = tmp.path().join("fallow-base");
    let cache_dir = current_root.join(".cache").join("fallow");

    let remapped = remap_cache_dir_for_base_worktree(&current_root, &base_root, &cache_dir);

    assert_eq!(remapped, base_root.join(".cache").join("fallow"));
}

#[test]
fn remap_cache_dir_keeps_external_absolute_cache_shared() {
    let tmp = tempfile::TempDir::new().expect("temp dir should be created");
    let current_root = tmp.path().join("repo");
    let base_root = tmp.path().join("fallow-base");
    let cache_dir = tmp.path().join("shared").join("fallow-cache");

    let remapped = remap_cache_dir_for_base_worktree(&current_root, &base_root, &cache_dir);

    assert_eq!(remapped, cache_dir);
}

#[test]
fn audit_gate_new_only_inherits_pre_existing_duplicates_in_focused_files() {
    let tmp = tempfile::TempDir::new().expect("temp dir should be created");
    let root_buf = tmp
        .path()
        .canonicalize()
        .expect("temp root should canonicalize");
    let root = root_buf.as_path();
    fs::create_dir_all(root.join("src")).expect("src dir should be created");
    fs::write(
        root.join("package.json"),
        r#"{"name":"audit-newonly-inherit","main":"src/changed.ts"}"#,
    )
    .expect("package.json should be written");
    fs::write(
        root.join(".fallowrc.json"),
        r#"{"duplicates":{"minTokens":10,"minLines":3,"mode":"strict"}}"#,
    )
    .expect("config should be written");

    let dup_block = "export function processItems(input: number[]): number[] {\n  const doubled = input.map((value) => value * 2);\n  const filtered = doubled.filter((value) => value > 0);\n  const summed = filtered.reduce((acc, value) => acc + value, 0);\n  const shifted = summed + 10;\n  const scaled = shifted * 3;\n  const rounded = Math.round(scaled / 7);\n  return [rounded, scaled, summed];\n}\n";
    fs::write(root.join("src/changed.ts"), dup_block).expect("changed should be written");
    fs::write(root.join("src/peer.ts"), dup_block).expect("peer should be written");

    git(root, &["init", "-b", "main"]);
    git(root, &["add", "."]);
    git(
        root,
        &["-c", "commit.gpgsign=false", "commit", "-m", "initial"],
    );
    fs::write(
        root.join("src/changed.ts"),
        format!("{dup_block}// touched\n"),
    )
    .expect("changed file should be modified");
    git(root, &["add", "."]);
    git(
        root,
        &["-c", "commit.gpgsign=false", "commit", "-m", "touch"],
    );

    let config_path = None;
    let cache_root = root.join(".fallow");
    let opts = AuditOptions {
        root,
        cache_dir: &cache_root,
        config_path: &config_path,
        output: OutputFormat::Json,
        no_cache: true,
        threads: 1,
        quiet: true,
        changed_since: Some("HEAD~1"),
        production: false,
        production_dead_code: None,
        production_health: None,
        production_dupes: None,
        workspace: None,
        changed_workspaces: None,
        explain: false,
        explain_skipped: false,
        performance: false,
        group_by: None,
        dead_code_baseline: None,
        health_baseline: None,
        dupes_baseline: None,
        max_crap: None,
        coverage: None,
        coverage_root: None,
        gate: AuditGate::NewOnly,
        include_entry_exports: false,
        css: false,
        css_deep: false,
        runtime_coverage: None,
        min_invocations_hot: 100,
        brief: false,
        max_decisions: 4,
        walkthrough_guide: false,
        walkthrough: false,
        mark_viewed: &[],
        show_cleared: false,
        walkthrough_file: None,
        show_deprioritized: false,
    };

    let result = execute_audit(&opts).expect("audit should execute");
    assert!(
        result.base_snapshot_skipped,
        "comment-only JS/TS diffs should reuse current keys as the base snapshot"
    );
    let dupes_report = &result.dupes.as_ref().expect("dupes should run").report;
    assert!(
        !dupes_report.clone_groups.is_empty(),
        "current run should detect the pre-existing duplicate"
    );
    assert_eq!(
        result.attribution.duplication_introduced, 0,
        "pre-existing duplicate must not be classified as introduced; \
             attribution = {:?}",
        result.attribution
    );
    assert!(
        result.attribution.duplication_inherited > 0,
        "pre-existing duplicate must be classified as inherited; \
             attribution = {:?}",
        result.attribution
    );
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "test fixture; linear setup/assert, length is not a maintainability concern"
)]
fn audit_base_preserves_tsconfig_paths_when_extends_is_in_untracked_node_modules() {
    let tmp = tempfile::TempDir::new().expect("temp dir should be created");
    let root = tmp.path();
    fs::create_dir_all(root.join("src/screens")).expect("src dir should be created");
    fs::create_dir_all(root.join("node_modules/@react-native/typescript-config"))
        .expect("node_modules config dir should be created");
    fs::write(root.join(".gitignore"), "node_modules/\n").expect("gitignore should be written");
    fs::write(
        root.join("package.json"),
        r#"{
                "name": "audit-react-native-tsconfig-base",
                "private": true,
                "main": "src/App.tsx",
                "dependencies": {
                    "react-native": "0.80.0"
                }
            }"#,
    )
    .expect("package.json should be written");
    fs::write(
        root.join("tsconfig.json"),
        r#"{
                "extends": "./node_modules/@react-native/typescript-config/tsconfig.json",
                "compilerOptions": {
                    "baseUrl": ".",
                    "paths": {
                        "@/*": ["src/*"]
                    }
                },
                "include": ["src/**/*"]
            }"#,
    )
    .expect("tsconfig should be written");
    fs::write(
        root.join("node_modules/@react-native/typescript-config/tsconfig.json"),
        r#"{"compilerOptions":{"strict":true,"jsx":"react-jsx"}}"#,
    )
    .expect("react native tsconfig should be written");
    fs::write(
        root.join("src/App.tsx"),
        r#"import { homeTitle } from "@/screens/Home";

export function App() {
  return homeTitle;
}
"#,
    )
    .expect("app should be written");
    fs::write(
        root.join("src/screens/Home.ts"),
        r#"export const homeTitle = "home";
"#,
    )
    .expect("home should be written");

    git(root, &["init", "-b", "main"]);
    git(root, &["add", "."]);
    git(
        root,
        &["-c", "commit.gpgsign=false", "commit", "-m", "initial"],
    );
    fs::write(
        root.join("src/App.tsx"),
        r#"import { homeTitle } from "@/screens/Home";

export function App() {
  return homeTitle.toUpperCase();
}
"#,
    )
    .expect("app should be modified");

    let config_path = None;
    let cache_root = root.join(".fallow");
    let opts = AuditOptions {
        root,
        cache_dir: &cache_root,
        config_path: &config_path,
        output: OutputFormat::Json,
        no_cache: true,
        threads: 1,
        quiet: true,
        changed_since: Some("HEAD"),
        production: false,
        production_dead_code: None,
        production_health: None,
        production_dupes: None,
        workspace: None,
        changed_workspaces: None,
        explain: false,
        explain_skipped: false,
        performance: false,
        group_by: None,
        dead_code_baseline: None,
        health_baseline: None,
        dupes_baseline: None,
        max_crap: None,
        coverage: None,
        coverage_root: None,
        gate: AuditGate::NewOnly,
        include_entry_exports: false,
        css: false,
        css_deep: false,
        runtime_coverage: None,
        min_invocations_hot: 100,
        brief: false,
        max_decisions: 4,
        walkthrough_guide: false,
        walkthrough: false,
        mark_viewed: &[],
        show_cleared: false,
        walkthrough_file: None,
        show_deprioritized: false,
    };

    let result = execute_audit(&opts).expect("audit should execute");
    assert!(
        !result.base_snapshot_skipped,
        "source diffs should run a real base snapshot"
    );
    let base = result
        .base_snapshot
        .as_ref()
        .expect("base snapshot should run");
    assert!(
        !base
            .dead_code
            .contains("unresolved-import:src/App.tsx:@/screens/Home"),
        "base audit must keep local @/* tsconfig aliases when extends points into ignored node_modules: {:?}",
        base.dead_code
    );
    assert!(
        !base.dead_code.contains("unused-file:src/screens/Home.ts"),
        "alias target should stay reachable in the base worktree: {:?}",
        base.dead_code
    );
    let check = result.check.as_ref().expect("dead-code audit should run");
    assert!(
        check.results.unresolved_imports.is_empty(),
        "HEAD audit should also resolve @/* aliases: {:?}",
        check.results.unresolved_imports
    );
}

#[test]
#[expect(
    clippy::too_many_lines,
    reason = "test fixture; linear setup/assert, length is not a maintainability concern"
)]
fn audit_base_preserves_subdirectory_root_resolution() {
    let tmp = tempfile::TempDir::new().expect("temp dir should be created");
    let repo = tmp.path().join("repo");
    let root = repo.join("apps/mobile");
    fs::create_dir_all(root.join("src/screens")).expect("src dir should be created");
    fs::create_dir_all(root.join("node_modules/@react-native/typescript-config"))
        .expect("node_modules config dir should be created");
    fs::write(repo.join(".gitignore"), "apps/mobile/node_modules/\n")
        .expect("gitignore should be written");
    fs::write(
        root.join("package.json"),
        r#"{
                "name": "audit-subdir-react-native-tsconfig-base",
                "private": true,
                "main": "src/App.tsx",
                "dependencies": {
                    "react-native": "0.80.0"
                }
            }"#,
    )
    .expect("package.json should be written");
    fs::write(
        root.join("tsconfig.json"),
        r#"{
                "extends": "./node_modules/@react-native/typescript-config/tsconfig.json",
                "compilerOptions": {
                    "baseUrl": ".",
                    "paths": {
                        "@/*": ["src/*"]
                    }
                },
                "include": ["src/**/*"]
            }"#,
    )
    .expect("tsconfig should be written");
    fs::write(
        root.join("node_modules/@react-native/typescript-config/tsconfig.json"),
        r#"{"compilerOptions":{"strict":true,"jsx":"react-jsx"}}"#,
    )
    .expect("react native tsconfig should be written");
    fs::write(
        root.join("src/App.tsx"),
        r#"import { homeTitle } from "@/screens/Home";

export function App() {
  return homeTitle;
}
"#,
    )
    .expect("app should be written");
    fs::write(
        root.join("src/screens/Home.ts"),
        r#"export const homeTitle = "home";
"#,
    )
    .expect("home should be written");

    git(&repo, &["init", "-b", "main"]);
    git(&repo, &["add", "."]);
    git(
        &repo,
        &["-c", "commit.gpgsign=false", "commit", "-m", "initial"],
    );
    fs::write(
        root.join("src/App.tsx"),
        r#"import { homeTitle } from "@/screens/Home";

export function App() {
  return homeTitle.toUpperCase();
}
"#,
    )
    .expect("app should be modified");

    let config_path = None;
    let cache_root = root.join(".fallow");
    let opts = AuditOptions {
        root: &root,
        cache_dir: &cache_root,
        config_path: &config_path,
        output: OutputFormat::Json,
        no_cache: true,
        threads: 1,
        quiet: true,
        changed_since: Some("HEAD"),
        production: false,
        production_dead_code: None,
        production_health: None,
        production_dupes: None,
        workspace: None,
        changed_workspaces: None,
        explain: false,
        explain_skipped: false,
        performance: false,
        group_by: None,
        dead_code_baseline: None,
        health_baseline: None,
        dupes_baseline: None,
        max_crap: None,
        coverage: None,
        coverage_root: None,
        gate: AuditGate::NewOnly,
        include_entry_exports: false,
        css: false,
        css_deep: false,
        runtime_coverage: None,
        min_invocations_hot: 100,
        brief: false,
        max_decisions: 4,
        walkthrough_guide: false,
        walkthrough: false,
        mark_viewed: &[],
        show_cleared: false,
        walkthrough_file: None,
        show_deprioritized: false,
    };

    let result = execute_audit(&opts).expect("audit should execute");
    assert!(
        !result.base_snapshot_skipped,
        "source diffs should run a real base snapshot"
    );
    let base = result
        .base_snapshot
        .as_ref()
        .expect("base snapshot should run");
    assert!(
        !base
            .dead_code
            .contains("unresolved-import:src/App.tsx:@/screens/Home"),
        "base audit should analyze from the app subdirectory, not the repo root: {:?}",
        base.dead_code
    );
    assert!(
        !base.dead_code.contains("unused-file:src/screens/Home.ts"),
        "subdirectory base audit should keep alias targets reachable: {:?}",
        base.dead_code
    );
}

#[test]
fn audit_base_uses_new_explicit_config_without_hard_failure() {
    let tmp = tempfile::TempDir::new().expect("temp dir should be created");
    let root = tmp.path();
    fs::create_dir_all(root.join("src")).expect("src dir should be created");
    fs::write(
        root.join("package.json"),
        r#"{"name":"audit-new-config","main":"src/index.ts"}"#,
    )
    .expect("package.json should be written");
    fs::write(root.join("src/index.ts"), "export const used = 1;\n")
        .expect("index should be written");

    git(root, &["init", "-b", "main"]);
    git(root, &["add", "."]);
    git(
        root,
        &["-c", "commit.gpgsign=false", "commit", "-m", "initial"],
    );

    let explicit_config = root.join(".fallowrc.json");
    fs::write(&explicit_config, r#"{"rules":{"unused-files":"error"}}"#)
        .expect("new config should be written");
    fs::write(root.join("src/index.ts"), "export const used = 2;\n")
        .expect("index should be modified");

    let config_path = Some(explicit_config);
    let cache_root = root.join(".fallow");
    let opts = AuditOptions {
        root,
        cache_dir: &cache_root,
        config_path: &config_path,
        output: OutputFormat::Json,
        no_cache: true,
        threads: 1,
        quiet: true,
        changed_since: Some("HEAD"),
        production: false,
        production_dead_code: None,
        production_health: None,
        production_dupes: None,
        workspace: None,
        changed_workspaces: None,
        explain: false,
        explain_skipped: false,
        performance: false,
        group_by: None,
        dead_code_baseline: None,
        health_baseline: None,
        dupes_baseline: None,
        max_crap: None,
        coverage: None,
        coverage_root: None,
        gate: AuditGate::NewOnly,
        include_entry_exports: false,
        css: false,
        css_deep: false,
        runtime_coverage: None,
        min_invocations_hot: 100,
        brief: false,
        max_decisions: 4,
        walkthrough_guide: false,
        walkthrough: false,
        mark_viewed: &[],
        show_cleared: false,
        walkthrough_file: None,
        show_deprioritized: false,
    };

    let result = execute_audit(&opts).expect("audit should execute with a new explicit config");
    assert!(
        result.base_snapshot.is_some(),
        "base snapshot should use the current explicit config even when the base commit lacks it"
    );
}

#[test]
fn audit_base_uses_current_discovered_config_for_attribution() {
    let tmp = tempfile::TempDir::new().expect("temp dir should be created");
    let root = tmp.path();
    fs::create_dir_all(root.join("src")).expect("src dir should be created");
    fs::write(
            root.join("package.json"),
            r#"{"name":"audit-current-config","main":"src/index.ts","dependencies":{"left-pad":"1.3.0"}}"#,
        )
        .expect("package.json should be written");
    fs::write(
        root.join(".fallowrc.json"),
        r#"{"rules":{"unused-dependencies":"off"}}"#,
    )
    .expect("base config should be written");
    fs::write(root.join("src/index.ts"), "export const used = 1;\n")
        .expect("index should be written");

    git(root, &["init", "-b", "main"]);
    git(root, &["add", "."]);
    git(
        root,
        &["-c", "commit.gpgsign=false", "commit", "-m", "initial"],
    );

    fs::write(
        root.join(".fallowrc.json"),
        r#"{"rules":{"unused-dependencies":"error"}}"#,
    )
    .expect("current config should be written");
    fs::write(
            root.join("package.json"),
            r#"{"name":"audit-current-config","main":"src/index.ts","dependencies":{"left-pad":"1.3.1"}}"#,
        )
        .expect("package.json should be touched");

    let config_path = None;
    let cache_root = root.join(".fallow");
    let opts = AuditOptions {
        root,
        cache_dir: &cache_root,
        config_path: &config_path,
        output: OutputFormat::Json,
        no_cache: true,
        threads: 1,
        quiet: true,
        changed_since: Some("HEAD"),
        production: false,
        production_dead_code: None,
        production_health: None,
        production_dupes: None,
        workspace: None,
        changed_workspaces: None,
        explain: false,
        explain_skipped: false,
        performance: false,
        group_by: None,
        dead_code_baseline: None,
        health_baseline: None,
        dupes_baseline: None,
        max_crap: None,
        coverage: None,
        coverage_root: None,
        gate: AuditGate::NewOnly,
        include_entry_exports: false,
        css: false,
        css_deep: false,
        runtime_coverage: None,
        min_invocations_hot: 100,
        brief: false,
        max_decisions: 4,
        walkthrough_guide: false,
        walkthrough: false,
        mark_viewed: &[],
        show_cleared: false,
        walkthrough_file: None,
        show_deprioritized: false,
    };

    let result = execute_audit(&opts).expect("audit should execute");
    assert_eq!(
        result.attribution.dead_code_introduced, 0,
        "enabling a rule should not make pre-existing changed-file findings look introduced: {:?}",
        result.attribution
    );
    assert!(
        result.attribution.dead_code_inherited > 0,
        "pre-existing changed-file findings should be classified as inherited: {:?}",
        result.attribution
    );
}

#[test]
fn audit_base_current_config_attribution_survives_cache_hit() {
    let tmp = tempfile::TempDir::new().expect("temp dir should be created");
    let root = tmp.path();
    fs::create_dir_all(root.join("src")).expect("src dir should be created");
    fs::write(
            root.join("package.json"),
            r#"{"name":"audit-current-config-cache","main":"src/index.ts","dependencies":{"left-pad":"1.3.0"}}"#,
        )
        .expect("package.json should be written");
    fs::write(
        root.join(".fallowrc.json"),
        r#"{"rules":{"unused-dependencies":"off"}}"#,
    )
    .expect("base config should be written");
    fs::write(root.join("src/index.ts"), "export const used = 1;\n")
        .expect("index should be written");

    git(root, &["init", "-b", "main"]);
    git(root, &["add", "."]);
    git(
        root,
        &["-c", "commit.gpgsign=false", "commit", "-m", "initial"],
    );

    fs::write(
        root.join(".fallowrc.json"),
        r#"{"rules":{"unused-dependencies":"error"}}"#,
    )
    .expect("current config should be written");
    fs::write(
            root.join("package.json"),
            r#"{"name":"audit-current-config-cache","main":"src/index.ts","dependencies":{"left-pad":"1.3.1"}}"#,
        )
        .expect("package.json should be touched");

    let config_path = None;
    let cache_root = root.join(".fallow");
    let opts = AuditOptions {
        root,
        cache_dir: &cache_root,
        config_path: &config_path,
        output: OutputFormat::Json,
        no_cache: false,
        threads: 1,
        quiet: true,
        changed_since: Some("HEAD"),
        production: false,
        production_dead_code: None,
        production_health: None,
        production_dupes: None,
        workspace: None,
        changed_workspaces: None,
        explain: false,
        explain_skipped: false,
        performance: false,
        group_by: None,
        dead_code_baseline: None,
        health_baseline: None,
        dupes_baseline: None,
        max_crap: None,
        coverage: None,
        coverage_root: None,
        gate: AuditGate::NewOnly,
        include_entry_exports: false,
        css: false,
        css_deep: false,
        runtime_coverage: None,
        min_invocations_hot: 100,
        brief: false,
        max_decisions: 4,
        walkthrough_guide: false,
        walkthrough: false,
        mark_viewed: &[],
        show_cleared: false,
        walkthrough_file: None,
        show_deprioritized: false,
    };

    let first = execute_audit(&opts).expect("first audit should execute");
    assert_eq!(
        first.attribution.dead_code_introduced, 0,
        "first audit should classify pre-existing findings as inherited: {:?}",
        first.attribution
    );

    let changed_files =
        crate::check::get_changed_files(root, "HEAD").expect("changed files should resolve");
    let key = audit_base_snapshot_cache_key(&opts, "HEAD", &changed_files)
        .expect("cache key should compute")
        .expect("cache key should exist");
    assert!(
        load_cached_base_snapshot(&opts, &key).is_some(),
        "first audit should store a reusable base snapshot"
    );

    let second = execute_audit(&opts).expect("second audit should execute");
    assert_eq!(
        second.attribution.dead_code_introduced, 0,
        "cache hit should keep current-config attribution stable: {:?}",
        second.attribution
    );
    assert!(
        second.attribution.dead_code_inherited > 0,
        "cache hit should preserve inherited base findings: {:?}",
        second.attribution
    );
}

#[test]
fn audit_dupes_only_materializes_groups_touching_changed_files() {
    let tmp = tempfile::TempDir::new().expect("temp dir should be created");
    let root_path = tmp
        .path()
        .canonicalize()
        .expect("temp root should canonicalize");
    let root = root_path.as_path();
    fs::create_dir_all(root.join("src")).expect("src dir should be created");
    fs::write(
        root.join("package.json"),
        r#"{"name":"audit-dupes-focus","main":"src/changed.ts"}"#,
    )
    .expect("package.json should be written");
    fs::write(
        root.join(".fallowrc.json"),
        r#"{"duplicates":{"minTokens":5,"minLines":2,"mode":"strict"}}"#,
    )
    .expect("config should be written");

    let focused_code = "export function focused(input: number): number {\n  const doubled = input * 2;\n  const shifted = doubled + 10;\n  return shifted / 2;\n}\n";
    let untouched_code = "export function untouched(input: string): string {\n  const lowered = input.toLowerCase();\n  const padded = lowered.padStart(10, \"x\");\n  return padded.slice(0, 8);\n}\n";
    fs::write(root.join("src/changed.ts"), focused_code).expect("changed should be written");
    fs::write(root.join("src/focused-copy.ts"), focused_code)
        .expect("focused copy should be written");
    fs::write(root.join("src/untouched-a.ts"), untouched_code)
        .expect("untouched a should be written");
    fs::write(root.join("src/untouched-b.ts"), untouched_code)
        .expect("untouched b should be written");

    git(root, &["init", "-b", "main"]);
    git(root, &["add", "."]);
    git(
        root,
        &["-c", "commit.gpgsign=false", "commit", "-m", "initial"],
    );
    fs::write(
        root.join("src/changed.ts"),
        format!("{focused_code}export const changedMarker = true;\n"),
    )
    .expect("changed file should be modified");

    let config_path = None;
    let cache_root = root.join(".fallow");
    let opts = AuditOptions {
        root,
        cache_dir: &cache_root,
        config_path: &config_path,
        output: OutputFormat::Json,
        no_cache: true,
        threads: 1,
        quiet: true,
        changed_since: Some("HEAD"),
        production: false,
        production_dead_code: None,
        production_health: None,
        production_dupes: None,
        workspace: None,
        changed_workspaces: None,
        explain: false,
        explain_skipped: false,
        performance: false,
        group_by: None,
        dead_code_baseline: None,
        health_baseline: None,
        dupes_baseline: None,
        max_crap: None,
        coverage: None,
        coverage_root: None,
        gate: AuditGate::All,
        include_entry_exports: false,
        css: false,
        css_deep: false,
        runtime_coverage: None,
        min_invocations_hot: 100,
        brief: false,
        max_decisions: 4,
        walkthrough_guide: false,
        walkthrough: false,
        mark_viewed: &[],
        show_cleared: false,
        walkthrough_file: None,
        show_deprioritized: false,
    };

    let result = execute_audit(&opts).expect("audit should execute");
    let dupes = result.dupes.expect("dupes should run");
    let changed_path = root.join("src/changed.ts");

    assert!(
        !dupes.report.clone_groups.is_empty(),
        "changed file should still match unchanged duplicate code"
    );
    assert!(dupes.report.clone_groups.iter().all(|group| {
        group
            .instances
            .iter()
            .any(|instance| instance.file == changed_path)
    }));
}

// Unit tests for js_ts_tokens_equivalent, is_analysis_input, is_non_behavioral_doc.

#[test]
fn tokens_equivalent_whitespace_only() {
    // Reformatting (indentation, blank lines) must not change token identity.
    let a = "export const x = 1;\nexport const y = 2;\n";
    let b = "export const x = 1;\n\n\nexport const y = 2;\n";
    assert!(
        js_ts_tokens_equivalent(Path::new("a.ts"), a, b),
        "whitespace-only change must be treated as equivalent"
    );
}

#[test]
fn tokens_equivalent_comment_only_change() {
    // Comments do not produce tokens; adding or removing a comment should be
    // treated as equivalent by the tokenizer.
    let a = "export const x = 1;\n";
    let b = "// note\nexport const x = 1;\n";
    assert!(
        js_ts_tokens_equivalent(Path::new("a.ts"), a, b),
        "comment-only change must be treated as equivalent (comments emit no tokens)"
    );
}

#[test]
fn tokens_equivalent_identifier_rename_is_not_equivalent() {
    // Identifier carries its text payload; a rename must not be reusable.
    let a = "export const a = 1;\n";
    let b = "export const b = 1;\n";
    assert!(
        !js_ts_tokens_equivalent(Path::new("a.ts"), a, b),
        "identifier rename must be treated as non-equivalent"
    );
}

#[test]
fn tokens_equivalent_string_literal_change_is_not_equivalent() {
    // StringLiteral carries its text payload; a changed import path must not be reusable.
    let a = r#"import x from "./a";"#;
    let b = r#"import x from "./b";"#;
    assert!(
        !js_ts_tokens_equivalent(Path::new("a.ts"), a, b),
        "string-literal change must be treated as non-equivalent"
    );
}

#[test]
fn tokens_equivalent_fallow_ignore_marker_forces_false() {
    // The guard fires before tokenization; even identical content containing the
    // marker must return false so suppression changes are never skipped.
    let code = "// fallow-ignore-next-line unused-exports\nexport const x = 1;\n";
    assert!(
        !js_ts_tokens_equivalent(Path::new("a.ts"), code, code),
        "fallow-ignore marker in either side must force false"
    );
}

#[test]
fn tokens_equivalent_non_js_extension_is_false() {
    // The extension check fires before tokenization; CSS content cannot be reused.
    let a = ".foo { color: red; }\n";
    let b = ".foo {\n  color: red;\n}\n";
    assert!(
        !js_ts_tokens_equivalent(Path::new("styles.css"), a, b),
        "non-JS/TS extension must always return false"
    );
}

/// KNOWN SOUNDNESS GAP: `TokenKind::TemplateLiteral` carries no payload
/// (see `crates/core/src/duplicates/token_types.rs`), so a change to the
/// content of a template literal is invisible to the tokenizer and is
/// treated as equivalent. This is safe for most template strings but
/// unsound for dynamic `import(\`...\`)` patterns where the quasi prefix
/// feeds module-resolution pattern edges. This test pins the current
/// behavior. A follow-up fix should give `TemplateLiteral` a payload to
/// close the gap.
#[test]
fn tokens_equivalent_template_literal_content_change_is_equivalent_known_gap() {
    let a = "const p = import(`./pages/${x}`);\n";
    let b = "const p = import(`./views/${x}`);\n";
    // KNOWN GAP: changing the quasi string of a template literal is NOT
    // detected as a behavioral change because TokenKind::TemplateLiteral
    // has no payload. Expected: true (equivalent), which is incorrect for
    // dynamic-import prefixes but documents the current reality.
    assert!(
        js_ts_tokens_equivalent(Path::new("a.ts"), a, b),
        "template-literal content change is CURRENTLY treated as equivalent (known gap)"
    );
}

/// Companion to the template-literal gap test: a regex-literal content
/// change is also invisible to the tokenizer.
#[test]
fn tokens_equivalent_regex_literal_content_change_is_equivalent_known_gap() {
    let a = "const re = /^foo/;\n";
    let b = "const re = /^bar/;\n";
    // KNOWN GAP: TokenKind::RegExpLiteral has no payload.
    assert!(
        js_ts_tokens_equivalent(Path::new("a.ts"), a, b),
        "regex-literal content change is CURRENTLY treated as equivalent (known gap)"
    );
}

#[test]
fn analysis_input_and_doc_classification() {
    // Analysis inputs: JS/TS variants and component formats are behavioral.
    assert!(is_analysis_input(Path::new("src/app.ts")));
    assert!(is_analysis_input(Path::new("src/app.tsx")));
    assert!(is_analysis_input(Path::new("src/app.js")));
    assert!(is_analysis_input(Path::new("src/app.jsx")));
    assert!(is_analysis_input(Path::new("src/app.mts")));
    assert!(is_analysis_input(Path::new("src/app.vue")));
    assert!(is_analysis_input(Path::new("src/styles.css")));

    // Non-analysis inputs.
    assert!(!is_analysis_input(Path::new("README.md")));
    assert!(!is_analysis_input(Path::new("package.json")));
    assert!(!is_analysis_input(Path::new("image.png")));

    // Non-behavioral docs.
    assert!(is_non_behavioral_doc(Path::new("README.md")));
    assert!(is_non_behavioral_doc(Path::new("CHANGELOG.txt")));
    assert!(is_non_behavioral_doc(Path::new("docs/guide.rst")));
    assert!(is_non_behavioral_doc(Path::new("docs/guide.adoc")));

    // .json is neither an analysis input nor a non-behavioral doc, so the
    // predicate treats it as behavioral (can_reuse returns false for it).
    assert!(!is_analysis_input(Path::new("package.json")));
    assert!(!is_non_behavioral_doc(Path::new("package.json")));
}
