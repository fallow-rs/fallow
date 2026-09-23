//! Persisted graph-cache identity contracts and on-disk store.
//!
//! The manifest types here define the invalidation surface a persisted graph
//! cache must satisfy before a cached graph can be trusted. Exact manifest hits
//! can reuse a previously-built `ModuleGraph`; stable-key resolver hits can
//! reuse resolver output and rebuild the graph with current `FileId`s.

use std::sync::Arc;

use std::path::{Path, PathBuf};

use fallow_types::cache_rejection::CacheRejection;
use fallow_types::discover::{DiscoveredFile, FileId, StableFileKey};
use fallow_types::extract::{ImportInfo, ReExportInfo};
use oxc_span::Span;

use crate::resolve::{
    ResolveResult, ResolvedImport, ResolvedModule, ResolvedProject, ResolvedReExport,
    ResolvedReplacedModuleTarget,
};

mod store;

pub use store::{GRAPH_CACHE_FILE, GraphCacheStore};

/// Persisted graph cache schema version.
///
/// Bump this whenever the serialized shape of the persisted graph (any of the
/// graph types that derive serde for the cache, the manifest types, or the
/// store envelope) changes, so a stale `graph-cache.bin` written by an older
/// binary is rejected rather than deserialized into the wrong shape.
///
/// Bump it for import-resolution semantics changes too, not only for wire-shape
/// changes. The manifest compares this constant, the cache mode, and per-file
/// fingerprints, and carries no binary version, so a cache written before a
/// classification change replays the old classification verbatim on an
/// unmodified tree and silently hides the new behaviour. The same applies to
/// plugin config extraction, which seeds entry points and path aliases.
///
/// Never reuse a version number that a published build wrote, even from a
/// development commit. Git history and the CHANGELOG record the reason for
/// each bump.
pub const GRAPH_CACHE_VERSION: u32 = 52;

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
    /// Resolved from CommonJS to a file within the project.
    CommonJsInternalModule(StableFileKey),
    /// Resolved to a project file through a framework convention auto-import.
    SyntheticAutoImport(StableFileKey),
    /// Resolved to a workspace or self package source file.
    InternalPackageModule {
        /// Stable source file reached by the package map.
        key: StableFileKey,
        /// Package name that was used in the import specifier.
        package_name: String,
    },
    /// Resolved from CommonJS to workspace or self-package source.
    CommonJsInternalPackageModule {
        /// Stable source file reached by the package map.
        key: StableFileKey,
        /// Package name used in the require specifier.
        package_name: String,
    },
    /// Resolved to a file outside the project.
    ExternalFile(PathBuf),
    /// Bare specifier.
    NpmPackage(String),
    /// Bare specifier referenced through CommonJS `require()`.
    CommonJsNpmPackage(String),
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
            ResolveResult::CommonJsInternalModule(file_id) => {
                Self::CommonJsInternalModule(key_by_file_id.get(file_id)?.clone())
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
            ResolveResult::CommonJsInternalPackageModule {
                file_id,
                package_name,
            } => Self::CommonJsInternalPackageModule {
                key: key_by_file_id.get(file_id)?.clone(),
                package_name: package_name.clone(),
            },
            ResolveResult::ExternalFile(path) => Self::ExternalFile(path.clone()),
            ResolveResult::NpmPackage(package_name) => Self::NpmPackage(package_name.clone()),
            ResolveResult::CommonJsNpmPackage(package_name) => {
                Self::CommonJsNpmPackage(package_name.clone())
            }
            ResolveResult::Unresolvable(specifier) => Self::Unresolvable(specifier.clone()),
        })
    }

    fn into_resolve_result(
        self,
        id_by_key: &rustc_hash::FxHashMap<StableFileKey, FileId>,
    ) -> Option<ResolveResult> {
        Some(match self {
            Self::InternalModule(key) => ResolveResult::InternalModule(*id_by_key.get(&key)?),
            Self::CommonJsInternalModule(key) => {
                ResolveResult::CommonJsInternalModule(*id_by_key.get(&key)?)
            }
            Self::SyntheticAutoImport(key) => {
                ResolveResult::SyntheticAutoImport(*id_by_key.get(&key)?)
            }
            Self::InternalPackageModule { key, package_name } => {
                ResolveResult::InternalPackageModule {
                    file_id: *id_by_key.get(&key)?,
                    package_name,
                }
            }
            Self::CommonJsInternalPackageModule { key, package_name } => {
                ResolveResult::CommonJsInternalPackageModule {
                    file_id: *id_by_key.get(&key)?,
                    package_name,
                }
            }
            Self::ExternalFile(path) => ResolveResult::ExternalFile(path),
            Self::NpmPackage(package_name) => ResolveResult::NpmPackage(package_name),
            Self::CommonJsNpmPackage(package_name) => {
                ResolveResult::CommonJsNpmPackage(package_name)
            }
            Self::Unresolvable(specifier) => ResolveResult::Unresolvable(specifier),
        })
    }
}

