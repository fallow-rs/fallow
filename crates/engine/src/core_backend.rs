//! Internal adapter over the current `fallow-core` backend.
//!
//! New engine code should call this module instead of reaching into
//! `fallow-core` directly. The goal is to keep core-backed orchestration
//! contained while the engine-owned contracts continue to stabilize.

use fallow_config::{ExternalPluginDef, PackageJson, ResolvedConfig};
use fallow_types::trace::PipelineTimings;
use rustc_hash::FxHashSet;
use std::path::{Path, PathBuf};

use crate::{
    EngineResult,
    discover::{AnalysisDiscovery, DiscoveredFile, EntryPoint, HiddenDirScope},
    engine_error,
    module_graph::RetainedModuleGraph,
    results::AnalysisResults,
    source::ModuleInfo,
};

// External-plugin dry-run primitives surfaced through the engine boundary for
// the CLI's `plugin-check` command (read-only; no analysis pipeline).
pub use fallow_core::plugins::manifest_entries::{
    CheckWarning, ManifestResult, RuleReport, WarningKind, check_manifest_entries,
};
pub use fallow_core::plugins::registry::is_external_plugin_active;

// Discovery vocabulary owned by the surviving core walk, re-exported so the
// engine boundary keeps one definition per constant.
pub use fallow_core::discover::ALLOWED_HIDDEN_DIRS;
pub use fallow_core::discover::PRODUCTION_EXCLUDE_PATTERNS;
pub use fallow_core::discover::SOURCE_EXTENSIONS;

#[derive(Debug, Clone, Copy)]
pub struct ParseMetrics {
    pub parse_ms: f64,
    pub cache_ms: f64,
    pub cache_hits: usize,
    pub cache_misses: usize,
    pub parse_cpu_ms: f64,
}

pub struct DeadCodeBackendPrelude<'a> {
    inner: fallow_core::DeadCodeBackendPrelude<'a>,
}

#[derive(Debug, Clone, Copy)]
#[expect(
    clippy::struct_field_names,
    reason = "timings are all milliseconds; the _ms suffix is the unit"
)]
pub struct DeadCodePreludeTimings {
    pub discover_ms: f64,
    pub workspaces_ms: f64,
    pub plugins_ms: f64,
    pub scripts_ms: f64,
}

pub struct DeadCodeEntryPoints {
    inner: fallow_core::DeadCodeEntryPoints,
}

impl DeadCodeEntryPoints {
    pub fn count(&self) -> usize {
        self.inner.count()
    }

    pub fn elapsed_ms(&self) -> f64 {
        self.inner.elapsed_ms()
    }
}

pub struct DeadCodeResolvedModules {
    pub project: fallow_graph::resolve::ResolvedProject,
    pub elapsed_ms: f64,
}

pub struct DeadCodeGraphRun {
    pub graph: RetainedModuleGraph,
    pub elapsed_ms: f64,
}

pub struct DeadCodeDetectorRun {
    pub results: AnalysisResults,
    pub elapsed_ms: f64,
}

impl DeadCodeBackendPrelude<'_> {
    pub fn timings(&self) -> DeadCodePreludeTimings {
        let timings = self.inner.timings();
        DeadCodePreludeTimings {
            discover_ms: timings.discover_ms,
            workspaces_ms: timings.workspaces_ms,
            plugins_ms: timings.plugins_ms,
            scripts_ms: timings.scripts_ms,
        }
    }

    pub fn elapsed_ms(&self) -> f64 {
        self.inner.elapsed_ms()
    }

    pub fn script_used_packages(&self) -> FxHashSet<String> {
        self.inner.script_used_packages()
    }

    pub fn finish(&self) {
        self.inner.finish();
    }
}

/// Run the shared core discovery walk with engine-owned hidden-dir scopes.
///
/// The core walk is the single implementation: it derives the config-candidate
/// basename set live from the plugin registry, so discovery can never drift
/// behind a new plugin's config patterns.
pub fn discover_files_and_config_candidates(
    config: &ResolvedConfig,
    additional_hidden_dir_scopes: &[HiddenDirScope],
) -> (Vec<DiscoveredFile>, Vec<PathBuf>) {
    let discovered =
        discover_files_config_candidates_and_diagnostics(config, additional_hidden_dir_scopes);
    (discovered.files, discovered.config_candidates)
}

/// Source files, config candidates, and the source-discovery diagnostics of
/// one walk, re-exported so engine modules never name the core type directly.
pub use fallow_core::discover::DiscoveredSources;

