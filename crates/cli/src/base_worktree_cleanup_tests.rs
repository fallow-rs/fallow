use super::*;

fn git(repo: &Path, args: &[&str]) -> String {
    let mut command = Command::new("git");
    command.args(args).current_dir(repo);
    clear_ambient_git_env(&mut command);
    let output = command.output().expect("git should run");
    assert!(
        output.status.success(),
        "git {args:?}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout).expect("git output should be UTF-8")
}

fn init_repo(root: &Path) -> PathBuf {
    let repo = root.join("repo");
    std::fs::create_dir(&repo).unwrap();
    git(&repo, &["init", "--quiet"]);
    git(
        &repo,
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.com",
            "-c",
            "commit.gpgsign=false",
            "commit",
            "--quiet",
            "--allow-empty",
            "-m",
            "test fixture",
        ],
    );
    repo
}

fn register(repo: &Path, path: &Path) -> PathBuf {
    git(
        repo,
        &[
            "worktree",
            "add",
            "--quiet",
            "--detach",
            path.to_str().unwrap(),
            "HEAD",
        ],
    );
    parse_worktree_gitdir(&std::fs::read_to_string(path.join(".git")).unwrap()).unwrap()
}

fn assert_unavailable_registration_preserved(repo: &Path, path: &Path, admin: &Path) {
    let listing = git(repo, &["worktree", "list", "--porcelain"]);
    assert!(
        listing.lines().any(|line| line
            .strip_prefix("worktree ")
            .is_some_and(|entry| paths_equal(Path::new(entry), path))),
        "unrelated unavailable worktree must stay registered: {listing}"
    );
    assert!(
        admin.join("gitdir").is_file(),
        "unrelated admin entry must remain available for repair"
    );
}

#[test]
fn legacy_cache_cleanup_preserves_unrelated_unavailable_registration() {
    assert_legacy_cache_cleanup_preserves_unrelated_registration(false);
}

#[test]
fn missing_legacy_cache_cleanup_preserves_unrelated_unavailable_registration() {
    assert_legacy_cache_cleanup_preserves_unrelated_registration(true);
}

#[test]
fn missing_legacy_cache_cleanup_reports_failed_git_removal() {
    let temp = tempfile::tempdir_in(dunce::canonicalize(std::env::temp_dir()).unwrap()).unwrap();
    let repo = init_repo(temp.path());
    let candidate = temp.path().join("fallow-audit-base-cache-locked");
    let admin = register(&repo, &candidate);
    git(&repo, &["worktree", "lock", candidate.to_str().unwrap()]);
    std::fs::rename(&candidate, temp.path().join("parked-locked-cache")).unwrap();

    let applied = sweep_reusable_caches_with_report(
        &repo,
        None,
        temp.path(),
        SweepMode::Apply,
        SweepSizes::Skip,
        SweepScope::All,
    );

    assert!(
        applied
            .entries
            .iter()
            .any(|entry| entry.path.file_name() == candidate.file_name()
                && entry.disposition == SweepDisposition::RemoveFailed),
        "a failed targeted Git removal must not be reported as reclamation"
    );
    assert_unavailable_registration_preserved(&repo, &candidate, &admin);
    assert!(
        admin.join("locked").is_file(),
        "Git's worktree lock must remain"
    );
}

fn assert_legacy_cache_cleanup_preserves_unrelated_registration(missing: bool) {
    let temp = tempfile::tempdir_in(dunce::canonicalize(std::env::temp_dir()).unwrap()).unwrap();
    let repo = init_repo(temp.path());
    let candidate = temp.path().join("fallow-audit-base-cache-legacy-owned");
    let owned_admin = register(&repo, &candidate);
    if missing {
        std::fs::rename(&candidate, temp.path().join("parked-owned-cache")).unwrap();
    }
    let unrelated = temp.path().join("user-worktree");
    let unrelated_admin = register(&repo, &unrelated);
    let parked = temp.path().join("parked-user-worktree");
    std::fs::rename(&unrelated, &parked).unwrap();
    assert_unavailable_registration_preserved(&repo, &unrelated, &unrelated_admin);

    let dry_run = sweep_reusable_caches_with_report(
        &repo,
        None,
        temp.path(),
        SweepMode::DryRun,
        SweepSizes::Skip,
        SweepScope::All,
    );
    assert!(
        dry_run
            .entries
            .iter()
            .any(|entry| entry.path.file_name() == candidate.file_name()
                && entry.disposition == SweepDisposition::ReclaimedLegacyRegistered)
    );
    assert!(
        owned_admin.is_dir() && candidate.is_dir() != missing,
        "dry run must preserve the owned registration and contents"
    );
    assert!(
        !reusable_worktree_lock_path(&candidate).exists(),
        "dry run must not create sidecars"
    );
    assert_unavailable_registration_preserved(&repo, &unrelated, &unrelated_admin);

    let applied = sweep_reusable_caches_with_report(
        &repo,
        None,
        temp.path(),
        SweepMode::Apply,
        SweepSizes::Skip,
        SweepScope::All,
    );
    assert!(
        applied
            .entries
            .iter()
            .any(|entry| entry.path.file_name() == candidate.file_name()
                && entry.disposition == SweepDisposition::ReclaimedLegacyRegistered)
    );
    assert!(
        !candidate.exists() && !owned_admin.exists(),
        "owned legacy cache must be removed and deregistered"
    );
    assert_unavailable_registration_preserved(&repo, &unrelated, &unrelated_admin);
    assert!(
        parked.join(".git").is_file(),
        "parked user contents must remain"
    );
}

