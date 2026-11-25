//! The analysis root inside a detached base worktree, and whether the base
//! commit contains it.

use std::path::{Path, PathBuf};

use super::base_analysis_root;

/// Analysis root for a detached base worktree, and whether the base commit
/// contains it at all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BaseAnalysisRoot {
    /// The head analysis root maps onto a directory that the base commit
    /// contains, so the base snapshot is analyzed there.
    Present(PathBuf),
    /// The head analysis root maps onto a directory the base commit does not
    /// contain, such as a package added on the branch. Everything under it is
    /// new, so the base snapshot for that root is empty.
    NewInHead(PathBuf),
}

/// Resolve the analysis root inside a detached base worktree and report
/// whether the base commit contains it.
///
/// A root that the base commit does not contain is the ordinary shape of
/// auditing a package added on the branch. Analyzing the whole base worktree
/// instead would compare a subdirectory head snapshot against a
/// whole-repository base snapshot, whose key spaces do not intersect, and
/// refusing the call would blame a `root` the caller spelled correctly.
///
/// The base commit can hold a link at or above the root. When the real path
/// of the root is outside the base worktree, or cannot be read, the root
/// counts as absent, so the base pass never reads or writes through that
/// link.
#[must_use]
pub fn resolve_base_analysis_root(
    current_root: &Path,
    base_worktree_root: &Path,
) -> BaseAnalysisRoot {
    let root = base_analysis_root(current_root, base_worktree_root);
    if !root.is_dir() {
        return BaseAnalysisRoot::NewInHead(root);
    }
    if !stays_inside(&root, base_worktree_root) {
        tracing::warn!(
            root = %root.display(),
            base_worktree = %base_worktree_root.display(),
            "The analysis root of the base commit resolves outside the base worktree through a link; the base snapshot is empty"
        );
        return BaseAnalysisRoot::NewInHead(root);
    }
    BaseAnalysisRoot::Present(root)
}

