use std::path::{Path, PathBuf};

#[expect(
    clippy::redundant_pub_crate,
    reason = "shared by sibling LSP modules through the private path_utils module"
)]
pub(crate) fn canonicalize_for_lsp(path: &Path) -> PathBuf {
    dunce::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// The analysis root of a workspace folder of the client.
#[expect(
    clippy::redundant_pub_crate,
    reason = "shared by sibling LSP modules through the private path_utils module"
)]
#[derive(Debug, PartialEq, Eq)]
pub(crate) enum FolderRoot {
    /// The folder does not exist (yet).
    Absent,
    /// The folder passed the root validation of the CLI. This is its real
    /// path.
    Valid(PathBuf),
    /// The root validation refused the folder, for example because a link
    /// that git tracks moves it out of its checkout.
    Refused(String),
}

/// Resolve the workspace folder `path` with the root validation of the CLI.
#[expect(
    clippy::redundant_pub_crate,
    reason = "shared by sibling LSP modules through the private path_utils module"
)]
pub(crate) fn resolve_folder(path: &Path) -> FolderRoot {
    if std::fs::symlink_metadata(path).is_err() {
        return FolderRoot::Absent;
    }
    match fallow_api::validate_root(path) {
        Ok(root) => FolderRoot::Valid(root),
        Err(message) => FolderRoot::Refused(message),
    }
}

/// Why the analysis must skip `root`, when the root now resolves to another
/// path than at initialize. A branch switch can replace the root, or a
/// directory above it, with a link. A root that does not exist is not moved.
#[expect(
    clippy::redundant_pub_crate,
    reason = "shared by sibling LSP modules through the private path_utils module"
)]
pub(crate) fn moved_root_reason(root: &Path) -> Option<String> {
    if std::fs::symlink_metadata(root).is_err() {
        return None;
    }
    match dunce::canonicalize(root) {
        Ok(resolved) if resolved == dunce::simplified(root) => None,
        Ok(resolved) => Some(format!(
            "the workspace root '{}' now resolves to '{}'; fallow skips the analysis, restart the server to analyze the new location",
            root.display(),
            resolved.display()
        )),
        Err(err) => Some(format!(
            "the workspace root '{}' cannot be resolved: {err}; fallow skips the analysis",
            root.display()
        )),
    }
}

#[cfg(test)]
mod spelling_tests {
    use super::*;

    /// On Windows `std::fs::canonicalize` gives a `\\?\` path. The same
    /// directory in that spelling has not moved.
    #[test]
    fn a_root_in_the_verbatim_spelling_has_not_moved() {
        let temp = tempfile::tempdir().unwrap();
        let root = std::fs::canonicalize(temp.path()).unwrap();
        assert_eq!(moved_root_reason(&root), None);
    }
}

#[cfg(all(test, unix))]
mod tests {
    use super::*;

    #[cfg(unix)]
    #[test]
    fn resolve_folder_refuses_a_folder_below_a_link_that_leaves_the_checkout() {
        let temp = tempfile::tempdir().expect("tempdir");
        let repo = temp.path().join("repo");
        std::fs::create_dir_all(&repo).expect("repo dir");
        fallow_api::git_fixture::fixture_git(&repo, &["init", "-q", "-b", "main"]);
        let outside = temp.path().join("outside");
        std::fs::create_dir_all(outside.join("app")).expect("outside dir");
        std::os::unix::fs::symlink(&outside, repo.join("packages")).expect("link");
        fallow_api::git_fixture::fixture_git(&repo, &["add", "packages"]);
        fallow_api::git_fixture::fixture_git(&repo, &["commit", "-q", "-m", "link"]);

        let refused = resolve_folder(&repo.join("packages").join("app"));
        assert!(
            matches!(&refused, FolderRoot::Refused(err) if err.contains("outside the checkout")),
            "got: {refused:?}"
        );
        assert_eq!(
            resolve_folder(&repo),
            FolderRoot::Valid(dunce::canonicalize(&repo).unwrap())
        );
        assert_eq!(
            resolve_folder(&temp.path().join("absent")),
            FolderRoot::Absent
        );
    }

    #[cfg(unix)]
    #[test]
    fn moved_root_reason_reports_a_root_replaced_by_a_link() {
        let temp = tempfile::tempdir().expect("tempdir");
        let root = dunce::canonicalize(temp.path()).unwrap().join("project");
        std::fs::create_dir(&root).expect("root");
        let elsewhere = temp.path().join("elsewhere");
        std::fs::create_dir(&elsewhere).expect("other dir");

        assert_eq!(moved_root_reason(&root), None);
        std::fs::remove_dir(&root).expect("remove root");
        assert_eq!(
            moved_root_reason(&root),
            None,
            "an absent root is not moved"
        );
        std::os::unix::fs::symlink(&elsewhere, &root).expect("link root");
        let reason = moved_root_reason(&root).expect("a moved root");
        assert!(reason.contains("now resolves to"), "got: {reason}");
    }
}
