//! fallow-core is the internal implementation crate behind the `fallow`
//! analyzer. External embedders should consume the curated programmatic
//! surface at `fallow_api` (e.g. `run_dead_code`,
//! `run_boundary_violations`, `run_duplication`, `run_health`). The typed
//! `run_*` functions are the primary embedder contract; serialize typed output
//! with the matching `serialize_*_programmatic_json` helper only at a protocol
//! boundary. See `docs/fallow-core-migration.md`
//! for the function-by-function migration map. Items in this crate may change
//! in any release, including patch releases. Publishing remains transitional
//! while `fallow-engine` still depends on core internals.

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
pub mod discover;
pub(crate) mod errors;
mod external_style_usage;
pub mod extract;
pub mod git_env;
mod package_assets;
pub mod plugins;
pub(crate) mod progress;
pub mod results;
pub(crate) mod scripts;
/// Public hook for the fuzz harness (fuzz/fuzz_targets/fuzz_scripts.rs) only;
/// not a supported API. The module itself stays crate-private.
#[doc(hidden)]
pub use scripts::parse_script;
pub mod suppress;

pub use fallow_graph::cache as graph_cache;
pub use fallow_graph::graph;
pub use fallow_graph::project;
pub use fallow_graph::resolve;

use std::path::{Path, PathBuf};
use std::time::Instant;

use errors::FallowError;
use fallow_config::{
    EntryPointRole, PackageJson, ResolvedConfig, discover_workspaces_with_diagnostics,
    find_undeclared_workspaces_with_ignores,
};
use fallow_types::cache_rejection::CacheRejection;
use fallow_types::trace::{EntryPointSpans, PipelineTimings};
use rayon::prelude::*;
use results::AnalysisResults;
use rustc_hash::FxHashSet;

const UNDECLARED_WORKSPACE_WARNING_PREVIEW: usize = 5;
type LoadedWorkspacePackage = (fallow_config::WorkspaceInfo, PackageJson);

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
    workspace_names: &FxHashSet<&'a str>,
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
            if matches!(
                import.target,
                resolve::ResolveResult::InternalModule(_)
                    | resolve::ResolveResult::CommonJsInternalModule(_)
            ) && let Some(package_name) =
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
#[doc(hidden)]
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

/// Parse/cache phase metrics supplied by callers that own parsing before
/// handing modules back to the core detector backend.
#[derive(Debug, Clone, Copy)]
#[doc(hidden)]
pub struct AnalysisParseMetrics {
    parse_ms: f64,
    cache_ms: f64,
    cache_hits: usize,
    cache_misses: usize,
    parse_cpu_ms: f64,
    cache_rejection: Option<CacheRejection>,
}

/// Update cache: write freshly parsed modules, refresh stale metadata entries,
/// and upgrade entries a complexity-blind run wrote earlier.
///
/// An entry whose content hash still matches is rewritten only when its
/// metadata fingerprint moved or when this run can add complexity the entry
/// lacks. A run with `need_complexity == false` never rewrites the complexity
/// an earlier `health` run stored: `module.complexity` is empty on such a run
/// by design, so copying it over would silently strip the entry and make every
/// later `health` run reparse the file.
fn update_cache(
    store: &mut cache::CacheStore,
    modules: &[extract::ModuleInfo],
    files: &[discover::DiscoveredFile],
    need_complexity: bool,
) -> bool {
    let mut dirty = false;
    for module in modules {
        if let Some(file) = files.get(module.file_id.0 as usize) {
            let fingerprint = file_fingerprint(&file.path);
            if let Some(cached) = store.get_by_path_only(&file.path)
                && cached.content_hash == module.content_hash
            {
                let stale_metadata = cached.source_fingerprint() != fingerprint;
                let adds_complexity = need_complexity && !cached.complexity_extracted;
                if stale_metadata || adds_complexity {
                    let preserved_last_access = cached.last_access_secs;
                    let preserved_complexity = (!need_complexity && cached.complexity_extracted)
                        .then(|| cached.complexity.clone());
                    let mut refreshed =
                        cache::module_to_cached(module, fingerprint, need_complexity);
                    refreshed.last_access_secs = preserved_last_access;
                    if let Some(complexity) = preserved_complexity {
                        refreshed.complexity = complexity;
                        refreshed.complexity_extracted = true;
                    }
                    store.insert(&file.path, refreshed);
                    dirty = true;
                }
                continue;
            }
            store.insert(
                &file.path,
                cache::module_to_cached(module, fingerprint, need_complexity),
            );
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
fn resolve_cache_max_size_bytes(config: &ResolvedConfig) -> usize {
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
#[doc(hidden)]
#[deprecated(
    since = "2.76.0",
    note = "fallow_core is internal; use fallow_api::run_dead_code for typed output; serialize with fallow_api::serialize_dead_code_programmatic_json for JSON output. See docs/fallow-core-migration.md."
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
#[doc(hidden)]
#[deprecated(
    since = "2.76.0",
    note = "fallow_core is internal; use fallow_api::run_dead_code for public typed output. NOTE: export-usage collection is not exposed in the programmatic surface today. See docs/fallow-core-migration.md."
)]
pub fn analyze_with_usages(config: &ResolvedConfig) -> Result<AnalysisResults, FallowError> {
    let output = analyze_full(config, false, true, false, false)?;
    Ok(output.results)
}

/// Run the full analysis pipeline with optional performance timings and graph retention.
///
/// # Errors
///
/// Returns an error if file discovery, parsing, or analysis fails.
#[doc(hidden)]
#[deprecated(
    since = "2.76.0",
    note = "fallow_core is internal; use fallow_api::run_dead_code for public typed output. NOTE: trace timings are not exposed in the programmatic surface today; use `fallow dead-code --performance` for CLI-side timings. See docs/fallow-core-migration.md."
)]
pub fn analyze_with_trace(config: &ResolvedConfig) -> Result<AnalysisOutput, FallowError> {
    analyze_full(config, true, false, false, false)
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
#[doc(hidden)]
#[deprecated(
    since = "2.76.0",
    note = "fallow_core is internal; use fallow_api::run_dead_code for public typed output. NOTE: combined-mode module retention is not exposed in the programmatic surface today. See docs/fallow-core-migration.md."
)]
pub fn analyze_retaining_modules(
    config: &ResolvedConfig,
    need_complexity: bool,
    retain_graph: bool,
) -> Result<AnalysisOutput, FallowError> {
    analyze_full(config, retain_graph, false, need_complexity, true)
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

fn discover_analysis_workspaces(
    config: &ResolvedConfig,
) -> Result<(Vec<fallow_config::WorkspaceInfo>, f64), FallowError> {
    let t = Instant::now();
    let (workspaces, diagnostics) =
        discover_workspaces_with_diagnostics(&config.root, &config.ignore_patterns)
            .map_err(|error| FallowError::config(error.to_string()))?;
    fallow_config::stash_workspace_diagnostics(&config.root, diagnostics);
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

    Ok((workspaces, workspaces_ms))
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
#[doc(hidden)]
pub struct AnalysisDiscovery {
    files: Vec<discover::DiscoveredFile>,
    workspaces: Vec<fallow_config::WorkspaceInfo>,
    root_pkg: Option<PackageJson>,
    config_candidates: Vec<std::path::PathBuf>,
    discover_ms: f64,
    workspaces_ms: f64,
}

impl AnalysisDiscovery {
    /// Build a discovery prelude from an engine-owned discovery run.
    #[must_use]
    pub fn from_parts(
        files: Vec<discover::DiscoveredFile>,
        workspaces: Vec<fallow_config::WorkspaceInfo>,
        root_pkg: Option<PackageJson>,
        config_candidates: Vec<std::path::PathBuf>,
        discover_ms: f64,
        workspaces_ms: f64,
    ) -> Self {
        Self {
            files,
            workspaces,
            root_pkg,
            config_candidates,
            discover_ms,
            workspaces_ms,
        }
    }

    /// Discovered source files, indexed by stable `FileId` for this session.
    #[must_use]
    fn files(&self) -> &[discover::DiscoveredFile] {
        &self.files
    }

    /// Discovered workspace packages for this session.
    #[must_use]
    pub fn workspaces(&self) -> &[fallow_config::WorkspaceInfo] {
        &self.workspaces
    }

    /// Consume this discovery prelude and return its source file registry.
    #[must_use]
    pub fn into_files(self) -> Vec<discover::DiscoveredFile> {
        self.files
    }
}

/// Owned state shared across one legacy core analysis run.
///
/// Engine-owned sessions use `fallow-engine`; this remains only for deprecated
/// core entrypoints while core is being narrowed to detector/backend helpers.
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
    fn new(config: &'a ResolvedConfig) -> Result<Self, FallowError> {
        let pipeline_start = Instant::now();
        let AnalysisSetup {
            progress,
            project,
            root_pkg,
            config_candidates,
            discover_ms,
            workspaces_ms,
        } = run_analysis_setup(config)?;

        Ok(Self {
            config,
            pipeline_start,
            progress,
            project,
            root_pkg,
            config_candidates,
            discover_ms,
            workspaces_ms,
        })
    }

    fn files(&self) -> &[discover::DiscoveredFile] {
        self.project.files()
    }

    fn workspaces(&self) -> &[fallow_config::WorkspaceInfo] {
        self.project.workspaces()
    }

    fn load_workspace_packages(&self) -> Vec<LoadedWorkspacePackage> {
        load_workspace_packages(self.workspaces())
    }

    fn run_plugins_and_scripts(
        &self,
        workspace_pkgs: &[LoadedWorkspacePackage],
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
        workspace_pkgs: &[LoadedWorkspacePackage],
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
        let mut graph_cache_rejection = None;
        let (resolved, graph) =
            match try_load_analysis_graph_cache(&shared, &entry_points, &modules) {
                Ok(hit) => (
                    TimedResolvedModules {
                        project: hit.project,
                        elapsed_ms: 0.0,
                    },
                    TimedGraph {
                        graph: hit.graph,
                        elapsed_ms: hit.elapsed_ms,
                    },
                ),
                Err(rejection) => {
                    graph_cache_rejection = rejection;
                    let resolved = resolve_analysis_imports_timed(&shared, &modules);
                    let graph = build_analysis_graph_timed(
                        &shared,
                        &resolved.project,
                        &entry_points,
                        &modules,
                    );
                    (resolved, graph)
                }
            };
        release_resolution_payloads(&mut modules);
        let analysis = analyze_dead_code_timed(
            &shared,
            &graph.graph,
            &resolved.project.modules,
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
            entry_point_spans: entry_points.spans,
            resolve_ms: resolved.elapsed_ms,
            graph_ms: graph.elapsed_ms,
            analyze_ms: analysis.elapsed_ms,
            graph_cache_rejection,
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
}

/// Run the shared prelude: progress setup, node_modules check, workspace and
/// root-package discovery, hidden-dir scoping, and file discovery.
fn run_analysis_setup(config: &ResolvedConfig) -> Result<AnalysisSetup, FallowError> {
    let progress = new_analysis_progress(config);

    let (workspaces_vec, workspaces_ms) = discover_analysis_workspaces(config)?;
    let root_pkg = fallow_config::load_dir_package_json(&config.root);
    let discovery_hidden_dir_scopes =
        discover::collect_hidden_dir_scopes(config, root_pkg.as_ref(), &workspaces_vec);

    let t = Instant::now();
    progress.set_stage("discovering files...");
    let (discovered_files, config_candidates) =
        discover::discover_files_and_config_candidates(config, &discovery_hidden_dir_scopes);
    let discover_ms = t.elapsed().as_secs_f64() * 1000.0;

    let project = project::ProjectState::new(discovered_files, workspaces_vec);

    Ok(AnalysisSetup {
        progress,
        project,
        root_pkg,
        config_candidates,
        discover_ms,
        workspaces_ms,
    })
}

/// Borrowed inputs for plugin detection and script analysis.
struct PluginScriptInput<'a> {
    config: &'a ResolvedConfig,
    progress: &'a progress::AnalysisProgress,
    files: &'a [discover::DiscoveredFile],
    workspaces: &'a [fallow_config::WorkspaceInfo],
    root_pkg: Option<&'a PackageJson>,
    workspace_pkgs: &'a [LoadedWorkspacePackage],
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

/// Timings captured by the dead-code backend prelude.
#[derive(Debug, Clone, Copy)]
#[doc(hidden)]
pub struct DeadCodePreludeTimings {
    pub discover_ms: f64,
    pub workspaces_ms: f64,
    pub plugins_ms: f64,
    pub scripts_ms: f64,
}

/// Opaque backend prelude for engine-owned dead-code orchestration.
///
/// The engine owns the phase ordering. Core keeps the detector/backend state
/// needed by those phases private.
#[doc(hidden)]
pub struct DeadCodeBackendPrelude<'a> {
    config: &'a ResolvedConfig,
    pipeline_start: Instant,
    progress: progress::AnalysisProgress,
    discovery: AnalysisDiscovery,
    workspace_pkgs: Vec<LoadedWorkspacePackage>,
    plugin_result: plugins::AggregatedPluginResult,
    plugins_ms: f64,
    scripts_ms: f64,
}

impl DeadCodeBackendPrelude<'_> {
    #[must_use]
    pub fn timings(&self) -> DeadCodePreludeTimings {
        DeadCodePreludeTimings {
            discover_ms: self.discovery.discover_ms,
            workspaces_ms: self.discovery.workspaces_ms,
            plugins_ms: self.plugins_ms,
            scripts_ms: self.scripts_ms,
        }
    }

    #[must_use]
    pub fn elapsed_ms(&self) -> f64 {
        self.pipeline_start.elapsed().as_secs_f64() * 1000.0
    }

    #[must_use]
    pub fn script_used_packages(&self) -> FxHashSet<String> {
        self.plugin_result.script_used_packages.clone()
    }

    pub fn finish(&self) {
        self.progress.finish();
    }
}

/// Entry-point discovery result for an engine-owned dead-code pipeline.
#[doc(hidden)]
pub struct DeadCodeEntryPoints {
    inner: TimedEntryPoints,
}

impl DeadCodeEntryPoints {
    #[must_use]
    pub fn count(&self) -> usize {
        self.inner.count
    }

    #[must_use]
    pub fn elapsed_ms(&self) -> f64 {
        self.inner.elapsed_ms
    }

    /// Sub-phase attribution for the discovery stage this result timed.
    #[must_use]
    pub fn spans(&self) -> EntryPointSpans {
        self.inner.spans
    }
}

/// Import-resolution result for an engine-owned dead-code pipeline.
#[doc(hidden)]
pub struct DeadCodeResolvedModules {
    pub project: resolve::ResolvedProject,
    pub elapsed_ms: f64,
}

/// Graph build or graph-cache result for an engine-owned dead-code pipeline.
#[doc(hidden)]
pub struct DeadCodeGraphRun {
    pub graph: graph::ModuleGraph,
    pub elapsed_ms: f64,
}

/// Detector result for an engine-owned dead-code pipeline.
#[doc(hidden)]
pub struct DeadCodeDetectorRun {
    pub results: AnalysisResults,
    pub elapsed_ms: f64,
}

/// Prepare plugin and script context for engine-owned dead-code orchestration.
///
/// # Errors
///
/// Returns an error if plugin detection fails.
pub fn prepare_dead_code_backend_prelude(
    config: &ResolvedConfig,
    discovery: AnalysisDiscovery,
) -> Result<DeadCodeBackendPrelude<'_>, FallowError> {
    let progress = new_analysis_progress(config);
    let pipeline_start = Instant::now();
    let workspace_pkgs = load_workspace_packages(&discovery.workspaces);
    let (plugin_result, plugins_ms, scripts_ms) = run_plugins_and_scripts(&PluginScriptInput {
        config,
        progress: &progress,
        files: discovery.files(),
        workspaces: &discovery.workspaces,
        root_pkg: discovery.root_pkg.as_ref(),
        workspace_pkgs: &workspace_pkgs,
        config_candidates: &discovery.config_candidates,
    })?;

    Ok(DeadCodeBackendPrelude {
        config,
        pipeline_start,
        progress,
        discovery,
        workspace_pkgs,
        plugin_result,
        plugins_ms,
        scripts_ms,
    })
}

