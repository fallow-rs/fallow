mod package_json;
mod parsers;
mod pnpm_catalog;
mod pnpm_overrides;

use std::path::{Path, PathBuf};

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

pub use package_json::PackageJson;
pub use parsers::parse_tsconfig_root_dir;
use parsers::{expand_workspace_glob, parse_pnpm_workspace_yaml, parse_tsconfig_references};
pub use pnpm_catalog::{PnpmCatalog, PnpmCatalogData, PnpmCatalogEntry, parse_pnpm_catalog_data};
pub use pnpm_overrides::{
    MisconfigReason, OverrideSource, ParsedOverrideKey, PnpmOverrideData, PnpmOverrideEntry,
    is_valid_override_value, override_misconfig_reason, override_source_label, parse_override_key,
    parse_pnpm_package_json_overrides, parse_pnpm_workspace_overrides,
};

/// Workspace configuration for monorepo support.
#[derive(Debug, Clone, Deserialize, Serialize, JsonSchema)]
pub struct WorkspaceConfig {
    /// Additional workspace patterns (beyond what's in root package.json).
    #[serde(default)]
    pub patterns: Vec<String>,
}

/// Discovered workspace info from package.json, pnpm-workspace.yaml, or tsconfig.json references.
#[derive(Debug, Clone)]
pub struct WorkspaceInfo {
    /// Workspace root path.
    pub root: PathBuf,
    /// Package name from package.json.
    pub name: String,
    /// Whether this workspace is depended on by other workspaces.
    pub is_internal_dependency: bool,
}

/// A diagnostic about workspace configuration issues.
#[derive(Debug, Clone)]
pub struct WorkspaceDiagnostic {
    /// Path to the directory with the issue.
    pub path: PathBuf,
    /// Human-readable description of the issue.
    pub message: String,
}

/// Discover all workspace packages in a monorepo.
///
/// Sources (additive, deduplicated by canonical path):
/// 1. `package.json` `workspaces` field
/// 2. `pnpm-workspace.yaml` `packages` field
/// 3. `tsconfig.json` `references` field (TypeScript project references)
#[must_use]
pub fn discover_workspaces(root: &Path) -> Vec<WorkspaceInfo> {
    let patterns = collect_workspace_patterns(root);
    let canonical_root = dunce::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());

    let mut workspaces = expand_patterns_to_workspaces(root, &patterns, &canonical_root);
    workspaces.extend(collect_tsconfig_workspaces(root, &canonical_root));
    if patterns.is_empty() {
        workspaces.extend(collect_shallow_package_workspaces(root, &canonical_root));
    }

    if workspaces.is_empty() {
        return Vec::new();
    }

    mark_internal_dependencies(&mut workspaces);
    workspaces.into_iter().map(|(ws, _)| ws).collect()
}

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
    find_undeclared_workspaces_with_ignores(root, declared, &globset::GlobSet::empty())
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
    ignore_patterns: &globset::GlobSet,
) -> Vec<WorkspaceDiagnostic> {
    // Only run when workspaces are declared
    let patterns = collect_workspace_patterns(root);
    if patterns.is_empty() {
        return Vec::new();
    }

    let declared_roots: rustc_hash::FxHashSet<PathBuf> = declared
        .iter()
        .map(|w| dunce::canonicalize(&w.root).unwrap_or_else(|_| w.root.clone()))
        .collect();

    let canonical_root = dunce::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());

    let mut undeclared = Vec::new();

    // Walk first two levels of directories
    let Ok(top_entries) = std::fs::read_dir(root) else {
        return Vec::new();
    };

    for entry in top_entries.filter_map(Result::ok) {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }

        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if name_str.starts_with('.') || name_str == "node_modules" || name_str == "build" {
            continue;
        }

        // Check this directory itself
        check_undeclared(
            &path,
            root,
            &canonical_root,
            &declared_roots,
            ignore_patterns,
            &mut undeclared,
        );

        // Check immediate children (second level)
        let Ok(child_entries) = std::fs::read_dir(&path) else {
            continue;
        };
        for child in child_entries.filter_map(Result::ok) {
            let child_path = child.path();
            if !child_path.is_dir() {
                continue;
            }
            let child_name = child.file_name();
            let child_name_str = child_name.to_string_lossy();
            if child_name_str.starts_with('.')
                || child_name_str == "node_modules"
                || child_name_str == "build"
            {
                continue;
            }
            check_undeclared(
                &child_path,
                root,
                &canonical_root,
                &declared_roots,
                ignore_patterns,
                &mut undeclared,
            );
        }
    }

    undeclared
}

