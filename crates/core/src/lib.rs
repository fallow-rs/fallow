//! fallow-core is the internal implementation crate behind the `fallow`
//! analyzer. External embedders should consume the curated programmatic
//! surface at `fallow_cli::programmatic` (e.g. `detect_dead_code`,
//! `detect_boundary_violations`, `detect_duplication`, `compute_complexity`,
//! `compute_health`); each returns a `serde_json::Value` matching the CLI's
//! `--format json` shape plus structured `ProgrammaticError` with the CLI's
//! exit-code ladder. See `decisions/008-fallow-core-internal-policy.md` for
//! the policy, and `docs/fallow-core-migration.md` for the function-by-function
//! migration map. Items in this crate may change in any release, including
//! patch releases; a subsequent minor will flip `publish = false` so the crate
//! is no longer fetchable from crates.io.

// fallow's analysis never executes the analyzed project's code. The only
// external program it spawns is `git`, routed through `crate::spawn`. This deny
// (paired with the `.clippy.toml` ban on `std::process::Command::new`) makes any
// new process spawn on the analysis path a build failure. Test helpers that
// shell out to `git` to build fixtures are exempt via `not(test)`.
#![cfg_attr(not(test), deny(clippy::disallowed_methods))]

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

// Re-export from fallow-graph for backwards compatibility
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
) {
    for module in modules {
        if let Some(file) = files.get(module.file_id.0 as usize) {
            let (mt, sz) = file_mtime_and_size(&file.path);
            // If content hash matches, just refresh mtime/size if stale
            // (e.g. `touch`ed file). Critically, preserve the existing
            // `last_access_secs` instead of rebuilding the entry via
            // `module_to_cached` (which would stamp the current epoch
            // second and defeat the LRU). A metadata-only refresh is NOT
            // a content change, so the entry's recency should not bump.
            if let Some(cached) = store.get_by_path_only(&file.path)
                && cached.content_hash == module.content_hash
            {
                if cached.mtime_secs != mt || cached.file_size != sz {
                    let preserved_last_access = cached.last_access_secs;
                    let mut refreshed = cache::module_to_cached(module, mt, sz);
                    refreshed.last_access_secs = preserved_last_access;
                    store.insert(&file.path, refreshed);
                }
                continue;
            }
            store.insert(&file.path, cache::module_to_cached(module, mt, sz));
        }
    }
    store.retain_paths(files);
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
    let undeclared = find_undeclared_workspaces_with_ignores(root, workspaces_vec, ignore_patterns);
    if undeclared.is_empty() {
        return;
    }

    // Filter out paths that ALREADY carry a config-load-time diagnostic
    // (typically `MalformedPackageJson` from issue #473). A directory whose
    // package.json failed to parse appears "undeclared" from the analyze
    // pipeline's perspective because `discover_workspaces` silently dropped
    // it, but the user IS declaring it; the malformed-package-json warning
    // already names the path and explains the fix, so re-flagging it as
    // "undeclared" actively misleads.
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

    // Fold the surviving undeclared diagnostics into the shared registry so
    // they appear in `workspace_diagnostics[]` on the JSON envelope and in
    // `fallow list --workspaces`. Quiet mode still populates the registry
    // (JSON consumers need the data) but skips the human warning.
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
    note = "fallow_core is internal; use fallow_cli::programmatic::detect_dead_code instead. NOTE: replacement returns serde_json::Value, not typed AnalysisResults. See docs/fallow-core-migration.md and ADR-008."
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
    note = "fallow_core is internal; use fallow_cli::programmatic::detect_dead_code instead. NOTE: export-usage collection is not exposed in the programmatic surface today. See docs/fallow-core-migration.md and ADR-008."
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
#[deprecated(
    since = "2.76.0",
    note = "fallow_core is internal; use fallow_cli::programmatic::detect_dead_code instead. NOTE: trace timings are not exposed in the programmatic surface today; use `fallow check --performance` for CLI-side timings. See docs/fallow-core-migration.md and ADR-008."
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
    note = "fallow_core is internal; the CLI fix command uses this via the workspace path dependency. External embedders should use fallow_cli::programmatic::detect_dead_code. See docs/fallow-core-migration.md and ADR-008."
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
    note = "fallow_core is internal; use fallow_cli::programmatic::detect_dead_code instead. NOTE: combined-mode module retention is not exposed in the programmatic surface today. See docs/fallow-core-migration.md and ADR-008."
)]
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
#[deprecated(
    since = "2.76.0",
    note = "fallow_core is internal; use fallow_cli::programmatic::detect_dead_code instead. NOTE: pre-parsed module reuse is not exposed in the programmatic surface today. See docs/fallow-core-migration.md and ADR-008."
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
    let root_pkg = load_root_package_json(config);
    let discovery_hidden_dir_scopes =
        discover::collect_hidden_dir_scopes(config, root_pkg.as_ref(), &workspaces_vec);

    // Stage 1: Discover files (cheap — needed for file registry and resolution)
    let t = Instant::now();
    progress.set_stage("discovering files...");
    let discovered_files =
        discover::discover_files_with_additional_hidden_dirs(config, &discovery_hidden_dir_scopes);
    let discover_ms = t.elapsed().as_secs_f64() * 1000.0;

    let project = project::ProjectState::new(discovered_files, workspaces_vec);
    let files = project.files();
    let workspaces = project.workspaces();
    let workspace_pkgs = load_workspace_packages(workspaces);

    // Stage 1.5: Run plugin system
    let t = Instant::now();
    progress.set_stage("detecting plugins...");
    let mut plugin_result = run_plugins(
        config,
        files,
        workspaces,
        root_pkg.as_ref(),
        &workspace_pkgs,
    );
    let plugins_ms = t.elapsed().as_secs_f64() * 1000.0;

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
    progress.set_stage("resolving imports...");
    let mut resolved = resolve::resolve_all_imports(
        modules,
        files,
        workspaces,
        &plugin_result.active_plugins,
        &plugin_result.path_aliases,
        &plugin_result.auto_imports,
        &plugin_result.scss_include_paths,
        &plugin_result.static_dir_mappings,
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

    // Stage 5: Build module graph
    let t = Instant::now();
    progress.set_stage("building module graph...");
    let mut graph = graph::ModuleGraph::build_with_reachability_roots(
        &resolved,
        &entry_points.all,
        &entry_points.runtime,
        &entry_points.test,
        files,
    );
    credit_workspace_package_usage(&mut graph, &resolved, workspaces);
    let graph_ms = t.elapsed().as_secs_f64() * 1000.0;

    // Stage 6: Analyze for dead code
    let t = Instant::now();
    progress.set_stage("analyzing...");
    #[expect(
        deprecated,
        reason = "ADR-008 keeps workspace path-dependency calls while warning external fallow-core consumers"
    )]
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
        parse_extract_ms: 0.0, // Skipped: modules were reused
        parse_cpu_ms: 0.0,     // Skipped: modules were reused
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

    let file_hashes: rustc_hash::FxHashMap<std::path::PathBuf, u64> = modules
        .iter()
        .filter_map(|module| {
            files
                .get(module.file_id.0 as usize)
                .map(|file| (file.path.clone(), module.content_hash))
        })
        .collect();

    Ok(AnalysisOutput {
        results: result,
        timings,
        graph: Some(graph),
        modules: None,
        files: None,
        script_used_packages: plugin_result.script_used_packages.clone(),
        file_hashes,
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
    let root_pkg = load_root_package_json(config);
    let discovery_hidden_dir_scopes =
        discover::collect_hidden_dir_scopes(config, root_pkg.as_ref(), &workspaces_vec);

    // Stage 1: Discover all source files
    let t = Instant::now();
    progress.set_stage("discovering files...");
    let discovered_files =
        discover::discover_files_with_additional_hidden_dirs(config, &discovery_hidden_dir_scopes);
    let discover_ms = t.elapsed().as_secs_f64() * 1000.0;

    // Build ProjectState: owns the file registry with stable FileIds and workspace metadata.
    // This is the foundation for cross-workspace resolution and future incremental analysis.
    let project = project::ProjectState::new(discovered_files, workspaces_vec);
    let files = project.files();
    let workspaces = project.workspaces();
    let workspace_pkgs = load_workspace_packages(workspaces);

    // Stage 1.5: Run plugin system — parse config files, discover dynamic entries
    let t = Instant::now();
    progress.set_stage("detecting plugins...");
    let mut plugin_result = run_plugins(
        config,
        files,
        workspaces,
        root_pkg.as_ref(),
        &workspace_pkgs,
    );
    let plugins_ms = t.elapsed().as_secs_f64() * 1000.0;

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
    progress.set_stage(&format!("parsing {} files...", files.len()));
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
    let cache_hits = parse_result.cache_hits;
    let cache_misses = parse_result.cache_misses;
    let parse_cpu_ms = parse_result.parse_cpu_ms;
    let parse_ms = t.elapsed().as_secs_f64() * 1000.0;

    // Update cache with freshly parsed modules and refresh stale mtime/size entries.
    let t = Instant::now();
    if !config.no_cache {
        let store = cache_store.get_or_insert_with(cache::CacheStore::new);
        update_cache(store, &modules, files);
        if let Err(e) = store.save(
            &config.cache_dir,
            config.cache_config_hash,
            cache_max_size_bytes,
        ) {
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
    progress.set_stage("resolving imports...");
    let mut resolved = resolve::resolve_all_imports(
        &modules,
        files,
        workspaces,
        &plugin_result.active_plugins,
        &plugin_result.path_aliases,
        &plugin_result.auto_imports,
        &plugin_result.scss_include_paths,
        &plugin_result.static_dir_mappings,
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

    // Stage 5: Build module graph
    let t = Instant::now();
    progress.set_stage("building module graph...");
    let mut graph = graph::ModuleGraph::build_with_reachability_roots(
        &resolved,
        &entry_points.all,
        &entry_points.runtime,
        &entry_points.test,
        files,
    );
    credit_workspace_package_usage(&mut graph, &resolved, workspaces);
    let graph_ms = t.elapsed().as_secs_f64() * 1000.0;

    // Compute entry-point summary before the graph consumes the entry_points vec
    let ep_summary = summarize_entry_points(&entry_points.all);

    // Stage 6: Analyze for dead code (with plugin context and workspace info)
    let t = Instant::now();
    progress.set_stage("analyzing...");
    #[expect(
        deprecated,
        reason = "ADR-008 keeps workspace path-dependency calls while warning external fallow-core consumers"
    )]
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
            parse_cpu_ms,
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

    let file_hashes: rustc_hash::FxHashMap<std::path::PathBuf, u64> = modules
        .iter()
        .filter_map(|module| {
            files
                .get(module.file_id.0 as usize)
                .map(|file| (file.path.clone(), module.content_hash))
        })
        .collect();

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
        file_hashes,
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
    use rayon::prelude::*;
    type WsScriptOut = (
        Vec<String>,
        Vec<(String, String)>,
        Vec<(plugins::PathRule, String)>,
    );
    let ws_results: Vec<WsScriptOut> = workspace_pkgs
        .par_iter()
        .map(|(ws, ws_pkg)| {
            let mut used_packages = Vec::new();
            let mut discovered_always_used: Vec<(String, String)> = Vec::new();
            let mut entry_patterns: Vec<(plugins::PathRule, String)> = Vec::new();
            if let Some(ref ws_scripts) = ws_pkg.scripts {
                let scripts_to_analyze = if config.production {
                    scripts::filter_production_scripts(ws_scripts)
                } else {
                    ws_scripts.clone()
                };
                let ws_analysis = scripts::analyze_scripts(&scripts_to_analyze, &ws.root, &bin_map);
                used_packages.extend(ws_analysis.used_packages);

                let ws_prefix = ws
                    .root
                    .strip_prefix(&config.root)
                    .unwrap_or(&ws.root)
                    .to_string_lossy();
                for config_file in &ws_analysis.config_files {
                    discovered_always_used
                        .push((format!("{ws_prefix}/{config_file}"), "scripts".to_string()));
                }
                for entry in &ws_analysis.entry_files {
                    if let Some(pat) = scripts::normalize_script_entry_pattern(&ws_prefix, entry) {
                        entry_patterns.push((plugins::PathRule::new(pat), "scripts".to_string()));
                    }
                }
            }
            (used_packages, discovered_always_used, entry_patterns)
        })
        .collect();
    for (used_packages, discovered_always_used, entry_patterns) in ws_results {
        plugin_result.script_used_packages.extend(used_packages);
        plugin_result
            .discovered_always_used
            .extend(discovered_always_used);
        plugin_result.entry_patterns.extend(entry_patterns);
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
    if let Some(pkg) = root_pkg {
        append_package_file_asset_patterns(&mut result, "", pkg);
    }

    if workspaces.is_empty() {
        gate_auto_import_entry_patterns(&mut result, config, workspaces);
        return result;
    }

    append_workspace_package_file_asset_patterns(&mut result, config, workspace_pkgs);

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

    // Merge workspace results sequentially (deterministic order via par_iter
    // index stability). Each result is prefix-transformed for its workspace,
    // then folded into the accumulator via the single field-exhaustive
    // `merge_into` (issue #444): adding a field to `AggregatedPluginResult`
    // becomes a compile error in `merge_into` rather than a silently-dropped
    // field that would diverge the CLI from the LSP.
    for (mut ws_result, ws_prefix) in ws_results {
        ws_result.apply_workspace_prefix(&ws_prefix);
        // Preserve pre-#444 behavior: the old workspace merge loop never
        // folded these fields into the root aggregate. Clearing them on the
        // incoming result keeps the merge byte-identical while `merge_into`
        // stays a prefix-agnostic full union.
        //   - `config_patterns`, `used_class_members`, `scss_include_paths`
        //     ARE populated by `run_workspace_fast` and were dropped; whether
        //     that drop is a latent bug is tracked in issue #772.
        //   - `script_used_packages` is never populated by `run_workspace_fast`
        //     (the root's script-used set is computed separately after this
        //     function returns), so clearing it is a no-op today; it is cleared
        //     anyway so a future change that starts populating it cannot
        //     silently alter root script-credit behavior.
        ws_result.config_patterns.clear();
        ws_result.used_class_members.clear();
        ws_result.scss_include_paths.clear();
        ws_result.script_used_packages.clear();
        result.merge_into(ws_result);
    }

    gate_auto_import_entry_patterns(&mut result, config, workspaces);

    result
}

/// When `autoImports` is enabled, drop the Nuxt component entry patterns so
/// genuinely-unreferenced components are reported as `unused-file` (their
/// reachability now comes from synthesized auto-import edges instead). Guarded by
/// `config_declares_components`: if the root OR any workspace package's
/// `nuxt.config` declares a `components:` key, the patterns are kept, because
/// custom `prefix` / `pathPrefix` / `dirs` are not yet modeled and dropping the
/// protection would risk false positives. Auto-import edge synthesis itself is
/// unconditional; only this entry-pattern removal is flag-gated. See issue #704.
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
    if plugins::nuxt::config_declares_components(&config.root)
        || workspaces
            .iter()
            .any(|ws| plugins::nuxt::config_declares_components(&ws.root))
    {
        return;
    }
    result.entry_patterns.retain(|(rule, plugin)| {
        !(plugin == "nuxt" && plugins::nuxt::is_component_entry_pattern(&rule.pattern))
    });
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
#[deprecated(
    since = "2.76.0",
    note = "fallow_core is internal; use fallow_cli::programmatic::detect_dead_code instead (build a `DeadCodeOptions { analysis: AnalysisOptions { root, ..default() }, ..default() }`). See docs/fallow-core-migration.md and ADR-008."
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
        Some((mut config, path)) => {
            let dead_code_production = config
                .production
                .for_analysis(fallow_config::ProductionAnalysis::DeadCode);
            config.production = dead_code_production.into();
            // Issue #468: validate boundary zone references and root-prefix
            // conflicts BEFORE resolve(). Mirrors the CLI's runtime_support
            // wiring; LSP and programmatic embedders surface the same exit-2
            // diagnostic via FallowError::config so editors / API consumers
            // get a structured failure instead of analysis-time noise.
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
            (
                config.resolve(
                    root.to_path_buf(),
                    fallow_config::OutputFormat::Human,
                    num_cpus(),
                    false,
                    true, // quiet: LSP/programmatic callers don't need progress bars
                    None, // LSP/programmatic embedders use the default cache cap
                ),
                Some(path),
            )
        }
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
        bucket_files_by_workspace, collect_config_search_roots,
        format_undeclared_workspace_warning, warn_undeclared_workspaces,
    };
    use std::path::{Path, PathBuf};

    use fallow_config::{WorkspaceDiagnostic, WorkspaceDiagnosticKind};

    fn diag(root: &Path, relative: &str) -> WorkspaceDiagnostic {
        WorkspaceDiagnostic::new(
            root,
            root.join(relative),
            WorkspaceDiagnosticKind::UndeclaredWorkspace,
        )
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
        // Regression test for the load-bearing dedup in
        // `warn_undeclared_workspaces`: when a declared workspace's
        // package.json is malformed, the discovery pass drops the directory
        // and stashes `MalformedPackageJson` in the registry. The later
        // undeclared-workspace pass would otherwise re-flag the SAME
        // directory as "undeclared" (because it never made it into the
        // `declared` Vec), confusing users who think the workspace is not
        // declared when it actually is, just typo'd. This test asserts the
        // pre-existing MalformedPackageJson entry suppresses the duplicate
        // undeclared warning.
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

        // Run discovery; in production `load_config_for_analysis` stashes
        // the returned diagnostics into the registry, so this test mirrors
        // that pattern by stashing manually.
        let (workspaces, diagnostics) = fallow_config::discover_workspaces_with_diagnostics(
            dir.path(),
            &globset::GlobSet::empty(),
        )
        .expect("root package.json is valid");
        assert_eq!(workspaces.len(), 1, "only the valid workspace discovers");
        fallow_config::stash_workspace_diagnostics(dir.path(), diagnostics);

        // Now run the undeclared pass via the public entry point. The
        // registry should contain the MalformedPackageJson diagnostic but
        // NOT an UndeclaredWorkspace for the same path.
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