/// Whether the real path of `path` is inside the real path of `dir`. A path
/// that cannot be resolved is not inside.
fn stays_inside(path: &Path, dir: &Path) -> bool {
    match (dunce::canonicalize(path), dunce::canonicalize(dir)) {
        (Ok(path), Ok(dir)) => path.starts_with(dir),
        _ => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Auditing a package added on the branch is the ordinary case where the
    /// remapped root is absent from the base worktree. Consumers need that
    /// reported rather than validating the joined path and refusing the call
    /// (issue #2699).
    #[test]
    fn resolve_base_analysis_root_reports_a_root_absent_from_the_base() {
        let temp = tempfile::tempdir().expect("temp dir");
        let repo = temp.path().join("repo");
        std::fs::create_dir_all(&repo).expect("create repo");
        crate::git_fixture::fixture_git(&repo, &["init", "-q", "-b", "main"]);
        let existing_root = repo.join("apps").join("mobile");
        std::fs::create_dir_all(&existing_root).expect("create existing root");
        let new_root = repo.join("apps").join("new");
        std::fs::create_dir_all(&new_root).expect("create new root");

        let base_worktree = temp.path().join("base-worktree");
        std::fs::create_dir_all(base_worktree.join("apps").join("mobile"))
            .expect("create base subdirectory");

        assert_eq!(
            resolve_base_analysis_root(&existing_root, &base_worktree),
            BaseAnalysisRoot::Present(base_worktree.join("apps").join("mobile"))
        );
        assert_eq!(
            resolve_base_analysis_root(&new_root, &base_worktree),
            BaseAnalysisRoot::NewInHead(base_worktree.join("apps").join("new"))
        );
    }

    /// The base commit can hold a link at or above the analysis root. A link
    /// whose target is outside the base worktree must not move the base pass
    /// out of the worktree, so the root counts as absent from the base.
    #[cfg(unix)]
    #[test]
    fn resolve_base_analysis_root_does_not_follow_a_link_out_of_the_base_worktree() {
        let temp = tempfile::tempdir().expect("temp dir");
        let repo = temp.path().join("repo");
        std::fs::create_dir_all(&repo).expect("create repo");
        crate::git_fixture::fixture_git(&repo, &["init", "-q", "-b", "main"]);
        let app_root = repo.join("apps").join("mobile");
        std::fs::create_dir_all(&app_root).expect("create app root");
        let outside = temp.path().join("outside");
        std::fs::create_dir_all(outside.join("mobile")).expect("create outside dir");
        let base_worktree = temp.path().join("base-worktree");
        std::fs::create_dir_all(&base_worktree).expect("create base worktree");
        std::os::unix::fs::symlink(&outside, base_worktree.join("apps")).expect("link");

        assert_eq!(
            resolve_base_analysis_root(&app_root, &base_worktree),
            BaseAnalysisRoot::NewInHead(base_worktree.join("apps").join("mobile"))
        );
    }

    /// A link that stays inside the base worktree is an ordinary layout.
    #[cfg(unix)]
    #[test]
    fn resolve_base_analysis_root_follows_a_link_inside_the_base_worktree() {
        let temp = tempfile::tempdir().expect("temp dir");
        let repo = temp.path().join("repo");
        std::fs::create_dir_all(&repo).expect("create repo");
        crate::git_fixture::fixture_git(&repo, &["init", "-q", "-b", "main"]);
        let app_root = repo.join("apps").join("mobile");
        std::fs::create_dir_all(&app_root).expect("create app root");
        let base_worktree = temp.path().join("base-worktree");
        std::fs::create_dir_all(base_worktree.join("packages").join("mobile"))
            .expect("create base dir");
        std::os::unix::fs::symlink("packages", base_worktree.join("apps")).expect("link");

        assert_eq!(
            resolve_base_analysis_root(&app_root, &base_worktree),
            BaseAnalysisRoot::Present(base_worktree.join("apps").join("mobile"))
        );
    }

    /// A base worktree from a real commit recreates its links. A link of
    /// the base commit with an absolute target outside the worktree must
    /// not become the base analysis root.
    #[cfg(unix)]
    #[test]
    fn a_base_worktree_with_a_committed_link_out_of_it_has_no_base_root() {
        use crate::git_fixture::fixture_git;
        let temp = tempfile::tempdir().expect("temp dir");
        let repo = temp.path().join("repo");
        std::fs::create_dir_all(&repo).expect("create repo");
        let outside = temp.path().join("outside");
        std::fs::create_dir_all(outside.join("app")).expect("outside dir");
        let outside = dunce::canonicalize(&outside).expect("outside path");
        fixture_git(&repo, &["init", "-q", "-b", "main"]);
        std::os::unix::fs::symlink(&outside, repo.join("packages")).expect("link");
        fixture_git(&repo, &["add", "."]);
        fixture_git(&repo, &["commit", "-q", "-m", "base with a link"]);
        std::fs::remove_file(repo.join("packages")).expect("remove link");
        std::fs::create_dir_all(repo.join("packages").join("app")).expect("app dir");
        std::fs::write(repo.join("packages/app/index.ts"), "export {};\n").expect("source");
        fixture_git(&repo, &["add", "-A"]);
        fixture_git(&repo, &["commit", "-q", "-m", "head with a directory"]);

        let worktree = super::super::TemporaryBaseWorktree::create(&repo, "HEAD~1", &[])
            .expect("base worktree");
        let linked = worktree.path().join("packages");
        assert!(
            std::fs::symlink_metadata(&linked).is_ok_and(|meta| meta.file_type().is_symlink()),
            "the base worktree recreates the link"
        );
        assert_eq!(
            resolve_base_analysis_root(&repo.join("packages").join("app"), worktree.path()),
            BaseAnalysisRoot::NewInHead(linked.join("app"))
        );
    }
}
