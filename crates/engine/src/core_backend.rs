//! Internal adapter over the current `fallow-core` backend.
//!
//! New engine code should call this module instead of reaching into
//! `fallow-core` directly. The goal is to keep core-backed orchestration
//! contained while the engine-owned contracts continue to stabilize.

use fallow_config::{
    DuplicatesConfig, ExternalPluginDef, PackageJson, ResolvedConfig, WorkspaceInfo,
};
use fallow_graph::graph::ModuleGraph;
use fallow_types::discover::{DiscoveredFile, EntryPoint};
use fallow_types::duplicates::{CloneGroup, CloneInstance, DuplicationReport};
use fallow_types::trace::PipelineTimings;
use rustc_hash::FxHashSet;
use std::path::{Path, PathBuf};

use crate::{
    EngineResult,
    churn::{AuthorContribution, ChurnResult, ChurnSpawnHook, FileChurn, SinceDuration},
    cross_reference::{
        CombinedFinding as EngineCombinedFinding,
        CrossReferenceResult as EngineCrossReferenceResult, DeadCodeKind as EngineDeadCodeKind,
    },
    discover::{AnalysisDiscovery, HiddenDirScope},
    engine_error,
    module_graph::RetainedModuleGraph,
    results::{AnalysisResults, DuplicationAnalysis},
    source::ModuleInfo,
};

#[derive(Debug, Clone)]
pub struct BackendAnalysisDiscovery {
    inner: fallow_core::AnalysisDiscovery,
}

impl BackendAnalysisDiscovery {
    pub fn from_parts(
        files: Vec<DiscoveredFile>,
        workspaces: Vec<WorkspaceInfo>,
        root_pkg: Option<PackageJson>,
        config_candidates: Vec<PathBuf>,
        discover_ms: f64,
        workspaces_ms: f64,
    ) -> Self {
        Self {
            inner: fallow_core::AnalysisDiscovery::from_parts(
                files,
                workspaces,
                root_pkg,
                config_candidates,
                discover_ms,
                workspaces_ms,
            ),
        }
    }

    fn as_core(&self) -> &fallow_core::AnalysisDiscovery {
        &self.inner
    }

    pub fn files(&self) -> &[DiscoveredFile] {
        self.inner.files()
    }

    pub fn workspaces(&self) -> &[WorkspaceInfo] {
        self.inner.workspaces()
    }

    pub fn into_files(self) -> Vec<DiscoveredFile> {
        self.inner.into_files()
    }
}

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
    pub resolved: Vec<fallow_graph::resolve::ResolvedModule>,
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

pub fn prepare_dead_code_backend_prelude<'a>(
    config: &'a ResolvedConfig,
    discovery: &'a AnalysisDiscovery,
) -> EngineResult<DeadCodeBackendPrelude<'a>> {
    fallow_core::prepare_dead_code_backend_prelude(config, discovery.as_backend().as_core())
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
                    resolved: resolved.resolved,
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
        resolved: resolved.resolved,
        elapsed_ms: resolved.elapsed_ms,
    }
}

pub fn build_dead_code_graph(
    prelude: &DeadCodeBackendPrelude<'_>,
    resolved: &[fallow_graph::resolve::ResolvedModule],
    entry_points: &DeadCodeEntryPoints,
    modules: &[ModuleInfo],
) -> DeadCodeGraphRun {
    let graph =
        fallow_core::build_dead_code_graph(&prelude.inner, resolved, &entry_points.inner, modules);
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

#[expect(
    clippy::too_many_arguments,
    reason = "pipeline timing assembly mirrors the explicit backend phases"
)]
pub fn dead_code_pipeline_profile(
    retain_timings: bool,
    prelude: &DeadCodeBackendPrelude<'_>,
    prelude_timings: DeadCodePreludeTimings,
    parse_metrics: ParseMetrics,
    module_count: usize,
    entry_points: &DeadCodeEntryPoints,
    resolved: &DeadCodeResolvedModules,
    graph: &DeadCodeGraphRun,
    detector: &DeadCodeDetectorRun,
    file_count: usize,
    workspace_count: usize,
) -> EngineDeadCodePipelineProfile {
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

impl From<ParseMetrics> for fallow_core::AnalysisParseMetrics {
    fn from(metrics: ParseMetrics) -> Self {
        Self {
            parse_ms: metrics.parse_ms,
            cache_ms: metrics.cache_ms,
            cache_hits: metrics.cache_hits,
            cache_misses: metrics.cache_misses,
            parse_cpu_ms: metrics.parse_cpu_ms,
        }
    }
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

#[derive(Debug, Clone)]
pub struct BackendCloneFingerprintSet {
    inner: fallow_core::duplicates::CloneFingerprintSet,
}

impl BackendCloneFingerprintSet {
    pub fn from_groups(groups: &[CloneGroup]) -> Self {
        Self {
            inner: fallow_core::duplicates::CloneFingerprintSet::from_groups(groups),
        }
    }

    pub fn fingerprint_for_group(&self, group: &CloneGroup) -> String {
        self.inner.fingerprint_for_group(group)
    }

    pub fn fingerprint_for_parts(
        &self,
        instances: &[CloneInstance],
        token_count: usize,
        line_count: usize,
    ) -> String {
        self.inner
            .fingerprint_for_parts(instances, token_count, line_count)
    }

    pub fn find_group<'a>(
        &self,
        groups: &'a [CloneGroup],
        fingerprint: &str,
    ) -> Option<&'a CloneGroup> {
        self.inner.find_group(groups, fingerprint)
    }
}

