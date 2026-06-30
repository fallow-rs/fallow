//! fallow-core is the internal implementation crate behind the `fallow`
//! analyzer. External embedders should consume the curated programmatic
//! surface at `fallow_api` (e.g. `run_dead_code`,
//! `run_boundary_violations`, `run_duplication`, `run_health`). The typed
//! `run_*` functions are the primary embedder contract; serialize typed output
//! with the matching `serialize_*_programmatic_json` helper only at a protocol
//! boundary. See ADR-008 for the policy, and `docs/fallow-core-migration.md`
//! for the function-by-function migration map. Items in this crate may change
//! in any release, including patch releases; a subsequent minor will flip
//! `publish = false` so the crate is no longer fetchable from crates.io.

#![cfg_attr(not(test), deny(clippy::disallowed_methods))]
#![cfg_attr(
    test,
    allow(
        clippy::unwrap_used,
        clippy::expect_used,
        reason = "tests use unwrap and expect to keep fixture setup concise"
    )
)]

pub mod analyze;
pub mod cache;
pub mod changed_files;
pub mod churn;
pub mod cross_reference;
pub mod discover;
pub mod duplicates;
pub(crate) mod errors;
mod external_style_usage;
pub mod extract;
pub mod git_env;
mod package_assets;
pub mod plugins;
pub(crate) mod progress;
pub mod results;
pub(crate) mod scripts;
pub(crate) mod spawn;
pub mod suppress;
pub mod trace;
pub mod trace_chain;

pub use fallow_graph::cache as graph_cache;
pub use fallow_graph::graph;
pub use fallow_graph::project;
pub use fallow_graph::resolve;

use std::path::{Path, PathBuf};
use std::time::Instant;

use errors::FallowError;
use fallow_config::{
    EntryPointRole, PackageJson, ResolvedConfig, discover_workspaces,
    find_undeclared_workspaces_with_ignores,
};
use rayon::prelude::*;
use results::AnalysisResults;
use rustc_hash::FxHashSet;
use trace::PipelineTimings;

const UNDECLARED_WORKSPACE_WARNING_PREVIEW: usize = 5;
type LoadedWorkspacePackage<'a> = (&'a fallow_config::WorkspaceInfo, PackageJson);

fn record_graph_package_usage(
    graph: &mut graph::ModuleGraph,
    package_name: &str,
    file_id: discover::FileId,
    is_type_only: bool,
) {
    graph
        .package_usage
        .entry(package_name.to_owned())
        .or_default()
        .push(file_id);
    if is_type_only {
        graph
            .type_only_package_usage
            .entry(package_name.to_owned())
            .or_default()
            .push(file_id);
    }
}

fn workspace_package_name<'a>(
    source: &str,
    workspace_names: &'a FxHashSet<&str>,
) -> Option<&'a str> {
    if !resolve::is_bare_specifier(source) {
        return None;
    }
    let package_name = resolve::extract_package_name(source);
    workspace_names.get(package_name.as_str()).copied()
}

fn credit_workspace_package_usage(
    graph: &mut graph::ModuleGraph,
    resolved: &[resolve::ResolvedModule],
    workspaces: &[fallow_config::WorkspaceInfo],
) {
    if workspaces.is_empty() {
        return;
    }

    let workspace_names: FxHashSet<&str> = workspaces.iter().map(|ws| ws.name.as_str()).collect();
    for module in resolved {
        for import in module.all_resolved_imports() {
            if matches!(import.target, resolve::ResolveResult::InternalModule(_))
                && let Some(package_name) =
                    workspace_package_name(&import.info.source, &workspace_names)
            {
                record_graph_package_usage(
                    graph,
                    package_name,
                    module.file_id,
                    import.info.is_type_only,
                );
            }
        }

        for re_export in &module.re_exports {
            if matches!(re_export.target, resolve::ResolveResult::InternalModule(_))
                && let Some(package_name) =
                    workspace_package_name(&re_export.info.source, &workspace_names)
            {
                record_graph_package_usage(
                    graph,
                    package_name,
                    module.file_id,
                    re_export.info.is_type_only,
                );
            }
        }
    }
}

fn credit_package_path_references(graph: &mut graph::ModuleGraph, modules: &[extract::ModuleInfo]) {
    for module in modules {
        for package_name in &module.package_path_references {
            record_graph_package_usage(graph, package_name, module.file_id, false);
        }
    }
}

/// Result of the full analysis pipeline, including optional performance timings.
pub struct AnalysisOutput {
    pub results: AnalysisResults,
    pub timings: Option<PipelineTimings>,
    pub graph: Option<graph::ModuleGraph>,
    /// Parsed modules from the pipeline, available when `retain_modules` is true.
    /// Used by combined and LSP flows to share downstream module data.
    /// Graph-only extraction payloads are released after graph construction.
    pub modules: Option<Vec<extract::ModuleInfo>>,
    /// Discovered files from the pipeline, available when `retain_modules` is true.
    pub files: Option<Vec<discover::DiscoveredFile>>,
    /// Package names invoked from package.json scripts and CI configs, mirroring
    /// what the unused-deps detector consults. Populated for every pipeline run;
    /// trace tooling reads it so `trace_dependency` agrees with `unused-deps` on
    /// "used vs unused" instead of returning false-negatives for script-only deps.
    pub script_used_packages: rustc_hash::FxHashSet<String>,
    /// xxh3 content hash of every parsed source file, keyed by absolute path.
    /// Used by `fallow fix` to detect on-disk drift between the in-process
    /// analysis read and the per-file write; if the file's current hash
    /// differs from the captured value, the fix for that file is skipped
    /// with a clear diagnostic and exit 2. The hash is the same value
    /// extract/cache uses for cache invalidation, so a cached parse contributes
    /// the same hash as a fresh parse. Roughly 8 bytes per file (negligible
    /// memory cost even on 100k-file projects).
    pub file_hashes: rustc_hash::FxHashMap<std::path::PathBuf, u64>,
}

/// Update cache: write freshly parsed modules and refresh stale mtime/size entries.
fn update_cache(
    store: &mut cache::CacheStore,
    modules: &[extract::ModuleInfo],
    files: &[discover::DiscoveredFile],
) -> bool {
    let mut dirty = false;
    for module in modules {
        if let Some(file) = files.get(module.file_id.0 as usize) {
            let fingerprint = file_fingerprint(&file.path);
            if let Some(cached) = store.get_by_path_only(&file.path)
                && cached.content_hash == module.content_hash
            {
                if cached.source_fingerprint() != fingerprint {
                    let preserved_last_access = cached.last_access_secs;
                    let mut refreshed = cache::module_to_cached(module, fingerprint);
                    refreshed.last_access_secs = preserved_last_access;
                    store.insert(&file.path, refreshed);
                    dirty = true;
                }
                continue;
            }
            store.insert(&file.path, cache::module_to_cached(module, fingerprint));
            dirty = true;
        }
    }
    let removed_stale_paths = store.retain_paths(files);
    dirty || removed_stale_paths
}

/// Resolve `config.cache_max_size_mb` into bytes, falling back to the
/// extract crate's `DEFAULT_CACHE_MAX_SIZE`. Lives at this layer (not on
/// `ResolvedConfig`) because `fallow-config` does not depend on
/// `fallow-extract`; the bytes conversion is owned by the cache callsite.
/// Public so CLI subcommands that load the cache directly (`flags`,
/// `health`, `coverage analyze`) can call it without re-deriving the
/// same fallback policy.
#[must_use]
pub fn resolve_cache_max_size_bytes(config: &ResolvedConfig) -> usize {
    config
        .cache_max_size_mb
        .map_or(cache::DEFAULT_CACHE_MAX_SIZE, |mb| {
            (mb as usize).saturating_mul(1024 * 1024)
        })
}

/// Extract source fingerprint metadata from a path.
fn file_fingerprint(path: &std::path::Path) -> fallow_types::source_fingerprint::SourceFingerprint {
    std::fs::metadata(path).map_or(
        fallow_types::source_fingerprint::SourceFingerprint::new(0, 0),
        |metadata| fallow_types::source_fingerprint::SourceFingerprint::from_metadata(&metadata),
    )
}

fn format_undeclared_workspace_warning(
    root: &Path,
    undeclared: &[fallow_config::WorkspaceDiagnostic],
) -> Option<String> {
    if undeclared.is_empty() {
        return None;
    }

    let preview = undeclared
        .iter()
        .take(UNDECLARED_WORKSPACE_WARNING_PREVIEW)
        .map(|diag| {
            diag.path
                .strip_prefix(root)
                .unwrap_or(&diag.path)
                .display()
                .to_string()
                .replace('\\', "/")
        })
        .collect::<Vec<_>>();
    let remaining = undeclared
        .len()
        .saturating_sub(UNDECLARED_WORKSPACE_WARNING_PREVIEW);
    let tail = if remaining > 0 {
        format!(" (and {remaining} more)")
    } else {
        String::new()
    };
    let noun = if undeclared.len() == 1 {
        "directory with package.json is"
    } else {
        "directories with package.json are"
    };
    let guidance = if undeclared.len() == 1 {
        "Add that path to package.json workspaces or pnpm-workspace.yaml if it should be analyzed as a workspace."
    } else {
        "Add those paths to package.json workspaces or pnpm-workspace.yaml if they should be analyzed as workspaces."
    };

    Some(format!(
        "{} {} not declared as {}: {}{}. {}",
        undeclared.len(),
        noun,
        if undeclared.len() == 1 {
            "a workspace"
        } else {
            "workspaces"
        },
        preview.join(", "),
        tail,
        guidance
    ))
}

fn warn_undeclared_workspaces(
    root: &Path,
    workspaces_vec: &[fallow_config::WorkspaceInfo],
    ignore_patterns: &globset::GlobSet,
    quiet: bool,
) {
    let undeclared = find_undeclared_workspaces_with_ignores(root, workspaces_vec, ignore_patterns);
    if undeclared.is_empty() {
        return;
    }

    let existing = fallow_config::workspace_diagnostics_for(root);
    let already_flagged: rustc_hash::FxHashSet<PathBuf> = existing
        .iter()
        .map(|d| dunce::canonicalize(&d.path).unwrap_or_else(|_| d.path.clone()))
        .collect();
    let undeclared: Vec<_> = undeclared
        .into_iter()
        .filter(|diag| {
            let canonical = dunce::canonicalize(&diag.path).unwrap_or_else(|_| diag.path.clone());
            !already_flagged.contains(&canonical)
        })
        .collect();
    if undeclared.is_empty() {
        return;
    }

    fallow_config::append_workspace_diagnostics(root, undeclared.clone());

    if !quiet && let Some(message) = format_undeclared_workspace_warning(root, &undeclared) {
        tracing::warn!("{message}");
    }
}

/// Run the full analysis pipeline.
///
/// # Errors
///
/// Returns an error if file discovery, parsing, or analysis fails.
#[deprecated(
    since = "2.76.0",
    note = "fallow_core is internal; use fallow_api::run_dead_code for typed output; serialize with fallow_api::serialize_dead_code_programmatic_json for JSON output. See docs/fallow-core-migration.md and ADR-008."
)]
pub fn analyze(config: &ResolvedConfig) -> Result<AnalysisResults, FallowError> {
    let output = analyze_full(config, false, false, false, false)?;
    Ok(output.results)
}

/// Run the full analysis pipeline with export usage collection (for LSP Code Lens).
///
/// # Errors
///
/// Returns an error if file discovery, parsing, or analysis fails.
#[deprecated(
    since = "2.76.0",
    note = "fallow_core is internal; use fallow_api::run_dead_code for public typed output. NOTE: export-usage collection is not exposed in the programmatic surface today. See docs/fallow-core-migration.md and ADR-008."
)]
pub fn analyze_with_usages(config: &ResolvedConfig) -> Result<AnalysisResults, FallowError> {
    let output = analyze_full(config, false, true, false, false)?;
    Ok(output.results)
}

