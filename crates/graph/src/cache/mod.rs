//! Persisted graph-cache identity contracts and on-disk store.
//!
//! The manifest types here define the invalidation surface a persisted graph
//! cache must satisfy before a cached graph can be trusted; the store implements
//! the coarse all-or-nothing load / save of a previously-built `ModuleGraph`
//! keyed by that manifest.

use std::path::Path;

use fallow_types::discover::{DiscoveredFile, StableFileKey};
use fallow_types::extract::{ImportInfo, ReExportInfo};
use fallow_types::source_fingerprint::SourceFingerprint;
use oxc_span::Span;

use crate::resolve::{ResolveResult, ResolvedImport, ResolvedModule, ResolvedReExport};

mod store;

pub use store::GraphCacheStore;

/// Persisted graph cache schema version.
///
/// Bump this whenever the serialized shape of the persisted graph (any of the
/// graph types that derive serde for the cache, the manifest types, or the
/// store envelope) changes, so a stale `graph-cache.bin` written by an older
/// binary is rejected rather than deserialized into the wrong shape.
pub const GRAPH_CACHE_VERSION: u32 = 2;

/// Cached import edge that can be restored without re-running resolution.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CachedResolvedImport {
    /// Import metadata mirrored from extraction or resolver synthesis.
    pub info: CachedImportInfo,
    /// Resolved target for this import edge.
    pub target: ResolveResult,
}

impl From<&ResolvedImport> for CachedResolvedImport {
    fn from(import: &ResolvedImport) -> Self {
        Self {
            info: CachedImportInfo::from(&import.info),
            target: import.target.clone(),
        }
    }
}

impl From<CachedResolvedImport> for ResolvedImport {
    fn from(import: CachedResolvedImport) -> Self {
        Self {
            info: import.info.into(),
            target: import.target,
        }
    }
}

/// Cached re-export edge that can be restored without re-running resolution.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CachedResolvedReExport {
    /// Re-export metadata mirrored from extraction.
    pub info: CachedReExportInfo,
    /// Resolved target for this re-export source.
    pub target: ResolveResult,
}

impl From<&ResolvedReExport> for CachedResolvedReExport {
    fn from(re_export: &ResolvedReExport) -> Self {
        Self {
            info: CachedReExportInfo::from(&re_export.info),
            target: re_export.target.clone(),
        }
    }
}

impl From<CachedResolvedReExport> for ResolvedReExport {
    fn from(re_export: CachedResolvedReExport) -> Self {
        Self {
            info: re_export.info.into(),
            target: re_export.target,
        }
    }
}

/// Cache-friendly mirror of [`ImportInfo`].
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CachedImportInfo {
    /// Import source specifier.
    pub source: String,
    /// Imported binding shape.
    pub imported_name: fallow_types::extract::ImportedName,
    /// Local binding name.
    pub local_name: String,
    /// Whether this import is type-only.
    pub is_type_only: bool,
    /// Whether this import originated from a style context.
    pub from_style: bool,
    /// Span of the full import declaration.
    pub span: [u32; 2],
    /// Span of the import source literal.
    pub source_span: [u32; 2],
}

impl From<&ImportInfo> for CachedImportInfo {
    fn from(info: &ImportInfo) -> Self {
        Self {
            source: info.source.clone(),
            imported_name: info.imported_name.clone(),
            local_name: info.local_name.clone(),
            is_type_only: info.is_type_only,
            from_style: info.from_style,
            span: span_to_pair(info.span),
            source_span: span_to_pair(info.source_span),
        }
    }
}

impl From<CachedImportInfo> for ImportInfo {
    fn from(info: CachedImportInfo) -> Self {
        Self {
            source: info.source,
            imported_name: info.imported_name,
            local_name: info.local_name,
            is_type_only: info.is_type_only,
            from_style: info.from_style,
            span: pair_to_span(info.span),
            source_span: pair_to_span(info.source_span),
        }
    }
}

/// Cache-friendly mirror of [`ReExportInfo`].
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CachedReExportInfo {
    /// Re-export source specifier.
    pub source: String,
    /// Imported name from the source module.
    pub imported_name: String,
    /// Exported name from this module.
    pub exported_name: String,
    /// Whether this re-export is type-only.
    pub is_type_only: bool,
    /// Span of the re-export declaration.
    pub span: [u32; 2],
}

