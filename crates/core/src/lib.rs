pub mod analyze;
pub mod cache;
pub mod churn;
pub mod cross_reference;
pub mod discover;
pub mod duplicates;
pub(crate) mod errors;
pub mod extract;
pub mod plugins;
pub(crate) mod progress;
pub mod results;
pub(crate) mod scripts;
pub mod suppress;
pub mod trace;

// Re-export from fallow-graph for backwards compatibility
pub use fallow_graph::graph;
pub use fallow_graph::project;
pub use fallow_graph::resolve;

use std::path::Path;
use std::time::Instant;

use errors::FallowError;
use fallow_config::{
    EntryPointRole, PackageJson, ResolvedConfig, discover_workspaces, find_undeclared_workspaces,
};
use rayon::prelude::*;
use results::AnalysisResults;
use trace::PipelineTimings;

/// Result of the full analysis pipeline, including optional performance timings.
pub struct AnalysisOutput {
    pub results: AnalysisResults,
    pub timings: Option<PipelineTimings>,
    pub graph: Option<graph::ModuleGraph>,
    /// Parsed modules from the pipeline, available when `retain_modules` is true.
    /// Used by the combined command to share a single parse across dead-code and health.
    pub modules: Option<Vec<extract::ModuleInfo>>,
    /// Discovered files from the pipeline, available when `retain_modules` is true.
    pub files: Option<Vec<discover::DiscoveredFile>>,
}

/// Update cache: write freshly parsed modules and refresh stale mtime/size entries.
fn update_cache(
    store: &mut cache::CacheStore,
    modules: &[extract::ModuleInfo],
    files: &[discover::DiscoveredFile],
) {
    for module in modules {
        if let Some(file) = files.get(module.file_id.0 as usize) {
            let (mt, sz) = file_mtime_and_size(&file.path);
            // If content hash matches, just refresh mtime/size if stale (e.g. `touch`ed file)
            if let Some(cached) = store.get_by_path_only(&file.path)
                && cached.content_hash == module.content_hash
            {
                if cached.mtime_secs != mt || cached.file_size != sz {
                    store.insert(&file.path, cache::module_to_cached(module, mt, sz));
                }
                continue;
            }
            store.insert(&file.path, cache::module_to_cached(module, mt, sz));
        }
    }
    store.retain_paths(files);
}

/// Extract mtime (seconds since epoch) and file size from a path.
fn file_mtime_and_size(path: &std::path::Path) -> (u64, u64) {
    std::fs::metadata(path).map_or((0, 0), |m| {
        let mt = m
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::SystemTime::UNIX_EPOCH).ok())
            .map_or(0, |d| d.as_secs());
        (mt, m.len())
    })
}

/// Run the full analysis pipeline.
///
/// # Errors
///
/// Returns an error if file discovery, parsing, or analysis fails.
pub fn analyze(config: &ResolvedConfig) -> Result<AnalysisResults, FallowError> {
    let output = analyze_full(config, false, false, false, false)?;
    Ok(output.results)
}

/// Run the full analysis pipeline with export usage collection (for LSP Code Lens).
///
/// # Errors
///
/// Returns an error if file discovery, parsing, or analysis fails.
pub fn analyze_with_usages(config: &ResolvedConfig) -> Result<AnalysisResults, FallowError> {
    let output = analyze_full(config, false, true, false, false)?;
    Ok(output.results)
}

/// Run the full analysis pipeline with optional performance timings and graph retention.
///
/// # Errors
///
/// Returns an error if file discovery, parsing, or analysis fails.
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
pub fn analyze_retaining_modules(
    config: &ResolvedConfig,
    need_complexity: bool,
    retain_graph: bool,
) -> Result<AnalysisOutput, FallowError> {
    analyze_full(config, retain_graph, false, need_complexity, true)
}

