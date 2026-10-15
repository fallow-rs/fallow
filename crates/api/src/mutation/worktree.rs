//! The git worktree guard of a run.
//!
//! A mutant can turn a guarded write into an unguarded one, and the run has
//! no sandbox copy. The run therefore starts only in a clean worktree unless
//! the caller allows a dirty one, and it compares the worktree after the run
//! with the start state.
//!
//! The engine models refs, the toplevel and diffs, but not the worktree
//! status, so this adapter spawns `git status` itself. That is an
//! adapter-level git spawn for a fact the engine does not model, as
//! `docs/architecture-invariants.md` allows. The toplevel comes from
//! `fallow_engine::changed_files::resolve_git_toplevel`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use fallow_engine::changed_files::{
    ChangedFilesError, clear_ambient_git_env, resolve_git_toplevel,
};
use sha2::{Digest, Sha256};

/// The most ignored files that a start snapshot fingerprints. The run checks
/// the first ones in path order.
const MAX_IGNORED_FILES: usize = 10_000;
/// The largest ignored file that a snapshot reads for a digest. A larger file
/// is compared by its size and modification time.
const MAX_IGNORED_DIGEST_BYTES: u64 = 1024 * 1024;
/// The porcelain status of an ignored path.
const IGNORED_CODE: &str = "!!";

/// The state of one dirty path.
#[derive(Debug, Clone, PartialEq, Eq)]
struct DirtyEntry {
    /// The two-letter porcelain status.
    code: String,
    /// The SHA-256 digest of the file content, when the file exists.
    digest: Option<String>,
}

/// The paths that a snapshot leaves out.
#[derive(Debug, Clone, Default)]
pub struct Exclusions {
    /// The prefixes that each check leaves out: the mutation cache and the
    /// output files of the run.
    pub paths: Vec<PathBuf>,
    /// The prefixes that only the ignored-file check leaves out, such as the
    /// whole Fallow cache, which the analysis writes during a run.
    pub ignored: Vec<PathBuf>,
}

/// The dirty paths of a worktree, relative to its toplevel, and the
/// fingerprints of its ignored files.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WorktreeSnapshot {
    entries: BTreeMap<String, DirtyEntry>,
    /// The fingerprint of each ignored file. An end snapshot holds the new
    /// fingerprints of the ignored files of its start snapshot, and `None`
    /// for a file that is gone.
    ignored: BTreeMap<String, Option<String>>,
}

impl WorktreeSnapshot {
    /// The dirty paths, sorted. An ignored file is not dirty.
    pub fn dirty_paths(&self) -> Vec<String> {
        self.entries.keys().cloned().collect()
    }

    /// The paths whose status or content differs between `self` and `after`,
    /// sorted. An ignored file of `self` is a change when its fingerprint in
    /// `after` is different.
    pub fn changed_paths(&self, after: &Self) -> Vec<String> {
        let mut changed: Vec<String> = self
            .entries
            .iter()
            .filter(|(path, entry)| after.entries.get(*path) != Some(entry))
            .map(|(path, _)| path.clone())
            .chain(
                after
                    .entries
                    .keys()
                    .filter(|path| !self.entries.contains_key(*path))
                    .cloned(),
            )
            .chain(
                self.ignored
                    .iter()
                    .filter(|(path, fingerprint)| after.ignored.get(*path) != Some(fingerprint))
                    .map(|(path, _)| path.clone()),
            )
            .collect();
        changed.sort();
        changed.dedup();
        changed
    }
}

/// Parses `git status --porcelain=v1 -z --no-renames` output into
/// `(code, path)` pairs.
fn parse_porcelain(output: &[u8]) -> Vec<(String, String)> {
    output
        .split(|byte| *byte == 0)
        .filter(|entry| entry.len() > 3)
        .map(|entry| {
            let text = String::from_utf8_lossy(entry);
            (text[..2].to_owned(), text[3..].to_owned())
        })
        .collect()
}

