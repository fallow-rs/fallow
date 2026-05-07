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
    /// Package names invoked from package.json scripts and CI configs, mirroring
    /// what the unused-deps detector consults. Populated for every pipeline run;
    /// trace tooling reads it so `trace_dependency` agrees with `unused-deps` on
    /// "used vs unused" instead of returning false-negatives for script-only deps.
    pub script_used_packages: rustc_hash::FxHashSet<String>,
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
    if quiet {
        return;
    }

    let undeclared = find_undeclared_workspaces_with_ignores(root, workspaces_vec, ignore_patterns);
    if let Some(message) = format_undeclared_workspace_warning(root, &undeclared) {
        tracing::warn!("{message}");
    }
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
#[allow(
    clippy::too_many_lines,
    reason = "pipeline orchestration stays easier to audit in one place"
)]
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
    warn_undeclared_workspaces(
        &config.root,
        &workspaces_vec,
        &config.ignore_patterns,
        config.quiet,
    );

    // Stage 1: Discover files (cheap — needed for file registry and resolution)
    let t = Instant::now();
    let pb = progress.stage_spinner("Discovering files...");
    let discovered_files = discover::discover_files(config);
    let discover_ms = t.elapsed().as_secs_f64() * 1000.0;
    pb.finish_and_clear();

    let project = project::ProjectState::new(discovered_files, workspaces_vec);
    let files = project.files();
    let workspaces = project.workspaces();
    let root_pkg = load_root_package_json(config);
    let workspace_pkgs = load_workspace_packages(workspaces);

    // Stage 1.5: Run plugin system
    let t = Instant::now();
    let pb = progress.stage_spinner("Detecting plugins...");
    let mut plugin_result = run_plugins(
        config,
        files,
        workspaces,
        root_pkg.as_ref(),
        &workspace_pkgs,
    );
    let plugins_ms = t.elapsed().as_secs_f64() * 1000.0;
    pb.finish_and_clear();

    // Stage 1.6: Analyze package.json scripts
    let t = Instant::now();
    analyze_all_scripts(
        config,
        workspaces,
        root_pkg.as_ref(),
        &workspace_pkgs,
        &mut plugin_result,
    );
    let scripts_ms = t.elapsed().as_secs_f64() * 1000.0;

    // Stage 2: SKIPPED — using pre-parsed modules from caller

    // Stage 3: Discover entry points
    let t = Instant::now();
    let entry_points = discover_all_entry_points(
        config,
        files,
        workspaces,
        root_pkg.as_ref(),
        &workspace_pkgs,
        &plugin_result,
    );
    let entry_points_ms = t.elapsed().as_secs_f64() * 1000.0;

    // Compute entry-point summary before the graph consumes the entry_points vec
    let ep_summary = summarize_entry_points(&entry_points.all);

    // Stage 4: Resolve imports to file IDs
    let t = Instant::now();
    let pb = progress.stage_spinner("Resolving imports...");
    let mut resolved = resolve::resolve_all_imports(
        modules,
        files,
        workspaces,
        &plugin_result.active_plugins,
        &plugin_result.path_aliases,
        &plugin_result.scss_include_paths,
        &config.root,
        &config.resolve.conditions,
    );
    external_style_usage::augment_external_style_package_usage(
        &mut resolved,
        config,
        workspaces,
        &plugin_result,
    );
    let resolve_ms = t.elapsed().as_secs_f64() * 1000.0;
    pb.finish_and_clear();

    // Stage 5: Build module graph
    let t = Instant::now();
    let pb = progress.stage_spinner("Building module graph...");
    let mut graph = graph::ModuleGraph::build_with_reachability_roots(
        &resolved,
        &entry_points.all,
        &entry_points.runtime,
        &entry_points.test,
        files,
    );
    credit_workspace_package_usage(&mut graph, &resolved, workspaces);
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
        duplication_ms: None,
        total_ms,
    });

    Ok(AnalysisOutput {
        results: result,
        timings,
        graph: Some(graph),
        modules: None,
        files: None,
        script_used_packages: plugin_result.script_used_packages.clone(),
    })
}