impl From<&ReExportInfo> for CachedReExportInfo {
    fn from(info: &ReExportInfo) -> Self {
        Self {
            source: info.source.clone(),
            imported_name: info.imported_name.clone(),
            exported_name: info.exported_name.clone(),
            is_type_only: info.is_type_only,
            span: span_to_pair(info.span),
        }
    }
}

impl From<CachedReExportInfo> for ReExportInfo {
    fn from(info: CachedReExportInfo) -> Self {
        Self {
            source: info.source,
            imported_name: info.imported_name,
            exported_name: info.exported_name,
            is_type_only: info.is_type_only,
            span: pair_to_span(info.span),
        }
    }
}

/// Cached resolver output for one module.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CachedResolvedModule {
    /// File identifier of the source module.
    pub file_id: fallow_types::discover::FileId,
    /// Static import and require edges after resolution.
    pub resolved_imports: Vec<CachedResolvedImport>,
    /// Literal dynamic import edges after resolution.
    pub resolved_dynamic_imports: Vec<CachedResolvedImport>,
    /// Re-export source edges after resolution.
    pub re_exports: Vec<CachedResolvedReExport>,
    /// Dynamic import pattern targets, aligned with current extracted patterns.
    pub resolved_dynamic_pattern_targets: Vec<Vec<fallow_types::discover::FileId>>,
}

impl From<&ResolvedModule> for CachedResolvedModule {
    fn from(module: &ResolvedModule) -> Self {
        Self {
            file_id: module.file_id,
            resolved_imports: module
                .resolved_imports
                .iter()
                .map(CachedResolvedImport::from)
                .collect(),
            resolved_dynamic_imports: module
                .resolved_dynamic_imports
                .iter()
                .map(CachedResolvedImport::from)
                .collect(),
            re_exports: module
                .re_exports
                .iter()
                .map(CachedResolvedReExport::from)
                .collect(),
            resolved_dynamic_pattern_targets: module
                .resolved_dynamic_patterns
                .iter()
                .map(|(_, targets)| targets.clone())
                .collect(),
        }
    }
}

/// Convert resolved modules into the compact graph-cache resolver payload.
#[must_use]
pub fn cache_resolved_modules(resolved: &[ResolvedModule]) -> Vec<CachedResolvedModule> {
    resolved.iter().map(CachedResolvedModule::from).collect()
}

/// Restore resolved modules from cached resolver payloads and current parsed modules.
///
/// Returns `None` if the payload no longer aligns with the current parse result.
/// A normal graph-cache manifest hit should keep these aligned; this extra check
/// keeps corrupt or hand-edited cache files on the safe miss path.
#[must_use]
pub fn restore_resolved_modules(
    modules: &[fallow_types::extract::ModuleInfo],
    files: &[DiscoveredFile],
    cached: &[CachedResolvedModule],
) -> Option<Vec<ResolvedModule>> {
    if modules.len() != cached.len() {
        return None;
    }

    let mut by_file_id: rustc_hash::FxHashMap<_, _> = modules
        .iter()
        .map(|module| (module.file_id, module))
        .collect();
    let path_by_file_id: rustc_hash::FxHashMap<_, _> = files
        .iter()
        .map(|file| (file.id, file.path.clone()))
        .collect();

    cached
        .iter()
        .map(|entry| {
            let module = by_file_id.remove(&entry.file_id)?;
            let path = path_by_file_id.get(&entry.file_id)?.clone();
            if entry.resolved_dynamic_pattern_targets.len() != module.dynamic_import_patterns.len()
            {
                return None;
            }

            Some(ResolvedModule {
                file_id: module.file_id,
                path,
                exports: module.exports.clone(),
                re_exports: entry
                    .re_exports
                    .iter()
                    .cloned()
                    .map(ResolvedReExport::from)
                    .collect(),
                resolved_imports: entry
                    .resolved_imports
                    .iter()
                    .cloned()
                    .map(ResolvedImport::from)
                    .collect(),
                resolved_dynamic_imports: entry
                    .resolved_dynamic_imports
                    .iter()
                    .cloned()
                    .map(ResolvedImport::from)
                    .collect(),
                resolved_dynamic_patterns: module
                    .dynamic_import_patterns
                    .iter()
                    .cloned()
                    .zip(entry.resolved_dynamic_pattern_targets.iter().cloned())
                    .collect(),
                member_accesses: module.member_accesses.clone(),
                semantic_facts: module.semantic_facts.clone(),
                whole_object_uses: module.whole_object_uses.clone(),
                has_cjs_exports: module.has_cjs_exports,
                has_angular_component_template_url: module.has_angular_component_template_url,
                unused_import_bindings: module.unused_import_bindings.iter().cloned().collect(),
                type_referenced_import_bindings: module.type_referenced_import_bindings.clone(),
                value_referenced_import_bindings: module.value_referenced_import_bindings.clone(),
                namespace_object_aliases: module.namespace_object_aliases.clone(),
                exported_factory_returns: module.exported_factory_returns.clone(),
            })
        })
        .collect()
}

