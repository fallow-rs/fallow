use std::path::{Path, PathBuf};
use std::process::ExitCode;

use fallow_config::{OutputFormat, WorkspaceInfo, discover_workspaces};
use globset::Glob;
use rustc_hash::FxHashSet;

use crate::error::emit_error;

// ── Workspace filtering ──────────────────────────────────────────

/// Scope results to the union of the given workspace roots.
///
/// The full cross-workspace graph is still built (so cross-package imports
/// are resolved), but only issues from files under any `ws_root` are reported.
///
/// Any issue whose path starts with one of the roots passes; dependency-level
/// issues are scoped to the matching workspaces' own `package.json` files.
pub fn filter_to_workspaces(
    results: &mut fallow_core::results::AnalysisResults,
    ws_roots: &[PathBuf],
) {
    let any_under = |p: &Path| ws_roots.iter().any(|r| p.starts_with(r));
    let pkg_jsons: Vec<PathBuf> = ws_roots.iter().map(|r| r.join("package.json")).collect();
    let in_pkg_jsons = |p: &Path| pkg_jsons.iter().any(|pkg| p == pkg);

    // File-scoped issues: retain only those under any workspace root
    results.unused_files.retain(|f| any_under(&f.path));
    results.unused_exports.retain(|e| any_under(&e.path));
    results.unused_types.retain(|e| any_under(&e.path));
    results.private_type_leaks.retain(|e| any_under(&e.path));
    results.unused_enum_members.retain(|m| any_under(&m.path));
    results.unused_class_members.retain(|m| any_under(&m.path));
    results.unresolved_imports.retain(|i| any_under(&i.path));

    // Dependency issues: scope to matching workspaces' package.json files
    results
        .unused_dependencies
        .retain(|d| in_pkg_jsons(&d.path));
    results
        .unused_dev_dependencies
        .retain(|d| in_pkg_jsons(&d.path));
    results
        .unused_optional_dependencies
        .retain(|d| in_pkg_jsons(&d.path));
    results
        .type_only_dependencies
        .retain(|d| in_pkg_jsons(&d.path));
    results
        .test_only_dependencies
        .retain(|d| in_pkg_jsons(&d.path));

    // Unlisted deps: keep only if any importing file is in a matched workspace
    results
        .unlisted_dependencies
        .retain(|d| d.imported_from.iter().any(|s| any_under(&s.path)));

    // Duplicate exports: filter locations to workspace, drop groups with < 2
    for dup in &mut results.duplicate_exports {
        dup.locations.retain(|loc| any_under(&loc.path));
    }
    results.duplicate_exports.retain(|d| d.locations.len() >= 2);

    // Circular deps: keep cycles where at least one file is in a matched workspace
    results
        .circular_dependencies
        .retain(|c| c.files.iter().any(|f| any_under(f)));

    // Boundary violations: keep if the importing file is in a matched workspace
    results
        .boundary_violations
        .retain(|v| any_under(&v.from_path));

    // Stale suppressions: keep if the file is in a matched workspace
    results.stale_suppressions.retain(|s| any_under(&s.path));

    // Catalog entries live in the project-root pnpm-workspace.yaml, not per-workspace.
    // Workspace scoping is asking "show me findings for this subset of packages";
    // catalog hygiene is a whole-project concern, so drop it when --workspace narrows.
    results.unused_catalog_entries.clear();
}

/// Resolve `--workspace <patterns...>` to a set of workspace roots, or exit with
/// an error.
///
/// Patterns support three forms:
/// - Exact package name (`web`) or relative workspace path (`apps/web`)
/// - Glob (`apps/*`, `@scope/*`), matched against BOTH `ws.name` AND the path
///   relative to the repo root; either match counts
/// - Negation (`!apps/legacy`), excludes matching workspaces from the result
///
/// Combination rules (gitignore-style):
/// - Only positive patterns: include matches
/// - Only negative patterns: include all workspaces then remove matches
/// - Mixed: start from union of positive matches, then remove negative matches
///
/// Reserved prefixes for future pnpm-style graph selectors: `^`, `+`, `...`
/// (not yet implemented; reject or repurpose only after panel review).
pub fn resolve_workspace_filters(
    root: &Path,
    patterns: &[String],
    output: OutputFormat,
) -> Result<Vec<PathBuf>, ExitCode> {
    let workspaces = discover_workspaces(root);
    if workspaces.is_empty() {
        let joined = patterns
            .iter()
            .map(|p| format!("'{p}'"))
            .collect::<Vec<_>>()
            .join(", ");
        let msg = format!(
            "--workspace {joined} specified but no workspaces found. \
             Ensure root package.json has a \"workspaces\" field, pnpm-workspace.yaml exists, \
             or tsconfig.json has \"references\"."
        );
        return Err(emit_error(&msg, 2, output));
    }

    let rel_paths: Vec<String> = workspaces
        .iter()
        .map(|ws| relative_workspace_path(&ws.root, root))
        .collect();

    let (positive, negative) = split_patterns(patterns);

    let mut matched: FxHashSet<usize> = FxHashSet::default();
    let mut unmatched: Vec<String> = Vec::new();

    if positive.is_empty() {
        matched.extend(0..workspaces.len());
    } else {
        for pat in &positive {
            let hits = find_matches(pat, &workspaces, &rel_paths, output)?;
            if hits.is_empty() {
                unmatched.push(pat.to_string());
            }
            matched.extend(hits);
        }
    }

    if !unmatched.is_empty() {
        let quoted: Vec<String> = unmatched.iter().map(|p| format!("'{p}'")).collect();
        let available = format_available_workspaces(&workspaces);
        let msg = format!(
            "--workspace: no workspaces matched pattern{}: {}. Available: {available}",
            if unmatched.len() == 1 { "" } else { "s" },
            quoted.join(", "),
        );
        return Err(emit_error(&msg, 2, output));
    }

    for pat in &negative {
        let hits = find_matches(pat, &workspaces, &rel_paths, output)?;
        for idx in hits {
            matched.remove(&idx);
        }
    }

    if matched.is_empty() {
        let include_desc = if positive.is_empty() {
            "<all>".to_owned()
        } else {
            positive
                .iter()
                .map(|p| format!("'{p}'"))
                .collect::<Vec<_>>()
                .join(", ")
        };
        let exclude_desc = negative
            .iter()
            .map(|p| format!("'{p}'"))
            .collect::<Vec<_>>()
            .join(", ");
        let msg = format!(
            "--workspace: all workspaces were excluded by the filter. \
             Included: {include_desc}. Excluded: {exclude_desc}."
        );
        return Err(emit_error(&msg, 2, output));
    }

    let mut roots: Vec<PathBuf> = matched
        .into_iter()
        .map(|i| workspaces[i].root.clone())
        .collect();
    roots.sort();
    Ok(roots)
}

