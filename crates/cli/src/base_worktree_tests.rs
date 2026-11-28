use super::*;

/// A sidecar that another user owns says nothing about this user's
/// snapshot, so neither reader returns its text.
#[cfg(unix)]
#[test]
fn sidecar_readers_ignore_a_sidecar_of_another_user() {
    let tmp = tempfile::tempdir().unwrap();
    let cache = tmp
        .path()
        .join("fallow-audit-base-cache-0000000000000000-root-0000000000000000");
    std::fs::write(reusable_worktree_sha_path(&cache), "abc\n").unwrap();
    std::fs::write(reusable_worktree_last_used_path(&cache), "/owner/root\n").unwrap();
    assert_eq!(read_reusable_sha(&cache).as_deref(), Some("abc"));
    assert_eq!(
        read_last_used_owner(&cache),
        Some(PathBuf::from("/owner/root"))
    );

    let other_user = rustix::process::geteuid().as_raw().wrapping_add(1);
    SIDECAR_OWNER_OVERRIDE.with(|owner| owner.set(Some(other_user)));
    let sha = read_reusable_sha(&cache);
    let owner = read_last_used_owner(&cache);
    SIDECAR_OWNER_OVERRIDE.with(|owner| owner.set(None));

    assert_eq!(sha, None);
    assert_eq!(owner, None);
}

/// Git prints a top level that ends in a space as it is. Trimming the
/// space would name the sibling `web`, here a link to another project,
/// and canonicalizing would follow that link.
#[cfg(unix)]
#[test]
fn git_toplevel_keeps_trailing_blanks_of_the_path() {
    let tmp = tempfile::tempdir().unwrap();
    let base = dunce::canonicalize(tmp.path()).unwrap();
    let repo = base.join("web ");
    let other = base.join("other-project");
    std::fs::create_dir_all(&repo).unwrap();
    std::fs::create_dir_all(&other).unwrap();
    std::os::unix::fs::symlink("other-project", base.join("web")).unwrap();
    let init = std::process::Command::new("git")
        .args(["init", "-q"])
        .current_dir(&repo)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_CONFIG_GLOBAL", "/dev/null")
        .env_remove("GIT_DIR")
        .env_remove("GIT_WORK_TREE")
        .status()
        .unwrap();
    assert!(init.success());

    assert_eq!(git_toplevel(&repo), Some(repo.clone()));
    assert_eq!(
        crate::agent_install::resolve_root(&repo.join("."), false),
        repo
    );
}

/// Many threads minting a non-reusable worktree path at the same instant
/// must each get a distinct path. Before the monotonic counter, concurrent
/// callers read the same non-monotonic `nanos` and collided, so two parallel
/// `audit` runs in one process raced on `git worktree add` and one aborted
/// with a generic error (a flaky exit 2 under parallel unit tests).
#[test]
fn non_reusable_worktree_paths_are_unique_under_concurrency() {
    const N: usize = 64;
    let barrier = std::sync::Barrier::new(N);
    let paths = std::sync::Mutex::new(Vec::with_capacity(N));
    std::thread::scope(|s| {
        for _ in 0..N {
            let barrier = &barrier;
            let paths = &paths;
            s.spawn(move || {
                barrier.wait();
                let path = non_reusable_worktree_path().expect("path should build");
                paths.lock().unwrap().push(path);
            });
        }
    });
    let mut paths = paths.into_inner().unwrap();
    assert_eq!(paths.len(), N);
    paths.sort();
    paths.dedup();
    assert_eq!(paths.len(), N, "non-reusable worktree paths collided");
}

/// The pid stays the first segment so orphan-sweep parsing keeps working.
#[test]
fn non_reusable_worktree_path_pid_is_parseable() {
    let path = non_reusable_worktree_path().expect("path should build");
    let name = path.file_name().unwrap().to_str().unwrap();
    assert!(is_fallow_audit_worktree_path(&path));
    assert!(!is_reusable_audit_worktree_path(&path));
    assert_eq!(audit_worktree_pid(name), Some(std::process::id()));
}

#[test]
fn directory_size_bytes_sums_regular_files_recursively() {
    let temp = tempfile::TempDir::new().expect("temp dir should be created");
    let root = temp.path().join("cache");
    std::fs::create_dir_all(root.join("node_modules/dep")).expect("tree should be created");
    std::fs::write(root.join("a.txt"), vec![0u8; 10]).expect("file should be written");
    std::fs::write(root.join("node_modules/dep/b.js"), vec![0u8; 32])
        .expect("nested file should be written");
    // A gitignore inside the snapshot must NOT hide the mass being
    // measured (the walk is deliberately ignore-crate-free).
    std::fs::write(root.join(".gitignore"), "node_modules\n").expect("gitignore should be written");

    let size = directory_size_bytes(&root).expect("size walk should succeed");
    assert_eq!(size, 10 + 32 + "node_modules\n".len() as u64);
    assert_eq!(
        directory_size_bytes(&temp.path().join("missing")),
        None,
        "an absent directory reports no size",
    );
}

#[cfg(unix)]
#[test]
fn directory_size_bytes_never_follows_symlinks() {
    let temp = tempfile::TempDir::new().expect("temp dir should be created");
    let root = temp.path().join("cache");
    let outside = temp.path().join("outside");
    std::fs::create_dir_all(&root).expect("root should be created");
    std::fs::create_dir_all(&outside).expect("outside dir should be created");
    std::fs::write(outside.join("big.bin"), vec![0u8; 4096])
        .expect("outside file should be written");
    std::os::unix::fs::symlink(&outside, root.join("link-dir"))
        .expect("dir symlink should be created");
    std::os::unix::fs::symlink(outside.join("big.bin"), root.join("link-file"))
        .expect("file symlink should be created");

    assert_eq!(
        directory_size_bytes(&root),
        Some(0),
        "symlinked directories and files must not be traversed or counted",
    );
}

#[cfg(unix)]
#[test]
fn cache_sidecar_open_does_not_follow_symlinks() {
    let temp = tempfile::TempDir::new().expect("temp dir should be created");
    let link_target = temp.path().join("link_target");
    let sidecar = temp.path().join("cache.lock");
    std::fs::write(&link_target, "unchanged\n").expect("link target should be written");
    std::os::unix::fs::symlink(&link_target, &sidecar).expect("sidecar symlink should be created");

    assert!(open_or_create_owned_sidecar(&sidecar).is_err());
    assert_eq!(
        std::fs::read_to_string(link_target).expect("link target should remain readable"),
        "unchanged\n",
    );
}

/// Audit snapshots live in a directory of this user inside the temp dir, not
/// in the shared temp dir itself, where other users can create files.
#[cfg(unix)]
#[test]
fn audit_snapshots_live_in_a_private_directory_of_this_user() {
    use std::os::unix::fs::{MetadataExt as _, PermissionsExt as _};

    let tmp = tempfile::tempdir().unwrap();
    let non_reusable = non_reusable_worktree_path().expect("path should build");
    let reusable = reusable_audit_worktree_path(tmp.path());
    let dir = non_reusable.parent().expect("snapshot directory");
    assert_eq!(reusable.parent(), Some(dir));
    assert_ne!(dir, std::env::temp_dir());
    assert!(dir.starts_with(std::env::temp_dir()), "{}", dir.display());

    let metadata = std::fs::symlink_metadata(dir).expect("snapshot directory exists");
    assert!(metadata.file_type().is_dir());
    assert_eq!(metadata.uid(), rustix::process::geteuid().as_raw());
    assert_eq!(metadata.permissions().mode() & 0o777, 0o700);
}