fn span_to_pair(span: Span) -> [u32; 2] {
    [span.start, span.end]
}

fn pair_to_span(pair: [u32; 2]) -> Span {
    Span::new(pair[0], pair[1])
}

/// Serialize an [`oxc_span::Span`] as a `[start, end]` `u32` pair.
///
/// `oxc_span::Span` does not enable its own serde feature in this workspace, so
/// the graph types that carry spans route them through this module via
/// `#[serde(with = "crate::cache::span_serde")]`. A 2-element array keeps the
/// postcard encoding compact (two varints) and is trivially lossless: a `Span`
/// is fully described by its `start` / `end` offsets.
pub(crate) mod span_serde {
    use oxc_span::Span;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    #[expect(
        clippy::trivially_copy_pass_by_ref,
        reason = "serde `serialize_with` / `with` requires a `&T` signature"
    )]
    pub fn serialize<S: Serializer>(span: &Span, serializer: S) -> Result<S::Ok, S::Error> {
        [span.start, span.end].serialize(serializer)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Span, D::Error> {
        let [start, end] = <[u32; 2]>::deserialize(deserializer)?;
        Ok(Span::new(start, end))
    }
}

/// Lossless cache (de)serialization for `Vec<MemberInfo>`.
///
/// `fallow_types::extract::MemberInfo` derives only `serde::Serialize`, and its
/// `span` field uses `serialize_with` with no matching deserializer, so it
/// cannot be deserialized through a plain derive. Rather than change the shared
/// type's serde shape (which would ripple into JSON output), the cache mirrors
/// it field-for-field into a dedicated `CachedMemberInfo` and converts both
/// ways. Every `MemberInfo` field is carried, so the round-trip is lossless.
pub(crate) mod member_serde {
    use fallow_types::extract::{MemberInfo, MemberKind};
    use oxc_span::Span;
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    #[derive(Serialize, Deserialize)]
    struct CachedMemberInfo {
        name: String,
        kind: MemberKind,
        span: [u32; 2],
        has_decorator: bool,
        decorator_names: Vec<String>,
        is_instance_returning_static: bool,
        is_self_returning: bool,
    }

    impl From<&MemberInfo> for CachedMemberInfo {
        fn from(member: &MemberInfo) -> Self {
            Self {
                name: member.name.clone(),
                kind: member.kind,
                span: [member.span.start, member.span.end],
                has_decorator: member.has_decorator,
                decorator_names: member.decorator_names.clone(),
                is_instance_returning_static: member.is_instance_returning_static,
                is_self_returning: member.is_self_returning,
            }
        }
    }

    impl From<CachedMemberInfo> for MemberInfo {
        fn from(cached: CachedMemberInfo) -> Self {
            Self {
                name: cached.name,
                kind: cached.kind,
                span: Span::new(cached.span[0], cached.span[1]),
                has_decorator: cached.has_decorator,
                decorator_names: cached.decorator_names,
                is_instance_returning_static: cached.is_instance_returning_static,
                is_self_returning: cached.is_self_returning,
            }
        }
    }

    pub fn serialize<S: Serializer>(
        members: &[MemberInfo],
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        let mirror: Vec<CachedMemberInfo> = members.iter().map(CachedMemberInfo::from).collect();
        mirror.serialize(serializer)
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Vec<MemberInfo>, D::Error> {
        let mirror = Vec::<CachedMemberInfo>::deserialize(deserializer)?;
        Ok(mirror.into_iter().map(MemberInfo::from).collect())
    }
}

/// Option dimensions that affect graph construction.
///
/// The hashes are intentionally opaque to this crate. Callers decide which
/// resolver/plugin/entry-point inputs feed each hash, while this contract keeps
/// graph-cache validation explicit and typed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct GraphCacheMode {
    /// Import resolver and tsconfig-relevant options.
    pub resolver_options_hash: u64,
    /// Entry point set and reachability root options.
    pub entry_points_hash: u64,
    /// Plugin-derived graph-affecting configuration.
    pub plugin_config_hash: u64,
}