/// Run the analysis pipeline using pre-parsed modules, skipping the parsing stage.
///
/// This avoids re-parsing files when the caller already has a `ParseResult` (e.g., from
/// `fallow_core::extract::parse_all_files`). Discovery, plugins, scripts, entry points,
/// import resolution, graph construction, and dead code detection still run normally.
/// The graph is always retained (needed for file scores).
///
/// # Errors
///
/// Returns an error if discovery, graph construction, or analysis fails.
pub fn analyze_with_parse_result(
    config: &ResolvedConfig,
    modules: &[extract::ModuleInfo],
) -> Result<AnalysisOutput, FallowError> {
    let _span = tracing::info_span!("fallow_analyze_with_parse_result").entered();
    let pipeline_start = Instant::now();

    let show_progress = !config.quiet
        && std::io::IsTerminal::is_terminal(&std::io::stderr())
        && matches!(
            config.output,
            fallow_config::OutputFormat::Human
                | fallow_config::OutputFormat::Compact
                | fallow_config::OutputFormat::Markdown
        );
    let progress = progress::AnalysisProgress::new(show_progress);

    if !config.root.join("node_modules").is_dir() {
        tracing::warn!(
            "node_modules directory not found. Run `npm install` / `pnpm install` first for accurate results."
        );
    }

    // Discover workspaces
    let t = Instant::now();
    let workspaces_vec = discover_workspaces(&config.root);
    let workspaces_ms = t.elapsed().as_secs_f64() * 1000.0;
    if !workspaces_vec.is_empty() {
        tracing::info!(count = workspaces_vec.len(), "workspaces discovered");
    }

    // Warn about directories with package.json not declared as workspaces
    if !config.quiet {
        let undeclared = find_undeclared_workspaces(&config.root, &workspaces_vec);
        for diag in &undeclared {
            tracing::warn!("{}", diag.message);
        }
    }

    // Stage 1: Discover files (cheap — needed for file registry and resolution)
    let t = Instant::now();
    let pb = progress.stage_spinner("Discovering files...");
    let discovered_files = discover::discover_files(config);
    let discover_ms = t.elapsed().as_secs_f64() * 1000.0;
    pb.finish_and_clear();

    let project = project::ProjectState::new(discovered_files, workspaces_vec);
    let files = project.files();
    let workspaces = project.workspaces();

    // Stage 1.5: Run plugin system
    let t = Instant::now();
    let pb = progress.stage_spinner("Detecting plugins...");
    let mut plugin_result = run_plugins(config, files, workspaces);
    let plugins_ms = t.elapsed().as_secs_f64() * 1000.0;
    pb.finish_and_clear();

    // Stage 1.6: Analyze package.json scripts
    let t = Instant::now();
    analyze_all_scripts(config, workspaces, &mut plugin_result);
    let scripts_ms = t.elapsed().as_secs_f64() * 1000.0;

    // Stage 2: SKIPPED — using pre-parsed modules from caller

    // Stage 3: Discover entry points
    let t = Instant::now();
    let entry_points = discover_all_entry_points(config, files, workspaces, &plugin_result);
    let entry_points_ms = t.elapsed().as_secs_f64() * 1000.0;

    // Compute entry-point summary before the graph consumes the entry_points vec
    let ep_summary = summarize_entry_points(&entry_points.all);

    // Stage 4: Resolve imports to file IDs
    let t = Instant::now();
    let pb = progress.stage_spinner("Resolving imports...");
    let resolved = resolve::resolve_all_imports(
        modules,
        files,
        workspaces,
        &plugin_result.active_plugins,
        &plugin_result.path_aliases,
        &plugin_result.scss_include_paths,
        &config.root,
        &config.resolve.conditions,
    );
    let resolve_ms = t.elapsed().as_secs_f64() * 1000.0;
    pb.finish_and_clear();

    // Stage 5: Build module graph
    let t = Instant::now();
    let pb = progress.stage_spinner("Building module graph...");
    let graph = graph::ModuleGraph::build_with_reachability_roots(
        &resolved,
        &entry_points.all,
        &entry_points.runtime,
        &entry_points.test,
        files,
    );
    let graph_ms = t.elapsed().as_secs_f64() * 1000.0;
    pb.finish_and_clear();

    // Stage 6: Analyze for dead code
    let t = Instant::now();
    let pb = progress.stage_spinner("Analyzing...");
    let mut result = analyze::find_dead_code_full(
        &graph,
        config,
        &resolved,
        Some(&plugin_result),
        workspaces,
        modules,
        false,
    );
    let analyze_ms = t.elapsed().as_secs_f64() * 1000.0;
    pb.finish_and_clear();
    progress.finish();

    result.entry_point_summary = Some(ep_summary);

    let total_ms = pipeline_start.elapsed().as_secs_f64() * 1000.0;

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
        discover_ms,
        files.len(),
        workspaces_ms,
        plugins_ms,
        scripts_ms,
        modules.len(),
        entry_points_ms,
        entry_points.all.len(),
        resolve_ms,
        graph_ms,
        analyze_ms,
        total_ms,
    );

    let timings = Some(PipelineTimings {
        discover_files_ms: discover_ms,
        file_count: files.len(),
        workspaces_ms,
        workspace_count: workspaces.len(),
        plugins_ms,
        script_analysis_ms: scripts_ms,
        parse_extract_ms: 0.0, // Skipped — modules were reused
        module_count: modules.len(),
        cache_hits: 0,
        cache_misses: 0,
        cache_update_ms: 0.0,
        entry_points_ms,
        entry_point_count: entry_points.all.len(),
        resolve_imports_ms: resolve_ms,
        build_graph_ms: graph_ms,
        analyze_ms,
        total_ms,
    });

    Ok(AnalysisOutput {
        results: result,
        timings,
        graph: Some(graph),
        modules: None,
        files: None,
    })
}