fn digest(path: &Path) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    let mut hex = String::with_capacity(64);
    for byte in Sha256::digest(&bytes) {
        use std::fmt::Write as _;
        let _ = write!(hex, "{byte:02x}");
    }
    Some(hex)
}

/// The fingerprint of an ignored file: the content digest of a small file,
/// or the size and modification time of a large one. `None` when the path is
/// missing or is not a regular file.
fn ignored_fingerprint(path: &Path) -> Option<String> {
    let metadata = std::fs::symlink_metadata(path).ok()?;
    if !metadata.is_file() {
        return None;
    }
    if metadata.len() <= MAX_IGNORED_DIGEST_BYTES {
        return digest(path);
    }
    let modified = metadata
        .modified()
        .ok()
        .and_then(|time| time.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0, |elapsed| elapsed.as_nanos());
    Some(format!("size:{}:modified:{modified}", metadata.len()))
}

/// The canonical form of `path`. A path that does not exist yet, such as a
/// cache directory before the first run, keeps its missing names below the
/// canonical form of its nearest existing ancestor.
pub fn canonical_path(path: &Path) -> PathBuf {
    let mut existing = path;
    let mut missing = Vec::new();
    loop {
        if let Ok(canonical) = dunce::canonicalize(existing) {
            return missing
                .iter()
                .rev()
                .fold(canonical, |base, name| base.join(name));
        }
        match (existing.parent(), existing.file_name()) {
            (Some(parent), Some(name)) => {
                missing.push(name.to_os_string());
                existing = parent;
            }
            _ => return path.to_path_buf(),
        }
    }
}

fn canonical(paths: &[PathBuf]) -> Vec<PathBuf> {
    paths.iter().map(|path| canonical_path(path)).collect()
}

fn is_below(absolute: &Path, prefixes: &[PathBuf]) -> bool {
    prefixes.iter().any(|prefix| absolute.starts_with(prefix))
}

/// The fingerprints of the first `MAX_IGNORED_FILES` ignored files in path
/// order. Git lists an ignored directory as one entry with a trailing slash,
/// so the files below it are not fingerprinted.
fn fingerprint_ignored(
    toplevel: &Path,
    mut paths: Vec<String>,
    excluded: &[PathBuf],
) -> BTreeMap<String, Option<String>> {
    paths.retain(|path| !path.ends_with('/') && !is_below(&toplevel.join(path), excluded));
    paths.sort();
    paths
        .into_iter()
        .take(MAX_IGNORED_FILES)
        .filter_map(|path| {
            let fingerprint = ignored_fingerprint(&toplevel.join(&path))?;
            Some((path, Some(fingerprint)))
        })
        .collect()
}

