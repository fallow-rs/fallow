mod entry_points;
mod infrastructure;
mod parse_scripts;
mod walk;

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
pub(crate) use walk::{HiddenDirScope, discover_files_with_additional_hidden_dirs};
pub use walk::{PRODUCTION_EXCLUDE_PATTERNS, SOURCE_EXTENSIONS, discover_files};

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