#[expect(
    clippy::unnecessary_wraps,
    reason = "Result kept for future error handling"
)]
#[expect(
    clippy::too_many_lines,
    reason = "main pipeline function; split candidate for sig-audit-loop"
)]
fn analyze_full(
    config: &ResolvedConfig,
    retain: bool,
    collect_usages: bool,
    need_complexity: bool,
    retain_modules: bool,
) -> Result<AnalysisOutput, FallowError> {
    let _span = tracing::info_span!("fallow_analyze").entered();
    let pipeline_start = Instant::now();

    // Progress bars: enabled when not quiet, stderr is a terminal, and output is human-readable.
    // Structured formats (JSON, SARIF) suppress spinners even on TTY — users piping structured
    // output don't expect progress noise on stderr.
    let show_progress = !config.quiet
        && std::io::IsTerminal::is_terminal(&std::io::stderr())
        && matches!(
            config.output,
            fallow_config::OutputFormat::Human
                | fallow_config::OutputFormat::Compact
                | fallow_config::OutputFormat::Markdown
        );
    let progress = progress::AnalysisProgress::new(show_progress);

    // Warn if node_modules is missing — resolution will be severely degraded
    if !config.root.join("node_modules").is_dir() {
        tracing::warn!(
            "node_modules directory not found. Run `npm install` / `pnpm install` first for accurate results."
        );
    }

    // Discover workspaces if in a monorepo
    let t = Instant::now();
    let workspaces_vec = discover_workspaces(&config.root);
    let workspaces_ms = t.elapsed().as_secs_f64() * 1000.0;
    if !workspaces_vec.is_empty() {
        tracing::info!(count = workspaces_vec.len(), "workspaces discovered");
    }

    // Warn about directories with package.json not declared as workspaces
    if !config.quiet {
        let undeclared = find_undeclared_workspaces(&config.root, &workspaces_vec);
        for diag in &undeclared {
            tracing::warn!("{}", diag.message);
        }
    }

    // Stage 1: Discover all source files
    let t = Instant::now();
    let pb = progress.stage_spinner("Discovering files...");
    let discovered_files = discover::discover_files(config);
    let discover_ms = t.elapsed().as_secs_f64() * 1000.0;
    pb.finish_and_clear();

    // Build ProjectState: owns the file registry with stable FileIds and workspace metadata.
    // This is the foundation for cross-workspace resolution and future incremental analysis.
    let project = project::ProjectState::new(discovered_files, workspaces_vec);
    let files = project.files();
    let workspaces = project.workspaces();

    // Stage 1.5: Run plugin system — parse config files, discover dynamic entries
    let t = Instant::now();
    let pb = progress.stage_spinner("Detecting plugins...");
    let mut plugin_result = run_plugins(config, files, workspaces);
    let plugins_ms = t.elapsed().as_secs_f64() * 1000.0;
    pb.finish_and_clear();

    // Stage 1.6: Analyze package.json scripts for binary usage and config file refs
    let t = Instant::now();
    analyze_all_scripts(config, workspaces, &mut plugin_result);
    let scripts_ms = t.elapsed().as_secs_f64() * 1000.0;

    // Stage 2: Parse all files in parallel and extract imports/exports
    let t = Instant::now();
    let pb = progress.stage_spinner(&format!("Parsing {} files...", files.len()));
    let mut cache_store = if config.no_cache {
        None
    } else {
        cache::CacheStore::load(&config.cache_dir)
    };

    let parse_result = extract::parse_all_files(files, cache_store.as_ref(), need_complexity);
    let modules = parse_result.modules;
    let cache_hits = parse_result.cache_hits;
    let cache_misses = parse_result.cache_misses;
    let parse_ms = t.elapsed().as_secs_f64() * 1000.0;
    pb.finish_and_clear();

    // Update cache with freshly parsed modules and refresh stale mtime/size entries.
    let t = Instant::now();
    if !config.no_cache {
        let store = cache_store.get_or_insert_with(cache::CacheStore::new);
        update_cache(store, &modules, files);
        if let Err(e) = store.save(&config.cache_dir) {
            tracing::warn!("Failed to save cache: {e}");
        }
    }
    let cache_ms = t.elapsed().as_secs_f64() * 1000.0;

    // Stage 3: Discover entry points (static patterns + plugin-discovered patterns)
    let t = Instant::now();
    let entry_points = discover_all_entry_points(config, files, workspaces, &plugin_result);
    let entry_points_ms = t.elapsed().as_secs_f64() * 1000.0;

    // Stage 4: Resolve imports to file IDs
    let t = Instant::now();
    let pb = progress.stage_spinner("Resolving imports...");
    let resolved = resolve::resolve_all_imports(
        &modules,
        files,
        workspaces,
        &plugin_result.active_plugins,
        &plugin_result.path_aliases,
        &plugin_result.scss_include_paths,
        &config.root,
        &config.resolve.conditions,
    );
    let resolve_ms = t.elapsed().as_secs_f64() * 1000.0;
    pb.finish_and_clear();

    // Stage 5: Build module graph
    let t = Instant::now();
    let pb = progress.stage_spinner("Building module graph...");
    let graph = graph::ModuleGraph::build_with_reachability_roots(
        &resolved,
        &entry_points.all,
        &entry_points.runtime,
        &entry_points.test,
        files,
    );
    let graph_ms = t.elapsed().as_secs_f64() * 1000.0;
    pb.finish_and_clear();

    // Compute entry-point summary before the graph consumes the entry_points vec
    let ep_summary = summarize_entry_points(&entry_points.all);

    // Stage 6: Analyze for dead code (with plugin context and workspace info)
    let t = Instant::now();
    let pb = progress.stage_spinner("Analyzing...");
    let mut result = analyze::find_dead_code_full(
        &graph,
        config,
        &resolved,
        Some(&plugin_result),
        workspaces,
        &modules,
        collect_usages,
    );
    let analyze_ms = t.elapsed().as_secs_f64() * 1000.0;
    pb.finish_and_clear();
    progress.finish();

    result.entry_point_summary = Some(ep_summary);

    let total_ms = pipeline_start.elapsed().as_secs_f64() * 1000.0;

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
        files.len(),
        workspaces_ms,
        plugins_ms,
        scripts_ms,
        parse_ms,
        modules.len(),
        cache_summary,
        cache_ms,
        entry_points_ms,
        entry_points.all.len(),
        resolve_ms,
        graph_ms,
        analyze_ms,
        total_ms,
    );

    let timings = if retain {
        Some(PipelineTimings {
            discover_files_ms: discover_ms,
            file_count: files.len(),
            workspaces_ms,
            workspace_count: workspaces.len(),
            plugins_ms,
            script_analysis_ms: scripts_ms,
            parse_extract_ms: parse_ms,
            module_count: modules.len(),
            cache_hits,
            cache_misses,
            cache_update_ms: cache_ms,
            entry_points_ms,
            entry_point_count: entry_points.all.len(),
            resolve_imports_ms: resolve_ms,
            build_graph_ms: graph_ms,
            analyze_ms,
            total_ms,
        })
    } else {
        None
    };

    Ok(AnalysisOutput {
        results: result,
        timings,
        graph: if retain { Some(graph) } else { None },
        modules: if retain_modules { Some(modules) } else { None },
        files: if retain_modules {
            Some(files.to_vec())
        } else {
            None
        },
    })
}

