//! Checks for the ignore files that a directory walk reads.
//!
//! The `ignore` crate opens the `.gitignore`, `.ignore` and
//! `.git/info/exclude` files of each directory that a walk enters, the same
//! files of each parent directory of the walk root, and the global excludes
//! file of Git. It reads each file to its end. A FIFO never ends and a link to
//! `/dev/zero` never ends, so such a file stops the walk or fills the memory.
//!
//! The walkers here check each of these files before the crate opens it. A
//! file passes when it is a regular file of at most [`MAX_IGNORE_FILE_BYTES`],
//! or a link to such a file inside the project. Each other file is skipped with
//! one warning:
//!
//! - A bad file in the walk root or in a parent directory turns off that kind
//!   of ignore file for the whole walk.
//! - A bad file in a directory below the root removes that directory from the
//!   walk, because the crate reads all ignore files of a directory that it
//!   enters.

use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use ignore::{DirEntry, WalkBuilder};
use rustc_hash::FxHashSet;

#[path = "ignore_parent_rules.rs"]
mod parent_rules;

/// The largest ignore file that a walk reads.
const MAX_IGNORE_FILE_BYTES: u64 = 1024 * 1024;

/// The ignore files that a walk can read in addition to `.ignore` files.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IgnoreFileSources {
    /// Read `.gitignore` files.
    pub git_ignore: bool,
    /// Read the `.git/info/exclude` file of each repository.
    pub git_exclude: bool,
    /// Read the global excludes file of Git.
    pub git_global: bool,
}

/// Why a walk does not read an ignore file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Refusal {
    NotRegularFile,
    TargetOutsideProject,
    TooLarge,
    NoMetadata,
}

impl Refusal {
    const fn describe(self) -> &'static str {
        match self {
            Self::NotRegularFile => "it is not a regular file",
            Self::TargetOutsideProject => "it links to a path outside the project",
            Self::TooLarge => "it is larger than 1 MiB",
            Self::NoMetadata => "its file type cannot be read",
        }
    }
}

/// Check an ignore file before a reader opens it.
///
/// `Ok(false)` means that a reader does not open the file: the file is absent,
/// it is a directory, or it is a link that does not resolve. `Ok(true)` means
/// that the file is safe to read. A link target must be inside `scope` when
/// `scope` is set.
fn check(path: &Path, scope: Option<&Path>) -> Result<bool, Refusal> {
    let metadata = match std::fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if is_absent(&error) => return Ok(false),
        Err(_) => return Err(Refusal::NoMetadata),
    };
    let metadata = if metadata.file_type().is_symlink() {
        // A link whose target is missing is absent: the `ignore` crate skips it
        // through `Path::exists`, and on Windows its open fails. A link that
        // does not resolve for another reason, such as a loop, is refused.
        let Ok(target) = dunce::canonicalize(path) else {
            return match std::fs::metadata(path) {
                Err(error) if is_absent(&error) => Ok(false),
                _ => Err(Refusal::NoMetadata),
            };
        };
        if scope.is_some_and(|scope| !target.starts_with(scope)) {
            return Err(Refusal::TargetOutsideProject);
        }
        std::fs::metadata(&target).map_err(|_| Refusal::NoMetadata)?
    } else {
        metadata
    };
    if metadata.is_dir() {
        return Ok(false);
    }
    if !metadata.is_file() {
        return Err(Refusal::NotRegularFile);
    }
    if metadata.len() > MAX_IGNORE_FILE_BYTES {
        return Err(Refusal::TooLarge);
    }
    Ok(true)
}

/// True when an error means that no file is at the path. A path below a file,
/// such as `.git/info/exclude` when `.git` is the file of a linked worktree,
/// gives `NotADirectory`.
fn is_absent(error: &std::io::Error) -> bool {
    matches!(
        error.kind(),
        std::io::ErrorKind::NotFound | std::io::ErrorKind::NotADirectory
    )
}

/// Give the warning for a skipped ignore file once per process.
fn warn_skipped(path: &Path, refusal: Refusal, consequence: &str) {
    static WARNED: OnceLock<Mutex<FxHashSet<PathBuf>>> = OnceLock::new();
    let first = WARNED
        .get_or_init(Mutex::default)
        .lock()
        .map_or(true, |mut warned| warned.insert(path.to_path_buf()));
    if first {
        tracing::warn!(
            "skipped the ignore file {path} because {reason}; {consequence}",
            path = path.display(),
            reason = refusal.describe(),
        );
    }
}

