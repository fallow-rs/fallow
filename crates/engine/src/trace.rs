//! Read-only trace helpers exposed through the engine boundary.

use std::path::Path;

use rustc_hash::FxHashSet;

use crate::duplicates::DuplicationReport;
use crate::module_graph::RetainedModuleGraph;

#[path = "trace_impl.rs"]
pub(crate) mod trace_impl;

pub use fallow_types::trace::{
    ClassMemberTrace, CloneTrace, DependencyTrace, ExportReference, ExportTrace, FileTrace,
    ImpactClosureGap, ImpactClosureTrace, ImportPathHop, ImportPathTrace, PipelineTimings,
    ReExportChain, TracedCloneGroup, TracedExport, TracedReExport,
};
pub use trace_impl::{ImportPathEndpoint, SemanticClassMethodResolutionError};

/// Trace why an export is considered used or unused.
#[must_use]
pub fn trace_export(
    graph: &RetainedModuleGraph,
    root: &Path,
    file_path: &str,
    export_name: &str,
) -> Option<ExportTrace> {
    trace_impl::trace_export(graph.as_graph(), root, file_path, export_name)
}

/// Reconcile checker-backed trace evidence with graph reachability while
/// retaining non-crediting evidence for inspection.
pub fn reconcile_semantic_trace_reachability(
    graph: &RetainedModuleGraph,
    root: &Path,
    target_reachable: bool,
    trace: &mut fallow_types::semantic::SemanticSymbolTrace,
) {
    trace_impl::reconcile_semantic_trace_reachability(
        graph.as_graph(),
        root,
        target_reachable,
        trace,
    );
}

/// Resolve the source identity for an exact semantic export query.
#[must_use]
pub fn semantic_symbol_for_export(
    graph: &RetainedModuleGraph,
    root: &Path,
    file_path: &str,
    export_name: &str,
) -> Option<fallow_types::semantic::SemanticSymbol> {
    trace_impl::semantic_symbol_for_export(graph.as_graph(), root, file_path, export_name)
}

/// Resolve the source identity for an exact semantic class-member query.
#[must_use]
pub fn semantic_symbol_for_class_member(
    graph: &RetainedModuleGraph,
    root: &Path,
    file_path: &str,
    member_name: &str,
) -> Option<fallow_types::semantic::SemanticSymbol> {
    trace_impl::semantic_symbol_for_class_member(graph.as_graph(), root, file_path, member_name)
}

/// Resolve one exact exported class method for semantic impact analysis.
pub fn semantic_symbol_for_exact_class_method(
    graph: &RetainedModuleGraph,
    root: &Path,
    file_path: &str,
    owner_name: &str,
    member_name: &str,
) -> Result<fallow_types::semantic::SemanticSymbol, SemanticClassMethodResolutionError> {
    trace_impl::semantic_symbol_for_exact_class_method(
        graph.as_graph(),
        root,
        file_path,
        owner_name,
        member_name,
    )
}

/// Trace a class / enum / store member (the `--trace FILE:MEMBER` fallback when
/// `MEMBER` is not a top-level export). See issue #1744.
#[must_use]
pub fn trace_class_member(
    graph: &RetainedModuleGraph,
    root: &Path,
    file_path: &str,
    member_name: &str,
) -> Option<ClassMemberTrace> {
    trace_impl::trace_class_member(graph.as_graph(), root, file_path, member_name)
}

/// Trace all graph edges for a file.
#[must_use]
pub fn trace_file(graph: &RetainedModuleGraph, root: &Path, file_path: &str) -> Option<FileTrace> {
    trace_impl::trace_file(graph.as_graph(), root, file_path)
}

/// Trace where a dependency is used.
#[must_use]
#[expect(
    clippy::implicit_hasher,
    reason = "fallow standardizes on FxHashSet across the workspace"
)]
pub fn trace_dependency(
    graph: &RetainedModuleGraph,
    root: &Path,
    package_name: &str,
    script_used_packages: &FxHashSet<String>,
) -> DependencyTrace {
    trace_impl::trace_dependency(graph.as_graph(), root, package_name, script_used_packages)
}

/// Trace duplicate-code groups that contain a source location.
#[must_use]
pub fn trace_clone(
    report: &DuplicationReport,
    root: &Path,
    file_path: &str,
    line: usize,
) -> CloneTrace {
    trace_impl::trace_clone(report, root, file_path, line)
}

/// Trace a duplicate-code group by its stable content fingerprint.
#[must_use]
pub fn trace_clone_by_fingerprint(
    report: &DuplicationReport,
    root: &Path,
    fingerprint: &str,
) -> CloneTrace {
    trace_impl::trace_clone_by_fingerprint(report, root, fingerprint)
}

/// Trace the impact closure for a file.
#[must_use]
pub fn trace_impact_closure(
    graph: &RetainedModuleGraph,
    root: &Path,
    file_path: &str,
) -> Option<ImpactClosureTrace> {
    trace_impl::trace_impact_closure(graph.as_graph(), root, file_path)
}

/// Trace the shortest import path between two modules.
///
/// # Errors
///
/// Returns the endpoint that did not resolve to exactly one module in the graph.
pub fn trace_import_path(
    graph: &RetainedModuleGraph,
    root: &Path,
    from_path: &str,
    to_path: &str,
) -> Result<ImportPathTrace, ImportPathEndpoint> {
    trace_impl::trace_import_path(graph.as_graph(), root, from_path, to_path)
}

/// Trace the shortest import path through an existing analysis session.
///
/// The inner `Result` carries the endpoint that did not resolve to a module.
///
/// # Errors
///
/// Returns an error if parsing or graph construction fails.
pub fn trace_import_path_with_session(
    session: &crate::session::AnalysisSession,
    from_path: &str,
    to_path: &str,
) -> crate::EngineResult<Result<ImportPathTrace, ImportPathEndpoint>> {
    let output = session.analyze_dead_code_with_shared_artifacts(false, true)?;
    let graph = output
        .graph
        .as_ref()
        .ok_or_else(|| crate::EngineError::new("trace --path requires a retained module graph"))?;
    Ok(trace_import_path(graph, session.root(), from_path, to_path))
}
