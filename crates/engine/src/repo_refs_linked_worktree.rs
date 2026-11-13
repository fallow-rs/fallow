//! Linked worktrees: the private worktree root, and the tree object and the
//! temporary worktree of a staged index.

use std::fs;
use std::path::Path;

use crate::{EngineError, EngineResult};

use super::{
    git_command, materialize_committed_tree, populate_worktree_index,
    register_no_checkout_worktree, remove_registered_worktree,
};

/// Write a Git index to a tree object and return the object id. `index_file`
/// is the index to read. Without it, Git reads the index of `repo_root`.
///
/// # Errors
///
/// Returns an engine error when Git cannot run, or the index has unmerged
/// entries, or Git prints no tree id.
pub fn write_index_tree(repo_root: &Path, index_file: Option<&Path>) -> EngineResult<String> {
    let mut command = git_command(repo_root);
    if let Some(index_file) = index_file {
        command.env("GIT_INDEX_FILE", index_file);
    }
    let output = command
        .arg("write-tree")
        .output()
        .map_err(|error| EngineError::new(format!("could not run git: {error}")))?;
    if !output.status.success() {
        return Err(EngineError::new(format!(
            "could not read the staged index (resolve unmerged paths first): {}",
            String::from_utf8_lossy(&output.stderr).trim()
        )));
    }
    let tree = String::from_utf8_lossy(&output.stdout).trim().to_owned();
    if tree.is_empty() {
        return Err(EngineError::new("git write-tree printed no tree id"));
    }
    Ok(tree)
}

/// Register a detached worktree at `HEAD` whose index and files hold the
/// staged tree `staged_tree` (the object id that `git write-tree` prints).
///
/// In the new worktree, `git diff HEAD` shows the staged changes of the host
/// checkout. The files come from Git objects, like in
/// [`super::create_detached_base_worktree`], so no checkout hook or filter runs.
/// The worktree stays registered, because the audit runs Git in it. The
/// caller removes it after the run.
///
/// # Errors
///
/// Returns an engine error when the destination is not absolute, Git cannot
/// create the worktree, or a staged object cannot be materialized. A failed
/// materialization removes both the registration and the partial directory.
pub fn create_staged_index_worktree(
    repo_root: &Path,
    destination: &Path,
    staged_tree: &str,
) -> EngineResult<()> {
    if !destination.is_absolute() {
        return Err(EngineError::new(format!(
            "staged worktree destination must be absolute: {}",
            destination.display()
        )));
    }

    register_no_checkout_worktree(repo_root, destination, "HEAD")?;
    let result = make_worktree_root_private(destination)
        .and_then(|()| populate_worktree_index(destination, staged_tree))
        .and_then(|()| materialize_committed_tree(repo_root, destination, staged_tree));
    if let Err(error) = result {
        remove_staged_index_worktree(repo_root, destination);
        return Err(error);
    }
    Ok(())
}

/// Remove a worktree that [`create_staged_index_worktree`] registered: the
/// Git registration and the directory.
pub fn remove_staged_index_worktree(repo_root: &Path, destination: &Path) {
    remove_registered_worktree(repo_root, destination);
    let _ = fs::remove_dir_all(destination);
}

#[cfg(unix)]
pub(super) fn make_worktree_root_private(destination: &Path) -> EngineResult<()> {
    use std::os::unix::fs::PermissionsExt as _;

    fs::set_permissions(destination, fs::Permissions::from_mode(0o700)).map_err(|error| {
        EngineError::new(format!(
            "could not make base worktree private at `{}`: {error}",
            destination.display()
        ))
    })
}

#[cfg(not(unix))]
#[expect(
    clippy::unnecessary_wraps,
    reason = "shared cross-platform signature; Unix applies privacy permissions"
)]
pub(super) fn make_worktree_root_private(_destination: &Path) -> EngineResult<()> {
    Ok(())
}