/// Format the workspace list for inclusion in error messages. Caps the
/// displayed names so large monorepos (30+ workspaces) don't produce an
/// unreadable wall of text.
fn format_available_workspaces(workspaces: &[WorkspaceInfo]) -> String {
    const MAX_SHOWN: usize = 10;
    let total = workspaces.len();
    if total <= MAX_SHOWN {
        return workspaces
            .iter()
            .map(|ws| ws.name.as_str())
            .collect::<Vec<_>>()
            .join(", ");
    }
    let shown: Vec<&str> = workspaces
        .iter()
        .take(MAX_SHOWN)
        .map(|ws| ws.name.as_str())
        .collect();
    format!(
        "{shown_list}, ... and {remaining} more ({total} total)",
        shown_list = shown.join(", "),
        remaining = total - MAX_SHOWN,
    )
}

/// Compute the workspace path relative to the repo root, normalized to forward
/// slashes so glob patterns written with `/` work on Windows.
fn relative_workspace_path(ws_root: &Path, root: &Path) -> String {
    ws_root
        .strip_prefix(root)
        .unwrap_or(ws_root)
        .to_string_lossy()
        .replace('\\', "/")
}

/// Split comma-separated patterns into (positive, negative). Whitespace-trimmed;
/// empty entries ignored; leading `!` marks negation.
fn split_patterns(patterns: &[String]) -> (Vec<&str>, Vec<&str>) {
    let mut positive = Vec::new();
    let mut negative = Vec::new();
    for raw in patterns {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some(neg) = trimmed.strip_prefix('!') {
            let neg = neg.trim();
            if !neg.is_empty() {
                negative.push(neg);
            }
        } else {
            positive.push(trimmed);
        }
    }
    (positive, negative)
}

/// Find workspace indices matching `pattern`. Exact-name and exact-relative-path
/// matches short-circuit before globbing, so literal names containing glob
/// metacharacters (e.g. `web-[staging]`) still work.
fn find_matches(
    pattern: &str,
    workspaces: &[WorkspaceInfo],
    rel_paths: &[String],
    output: OutputFormat,
) -> Result<Vec<usize>, ExitCode> {
    if let Some(idx) = workspaces.iter().position(|ws| ws.name == pattern) {
        return Ok(vec![idx]);
    }
    if let Some(idx) = rel_paths.iter().position(|p| p == pattern) {
        return Ok(vec![idx]);
    }

    let glob = Glob::new(pattern).map_err(|e| {
        let msg = format!("--workspace: invalid pattern '{pattern}': {e}");
        emit_error(&msg, 2, output)
    })?;
    let matcher = glob.compile_matcher();

    let mut hits = Vec::new();
    for (idx, ws) in workspaces.iter().enumerate() {
        if matcher.is_match(&ws.name) || matcher.is_match(&rel_paths[idx]) {
            hits.push(idx);
        }
    }
    Ok(hits)
}

// ── Changed-file filtering ───────────────────────────────────────

// `filter_changed_files`, `try_get_changed_files`, `get_changed_files`, and
// `ChangedFilesError` were promoted to `fallow_core::changed_files` so the LSP
// (which depends on `fallow-core` but not `fallow-cli`) can reuse the exact
// same filter and git-resolution logic. Re-exported below for the existing
// internal call sites in this crate.
pub use fallow_core::changed_files::{
    filter_results_by_changed_files as filter_changed_files, get_changed_files,
    try_get_changed_files,
};

// ── Changed workspaces ───────────────────────────────────────────

/// Given a list of discovered workspaces and a set of changed file paths,
/// return the indices of workspaces that contain any changed file.
///
/// Pure function, intentionally independent of git / filesystem so the mapping
/// logic can be exercised without a repo. Files outside any workspace (e.g.
/// root-level `package.json`, lockfiles, CI configs) are ignored; they map to
/// zero workspaces, and the caller decides what to do with an empty result.
fn workspaces_containing_any(
    workspaces: &[WorkspaceInfo],
    changed_files: &FxHashSet<std::path::PathBuf>,
) -> Vec<usize> {
    let mut hits: Vec<usize> = Vec::new();
    for (idx, ws) in workspaces.iter().enumerate() {
        if changed_files.iter().any(|f| f.starts_with(&ws.root)) {
            hits.push(idx);
        }
    }
    hits
}

/// Resolve `--changed-workspaces <REF>` to a set of workspace roots containing
/// files changed since `git_ref`.
///
/// Unlike `--changed-since`, which silently falls back to full-scope analysis
/// if git fails, this resolver treats any git failure as a hard error: the
/// flag's entire purpose is to narrow CI scope, so silently widening back to
/// the whole monorepo would defeat the optimization and surprise the user.
///
/// Returns `Ok(vec![])` when git succeeded but no tracked workspace files
/// changed (normal CI outcome: a root-only lockfile bump, for example).
pub fn resolve_changed_workspaces(
    root: &Path,
    git_ref: &str,
    output: OutputFormat,
) -> Result<Vec<PathBuf>, ExitCode> {
    let workspaces = discover_workspaces(root);
    if workspaces.is_empty() {
        let msg = format!(
            "--changed-workspaces '{git_ref}' specified but no workspaces found. \
             Ensure root package.json has a \"workspaces\" field, pnpm-workspace.yaml exists, \
             or tsconfig.json has \"references\"."
        );
        return Err(emit_error(&msg, 2, output));
    }

    let changed_files = try_get_changed_files(root, git_ref).map_err(|err| {
        let msg = format!(
            "--changed-workspaces failed for ref '{git_ref}': {}",
            err.describe()
        );
        emit_error(&msg, 2, output)
    })?;

    let hits = workspaces_containing_any(&workspaces, &changed_files);
    let mut roots: Vec<PathBuf> = hits
        .into_iter()
        .map(|i| workspaces[i].root.clone())
        .collect();
    roots.sort();
    Ok(roots)
}

