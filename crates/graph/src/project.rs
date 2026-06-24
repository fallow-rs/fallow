//! Centralized project state with file registry and workspace metadata.

use std::path::{Path, PathBuf};

use rustc_hash::FxHashMap;

use fallow_config::WorkspaceInfo;

use fallow_types::discover::{DiscoveredFile, FileId, StableFileKey};

/// Centralized project state owning the file registry and workspace metadata.
///
/// Provides:
/// - Stable `FileId` assignment (deterministic by path, not by size)
/// - O(1) path-to-id lookups for cross-workspace module resolution
/// - Workspace-aware queries (which workspace owns a file, files in a workspace)
///
/// Future incremental analysis will persist the id assignment across runs so
/// that adding/removing files does not invalidate cached graph data.
pub struct ProjectState {
    files: Vec<DiscoveredFile>,
    path_to_id: FxHashMap<PathBuf, FileId>,
    workspaces: Vec<WorkspaceInfo>,
}

impl ProjectState {
    /// Build a new project state from discovered files and workspaces.
    #[must_use]
    pub fn new(files: Vec<DiscoveredFile>, workspaces: Vec<WorkspaceInfo>) -> Self {
        debug_assert!(
            files.iter().enumerate().all(|(i, f)| f.id.0 as usize == i),
            "FileIds must be densely packed starting at 0"
        );
        let path_to_id = files.iter().map(|f| (f.path.clone(), f.id)).collect();
        Self {
            files,
            path_to_id,
            workspaces,
        }
    }

    /// All discovered files, indexed by `FileId`.
    #[must_use]
    pub fn files(&self) -> &[DiscoveredFile] {
        &self.files
    }

    /// All discovered workspace packages.
    #[must_use]
    pub fn workspaces(&self) -> &[WorkspaceInfo] {
        &self.workspaces
    }

    /// Look up a file by its `FileId`.
    #[must_use]
    pub fn file_by_id(&self, id: FileId) -> Option<&DiscoveredFile> {
        self.files.get(id.0 as usize)
    }

    /// Look up a `FileId` by absolute path.
    #[must_use]
    pub fn id_for_path(&self, path: &Path) -> Option<FileId> {
        self.path_to_id.get(path).copied()
    }

    /// Stable file key for a `FileId`, root-relative where possible.
    #[must_use]
    pub fn stable_key_for_file(&self, root: &Path, id: FileId) -> Option<StableFileKey> {
        let file = self.file_by_id(id)?;
        Some(StableFileKey::from_root_relative(root, &file.path))
    }

    /// Look up the current in-memory `FileId` for a stable root-relative key.
    #[must_use]
    pub fn id_for_stable_key(&self, root: &Path, key: &StableFileKey) -> Option<FileId> {
        self.files
            .iter()
            .find(|file| StableFileKey::from_root_relative(root, &file.path) == *key)
            .map(|file| file.id)
    }

    /// Find which workspace a file belongs to, if any.
    #[must_use]
    pub fn workspace_for_file(&self, id: FileId) -> Option<&WorkspaceInfo> {
        let path = &self.files.get(id.0 as usize)?.path;
        self.workspaces.iter().find(|ws| path.starts_with(&ws.root))
    }

    /// Look up a workspace by package name.
    #[must_use]
    pub fn workspace_by_name(&self, name: &str) -> Option<&WorkspaceInfo> {
        self.workspaces.iter().find(|ws| ws.name == name)
    }

