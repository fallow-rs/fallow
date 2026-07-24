//! Read-only trace helpers exposed through the engine boundary.

use std::path::Path;

use rustc_hash::FxHashSet;

use crate::duplicates::DuplicationReport;
use crate::module_graph::RetainedModuleGraph;

#[expect(
    unused_imports,
    reason = "engine owns a copied trace implementation whose internal pub uses serve its tests"
)]
#[path = "trace_impl.rs"]
mod trace_impl;

pub type ClassMemberTrace = fallow_types::trace::ClassMemberTrace;
pub type CloneTrace = fallow_types::trace::CloneTrace;
pub type DependencyTrace = fallow_types::trace::DependencyTrace;
pub type ExportReference = fallow_types::trace::ExportReference;
pub type ExportTrace = fallow_types::trace::ExportTrace;
pub type FileTrace = fallow_types::trace::FileTrace;
pub type ImpactClosureGap = fallow_types::trace::ImpactClosureGap;
pub type ImpactClosureTrace = fallow_types::trace::ImpactClosureTrace;
pub type PipelineTimings = fallow_types::trace::PipelineTimings;
pub type ReExportChain = fallow_types::trace::ReExportChain;
pub use trace_impl::SemanticClassMethodResolutionError;
pub type TracedCloneGroup = fallow_types::trace::TracedCloneGroup;
pub type TracedExport = fallow_types::trace::TracedExport;
pub type TracedReExport = fallow_types::trace::TracedReExport;

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