pub fn clone_fingerprint(instances: &[CloneInstance]) -> String {
    fallow_core::duplicates::clone_fingerprint(instances)
}

pub fn fingerprint_for_fragment(fragment: &str) -> String {
    fallow_core::duplicates::fingerprint_for_fragment(fragment)
}

pub fn dominant_identifier(group: &CloneGroup) -> Option<String> {
    fallow_core::duplicates::dominant_identifier(group)
}

pub fn refresh_clone_families(report: &mut DuplicationReport, root: &Path) {
    report.clone_families =
        fallow_core::duplicates::families::group_into_families(&report.clone_groups, root);
    report.mirrored_directories = fallow_core::duplicates::families::detect_mirrored_directories(
        &report.clone_families,
        root,
    );
}

pub fn rules_applying_to_path<'a>(
    config: &'a ResolvedConfig,
    rel_path: &str,
) -> Vec<(&'a str, &'a fallow_config::RulePackRule)> {
    fallow_core::analyze::rules_applying_to_path(&config.rule_packs, &config.boundaries, rel_path)
}

pub fn source_token_kinds_equivalent(
    path: &Path,
    current: &str,
    base: &str,
    cross_language: bool,
) -> bool {
    let current_tokens =
        fallow_core::duplicates::tokenize::tokenize_file(path, current, cross_language);
    let base_tokens = fallow_core::duplicates::tokenize::tokenize_file(path, base, cross_language);
    current_tokens
        .tokens
        .iter()
        .map(|token| &token.kind)
        .eq(base_tokens.tokens.iter().map(|token| &token.kind))
}

pub fn find_duplicates(
    root: &Path,
    files: &[DiscoveredFile],
    config: &DuplicatesConfig,
) -> DuplicationReport {
    fallow_core::duplicates::find_duplicates(root, files, config)
}

pub fn find_duplicates_cached(
    root: &Path,
    files: &[DiscoveredFile],
    config: &DuplicatesConfig,
    cache_dir: &Path,
) -> DuplicationReport {
    fallow_core::duplicates::find_duplicates_cached(root, files, config, cache_dir)
}

pub fn find_duplicates_with_defaults(
    root: &Path,
    files: &[DiscoveredFile],
    config: &DuplicatesConfig,
    cache_dir: Option<&Path>,
) -> DuplicationAnalysis {
    let (report, default_ignore_skips) = if let Some(cache_dir) = cache_dir {
        fallow_core::duplicates::find_duplicates_cached_with_default_ignore_skips(
            root, files, config, cache_dir,
        )
    } else {
        fallow_core::duplicates::find_duplicates_with_default_ignore_skips(root, files, config)
    };
    DuplicationAnalysis {
        report,
        default_ignore_skips,
    }
}

pub fn find_duplicates_touching_files_with_defaults(
    root: &Path,
    files: &[DiscoveredFile],
    config: &DuplicatesConfig,
    changed_files: &[PathBuf],
    cache_dir: Option<&Path>,
) -> DuplicationAnalysis {
    let changed_files = changed_files.iter().cloned().collect::<FxHashSet<_>>();
    let (report, default_ignore_skips) = if let Some(cache_dir) = cache_dir {
        fallow_core::duplicates::find_duplicates_touching_files_cached_with_default_ignore_skips(
            root,
            files,
            config,
            &changed_files,
            cache_dir,
        )
    } else {
        fallow_core::duplicates::find_duplicates_touching_files_with_default_ignore_skips(
            root,
            files,
            config,
            &changed_files,
        )
    };
    DuplicationAnalysis {
        report,
        default_ignore_skips,
    }
}

fn core_since_duration(duration: &SinceDuration) -> fallow_core::churn::SinceDuration {
    fallow_core::churn::SinceDuration {
        git_after: duration.git_after.clone(),
        display: duration.display.clone(),
    }
}