/// Cached import edge that can be restored without re-running resolution.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CachedResolvedImport {
    /// Import metadata mirrored from extraction or resolver synthesis.
    info: CachedImportInfo,
    /// Resolved target for this import edge.
    target: CachedResolveResult,
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
    info: CachedReExportInfo,
    /// Resolved target for this re-export source.
    target: CachedResolveResult,
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
    source: String,
    /// Imported binding shape.
    imported_name: fallow_types::extract::ImportedName,
    /// Local binding name.
    local_name: String,
    /// Whether this import is type-only.
    is_type_only: bool,
    /// Whether this whole-module import only carries the target's type meanings.
    is_type_only_star: bool,
    /// Whether this import originated from a style context.
    from_style: bool,
    /// Span of the full import declaration.
    span: [u32; 2],
    /// Span of the import source literal.
    source_span: [u32; 2],
}

impl From<&ImportInfo> for CachedImportInfo {
    fn from(info: &ImportInfo) -> Self {
        Self {
            source: info.source.clone(),
            imported_name: info.imported_name.clone(),
            local_name: info.local_name.clone(),
            is_type_only: info.is_type_only,
            is_type_only_star: info.is_type_only_star,
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
            is_type_only_star: info.is_type_only_star,
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
    source: String,
    /// Imported name from the source module.
    imported_name: String,
    /// Exported name from this module.
    exported_name: String,
    /// Whether this re-export is type-only.
    is_type_only: bool,
    /// Span of the re-export declaration.
    span: [u32; 2],
    /// Span of the enclosing re-export statement.
    statement_span: [u32; 2],
    /// Span of the source string literal.
    source_span: [u32; 2],
}

impl From<&ReExportInfo> for CachedReExportInfo {
    fn from(info: &ReExportInfo) -> Self {
        Self {
            source: info.source.clone(),
            imported_name: info.imported_name.clone(),
            exported_name: info.exported_name.clone(),
            is_type_only: info.is_type_only,
            span: span_to_pair(info.span),
            statement_span: span_to_pair(info.statement_span),
            source_span: span_to_pair(info.source_span),
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
            statement_span: pair_to_span(info.statement_span),
            source_span: pair_to_span(info.source_span),
        }
    }
}

/// Cached resolver output for one module.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CachedResolvedModule {
    /// Stable identity of the source module.
    key: StableFileKey,
    /// Static import and require edges after resolution.
    resolved_imports: Vec<CachedResolvedImport>,
    /// Literal dynamic import edges after resolution.
    resolved_dynamic_imports: Vec<CachedResolvedImport>,
    /// Re-export source edges after resolution.
    re_exports: Vec<CachedResolvedReExport>,
    /// Dynamic import pattern targets, aligned with current extracted patterns.
    resolved_dynamic_pattern_targets: Vec<Vec<StableFileKey>>,
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

/// Stable-key cache form of one resolved project-internal replacement.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
struct CachedResolvedReplacedModuleTarget {
    source_key: StableFileKey,
    target_key: StableFileKey,
}

impl CachedResolvedReplacedModuleTarget {
    fn from_resolved(
        target: ResolvedReplacedModuleTarget,
        key_by_file_id: &rustc_hash::FxHashMap<FileId, StableFileKey>,
    ) -> Option<Self> {
        Some(Self {
            source_key: key_by_file_id.get(&target.source_file)?.clone(),
            target_key: key_by_file_id.get(&target.target_file)?.clone(),
        })
    }

    fn into_resolved(
        self,
        id_by_key: &rustc_hash::FxHashMap<StableFileKey, FileId>,
    ) -> Option<ResolvedReplacedModuleTarget> {
        Some(ResolvedReplacedModuleTarget {
            source_file: *id_by_key.get(&self.source_key)?,
            target_file: *id_by_key.get(&self.target_key)?,
        })
    }
}

/// Cache-friendly mirror of the complete resolver output.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CachedResolvedProject {
    modules: Vec<CachedResolvedModule>,
    replaced_module_targets: Vec<CachedResolvedReplacedModuleTarget>,
}

/// Convert a resolved project into the compact graph-cache resolver payload.
#[must_use]
pub fn cache_resolved_project(
    root: &Path,
    files: &[DiscoveredFile],
    resolved: &ResolvedProject,
) -> Option<CachedResolvedProject> {
    let key_by_file_id = stable_key_by_file_id(root, files);
    let modules = resolved
        .modules
        .iter()
        .map(|module| CachedResolvedModule::from_resolved(module, &key_by_file_id))
        .collect::<Option<Vec<_>>>()?;
    let replaced_module_targets = resolved
        .replaced_module_targets
        .iter()
        .copied()
        .map(|target| CachedResolvedReplacedModuleTarget::from_resolved(target, &key_by_file_id))
        .collect::<Option<Vec<_>>>()?;
    Some(CachedResolvedProject {
        modules,
        replaced_module_targets,
    })
}

/// Restore a resolved project from cached resolver payloads and current parsed modules.
///
/// Returns `None` if the payload no longer aligns with the current parse result.
/// A normal graph-cache manifest hit should keep these aligned; this extra check
/// keeps corrupt or hand-edited cache files on the safe miss path.
#[must_use]
pub fn restore_resolved_project(
    root: &Path,
    modules: &[fallow_types::extract::ModuleInfo],
    files: &[DiscoveredFile],
    cached: &CachedResolvedProject,
) -> Option<ResolvedProject> {
    if modules.len() != cached.modules.len() {
        return None;
    }

    let mut indexes = RestoreResolvedModuleIndexes::new(root, modules, files);
    let resolved_modules = cached
        .modules
        .iter()
        .map(|entry| restore_cached_resolved_module(entry, &mut indexes))
        .collect::<Option<Vec<_>>>()?;
    let mut replaced_module_targets = cached
        .replaced_module_targets
        .iter()
        .cloned()
        .map(|target| target.into_resolved(&indexes.file_ids))
        .collect::<Option<Vec<_>>>()?;
    replaced_module_targets
        .sort_unstable_by_key(|target| (target.source_file.0, target.target_file.0));
    replaced_module_targets.dedup();
    Some(ResolvedProject {
        modules: resolved_modules,
        replaced_module_targets,
    })
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
        exports: Arc::clone(&module.exports),
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
        member_accesses: Arc::clone(&module.member_accesses),
        semantic_facts: Arc::clone(&module.semantic_facts),
        whole_object_uses: Arc::clone(&module.whole_object_uses),
        has_cjs_exports: module.has_cjs_exports,
        has_angular_component_template_url: module.has_angular_component_template_url,
        unused_import_bindings: module.unused_import_bindings.iter().cloned().collect(),
        type_referenced_import_bindings: module.type_referenced_import_bindings.clone(),
        value_referenced_import_bindings: module.value_referenced_import_bindings.clone(),
        namespace_object_aliases: module.namespace_object_aliases.clone(),
        exported_factory_returns: Arc::clone(&module.exported_factory_returns),
        exported_factory_return_object_shapes: Arc::clone(
            &module.exported_factory_return_object_shapes,
        ),
        type_member_types: Arc::clone(&module.type_member_types),
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
    /// xxh3 hash of the parsed source content.
    ///
    /// Content, not `(mtime, size)`: the metadata pair is writer-controlled and
    /// a same-length rewrite with a restored mtime leaves it unchanged, so a
    /// metadata-keyed manifest can hand back the previous run's graph for a
    /// tree that no longer matches it. The hash is free here because the parse
    /// stage already computed it, and unlike a ctime it survives `cp -Rp` and
    /// CI cache restores, which is what makes a warm graph reusable across
    /// checkouts at all.
    ///
    /// `0` when the run produced no module for the file (an unreadable source).
    pub content_hash: u64,
}

impl GraphCacheFile {
    /// Build a graph-cache file row from a discovered file and content hash.
    #[must_use]
    fn from_discovered_file(root: &Path, file: &DiscoveredFile, content_hash: u64) -> Self {
        Self {
            key: StableFileKey::from_root_relative(root, &file.path),
            file_id: file.id,
            content_hash,
        }
    }
}

/// Manifest inputs required to trust a persisted graph cache entry.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct GraphCacheManifest {
    /// Root anchoring the absolute paths retained in the graph and resolver payload.
    pub root: PathBuf,
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
    fn new(root: &Path, mode: GraphCacheMode, mut files: Vec<GraphCacheFile>) -> Self {
        sort_files(&mut files);
        Self {
            root: root.to_path_buf(),
            version: GRAPH_CACHE_VERSION,
            mode,
            files,
        }
    }

