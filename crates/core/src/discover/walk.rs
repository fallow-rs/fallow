use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

use fallow_config::ResolvedConfig;
use fallow_types::discover::{DiscoveredFile, FileId};
use ignore::WalkBuilder;

use super::ALLOWED_HIDDEN_DIRS;

/// Package-scoped hidden directories that source discovery should traverse.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HiddenDirScope {
    root: PathBuf,
    dirs: Vec<String>,
}

impl HiddenDirScope {
    pub fn new(root: PathBuf, dirs: Vec<String>) -> Self {
        Self { root, dirs }
    }

    fn allows(&self, path: &Path, name: &OsStr) -> bool {
        path.starts_with(&self.root) && self.dirs.iter().any(|dir| OsStr::new(dir) == name)
    }
}

/// Per-thread file collector for the parallel walker.
struct FileVisitor<'a> {
    root: &'a Path,
    ignore_patterns: &'a globset::GlobSet,
    production_excludes: &'a Option<globset::GlobSet>,
    shared: &'a Mutex<Vec<(std::path::PathBuf, u64)>>,
    local: Vec<(std::path::PathBuf, u64)>,
}

impl ignore::ParallelVisitor for FileVisitor<'_> {
    fn visit(&mut self, result: Result<ignore::DirEntry, ignore::Error>) -> ignore::WalkState {
        let Ok(entry) = result else {
            return ignore::WalkState::Continue;
        };
        if entry.file_type().is_some_and(|ft| ft.is_dir()) {
            return ignore::WalkState::Continue;
        }
        let relative = entry
            .path()
            .strip_prefix(self.root)
            .unwrap_or_else(|_| entry.path());
        if self.ignore_patterns.is_match(relative) {
            return ignore::WalkState::Continue;
        }
        if self
            .production_excludes
            .as_ref()
            .is_some_and(|excludes| excludes.is_match(relative))
        {
            return ignore::WalkState::Continue;
        }
        let size_bytes = entry.metadata().map_or(0, |m| m.len());
        self.local.push((entry.into_path(), size_bytes));
        ignore::WalkState::Continue
    }
}

impl Drop for FileVisitor<'_> {
    fn drop(&mut self) {
        if !self.local.is_empty() {
            self.shared
                .lock()
                .expect("walk collector lock poisoned")
                .append(&mut self.local);
        }
    }
}

/// Builder that creates per-thread `FileVisitor` instances for the parallel walker.
struct FileVisitorBuilder<'a> {
    root: &'a Path,
    ignore_patterns: &'a globset::GlobSet,
    production_excludes: &'a Option<globset::GlobSet>,
    shared: &'a Mutex<Vec<(std::path::PathBuf, u64)>>,
}

impl<'s> ignore::ParallelVisitorBuilder<'s> for FileVisitorBuilder<'s> {
    fn build(&mut self) -> Box<dyn ignore::ParallelVisitor + 's> {
        Box::new(FileVisitor {
            root: self.root,
            ignore_patterns: self.ignore_patterns,
            production_excludes: self.production_excludes,
            shared: self.shared,
            local: Vec::new(),
        })
    }
}

pub const SOURCE_EXTENSIONS: &[&str] = &[
    "ts", "tsx", "mts", "cts", "gts", "js", "jsx", "mjs", "cjs", "gjs", "vue", "svelte", "astro",
    "mdx", "css", "scss", "html", "graphql", "gql",
];

/// Glob patterns for test/dev/story files excluded in production mode.
pub const PRODUCTION_EXCLUDE_PATTERNS: &[&str] = &[
    // Test files
    "**/*.test.*",
    "**/*.spec.*",
    "**/*.e2e.*",
    "**/*.e2e-spec.*",
    "**/*.bench.*",
    "**/*.fixture.*",
    // Story files
    "**/*.stories.*",
    "**/*.story.*",
    // Test directories
    "**/__tests__/**",
    "**/__mocks__/**",
    "**/__snapshots__/**",
    "**/__fixtures__/**",
    "**/test/**",
    "**/tests/**",
    // Dev/config files at project root only (not nested src/ files like Angular's app.config.ts)
    "*.config.*",
    "**/.*.js",
    "**/.*.ts",
    "**/.*.mjs",
    "**/.*.cjs",
];

/// Check if a hidden directory name is on the allowlist.
pub fn is_allowed_hidden_dir(name: &OsStr) -> bool {
    ALLOWED_HIDDEN_DIRS.iter().any(|&d| OsStr::new(d) == name)
}