/// Run the full analysis pipeline with export usage collection from a reusable
/// discovery prelude.
///
/// # Errors
///
/// Returns an error if parsing or analysis fails.
pub fn analyze_with_usages_from_discovery(
    config: &ResolvedConfig,
    discovery: &AnalysisDiscovery,
) -> Result<AnalysisResults, FallowError> {
    let output = analyze_full_from_discovery(config, discovery, false, true, false, false)?;
    Ok(output.results)
}

/// Run the full analysis pipeline with export usage collection and retained
/// per-function complexity modules.
///
/// Used by the LSP when opt-in inline complexity code lenses are enabled so
/// the editor keeps existing export reference lenses while also reading
/// complexity data from the same parse.
///
/// # Errors
///
/// Returns an error if file discovery, parsing, or analysis fails.
#[deprecated(
    since = "2.90.0",
    note = "fallow_core is internal; use fallow_api::run_dead_code and run_health instead. NOTE: this combined LSP-only typed surface is not exposed externally. See docs/fallow-core-migration.md and ADR-008."
)]
pub fn analyze_with_usages_and_complexity(
    config: &ResolvedConfig,
) -> Result<AnalysisOutput, FallowError> {
    analyze_full(config, false, true, true, true)
}

/// Run the full analysis pipeline with export usage collection and retained
/// complexity artifacts from a reusable discovery prelude.
///
/// # Errors
///
/// Returns an error if parsing or analysis fails.
pub fn analyze_with_usages_and_complexity_from_discovery(
    config: &ResolvedConfig,
    discovery: &AnalysisDiscovery,
) -> Result<AnalysisOutput, FallowError> {
    analyze_full_from_discovery(config, discovery, false, true, true, true)
}

/// Run the full analysis pipeline with optional performance timings and graph retention.
///
/// # Errors
///
/// Returns an error if file discovery, parsing, or analysis fails.
#[deprecated(
    since = "2.76.0",
    note = "fallow_core is internal; use fallow_api::run_dead_code for public typed output. NOTE: trace timings are not exposed in the programmatic surface today; use `fallow dead-code --performance` for CLI-side timings. See docs/fallow-core-migration.md and ADR-008."
)]
pub fn analyze_with_trace(config: &ResolvedConfig) -> Result<AnalysisOutput, FallowError> {
    analyze_full(config, true, false, false, false)
}

/// Run the full analysis pipeline and return the full `AnalysisOutput`, including
/// `file_hashes` (used by `fallow fix` to detect on-disk drift between analysis
/// and per-file write). Graphs and modules are NOT retained; the only difference
/// from `analyze` is that the caller can access `AnalysisOutput.file_hashes`.
///
/// # Errors
///
/// Returns an error if file discovery, parsing, or analysis fails.
#[deprecated(
    since = "2.76.0",
    note = "fallow_core is internal; the CLI fix command uses this via the workspace path dependency. External embedders should use fallow_api::run_dead_code for typed output; serialize with fallow_api::serialize_dead_code_programmatic_json for JSON output. See docs/fallow-core-migration.md and ADR-008."
)]
pub fn analyze_with_file_hashes(config: &ResolvedConfig) -> Result<AnalysisOutput, FallowError> {
    analyze_full(config, false, false, false, false)
}

/// Run the full analysis pipeline, retaining parsed modules and discovered files.
///
/// Used by the combined command to share a single parse across dead-code and health.
/// When `need_complexity` is true, the `ComplexityVisitor` runs during parsing so
/// the returned modules contain per-function complexity data.
///
/// # Errors
///
/// Returns an error if file discovery, parsing, or analysis fails.
#[deprecated(
    since = "2.76.0",
    note = "fallow_core is internal; use fallow_api::run_dead_code for public typed output. NOTE: combined-mode module retention is not exposed in the programmatic surface today. See docs/fallow-core-migration.md and ADR-008."
)]
pub fn analyze_retaining_modules(
    config: &ResolvedConfig,
    need_complexity: bool,
    retain_graph: bool,
) -> Result<AnalysisOutput, FallowError> {
    analyze_full(config, retain_graph, false, need_complexity, true)
}

/// Run the full analysis pipeline with retained modules/files from a reusable
/// discovery prelude.
///
/// # Errors
///
/// Returns an error if parsing or analysis fails.
pub fn analyze_retaining_modules_from_discovery(
    config: &ResolvedConfig,
    discovery: &AnalysisDiscovery,
    need_complexity: bool,
    retain_graph: bool,
) -> Result<AnalysisOutput, FallowError> {
    analyze_full_from_discovery(
        config,
        discovery,
        retain_graph,
        false,
        need_complexity,
        true,
    )
}

fn new_analysis_progress(config: &ResolvedConfig) -> progress::AnalysisProgress {
    let show_progress = !config.quiet
        && std::io::IsTerminal::is_terminal(&std::io::stderr())
        && matches!(
            config.output,
            fallow_config::OutputFormat::Human
                | fallow_config::OutputFormat::Compact
                | fallow_config::OutputFormat::Markdown
        );
    progress::AnalysisProgress::new(show_progress)
}

fn warn_missing_node_modules(config: &ResolvedConfig) {
    if config.root.join("node_modules").is_dir() {
        return;
    }

    tracing::warn!(
        "node_modules directory not found. Run `npm install` / `pnpm install` first for accurate results."
    );
}

fn discover_analysis_workspaces(
    config: &ResolvedConfig,
) -> (Vec<fallow_config::WorkspaceInfo>, f64) {
    let t = Instant::now();
    let workspaces = discover_workspaces(&config.root);
    let workspaces_ms = t.elapsed().as_secs_f64() * 1000.0;
    if !workspaces.is_empty() {
        tracing::info!(count = workspaces.len(), "workspaces discovered");
    }

    warn_undeclared_workspaces(
        &config.root,
        &workspaces,
        &config.ignore_patterns,
        config.quiet,
    );

    (workspaces, workspaces_ms)
}

/// Owned products of the shared pipeline prelude: progress reporter, project
/// state (owns discovered files and workspaces), root package.json, and the
/// discovery/workspace timings.
struct AnalysisSetup {
    progress: progress::AnalysisProgress,
    project: project::ProjectState,
    root_pkg: Option<PackageJson>,
    /// Non-source config-candidate files captured by the same discovery walk,
    /// used to resolve plugin config patterns in-memory (empty in production
    /// mode, where the filesystem path is kept). Carried alongside `project`
    /// rather than inside it to avoid churning `ProjectState`'s many callers.
    config_candidates: Vec<std::path::PathBuf>,
    discover_ms: f64,
    workspaces_ms: f64,
}

/// Reusable discovery prelude for a resolved project.
///
/// This carries the file registry plus the workspace and config-candidate state
/// that plugin detection needs, so engine sessions can run several analyses
/// over one stable discovery boundary without re-walking the project.
#[derive(Debug, Clone)]
pub struct AnalysisDiscovery {
    files: Vec<discover::DiscoveredFile>,
    workspaces: Vec<fallow_config::WorkspaceInfo>,
    root_pkg: Option<PackageJson>,
    config_candidates: Vec<std::path::PathBuf>,
    discover_ms: f64,
    workspaces_ms: f64,
}

impl AnalysisDiscovery {
    /// Discovered source files, indexed by stable `FileId` for this session.
    #[must_use]
    pub fn files(&self) -> &[discover::DiscoveredFile] {
        &self.files
    }

    /// Consume this discovery prelude and return its source file registry.
    #[must_use]
    pub fn into_files(self) -> Vec<discover::DiscoveredFile> {
        self.files
    }
}

/// Owned state shared across one core analysis run.
///
/// This is the first non-persisted session boundary for the engine. It keeps
/// discovery, workspace and package prelude state together so later slices can
/// reuse pipeline products without introducing graph-cache invalidation risk.
pub(crate) struct AnalysisSession<'a> {
    config: &'a ResolvedConfig,
    pipeline_start: Instant,
    progress: progress::AnalysisProgress,
    project: project::ProjectState,
    root_pkg: Option<PackageJson>,
    config_candidates: Vec<std::path::PathBuf>,
    discover_ms: f64,
    workspaces_ms: f64,
}

impl<'a> AnalysisSession<'a> {
    fn new(config: &'a ResolvedConfig) -> Self {
        let pipeline_start = Instant::now();
        let AnalysisSetup {
            progress,
            project,
            root_pkg,
            config_candidates,
            discover_ms,
            workspaces_ms,
        } = run_analysis_setup(config);

        Self {
            config,
            pipeline_start,
            progress,
            project,
            root_pkg,
            config_candidates,
            discover_ms,
            workspaces_ms,
        }
    }

    fn from_discovery(config: &'a ResolvedConfig, discovery: AnalysisDiscovery) -> Self {
        Self {
            config,
            pipeline_start: Instant::now(),
            progress: new_analysis_progress(config),
            project: project::ProjectState::new(discovery.files, discovery.workspaces),
            root_pkg: discovery.root_pkg,
            config_candidates: discovery.config_candidates,
            discover_ms: discovery.discover_ms,
            workspaces_ms: discovery.workspaces_ms,
        }
    }

    fn files(&self) -> &[discover::DiscoveredFile] {
        self.project.files()
    }

    fn workspaces(&self) -> &[fallow_config::WorkspaceInfo] {
        self.project.workspaces()
    }

