//! Shared trace output contracts for analysis and integration surfaces.

use std::path::PathBuf;

use serde::Serialize;

use crate::cache_rejection::CacheRejection;
use crate::duplicates::{CloneInstance, RefactoringSuggestion};
use crate::semantic::SemanticNamespace;
use crate::serde_path;
use crate::trace_chain::StarExportAmbiguity;

/// Result of tracing an export: why it is considered used or unused.
#[derive(Debug, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ExportTrace {
    /// The file containing the export.
    #[serde(serialize_with = "serde_path::serialize")]
    pub file: PathBuf,
    /// The export name being traced.
    pub export_name: String,
    /// Namespace whose references are listed for the traced export. The
    /// preferred lane wins whenever it carries a reference: `value` for a
    /// value export, `type` for a type-only one. When the preferred lane
    /// carries none and the other lane resolves to the same declaration, the
    /// other lane's references are listed and this field names it, so a value
    /// export whose only credit is a bound `import type` reports `type` with
    /// `is_used: true`. `is_used` and `direct_references` follow the listed
    /// lane only, and only reachable reference sources can credit it. Legal
    /// declaration merges share one declaration group across lanes, including
    /// an `interface` next to a same-name `class` and a `class` next to a
    /// same-name `namespace`, so references to either lane credit the merged
    /// declaration. Distinct same-name declarations outside a merge remain
    /// separate and keep the preferred lane. `semantic.target.namespace`
    /// names the lane the declaration itself occupies and can therefore differ
    /// from this field. Producers always emit the field; the schema permits
    /// omission by payloads created before namespaces were exposed.
    #[cfg_attr(feature = "schema", schemars(default))]
    pub namespace: crate::semantic::SemanticNamespace,
    /// Whether the file is reachable from an entry point.
    pub file_reachable: bool,
    /// Whether the file is an entry point.
    pub is_entry_point: bool,
    /// Whether the export is considered used.
    pub is_used: bool,
    /// Files that reference this export directly.
    pub direct_references: Vec<ExportReference>,
    /// Reachable direct references grouped by namespace. This is additive to
    /// `namespace` and `direct_references`, whose winning-lane meaning remains
    /// unchanged for backwards compatibility.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub direct_references_by_namespace: Vec<NamespacedExportReferences>,
    /// A star-export collision that makes the traced name ambiguous. When
    /// present, `is_used: false` is an abstention rather than an unused-code
    /// verdict.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub star_export_ambiguity: Option<StarExportAmbiguity>,
    /// Re-export chains that pass through this export.
    pub re_export_chains: Vec<ReExportChain>,
    /// Human-readable reason summary.
    pub reason: String,
    /// Exact checker-backed references when type-aware tracing is enabled.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub semantic: Option<crate::semantic::SemanticSymbolTrace>,
}

/// Result of tracing a class / enum / store MEMBER: the `--trace FILE:NAME`
/// fallback when `NAME` is not a top-level export but a member declared on one
/// (issue #1744). The trace runs on the module graph only, so it reports the
/// OWNING export's reachability and usage (the gating precondition for
/// member-level crediting) plus a pointer to the right `--unused-*-members`
/// command, rather than per-member crediting provenance.
#[derive(Debug, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ClassMemberTrace {
    /// The file containing the member.
    #[serde(serialize_with = "serde_path::serialize")]
    pub file: PathBuf,
    /// The member name being traced.
    pub member_name: String,
    /// The member kind: `class-method`, `class-property`, `enum-member`,
    /// `store-member`, or `namespace-member`.
    pub member_kind: String,
    /// The export that declares this member (the class / enum / store name).
    pub owner_export: String,
    /// Namespace whose references credit the owning export, mirroring
    /// [`ExportTrace::namespace`] for the export this member is declared on.
    /// `owner_is_used` and `owner_direct_references` describe that lane, so a
    /// member of a value export credited only by a bound `import type` reports
    /// `type` here. `semantic.target.namespace` names the lane the checker
    /// proof covers and can therefore differ. Producers always emit the field;
    /// the schema permits omission by payloads created before the owner
    /// namespace was exposed.
    #[cfg_attr(feature = "schema", schemars(default))]
    pub owner_namespace: crate::semantic::SemanticNamespace,
    /// Whether the owning export is considered used.
    pub owner_is_used: bool,
    /// Whether the file is reachable from an entry point.
    pub owner_file_reachable: bool,
    /// Whether the file is an entry point.
    pub owner_is_entry_point: bool,
    /// Files that reference the owning export directly.
    pub owner_direct_references: Vec<ExportReference>,
    /// Re-export chains through which the owning export is reachable. Populated
    /// so a machine consumer can tell "used via a barrel" (empty direct refs but
    /// non-empty chains) from "genuinely unreferenced".
    pub owner_re_export_chains: Vec<ReExportChain>,
    /// Human-readable reason summary plus the follow-up command to inspect the
    /// member finding.
    pub reason: String,
    /// Exact checker-backed member references when type-aware tracing is enabled.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub semantic: Option<crate::semantic::SemanticSymbolTrace>,
}

