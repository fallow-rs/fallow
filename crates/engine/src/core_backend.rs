//! Internal adapter over the current `fallow-core` backend.
//!
//! New engine code should call this module instead of reaching into
//! `fallow-core` directly. The goal is to keep core-backed orchestration
//! contained while the engine-owned contracts continue to stabilize.

use fallow_config::ResolvedConfig;
use fallow_graph::graph::ModuleGraph;
use rustc_hash::FxHashSet;
use std::path::{Path, PathBuf};

use crate::{
    AnalysisResults, DeadCodeAnalysis, DeadCodeAnalysisArtifacts, DeadCodeAnalysisOutput,
    EngineResult, ModuleInfo, discover::AnalysisDiscovery, duplicates::DuplicationReport,
    engine_error, module_graph::RetainedModuleGraph,
};

pub fn prepare_analysis_discovery(config: &ResolvedConfig) -> AnalysisDiscovery {
    AnalysisDiscovery::from_core(fallow_core::prepare_analysis_discovery(config))
}

pub fn analyze_with_usages_from_discovery(
    config: &ResolvedConfig,
    discovery: &AnalysisDiscovery,
) -> EngineResult<DeadCodeAnalysis> {
    fallow_core::analyze_with_usages_from_discovery(config, discovery.as_core())
        .map(|results| DeadCodeAnalysis { results })
        .map_err(engine_error)
}

pub fn analyze_with_usages_and_complexity_from_discovery(
    config: &ResolvedConfig,
    discovery: &AnalysisDiscovery,
) -> EngineResult<DeadCodeAnalysisOutput> {
    fallow_core::analyze_with_usages_and_complexity_from_discovery(config, discovery.as_core())
        .map(|output| DeadCodeAnalysisOutput {
            results: output.results,
            modules: output.modules,
            files: output.files,
        })
        .map_err(engine_error)
}

pub fn analyze_retaining_modules_from_discovery(
    config: &ResolvedConfig,
    discovery: &AnalysisDiscovery,
    need_complexity: bool,
    retain_graph: bool,
) -> EngineResult<DeadCodeAnalysisArtifacts> {
    fallow_core::analyze_retaining_modules_from_discovery(
        config,
        discovery.as_core(),
        need_complexity,
        retain_graph,
    )
    .map(dead_code_artifacts)
    .map_err(engine_error)
}

pub fn analyze_with_parse_result(
    config: &ResolvedConfig,
    modules: &[ModuleInfo],
) -> EngineResult<DeadCodeAnalysisArtifacts> {
    #[expect(
        deprecated,
        reason = "fallow-engine is the typed migration boundary over the internal core backend"
    )]
    fallow_core::analyze_with_parse_result(config, modules)
        .map(dead_code_artifacts)
        .map_err(engine_error)
}

fn dead_code_artifacts(output: fallow_core::AnalysisOutput) -> DeadCodeAnalysisArtifacts {
    DeadCodeAnalysisArtifacts {
        results: output.results,
        timings: output.timings,
        graph: output.graph.map(RetainedModuleGraph::from),
        modules: output.modules,
        files: output.files,
        script_used_packages: output.script_used_packages,
        file_hashes: output.file_hashes,
    }
}

pub fn filter_results_by_changed_files(
    results: &mut AnalysisResults,
    changed_files: &FxHashSet<PathBuf>,
) {
    fallow_core::changed_files::filter_results_by_changed_files(results, changed_files);
}

pub fn trace_export(
    graph: &ModuleGraph,
    root: &Path,
    file_path: &str,
    export_name: &str,
) -> Option<fallow_types::trace::ExportTrace> {
    fallow_core::trace::trace_export(graph, root, file_path, export_name)
}

pub fn trace_file(
    graph: &ModuleGraph,
    root: &Path,
    file_path: &str,
) -> Option<fallow_types::trace::FileTrace> {
    fallow_core::trace::trace_file(graph, root, file_path)
}

pub fn trace_dependency(
    graph: &ModuleGraph,
    root: &Path,
    package_name: &str,
    script_used_packages: &FxHashSet<String>,
) -> fallow_types::trace::DependencyTrace {
    fallow_core::trace::trace_dependency(graph, root, package_name, script_used_packages)
}

pub fn trace_clone(
    report: &DuplicationReport,
    root: &Path,
    file_path: &str,
    line: usize,
) -> fallow_types::trace::CloneTrace {
    fallow_core::trace::trace_clone(report, root, file_path, line)
}

pub fn trace_clone_by_fingerprint(
    report: &DuplicationReport,
    root: &Path,
    fingerprint: &str,
) -> fallow_types::trace::CloneTrace {
    fallow_core::trace::trace_clone_by_fingerprint(report, root, fingerprint)
}

pub fn trace_impact_closure(
    graph: &ModuleGraph,
    root: &Path,
    file_path: &str,
) -> Option<fallow_types::trace::ImpactClosureTrace> {
    fallow_core::trace::trace_impact_closure(graph, root, file_path)
}

pub fn trace_symbol_chain(
    graph: &ModuleGraph,
    modules: &[ModuleInfo],
    root: &Path,
    query: fallow_types::trace_chain::SymbolChainQuery<'_>,
) -> Option<fallow_types::trace_chain::SymbolChainTrace> {
    fallow_core::trace_chain::trace_symbol_chain(graph, modules, root, query)
}