/// The same walk, keeping the source-discovery diagnostics it produced so a
/// session can carry its own snapshot instead of reading the process registry
/// a concurrent walk may already have replaced (issue #2366).
pub fn discover_files_config_candidates_and_diagnostics(
    config: &ResolvedConfig,
    additional_hidden_dir_scopes: &[HiddenDirScope],
) -> DiscoveredSources {
    let scopes = additional_hidden_dir_scopes
        .iter()
        .map(|scope| {
            fallow_core::discover::HiddenDirScope::with_match_mode(
                scope.root().to_path_buf(),
                scope.dirs().to_vec(),
                match scope.match_mode() {
                    crate::discover::HiddenDirMatch::AnyDepth => {
                        fallow_core::discover::HiddenDirMatch::AnyDepth
                    }
                    crate::discover::HiddenDirMatch::ExactPath => {
                        fallow_core::discover::HiddenDirMatch::ExactPath
                    }
                },
            )
        })
        .collect::<Vec<_>>();
    fallow_core::discover::discover_files_config_candidates_and_diagnostics(config, &scopes)
}

/// Discover configured and inferred entry points via the shared core implementation.
///
/// Entry-point discovery has one implementation, in fallow-core, so the
/// analysis pipeline and list inventory can never drift apart.
pub fn discover_entry_points(config: &ResolvedConfig, files: &[DiscoveredFile]) -> Vec<EntryPoint> {
    fallow_core::discover::discover_entry_points(config, files)
}

/// Discover workspace entry points via the shared core implementation.
pub fn discover_workspace_entry_points(
    ws_root: &Path,
    config: &ResolvedConfig,
    all_files: &[DiscoveredFile],
) -> Vec<EntryPoint> {
    fallow_core::discover::discover_workspace_entry_points(ws_root, config, all_files)
}

/// Discover plugin-derived entry points via the shared core implementation.
pub fn discover_plugin_entry_points(
    plugin_result: &BackendAggregatedPluginResult,
    config: &ResolvedConfig,
    files: &[DiscoveredFile],
) -> Vec<EntryPoint> {
    fallow_core::discover::discover_plugin_entry_points(&plugin_result.inner, config, files)
}

pub fn prepare_dead_code_backend_prelude<'a>(
    config: &'a ResolvedConfig,
    discovery: &'a AnalysisDiscovery,
) -> EngineResult<DeadCodeBackendPrelude<'a>> {
    let core_discovery = fallow_core::AnalysisDiscovery::from_parts(
        discovery.files().to_vec(),
        discovery.workspaces().to_vec(),
        discovery.root_pkg().cloned(),
        discovery.config_candidates().to_vec(),
        discovery.discover_ms(),
        discovery.workspaces_ms(),
    );
    fallow_core::prepare_dead_code_backend_prelude(config, core_discovery)
        .map(|inner| DeadCodeBackendPrelude { inner })
        .map_err(engine_error)
}

pub fn discover_dead_code_entry_points(
    prelude: &DeadCodeBackendPrelude<'_>,
) -> DeadCodeEntryPoints {
    DeadCodeEntryPoints {
        inner: fallow_core::discover_dead_code_entry_points(&prelude.inner),
    }
}

pub fn try_load_dead_code_graph_cache(
    prelude: &DeadCodeBackendPrelude<'_>,
    entry_points: &DeadCodeEntryPoints,
    modules: &[ModuleInfo],
) -> Option<(DeadCodeResolvedModules, DeadCodeGraphRun)> {
    fallow_core::try_load_dead_code_graph_cache(&prelude.inner, &entry_points.inner, modules).map(
        |(resolved, graph)| {
            (
                DeadCodeResolvedModules {
                    project: resolved.project,
                    elapsed_ms: resolved.elapsed_ms,
                },
                DeadCodeGraphRun {
                    graph: RetainedModuleGraph::from(graph.graph),
                    elapsed_ms: graph.elapsed_ms,
                },
            )
        },
    )
}

pub fn resolve_dead_code_imports(
    prelude: &DeadCodeBackendPrelude<'_>,
    modules: &[ModuleInfo],
) -> DeadCodeResolvedModules {
    let resolved = fallow_core::resolve_dead_code_imports(&prelude.inner, modules);
    DeadCodeResolvedModules {
        project: resolved.project,
        elapsed_ms: resolved.elapsed_ms,
    }
}