/// A direct reference to an export.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ExportReference {
    /// File that contains the reference.
    #[serde(serialize_with = "serde_path::serialize")]
    pub from_file: PathBuf,
    /// Reference kind, such as named import, default import, or re-export.
    pub kind: String,
}

/// Direct references that credit one namespace of an export binding.
#[derive(Debug, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct NamespacedExportReferences {
    /// Credited namespace.
    pub namespace: SemanticNamespace,
    /// Number of reachable references in this namespace.
    pub reference_count: usize,
    /// Reachable references in deterministic graph order.
    pub references: Vec<ExportReference>,
}

/// A re-export chain showing how an export is propagated.
#[derive(Debug, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ReExportChain {
    /// The barrel file that re-exports this symbol.
    #[serde(serialize_with = "serde_path::serialize")]
    pub barrel_file: PathBuf,
    /// The name it is re-exported as.
    pub exported_as: String,
    /// Number of references on the barrel's re-exported symbol.
    pub reference_count: usize,
}

/// Result of tracing all edges for a file.
#[derive(Debug, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct FileTrace {
    /// The traced file.
    #[serde(serialize_with = "serde_path::serialize")]
    pub file: PathBuf,
    /// Whether this file is reachable from entry points.
    pub is_reachable: bool,
    /// Whether this file is an entry point.
    pub is_entry_point: bool,
    /// Exports declared by this file.
    pub exports: Vec<TracedExport>,
    /// Files that this file imports from.
    #[serde(serialize_with = "serde_path::serialize_vec")]
    pub imports_from: Vec<PathBuf>,
    /// Files that import from this file.
    #[serde(serialize_with = "serde_path::serialize_vec")]
    pub imported_by: Vec<PathBuf>,
    /// Re-exports declared by this file.
    pub re_exports: Vec<TracedReExport>,
    /// The configs that make this file an entry point through Module
    /// Federation `exposes`, one per config. Absent when no Federation config
    /// exposes the file (issue #2796).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sources: Vec<TraceSource>,
}

/// Which configs name which files and which dependency names, collected by one
/// analysis for the trace output.
///
/// It holds plain data, so a trace looks a file up without the rules that
/// produced it.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TraceProvenance {
    /// Root-relative file path and the config that names it.
    files: Vec<(PathBuf, TraceSource)>,
    /// Dependency name and the config that names it.
    dependencies: Vec<(String, TraceSource)>,
}

impl TraceProvenance {
    /// Record that `source` names the root-relative `file`.
    pub fn push_file(&mut self, file: PathBuf, source: TraceSource) {
        if !self
            .files
            .iter()
            .any(|(known, known_source)| *known == file && *known_source == source)
        {
            self.files.push((file, source));
        }
    }

    /// Record that `source` names the dependency `name`.
    pub fn push_dependency(&mut self, name: String, source: TraceSource) {
        if !self
            .dependencies
            .iter()
            .any(|(known, known_source)| *known == name && *known_source == source)
        {
            self.dependencies.push((name, source));
        }
    }

    /// The configs that name `file`, a root-relative path.
    #[must_use]
    pub fn file_sources(&self, file: &std::path::Path) -> Vec<TraceSource> {
        self.files
            .iter()
            .filter(|(known, _)| known == file)
            .map(|(_, source)| source.clone())
            .collect()
    }

    /// The configs that name the dependency `name`.
    #[must_use]
    pub fn dependency_sources(&self, name: &str) -> Vec<TraceSource> {
        self.dependencies
            .iter()
            .filter(|(known, _)| known == name)
            .map(|(_, source)| source.clone())
            .collect()
    }
}

