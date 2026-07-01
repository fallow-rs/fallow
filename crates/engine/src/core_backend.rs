//! Internal adapter over the current `fallow-core` backend.
//!
//! New engine code should call this module instead of reaching into
//! `fallow-core` directly. The goal is to keep core-backed orchestration
//! contained while the engine-owned contracts continue to stabilize.

use fallow_config::ResolvedConfig;
use fallow_config::{ExternalPluginDef, PackageJson};
use fallow_graph::graph::ModuleGraph;
use fallow_types::results::{SecurityFinding, SecuritySeverity};
use rustc_hash::FxHashSet;
use std::path::{Path, PathBuf};

use crate::{
    AnalysisResults, DeadCodeAnalysis, DeadCodeAnalysisArtifacts, DeadCodeAnalysisOutput,
    EngineResult, ModuleInfo,
    changed_files::{ChangedFilesError, ChangedFilesSpawnHook},
    cross_reference::{
        CombinedFinding as EngineCombinedFinding,
        CrossReferenceResult as EngineCrossReferenceResult, DeadCodeKind as EngineDeadCodeKind,
    },
    discover::AnalysisDiscovery,
    duplicates::DuplicationReport,
    engine_error,
    module_graph::RetainedModuleGraph,
};

pub fn prepare_analysis_discovery(config: &ResolvedConfig) -> AnalysisDiscovery {
    AnalysisDiscovery::from_core(fallow_core::prepare_analysis_discovery(config))
}

pub fn config_for_project(
    root: &Path,
    config_path: Option<&Path>,
) -> EngineResult<(ResolvedConfig, Option<PathBuf>)> {
    fallow_core::config_for_project(root, config_path).map_err(engine_error)
}