    fn load_workspace_packages(&self) -> Vec<LoadedWorkspacePackage<'_>> {
        load_workspace_packages(self.workspaces())
    }

    fn run_plugins_and_scripts(
        &self,
        workspace_pkgs: &[LoadedWorkspacePackage<'_>],
    ) -> Result<(plugins::AggregatedPluginResult, f64, f64), FallowError> {
        run_plugins_and_scripts(&PluginScriptInput {
            config: self.config,
            progress: &self.progress,
            files: self.files(),
            workspaces: self.workspaces(),
            root_pkg: self.root_pkg.as_ref(),
            workspace_pkgs,
            config_candidates: &self.config_candidates,
        })
    }

    fn prelude_timings(&self, plugins_ms: f64, scripts_ms: f64) -> PreludeTimings {
        PreludeTimings {
            discover_ms: self.discover_ms,
            workspaces_ms: self.workspaces_ms,
            plugins_ms,
            scripts_ms,
        }
    }

    fn parse_modules(&self, need_complexity: bool) -> AnalysisParseOutput {
        let t = Instant::now();
        self.progress
            .set_stage(&format!("parsing {} files...", self.files().len()));
        parse_analysis_modules(self.config, self.files(), need_complexity, t)
    }

    fn run_owned_core(
        &self,
        workspace_pkgs: &[LoadedWorkspacePackage<'_>],
        plugin_result: &plugins::AggregatedPluginResult,
        mut modules: Vec<extract::ModuleInfo>,
        collect_usages: bool,
    ) -> OwnedAnalysisCore {
        let shared = AnalysisCoreSharedInput {
            config: self.config,
            progress: &self.progress,
            files: self.files(),
            workspaces: self.workspaces(),
            root_pkg: self.root_pkg.as_ref(),
            workspace_pkgs,
            plugin_result,
        };

        let entry_points = discover_analysis_entry_points(&shared);
        let resolved = resolve_analysis_imports_timed(&shared, &modules);
        let graph =
            build_analysis_graph_timed(&shared, &resolved.resolved, &entry_points, &modules);
        release_resolution_payloads(&mut modules);
        let analysis = analyze_dead_code_timed(
            &shared,
            &graph.graph,
            &resolved.resolved,
            &modules,
            collect_usages,
            entry_points.summary,
        );

        OwnedAnalysisCore {
            result: analysis.result,
            graph: graph.graph,
            modules,
            entry_point_count: entry_points.count,
            entry_points_ms: entry_points.elapsed_ms,
            resolve_ms: resolved.elapsed_ms,
            graph_ms: graph.elapsed_ms,
            analyze_ms: analysis.elapsed_ms,
        }
    }

    fn run_full(
        self,
        retain: bool,
        collect_usages: bool,
        need_complexity: bool,
        retain_modules: bool,
    ) -> Result<AnalysisOutput, FallowError> {
        let workspace_pkgs = self.load_workspace_packages();
        let (plugin_result, plugins_ms, scripts_ms) =
            self.run_plugins_and_scripts(&workspace_pkgs)?;

        let AnalysisParseOutput { modules, metrics } = self.parse_modules(need_complexity);
        let core = self.run_owned_core(&workspace_pkgs, &plugin_result, modules, collect_usages);
        self.progress.finish();

        let profile = full_analysis_pipeline_profile(
            &self.prelude_timings(plugins_ms, scripts_ms),
            self.pipeline_start,
            self.files(),
            self.workspaces(),
            &core,
            &metrics,
        );
        trace_pipeline_profile(&profile);

        Ok(assemble_full_output(
            core,
            plugin_result,
            &profile,
            self.files(),
            retain,
            retain_modules,
        ))
    }

    fn run_with_parse_result(
        self,
        modules: &[extract::ModuleInfo],
    ) -> Result<AnalysisOutput, FallowError> {
        let workspace_pkgs = self.load_workspace_packages();
        let (plugin_result, plugins_ms, scripts_ms) =
            self.run_plugins_and_scripts(&workspace_pkgs)?;

        let core = run_reused_analysis_core(&ReusedAnalysisCoreInput {
            config: self.config,
            progress: &self.progress,
            files: self.files(),
            workspaces: self.workspaces(),
            root_pkg: self.root_pkg.as_ref(),
            workspace_pkgs: &workspace_pkgs,
            plugin_result: &plugin_result,
            modules,
        });
        self.progress.finish();

        let timings = self.prelude_timings(plugins_ms, scripts_ms);
        let prelude = prelude_metrics(
            &timings,
            self.pipeline_start,
            self.files(),
            self.workspaces(),
            modules.len(),
        );
        let profile = reused_pipeline_profile(&prelude, &core);
        trace_reused_pipeline_profile(&profile);

        Ok(AnalysisOutput {
            results: core.result,
            timings: retained_pipeline_timings(true, &profile),
            graph: Some(core.graph),
            modules: None,
            files: None,
            script_used_packages: plugin_result.script_used_packages,
            file_hashes: collect_file_hashes(modules, self.files()),
        })
    }
}

/// Run the shared prelude: progress setup, node_modules check, workspace and
/// root-package discovery, hidden-dir scoping, and file discovery.
fn run_analysis_setup(config: &ResolvedConfig) -> AnalysisSetup {
    let progress = new_analysis_progress(config);
    warn_missing_node_modules(config);

    let (workspaces_vec, workspaces_ms) = discover_analysis_workspaces(config);
    let root_pkg = load_root_package_json(config);
    let discovery_hidden_dir_scopes =
        discover::collect_hidden_dir_scopes(config, root_pkg.as_ref(), &workspaces_vec);

    let t = Instant::now();
    progress.set_stage("discovering files...");
    let (discovered_files, config_candidates) =
        discover::discover_files_and_config_candidates(config, &discovery_hidden_dir_scopes);
    let discover_ms = t.elapsed().as_secs_f64() * 1000.0;

    let project = project::ProjectState::new(discovered_files, workspaces_vec);

    AnalysisSetup {
        progress,
        project,
        root_pkg,
        config_candidates,
        discover_ms,
        workspaces_ms,
    }
}

/// Run the shared discovery prelude once for callers that need a reusable
/// analysis session.
///
/// # Panics
///
/// Panics only when the underlying source discovery walk hits its documented
/// compile-time glob/template invariants.
#[must_use]
pub fn prepare_analysis_discovery(config: &ResolvedConfig) -> AnalysisDiscovery {
    let AnalysisSetup {
        progress,
        project,
        root_pkg,
        config_candidates,
        discover_ms,
        workspaces_ms,
    } = run_analysis_setup(config);
    progress.finish();

    AnalysisDiscovery {
        files: project.files().to_vec(),
        workspaces: project.workspaces().to_vec(),
        root_pkg,
        config_candidates,
        discover_ms,
        workspaces_ms,
    }
}

/// Borrowed inputs for plugin detection and script analysis.
struct PluginScriptInput<'a> {
    config: &'a ResolvedConfig,
    progress: &'a progress::AnalysisProgress,
    files: &'a [discover::DiscoveredFile],
    workspaces: &'a [fallow_config::WorkspaceInfo],
    root_pkg: Option<&'a PackageJson>,
    workspace_pkgs: &'a [LoadedWorkspacePackage<'a>],
    config_candidates: &'a [std::path::PathBuf],
}

/// Run plugin detection and package.json/CI script analysis, returning the
/// aggregated plugin result plus the two phase timings.
fn run_plugins_and_scripts(
    input: &PluginScriptInput<'_>,
) -> Result<(plugins::AggregatedPluginResult, f64, f64), FallowError> {
    let t = Instant::now();
    input.progress.set_stage("detecting plugins...");
    let mut plugin_result = run_plugins(
        input.config,
        input.files,
        input.workspaces,
        input.root_pkg,
        input.workspace_pkgs,
        input.config_candidates,
    )?;
    let plugins_ms = t.elapsed().as_secs_f64() * 1000.0;

    let t = Instant::now();
    analyze_all_scripts(
        input.config,
        input.workspaces,
        input.root_pkg,
        input.workspace_pkgs,
        &mut plugin_result,
    );
    let scripts_ms = t.elapsed().as_secs_f64() * 1000.0;

    Ok((plugin_result, plugins_ms, scripts_ms))
}

/// Run the analysis pipeline using pre-parsed modules, skipping the parsing stage.
///
/// This avoids re-parsing files when the caller already has a `ParseResult` (e.g., from
/// `fallow_core::extract::parse_all_files`). Discovery, plugins, scripts, entry points,
/// import resolution, graph construction, and dead code detection still run normally.
/// The graph is always retained (needed for file scores). Caller-owned modules
/// are borrowed and are not compacted by this API.
///
/// # Errors
///
/// Returns an error if discovery, graph construction, or analysis fails.
#[deprecated(
    since = "2.76.0",
    note = "fallow_core is internal; use fallow_api::run_dead_code for public typed output. NOTE: pre-parsed module reuse is not exposed in the programmatic surface today. See docs/fallow-core-migration.md and ADR-008."
)]
pub fn analyze_with_parse_result(
    config: &ResolvedConfig,
    modules: &[extract::ModuleInfo],
) -> Result<AnalysisOutput, FallowError> {
    let _span = tracing::info_span!("fallow_analyze_with_parse_result").entered();
    AnalysisSession::new(config).run_with_parse_result(modules)
}

/// Prelude/aggregate metrics shared between the parse and reuse pipeline paths
/// when assembling the `PipelineProfile`.
struct PreludeMetrics {
    discover_ms: f64,
    workspaces_ms: f64,
    plugins_ms: f64,
    scripts_ms: f64,
    total_ms: f64,
    file_count: usize,
    workspace_count: usize,
    module_count: usize,
}

/// The four prelude phase timings (discovery through script analysis).
#[expect(
    clippy::struct_field_names,
    reason = "timings are all milliseconds; the _ms suffix is the unit"
)]
struct PreludeTimings {
    discover_ms: f64,
    workspaces_ms: f64,
    plugins_ms: f64,
    scripts_ms: f64,
}

/// Build `PreludeMetrics` from the prelude timings, pipeline start instant, and
/// the discovered file/workspace/module counts.
fn prelude_metrics(
    timings: &PreludeTimings,
    pipeline_start: Instant,
    files: &[discover::DiscoveredFile],
    workspaces: &[fallow_config::WorkspaceInfo],
    module_count: usize,
) -> PreludeMetrics {
    PreludeMetrics {
        discover_ms: timings.discover_ms,
        workspaces_ms: timings.workspaces_ms,
        plugins_ms: timings.plugins_ms,
        scripts_ms: timings.scripts_ms,
        total_ms: pipeline_start.elapsed().as_secs_f64() * 1000.0,
        file_count: files.len(),
        workspace_count: workspaces.len(),
        module_count,
    }
}

/// Assemble the `PipelineProfile` for the reused-module path (parse/cache phases
/// are skipped, so their timings are zero).
fn reused_pipeline_profile(prelude: &PreludeMetrics, core: &ReusedAnalysisCore) -> PipelineProfile {
    PipelineProfile {
        discover_ms: prelude.discover_ms,
        workspaces_ms: prelude.workspaces_ms,
        plugins_ms: prelude.plugins_ms,
        scripts_ms: prelude.scripts_ms,
        parse_ms: 0.0,
        cache_ms: 0.0,
        entry_points_ms: core.entry_points_ms,
        resolve_ms: core.resolve_ms,
        graph_ms: core.graph_ms,
        analyze_ms: core.analyze_ms,
        total_ms: prelude.total_ms,
        file_count: prelude.file_count,
        workspace_count: prelude.workspace_count,
        module_count: prelude.module_count,
        entry_point_count: core.entry_point_count,
        cache_hits: 0,
        cache_misses: 0,
        parse_cpu_ms: 0.0,
    }
}

/// Borrowed inputs for `run_reused_analysis_core`.
struct ReusedAnalysisCoreInput<'a> {
    config: &'a ResolvedConfig,
    progress: &'a progress::AnalysisProgress,
    files: &'a [discover::DiscoveredFile],
    workspaces: &'a [fallow_config::WorkspaceInfo],
    root_pkg: Option<&'a PackageJson>,
    workspace_pkgs: &'a [LoadedWorkspacePackage<'a>],
    plugin_result: &'a plugins::AggregatedPluginResult,
    modules: &'a [extract::ModuleInfo],
}

/// Result of the reused-module analysis core, with the per-phase timings the
/// caller folds into the pipeline profile.
struct ReusedAnalysisCore {
    result: AnalysisResults,
    graph: graph::ModuleGraph,
    entry_point_count: usize,
    entry_points_ms: f64,
    resolve_ms: f64,
    graph_ms: f64,
    analyze_ms: f64,
}

struct AnalysisCoreSharedInput<'a> {
    config: &'a ResolvedConfig,
    progress: &'a progress::AnalysisProgress,
    files: &'a [discover::DiscoveredFile],
    workspaces: &'a [fallow_config::WorkspaceInfo],
    root_pkg: Option<&'a PackageJson>,
    workspace_pkgs: &'a [LoadedWorkspacePackage<'a>],
    plugin_result: &'a plugins::AggregatedPluginResult,
}

struct TimedEntryPoints {
    entry_points: discover::CategorizedEntryPoints,
    summary: results::EntryPointSummary,
    count: usize,
    elapsed_ms: f64,
}

struct TimedResolvedModules {
    resolved: Vec<resolve::ResolvedModule>,
    elapsed_ms: f64,
}

struct TimedGraph {
    graph: graph::ModuleGraph,
    elapsed_ms: f64,
}

struct TimedAnalysis {
    result: AnalysisResults,
    elapsed_ms: f64,
}

/// Run entry-point discovery, import resolution, graph construction, and dead-code
/// analysis over caller-owned modules (which are copied before payload release).
fn run_reused_analysis_core(input: &ReusedAnalysisCoreInput<'_>) -> ReusedAnalysisCore {
    let &ReusedAnalysisCoreInput {
        config,
        progress,
        files,
        workspaces,
        root_pkg,
        workspace_pkgs,
        plugin_result,
        modules,
    } = input;
    let shared = AnalysisCoreSharedInput {
        config,
        progress,
        files,
        workspaces,
        root_pkg,
        workspace_pkgs,
        plugin_result,
    };

    let entry_points = discover_analysis_entry_points(&shared);
    let resolved = resolve_analysis_imports_timed(&shared, modules);
    let graph = build_analysis_graph_timed(&shared, &resolved.resolved, &entry_points, modules);

    let mut analysis_modules = modules.to_vec();
    release_resolution_payloads(&mut analysis_modules);
    let analysis = analyze_dead_code_timed(
        &shared,
        &graph.graph,
        &resolved.resolved,
        &analysis_modules,
        false,
        entry_points.summary,
    );

    ReusedAnalysisCore {
        result: analysis.result,
        graph: graph.graph,
        entry_point_count: entry_points.count,
        entry_points_ms: entry_points.elapsed_ms,
        resolve_ms: resolved.elapsed_ms,
        graph_ms: graph.elapsed_ms,
        analyze_ms: analysis.elapsed_ms,
    }
}

