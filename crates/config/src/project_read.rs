//! The state of a project file that a config key names, decided without a
//! read of the file.

use std::path::{Component, Path, PathBuf};
use std::sync::{Mutex, OnceLock};

use rustc_hash::{FxHashMap, FxHashSet};

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
/// - In a git checkout, it is the git top level of the root.
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
    let cache = SCOPE_BASES.get_or_init(Mutex::default);
    if root.is_absolute()
        && let Ok(bases) = cache.lock()
        && let Some(base) = bases.given_bases.get(root)
    {
        return base.clone();
    }
    let Ok(resolved) = dunce::canonicalize(root) else {
        return root.to_path_buf();
    };
    let (generation, known) = match cache.lock() {
        Ok(bases) => (bases.generation, bases.bases.get(&resolved).cloned()),
        Err(_) => return uncached_scope_base(&resolved),
    };
    let base = known.unwrap_or_else(|| uncached_scope_base(&resolved));
    // A clear during the search makes the result stale, so it is not kept.
    if let Ok(mut bases) = cache.lock()
        && bases.generation == generation
    {
        if root.is_absolute() {
            bases.given_bases.insert(root.to_path_buf(), base.clone());
        }
        bases.bases.insert(resolved, base.clone());
    }
    base
}

/// The scope bases that [`project_scope_base`] found, by resolved root, and
/// the count of clears so far.
#[derive(Default)]
struct ScopeBases {
    bases: FxHashMap<PathBuf, PathBuf>,
    /// The scope base of each absolute root, by the root as the caller gave
    /// it, so a repeated call does not resolve the root again.
    given_bases: FxHashMap<PathBuf, PathBuf>,
    /// The probe scope of each absolute root, by the root as the caller
    /// gave it.
    probes: FxHashMap<PathBuf, Option<ProbeScope>>,
    /// Directories that resolved inside a scope base, by base.
    inside_dirs: FxHashMap<PathBuf, FxHashSet<PathBuf>>,
    generation: u64,
}

/// The resolved paths that [`may_probe_project_path`] checks a probe
/// against, for one root.
#[derive(Clone)]
struct ProbeScope {
    canonical_root: PathBuf,
    canonical_base: PathBuf,
    base_as_given: PathBuf,
}

static SCOPE_BASES: OnceLock<Mutex<ScopeBases>> = OnceLock::new();

/// The count of [`clear_project_scope_bases`] calls, for caches outside this
/// module that must not outlive a clear.
static SCOPE_GENERATION: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);

/// The count of [`clear_project_scope_bases`] calls so far. A cache of scope
/// results is valid only while this value stays the same.
#[must_use]
pub fn scope_generation() -> u64 {
    SCOPE_GENERATION.load(std::sync::atomic::Ordering::Acquire)
}