    /// Build a manifest from discovered files plus a content-hash provider.
    pub fn from_discovered_files(
        root: &Path,
        files: &[DiscoveredFile],
        mode: GraphCacheMode,
        mut content_hash_for_file: impl FnMut(&DiscoveredFile) -> u64,
    ) -> Self {
        let rows = files
            .iter()
            .map(|file| {
                GraphCacheFile::from_discovered_file(root, file, content_hash_for_file(file))
            })
            .collect();
        Self::new(root, mode, rows)
    }

    /// True when a persisted manifest matches the current graph inputs.
    #[must_use]
    pub fn matches_inputs(&self, current: &Self) -> bool {
        self.version == GRAPH_CACHE_VERSION
            && current.version == GRAPH_CACHE_VERSION
            && self.root == current.root
            && self.mode == current.mode
            && self.files == current.files
    }

    /// Name the first input dimension that differs from the current run, for
    /// a manifest whose resolution inputs may differ from the current run.
    ///
    /// The caller decides graph reuse after a successful, fully paid load, so
    /// this is the most expensive refusal in the pipeline and used to be the
    /// only silent one. `None` means the resolution inputs actually match and
    /// the caller should not be asking.
    #[must_use]
    pub fn classify_resolution_mismatch(&self, current: &Self) -> Option<CacheRejection> {
        if self.version != GRAPH_CACHE_VERSION || current.version != GRAPH_CACHE_VERSION {
            return Some(CacheRejection::VersionMismatch);
        }
        if self.root != current.root {
            return Some(CacheRejection::RootMismatch);
        }
        if self.mode != current.mode {
            return Some(CacheRejection::ModeMismatch);
        }
        if self.files.len() != current.files.len()
            || self
                .files
                .iter()
                .zip(current.files.iter())
                .any(|(cached, current)| cached.key != current.key)
        {
            return Some(CacheRejection::FileSetChanged);
        }
        self.files
            .iter()
            .zip(current.files.iter())
            .any(|(cached, current)| cached.content_hash != current.content_hash)
            .then_some(CacheRejection::FingerprintChanged)
    }
}

