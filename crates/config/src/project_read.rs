//! The state of a project file that a config key names, decided without a
//! read of the file.

use std::path::{Component, Path, PathBuf};

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
    project_entry_within(canonical_root, canonical_root, path)
}

/// The state of `path`, taken relative to the canonical project root
/// `canonical_root`, that must stay inside the canonical directory
/// `canonical_base`. The base is the root or a directory above it.
///
/// The check walks `path` one component at a time, in the same order as
/// the operating system. A link resolves before the next component, so a
/// `..` after a link goes to the parent of the link target. The walk stops
/// at the first missing entry, and the rest of the path is taken by text,
/// because no link exists below a missing entry.
pub fn project_entry_within(
    canonical_base: &Path,
    canonical_root: &Path,
    path: &Path,
) -> ProjectEntry {
    let joined = canonical_root.join(path);
    let Ok(relative) = joined.strip_prefix(canonical_base) else {
        return ProjectEntry::Outside;
    };
    let mut current = canonical_base.to_path_buf();
    let mut components = relative.components();
    while let Some(component) = components.next() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                current.pop();
                if !current.starts_with(canonical_base) {
                    return ProjectEntry::Outside;
                }
            }
            Component::Normal(name) => {
                let next = current.join(name);
                match next.symlink_metadata() {
                    Ok(meta) if meta.file_type().is_symlink() => match dunce::canonicalize(&next) {
                        Ok(target) if target.starts_with(canonical_base) => current = target,
                        _ => return ProjectEntry::Outside,
                    },
                    Ok(_) => current = next,
                    Err(_) => return missing_rest(canonical_base, next, components),
                }
            }
            Component::RootDir | Component::Prefix(_) => return ProjectEntry::Outside,
        }
    }
    ProjectEntry::Inside
}

/// The state of a path whose walk reached the missing entry `missing`, with
/// `rest` still to walk. The rest is taken by text.
fn missing_rest(
    canonical_base: &Path,
    missing: PathBuf,
    rest: std::path::Components<'_>,
) -> ProjectEntry {
    let mut current = missing;
    for component in rest {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                current.pop();
                if !current.starts_with(canonical_base) {
                    return ProjectEntry::Outside;
                }
            }
            Component::Normal(name) => current.push(name),
            Component::RootDir | Component::Prefix(_) => return ProjectEntry::Outside,
        }
    }
    if current.symlink_metadata().is_ok() {
        // A `..` led back to an entry that exists, so walk it again on disk.
        return project_entry_within(canonical_base, canonical_base, &current);
    }
    ProjectEntry::Missing
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

    #[cfg(unix)]
    #[test]
    #[cfg_attr(miri, ignore = "tempdir is blocked by Miri isolation")]
    fn a_dot_dot_after_a_link_goes_to_the_parent_of_the_link_target() {
        let dir = tempfile::tempdir().unwrap();
        let outer = dunce::canonicalize(dir.path()).unwrap();
        let root = outer.join("project");
        std::fs::create_dir_all(root.join("sub")).unwrap();
        std::fs::create_dir_all(outer.join("elsewhere/deep")).unwrap();
        std::fs::write(outer.join("elsewhere/a.json"), "{}").unwrap();
        std::fs::write(root.join("a.json"), "{}").unwrap();
        std::os::unix::fs::symlink(outer.join("elsewhere/deep"), root.join("away")).unwrap();
        std::os::unix::fs::symlink(&root, root.join("sub/home")).unwrap();
        for path in [
            "away/../a.json",
            "away/../missing.json",
            "sub/home/../../a.json",
            "sub/home/../missing/../../a.json",
        ] {
            assert_eq!(
                project_entry(&root, Path::new(path)),
                ProjectEntry::Outside,
                "{path}"
            );
        }
        assert_eq!(
            project_entry(&root, Path::new("sub/home/a.json")),
            ProjectEntry::Inside
        );
        assert_eq!(
            project_entry(&root, Path::new("sub/home/../project/a.json")),
            ProjectEntry::Outside
        );
        assert_eq!(
            project_entry(&root, Path::new("missing/../a.json")),
            ProjectEntry::Inside
        );
        assert_eq!(
            project_entry_within(&outer, &root, Path::new("../elsewhere/a.json")),
            ProjectEntry::Inside
        );
        assert_eq!(
            project_entry_within(&root, &root, Path::new("../elsewhere/a.json")),
            ProjectEntry::Outside
        );
    }
}
