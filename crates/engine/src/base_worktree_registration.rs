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
use std::process::Command;

/// Last segment of the gitdir that the stub of a deregistered snapshot names.
const UNREGISTERED_GITDIR_NAME: &str = "fallow-audit-unregistered";

/// Stub of older versions. Its gitdir is relative, so Git resolves it inside
/// the snapshot, where the untrusted base commit can supply a directory of
/// that name with its own config. It is recognized only to replace it.
const LEGACY_UNREGISTERED_GITDIR_STUB: &str = "gitdir: fallow-audit-unregistered\n";

/// Prefix of every admin entry and snapshot directory that Fallow creates.
const FALLOW_BASE_WORKTREE_PREFIX: &str = "fallow-audit-base-";

/// Gitfile contents written into `<snapshot>/.git` after Fallow deregisters
/// a base snapshot.
///
/// The `.git` file is REPLACED, never deleted: both discovery walkers use the
/// `ignore` crate with `require_git` on, whose gitignore handling is gated on
/// `<root>/.git` existing. Deleting the gitfile would silently stop the base
/// pass from honoring `.gitignore`, inflating base findings and skewing
/// audit's introduced-vs-inherited split.
///
/// The stub names the absolute path `<snapshot>/.git/fallow-audit-unregistered`.
/// That path can never exist, because `<snapshot>/.git` is this regular file,
/// and the committed tree cannot supply a `.git` entry. So a `git` command in
/// the snapshot fails and never reads a config from the untrusted base commit.
#[must_use]
pub fn unregistered_gitdir_stub(snapshot: &Path) -> String {
    let gitdir = snapshot.join(".git").join(UNREGISTERED_GITDIR_NAME);
    format!("gitdir: {}\n", gitdir.display())
}

/// Return whether `contents` is the stub of a deregistered snapshot at
/// `snapshot`, in the current or the legacy form.
#[must_use]
pub fn is_unregistered_gitdir_stub(snapshot: &Path, contents: &str) -> bool {
    if contents == LEGACY_UNREGISTERED_GITDIR_STUB {
        return true;
    }
    let Some(gitdir) = parse_worktree_gitdir(contents) else {
        return false;
    };
    let Some(gitfile) = gitdir
        .file_name()
        .is_some_and(|name| name == UNREGISTERED_GITDIR_NAME)
        .then(|| gitdir.parent())
        .flatten()
    else {
        return false;
    };
    if gitfile.file_name().is_none_or(|name| name != ".git") {
        return false;
    }
    gitfile
        .parent()
        .is_some_and(|owner| owner == snapshot || same_existing_path(owner, snapshot))
}

fn same_existing_path(left: &Path, right: &Path) -> bool {
    match (dunce::canonicalize(left), dunce::canonicalize(right)) {
        (Ok(left), Ok(right)) => left == right,
        _ => false,
    }
}

/// Return the deregistered snapshot that contains `dir`, if any.
///
/// The walk stops at the first `.git` entry. A Fallow stub marks a snapshot.
/// Any other `.git` entry is an ordinary repository boundary.
#[must_use]
pub fn enclosing_unregistered_snapshot(dir: &Path) -> Option<PathBuf> {
    let dir = std::path::absolute(dir).ok()?;
    for candidate in dir.ancestors() {
        let gitfile = candidate.join(".git");
        let Ok(metadata) = fs::symlink_metadata(&gitfile) else {
            continue;
        };
        if !is_regular_file(&metadata) {
            return None;
        }
        let contents = fs::read_to_string(&gitfile).ok()?;
        return is_unregistered_gitdir_stub(candidate, &contents).then(|| candidate.to_path_buf());
    }
    None
}

/// Keep a `git` command that runs in `cwd` from discovering a repository
/// inside a deregistered base snapshot.
///
/// The snapshot holds the untrusted base commit. It can contain a directory
/// that looks like a bare repository, and Git would read its config. When
/// `cwd` is inside a snapshot, the command gets a `GIT_DIR` that does not
/// exist and a `GIT_CEILING_DIRECTORIES` at the snapshot parent, so Git stops
/// with "not a git repository". Outside a snapshot, the command is unchanged.
pub fn confine_git_to_host(command: &mut Command, cwd: &Path) {
    let Some(snapshot) = enclosing_unregistered_snapshot(cwd) else {
        return;
    };
    command.env(
        "GIT_DIR",
        snapshot.join(".git").join(UNREGISTERED_GITDIR_NAME),
    );
    if let Some(parent) = snapshot.parent() {
        command.env("GIT_CEILING_DIRECTORIES", parent);
    }
}

