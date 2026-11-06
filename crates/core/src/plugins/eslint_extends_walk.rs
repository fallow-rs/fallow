//! The state of one walk over the `extends` chain of an `ESLint` config.

use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex, PoisonError};
use std::time::SystemTime;

use rustc_hash::{FxHashMap, FxHashSet};

/// The configs already read, and the workspace packages of the project.
///
/// A project without an install has no `node_modules` link for a workspace
/// package, so an `extends` entry that names one is found by package name.
/// The walk gets the workspace index once, when the first entry needs it.
pub(super) struct ExtendsWalk<'a> {
    pub(super) visited: FxHashSet<PathBuf>,
    project_root: &'a Path,
    workspaces: Option<Arc<WorkspaceIndex>>,
}

impl<'a> ExtendsWalk<'a> {
    pub(super) fn new(project_root: &'a Path) -> Self {
        Self {
            visited: FxHashSet::default(),
            project_root,
            workspaces: None,
        }
    }

    /// The directory of the workspace package `name`, if the project declares
    /// one with that name.
    pub(super) fn workspace_package_dir(&mut self, name: &str) -> Option<PathBuf> {
        let project_root = self.project_root;
        self.workspaces
            .get_or_insert_with(|| workspace_index(project_root))
            .dirs
            .get(name)
            .cloned()
    }
}

/// The workspace packages of a project by name, and the modification times
/// of the files and directories that the discovery read.
struct WorkspaceIndex {
    dirs: FxHashMap<String, PathBuf>,
    stamps: Vec<(PathBuf, Option<SystemTime>)>,
}

impl WorkspaceIndex {
    fn build(project_root: &Path) -> Self {
        let workspaces = fallow_config::discover_workspaces(project_root);
        let mut stamped: Vec<PathBuf> = ROOT_MANIFESTS
            .iter()
            .map(|name| project_root.join(name))
            .collect();
        for workspace in &workspaces {
            stamped.push(workspace.root.join("package.json"));
            if let Some(parent) = workspace.root.parent() {
                stamped.push(parent.to_path_buf());
            }
        }
        stamped.sort_unstable();
        stamped.dedup();
        Self {
            dirs: workspaces
                .into_iter()
                .map(|workspace| (workspace.name, workspace.root))
                .collect(),
            stamps: stamped
                .into_iter()
                .map(|path| {
                    let stamp = modified(&path);
                    (path, stamp)
                })
                .collect(),
        }
    }

    /// True when no file or directory that the discovery read has changed.
    /// A new package directory changes the time of its parent directory.
    fn is_current(&self) -> bool {
        self.stamps
            .iter()
            .all(|(path, stamp)| modified(path) == *stamp)
    }
}

/// The root files that declare the workspace patterns.
const ROOT_MANIFESTS: &[&str] = &["package.json", "pnpm-workspace.yaml"];

fn modified(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path)
        .and_then(|meta| meta.modified())
        .ok()
}

/// Workspace discovery reads every workspace manifest, and a project can have
/// one `ESLint` config per package. One index per project root, rebuilt when
/// a stamp changes, keeps the cost to one discovery per analysis run.
static WORKSPACE_INDEXES: LazyLock<Mutex<FxHashMap<PathBuf, Arc<WorkspaceIndex>>>> =
    LazyLock::new(Mutex::default);

fn workspace_index(project_root: &Path) -> Arc<WorkspaceIndex> {
    let cached = WORKSPACE_INDEXES
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .get(project_root)
        .cloned();
    if let Some(index) = cached.filter(|index| index.is_current()) {
        return index;
    }
    let index = Arc::new(WorkspaceIndex::build(project_root));
    WORKSPACE_INDEXES
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .insert(project_root.to_path_buf(), Arc::clone(&index));
    index
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(root: &Path, rel: &str, contents: &str) {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, contents).unwrap();
    }

    #[test]
    fn workspace_index_finds_packages_and_sees_a_new_package() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(
            root,
            "package.json",
            r#"{"name":"root","private":true,"workspaces":["packages/*"]}"#,
        );
        write(root, "packages/a/package.json", r#"{"name":"pkg-a"}"#);

        let mut walk = ExtendsWalk::new(root);
        assert_eq!(
            walk.workspace_package_dir("pkg-a"),
            Some(root.join("packages/a"))
        );
        assert_eq!(walk.workspace_package_dir("pkg-b"), None);

        write(root, "packages/b/package.json", r#"{"name":"pkg-b"}"#);
        let mut next_walk = ExtendsWalk::new(root);
        assert_eq!(
            next_walk.workspace_package_dir("pkg-b"),
            Some(root.join("packages/b"))
        );
    }
}