/// Check a single directory for an undeclared workspace.
fn check_undeclared(
    dir: &Path,
    root: &Path,
    canonical_root: &Path,
    declared_roots: &rustc_hash::FxHashSet<PathBuf>,
    ignore_patterns: &globset::GlobSet,
    undeclared: &mut Vec<WorkspaceDiagnostic>,
) {
    if !dir.join("package.json").exists() {
        return;
    }
    let canonical = dunce::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
    // Skip the project root itself
    if canonical == *canonical_root {
        return;
    }
    if declared_roots.contains(&canonical) {
        return;
    }
    let relative = dir.strip_prefix(root).unwrap_or(dir);
    // Honor user-supplied ignorePatterns: directories explicitly excluded should not
    // trigger an undeclared-workspace warning. Match using forward-slash normalized
    // relative path so cross-platform globs (`references/*`) work on Windows.
    let relative_str = relative.to_string_lossy().replace('\\', "/");
    if ignore_patterns.is_match(relative_str.as_str())
        || ignore_patterns.is_match(format!("{relative_str}/package.json").as_str())
    {
        return;
    }
    undeclared.push(WorkspaceDiagnostic {
        path: dir.to_path_buf(),
        message: format!(
            "Directory '{}' contains package.json but is not declared as a workspace",
            relative.display()
        ),
    });
}

/// Collect glob patterns from `package.json` `workspaces` field and `pnpm-workspace.yaml`.
fn collect_workspace_patterns(root: &Path) -> Vec<String> {
    let mut patterns = Vec::new();

    // Check root package.json for workspace patterns
    let pkg_path = root.join("package.json");
    if let Ok(pkg) = PackageJson::load(&pkg_path) {
        patterns.extend(pkg.workspace_patterns());
    }

    // Check pnpm-workspace.yaml
    let pnpm_workspace = root.join("pnpm-workspace.yaml");
    if pnpm_workspace.exists()
        && let Ok(content) = std::fs::read_to_string(&pnpm_workspace)
    {
        patterns.extend(parse_pnpm_workspace_yaml(&content));
    }

    patterns
}

/// Expand workspace glob patterns to discover workspace directories.
///
/// Handles positive/negated pattern splitting, glob matching, and package.json
/// loading for each matched directory.
fn expand_patterns_to_workspaces(
    root: &Path,
    patterns: &[String],
    canonical_root: &Path,
) -> Vec<(WorkspaceInfo, Vec<String>)> {
    if patterns.is_empty() {
        return Vec::new();
    }

    let mut workspaces = Vec::new();

    // Separate positive and negated patterns.
    // Negated patterns (e.g., `!**/test/**`) are used as exclusion filters —
    // the `glob` crate does not support `!` prefixed patterns natively.
    let (positive, negative): (Vec<&String>, Vec<&String>) =
        patterns.iter().partition(|p| !p.starts_with('!'));
    let negation_matchers: Vec<globset::GlobMatcher> = negative
        .iter()
        .filter_map(|p| {
            let stripped = p.strip_prefix('!').unwrap_or(p);
            globset::Glob::new(stripped)
                .ok()
                .map(|g| g.compile_matcher())
        })
        .collect();

    for pattern in &positive {
        // Normalize the pattern for directory matching:
        // - `packages/*` → glob for `packages/*` (find all subdirs)
        // - `packages/` → glob for `packages/*` (trailing slash means "contents of")
        // - `apps`       → glob for `apps` (exact directory)
        let glob_pattern = if pattern.ends_with('/') {
            format!("{pattern}*")
        } else if !pattern.contains('*') && !pattern.contains('?') && !pattern.contains('{') {
            // Bare directory name — treat as exact match
            (*pattern).clone()
        } else {
            (*pattern).clone()
        };

        // Walk directories matching the glob.
        // expand_workspace_glob already filters to dirs with package.json
        // and returns (original_path, canonical_path) — no redundant canonicalize().
        let matched_dirs = expand_workspace_glob(root, &glob_pattern, canonical_root);
        for (dir, canonical_dir) in matched_dirs {
            // Skip workspace entries that point to the project root itself
            // (e.g. pnpm-workspace.yaml listing `.` as a workspace)
            if canonical_dir == *canonical_root {
                continue;
            }

            // Check against negation patterns — skip directories that match any negated pattern
            let relative = dir.strip_prefix(root).unwrap_or(&dir);
            let relative_str = relative.to_string_lossy();
            if negation_matchers
                .iter()
                .any(|m| m.is_match(relative_str.as_ref()))
            {
                continue;
            }

            // package.json existence already checked in expand_workspace_glob
            let ws_pkg_path = dir.join("package.json");
            if let Ok(pkg) = PackageJson::load(&ws_pkg_path) {
                // Collect dependency names during initial load to avoid
                // re-reading package.json later.
                let dep_names = pkg.all_dependency_names();
                let name = pkg.name.unwrap_or_else(|| {
                    dir.file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_default()
                });
                workspaces.push((
                    WorkspaceInfo {
                        root: dir,
                        name,
                        is_internal_dependency: false,
                    },
                    dep_names,
                ));
            }
        }
    }

    workspaces
}

