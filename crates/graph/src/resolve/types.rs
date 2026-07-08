//! Type definitions and constants for import resolution.

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use oxc_resolver::Resolver;
use rustc_hash::{FxHashMap, FxHashSet};
use serde_json::Value;

use fallow_types::discover::FileId;

/// Result of resolving an import specifier.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum ResolveResult {
    /// Resolved to a file within the project.
    InternalModule(FileId),
    /// Resolved to a project file through a framework convention auto-import.
    SyntheticAutoImport(FileId),
    /// Resolved to a workspace or self package source file while preserving
    /// dependency usage for package accounting.
    InternalPackageModule {
        /// Internal source file reached by the package map.
        file_id: FileId,
        /// Package name that was used in the import specifier.
        package_name: String,
    },
    /// Resolved to a file outside the project (`node_modules`, `.json`, etc.).
    ExternalFile(PathBuf),
    /// Bare specifier — an npm package.
    NpmPackage(String),
    /// Could not resolve.
    Unresolvable(String),
}

impl ResolveResult {
    /// Return the target file for any project-internal result.
    #[must_use]
    pub const fn internal_file_id(&self) -> Option<FileId> {
        match self {
            Self::InternalModule(file_id)
            | Self::SyntheticAutoImport(file_id)
            | Self::InternalPackageModule { file_id, .. } => Some(*file_id),
            Self::ExternalFile(_) | Self::NpmPackage(_) | Self::Unresolvable(_) => None,
        }
    }

    /// Return whether this edge was synthesized from framework auto-import conventions.
    #[must_use]
    pub const fn is_synthetic_auto_import(&self) -> bool {
        matches!(self, Self::SyntheticAutoImport(_))
    }

    /// Return the package name that should receive dependency usage credit.
    #[must_use]
    pub fn package_usage_name(&self) -> Option<&str> {
        match self {
            Self::InternalPackageModule { package_name, .. } | Self::NpmPackage(package_name) => {
                Some(package_name)
            }
            Self::InternalModule(_)
            | Self::SyntheticAutoImport(_)
            | Self::ExternalFile(_)
            | Self::Unresolvable(_) => None,
        }
    }
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

/// Any source-bearing module edge that resolves one literal specifier.
pub enum ResolvedSourceEdge<'a> {
    /// Static or literal dynamic import edge.
    Import(&'a ResolvedImport),
    /// Re-export source edge.
    ReExport(&'a ResolvedReExport),
}

impl<'a> ResolvedSourceEdge<'a> {
    /// Return the original source specifier.
    #[must_use]
    pub fn source_specifier(&self) -> &'a str {
        match self {
            Self::Import(import) => &import.info.source,
            Self::ReExport(re_export) => &re_export.info.source,
        }
    }

    /// Return the resolved target.
    #[must_use]
    pub const fn target(&self) -> &'a ResolveResult {
        match self {
            Self::Import(import) => &import.target,
            Self::ReExport(re_export) => &re_export.target,
        }
    }

    /// Return whether this edge is type-only.
    #[must_use]
    pub const fn is_type_only(&self) -> bool {
        match self {
            Self::Import(import) => import.info.is_type_only,
            Self::ReExport(re_export) => re_export.info.is_type_only,
        }
    }

    /// Return the span of the full import or re-export declaration.
    #[must_use]
    pub const fn span(&self) -> oxc_span::Span {
        match self {
            Self::Import(import) => import.info.span,
            Self::ReExport(re_export) => re_export.info.span,
        }
    }

    /// Return the source literal span when the extractor has one.
    #[must_use]
    pub const fn source_span(&self) -> oxc_span::Span {
        match self {
            Self::Import(import) => import.info.source_span,
            Self::ReExport(_) => oxc_span::Span::new(0, 0),
        }
    }
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
    /// Typed semantic facts produced by extraction for cross-layer analysis.
    pub semantic_facts: Box<[fallow_types::extract::SemanticFact]>,
    /// Identifiers used as whole objects (Object.values, for..in, spread, etc.).
    pub whole_object_uses: Box<[String]>,
    /// Whether this module uses `CommonJS` exports.
    pub has_cjs_exports: bool,
    /// Whether this module declares at least one Angular `@Component({
    /// templateUrl: ... })` decorator. Mirrors `ModuleInfo.has_angular_component_template_url`;
    /// see that field for the contract this gate enforces.
    pub has_angular_component_template_url: bool,
    /// Local names of import bindings that are never referenced in this file.
    pub unused_import_bindings: FxHashSet<String>,
    /// Local import bindings referenced from type positions.
    pub type_referenced_import_bindings: Vec<String>,
    /// Local import bindings referenced from runtime/value positions.
    pub value_referenced_import_bindings: Vec<String>,
    /// Namespace-import aliases re-exported through an object literal.
    /// See `fallow_types::extract::NamespaceObjectAlias` for the shape.
    pub namespace_object_aliases: Vec<fallow_types::extract::NamespaceObjectAlias>,
    /// Exported free-function factories that provably return one class instance.
    /// See `fallow_types::extract::FactoryReturnExport` and issue #1441 (Part A).
    pub exported_factory_returns: Box<[fallow_types::extract::FactoryReturnExport]>,
    /// Named-type property types declared by this module's top-level interfaces
    /// and type-literal aliases. See `fallow_types::extract::TypeMemberTypeEntry`
    /// and issue #1785.
    pub type_member_types: Box<[fallow_types::extract::TypeMemberTypeEntry]>,
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
            semantic_facts: Box::default(),
            whole_object_uses: Box::default(),
            has_cjs_exports: false,
            has_angular_component_template_url: false,
            unused_import_bindings: FxHashSet::default(),
            type_referenced_import_bindings: vec![],
            value_referenced_import_bindings: vec![],
            namespace_object_aliases: vec![],
            exported_factory_returns: Box::default(),
            type_member_types: Box::default(),
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