#[test]
fn legacy_orphan_cleanup_preserves_unrelated_unavailable_registration() {
    const DEAD_PID: u32 = 99_999_998;
    assert!(!process_is_alive(DEAD_PID));
    let temp = tempfile::tempdir_in(dunce::canonicalize(std::env::temp_dir()).unwrap()).unwrap();
    let repo = init_repo(temp.path());
    let candidate = temp
        .path()
        .join(format!("fallow-audit-base-{DEAD_PID}-1-0"));
    let owned_admin = register(&repo, &candidate);
    let missing = temp
        .path()
        .join(format!("fallow-audit-base-{DEAD_PID}-2-0"));
    let missing_admin = register(&repo, &missing);
    std::fs::rename(&missing, temp.path().join("parked-owned-worktree")).unwrap();
    let unrelated = temp.path().join("user-worktree");
    let unrelated_admin = register(&repo, &unrelated);
    let parked = temp.path().join("parked-user-worktree");
    std::fs::rename(&unrelated, &parked).unwrap();
    assert_unavailable_registration_preserved(&repo, &unrelated, &unrelated_admin);

    sweep_orphan_audit_worktrees_in(&repo, temp.path(), std::time::Duration::ZERO);

    assert!(
        !candidate.exists() && !owned_admin.exists(),
        "owned dead-process worktree must be removed and deregistered"
    );
    assert!(
        !missing_admin.exists(),
        "an unavailable owned dead-process worktree must still be deregistered"
    );
    assert_unavailable_registration_preserved(&repo, &unrelated, &unrelated_admin);
    assert!(
        parked.join(".git").is_file(),
        "parked user contents must remain"
    );
}

#[test]
fn deregistration_rejects_admin_entry_from_another_repository() {
    let temp = tempfile::TempDir::new().unwrap();
    let repo = init_repo(temp.path());
    let other_root = temp.path().join("other");
    std::fs::create_dir(&other_root).unwrap();
    let other_repo = init_repo(&other_root);
    let candidate = temp.path().join("fallow-audit-base-cache-other-repo");
    let other_admin = register(&other_repo, &candidate);
    let original = std::fs::read(candidate.join(".git")).unwrap();

    assert!(
        unregister_legacy_worktree_checked(&repo, &candidate).is_err(),
        "a correctly named admin entry in another repository must be rejected"
    );
    assert_eq!(std::fs::read(candidate.join(".git")).unwrap(), original);
    assert!(
        other_admin.join("gitdir").is_file(),
        "another repository's admin entry must survive"
    );

    unregister_legacy_worktree_checked(&other_repo, &candidate).unwrap();
    assert!(
        !other_admin.exists(),
        "the owning repository can deregister its entry"
    );
    assert_eq!(
        std::fs::read_to_string(candidate.join(".git")).unwrap(),
        unregistered_gitdir_stub(&candidate)
    );
}

#[test]
fn deregistration_rejects_admin_entry_with_another_worktree_backlink() {
    let temp = tempfile::TempDir::new().unwrap();
    let repo = init_repo(temp.path());
    let candidate = temp.path().join("fallow-audit-base-cache-candidate");
    let candidate_admin = register(&repo, &candidate);
    let other = temp.path().join("fallow-audit-base-cache-other");
    let other_admin = register(&repo, &other);
    let other_gitfile = std::fs::read(other.join(".git")).unwrap();
    std::fs::write(candidate.join(".git"), &other_gitfile).unwrap();

    assert!(
        unregister_legacy_worktree_checked(&repo, &candidate).is_err(),
        "an admin entry must link back to the selected worktree"
    );
    assert_eq!(
        std::fs::read(candidate.join(".git")).unwrap(),
        other_gitfile
    );
    assert!(
        candidate_admin.join("gitdir").is_file() && other_admin.join("gitdir").is_file(),
        "neither registration may be removed through the forged pointer"
    );

    unregister_legacy_worktree_checked(&repo, &other).unwrap();
    assert!(
        !other_admin.exists(),
        "the matching backlink allows owned deregistration"
    );
    assert!(
        candidate_admin.join("gitdir").is_file(),
        "the separate admin entry must remain"
    );
}