/// Resolve whichever workspace scoping flag the user passed. Returns `None`
/// when neither `--workspace` nor `--changed-workspaces` is set, so callers
/// can leave analysis at full scope.
///
/// `--workspace` and `--changed-workspaces` are mutually exclusive at the
/// CLI layer; this helper errors if both are set as a defence-in-depth check.
pub fn resolve_workspace_scope(
    root: &Path,
    workspace: Option<&[String]>,
    changed_workspaces: Option<&str>,
    output: OutputFormat,
) -> Result<Option<Vec<PathBuf>>, ExitCode> {
    match (workspace, changed_workspaces) {
        (Some(patterns), None) => Ok(Some(resolve_workspace_filters(root, patterns, output)?)),
        (None, Some(git_ref)) => Ok(Some(resolve_changed_workspaces(root, git_ref, output)?)),
        (None, None) => Ok(None),
        (Some(_), Some(_)) => {
            let msg = "--workspace and --changed-workspaces are mutually exclusive. \
                 Pick one: --workspace for explicit package names/globs, \
                 --changed-workspaces for git-derived monorepo CI scoping.";
            Err(emit_error(msg, 2, output))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fallow_core::extract::MemberKind;
    use fallow_core::results::*;
    use std::path::PathBuf;

    /// Test shim: single-workspace variant on top of `filter_to_workspaces`.
    fn filter_to_workspace(results: &mut AnalysisResults, ws_root: &Path) {
        filter_to_workspaces(results, std::slice::from_ref(&ws_root.to_path_buf()));
    }

    #[test]
    fn filter_to_workspace_keeps_files_under_ws_root() {
        let mut results = AnalysisResults::default();
        results.unused_files.push(UnusedFile {
            path: PathBuf::from("/project/packages/ui/src/button.ts"),
        });
        results.unused_files.push(UnusedFile {
            path: PathBuf::from("/project/packages/api/src/handler.ts"),
        });

        let ws_root = PathBuf::from("/project/packages/ui");
        filter_to_workspace(&mut results, &ws_root);

        assert_eq!(results.unused_files.len(), 1);
        assert_eq!(
            results.unused_files[0].path,
            PathBuf::from("/project/packages/ui/src/button.ts")
        );
    }

    #[test]
    fn filter_to_workspace_scopes_dependencies_to_ws_package_json() {
        let mut results = AnalysisResults::default();
        results.unused_dependencies.push(UnusedDependency {
            package_name: "lodash".into(),
            location: DependencyLocation::Dependencies,
            path: PathBuf::from("/project/package.json"),
            line: 5,
            used_in_workspaces: Vec::new(),
        });
        results.unused_dependencies.push(UnusedDependency {
            package_name: "react".into(),
            location: DependencyLocation::Dependencies,
            path: PathBuf::from("/project/packages/ui/package.json"),
            line: 5,
            used_in_workspaces: Vec::new(),
        });
        results.unused_dev_dependencies.push(UnusedDependency {
            package_name: "vitest".into(),
            location: DependencyLocation::DevDependencies,
            path: PathBuf::from("/project/packages/ui/package.json"),
            line: 5,
            used_in_workspaces: Vec::new(),
        });

        let ws_root = PathBuf::from("/project/packages/ui");
        filter_to_workspace(&mut results, &ws_root);

        assert_eq!(results.unused_dependencies.len(), 1);
        assert_eq!(results.unused_dependencies[0].package_name, "react");
        assert_eq!(results.unused_dev_dependencies.len(), 1);
        assert_eq!(results.unused_dev_dependencies[0].package_name, "vitest");
    }

    #[test]
    fn filter_to_workspace_scopes_unlisted_deps_by_importer() {
        let mut results = AnalysisResults::default();
        results.unlisted_dependencies.push(UnlistedDependency {
            package_name: "chalk".into(),
            imported_from: vec![ImportSite {
                path: PathBuf::from("/project/packages/ui/src/a.ts"),
                line: 1,
                col: 0,
            }],
        });
        results.unlisted_dependencies.push(UnlistedDependency {
            package_name: "debug".into(),
            imported_from: vec![ImportSite {
                path: PathBuf::from("/project/packages/api/src/b.ts"),
                line: 1,
                col: 0,
            }],
        });

        let ws_root = PathBuf::from("/project/packages/ui");
        filter_to_workspace(&mut results, &ws_root);

        assert_eq!(results.unlisted_dependencies.len(), 1);
        assert_eq!(results.unlisted_dependencies[0].package_name, "chalk");
    }

    #[test]
    fn filter_to_workspace_drops_duplicate_exports_below_two_locations() {
        let mut results = AnalysisResults::default();
        results.duplicate_exports.push(DuplicateExport {
            export_name: "helper".into(),
            locations: vec![
                DuplicateLocation {
                    path: PathBuf::from("/project/packages/ui/src/a.ts"),
                    line: 15,
                    col: 0,
                },
                DuplicateLocation {
                    path: PathBuf::from("/project/packages/api/src/b.ts"),
                    line: 30,
                    col: 0,
                },
            ],
        });
        results.duplicate_exports.push(DuplicateExport {
            export_name: "utils".into(),
            locations: vec![
                DuplicateLocation {
                    path: PathBuf::from("/project/packages/ui/src/c.ts"),
                    line: 15,
                    col: 0,
                },
                DuplicateLocation {
                    path: PathBuf::from("/project/packages/ui/src/d.ts"),
                    line: 30,
                    col: 0,
                },
            ],
        });

        let ws_root = PathBuf::from("/project/packages/ui");
        filter_to_workspace(&mut results, &ws_root);

        // "helper" had only 1 location in workspace — dropped
        // "utils" had 2 locations in workspace — kept
        assert_eq!(results.duplicate_exports.len(), 1);
        assert_eq!(results.duplicate_exports[0].export_name, "utils");
    }

    #[test]
    fn filter_to_workspace_scopes_exports_and_types() {
        let mut results = AnalysisResults::default();
        results.unused_exports.push(UnusedExport {
            path: PathBuf::from("/project/packages/ui/src/a.ts"),
            export_name: "A".into(),
            is_type_only: false,
            line: 1,
            col: 0,
            span_start: 0,
            is_re_export: false,
        });
        results.unused_exports.push(UnusedExport {
            path: PathBuf::from("/project/packages/api/src/b.ts"),
            export_name: "B".into(),
            is_type_only: false,
            line: 2,
            col: 0,
            span_start: 0,
            is_re_export: false,
        });
        results.unused_types.push(UnusedExport {
            path: PathBuf::from("/project/packages/ui/src/types.ts"),
            export_name: "T".into(),
            is_type_only: true,
            line: 1,
            col: 0,
            span_start: 0,
            is_re_export: false,
        });

        let ws_root = PathBuf::from("/project/packages/ui");
        filter_to_workspace(&mut results, &ws_root);

        assert_eq!(results.unused_exports.len(), 1);
        assert_eq!(results.unused_exports[0].export_name, "A");
        assert_eq!(results.unused_types.len(), 1);
        assert_eq!(results.unused_types[0].export_name, "T");
    }

    #[test]
    fn filter_to_workspace_scopes_type_only_dependencies() {
        let mut results = AnalysisResults::default();
        results.type_only_dependencies.push(TypeOnlyDependency {
            package_name: "zod".into(),
            path: PathBuf::from("/project/packages/ui/package.json"),
            line: 8,
        });
        results.type_only_dependencies.push(TypeOnlyDependency {
            package_name: "yup".into(),
            path: PathBuf::from("/project/package.json"),
            line: 8,
        });

        let ws_root = PathBuf::from("/project/packages/ui");
        filter_to_workspace(&mut results, &ws_root);

        assert_eq!(results.type_only_dependencies.len(), 1);
        assert_eq!(results.type_only_dependencies[0].package_name, "zod");
    }

    #[test]
    fn filter_to_workspace_scopes_enum_and_class_members() {
        let mut results = AnalysisResults::default();
        results.unused_enum_members.push(UnusedMember {
            path: PathBuf::from("/project/packages/ui/src/enums.ts"),
            parent_name: "Color".into(),
            member_name: "Red".into(),
            kind: MemberKind::EnumMember,
            line: 2,
            col: 0,
        });
        results.unused_enum_members.push(UnusedMember {
            path: PathBuf::from("/project/packages/api/src/enums.ts"),
            parent_name: "Status".into(),
            member_name: "Active".into(),
            kind: MemberKind::EnumMember,
            line: 3,
            col: 0,
        });
        results.unused_class_members.push(UnusedMember {
            path: PathBuf::from("/project/packages/ui/src/service.ts"),
            parent_name: "Svc".into(),
            member_name: "init".into(),
            kind: MemberKind::ClassMethod,
            line: 5,
            col: 0,
        });

        let ws_root = PathBuf::from("/project/packages/ui");
        filter_to_workspace(&mut results, &ws_root);

        assert_eq!(results.unused_enum_members.len(), 1);
        assert_eq!(results.unused_enum_members[0].member_name, "Red");
        assert_eq!(results.unused_class_members.len(), 1);
        assert_eq!(results.unused_class_members[0].member_name, "init");
    }

    // ── filter_changed_files ────────────────────────────────────────

    #[test]
    fn filter_changed_files_keeps_only_changed() {
        let mut results = AnalysisResults::default();
        results.unused_files.push(UnusedFile {
            path: PathBuf::from("/project/src/a.ts"),
        });
        results.unused_files.push(UnusedFile {
            path: PathBuf::from("/project/src/b.ts"),
        });

        let mut changed = rustc_hash::FxHashSet::default();
        changed.insert(PathBuf::from("/project/src/a.ts"));

        filter_changed_files(&mut results, &changed);

        assert_eq!(results.unused_files.len(), 1);
        assert_eq!(
            results.unused_files[0].path,
            PathBuf::from("/project/src/a.ts")
        );
    }

    #[test]
    fn filter_changed_files_preserves_unused_deps() {
        let mut results = AnalysisResults::default();
        results.unused_dependencies.push(UnusedDependency {
            package_name: "lodash".into(),
            location: DependencyLocation::Dependencies,
            path: PathBuf::from("/project/package.json"),
            line: 5,
            used_in_workspaces: Vec::new(),
        });
        results.unused_dev_dependencies.push(UnusedDependency {
            package_name: "jest".into(),
            location: DependencyLocation::DevDependencies,
            path: PathBuf::from("/project/package.json"),
            line: 10,
            used_in_workspaces: Vec::new(),
        });

        let changed = rustc_hash::FxHashSet::default(); // empty set

        filter_changed_files(&mut results, &changed);

        // Dependency-level issues are NOT filtered by changed files
        assert_eq!(results.unused_dependencies.len(), 1);
        assert_eq!(results.unused_dev_dependencies.len(), 1);
    }

    #[test]
    fn filter_changed_files_filters_exports_by_path() {
        let mut results = AnalysisResults::default();
        results.unused_exports.push(UnusedExport {
            path: PathBuf::from("/project/src/a.ts"),
            export_name: "foo".into(),
            is_type_only: false,
            line: 1,
            col: 0,
            span_start: 0,
            is_re_export: false,
        });
        results.unused_exports.push(UnusedExport {
            path: PathBuf::from("/project/src/b.ts"),
            export_name: "bar".into(),
            is_type_only: false,
            line: 1,
            col: 0,
            span_start: 0,
            is_re_export: false,
        });

        let mut changed = rustc_hash::FxHashSet::default();
        changed.insert(PathBuf::from("/project/src/b.ts"));

        filter_changed_files(&mut results, &changed);

        assert_eq!(results.unused_exports.len(), 1);
        assert_eq!(results.unused_exports[0].export_name, "bar");
    }

    #[test]
    fn filter_changed_files_drops_duplicate_exports_below_two() {
        let mut results = AnalysisResults::default();
        results.duplicate_exports.push(DuplicateExport {
            export_name: "helper".into(),
            locations: vec![
                DuplicateLocation {
                    path: PathBuf::from("/project/src/a.ts"),
                    line: 1,
                    col: 0,
                },
                DuplicateLocation {
                    path: PathBuf::from("/project/src/b.ts"),
                    line: 2,
                    col: 0,
                },
            ],
        });

        let mut changed = rustc_hash::FxHashSet::default();
        changed.insert(PathBuf::from("/project/src/a.ts"));

        filter_changed_files(&mut results, &changed);

        // Only one location is in changed files -> group dropped
        assert!(results.duplicate_exports.is_empty());
    }

    #[test]
    fn filter_changed_files_keeps_circular_deps_if_any_file_changed() {
        let mut results = AnalysisResults::default();
        results.circular_dependencies.push(CircularDependency {
            files: vec![
                PathBuf::from("/project/src/a.ts"),
                PathBuf::from("/project/src/b.ts"),
            ],
            length: 2,
            line: 1,
            col: 0,
            is_cross_package: false,
        });

        let mut changed = rustc_hash::FxHashSet::default();
        changed.insert(PathBuf::from("/project/src/b.ts"));

        filter_changed_files(&mut results, &changed);

        assert_eq!(results.circular_dependencies.len(), 1);
    }

    #[test]
    fn filter_changed_files_removes_circular_deps_if_no_file_changed() {
        let mut results = AnalysisResults::default();
        results.circular_dependencies.push(CircularDependency {
            files: vec![
                PathBuf::from("/project/src/a.ts"),
                PathBuf::from("/project/src/b.ts"),
            ],
            length: 2,
            line: 1,
            col: 0,
            is_cross_package: false,
        });

        let mut changed = rustc_hash::FxHashSet::default();
        changed.insert(PathBuf::from("/project/src/c.ts"));

        filter_changed_files(&mut results, &changed);

        assert!(results.circular_dependencies.is_empty());
    }

    #[test]
    fn filter_changed_files_keeps_unlisted_dep_if_importer_changed() {
        let mut results = AnalysisResults::default();
        results.unlisted_dependencies.push(UnlistedDependency {
            package_name: "chalk".into(),
            imported_from: vec![ImportSite {
                path: PathBuf::from("/project/src/a.ts"),
                line: 1,
                col: 0,
            }],
        });

        let mut changed = rustc_hash::FxHashSet::default();
        changed.insert(PathBuf::from("/project/src/a.ts"));

        filter_changed_files(&mut results, &changed);

        assert_eq!(results.unlisted_dependencies.len(), 1);
    }

    #[test]
    fn filter_changed_files_removes_unlisted_dep_if_no_importer_changed() {
        let mut results = AnalysisResults::default();
        results.unlisted_dependencies.push(UnlistedDependency {
            package_name: "chalk".into(),
            imported_from: vec![ImportSite {
                path: PathBuf::from("/project/src/a.ts"),
                line: 1,
                col: 0,
            }],
        });

        let mut changed = rustc_hash::FxHashSet::default();
        changed.insert(PathBuf::from("/project/src/b.ts"));

        filter_changed_files(&mut results, &changed);

        assert!(results.unlisted_dependencies.is_empty());
    }

    // ── filter_to_workspace: additional coverage ───────────────────

    #[test]
    fn filter_to_workspace_scopes_optional_dependencies() {
        let mut results = AnalysisResults::default();
        results.unused_optional_dependencies.push(UnusedDependency {
            package_name: "fsevents".into(),
            location: DependencyLocation::OptionalDependencies,
            path: PathBuf::from("/project/packages/ui/package.json"),
            line: 3,
            used_in_workspaces: Vec::new(),
        });
        results.unused_optional_dependencies.push(UnusedDependency {
            package_name: "esbuild".into(),
            location: DependencyLocation::OptionalDependencies,
            path: PathBuf::from("/project/package.json"),
            line: 7,
            used_in_workspaces: Vec::new(),
        });

        let ws_root = PathBuf::from("/project/packages/ui");
        filter_to_workspace(&mut results, &ws_root);

        assert_eq!(results.unused_optional_dependencies.len(), 1);
        assert_eq!(
            results.unused_optional_dependencies[0].package_name,
            "fsevents"
        );
    }

    #[test]
    fn filter_to_workspace_scopes_test_only_dependencies() {
        let mut results = AnalysisResults::default();
        results.test_only_dependencies.push(TestOnlyDependency {
            package_name: "msw".into(),
            path: PathBuf::from("/project/packages/ui/package.json"),
            line: 4,
        });
        results.test_only_dependencies.push(TestOnlyDependency {
            package_name: "nock".into(),
            path: PathBuf::from("/project/packages/api/package.json"),
            line: 6,
        });

        let ws_root = PathBuf::from("/project/packages/ui");
        filter_to_workspace(&mut results, &ws_root);

        assert_eq!(results.test_only_dependencies.len(), 1);
        assert_eq!(results.test_only_dependencies[0].package_name, "msw");
    }

    #[test]
    fn filter_to_workspace_scopes_circular_dependencies() {
        let mut results = AnalysisResults::default();
        results.circular_dependencies.push(CircularDependency {
            files: vec![
                PathBuf::from("/project/packages/ui/src/a.ts"),
                PathBuf::from("/project/packages/ui/src/b.ts"),
            ],
            length: 2,
            line: 1,
            col: 0,
            is_cross_package: false,
        });
        results.circular_dependencies.push(CircularDependency {
            files: vec![
                PathBuf::from("/project/packages/api/src/x.ts"),
                PathBuf::from("/project/packages/api/src/y.ts"),
            ],
            length: 2,
            line: 1,
            col: 0,
            is_cross_package: false,
        });

        let ws_root = PathBuf::from("/project/packages/ui");
        filter_to_workspace(&mut results, &ws_root);

        assert_eq!(results.circular_dependencies.len(), 1);
        assert_eq!(
            results.circular_dependencies[0].files[0],
            PathBuf::from("/project/packages/ui/src/a.ts")
        );
    }

    #[test]
    fn filter_to_workspace_keeps_circular_dep_if_any_file_in_workspace() {
        let mut results = AnalysisResults::default();
        results.circular_dependencies.push(CircularDependency {
            files: vec![
                PathBuf::from("/project/packages/ui/src/a.ts"),
                PathBuf::from("/project/packages/api/src/b.ts"),
            ],
            length: 2,
            line: 1,
            col: 0,
            is_cross_package: false,
        });

        let ws_root = PathBuf::from("/project/packages/ui");
        filter_to_workspace(&mut results, &ws_root);

        // Kept because at least one file is in the workspace
        assert_eq!(results.circular_dependencies.len(), 1);
    }

    #[test]
    fn filter_to_workspace_scopes_unresolved_imports() {
        let mut results = AnalysisResults::default();
        results.unresolved_imports.push(UnresolvedImport {
            path: PathBuf::from("/project/packages/ui/src/a.ts"),
            specifier: "./missing".into(),
            line: 1,
            col: 0,
            specifier_col: 0,
        });
        results.unresolved_imports.push(UnresolvedImport {
            path: PathBuf::from("/project/packages/api/src/b.ts"),
            specifier: "./gone".into(),
            line: 2,
            col: 0,
            specifier_col: 0,
        });

        let ws_root = PathBuf::from("/project/packages/ui");
        filter_to_workspace(&mut results, &ws_root);

        assert_eq!(results.unresolved_imports.len(), 1);
        assert_eq!(results.unresolved_imports[0].specifier, "./missing");
    }

    #[test]
    fn filter_to_workspace_on_empty_results_stays_empty() {
        let mut results = AnalysisResults::default();
        let ws_root = PathBuf::from("/project/packages/ui");
        filter_to_workspace(&mut results, &ws_root);
        assert_eq!(results.total_issues(), 0);
    }

    // ── filter_changed_files: additional coverage ──────────────────

    #[test]
    fn filter_changed_files_filters_types_by_path() {
        let mut results = AnalysisResults::default();
        results.unused_types.push(UnusedExport {
            path: PathBuf::from("/project/src/types.ts"),
            export_name: "Foo".into(),
            is_type_only: true,
            line: 1,
            col: 0,
            span_start: 0,
            is_re_export: false,
        });
        results.unused_types.push(UnusedExport {
            path: PathBuf::from("/project/src/other.ts"),
            export_name: "Bar".into(),
            is_type_only: true,
            line: 2,
            col: 0,
            span_start: 0,
            is_re_export: false,
        });

        let mut changed = rustc_hash::FxHashSet::default();
        changed.insert(PathBuf::from("/project/src/types.ts"));

        filter_changed_files(&mut results, &changed);

        assert_eq!(results.unused_types.len(), 1);
        assert_eq!(results.unused_types[0].export_name, "Foo");
    }

    #[test]
    fn filter_changed_files_filters_enum_members_by_path() {
        let mut results = AnalysisResults::default();
        results.unused_enum_members.push(UnusedMember {
            path: PathBuf::from("/project/src/enums.ts"),
            parent_name: "Color".into(),
            member_name: "Red".into(),
            kind: MemberKind::EnumMember,
            line: 2,
            col: 0,
        });
        results.unused_enum_members.push(UnusedMember {
            path: PathBuf::from("/project/src/other.ts"),
            parent_name: "Status".into(),
            member_name: "Active".into(),
            kind: MemberKind::EnumMember,
            line: 3,
            col: 0,
        });

        let mut changed = rustc_hash::FxHashSet::default();
        changed.insert(PathBuf::from("/project/src/enums.ts"));

        filter_changed_files(&mut results, &changed);

        assert_eq!(results.unused_enum_members.len(), 1);
        assert_eq!(results.unused_enum_members[0].member_name, "Red");
    }

    #[test]
    fn filter_changed_files_filters_class_members_by_path() {
        let mut results = AnalysisResults::default();
        results.unused_class_members.push(UnusedMember {
            path: PathBuf::from("/project/src/service.ts"),
            parent_name: "Svc".into(),
            member_name: "init".into(),
            kind: MemberKind::ClassMethod,
            line: 5,
            col: 0,
        });
        results.unused_class_members.push(UnusedMember {
            path: PathBuf::from("/project/src/other.ts"),
            parent_name: "Other".into(),
            member_name: "run".into(),
            kind: MemberKind::ClassMethod,
            line: 10,
            col: 0,
        });

        let mut changed = rustc_hash::FxHashSet::default();
        changed.insert(PathBuf::from("/project/src/service.ts"));

        filter_changed_files(&mut results, &changed);

        assert_eq!(results.unused_class_members.len(), 1);
        assert_eq!(results.unused_class_members[0].member_name, "init");
    }

    #[test]
    fn filter_changed_files_preserves_optional_and_type_only_and_test_only_deps() {
        let mut results = AnalysisResults::default();
        results.unused_optional_dependencies.push(UnusedDependency {
            package_name: "fsevents".into(),
            location: DependencyLocation::OptionalDependencies,
            path: PathBuf::from("/project/package.json"),
            line: 3,
            used_in_workspaces: Vec::new(),
        });
        results.type_only_dependencies.push(TypeOnlyDependency {
            package_name: "zod".into(),
            path: PathBuf::from("/project/package.json"),
            line: 8,
        });
        results.test_only_dependencies.push(TestOnlyDependency {
            package_name: "msw".into(),
            path: PathBuf::from("/project/package.json"),
            line: 12,
        });

        let changed = rustc_hash::FxHashSet::default();

        filter_changed_files(&mut results, &changed);

        // Dependency-level issues are NOT filtered by changed files
        assert_eq!(results.unused_optional_dependencies.len(), 1);
        assert_eq!(results.type_only_dependencies.len(), 1);
        assert_eq!(results.test_only_dependencies.len(), 1);
    }

    #[test]
    fn filter_changed_files_keeps_duplicate_exports_when_both_changed() {
        let mut results = AnalysisResults::default();
        results.duplicate_exports.push(DuplicateExport {
            export_name: "helper".into(),
            locations: vec![
                DuplicateLocation {
                    path: PathBuf::from("/project/src/a.ts"),
                    line: 1,
                    col: 0,
                },
                DuplicateLocation {
                    path: PathBuf::from("/project/src/b.ts"),
                    line: 2,
                    col: 0,
                },
            ],
        });

        let mut changed = rustc_hash::FxHashSet::default();
        changed.insert(PathBuf::from("/project/src/a.ts"));
        changed.insert(PathBuf::from("/project/src/b.ts"));

        filter_changed_files(&mut results, &changed);

        assert_eq!(results.duplicate_exports.len(), 1);
        assert_eq!(results.duplicate_exports[0].locations.len(), 2);
    }

    #[test]
    fn filter_changed_files_empty_set_clears_file_scoped_issues() {
        let mut results = AnalysisResults::default();
        results.unused_files.push(UnusedFile {
            path: PathBuf::from("/project/src/a.ts"),
        });
        results.unused_exports.push(UnusedExport {
            path: PathBuf::from("/project/src/b.ts"),
            export_name: "foo".into(),
            is_type_only: false,
            line: 1,
            col: 0,
            span_start: 0,
            is_re_export: false,
        });
        results.unused_types.push(UnusedExport {
            path: PathBuf::from("/project/src/c.ts"),
            export_name: "T".into(),
            is_type_only: true,
            line: 1,
            col: 0,
            span_start: 0,
            is_re_export: false,
        });
        results.unused_enum_members.push(UnusedMember {
            path: PathBuf::from("/project/src/d.ts"),
            parent_name: "E".into(),
            member_name: "A".into(),
            kind: MemberKind::EnumMember,
            line: 1,
            col: 0,
        });
        results.unused_class_members.push(UnusedMember {
            path: PathBuf::from("/project/src/e.ts"),
            parent_name: "C".into(),
            member_name: "m".into(),
            kind: MemberKind::ClassMethod,
            line: 1,
            col: 0,
        });
        results.unresolved_imports.push(UnresolvedImport {
            path: PathBuf::from("/project/src/f.ts"),
            specifier: "./x".into(),
            line: 1,
            col: 0,
            specifier_col: 0,
        });

        let changed = rustc_hash::FxHashSet::default();

        filter_changed_files(&mut results, &changed);

        assert!(results.unused_files.is_empty());
        assert!(results.unused_exports.is_empty());
        assert!(results.unused_types.is_empty());
        assert!(results.unused_enum_members.is_empty());
        assert!(results.unused_class_members.is_empty());
        assert!(results.unresolved_imports.is_empty());
    }

    #[test]
    fn filter_changed_files_on_empty_results_stays_empty() {
        let mut results = AnalysisResults::default();
        let mut changed = rustc_hash::FxHashSet::default();
        changed.insert(PathBuf::from("/project/src/a.ts"));

        filter_changed_files(&mut results, &changed);

        assert_eq!(results.total_issues(), 0);
    }

    #[test]
    fn filter_changed_files_unlisted_dep_with_multiple_importers_keeps_if_any_changed() {
        let mut results = AnalysisResults::default();
        results.unlisted_dependencies.push(UnlistedDependency {
            package_name: "chalk".into(),
            imported_from: vec![
                ImportSite {
                    path: PathBuf::from("/project/src/a.ts"),
                    line: 1,
                    col: 0,
                },
                ImportSite {
                    path: PathBuf::from("/project/src/b.ts"),
                    line: 5,
                    col: 0,
                },
            ],
        });

        let mut changed = rustc_hash::FxHashSet::default();
        changed.insert(PathBuf::from("/project/src/b.ts"));

        filter_changed_files(&mut results, &changed);

        assert_eq!(results.unlisted_dependencies.len(), 1);
    }

    #[test]
    fn filter_changed_files_filters_unresolved_imports_by_path() {
        let mut results = AnalysisResults::default();
        results.unresolved_imports.push(UnresolvedImport {
            path: PathBuf::from("/project/src/a.ts"),
            specifier: "./missing".into(),
            line: 1,
            col: 0,
            specifier_col: 0,
        });
        results.unresolved_imports.push(UnresolvedImport {
            path: PathBuf::from("/project/src/b.ts"),
            specifier: "./gone".into(),
            line: 2,
            col: 0,
            specifier_col: 0,
        });

        let mut changed = rustc_hash::FxHashSet::default();
        changed.insert(PathBuf::from("/project/src/a.ts"));

        filter_changed_files(&mut results, &changed);

        assert_eq!(results.unresolved_imports.len(), 1);
        assert_eq!(results.unresolved_imports[0].specifier, "./missing");
    }

    // ── multi-workspace resolution ──────────────────────────────────

    fn ws(name: &str, rel: &str) -> fallow_config::WorkspaceInfo {
        fallow_config::WorkspaceInfo {
            root: PathBuf::from("/project").join(rel),
            name: name.to_owned(),
            is_internal_dependency: false,
        }
    }

    fn rel(workspaces: &[fallow_config::WorkspaceInfo]) -> Vec<String> {
        workspaces
            .iter()
            .map(|w| relative_workspace_path(&w.root, Path::new("/project")))
            .collect()
    }

    #[test]
    fn split_patterns_separates_positive_and_negative() {
        let input = vec![
            "web".to_owned(),
            "apps/*".to_owned(),
            "!apps/legacy".to_owned(),
            "  ".to_owned(),
            String::new(),
            "!  ".to_owned(),
        ];
        let (pos, neg) = split_patterns(&input);
        assert_eq!(pos, vec!["web", "apps/*"]);
        assert_eq!(neg, vec!["apps/legacy"]);
    }

    #[test]
    fn find_matches_exact_name_short_circuits_glob_metachars() {
        // Package named `web-[staging]` contains glob metachars. Exact-name
        // short-circuit must match it without attempting to compile as a glob.
        let workspaces = vec![ws("web-[staging]", "apps/web-staging")];
        let rels = rel(&workspaces);
        let hits = find_matches(
            "web-[staging]",
            &workspaces,
            &rels,
            fallow_config::OutputFormat::Human,
        )
        .unwrap();
        assert_eq!(hits, vec![0]);
    }

    #[test]
    fn find_matches_glob_against_name_and_path() {
        let workspaces = vec![
            ws("@scope/ui", "packages/ui"),
            ws("admin", "apps/admin"),
            ws("web", "apps/web"),
        ];
        let rels = rel(&workspaces);

        // Glob matching via name
        let hits = find_matches(
            "@scope/*",
            &workspaces,
            &rels,
            fallow_config::OutputFormat::Human,
        )
        .unwrap();
        assert_eq!(hits, vec![0]);

        // Glob matching via relative path
        let hits = find_matches(
            "apps/*",
            &workspaces,
            &rels,
            fallow_config::OutputFormat::Human,
        )
        .unwrap();
        assert_eq!(hits, vec![1, 2]);
    }

    #[test]
    fn find_matches_invalid_glob_after_no_literal_match_errors() {
        let workspaces = vec![ws("web", "apps/web")];
        let rels = rel(&workspaces);
        // `[` without closing is invalid glob syntax AND not a literal name.
        assert!(
            find_matches(
                "web-[bad",
                &workspaces,
                &rels,
                fallow_config::OutputFormat::Human,
            )
            .is_err()
        );
    }

    #[test]
    fn format_available_workspaces_truncates_when_above_cap() {
        let workspaces: Vec<WorkspaceInfo> = (0..15)
            .map(|i| ws(&format!("pkg-{i}"), &format!("packages/pkg-{i}")))
            .collect();
        let rendered = format_available_workspaces(&workspaces);
        assert!(rendered.starts_with("pkg-0, pkg-1,"));
        assert!(rendered.contains("and 5 more"));
        assert!(rendered.contains("15 total"));
    }

    #[test]
    fn format_available_workspaces_does_not_truncate_below_cap() {
        let workspaces = vec![ws("a", "packages/a"), ws("b", "packages/b")];
        assert_eq!(format_available_workspaces(&workspaces), "a, b");
    }

    #[test]
    fn filter_to_workspaces_unions_multiple_roots() {
        let mut results = AnalysisResults::default();
        results.unused_files.push(UnusedFile {
            path: PathBuf::from("/project/packages/ui/src/a.ts"),
        });
        results.unused_files.push(UnusedFile {
            path: PathBuf::from("/project/packages/api/src/b.ts"),
        });
        results.unused_files.push(UnusedFile {
            path: PathBuf::from("/project/packages/legacy/src/c.ts"),
        });

        let roots = [
            PathBuf::from("/project/packages/ui"),
            PathBuf::from("/project/packages/api"),
        ];
        filter_to_workspaces(&mut results, &roots);

        assert_eq!(results.unused_files.len(), 2);
    }

    #[test]
    fn filter_to_workspaces_scopes_deps_to_matched_package_jsons() {
        let mut results = AnalysisResults::default();
        results.unused_dependencies.push(UnusedDependency {
            package_name: "lodash".into(),
            location: DependencyLocation::Dependencies,
            path: PathBuf::from("/project/packages/ui/package.json"),
            line: 5,
            used_in_workspaces: Vec::new(),
        });
        results.unused_dependencies.push(UnusedDependency {
            package_name: "react".into(),
            location: DependencyLocation::Dependencies,
            path: PathBuf::from("/project/packages/api/package.json"),
            line: 5,
            used_in_workspaces: Vec::new(),
        });
        results.unused_dependencies.push(UnusedDependency {
            package_name: "axios".into(),
            location: DependencyLocation::Dependencies,
            path: PathBuf::from("/project/packages/legacy/package.json"),
            line: 5,
            used_in_workspaces: Vec::new(),
        });

        let roots = [
            PathBuf::from("/project/packages/ui"),
            PathBuf::from("/project/packages/api"),
        ];
        filter_to_workspaces(&mut results, &roots);

        assert_eq!(results.unused_dependencies.len(), 2);
        let names: Vec<&str> = results
            .unused_dependencies
            .iter()
            .map(|d| d.package_name.as_ref())
            .collect();
        assert!(names.contains(&"lodash"));
        assert!(names.contains(&"react"));
        assert!(!names.contains(&"axios"));
    }

    #[test]
    fn filter_to_workspaces_empty_slice_drops_everything() {
        let mut results = AnalysisResults::default();
        results.unused_files.push(UnusedFile {
            path: PathBuf::from("/project/packages/ui/src/a.ts"),
        });
        filter_to_workspaces(&mut results, &[]);
        assert_eq!(results.unused_files.len(), 0);
    }

    // ── workspaces_containing_any (pure mapping) ────────────────────

    #[test]
    fn workspaces_containing_any_returns_only_hits() {
        let workspaces = vec![
            ws("ui", "packages/ui"),
            ws("api", "packages/api"),
            ws("legacy", "packages/legacy"),
        ];
        let mut changed = FxHashSet::default();
        changed.insert(PathBuf::from("/project/packages/ui/src/a.ts"));
        changed.insert(PathBuf::from("/project/packages/api/src/b.ts"));

        let hits = workspaces_containing_any(&workspaces, &changed);
        assert_eq!(hits, vec![0, 1]);
    }

    #[test]
    fn workspaces_containing_any_ignores_root_only_changes() {
        // Root-level changes (lockfiles, CI config, top package.json) must not
        // implicitly scope to "every workspace": they map to zero workspaces.
        let workspaces = vec![ws("ui", "packages/ui"), ws("api", "packages/api")];
        let mut changed = FxHashSet::default();
        changed.insert(PathBuf::from("/project/package.json"));
        changed.insert(PathBuf::from("/project/pnpm-lock.yaml"));

        let hits = workspaces_containing_any(&workspaces, &changed);
        assert!(hits.is_empty());
    }

    #[test]
    fn workspaces_containing_any_empty_changed_set_is_no_hits() {
        let workspaces = vec![ws("ui", "packages/ui")];
        let changed = FxHashSet::default();

        let hits = workspaces_containing_any(&workspaces, &changed);
        assert!(hits.is_empty());
    }

    #[test]
    fn workspaces_containing_any_single_changed_file_maps_to_one_workspace() {
        let workspaces = vec![
            ws("ui", "packages/ui"),
            ws("api", "packages/api"),
            ws("cli", "packages/cli"),
        ];
        let mut changed = FxHashSet::default();
        changed.insert(PathBuf::from("/project/packages/api/src/b.ts"));

        let hits = workspaces_containing_any(&workspaces, &changed);
        assert_eq!(hits, vec![1]);
    }

    // ── resolve_workspace_scope ─────────────────────────────────────

    #[test]
    fn resolve_workspace_scope_neither_flag_returns_none() {
        let root = Path::new("/project");
        let got = resolve_workspace_scope(root, None, None, OutputFormat::Human).unwrap();
        assert!(got.is_none());
    }

    #[test]
    fn resolve_workspace_scope_both_flags_is_error() {
        let root = Path::new("/project");
        let patterns = ["web".to_owned()];
        let got = resolve_workspace_scope(root, Some(&patterns), Some("main"), OutputFormat::Human);
        assert!(
            got.is_err(),
            "--workspace + --changed-workspaces must error out"
        );
    }

    // ChangedFilesError::describe is tested in fallow_core::changed_files
}