fn is_allowed_scoped_hidden_dir(
    name: &OsStr,
    path: &Path,
    additional_hidden_dir_scopes: &[HiddenDirScope],
) -> bool {
    additional_hidden_dir_scopes
        .iter()
        .any(|scope| scope.allows(path, name))
}

/// Check if a hidden directory entry should be allowed through the filter.
///
/// Returns `true` if the entry is not hidden or is on the allowlist.
/// Hidden files (not directories) are always allowed through since the type
/// filter handles them.
fn is_allowed_hidden(entry: &ignore::DirEntry) -> bool {
    is_allowed_hidden_with_scopes(entry, &[])
}

fn is_allowed_hidden_with_scopes(
    entry: &ignore::DirEntry,
    additional_hidden_dir_scopes: &[HiddenDirScope],
) -> bool {
    let name = entry.file_name();
    let name_str = name.to_string_lossy();

    // Not hidden — always allow
    if !name_str.starts_with('.') {
        return true;
    }

    // Hidden files are fine — the type filter (source extensions) will handle them
    if entry.file_type().is_some_and(|ft| !ft.is_dir()) {
        return true;
    }

    // Hidden directory — check against the allowlist
    is_allowed_hidden_dir(name)
        || is_allowed_scoped_hidden_dir(name, entry.path(), additional_hidden_dir_scopes)
}

/// Discover all source files in the project.
///
/// # Panics
///
/// Panics if the file type glob or progress template is invalid (compile-time constants).
pub fn discover_files(config: &ResolvedConfig) -> Vec<DiscoveredFile> {
    discover_files_with_additional_hidden_dirs(config, &[])
}

