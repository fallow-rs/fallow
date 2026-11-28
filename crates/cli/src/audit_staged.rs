//! The temporary worktree of `fallow audit --staged`.
//!
//! The audit reads files from disk, so a staged audit needs the staged
//! contents on disk. [`StagedTree::create`] writes the staged index to a tree
//! object and materializes that tree in a temporary worktree at `HEAD`. In
//! that worktree, `git diff HEAD` shows exactly the staged changes. Untracked
//! files and unstaged edits of the checkout are not in it.
//!
//! The worktree is new on each run, so its own cache directory is empty.
//! [`super::worktree_cache::WorktreeParseCache`] gives the run the parse
//! cache of the checkout.

use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::SystemTime;

/// The variable that Git sets for a hook when the commit uses another index
/// file, for example `git commit -a` or `git commit <path>`.
const INDEX_FILE_ENV: &str = "GIT_INDEX_FILE";

/// A temporary worktree that holds the staged index. Drop removes it.
pub struct StagedTree {
    host_toplevel: PathBuf,
    path: PathBuf,
    root: PathBuf,
}

impl StagedTree {
    /// Materialize the staged index of the repository that contains
    /// `host_root`.
    ///
    /// # Errors
    ///
    /// Returns a message when `host_root` is not in a Git repository, `HEAD`
    /// has no commit, the index has unmerged entries, or Git cannot create
    /// the worktree.
    pub fn create(
        host_root: &Path,
        config: crate::runtime_support::ConfigFlags<'_>,
    ) -> Result<Self, String> {
        let host_toplevel = fallow_engine::changed_files::resolve_git_toplevel(host_root)
            .map_err(|error| format!("--staged needs a git repository: {}", error.describe()))?;
        let relative_root = relative_root(host_root, &host_toplevel)?;
        require_head_commit(&host_toplevel)?;
        let index_file = ambient_index_file(&host_toplevel);
        let staged_tree =
            fallow_engine::repo_refs::write_index_tree(&host_toplevel, index_file.as_deref())
                .map_err(|error| error.to_string())?;
        let path = staged_worktree_path()
            .ok_or_else(|| "could not choose a temporary directory for --staged".to_owned())?;
        fallow_engine::repo_refs::create_staged_index_worktree(&host_toplevel, &path, &staged_tree)
            .map_err(|error| format!("could not materialize the staged index: {error}"))?;
        let path = dunce::canonicalize(&path).unwrap_or(path);
        // `join("")` adds a trailing separator, which breaks the root-prefix
        // strip of the report paths.
        let root = if relative_root.as_os_str().is_empty() {
            path.clone()
        } else {
            path.join(&relative_root)
        };
        let host_root_in_checkout = host_toplevel.join(&relative_root);
        let tree = Self {
            host_toplevel,
            path,
            root,
        };
        // A drop of `tree` removes the worktree, so the check runs after its
        // construction and before the first read or write under the root.
        check_staged_root(&tree.path, &tree.root, &host_root_in_checkout)?;
        let patterns = workspace_patterns(&tree.root, config);
        fallow_engine::repo_refs::materialize_base_dependency_context(
            host_root, &tree.root, &patterns,
        );
        // The dependency links are untracked entries of the worktree. They
        // are not changes of the commit.
        fallow_engine::changed_files::exclude_untracked_files();
        Ok(tree)
    }

    /// The analysis root inside the worktree. It maps onto the host root.
    pub fn root(&self) -> &Path {
        &self.root
    }
}

impl Drop for StagedTree {
    fn drop(&mut self) {
        fallow_engine::repo_refs::remove_staged_index_worktree(&self.host_toplevel, &self.path);
    }
}

/// Refuse a `root` in the staged worktree at `worktree` that a link in the
/// staged index moves out of the worktree. `host_root` is the same root in
/// the checkout of the user, for the message. A root that the index does not
/// hold stays allowed here.
fn check_staged_root(worktree: &Path, root: &Path, host_root: &Path) -> Result<(), String> {
    if std::fs::symlink_metadata(root).is_err() {
        return Ok(());
    }
    match dunce::canonicalize(root) {
        Ok(resolved) if resolved.starts_with(worktree) => Ok(()),
        Ok(resolved) => Err(format!(
            "--staged refuses the root '{}': a link in the staged index resolves it to '{}', outside the staged worktree",
            host_root.display(),
            resolved.display()
        )),
        Err(err) => Err(format!(
            "--staged refuses the root '{}': a link in the staged index makes it a path that cannot be resolved: {err}",
            host_root.display()
        )),
    }
}

fn relative_root(host_root: &Path, host_toplevel: &Path) -> Result<PathBuf, String> {
    let canonical = dunce::canonicalize(host_root).unwrap_or_else(|_| host_root.to_path_buf());
    canonical
        .strip_prefix(host_toplevel)
        .map(Path::to_path_buf)
        .map_err(|_| {
            format!(
                "--staged needs a root inside the git repository at {}",
                host_toplevel.display()
            )
        })
}

fn require_head_commit(toplevel: &Path) -> Result<(), String> {
    match fallow_engine::repo_refs::head_sha(toplevel) {
        Ok(Some(_)) => Ok(()),
        Ok(None) => {
            Err("--staged needs a commit at HEAD to compare the staged index with".to_owned())
        }
        Err(error) => Err(format!("could not run git: {error}")),
    }
}

