//! Ignore-file settings that source discovery uses.

use std::path::{Path, PathBuf};

use ignore::WalkBuilder;

use crate::WorkspaceInfo;

/// Make a directory walker with the ignore-file settings of source discovery.
///
/// The walker reads `.gitignore` files at each level, the global gitignore,
/// `.git/info/exclude` and `.ignore` files. Gitignore rules apply only inside a
/// git repository. Hidden entries are not skipped here: each caller filters
/// them, because source discovery admits some hidden directories.
///
/// Source discovery and each check that must agree with it start from this
/// builder, so the two cannot apply different ignore rules.
#[must_use]
pub fn source_walk_builder(root: &Path) -> WalkBuilder {
    let mut builder = WalkBuilder::new(root);
    builder
        .hidden(false)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true);
    builder
}

/// The workspaces whose root directory source discovery walks.
///
/// A member under a gitignored directory, for example an upstream repository
/// that CI clones into the workspace, has no discovered source files. The run
/// cannot prove that the dependencies of its manifest are unused, so the
/// unused-dependency checks skip it. A member that is outside `root` or that
/// does not canonicalize stays in the result. Only ignore files count here: a
/// member under a hidden directory that source discovery skips stays in the
/// result.
#[must_use]
pub fn source_walked_workspaces(root: &Path, workspaces: &[WorkspaceInfo]) -> Vec<WorkspaceInfo> {
    let canonical_root = dunce::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    let canonical: Vec<Option<PathBuf>> = workspaces
        .iter()
        .map(|workspace| {
            dunce::canonicalize(&workspace.root)
                .ok()
                .filter(|dir| dir.starts_with(&canonical_root))
        })
        .collect();
    let candidates: Vec<PathBuf> = canonical.iter().flatten().cloned().collect();
    let reached = dirs_reached_by_source_walk(&canonical_root, &candidates);
    workspaces
        .iter()
        .zip(&canonical)
        .filter(|(_, dir)| dir.as_ref().is_none_or(|dir| reached.contains(dir)))
        .map(|(workspace, _)| workspace.clone())
        .collect()
}

/// The directories in `targets` that the source discovery walk reaches.
///
/// `targets` are canonical directories under `canonical_root`. A directory
/// under a gitignored directory has no discovered source files. The walk uses
/// the ignore-file settings of source discovery ([`source_walk_builder`]), so nested
/// `.gitignore` files, `.git/info/exclude` and the global gitignore apply with
/// the same rules. The walk enters only the ancestor directories of the
/// targets, so it reads a small number of directories. A gitignore rule that
/// ignores files but not their directory (for example `*.ts`) does not stop
/// the walk, so the walk reaches such a target.
pub fn dirs_reached_by_source_walk(
    canonical_root: &Path,
    targets: &[PathBuf],
) -> rustc_hash::FxHashSet<PathBuf> {
    let mut reached = rustc_hash::FxHashSet::default();
    if targets.is_empty() {
        return reached;
    }
    let mut on_path: rustc_hash::FxHashSet<PathBuf> = rustc_hash::FxHashSet::default();
    for target in targets {
        for ancestor in target.ancestors() {
            if ancestor == canonical_root || !on_path.insert(ancestor.to_path_buf()) {
                break;
            }
        }
    }
    let max_depth = targets
        .iter()
        .filter_map(|target| target.strip_prefix(canonical_root).ok())
        .map(|relative| relative.components().count())
        .max();
    let mut builder = source_walk_builder(canonical_root);
    builder
        .max_depth(max_depth)
        .filter_entry(move |entry| entry.depth() == 0 || on_path.contains(entry.path()));
    let targets: rustc_hash::FxHashSet<&Path> = targets.iter().map(PathBuf::as_path).collect();
    for entry in builder.build().flatten() {
        if targets.contains(entry.path()) {
            reached.insert(entry.into_path());
        }
    }
    reached
}

#[cfg(test)]
mod tests {
    use super::*;

    fn workspace(root: PathBuf) -> WorkspaceInfo {
        WorkspaceInfo {
            name: root
                .file_name()
                .map_or_else(String::new, |name| name.to_string_lossy().into_owned()),
            root,
            is_internal_dependency: false,
        }
    }

    fn walked_names(root: &Path, workspaces: &[WorkspaceInfo]) -> Vec<String> {
        source_walked_workspaces(root, workspaces)
            .into_iter()
            .map(|workspace| workspace.name)
            .collect()
    }

    #[test]
    fn source_walked_workspaces_drops_gitignored_members_only_in_a_git_repository() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let root = directory.path();
        for dir in ["packages/app", "clone/packages/lib"] {
            std::fs::create_dir_all(root.join(dir)).expect("member directory");
        }
        std::fs::write(root.join(".gitignore"), "clone\n").expect("gitignore");
        let outside = tempfile::tempdir().expect("outside directory");
        let workspaces = [
            workspace(root.to_path_buf()),
            workspace(root.join("packages/app")),
            workspace(root.join("clone")),
            workspace(root.join("clone/packages/lib")),
            workspace(root.join("missing")),
            workspace(outside.path().to_path_buf()),
        ];
        let all: Vec<String> = workspaces.iter().map(|ws| ws.name.clone()).collect();

        assert_eq!(walked_names(root, &workspaces), all, "no git repository");

        std::fs::create_dir_all(root.join(".git")).expect("git directory");
        let walked = walked_names(root, &workspaces);
        assert!(!walked.contains(&"clone".to_string()), "{walked:?}");
        assert!(!walked.contains(&"lib".to_string()), "{walked:?}");
        assert_eq!(walked.len(), all.len() - 2, "{walked:?}");
    }
}
