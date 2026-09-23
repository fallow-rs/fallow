//! Centralized project state with file registry and workspace metadata.

use fallow_config::WorkspaceInfo;

use fallow_types::discover::{DiscoveredFile, FileId};

/// Discovered files and workspace packages for one analysis run.
///
/// The files are indexed by their dense `FileId`.
pub struct ProjectState {
    files: Vec<DiscoveredFile>,
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
        Self { files, workspaces }
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
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

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
    fn empty_state() {
        let state = ProjectState::new(vec![], vec![]);
        assert!(state.files().is_empty());
        assert!(state.workspaces().is_empty());
        assert!(state.file_by_id(FileId(0)).is_none());
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
}
