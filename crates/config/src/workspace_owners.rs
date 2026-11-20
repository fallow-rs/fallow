//! Deepest-root lookup for a set of workspace roots.

use std::path::{Path, PathBuf};

use rustc_hash::FxHashMap;

use crate::WorkspaceInfo;

/// Finds the deepest root that holds a path.
#[derive(Debug, Clone, Default)]
pub struct WorkspaceOwners {
    index_by_root: FxHashMap<PathBuf, usize>,
}

impl WorkspaceOwners {
    /// Index the roots of `workspaces`. An owner is an index into `workspaces`.
    #[must_use]
    pub fn new(workspaces: &[WorkspaceInfo]) -> Self {
        Self::from_roots(workspaces.iter().map(|workspace| workspace.root.as_path()))
    }

    /// Index `roots`. An owner is the position of a root in `roots`. When a
    /// root occurs two times, the first position owns it.
    #[must_use]
    pub fn from_roots<'a>(roots: impl IntoIterator<Item = &'a Path>) -> Self {
        let mut index_by_root = FxHashMap::default();
        for (index, root) in roots.into_iter().enumerate() {
            index_by_root.entry(root.to_path_buf()).or_insert(index);
        }
        Self { index_by_root }
    }

    /// The owner of the deepest root that is `path` or an ancestor of `path`.
    #[must_use]
    pub fn owner_of(&self, path: &Path) -> Option<usize> {
        path.ancestors()
            .find_map(|dir| self.index_by_root.get(dir).copied())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn owners(roots: &[&str]) -> WorkspaceOwners {
        WorkspaceOwners::from_roots(roots.iter().map(Path::new))
    }

    #[test]
    fn deepest_root_owns_a_path() {
        let owners = owners(&["/p/apps/web", "/p/apps/web/plugin"]);
        assert_eq!(
            owners.owner_of(Path::new("/p/apps/web/plugin/src/a.ts")),
            Some(1)
        );
        assert_eq!(owners.owner_of(Path::new("/p/apps/web/src/a.ts")), Some(0));
    }

    #[test]
    fn a_root_owns_itself() {
        assert_eq!(
            owners(&["/p/apps/web"]).owner_of(Path::new("/p/apps/web")),
            Some(0)
        );
    }

    #[test]
    fn a_sibling_with_the_same_name_prefix_is_not_owned() {
        assert_eq!(
            owners(&["/p/apps/web"]).owner_of(Path::new("/p/apps/web-admin/src/a.ts")),
            None
        );
    }

    #[test]
    fn a_path_outside_every_root_has_no_owner() {
        assert_eq!(
            owners(&["/p/apps/web"]).owner_of(Path::new("/p/src/a.ts")),
            None
        );
    }

    #[test]
    fn the_first_of_two_equal_roots_owns_the_path() {
        assert_eq!(
            owners(&["/p/apps/web", "/p/apps/web"]).owner_of(Path::new("/p/apps/web/a.ts")),
            Some(0)
        );
    }
}
