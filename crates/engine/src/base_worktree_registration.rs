//! Git registration of detached base snapshots.
//!
//! A base snapshot is a linked worktree only while it is created. Fallow then
//! removes its Git admin entry and keeps the directory, so Git-aware tools do
//! not see the snapshot while the base analysis reads it, and a process that
//! dies before cleanup leaves no registered worktree behind (issues #1815 and
//! #3885). The CLI audit cache and the programmatic base snapshot share this
//! lifecycle.

use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};

/// Invalid gitdir pointer written into `<snapshot>/.git` after Fallow
/// deregisters a base snapshot.
///
/// The `.git` file is REPLACED, never deleted: both discovery walkers use the
/// `ignore` crate with `require_git` on, whose gitignore handling is gated on
/// `<root>/.git` existing. Deleting the gitfile would silently stop the base
/// pass from honoring `.gitignore`, inflating base findings and skewing
/// audit's introduced-vs-inherited split. The stub keeps gitignore parity
/// while pointing at a nonexistent gitdir, so any stray `git` command inside
/// the snapshot fails loudly instead of operating on the host repo.
pub const UNREGISTERED_GITDIR_STUB: &str = "gitdir: fallow-audit-unregistered\n";

/// Prefix of every admin entry and snapshot directory that Fallow creates.
const FALLOW_BASE_WORKTREE_PREFIX: &str = "fallow-audit-base-";

/// Deregister a Fallow base snapshot from Git while KEEPING its directory.
///
/// Targets the single admin dir this worktree owns via its `.git` gitfile
/// pointer, rather than a global `git worktree prune`, so a user's unrelated
/// prunable worktrees are never collaterally deregistered and Git's
/// name-collision admin suffixing (`<name>1`) is handled for free. The `.git`
/// gitfile is REPLACED with [`UNREGISTERED_GITDIR_STUB`], never deleted.
/// A missing directory and an already deregistered snapshot are success.
///
/// # Errors
///
/// Returns an error when the gitfile is not a regular file, when its admin
/// entry is not a Fallow entry of `repo_root` that links back to `path`, or
/// when the admin entry or the gitfile cannot be changed.
pub fn deregister_base_worktree(repo_root: &Path, path: &Path) -> std::io::Result<()> {
    if !path.exists() {
        return Ok(());
    }
    let gitfile = path.join(".git");
    let metadata = fs::symlink_metadata(&gitfile)?;
    if !is_regular_file(&metadata) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "refusing to deregister through a non-file audit worktree .git entry",
        ));
    }
    let contents = fs::read_to_string(&gitfile)?;
    if contents == UNREGISTERED_GITDIR_STUB {
        return Ok(());
    }
    let Some(admin_dir) = trusted_base_worktree_admin_dir(repo_root, path) else {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "refusing to deregister an unverified audit worktree admin entry",
        ));
    };
    match fs::remove_dir_all(&admin_dir) {
        Ok(()) => {}
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
        Err(err) => return Err(err),
    }
    write_unregistered_gitdir_stub(&gitfile)
}

/// Write [`UNREGISTERED_GITDIR_STUB`] into `gitfile`, creating it when it is
/// missing.
///
/// # Errors
///
/// Returns an error when `gitfile` exists but is not a regular file, or when
/// the write fails.
pub fn write_unregistered_gitdir_stub(gitfile: &Path) -> std::io::Result<()> {
    let mut options = fs::OpenOptions::new();
    options.write(true).truncate(true);
    match fs::symlink_metadata(gitfile) {
        Ok(metadata) if is_regular_file(&metadata) => {}
        Ok(_) => {
            return Err(std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "refusing to replace non-file audit worktree .git entry",
            ));
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            options.create_new(true);
        }
        Err(error) => return Err(error),
    }
    let mut file = options.open(gitfile)?;
    file.write_all(UNREGISTERED_GITDIR_STUB.as_bytes())?;
    file.sync_all()
}

/// Return the Git admin entry of the snapshot at `path` when it is a Fallow
/// entry of `repo_root` whose backlink points at `path`.
#[must_use]
pub fn trusted_base_worktree_admin_dir(repo_root: &Path, path: &Path) -> Option<PathBuf> {
    let gitfile = path.join(".git");
    let metadata = fs::symlink_metadata(&gitfile).ok()?;
    if !is_regular_file(&metadata) {
        return None;
    }
    let contents = fs::read_to_string(&gitfile).ok()?;
    let admin_dir = parse_worktree_gitdir(&contents)?;
    if !is_fallow_admin_dir(&admin_dir) {
        return None;
    }
    let common_dir = crate::changed_files::resolve_git_common_dir(repo_root).ok()?;
    let worktrees_dir = dunce::canonicalize(common_dir.join("worktrees")).ok()?;
    let admin_parent = dunce::canonicalize(admin_dir.parent()?).ok()?;
    if admin_parent != worktrees_dir {
        return None;
    }
    let backlink = fs::read_to_string(admin_dir.join("gitdir")).ok()?;
    let expected_gitfile = dunce::canonicalize(&gitfile).ok()?;
    let actual_gitfile = dunce::canonicalize(Path::new(backlink.trim())).ok()?;
    (actual_gitfile == expected_gitfile).then_some(admin_dir)
}

