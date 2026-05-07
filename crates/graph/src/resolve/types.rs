//! Type definitions and constants for import resolution.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use oxc_resolver::Resolver;
use rustc_hash::{FxHashMap, FxHashSet};

use fallow_types::discover::FileId;

/// Result of resolving an import specifier.
#[derive(Debug, Clone)]
pub enum ResolveResult {
    /// Resolved to a file within the project.
    InternalModule(FileId),
    /// Resolved to a file outside the project (`node_modules`, `.json`, etc.).
    ExternalFile(PathBuf),
    /// Bare specifier — an npm package.
    NpmPackage(String),
    /// Could not resolve.
    Unresolvable(String),
}

/// A resolved import with its target.
#[derive(Debug, Clone)]
pub struct ResolvedImport {
    /// The original import information.
    pub info: fallow_types::extract::ImportInfo,
    /// Where the import resolved to.
    pub target: ResolveResult,
}

/// A resolved re-export with its target.
#[derive(Debug, Clone)]
pub struct ResolvedReExport {
    /// The original re-export information.
    pub info: fallow_types::extract::ReExportInfo,
    /// Where the re-export source resolved to.
    pub target: ResolveResult,
}

/// Fully resolved module with all imports mapped to targets.
#[derive(Debug)]
pub struct ResolvedModule {
    /// Unique file identifier.
    pub file_id: FileId,
    /// Absolute path to the module file.
    pub path: PathBuf,
    /// All export declarations in this module.
    pub exports: Vec<fallow_types::extract::ExportInfo>,
    /// All re-exports with resolved targets.
    pub re_exports: Vec<ResolvedReExport>,
    /// All static imports with resolved targets.
    pub resolved_imports: Vec<ResolvedImport>,
    /// All dynamic imports with resolved targets.
    pub resolved_dynamic_imports: Vec<ResolvedImport>,
    /// Dynamic import patterns matched against discovered files.
    pub resolved_dynamic_patterns: Vec<(fallow_types::extract::DynamicImportPattern, Vec<FileId>)>,
    /// Static member accesses (e.g., `Status.Active`).
    pub member_accesses: Vec<fallow_types::extract::MemberAccess>,
    /// Identifiers used as whole objects (Object.values, for..in, spread, etc.).
    pub whole_object_uses: Vec<String>,
    /// Whether this module uses `CommonJS` exports.
    pub has_cjs_exports: bool,
    /// Local names of import bindings that are never referenced in this file.
    pub unused_import_bindings: FxHashSet<String>,
    /// Local import bindings referenced from type positions.
    pub type_referenced_import_bindings: Vec<String>,
    /// Local import bindings referenced from runtime/value positions.
    pub value_referenced_import_bindings: Vec<String>,
}

impl Default for ResolvedModule {
    fn default() -> Self {
        Self {
            file_id: FileId(0),
            path: PathBuf::new(),
            exports: vec![],
            re_exports: vec![],
            resolved_imports: vec![],
            resolved_dynamic_imports: vec![],
            resolved_dynamic_patterns: vec![],
            member_accesses: vec![],
            whole_object_uses: vec![],
            has_cjs_exports: false,
            unused_import_bindings: FxHashSet::default(),
            type_referenced_import_bindings: vec![],
            value_referenced_import_bindings: vec![],
        }
    }
}

impl ResolvedModule {
    /// Iterate over all concrete resolved imports in source order buckets.
    ///
    /// Includes static `import`/`require` edges and literal dynamic `import()`
    /// edges. Dynamic import patterns are intentionally excluded because they
    /// resolve to sets of files rather than single import specifiers.
    pub fn all_resolved_imports(&self) -> impl Iterator<Item = &ResolvedImport> {
        self.resolved_imports
            .iter()
            .chain(self.resolved_dynamic_imports.iter())
    }
}

