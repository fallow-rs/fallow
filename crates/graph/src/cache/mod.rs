//! Persisted graph-cache identity contracts and on-disk store.
//!
//! The manifest types here define the invalidation surface a persisted graph
//! cache must satisfy before a cached graph can be trusted. Exact manifest hits
//! can reuse a previously-built `ModuleGraph`; stable-key resolver hits can
//! reuse resolver output and rebuild the graph with current `FileId`s.

use std::path::{Path, PathBuf};

use fallow_types::discover::{DiscoveredFile, FileId, StableFileKey};
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
pub const GRAPH_CACHE_VERSION: u32 = 3;

/// Cached form of a resolved target.
///
/// Internal targets are stored by stable file key, not by `FileId`, so resolver
/// output can be reused across a future FileId assignment shift. The persisted
/// `ModuleGraph` itself is still `FileId`-keyed; callers may only trust the
/// cached graph when the manifest's `file_id` assignments match, but they may
/// remap this resolver payload and rebuild the graph.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum CachedResolveResult {
    /// Resolved to a file within the project.
    InternalModule(StableFileKey),
    /// Resolved to a project file through a framework convention auto-import.
    SyntheticAutoImport(StableFileKey),
    /// Resolved to a workspace or self package source file.
    InternalPackageModule {
        /// Stable source file reached by the package map.
        key: StableFileKey,
        /// Package name that was used in the import specifier.
        package_name: String,
    },
    /// Resolved to a file outside the project.
    ExternalFile(PathBuf),
    /// Bare specifier.
    NpmPackage(String),
    /// Could not resolve.
    Unresolvable(String),
}

impl CachedResolveResult {
    fn from_resolve_result(
        target: &ResolveResult,
        key_by_file_id: &rustc_hash::FxHashMap<FileId, StableFileKey>,
    ) -> Option<Self> {
        Some(match target {
            ResolveResult::InternalModule(file_id) => {
                Self::InternalModule(key_by_file_id.get(file_id)?.clone())
            }
            ResolveResult::SyntheticAutoImport(file_id) => {
                Self::SyntheticAutoImport(key_by_file_id.get(file_id)?.clone())
            }
            ResolveResult::InternalPackageModule {
                file_id,
                package_name,
            } => Self::InternalPackageModule {
                key: key_by_file_id.get(file_id)?.clone(),
                package_name: package_name.clone(),
            },
            ResolveResult::ExternalFile(path) => Self::ExternalFile(path.clone()),
            ResolveResult::NpmPackage(package_name) => Self::NpmPackage(package_name.clone()),
            ResolveResult::Unresolvable(specifier) => Self::Unresolvable(specifier.clone()),
        })
    }

    fn into_resolve_result(
        self,
        id_by_key: &rustc_hash::FxHashMap<StableFileKey, FileId>,
    ) -> Option<ResolveResult> {
        Some(match self {
            Self::InternalModule(key) => ResolveResult::InternalModule(*id_by_key.get(&key)?),
            Self::SyntheticAutoImport(key) => {
                ResolveResult::SyntheticAutoImport(*id_by_key.get(&key)?)
            }
            Self::InternalPackageModule { key, package_name } => {
                ResolveResult::InternalPackageModule {
                    file_id: *id_by_key.get(&key)?,
                    package_name,
                }
            }
            Self::ExternalFile(path) => ResolveResult::ExternalFile(path),
            Self::NpmPackage(package_name) => ResolveResult::NpmPackage(package_name),
            Self::Unresolvable(specifier) => ResolveResult::Unresolvable(specifier),
        })
    }
}

/// Cached import edge that can be restored without re-running resolution.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CachedResolvedImport {
    /// Import metadata mirrored from extraction or resolver synthesis.
    pub info: CachedImportInfo,
    /// Resolved target for this import edge.
    pub target: CachedResolveResult,
}

impl CachedResolvedImport {
    fn from_resolved(
        import: &ResolvedImport,
        key_by_file_id: &rustc_hash::FxHashMap<FileId, StableFileKey>,
    ) -> Option<Self> {
        Some(Self {
            info: CachedImportInfo::from(&import.info),
            target: CachedResolveResult::from_resolve_result(&import.target, key_by_file_id)?,
        })
    }