fn discover_analysis_entry_points(input: &AnalysisCoreSharedInput<'_>) -> TimedEntryPoints {
    let t = Instant::now();
    let entry_points = discover_all_entry_points(
        input.config,
        input.files,
        input.workspaces,
        input.root_pkg,
        input.workspace_pkgs,
        input.plugin_result,
    );
    let elapsed_ms = t.elapsed().as_secs_f64() * 1000.0;
    let summary = summarize_entry_points(&entry_points.all);
    let count = entry_points.all.len();

    TimedEntryPoints {
        entry_points,
        summary,
        count,
        elapsed_ms,
    }
}

fn resolve_analysis_imports_timed(
    input: &AnalysisCoreSharedInput<'_>,
    modules: &[extract::ModuleInfo],
) -> TimedResolvedModules {
    let t = Instant::now();
    input.progress.set_stage("resolving imports...");
    let resolved = resolve_analysis_imports(
        modules,
        input.files,
        input.workspaces,
        input.plugin_result,
        input.config,
    );
    TimedResolvedModules {
        resolved,
        elapsed_ms: t.elapsed().as_secs_f64() * 1000.0,
    }
}

fn build_analysis_graph_timed(
    input: &AnalysisCoreSharedInput<'_>,
    resolved: &[resolve::ResolvedModule],
    entry_points: &TimedEntryPoints,
    modules: &[extract::ModuleInfo],
) -> TimedGraph {
    let t = Instant::now();
    input.progress.set_stage("building module graph...");
    let graph = build_analysis_graph(&BuildAnalysisGraphInput {
        config: input.config,
        plugin_result: input.plugin_result,
        resolved,
        entry_points: &entry_points.entry_points,
        files: input.files,
        modules,
        workspaces: input.workspaces,
    });
    TimedGraph {
        graph,
        elapsed_ms: t.elapsed().as_secs_f64() * 1000.0,
    }
}

fn release_resolution_payloads(modules: &mut [extract::ModuleInfo]) {
    for module in modules {
        module.release_resolution_payload();
    }
}

fn analyze_dead_code_timed(
    input: &AnalysisCoreSharedInput<'_>,
    graph: &graph::ModuleGraph,
    resolved: &[resolve::ResolvedModule],
    modules: &[extract::ModuleInfo],
    collect_usages: bool,
    entry_point_summary: results::EntryPointSummary,
) -> TimedAnalysis {
    let t = Instant::now();
    input.progress.set_stage("analyzing...");
    #[expect(
        deprecated,
        reason = "ADR-008 keeps workspace path-dependency calls while warning external fallow-core consumers"
    )]
    let mut result = analyze::find_dead_code_full(
        graph,
        input.config,
        resolved,
        Some(input.plugin_result),
        input.workspaces,
        modules,
        collect_usages,
    );
    result.entry_point_summary = Some(entry_point_summary);
    TimedAnalysis {
        result,
        elapsed_ms: t.elapsed().as_secs_f64() * 1000.0,
    }
}

fn analyze_full(
    config: &ResolvedConfig,
    retain: bool,
    collect_usages: bool,
    need_complexity: bool,
    retain_modules: bool,
) -> Result<AnalysisOutput, FallowError> {
    let _span = tracing::info_span!("fallow_analyze").entered();
    AnalysisSession::new(config).run_full(retain, collect_usages, need_complexity, retain_modules)
}

fn analyze_full_from_discovery(
    config: &ResolvedConfig,
    discovery: &AnalysisDiscovery,
    retain: bool,
    collect_usages: bool,
    need_complexity: bool,
    retain_modules: bool,
) -> Result<AnalysisOutput, FallowError> {
    let _span = tracing::info_span!("fallow_analyze").entered();
    AnalysisSession::from_discovery(config, discovery.clone()).run_full(
        retain,
        collect_usages,
        need_complexity,
        retain_modules,
    )
}

fn full_analysis_pipeline_profile(
    timings: &PreludeTimings,
    pipeline_start: Instant,
    files: &[discover::DiscoveredFile],
    workspaces: &[fallow_config::WorkspaceInfo],
    core: &OwnedAnalysisCore,
    metrics: &ParseMetrics,
) -> PipelineProfile {
    let prelude = prelude_metrics(
        timings,
        pipeline_start,
        files,
        workspaces,
        core.modules.len(),
    );
    full_pipeline_profile(&prelude, core, metrics)
}

/// Assemble the `AnalysisOutput` for the full pipeline, honoring the graph/module
/// retention flags and computing per-file content hashes.
fn assemble_full_output(
    core: OwnedAnalysisCore,
    plugin_result: plugins::AggregatedPluginResult,
    profile: &PipelineProfile,
    files: &[discover::DiscoveredFile],
    retain: bool,
    retain_modules: bool,
) -> AnalysisOutput {
    let file_hashes = collect_file_hashes(&core.modules, files);
    AnalysisOutput {
        results: core.result,
        timings: retained_pipeline_timings(retain, profile),
        graph: if retain { Some(core.graph) } else { None },
        modules: if retain_modules {
            Some(core.modules)
        } else {
            None
        },
        files: if retain_modules {
            Some(files.to_vec())
        } else {
            None
        },
        script_used_packages: plugin_result.script_used_packages,
        file_hashes,
    }
}

/// Result of the freshly-parsed analysis core; returns the owned `modules` (so the
/// caller can retain them) plus the per-phase timings.
struct OwnedAnalysisCore {
    result: AnalysisResults,
    graph: graph::ModuleGraph,
    modules: Vec<extract::ModuleInfo>,
    entry_point_count: usize,
    entry_points_ms: f64,
    resolve_ms: f64,
    graph_ms: f64,
    analyze_ms: f64,
}

/// Assemble the `PipelineProfile` for the full (freshly parsed) pipeline path.
fn full_pipeline_profile(
    prelude: &PreludeMetrics,
    core: &OwnedAnalysisCore,
    parse: &ParseMetrics,
) -> PipelineProfile {
    PipelineProfile {
        discover_ms: prelude.discover_ms,
        workspaces_ms: prelude.workspaces_ms,
        plugins_ms: prelude.plugins_ms,
        scripts_ms: prelude.scripts_ms,
        parse_ms: parse.parse_ms,
        cache_ms: parse.cache_ms,
        entry_points_ms: core.entry_points_ms,
        resolve_ms: core.resolve_ms,
        graph_ms: core.graph_ms,
        analyze_ms: core.analyze_ms,
        total_ms: prelude.total_ms,
        file_count: prelude.file_count,
        workspace_count: prelude.workspace_count,
        module_count: prelude.module_count,
        entry_point_count: core.entry_point_count,
        cache_hits: parse.cache_hits,
        cache_misses: parse.cache_misses,
        parse_cpu_ms: parse.parse_cpu_ms,
    }
}

#[derive(Clone, Copy)]
struct PipelineProfile {
    discover_ms: f64,
    workspaces_ms: f64,
    plugins_ms: f64,
    scripts_ms: f64,
    parse_ms: f64,
    cache_ms: f64,
    entry_points_ms: f64,
    resolve_ms: f64,
    graph_ms: f64,
    analyze_ms: f64,
    total_ms: f64,
    file_count: usize,
    workspace_count: usize,
    module_count: usize,
    entry_point_count: usize,
    cache_hits: usize,
    cache_misses: usize,
    parse_cpu_ms: f64,
}

struct AnalysisParseOutput {
    modules: Vec<extract::ModuleInfo>,
    metrics: ParseMetrics,
}

/// Parse/cache phase metrics carried into the full-pipeline `PipelineProfile`.
struct ParseMetrics {
    parse_ms: f64,
    cache_ms: f64,
    cache_hits: usize,
    cache_misses: usize,
    parse_cpu_ms: f64,
}

fn parse_analysis_modules(
    config: &ResolvedConfig,
    files: &[discover::DiscoveredFile],
    need_complexity: bool,
    start: Instant,
) -> AnalysisParseOutput {
    let cache_max_size_bytes = resolve_cache_max_size_bytes(config);
    let mut cache_store = if config.no_cache {
        None
    } else {
        cache::CacheStore::load(
            &config.cache_dir,
            config.cache_config_hash,
            cache_max_size_bytes,
        )
    };

    let parse_result = extract::parse_all_files(files, cache_store.as_ref(), need_complexity);
    let modules = parse_result.modules;
    let parse_ms = start.elapsed().as_secs_f64() * 1000.0;
    let cache_ms = update_parse_cache_if_enabled(
        config,
        &mut cache_store,
        &modules,
        files,
        cache_max_size_bytes,
    );

    AnalysisParseOutput {
        modules,
        metrics: ParseMetrics {
            parse_ms,
            cache_ms,
            cache_hits: parse_result.cache_hits,
            cache_misses: parse_result.cache_misses,
            parse_cpu_ms: parse_result.parse_cpu_ms,
        },
    }
}

fn retained_pipeline_timings(retain: bool, profile: &PipelineProfile) -> Option<PipelineTimings> {
    retain.then_some(PipelineTimings {
        discover_files_ms: profile.discover_ms,
        file_count: profile.file_count,
        workspaces_ms: profile.workspaces_ms,
        workspace_count: profile.workspace_count,
        plugins_ms: profile.plugins_ms,
        script_analysis_ms: profile.scripts_ms,
        parse_extract_ms: profile.parse_ms,
        parse_cpu_ms: profile.parse_cpu_ms,
        module_count: profile.module_count,
        cache_hits: profile.cache_hits,
        cache_misses: profile.cache_misses,
        cache_update_ms: profile.cache_ms,
        entry_points_ms: profile.entry_points_ms,
        entry_point_count: profile.entry_point_count,
        resolve_imports_ms: profile.resolve_ms,
        build_graph_ms: profile.graph_ms,
        analyze_ms: profile.analyze_ms,
        duplication_ms: None,
        total_ms: profile.total_ms,
    })
}

fn trace_reused_pipeline_profile(profile: &PipelineProfile) {
    tracing::debug!(
        "\n┌─ Pipeline Profile (reuse) ─────────────────────\n\
         │  discover files:   {:>8.1}ms  ({} files)\n\
         │  workspaces:       {:>8.1}ms\n\
         │  plugins:          {:>8.1}ms\n\
         │  script analysis:  {:>8.1}ms\n\
         │  parse/extract:    SKIPPED (reused {} modules)\n\
         │  entry points:     {:>8.1}ms  ({} entries)\n\
         │  resolve imports:  {:>8.1}ms\n\
         │  build graph:      {:>8.1}ms\n\
         │  analyze:          {:>8.1}ms\n\
         │  ────────────────────────────────────────────\n\
         │  TOTAL:            {:>8.1}ms\n\
         └─────────────────────────────────────────────────",
        profile.discover_ms,
        profile.file_count,
        profile.workspaces_ms,
        profile.plugins_ms,
        profile.scripts_ms,
        profile.module_count,
        profile.entry_points_ms,
        profile.entry_point_count,
        profile.resolve_ms,
        profile.graph_ms,
        profile.analyze_ms,
        profile.total_ms,
    );
}

fn update_parse_cache_if_enabled(
    config: &ResolvedConfig,
    cache_store: &mut Option<cache::CacheStore>,
    modules: &[extract::ModuleInfo],
    files: &[discover::DiscoveredFile],
    cache_max_size_bytes: usize,
) -> f64 {
    let t = Instant::now();
    if !config.no_cache {
        let store = cache_store.get_or_insert_with(cache::CacheStore::new);
        if update_cache(store, modules, files)
            && let Err(error) = store.save(
                &config.cache_dir,
                config.cache_config_hash,
                cache_max_size_bytes,
            )
        {
            tracing::warn!("Failed to save cache: {error}");
        }
    }
    t.elapsed().as_secs_f64() * 1000.0
}