/// Discover entry points for an engine-owned dead-code pipeline.
#[must_use]
pub fn discover_dead_code_entry_points(
    prelude: &DeadCodeBackendPrelude<'_>,
) -> DeadCodeEntryPoints {
    let shared = prelude.shared_input();
    DeadCodeEntryPoints {
        inner: discover_analysis_entry_points(&shared),
    }
}

/// Try loading the graph cache for an engine-owned dead-code pipeline.
///
/// # Errors
///
/// Returns the reason the persisted graph was refused, the same way the
/// owned-core path does. The engine pipeline reports it in the perf table, so
/// collapsing a refusal into a bare miss here would make the only feature that
/// explains a slow warm run unreachable from every engine-backed command.
/// `Err(None)` means there was nothing to refuse: the run disabled caching.
pub fn try_load_dead_code_graph_cache(
    prelude: &DeadCodeBackendPrelude<'_>,
    entry_points: &DeadCodeEntryPoints,
    modules: &[extract::ModuleInfo],
) -> Result<(DeadCodeResolvedModules, DeadCodeGraphRun), Option<CacheRejection>> {
    let shared = prelude.shared_input();
    try_load_analysis_graph_cache(&shared, &entry_points.inner, modules).map(|hit| {
        (
            DeadCodeResolvedModules {
                project: hit.project,
                elapsed_ms: 0.0,
            },
            DeadCodeGraphRun {
                graph: hit.graph,
                elapsed_ms: hit.elapsed_ms,
            },
        )
    })
}

/// Resolve imports for an engine-owned dead-code pipeline.
#[must_use]
pub fn resolve_dead_code_imports(
    prelude: &DeadCodeBackendPrelude<'_>,
    modules: &[extract::ModuleInfo],
) -> DeadCodeResolvedModules {
    let shared = prelude.shared_input();
    let resolved = resolve_analysis_imports_timed(&shared, modules);
    DeadCodeResolvedModules {
        project: resolved.project,
        elapsed_ms: resolved.elapsed_ms,
    }
}

/// Build the module graph for an engine-owned dead-code pipeline.
#[must_use]
pub fn build_dead_code_graph(
    prelude: &DeadCodeBackendPrelude<'_>,
    project: &resolve::ResolvedProject,
    entry_points: &DeadCodeEntryPoints,
    modules: &[extract::ModuleInfo],
) -> DeadCodeGraphRun {
    let shared = prelude.shared_input();
    let graph = build_analysis_graph_timed(&shared, project, &entry_points.inner, modules);
    DeadCodeGraphRun {
        graph: graph.graph,
        elapsed_ms: graph.elapsed_ms,
    }
}

/// Run the dead-code detectors for an engine-owned pipeline.
#[must_use]
pub fn run_dead_code_detectors(
    prelude: &DeadCodeBackendPrelude<'_>,
    graph: &graph::ModuleGraph,
    resolved: &[resolve::ResolvedModule],
    modules: &[extract::ModuleInfo],
    collect_usages: bool,
    entry_points: &DeadCodeEntryPoints,
) -> DeadCodeDetectorRun {
    let shared = prelude.shared_input();
    let analysis = analyze_dead_code_timed(
        &shared,
        graph,
        resolved,
        modules,
        collect_usages,
        entry_points.inner.summary.clone(),
    );
    DeadCodeDetectorRun {
        results: analysis.result,
        elapsed_ms: analysis.elapsed_ms,
    }
}