    /// Get all `FileId`s for files within a workspace.
    #[must_use]
    pub fn files_in_workspace(&self, ws: &WorkspaceInfo) -> Vec<FileId> {
        self.files
            .iter()
            .filter(|f| f.path.starts_with(&ws.root))
            .map(|f| f.id)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_file(id: u32, path: &str) -> DiscoveredFile {
        DiscoveredFile {
            id: FileId(id),
            path: PathBuf::from(path),
            size_bytes: 100,
        }
    }

    fn make_workspace(name: &str, root: &str) -> WorkspaceInfo {
        WorkspaceInfo {
            root: PathBuf::from(root),
            name: name.to_string(),
            is_internal_dependency: false,
        }
    }

    #[test]
    fn id_for_path_lookup() {
        let files = vec![
            make_file(0, "/project/packages/a/src/index.ts"),
            make_file(1, "/project/packages/b/src/index.ts"),
        ];
        let state = ProjectState::new(files, vec![]);
        assert_eq!(
            state.id_for_path(Path::new("/project/packages/a/src/index.ts")),
            Some(FileId(0))
        );
        assert_eq!(
            state.id_for_path(Path::new("/project/packages/b/src/index.ts")),
            Some(FileId(1))
        );
        assert_eq!(state.id_for_path(Path::new("/project/missing.ts")), None);
    }

    #[test]
    fn stable_key_for_file_is_root_relative() {
        let files = vec![make_file(0, "/project/src/a.ts")];
        let state = ProjectState::new(files, vec![]);

        let key = state
            .stable_key_for_file(Path::new("/project"), FileId(0))
            .unwrap();

        assert_eq!(key.as_str(), "src/a.ts");
        assert_eq!(
            state.id_for_stable_key(Path::new("/project"), &key),
            Some(FileId(0))
        );
    }

    #[test]
    fn stable_key_survives_file_id_shift() {
        let before = ProjectState::new(
            vec![
                make_file(0, "/project/src/a.ts"),
                make_file(1, "/project/src/c.ts"),
            ],
            vec![],
        );
        let after = ProjectState::new(
            vec![
                make_file(0, "/project/src/a.ts"),
                make_file(1, "/project/src/b.ts"),
                make_file(2, "/project/src/c.ts"),
            ],
            vec![],
        );
        let root = Path::new("/project");
        let key = before.stable_key_for_file(root, FileId(1)).unwrap();

        assert_eq!(key.as_str(), "src/c.ts");
        assert_eq!(before.id_for_stable_key(root, &key), Some(FileId(1)));
        assert_eq!(after.id_for_stable_key(root, &key), Some(FileId(2)));
    }

    #[test]
    fn workspace_for_file_lookup() {
        let files = vec![
            make_file(0, "/project/packages/ui/src/button.ts"),
            make_file(1, "/project/src/app.ts"),
        ];
        let workspaces = vec![make_workspace("ui", "/project/packages/ui")];
        let state = ProjectState::new(files, workspaces);

        assert_eq!(
            state.workspace_for_file(FileId(0)).map(|ws| &ws.name),
            Some(&"ui".to_string())
        );
        assert!(state.workspace_for_file(FileId(1)).is_none());
    }

    #[test]
    fn workspace_by_name_lookup() {
        let workspaces = vec![
            make_workspace("ui", "/project/packages/ui"),
            make_workspace("core", "/project/packages/core"),
        ];
        let state = ProjectState::new(vec![], workspaces);

        assert!(state.workspace_by_name("ui").is_some());
        assert!(state.workspace_by_name("core").is_some());
        assert!(state.workspace_by_name("missing").is_none());
    }

    #[test]
    fn files_in_workspace() {
        let files = vec![
            make_file(0, "/project/packages/ui/src/a.ts"),
            make_file(1, "/project/packages/ui/src/b.ts"),
            make_file(2, "/project/packages/core/src/c.ts"),
            make_file(3, "/project/src/app.ts"),
        ];
        let workspaces = vec![
            make_workspace("ui", "/project/packages/ui"),
            make_workspace("core", "/project/packages/core"),
        ];
        let state = ProjectState::new(files, workspaces);

        let ui_ws = state.workspace_by_name("ui").unwrap();
        let ui_files = state.files_in_workspace(ui_ws);
        assert_eq!(ui_files, vec![FileId(0), FileId(1)]);

        let core_ws = state.workspace_by_name("core").unwrap();
        let core_files = state.files_in_workspace(core_ws);
        assert_eq!(core_files, vec![FileId(2)]);
    }

    #[test]
    fn file_by_id_valid() {
        let files = vec![
            make_file(0, "/project/src/a.ts"),
            make_file(1, "/project/src/b.ts"),
        ];
        let state = ProjectState::new(files, vec![]);
        let file = state.file_by_id(FileId(0)).unwrap();
        assert_eq!(file.path, PathBuf::from("/project/src/a.ts"));
        assert_eq!(file.id, FileId(0));
    }

    #[test]
    fn file_by_id_out_of_bounds() {
        let files = vec![make_file(0, "/project/src/a.ts")];
        let state = ProjectState::new(files, vec![]);
        assert!(state.file_by_id(FileId(999)).is_none());
    }

    #[test]
    fn workspace_for_file_out_of_bounds() {
        let files = vec![make_file(0, "/project/src/a.ts")];
        let workspaces = vec![make_workspace("app", "/project")];
        let state = ProjectState::new(files, workspaces);
        assert!(state.workspace_for_file(FileId(999)).is_none());
    }

    #[test]
    fn empty_state() {
        let state = ProjectState::new(vec![], vec![]);
        assert!(state.files().is_empty());
        assert!(state.workspaces().is_empty());
        assert!(state.file_by_id(FileId(0)).is_none());
        assert!(state.id_for_path(Path::new("/any")).is_none());
        assert!(state.workspace_by_name("any").is_none());
    }

    #[test]
    fn files_returns_all_files() {
        let files = vec![
            make_file(0, "/project/src/a.ts"),
            make_file(1, "/project/src/b.ts"),
        ];
        let state = ProjectState::new(files, vec![]);
        assert_eq!(state.files().len(), 2);
        assert_eq!(state.files()[0].id, FileId(0));
        assert_eq!(state.files()[1].id, FileId(1));
    }

    #[test]
    fn workspaces_returns_all_workspaces() {
        let workspaces = vec![
            make_workspace("a", "/project/packages/a"),
            make_workspace("b", "/project/packages/b"),
        ];
        let state = ProjectState::new(vec![], workspaces);
        assert_eq!(state.workspaces().len(), 2);
    }

    #[test]
    fn files_in_workspace_empty_when_no_match() {
        let files = vec![make_file(0, "/other/path/file.ts")];
        let workspaces = vec![make_workspace("ui", "/project/packages/ui")];
        let state = ProjectState::new(files, workspaces);
        let ws = state.workspace_by_name("ui").unwrap();
        assert!(state.files_in_workspace(ws).is_empty());
    }

    #[test]
    fn workspace_for_file_nested_workspaces() {
        let files = vec![make_file(0, "/project/packages/ui/components/Button.ts")];
        let workspaces = vec![
            make_workspace("root", "/project"),
            make_workspace("ui", "/project/packages/ui"),
        ];
        let state = ProjectState::new(files, workspaces);
        let ws = state.workspace_for_file(FileId(0)).unwrap();
        assert_eq!(ws.name, "root");
    }
}
