//! The state of a project file that a config key names, decided without a
//! read of the file.

use std::path::Path;

use fallow_types::path_guard::{lexically_normalized, outside_link};

/// The state of a path that a config key names, relative to the project.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProjectEntry {
    /// The path stays inside the project, and no entry exists there.
    Missing,
    /// The path leaves the project by text or through a link, or the check
    /// cannot decide. A missing target outside the project also gets this
    /// state, the same as a target there that exists.
    Outside,
    /// The path exists and resolves inside the project.
    Inside,
}

/// The state of `path` below the canonical project root `canonical_root`.
/// A relative `path` is taken relative to the root.
pub fn project_entry(canonical_root: &Path, path: &Path) -> ProjectEntry {
    let normalized = lexically_normalized(&canonical_root.join(path));
    if outside_link(canonical_root, &normalized).is_some() {
        return ProjectEntry::Outside;
    }
    if normalized.symlink_metadata().is_err() {
        return ProjectEntry::Missing;
    }
    if fallow_types::read_guard::is_inside_root(canonical_root, &normalized) {
        ProjectEntry::Inside
    } else {
        ProjectEntry::Outside
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg_attr(miri, ignore = "tempdir is blocked by Miri isolation")]
    fn missing_outside_and_inside_paths() {
        let dir = tempfile::tempdir().unwrap();
        let root = dunce::canonicalize(dir.path()).unwrap();
        std::fs::write(root.join("a.json"), "{}").unwrap();
        assert_eq!(
            project_entry(&root, Path::new("a.json")),
            ProjectEntry::Inside
        );
        assert_eq!(
            project_entry(&root, Path::new("b.json")),
            ProjectEntry::Missing
        );
        assert_eq!(
            project_entry(&root, Path::new("../b.json")),
            ProjectEntry::Outside
        );
        assert_eq!(
            project_entry(&root, Path::new("/nonexistent/b.json")),
            ProjectEntry::Outside
        );
        assert_eq!(
            project_entry(&root, Path::new("sub/../a.json")),
            ProjectEntry::Inside
        );
    }

    #[cfg(unix)]
    #[test]
    #[cfg_attr(miri, ignore = "tempdir is blocked by Miri isolation")]
    fn a_missing_file_behind_an_outside_directory_link_is_outside() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("project");
        std::fs::create_dir(&root).unwrap();
        let root = dunce::canonicalize(&root).unwrap();
        std::os::unix::fs::symlink(dir.path(), root.join("up")).unwrap();
        assert_eq!(
            project_entry(&root, Path::new("up/missing.json")),
            ProjectEntry::Outside
        );
    }
}