fn resolve_analysis_imports(
    modules: &[extract::ModuleInfo],
    files: &[discover::DiscoveredFile],
    workspaces: &[fallow_config::WorkspaceInfo],
    plugin_result: &plugins::AggregatedPluginResult,
    config: &ResolvedConfig,
) -> Vec<resolve::ResolvedModule> {
    let mut resolved = resolve::resolve_all_imports(&resolve::ResolveAllImportsInput {
        modules,
        files,
        workspaces,
        active_plugins: &plugin_result.active_plugins,
        path_aliases: &plugin_result.path_aliases,
        auto_imports: &plugin_result.auto_imports,
        scss_include_paths: &plugin_result.scss_include_paths,
        static_dir_mappings: &plugin_result.static_dir_mappings,
        root: &config.root,
        extra_conditions: &config.resolve.conditions,
    });
    external_style_usage::augment_external_style_package_usage(
        &mut resolved,
        config,
        workspaces,
        plugin_result,
    );
    resolved
}

struct BuildAnalysisGraphInput<'a> {
    config: &'a ResolvedConfig,
    plugin_result: &'a plugins::AggregatedPluginResult,
    resolved: &'a [resolve::ResolvedModule],
    entry_points: &'a discover::CategorizedEntryPoints,
    files: &'a [discover::DiscoveredFile],
    modules: &'a [extract::ModuleInfo],
    workspaces: &'a [fallow_config::WorkspaceInfo],
}

/// Build the analysis graph, transparently backed by a persisted graph cache.
///
/// On a re-run whose file set + fingerprints + graph-affecting options are
/// byte-identical to the previous run, the previously-built `ModuleGraph` is
/// loaded from `.fallow/graph-cache.bin` and the (potentially large) build is
/// skipped. The persisted graph already includes the `credit_*` post-build
/// steps, so the cache hit returns it directly; a miss builds fresh, runs both
/// credit steps, and persists for next time. The cache is gated on
/// `config.no_cache` (same flag as the extraction cache) and is a strict
/// performance optimization: a cache hit produces an identical `ModuleGraph`,
/// so analysis results are unchanged (the mandatory correctness gate).
fn build_analysis_graph(input: &BuildAnalysisGraphInput<'_>) -> graph::ModuleGraph {
    let caching_enabled = !input.config.no_cache;

    let current_manifest = caching_enabled.then(|| build_graph_cache_manifest(input));

    if let Some(current) = current_manifest.as_ref()
        && let Some(store) = graph_cache::GraphCacheStore::load(&input.config.cache_dir)
        && store.manifest.matches_inputs(current)
    {
        // Cache hit: the persisted graph already includes the post-build credit
        // steps and a reconstructed `namespace_imported` bitset (rebuilt inside
        // `GraphCacheStore::load`), so it is returned verbatim.
        tracing::debug!("Graph cache hit: skipping graph build");
        return store.graph;
    }

    let mut graph = graph::ModuleGraph::build_with_reachability_roots(
        input.resolved,
        &input.entry_points.all,
        &input.entry_points.runtime,
        &input.entry_points.test,
        input.files,
    );
    credit_package_path_references(&mut graph, input.modules);
    credit_workspace_package_usage(&mut graph, input.resolved, input.workspaces);

    if let Some(manifest) = current_manifest {
        let store = graph_cache::GraphCacheStore {
            version: graph_cache::GRAPH_CACHE_VERSION,
            manifest,
            graph,
        };
        store.save(&input.config.cache_dir);
        // `save` borrows the store, so the freshly built graph is moved back out
        // and returned in-memory. The warm path loads-and-reconstructs an
        // identical graph from this same persisted blob (proven by the
        // cold-vs-warm correctness gate).
        return store.graph;
    }

    graph
}

/// Build the current `GraphCacheManifest` from the run's discovered files and
/// graph-affecting option hashes.
fn build_graph_cache_manifest(
    input: &BuildAnalysisGraphInput<'_>,
) -> graph_cache::GraphCacheManifest {
    let mode = graph_cache::GraphCacheMode::new(
        resolver_options_hash(input.config),
        entry_points_hash(input.entry_points),
        plugin_config_hash(input.plugin_result),
    );
    graph_cache::GraphCacheManifest::from_discovered_files(
        &input.config.root,
        input.files,
        mode,
        |path| {
            std::fs::metadata(path).map_or(
                fallow_types::source_fingerprint::SourceFingerprint::new(0, 0),
                |metadata| {
                    fallow_types::source_fingerprint::SourceFingerprint::from_metadata(&metadata)
                },
            )
        },
    )
}

/// Hash the resolver-affecting options: the project root, extraction config
/// hash (which already folds tsconfig / resolver-relevant config), and the
/// user-supplied resolve `conditions`.
///
/// `production` and `ignore_patterns` intentionally stay out of this hash:
/// they shape discovery, so changed file sets already miss through stable file
/// keys and source fingerprints in the manifest.
fn resolver_options_hash(config: &ResolvedConfig) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = rustc_hash::FxHasher::default();
    config.root.hash(&mut hasher);
    config.cache_config_hash.hash(&mut hasher);
    config.resolve.conditions.hash(&mut hasher);
    hasher.finish()
}

/// Hash the entry-point set (sorted paths per role) so any change in reachability
/// roots misses the cache.
fn entry_points_hash(entry_points: &discover::CategorizedEntryPoints) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = rustc_hash::FxHasher::default();
    for role in [&entry_points.all, &entry_points.runtime, &entry_points.test] {
        let mut paths: Vec<&std::path::Path> = role.iter().map(|ep| ep.path.as_path()).collect();
        paths.sort_unstable();
        paths.len().hash(&mut hasher);
        for path in paths {
            path.hash(&mut hasher);
        }
    }
    hasher.finish()
}

/// Hash the plugin-derived graph-affecting configuration.
fn plugin_config_hash(plugin_result: &plugins::AggregatedPluginResult) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = rustc_hash::FxHasher::default();

    let mut active: Vec<&str> = plugin_result
        .active_plugins
        .iter()
        .map(String::as_str)
        .collect();
    active.sort_unstable();
    active.len().hash(&mut hasher);
    for name in active {
        name.hash(&mut hasher);
    }

    let mut aliases: Vec<(&str, &str)> = plugin_result
        .path_aliases
        .iter()
        .map(|(prefix, replacement)| (prefix.as_str(), replacement.as_str()))
        .collect();
    aliases.sort_unstable();
    aliases.len().hash(&mut hasher);
    for (prefix, replacement) in aliases {
        prefix.hash(&mut hasher);
        replacement.hash(&mut hasher);
    }

    let mut auto_imports: Vec<(&str, &std::path::Path, fallow_config::AutoImportKind)> =
        plugin_result
            .auto_imports
            .iter()
            .map(|rule| (rule.name.as_str(), rule.source.as_path(), rule.kind))
            .collect();
    auto_imports.sort_unstable_by(|a, b| {
        a.0.cmp(b.0)
            .then_with(|| a.1.cmp(b.1))
            .then_with(|| auto_import_kind_rank(a.2).cmp(&auto_import_kind_rank(b.2)))
    });
    auto_imports.len().hash(&mut hasher);
    for (name, source, kind) in auto_imports {
        name.hash(&mut hasher);
        source.hash(&mut hasher);
        auto_import_kind_rank(kind).hash(&mut hasher);
    }

    let mut scss_include_paths: Vec<&std::path::Path> = plugin_result
        .scss_include_paths
        .iter()
        .map(std::path::PathBuf::as_path)
        .collect();
    scss_include_paths.sort_unstable();
    scss_include_paths.len().hash(&mut hasher);
    for path in scss_include_paths {
        path.hash(&mut hasher);
    }

    let mut static_dir_mappings: Vec<(&std::path::Path, &str)> = plugin_result
        .static_dir_mappings
        .iter()
        .map(|(from_dir, mount)| (from_dir.as_path(), mount.as_str()))
        .collect();
    static_dir_mappings.sort_unstable();
    static_dir_mappings.len().hash(&mut hasher);
    for (from_dir, mount) in static_dir_mappings {
        from_dir.hash(&mut hasher);
        mount.hash(&mut hasher);
    }

    hasher.finish()
}

fn auto_import_kind_rank(kind: fallow_config::AutoImportKind) -> u8 {
    match kind {
        fallow_config::AutoImportKind::Named => 0,
        fallow_config::AutoImportKind::Default => 1,
        fallow_config::AutoImportKind::DefaultComponent => 2,
    }
}

fn collect_file_hashes(
    modules: &[extract::ModuleInfo],
    files: &[discover::DiscoveredFile],
) -> rustc_hash::FxHashMap<std::path::PathBuf, u64> {
    modules
        .iter()
        .filter_map(|module| {
            files
                .get(module.file_id.0 as usize)
                .map(|file| (file.path.clone(), module.content_hash))
        })
        .collect()
}

fn trace_pipeline_profile(profile: &PipelineProfile) {
    let PipelineProfile {
        discover_ms,
        workspaces_ms,
        plugins_ms,
        scripts_ms,
        parse_ms,
        cache_ms,
        entry_points_ms,
        resolve_ms,
        graph_ms,
        analyze_ms,
        total_ms,
        file_count,
        module_count,
        entry_point_count,
        cache_hits,
        cache_misses,
        ..
    } = *profile;
    let cache_summary = if cache_hits > 0 {
        format!(" ({cache_hits} cached, {cache_misses} parsed)")
    } else {
        String::new()
    };

    tracing::debug!(
        "\n┌─ Pipeline Profile ─────────────────────────────\n\
         │  discover files:   {:>8.1}ms  ({} files)\n\
         │  workspaces:       {:>8.1}ms\n\
         │  plugins:          {:>8.1}ms\n\
         │  script analysis:  {:>8.1}ms\n\
         │  parse/extract:    {:>8.1}ms  ({} modules{})\n\
         │  cache update:     {:>8.1}ms\n\
         │  entry points:     {:>8.1}ms  ({} entries)\n\
         │  resolve imports:  {:>8.1}ms\n\
         │  build graph:      {:>8.1}ms\n\
         │  analyze:          {:>8.1}ms\n\
         │  ────────────────────────────────────────────\n\
         │  TOTAL:            {:>8.1}ms\n\
         └─────────────────────────────────────────────────",
        discover_ms,
        file_count,
        workspaces_ms,
        plugins_ms,
        scripts_ms,
        parse_ms,
        module_count,
        cache_summary,
        cache_ms,
        entry_points_ms,
        entry_point_count,
        resolve_ms,
        graph_ms,
        analyze_ms,
        total_ms,
    );
}

/// Analyze package.json scripts from root and all workspace packages.
///
/// Populates the plugin result with script-used packages and config file
/// entry patterns. Also scans CI config files for binary invocations.
fn load_root_package_json(config: &ResolvedConfig) -> Option<PackageJson> {
    PackageJson::load(&config.root.join("package.json")).ok()
}

fn load_workspace_packages(
    workspaces: &[fallow_config::WorkspaceInfo],
) -> Vec<LoadedWorkspacePackage<'_>> {
    workspaces
        .iter()
        .filter_map(|ws| {
            PackageJson::load(&ws.root.join("package.json"))
                .ok()
                .map(|pkg| (ws, pkg))
        })
        .collect()
}