    fn into_resolved(
        self,
        id_by_key: &rustc_hash::FxHashMap<StableFileKey, FileId>,
    ) -> Option<ResolvedImport> {
        Some(ResolvedImport {
            info: self.info.into(),
            target: self.target.into_resolve_result(id_by_key)?,
        })
    }
}

/// Cached re-export edge that can be restored without re-running resolution.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CachedResolvedReExport {
    /// Re-export metadata mirrored from extraction.
    pub info: CachedReExportInfo,
    /// Resolved target for this re-export source.
    pub target: CachedResolveResult,
}

impl CachedResolvedReExport {
    fn from_resolved(
        re_export: &ResolvedReExport,
        key_by_file_id: &rustc_hash::FxHashMap<FileId, StableFileKey>,
    ) -> Option<Self> {
        Some(Self {
            info: CachedReExportInfo::from(&re_export.info),
            target: CachedResolveResult::from_resolve_result(&re_export.target, key_by_file_id)?,
        })
    }

    fn into_resolved(
        self,
        id_by_key: &rustc_hash::FxHashMap<StableFileKey, FileId>,
    ) -> Option<ResolvedReExport> {
        Some(ResolvedReExport {
            info: self.info.into(),
            target: self.target.into_resolve_result(id_by_key)?,
        })
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
    /// Stable identity of the source module.
    pub key: StableFileKey,
    /// Static import and require edges after resolution.
    pub resolved_imports: Vec<CachedResolvedImport>,
    /// Literal dynamic import edges after resolution.
    pub resolved_dynamic_imports: Vec<CachedResolvedImport>,
    /// Re-export source edges after resolution.
    pub re_exports: Vec<CachedResolvedReExport>,
    /// Dynamic import pattern targets, aligned with current extracted patterns.
    pub resolved_dynamic_pattern_targets: Vec<Vec<StableFileKey>>,
}

impl CachedResolvedModule {
    fn from_resolved(
        module: &ResolvedModule,
        key_by_file_id: &rustc_hash::FxHashMap<FileId, StableFileKey>,
    ) -> Option<Self> {
        Some(Self {
            key: key_by_file_id.get(&module.file_id)?.clone(),
            resolved_imports: module
                .resolved_imports
                .iter()
                .map(|import| CachedResolvedImport::from_resolved(import, key_by_file_id))
                .collect::<Option<Vec<_>>>()?,
            resolved_dynamic_imports: module
                .resolved_dynamic_imports
                .iter()
                .map(|import| CachedResolvedImport::from_resolved(import, key_by_file_id))
                .collect::<Option<Vec<_>>>()?,
            re_exports: module
                .re_exports
                .iter()
                .map(|re_export| CachedResolvedReExport::from_resolved(re_export, key_by_file_id))
                .collect::<Option<Vec<_>>>()?,
            resolved_dynamic_pattern_targets: module
                .resolved_dynamic_patterns
                .iter()
                .map(|(_, targets)| {
                    targets
                        .iter()
                        .map(|target| key_by_file_id.get(target).cloned())
                        .collect::<Option<Vec<_>>>()
                })
                .collect::<Option<Vec<_>>>()?,
        })
    }
}

/// Convert resolved modules into the compact graph-cache resolver payload.
#[must_use]
pub fn cache_resolved_modules(
    root: &Path,
    files: &[DiscoveredFile],
    resolved: &[ResolvedModule],
) -> Option<Vec<CachedResolvedModule>> {
    let key_by_file_id = stable_key_by_file_id(root, files);
    resolved
        .iter()
        .map(|module| CachedResolvedModule::from_resolved(module, &key_by_file_id))
        .collect()
}

/// Restore resolved modules from cached resolver payloads and current parsed modules.
///
/// Returns `None` if the payload no longer aligns with the current parse result.
/// A normal graph-cache manifest hit should keep these aligned; this extra check
/// keeps corrupt or hand-edited cache files on the safe miss path.
#[must_use]
pub fn restore_resolved_modules(
    root: &Path,
    modules: &[fallow_types::extract::ModuleInfo],
    files: &[DiscoveredFile],
    cached: &[CachedResolvedModule],
) -> Option<Vec<ResolvedModule>> {
    if modules.len() != cached.len() {
        return None;
    }

    let mut indexes = RestoreResolvedModuleIndexes::new(root, modules, files);
    cached
        .iter()
        .map(|entry| restore_cached_resolved_module(entry, &mut indexes))
        .collect()
}

struct RestoreResolvedModuleIndexes<'a> {
    file_ids: rustc_hash::FxHashMap<StableFileKey, FileId>,
    modules: rustc_hash::FxHashMap<StableFileKey, &'a fallow_types::extract::ModuleInfo>,
    paths: rustc_hash::FxHashMap<StableFileKey, std::path::PathBuf>,
}