/// A config that names a traced file or a traced dependency, and the key that
/// names it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct TraceSource {
    /// The mechanism that names the file or the dependency:
    /// `module-federation`. The set is open.
    pub kind: String,
    /// The plugin that read the config, as it labels itself:
    /// `module-federation` for a standalone `module-federation.config.*`,
    /// or the bundler plugin (`webpack`, `rspack`, `rsbuild`, `vite`,
    /// `nextjs`) that read the same options inline from its own config.
    pub plugin: String,
    /// The file that names the file or the dependency, relative to the
    /// project root: the config file, or the source file of a Module
    /// Federation runtime call.
    #[serde(serialize_with = "serde_path::serialize")]
    pub config: PathBuf,
    /// The config key or the runtime function that names the file or the
    /// dependency: `exposes` for an exposed file, `remotes` for a remote
    /// alias, and `registerRemotes`, `loadRemote`, `init` or `createInstance`
    /// for a remote that a runtime call names. The set is open.
    pub key: String,
}

/// An export with usage information.
#[derive(Debug, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct TracedExport {
    /// Export name.
    pub name: String,
    /// Whether the export is type-only.
    pub is_type_only: bool,
    /// Number of references to this export.
    pub reference_count: usize,
    /// Files that reference this export.
    pub referenced_by: Vec<ExportReference>,
}

/// A re-export with source information.
#[derive(Debug, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct TracedReExport {
    /// Source file being re-exported from.
    #[serde(serialize_with = "serde_path::serialize")]
    pub source_file: PathBuf,
    /// Imported symbol name.
    pub imported_name: String,
    /// Exported symbol name.
    pub exported_name: String,
}

/// Result of tracing a dependency: where it is used.
#[derive(Debug, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct DependencyTrace {
    /// The dependency name being traced.
    pub package_name: String,
    /// Files that import this dependency.
    #[serde(serialize_with = "serde_path::serialize_vec")]
    pub imported_by: Vec<PathBuf>,
    /// Files that import this dependency with type-only imports.
    #[serde(serialize_with = "serde_path::serialize_vec")]
    pub type_only_imported_by: Vec<PathBuf>,
    /// Whether the dependency is invoked from package.json scripts or CI configs.
    pub used_in_scripts: bool,
    /// Whether the dependency is used at all.
    pub is_used: bool,
    /// Total import count.
    pub import_count: usize,
    /// The configs that declare this name as a Module Federation remote alias
    /// under `remotes`, one per config. A remote alias is provided by a
    /// remote container at runtime, not by an npm package. Absent when no
    /// Federation config declares the name (issue #2796).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sources: Vec<TraceSource>,
}

/// Sub-phase attribution inside the entry-point discovery stage.
///
/// `PipelineTimings::entry_points_ms` is a single opaque number; these are the
/// consecutive wall-clock spans that make it up, so a slow discovery stage can
/// be attributed instead of guessed at. The spans cover the discovery sections
/// only, so they sum to slightly less than `entry_points_ms`: the summary and
/// count that follow discovery are not attributed to any span.
#[derive(Debug, Clone, Copy, Default, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct EntryPointSpans {
    /// Root-package discovery: manual entry globs, root `package.json` fields,
    /// and the nested `package.json` scan under the conventional monorepo
    /// directories.
    pub root_ms: f64,
    /// Runtime script seed collection plus per-workspace discovery.
    pub workspaces_ms: f64,
    /// Plugin entry-point glob compilation and matching.
    pub plugins_ms: f64,
    /// The part of `plugins_ms` spent compiling plugin patterns into a glob
    /// set. Scales with active pattern count, not with project size.
    pub plugin_glob_build_ms: f64,
    /// The part of `plugins_ms` spent matching the compiled set against every
    /// discovered file. Scales with file count times pattern count.
    pub plugin_glob_match_ms: f64,
    /// Infrastructure config-file probing at the project root.
    pub infrastructure_ms: f64,
    /// Configured `dynamicallyLoaded` glob expansion. Zero when unconfigured.
    pub dynamic_ms: f64,
    /// Sorting and deduplicating the merged entry set.
    pub dedup_ms: f64,
}