fn analyze_all_scripts(
    config: &ResolvedConfig,
    workspaces: &[fallow_config::WorkspaceInfo],
    root_pkg: Option<&PackageJson>,
    workspace_pkgs: &[LoadedWorkspacePackage<'_>],
    plugin_result: &mut plugins::AggregatedPluginResult,
) {
    let all_dep_names = collect_all_dependency_names(root_pkg, workspace_pkgs);
    let all_dep_set: FxHashSet<String> = all_dep_names.iter().cloned().collect();
    let all_script_names = collect_all_script_names(root_pkg, workspace_pkgs);

    let nm_roots = collect_node_modules_roots(config, workspaces);
    let bin_map = scripts::build_bin_to_package_map(&nm_roots, &all_dep_names);

    analyze_root_scripts(config, root_pkg, &bin_map, &all_dep_set, plugin_result);
    analyze_workspace_scripts(
        config,
        workspace_pkgs,
        &bin_map,
        &all_dep_set,
        plugin_result,
    );
    analyze_ci_scripts(
        config,
        &bin_map,
        &all_dep_set,
        &all_script_names,
        plugin_result,
    );

    plugin_result
        .entry_point_roles
        .entry("scripts".to_string())
        .or_insert(EntryPointRole::Support);
}

/// Gather sorted, deduped dependency names across the root and workspace packages.
fn collect_all_dependency_names(
    root_pkg: Option<&PackageJson>,
    workspace_pkgs: &[LoadedWorkspacePackage<'_>],
) -> Vec<String> {
    let mut all_dep_names: Vec<String> = Vec::new();
    if let Some(pkg) = root_pkg {
        all_dep_names.extend(pkg.all_dependency_names());
    }
    for (_, ws_pkg) in workspace_pkgs {
        all_dep_names.extend(ws_pkg.all_dependency_names());
    }
    all_dep_names.sort_unstable();
    all_dep_names.dedup();
    all_dep_names
}

/// Gather the union of script names declared in the root and workspace packages.
fn collect_all_script_names(
    root_pkg: Option<&PackageJson>,
    workspace_pkgs: &[LoadedWorkspacePackage<'_>],
) -> FxHashSet<String> {
    let mut all_script_names: FxHashSet<String> = FxHashSet::default();
    if let Some(pkg) = root_pkg
        && let Some(ref pkg_scripts) = pkg.scripts
    {
        all_script_names.extend(pkg_scripts.keys().cloned());
    }
    for (_, ws_pkg) in workspace_pkgs {
        if let Some(ref ws_scripts) = ws_pkg.scripts {
            all_script_names.extend(ws_scripts.keys().cloned());
        }
    }
    all_script_names
}

/// Collect every directory (root and workspaces) that has a local `node_modules`.
fn collect_node_modules_roots<'a>(
    config: &'a ResolvedConfig,
    workspaces: &'a [fallow_config::WorkspaceInfo],
) -> Vec<&'a std::path::Path> {
    let mut nm_roots: Vec<&std::path::Path> = Vec::new();
    if config.root.join("node_modules").is_dir() {
        nm_roots.push(&config.root);
    }
    for ws in workspaces {
        if ws.root.join("node_modules").is_dir() {
            nm_roots.push(&ws.root);
        }
    }
    nm_roots
}

/// Analyze the root package.json scripts and fold the results into the plugin result.
fn analyze_root_scripts(
    config: &ResolvedConfig,
    root_pkg: Option<&PackageJson>,
    bin_map: &rustc_hash::FxHashMap<String, String>,
    all_dep_set: &FxHashSet<String>,
    plugin_result: &mut plugins::AggregatedPluginResult,
) {
    let Some(pkg) = root_pkg else {
        return;
    };
    let Some(ref pkg_scripts) = pkg.scripts else {
        return;
    };
    let scripts_to_analyze = if config.production {
        scripts::filter_production_scripts(pkg_scripts)
    } else {
        pkg_scripts.clone()
    };
    let script_names: FxHashSet<String> = pkg_scripts.keys().cloned().collect();
    let script_analysis = scripts::analyze_scripts_with_dependency_context(
        &scripts_to_analyze,
        &config.root,
        bin_map,
        all_dep_set,
        &script_names,
    );
    plugin_result.script_used_packages = script_analysis.used_packages;

    for config_file in &script_analysis.config_files {
        plugin_result
            .discovered_always_used
            .push((config_file.clone(), "scripts".to_string()));
    }
    for entry in &script_analysis.entry_files {
        if let Some(pat) = scripts::normalize_script_entry_pattern("", entry) {
            plugin_result
                .entry_patterns
                .push((plugins::PathRule::new(pat), "scripts".to_string()));
        }
    }
}

/// Analyze each workspace package's scripts in parallel and merge the results.
type WsScriptOut = (
    Vec<String>,
    Vec<(String, String)>,
    Vec<(plugins::PathRule, String)>,
);

fn analyze_workspace_scripts(
    config: &ResolvedConfig,
    workspace_pkgs: &[LoadedWorkspacePackage<'_>],
    bin_map: &rustc_hash::FxHashMap<String, String>,
    all_dep_set: &FxHashSet<String>,
    plugin_result: &mut plugins::AggregatedPluginResult,
) {
    let ws_results: Vec<WsScriptOut> = workspace_pkgs
        .par_iter()
        .map(|(ws, ws_pkg)| analyze_one_workspace_scripts(config, ws, ws_pkg, bin_map, all_dep_set))
        .collect();
    for (used_packages, discovered_always_used, entry_patterns) in ws_results {
        plugin_result.script_used_packages.extend(used_packages);
        plugin_result
            .discovered_always_used
            .extend(discovered_always_used);
        plugin_result.entry_patterns.extend(entry_patterns);
    }
}

/// Analyze a single workspace package's scripts, returning its used packages,
/// always-used config files, and entry patterns (all workspace-prefixed).
fn analyze_one_workspace_scripts(
    config: &ResolvedConfig,
    ws: &fallow_config::WorkspaceInfo,
    ws_pkg: &PackageJson,
    bin_map: &rustc_hash::FxHashMap<String, String>,
    all_dep_set: &FxHashSet<String>,
) -> WsScriptOut {
    let mut used_packages = Vec::new();
    let mut discovered_always_used: Vec<(String, String)> = Vec::new();
    let mut entry_patterns: Vec<(plugins::PathRule, String)> = Vec::new();
    let Some(ref ws_scripts) = ws_pkg.scripts else {
        return (used_packages, discovered_always_used, entry_patterns);
    };
    let scripts_to_analyze = if config.production {
        scripts::filter_production_scripts(ws_scripts)
    } else {
        ws_scripts.clone()
    };
    let script_names: FxHashSet<String> = ws_scripts.keys().cloned().collect();
    let ws_analysis = scripts::analyze_scripts_with_dependency_context(
        &scripts_to_analyze,
        &ws.root,
        bin_map,
        all_dep_set,
        &script_names,
    );
    used_packages.extend(ws_analysis.used_packages);

    let ws_prefix = ws
        .root
        .strip_prefix(&config.root)
        .unwrap_or(&ws.root)
        .to_string_lossy();
    for config_file in &ws_analysis.config_files {
        discovered_always_used.push((format!("{ws_prefix}/{config_file}"), "scripts".to_string()));
    }
    for entry in &ws_analysis.entry_files {
        if let Some(pat) = scripts::normalize_script_entry_pattern(&ws_prefix, entry) {
            entry_patterns.push((plugins::PathRule::new(pat), "scripts".to_string()));
        }
    }
    (used_packages, discovered_always_used, entry_patterns)
}

/// Analyze CI config files for binary invocations and merge the results.
fn analyze_ci_scripts(
    config: &ResolvedConfig,
    bin_map: &rustc_hash::FxHashMap<String, String>,
    all_dep_set: &FxHashSet<String>,
    all_script_names: &FxHashSet<String>,
    plugin_result: &mut plugins::AggregatedPluginResult,
) {
    let ci_analysis =
        scripts::ci::analyze_ci_files(&config.root, bin_map, all_dep_set, all_script_names);
    plugin_result
        .script_used_packages
        .extend(ci_analysis.used_packages);
    for entry in &ci_analysis.entry_files {
        if let Some(pat) = scripts::normalize_script_entry_pattern("", entry) {
            plugin_result
                .entry_patterns
                .push((plugins::PathRule::new(pat), "scripts".to_string()));
        }
    }
}

/// Discover all entry points from static patterns, workspaces, plugins, and infrastructure.
fn discover_all_entry_points(
    config: &ResolvedConfig,
    files: &[discover::DiscoveredFile],
    workspaces: &[fallow_config::WorkspaceInfo],
    root_pkg: Option<&PackageJson>,
    workspace_pkgs: &[LoadedWorkspacePackage<'_>],
    plugin_result: &plugins::AggregatedPluginResult,
) -> discover::CategorizedEntryPoints {
    let mut entry_points = discover::CategorizedEntryPoints::default();
    let root_discovery = discover::discover_entry_points_with_warnings_from_pkg(
        config,
        files,
        root_pkg,
        workspaces.is_empty(),
    );

    let workspace_pkg_by_root: rustc_hash::FxHashMap<std::path::PathBuf, &PackageJson> =
        workspace_pkgs
            .iter()
            .map(|(ws, pkg)| (ws.root.clone(), pkg))
            .collect();

    let workspace_discovery: Vec<discover::EntryPointDiscovery> = workspaces
        .par_iter()
        .map(|ws| {
            let pkg = workspace_pkg_by_root.get(&ws.root).copied();
            discover::discover_workspace_entry_points_with_warnings_from_pkg(&ws.root, files, pkg)
        })
        .collect();
    let mut skipped_entries = rustc_hash::FxHashMap::default();
    entry_points.extend_runtime(root_discovery.entries);
    for (path, count) in root_discovery.skipped_entries {
        *skipped_entries.entry(path).or_insert(0) += count;
    }
    let mut ws_entries = Vec::new();
    for workspace in workspace_discovery {
        ws_entries.extend(workspace.entries);
        for (path, count) in workspace.skipped_entries {
            *skipped_entries.entry(path).or_insert(0) += count;
        }
    }
    discover::warn_skipped_entry_summary(&skipped_entries);
    entry_points.extend_runtime(ws_entries);

    let plugin_entries = discover::discover_plugin_entry_point_sets(plugin_result, config, files);
    entry_points.extend(plugin_entries);

    let infra_entries = discover::discover_infrastructure_entry_points(&config.root);
    entry_points.extend_runtime(infra_entries);

    if !config.dynamically_loaded.is_empty() {
        let dynamic_entries = discover::discover_dynamically_loaded_entry_points(config, files);
        entry_points.extend_runtime(dynamic_entries);
    }

    entry_points.dedup()
}

/// Summarize entry points by source category for user-facing output.
fn summarize_entry_points(entry_points: &[discover::EntryPoint]) -> results::EntryPointSummary {
    let mut counts: rustc_hash::FxHashMap<String, usize> = rustc_hash::FxHashMap::default();
    for ep in entry_points {
        let category = match &ep.source {
            discover::EntryPointSource::PackageJsonMain
            | discover::EntryPointSource::PackageJsonModule
            | discover::EntryPointSource::PackageJsonExports
            | discover::EntryPointSource::PackageJsonBin
            | discover::EntryPointSource::PackageJsonScript => "package.json",
            discover::EntryPointSource::Plugin { .. } => "plugin",
            discover::EntryPointSource::TestFile => "test file",
            discover::EntryPointSource::DefaultIndex => "default index",
            discover::EntryPointSource::ManualEntry => "manual entry",
            discover::EntryPointSource::InfrastructureConfig => "config",
            discover::EntryPointSource::DynamicallyLoaded => "dynamically loaded",
        };
        *counts.entry(category.to_string()).or_insert(0) += 1;
    }
    let mut by_source: Vec<(String, usize)> = counts.into_iter().collect();
    by_source.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    results::EntryPointSummary {
        total: entry_points.len(),
        by_source,
    }
}

fn append_package_file_asset_patterns(
    result: &mut plugins::AggregatedPluginResult,
    prefix: &str,
    pkg: &PackageJson,
) {
    let prefix = prefix.trim_matches('/');
    for pattern in package_assets::scaffold_template_asset_patterns(pkg) {
        let pattern = if prefix.is_empty() {
            pattern
        } else {
            format!("{prefix}/{pattern}")
        };
        result
            .discovered_always_used
            .push((pattern, package_assets::PACKAGE_FILES_SOURCE.to_string()));
    }
}

fn append_workspace_package_file_asset_patterns(
    result: &mut plugins::AggregatedPluginResult,
    config: &ResolvedConfig,
    workspace_pkgs: &[LoadedWorkspacePackage<'_>],
) {
    for (ws, ws_pkg) in workspace_pkgs {
        let ws_prefix = ws
            .root
            .strip_prefix(&config.root)
            .unwrap_or(&ws.root)
            .to_string_lossy()
            .replace('\\', "/");
        append_package_file_asset_patterns(result, &ws_prefix, ws_pkg);
    }
}

/// Run plugins for root project and all workspace packages.
fn run_plugins(
    config: &ResolvedConfig,
    files: &[discover::DiscoveredFile],
    workspaces: &[fallow_config::WorkspaceInfo],
    root_pkg: Option<&PackageJson>,
    workspace_pkgs: &[LoadedWorkspacePackage<'_>],
    config_candidates: &[std::path::PathBuf],
) -> Result<plugins::AggregatedPluginResult, FallowError> {
    let registry = plugins::PluginRegistry::new(config.external_plugins.clone());
    let file_paths: Vec<std::path::PathBuf> = files.iter().map(|f| f.path.clone()).collect();

    // The non-production config-discovery fast path: resolve plugin config
    // patterns against the files the discovery walk already collected (source
    // files unioned with non-source config candidates) instead of re-walking the
    // filesystem. Production keeps the filesystem path (no candidates captured).
    let candidate_index = (!config.production).then(|| {
        plugins::registry::ConfigCandidateIndex::build(
            file_paths
                .iter()
                .map(std::path::PathBuf::as_path)
                .chain(config_candidates.iter().map(std::path::PathBuf::as_path)),
        )
    });

    let mut result = run_root_plugins(
        &registry,
        config,
        root_pkg,
        &file_paths,
        candidate_index.as_ref(),
    )?;

    if workspaces.is_empty() {
        gate_auto_import_entry_patterns(&mut result, config, workspaces);
        return Ok(result);
    }

    append_workspace_package_file_asset_patterns(&mut result, config, workspace_pkgs);

    let ws_results = run_workspace_plugins(
        &registry,
        config,
        workspace_pkgs,
        &file_paths,
        &result.active_plugins,
        candidate_index.as_ref(),
    );
    merge_workspace_plugin_results(&mut result, ws_results)?;

    gate_auto_import_entry_patterns(&mut result, config, workspaces);

    Ok(result)
}

type WorkspacePluginResult = Result<
    (plugins::AggregatedPluginResult, String),
    Vec<plugins::registry::PluginRegexValidationError>,
>;

/// Run plugins for the root project and apply its package-file asset patterns.
fn run_root_plugins(
    registry: &plugins::PluginRegistry,
    config: &ResolvedConfig,
    root_pkg: Option<&PackageJson>,
    file_paths: &[std::path::PathBuf],
    candidate_index: Option<&plugins::registry::ConfigCandidateIndex>,
) -> Result<plugins::AggregatedPluginResult, FallowError> {
    let root_config_search_roots = collect_config_search_roots(&config.root, file_paths);
    let root_config_search_root_refs: Vec<&Path> = root_config_search_roots
        .iter()
        .map(std::path::PathBuf::as_path)
        .collect();

    let mut result = if let Some(pkg) = root_pkg {
        registry
            .try_run_with_search_roots(
                pkg,
                &config.root,
                file_paths,
                &root_config_search_root_refs,
                config.production,
                candidate_index,
            )
            .map_err(|errors| {
                FallowError::config(plugins::registry::format_plugin_regex_errors(&errors))
            })?
    } else {
        plugins::AggregatedPluginResult::default()
    };
    if let Some(pkg) = root_pkg {
        append_package_file_asset_patterns(&mut result, "", pkg);
    }
    Ok(result)
}

/// Run plugins for every workspace package in parallel, returning per-workspace
/// results (or regex errors) for the caller to merge.
fn run_workspace_plugins(
    registry: &plugins::PluginRegistry,
    config: &ResolvedConfig,
    workspace_pkgs: &[LoadedWorkspacePackage<'_>],
    file_paths: &[std::path::PathBuf],
    root_active_plugins: &[String],
    candidate_index: Option<&plugins::registry::ConfigCandidateIndex>,
) -> Vec<WorkspacePluginResult> {
    let root_active_plugins: rustc_hash::FxHashSet<&str> =
        root_active_plugins.iter().map(String::as_str).collect();

    let precompiled_matchers = registry.precompile_config_matchers();
    let workspace_relative_files = bucket_files_by_workspace(workspace_pkgs, file_paths);

    workspace_pkgs
        .par_iter()
        .zip(workspace_relative_files.par_iter())
        .filter_map(|((ws, ws_pkg), relative_files)| {
            let ws_result =
                match registry.try_run_workspace_fast(&plugins::registry::WorkspacePluginRunInput {
                    pkg: ws_pkg,
                    root: &ws.root,
                    project_root: &config.root,
                    precompiled_config_matchers: &precompiled_matchers,
                    relative_files,
                    skip_config_plugins: &root_active_plugins,
                    production_mode: config.production,
                    candidate_index,
                }) {
                    Ok(result) => result,
                    Err(errors) => return Some(Err(errors)),
                };
            if ws_result.active_plugins.is_empty() {
                return None;
            }
            let ws_prefix = ws
                .root
                .strip_prefix(&config.root)
                .unwrap_or(&ws.root)
                .to_string_lossy()
                .into_owned();
            Some(Ok((ws_result, ws_prefix)))
        })
        .collect::<Vec<_>>()
}

/// Merge per-workspace plugin results into the root result, surfacing any
/// accumulated regex errors as a single config error.
fn merge_workspace_plugin_results(
    result: &mut plugins::AggregatedPluginResult,
    ws_results: Vec<WorkspacePluginResult>,
) -> Result<(), FallowError> {
    let mut regex_errors = Vec::new();
    for ws_result in ws_results {
        match ws_result {
            Ok((mut ws_result, ws_prefix)) => {
                ws_result.apply_workspace_prefix(&ws_prefix);
                ws_result.config_patterns.clear();
                ws_result.script_used_packages.clear();
                result.merge_into(ws_result);
            }
            Err(mut errors) => regex_errors.append(&mut errors),
        }
    }
    if !regex_errors.is_empty() {
        return Err(FallowError::config(
            plugins::registry::format_plugin_regex_errors(&regex_errors),
        ));
    }
    Ok(())
}

/// When `autoImports` is enabled, drop the modeled Nuxt convention entry
/// patterns so genuinely-unreferenced convention files are reported as
/// `unused-file`. Component and script fallbacks have separate conservative
/// config guards because custom `components:` and `imports:` settings affect
/// different convention surfaces.
fn gate_auto_import_entry_patterns(
    result: &mut plugins::AggregatedPluginResult,
    config: &ResolvedConfig,
    workspaces: &[fallow_config::WorkspaceInfo],
) {
    if !config.auto_imports {
        return;
    }
    if !result.active_plugins.iter().any(|name| name == "nuxt") {
        return;
    }
    let components_custom = plugins::nuxt::config_declares_components(&config.root)
        || workspaces
            .iter()
            .any(|ws| plugins::nuxt::config_declares_components(&ws.root));
    let imports_custom = plugins::nuxt::config_declares_imports(&config.root)
        || workspaces
            .iter()
            .any(|ws| plugins::nuxt::config_declares_imports(&ws.root));
    result.entry_patterns.retain(|(rule, plugin)| {
        if plugin != "nuxt" {
            return true;
        }
        if !components_custom && plugins::nuxt::is_component_entry_pattern(&rule.pattern) {
            return false;
        }
        if !imports_custom && plugins::nuxt::is_script_auto_import_entry_pattern(&rule.pattern) {
            return false;
        }
        true
    });
}

fn bucket_files_by_workspace(
    workspace_pkgs: &[LoadedWorkspacePackage<'_>],
    file_paths: &[std::path::PathBuf],
) -> Vec<Vec<(std::path::PathBuf, String)>> {
    use rayon::prelude::*;

    // Assign each file to its first matching workspace in parallel. On large
    // monorepos this is O(files x workspaces) prefix scans plus a path clone and
    // a relative-path allocation per file; doing it per file on one thread was a
    // measurable slice of the plugins stage. The assignment is independent per
    // file, so the only ordering contract to preserve is first-match-by-workspace-
    // declaration-order (the original `break`) and per-bucket file order, both of
    // which hold because the parallel map keeps file indexing and the bucket fill
    // below walks the assignments in original file order.
    let assignments: Vec<Option<(usize, std::path::PathBuf, String)>> = file_paths
        .par_iter()
        .map(|file_path| {
            workspace_pkgs
                .iter()
                .enumerate()
                .find_map(|(idx, (ws, _))| {
                    file_path.strip_prefix(&ws.root).ok().map(|relative| {
                        (
                            idx,
                            file_path.clone(),
                            relative.to_string_lossy().into_owned(),
                        )
                    })
                })
        })
        .collect();

    let mut buckets = vec![Vec::new(); workspace_pkgs.len()];
    for (idx, file_path, relative) in assignments.into_iter().flatten() {
        buckets[idx].push((file_path, relative));
    }

    buckets
}

fn collect_config_search_roots(
    root: &Path,
    file_paths: &[std::path::PathBuf],
) -> Vec<std::path::PathBuf> {
    let mut roots: rustc_hash::FxHashSet<std::path::PathBuf> = rustc_hash::FxHashSet::default();
    roots.insert(root.to_path_buf());

    for file_path in file_paths {
        let mut current = file_path.parent();
        while let Some(dir) = current {
            if !dir.starts_with(root) {
                break;
            }
            roots.insert(dir.to_path_buf());
            if dir == root {
                break;
            }
            current = dir.parent();
        }
    }

    let mut roots_vec: Vec<_> = roots.into_iter().collect();
    roots_vec.sort();
    roots_vec
}

/// Run analysis on a project directory (with export usages for LSP Code Lens).
///
/// # Errors
///
/// Returns an error if config loading, file discovery, parsing, or analysis fails.
#[deprecated(
    since = "2.76.0",
    note = "fallow_core is internal; use fallow_api::run_dead_code for typed output; serialize with fallow_api::serialize_dead_code_programmatic_json for JSON output. Build a `DeadCodeOptions { analysis: AnalysisOptions { root, ..default() }, ..default() }`. See docs/fallow-core-migration.md and ADR-008."
)]
pub fn analyze_project(root: &Path) -> Result<AnalysisResults, FallowError> {
    let config = default_config(root);
    #[expect(
        deprecated,
        reason = "ADR-008: thin wrapper, internal call into the same deprecated surface"
    )]
    analyze_with_usages(&config)
}

/// Resolve the analysis config for a project, mirroring the CLI's `--config`
/// behavior when `config_path` is provided.
///
/// # Errors
///
/// Returns an error when an explicit config cannot be loaded or automatic
/// config discovery finds an invalid config.
pub fn config_for_project(
    root: &Path,
    config_path: Option<&Path>,
) -> Result<(ResolvedConfig, Option<std::path::PathBuf>), FallowError> {
    let user_config = if let Some(path) = config_path {
        Some((
            fallow_config::FallowConfig::load(path)
                .map_err(|e| FallowError::config(format!("{e:#}")))?,
            path.to_path_buf(),
        ))
    } else {
        fallow_config::FallowConfig::find_and_load(root).map_err(FallowError::config)?
    };

    let config = match user_config {
        Some((config, path)) => resolve_user_config(config, path, root)?,
        None => (
            fallow_config::FallowConfig::default().resolve(
                root.to_path_buf(),
                fallow_config::OutputFormat::Human,
                num_cpus(),
                false,
                true,
                None,
            ),
            None,
        ),
    };

    Ok(config)
}

/// Flatten the dead-code production flag, validate boundaries and rule packs,
/// then resolve a user-supplied config for LSP/programmatic callers.
fn resolve_user_config(
    mut config: fallow_config::FallowConfig,
    path: std::path::PathBuf,
    root: &Path,
) -> Result<(ResolvedConfig, Option<std::path::PathBuf>), FallowError> {
    let dead_code_production = config
        .production
        .for_analysis(fallow_config::ProductionAnalysis::DeadCode);
    config.production = dead_code_production.into();
    config
        .validate_resolved_boundaries(root)
        .map_err(|errors| {
            let joined = errors
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("\n  - ");
            FallowError::config(format!("invalid boundary configuration:\n  - {joined}"))
        })?;
    fallow_config::load_rule_packs(root, &config.rule_packs).map_err(|errors| {
        let joined = errors
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n  - ");
        FallowError::config(format!("invalid rule pack:\n  - {joined}"))
    })?;
    Ok((
        config.resolve(
            root.to_path_buf(),
            fallow_config::OutputFormat::Human,
            num_cpus(),
            false,
            true, // quiet: LSP/programmatic callers don't need progress bars
            None, // LSP/programmatic embedders use the default cache cap
        ),
        Some(path),
    ))
}

/// Create a default config for a project root.
///
/// `analyze_project` is the dead-code entry point used by the LSP and other
/// programmatic embedders. When the loaded config uses the per-analysis
/// production form (`production: { deadCode: true, ... }`), the production
/// flag must be flattened to the dead-code analysis here. Otherwise
/// `ResolvedConfig::resolve` calls `.global()` which returns false for the
/// per-analysis variant and the production-mode rule overrides
/// (`unused_dev_dependencies: off`, etc.) plus `resolved.production = true`
/// are silently dropped.
pub(crate) fn default_config(root: &Path) -> ResolvedConfig {
    config_for_project(root, None).map_or_else(
        |_| {
            fallow_config::FallowConfig::default().resolve(
                root.to_path_buf(),
                fallow_config::OutputFormat::Human,
                num_cpus(),
                false,
                true,
                None,
            )
        },
        |(config, _)| config,
    )
}

fn num_cpus() -> usize {
    std::thread::available_parallelism().map_or(4, std::num::NonZeroUsize::get)
}

#[cfg(test)]
mod tests {
    use super::{
        AnalysisSession, bucket_files_by_workspace, collect_config_search_roots, default_config,
        format_undeclared_workspace_warning, plugin_config_hash, resolver_options_hash,
        warn_undeclared_workspaces,
    };
    use std::path::{Path, PathBuf};

    use fallow_config::{
        AutoImportKind, AutoImportRule, WorkspaceDiagnostic, WorkspaceDiagnosticKind,
    };

    fn plugin_result() -> crate::plugins::AggregatedPluginResult {
        let mut result = crate::plugins::AggregatedPluginResult::default();
        result.active_plugins.push("nuxt".to_string());
        result
            .path_aliases
            .push(("@/".to_string(), "src/".to_string()));
        result
    }

    #[test]
    fn graph_cache_resolver_hash_includes_project_root() {
        let dir_a = tempfile::tempdir().expect("create temp dir a");
        let dir_b = tempfile::tempdir().expect("create temp dir b");
        let config_a = session_config(dir_a.path());
        let config_b = session_config(dir_b.path());

        assert_ne!(
            resolver_options_hash(&config_a),
            resolver_options_hash(&config_b),
            "shared cache dirs must not reuse graphs across project roots"
        );
    }

    #[test]
    fn graph_cache_plugin_hash_includes_auto_imports() {
        let mut without_auto_import = plugin_result();
        let mut with_auto_import = plugin_result();
        with_auto_import.auto_imports.push(AutoImportRule {
            name: "useCounter".to_string(),
            source: PathBuf::from("/project/composables/useCounter.ts"),
            kind: AutoImportKind::Named,
        });

        assert_ne!(
            plugin_config_hash(&without_auto_import),
            plugin_config_hash(&with_auto_import),
            "auto-import edge changes must invalidate the graph cache"
        );

        without_auto_import.auto_imports.push(AutoImportRule {
            name: "useCounter".to_string(),
            source: PathBuf::from("/project/composables/useCounter.ts"),
            kind: AutoImportKind::Default,
        });
        assert_ne!(
            plugin_config_hash(&without_auto_import),
            plugin_config_hash(&with_auto_import),
            "auto-import kind changes must invalidate the graph cache"
        );
    }

    #[test]
    fn graph_cache_plugin_hash_includes_style_and_static_mappings() {
        let base = plugin_result();
        let mut with_scss = base.clone();
        with_scss
            .scss_include_paths
            .push(PathBuf::from("/project/styles"));
        assert_ne!(
            plugin_config_hash(&base),
            plugin_config_hash(&with_scss),
            "SCSS include path changes must invalidate the graph cache"
        );

        let mut with_static_dir = base.clone();
        with_static_dir
            .static_dir_mappings
            .push((PathBuf::from("/project/public"), "/".to_string()));
        assert_ne!(
            plugin_config_hash(&base),
            plugin_config_hash(&with_static_dir),
            "static directory mapping changes must invalidate the graph cache"
        );
    }

    fn diag(root: &Path, relative: &str) -> WorkspaceDiagnostic {
        WorkspaceDiagnostic::new(
            root,
            root.join(relative),
            WorkspaceDiagnosticKind::UndeclaredWorkspace,
        )
    }

    fn session_config(root: &Path) -> fallow_config::ResolvedConfig {
        let mut config = default_config(root);
        config.no_cache = true;
        config.quiet = true;
        config
    }

    fn write_session_fixture(root: &Path) {
        let src = root.join("src");
        std::fs::create_dir_all(&src).expect("create src");
        std::fs::write(
            root.join("package.json"),
            r#"{"name":"session-fixture","type":"module"}"#,
        )
        .expect("write package json");
        std::fs::write(
            src.join("index.ts"),
            "import { used } from './used';\nconsole.log(used);\n",
        )
        .expect("write index");
        std::fs::write(src.join("used.ts"), "export const used = 1;\n").expect("write used");
    }

    #[test]
    fn analysis_session_discovers_project_files() {
        let dir = tempfile::tempdir().expect("create temp dir");
        write_session_fixture(dir.path());
        let config = session_config(dir.path());

        let session = AnalysisSession::new(&config);

        assert!(
            session
                .files()
                .iter()
                .any(|file| file.path.ends_with("src/index.ts")),
            "session should own discovered project files"
        );
        assert_eq!(session.workspaces().len(), 0);
    }

    #[test]
    fn analysis_session_parses_owned_modules() {
        let dir = tempfile::tempdir().expect("create temp dir");
        write_session_fixture(dir.path());
        let config = session_config(dir.path());

        let session = AnalysisSession::new(&config);
        let parsed = session.parse_modules(false);

        assert!(
            parsed
                .modules
                .iter()
                .any(|module| session.files()[module.file_id.0 as usize]
                    .path
                    .ends_with("src/index.ts")),
            "session parsing should return modules keyed to session files"
        );
    }

    #[test]
    fn analysis_session_reuses_parse_result_with_matching_results() {
        let dir = tempfile::tempdir().expect("create temp dir");
        write_session_fixture(dir.path());
        let config = session_config(dir.path());

        let fresh = AnalysisSession::new(&config)
            .run_full(true, false, false, true)
            .expect("fresh analysis succeeds");
        let modules = fresh.modules.as_ref().expect("fresh modules retained");
        let reused = AnalysisSession::new(&config)
            .run_with_parse_result(modules)
            .expect("reused analysis succeeds");

        assert!(reused.graph.is_some());
        assert!(reused.timings.is_some());
        assert_eq!(reused.file_hashes, fresh.file_hashes);
        assert_eq!(
            serde_json::to_value(&reused.results).expect("serialize reused results"),
            serde_json::to_value(&fresh.results).expect("serialize fresh results")
        );
    }

    #[test]
    fn undeclared_workspace_warning_is_singular_for_one_path() {
        let root = Path::new("/repo");
        let warning = format_undeclared_workspace_warning(root, &[diag(root, "packages/api")])
            .expect("warning should be rendered");

        assert_eq!(
            warning,
            "1 directory with package.json is not declared as a workspace: packages/api. Add that path to package.json workspaces or pnpm-workspace.yaml if it should be analyzed as a workspace."
        );
    }

    #[test]
    fn undeclared_workspace_warning_summarizes_many_paths() {
        let root = PathBuf::from("/repo");
        let diagnostics = [
            "examples/a",
            "examples/b",
            "examples/c",
            "examples/d",
            "examples/e",
            "examples/f",
        ]
        .into_iter()
        .map(|path| diag(&root, path))
        .collect::<Vec<_>>();

        let warning = format_undeclared_workspace_warning(&root, &diagnostics)
            .expect("warning should be rendered");

        assert_eq!(
            warning,
            "6 directories with package.json are not declared as workspaces: examples/a, examples/b, examples/c, examples/d, examples/e (and 1 more). Add those paths to package.json workspaces or pnpm-workspace.yaml if they should be analyzed as workspaces."
        );
    }

    #[test]
    fn collect_config_search_roots_includes_file_ancestors_once() {
        let root = PathBuf::from("/repo");
        let search_roots = collect_config_search_roots(
            &root,
            &[
                root.join("apps/query/src/main.ts"),
                root.join("packages/shared/lib/index.ts"),
            ],
        );

        assert_eq!(
            search_roots,
            vec![
                root.clone(),
                root.join("apps"),
                root.join("apps/query"),
                root.join("apps/query/src"),
                root.join("packages"),
                root.join("packages/shared"),
                root.join("packages/shared/lib"),
            ]
        );
    }

    #[test]
    fn bucket_files_by_workspace_uses_workspace_relative_paths() {
        let root = PathBuf::from("/repo");
        let ui = fallow_config::WorkspaceInfo {
            root: root.join("apps/ui"),
            name: "ui".to_string(),
            is_internal_dependency: false,
        };
        let api = fallow_config::WorkspaceInfo {
            root: root.join("apps/api"),
            name: "api".to_string(),
            is_internal_dependency: false,
        };
        let workspace_pkgs = vec![
            (
                &ui,
                fallow_config::PackageJson {
                    name: Some("ui".to_string()),
                    ..Default::default()
                },
            ),
            (
                &api,
                fallow_config::PackageJson {
                    name: Some("api".to_string()),
                    ..Default::default()
                },
            ),
        ];
        let files = vec![
            root.join("apps/ui/vite.config.ts"),
            root.join("apps/ui/src/main.ts"),
            root.join("apps/api/src/server.ts"),
            root.join("tools/build.ts"),
        ];

        let buckets = bucket_files_by_workspace(&workspace_pkgs, &files);

        assert_eq!(
            buckets[0],
            vec![
                (
                    root.join("apps/ui/vite.config.ts"),
                    "vite.config.ts".to_string()
                ),
                (root.join("apps/ui/src/main.ts"), "src/main.ts".to_string()),
            ]
        );
        assert_eq!(
            buckets[1],
            vec![(
                root.join("apps/api/src/server.ts"),
                "src/server.ts".to_string()
            )]
        );
    }

    #[test]
    fn warn_undeclared_workspaces_suppresses_paths_already_flagged_as_malformed() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let pkg_good = dir.path().join("packages").join("good");
        let pkg_bad = dir.path().join("packages").join("bad");
        std::fs::create_dir_all(&pkg_good).unwrap();
        std::fs::create_dir_all(&pkg_bad).unwrap();
        std::fs::write(
            dir.path().join("package.json"),
            r#"{"workspaces": ["packages/*"]}"#,
        )
        .unwrap();
        std::fs::write(pkg_good.join("package.json"), r#"{"name": "good"}"#).unwrap();
        std::fs::write(pkg_bad.join("package.json"), r"{,").unwrap();

        let (workspaces, diagnostics) = fallow_config::discover_workspaces_with_diagnostics(
            dir.path(),
            &globset::GlobSet::empty(),
        )
        .expect("root package.json is valid");
        assert_eq!(workspaces.len(), 1, "only the valid workspace discovers");
        fallow_config::stash_workspace_diagnostics(dir.path(), diagnostics);

        warn_undeclared_workspaces(dir.path(), &workspaces, &globset::GlobSet::empty(), false);

        let diagnostics = fallow_config::workspace_diagnostics_for(dir.path());
        let mut malformed = 0;
        let mut undeclared_for_bad = 0;
        for diag in &diagnostics {
            if matches!(
                diag.kind,
                WorkspaceDiagnosticKind::MalformedPackageJson { .. }
            ) && diag.path.ends_with("bad")
            {
                malformed += 1;
            }
            if matches!(diag.kind, WorkspaceDiagnosticKind::UndeclaredWorkspace)
                && diag.path.ends_with("bad")
            {
                undeclared_for_bad += 1;
            }
        }
        assert_eq!(
            malformed, 1,
            "expected one MalformedPackageJson for packages/bad: {diagnostics:?}"
        );
        assert_eq!(
            undeclared_for_bad, 0,
            "warn_undeclared_workspaces must NOT re-flag a path that already \
             carries MalformedPackageJson; got duplicates: {diagnostics:?}"
        );
    }
}
