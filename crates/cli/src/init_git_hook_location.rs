//! The location of the Git `pre-commit` hook that `fallow hooks install
//! --target git` manages.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use crate::exit_codes::exit_invalid_input;

/// The hook manager that `hooks install --target git` writes for.
pub(super) enum HookTarget {
    Husky(PathBuf),
    Lefthook,
    GitHooks(PathBuf),
}

/// The hook manager of `root`: Husky, Lefthook, or the Git hooks folder.
///
/// A Husky or Lefthook path that links out of the project is refused before
/// the probe, so the answer does not depend on the link target.
pub(super) fn detect_hook_target(root: &Path, removing: bool) -> Result<HookTarget, ExitCode> {
    super::project_files::refuse_outside_link(root, &root.join(".husky"))?;
    for name in [".lefthook", "lefthook.yml", "lefthook.json"] {
        super::project_files::refuse_outside_link_to_check(root, &root.join(name))?;
    }
    if root.join(".husky").is_dir() {
        Ok(HookTarget::Husky(root.join(".husky/pre-commit")))
    } else if root.join(".lefthook").is_dir()
        || root.join("lefthook.yml").exists()
        || root.join("lefthook.json").exists()
    {
        Ok(HookTarget::Lefthook)
    } else {
        git_pre_commit_target(root, removing).map(HookTarget::GitHooks)
    }
}

/// The `pre-commit` file that Git runs for `root`. Git missing from `PATH`
/// gives `.git/hooks/pre-commit` when that folder exists.
///
/// A `core.hooksPath` outside the repository can be shared by other
/// repositories, so install refuses it. Uninstall (`removing`) then uses
/// `.git/hooks/pre-commit` instead, where an earlier release wrote the hook.
pub fn git_pre_commit_target(root: &Path, removing: bool) -> Result<PathBuf, ExitCode> {
    let legacy = root.join(".git/hooks/pre-commit");
    let Ok(dir) = fallow_engine::changed_files::resolve_git_hooks_dir(root) else {
        if root.join(".git/hooks").is_dir() {
            return Ok(legacy);
        }
        eprintln!(
            "Error: No .git directory found. Run `git init` first, or use --hooks \
             from the repository root."
        );
        return Err(exit_invalid_input());
    };
    let Some(top) = repository_folder_of(root, &dir) else {
        if removing {
            return Ok(legacy);
        }
        eprintln!(
            "Error: Git runs the hooks of this repository from {}, which core.hooksPath \
             sets outside the repository. Other repositories can share that folder, so \
             fallow does not write to it. Add `fallow audit --staged` to that pre-commit \
             hook yourself, or set core.hooksPath for this repository only.",
            dir.display()
        );
        return Err(exit_invalid_input());
    };
    let hook = dir.join("pre-commit");
    super::project_files::refuse_tracked_hook_link(&top, &hook)?;
    Ok(rebase_on_root(root, hook))
}

/// The path of the Git `pre-commit` hook of `root`, for status output.
pub fn git_pre_commit_path(root: &Path) -> PathBuf {
    fallow_engine::changed_files::resolve_git_hooks_dir(root).map_or_else(
        |_| root.join(".git").join("hooks").join("pre-commit"),
        |dir| rebase_on_root(root, dir.join("pre-commit")),
    )
}

/// Git prints canonical paths. Put a path below the canonical root back
/// below `root`, so messages show it relative to the root as given.
fn rebase_on_root(root: &Path, path: PathBuf) -> PathBuf {
    let canonical_root = dunce::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    match path.strip_prefix(&canonical_root) {
        Ok(relative) => root.join(relative),
        Err(_) => path,
    }
}

/// The work tree top folder of `root` when the hooks folder is inside it, or
/// inside the common Git directory (a linked worktree runs the hooks of that
/// directory). `None` when the folder is outside the repository.
fn repository_folder_of(root: &Path, dir: &Path) -> Option<PathBuf> {
    let top = fallow_engine::changed_files::resolve_git_toplevel(root).ok()?;
    if dir.starts_with(&top) {
        return Some(top);
    }
    fallow_engine::changed_files::resolve_git_common_dir(root)
        .is_ok_and(|common| dir.starts_with(&common))
        .then_some(top)
}

/// Write a hook file and set the executable permission on Unix.
pub fn write_hook(path: &Path, content: &str) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    std::fs::write(path, content)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(path)?.permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(path, perms)?;
    }
    Ok(())
}