impl GraphCacheMode {
    /// Build a mode from explicit hash dimensions.
    #[must_use]
    pub const fn new(
        resolver_options_hash: u64,
        entry_points_hash: u64,
        plugin_config_hash: u64,
    ) -> Self {
        Self {
            resolver_options_hash,
            entry_points_hash,
            plugin_config_hash,
        }
    }
}

/// Source freshness for one file in a graph-cache manifest.
#[derive(Debug, Clone, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
pub struct GraphCacheFile {
    /// Persistable identity for the file.
    pub key: StableFileKey,
    /// Metadata fingerprint for cache invalidation.
    pub fingerprint: SourceFingerprint,
}

impl GraphCacheFile {
    /// Build a graph-cache file row from a discovered file and fingerprint.
    #[must_use]
    pub fn from_discovered_file(
        root: &Path,
        file: &DiscoveredFile,
        fingerprint: SourceFingerprint,
    ) -> Self {
        Self {
            key: StableFileKey::from_root_relative(root, &file.path),
            fingerprint,
        }
    }
}

/// Manifest inputs required to trust a persisted graph cache entry.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct GraphCacheManifest {
    /// Schema version used by the persisted graph-cache entry.
    pub version: u32,
    /// Graph-affecting option dimensions.
    pub mode: GraphCacheMode,
    /// Stable file identities and freshness metadata.
    pub files: Vec<GraphCacheFile>,
}

impl GraphCacheManifest {
    /// Build a manifest and sort files by stable key for deterministic compare.
    #[must_use]
    pub fn new(mode: GraphCacheMode, mut files: Vec<GraphCacheFile>) -> Self {
        sort_files(&mut files);
        Self {
            version: GRAPH_CACHE_VERSION,
            mode,
            files,
        }
    }

    /// Build a manifest from discovered files plus a fingerprint provider.
    pub fn from_discovered_files(
        root: &Path,
        files: &[DiscoveredFile],
        mode: GraphCacheMode,
        mut fingerprint_for_path: impl FnMut(&Path) -> SourceFingerprint,
    ) -> Self {
        let rows = files
            .iter()
            .map(|file| {
                GraphCacheFile::from_discovered_file(root, file, fingerprint_for_path(&file.path))
            })
            .collect();
        Self::new(mode, rows)
    }

    /// True when a persisted manifest matches the current graph inputs.
    #[must_use]
    pub fn matches_inputs(&self, current: &Self) -> bool {
        self.version == GRAPH_CACHE_VERSION
            && current.version == GRAPH_CACHE_VERSION
            && self.mode == current.mode
            && self.files == current.files
    }
}