impl<'a> RestoreResolvedModuleIndexes<'a> {
    fn new(
        root: &Path,
        modules: &'a [fallow_types::extract::ModuleInfo],
        files: &[DiscoveredFile],
    ) -> Self {
        let key_by_file_id = stable_key_by_file_id(root, files);
        let id_by_key: rustc_hash::FxHashMap<_, _> = key_by_file_id
            .iter()
            .map(|(file_id, key)| (key.clone(), *file_id))
            .collect();
        let by_key: rustc_hash::FxHashMap<_, _> = modules
            .iter()
            .filter_map(|module| {
                key_by_file_id
                    .get(&module.file_id)
                    .map(|key| (key.clone(), module))
            })
            .collect();
        let path_by_key: rustc_hash::FxHashMap<_, _> = files
            .iter()
            .map(|file| {
                (
                    StableFileKey::from_root_relative(root, &file.path),
                    file.path.clone(),
                )
            })
            .collect();

        Self {
            file_ids: id_by_key,
            modules: by_key,
            paths: path_by_key,
        }
    }
}

fn restore_cached_resolved_module(
    entry: &CachedResolvedModule,
    indexes: &mut RestoreResolvedModuleIndexes<'_>,
) -> Option<ResolvedModule> {
    let module = indexes.modules.remove(&entry.key)?;
    let path = indexes.paths.get(&entry.key)?.clone();
    let resolved_dynamic_pattern_targets =
        restore_dynamic_pattern_targets(entry, module, &indexes.file_ids)?;

    Some(ResolvedModule {
        file_id: module.file_id,
        path,
        exports: module.exports.clone(),
        re_exports: entry
            .re_exports
            .iter()
            .cloned()
            .map(|re_export| re_export.into_resolved(&indexes.file_ids))
            .collect::<Option<Vec<_>>>()?,
        resolved_imports: entry
            .resolved_imports
            .iter()
            .cloned()
            .map(|import| import.into_resolved(&indexes.file_ids))
            .collect::<Option<Vec<_>>>()?,
        resolved_dynamic_imports: entry
            .resolved_dynamic_imports
            .iter()
            .cloned()
            .map(|import| import.into_resolved(&indexes.file_ids))
            .collect::<Option<Vec<_>>>()?,
        resolved_dynamic_patterns: module
            .dynamic_import_patterns
            .iter()
            .cloned()
            .zip(resolved_dynamic_pattern_targets)
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
        type_member_types: module.type_member_types.clone(),
    })
}

fn restore_dynamic_pattern_targets(
    entry: &CachedResolvedModule,
    module: &fallow_types::extract::ModuleInfo,
    id_by_key: &rustc_hash::FxHashMap<StableFileKey, FileId>,
) -> Option<Vec<Vec<FileId>>> {
    if entry.resolved_dynamic_pattern_targets.len() != module.dynamic_import_patterns.len() {
        return None;
    }
    entry
        .resolved_dynamic_pattern_targets
        .iter()
        .map(|targets| {
            targets
                .iter()
                .map(|key| id_by_key.get(key).copied())
                .collect::<Option<Vec<_>>>()
        })
        .collect()
}