/// Discover all source files in the project, with package-scoped hidden dirs.
///
/// # Panics
///
/// Panics if the file type glob or progress template is invalid (compile-time constants).
#[expect(
    clippy::cast_possible_truncation,
    reason = "file count is bounded by project size, well under u32::MAX"
)]
pub fn discover_files_with_additional_hidden_dirs(
    config: &ResolvedConfig,
    additional_hidden_dir_scopes: &[HiddenDirScope],
) -> Vec<DiscoveredFile> {
    let _span = tracing::info_span!("discover_files").entered();

    let mut types_builder = ignore::types::TypesBuilder::new();
    for ext in SOURCE_EXTENSIONS {
        types_builder
            .add("source", &format!("*.{ext}"))
            .expect("valid glob");
    }
    types_builder.select("source");
    let types = types_builder.build().expect("valid types");

    let mut walk_builder = WalkBuilder::new(&config.root);
    walk_builder
        .hidden(false)
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true)
        .types(types)
        .threads(config.threads);
    if additional_hidden_dir_scopes.is_empty() {
        walk_builder.filter_entry(is_allowed_hidden);
    } else {
        let scopes = additional_hidden_dir_scopes.to_vec();
        walk_builder.filter_entry(move |entry| is_allowed_hidden_with_scopes(entry, &scopes));
    }

    // Build production exclude matcher if needed
    let production_excludes = if config.production {
        let mut builder = globset::GlobSetBuilder::new();
        for pattern in PRODUCTION_EXCLUDE_PATTERNS {
            if let Ok(glob) = globset::GlobBuilder::new(pattern)
                .literal_separator(true)
                .build()
            {
                builder.add(glob);
            }
        }
        builder.build().ok()
    } else {
        None
    };

    // Parallel filesystem walk — uses work-stealing across config.threads threads.
    // `build_parallel()` honors the `.threads()` setting (unlike sequential `build()`).
    // Each thread collects results into a local buffer, flushed on drop to avoid
    // per-entry Mutex contention.
    let collected: Mutex<Vec<(std::path::PathBuf, u64)>> = Mutex::new(Vec::new());
    let mut visitor_builder = FileVisitorBuilder {
        root: &config.root,
        ignore_patterns: &config.ignore_patterns,
        production_excludes: &production_excludes,
        shared: &collected,
    };
    walk_builder.build_parallel().visit(&mut visitor_builder);

    // Sort by path for stable, deterministic FileId assignment.
    // The same set of files always produces the same IDs regardless of file
    // size changes, which is the foundation for incremental analysis and
    // cross-run graph caching.
    let mut raw = collected
        .into_inner()
        .expect("walk collector lock poisoned");
    raw.sort_unstable_by(|a, b| a.0.cmp(&b.0));

    let files: Vec<DiscoveredFile> = raw
        .into_iter()
        .enumerate()
        .map(|(idx, (path, size_bytes))| DiscoveredFile {
            id: FileId(idx as u32),
            path,
            size_bytes,
        })
        .collect();

    files
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;

    use super::*;

    // is_allowed_hidden_dir tests
    #[test]
    fn allowed_hidden_dirs() {
        assert!(is_allowed_hidden_dir(OsStr::new(".storybook")));
        assert!(is_allowed_hidden_dir(OsStr::new(".vitepress")));
        assert!(is_allowed_hidden_dir(OsStr::new(".well-known")));
        assert!(is_allowed_hidden_dir(OsStr::new(".changeset")));
        assert!(is_allowed_hidden_dir(OsStr::new(".github")));
    }

    #[test]
    fn disallowed_hidden_dirs() {
        assert!(!is_allowed_hidden_dir(OsStr::new(".git")));
        assert!(!is_allowed_hidden_dir(OsStr::new(".cache")));
        assert!(!is_allowed_hidden_dir(OsStr::new(".vscode")));
        assert!(!is_allowed_hidden_dir(OsStr::new(".fallow")));
        assert!(!is_allowed_hidden_dir(OsStr::new(".next")));
    }

    #[test]
    fn non_hidden_dirs_not_in_allowlist() {
        // Non-hidden names should not match the allowlist (they are always allowed
        // by is_allowed_hidden because they don't start with '.')
        assert!(!is_allowed_hidden_dir(OsStr::new("src")));
        assert!(!is_allowed_hidden_dir(OsStr::new("node_modules")));
    }

    // SOURCE_EXTENSIONS tests
    #[test]
    fn source_extensions_include_typescript() {
        assert!(SOURCE_EXTENSIONS.contains(&"ts"));
        assert!(SOURCE_EXTENSIONS.contains(&"tsx"));
        assert!(SOURCE_EXTENSIONS.contains(&"mts"));
        assert!(SOURCE_EXTENSIONS.contains(&"cts"));
        assert!(SOURCE_EXTENSIONS.contains(&"gts"));
    }

    #[test]
    fn source_extensions_include_javascript() {
        assert!(SOURCE_EXTENSIONS.contains(&"js"));
        assert!(SOURCE_EXTENSIONS.contains(&"jsx"));
        assert!(SOURCE_EXTENSIONS.contains(&"mjs"));
        assert!(SOURCE_EXTENSIONS.contains(&"cjs"));
        assert!(SOURCE_EXTENSIONS.contains(&"gjs"));
    }

    #[test]
    fn source_extensions_include_sfc_formats() {
        assert!(SOURCE_EXTENSIONS.contains(&"vue"));
        assert!(SOURCE_EXTENSIONS.contains(&"svelte"));
        assert!(SOURCE_EXTENSIONS.contains(&"astro"));
    }

    #[test]
    fn source_extensions_include_styles() {
        assert!(SOURCE_EXTENSIONS.contains(&"css"));
        assert!(SOURCE_EXTENSIONS.contains(&"scss"));
    }

    #[test]
    fn source_extensions_exclude_non_source() {
        assert!(!SOURCE_EXTENSIONS.contains(&"json"));
        assert!(!SOURCE_EXTENSIONS.contains(&"yaml"));
        assert!(!SOURCE_EXTENSIONS.contains(&"md"));
        assert!(!SOURCE_EXTENSIONS.contains(&"png"));
        assert!(!SOURCE_EXTENSIONS.contains(&"htm"));
    }

    #[test]
    fn source_extensions_include_html() {
        assert!(SOURCE_EXTENSIONS.contains(&"html"));
    }

    #[test]
    fn source_extensions_include_graphql_documents() {
        assert!(SOURCE_EXTENSIONS.contains(&"graphql"));
        assert!(SOURCE_EXTENSIONS.contains(&"gql"));
    }

    // PRODUCTION_EXCLUDE_PATTERNS tests — verify actual glob matching, not just string contains
    fn build_production_glob_set() -> globset::GlobSet {
        let mut builder = globset::GlobSetBuilder::new();
        for pattern in PRODUCTION_EXCLUDE_PATTERNS {
            builder.add(
                globset::GlobBuilder::new(pattern)
                    .literal_separator(true)
                    .build()
                    .expect("valid glob pattern"),
            );
        }
        builder.build().expect("valid glob set")
    }

    #[test]
    fn production_excludes_test_files() {
        let set = build_production_glob_set();
        assert!(set.is_match("src/Button.test.ts"));
        assert!(set.is_match("src/utils.spec.tsx"));
        assert!(set.is_match("src/__tests__/helper.ts"));
        // Non-test files should NOT match
        assert!(!set.is_match("src/Button.ts"));
        assert!(!set.is_match("src/utils.tsx"));
    }

    #[test]
    fn production_excludes_story_files() {
        let set = build_production_glob_set();
        assert!(set.is_match("src/Button.stories.tsx"));
        assert!(set.is_match("src/Card.story.ts"));
        // Non-story files should NOT match
        assert!(!set.is_match("src/Button.tsx"));
    }

    #[test]
    fn production_excludes_config_files_at_root_only() {
        let set = build_production_glob_set();
        // Root-level tool configs should match
        assert!(set.is_match("vitest.config.ts"));
        assert!(set.is_match("jest.config.js"));
        // Nested config files should NOT match (e.g. Angular app.config.ts)
        assert!(!set.is_match("src/app/app.config.ts"));
        assert!(!set.is_match("src/app/app.config.server.ts"));
        // Workspace-level tool configs are no longer excluded (acceptable trade-off)
        assert!(!set.is_match("packages/foo/vitest.config.ts"));
        // Source files should NOT match
        assert!(!set.is_match("src/config.ts"));
    }

    #[test]
    fn production_patterns_are_valid_globs() {
        // build_production_glob_set() already validates all patterns compile
        let _ = build_production_glob_set();
    }

    #[test]
    fn disallowed_hidden_dirs_idea() {
        assert!(!is_allowed_hidden_dir(OsStr::new(".idea")));
    }

    #[test]
    fn source_extensions_include_mdx() {
        assert!(SOURCE_EXTENSIONS.contains(&"mdx"));
    }

    #[test]
    fn source_extensions_exclude_image_and_data_formats() {
        assert!(!SOURCE_EXTENSIONS.contains(&"png"));
        assert!(!SOURCE_EXTENSIONS.contains(&"jpg"));
        assert!(!SOURCE_EXTENSIONS.contains(&"svg"));
        assert!(!SOURCE_EXTENSIONS.contains(&"txt"));
        assert!(!SOURCE_EXTENSIONS.contains(&"csv"));
        assert!(!SOURCE_EXTENSIONS.contains(&"wasm"));
    }

    // discover_files integration tests using tempdir fixtures
    mod discover_files_integration {
        use std::path::PathBuf;

        use fallow_config::{
            DuplicatesConfig, FallowConfig, FlagsConfig, HealthConfig, OutputFormat, ResolveConfig,
            RulesConfig,
        };

        use super::*;

        /// Create a minimal ResolvedConfig pointing at the given root directory.
        fn make_config(root: PathBuf, production: bool) -> ResolvedConfig {
            FallowConfig {
                production: production.into(),
                ..Default::default()
            }
            .resolve(root, OutputFormat::Human, 1, true, true)
        }

        /// Helper to collect discovered file names (relative to root) for assertions.
        /// Normalizes path separators to `/` for cross-platform test consistency.
        fn file_names(files: &[DiscoveredFile], root: &std::path::Path) -> Vec<String> {
            files
                .iter()
                .map(|f| {
                    f.path
                        .strip_prefix(root)
                        .unwrap_or(&f.path)
                        .to_string_lossy()
                        .replace('\\', "/")
                })
                .collect()
        }

        #[test]
        fn discovers_source_files_with_valid_extensions() {
            let dir = tempfile::tempdir().expect("create temp dir");
            let src = dir.path().join("src");
            std::fs::create_dir_all(&src).unwrap();

            // Source files that should be discovered
            std::fs::write(src.join("app.ts"), "export const a = 1;").unwrap();
            std::fs::write(src.join("component.tsx"), "export default () => {};").unwrap();
            std::fs::write(src.join("utils.js"), "module.exports = {};").unwrap();
            std::fs::write(src.join("helper.jsx"), "export const h = 1;").unwrap();
            std::fs::write(src.join("config.mjs"), "export default {};").unwrap();
            std::fs::write(src.join("legacy.cjs"), "module.exports = {};").unwrap();
            std::fs::write(src.join("types.mts"), "export type T = string;").unwrap();
            std::fs::write(src.join("compat.cts"), "module.exports = {};").unwrap();

            let config = make_config(dir.path().to_path_buf(), false);
            let files = discover_files(&config);
            let names = file_names(&files, dir.path());

            assert!(names.contains(&"src/app.ts".to_string()));
            assert!(names.contains(&"src/component.tsx".to_string()));
            assert!(names.contains(&"src/utils.js".to_string()));
            assert!(names.contains(&"src/helper.jsx".to_string()));
            assert!(names.contains(&"src/config.mjs".to_string()));
            assert!(names.contains(&"src/legacy.cjs".to_string()));
            assert!(names.contains(&"src/types.mts".to_string()));
            assert!(names.contains(&"src/compat.cts".to_string()));
        }

        #[test]
        fn excludes_non_source_extensions() {
            let dir = tempfile::tempdir().expect("create temp dir");
            let src = dir.path().join("src");
            std::fs::create_dir_all(&src).unwrap();

            // Source file to ensure discovery works at all
            std::fs::write(src.join("app.ts"), "export const a = 1;").unwrap();

            // Non-source files that should be excluded
            std::fs::write(src.join("data.json"), "{}").unwrap();
            std::fs::write(src.join("readme.md"), "# Hello").unwrap();
            std::fs::write(src.join("notes.txt"), "notes").unwrap();
            std::fs::write(src.join("logo.png"), [0u8; 8]).unwrap();

            let config = make_config(dir.path().to_path_buf(), false);
            let files = discover_files(&config);
            let names = file_names(&files, dir.path());

            assert_eq!(names.len(), 1, "only the .ts file should be discovered");
            assert!(names.contains(&"src/app.ts".to_string()));
        }

        #[test]
        fn excludes_disallowed_hidden_directories() {
            let dir = tempfile::tempdir().expect("create temp dir");

            // Files inside disallowed hidden directories
            let git_dir = dir.path().join(".git");
            std::fs::create_dir_all(&git_dir).unwrap();
            std::fs::write(git_dir.join("hooks.ts"), "// git hook").unwrap();

            let idea_dir = dir.path().join(".idea");
            std::fs::create_dir_all(&idea_dir).unwrap();
            std::fs::write(idea_dir.join("workspace.ts"), "// idea").unwrap();

            let cache_dir = dir.path().join(".cache");
            std::fs::create_dir_all(&cache_dir).unwrap();
            std::fs::write(cache_dir.join("cached.js"), "// cached").unwrap();

            // A normal source file to confirm discovery works
            let src = dir.path().join("src");
            std::fs::create_dir_all(&src).unwrap();
            std::fs::write(src.join("app.ts"), "export const a = 1;").unwrap();

            let config = make_config(dir.path().to_path_buf(), false);
            let files = discover_files(&config);
            let names = file_names(&files, dir.path());

            assert_eq!(names.len(), 1, "only src/app.ts should be discovered");
            assert!(names.contains(&"src/app.ts".to_string()));
        }

        #[test]
        fn includes_allowed_hidden_directories() {
            let dir = tempfile::tempdir().expect("create temp dir");

            // Files inside allowed hidden directories
            let storybook = dir.path().join(".storybook");
            std::fs::create_dir_all(&storybook).unwrap();
            std::fs::write(storybook.join("main.ts"), "export default {};").unwrap();

            let github = dir.path().join(".github");
            std::fs::create_dir_all(&github).unwrap();
            std::fs::write(github.join("actions.js"), "module.exports = {};").unwrap();

            let changeset = dir.path().join(".changeset");
            std::fs::create_dir_all(&changeset).unwrap();
            std::fs::write(changeset.join("config.js"), "module.exports = {};").unwrap();

            let config = make_config(dir.path().to_path_buf(), false);
            let files = discover_files(&config);
            let names = file_names(&files, dir.path());

            assert!(
                names.contains(&".storybook/main.ts".to_string()),
                "files in .storybook should be discovered"
            );
            assert!(
                names.contains(&".github/actions.js".to_string()),
                "files in .github should be discovered"
            );
            assert!(
                names.contains(&".changeset/config.js".to_string()),
                "files in .changeset should be discovered"
            );
        }

        #[test]
        fn default_discovery_excludes_client_and_server_hidden_directories() {
            let dir = tempfile::tempdir().expect("create temp dir");
            let app = dir.path().join("app");
            std::fs::create_dir_all(app.join(".client")).unwrap();
            std::fs::create_dir_all(app.join(".server")).unwrap();
            std::fs::write(app.join(".client/analytics.ts"), "export const a = 1;").unwrap();
            std::fs::write(app.join(".server/db.ts"), "export const db = {};").unwrap();
            std::fs::write(app.join("root.tsx"), "export default function Root() {}").unwrap();

            let config = make_config(dir.path().to_path_buf(), false);
            let files = discover_files(&config);
            let names = file_names(&files, dir.path());

            assert!(names.contains(&"app/root.tsx".to_string()));
            assert!(!names.contains(&"app/.client/analytics.ts".to_string()));
            assert!(!names.contains(&"app/.server/db.ts".to_string()));
        }

        #[test]
        fn scoped_hidden_dirs_include_client_and_server_under_package_root() {
            let dir = tempfile::tempdir().expect("create temp dir");
            let package = dir.path().join("packages/app");
            std::fs::create_dir_all(package.join("app/.client")).unwrap();
            std::fs::create_dir_all(package.join("app/.server")).unwrap();
            std::fs::write(
                package.join("app/.client/analytics.ts"),
                "export const track = () => {};",
            )
            .unwrap();
            std::fs::write(package.join("app/.server/db.ts"), "export const db = {};").unwrap();

            let config = make_config(dir.path().to_path_buf(), false);
            let scopes = [HiddenDirScope::new(
                package,
                vec![".client".to_string(), ".server".to_string()],
            )];
            let files = discover_files_with_additional_hidden_dirs(&config, &scopes);
            let names = file_names(&files, dir.path());

            assert!(names.contains(&"packages/app/app/.client/analytics.ts".to_string()));
            assert!(names.contains(&"packages/app/app/.server/db.ts".to_string()));
        }

        #[test]
        fn scoped_hidden_dirs_do_not_include_unscoped_packages() {
            let dir = tempfile::tempdir().expect("create temp dir");
            let active = dir.path().join("packages/active");
            let inactive = dir.path().join("packages/inactive");
            std::fs::create_dir_all(active.join("app/.server")).unwrap();
            std::fs::create_dir_all(inactive.join("app/.server")).unwrap();
            std::fs::write(active.join("app/.server/db.ts"), "export const db = {};").unwrap();
            std::fs::write(inactive.join("app/.server/db.ts"), "export const db = {};").unwrap();

            let config = make_config(dir.path().to_path_buf(), false);
            let scopes = [HiddenDirScope::new(active, vec![".server".to_string()])];
            let files = discover_files_with_additional_hidden_dirs(&config, &scopes);
            let names = file_names(&files, dir.path());

            assert!(names.contains(&"packages/active/app/.server/db.ts".to_string()));
            assert!(!names.contains(&"packages/inactive/app/.server/db.ts".to_string()));
        }

        #[test]
        fn excludes_root_build_directory() {
            let dir = tempfile::tempdir().expect("create temp dir");

            // The `ignore` crate respects `.ignore` files (independent of git).
            // Use this to simulate build/ exclusion as it happens in real projects.
            std::fs::write(dir.path().join(".ignore"), "/build/\n").unwrap();

            // Root-level build/ should be ignored
            let build_dir = dir.path().join("build");
            std::fs::create_dir_all(&build_dir).unwrap();
            std::fs::write(build_dir.join("output.js"), "// build output").unwrap();

            // Normal source file
            let src = dir.path().join("src");
            std::fs::create_dir_all(&src).unwrap();
            std::fs::write(src.join("app.ts"), "export const a = 1;").unwrap();

            let config = make_config(dir.path().to_path_buf(), false);
            let files = discover_files(&config);
            let names = file_names(&files, dir.path());

            assert_eq!(names.len(), 1, "root build/ should be excluded via .ignore");
            assert!(names.contains(&"src/app.ts".to_string()));
        }

        #[test]
        fn includes_nested_build_directory() {
            let dir = tempfile::tempdir().expect("create temp dir");

            // Nested build/ directory should NOT be ignored
            let nested_build = dir.path().join("src").join("build");
            std::fs::create_dir_all(&nested_build).unwrap();
            std::fs::write(nested_build.join("helper.ts"), "export const h = 1;").unwrap();

            let config = make_config(dir.path().to_path_buf(), false);
            let files = discover_files(&config);
            let names = file_names(&files, dir.path());

            assert!(
                names.contains(&"src/build/helper.ts".to_string()),
                "nested build/ directories should be included"
            );
        }

        #[test]
        #[expect(
            clippy::cast_possible_truncation,
            reason = "test file counts are trivially small"
        )]
        fn file_ids_are_sequential_after_sorting() {
            let dir = tempfile::tempdir().expect("create temp dir");
            let src = dir.path().join("src");
            std::fs::create_dir_all(&src).unwrap();

            std::fs::write(src.join("z_last.ts"), "export const z = 1;").unwrap();
            std::fs::write(src.join("a_first.ts"), "export const a = 1;").unwrap();
            std::fs::write(src.join("m_middle.ts"), "export const m = 1;").unwrap();

            let config = make_config(dir.path().to_path_buf(), false);
            let files = discover_files(&config);

            // IDs should be sequential 0, 1, 2
            for (idx, file) in files.iter().enumerate() {
                assert_eq!(file.id, FileId(idx as u32), "FileId should be sequential");
            }

            // Files should be sorted by path
            for pair in files.windows(2) {
                assert!(
                    pair[0].path < pair[1].path,
                    "files should be sorted by path"
                );
            }
        }

        #[test]
        fn production_mode_excludes_test_files() {
            let dir = tempfile::tempdir().expect("create temp dir");
            let src = dir.path().join("src");
            std::fs::create_dir_all(&src).unwrap();

            std::fs::write(src.join("app.ts"), "export const a = 1;").unwrap();
            std::fs::write(src.join("app.test.ts"), "test('a', () => {});").unwrap();
            std::fs::write(src.join("app.spec.ts"), "describe('a', () => {});").unwrap();
            std::fs::write(src.join("app.stories.tsx"), "export default {};").unwrap();

            let config = make_config(dir.path().to_path_buf(), true);
            let files = discover_files(&config);
            let names = file_names(&files, dir.path());

            assert!(
                names.contains(&"src/app.ts".to_string()),
                "source files should be included in production mode"
            );
            assert!(
                !names.contains(&"src/app.test.ts".to_string()),
                "test files should be excluded in production mode"
            );
            assert!(
                !names.contains(&"src/app.spec.ts".to_string()),
                "spec files should be excluded in production mode"
            );
            assert!(
                !names.contains(&"src/app.stories.tsx".to_string()),
                "story files should be excluded in production mode"
            );
        }

        #[test]
        fn non_production_mode_includes_test_files() {
            let dir = tempfile::tempdir().expect("create temp dir");
            let src = dir.path().join("src");
            std::fs::create_dir_all(&src).unwrap();

            std::fs::write(src.join("app.ts"), "export const a = 1;").unwrap();
            std::fs::write(src.join("app.test.ts"), "test('a', () => {});").unwrap();

            let config = make_config(dir.path().to_path_buf(), false);
            let files = discover_files(&config);
            let names = file_names(&files, dir.path());

            assert!(names.contains(&"src/app.ts".to_string()));
            assert!(
                names.contains(&"src/app.test.ts".to_string()),
                "test files should be included in non-production mode"
            );
        }

        #[test]
        fn empty_directory_returns_no_files() {
            let dir = tempfile::tempdir().expect("create temp dir");
            let config = make_config(dir.path().to_path_buf(), false);
            let files = discover_files(&config);
            assert!(files.is_empty(), "empty project should discover no files");
        }

        #[test]
        fn hidden_files_not_discovered_as_source() {
            let dir = tempfile::tempdir().expect("create temp dir");

            // Hidden files at root — these have source extensions but are dotfiles.
            // The type filter (`*.ts`, not `.*ts`) will exclude them because the
            // `ignore` crate's type matcher only matches non-hidden filenames.
            std::fs::write(dir.path().join(".env"), "SECRET=abc").unwrap();
            std::fs::write(dir.path().join(".gitignore"), "node_modules").unwrap();
            std::fs::write(dir.path().join(".eslintrc.js"), "module.exports = {};").unwrap();

            // Normal source file
            let src = dir.path().join("src");
            std::fs::create_dir_all(&src).unwrap();
            std::fs::write(src.join("app.ts"), "export const a = 1;").unwrap();

            let config = make_config(dir.path().to_path_buf(), false);
            let files = discover_files(&config);
            let names = file_names(&files, dir.path());

            assert!(
                !names.contains(&".env".to_string()),
                ".env should not be discovered"
            );
            assert!(
                !names.contains(&".gitignore".to_string()),
                ".gitignore should not be discovered"
            );
        }

        /// Create a config with custom ignore patterns.
        fn make_config_with_ignores(root: PathBuf, ignores: Vec<String>) -> ResolvedConfig {
            FallowConfig {
                schema: None,
                extends: vec![],
                entry: vec![],
                ignore_patterns: ignores,
                framework: vec![],
                workspaces: None,
                ignore_dependencies: vec![],
                ignore_exports: vec![],
                ignore_catalog_references: vec![],
                ignore_dependency_overrides: vec![],
                ignore_exports_used_in_file: fallow_config::IgnoreExportsUsedInFileConfig::default(
                ),
                used_class_members: vec![],
                duplicates: DuplicatesConfig::default(),
                health: HealthConfig::default(),
                rules: RulesConfig::default(),
                boundaries: fallow_config::BoundaryConfig::default(),
                production: false.into(),
                plugins: vec![],
                dynamically_loaded: vec![],
                overrides: vec![],
                regression: None,
                audit: fallow_config::AuditConfig::default(),
                codeowners: None,
                public_packages: vec![],
                flags: FlagsConfig::default(),
                resolve: ResolveConfig::default(),
                sealed: false,
                include_entry_exports: false,
            }
            .resolve(root, OutputFormat::Human, 1, true, true)
        }

        #[test]
        fn custom_ignore_patterns_exclude_matching_files() {
            let dir = tempfile::tempdir().expect("create temp dir");

            let generated = dir.path().join("src").join("api").join("generated");
            std::fs::create_dir_all(&generated).unwrap();
            std::fs::write(generated.join("client.ts"), "export const api = {};").unwrap();

            let client = dir.path().join("src").join("api").join("client");
            std::fs::create_dir_all(&client).unwrap();
            std::fs::write(client.join("fetch.ts"), "export const fetch = {};").unwrap();

            let src = dir.path().join("src");
            std::fs::write(src.join("index.ts"), "export const x = 1;").unwrap();

            let config = make_config_with_ignores(
                dir.path().to_path_buf(),
                vec![
                    "src/api/generated/**".to_string(),
                    "src/api/client/**".to_string(),
                ],
            );
            let files = discover_files(&config);
            let names = file_names(&files, dir.path());

            assert_eq!(names.len(), 1, "only non-ignored files: {names:?}");
            assert!(names.contains(&"src/index.ts".to_string()));
        }

        #[test]
        fn default_ignore_patterns_exclude_node_modules_and_dist() {
            let dir = tempfile::tempdir().expect("create temp dir");

            let nm = dir.path().join("node_modules").join("lodash");
            std::fs::create_dir_all(&nm).unwrap();
            std::fs::write(nm.join("lodash.js"), "module.exports = {};").unwrap();

            let dist = dir.path().join("dist");
            std::fs::create_dir_all(&dist).unwrap();
            std::fs::write(dist.join("bundle.js"), "// bundled").unwrap();

            let src = dir.path().join("src");
            std::fs::create_dir_all(&src).unwrap();
            std::fs::write(src.join("index.ts"), "export const x = 1;").unwrap();

            let config = make_config(dir.path().to_path_buf(), false);
            let files = discover_files(&config);
            let names = file_names(&files, dir.path());

            assert_eq!(names.len(), 1);
            assert!(names.contains(&"src/index.ts".to_string()));
        }

        #[test]
        fn default_ignore_patterns_exclude_root_build() {
            let dir = tempfile::tempdir().expect("create temp dir");

            // Root-level build/ should be excluded
            let build = dir.path().join("build");
            std::fs::create_dir_all(&build).unwrap();
            std::fs::write(build.join("output.js"), "// built").unwrap();

            // Nested build/ should NOT be excluded
            let nested_build = dir.path().join("src").join("build");
            std::fs::create_dir_all(&nested_build).unwrap();
            std::fs::write(nested_build.join("helper.ts"), "export const h = 1;").unwrap();

            let src = dir.path().join("src");
            std::fs::write(src.join("index.ts"), "export const x = 1;").unwrap();

            let config = make_config(dir.path().to_path_buf(), false);
            let files = discover_files(&config);
            let names = file_names(&files, dir.path());

            assert_eq!(
                names.len(),
                2,
                "root build/ excluded, nested kept: {names:?}"
            );
            assert!(names.contains(&"src/index.ts".to_string()));
            assert!(names.contains(&"src/build/helper.ts".to_string()));
        }
    }
}
