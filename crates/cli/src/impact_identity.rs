//! Project identity for the impact report.

use super::*;

/// The git common dir and toplevel for `root`, from the result of the combined
/// probe.
///
/// One git call answers both in a work tree. Outside a repository, or without
/// git, each single probe fails in the same way, so both paths fall back to
/// the root and no more git processes start. After another git error, for
/// example in a bare repository, each single probe decides on its own.
pub(super) fn identity_paths_from(
    root: &Path,
    combined: Result<(PathBuf, PathBuf), ChangedFilesError>,
    common_dir: impl FnOnce() -> Option<PathBuf>,
    toplevel: impl FnOnce() -> Option<PathBuf>,
) -> (PathBuf, PathBuf) {
    match combined {
        Ok(paths) => paths,
        Err(ChangedFilesError::NotARepository | ChangedFilesError::GitMissing(_)) => {
            let fallback = resolve_or_root(None, root);
            (fallback.clone(), fallback)
        }
        Err(_) => (
            resolve_or_root(common_dir(), root),
            resolve_or_root(toplevel(), root),
        ),
    }
}

/// Resolve (and memoize) the `(project_key, worktree_key, display_name)` for
/// `root`. `project_key` collapses all worktrees of a repo onto one identity via
/// the git common dir (falling back to the canonicalized root for non-git);
/// `worktree_key` is the per-tree toplevel (namespaces the attribution frontier
/// so concurrent worktrees do not prune each other's baseline); `display_name`
/// is the repo's basename for legible cross-repo rows. All three derive from a
/// single common-dir + toplevel resolution so the git subprocesses run at most
/// once per root per run (`fallow audit` is the perf-priority path).
pub(super) fn project_identity(root: &Path) -> ProjectIdentity {
    let cache = IDENTITY_CACHE.get_or_init(|| std::sync::Mutex::new(FxHashMap::default()));
    if let Ok(map) = cache.lock()
        && let Some(found) = map.get(root)
    {
        return found.clone();
    }
    let (common, toplevel) = identity_paths_from(
        root,
        fallow_engine::changed_files::resolve_git_common_dir_and_toplevel(root),
        || fallow_engine::changed_files::resolve_git_common_dir(root).ok(),
        || fallow_engine::changed_files::resolve_git_toplevel(root).ok(),
    );
    let identity = (
        hash_path_identity(&common),
        hash_path_identity(&toplevel),
        repo_basename(&common),
    );
    if let Ok(mut map) = cache.lock() {
        map.insert(root.to_path_buf(), identity.clone());
    }
    identity
}
