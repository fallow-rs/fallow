//! The temporary worktree of `fallow audit --staged`.
//!
//! The audit reads files from disk, so a staged audit needs the staged
//! contents on disk. [`StagedTree::create`] writes the staged index to a tree
//! object and materializes that tree in a temporary worktree at `HEAD`. In
//! that worktree, `git diff HEAD` shows exactly the staged changes. Untracked
//! files and unstaged edits of the checkout are not in it.

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
    pub fn create(host_root: &Path) -> Result<Self, String> {
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
            path.join(relative_root)
        };
        let tree = Self {
            host_toplevel,
            path,
            root,
        };
        let patterns = workspace_patterns(&tree.root);
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
    Some(std::env::temp_dir().join(format!(
        "fallow-audit-base-{}-staged-{nanos}-{seq}",
        std::process::id()
    )))
}

fn workspace_patterns(root: &Path) -> Vec<String> {
    fallow_config::FallowConfig::find_and_load_with_options(
        root,
        fallow_config::ConfigLoadOptions {
            allow_remote_extends: false,
        },
    )
    .ok()
    .flatten()
    .and_then(|(config, _)| config.workspaces)
    .map(|workspaces| workspaces.patterns)
    .unwrap_or_default()
}
