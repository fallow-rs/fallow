//! Ignore-file settings that source discovery uses.

use std::path::{Path, PathBuf};

use ignore::{DirEntry, WalkBuilder};

use crate::WorkspaceInfo;
use crate::ignore_files::{IgnoreFileSources, checked_walk_builder};

/// Make a directory walker with the ignore-file settings of source discovery.
///
/// The walker reads `.gitignore` files at each level, the global gitignore,
/// `.git/info/exclude` and `.ignore` files. Gitignore rules apply only inside a
/// git repository. Hidden entries are not skipped here: each caller filters
/// them in `filter`, because source discovery admits some hidden directories.
/// Each ignore file is checked before the walk reads it (see
/// [`checked_walk_builder`]). Do not call [`WalkBuilder::filter_entry`] on the
/// result: it replaces that check.
///
/// Source discovery and each check that must agree with it start from this
/// builder, so the two cannot apply different ignore rules.
#[must_use]
pub fn source_walk_builder<F>(root: &Path, filter: F) -> WalkBuilder
where
    F: Fn(&DirEntry) -> bool + Send + Sync + 'static,
{
    let sources = IgnoreFileSources {
        git_ignore: true,
        git_exclude: true,
        git_global: true,
    };
    checked_walk_builder(root, sources, filter)
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
    let mut builder = source_walk_builder(canonical_root, move |entry| {
        entry.depth() == 0 || on_path.contains(entry.path())
    });
    builder.max_depth(max_depth);
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

    /// The root-relative paths of the files that the walk yields, or `None`
    /// when the walk does not end within `limit`.
    #[cfg(unix)]
    fn walked_files_within(builder: WalkBuilder, root: &Path) -> Option<Vec<String>> {
        const LIMIT: std::time::Duration = std::time::Duration::from_secs(3);
        let root = root.to_path_buf();
        let (sender, receiver) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut files: Vec<String> = builder
                .build()
                .flatten()
                .filter(|entry| entry.file_type().is_some_and(|kind| !kind.is_dir()))
                .filter_map(|entry| {
                    let relative = entry.path().strip_prefix(&root).ok()?.to_path_buf();
                    Some(relative.to_string_lossy().replace('\\', "/"))
                })
                .collect();
            files.sort();
            let _ = sender.send(files);
        });
        receiver.recv_timeout(LIMIT).ok()
    }

    #[cfg(unix)]
    fn make_fifo(path: &Path) {
        let status = std::process::Command::new("mkfifo")
            .arg(path)
            .status()
            .expect("run mkfifo");
        assert!(status.success(), "mkfifo {}", path.display());
    }

    #[cfg(unix)]
    fn write_file(path: &Path, contents: &str) {
        std::fs::create_dir_all(path.parent().expect("parent")).expect("parent directory");
        std::fs::write(path, contents).expect("write file");
    }

    #[cfg(unix)]
    #[test]
    fn source_walk_skips_a_fifo_gitignore_in_the_root() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let root = directory.path();
        std::fs::create_dir_all(root.join(".git")).expect("git directory");
        write_file(&root.join("src/index.ts"), "");
        make_fifo(&root.join(".gitignore"));

        let walked = walked_files_within(source_walk_builder(root, |_| true), root)
            .expect("the walk must end although .gitignore is a FIFO");
        assert!(walked.contains(&"src/index.ts".to_string()), "{walked:?}");
    }

    #[cfg(unix)]
    #[test]
    fn source_walk_skips_a_nested_ignore_file_that_links_to_a_device() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let root = directory.path();
        std::fs::create_dir_all(root.join(".git")).expect("git directory");
        write_file(&root.join("src/index.ts"), "");
        write_file(&root.join("packages/app/main.ts"), "");
        std::os::unix::fs::symlink("/dev/zero", root.join("packages/app/.ignore"))
            .expect("link to /dev/zero");

        let walked = walked_files_within(source_walk_builder(root, |_| true), root)
            .expect("the walk must end although .ignore links to /dev/zero");
        assert!(walked.contains(&"src/index.ts".to_string()), "{walked:?}");
        assert!(
            !walked.contains(&"packages/app/main.ts".to_string()),
            "the walk skips the directory of the skipped file: {walked:?}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn source_walk_skips_a_gitignore_that_links_outside_the_project() {
        let outside = tempfile::tempdir().expect("outside directory");
        write_file(&outside.path().join("rules"), "hidden.ts\n");
        let directory = tempfile::tempdir().expect("temporary directory");
        let root = directory.path();
        std::fs::create_dir_all(root.join(".git")).expect("git directory");
        write_file(&root.join("src/hidden.ts"), "");
        std::os::unix::fs::symlink(outside.path().join("rules"), root.join(".gitignore"))
            .expect("link outside the project");

        let walked =
            walked_files_within(source_walk_builder(root, |_| true), root).expect("walk ends");
        assert!(
            walked.contains(&"src/hidden.ts".to_string()),
            "the rules of a link out of the project do not apply: {walked:?}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn source_walk_checks_the_ignore_files_of_parent_directories() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let repository = directory.path();
        std::fs::create_dir_all(repository.join(".git")).expect("git directory");
        make_fifo(&repository.join(".gitignore"));
        let root = repository.join("app");
        write_file(&root.join("src/index.ts"), "");

        let walked = walked_files_within(source_walk_builder(&root, |_| true), &root)
            .expect("the walk must end although a parent .gitignore is a FIFO");
        assert!(walked.contains(&"src/index.ts".to_string()), "{walked:?}");
    }

    /// A walk from the top level of a repository reads no ignore file of the
    /// directories above it, the same as Git. An audit walks its base
    /// snapshot from such a top level in the temp dir, so the files of the
    /// temp dir must not change which base files the walk sees.
    #[cfg(unix)]
    #[test]
    fn source_walk_reads_no_ignore_file_above_the_repository_top_level() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let base = directory.path();
        write_file(&base.join(".ignore"), "consumer.ts\n");
        let root = base.join("snapshot");
        std::fs::create_dir_all(root.join(".git")).expect("git directory");
        write_file(&root.join(".gitignore"), "dist\n");
        write_file(&root.join("src/consumer.ts"), "");
        write_file(&root.join("dist/out.js"), "");

        let walked =
            walked_files_within(source_walk_builder(&root, |_| true), &root).expect("walk ends");
        assert!(
            walked.contains(&"src/consumer.ts".to_string()),
            "{walked:?}"
        );
        assert!(
            !walked.contains(&"dist/out.js".to_string()),
            "the .gitignore of the repository still applies: {walked:?}"
        );

        let app = root.join("app");
        write_file(&root.join(".ignore"), "skipped.ts\n");
        write_file(&app.join("skipped.ts"), "");
        write_file(&app.join("kept.ts"), "");
        let walked =
            walked_files_within(source_walk_builder(&app, |_| true), &app).expect("walk ends");
        assert_eq!(
            walked,
            vec!["kept.ts".to_string()],
            "a root below the top level keeps the rules of the repository"
        );
    }

    #[cfg(unix)]
    #[test]
    fn source_walk_reads_a_parent_gitignore_that_links_outside_the_project() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let base = directory.path();
        write_file(&base.join("dotfiles/gitignore"), "*.log\n");
        std::fs::create_dir_all(base.join("sites")).expect("sites directory");
        std::os::unix::fs::symlink(
            base.join("dotfiles/gitignore"),
            base.join("sites/.gitignore"),
        )
        .expect("link to the dotfiles");
        let root = base.join("sites/app");
        std::fs::create_dir_all(root.join(".git")).expect("git directory");
        write_file(&root.join(".gitignore"), "generated\n");
        write_file(&root.join("src/index.ts"), "");
        write_file(&root.join("generated/gen.ts"), "");

        let walked =
            walked_files_within(source_walk_builder(&root, |_| true), &root).expect("walk ends");
        assert!(walked.contains(&"src/index.ts".to_string()), "{walked:?}");
        assert!(
            !walked.contains(&"generated/gen.ts".to_string()),
            "the project .gitignore still applies: {walked:?}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn source_walk_checks_the_exclude_file_of_a_linked_worktree() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let base = directory.path();
        let git_dir = base.join("repository-git");
        std::fs::create_dir_all(git_dir.join("info")).expect("git info directory");
        std::fs::write(git_dir.join("commondir"), ".\n").expect("commondir");
        make_fifo(&git_dir.join("info/exclude"));
        let root = base.join("worktree");
        write_file(&root.join("src/index.ts"), "");
        write_file(
            &root.join(".git"),
            &format!("gitdir: {}\n", git_dir.display()),
        );

        let walked = walked_files_within(source_walk_builder(&root, |_| true), &root)
            .expect("the walk must end although the worktree exclude file is a FIFO");
        assert!(walked.contains(&"src/index.ts".to_string()), "{walked:?}");
    }

    /// A worktree at `base/worktree` whose `.git` file holds `git_file`.
    #[cfg(unix)]
    fn worktree_with_git_file(base: &Path, git_file: &str) -> PathBuf {
        let root = base.join("worktree");
        write_file(&root.join("src/index.ts"), "");
        write_file(&root.join(".git"), git_file);
        root
    }

    #[cfg(unix)]
    #[test]
    fn source_walk_keeps_a_carriage_return_at_the_end_of_an_unterminated_gitdir_line() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let base = directory.path();
        let git_dir = base.join("repository-git\r");
        std::fs::create_dir_all(&git_dir).expect("git directory");
        make_fifo(&git_dir.join("commondir"));
        let root = worktree_with_git_file(base, &format!("gitdir: {}", git_dir.display()));

        let walked = walked_files_within(source_walk_builder(&root, |_| true), &root)
            .expect("the walk must end although the commondir file is a FIFO");
        assert!(walked.contains(&"src/index.ts".to_string()), "{walked:?}");
    }

    #[cfg(unix)]
    #[test]
    fn source_walk_keeps_a_carriage_return_at_the_end_of_an_unterminated_commondir_line() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let base = directory.path();
        let git_dir = base.join("repository-git");
        let common = base.join("common\r");
        std::fs::create_dir_all(common.join("info")).expect("common info directory");
        make_fifo(&common.join("info/exclude"));
        write_file(&git_dir.join("commondir"), &common.display().to_string());
        let root = worktree_with_git_file(base, &format!("gitdir: {}\n", git_dir.display()));

        let walked = walked_files_within(source_walk_builder(&root, |_| true), &root)
            .expect("the walk must end although the exclude file is a FIFO");
        assert!(walked.contains(&"src/index.ts".to_string()), "{walked:?}");
    }

    #[cfg(unix)]
    #[test]
    fn source_walk_skips_an_ignore_file_above_the_size_cap() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let root = directory.path();
        std::fs::create_dir_all(root.join(".git")).expect("git directory");
        write_file(&root.join("src/big.ts"), "");
        let mut rules = String::from("big.ts\n");
        rules.push_str(&"#".repeat(2 * 1024 * 1024));
        write_file(&root.join(".gitignore"), &rules);

        let walked =
            walked_files_within(source_walk_builder(root, |_| true), root).expect("walk ends");
        assert!(
            walked.contains(&"src/big.ts".to_string()),
            "the rules of a file above the cap do not apply: {walked:?}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn source_walk_reads_regular_and_in_project_linked_ignore_files() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let root = directory.path();
        std::fs::create_dir_all(root.join(".git")).expect("git directory");
        write_file(&root.join(".gitignore"), "dist\n");
        write_file(&root.join("shared-rules"), "generated.ts\n");
        write_file(&root.join("src/index.ts"), "");
        write_file(&root.join("src/generated.ts"), "");
        write_file(&root.join("dist/out.js"), "");
        std::os::unix::fs::symlink("../shared-rules", root.join("src/.gitignore"))
            .expect("link inside the project");

        let walked =
            walked_files_within(source_walk_builder(root, |_| true), root).expect("walk ends");
        assert!(walked.contains(&"src/index.ts".to_string()), "{walked:?}");
        assert!(!walked.contains(&"dist/out.js".to_string()), "{walked:?}");
        assert!(
            !walked.contains(&"src/generated.ts".to_string()),
            "{walked:?}"
        );
    }
}