/// Analyze package.json scripts from root and all workspace packages.
///
/// Populates the plugin result with script-used packages and config file
/// entry patterns. Also scans CI config files for binary invocations.
fn analyze_all_scripts(
    config: &ResolvedConfig,
    workspaces: &[fallow_config::WorkspaceInfo],
    plugin_result: &mut plugins::AggregatedPluginResult,
) {
    // Load all package.jsons once: root + workspaces. Each is reused for both
    // dep name collection (bin map) and script analysis (no double I/O).
    let pkg_path = config.root.join("package.json");
    let root_pkg = PackageJson::load(&pkg_path).ok();

    let ws_pkgs: Vec<_> = workspaces
        .iter()
        .filter_map(|ws| {
            PackageJson::load(&ws.root.join("package.json"))
                .ok()
                .map(|pkg| (ws, pkg))
        })
        .collect();

    // Collect all dependency names to build the bin-name → package-name reverse map.
    // This resolves binaries like "attw" to "@arethetypeswrong/cli" even without
    // node_modules/.bin symlinks.
    let mut all_dep_names: Vec<String> = Vec::new();
    if let Some(ref pkg) = root_pkg {
        all_dep_names.extend(pkg.all_dependency_names());
    }
    for (_, ws_pkg) in &ws_pkgs {
        all_dep_names.extend(ws_pkg.all_dependency_names());
    }
    all_dep_names.sort_unstable();
    all_dep_names.dedup();

    // Probe node_modules/ at project root and each workspace root so non-hoisted
    // deps (pnpm strict, Yarn workspaces) are also discovered.
    let mut nm_roots: Vec<&std::path::Path> = vec![&config.root];
    for ws in workspaces {
        nm_roots.push(&ws.root);
    }
    let bin_map = scripts::build_bin_to_package_map(&nm_roots, &all_dep_names);

    if let Some(ref pkg) = root_pkg
        && let Some(ref pkg_scripts) = pkg.scripts
    {
        let scripts_to_analyze = if config.production {
            scripts::filter_production_scripts(pkg_scripts)
        } else {
            pkg_scripts.clone()
        };
        let script_analysis = scripts::analyze_scripts(&scripts_to_analyze, &config.root, &bin_map);
        plugin_result.script_used_packages = script_analysis.used_packages;

        for config_file in &script_analysis.config_files {
            plugin_result
                .discovered_always_used
                .push((config_file.clone(), "scripts".to_string()));
        }
    }
    for (ws, ws_pkg) in &ws_pkgs {
        if let Some(ref ws_scripts) = ws_pkg.scripts {
            let scripts_to_analyze = if config.production {
                scripts::filter_production_scripts(ws_scripts)
            } else {
                ws_scripts.clone()
            };
            let ws_analysis = scripts::analyze_scripts(&scripts_to_analyze, &ws.root, &bin_map);
            plugin_result
                .script_used_packages
                .extend(ws_analysis.used_packages);

            let ws_prefix = ws
                .root
                .strip_prefix(&config.root)
                .unwrap_or(&ws.root)
                .to_string_lossy();
            for config_file in &ws_analysis.config_files {
                plugin_result
                    .discovered_always_used
                    .push((format!("{ws_prefix}/{config_file}"), "scripts".to_string()));
            }
        }
    }

    // Scan CI config files for binary invocations
    let ci_packages = scripts::ci::analyze_ci_files(&config.root, &bin_map);
    plugin_result.script_used_packages.extend(ci_packages);
    plugin_result
        .entry_point_roles
        .entry("scripts".to_string())
        .or_insert(EntryPointRole::Support);
}

