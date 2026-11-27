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
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(false),
        Err(_) => return Err(Refusal::NoMetadata),
    };
    let metadata = if metadata.file_type().is_symlink() {
        // The `ignore` crate opens a file only when `Path::exists` is true, so a
        // link that does not resolve is never read.
        let Ok(target) = dunce::canonicalize(path) else {
            return Ok(false);
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
/// each read. A link target must be inside the directory `scope`. A skipped
/// file gets one warning.
#[must_use]
pub fn readable_ignore_file(path: &Path, scope: &Path) -> bool {
    let scope = dunce::canonicalize(scope).unwrap_or_else(|_| scope.to_path_buf());
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
        .find(|dir| std::fs::symlink_metadata(dir.join(".git")).is_ok())
        .unwrap_or(canonical_root)
        .to_path_buf()
}

/// The ignore files of one directory that a walk reads.
#[derive(Debug, Clone, Copy)]
struct DirReads {
    dot_ignore: bool,
    sources: IgnoreFileSources,
}

impl DirReads {
    /// The first ignore file of `dir` that the walk reads and that fails the
    /// check, with the reason.
    fn first_refusal(self, dir: &Path, scope: &Path) -> Option<(PathBuf, Refusal, &'static str)> {
        let named = [
            (self.dot_ignore, ".ignore"),
            (self.sources.git_ignore, ".gitignore"),
        ];
        for (read, name) in named {
            let path = dir.join(name);
            if read && let Err(refusal) = check(&path, Some(scope)) {
                return Some((path, refusal, name));
            }
        }
        if self.sources.git_exclude {
            let path = git_exclude_input(dir)?;
            if let Err(refusal) = check(&path, Some(scope)) {
                return Some((path, refusal, ".git/info/exclude"));
            }
        }
        None
    }
}

/// The file that the `ignore` crate reads for the exclude rules of `dir`.
///
/// A `.git` directory holds `info/exclude`. A `.git` file (a linked worktree)
/// is read first to find the repository directory.
fn git_exclude_input(dir: &Path) -> Option<PathBuf> {
    let git = dir.join(".git");
    let metadata = std::fs::metadata(&git).ok()?;
    if metadata.is_dir() {
        Some(git.join("info").join("exclude"))
    } else {
        metadata.is_file().then_some(git)
    }
}

/// The ignore files of the walk root and of its parent directories that pass
/// the check. A kind of ignore file with a skipped file is turned off.
fn checked_root_reads(canonical_root: &Path, scope: &Path, sources: IgnoreFileSources) -> DirReads {
    let mut reads = DirReads {
        dot_ignore: true,
        sources,
    };
    for dir in canonical_root.ancestors() {
        let file_scope = if dir.starts_with(scope) { scope } else { dir };
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

/// Make a directory walker that checks each ignore file before the `ignore`
/// crate reads it.
///
/// `filter` is the entry filter of the caller. Do not call
/// [`WalkBuilder::filter_entry`] on the result: it replaces the filter that
/// checks the ignore files of each directory. Hidden entries are not skipped.
#[must_use]
pub fn checked_walk_builder<F>(root: &Path, sources: IgnoreFileSources, filter: F) -> WalkBuilder
where
    F: Fn(&DirEntry) -> bool + Send + Sync + 'static,
{
    let canonical_root = dunce::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let scope = project_scope(&canonical_root);
    let reads = checked_root_reads(&canonical_root, &scope, sources);
    let mut builder = WalkBuilder::new(root);
    builder
        .hidden(false)
        .ignore(reads.dot_ignore)
        .git_ignore(reads.sources.git_ignore)
        .git_global(reads.sources.git_global)
        .git_exclude(reads.sources.git_exclude)
        .filter_entry(move |entry| {
            let is_dir = entry.file_type().is_some_and(|kind| kind.is_dir());
            if entry.depth() > 0
                && is_dir
                && let Some((path, refusal, _)) = reads.first_refusal(entry.path(), &scope)
            {
                warn_skipped(&path, refusal, "the walk skips its directory");
                return false;
            }
            filter(entry)
        });
    builder
}
