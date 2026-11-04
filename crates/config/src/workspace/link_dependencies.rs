//! Workspaces that `link:` and `file:` dependency specs declare.

use std::path::{Path, PathBuf};

use super::diagnostics::{is_ignored_workspace_dir, is_skip_listed_dir};
use super::{
    ManifestCache, PackageJson, WorkspaceDiagnostic, WorkspaceInfo,
    load_member_package_manifest_cached, register_matched_workspace,
};

/// Discover workspaces from `link:` and `file:` dependency specs in the root
/// `package.json` and in the manifests of the `member_dirs` workspaces.
///
/// A workspace package can keep a local package in a subfolder and depend on
/// it with `file:./modules/pkg`. pnpm installs a `file:` folder as a copy in
/// its store, so the import does not reach the source through a symlink.
/// The local package is then a workspace with its own manifest. An import in
/// the local package of a dependency that only the parent manifest lists is an
/// unlisted dependency, and the parent entry is an unused dependency. npm and
/// yarn symlink the folder, so Node can resolve such an import from the parent
/// `node_modules`, but pnpm installs only the dependencies of the local
/// manifest. Root `link:` and `file:` workspaces follow the same model.
///
/// Older yarn monorepos declare no
/// `workspaces` field. The root lists each local package as
/// `"@scope/pkg": "link:path/to/pkg"`, often three to five directories deep,
/// out of reach of the shallow fallback scan. A target is a workspace only when
/// it is a directory inside the project root that holds a package manifest.
/// Targets outside the root, missing targets and tarballs are skipped without a
/// diagnostic, because a dependency spec is not a workspace declaration. A
/// target that source discovery does not walk is also skipped (see
/// [`is_skipped_link_target`] and [`crate::source_walk::dirs_reached_by_source_walk`]). A
/// target with a malformed `package.json` gets the same diagnostic as a
/// declared workspace.
pub(super) fn collect_link_dependency_workspaces(
    root: &Path,
    canonical_root: &Path,
    member_dirs: &[PathBuf],
    ignore_patterns: &crate::IgnorePatternSet,
    diagnostics: &mut Vec<WorkspaceDiagnostic>,
    manifest_cache: &mut ManifestCache,
) -> Vec<(WorkspaceInfo, Vec<String>)> {
    let mut link_targets: Vec<PathBuf> = Vec::new();
    if let Ok(root_pkg) = PackageJson::load(&root.join("package.json")) {
        link_targets.extend(
            root_pkg
                .local_link_dependencies()
                .into_iter()
                .map(|(_name, target)| root.join(target)),
        );
    }
    for dir in member_dirs {
        if let Ok(Some((_name, pkg, _deps))) =
            load_member_package_manifest_cached(dir, manifest_cache)
        {
            link_targets.extend(
                pkg.local_link_dependencies()
                    .into_iter()
                    .map(|(_name, target)| dir.join(target)),
            );
        }
    }

    let mut candidates: Vec<PathBuf> = link_targets
        .into_iter()
        .filter_map(|target| dunce::canonicalize(target).ok())
        .filter(|canonical_dir| {
            *canonical_dir != *canonical_root
                && canonical_dir.starts_with(canonical_root)
                && canonical_dir.is_dir()
                && !is_skipped_link_target(canonical_root, canonical_dir, ignore_patterns)
        })
        .collect();
    candidates.sort_unstable();
    candidates.dedup();
    let walked = crate::source_walk::dirs_reached_by_source_walk(canonical_root, &candidates);

    let mut workspaces = Vec::new();
    for canonical_dir in candidates {
        if !walked.contains(&canonical_dir) {
            continue;
        }
        let dir = normalize_link_target(root, canonical_root, &canonical_dir);
        register_matched_workspace(root, dir, &mut workspaces, diagnostics, manifest_cache);
    }
    workspaces
}

/// Whether a link target is under a path that source discovery skips by name.
///
/// A yalc copy (`file:.yalc/pkg`), a package under `node_modules`, a build
/// output or a path that `ignorePatterns` matches has no discovered source
/// files. A workspace there makes each import of the package an unresolved
/// import, so such a target stays an external package. The skip list is the
/// one the workspace glob expansion uses.
fn is_skipped_link_target(
    canonical_root: &Path,
    canonical_dir: &Path,
    ignore_patterns: &crate::IgnorePatternSet,
) -> bool {
    let Ok(relative) = canonical_dir.strip_prefix(canonical_root) else {
        return true;
    };
    relative
        .components()
        .any(|component| is_skip_listed_dir(&component.as_os_str().to_string_lossy()))
        || is_ignored_workspace_dir(relative, ignore_patterns)
}

/// Express a canonical link target under the caller's `root` spelling.
///
/// A spec such as `link:./pkg/../pkg` or a symlinked root would otherwise give
/// a workspace root that does not prefix-match the discovered source paths.
fn normalize_link_target(root: &Path, canonical_root: &Path, canonical_dir: &Path) -> PathBuf {
    canonical_dir
        .strip_prefix(canonical_root)
        .map_or_else(|_| canonical_dir.to_path_buf(), |rel| root.join(rel))
}