pub fn build_dead_code_graph(
    prelude: &DeadCodeBackendPrelude<'_>,
    project: &fallow_graph::resolve::ResolvedProject,
    entry_points: &DeadCodeEntryPoints,
    modules: &[ModuleInfo],
) -> DeadCodeGraphRun {
    let graph =
        fallow_core::build_dead_code_graph(&prelude.inner, project, &entry_points.inner, modules);
    DeadCodeGraphRun {
        graph: RetainedModuleGraph::from(graph.graph),
        elapsed_ms: graph.elapsed_ms,
    }
}

pub fn run_dead_code_detectors(
    prelude: &DeadCodeBackendPrelude<'_>,
    graph: &RetainedModuleGraph,
    resolved: &[fallow_graph::resolve::ResolvedModule],
    modules: &[ModuleInfo],
    collect_usages: bool,
    entry_points: &DeadCodeEntryPoints,
) -> DeadCodeDetectorRun {
    let detector = fallow_core::run_dead_code_detectors(
        &prelude.inner,
        graph.as_graph(),
        resolved,
        modules,
        collect_usages,
        &entry_points.inner,
    );
    DeadCodeDetectorRun {
        results: detector.results,
        elapsed_ms: detector.elapsed_ms,
    }
}

pub struct EngineDeadCodePipelineProfile {
    pub timings: Option<PipelineTimings>,
}

#[derive(Clone, Copy)]
pub struct DeadCodePipelineProfileInput<'a> {
    pub retain_timings: bool,
    pub prelude: &'a DeadCodeBackendPrelude<'a>,
    pub prelude_timings: DeadCodePreludeTimings,
    pub parse_metrics: ParseMetrics,
    pub module_count: usize,
    pub entry_points: &'a DeadCodeEntryPoints,
    pub resolved: &'a DeadCodeResolvedModules,
    pub graph: &'a DeadCodeGraphRun,
    pub detector: &'a DeadCodeDetectorRun,
    pub file_count: usize,
    pub workspace_count: usize,
}

pub fn dead_code_pipeline_profile(
    input: DeadCodePipelineProfileInput<'_>,
) -> EngineDeadCodePipelineProfile {
    let DeadCodePipelineProfileInput {
        retain_timings,
        prelude,
        prelude_timings,
        parse_metrics,
        module_count,
        entry_points,
        resolved,
        graph,
        detector,
        file_count,
        workspace_count,
    } = input;
    EngineDeadCodePipelineProfile {
        timings: retain_timings.then_some(PipelineTimings {
            discover_files_ms: prelude_timings.discover_ms,
            file_count,
            workspaces_ms: prelude_timings.workspaces_ms,
            workspace_count,
            plugins_ms: prelude_timings.plugins_ms,
            script_analysis_ms: prelude_timings.scripts_ms,
            parse_extract_ms: parse_metrics.parse_ms,
            parse_cpu_ms: parse_metrics.parse_cpu_ms,
            module_count,
            cache_hits: parse_metrics.cache_hits,
            cache_misses: parse_metrics.cache_misses,
            cache_update_ms: parse_metrics.cache_ms,
            entry_points_ms: entry_points.elapsed_ms(),
            entry_point_count: entry_points.count(),
            resolve_imports_ms: resolved.elapsed_ms,
            build_graph_ms: graph.elapsed_ms,
            analyze_ms: detector.elapsed_ms,
            duplication_ms: None,
            total_ms: prelude.elapsed_ms(),
        }),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackendPluginRegexValidationError {
    message: String,
}

impl BackendPluginRegexValidationError {
    pub fn message(&self) -> String {
        self.message.clone()
    }
}

#[derive(Debug, Clone, Default)]
pub struct BackendAggregatedPluginResult {
    inner: fallow_core::plugins::AggregatedPluginResult,
}

impl BackendAggregatedPluginResult {
    fn from_core(inner: fallow_core::plugins::AggregatedPluginResult) -> Self {
        Self { inner }
    }

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
    pub(crate) fn push_active_plugin_for_test(&mut self, plugin_name: impl Into<String>) {
        self.inner.active_plugins.push(plugin_name.into());
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
            .map(BackendAggregatedPluginResult::from_core)
            .map_err(|errors| {
                errors
                    .into_iter()
                    .map(|error| BackendPluginRegexValidationError {
                        message: error.to_string(),
                    })
                    .collect()
            })
    }
}