/// Deterministic work counts for one dead-code pipeline run.
///
/// Every count is exact for a given project, commit and cache state. It does
/// not depend on the thread count, the machine or the load, so a regression
/// test can compare it with exact equality where a millisecond value is too
/// noisy. A count is zero when its stage did no work, for example the resolver
/// counts on a run that reused the persisted module graph.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct PipelineCounters {
    /// Source files whose bytes the parse stage read from disk. A warm cache
    /// hit on file metadata reads no bytes. `cache_misses` counts the files
    /// that were parsed.
    pub files_read: u64,
    /// Bytes of source read from disk by the parse stage.
    pub source_bytes_read: u64,
    /// Bytes of the persisted parse cache read from disk. Zero when the run
    /// had no parse cache or used `--no-cache`.
    pub parse_cache_bytes_read: u64,
    /// Bytes of stylesheet source that the parse stage passed through the CSS
    /// comment mask. The parse masks each stylesheet once, so this value is
    /// the size of the parsed stylesheets. A higher value shows a repeated
    /// mask. Zero when no stylesheet was parsed.
    pub css_masked_bytes: u64,
    /// Specifier resolutions that the import sites asked for: one for each
    /// static import binding, re-export, `require()`, `import()` and module
    /// mock. Internal retries inside the resolver are not counted here.
    pub resolve_specifier_calls: u64,
    /// Distinct `(specifier, from_style)` pairs for each importing file,
    /// summed over all files. A ratio of `resolve_specifier_calls` to this
    /// value above 1.0 shows bindings that share a specifier. The resolver
    /// runs at most once for each of these pairs. A pair that returns before
    /// the resolver, such as an external URL, makes no resolver call.
    pub unique_specifiers: u64,
    /// Calls into the module resolver, including fallback retries.
    pub oxc_resolve_calls: u64,
    /// Path canonicalize calls that import resolution needs: each direct call,
    /// plus one for each distinct path that goes through the canonicalize
    /// cache. Resolver setup is not counted.
    pub canonicalize_calls: u64,
}

/// Pipeline performance timings.
#[derive(Debug, Clone, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct PipelineTimings {
    /// Time spent discovering files.
    pub discover_files_ms: f64,
    /// Number of discovered files.
    pub file_count: usize,
    /// Time spent discovering workspaces.
    pub workspaces_ms: f64,
    /// Number of discovered workspaces.
    pub workspace_count: usize,
    /// Time spent running plugin discovery.
    pub plugins_ms: f64,
    /// Time spent analyzing package scripts and CI configuration.
    pub script_analysis_ms: f64,
    /// Wall-clock time spent parsing and extracting modules.
    pub parse_extract_ms: f64,
    /// Summed parser CPU time across workers.
    pub parse_cpu_ms: f64,
    /// The part of `parse_extract_ms` that reads and decodes the persisted
    /// parse cache. Zero with `--no-cache`.
    pub parse_cache_load_ms: f64,
    /// Number of extracted modules.
    pub module_count: usize,
    /// Number of files loaded from the parse cache.
    pub cache_hits: usize,
    /// Number of files parsed without a cache hit.
    pub cache_misses: usize,
    /// Why the persisted parse cache was not reused, when it was not. `None`
    /// means the cache was loaded; the hit and miss counts then describe how
    /// much of it applied.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cache_rejection: Option<CacheRejection>,
    /// Why the persisted module-graph cache was not reused, when it was not.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub graph_cache_rejection: Option<CacheRejection>,
    /// Time spent updating the parse cache.
    pub cache_update_ms: f64,
    /// Time spent categorizing entry points.
    pub entry_points_ms: f64,
    /// Sub-phase attribution for `entry_points_ms`.
    pub entry_point_spans: EntryPointSpans,
    /// Number of entry points considered.
    pub entry_point_count: usize,
    /// Time spent resolving imports.
    pub resolve_imports_ms: f64,
    /// Time spent building the module graph.
    pub build_graph_ms: f64,
    /// Time spent running analysis.
    pub analyze_ms: f64,
    /// Time spent running duplicate-code analysis, when included.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duplication_ms: Option<f64>,
    /// Total pipeline time.
    pub total_ms: f64,
    /// Deterministic work counts for this run.
    pub counters: PipelineCounters,
}

/// Result of computing the impact closure for a single file as the seed.
#[derive(Debug, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ImpactClosureTrace {
    /// The seed file, root-relative.
    pub seed: String,
    /// Root-relative paths transitively affected by the seed.
    pub affected_not_shown: Vec<String>,
    /// Coordination gaps between the seed and consumers.
    pub coordination_gap: Vec<ImpactClosureGap>,
}