/// Parse the `gitdir: <path>` pointer from a linked-worktree `.git` gitfile.
#[must_use]
pub fn parse_worktree_gitdir(contents: &str) -> Option<PathBuf> {
    contents
        .lines()
        .find_map(|line| line.trim().strip_prefix("gitdir:"))
        .map(|rest| PathBuf::from(rest.trim()))
}

#[expect(
    clippy::filetype_is_file,
    reason = "a gitfile must be a regular file, not an arbitrary non-directory"
)]
fn is_regular_file(metadata: &fs::Metadata) -> bool {
    metadata.file_type().is_file()
}

/// True when an admin dir basename is one Fallow itself created, used as a
/// safety belt before removing it.
fn is_fallow_admin_dir(admin_dir: &Path) -> bool {
    admin_dir
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.starts_with(FALLOW_BASE_WORKTREE_PREFIX))
}

#[cfg(test)]
mod tests {
    use std::process::Command;

    use super::*;
    use crate::repo_refs::{TemporaryBaseWorktree, create_detached_base_worktree};

    fn git(root: &Path, args: &[&str]) -> String {
        let output = crate::changed_files::clear_ambient_git_env(&mut Command::new("git"))
            .arg("-C")
            .arg(root)
            .args(args)
            .output()
            .expect("git command starts");
        assert!(
            output.status.success(),
            "git {args:?} failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
        String::from_utf8_lossy(&output.stdout).trim().to_owned()
    }

    fn committed_repo(parent: &Path) -> PathBuf {
        let repo = parent.join("repo");
        fs::create_dir_all(&repo).expect("create repo");
        git(&repo, &["init", "-b", "main"]);
        git(&repo, &["config", "user.name", "Test User"]);
        git(&repo, &["config", "user.email", "test@example.com"]);
        git(&repo, &["config", "commit.gpgsign", "false"]);
        fs::write(repo.join(".gitignore"), "ignored/\n").expect("write gitignore");
        fs::write(repo.join("tracked.txt"), "committed\n").expect("write tracked file");
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-m", "initial"]);
        repo
    }

    fn registered_worktrees(repo: &Path) -> Vec<String> {
        git(repo, &["worktree", "list", "--porcelain"])
            .lines()
            .filter_map(|line| line.strip_prefix("worktree "))
            .map(str::to_owned)
            .collect()
    }

    /// Issue #3885: the programmatic base snapshot must not stay registered
    /// while the base analysis reads it, so a host process that stops before
    /// `Drop` leaves no Git worktree entry behind.
    #[test]
    fn temporary_base_worktree_is_unregistered_while_it_is_in_use() {
        let temp = tempfile::tempdir().expect("temp dir");
        let repo = committed_repo(temp.path());
        let before = registered_worktrees(&repo);

        let worktree = TemporaryBaseWorktree::create(&repo, "HEAD", &[])
            .expect("temporary worktree should be created");

        assert_eq!(
            registered_worktrees(&repo),
            before,
            "the base snapshot must be deregistered before analysis reads it"
        );
        assert_eq!(
            fs::read_to_string(worktree.path().join("tracked.txt")).expect("read tracked file"),
            "committed\n"
        );
        assert_eq!(
            fs::read_to_string(worktree.path().join(".git")).expect("read gitfile"),
            UNREGISTERED_GITDIR_STUB,
            "the gitfile stays as a stub so gitignore handling still applies"
        );

        let path = worktree.path().to_path_buf();
        drop(worktree);
        assert!(!path.exists(), "temporary worktree must clean up on drop");
        assert_eq!(registered_worktrees(&repo), before);
    }

    #[test]
    fn registered_snapshot_index_keeps_gitlinks_until_deregistration() {
        let temp = tempfile::tempdir().expect("temp dir");
        let repo = committed_repo(temp.path());
        let gitlink_commit = git(&repo, &["rev-parse", "HEAD"]);
        let cacheinfo = format!("160000,{gitlink_commit},vendor/submodule");
        git(&repo, &["update-index", "--add", "--cacheinfo", &cacheinfo]);
        git(&repo, &["commit", "-m", "gitlink"]);
        let destination = temp.path().join("fallow-audit-base-index-probe");

        create_detached_base_worktree(&repo, &destination, "HEAD")
            .expect("registered worktree should be created");
        assert!(
            git(&destination, &["ls-files", "--stage", "vendor/submodule"])
                .starts_with(&format!("160000 {gitlink_commit} 0\t")),
            "the linked worktree index must retain the gitlink object id"
        );

        deregister_base_worktree(&repo, &destination).expect("snapshot deregisters");
        assert_eq!(registered_worktrees(&repo).len(), 1);
        assert!(destination.join("tracked.txt").is_file());
    }
}