/// Discover all entry points from static patterns, workspaces, plugins, and infrastructure.
fn discover_all_entry_points(
    config: &ResolvedConfig,
    files: &[discover::DiscoveredFile],
    workspaces: &[fallow_config::WorkspaceInfo],
    plugin_result: &plugins::AggregatedPluginResult,
) -> discover::CategorizedEntryPoints {
    let mut entry_points = discover::CategorizedEntryPoints::default();
    entry_points.extend_runtime(discover::discover_entry_points(config, files));

    let ws_entries: Vec<_> = workspaces
        .par_iter()
        .flat_map(|ws| discover::discover_workspace_entry_points(&ws.root, config, files))
        .collect();
    entry_points.extend_runtime(ws_entries);

    let plugin_entries = discover::discover_plugin_entry_point_sets(plugin_result, config, files);
    entry_points.extend(plugin_entries);

    let infra_entries = discover::discover_infrastructure_entry_points(&config.root);
    entry_points.extend_runtime(infra_entries);

    // Add dynamically loaded files from config as entry points
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

/// Run plugins for root project and all workspace packages.
fn run_plugins(
    config: &ResolvedConfig,
    files: &[discover::DiscoveredFile],
    workspaces: &[fallow_config::WorkspaceInfo],
) -> plugins::AggregatedPluginResult {
    let registry = plugins::PluginRegistry::new(config.external_plugins.clone());
    let file_paths: Vec<std::path::PathBuf> = files.iter().map(|f| f.path.clone()).collect();

    // Run plugins for root project (full run with external plugins, inline config, etc.)
    let pkg_path = config.root.join("package.json");
    let mut result = PackageJson::load(&pkg_path).map_or_else(
        |_| plugins::AggregatedPluginResult::default(),
        |pkg| registry.run(&pkg, &config.root, &file_paths),
    );

    if workspaces.is_empty() {
        return result;
    }

    // Pre-compile config matchers and relative files once for all workspace runs.
    // This avoids re-compiling glob patterns and re-computing relative paths per workspace
    // (previously O(workspaces × plugins × files) glob compilations).
    let precompiled_matchers = registry.precompile_config_matchers();
    let relative_files: Vec<(&std::path::PathBuf, String)> = file_paths
        .iter()
        .map(|f| {
            let rel = f
                .strip_prefix(&config.root)
                .unwrap_or(f)
                .to_string_lossy()
                .into_owned();
            (f, rel)
        })
        .collect();

    // Run plugins for each workspace package in parallel, then merge results.
    let ws_results: Vec<_> = workspaces
        .par_iter()
        .filter_map(|ws| {
            let ws_pkg_path = ws.root.join("package.json");
            let ws_pkg = PackageJson::load(&ws_pkg_path).ok()?;
            let ws_result = registry.run_workspace_fast(
                &ws_pkg,
                &ws.root,
                &config.root,
                &precompiled_matchers,
                &relative_files,
            );
            if ws_result.active_plugins.is_empty() {
                return None;
            }
            let ws_prefix = ws
                .root
                .strip_prefix(&config.root)
                .unwrap_or(&ws.root)
                .to_string_lossy()
                .into_owned();
            Some((ws_result, ws_prefix))
        })
        .collect();

    // Merge workspace results sequentially (deterministic order via par_iter index stability)
    // Track seen names for O(1) dedup instead of O(n) Vec::contains
    let mut seen_plugins: rustc_hash::FxHashSet<String> =
        result.active_plugins.iter().cloned().collect();
    let mut seen_prefixes: rustc_hash::FxHashSet<String> =
        result.virtual_module_prefixes.iter().cloned().collect();
    let mut seen_generated: rustc_hash::FxHashSet<String> =
        result.generated_import_patterns.iter().cloned().collect();
    for (ws_result, ws_prefix) in ws_results {
        // Prefix helper: workspace-relative patterns need the workspace prefix
        // to be matchable from the monorepo root. But patterns that are already
        // project-root-relative (e.g., from angular.json which uses absolute paths
        // like "apps/client/src/styles.css") should not be double-prefixed.
        let prefix_if_needed = |pat: &str| -> String {
            if pat.starts_with(ws_prefix.as_str()) || pat.starts_with('/') {
                pat.to_string()
            } else {
                format!("{ws_prefix}/{pat}")
            }
        };

        for (rule, pname) in &ws_result.entry_patterns {
            result
                .entry_patterns
                .push((rule.prefixed(&ws_prefix), pname.clone()));
        }
        for (plugin_name, role) in ws_result.entry_point_roles {
            result.entry_point_roles.entry(plugin_name).or_insert(role);
        }
        for (pat, pname) in &ws_result.always_used {
            result
                .always_used
                .push((prefix_if_needed(pat), pname.clone()));
        }
        for (pat, pname) in &ws_result.discovered_always_used {
            result
                .discovered_always_used
                .push((prefix_if_needed(pat), pname.clone()));
        }
        for (pat, pname) in &ws_result.fixture_patterns {
            result
                .fixture_patterns
                .push((prefix_if_needed(pat), pname.clone()));
        }
        for rule in &ws_result.used_exports {
            result.used_exports.push(rule.prefixed(&ws_prefix));
        }
        // Merge active plugin names (deduplicated via HashSet)
        for plugin_name in ws_result.active_plugins {
            if !seen_plugins.contains(&plugin_name) {
                seen_plugins.insert(plugin_name.clone());
                result.active_plugins.push(plugin_name);
            }
        }
        // These don't need prefixing (absolute paths / package names)
        result
            .referenced_dependencies
            .extend(ws_result.referenced_dependencies);
        result.setup_files.extend(ws_result.setup_files);
        result
            .tooling_dependencies
            .extend(ws_result.tooling_dependencies);
        // Virtual module prefixes (e.g., Docusaurus @theme/, @site/) are
        // package-name prefixes, not file paths — no workspace prefix needed.
        for prefix in ws_result.virtual_module_prefixes {
            if !seen_prefixes.contains(&prefix) {
                seen_prefixes.insert(prefix.clone());
                result.virtual_module_prefixes.push(prefix);
            }
        }
        // Generated import patterns (e.g., SvelteKit /$types) are suffix
        // matches on specifiers, not file paths — no workspace prefix needed.
        for pattern in ws_result.generated_import_patterns {
            if !seen_generated.contains(&pattern) {
                seen_generated.insert(pattern.clone());
                result.generated_import_patterns.push(pattern);
            }
        }
        // Path aliases from workspace plugins (e.g., SvelteKit $lib/ → src/lib).
        // Prefix the replacement directory so it resolves from the monorepo root.
        for (prefix, replacement) in ws_result.path_aliases {
            result
                .path_aliases
                .push((prefix, format!("{ws_prefix}/{replacement}")));
        }
    }

    result
}

/// Run analysis on a project directory (with export usages for LSP Code Lens).
///
/// # Errors
///
/// Returns an error if config loading, file discovery, parsing, or analysis fails.
pub fn analyze_project(root: &Path) -> Result<AnalysisResults, FallowError> {
    let config = default_config(root);
    analyze_with_usages(&config)
}

/// Create a default config for a project root.
pub(crate) fn default_config(root: &Path) -> ResolvedConfig {
    let user_config = fallow_config::FallowConfig::find_and_load(root)
        .ok()
        .flatten();
    match user_config {
        Some((config, _path)) => config.resolve(
            root.to_path_buf(),
            fallow_config::OutputFormat::Human,
            num_cpus(),
            false,
            true, // quiet: LSP/programmatic callers don't need progress bars
        ),
        None => fallow_config::FallowConfig::default().resolve(
            root.to_path_buf(),
            fallow_config::OutputFormat::Human,
            num_cpus(),
            false,
            true,
        ),
    }
}

fn num_cpus() -> usize {
    std::thread::available_parallelism().map_or(4, std::num::NonZeroUsize::get)
}