/// True when the ignore file at `path` exists and is safe to read.
///
/// A reader that builds a matcher from named ignore files calls this before
/// each read. A link target must be inside the project of `root`, the same
/// scope that a walk from `root` uses. A skipped file gets one warning.
#[must_use]
pub fn readable_ignore_file(path: &Path, root: &Path) -> bool {
    let canonical_root = dunce::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let scope = project_scope(&canonical_root);
    match check(path, Some(&scope)) {
        Ok(exists) => exists,
        Err(refusal) => {
            warn_skipped(path, refusal, "its rules do not apply");
            false
        }
    }
}

/// The directory that holds the project of `root`: the nearest directory at or
/// above the canonical root that has a `.git` entry, else the canonical root.
fn project_scope(canonical_root: &Path) -> PathBuf {
    canonical_root
        .ancestors()
        .find(|dir| has_git_entry(dir))
        .unwrap_or(canonical_root)
        .to_path_buf()
}

fn has_git_entry(dir: &Path) -> bool {
    std::fs::symlink_metadata(dir.join(".git")).is_ok()
}

/// True when the `ignore` crate may apply the parent ignore files of a walk
/// inside the repository at `top`: no directory above `top` holds a
/// `.ignore` file, and only the current user or root can create a file in
/// each of them (see [`fallow_types::read_scope::only_owner_writes`]).
fn crate_may_read_parents(top: &Path, reads: DirReads) -> bool {
    top.ancestors().skip(1).all(|dir| {
        fallow_types::read_scope::only_owner_writes(dir)
            && !(reads.dot_ignore && std::fs::symlink_metadata(dir.join(".ignore")).is_ok())
    })
}

/// The ignore files of one directory that a walk reads.
#[derive(Debug, Clone, Copy)]
struct DirReads {
    dot_ignore: bool,
    sources: IgnoreFileSources,
}

impl DirReads {
    /// The first ignore file of `dir` that the walk reads and that fails the
    /// check, with the reason. A link target must be inside `scope` when
    /// `scope` is set.
    fn first_refusal(
        self,
        dir: &Path,
        scope: Option<&Path>,
    ) -> Option<(PathBuf, Refusal, &'static str)> {
        let named = [
            (self.dot_ignore, ".ignore"),
            (self.sources.git_ignore, ".gitignore"),
        ];
        for (read, name) in named {
            let path = dir.join(name);
            if read && let Err(refusal) = check(&path, scope) {
                return Some((path, refusal, name));
            }
        }
        if self.sources.git_exclude {
            return git_exclude_refusal(dir, scope)
                .map(|(path, refusal)| (path, refusal, ".git/info/exclude"));
        }
        None
    }
}

/// The first file that the `ignore` crate reads for the exclude rules of
/// `dir` and that fails the check.
///
/// A `.git` directory holds `info/exclude`. A `.git` file (a linked worktree)
/// names the repository directory on its first line (`gitdir: <path>`). The
/// `commondir` file of that directory names the shared directory that holds
/// `info/exclude`. The crate reads these files in this order, the same as
/// `resolve_git_commondir` in `ignore`. The files that the `.git` file names
/// can be outside the project, so they get only the type and size check.
fn git_exclude_refusal(dir: &Path, scope: Option<&Path>) -> Option<(PathBuf, Refusal)> {
    let refused = |path: PathBuf, scope: Option<&Path>| {
        check(&path, scope).err().map(|refusal| (path, refusal))
    };
    let git = dir.join(".git");
    let metadata = std::fs::metadata(&git).ok()?;
    if metadata.is_dir() {
        return refused(git.join("info").join("exclude"), scope);
    }
    if !metadata.is_file() {
        return None;
    }
    if let Some(refusal) = refused(git.clone(), scope) {
        return Some(refusal);
    }
    let git_dir = PathBuf::from(first_line(&git)?.strip_prefix("gitdir: ")?);
    let commondir_file = git_dir.join("commondir");
    if let Some(refusal) = refused(commondir_file.clone(), None) {
        return Some(refusal);
    }
    let commondir = first_line(&commondir_file)?;
    let common = if commondir.starts_with('.') {
        git_dir.join(commondir)
    } else {
        PathBuf::from(commondir)
    };
    refused(common.join("info").join("exclude"), None)
}

/// The first line of a file that passed the check, as `BufRead::lines` gives
/// it: without a `\n` line end, and without a `\r` only when a `\n` follows
/// it. `None` when the file is absent or empty, or the line is not valid
/// UTF-8, the cases in which the `ignore` crate reads no exclude file.
fn first_line(path: &Path) -> Option<String> {
    let bytes = fallow_types::read_guard::read_bytes_bounded(path, MAX_IGNORE_FILE_BYTES).ok()?;
    if bytes.is_empty() {
        return None;
    }
    let line = match bytes.iter().position(|byte| *byte == b'\n') {
        Some(end) => {
            let line = &bytes[..end];
            line.strip_suffix(b"\r").unwrap_or(line)
        }
        None => bytes.as_slice(),
    };
    String::from_utf8(line.to_vec()).ok()
}