/// Forget the scope bases that [`project_scope_base`] found. Workspace
/// discovery calls it at the start of each analysis, so a long-lived
/// process (LSP, watch) sees a new `.git` entry or workspace manifest. A
/// search that runs during the clear does not keep its result.
pub fn clear_project_scope_bases() {
    SCOPE_GENERATION.fetch_add(1, std::sync::atomic::Ordering::AcqRel);
    if let Some(Ok(mut bases)) = SCOPE_BASES.get().map(Mutex::lock) {
        bases.bases.clear();
        bases.given_bases.clear();
        bases.probes.clear();
        bases.inside_dirs.clear();
        bases.generation = bases.generation.wrapping_add(1);
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
/// checkout and not to the project, so it gives `None`. A top level above
/// the root that all users can write, or that neither the current user nor
/// the owner of the root owns, gives `None` too (see
/// [`fallow_types::read_scope`]).
pub fn project_checkout_top(root: &Path) -> Option<&Path> {
    let top = fallow_types::audit_snapshot::lookup_ancestors(root)
        .take_while(|dir| test_ceiling::allows(dir))
        .find(|dir| dir.join(".git").symlink_metadata().is_ok())?;
    (top == root
        || (!is_home_or_above(top) && fallow_types::read_scope::may_widen_scope(top, root)))
    .then_some(top)
}

/// The nearest workspace root above the canonical directory `root` that
/// lists the root as a member, below the home directory and the file
/// system root. A workspace root that all users can write, or that neither
/// the current user nor the owner of the root owns, does not count.
fn workspace_root_above(root: &Path) -> Option<PathBuf> {
    fallow_types::audit_snapshot::lookup_ancestors(root)
        .skip(1)
        .take_while(|dir| !is_home_or_above(dir) && test_ceiling::allows(dir))
        .find(|dir| {
            fallow_types::read_scope::may_widen_scope(dir, root) && workspace_root_lists(dir, root)
        })
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
    let homes = ["HOME", "USERPROFILE"]
        .iter()
        .filter_map(std::env::var_os)
        .filter(|home| !home.is_empty())
        .filter_map(|home| dunce::canonicalize(home).ok());
    is_home_or_above_in(dir, homes)
}

/// [`is_home_or_above`] for the resolved home directories `homes`.
fn is_home_or_above_in(dir: &Path, mut homes: impl Iterator<Item = PathBuf>) -> bool {
    dir.parent().is_none() || homes.any(|home| home.starts_with(dir))
}

/// A test sets a ceiling for its own thread, so that the ancestor searches
/// of the scope base do not see a git checkout or a workspace above the
/// temp directory of the test.
mod test_ceiling {
    use std::path::Path;

    #[cfg(test)]
    thread_local! {
        static CEILING: std::cell::RefCell<Option<std::path::PathBuf>> =
            const { std::cell::RefCell::new(None) };
    }

    /// Search only `dir` and the directories below it on this thread.
    #[cfg(test)]
    pub fn set(dir: &Path) {
        CEILING.with(|ceiling| *ceiling.borrow_mut() = Some(dir.to_path_buf()));
    }

    #[cfg(test)]
    pub(super) fn allows(dir: &Path) -> bool {
        CEILING.with(|ceiling| {
            ceiling
                .borrow()
                .as_ref()
                .is_none_or(|top| dir.starts_with(top))
        })
    }

    #[cfg(not(test))]
    pub(super) const fn allows(_dir: &Path) -> bool {
        true
    }
}

#[cfg(test)]
pub use test_ceiling::set as set_test_ceiling;

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
    // The canonical root holds no link, so a relative path that starts
    // inside it walks from the root and not from the base.
    let joined;
    let (mut current, mut components) =
        if path.is_relative() && canonical_root.starts_with(canonical_base) {
            (canonical_root.to_path_buf(), path.components())
        } else {
            joined = canonical_root.join(path);
            let Ok(relative) = joined.strip_prefix(canonical_base) else {
                return ProjectEntry::Outside;
            };
            (canonical_base.to_path_buf(), relative.components())
        };
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

/// [`project_entry_within`] for a `path` that can also be absolute.
///
/// An absolute path must pass [`absolute_path_text_inside`] against the
/// base, in its canonical form or in `base_as_given`, before any file
/// system call. Then its longest part that exists resolves, and the walk
/// checks the rest, so the path does not need the canonical spelling.
pub fn project_path_entry(
    canonical_base: &Path,
    base_as_given: &Path,
    canonical_root: &Path,
    path: &Path,
) -> ProjectEntry {
    if !path.is_absolute() {
        return project_entry_within(canonical_base, canonical_root, path);
    }
    if !absolute_path_text_inside(path, canonical_base, base_as_given) {
        return ProjectEntry::Outside;
    }
    let Some((existing, resolved)) = path.ancestors().find_map(|dir| {
        dunce::canonicalize(dir)
            .ok()
            .map(|resolved| (dir, resolved))
    }) else {
        return ProjectEntry::Outside;
    };
    let Ok(rest) = path.strip_prefix(existing) else {
        return ProjectEntry::Outside;
    };
    project_entry_within(canonical_base, canonical_base, &resolved.join(rest))
}

/// Whether the absolute `path` from a config can name an entry below
/// `canonical_base` or below `base_as_given`, by its text alone. Call it
/// before any file system call on such a path.
///
/// A path in UNC or device form (`\\host\share`, `//host/share`,
/// `\\?\` and `\\.\`) is refused, so that a check never reaches a
/// network share. Else the path, with each `..` applied by text, must
/// start with one of the two spellings of the base.
#[must_use]
pub fn absolute_path_text_inside(path: &Path, canonical_base: &Path, base_as_given: &Path) -> bool {
    if is_share_or_device_text(path) {
        return false;
    }
    let normalized = fallow_types::path_guard::lexically_normalized(path);
    let given = std::path::absolute(base_as_given)
        .map(|base| fallow_types::path_guard::lexically_normalized(&base));
    normalized.starts_with(canonical_base) || given.is_ok_and(|given| normalized.starts_with(given))
}

/// Whether the directory `dir` resolves inside the scope base `base`, with
/// `check` for a directory that this run has not checked yet. A directory
/// that passes is kept until the next [`clear_project_scope_bases`], so the
/// probes of many candidate files in one directory check it once.
pub fn dir_inside_scope_base(base: &Path, dir: &Path, check: impl FnOnce() -> bool) -> bool {
    let cache = SCOPE_BASES.get_or_init(Mutex::default);
    let generation = match cache.lock() {
        Ok(bases) => {
            if bases
                .inside_dirs
                .get(base)
                .is_some_and(|dirs| dirs.contains(dir))
            {
                return true;
            }
            bases.generation
        }
        Err(_) => return check(),
    };
    let inside = check();
    if inside
        && let Ok(mut bases) = cache.lock()
        && bases.generation == generation
    {
        bases
            .inside_dirs
            .entry(base.to_path_buf())
            .or_default()
            .insert(dir.to_path_buf());
    }
    inside
}

/// Whether a probe of `path` stays inside the read scope of the project
/// root `root` (see [`project_scope_base`]). A relative `path` is taken
/// relative to the root.
///
/// The check runs before any probe of `path`. An absolute `path` must name
/// an entry below the scope base by its text. Then no part of `path` that
/// exists may resolve outside the scope base. So a probe of `path`, or of a
/// missing file below it, never follows a link out of the scope. A path
/// that fails the check gets the same answer whether its target exists or
/// not.
#[must_use]
pub fn may_probe_project_path(root: &Path, path: &Path) -> bool {
    let Some(scope) = probe_scope(root) else {
        return false;
    };
    project_path_entry(
        &scope.canonical_base,
        &scope.base_as_given,
        &scope.canonical_root,
        path,
    ) != ProjectEntry::Outside
}

/// The probe scope of `root`, resolved once per root until the next
/// [`clear_project_scope_bases`]. Entry discovery checks many probes of one
/// package root, so each check must not resolve the root and its base
/// again. `None` when the root does not resolve.
fn probe_scope(root: &Path) -> Option<ProbeScope> {
    // A relative root depends on the current directory, so it is not kept.
    if !root.is_absolute() {
        return uncached_probe_scope(root);
    }
    let cache = SCOPE_BASES.get_or_init(Mutex::default);
    let generation = match cache.lock() {
        Ok(bases) => {
            if let Some(scope) = bases.probes.get(root) {
                return scope.clone();
            }
            bases.generation
        }
        Err(_) => return uncached_probe_scope(root),
    };
    let scope = uncached_probe_scope(root);
    if let Ok(mut bases) = cache.lock()
        && bases.generation == generation
    {
        bases.probes.insert(root.to_path_buf(), scope.clone());
    }
    scope
}

fn uncached_probe_scope(root: &Path) -> Option<ProbeScope> {
    let canonical_root = dunce::canonicalize(root).ok()?;
    let canonical_base = project_scope_base(root);
    let base_as_given = base_as_given(root, &canonical_root, &canonical_base);
    Some(ProbeScope {
        canonical_root,
        canonical_base,
        base_as_given,
    })
}

/// Whether `path` starts with two separators, or with a Windows prefix
/// other than a plain drive letter.
fn is_share_or_device_text(path: &Path) -> bool {
    let bytes = path.as_os_str().as_encoded_bytes();
    let separator = |byte: u8| byte == b'/' || byte == b'\\';
    if bytes.len() >= 2 && separator(bytes[0]) && separator(bytes[1]) {
        return true;
    }
    matches!(
        path.components().next(),
        Some(Component::Prefix(prefix)) if !matches!(prefix.kind(), std::path::Prefix::Disk(_))
    )
}

/// The spelling of `canonical_base` as an ancestor of `root_as_given`,
/// the root as the user named it. The base is the canonical root or an
/// ancestor of it, so the same number of components goes up from the
/// given root. When the base is not an ancestor, it is its own spelling.
#[must_use]
pub fn base_as_given(
    root_as_given: &Path,
    canonical_root: &Path,
    canonical_base: &Path,
) -> PathBuf {
    let Ok(below) = canonical_root.strip_prefix(canonical_base) else {
        return canonical_base.to_path_buf();
    };
    let mut given = std::path::absolute(root_as_given).map_or_else(
        |_| root_as_given.to_path_buf(),
        |root| fallow_types::path_guard::lexically_normalized(&root),
    );
    for _ in below.components() {
        given.pop();
    }
    given
}

/// The state of a path whose walk reached the missing entry `missing`, with
/// `rest` still to walk. The rest is taken by text until a `..` leads back
/// to an entry that exists; from there the walk is on disk again.
fn missing_rest(
    canonical_base: &Path,
    missing: PathBuf,
    mut rest: std::path::Components<'_>,
) -> ProjectEntry {
    let mut current = missing;
    while let Some(component) = rest.next() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                current.pop();
                if !current.starts_with(canonical_base) {
                    return ProjectEntry::Outside;
                }
                if current.symlink_metadata().is_ok() {
                    // A `..` led back to an entry that exists, so the rest
                    // can pass a link again: walk it on disk from here.
                    return project_entry_within(
                        canonical_base,
                        canonical_base,
                        &current.join(rest.as_path()),
                    );
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
    fn a_directory_inside_the_scope_base_is_checked_once() {
        let base = Path::new("/scope-cache-test/base");
        let dir = base.join("packages/a");
        let checks = std::cell::Cell::new(0);
        let check = || {
            checks.set(checks.get() + 1);
            true
        };
        assert!(dir_inside_scope_base(base, &dir, check));
        assert!(dir_inside_scope_base(base, &dir, check));
        assert_eq!(checks.get(), 1);

        let outside = base.join("packages/out");
        let refuse = || {
            checks.set(checks.get() + 1);
            false
        };
        assert!(!dir_inside_scope_base(base, &outside, refuse));
        assert!(!dir_inside_scope_base(base, &outside, refuse));
        assert_eq!(checks.get(), 3, "a refused directory is checked again");
    }

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
        set_test_ceiling(&top);
        let package = top.join("packages/a");
        std::fs::create_dir_all(&package).unwrap();
        assert_eq!(project_scope_base(&package), package);
        std::fs::create_dir(top.join(".git")).unwrap();
        // An analysis clears the kept bases at its start.
        clear_project_scope_bases();
        assert_eq!(project_scope_base(&package), top);
        assert_eq!(project_scope_base(&top), top);
    }

    /// A git top level or a workspace root above the root that all users can
    /// write does not widen the scope.
    #[test]
    #[cfg(unix)]
    #[cfg_attr(miri, ignore = "tempdir is blocked by Miri isolation")]
    fn a_shared_writable_directory_above_the_root_is_not_the_base() {
        use std::os::unix::fs::PermissionsExt;
        let (_dir, top, member) =
            workspace_without_git(("pnpm-workspace.yaml", "packages:\n  - 'packages/*'\n"));
        std::fs::create_dir(top.join(".git")).unwrap();
        clear_project_scope_bases();
        assert_eq!(project_scope_base(&member), top);
        std::fs::set_permissions(&top, std::fs::Permissions::from_mode(0o777)).unwrap();
        clear_project_scope_bases();
        assert_eq!(project_scope_base(&member), member);
        std::fs::remove_dir(top.join(".git")).unwrap();
        clear_project_scope_bases();
        assert_eq!(project_scope_base(&member), member, "workspace root");
        std::fs::set_permissions(&top, std::fs::Permissions::from_mode(0o755)).unwrap();
    }

    /// A workspace below a temp dir without a `.git` entry, with the member
    /// `packages/a`. `manifest` is the name and text of the root file that
    /// lists the members.
    fn workspace_without_git(manifest: (&str, &str)) -> (tempfile::TempDir, PathBuf, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let outer = dunce::canonicalize(dir.path()).unwrap();
        set_test_ceiling(&outer);
        let top = outer.join("ws");
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

    #[test]
    fn without_home_variables_only_the_file_system_root_counts_as_home() {
        assert!(is_home_or_above_in(Path::new("/"), std::iter::empty()));
        assert!(!is_home_or_above_in(Path::new("/srv"), std::iter::empty()));
        assert!(!is_home_or_above_in(
            Path::new("/srv/project"),
            std::iter::empty()
        ));
        let homes = || std::iter::once(PathBuf::from("/srv/user"));
        assert!(is_home_or_above_in(Path::new("/srv"), homes()));
        assert!(is_home_or_above_in(Path::new("/srv/user"), homes()));
        assert!(!is_home_or_above_in(Path::new("/srv/user/code"), homes()));
    }

    #[test]
    fn share_and_device_paths_are_refused_by_text() {
        let base = &abs("/base");
        for path in [
            "//host/share/x",
            r"\\host\share\x",
            r"\\?\C:\base\x",
            r"\\?\UNC\host\share\x",
            r"\\.\pipe\x",
            "//./pipe/x",
        ] {
            assert!(
                !absolute_path_text_inside(Path::new(path), base, base),
                "{path}"
            );
        }
        assert!(absolute_path_text_inside(&abs("/base/a/../b"), base, base));
        assert!(!absolute_path_text_inside(
            &abs("/base/../other"),
            base,
            base
        ));
        assert!(absolute_path_text_inside(
            &abs("/given/a"),
            base,
            &abs("/given")
        ));
    }

    #[cfg(windows)]
    #[test]
    fn windows_share_and_device_prefixes_are_refused() {
        let base = Path::new(r"C:\base");
        for path in [
            r"\\host\share\x",
            r"\\?\UNC\host\share\x",
            r"\\?\C:\base\x",
            r"\\.\COM1",
        ] {
            assert!(
                !absolute_path_text_inside(Path::new(path), base, base),
                "{path}"
            );
            assert_eq!(
                project_path_entry(base, base, base, Path::new(path)),
                ProjectEntry::Outside,
                "{path}"
            );
        }
        assert!(absolute_path_text_inside(
            Path::new(r"C:\base\x"),
            base,
            base
        ));
    }

    #[test]
    fn the_base_keeps_the_spelling_of_the_given_root() {
        assert_eq!(
            base_as_given(
                &abs("/link/repo/packages/a"),
                &abs("/real/repo/packages/a"),
                &abs("/real/repo"),
            ),
            abs("/link/repo")
        );
        assert_eq!(
            base_as_given(&abs("/x"), &abs("/real/x"), &abs("/other")),
            abs("/other")
        );
    }

    /// The absolute path of the Unix spelling `path`: the same path on Unix,
    /// and the path on drive `C:` with backslash separators on Windows.
    fn abs(path: &str) -> PathBuf {
        if cfg!(windows) {
            PathBuf::from(format!("C:{}", path.replace('/', "\\")))
        } else {
            PathBuf::from(path)
        }
    }

    #[cfg(unix)]
    #[test]
    fn a_share_path_key_is_outside_without_a_file_system_call() {
        let base = Path::new("/nonexistent-base");
        assert_eq!(
            project_path_entry(base, base, base, Path::new("//host/share/CODEOWNERS")),
            ProjectEntry::Outside
        );
    }

    #[cfg(unix)]
    #[test]
    #[cfg_attr(miri, ignore = "tempdir is blocked by Miri isolation")]
    fn a_link_after_a_dot_dot_out_of_a_missing_dir_is_walked_on_disk() {
        let dir = tempfile::tempdir().unwrap();
        let outer = dunce::canonicalize(dir.path()).unwrap();
        let root = outer.join("project");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(outer.join("elsewhere")).unwrap();
        std::os::unix::fs::symlink(outer.join("elsewhere"), root.join("away")).unwrap();
        for path in ["missing/../away/new.json", "a/b/../../away/new.json"] {
            assert_eq!(
                project_entry(&root, Path::new(path)),
                ProjectEntry::Outside,
                "{path}"
            );
        }
        assert_eq!(
            project_entry(&root, Path::new("missing/../new.json")),
            ProjectEntry::Missing
        );
    }
}
