//! Shared trace output contracts for analysis and integration surfaces.

use std::path::PathBuf;

use serde::Serialize;

use crate::duplicates::{CloneInstance, RefactoringSuggestion};
use crate::serde_path;

/// Result of tracing an export: why it is considered used or unused.
#[derive(Debug, Serialize)]
pub struct ExportTrace {
    /// The file containing the export.
    #[serde(serialize_with = "serde_path::serialize")]
    pub file: PathBuf,
    /// The export name being traced.
    pub export_name: String,
    /// Whether the file is reachable from an entry point.
    pub file_reachable: bool,
    /// Whether the file is an entry point.
    pub is_entry_point: bool,
    /// Whether the export is considered used.
    pub is_used: bool,
    /// Files that reference this export directly.
    pub direct_references: Vec<ExportReference>,
    /// Re-export chains that pass through this export.
    pub re_export_chains: Vec<ReExportChain>,
    /// Human-readable reason summary.
    pub reason: String,
}

/// Result of tracing a class / enum / store MEMBER: the `--trace FILE:NAME`
/// fallback when `NAME` is not a top-level export but a member declared on one
/// (issue #1744). The trace runs on the module graph only, so it reports the
/// OWNING export's reachability and usage (the gating precondition for
/// member-level crediting) plus a pointer to the right `--unused-*-members`
/// command, rather than per-member crediting provenance.
#[derive(Debug, Serialize)]
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
}

/// A direct reference to an export.
#[derive(Debug, Serialize)]
pub struct ExportReference {
    /// File that contains the reference.
    #[serde(serialize_with = "serde_path::serialize")]
    pub from_file: PathBuf,
    /// Reference kind, such as named import, default import, or re-export.
    pub kind: String,
}

/// A re-export chain showing how an export is propagated.
#[derive(Debug, Serialize)]
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
}

/// An export with usage information.
#[derive(Debug, Serialize)]
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
}

/// Pipeline performance timings.
#[derive(Debug, Clone, Serialize)]
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
    /// Number of extracted modules.
    pub module_count: usize,
    /// Number of files loaded from the parse cache.
    pub cache_hits: usize,
    /// Number of files parsed without a cache hit.
    pub cache_misses: usize,
    /// Time spent updating the parse cache.
    pub cache_update_ms: f64,
    /// Time spent categorizing entry points.
    pub entry_points_ms: f64,
    /// Number of entry points considered.
    pub entry_point_count: usize,
    /// Time spent resolving imports.
    pub resolve_imports_ms: f64,
    /// Time spent building the module graph.
    pub build_graph_ms: f64,
    /// Time spent running analysis.
    pub analyze_ms: f64,
    /// Time spent running duplicate-code analysis, when included.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub duplication_ms: Option<f64>,
    /// Total pipeline time.
    pub total_ms: f64,
}

/// Result of computing the impact closure for a single file as the seed.
#[derive(Debug, Serialize)]
pub struct ImpactClosureTrace {
    /// The seed file, root-relative.
    pub seed: String,
    /// Root-relative paths transitively affected by the seed.
    pub affected_not_shown: Vec<String>,
    /// Coordination gaps between the seed and consumers.
    pub coordination_gap: Vec<ImpactClosureGap>,
}

/// One coordination-gap entry in an [`ImpactClosureTrace`].
#[derive(Debug, Serialize)]
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
pub struct TracedCloneGroup {
    /// Stable content fingerprint, usually `dup:<8hex>` and widened on rare
    /// report collisions.
    pub fingerprint: String,
    /// Number of tokens in the duplicated block.
    pub token_count: usize,
    /// Number of lines in the duplicated block.
    pub line_count: usize,
    /// Root-relative clone instances in this group.
    pub instances: Vec<CloneInstance>,
    /// Group-level refactoring suggestion.
    pub suggestion: RefactoringSuggestion,
    /// Best-effort name for the extracted function. Advisory only.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub suggested_name: Option<String>,
}