fn sort_files(files: &mut [GraphCacheFile]) {
    files.sort_unstable_by(|a, b| a.key.cmp(&b.key));
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use fallow_types::discover::FileId;
    use rustc_hash::FxHashMap;

    use super::*;

    fn file(id: u32, path: &str) -> DiscoveredFile {
        DiscoveredFile {
            id: FileId(id),
            path: PathBuf::from(path),
            size_bytes: 1,
        }
    }

    fn mode() -> GraphCacheMode {
        GraphCacheMode::new(1, 2, 3)
    }

    fn fingerprints(pairs: &[(&str, SourceFingerprint)]) -> FxHashMap<PathBuf, SourceFingerprint> {
        pairs
            .iter()
            .map(|(path, fingerprint)| (PathBuf::from(path), *fingerprint))
            .collect()
    }

    fn manifest(
        files: &[DiscoveredFile],
        mode: GraphCacheMode,
        map: &FxHashMap<PathBuf, SourceFingerprint>,
    ) -> GraphCacheManifest {
        GraphCacheManifest::from_discovered_files(Path::new("/project"), files, mode, |path| {
            *map.get(path).unwrap()
        })
    }

    #[test]
    fn manifest_sorts_by_stable_file_key() {
        let files = vec![file(0, "/project/src/z.ts"), file(1, "/project/src/a.ts")];
        let map = fingerprints(&[
            ("/project/src/z.ts", SourceFingerprint::new(10, 1)),
            ("/project/src/a.ts", SourceFingerprint::new(20, 1)),
        ]);

        let manifest = manifest(&files, mode(), &map);

        let keys: Vec<&str> = manifest
            .files
            .iter()
            .map(|file| file.key.as_str())
            .collect();
        assert_eq!(keys, vec!["src/a.ts", "src/z.ts"]);
    }

    #[test]
    fn manifest_matches_across_file_id_shift() {
        let before = vec![file(0, "/project/src/a.ts"), file(1, "/project/src/c.ts")];
        let after = vec![file(9, "/project/src/c.ts"), file(2, "/project/src/a.ts")];
        let map = fingerprints(&[
            ("/project/src/a.ts", SourceFingerprint::new(10, 1)),
            ("/project/src/c.ts", SourceFingerprint::new(20, 1)),
        ]);

        let cached = manifest(&before, mode(), &map);
        let current = manifest(&after, mode(), &map);

        assert!(cached.matches_inputs(&current));
    }

    #[test]
    fn manifest_misses_on_fingerprint_change() {
        let files = vec![file(0, "/project/src/a.ts")];
        let cached_map = fingerprints(&[("/project/src/a.ts", SourceFingerprint::new(10, 1))]);
        let current_map = fingerprints(&[("/project/src/a.ts", SourceFingerprint::new(11, 1))]);

        let cached = manifest(&files, mode(), &cached_map);
        let current = manifest(&files, mode(), &current_map);

        assert!(!cached.matches_inputs(&current));
    }

    #[test]
    fn manifest_misses_on_file_deletion() {
        let before = vec![
            file(0, "/project/src/a.ts"),
            file(1, "/project/src/deleted.ts"),
        ];
        let after = vec![file(0, "/project/src/a.ts")];
        let map = fingerprints(&[
            ("/project/src/a.ts", SourceFingerprint::new(10, 1)),
            ("/project/src/deleted.ts", SourceFingerprint::new(20, 1)),
        ]);

        let cached = manifest(&before, mode(), &map);
        let current = manifest(&after, mode(), &map);

        assert!(!cached.matches_inputs(&current));
    }

    #[test]
    fn manifest_misses_on_file_rename_with_same_fingerprint() {
        let before = vec![file(0, "/project/src/old.ts")];
        let after = vec![file(0, "/project/src/new.ts")];
        let map = fingerprints(&[
            ("/project/src/old.ts", SourceFingerprint::new(10, 1)),
            ("/project/src/new.ts", SourceFingerprint::new(10, 1)),
        ]);

        let cached = manifest(&before, mode(), &map);
        let current = manifest(&after, mode(), &map);

        assert!(!cached.matches_inputs(&current));
    }

    #[test]
    fn manifest_misses_on_workspace_scoped_file_set() {
        let full_project = vec![
            file(0, "/project/packages/app/src/index.ts"),
            file(1, "/project/packages/shared/src/index.ts"),
        ];
        let workspace_scoped = vec![file(0, "/project/packages/app/src/index.ts")];
        let map = fingerprints(&[
            (
                "/project/packages/app/src/index.ts",
                SourceFingerprint::new(10, 1),
            ),
            (
                "/project/packages/shared/src/index.ts",
                SourceFingerprint::new(20, 1),
            ),
        ]);

        let cached = manifest(&full_project, mode(), &map);
        let current = manifest(&workspace_scoped, mode(), &map);

        assert!(!cached.matches_inputs(&current));
    }

    #[test]
    fn manifest_misses_on_mode_change() {
        let files = vec![file(0, "/project/src/a.ts")];
        let map = fingerprints(&[("/project/src/a.ts", SourceFingerprint::new(10, 1))]);

        let cached = manifest(&files, mode(), &map);
        let current = manifest(&files, GraphCacheMode::new(1, 99, 3), &map);

        assert!(!cached.matches_inputs(&current));
    }

    #[test]
    fn manifest_misses_on_version_change() {
        let files = vec![file(0, "/project/src/a.ts")];
        let map = fingerprints(&[("/project/src/a.ts", SourceFingerprint::new(10, 1))]);
        let mut cached = manifest(&files, mode(), &map);
        let current = manifest(&files, mode(), &map);

        cached.version = GRAPH_CACHE_VERSION + 1;

        assert!(!cached.matches_inputs(&current));
    }
}