fn author_contribution(author: fallow_core::churn::AuthorContribution) -> AuthorContribution {
    AuthorContribution {
        commits: author.commits,
        weighted_commits: author.weighted_commits,
        first_commit_ts: author.first_commit_ts,
        last_commit_ts: author.last_commit_ts,
    }
}

fn file_churn(file: fallow_core::churn::FileChurn) -> FileChurn {
    FileChurn {
        path: file.path,
        commits: file.commits,
        weighted_commits: file.weighted_commits,
        lines_added: file.lines_added,
        lines_deleted: file.lines_deleted,
        trend: file.trend,
        authors: file
            .authors
            .into_iter()
            .map(|(index, author)| (index, author_contribution(author)))
            .collect(),
    }
}

fn churn_result(result: fallow_core::churn::ChurnResult) -> ChurnResult {
    ChurnResult {
        files: result
            .files
            .into_iter()
            .map(|(path, file)| (path, file_churn(file)))
            .collect(),
        shallow_clone: result.shallow_clone,
        author_pool: result.author_pool,
    }
}

pub fn set_churn_spawn_hook(hook: ChurnSpawnHook) {
    fallow_core::churn::set_spawn_hook(hook);
}

pub fn parse_since(input: &str) -> Result<SinceDuration, String> {
    fallow_core::churn::parse_since(input).map(|duration| SinceDuration {
        git_after: duration.git_after,
        display: duration.display,
    })
}

pub fn analyze_churn(root: &Path, since: &SinceDuration) -> Option<ChurnResult> {
    let since = core_since_duration(since);
    fallow_core::churn::analyze_churn(root, &since).map(churn_result)
}

pub fn analyze_churn_from_file(path: &Path, root: &Path) -> Result<ChurnResult, String> {
    fallow_core::churn::analyze_churn_from_file(path, root).map(churn_result)
}

pub fn is_git_repo(root: &Path) -> bool {
    fallow_core::churn::is_git_repo(root)
}

pub fn analyze_churn_cached(
    root: &Path,
    since: &SinceDuration,
    cache_dir: &Path,
    no_cache: bool,
) -> Option<(ChurnResult, bool)> {
    let since = core_since_duration(since);
    fallow_core::churn::analyze_churn_cached(root, &since, cache_dir, no_cache)
        .map(|(result, cache_hit)| (churn_result(result), cache_hit))
}

fn hidden_dir_scope(value: &fallow_core::discover::HiddenDirScope) -> HiddenDirScope {
    HiddenDirScope::new(value.root().to_path_buf(), value.dirs().to_vec())
}

fn core_hidden_dir_scope(value: &HiddenDirScope) -> fallow_core::discover::HiddenDirScope {
    fallow_core::discover::HiddenDirScope::new(value.root().to_path_buf(), value.dirs().to_vec())
}

fn core_hidden_dir_scopes(scopes: &[HiddenDirScope]) -> Vec<fallow_core::discover::HiddenDirScope> {
    scopes.iter().map(core_hidden_dir_scope).collect()
}

pub fn is_allowed_hidden_dir(name: &std::ffi::OsStr) -> bool {
    fallow_core::discover::is_allowed_hidden_dir(name)
}

pub fn collect_plugin_hidden_dir_scopes(
    config: &ResolvedConfig,
    root_pkg: Option<&PackageJson>,
    workspaces: &[WorkspaceInfo],
) -> Vec<HiddenDirScope> {
    fallow_core::discover::collect_plugin_hidden_dir_scopes(config, root_pkg, workspaces)
        .iter()
        .map(hidden_dir_scope)
        .collect()
}

pub fn collect_hidden_dir_scopes(
    config: &ResolvedConfig,
    root_pkg: Option<&PackageJson>,
    workspaces: &[WorkspaceInfo],
) -> Vec<HiddenDirScope> {
    fallow_core::discover::collect_hidden_dir_scopes(config, root_pkg, workspaces)
        .iter()
        .map(hidden_dir_scope)
        .collect()
}

pub fn discover_files_and_config_candidates(
    config: &ResolvedConfig,
    additional_hidden_dir_scopes: &[HiddenDirScope],
) -> (Vec<DiscoveredFile>, Vec<PathBuf>) {
    let scopes = core_hidden_dir_scopes(additional_hidden_dir_scopes);
    fallow_core::discover::discover_files_and_config_candidates(config, &scopes)
}

pub fn discover_entry_points(config: &ResolvedConfig, files: &[DiscoveredFile]) -> Vec<EntryPoint> {
    fallow_core::discover::discover_entry_points(config, files)
}

pub fn discover_workspace_entry_points(
    ws_root: &Path,
    config: &ResolvedConfig,
    all_files: &[DiscoveredFile],
) -> Vec<EntryPoint> {
    fallow_core::discover::discover_workspace_entry_points(ws_root, config, all_files)
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