/// Wire-version discriminator for [`ImportPathTrace`]. Independent from the
/// global `SchemaVersion`: the import-path payload versions on its own cadence,
/// like the other independently-versioned envelopes. Serializes as a string
/// `const` so JSON consumers can switch on it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub enum ImportPathTraceSchemaVersion {
    /// First release of the `fallow trace --path` shape.
    #[serde(rename = "1")]
    V1,
}

/// Result of asking how one module reaches another: the shortest import path.
///
/// `reachable` is the only field that separates "no route exists" from "the
/// route is empty because both ends are the same module". Both report
/// `hops: 0`, so a consumer must read `reachable`, never the hop count.
#[derive(Debug, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[cfg_attr(feature = "schema", schemars(title = "fallow trace --path"))]
pub struct ImportPathTrace {
    /// Wire-shape version of this payload.
    pub schema_version: ImportPathTraceSchemaVersion,
    /// The module the walk started from, root-relative.
    pub from: String,
    /// The module the walk was looking for, root-relative.
    pub to: String,
    /// Whether `to` is reachable from `from` by following import edges.
    pub reachable: bool,
    /// Number of import edges on the reported route. `0` both when the two ends
    /// are the same module and when there is no route at all.
    pub hops: usize,
    /// The route, in import order. Empty whenever `hops` is `0`.
    pub path: Vec<ImportPathHop>,
    /// Human-readable summary of the outcome.
    pub reason: String,
}

/// One import edge on an [`ImportPathTrace`].
#[derive(Debug, Serialize, PartialEq, Eq)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ImportPathHop {
    /// The importing module, root-relative.
    pub from: String,
    /// The imported module, root-relative.
    pub to: String,
    /// Whether every symbol on this edge is type-only, so the hop is erased at
    /// build time. Type-only hops are reported, never skipped: an `import type`
    /// chain is a real compile-time coupling.
    pub type_only: bool,
    /// Whether the edge carries a runtime value but no static one: the target
    /// loads only on demand (`import()`, a lazy glob or template pattern) or
    /// on another thread (a worker URL, `child_process.fork`). False for a
    /// static hop and for a type-only hop.
    pub dynamic: bool,
    /// 1-based line in `from` of the imported binding that creates this edge:
    /// the first value-carrying symbol on the import, or the first symbol when
    /// every symbol is type-only. On a multi-line import that is the binding's
    /// own line, not the `import` keyword's. Absent when the edge carries no
    /// span or the source could not be read.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub import_line: Option<u32>,
}

/// One coordination-gap entry in an [`ImpactClosureTrace`].
#[derive(Debug, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct ImpactClosureGap {
    /// Root-relative path of the consumer module.
    pub consumer_file: String,
    /// Exported symbol names the consumer references.
    pub consumed_symbols: Vec<String>,
    /// Scope note for the syntactic trace.
    pub note: String,
}

/// Result of tracing a clone: all groups containing the code at a source
/// location or addressed by a stable clone fingerprint.
#[derive(Debug, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct CloneTrace {
    /// File passed to the trace request, root-relative when a group matches.
    #[serde(serialize_with = "serde_path::serialize")]
    pub file: PathBuf,
    /// 1-based line passed to the trace request or representative group line.
    pub line: usize,
    /// The matched clone instance, if one exists.
    pub matched_instance: Option<CloneInstance>,
    /// Clone groups matched by the trace request.
    pub clone_groups: Vec<TracedCloneGroup>,
}

/// One clone group returned from a clone trace request.
#[derive(Debug, Serialize)]
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
pub struct TracedCloneGroup {
    /// Stable content fingerprint, usually `dup:<8hex>` and widened on rare
    /// report collisions.
    pub fingerprint: String,
    /// Number of tokens in the duplicated block.
    pub token_count: usize,
    /// Number of lines in the duplicated block.
    pub line_count: usize,
    /// Maximum directory-tree or same-file line distance between instances.
    pub spread: usize,
    /// Lowest all-pairs similarity for a near-miss clone group.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[cfg_attr(feature = "schema", schemars(with = "f64"))]
    pub similarity: Option<f64>,
    /// Root-relative clone instances in this group.
    pub instances: Vec<CloneInstance>,
    /// Group-level refactoring suggestion.
    pub suggestion: RefactoringSuggestion,
    /// Best-effort name for the extracted function. Advisory only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub suggested_name: Option<String>,
}