/// Deregister a Fallow base snapshot from Git while KEEPING its directory.
///
/// Targets the single admin dir this worktree owns via its `.git` gitfile
/// pointer, rather than a global `git worktree prune`, so a user's unrelated
/// prunable worktrees are never collaterally deregistered and Git's
/// name-collision admin suffixing (`<name>1`) is handled for free. The `.git`
/// gitfile is REPLACED with [`unregistered_gitdir_stub`], never deleted.
/// A missing directory and an already deregistered snapshot are success. A
/// legacy stub is replaced with the current one.
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
    if is_unregistered_gitdir_stub(path, &contents) {
        if contents == unregistered_gitdir_stub(path) {
            return Ok(());
        }
        return write_unregistered_gitdir_stub(&gitfile);
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

/// Write the [`unregistered_gitdir_stub`] of the snapshot that owns `gitfile`
/// into `gitfile`, creating it when it is missing.
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
    let snapshot = gitfile.parent().ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "audit worktree .git entry has no parent directory",
        )
    })?;
    let stub = unregistered_gitdir_stub(snapshot);
    let mut file = options.open(gitfile)?;
    file.write_all(stub.as_bytes())?;
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
            unregistered_gitdir_stub(worktree.path()),
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

    /// Write a directory that Git accepts as a repository, whose config runs
    /// `marker` as the fsmonitor hook.
    fn write_crafted_gitdir(dir: &Path, marker: &Path) {
        fs::create_dir_all(dir.join("objects")).expect("create objects");
        fs::create_dir_all(dir.join("refs")).expect("create refs");
        fs::write(dir.join("objects/.keep"), "").expect("write objects keep");
        fs::write(dir.join("refs/.keep"), "").expect("write refs keep");
        fs::write(dir.join("HEAD"), "ref: refs/heads/main\n").expect("write HEAD");
        fs::write(
            dir.join("config"),
            format!(
                "[core]\n\trepositoryformatversion = 0\n\tfsmonitor = \"touch '{}'; false\"\n",
                marker.display()
            ),
        )
        .expect("write config");
    }

    fn raw_git_status(cwd: &Path, confine: bool) -> bool {
        let mut command = Command::new("git");
        crate::changed_files::clear_ambient_git_env(&mut command);
        if confine {
            confine_git_to_host(&mut command, cwd);
        }
        command
            .args(["status", "--porcelain"])
            .current_dir(cwd)
            .output()
            .expect("git command starts")
            .status
            .success()
    }

    /// The base commit is untrusted. A top-level `fallow-audit-unregistered/`
    /// or a nested directory shaped as a bare repository must never become
    /// the repository of a `git` command inside the snapshot.
    #[cfg(unix)]
    #[test]
    fn crafted_gitdirs_in_the_base_commit_are_never_read() {
        let temp = tempfile::tempdir().expect("temp dir");
        let repo = committed_repo(temp.path());
        let marker = temp.path().join("fsmonitor-ran");
        write_crafted_gitdir(&repo.join(UNREGISTERED_GITDIR_NAME), &marker);
        write_crafted_gitdir(&repo.join("pkg"), &marker);
        git(&repo, &["add", "-f", "."]);
        git(&repo, &["commit", "-m", "crafted"]);

        let worktree = TemporaryBaseWorktree::create(&repo, "HEAD", &[])
            .expect("temporary worktree should be created");
        let snapshot = worktree.path();
        assert!(
            snapshot
                .join(UNREGISTERED_GITDIR_NAME)
                .join("config")
                .is_file()
        );

        assert!(
            !raw_git_status(snapshot, false),
            "Git at the snapshot root must not find a repository"
        );
        assert!(!raw_git_status(&snapshot.join("pkg"), true));
        assert!(!crate::churn::is_git_repo(snapshot));
        assert!(!crate::churn::is_git_repo(&snapshot.join("pkg")));
        assert!(
            crate::changed_files::resolve_git_toplevel(&snapshot.join("pkg")).is_err(),
            "changed-file probes must not find the nested bare repository"
        );
        for dir in [snapshot.to_path_buf(), snapshot.join("pkg")] {
            let status = crate::git_env::git_command(&dir)
                .args(["status", "--porcelain"])
                .output()
                .expect("git command starts")
                .status;
            assert!(
                !status.success(),
                "the shared Git wrapper found a repository"
            );
        }
        assert!(
            !marker.exists(),
            "a config from the base commit ran a command"
        );
    }

    /// The shared Git wrapper also refuses a bare repository that Git would
    /// find on its own outside a snapshot, and keeps ordinary checkouts.
    #[cfg(unix)]
    #[test]
    fn shared_git_wrapper_refuses_implicit_bare_repositories() {
        let temp = tempfile::tempdir().expect("temp dir");
        let repo = committed_repo(temp.path());
        let marker = temp.path().join("fsmonitor-ran");
        let bare = temp.path().join("bare");
        write_crafted_gitdir(&bare, &marker);
        fs::create_dir_all(repo.join("sub")).expect("create subdirectory");

        let run = |dir: &Path| {
            crate::git_env::git_command(dir)
                .args(["status", "--porcelain"])
                .output()
                .expect("git command starts")
                .status
                .success()
        };

        assert!(!run(&bare), "an implicit bare repository must be refused");
        assert!(!marker.exists());
        assert!(run(&repo), "an ordinary checkout must keep working");
        assert!(
            run(&repo.join("sub")),
            "a checkout subdirectory must keep working"
        );
    }

    /// The stub keeps `.gitignore` handling of the base pass, which needs a
    /// `.git` entry at the walk root.
    #[test]
    fn unregistered_snapshot_keeps_gitignore_handling() {
        let temp = tempfile::tempdir().expect("temp dir");
        let repo = committed_repo(temp.path());
        let worktree = TemporaryBaseWorktree::create(&repo, "HEAD", &[])
            .expect("temporary worktree should be created");
        fs::create_dir_all(worktree.path().join("ignored")).expect("create ignored dir");
        fs::write(worktree.path().join("ignored/skip.ts"), "").expect("write ignored file");

        let walked: Vec<PathBuf> = ignore::WalkBuilder::new(worktree.path())
            .build()
            .filter_map(Result::ok)
            .map(|entry| entry.into_path())
            .collect();

        assert!(walked.iter().any(|path| path.ends_with("tracked.txt")));
        assert!(
            !walked.iter().any(|path| path.ends_with("ignored/skip.ts")),
            "the base walk must honor .gitignore: {walked:?}"
        );
        assert_eq!(
            enclosing_unregistered_snapshot(&worktree.path().join("ignored")).as_deref(),
            Some(worktree.path())
        );
    }
}
