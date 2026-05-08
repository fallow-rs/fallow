mod entry_points;
mod infrastructure;
mod parse_scripts;
mod walk;

use std::path::Path;

use fallow_config::{PackageJson, ResolvedConfig};

// Re-export types from fallow-types
pub use fallow_types::discover::{DiscoveredFile, EntryPoint, EntryPointSource, FileId};

// Re-export public functions — preserves the existing `crate::discover::*` API
pub use entry_points::{
    CategorizedEntryPoints, compile_glob_set, discover_dynamically_loaded_entry_points,
    discover_entry_points, discover_plugin_entry_point_sets, discover_plugin_entry_points,
    discover_workspace_entry_points,
};
pub(crate) use entry_points::{
    EntryPointDiscovery, discover_entry_points_with_warnings_from_pkg,
    discover_workspace_entry_points_with_warnings_from_pkg, warn_skipped_entry_summary,
};
pub use infrastructure::discover_infrastructure_entry_points;
pub use walk::{
    HiddenDirScope, PRODUCTION_EXCLUDE_PATTERNS, SOURCE_EXTENSIONS, discover_files,
    discover_files_with_additional_hidden_dirs,
};

/// Collect package-scoped hidden directory traversal rules for active plugins.
///
/// Source discovery runs before full plugin execution, so this consults
/// package-activation checks and static plugin metadata only. Callers that have
/// already loaded the root `package.json` and discovered workspaces should pass
/// them in to avoid redoing the work; standalone CLI command paths can use
/// [`discover_files_with_plugin_scopes`] instead.
#[must_use]
pub fn collect_plugin_hidden_dir_scopes(
    config: &ResolvedConfig,
    root_pkg: Option<&PackageJson>,
    workspaces: &[fallow_config::WorkspaceInfo],
) -> Vec<HiddenDirScope> {
    let registry = crate::plugins::PluginRegistry::new(config.external_plugins.clone());
    let mut scopes = Vec::new();

    if let Some(pkg) = root_pkg {
        push_plugin_hidden_dir_scope(&mut scopes, &registry, pkg, &config.root);
    }

    for ws in workspaces {
        if let Ok(pkg) = PackageJson::load(&ws.root.join("package.json")) {
            push_plugin_hidden_dir_scope(&mut scopes, &registry, &pkg, &ws.root);
        }
    }

    scopes
}

fn push_plugin_hidden_dir_scope(
    scopes: &mut Vec<HiddenDirScope>,
    registry: &crate::plugins::PluginRegistry,
    pkg: &PackageJson,
    root: &Path,
) {
    let dirs = registry.discovery_hidden_dirs(pkg, root);
    if !dirs.is_empty() {
        scopes.push(HiddenDirScope::new(root.to_path_buf(), dirs));
    }
}

/// Discover files with plugin-aware hidden directory traversal.
///
/// Convenience wrapper for command paths (list, dupes, health, flags, coverage)
/// that don't already have workspaces / root `package.json` on hand. Internally
/// loads the root `package.json` and discovers workspaces so plugin-contributed
/// hidden directories (e.g. React Router's `.client` / `.server` folders) are
/// traversed consistently across every command.
#[must_use]
pub fn discover_files_with_plugin_scopes(config: &ResolvedConfig) -> Vec<DiscoveredFile> {
    let root_pkg = PackageJson::load(&config.root.join("package.json")).ok();
    let workspaces = fallow_config::discover_workspaces(&config.root);
    let scopes = collect_plugin_hidden_dir_scopes(config, root_pkg.as_ref(), &workspaces);
    discover_files_with_additional_hidden_dirs(config, &scopes)
}

/// Hidden (dot-prefixed) directories that should be included in file discovery.
///
/// Most hidden directories (`.git`, `.cache`, etc.) should be skipped, but certain
/// convention directories contain source or config files that fallow needs to see:
/// - `.storybook` — Storybook configuration (the Storybook plugin depends on this)
/// - `.vitepress` — VitePress configuration and theme files
/// - `.well-known` — Standard web convention directory
/// - `.changeset` — Changesets configuration
/// - `.github` — GitHub workflows and CI scripts
const ALLOWED_HIDDEN_DIRS: &[&str] = &[
    ".storybook",
    ".vitepress",
    ".well-known",
    ".changeset",
    ".github",
];

#[cfg(test)]
mod tests {
    use super::*;

    // ── ALLOWED_HIDDEN_DIRS exhaustiveness ───────────────────────────

    #[test]
    fn allowed_hidden_dirs_count() {
        // Guard: if a new dir is added, add a test for it
        assert_eq!(
            ALLOWED_HIDDEN_DIRS.len(),
            5,
            "update tests when adding new allowed hidden dirs"
        );
    }

    #[test]
    fn allowed_hidden_dirs_all_start_with_dot() {
        for dir in ALLOWED_HIDDEN_DIRS {
            assert!(
                dir.starts_with('.'),
                "allowed hidden dir '{dir}' must start with '.'"
            );
        }
    }

    #[test]
    fn allowed_hidden_dirs_no_duplicates() {
        let mut seen = rustc_hash::FxHashSet::default();
        for dir in ALLOWED_HIDDEN_DIRS {
            assert!(seen.insert(*dir), "duplicate allowed hidden dir: {dir}");
        }
    }

    #[test]
    fn allowed_hidden_dirs_no_trailing_slash() {
        for dir in ALLOWED_HIDDEN_DIRS {
            assert!(
                !dir.ends_with('/'),
                "allowed hidden dir '{dir}' should not have trailing slash"
            );
        }
    }

    // ── Re-export smoke tests ───────────────────────────────────────

    #[test]
    fn file_id_re_exported() {
        // Verify the re-export works by constructing a FileId through the discover module
        let id = FileId(42);
        assert_eq!(id.0, 42);
    }

    #[test]
    fn source_extensions_re_exported() {
        assert!(SOURCE_EXTENSIONS.contains(&"ts"));
        assert!(SOURCE_EXTENSIONS.contains(&"tsx"));
    }

    #[test]
    fn compile_glob_set_re_exported() {
        let result = compile_glob_set(&["**/*.ts".to_string()]);
        assert!(result.is_some());
    }
}