    /// Iterate over every literal source edge that has one resolved target.
    ///
    /// Includes static imports, literal dynamic imports, and re-export sources.
    /// Dynamic import patterns are excluded because they resolve to sets of
    /// files rather than single import specifiers.
    pub fn all_resolved_source_edges(&self) -> impl Iterator<Item = ResolvedSourceEdge<'_>> {
        self.resolved_imports
            .iter()
            .map(ResolvedSourceEdge::Import)
            .chain(
                self.resolved_dynamic_imports
                    .iter()
                    .map(ResolvedSourceEdge::Import),
            )
            .chain(self.re_exports.iter().map(ResolvedSourceEdge::ReExport))
    }
}

/// Shared context for resolving import specifiers.
///
/// Groups the immutable lookup tables and caches that are shared across all
/// `resolve_specifier` calls within a single `resolve_all_imports` invocation.
pub(super) struct ResolveContext<'a> {
    /// The oxc_resolver instance (configured once, shared across threads).
    pub resolver: &'a Resolver,
    /// CSS-only resolver with package.json `sass` and `style` conditions enabled.
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
    /// Package manifests for the root package and workspace packages.
    pub package_manifests: &'a [PackageManifestInfo],
    /// Ordered package condition names matching the resolver configuration.
    pub condition_names: &'a [String],
    /// Plugin-provided path aliases (prefix, replacement).
    pub path_aliases: &'a [(String, String)],
    /// Absolute directories to search when resolving bare SCSS/Sass
    /// `@import` / `@use` specifiers. Populated from Angular's
    /// `stylePreprocessorOptions.includePaths` and equivalent settings.
    pub scss_include_paths: &'a [PathBuf],
    /// Static directory URL mappings from framework config.
    /// Each tuple is `(absolute_source_dir, normalized_url_mount)`.
    pub static_dir_mappings: &'a [(PathBuf, String)],
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
    /// Per-analysis cache for local tsconfig discovery and JSON parsing.
    /// Import resolution calls these fallbacks for every unresolved or
    /// tsconfig-poisoned specifier, so keeping it session-local avoids
    /// repeated filesystem work without risking stale data across runs.
    pub tsconfig_cache: &'a TsconfigCache,
    /// Per-analysis cache of `dunce::canonicalize` results keyed by resolved
    /// path. Every import resolving to a `node_modules` / output-dir / symlinked
    /// target is realpath'd during classification, and the same package path is
    /// re-canonicalized for every file that imports the package. The result is a
    /// pure function of the path's on-disk state (constant within a run), so the
    /// cache is session-local for watch-mode safety.
    pub canonicalize_cache: &'a CanonicalizeCache,
}

/// Session-local cache of `dunce::canonicalize` results keyed by input path.
#[derive(Default)]
pub(super) struct CanonicalizeCache {
    map: Mutex<FxHashMap<PathBuf, Option<PathBuf>>>,
}

impl CanonicalizeCache {
    /// Return the cached `dunce::canonicalize(path)` outcome, computing it on
    /// first miss. `None` (a path that fails to canonicalize) is cached too so a
    /// repeated probe of the same missing path does not re-issue the syscall.
    pub fn get(&self, path: &Path) -> Option<PathBuf> {
        if let Ok(cache) = self.map.lock()
            && let Some(value) = cache.get(path)
        {
            return value.clone();
        }
        let value = dunce::canonicalize(path).ok();
        if let Ok(mut cache) = self.map.lock() {
            cache.insert(path.to_path_buf(), value.clone());
        }
        value
    }
}

/// Session-local cache for tsconfig helper lookups used during import resolution.
#[derive(Default)]
pub(super) struct TsconfigCache {
    json: Mutex<FxHashMap<PathBuf, Option<Value>>>,
    chains: Mutex<FxHashMap<PathBuf, Vec<PathBuf>>>,
}

impl TsconfigCache {
    /// Return a cached parsed tsconfig JSON value, loading it on first miss.
    pub fn json(&self, path: &Path, load: impl FnOnce(&Path) -> Option<Value>) -> Option<Value> {
        if let Ok(cache) = self.json.lock()
            && let Some(value) = cache.get(path)
        {
            return value.clone();
        }

        let value = load(path);
        if let Ok(mut cache) = self.json.lock() {
            cache.insert(path.to_path_buf(), value.clone());
        }
        value
    }

    /// Return the cached tsconfig chain for a source file, if one exists.
    pub fn chain(&self, from_file: &Path) -> Option<Vec<PathBuf>> {
        self.chains
            .lock()
            .ok()
            .and_then(|cache| cache.get(from_file).cloned())
    }

    /// Store the computed tsconfig chain for a source file.
    pub fn store_chain(&self, from_file: &Path, chain: Vec<PathBuf>) {
        if let Ok(mut cache) = self.chains.lock() {
            cache.insert(from_file.to_path_buf(), chain);
        }
    }
}

/// Package manifest data used by source fallbacks.
#[derive(Debug, Clone)]
pub(super) struct PackageManifestInfo {
    /// Package root path as discovered from the workspace tree.
    pub root: PathBuf,
    /// Canonical package root path for node_modules symlink comparisons.
    pub canonical_root: PathBuf,
    /// Parsed package name.
    pub name: Option<String>,
    /// Parsed package.json fields.
    pub package_json: fallow_config::PackageJson,
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