/// Shared context for resolving import specifiers.
///
/// Groups the immutable lookup tables and caches that are shared across all
/// `resolve_specifier` calls within a single `resolve_all_imports` invocation.
pub(super) struct ResolveContext<'a> {
    /// The oxc_resolver instance (configured once, shared across threads).
    pub resolver: &'a Resolver,
    /// CSS-only resolver with the package.json `style` condition enabled.
    /// Used only for stylesheet package subpaths so JS/TS imports do not
    /// accidentally prefer CSS export branches.
    pub style_resolver: &'a Resolver,
    /// Ordered extension list used by the resolver.
    pub extensions: &'a [String],
    /// Canonical path → FileId lookup (raw paths when root is canonical).
    pub path_to_id: &'a FxHashMap<&'a Path, FileId>,
    /// Raw (non-canonical) path → FileId lookup.
    pub raw_path_to_id: &'a FxHashMap<&'a Path, FileId>,
    /// Workspace name → canonical root path.
    pub workspace_roots: &'a FxHashMap<&'a str, &'a Path>,
    /// Plugin-provided path aliases (prefix, replacement).
    pub path_aliases: &'a [(String, String)],
    /// Absolute directories to search when resolving bare SCSS/Sass
    /// `@import` / `@use` specifiers. Populated from Angular's
    /// `stylePreprocessorOptions.includePaths` and equivalent settings.
    pub scss_include_paths: &'a [PathBuf],
    /// Project root directory.
    pub root: &'a Path,
    /// Lazy canonical path → FileId fallback for intra-project symlinks.
    /// Only initialized on first miss when root is canonical. `None` when
    /// path_to_id already uses canonical paths (root is not canonical).
    pub canonical_fallback: Option<&'a CanonicalFallback<'a>>,
    /// Dedup set for broken-tsconfig warnings. Emits one `tracing::warn!`
    /// per unique error message instead of spamming the log with one
    /// warning per affected file. Shared across all parallel resolver
    /// threads via `Mutex`. Empty and unused when no tsconfig errors occur.
    pub tsconfig_warned: &'a Mutex<FxHashSet<String>>,
}

/// Thread-safe lazy canonical path index, built on first access.
pub(super) struct CanonicalFallback<'a> {
    files: &'a [fallow_types::discover::DiscoveredFile],
    map: std::sync::OnceLock<FxHashMap<std::path::PathBuf, FileId>>,
}

impl<'a> CanonicalFallback<'a> {
    pub const fn new(files: &'a [fallow_types::discover::DiscoveredFile]) -> Self {
        Self {
            files,
            map: std::sync::OnceLock::new(),
        }
    }

    /// Look up a canonical path, lazily building the index on first call.
    pub fn get(&self, canonical: &Path) -> Option<FileId> {
        let map = self.map.get_or_init(|| {
            tracing::debug!(
                "intra-project symlinks detected, building canonical path index ({} files)",
                self.files.len()
            );
            self.files
                .iter()
                .filter_map(|f| {
                    dunce::canonicalize(&f.path)
                        .ok()
                        .map(|canonical| (canonical, f.id))
                })
                .collect()
        });
        map.get(canonical).copied()
    }
}

#[cfg(all(test, not(miri)))]
mod tests {
    use super::*;
    use fallow_types::discover::DiscoveredFile;

    #[test]
    fn canonical_fallback_returns_none_for_empty_files() {
        let files: Vec<DiscoveredFile> = vec![];
        let fallback = CanonicalFallback::new(&files);
        assert!(fallback.get(Path::new("/nonexistent")).is_none());
    }

    #[test]
    fn canonical_fallback_finds_existing_file() {
        let temp = std::env::temp_dir().join("fallow-test-canonical-fallback");
        let _ = std::fs::create_dir_all(&temp);
        let test_file = temp.join("test.ts");
        std::fs::write(&test_file, "").unwrap();

        let files = vec![DiscoveredFile {
            id: FileId(42),
            path: test_file.clone(),
            size_bytes: 0,
        }];
        let fallback = CanonicalFallback::new(&files);

        let canonical = dunce::canonicalize(&test_file).unwrap();
        assert_eq!(fallback.get(&canonical), Some(FileId(42)));

        // Second call uses cached map (OnceLock)
        assert_eq!(fallback.get(&canonical), Some(FileId(42)));

        let _ = std::fs::remove_dir_all(&temp);
    }

    #[test]
    fn canonical_fallback_returns_none_for_missing_path() {
        let temp = std::env::temp_dir().join("fallow-test-canonical-miss");
        let _ = std::fs::create_dir_all(&temp);
        let test_file = temp.join("exists.ts");
        std::fs::write(&test_file, "").unwrap();

        let files = vec![DiscoveredFile {
            id: FileId(1),
            path: test_file,
            size_bytes: 0,
        }];
        let fallback = CanonicalFallback::new(&files);
        assert!(fallback.get(Path::new("/nonexistent/file.ts")).is_none());

        let _ = std::fs::remove_dir_all(&temp);
    }
}

/// Known output directory names that may appear in exports map targets.
/// When an exports map points to `./dist/utils.js`, we try replacing these
/// prefixes with `src/` (the conventional source directory) to find the tracked
/// source file.
pub const OUTPUT_DIRS: &[&str] = &["dist", "build", "out", "esm", "cjs"];

/// Source extensions to try when mapping a built output file back to source.
pub const SOURCE_EXTS: &[&str] = &["ts", "tsx", "mts", "cts", "js", "jsx", "mjs", "cjs"];

/// React Native platform extension prefixes.
/// Metro resolves platform-specific files (e.g., `./foo` -> `./foo.web.tsx` on web).
pub const RN_PLATFORM_PREFIXES: &[&str] = &[".web", ".ios", ".android", ".native"];
