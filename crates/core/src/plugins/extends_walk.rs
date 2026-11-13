//! The state of one walk over the `extends` chain of an `ESLint` or
//! Stylelint config, and the lookup of the workspace packages it names.

use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex, PoisonError};
use std::time::SystemTime;

use rustc_hash::{FxHashMap, FxHashSet};

use super::eslint::{find_package_dir, resolve_package_entry};

/// The configs already read, and the workspace packages of the project.
///
/// A project without an install has no `node_modules` link for a workspace
/// package, so an `extends` entry that names one is found by package name.
/// The walk gets the workspace index once, when the first entry needs it.
pub(in crate::plugins) struct ExtendsWalk<'a> {
    pub(in crate::plugins) visited: FxHashSet<PathBuf>,
    project_root: &'a Path,
    workspaces: Option<Arc<WorkspaceIndex>>,
}

impl<'a> ExtendsWalk<'a> {
    pub(in crate::plugins) fn new(project_root: &'a Path) -> Self {
        Self {
            visited: FxHashSet::default(),
            project_root,
            workspaces: None,
        }
    }

    /// The analysis root of the walk.
    pub(in crate::plugins) fn project_root(&self) -> &'a Path {
        self.project_root
    }

    /// The directory of the workspace package `name`, if the project declares
    /// one with that name.
    fn workspace_package_dir(&mut self, name: &str) -> Option<PathBuf> {
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
/// one `ESLint` or Stylelint config per package. One index per project root,
/// rebuilt when a stamp changes, keeps the cost to one discovery per analysis
/// run.
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

/// The directory of a package that a config names: the `node_modules` entry
/// that Node finds from `config_dir`, or the workspace package of that name.
pub(in crate::plugins) fn workspace_package_dir(
    config_dir: &Path,
    pkg_name: &str,
    walk: &mut ExtendsWalk<'_>,
) -> Option<PathBuf> {
    find_package_dir(config_dir, pkg_name).or_else(|| walk.workspace_package_dir(pkg_name))
}

/// The file that `require` loads for `subpath` (empty or `/<path>`) of the
/// package in `pkg_dir`, when that file is a project file. A file under
/// `node_modules` gives `None`. The returned path uses the spelling of
/// `root`, so it matches the discovered file.
pub(in crate::plugins) fn workspace_package_file(
    pkg_dir: &Path,
    subpath: &str,
    root: &Path,
) -> Option<PathBuf> {
    let pkg_json_str = std::fs::read_to_string(pkg_dir.join("package.json")).ok()?;
    let pkg_json: serde_json::Value = serde_json::from_str(&pkg_json_str).ok()?;
    let subpath_key = format!(".{subpath}");
    let entry_rel = required_main_entry(&pkg_json, &subpath_key)
        .or_else(|| resolve_package_entry(&pkg_json, &subpath_key, pkg_dir))?;
    let entry = dunce::canonicalize(pkg_dir.join(entry_rel)).ok()?;
    let (ancestor, rel) = root.ancestors().find_map(|ancestor| {
        let canonical = dunce::canonicalize(ancestor).ok()?;
        let rel = entry.strip_prefix(&canonical).ok()?.to_path_buf();
        Some((ancestor, rel))
    })?;
    if rel
        .components()
        .any(|component| component.as_os_str() == "node_modules")
    {
        return None;
    }
    Some(ancestor.join(rel))
}

/// The `main` entry that `require` loads for a package without `exports`.
/// The config loaders use `require`, so `module` does not apply.
fn required_main_entry(pkg_json: &serde_json::Value, subpath_key: &str) -> Option<String> {
    if subpath_key != "." || pkg_json.get("exports").is_some_and(|e| !e.is_null()) {
        return None;
    }
    pkg_json.get("main")?.as_str().map(str::to_string)
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