impl<'a> DeadCodeBackendPrelude<'a> {
    fn shared_input(&'a self) -> AnalysisCoreSharedInput<'a> {
        AnalysisCoreSharedInput {
            config: self.config,
            progress: &self.progress,
            files: self.discovery.files(),
            workspaces: &self.discovery.workspaces,
            root_pkg: self.discovery.root_pkg.as_ref(),
            workspace_pkgs: &self.workspace_pkgs,
            plugin_result: &self.plugin_result,
        }
    }
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

struct AnalysisCoreSharedInput<'a> {
    config: &'a ResolvedConfig,
    progress: &'a progress::AnalysisProgress,
    files: &'a [discover::DiscoveredFile],
    workspaces: &'a [fallow_config::WorkspaceInfo],
    root_pkg: Option<&'a PackageJson>,
    workspace_pkgs: &'a [LoadedWorkspacePackage],
    plugin_result: &'a plugins::AggregatedPluginResult,
}

struct TimedEntryPoints {
    entry_points: discover::CategorizedEntryPoints,
    summary: results::EntryPointSummary,
    count: usize,
    elapsed_ms: f64,
    spans: EntryPointSpans,
}

struct TimedResolvedModules {
    project: resolve::ResolvedProject,
    elapsed_ms: f64,
}

struct TimedGraph {
    graph: graph::ModuleGraph,
    elapsed_ms: f64,
}

struct GraphCacheHit {
    graph: graph::ModuleGraph,
    project: resolve::ResolvedProject,
    elapsed_ms: f64,
}

#[derive(Clone, Copy)]
struct DiscoverAllEntryPointsInput<'a> {
    config: &'a ResolvedConfig,
    files: &'a [discover::DiscoveredFile],
    workspaces: &'a [fallow_config::WorkspaceInfo],
    root_pkg: Option<&'a PackageJson>,
    workspace_pkgs: &'a [LoadedWorkspacePackage],
    plugin_result: &'a plugins::AggregatedPluginResult,
}

struct TimedAnalysis {
    result: AnalysisResults,
    elapsed_ms: f64,
}

fn discover_analysis_entry_points(input: &AnalysisCoreSharedInput<'_>) -> TimedEntryPoints {
    let t = Instant::now();
    let (entry_points, spans) = discover_all_entry_points(DiscoverAllEntryPointsInput {
        config: input.config,
        files: input.files,
        workspaces: input.workspaces,
        root_pkg: input.root_pkg,
        workspace_pkgs: input.workspace_pkgs,
        plugin_result: input.plugin_result,
    });
    let elapsed_ms = t.elapsed().as_secs_f64() * 1000.0;
    let summary = summarize_entry_points(&entry_points.all);
    let count = entry_points.all.len();

    TimedEntryPoints {
        entry_points,
        summary,
        count,
        elapsed_ms,
        spans,
    }
}

/// Try to reuse the persisted module graph.
///
/// # Errors
///
/// `Err(Some(reason))` names why persisted work was refused. Two of these
/// branches are decided AFTER a full decode of a multi-megabyte blob, so they
/// are the most expensive refusals in the pipeline; leaving them silent made a
/// fully paid, fully wasted load indistinguishable from a first run.
/// `Err(None)` means there was nothing to refuse: the run disabled caching.
fn try_load_analysis_graph_cache(
    input: &AnalysisCoreSharedInput<'_>,
    entry_points: &TimedEntryPoints,
    modules: &[extract::ModuleInfo],
) -> Result<GraphCacheHit, Option<CacheRejection>> {
    if input.config.no_cache {
        return Err(None);
    }

    let t = Instant::now();
    input.progress.set_stage("loading module graph cache...");
    let current = build_graph_cache_manifest(
        input.config,
        input.plugin_result,
        &entry_points.entry_points,
        input.files,
        modules,
    );
    let store = graph_cache::GraphCacheStore::load(&input.config.cache_dir).map_err(Some)?;
    if store.manifest.matches_inputs(&current) {
        let project = restore_cached_resolved_project(input, modules, &store.resolved_project)?;
        tracing::debug!("Graph cache hit: skipping import resolution and graph build");

        return Ok(GraphCacheHit {
            graph: store.graph,
            project,
            elapsed_ms: t.elapsed().as_secs_f64() * 1000.0,
        });
    }

    if let Some(rejection) = store.manifest.classify_resolution_mismatch(&current) {
        // The level is the rejection's own judgement of whether the reader can
        // do anything about it. Content drift is what a cache is for, so it
        // stays off stderr and is read from doctor or the performance table
        // instead; `describe()` already names the reason, so no field repeats it.
        if rejection.discarded_existing_work() {
            tracing::warn!(
                "Graph cache decoded but not reused: {}",
                rejection.describe()
            );
        } else {
            tracing::debug!(
                "Graph cache decoded but not reused: {}",
                rejection.describe()
            );
        }
        return Err(Some(rejection));
    }

    let project = restore_cached_resolved_project(input, modules, &store.resolved_project)?;
    tracing::debug!("Graph resolver cache hit: skipping import resolution and rebuilding graph");
    let graph = build_analysis_graph_timed(input, &project, entry_points, modules);

    Ok(GraphCacheHit {
        graph: graph.graph,
        project,
        elapsed_ms: t.elapsed().as_secs_f64() * 1000.0,
    })
}

/// Remap a decoded resolver payload onto the current `FileId`s.
///
/// A failure here means the persisted stable keys no longer describe the
/// discovered files, which is a file-set change reported the same way as a
/// manifest-level one rather than a silent miss.
fn restore_cached_resolved_project(
    input: &AnalysisCoreSharedInput<'_>,
    modules: &[extract::ModuleInfo],
    resolved_project: &graph_cache::CachedResolvedProject,
) -> Result<resolve::ResolvedProject, Option<CacheRejection>> {
    graph_cache::restore_resolved_project(
        &input.config.root,
        modules,
        input.files,
        resolved_project,
    )
    .inspect(|project| {
        record_unreadable_auto_import_reads(
            project,
            input.files,
            input.plugin_result,
            input.config,
        );
    })
    .ok_or_else(|| {
        // Same file-set drift the manifest reports, and equally routine: at
        // debug for the reason `CacheRejection::discarded_existing_work`
        // documents.
        tracing::debug!(
            "Graph cache decoded but its resolver payload no longer maps to the discovered files"
        );
        Some(CacheRejection::FileSetChanged)
    })
}

fn resolve_analysis_imports_timed(
    input: &AnalysisCoreSharedInput<'_>,
    modules: &[extract::ModuleInfo],
) -> TimedResolvedModules {
    let t = Instant::now();
    input.progress.set_stage("resolving imports...");
    let project = resolve_analysis_imports(
        modules,
        input.files,
        input.workspaces,
        input.plugin_result,
        input.config,
    );
    TimedResolvedModules {
        project,
        elapsed_ms: t.elapsed().as_secs_f64() * 1000.0,
    }
}