/// The ignore files of the walk root and of its parent directories that pass
/// the check. A kind of ignore file with a skipped file is turned off.
fn checked_root_reads(canonical_root: &Path, scope: &Path, sources: IgnoreFileSources) -> DirReads {
    let mut reads = DirReads {
        dot_ignore: true,
        sources,
    };
    for dir in canonical_root.ancestors() {
        // A file above the project belongs to the user, for example a
        // `~/.gitignore` that links to a dotfiles repository, so only its type
        // and size are checked.
        let file_scope = dir.starts_with(scope).then_some(scope);
        while let Some((path, refusal, name)) = reads.first_refusal(dir, file_scope) {
            let consequence = format!("the walk reads no {name} files");
            warn_skipped(&path, refusal, &consequence);
            match name {
                ".ignore" => reads.dot_ignore = false,
                ".gitignore" => reads.sources.git_ignore = false,
                _ => reads.sources.git_exclude = false,
            }
        }
    }
    reads.sources.git_global &= global_excludes_readable();
    reads
}

/// True unless the global excludes file of Git fails the check. The file is
/// outside the project, so a link to it can point anywhere.
fn global_excludes_readable() -> bool {
    let Some(path) = ignore::gitignore::gitconfig_excludes_path() else {
        return true;
    };
    match check(&path, None) {
        Ok(_) => true,
        Err(refusal) => {
            warn_skipped(&path, refusal, "its rules do not apply");
            false
        }
    }
}

/// The matcher of the global excludes file of Git, after the check of the
/// file. A skipped file gives an empty matcher.
#[must_use]
pub fn checked_global_gitignore() -> ignore::gitignore::Gitignore {
    if global_excludes_readable() {
        ignore::gitignore::Gitignore::global().0
    } else {
        ignore::gitignore::Gitignore::empty()
    }
}

/// The walker that the caller runs on a builder.
///
/// The two walkers of the `ignore` crate find the ignore files of a walked
/// directory in different ways: the serial walker opens them by path, and
/// the parallel walker takes their names from the directory listing. On a
/// file system that ignores case the two differ for a name such as
/// `.GitIgnore`, and the walk follows the walker that runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WalkKind {
    /// [`WalkBuilder::build`].
    Serial,
    /// [`WalkBuilder::build_parallel`].
    Parallel,
}

/// Make a directory walker that checks each ignore file before the `ignore`
/// crate reads it. The caller runs it with the walker `kind`.
///
/// `filter` is the entry filter of the caller. Do not call
/// [`WalkBuilder::filter_entry`] on the result: it replaces the filter that
/// checks the ignore files of each directory. Hidden entries are not skipped.
#[must_use]
pub fn checked_walk_builder<F>(
    root: &Path,
    sources: IgnoreFileSources,
    kind: WalkKind,
    filter: F,
) -> WalkBuilder
where
    F: Fn(&DirEntry) -> bool + Send + Sync + 'static,
{
    let canonical_root = dunce::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let scope = project_scope(&canonical_root);
    let reads = checked_root_reads(&canonical_root, &scope, sources);
    // A walk inside a repository applies no ignore rule from above the top
    // level, the same as Git. The `ignore` crate stops `.gitignore` rules at
    // a repository but applies each parent `.ignore` file. An audit walks its
    // base snapshot in the temp dir, where these files belong to whoever can
    // write the temp dir. The crate keeps its own parent rules only when no
    // `.ignore` is above the top level and only the user or root can create
    // one there, so no other user can add one between this check and the
    // read. Otherwise the crate gets `parents(false)`. A root below the top
    // level then also needs the rules between the root and the top level:
    // `ParentRules` applies them with the precedence of the crate, and
    // decides which rules the crate keeps.
    let in_repository = has_git_entry(&scope);
    let crate_parents = !in_repository || crate_may_read_parents(&scope, reads);
    let emulated = !crate_parents && scope != canonical_root;
    let parent_rules = emulated
        .then(|| parent_rules::ParentRules::load(root, &canonical_root, &scope, reads, kind))
        .flatten();
    let crate_reads_walk = parent_rules.as_ref().is_none_or(|rules| !rules.full());
    let mut builder = WalkBuilder::new(root);
    builder
        .hidden(false)
        .parents(crate_parents)
        .ignore(reads.dot_ignore && crate_reads_walk)
        .git_ignore(reads.sources.git_ignore && crate_reads_walk)
        .git_global(reads.sources.git_global && parent_rules.is_none())
        .git_exclude(reads.sources.git_exclude && crate_reads_walk)
        .filter_entry(move |entry| {
            let is_dir = entry.file_type().is_some_and(|kind| kind.is_dir());
            if entry.depth() > 0
                && is_dir
                && let Some((path, refusal, _)) = reads.first_refusal(entry.path(), Some(&scope))
            {
                warn_skipped(&path, refusal, "the walk skips its directory");
                return false;
            }
            if let Some(rules) = &parent_rules
                && rules.ignores(entry.path(), is_dir)
            {
                return false;
            }
            filter(entry)
        });
    builder
}

