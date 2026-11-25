//! The state of a project file that a config key names, decided without a
//! read of the file.

use std::path::{Component, Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use rustc_hash::FxHashMap;

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

/// The directory that project files of the analysis root `root` may
/// resolve in.
///
/// - In a git checkout, it is the git top level of the root, see
///   [`project_checkout_top`].
/// - Else it is the nearest workspace root above the root that lists the
///   root as a member: a directory with a `pnpm-workspace.yaml`, or with a
///   `package.json` that has `workspaces`. The search stops below the home
///   directory and below the file system root.
/// - Else it is the root itself.
///
/// A root that does not resolve is its own base.
///
/// The result for each resolved root is kept until the next
/// [`clear_project_scope_bases`], because a run without git asks for the
/// base of each installed manifest and the search reads files above the
/// root.
#[must_use]
pub fn project_scope_base(root: &Path) -> PathBuf {
    let Ok(root) = dunce::canonicalize(root) else {
        return root.to_path_buf();
    };
    let cache = SCOPE_BASES.get_or_init(Mutex::default);
    if let Some(base) = cache
        .lock()
        .ok()
        .and_then(|bases| bases.get(&root).cloned())
    {
        return base;
    }
    let base = uncached_scope_base(&root);
    if let Ok(mut bases) = cache.lock() {
        bases.insert(root, base.clone());
    }
    base
}

/// The scope bases that [`project_scope_base`] found, by resolved root.
static SCOPE_BASES: OnceLock<Mutex<FxHashMap<PathBuf, PathBuf>>> = OnceLock::new();

/// Forget the scope bases that [`project_scope_base`] found. Workspace
/// discovery calls it at the start of each analysis, so a long-lived
/// process (LSP, watch) sees a new `.git` entry or workspace manifest.
pub fn clear_project_scope_bases() {
    if let Some(Ok(mut bases)) = SCOPE_BASES.get().map(Mutex::lock) {
        bases.clear();
    }
}

fn uncached_scope_base(root: &Path) -> PathBuf {
    if let Some(top) = project_checkout_top(root) {
        return top.to_path_buf();
    }
    workspace_root_above(root).unwrap_or_else(|| root.to_path_buf())
}

/// The git top level of the canonical directory `root` that counts as the
/// checkout of the project: the nearest directory at or above the root with
/// a `.git` entry. A top level above the root that is the home directory,
/// an ancestor of it, or the file system root belongs to a dotfiles
/// checkout and not to the project, so it gives `None`.
pub fn project_checkout_top(root: &Path) -> Option<&Path> {
    let top = root
        .ancestors()
        .find(|dir| dir.join(".git").symlink_metadata().is_ok())?;
    (top == root || !is_home_or_above(top)).then_some(top)
}

/// The nearest workspace root above the canonical directory `root` that
/// lists the root as a member, below the home directory and the file
/// system root.
fn workspace_root_above(root: &Path) -> Option<PathBuf> {
    root.ancestors()
        .skip(1)
        .take_while(|dir| !is_home_or_above(dir))
        .find(|dir| workspace_root_lists(dir, root))
        .map(Path::to_path_buf)
}

/// Whether the `pnpm-workspace.yaml` or the `package.json` `workspaces` of
/// `workspace_root` lists `member`, a directory below it. The reads are
/// bounded and must stay inside `workspace_root`.
fn workspace_root_lists(workspace_root: &Path, member: &Path) -> bool {
    use fallow_types::read_guard::{DEFAULT_MAX_READ_BYTES, read_to_string_inside};
    let mut patterns = read_to_string_inside(
        workspace_root,
        Path::new("pnpm-workspace.yaml"),
        DEFAULT_MAX_READ_BYTES,
    )
    .map(|content| crate::pnpm_workspace_packages(&content))
    .unwrap_or_default();
    let manifest = workspace_root.join("package.json");
    if manifest.symlink_metadata().is_ok()
        && let Ok(pkg) = crate::PackageJson::load_inside(workspace_root, &manifest)
    {
        patterns.extend(pkg.workspace_patterns());
    }
    !patterns.is_empty()
        && crate::ManifestPatternMatcher::from_patterns(patterns.iter().map(String::as_str))
            .declares(workspace_root, member)
}

/// Whether `dir` is the file system root, the home directory, or an
/// ancestor of the home directory. Without a `HOME` or `USERPROFILE`
/// variable, only the file system root counts.
pub fn is_home_or_above(dir: &Path) -> bool {
    dir.parent().is_none()
        || ["HOME", "USERPROFILE"]
            .iter()
            .filter_map(std::env::var_os)
            .filter(|home| !home.is_empty())
            .filter_map(|home| dunce::canonicalize(home).ok())
            .any(|home| home.starts_with(dir))
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

    #[test]
    #[cfg_attr(miri, ignore = "tempdir is blocked by Miri isolation")]
    fn the_scope_base_is_the_git_top_level_or_the_root() {
        let dir = tempfile::tempdir().unwrap();
        let top = dunce::canonicalize(dir.path()).unwrap();
        let package = top.join("packages/a");
        std::fs::create_dir_all(&package).unwrap();
        assert_eq!(project_scope_base(&package), package);
        std::fs::create_dir(top.join(".git")).unwrap();
        // An analysis clears the kept bases at its start.
        clear_project_scope_bases();
        assert_eq!(project_scope_base(&package), top);
        assert_eq!(project_scope_base(&top), top);
    }

    /// A workspace below a temp dir without a `.git` entry, with the member
    /// `packages/a`. `manifest` is the name and text of the root file that
    /// lists the members.
    fn workspace_without_git(manifest: (&str, &str)) -> (tempfile::TempDir, PathBuf, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let top = dunce::canonicalize(dir.path()).unwrap().join("ws");
        let member = top.join("packages/a");
        std::fs::create_dir_all(&member).unwrap();
        std::fs::write(top.join(manifest.0), manifest.1).unwrap();
        (dir, top, member)
    }

    #[test]
    #[cfg_attr(miri, ignore = "tempdir is blocked by Miri isolation")]
    fn without_git_the_scope_base_is_the_workspace_root_that_lists_the_root() {
        for manifest in [
            ("pnpm-workspace.yaml", "packages:\n  - 'packages/*'\n"),
            ("package.json", r#"{"workspaces": ["packages/*"]}"#),
            (
                "package.json",
                r#"{"workspaces": {"packages": ["./packages/*"]}}"#,
            ),
        ] {
            let (_dir, top, member) = workspace_without_git(manifest);
            assert_eq!(project_scope_base(&member), top, "{manifest:?}");
            assert_eq!(project_scope_base(&top), top, "{manifest:?}");
        }
    }

    #[test]
    #[cfg_attr(miri, ignore = "tempdir is blocked by Miri isolation")]
    fn without_git_a_workspace_root_that_does_not_list_the_root_is_not_the_base() {
        for manifest in [
            ("pnpm-workspace.yaml", "packages:\n  - 'apps/*'\n"),
            (
                "package.json",
                r#"{"workspaces": ["packages/*", "!packages/a"]}"#,
            ),
            ("package.json", r#"{"name": "no-workspaces"}"#),
        ] {
            let (_dir, _top, member) = workspace_without_git(manifest);
            assert_eq!(project_scope_base(&member), member, "{manifest:?}");
        }
    }

    #[test]
    fn the_file_system_root_is_never_a_checkout_top_level() {
        assert!(is_home_or_above(Path::new("/")));
    }
}