/// Discover workspaces from TypeScript project references in `tsconfig.json`.
///
/// Referenced directories are added as workspaces, supplementing npm/pnpm workspaces.
/// This enables cross-workspace resolution for TypeScript composite projects.
fn collect_tsconfig_workspaces(
    root: &Path,
    canonical_root: &Path,
) -> Vec<(WorkspaceInfo, Vec<String>)> {
    let mut workspaces = Vec::new();

    for dir in parse_tsconfig_references(root) {
        let canonical_dir = dunce::canonicalize(&dir).unwrap_or_else(|_| dir.clone());
        // Security: skip references pointing to project root or outside it
        if canonical_dir == *canonical_root || !canonical_dir.starts_with(canonical_root) {
            continue;
        }

        // Read package.json if available; otherwise use directory name
        let ws_pkg_path = dir.join("package.json");
        let (name, dep_names) = if ws_pkg_path.exists() {
            if let Ok(pkg) = PackageJson::load(&ws_pkg_path) {
                let deps = pkg.all_dependency_names();
                let n = pkg.name.unwrap_or_else(|| dir_name(&dir));
                (n, deps)
            } else {
                (dir_name(&dir), Vec::new())
            }
        } else {
            // No package.json — use directory name, no deps.
            // Valid for TypeScript-only composite projects.
            (dir_name(&dir), Vec::new())
        };

        workspaces.push((
            WorkspaceInfo {
                root: dir,
                name,
                is_internal_dependency: false,
            },
            dep_names,
        ));
    }

    workspaces
}

/// Discover shallow package workspaces when no explicit workspace config exists.
///
/// Scans direct children of the project root and their immediate children for
/// `package.json` files. This catches repos that contain multiple standalone
/// packages (for example `benchmarks/` or `editors/vscode/`) without declaring
/// npm/pnpm workspaces at the root.
fn collect_shallow_package_workspaces(
    root: &Path,
    canonical_root: &Path,
) -> Vec<(WorkspaceInfo, Vec<String>)> {
    let mut workspaces = Vec::new();
    let Ok(top_entries) = std::fs::read_dir(root) else {
        return workspaces;
    };

    for entry in top_entries.filter_map(Result::ok) {
        let path = entry.path();
        if !path.is_dir() || should_skip_workspace_scan_dir(&entry.file_name().to_string_lossy()) {
            continue;
        }

        collect_shallow_workspace_candidate(&path, canonical_root, &mut workspaces);

        let Ok(child_entries) = std::fs::read_dir(&path) else {
            continue;
        };
        for child in child_entries.filter_map(Result::ok) {
            let child_path = child.path();
            if !child_path.is_dir()
                || should_skip_workspace_scan_dir(&child.file_name().to_string_lossy())
            {
                continue;
            }

            collect_shallow_workspace_candidate(&child_path, canonical_root, &mut workspaces);
        }
    }

    workspaces
}

fn collect_shallow_workspace_candidate(
    dir: &Path,
    canonical_root: &Path,
    workspaces: &mut Vec<(WorkspaceInfo, Vec<String>)>,
) {
    let pkg_path = dir.join("package.json");
    if !pkg_path.exists() {
        return;
    }

    let canonical_dir = dunce::canonicalize(dir).unwrap_or_else(|_| dir.to_path_buf());
    if canonical_dir == *canonical_root || !canonical_dir.starts_with(canonical_root) {
        return;
    }

    let Ok(pkg) = PackageJson::load(&pkg_path) else {
        return;
    };
    let dep_names = pkg.all_dependency_names();
    let name = pkg.name.unwrap_or_else(|| dir_name(dir));

    workspaces.push((
        WorkspaceInfo {
            root: dir.to_path_buf(),
            name,
            is_internal_dependency: false,
        },
        dep_names,
    ));
}

fn should_skip_workspace_scan_dir(name: &str) -> bool {
    name.starts_with('.') || name == "node_modules" || name == "build"
}