/// Base directories for the tests of the parent rules of a walk.
#[cfg(all(test, unix))]
pub mod test_bases {
    use std::path::Path;

    /// A base directory under the home directory, above which only the user
    /// or root can create a file and no `.ignore` file is present. A test
    /// temp dir can be below a directory that all users can write, such as
    /// `/tmp`. `None`, with a message, when the home directory gives no such
    /// base.
    #[must_use]
    #[expect(
        clippy::print_stderr,
        reason = "a test that skips its private base case names the reason"
    )]
    pub fn private_base_directory() -> Option<tempfile::TempDir> {
        let found = private_base();
        if let Err(reason) = &found {
            eprintln!("skipped the private base case: {reason}");
        }
        found.ok()
    }

    fn private_base() -> Result<tempfile::TempDir, String> {
        let home = std::env::var_os("HOME").ok_or("HOME is not set")?;
        let directory = tempfile::Builder::new()
            .prefix("fallow-walk-test-")
            .tempdir_in(home)
            .map_err(|error| format!("no temp dir in the home directory: {error}"))?;
        let base = dunce::canonicalize(directory.path())
            .map_err(|error| format!("the temp dir does not resolve: {error}"))?;
        let private = base.ancestors().all(|dir| {
            fallow_types::read_scope::only_owner_writes(dir)
                && std::fs::symlink_metadata(dir.join(".ignore")).is_err()
        });
        if !private {
            return Err(format!(
                "a directory above {} can get a file from another user, or holds a \
                 .ignore file",
                base.display()
            ));
        }
        Ok(directory)
    }

    /// A base directory that all users can write.
    #[must_use]
    pub fn shared_base_directory() -> tempfile::TempDir {
        let directory = tempfile::tempdir().expect("temporary directory");
        set_mode(directory.path(), 0o777);
        directory
    }

    fn set_mode(path: &Path, mode: u32) {
        use std::os::unix::fs::PermissionsExt as _;

        std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode))
            .expect("set the mode of the base directory");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The crate keeps its parent rules only when no `.ignore` is above the
    /// top level and only the user or root can add one there.
    #[cfg(unix)]
    #[test]
    fn the_crate_reads_parent_rules_only_below_directories_that_others_cannot_write() {
        use std::os::unix::fs::PermissionsExt as _;

        let reads = DirReads {
            dot_ignore: true,
            sources: IgnoreFileSources {
                git_ignore: true,
                git_exclude: true,
                git_global: true,
            },
        };
        let repository_in = |base: &Path| {
            let top = base.join("repo");
            std::fs::create_dir_all(top.join(".git")).expect("git directory");
            top
        };
        if let Some(directory) = test_bases::private_base_directory() {
            let base = dunce::canonicalize(directory.path()).expect("canonical base");
            assert!(crate_may_read_parents(&repository_in(&base), reads));
        }

        let directory = tempfile::tempdir().expect("temporary directory");
        let base = dunce::canonicalize(directory.path()).expect("canonical base");
        let top = repository_in(&base);
        std::fs::write(base.join(".ignore"), "x\n").expect("ignore file");
        assert!(!crate_may_read_parents(&top, reads));
        std::fs::remove_file(base.join(".ignore")).expect("remove ignore file");

        std::fs::set_permissions(&base, std::fs::Permissions::from_mode(0o777))
            .expect("make the base writable by all");
        assert!(!crate_may_read_parents(&top, reads));
    }

    #[cfg(unix)]
    #[test]
    fn a_link_loop_is_refused_and_a_dangling_link_is_absent() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let looped = directory.path().join(".gitignore");
        std::os::unix::fs::symlink(&looped, &looped).expect("link to itself");
        let dangling = directory.path().join(".ignore");
        std::os::unix::fs::symlink(directory.path().join("missing"), &dangling)
            .expect("dangling link");

        assert_eq!(check(&looped, None), Err(Refusal::NoMetadata));
        assert_eq!(check(&dangling, None), Ok(false));
    }

    #[test]
    fn an_exclude_path_below_a_git_file_is_absent() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let root = directory.path();
        std::fs::write(root.join(".git"), "gitdir: /missing/worktree\n").expect("git file");

        assert_eq!(check(&root.join(".git/info/exclude"), None), Ok(false));
    }
}