/// Takes a snapshot of the worktree that contains `root`. Paths below one of
/// `exclusions` (Fallow's own files) are left out.
///
/// A start snapshot (`start` is `None`) also fingerprints the ignored files
/// that `git status --ignored=matching` lists. An end snapshot fingerprints
/// the ignored files of `start` again. A new ignored file, or a file below
/// an ignored directory, is therefore not a change.
///
/// Returns `None` when `root` is not in a git worktree.
///
/// # Errors
///
/// Returns an error when git is missing or `git status` fails.
pub fn snapshot(
    root: &Path,
    exclusions: &Exclusions,
    start: Option<&WorktreeSnapshot>,
) -> Result<Option<WorktreeSnapshot>, String> {
    let toplevel = match resolve_git_toplevel(root) {
        Ok(toplevel) => toplevel,
        Err(ChangedFilesError::NotARepository) => return Ok(None),
        Err(error) => return Err(error.describe()),
    };
    let mut command = Command::new("git");
    command.args([
        "status",
        "--porcelain=v1",
        "-z",
        "--no-renames",
        "--untracked-files=all",
    ]);
    if start.is_none() {
        command.arg("--ignored=matching");
    }
    command.current_dir(&toplevel).stdin(Stdio::null());
    clear_ambient_git_env(&mut command);
    let output = fallow_process::output(&mut command)
        .map_err(|error| format!("cannot run git status: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "git status failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let excluded = canonical(&exclusions.paths);
    let mut entries = BTreeMap::new();
    let mut ignored_paths = Vec::new();
    for (code, path) in parse_porcelain(&output.stdout) {
        let absolute = toplevel.join(&path);
        if is_below(&absolute, &excluded) {
            continue;
        }
        if code == IGNORED_CODE {
            ignored_paths.push(path);
        } else {
            let digest = digest(&absolute);
            entries.insert(path, DirtyEntry { code, digest });
        }
    }
    let ignored = match start {
        Some(start) => start
            .ignored
            .keys()
            .map(|path| (path.clone(), ignored_fingerprint(&toplevel.join(path))))
            .collect(),
        None => fingerprint_ignored(&toplevel, ignored_paths, &canonical(&exclusions.ignored)),
    };
    Ok(Some(WorktreeSnapshot { entries, ignored }))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn git(root: &Path, args: &[&str]) {
        let mut command = Command::new("git");
        command.args(args).current_dir(root);
        clear_ambient_git_env(&mut command);
        let output = command.output().expect("git runs");
        assert!(output.status.success(), "git {args:?}: {output:?}");
    }

    fn repo() -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("dir");
        git(dir.path(), &["init", "-q"]);
        git(dir.path(), &["config", "user.email", "test@example.com"]);
        git(dir.path(), &["config", "user.name", "Test"]);
        git(dir.path(), &["config", "commit.gpgsign", "false"]);
        std::fs::write(dir.path().join("a.ts"), "export const a = 1;\n").expect("write");
        git(dir.path(), &["add", "."]);
        git(dir.path(), &["commit", "-q", "-m", "init"]);
        dir
    }

    fn take(root: &Path, exclusions: &Exclusions) -> WorktreeSnapshot {
        snapshot(root, exclusions, None)
            .expect("status")
            .expect("repo")
    }

    fn take_end(
        root: &Path,
        exclusions: &Exclusions,
        start: &WorktreeSnapshot,
    ) -> WorktreeSnapshot {
        snapshot(root, exclusions, Some(start))
            .expect("status")
            .expect("repo")
    }

    #[test]
    fn porcelain_entries_parse_into_code_and_path() {
        assert_eq!(
            parse_porcelain(b" M src/a.ts\0?? new file.ts\0"),
            vec![
                (" M".to_owned(), "src/a.ts".to_owned()),
                ("??".to_owned(), "new file.ts".to_owned())
            ]
        );
    }

    #[test]
    fn a_clean_tree_has_no_dirty_path_and_an_edit_is_a_change() {
        let dir = repo();
        let none = Exclusions::default();
        let start = take(dir.path(), &none);
        assert!(start.dirty_paths().is_empty());
        std::fs::write(dir.path().join("a.ts"), "export const a = 2;\n").expect("write");
        std::fs::write(dir.path().join("b.ts"), "x").expect("write");
        let end = take_end(dir.path(), &none, &start);
        assert_eq!(end.dirty_paths(), vec!["a.ts", "b.ts"]);
        assert_eq!(start.changed_paths(&end), vec!["a.ts", "b.ts"]);
    }

    #[test]
    fn a_second_edit_of_a_dirty_file_is_a_change_and_no_edit_is_none() {
        let dir = repo();
        let none = Exclusions::default();
        std::fs::write(dir.path().join("a.ts"), "export const a = 2;\n").expect("write");
        let start = take(dir.path(), &none);
        let same = take_end(dir.path(), &none, &start);
        assert_eq!(start.changed_paths(&same), Vec::<String>::new());
        std::fs::write(dir.path().join("a.ts"), "export const a = 3;\n").expect("write");
        let end = take_end(dir.path(), &none, &start);
        assert_eq!(start.changed_paths(&end), vec!["a.ts"]);
    }

    #[test]
    fn excluded_directories_and_non_repositories_are_left_out() {
        let dir = repo();
        let cache = dir.path().join(".fallow");
        std::fs::create_dir_all(cache.join("mutation")).expect("dir");
        std::fs::write(cache.join("mutation/x"), "x").expect("write");
        std::fs::write(dir.path().join("c.ts"), "x").expect("write");
        let exclusions = Exclusions {
            paths: vec![cache.join("mutation")],
            ignored: Vec::new(),
        };
        assert_eq!(take(dir.path(), &exclusions).dirty_paths(), vec!["c.ts"]);

        let plain = tempfile::tempdir().expect("dir");
        assert_eq!(
            snapshot(plain.path(), &exclusions, None).expect("status"),
            None
        );
    }

    #[test]
    fn a_write_to_an_existing_ignored_file_is_a_change_and_a_new_one_is_not() {
        let dir = repo();
        std::fs::write(dir.path().join(".gitignore"), ".env\n*.log\ncoverage/\n").expect("write");
        git(dir.path(), &["add", ".gitignore"]);
        git(dir.path(), &["commit", "-q", "-m", "ignore"]);
        std::fs::write(dir.path().join(".env"), "KEY=1\n").expect("write");
        std::fs::write(dir.path().join("kept.log"), "x").expect("write");
        let cache = dir.path().join(".fallow");
        std::fs::create_dir_all(&cache).expect("dir");
        std::fs::write(cache.join(".gitignore"), "*\n").expect("write");
        std::fs::write(cache.join("cache.bin"), "1").expect("write");
        let exclusions = Exclusions {
            paths: vec![cache.join("mutation")],
            ignored: vec![cache.clone()],
        };
        let start = take(dir.path(), &exclusions);
        assert!(
            start.dirty_paths().is_empty(),
            "an ignored file is not dirty"
        );

        std::fs::write(dir.path().join("new.log"), "x").expect("write");
        std::fs::create_dir_all(dir.path().join("coverage")).expect("dir");
        std::fs::write(dir.path().join("coverage/out.json"), "{}").expect("write");
        std::fs::write(cache.join("cache.bin"), "2").expect("write");
        let unchanged = take_end(dir.path(), &exclusions, &start);
        assert_eq!(start.changed_paths(&unchanged), Vec::<String>::new());

        std::fs::write(dir.path().join(".env"), "KEY=2\n").expect("write");
        std::fs::remove_file(dir.path().join("kept.log")).expect("remove");
        let end = take_end(dir.path(), &exclusions, &start);
        assert_eq!(start.changed_paths(&end), vec![".env", "kept.log"]);
    }

    #[test]
    fn a_missing_path_is_canonical_below_its_nearest_existing_ancestor() {
        let dir = tempfile::tempdir().expect("dir");
        let base = dunce::canonicalize(dir.path()).expect("base");
        assert_eq!(
            canonical_path(&dir.path().join("cache/mutation")),
            base.join("cache/mutation")
        );
        assert_eq!(canonical_path(&dir.path().join(".")), base);
    }

    #[test]
    fn a_large_ignored_file_is_compared_by_size_and_modification_time() {
        let dir = tempfile::tempdir().expect("dir");
        let path = dir.path().join("big.bin");
        let size = usize::try_from(MAX_IGNORED_DIGEST_BYTES).expect("size") + 1;
        std::fs::write(&path, vec![b'a'; size]).expect("write");
        let before = ignored_fingerprint(&path).expect("fingerprint");
        assert!(before.starts_with("size:"), "{before}");
        std::fs::write(&path, vec![b'a'; size + 1]).expect("write");
        assert_ne!(ignored_fingerprint(&path), Some(before));
        assert_eq!(
            ignored_fingerprint(dir.path()),
            None,
            "a directory has none"
        );
    }
}