fn stable_key_by_file_id(
    root: &Path,
    files: &[DiscoveredFile],
) -> rustc_hash::FxHashMap<FileId, StableFileKey> {
    files
        .iter()
        .map(|file| (file.id, StableFileKey::from_root_relative(root, &file.path)))
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
    /// Current in-memory identifier for the file.
    ///
    /// The stable key is the durable identity, but the persisted `ModuleGraph`
    /// is still `FileId`-keyed. Until a future graph-cache format remaps graph
    /// edges through stable keys, a changed assignment must miss rather than
    /// trust a graph whose `modules[file_id]` indexes point at different files.
    pub file_id: FileId,
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
            file_id: file.id,
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
    /// Stable file identities, current FileId assignments, and freshness metadata.
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

    /// True when a persisted resolver payload can be remapped to current FileIds.
    ///
    /// Unlike [`Self::matches_inputs`], this intentionally ignores each row's
    /// `file_id`. It is not sufficient to trust the persisted `ModuleGraph`, but
    /// it is sufficient to reuse stable-keyed resolver output and rebuild the
    /// graph with current FileIds.
    #[must_use]
    pub fn matches_resolution_inputs(&self, current: &Self) -> bool {
        self.version == GRAPH_CACHE_VERSION
            && current.version == GRAPH_CACHE_VERSION
            && self.mode == current.mode
            && self.files.len() == current.files.len()
            && self
                .files
                .iter()
                .zip(current.files.iter())
                .all(|(cached, current)| {
                    cached.key == current.key && cached.fingerprint == current.fingerprint
                })
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

    fn import_info(source: &str) -> ImportInfo {
        ImportInfo {
            source: source.to_string(),
            imported_name: fallow_types::extract::ImportedName::SideEffect,
            local_name: String::new(),
            is_type_only: false,
            from_style: false,
            span: Span::new(0, 0),
            source_span: Span::new(0, 0),
        }
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
    fn manifest_misses_on_file_id_shift_until_graph_remap_exists() {
        let before = vec![file(0, "/project/src/a.ts"), file(1, "/project/src/c.ts")];
        let after = vec![file(9, "/project/src/c.ts"), file(2, "/project/src/a.ts")];
        let map = fingerprints(&[
            ("/project/src/a.ts", SourceFingerprint::new(10, 1)),
            ("/project/src/c.ts", SourceFingerprint::new(20, 1)),
        ]);

        let cached = manifest(&before, mode(), &map);
        let current = manifest(&after, mode(), &map);

        assert!(
            !cached.matches_inputs(&current),
            "the persisted graph is still FileId-keyed, so FileId shifts cannot trust it"
        );
        assert!(
            cached.matches_resolution_inputs(&current),
            "stable-keyed resolver payloads may be remapped across FileId shifts"
        );
    }

    #[test]
    fn cached_resolve_result_remaps_internal_targets_by_stable_key() {
        let key_a = StableFileKey::from_root_relative(
            Path::new("/project"),
            Path::new("/project/src/a.ts"),
        );
        let key_b = StableFileKey::from_root_relative(
            Path::new("/project"),
            Path::new("/project/src/b.ts"),
        );
        let key_by_file_id =
            FxHashMap::from_iter([(FileId(0), key_a.clone()), (FileId(1), key_b.clone())]);
        let id_by_key = FxHashMap::from_iter([(key_a, FileId(7)), (key_b, FileId(9))]);

        let cached = CachedResolveResult::from_resolve_result(
            &ResolveResult::InternalPackageModule {
                file_id: FileId(1),
                package_name: "@scope/pkg".to_string(),
            },
            &key_by_file_id,
        )
        .expect("target file id should map to a stable key");

        let restored = cached
            .into_resolve_result(&id_by_key)
            .expect("stable key should map to current FileId");

        assert!(matches!(
            restored,
            ResolveResult::InternalPackageModule {
                file_id: FileId(9),
                ref package_name,
            } if package_name == "@scope/pkg"
        ));
    }

    #[test]
    fn cache_resolved_modules_rejects_unknown_internal_targets() {
        let files = vec![file(0, "/project/src/a.ts")];
        let module = ResolvedModule {
            file_id: FileId(0),
            path: PathBuf::from("/project/src/a.ts"),
            resolved_imports: vec![ResolvedImport {
                info: import_info("./missing"),
                target: ResolveResult::InternalModule(FileId(1)),
            }],
            ..ResolvedModule::default()
        };

        let cached = cache_resolved_modules(Path::new("/project"), &files, &[module]);

        assert!(cached.is_none());
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
        assert!(!cached.matches_resolution_inputs(&current));
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