/// The index file that Git gave this process (`GIT_INDEX_FILE`, for example
/// for `git commit -a` or `git commit <path>`), as an absolute path. A plain
/// `git commit` sets the relative value `.git/index`. Git resolves a relative
/// value against the top level of the work tree, not against the current
/// directory, so a hook that runs from a subdirectory still finds the index.
fn ambient_index_file(toplevel: &Path) -> Option<PathBuf> {
    let value: OsString = std::env::var_os(INDEX_FILE_ENV).filter(|value| !value.is_empty())?;
    Some(toplevel.join(value))
}

/// A unique temporary path. The `fallow-audit-base-<pid>-` prefix lets the
/// orphan sweep of later audits remove the worktree after a crash.
fn staged_worktree_path() -> Option<PathBuf> {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let seq = SEQ.fetch_add(1, Ordering::Relaxed);
    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .ok()?
        .as_nanos();
    Some(crate::base_worktree::ensure_snapshot_dir()?.join(format!(
        "fallow-audit-base-{}-staged-{nanos}-{seq}",
        std::process::id()
    )))
}

/// The `workspaces.patterns` of the staged tree, from the config that the
/// audit of the tree reads: the `--config` file, else the config file of the
/// index that discovery finds from the tree root. A load error gives no
/// patterns.
fn workspace_patterns(root: &Path, config: crate::runtime_support::ConfigFlags<'_>) -> Vec<String> {
    crate::runtime_support::loaded_config(root, config.path, config.file_options())
        .and_then(|config| config.workspaces)
        .map(|workspaces| workspaces.patterns)
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runtime_support::ConfigFlags;

    #[test]
    fn workspace_patterns_come_from_the_explicit_config() {
        let dir = tempfile::tempdir().expect("tempdir");
        let root = dir.path();
        std::fs::write(
            root.join(".fallowrc.json"),
            r#"{"workspaces":{"patterns":["apps/*"]}}"#,
        )
        .expect("discovered config");
        let explicit = root.join("ci.json");
        std::fs::write(&explicit, r#"{"workspaces":{"patterns":["packages/*"]}}"#)
            .expect("explicit config");

        assert_eq!(workspace_patterns(root, ConfigFlags::default()), ["apps/*"]);
        let flags = ConfigFlags {
            path: Some(&explicit),
            ..ConfigFlags::default()
        };
        assert_eq!(workspace_patterns(root, flags), ["packages/*"]);
    }

    /// The index can hold a link where the checkout has a directory. A root
    /// below that link must not move the staged audit out of its worktree.
    #[cfg(unix)]
    #[test]
    fn staged_tree_refuses_a_root_that_a_staged_link_moves_outside_the_worktree() {
        use fallow_engine::git_fixture::fixture_git_capture as git;

        let temp = tempfile::tempdir().expect("tempdir");
        let repo = temp.path().join("repo");
        let app = repo.join("apps").join("mobile");
        std::fs::create_dir_all(&app).expect("app dir");
        std::fs::write(app.join("index.ts"), "export const a = 1;\n").expect("source");
        let outside = temp.path().join("outside");
        std::fs::create_dir_all(outside.join("mobile")).expect("outside dir");
        git(&repo, &["init", "-q", "-b", "main"]);
        git(&repo, &["add", "."]);
        git(&repo, &["commit", "-q", "-m", "initial"]);
        git(&repo, &["rm", "-q", "-r", "--cached", "apps"]);
        let link_blob = {
            let path = temp.path().join("link-target");
            std::fs::write(&path, outside.display().to_string()).expect("link target");
            git(&repo, &["hash-object", "-w", path.to_str().expect("utf-8")])
        };
        git(
            &repo,
            &[
                "update-index",
                "--add",
                "--cacheinfo",
                &format!("120000,{link_blob},apps"),
            ],
        );

        let err = StagedTree::create(&app, ConfigFlags::default())
            .err()
            .expect("the staged tree refuses the root");
        assert!(err.contains("outside the staged worktree"), "got: {err}");
        let host_root = format!("'{}'", dunce::canonicalize(&app).unwrap().display());
        assert!(err.contains(&host_root), "the host root: {err}");
        let target = dunce::canonicalize(outside.join("mobile")).unwrap();
        assert!(
            err.contains(&format!("'{}'", target.display())),
            "the real target: {err}"
        );
    }

    /// A staged link with no target leaves the root without a real path.
    #[cfg(unix)]
    #[test]
    fn staged_root_check_names_a_root_that_cannot_be_resolved() {
        let temp = tempfile::tempdir().expect("tempdir");
        let worktree = temp.path().join("worktree");
        std::fs::create_dir_all(&worktree).expect("worktree");
        std::os::unix::fs::symlink("/nonexistent-fallow-target", worktree.join("apps"))
            .expect("link");
        let host_root = Path::new("/host/repo/apps");

        let err = check_staged_root(&worktree, &worktree.join("apps"), host_root).unwrap_err();
        assert!(err.contains("'/host/repo/apps'"), "got: {err}");
        assert!(err.contains("cannot be resolved"), "got: {err}");
        assert_eq!(
            check_staged_root(&worktree, &worktree.join("absent"), host_root),
            Ok(()),
            "a root that the index does not hold"
        );
    }
}
