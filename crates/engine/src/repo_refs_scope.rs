//! The materialization scope of a base worktree: the committed-tree paths
//! that the snapshot writes to disk.

use std::path::{Path, PathBuf};

use xxhash_rust::xxh3::xxh3_64;

use super::{git_command, run_git};

/// Which committed-tree paths a base worktree actually needs on disk.
///
/// The raw object materialization deliberately bypasses git's checkout
/// pipeline (no hooks, smudge filters, or line-ending conversion). Before
/// that change the worktree checkout honored the host's sparse-checkout cone
/// and, for a subdirectory analysis root, only the cone was ever read. The
/// unscoped materialization reads EVERY blob in the commit instead: on a
/// blobless partial clone (`actions/checkout` sets `--filter=blob:none`
/// whenever `sparse-checkout` is set) each out-of-cone blob triggers a lazy
/// promisor fetch via `git-remote-https`. For a large monorepo checked out
/// sparsely to one subdirectory that turns a seconds-long snapshot into a
/// fetch of the whole monorepo, which presents as `fallow audit` hanging to
/// the CI timeout with `git` / `git-remote-https` orphans (issue #2615).
///
/// The scope restores the old working set without reintroducing checkout:
/// - a subdirectory analysis root materializes only that subtree, plus the
///   files directly inside each ancestor directory (ignore files and parent
///   configs such as a shared `tsconfig.base.json`),
/// - a repository-root run on a sparse checkout materializes what git cone
///   mode checks out: the listed cone directories, plus the files directly
///   inside each ancestor of a cone directory, including the root,
/// - otherwise everything is materialized as before.
///
/// Both probes fail open to full materialization: a probe error is at worst a
/// slower snapshot, never a missing-file misattribution.
pub(super) struct MaterializationScope {
    /// Forward-slash repo-relative analysis subdir (e.g. `apps/web`), or
    /// `None` when the requested root is the repository top level.
    subdir_prefix: Option<String>,
    /// Cone-mode sparse directories (forward-slash, no trailing slash), or
    /// `None` when sparse-checkout is off, non-cone, or unreadable.
    sparse_dirs: Option<Vec<String>>,
}

impl MaterializationScope {
    pub(super) fn should_materialize(&self, path: &Path) -> bool {
        let Some(relative) = forward_slash_path(path) else {
            return true;
        };
        if let Some(prefix) = self.subdir_prefix.as_deref() {
            if relative == prefix || relative.starts_with(&format!("{prefix}/")) {
                return true;
            }
            // The files directly inside each ancestor of the subtree, such
            // as a root `.gitignore` or a parent `tsconfig.base.json` that the
            // subtree extends, shape the analysis of the subtree. They are few
            // and a sparse checkout of the subtree has them too.
            return is_in_ancestor_dir(&relative, prefix);
        }
        if let Some(dirs) = self.sparse_dirs.as_deref() {
            // Git cone mode checks out the cone directories in full and the
            // files directly inside each ancestor of a cone directory,
            // including the repository root.
            return dirs.iter().any(|dir| {
                relative.starts_with(&format!("{dir}/")) || is_in_ancestor_dir(&relative, dir)
            }) || !relative.contains('/');
        }
        true
    }

    /// A stable key of the materialized working set. Two snapshots of one
    /// commit hold the same files only when their keys are equal.
    fn key(&self) -> String {
        let mut identity = String::from(SCOPE_RULES_VERSION);
        identity.push('\0');
        if let Some(prefix) = self.subdir_prefix.as_deref() {
            identity.push_str("subdir\0");
            identity.push_str(prefix);
            identity.push('\0');
        }
        match self.sparse_dirs.as_deref() {
            Some(dirs) => {
                let mut dirs = dirs.to_vec();
                dirs.sort();
                dirs.dedup();
                identity.push_str("sparse\0");
                for dir in dirs {
                    identity.push_str(&dir);
                    identity.push('\0');
                }
            }
            None => identity.push_str("full\0"),
        }
        format!("{:016x}", xxh3_64(identity.as_bytes()))
    }
}

/// The version of the rules in [`MaterializationScope::should_materialize`].
/// A change to the rules changes the files of a snapshot with equal inputs,
/// so it needs a new version, and the scope key then rebuilds old snapshots.
const SCOPE_RULES_VERSION: &str = "rules-2";

/// A stable key of the files that [`create_detached_base_worktree`](super::create_detached_base_worktree)
/// materializes for `repo_root`: the analysis subdirectory and the sparse
/// cone of the host checkout. A reusable snapshot of one commit is valid only
/// for the key that it was built with, because a cone change at the same
/// commit changes the set of files on disk.
#[must_use]
pub fn materialization_scope_key(repo_root: &Path) -> String {
    materialization_scope(repo_root).key()
}

/// Forward-slash repo-relative path for scope matching, or `None` when the
/// path is not valid UTF-8. Non-UTF-8 tree paths are rare; failing open keeps
/// them materialized rather than risking a misattributed base snapshot.
fn forward_slash_path(path: &Path) -> Option<String> {
    let raw = path.to_str()?;
    Some(raw.replace('\\', "/"))
}

/// True for a file directly inside an ancestor directory of `dir`, including
/// the repository root, e.g. `.gitignore` or `apps/package.json` for the
/// directory `apps/web`.
fn is_in_ancestor_dir(relative: &str, dir: &str) -> bool {
    let parent = relative.rsplit_once('/').map_or("", |(parent, _)| parent);
    parent.is_empty() || dir.starts_with(&format!("{parent}/"))
}

pub(super) fn materialization_scope(repo_root: &Path) -> MaterializationScope {
    MaterializationScope {
        subdir_prefix: analysis_subdir_prefix(repo_root),
        sparse_dirs: sparse_cone_dirs(repo_root),
    }
}

