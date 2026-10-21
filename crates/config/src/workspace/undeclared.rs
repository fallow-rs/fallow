//! The scan for package directories that no workspace pattern declares.

use std::path::{Path, PathBuf};

use super::{
    WorkspaceDiagnostic, WorkspaceDiagnosticKind, WorkspaceInfo, collect_declared_patterns,
    deno_json::manifest_declares_package, dir_has_package_manifest,
    nested_project::is_nested_project_dir,
};

/// Find directories containing `package.json` that are not declared as workspaces.
///
/// Only meaningful in monorepos that declare workspaces (via `package.json` `workspaces`
/// field or `pnpm-workspace.yaml`). Scans up to two directory levels deep, skipping
/// hidden directories, `node_modules`, and `build`.
#[must_use]
pub fn find_undeclared_workspaces(
    root: &Path,
    declared: &[WorkspaceInfo],
) -> Vec<WorkspaceDiagnostic> {
    find_undeclared_workspaces_with_ignores(root, declared, &crate::IgnorePatternSet::empty())
}

/// Find directories containing `package.json` that are not declared as workspaces,
/// excluding candidates covered by the supplied ignore globset.
///
/// This is the ignore-aware variant used by the full analyzer after config
/// resolution. See [`find_undeclared_workspaces`] for the compatibility wrapper.
///
/// Directories whose project-root-relative path matches `ignore_patterns` are skipped
/// so users who already excluded a path via `ignorePatterns` don't see a redundant
/// "not declared as workspace" warning. See issue #193.
#[must_use]
pub fn find_undeclared_workspaces_with_ignores(
    root: &Path,
    declared: &[WorkspaceInfo],
    ignore_patterns: &crate::IgnorePatternSet,
) -> Vec<WorkspaceDiagnostic> {
    let declares_workspaces = collect_declared_patterns(root, &[]).is_ok_and(|p| !p.is_empty());
    if !declares_workspaces {
        return Vec::new();
    }

    let declared_roots: rustc_hash::FxHashSet<PathBuf> = declared
        .iter()
        .map(|w| dunce::canonicalize(&w.root).unwrap_or_else(|_| w.root.clone()))
        .collect();

    let canonical_root = dunce::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());

    let mut undeclared = Vec::new();

    let Ok(top_entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };

    for entry in top_entries.filter_map(Result::ok) {
        let path = entry.path();
        if !path.is_dir() || is_undeclared_scan_skip_dir(&entry.file_name().to_string_lossy()) {
            continue;
        }

        let mut input = UndeclaredScanInput {
            root,
            canonical_root: &canonical_root,
            declared_roots: &declared_roots,
            ignore_patterns,
            undeclared: &mut undeclared,
        };
        check_undeclared(
            &path,
            input.root,
            input.canonical_root,
            input.declared_roots,
            input.ignore_patterns,
            input.undeclared,
        );
        // The children of an independent pnpm project belong to that project.
        if !is_nested_project_dir(&path) {
            scan_child_dirs_for_undeclared(&path, &mut input);
        }
    }

    undeclared
}

/// Whether an undeclared-workspace scan should skip a directory by leaf name.
pub(super) fn is_undeclared_scan_skip_dir(name: &str) -> bool {
    name.starts_with('.') || name == "node_modules" || name == "build"
}

/// Borrowed inputs threaded through the second-level undeclared-workspace scan.
struct UndeclaredScanInput<'a> {
    root: &'a Path,
    canonical_root: &'a Path,
    declared_roots: &'a rustc_hash::FxHashSet<PathBuf>,
    ignore_patterns: &'a crate::IgnorePatternSet,
    undeclared: &'a mut Vec<WorkspaceDiagnostic>,
}

/// Check each immediate child of `parent` for an undeclared workspace.
fn scan_child_dirs_for_undeclared(parent: &Path, input: &mut UndeclaredScanInput<'_>) {
    let Ok(child_entries) = std::fs::read_dir(parent) else {
        return;
    };
    for child in child_entries.filter_map(Result::ok) {
        let child_path = child.path();
        if !child_path.is_dir() || is_undeclared_scan_skip_dir(&child.file_name().to_string_lossy())
        {
            continue;
        }
        check_undeclared(
            &child_path,
            input.root,
            input.canonical_root,
            input.declared_roots,
            input.ignore_patterns,
            input.undeclared,
        );
    }
}

/// Check a single directory for an undeclared workspace.
fn check_undeclared(
    dir: &Path,
    root: &Path,
    canonical_root: &Path,
    declared_roots: &rustc_hash::FxHashSet<PathBuf>,
    ignore_patterns: &crate::IgnorePatternSet,
    undeclared: &mut Vec<WorkspaceDiagnostic>,
) {
    if !dir_has_package_manifest(dir) || !manifest_declares_package(dir) {
        return;
    }
    let canonical = dunce::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
    if canonical == *canonical_root {
        return;
    }
    if declared_roots.contains(&canonical) {
        return;
    }
    if is_ignored_scan_dir(root, dir, ignore_patterns) {
        return;
    }
    undeclared.push(WorkspaceDiagnostic::new(
        root,
        dir.to_path_buf(),
        WorkspaceDiagnosticKind::UndeclaredWorkspace,
    ));
}

/// Whether `ignore_patterns` excludes a directory, or its manifest, that the
/// undeclared-workspace scan or the nested-project scan visits.
pub(super) fn is_ignored_scan_dir(
    root: &Path,
    dir: &Path,
    ignore_patterns: &crate::IgnorePatternSet,
) -> bool {
    let relative = dir.strip_prefix(root).unwrap_or(dir);
    let relative_str = relative.to_string_lossy().replace('\\', "/");
    ignore_patterns.is_match(relative_str.as_str())
        || ignore_patterns.is_match(format!("{relative_str}/package.json").as_str())
        || ignore_patterns.is_match(format!("{relative_str}/deno.json").as_str())
        || ignore_patterns.is_match(format!("{relative_str}/deno.jsonc").as_str())
}