fn build_analysis_graph_timed(
    input: &AnalysisCoreSharedInput<'_>,
    project: &resolve::ResolvedProject,
    entry_points: &TimedEntryPoints,
    modules: &[extract::ModuleInfo],
) -> TimedGraph {
    let t = Instant::now();
    input.progress.set_stage("building module graph...");
    let graph = build_analysis_graph(&BuildAnalysisGraphInput {
        config: input.config,
        plugin_result: input.plugin_result,
        project,
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
    AnalysisSession::new(config)?.run_full(retain, collect_usages, need_complexity, retain_modules)
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
    entry_point_spans: EntryPointSpans,
    resolve_ms: f64,
    graph_ms: f64,
    analyze_ms: f64,
    graph_cache_rejection: Option<CacheRejection>,
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
        entry_point_spans: core.entry_point_spans,
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
        cache_rejection: parse.cache_rejection,
        graph_cache_rejection: core.graph_cache_rejection,
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
    entry_point_spans: EntryPointSpans,
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
    cache_rejection: Option<CacheRejection>,
    graph_cache_rejection: Option<CacheRejection>,
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
    /// Why the persisted parse cache was not reused, when it was not.
    cache_rejection: Option<CacheRejection>,
}

impl From<AnalysisParseMetrics> for ParseMetrics {
    fn from(metrics: AnalysisParseMetrics) -> Self {
        Self {
            parse_ms: metrics.parse_ms,
            cache_ms: metrics.cache_ms,
            cache_hits: metrics.cache_hits,
            cache_misses: metrics.cache_misses,
            parse_cpu_ms: metrics.parse_cpu_ms,
            cache_rejection: metrics.cache_rejection,
        }
    }
}

fn parse_analysis_modules(
    config: &ResolvedConfig,
    files: &[discover::DiscoveredFile],
    need_complexity: bool,
    start: Instant,
) -> AnalysisParseOutput {
    let cache_max_size_bytes = resolve_cache_max_size_bytes(config);
    let mut cache_rejection = None;
    let mut cache_store = if config.no_cache {
        None
    } else {
        match cache::CacheStore::load(
            &config.cache_dir,
            &config.root,
            config.cache_config_hash,
            cache_max_size_bytes,
        ) {
            Ok(store) => Some(store),
            Err(rejection) => {
                cache_rejection = Some(rejection);
                None
            }
        }
    };

    let parse_result = extract::parse_all_files(files, cache_store.as_ref(), need_complexity);
    let _ = fallow_config::record_source_read_failures(&config.root, &parse_result.read_failures);
    let _ = fallow_config::record_source_parse_degradations(
        &config.root,
        &parse_result.parse_degradations,
    );
    let modules = parse_result.modules;
    let parse_ms = start.elapsed().as_secs_f64() * 1000.0;
    let cache_ms = update_parse_cache_if_enabled(
        config,
        &mut cache_store,
        &modules,
        files,
        cache_max_size_bytes,
        need_complexity,
    );

    AnalysisParseOutput {
        modules,
        metrics: ParseMetrics {
            parse_ms,
            cache_ms,
            cache_hits: parse_result.cache_hits,
            cache_misses: parse_result.cache_misses,
            parse_cpu_ms: parse_result.parse_cpu_ms,
            cache_rejection,
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
        cache_rejection: profile.cache_rejection,
        graph_cache_rejection: profile.graph_cache_rejection,
        cache_update_ms: profile.cache_ms,
        entry_points_ms: profile.entry_points_ms,
        entry_point_spans: profile.entry_point_spans,
        entry_point_count: profile.entry_point_count,
        resolve_imports_ms: profile.resolve_ms,
        build_graph_ms: profile.graph_ms,
        analyze_ms: profile.analyze_ms,
        duplication_ms: None,
        total_ms: profile.total_ms,
    })
}

fn update_parse_cache_if_enabled(
    config: &ResolvedConfig,
    cache_store: &mut Option<cache::CacheStore>,
    modules: &[extract::ModuleInfo],
    files: &[discover::DiscoveredFile],
    cache_max_size_bytes: usize,
    need_complexity: bool,
) -> f64 {
    let t = Instant::now();
    if !config.no_cache {
        let store = cache_store.get_or_insert_with(|| cache::CacheStore::new(&config.root));
        if update_cache(store, modules, files, need_complexity)
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
) -> resolve::ResolvedProject {
    let mut project = resolve::resolve_all_imports(&resolve::ResolveAllImportsInput {
        modules,
        files,
        workspaces,
        active_plugins: &plugin_result.active_plugins,
        path_aliases: &plugin_result.path_aliases,
        auto_imports: &plugin_result.auto_imports,
        scss_include_paths: &plugin_result.scss_include_paths,
        static_dir_mappings: &plugin_result.static_dir_mappings,
        framework_static_dir_mappings: &plugin_result.framework_static_dir_mappings,
        root: &config.root,
        extra_conditions: &config.resolve.conditions,
    });
    external_style_usage::augment_external_style_package_usage(
        &mut project.modules,
        config,
        workspaces,
        plugin_result,
    );
    record_unreadable_auto_import_reads(&project, files, plugin_result, config);
    project
}

/// Record one `plugin-effect-not-modeled` diagnostic for each file that reads
/// `#components` or `#imports` in a way the graph cannot narrow to names, such
/// as a spread of a namespace import. The read credits every name of that
/// module, so the run can miss unused convention files, and nothing else says
/// so. Only a run with `autoImports` on drops the convention entry patterns, so
/// only that run records it. See issue #2752.
///
/// The plugin stage replaces every plugin-stage diagnostic at the start of the
/// run, so an append here cannot leave a stale entry. The input is the resolved
/// project, which a warm graph cache restores, so a warm run records the same
/// entries as a cold one.
fn record_unreadable_auto_import_reads(
    project: &resolve::ResolvedProject,
    files: &[discover::DiscoveredFile],
    plugin_result: &plugins::AggregatedPluginResult,
    config: &ResolvedConfig,
) {
    if !config.auto_imports
        || plugin_result.auto_imports.is_empty()
        || !plugin_result
            .active_plugins
            .iter()
            .any(|name| name == "nuxt")
    {
        return;
    }
    let diagnostics: Vec<fallow_config::WorkspaceDiagnostic> =
        resolve::unreadable_auto_import_reads(&project.modules)
            .into_iter()
            .filter_map(|read| {
                let file = files.get(read.file_id.0 as usize)?;
                let diagnostic = plugins::PluginConfigDiagnostic::not_modeled(
                    &file.path,
                    "nuxt",
                    read.module,
                    AUTO_IMPORT_KEY_NOT_MODELED,
                );
                Some(diagnostic.into_workspace_diagnostic(&config.root))
            })
            .collect();
    fallow_config::append_workspace_diagnostics(&config.root, diagnostics);
}

struct BuildAnalysisGraphInput<'a> {
    config: &'a ResolvedConfig,
    plugin_result: &'a plugins::AggregatedPluginResult,
    project: &'a resolve::ResolvedProject,
    entry_points: &'a discover::CategorizedEntryPoints,
    files: &'a [discover::DiscoveredFile],
    modules: &'a [extract::ModuleInfo],
    workspaces: &'a [fallow_config::WorkspaceInfo],
}

/// Build the analysis graph and persist it for the next identical run.
///
/// The warm hit path happens before import resolution in
/// `try_load_analysis_graph_cache`. This miss path always builds fresh, runs
/// both credit steps, and persists the graph plus resolver outputs for next
/// time. The cache is gated on `config.no_cache` and is a strict performance
/// optimization: a cache hit produces identical analysis results.
fn build_analysis_graph(input: &BuildAnalysisGraphInput<'_>) -> graph::ModuleGraph {
    let caching_enabled = !input.config.no_cache;
    let current_manifest = caching_enabled.then(|| {
        build_graph_cache_manifest(
            input.config,
            input.plugin_result,
            input.entry_points,
            input.files,
            input.modules,
        )
    });

    let mut graph = graph::ModuleGraph::build_with_reachability_roots_and_replacements(
        &input.project.modules,
        &input.project.replaced_module_targets,
        &input.entry_points.all,
        &input.entry_points.runtime,
        &input.entry_points.test,
        input.files,
    );
    credit_package_path_references(&mut graph, input.modules);
    credit_workspace_package_usage(&mut graph, &input.project.modules, input.workspaces);

    if let Some(manifest) = current_manifest {
        let Some(resolved_project) =
            graph_cache::cache_resolved_project(&input.config.root, input.files, input.project)
        else {
            return graph;
        };
        let store = graph_cache::GraphCacheStore {
            version: graph_cache::GRAPH_CACHE_VERSION,
            manifest,
            graph,
            resolved_project,
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
    config: &ResolvedConfig,
    plugin_result: &plugins::AggregatedPluginResult,
    entry_points: &discover::CategorizedEntryPoints,
    files: &[discover::DiscoveredFile],
    modules: &[extract::ModuleInfo],
) -> graph_cache::GraphCacheManifest {
    let mode = graph_cache::GraphCacheMode::new(
        resolver_options_hash(config),
        entry_points_hash(entry_points, &config.root),
        plugin_config_hash(plugin_result, &config.root),
    );
    // The parse stage already hashed every file it read, so keying the manifest
    // on content costs nothing and does not inherit mtime's blind spot for a
    // same-size rewrite. A file with no module (unreadable) hashes to 0, which
    // compares equal only against another run that also failed to read it.
    let mut content_hashes = vec![0u64; files.len()];
    for module in modules {
        if let Some(slot) = content_hashes.get_mut(module.file_id.0 as usize) {
            *slot = module.content_hash;
        }
    }
    graph_cache::GraphCacheManifest::from_discovered_files(&config.root, files, mode, |file| {
        content_hashes
            .get(file.id.0 as usize)
            .copied()
            .unwrap_or_default()
    })
}

/// Hash the resolver-affecting options: the extraction config hash and the
/// user-supplied resolve `conditions`.
///
/// The project root is deliberately NOT hashed. It is not a resolver option:
/// it is where the project happens to sit, and hashing it meant a container
/// job, a matrix over roots, or a copied worktree could never reuse a cache it
/// had fully decoded. Every path this mode covers is hashed root-relative
/// instead.
///
/// `cache_config_hash` does NOT fold tsconfig `paths`, despite what this
/// comment used to claim: it hashes extraction-affecting config plus external
/// plugin names. tsconfig-derived aliases reach the mode through
/// [`hash_path_aliases`].
///
/// `production` and `ignore_patterns` intentionally stay out of this hash:
/// they shape discovery, so changed file sets already miss through stable file
/// keys and source fingerprints in the manifest.
fn resolver_options_hash(config: &ResolvedConfig) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = rustc_hash::FxHasher::default();
    config.cache_config_hash.hash(&mut hasher);
    config.resolve.conditions.hash(&mut hasher);
    hasher.finish()
}

/// Render `path` the way every graph-cache mode hash spells a path: relative to
/// the project root where possible, with forward slashes, so an identical
/// project under a different absolute path hashes identically.
fn root_relative_key(root: &std::path::Path, path: &std::path::Path) -> String {
    path.strip_prefix(root)
        .unwrap_or(path)
        .to_string_lossy()
        .replace('\\', "/")
}

/// Hash the entry-point set (sorted root-relative paths per role) so any change
/// in reachability roots misses the cache.
fn entry_points_hash(
    entry_points: &discover::CategorizedEntryPoints,
    root: &std::path::Path,
) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = rustc_hash::FxHasher::default();
    for role in [&entry_points.all, &entry_points.runtime, &entry_points.test] {
        let mut keys: Vec<String> = role
            .iter()
            .map(|ep| root_relative_key(root, &ep.path))
            .collect();
        keys.sort_unstable();
        keys.len().hash(&mut hasher);
        for key in keys {
            key.hash(&mut hasher);
        }
    }
    hasher.finish()
}

/// Hash the plugin-derived graph-affecting configuration, with every path
/// spelled root-relative.
fn plugin_config_hash(
    plugin_result: &plugins::AggregatedPluginResult,
    root: &std::path::Path,
) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = rustc_hash::FxHasher::default();

    hash_active_plugins(plugin_result, &mut hasher);
    hash_path_aliases(plugin_result, root, &mut hasher);

    let mut auto_imports: Vec<AutoImportHashKey<'_>> = plugin_result
        .auto_imports
        .iter()
        .map(|rule| {
            let mut scope: Vec<String> = rule
                .scope
                .iter()
                .map(|scope_root| root_relative_key(root, scope_root))
                .collect();
            scope.sort_unstable();
            (
                rule.name.as_str(),
                root_relative_key(root, &rule.source),
                auto_import_kind_rank(rule.kind),
                scope,
            )
        })
        .collect();
    auto_imports.sort_unstable();
    auto_imports.len().hash(&mut hasher);
    for key in &auto_imports {
        key.hash(&mut hasher);
    }

    let mut scss_include_paths: Vec<String> = plugin_result
        .scss_include_paths
        .iter()
        .map(|path| root_relative_key(root, path))
        .collect();
    scss_include_paths.sort_unstable();
    scss_include_paths.len().hash(&mut hasher);
    for path in scss_include_paths {
        path.hash(&mut hasher);
    }

    let mut static_dir_mappings: Vec<(String, &str)> = plugin_result
        .static_dir_mappings
        .iter()
        .map(|(from_dir, mount)| (root_relative_key(root, from_dir), mount.as_str()))
        .collect();
    static_dir_mappings.sort_unstable();
    static_dir_mappings.len().hash(&mut hasher);
    for (from_dir, mount) in static_dir_mappings {
        from_dir.hash(&mut hasher);
        mount.hash(&mut hasher);
    }

    hasher.finish()
}

fn hash_active_plugins(
    plugin_result: &plugins::AggregatedPluginResult,
    hasher: &mut rustc_hash::FxHasher,
) {
    use std::hash::Hash;
    let mut active: Vec<&str> = plugin_result
        .active_plugins
        .iter()
        .map(String::as_str)
        .collect();
    active.sort_unstable();
    active.len().hash(hasher);
    for name in active {
        name.hash(hasher);
    }
}

/// Hash tsconfig- and plugin-derived path aliases. Replacements are absolute
/// filesystem paths, so they are spelled root-relative like every other path in
/// the mode hash.
fn hash_path_aliases(
    plugin_result: &plugins::AggregatedPluginResult,
    root: &std::path::Path,
    hasher: &mut rustc_hash::FxHasher,
) {
    use std::hash::Hash;
    let mut aliases: Vec<(&str, String)> = plugin_result
        .path_aliases
        .iter()
        .map(|(prefix, replacement)| {
            (
                prefix.as_str(),
                root_relative_key(root, std::path::Path::new(replacement)),
            )
        })
        .collect();
    aliases.sort_unstable();
    aliases.len().hash(hasher);
    for (prefix, replacement) in aliases {
        prefix.hash(hasher);
        replacement.hash(hasher);
    }
}

/// The fields of one auto-import rule that decide its graph edges: name,
/// root-relative source, kind rank, and sorted root-relative scope.
type AutoImportHashKey<'a> = (&'a str, String, u8, Vec<String>);

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
        cache_rejection,
        ..
    } = *profile;
    let cache_summary = cache_rejection.map_or_else(
        || format!(" ({cache_hits} cached, {cache_misses} parsed)"),
        |rejection| {
            format!(
                " ({cache_hits} cached, {cache_misses} parsed, cache refused: {})",
                rejection.describe()
            )
        },
    );

    tracing::debug!(
        "\n┌─ Pipeline Profile ─────────────────────────────\n\
         │  discover files:   {:>8.1}ms  ({} files)\n\
         │  workspaces:       {:>8.1}ms\n\
         │  plugin detection: {:>8.1}ms\n\
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

fn load_workspace_packages(
    workspaces: &[fallow_config::WorkspaceInfo],
) -> Vec<LoadedWorkspacePackage> {
    workspaces
        .iter()
        .filter_map(|ws| {
            fallow_config::load_dir_package_json(&ws.root).map(|pkg| (ws.clone(), pkg))
        })
        .collect()
}

/// Analyze package.json scripts from root and all workspace packages.
///
/// Populates the plugin result with script-used packages and config file
/// entry patterns. Also scans CI config files for binary invocations.
fn analyze_all_scripts(
    config: &ResolvedConfig,
    workspaces: &[fallow_config::WorkspaceInfo],
    root_pkg: Option<&PackageJson>,
    workspace_pkgs: &[LoadedWorkspacePackage],
    plugin_result: &mut plugins::AggregatedPluginResult,
) {
    let all_dep_names = collect_all_dependency_names(root_pkg, workspace_pkgs);
    let all_dep_set: FxHashSet<String> = all_dep_names.iter().cloned().collect();
    let all_scripts = collect_all_scripts(root_pkg, workspace_pkgs);

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
    analyze_ci_scripts(config, &bin_map, &all_dep_set, &all_scripts, plugin_result);

    plugin_result
        .entry_point_roles
        .entry("scripts".to_string())
        .or_insert(EntryPointRole::Support);
}

/// Gather sorted, deduped dependency names across the root and workspace packages.
fn collect_all_dependency_names(
    root_pkg: Option<&PackageJson>,
    workspace_pkgs: &[LoadedWorkspacePackage],
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

/// Gather the scripts declared by the root and workspace packages.
fn collect_all_scripts(
    root_pkg: Option<&PackageJson>,
    workspace_pkgs: &[LoadedWorkspacePackage],
) -> scripts::ScriptCatalog {
    let mut catalog = scripts::ScriptCatalog::default();
    if let Some(pkg) = root_pkg
        && let Some(ref pkg_scripts) = pkg.scripts
    {
        catalog.merge_scripts(pkg_scripts);
    }
    for (_, ws_pkg) in workspace_pkgs {
        if let Some(ref ws_scripts) = ws_pkg.scripts {
            catalog.merge_workspace_scripts(ws_scripts);
        }
    }
    catalog
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
    let catalog =
        scripts::ScriptCatalog::from_scripts_with_bodies(pkg_scripts, &scripts_to_analyze);
    let script_analysis = scripts::analyze_scripts_with_dependency_context(
        &scripts_to_analyze,
        &config.root,
        bin_map,
        all_dep_set,
        &catalog,
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
    workspace_pkgs: &[LoadedWorkspacePackage],
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
    let catalog = scripts::ScriptCatalog::from_scripts_with_bodies(ws_scripts, &scripts_to_analyze);
    let ws_analysis = scripts::analyze_scripts_with_dependency_context(
        &scripts_to_analyze,
        &ws.root,
        bin_map,
        all_dep_set,
        &catalog,
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
    all_scripts: &scripts::ScriptCatalog,
    plugin_result: &mut plugins::AggregatedPluginResult,
) {
    let ci_analysis =
        scripts::ci::analyze_ci_files(&config.root, bin_map, all_dep_set, all_scripts);
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
    input: DiscoverAllEntryPointsInput<'_>,
) -> (discover::CategorizedEntryPoints, EntryPointSpans) {
    let mut spans = EntryPointSpans::default();
    let mut mark = Instant::now();
    let mut entry_points = discover::CategorizedEntryPoints::default();
    let root_discovery = discover::discover_entry_points_with_warnings_from_pkg(
        input.config,
        input.files,
        input.root_pkg,
        input.workspaces.is_empty(),
    );
    spans.root_ms = split_ms(&mut mark);

    let workspace_pkg_by_root: rustc_hash::FxHashMap<std::path::PathBuf, &PackageJson> = input
        .workspace_pkgs
        .iter()
        .map(|(ws, pkg)| (ws.root.clone(), pkg))
        .collect();
    let workspace_script_seeds = discover::workspace_runtime_script_seeds(
        &input.config.root,
        input.root_pkg,
        input.workspace_pkgs,
    );

    let workspace_discovery: Vec<discover::EntryPointDiscovery> = input
        .workspaces
        .par_iter()
        .map(|ws| {
            let pkg = workspace_pkg_by_root.get(&ws.root).copied();
            let seeds = workspace_script_seeds
                .get(&ws.name)
                .cloned()
                .unwrap_or_default();
            discover::discover_workspace_entry_points_with_runtime_scripts(
                &ws.root,
                input.files,
                pkg,
                &seeds,
            )
        })
        .collect();
    let mut skipped_entries = rustc_hash::FxHashMap::default();
    entry_points.extend_runtime(root_discovery.entries);
    entry_points.extend_support(root_discovery.support_entries);
    for (path, count) in root_discovery.skipped_entries {
        *skipped_entries.entry(path).or_insert(0) += count;
    }
    let mut ws_entries = Vec::new();
    let mut ws_support_entries = Vec::new();
    for workspace in workspace_discovery {
        ws_entries.extend(workspace.entries);
        ws_support_entries.extend(workspace.support_entries);
        for (path, count) in workspace.skipped_entries {
            *skipped_entries.entry(path).or_insert(0) += count;
        }
    }
    discover::warn_skipped_entry_summary(&skipped_entries);
    entry_points.extend_runtime(ws_entries);
    entry_points.extend_support(ws_support_entries);
    spans.workspaces_ms = split_ms(&mut mark);

    let plugin_entries = discover::discover_plugin_entry_point_sets_timed(
        input.plugin_result,
        input.config,
        input.files,
    );
    spans.plugin_glob_build_ms = plugin_entries.build_ms;
    spans.plugin_glob_match_ms = plugin_entries.match_ms;
    entry_points.extend(plugin_entries.entries);
    spans.plugins_ms = split_ms(&mut mark);

    let infra_entries = discover::discover_infrastructure_entry_points(&input.config.root);
    entry_points.extend_runtime(infra_entries);
    spans.infrastructure_ms = split_ms(&mut mark);

    if !input.config.dynamically_loaded.is_empty() {
        let dynamic_entries =
            discover::discover_dynamically_loaded_entry_points(input.config, input.files);
        entry_points.extend_runtime(dynamic_entries);
    }
    spans.dynamic_ms = split_ms(&mut mark);

    let deduped = entry_points.dedup();
    spans.dedup_ms = split_ms(&mut mark);
    (deduped, spans)
}

/// Elapsed milliseconds since `mark`, resetting `mark` to now.
///
/// Consecutive calls carve one stage into adjacent spans with no gap between
/// them, so the spans sum to the stage they subdivide.
fn split_ms(mark: &mut Instant) -> f64 {
    let now = Instant::now();
    let elapsed = now.duration_since(*mark).as_secs_f64() * 1000.0;
    *mark = now;
    elapsed
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
    workspace_pkgs: &[LoadedWorkspacePackage],
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
    workspace_pkgs: &[LoadedWorkspacePackage],
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
        record_plugin_config_diagnostics(&result, &config.root);
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

    share_auto_imports_with_package_layer_apps(&mut result, config, workspaces);
    gate_auto_import_entry_patterns(&mut result, config, workspaces);
    record_plugin_config_diagnostics(&result, &config.root);

    Ok(result)
}

/// Publish the plugin stage's advisories: one registry write per analysis, after
/// the workspace merge and the auto-import gate, which is the single point where
/// every plugin result has converged on the project root.
///
/// Always called, including with nothing to publish, because the write REPLACES
/// the previous run's set: a config the user fixed drops out on the next run
/// rather than persisting through a watch-mode rerun or a long-lived session.
///
/// Plugin config parsing is not cached (each analysis re-reads every config file
/// from disk and feeds only the plugin config hash), so a warm graph cache
/// carries these entries exactly like a cold one.
fn record_plugin_config_diagnostics(result: &plugins::AggregatedPluginResult, root: &Path) {
    let diagnostics = result
        .config_diagnostics
        .iter()
        .cloned()
        .map(|diagnostic| diagnostic.into_workspace_diagnostic(root))
        .collect();
    let _ = fallow_config::record_plugin_config_diagnostics(root, diagnostics);
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
    workspace_pkgs: &[LoadedWorkspacePackage],
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
            Some(Ok((ws_result, workspace_prefix(&config.root, &ws.root))))
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

/// The project-relative prefix of a workspace root, empty for the project root
/// itself. A root outside the project tree keeps its own path, which is also the
/// prefix its patterns carry.
fn workspace_prefix(root: &Path, workspace_root: &Path) -> String {
    workspace_root
        .strip_prefix(root)
        .unwrap_or(workspace_root)
        .to_string_lossy()
        .into_owned()
}

/// Make the auto-imports of a workspace that holds a Nuxt layer visible to each
/// app that names the workspace package in `extends`.
///
/// Each plugin run scopes its rules to its own root and to the local layers it
/// reads from disk. A layer that an app extends by package name is a workspace
/// root of its own, so only this step, which knows the workspace names, can
/// link the two roots. A chain of package layers shares names along the whole
/// chain. See issue #2752.
fn share_auto_imports_with_package_layer_apps(
    result: &mut plugins::AggregatedPluginResult,
    config: &ResolvedConfig,
    workspaces: &[fallow_config::WorkspaceInfo],
) {
    if result.auto_imports.is_empty() || !result.active_plugins.iter().any(|name| name == "nuxt") {
        return;
    }
    let roots_by_name: rustc_hash::FxHashMap<&str, &Path> = workspaces
        .iter()
        .map(|ws| (ws.name.as_str(), ws.root.as_path()))
        .collect();
    let app_roots =
        std::iter::once(config.root.as_path()).chain(workspaces.iter().map(|ws| ws.root.as_path()));
    let mut links: Vec<(&Path, &Path)> = Vec::new();
    for app in app_roots {
        for name in plugins::nuxt::package_layer_names(app) {
            if let Some(layer) = roots_by_name.get(name.as_str())
                && *layer != app
            {
                links.push((layer, app));
            }
        }
    }
    if links.is_empty() {
        return;
    }
    let mut changed = true;
    while changed {
        changed = false;
        for rule in &mut result.auto_imports {
            for (layer, app) in &links {
                if rule.scope.iter().any(|root| root == layer)
                    && !rule.scope.iter().any(|root| root == app)
                {
                    rule.scope.push(app.to_path_buf());
                    changed = true;
                }
            }
        }
    }
}

/// When `autoImports` is enabled, drop the modeled Nuxt convention entry
/// patterns so genuinely-unreferenced convention files are reported as
/// `unused-file`. Component and script fallbacks are classified separately
/// because `components:` and `imports:` settings affect different convention
/// surfaces. A surface whose settings are not modeled keeps its patterns; a
/// config that statically proves the surface scans no more than the modeled
/// defaults is treated like the default and loses them.
///
/// Each root is classified on its own: the project root plus every workspace
/// root, and a pattern is judged by the config of the root whose prefix it
/// carries. One custom `nuxt.config` in a monorepo therefore no longer keeps
/// every other app's patterns. See issue #2737.
///
/// A surface that KEPT its patterns records one advisory per root and surface,
/// because the user enabled `autoImports` and did not get the findings it
/// promises on that surface, and nothing else said so. Only a surface whose
/// patterns were actually retained is recorded: a root whose `nuxt.config` fallow
/// models has nothing to report. See issue #2736.
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
    let root_settings = plugins::nuxt::auto_import_settings(&config.root);
    let workspace_settings: Vec<_> = workspaces
        .iter()
        .map(|ws| {
            (
                workspace_prefix(&config.root, &ws.root),
                plugins::nuxt::auto_import_settings(&ws.root),
            )
        })
        .collect();
    let mut retained: Vec<plugins::PluginConfigDiagnostic> = Vec::new();
    result.entry_patterns.retain(|(rule, plugin)| {
        if plugin != "nuxt" {
            return true;
        }
        let setting =
            settings_for_entry_pattern(&root_settings, &workspace_settings, &rule.pattern);
        if plugins::nuxt::is_component_entry_pattern(&rule.pattern) {
            if !setting.components.is_custom() {
                return false;
            }
            record_retained_auto_import_surface(
                &mut retained,
                &setting.components_origin,
                "components",
            );
            return true;
        }
        if plugins::nuxt::is_script_auto_import_entry_pattern(&rule.pattern) {
            if !setting.scripts.is_custom() {
                return false;
            }
            record_retained_auto_import_surface(&mut retained, &setting.scripts_origin, "imports");
            return true;
        }
        true
    });
    result.config_diagnostics.extend(retained);
}

/// The reason token for a surface that kept its patterns: a config file with a
/// property this reader cannot resolve statically needs the property fixed
/// before any surface in it can be classified, so it is named separately from a
/// surface whose own value is the thing fallow does not model.
const AUTO_IMPORT_PROPERTY_UNREADABLE: &str = "config-property-unreadable";
const AUTO_IMPORT_KEY_NOT_MODELED: &str = "key-effect-not-modeled";

/// Record one advisory per `nuxt.config` and surface, however many patterns that
/// surface kept.
fn record_retained_auto_import_surface(
    retained: &mut Vec<plugins::PluginConfigDiagnostic>,
    origin: &plugins::nuxt::SurfaceOrigin,
    key: &str,
) {
    let Some(config_path) = origin.config_path.as_deref() else {
        return;
    };
    let reason = if origin.unreadable_property {
        AUTO_IMPORT_PROPERTY_UNREADABLE
    } else {
        AUTO_IMPORT_KEY_NOT_MODELED
    };
    let diagnostic = plugins::PluginConfigDiagnostic::not_modeled(config_path, "nuxt", key, reason);
    if !retained.contains(&diagnostic) {
        retained.push(diagnostic);
    }
}

/// Pick the settings of the root that owns an entry pattern: the workspace with
/// the longest matching prefix, on a `{prefix}/` boundary so `packages/web` does
/// not capture `packages/web-admin`. A pattern under no workspace prefix, such as
/// the project root's own `app/components/**`, is the project root's own.
fn settings_for_entry_pattern<'a>(
    root: &'a plugins::nuxt::AutoImportSettings,
    workspaces: &'a [(String, plugins::nuxt::AutoImportSettings)],
    pattern: &str,
) -> &'a plugins::nuxt::AutoImportSettings {
    workspaces
        .iter()
        .filter(|(prefix, _)| {
            !prefix.is_empty()
                && pattern
                    .strip_prefix(prefix.as_str())
                    .is_some_and(|rest| rest.starts_with('/'))
        })
        .max_by_key(|(prefix, _)| prefix.len())
        .map_or(root, |(_, setting)| setting)
}

fn bucket_files_by_workspace(
    workspace_pkgs: &[LoadedWorkspacePackage],
    file_paths: &[std::path::PathBuf],
) -> Vec<Vec<(std::path::PathBuf, String)>> {
    let workspace_roots: Vec<_> = workspace_pkgs
        .iter()
        .map(|(workspace, _)| workspace.root.as_path())
        .collect();
    bucket_files_by_workspace_roots(&workspace_roots, file_paths)
}

fn bucket_files_by_workspace_roots(
    workspace_roots: &[&Path],
    file_paths: &[std::path::PathBuf],
) -> Vec<Vec<(std::path::PathBuf, String)>> {
    use rayon::prelude::*;

    // A file may match nested or duplicate workspace roots. Keep the original
    // first-declaration-wins contract by storing the first index for each root
    // and selecting the lowest index among the file's matching ancestors.
    let mut workspace_by_root: rustc_hash::FxHashMap<&Path, usize> =
        rustc_hash::FxHashMap::default();
    for (idx, root) in workspace_roots.iter().enumerate() {
        workspace_by_root.entry(root).or_insert(idx);
    }

    let assignments: Vec<Option<(usize, std::path::PathBuf, String)>> = file_paths
        .par_iter()
        .map(|file_path| {
            let idx = file_path
                .ancestors()
                .filter_map(|ancestor| workspace_by_root.get(ancestor).copied())
                .min()?;
            let relative = file_path.strip_prefix(workspace_roots[idx]).ok()?;
            Some((
                idx,
                file_path.clone(),
                relative.to_string_lossy().into_owned(),
            ))
        })
        .collect();

    let mut buckets = vec![Vec::new(); workspace_roots.len()];
    for (idx, file_path, relative) in assignments.into_iter().flatten() {
        buckets[idx].push((file_path, relative));
    }

    buckets
}

/// Benchmark hook for workspace file assignment. This is not a supported API.
#[doc(hidden)]
pub fn benchmark_bucket_files_by_workspace(
    workspace_roots: &[std::path::PathBuf],
    file_paths: &[std::path::PathBuf],
) -> Vec<Vec<(std::path::PathBuf, String)>> {
    let workspace_roots: Vec<_> = workspace_roots
        .iter()
        .map(std::path::PathBuf::as_path)
        .collect();
    bucket_files_by_workspace_roots(&workspace_roots, file_paths)
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

/// Resolve the analysis config for a project, mirroring the CLI's `--config`
/// behavior when `config_path` is provided.
///
/// # Errors
///
/// Returns an error when an explicit config cannot be loaded or automatic
/// config discovery finds an invalid config.
fn config_for_project(
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
    let packs = fallow_config::load_rule_packs(root, &config.rule_packs).map_err(|errors| {
        let joined = errors
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n  - ");
        FallowError::config(format!("invalid rule pack:\n  - {joined}"))
    })?;
    let zone_errors = fallow_config::validate_rule_pack_zones(
        root,
        &config.boundaries,
        &config.rule_packs,
        &packs,
    );
    if !zone_errors.is_empty() {
        let joined = zone_errors
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join("\n  - ");
        return Err(FallowError::config(format!(
            "invalid rule pack:\n  - {joined}"
        )));
    }
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
#[cfg_attr(
    not(test),
    allow(
        dead_code,
        reason = "config resolution fallback is exercised by session tests"
    )
)]
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
        AnalysisSession, bucket_files_by_workspace, bucket_files_by_workspace_roots,
        collect_config_search_roots, credit_workspace_package_usage, default_config,
        format_undeclared_workspace_warning, gate_auto_import_entry_patterns,
        parse_analysis_modules, plugin_config_hash, resolver_options_hash,
        settings_for_entry_pattern, warn_undeclared_workspaces, workspace_prefix,
    };
    use std::path::{Path, PathBuf};
    use std::time::Instant;

    use fallow_config::{
        AutoImportKind, AutoImportRule, WorkspaceDiagnostic, WorkspaceDiagnosticKind,
    };
    use fallow_types::discover::{DiscoveredFile, FileId};
    use fallow_types::extract::{ImportInfo, ImportedName};

    fn plugin_result() -> crate::plugins::AggregatedPluginResult {
        let mut result = crate::plugins::AggregatedPluginResult::default();
        result.active_plugins.push("nuxt".to_string());
        result
            .path_aliases
            .push(("@/".to_string(), "src/".to_string()));
        result
    }

    fn auto_import_settings(
        components: crate::plugins::nuxt::AutoImportSetting,
    ) -> crate::plugins::nuxt::AutoImportSettings {
        crate::plugins::nuxt::AutoImportSettings {
            components,
            scripts: crate::plugins::nuxt::AutoImportSetting::Default,
            components_origin: crate::plugins::nuxt::SurfaceOrigin {
                config_path: Some(std::path::PathBuf::from("nuxt.config.ts")),
                unreadable_property: false,
            },
            scripts_origin: crate::plugins::nuxt::SurfaceOrigin::default(),
        }
    }

    #[test]
    fn entry_pattern_settings_prefer_the_longest_workspace_prefix() {
        let root = auto_import_settings(crate::plugins::nuxt::AutoImportSetting::Default);
        let workspaces = vec![
            (
                "packages/web".to_string(),
                auto_import_settings(crate::plugins::nuxt::AutoImportSetting::Custom),
            ),
            (
                "packages/web-admin".to_string(),
                auto_import_settings(crate::plugins::nuxt::AutoImportSetting::Disabled),
            ),
        ];

        let admin = settings_for_entry_pattern(
            &root,
            &workspaces,
            "packages/web-admin/components/**/*.{vue,ts,tsx,js,jsx}",
        );
        assert_eq!(
            admin.components,
            crate::plugins::nuxt::AutoImportSetting::Disabled,
            "a sibling whose name starts with another workspace name is its own"
        );

        let web = settings_for_entry_pattern(
            &root,
            &workspaces,
            "packages/web/components/**/*.{vue,ts,tsx,js,jsx}",
        );
        assert_eq!(
            web.components,
            crate::plugins::nuxt::AutoImportSetting::Custom
        );
    }

    #[test]
    fn entry_pattern_without_a_workspace_prefix_belongs_to_the_project_root() {
        let root = auto_import_settings(crate::plugins::nuxt::AutoImportSetting::Custom);
        let workspaces = vec![(
            "packages/web".to_string(),
            auto_import_settings(crate::plugins::nuxt::AutoImportSetting::Default),
        )];

        let setting = settings_for_entry_pattern(
            &root,
            &workspaces,
            "app/components/**/*.{vue,ts,tsx,js,jsx}",
        );
        assert_eq!(
            setting.components,
            crate::plugins::nuxt::AutoImportSetting::Custom
        );
    }

    /// Build a project root holding one `nuxt.config.ts` and a plugin result
    /// carrying the two gated convention entry patterns.
    fn nuxt_gate_fixture(
        config_source: &str,
    ) -> (tempfile::TempDir, crate::plugins::AggregatedPluginResult) {
        let dir = tempfile::tempdir().expect("temp project");
        std::fs::write(dir.path().join("nuxt.config.ts"), config_source).expect("nuxt config");
        let mut result = crate::plugins::AggregatedPluginResult::default();
        result.active_plugins.push("nuxt".to_string());
        for pattern in [
            "app/components/**/*.{vue,ts,tsx,js,jsx}",
            "app/composables/*.{ts,tsx,js,jsx,mts,cts,mjs,cjs}",
        ] {
            result.entry_patterns.push((
                crate::plugins::PathRule::new(pattern.to_string()),
                "nuxt".to_string(),
            ));
        }
        (dir, result)
    }

    fn gate(root: &Path, result: &mut crate::plugins::AggregatedPluginResult) {
        let config = fallow_config::FallowConfig {
            auto_imports: true,
            ..fallow_config::FallowConfig::default()
        };
        let resolved = config.resolve(
            root.to_path_buf(),
            fallow_config::OutputFormat::Json,
            1,
            false,
            true,
            None,
        );
        assert!(resolved.auto_imports, "the gate only runs when opted in");
        gate_auto_import_entry_patterns(result, &resolved, &[]);
    }

    /// A surface that kept its patterns records one advisory naming the config
    /// file, the surface and why, because the user asked for the findings that
    /// surface no longer produces (issue #2736).
    #[test]
    fn a_retained_auto_import_surface_records_one_advisory_per_surface() {
        let (project, mut result) =
            nuxt_gate_fixture("export default { components: { dirs: ['~/ui'] } };\n");
        gate(project.path(), &mut result);

        let recorded: Vec<(&str, &str, &str)> = result
            .config_diagnostics
            .iter()
            .map(|diagnostic| {
                (
                    diagnostic.plugin.as_str(),
                    diagnostic.key.as_str(),
                    diagnostic.reason.as_str(),
                )
            })
            .collect();
        assert_eq!(
            recorded,
            vec![("nuxt", "components", "key-effect-not-modeled")],
            "only the surface that kept its patterns is reported: {:?}",
            result.config_diagnostics
        );
        assert_eq!(
            result.config_diagnostics[0].config_path,
            project.path().join("nuxt.config.ts")
        );
        assert_eq!(
            result.entry_patterns.len(),
            1,
            "the modeled surface still loses its patterns: {:?}",
            result.entry_patterns
        );
    }

    /// A top-level property this reader cannot resolve stands both surfaces
    /// down, and the remedy is the property rather than either surface, so it
    /// carries its own reason token.
    #[test]
    fn an_unreadable_top_level_property_reports_both_surfaces_with_its_own_reason() {
        let (project, mut result) =
            nuxt_gate_fixture("export default { ...baseConfig, modules: [] };\n");
        gate(project.path(), &mut result);

        let recorded: Vec<(&str, &str)> = result
            .config_diagnostics
            .iter()
            .map(|diagnostic| (diagnostic.key.as_str(), diagnostic.reason.as_str()))
            .collect();
        assert_eq!(
            recorded,
            vec![
                ("components", "config-property-unreadable"),
                ("imports", "config-property-unreadable"),
            ],
            "{:?}",
            result.config_diagnostics
        );
        assert_eq!(
            result.entry_patterns.len(),
            2,
            "a spread keeps every gated pattern"
        );
    }

    /// A config fallow models fully loses its patterns and reports nothing, so
    /// the advisory fires only where a finding was actually suppressed.
    #[test]
    fn a_modeled_nuxt_config_records_no_advisory() {
        let (project, mut result) = nuxt_gate_fixture("export default { modules: [] };\n");
        gate(project.path(), &mut result);
        assert!(
            result.config_diagnostics.is_empty(),
            "{:?}",
            result.config_diagnostics
        );
        assert!(
            result.entry_patterns.is_empty(),
            "both modeled surfaces lose their patterns: {:?}",
            result.entry_patterns
        );
    }

    #[test]
    fn a_workspace_outside_the_project_keeps_its_own_prefix() {
        let project_root = Path::new("/repo");
        let outside = Path::new("/elsewhere/app");
        let prefix = workspace_prefix(project_root, outside);
        assert_eq!(prefix, "/elsewhere/app");

        let root = auto_import_settings(crate::plugins::nuxt::AutoImportSetting::Default);
        let workspaces = vec![(
            prefix,
            auto_import_settings(crate::plugins::nuxt::AutoImportSetting::Custom),
        )];
        let setting = settings_for_entry_pattern(
            &root,
            &workspaces,
            "/elsewhere/app/components/**/*.{vue,ts,tsx,js,jsx}",
        );
        assert_eq!(
            setting.components,
            crate::plugins::nuxt::AutoImportSetting::Custom,
            "an out-of-tree workspace is classified on its own config"
        );
    }

    #[test]
    fn commonjs_internal_import_credits_workspace_package_usage() {
        let workspace = fallow_config::WorkspaceInfo {
            root: PathBuf::from("/repo/packages/shared"),
            name: "@repo/shared".to_string(),
            is_internal_dependency: true,
        };
        let resolved = vec![crate::resolve::ResolvedModule {
            file_id: FileId(0),
            resolved_imports: vec![crate::resolve::ResolvedImport {
                info: ImportInfo {
                    source: "@repo/shared".to_string(),
                    imported_name: ImportedName::Namespace,
                    local_name: "shared".to_string(),
                    is_type_only: false,
                    is_type_only_star: false,
                    from_style: false,
                    span: oxc_span::Span::new(0, 20),
                    source_span: oxc_span::Span::new(8, 20),
                },
                target: crate::resolve::ResolveResult::CommonJsInternalModule(FileId(1)),
            }],
            ..crate::resolve::ResolvedModule::default()
        }];
        let mut graph = crate::graph::ModuleGraph::build(&[], &[], &[]);

        credit_workspace_package_usage(&mut graph, &resolved, &[workspace]);

        assert_eq!(
            graph.package_usage.get("@repo/shared"),
            Some(&vec![FileId(0)])
        );
    }

    /// Root identity is checked by the manifest. The resolver options hash
    /// describes configuration independently of where the project resides.
    #[test]
    fn graph_cache_resolver_hash_is_independent_of_the_project_root() {
        let dir_a = tempfile::tempdir().expect("create temp dir a");
        let dir_b = tempfile::tempdir().expect("create temp dir b");
        let config_a = session_config(dir_a.path());
        let config_b = session_config(dir_b.path());

        assert_eq!(
            resolver_options_hash(&config_a),
            resolver_options_hash(&config_b),
            "root identity is handled separately from resolver options"
        );
    }

    /// A changed file set invalidates a graph even when its root and resolver
    /// options are unchanged.
    #[test]
    fn graph_cache_manifest_still_rejects_a_different_file_set() {
        let dir_a = tempfile::tempdir().expect("create temp dir a");
        let mode = crate::graph_cache::GraphCacheMode::new(1, 2, 3);
        let files_a = [crate::discover::DiscoveredFile {
            id: crate::discover::FileId(0),
            path: dir_a.path().join("src/a.ts"),
            size_bytes: 1,
        }];
        let files_b = [crate::discover::DiscoveredFile {
            id: crate::discover::FileId(0),
            path: dir_a.path().join("src/b.ts"),
            size_bytes: 1,
        }];

        let manifest_a = crate::graph_cache::GraphCacheManifest::from_discovered_files(
            dir_a.path(),
            &files_a,
            mode,
            |_| 7,
        );
        let manifest_b = crate::graph_cache::GraphCacheManifest::from_discovered_files(
            dir_a.path(),
            &files_b,
            mode,
            |_| 7,
        );

        assert_eq!(
            manifest_a.classify_resolution_mismatch(&manifest_b),
            Some(fallow_types::cache_rejection::CacheRejection::FileSetChanged)
        );
    }

    #[test]
    fn graph_cache_resolver_hash_includes_resolve_conditions() {
        let dir = tempfile::tempdir().expect("create temp dir");
        let config_a = session_config(dir.path());
        let mut config_b = session_config(dir.path());
        config_b.resolve.conditions.push("react-server".to_string());

        assert_ne!(
            resolver_options_hash(&config_a),
            resolver_options_hash(&config_b),
            "resolve condition changes must invalidate the graph cache"
        );
    }

    #[test]
    fn graph_cache_plugin_hash_includes_auto_imports() {
        let mut without_auto_import = plugin_result();
        let mut with_auto_import = plugin_result();
        with_auto_import.auto_imports.push(AutoImportRule::new(
            "useCounter".to_string(),
            PathBuf::from("/project/composables/useCounter.ts"),
            AutoImportKind::Named,
        ));

        assert_ne!(
            plugin_config_hash(&without_auto_import, std::path::Path::new("")),
            plugin_config_hash(&with_auto_import, std::path::Path::new("")),
            "auto-import edge changes must invalidate the graph cache"
        );

        without_auto_import.auto_imports.push(AutoImportRule::new(
            "useCounter".to_string(),
            PathBuf::from("/project/composables/useCounter.ts"),
            AutoImportKind::Default,
        ));
        assert_ne!(
            plugin_config_hash(&without_auto_import, std::path::Path::new("")),
            plugin_config_hash(&with_auto_import, std::path::Path::new("")),
            "auto-import kind changes must invalidate the graph cache"
        );

        let mut scoped = with_auto_import.auto_imports.clone();
        scoped[0].scope = vec![PathBuf::from("/project/packages/a")];
        let mut with_scoped_auto_import = plugin_result();
        with_scoped_auto_import.auto_imports = scoped;
        assert_ne!(
            plugin_config_hash(&with_scoped_auto_import, std::path::Path::new("")),
            plugin_config_hash(&with_auto_import, std::path::Path::new("")),
            "auto-import scope changes must invalidate the graph cache"
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
            plugin_config_hash(&base, std::path::Path::new("")),
            plugin_config_hash(&with_scss, std::path::Path::new("")),
            "SCSS include path changes must invalidate the graph cache"
        );

        let mut with_static_dir = base.clone();
        with_static_dir
            .static_dir_mappings
            .push((PathBuf::from("/project/public"), "/".to_string()));
        assert_ne!(
            plugin_config_hash(&base, std::path::Path::new("")),
            plugin_config_hash(&with_static_dir, std::path::Path::new("")),
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

        let session = AnalysisSession::new(&config).expect("session setup should succeed");

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
    fn direct_core_parse_surfaces_source_read_failure_diagnostic() {
        let project = tempfile::tempdir().expect("create project");
        let root = project.path();
        let paths = ["a.ts", "b.ts", "c.ts"].map(|name| root.join(name));
        for (index, path) in paths.iter().enumerate() {
            std::fs::write(path, format!("export const value{index} = {index};\n"))
                .expect("write source");
        }
        let files: Vec<DiscoveredFile> = paths
            .iter()
            .enumerate()
            .map(|(index, path)| DiscoveredFile {
                id: FileId(u32::try_from(index).expect("test index fits u32")),
                path: path.clone(),
                size_bytes: std::fs::metadata(path).expect("source metadata").len(),
            })
            .collect();
        std::fs::remove_file(&paths[1]).expect("remove source after discovery");
        let config = session_config(root);

        let parsed = parse_analysis_modules(&config, &files, false, Instant::now());

        assert_eq!(
            parsed
                .modules
                .iter()
                .map(|module| module.file_id)
                .collect::<Vec<_>>(),
            vec![FileId(0), FileId(2)]
        );
        let diagnostics = fallow_config::workspace_diagnostics_for(root);
        let diagnostic = diagnostics
            .iter()
            .find(|diagnostic| diagnostic.kind.id() == "source-read-failure")
            .expect("source read failure diagnostic");
        assert_eq!(diagnostic.path, paths[1]);
        assert!(matches!(
            diagnostic.kind,
            WorkspaceDiagnosticKind::SourceReadFailure { .. }
        ));
    }

    #[test]
    fn analysis_session_parses_owned_modules() {
        let dir = tempfile::tempdir().expect("create temp dir");
        write_session_fixture(dir.path());
        let config = session_config(dir.path());

        let session = AnalysisSession::new(&config).expect("session setup should succeed");
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
                ui,
                fallow_config::PackageJson {
                    name: Some("ui".to_string()),
                    ..Default::default()
                },
            ),
            (
                api,
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
    fn workspace_bucketing_preserves_first_declared_match_and_file_order() {
        let root = PathBuf::from("/repo");
        let parent = root.join("apps");
        let child = parent.join("web");
        let nested_first = child.join("src/first.ts");
        let nested_second = child.join("src/second.ts");
        let unmatched = root.join("tools/build.ts");
        let files = vec![nested_first.clone(), unmatched, nested_second.clone()];

        // The relative path preserves the input path's original separators, which
        // are mixed on Windows when the fixture is built via multiple `join` calls
        // (`web\src/first.ts`). Normalize separators before comparing so the
        // assertion checks bucketing + ordering, not host path formatting.
        let normalize = |bucket: &[(PathBuf, String)]| -> Vec<(PathBuf, String)> {
            bucket
                .iter()
                .map(|(path, rel)| (path.clone(), rel.replace('\\', "/")))
                .collect()
        };

        let parent_first = bucket_files_by_workspace_roots(&[&parent, &child, &child], &files);
        assert_eq!(
            normalize(&parent_first[0]),
            vec![
                (nested_first.clone(), "web/src/first.ts".to_string()),
                (nested_second.clone(), "web/src/second.ts".to_string()),
            ]
        );
        assert!(parent_first[1].is_empty());
        assert!(parent_first[2].is_empty());

        let child_first = bucket_files_by_workspace_roots(&[&child, &parent], &files);
        assert_eq!(
            normalize(&child_first[0]),
            vec![
                (nested_first, "src/first.ts".to_string()),
                (nested_second, "src/second.ts".to_string()),
            ]
        );
        assert!(child_first[1].is_empty());
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