/// Repo-relative forward-slash subdir of the requested analysis root, or
/// `None` when it is the repository top level (or the top level cannot be
/// resolved, which fails open to full materialization).
fn analysis_subdir_prefix(repo_root: &Path) -> Option<String> {
    let toplevel = run_git(repo_root, &["rev-parse", "--show-toplevel"])?;
    let toplevel = PathBuf::from(toplevel);
    let canonical_toplevel = dunce::canonicalize(&toplevel).unwrap_or(toplevel);
    let canonical_root = dunce::canonicalize(repo_root).unwrap_or_else(|_| repo_root.to_path_buf());
    let relative = canonical_root.strip_prefix(&canonical_toplevel).ok()?;
    if relative.as_os_str().is_empty() {
        return None;
    }
    let prefix = forward_slash_path(relative)?;
    if prefix.is_empty() {
        return None;
    }
    Some(prefix)
}

/// Cone-mode sparse-checkout directories of the host checkout, or `None` when
/// sparse-checkout is off, non-cone, or unreadable (fail open to full).
///
/// `git sparse-checkout list` exits non-zero on a non-sparse worktree, which
/// is the common full-clone case. Non-cone mode uses glob patterns that this
/// matcher does not implement, so it also falls back to full materialization.
fn sparse_cone_dirs(repo_root: &Path) -> Option<Vec<String>> {
    if run_git(repo_root, &["config", "--get", "core.sparseCheckout"])? != "true" {
        return None;
    }
    if run_git(repo_root, &["config", "--get", "core.sparseCheckoutCone"])? != "true" {
        return None;
    }
    let output = git_command(repo_root)
        .args(["sparse-checkout", "list"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let list = String::from_utf8(output.stdout).ok()?;
    let mut dirs = Vec::new();
    for line in list.lines() {
        let pattern = line.trim().trim_matches('/');
        if pattern.is_empty() {
            continue;
        }
        // Cone mode lists directories; a stray glob (non-cone residue) cannot
        // be matched exactly, so fail open rather than under-materialize.
        // Git quotes a directory name with special characters in C style,
        // and the matcher does not unquote it, so that also fails open.
        if pattern.contains(['*', '?', '[', '!', '"']) {
            return None;
        }
        dirs.push(pattern.replace('\\', "/"));
    }
    Some(dirs)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Pure scope unit coverage: subdir runs keep their subtree plus the files
    /// directly inside each ancestor; root sparse runs keep the cone and the
    /// files of its ancestors; full clones keep everything.
    #[test]
    fn materialization_scope_filters_to_the_needed_working_set() {
        let subdir = MaterializationScope {
            subdir_prefix: Some("apps/web".to_string()),
            sparse_dirs: None,
        };
        assert!(subdir.should_materialize(Path::new("apps/web/a.ts")));
        assert!(subdir.should_materialize(Path::new("top.ts")));
        assert!(subdir.should_materialize(Path::new(".gitignore")));
        assert!(subdir.should_materialize(Path::new("apps/.gitignore")));
        assert!(!subdir.should_materialize(Path::new("apps/other/b.ts")));
        assert!(subdir.should_materialize(Path::new("apps/tsconfig.base.json")));
        assert!(!subdir.should_materialize(Path::new("apps/other/.gitignore")));

        let sparse = MaterializationScope {
            subdir_prefix: None,
            sparse_dirs: Some(vec!["apps/web".to_string()]),
        };
        assert!(sparse.should_materialize(Path::new("apps/web/a.ts")));
        assert!(sparse.should_materialize(Path::new("top.ts")));
        assert!(!sparse.should_materialize(Path::new("apps/other/b.ts")));
        assert!(sparse.should_materialize(Path::new("apps/package.json")));
        assert!(!sparse.should_materialize(Path::new("apps/web-old/a.ts")));
        assert!(!sparse.should_materialize(Path::new("libs/package.json")));

        let nested = MaterializationScope {
            subdir_prefix: None,
            sparse_dirs: Some(vec!["apps/web/sub".to_string()]),
        };
        assert!(nested.should_materialize(Path::new("apps/web/package.json")));
        assert!(nested.should_materialize(Path::new("apps/package.json")));
        assert!(!nested.should_materialize(Path::new("apps/other/x.ts")));
        assert!(!nested.should_materialize(Path::new("apps/web/other/x.ts")));

        let empty = MaterializationScope {
            subdir_prefix: None,
            sparse_dirs: Some(Vec::new()),
        };
        assert!(empty.should_materialize(Path::new("top.ts")));
        assert!(!empty.should_materialize(Path::new("apps/package.json")));

        let full = MaterializationScope {
            subdir_prefix: None,
            sparse_dirs: None,
        };
        assert!(full.should_materialize(Path::new("apps/other/b.ts")));
    }

    /// The scope key changes with the cone and the subdirectory, and not
    /// with the order of the cone directories.
    #[test]
    fn materialization_scope_key_follows_the_working_set() {
        let scope = |subdir: Option<&str>, dirs: Option<&[&str]>| MaterializationScope {
            subdir_prefix: subdir.map(str::to_owned),
            sparse_dirs: dirs.map(|dirs| dirs.iter().map(|dir| (*dir).to_owned()).collect()),
        };
        let full = scope(None, None).key();
        let web = scope(None, Some(&["apps/web"])).key();
        let both = scope(None, Some(&["apps/web", "libs/ui"])).key();
        let reversed = scope(None, Some(&["libs/ui", "apps/web"])).key();
        let subdir = scope(Some("apps/web"), None).key();
        assert_ne!(full, web);
        assert_ne!(web, both);
        assert_eq!(both, reversed);
        assert_ne!(full, subdir);
        assert_ne!(subdir, web);
    }
}
