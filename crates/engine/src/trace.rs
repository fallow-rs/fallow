//! Read-only trace helpers exposed through the engine boundary.

use std::path::Path;

use rustc_hash::FxHashSet;

use crate::duplicates::DuplicationReport;
use crate::module_graph::RetainedModuleGraph;

#[path = "trace_impl.rs"]
pub(crate) mod trace_impl;
#[path = "trace_usage_impl.rs"]
mod trace_usage_impl;

pub use fallow_types::trace_usage::{DependencyUsage, DependencyUsageQuery, SitePageRequest};
pub use trace_usage_impl::UsageError;

pub use fallow_types::trace::{
    ClassMemberTrace, CloneTrace, DependencyTrace, ExportReference, ExportTrace, FileTrace,
    ImpactClosureGap, ImpactClosureTrace, ImportPathHop, ImportPathTrace, PipelineTimings,
    ReExportChain, TraceProvenance, TraceSource, TracedCloneGroup, TracedExport, TracedReExport,
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
///
/// `workspace_roots` are the roots of the workspaces, so the peer-dependency
/// credit runs one closure per workspace, as the unused-dependency check does.
/// `ignore_patterns` are the configured `ignorePatterns`. A workspace
/// `package.json` that they match does not turn on an optional peer, as in the
/// unused-dependency check.
#[must_use]
#[expect(
    clippy::implicit_hasher,
    reason = "fallow standardizes on FxHashSet across the workspace"
)]
pub fn trace_dependency(
    graph: &RetainedModuleGraph,
    root: &Path,
    workspace_roots: &[&Path],
    ignore_patterns: &fallow_config::IgnorePatternSet,
    package_name: &str,
    script_used_packages: &FxHashSet<String>,
) -> DependencyTrace {
    trace_impl::trace_dependency(
        graph.as_graph(),
        root,
        workspace_roots,
        ignore_patterns,
        package_name,
        script_used_packages,
    )
}

/// The analysis facts that a dependency trace reads.
pub struct DependencyTraceInputs<'a> {
    /// The retained module graph.
    pub graph: &'a RetainedModuleGraph,
    /// The project root.
    pub root: &'a Path,
    /// The roots of the workspaces, for the peer-dependency credit.
    pub workspace_roots: &'a [&'a Path],
    /// The configured `ignorePatterns`.
    pub ignore_patterns: &'a fallow_config::IgnorePatternSet,
    /// Packages invoked from package.json scripts, CI configs and git hooks.
    pub script_used_packages: &'a FxHashSet<String>,
    /// Which configs name which files and dependency names.
    pub provenance: &'a TraceProvenance,
    /// The findings of the analysis, which name the manifests that the
    /// unused-dependency check flags.
    pub results: &'a fallow_types::results::AnalysisResults,
}

/// Build the complete dependency trace: the importers, the config sources,
/// the tooling credit and the flagged manifests.
///
/// `fallow dead-code --trace-dependency`, `fallow trace --dependency` and the
/// programmatic API share this builder, so the base fields agree on every
/// surface.
#[must_use]
pub fn build_dependency_trace(
    inputs: &DependencyTraceInputs<'_>,
    package_name: &str,
) -> DependencyTrace {
    let mut trace = trace_dependency(
        inputs.graph,
        inputs.root,
        inputs.workspace_roots,
        inputs.ignore_patterns,
        package_name,
        inputs.script_used_packages,
    );
    trace.sources = inputs.provenance.dependency_sources(package_name);
    trace.apply_tooling_credit(inputs.provenance.tooling_credit(package_name));
    trace.apply_unused_declarations(inputs.results, inputs.root);
    trace
}

/// Trace a dependency with its per-specifier usage through an existing
/// analysis session.
///
/// The inner `Result` carries a query error, such as a cursor that belongs
/// to another query.
///
/// # Errors
///
/// Returns an error if parsing, graph construction, or analysis fails.
pub fn trace_dependency_with_session(
    session: &crate::session::AnalysisSession,
    package_name: &str,
    query: &DependencyUsageQuery,
) -> crate::EngineResult<Result<DependencyTrace, UsageError>> {
    // Keep the parsed modules: the usage walk reads their import facts.
    let output = session.analyze_dead_code_with_shared_artifacts(true, true)?;
    let graph = output.graph.as_ref().ok_or_else(|| {
        crate::EngineError::new("trace --dependency requires a retained module graph")
    })?;
    let modules = output.modules.as_deref().unwrap_or(&[]);
    let workspace_roots: Vec<&Path> = session
        .workspaces()
        .iter()
        .map(|workspace| workspace.root.as_path())
        .collect();
    let mut trace = build_dependency_trace(
        &DependencyTraceInputs {
            graph,
            root: session.root(),
            workspace_roots: &workspace_roots,
            ignore_patterns: &session.config().ignore_patterns,
            script_used_packages: &output.script_used_packages,
            provenance: &output.trace_provenance,
            results: &output.results,
        },
        package_name,
    );
    match trace_usage_impl::dependency_usage(
        graph.as_graph(),
        modules,
        session.root(),
        package_name,
        &trace.imported_by,
        query,
    ) {
        Ok(usage) => {
            trace.usage = Some(usage);
            Ok(Ok(trace))
        }
        Err(err) => Ok(Err(err)),
    }
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
    trace_impl::trace_import_path(graph.as_graph(), root, from_path, to_path, false)
}

/// Trace the shortest import path over eager edges only: static imports that
/// carry a runtime value. The route explains why `to_path` loads before
/// `from_path` runs.
///
/// # Errors
///
/// Returns the endpoint that did not resolve to exactly one module in the graph.
pub fn trace_eager_import_path(
    graph: &RetainedModuleGraph,
    root: &Path,
    from_path: &str,
    to_path: &str,
) -> Result<ImportPathTrace, ImportPathEndpoint> {
    trace_impl::trace_import_path(graph.as_graph(), root, from_path, to_path, true)
}

/// Trace the shortest import path through an existing analysis session.
/// With `eager_only`, the walk follows static value imports only.
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
    eager_only: bool,
) -> crate::EngineResult<Result<ImportPathTrace, ImportPathEndpoint>> {
    let output = session.analyze_dead_code_with_shared_artifacts(false, true)?;
    let graph = output
        .graph
        .as_ref()
        .ok_or_else(|| crate::EngineError::new("trace --path requires a retained module graph"))?;
    Ok(trace_impl::trace_import_path(
        graph.as_graph(),
        session.root(),
        from_path,
        to_path,
        eager_only,
    ))
}
