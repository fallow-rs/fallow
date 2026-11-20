//! Short commit SHAs for human output: the current HEAD, and the merge-base
//! that an audit diff starts from.

use std::path::Path;

use super::run_git;

/// Short SHA for the current HEAD.
#[must_use]
pub fn short_head_sha(root: &Path) -> Option<String> {
    run_git(root, &["rev-parse", "--short", "HEAD"])
}

/// Short SHA of the merge-base of `base_ref` and the current HEAD: the commit
/// that a changed-file diff against `base_ref` starts from. `None` when git
/// cannot resolve the ref or the two commits share no history.
#[must_use]
pub fn short_merge_base_sha(root: &Path, base_ref: &str) -> Option<String> {
    let merge_base = run_git(root, &["merge-base", "--end-of-options", base_ref, "HEAD"])?;
    run_git(root, &["rev-parse", "--short", &merge_base])
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::{short_head_sha, short_merge_base_sha};
    use crate::repo_refs::tests::{commit_all, commit_file, git, init_repo, seeded_repo};

    #[test]
    fn short_head_sha_omits_git_line_ending() {
        let temp = tempfile::tempdir().expect("temp dir");
        let repo = temp.path().join("repo");
        init_repo(&repo);
        fs::write(repo.join("tracked.txt"), "committed\n").expect("write tracked file");
        commit_all(&repo, "initial");

        let sha = short_head_sha(&repo).expect("HEAD sha");
        assert_eq!(sha, sha.trim());
        assert!(!sha.is_empty());
    }

    #[test]
    fn short_head_sha_is_absent_outside_a_git_repo() {
        let temp = tempfile::tempdir().expect("temp dir");

        assert_eq!(short_head_sha(temp.path()), None);
    }

    #[test]
    fn short_merge_base_sha_names_the_fork_point_of_the_branch() {
        let temp = tempfile::tempdir().expect("temp dir");
        let repo = seeded_repo(temp.path());
        let fork = git(&repo, &["rev-parse", "--short", "HEAD"]);
        git(&repo, &["checkout", "-b", "feature"]);
        commit_file(&repo, "feature.txt", "feature\n");
        git(&repo, &["checkout", "main"]);
        commit_file(&repo, "main.txt", "main\n");
        git(&repo, &["checkout", "feature"]);

        assert_eq!(short_merge_base_sha(&repo, "main"), Some(fork));
        assert_eq!(short_merge_base_sha(&repo, "no-such-ref"), None);
    }
}