pub fn resolve_cache_max_size_bytes(config: &ResolvedConfig) -> usize {
    fallow_core::resolve_cache_max_size_bytes(config)
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

fn dead_code_kind(kind: fallow_core::cross_reference::DeadCodeKind) -> EngineDeadCodeKind {
    match kind {
        fallow_core::cross_reference::DeadCodeKind::UnusedFile => EngineDeadCodeKind::UnusedFile,
        fallow_core::cross_reference::DeadCodeKind::UnusedExport { export_name } => {
            EngineDeadCodeKind::UnusedExport { export_name }
        }
        fallow_core::cross_reference::DeadCodeKind::UnusedType { type_name } => {
            EngineDeadCodeKind::UnusedType { type_name }
        }
    }
}

fn combined_finding(
    finding: fallow_core::cross_reference::CombinedFinding,
) -> EngineCombinedFinding {
    EngineCombinedFinding {
        clone_instance: finding.clone_instance,
        dead_code_kind: dead_code_kind(finding.dead_code_kind),
        group_index: finding.group_index,
    }
}

fn cross_reference_result(
    result: fallow_core::cross_reference::CrossReferenceResult,
) -> EngineCrossReferenceResult {
    EngineCrossReferenceResult {
        combined_findings: result
            .combined_findings
            .into_iter()
            .map(combined_finding)
            .collect(),
        clones_in_unused_files: result.clones_in_unused_files,
        clones_with_unused_exports: result.clones_with_unused_exports,
    }
}

pub fn cross_reference(
    duplication: &DuplicationReport,
    dead_code: &AnalysisResults,
) -> EngineCrossReferenceResult {
    cross_reference_result(fallow_core::cross_reference::cross_reference(
        duplication,
        dead_code,
    ))
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

fn changed_files_error(error: fallow_core::changed_files::ChangedFilesError) -> ChangedFilesError {
    match error {
        fallow_core::changed_files::ChangedFilesError::InvalidRef(err) => {
            ChangedFilesError::InvalidRef(err)
        }
        fallow_core::changed_files::ChangedFilesError::GitMissing(err) => {
            ChangedFilesError::GitMissing(err)
        }
        fallow_core::changed_files::ChangedFilesError::NotARepository => {
            ChangedFilesError::NotARepository
        }
        fallow_core::changed_files::ChangedFilesError::GitFailed(stderr) => {
            ChangedFilesError::GitFailed(stderr)
        }
    }
}

pub fn set_changed_files_spawn_hook(hook: ChangedFilesSpawnHook) {
    fallow_core::changed_files::set_spawn_hook(hook);
}

pub fn validate_git_ref(s: &str) -> Result<&str, String> {
    fallow_core::changed_files::validate_git_ref(s)
}

pub fn resolve_git_toplevel(cwd: &Path) -> Result<PathBuf, ChangedFilesError> {
    fallow_core::changed_files::resolve_git_toplevel(cwd).map_err(changed_files_error)
}

pub fn resolve_git_common_dir(cwd: &Path) -> Result<PathBuf, ChangedFilesError> {
    fallow_core::changed_files::resolve_git_common_dir(cwd).map_err(changed_files_error)
}

pub fn try_get_changed_files(
    root: &Path,
    git_ref: &str,
) -> Result<FxHashSet<PathBuf>, ChangedFilesError> {
    fallow_core::changed_files::try_get_changed_files(root, git_ref).map_err(changed_files_error)
}

pub fn try_get_changed_files_with_toplevel(
    cwd: &Path,
    toplevel: &Path,
    git_ref: &str,
) -> Result<FxHashSet<PathBuf>, ChangedFilesError> {
    fallow_core::changed_files::try_get_changed_files_with_toplevel(cwd, toplevel, git_ref)
        .map_err(changed_files_error)
}

pub fn try_get_changed_diff(root: &Path, git_ref: &str) -> Result<String, ChangedFilesError> {
    fallow_core::changed_files::try_get_changed_diff(root, git_ref).map_err(changed_files_error)
}

pub fn get_changed_files(root: &Path, git_ref: &str) -> Option<FxHashSet<PathBuf>> {
    fallow_core::changed_files::get_changed_files(root, git_ref)
}

pub fn filter_duplication_by_changed_files(
    report: &mut DuplicationReport,
    changed_files: &FxHashSet<PathBuf>,
    root: &Path,
) {
    fallow_core::changed_files::filter_duplication_by_changed_files(report, changed_files, root);
}

pub fn derive_security_severity(finding: &SecurityFinding) -> SecuritySeverity {
    fallow_core::analyze::derive_security_severity(finding)
}

pub fn security_catalogue_title(kind: &str) -> Option<&'static str> {
    fallow_core::analyze::security_catalogue_title(kind)
}

pub fn public_api_package_entry_points(
    graph: &ModuleGraph,
    config: &ResolvedConfig,
    root_pkg: Option<&fallow_config::PackageJson>,
    workspaces: &[fallow_config::WorkspaceInfo],
) -> FxHashSet<fallow_types::discover::FileId> {
    fallow_core::analyze::public_api_package_entry_points(graph, config, root_pkg, workspaces)
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackendPluginRegexValidationError {
    inner: fallow_core::plugins::registry::PluginRegexValidationError,
}

impl From<fallow_core::plugins::registry::PluginRegexValidationError>
    for BackendPluginRegexValidationError
{
    fn from(inner: fallow_core::plugins::registry::PluginRegexValidationError) -> Self {
        Self { inner }
    }
}

pub fn builtin_plugin_names() -> Vec<&'static str> {
    fallow_core::plugins::registry::builtin_plugin_names()
}

pub fn format_plugin_regex_errors(errors: &[BackendPluginRegexValidationError]) -> String {
    let core_errors = errors
        .iter()
        .map(|error| error.inner.clone())
        .collect::<Vec<_>>();
    fallow_core::plugins::registry::format_plugin_regex_errors(&core_errors)
}

#[derive(Debug, Clone, Default)]
pub struct BackendAggregatedPluginResult {
    inner: fallow_core::plugins::AggregatedPluginResult,
}

impl BackendAggregatedPluginResult {
    pub fn active_plugins(&self) -> &[String] {
        &self.inner.active_plugins
    }

    pub fn merge_active_plugins_from(&mut self, other: &Self) {
        for plugin_name in &other.inner.active_plugins {
            if !self.inner.active_plugins.contains(plugin_name) {
                self.inner.active_plugins.push(plugin_name.clone());
            }
        }
    }

    #[cfg(test)]
    pub fn push_active_plugin_for_test(&mut self, plugin_name: impl Into<String>) {
        self.inner.active_plugins.push(plugin_name.into());
    }
}

impl From<fallow_core::plugins::AggregatedPluginResult> for BackendAggregatedPluginResult {
    fn from(inner: fallow_core::plugins::AggregatedPluginResult) -> Self {
        Self { inner }
    }
}

pub struct BackendPluginRegistry {
    inner: fallow_core::plugins::PluginRegistry,
}

impl BackendPluginRegistry {
    pub fn new(external: Vec<ExternalPluginDef>) -> Self {
        Self {
            inner: fallow_core::plugins::PluginRegistry::new(external),
        }
    }

    pub fn discovery_hidden_dirs(&self, pkg: &PackageJson, root: &Path) -> Vec<String> {
        self.inner.discovery_hidden_dirs(pkg, root)
    }

    pub fn try_run(
        &self,
        pkg: &PackageJson,
        root: &Path,
        discovered_files: &[PathBuf],
    ) -> Result<BackendAggregatedPluginResult, Vec<BackendPluginRegexValidationError>> {
        self.inner
            .try_run(pkg, root, discovered_files)
            .map(Into::into)
            .map_err(|errors| errors.into_iter().map(Into::into).collect())
    }
}

pub fn discover_plugin_entry_points(
    plugin_result: &BackendAggregatedPluginResult,
    config: &ResolvedConfig,
    files: &[fallow_types::discover::DiscoveredFile],
) -> Vec<fallow_types::discover::EntryPoint> {
    fallow_core::discover::discover_plugin_entry_points(&plugin_result.inner, config, files)
}