#[expect(
    clippy::unnecessary_wraps,
    reason = "Result kept for future error handling"
)]
#[expect(
    clippy::too_many_lines,
    reason = "main pipeline function; sequential phases are held together for clarity"
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
    warn_undeclared_workspaces(
        &config.root,
        &workspaces_vec,
        &config.ignore_patterns,
        config.quiet,
    );

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
    let root_pkg = load_root_package_json(config);
    let workspace_pkgs = load_workspace_packages(workspaces);

    // Stage 1.5: Run plugin system — parse config files, discover dynamic entries
    let t = Instant::now();
    let pb = progress.stage_spinner("Detecting plugins...");
    let mut plugin_result = run_plugins(
        config,
        files,
        workspaces,
        root_pkg.as_ref(),
        &workspace_pkgs,
    );
    let plugins_ms = t.elapsed().as_secs_f64() * 1000.0;
    pb.finish_and_clear();

    // Stage 1.6: Analyze package.json scripts for binary usage and config file refs
    let t = Instant::now();
    analyze_all_scripts(
        config,
        workspaces,
        root_pkg.as_ref(),
        &workspace_pkgs,
        &mut plugin_result,
    );
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
    let entry_points = discover_all_entry_points(
        config,
        files,
        workspaces,
        root_pkg.as_ref(),
        &workspace_pkgs,
        &plugin_result,
    );
    let entry_points_ms = t.elapsed().as_secs_f64() * 1000.0;

    // Stage 4: Resolve imports to file IDs
    let t = Instant::now();
    let pb = progress.stage_spinner("Resolving imports...");
    let mut resolved = resolve::resolve_all_imports(
        &modules,
        files,
        workspaces,
        &plugin_result.active_plugins,
        &plugin_result.path_aliases,
        &plugin_result.scss_include_paths,
        &config.root,
        &config.resolve.conditions,
    );
    external_style_usage::augment_external_style_package_usage(
        &mut resolved,
        config,
        workspaces,
        &plugin_result,
    );
    let resolve_ms = t.elapsed().as_secs_f64() * 1000.0;
    pb.finish_and_clear();

    // Stage 5: Build module graph
    let t = Instant::now();
    let pb = progress.stage_spinner("Building module graph...");
    let mut graph = graph::ModuleGraph::build_with_reachability_roots(
        &resolved,
        &entry_points.all,
        &entry_points.runtime,
        &entry_points.test,
        files,
    );
    credit_workspace_package_usage(&mut graph, &resolved, workspaces);
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
            duplication_ms: None,
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
        script_used_packages: plugin_result.script_used_packages,
    })
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
    // Collect all dependency names to build the bin-name → package-name reverse map.
    // This resolves binaries like "attw" to "@arethetypeswrong/cli" even without
    // node_modules/.bin symlinks.
    let mut all_dep_names: Vec<String> = Vec::new();
    if let Some(pkg) = root_pkg {
        all_dep_names.extend(pkg.all_dependency_names());
    }
    for (_, ws_pkg) in workspace_pkgs {
        all_dep_names.extend(ws_pkg.all_dependency_names());
    }
    all_dep_names.sort_unstable();
    all_dep_names.dedup();

    // Probe node_modules/ at project root and each workspace root so non-hoisted
    // deps (pnpm strict, Yarn workspaces) are also discovered.
    let mut nm_roots: Vec<&std::path::Path> = Vec::new();
    if config.root.join("node_modules").is_dir() {
        nm_roots.push(&config.root);
    }
    for ws in workspaces {
        if ws.root.join("node_modules").is_dir() {
            nm_roots.push(&ws.root);
        }
    }
    let bin_map = scripts::build_bin_to_package_map(&nm_roots, &all_dep_names);

    if let Some(pkg) = root_pkg
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
        for entry in &script_analysis.entry_files {
            if let Some(pat) = scripts::normalize_script_entry_pattern("", entry) {
                plugin_result
                    .entry_patterns
                    .push((plugins::PathRule::new(pat), "scripts".to_string()));
            }
        }
    }
    for (ws, ws_pkg) in workspace_pkgs {
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
            for entry in &ws_analysis.entry_files {
                if let Some(pat) = scripts::normalize_script_entry_pattern(&ws_prefix, entry) {
                    plugin_result
                        .entry_patterns
                        .push((plugins::PathRule::new(pat), "scripts".to_string()));
                }
            }
        }
    }

    // Scan CI config files for binary invocations and positional file references.
    // Returns both packages used by CI tooling AND project-relative file paths
    // referenced as command-line arguments (e.g., `node scripts/deploy.ts` in a
    // GitHub Actions `run:` block) so the referenced files become reachable
    // entry points. CI files always live at the project root, so file paths
    // need no workspace-prefix transformation. See issue #195 (Case D).
    let ci_analysis = scripts::ci::analyze_ci_files(&config.root, &bin_map);
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
    root_pkg: Option<&PackageJson>,
    workspace_pkgs: &[LoadedWorkspacePackage<'_>],
) -> plugins::AggregatedPluginResult {
    let registry = plugins::PluginRegistry::new(config.external_plugins.clone());
    let file_paths: Vec<std::path::PathBuf> = files.iter().map(|f| f.path.clone()).collect();
    let root_config_search_roots = collect_config_search_roots(&config.root, &file_paths);
    let root_config_search_root_refs: Vec<&Path> = root_config_search_roots
        .iter()
        .map(std::path::PathBuf::as_path)
        .collect();

    // Run plugins for root project (full run with external plugins, inline config, etc.)
    let mut result = root_pkg.map_or_else(plugins::AggregatedPluginResult::default, |pkg| {
        registry.run_with_search_roots(
            pkg,
            &config.root,
            &file_paths,
            &root_config_search_root_refs,
            config.production,
        )
    });

    if workspaces.is_empty() {
        return result;
    }

    let root_active_plugins: rustc_hash::FxHashSet<&str> =
        result.active_plugins.iter().map(String::as_str).collect();

    // Pre-compile config matchers once and bucket source files by workspace.
    // Workspace config matching can then scan only files below that workspace
    // instead of every project file for every active matcher.
    let precompiled_matchers = registry.precompile_config_matchers();
    let workspace_relative_files = bucket_files_by_workspace(workspace_pkgs, &file_paths);

    // Run plugins for each workspace package in parallel, then merge results.
    let ws_results: Vec<_> = workspace_pkgs
        .par_iter()
        .zip(workspace_relative_files.par_iter())
        .filter_map(|((ws, ws_pkg), relative_files)| {
            let ws_result = registry.run_workspace_fast(
                ws_pkg,
                &ws.root,
                &config.root,
                &precompiled_matchers,
                relative_files,
                &root_active_plugins,
                config.production,
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
    let mut seen_suffixes: rustc_hash::FxHashSet<String> =
        result.virtual_package_suffixes.iter().cloned().collect();

    fn extend_unique(
        target: &mut Vec<String>,
        seen: &mut rustc_hash::FxHashSet<String>,
        items: Vec<String>,
    ) {
        for item in items {
            if seen.insert(item.clone()) {
                target.push(item);
            }
        }
    }
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
        // Virtual import boundaries — prefixes (e.g., Docusaurus `@theme/`),
        // generated import patterns (e.g., SvelteKit `/$types`), and package-name
        // suffixes (e.g., Vitest `/__mocks__`) — match against import specifiers
        // or package names, never file paths, so no workspace prefix is applied.
        extend_unique(
            &mut result.virtual_module_prefixes,
            &mut seen_prefixes,
            ws_result.virtual_module_prefixes,
        );
        extend_unique(
            &mut result.generated_import_patterns,
            &mut seen_generated,
            ws_result.generated_import_patterns,
        );
        extend_unique(
            &mut result.virtual_package_suffixes,
            &mut seen_suffixes,
            ws_result.virtual_package_suffixes,
        );
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

fn bucket_files_by_workspace(
    workspace_pkgs: &[LoadedWorkspacePackage<'_>],
    file_paths: &[std::path::PathBuf],
) -> Vec<Vec<(std::path::PathBuf, String)>> {
    let mut buckets = vec![Vec::new(); workspace_pkgs.len()];

    for file_path in file_paths {
        for (idx, (ws, _)) in workspace_pkgs.iter().enumerate() {
            if let Ok(relative) = file_path.strip_prefix(&ws.root) {
                buckets[idx].push((file_path.clone(), relative.to_string_lossy().into_owned()));
                break;
            }
        }
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
pub fn analyze_project(root: &Path) -> Result<AnalysisResults, FallowError> {
    let config = default_config(root);
    analyze_with_usages(&config)
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
    let user_config = fallow_config::FallowConfig::find_and_load(root)
        .ok()
        .flatten();
    match user_config {
        Some((mut config, _path)) => {
            let dead_code_production = config
                .production
                .for_analysis(fallow_config::ProductionAnalysis::DeadCode);
            config.production = dead_code_production.into();
            config.resolve(
                root.to_path_buf(),
                fallow_config::OutputFormat::Human,
                num_cpus(),
                false,
                true, // quiet: LSP/programmatic callers don't need progress bars
            )
        }
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

#[cfg(test)]
mod tests {
    use super::{
        bucket_files_by_workspace, collect_config_search_roots, format_undeclared_workspace_warning,
    };
    use std::path::{Path, PathBuf};

    use fallow_config::WorkspaceDiagnostic;

    fn diag(root: &Path, relative: &str) -> WorkspaceDiagnostic {
        WorkspaceDiagnostic {
            path: root.join(relative),
            message: String::new(),
        }
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
}