/// Deduplicate workspaces by canonical path and mark internal dependencies.
///
/// Overlapping sources (npm workspaces + tsconfig references pointing to the same
/// directory) are collapsed. npm-discovered entries take precedence (they appear first).
/// Workspaces depended on by other workspaces are marked as `is_internal_dependency`.
fn mark_internal_dependencies(workspaces: &mut Vec<(WorkspaceInfo, Vec<String>)>) {
    // Deduplicate by canonical path
    {
        let mut seen = rustc_hash::FxHashSet::default();
        workspaces.retain(|(ws, _)| {
            let canonical = dunce::canonicalize(&ws.root).unwrap_or_else(|_| ws.root.clone());
            seen.insert(canonical)
        });
    }

    // Mark workspaces that are depended on by other workspaces.
    // Uses dep names collected during initial package.json load
    // to avoid re-reading all workspace package.json files.
    let all_dep_names: rustc_hash::FxHashSet<String> = workspaces
        .iter()
        .flat_map(|(_, deps)| deps.iter().cloned())
        .collect();
    for (ws, _) in &mut *workspaces {
        ws.is_internal_dependency = all_dep_names.contains(&ws.name);
    }
}

/// Extract the directory name as a string, for workspace name fallback.
fn dir_name(dir: &Path) -> String {
    dir.file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn discover_workspaces_from_tsconfig_references() {
        let temp_dir = std::env::temp_dir().join("fallow-test-ws-tsconfig-refs");
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(temp_dir.join("packages/core")).unwrap();
        std::fs::create_dir_all(temp_dir.join("packages/ui")).unwrap();

        // No package.json workspaces — only tsconfig references
        std::fs::write(
            temp_dir.join("tsconfig.json"),
            r#"{"references": [{"path": "./packages/core"}, {"path": "./packages/ui"}]}"#,
        )
        .unwrap();

        // core has package.json with a name
        std::fs::write(
            temp_dir.join("packages/core/package.json"),
            r#"{"name": "@project/core"}"#,
        )
        .unwrap();

        // ui has NO package.json — name should fall back to directory name
        let workspaces = discover_workspaces(&temp_dir);
        assert_eq!(workspaces.len(), 2);
        assert!(workspaces.iter().any(|ws| ws.name == "@project/core"));
        assert!(workspaces.iter().any(|ws| ws.name == "ui"));

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn tsconfig_references_outside_root_rejected() {
        let temp_dir = std::env::temp_dir().join("fallow-test-tsconfig-outside");
        let _ = std::fs::remove_dir_all(&temp_dir);
        std::fs::create_dir_all(temp_dir.join("project/packages/core")).unwrap();
        // "outside" is a sibling of "project", not inside it
        std::fs::create_dir_all(temp_dir.join("outside")).unwrap();

        std::fs::write(
            temp_dir.join("project/tsconfig.json"),
            r#"{"references": [{"path": "./packages/core"}, {"path": "../outside"}]}"#,
        )
        .unwrap();

        // Security: "../outside" points outside the project root and should be rejected
        let workspaces = discover_workspaces(&temp_dir.join("project"));
        assert_eq!(
            workspaces.len(),
            1,
            "reference outside project root should be rejected: {workspaces:?}"
        );
        assert!(
            workspaces[0]
                .root
                .to_string_lossy()
                .contains("packages/core")
        );

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    // ── dir_name ────────────────────────────────────────────────────

    #[test]
    fn dir_name_extracts_last_component() {
        assert_eq!(dir_name(Path::new("/project/packages/core")), "core");
        assert_eq!(dir_name(Path::new("/my-app")), "my-app");
    }

    #[test]
    fn dir_name_empty_for_root_path() {
        // Root path has no file_name component
        assert_eq!(dir_name(Path::new("/")), "");
    }

    // ── WorkspaceConfig deserialization ──────────────────────────────

    #[test]
    fn workspace_config_deserialize_json() {
        let json = r#"{"patterns": ["packages/*", "apps/*"]}"#;
        let config: WorkspaceConfig = serde_json::from_str(json).unwrap();
        assert_eq!(config.patterns, vec!["packages/*", "apps/*"]);
    }

    #[test]
    fn workspace_config_deserialize_empty_patterns() {
        let json = r#"{"patterns": []}"#;
        let config: WorkspaceConfig = serde_json::from_str(json).unwrap();
        assert!(config.patterns.is_empty());
    }

    #[test]
    fn workspace_config_default_patterns() {
        let json = "{}";
        let config: WorkspaceConfig = serde_json::from_str(json).unwrap();
        assert!(config.patterns.is_empty());
    }

    // ── WorkspaceInfo ───────────────────────────────────────────────

    #[test]
    fn workspace_info_default_not_internal() {
        let ws = WorkspaceInfo {
            root: PathBuf::from("/project/packages/a"),
            name: "a".to_string(),
            is_internal_dependency: false,
        };
        assert!(!ws.is_internal_dependency);
    }

    // ── mark_internal_dependencies ──────────────────────────────────

    #[test]
    fn mark_internal_deps_detects_cross_references() {
        let temp_dir = tempfile::tempdir().expect("create temp dir");
        let pkg_a = temp_dir.path().join("a");
        let pkg_b = temp_dir.path().join("b");
        std::fs::create_dir_all(&pkg_a).unwrap();
        std::fs::create_dir_all(&pkg_b).unwrap();

        let mut workspaces = vec![
            (
                WorkspaceInfo {
                    root: pkg_a,
                    name: "@scope/a".to_string(),
                    is_internal_dependency: false,
                },
                vec!["@scope/b".to_string()], // "a" depends on "b"
            ),
            (
                WorkspaceInfo {
                    root: pkg_b,
                    name: "@scope/b".to_string(),
                    is_internal_dependency: false,
                },
                vec!["lodash".to_string()], // "b" depends on external only
            ),
        ];

        mark_internal_dependencies(&mut workspaces);

        // "b" is depended on by "a", so it should be marked as internal
        let ws_a = workspaces
            .iter()
            .find(|(ws, _)| ws.name == "@scope/a")
            .unwrap();
        assert!(
            !ws_a.0.is_internal_dependency,
            "a is not depended on by others"
        );

        let ws_b = workspaces
            .iter()
            .find(|(ws, _)| ws.name == "@scope/b")
            .unwrap();
        assert!(ws_b.0.is_internal_dependency, "b is depended on by a");
    }

    #[test]
    fn mark_internal_deps_no_cross_references() {
        let temp_dir = tempfile::tempdir().expect("create temp dir");
        let pkg_a = temp_dir.path().join("a");
        let pkg_b = temp_dir.path().join("b");
        std::fs::create_dir_all(&pkg_a).unwrap();
        std::fs::create_dir_all(&pkg_b).unwrap();

        let mut workspaces = vec![
            (
                WorkspaceInfo {
                    root: pkg_a,
                    name: "a".to_string(),
                    is_internal_dependency: false,
                },
                vec!["react".to_string()],
            ),
            (
                WorkspaceInfo {
                    root: pkg_b,
                    name: "b".to_string(),
                    is_internal_dependency: false,
                },
                vec!["lodash".to_string()],
            ),
        ];

        mark_internal_dependencies(&mut workspaces);

        assert!(!workspaces[0].0.is_internal_dependency);
        assert!(!workspaces[1].0.is_internal_dependency);
    }

    #[test]
    fn mark_internal_deps_deduplicates_by_path() {
        let temp_dir = tempfile::tempdir().expect("create temp dir");
        let pkg_a = temp_dir.path().join("a");
        std::fs::create_dir_all(&pkg_a).unwrap();

        let mut workspaces = vec![
            (
                WorkspaceInfo {
                    root: pkg_a.clone(),
                    name: "a".to_string(),
                    is_internal_dependency: false,
                },
                vec![],
            ),
            (
                WorkspaceInfo {
                    root: pkg_a,
                    name: "a".to_string(),
                    is_internal_dependency: false,
                },
                vec![],
            ),
        ];

        mark_internal_dependencies(&mut workspaces);
        assert_eq!(
            workspaces.len(),
            1,
            "duplicate paths should be deduplicated"
        );
    }

    // ── collect_workspace_patterns ──────────────────────────────────

    #[test]
    fn collect_patterns_from_package_json() {
        let dir = tempfile::tempdir().expect("create temp dir");
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"workspaces": ["packages/*", "apps/*"]}"#,
        )
        .unwrap();

        let patterns = collect_workspace_patterns(dir.path());
        assert_eq!(patterns, vec!["packages/*", "apps/*"]);
    }

    #[test]
    fn collect_patterns_from_pnpm_workspace() {
        let dir = tempfile::tempdir().expect("create temp dir");
        std::fs::write(
            dir.path().join("pnpm-workspace.yaml"),
            "packages:\n  - 'packages/*'\n  - 'libs/*'\n",
        )
        .unwrap();

        let patterns = collect_workspace_patterns(dir.path());
        assert_eq!(patterns, vec!["packages/*", "libs/*"]);
    }

    #[test]
    fn collect_patterns_combines_sources() {
        let dir = tempfile::tempdir().expect("create temp dir");
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"workspaces": ["packages/*"]}"#,
        )
        .unwrap();
        std::fs::write(
            dir.path().join("pnpm-workspace.yaml"),
            "packages:\n  - 'apps/*'\n",
        )
        .unwrap();

        let patterns = collect_workspace_patterns(dir.path());
        assert!(patterns.contains(&"packages/*".to_string()));
        assert!(patterns.contains(&"apps/*".to_string()));
    }

    #[test]
    fn collect_patterns_empty_when_no_configs() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let patterns = collect_workspace_patterns(dir.path());
        assert!(patterns.is_empty());
    }

    // ── discover_workspaces integration ─────────────────────────────

    #[test]
    fn discover_workspaces_from_package_json() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let pkg_a = dir.path().join("packages").join("a");
        let pkg_b = dir.path().join("packages").join("b");
        std::fs::create_dir_all(&pkg_a).unwrap();
        std::fs::create_dir_all(&pkg_b).unwrap();

        std::fs::write(
            dir.path().join("package.json"),
            r#"{"workspaces": ["packages/*"]}"#,
        )
        .unwrap();
        std::fs::write(
            pkg_a.join("package.json"),
            r#"{"name": "@test/a", "dependencies": {"@test/b": "workspace:*"}}"#,
        )
        .unwrap();
        std::fs::write(pkg_b.join("package.json"), r#"{"name": "@test/b"}"#).unwrap();

        let workspaces = discover_workspaces(dir.path());
        assert_eq!(workspaces.len(), 2);

        let ws_a = workspaces.iter().find(|ws| ws.name == "@test/a").unwrap();
        assert!(!ws_a.is_internal_dependency);

        let ws_b = workspaces.iter().find(|ws| ws.name == "@test/b").unwrap();
        assert!(ws_b.is_internal_dependency, "b is depended on by a");
    }

    #[test]
    fn discover_workspaces_empty_project() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let workspaces = discover_workspaces(dir.path());
        assert!(workspaces.is_empty());
    }

    #[test]
    fn discover_workspaces_falls_back_to_shallow_packages_without_workspace_config() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let benchmarks = dir.path().join("benchmarks");
        let vscode = dir.path().join("editors").join("vscode");
        let deep = dir.path().join("tests").join("fixtures").join("demo");
        std::fs::create_dir_all(&benchmarks).unwrap();
        std::fs::create_dir_all(&vscode).unwrap();
        std::fs::create_dir_all(&deep).unwrap();

        std::fs::write(benchmarks.join("package.json"), r#"{"name": "benchmarks"}"#).unwrap();
        std::fs::write(vscode.join("package.json"), r#"{"name": "fallow-vscode"}"#).unwrap();
        std::fs::write(deep.join("package.json"), r#"{"name": "deep-fixture"}"#).unwrap();

        let workspaces = discover_workspaces(dir.path());
        let names: Vec<&str> = workspaces.iter().map(|ws| ws.name.as_str()).collect();

        assert!(
            names.contains(&"benchmarks"),
            "top-level nested package should be discovered: {workspaces:?}"
        );
        assert!(
            names.contains(&"fallow-vscode"),
            "second-level nested package should be discovered: {workspaces:?}"
        );
        assert!(
            !names.contains(&"deep-fixture"),
            "fallback should stay shallow and skip deep fixtures: {workspaces:?}"
        );
    }

    #[test]
    fn discover_workspaces_with_negated_patterns() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let pkg_a = dir.path().join("packages").join("a");
        let pkg_test = dir.path().join("packages").join("test-utils");
        std::fs::create_dir_all(&pkg_a).unwrap();
        std::fs::create_dir_all(&pkg_test).unwrap();

        std::fs::write(
            dir.path().join("package.json"),
            r#"{"workspaces": ["packages/*", "!packages/test-*"]}"#,
        )
        .unwrap();
        std::fs::write(pkg_a.join("package.json"), r#"{"name": "a"}"#).unwrap();
        std::fs::write(pkg_test.join("package.json"), r#"{"name": "test-utils"}"#).unwrap();

        let workspaces = discover_workspaces(dir.path());
        assert_eq!(workspaces.len(), 1);
        assert_eq!(workspaces[0].name, "a");
    }

    #[test]
    fn discover_workspaces_skips_root_as_workspace() {
        let dir = tempfile::tempdir().expect("create temp dir");
        // pnpm-workspace.yaml listing "." should not add root as workspace
        std::fs::write(
            dir.path().join("pnpm-workspace.yaml"),
            "packages:\n  - '.'\n",
        )
        .unwrap();
        std::fs::write(dir.path().join("package.json"), r#"{"name": "root"}"#).unwrap();

        let workspaces = discover_workspaces(dir.path());
        assert!(
            workspaces.is_empty(),
            "root directory should not be added as workspace"
        );
    }

    #[test]
    fn discover_workspaces_name_fallback_to_dir_name() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let pkg_a = dir.path().join("packages").join("my-app");
        std::fs::create_dir_all(&pkg_a).unwrap();

        std::fs::write(
            dir.path().join("package.json"),
            r#"{"workspaces": ["packages/*"]}"#,
        )
        .unwrap();
        // package.json without a name field
        std::fs::write(pkg_a.join("package.json"), "{}").unwrap();

        let workspaces = discover_workspaces(dir.path());
        assert_eq!(workspaces.len(), 1);
        assert_eq!(workspaces[0].name, "my-app", "should fall back to dir name");
    }

    #[test]
    fn discover_workspaces_explicit_patterns_disable_shallow_fallback() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let pkg_a = dir.path().join("packages").join("a");
        let benchmarks = dir.path().join("benchmarks");
        std::fs::create_dir_all(&pkg_a).unwrap();
        std::fs::create_dir_all(&benchmarks).unwrap();

        std::fs::write(
            dir.path().join("package.json"),
            r#"{"workspaces": ["packages/*"]}"#,
        )
        .unwrap();
        std::fs::write(pkg_a.join("package.json"), r#"{"name": "a"}"#).unwrap();
        std::fs::write(benchmarks.join("package.json"), r#"{"name": "benchmarks"}"#).unwrap();

        let workspaces = discover_workspaces(dir.path());
        let names: Vec<&str> = workspaces.iter().map(|ws| ws.name.as_str()).collect();

        assert_eq!(workspaces.len(), 1);
        assert!(names.contains(&"a"));
        assert!(
            !names.contains(&"benchmarks"),
            "explicit workspace config should keep undeclared packages out: {workspaces:?}"
        );
    }

    // ── find_undeclared_workspaces ─────────────────────────────────

    #[test]
    fn undeclared_workspace_detected() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let pkg_a = dir.path().join("packages").join("a");
        let pkg_b = dir.path().join("packages").join("b");
        std::fs::create_dir_all(&pkg_a).unwrap();
        std::fs::create_dir_all(&pkg_b).unwrap();

        // Only packages/a is declared as a workspace
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"workspaces": ["packages/a"]}"#,
        )
        .unwrap();
        std::fs::write(pkg_a.join("package.json"), r#"{"name": "a"}"#).unwrap();
        std::fs::write(pkg_b.join("package.json"), r#"{"name": "b"}"#).unwrap();

        let declared = discover_workspaces(dir.path());
        assert_eq!(declared.len(), 1);

        let undeclared = find_undeclared_workspaces(dir.path(), &declared);
        assert_eq!(undeclared.len(), 1);
        assert!(
            undeclared[0]
                .path
                .to_string_lossy()
                .replace('\\', "/")
                .contains("packages/b"),
            "should detect packages/b as undeclared: {:?}",
            undeclared[0].path
        );
    }

    #[test]
    fn no_undeclared_when_all_covered() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let pkg_a = dir.path().join("packages").join("a");
        std::fs::create_dir_all(&pkg_a).unwrap();

        std::fs::write(
            dir.path().join("package.json"),
            r#"{"workspaces": ["packages/*"]}"#,
        )
        .unwrap();
        std::fs::write(pkg_a.join("package.json"), r#"{"name": "a"}"#).unwrap();

        let declared = discover_workspaces(dir.path());
        let undeclared = find_undeclared_workspaces(dir.path(), &declared);
        assert!(undeclared.is_empty());
    }

    #[test]
    fn no_undeclared_when_no_workspace_patterns() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let sub = dir.path().join("lib");
        std::fs::create_dir_all(&sub).unwrap();

        // No workspaces field at all, non-monorepo project
        std::fs::write(dir.path().join("package.json"), r#"{"name": "app"}"#).unwrap();
        std::fs::write(sub.join("package.json"), r#"{"name": "lib"}"#).unwrap();

        let undeclared = find_undeclared_workspaces(dir.path(), &[]);
        assert!(
            undeclared.is_empty(),
            "should skip check when no workspace patterns exist"
        );
    }

    #[test]
    fn undeclared_skips_node_modules_and_hidden_dirs() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let nm = dir.path().join("node_modules").join("some-pkg");
        let hidden = dir.path().join(".hidden");
        std::fs::create_dir_all(&nm).unwrap();
        std::fs::create_dir_all(&hidden).unwrap();

        std::fs::write(
            dir.path().join("package.json"),
            r#"{"workspaces": ["packages/*"]}"#,
        )
        .unwrap();
        // Put package.json in node_modules and hidden dirs
        std::fs::write(nm.join("package.json"), r#"{"name": "nm-pkg"}"#).unwrap();
        std::fs::write(hidden.join("package.json"), r#"{"name": "hidden"}"#).unwrap();

        let undeclared = find_undeclared_workspaces(dir.path(), &[]);
        assert!(
            undeclared.is_empty(),
            "should not flag node_modules or hidden directories"
        );
    }

    fn build_globset(patterns: &[&str]) -> globset::GlobSet {
        let mut builder = globset::GlobSetBuilder::new();
        for pattern in patterns {
            builder.add(globset::Glob::new(pattern).expect("valid glob"));
        }
        builder.build().expect("build globset")
    }

    #[test]
    fn undeclared_skips_dirs_matching_ignore_patterns() {
        // Reproduces issue #193: a `references/*` directory containing package.json
        // should not be reported as undeclared workspace when listed in ignorePatterns.
        let dir = tempfile::tempdir().expect("create temp dir");
        let pkg_a = dir.path().join("packages").join("a");
        let vitest_ref = dir.path().join("references").join("vitest");
        let tanstack_ref = dir.path().join("references").join("tanstack-router");
        std::fs::create_dir_all(&pkg_a).unwrap();
        std::fs::create_dir_all(&vitest_ref).unwrap();
        std::fs::create_dir_all(&tanstack_ref).unwrap();

        std::fs::write(
            dir.path().join("package.json"),
            r#"{"workspaces": ["packages/*"]}"#,
        )
        .unwrap();
        std::fs::write(pkg_a.join("package.json"), r#"{"name": "a"}"#).unwrap();
        std::fs::write(
            vitest_ref.join("package.json"),
            r#"{"name": "vitest-reference"}"#,
        )
        .unwrap();
        std::fs::write(
            tanstack_ref.join("package.json"),
            r#"{"name": "tanstack-reference"}"#,
        )
        .unwrap();

        let declared = discover_workspaces(dir.path());
        let ignore = build_globset(&["references/*"]);
        let undeclared = find_undeclared_workspaces_with_ignores(dir.path(), &declared, &ignore);
        assert!(
            undeclared.is_empty(),
            "references/* should be ignored: {undeclared:?}"
        );
    }

    #[test]
    fn undeclared_still_reported_when_ignore_does_not_match() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let pkg_b = dir.path().join("packages").join("b");
        std::fs::create_dir_all(&pkg_b).unwrap();

        std::fs::write(
            dir.path().join("package.json"),
            r#"{"workspaces": ["packages/a"]}"#,
        )
        .unwrap();
        std::fs::write(pkg_b.join("package.json"), r#"{"name": "b"}"#).unwrap();

        let declared = discover_workspaces(dir.path());
        // ignore pattern is unrelated to packages/b
        let ignore = build_globset(&["references/*"]);
        let undeclared = find_undeclared_workspaces_with_ignores(dir.path(), &declared, &ignore);
        assert_eq!(
            undeclared.len(),
            1,
            "non-matching ignore patterns should not silence other undeclared dirs"
        );
    }

    #[test]
    fn undeclared_skips_dirs_matching_package_json_glob() {
        // Some users write ignore patterns as `references/*/package.json`
        // (matching the file rather than the directory). Both styles should silence
        // the undeclared-workspace warning.
        let dir = tempfile::tempdir().expect("create temp dir");
        let pkg_a = dir.path().join("packages").join("a");
        let vitest_ref = dir.path().join("references").join("vitest");
        std::fs::create_dir_all(&pkg_a).unwrap();
        std::fs::create_dir_all(&vitest_ref).unwrap();

        std::fs::write(
            dir.path().join("package.json"),
            r#"{"workspaces": ["packages/*"]}"#,
        )
        .unwrap();
        std::fs::write(pkg_a.join("package.json"), r#"{"name": "a"}"#).unwrap();
        std::fs::write(
            vitest_ref.join("package.json"),
            r#"{"name": "vitest-reference"}"#,
        )
        .unwrap();

        let declared = discover_workspaces(dir.path());
        let ignore = build_globset(&["references/*/package.json"]);
        let undeclared = find_undeclared_workspaces_with_ignores(dir.path(), &declared, &ignore);
        assert!(
            undeclared.is_empty(),
            "package.json-suffixed glob should silence the warning: {undeclared:?}"
        );
    }

    #[test]
    fn undeclared_skips_dirs_matching_doublestar_ignore() {
        // `references/**` should also cover `references/<name>` candidates.
        let dir = tempfile::tempdir().expect("create temp dir");
        let pkg_a = dir.path().join("packages").join("a");
        let nested_ref = dir.path().join("references").join("vitest");
        std::fs::create_dir_all(&pkg_a).unwrap();
        std::fs::create_dir_all(&nested_ref).unwrap();

        std::fs::write(
            dir.path().join("package.json"),
            r#"{"workspaces": ["packages/*"]}"#,
        )
        .unwrap();
        std::fs::write(pkg_a.join("package.json"), r#"{"name": "a"}"#).unwrap();
        std::fs::write(
            nested_ref.join("package.json"),
            r#"{"name": "vitest-reference"}"#,
        )
        .unwrap();

        let declared = discover_workspaces(dir.path());
        let ignore = build_globset(&["**/references/**"]);
        let undeclared = find_undeclared_workspaces_with_ignores(dir.path(), &declared, &ignore);
        assert!(
            undeclared.is_empty(),
            "**/references/** should ignore nested package.json dirs: {undeclared:?}"
        );
    }
}
