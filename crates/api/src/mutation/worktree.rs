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

/// The state of one dirty path.
#[derive(Debug, Clone, PartialEq, Eq)]
struct DirtyEntry {
    /// The two-letter porcelain status.
    code: String,
    /// The SHA-256 digest of the file content, when the file exists.
    digest: Option<String>,
}

/// The dirty paths of a worktree, relative to its toplevel.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WorktreeSnapshot {
    entries: BTreeMap<String, DirtyEntry>,
}

impl WorktreeSnapshot {
    /// The dirty paths, sorted.
    pub fn dirty_paths(&self) -> Vec<String> {
        self.entries.keys().cloned().collect()
    }

    /// The paths whose status or content differs between `self` and `after`,
    /// sorted.
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

/// Takes a snapshot of the worktree that contains `root`. Paths below one of
/// `excluded` (Fallow's own cache) are left out.
///
/// Returns `None` when `root` is not in a git worktree.
///
/// # Errors
///
/// Returns an error when git is missing or `git status` fails.
pub fn snapshot(root: &Path, excluded: &[PathBuf]) -> Result<Option<WorktreeSnapshot>, String> {
    let toplevel = match resolve_git_toplevel(root) {
        Ok(toplevel) => toplevel,
        Err(ChangedFilesError::NotARepository) => return Ok(None),
        Err(error) => return Err(error.describe()),
    };
    let mut command = Command::new("git");
    command
        .args([
            "status",
            "--porcelain=v1",
            "-z",
            "--no-renames",
            "--untracked-files=all",
        ])
        .current_dir(&toplevel)
        .stdin(Stdio::null());
    clear_ambient_git_env(&mut command);
    let output = fallow_process::output(&mut command)
        .map_err(|error| format!("cannot run git status: {error}"))?;
    if !output.status.success() {
        return Err(format!(
            "git status failed: {}",
            String::from_utf8_lossy(&output.stderr).trim()
        ));
    }
    let excluded: Vec<PathBuf> = excluded
        .iter()
        .map(|path| dunce::canonicalize(path).unwrap_or_else(|_| path.clone()))
        .collect();
    let entries = parse_porcelain(&output.stdout)
        .into_iter()
        .filter_map(|(code, path)| {
            let absolute = toplevel.join(&path);
            if excluded.iter().any(|dir| absolute.starts_with(dir)) {
                return None;
            }
            let digest = digest(&absolute);
            Some((path, DirtyEntry { code, digest }))
        })
        .collect();
    Ok(Some(WorktreeSnapshot { entries }))
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
        let start = snapshot(dir.path(), &[]).expect("status").expect("repo");
        assert!(start.dirty_paths().is_empty());
        std::fs::write(dir.path().join("a.ts"), "export const a = 2;\n").expect("write");
        std::fs::write(dir.path().join("b.ts"), "x").expect("write");
        let end = snapshot(dir.path(), &[]).expect("status").expect("repo");
        assert_eq!(end.dirty_paths(), vec!["a.ts", "b.ts"]);
        assert_eq!(start.changed_paths(&end), vec!["a.ts", "b.ts"]);
    }

    #[test]
    fn a_second_edit_of_a_dirty_file_is_a_change_and_no_edit_is_none() {
        let dir = repo();
        std::fs::write(dir.path().join("a.ts"), "export const a = 2;\n").expect("write");
        let start = snapshot(dir.path(), &[]).expect("status").expect("repo");
        let same = snapshot(dir.path(), &[]).expect("status").expect("repo");
        assert_eq!(start.changed_paths(&same), Vec::<String>::new());
        std::fs::write(dir.path().join("a.ts"), "export const a = 3;\n").expect("write");
        let end = snapshot(dir.path(), &[]).expect("status").expect("repo");
        assert_eq!(start.changed_paths(&end), vec!["a.ts"]);
    }

    #[test]
    fn excluded_directories_and_non_repositories_are_left_out() {
        let dir = repo();
        let cache = dir.path().join(".fallow");
        std::fs::create_dir_all(cache.join("mutation")).expect("dir");
        std::fs::write(cache.join("mutation/x"), "x").expect("write");
        std::fs::write(dir.path().join("c.ts"), "x").expect("write");
        let snapshot_with_cache = snapshot(dir.path(), &[cache])
            .expect("status")
            .expect("repo");
        assert_eq!(snapshot_with_cache.dirty_paths(), vec!["c.ts"]);

        let plain = tempfile::tempdir().expect("dir");
        assert_eq!(snapshot(plain.path(), &[]).expect("status"), None);
    }
}