fn sort_files(files: &mut [GraphCacheFile]) {
    files.sort_unstable_by(|a, b| a.key.cmp(&b.key));
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use fallow_types::discover::FileId;
    use fallow_types::extract::{DynamicImportPattern, ModuleInfo, ModuleLoadMechanism};
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

    fn content_hashes(pairs: &[(&str, u64)]) -> FxHashMap<PathBuf, u64> {
        pairs
            .iter()
            .map(|(path, hash)| (PathBuf::from(path), *hash))
            .collect()
    }

    fn manifest(
        files: &[DiscoveredFile],
        mode: GraphCacheMode,
        map: &FxHashMap<PathBuf, u64>,
    ) -> GraphCacheManifest {
        GraphCacheManifest::from_discovered_files(Path::new("/project"), files, mode, |file| {
            *map.get(&file.path).unwrap()
        })
    }

    fn import_info(source: &str) -> ImportInfo {
        ImportInfo {
            source: source.to_string(),
            imported_name: fallow_types::extract::ImportedName::SideEffect,
            local_name: String::new(),
            is_type_only: false,
            is_type_only_star: false,
            from_style: false,
            span: Span::new(0, 0),
            source_span: Span::new(0, 0),
        }
    }

    #[test]
    fn cached_import_info_round_trip_keeps_every_field() {
        // The resolver payload is replayed into a fresh graph build, so any
        // field the mirror drops silently changes analysis behaviour behind a
        // cache hit. Compare the debug rendering so a future field that is not
        // mirrored fails here instead of in a warm-only output diff.
        let original = ImportInfo {
            source: "./impl".to_string(),
            imported_name: fallow_types::extract::ImportedName::Namespace,
            local_name: "ns".to_string(),
            is_type_only: true,
            is_type_only_star: true,
            from_style: true,
            span: Span::new(3, 41),
            source_span: Span::new(17, 25),
        };

        let restored = ImportInfo::from(CachedImportInfo::from(&original));

        assert_eq!(format!("{restored:?}"), format!("{original:?}"));
    }

    #[test]
    fn manifest_sorts_by_stable_file_key() {
        let files = vec![file(0, "/project/src/z.ts"), file(1, "/project/src/a.ts")];
        let map = content_hashes(&[("/project/src/z.ts", 10), ("/project/src/a.ts", 20)]);

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
        let map = content_hashes(&[("/project/src/a.ts", 10), ("/project/src/c.ts", 20)]);

        let cached = manifest(&before, mode(), &map);
        let current = manifest(&after, mode(), &map);

        assert!(
            !cached.matches_inputs(&current),
            "the persisted graph is still FileId-keyed, so FileId shifts cannot trust it"
        );
        assert_eq!(
            cached.classify_resolution_mismatch(&current),
            None,
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
    fn cached_resolve_result_preserves_commonjs_provenance() {
        let key = StableFileKey::from_root_relative(
            Path::new("/project"),
            Path::new("/project/src/dependency.ts"),
        );
        let key_by_file_id = FxHashMap::from_iter([(FileId(3), key.clone())]);
        let id_by_key = FxHashMap::from_iter([(key, FileId(8))]);

        let cached = CachedResolveResult::from_resolve_result(
            &ResolveResult::CommonJsInternalModule(FileId(3)),
            &key_by_file_id,
        )
        .expect("CommonJS target should map to a stable key");
        let restored = cached
            .into_resolve_result(&id_by_key)
            .expect("stable key should map to the current FileId");

        assert!(matches!(
            restored,
            ResolveResult::CommonJsInternalModule(FileId(8))
        ));
    }

    #[test]
    fn cached_resolve_result_preserves_commonjs_bare_package_provenance() {
        let cached = CachedResolveResult::from_resolve_result(
            &ResolveResult::CommonJsNpmPackage("shared-package".to_string()),
            &FxHashMap::default(),
        )
        .expect("bare CommonJS package should not need a stable file key");
        let restored = cached
            .into_resolve_result(&FxHashMap::default())
            .expect("bare CommonJS package should restore without a file map");

        assert!(matches!(
            restored,
            ResolveResult::CommonJsNpmPackage(package_name)
                if package_name == "shared-package"
        ));
    }

    #[test]
    fn cache_resolved_project_rejects_unknown_internal_targets() {
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
        let project = ResolvedProject {
            modules: vec![module],
            replaced_module_targets: Vec::new(),
        };

        let cached = cache_resolved_project(Path::new("/project"), &files, &project);

        assert!(cached.is_none());
    }

    #[test]
    fn cached_dynamic_pattern_targets_preserve_empty_rows() {
        let files = vec![file(0, "/project/src/app.ts")];
        let pattern = DynamicImportPattern {
            prefix: "./missing/".into(),
            suffix: Some(".ts".into()),
            span: Span::new(0, 1),
            mechanism: ModuleLoadMechanism::EsModule,
        };
        let module = ModuleInfo {
            dynamic_import_patterns: vec![pattern.clone()],
            ..ModuleInfo::empty(FileId(0))
        };
        let resolved = ResolvedProject {
            modules: vec![ResolvedModule {
                file_id: FileId(0),
                path: PathBuf::from("/project/src/app.ts"),
                resolved_dynamic_patterns: vec![(pattern, Vec::new())],
                ..ResolvedModule::default()
            }],
            replaced_module_targets: Vec::new(),
        };

        let cached = cache_resolved_project(Path::new("/project"), &files, &resolved)
            .expect("all dynamic pattern targets should have stable keys");
        let restored = restore_resolved_project(
            Path::new("/project"),
            std::slice::from_ref(&module),
            &files,
            &cached,
        )
        .expect("cached dynamic pattern rows should align with extracted patterns");

        assert_eq!(restored.modules[0].resolved_dynamic_patterns.len(), 1);
        assert!(
            restored.modules[0].resolved_dynamic_patterns[0]
                .1
                .is_empty()
        );

        let mut sparse_cached = cached;
        sparse_cached.modules[0]
            .resolved_dynamic_pattern_targets
            .clear();
        assert!(
            restore_resolved_project(
                Path::new("/project"),
                std::slice::from_ref(&module),
                &files,
                &sparse_cached,
            )
            .is_none(),
            "version-50 sparse rows must take the safe cache-miss path"
        );
    }

    #[test]
    fn cached_replaced_target_remaps_both_file_ids_by_stable_key() {
        let source_key = StableFileKey::from_root_relative(
            Path::new("/project"),
            Path::new("/project/src/example.test.ts"),
        );
        let target_key = StableFileKey::from_root_relative(
            Path::new("/project"),
            Path::new("/project/src/dependency.ts"),
        );
        let key_by_file_id = FxHashMap::from_iter([
            (FileId(2), source_key.clone()),
            (FileId(3), target_key.clone()),
        ]);
        let id_by_key = FxHashMap::from_iter([(source_key, FileId(8)), (target_key, FileId(9))]);
        let resolved = ResolvedReplacedModuleTarget {
            source_file: FileId(2),
            target_file: FileId(3),
        };

        let cached = CachedResolvedReplacedModuleTarget::from_resolved(resolved, &key_by_file_id)
            .expect("both file ids should map to stable keys");
        let restored = cached
            .into_resolved(&id_by_key)
            .expect("both stable keys should map to current file ids");

        assert_eq!(
            restored,
            ResolvedReplacedModuleTarget {
                source_file: FileId(8),
                target_file: FileId(9),
            }
        );
    }

    #[test]
    fn cache_resolved_project_rejects_unknown_replacement_targets() {
        let files = vec![file(0, "/project/src/example.test.ts")];
        let project = ResolvedProject {
            modules: vec![ResolvedModule {
                file_id: FileId(0),
                path: PathBuf::from("/project/src/example.test.ts"),
                ..ResolvedModule::default()
            }],
            replaced_module_targets: vec![ResolvedReplacedModuleTarget {
                source_file: FileId(0),
                target_file: FileId(1),
            }],
        };

        let cached = cache_resolved_project(Path::new("/project"), &files, &project);

        assert!(cached.is_none());
    }

    #[test]
    fn manifest_misses_on_content_change() {
        let files = vec![file(0, "/project/src/a.ts")];
        let cached_map = content_hashes(&[("/project/src/a.ts", 10)]);
        let current_map = content_hashes(&[("/project/src/a.ts", 11)]);

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
        let map = content_hashes(&[("/project/src/a.ts", 10), ("/project/src/deleted.ts", 20)]);

        let cached = manifest(&before, mode(), &map);
        let current = manifest(&after, mode(), &map);

        assert!(!cached.matches_inputs(&current));
    }

    #[test]
    fn manifest_misses_on_file_rename_with_same_content() {
        let before = vec![file(0, "/project/src/old.ts")];
        let after = vec![file(0, "/project/src/new.ts")];
        let map = content_hashes(&[("/project/src/old.ts", 10), ("/project/src/new.ts", 10)]);

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
        let map = content_hashes(&[
            ("/project/packages/app/src/index.ts", 10),
            ("/project/packages/shared/src/index.ts", 20),
        ]);

        let cached = manifest(&full_project, mode(), &map);
        let current = manifest(&workspace_scoped, mode(), &map);

        assert!(!cached.matches_inputs(&current));
        assert_eq!(
            cached.classify_resolution_mismatch(&current),
            Some(CacheRejection::FileSetChanged)
        );
    }

    #[test]
    fn manifest_misses_on_mode_change() {
        let files = vec![file(0, "/project/src/a.ts")];
        let map = content_hashes(&[("/project/src/a.ts", 10)]);

        let cached = manifest(&files, mode(), &map);
        let current = manifest(&files, GraphCacheMode::new(1, 99, 3), &map);

        assert!(!cached.matches_inputs(&current));
    }

    #[test]
    fn manifest_misses_on_version_change() {
        let files = vec![file(0, "/project/src/a.ts")];
        let map = content_hashes(&[("/project/src/a.ts", 10)]);
        let mut cached = manifest(&files, mode(), &map);
        let current = manifest(&files, mode(), &map);

        cached.version = GRAPH_CACHE_VERSION + 1;

        assert!(!cached.matches_inputs(&current));
    }
}
