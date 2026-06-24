//! Programmatic runtime entry points that do not depend on `fallow-cli`.

use std::path::{Path, PathBuf};
use std::time::Instant;

use fallow_config::{
    DetectionMode, DuplicatesConfig, OutputFormat, ProductionAnalysis, WorkspaceInfo,
};
use fallow_engine::duplicates::{CloneInstance, DuplicationReport, DuplicationStats};
use fallow_engine::{AnalysisResults, AnalysisSession, ProjectConfig, ProjectConfigOptions};
use fallow_output::{CHECK_SCHEMA_VERSION, CheckOutputInput, build_check_output};
use fallow_types::envelope::{ElapsedMs, SchemaVersion, ToolVersion};
use globset::Glob;
use rustc_hash::{FxHashMap, FxHashSet};
use serde::Serialize;

use crate::{
    AnalysisOptions, DeadCodeFilters, DeadCodeOptions, DupesReportPayload, DuplicationMode,
    DuplicationOptions, ProgrammaticError,
};

const SCHEMA_VERSION: u32 = 1;
const MAX_DIFF_BYTES: u64 = 10 * 1024 * 1024;

type ProgrammaticResult<T> = Result<T, ProgrammaticError>;

#[derive(Debug, Clone, Serialize)]
struct DupesOutput {
    schema_version: SchemaVersion,
    version: ToolVersion,
    elapsed_ms: ElapsedMs,
    #[serde(flatten)]
    report: DupesReportPayload,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    total_issues: Option<usize>,
}

struct ResolvedAnalysisOptions {
    root: PathBuf,
    config_path: Option<PathBuf>,
    no_cache: bool,
    threads: usize,
    pool: rayon::ThreadPool,
    diff: Option<DiffIndex>,
    production_override: Option<bool>,
    changed_since: Option<String>,
    workspace_roots: Option<Vec<PathBuf>>,
    legacy_envelope: bool,
}

/// Run duplication analysis and return the JSON output contract.
///
/// This is the first runtime path owned by `fallow-api` instead of the CLI
/// crate. It intentionally returns the same root JSON shape that embedders
/// already receive from `fallow-node`.
///
/// # Errors
///
/// Returns a structured programmatic error for invalid options, config load
/// failures, git changed-file failures, or serialization failures.
pub fn detect_duplication(options: &DuplicationOptions) -> ProgrammaticResult<serde_json::Value> {
    let resolved = resolve_analysis_options(&options.analysis)?;
    resolved.install(|| detect_duplication_inner(options, &resolved))
}

/// Run dead-code analysis and return the JSON output contract.
///
/// This runtime path is owned by `fallow-api` and uses the typed engine plus
/// output crates directly. NAPI still calls the legacy CLI-backed route until
/// follow-up parity slices move the public binding over.
///
/// # Errors
///
/// Returns a structured programmatic error for unsupported options, invalid
/// options, config load failures, analysis failures, git changed-file failures,
/// or serialization failures.
pub fn detect_dead_code(options: &DeadCodeOptions) -> ProgrammaticResult<serde_json::Value> {
    let resolved = resolve_analysis_options(&options.analysis)?;
    resolved.install(|| detect_dead_code_inner(options, &resolved))
}

fn detect_dead_code_inner(
    options: &DeadCodeOptions,
    resolved: &ResolvedAnalysisOptions,
) -> ProgrammaticResult<serde_json::Value> {
    validate_dead_code_runtime_options(options, resolved)?;
    let start = Instant::now();
    let session = load_dead_code_session(options, resolved)?;
    let analysis = session.analyze_dead_code().map_err(|err| {
        ProgrammaticError::new(format!("dead-code analysis failed: {err}"), 2)
            .with_code("FALLOW_DEAD_CODE_FAILED")
            .with_context("dead-code")
    })?;
    let mut results = analysis.results;

    apply_dead_code_scope(options, resolved, &session, &mut results)?;
    apply_dead_code_filters(&options.filters, &mut results);

    let envelope = build_check_output(CheckOutputInput {
        schema_version: CHECK_SCHEMA_VERSION,
        version: env!("CARGO_PKG_VERSION").to_string(),
        elapsed: start.elapsed(),
        results,
        config_fixable: false,
        workspace_diagnostics: Vec::new(),
        next_steps: Vec::new(),
    });
    let mut output = serde_json::to_value(envelope).map_err(|err| {
        ProgrammaticError::new(format!("failed to serialize dead-code report: {err}"), 2)
            .with_code("FALLOW_SERIALIZE_DEAD_CODE_REPORT")
            .with_context("dead-code")
    })?;
    if let serde_json::Value::Object(map) = &mut output
        && !resolved.legacy_envelope
    {
        map.insert(
            "kind".to_string(),
            serde_json::Value::String("dead_code".to_string()),
        );
    }
    let root_prefix = format!("{}/", session.root().display());
    strip_root_prefix(&mut output, &root_prefix);
    Ok(output)
}

fn validate_dead_code_runtime_options(
    options: &DeadCodeOptions,
    resolved: &ResolvedAnalysisOptions,
) -> ProgrammaticResult<()> {
    if resolved.diff.is_some() {
        return Err(ProgrammaticError::new(
            "`diff_file` is not supported by the API dead-code runtime yet",
            2,
        )
        .with_code("FALLOW_UNSUPPORTED_DEAD_CODE_DIFF_FILE")
        .with_context("analysis.diffFile")
        .with_help("Use `changed_since` or `files` for scoped API dead-code runs"));
    }
    if options.analysis.explain {
        return Err(ProgrammaticError::new(
            "`explain` is not supported by the API dead-code runtime yet",
            2,
        )
        .with_code("FALLOW_UNSUPPORTED_DEAD_CODE_EXPLAIN")
        .with_context("analysis.explain")
        .with_help("Use the CLI dead-code JSON output for `_meta` until the metadata contract moves out of fallow-cli"));
    }
    Ok(())
}

fn load_dead_code_session(
    options: &DeadCodeOptions,
    resolved: &ResolvedAnalysisOptions,
) -> ProgrammaticResult<AnalysisSession> {
    let project_config = fallow_engine::config_for_project_analysis(
        &resolved.root,
        resolved.config_path.as_deref(),
        ProjectConfigOptions {
            output: OutputFormat::Json,
            no_cache: resolved.no_cache,
            threads: resolved.threads,
            production_override: resolved.production_override,
            quiet: true,
            analysis: ProductionAnalysis::DeadCode,
        },
    )
    .map_err(|err| {
        ProgrammaticError::new(format!("failed to load config: {err}"), 2)
            .with_code("FALLOW_CONFIG_LOAD_FAILED")
            .with_context("analysis.configPath")
    })?;
    let project_config = configure_project_for_dead_code(project_config, options);
    Ok(AnalysisSession::from_config(project_config))
}

fn configure_project_for_dead_code(
    mut project_config: ProjectConfig,
    options: &DeadCodeOptions,
) -> ProjectConfig {
    if options.include_entry_exports {
        project_config.config.include_entry_exports = true;
    }
    activate_explicit_dead_code_opt_ins(&options.filters, &mut project_config.config.rules);
    project_config
}

fn activate_explicit_dead_code_opt_ins(
    filters: &DeadCodeFilters,
    rules: &mut fallow_config::RulesConfig,
) {
    if filters.private_type_leaks && rules.private_type_leaks == fallow_config::Severity::Off {
        rules.private_type_leaks = fallow_config::Severity::Warn;
    }
}

fn apply_dead_code_scope(
    options: &DeadCodeOptions,
    resolved: &ResolvedAnalysisOptions,
    session: &AnalysisSession,
    results: &mut AnalysisResults,
) -> ProgrammaticResult<()> {
    if let Some(workspace_roots) = resolved.workspace_roots.as_ref() {
        fallow_engine::dead_code::filter_to_workspaces(results, workspace_roots);
    }
    if let Some(changed_files) = changed_files_for_run(resolved)? {
        fallow_engine::dead_code::filter_by_changed_files(results, &changed_files);
    }
    apply_dead_code_file_filter(options, session.root(), results);
    Ok(())
}

fn apply_dead_code_file_filter(
    options: &DeadCodeOptions,
    root: &Path,
    results: &mut AnalysisResults,
) {
    if options.files.is_empty() {
        return;
    }
    let file_set = options
        .files
        .iter()
        .map(|path| {
            if is_absolute_path_any_platform(path) {
                path.clone()
            } else {
                root.join(path)
            }
        })
        .collect::<FxHashSet<_>>();
    fallow_engine::dead_code::filter_by_changed_files(results, &file_set);
    clear_dead_code_dependency_findings(results);
}

fn apply_dead_code_filters(filters: &DeadCodeFilters, results: &mut AnalysisResults) {
    if !dead_code_filters_active(filters) {
        return;
    }
    apply_dead_code_core_filters(filters, results);
    apply_dead_code_component_filters(filters, results);
    apply_dead_code_graph_filters(filters, results);
    apply_dead_code_policy_filters(filters, results);
    apply_dead_code_catalog_filters(filters, results);
}

fn dead_code_filters_active(filters: &DeadCodeFilters) -> bool {
    filters.unused_files
        || filters.unused_exports
        || filters.unused_deps
        || filters.unused_types
        || filters.private_type_leaks
        || filters.unused_enum_members
        || filters.unused_class_members
        || filters.unused_store_members
        || filters.unprovided_injects
        || filters.unrendered_components
        || filters.unused_component_props
        || filters.unused_component_emits
        || filters.unused_component_inputs
        || filters.unused_component_outputs
        || filters.unused_svelte_events
        || filters.unused_server_actions
        || filters.unused_load_data_keys
        || filters.unresolved_imports
        || filters.unlisted_deps
        || filters.duplicate_exports
        || filters.circular_deps
        || filters.re_export_cycles
        || filters.boundary_violations
        || filters.policy_violations
        || filters.stale_suppressions
        || filters.unused_catalog_entries
        || filters.empty_catalog_groups
        || filters.unresolved_catalog_references
        || filters.unused_dependency_overrides
        || filters.misconfigured_dependency_overrides
}

fn apply_dead_code_core_filters(filters: &DeadCodeFilters, results: &mut AnalysisResults) {
    if !filters.unused_files {
        results.unused_files.clear();
    }
    if !filters.unused_exports {
        results.unused_exports.clear();
    }
    if !filters.unused_types {
        results.unused_types.clear();
    }
    if !filters.private_type_leaks {
        results.private_type_leaks.clear();
    }
    if !filters.unused_deps {
        clear_dead_code_dependency_findings(results);
    }
    if !filters.unused_enum_members {
        results.unused_enum_members.clear();
    }
    if !filters.unused_class_members {
        results.unused_class_members.clear();
    }
    if !filters.unused_store_members {
        results.unused_store_members.clear();
    }
    if !filters.unlisted_deps {
        results.unlisted_dependencies.clear();
    }
}

fn clear_dead_code_dependency_findings(results: &mut AnalysisResults) {
    results.unused_dependencies.clear();
    results.unused_dev_dependencies.clear();
    results.unused_optional_dependencies.clear();
    results.type_only_dependencies.clear();
    results.test_only_dependencies.clear();
}

fn apply_dead_code_component_filters(filters: &DeadCodeFilters, results: &mut AnalysisResults) {
    if !filters.unprovided_injects {
        results.unprovided_injects.clear();
    }
    if !filters.unrendered_components {
        results.unrendered_components.clear();
    }
    if !filters.unused_component_props {
        results.unused_component_props.clear();
    }
    if !filters.unused_component_emits {
        results.unused_component_emits.clear();
    }
    if !filters.unused_component_inputs {
        results.unused_component_inputs.clear();
    }
    if !filters.unused_component_outputs {
        results.unused_component_outputs.clear();
    }
    if !filters.unused_svelte_events {
        results.unused_svelte_events.clear();
    }
    if !filters.unused_server_actions {
        results.unused_server_actions.clear();
    }
    if !filters.unused_load_data_keys {
        results.unused_load_data_keys.clear();
    }
    if !filters.unresolved_imports {
        results.unresolved_imports.clear();
    }
}

fn apply_dead_code_graph_filters(filters: &DeadCodeFilters, results: &mut AnalysisResults) {
    if !filters.duplicate_exports {
        results.duplicate_exports.clear();
    }
    if !filters.circular_deps {
        results.circular_dependencies.clear();
    }
    if !filters.re_export_cycles {
        results.re_export_cycles.clear();
    }
    if !filters.boundary_violations {
        results.boundary_violations.clear();
        results.boundary_coverage_violations.clear();
        results.boundary_call_violations.clear();
    }
}

fn apply_dead_code_policy_filters(filters: &DeadCodeFilters, results: &mut AnalysisResults) {
    if !filters.policy_violations {
        results.policy_violations.clear();
    }
    if !filters.stale_suppressions {
        results.stale_suppressions.clear();
    }
}

fn apply_dead_code_catalog_filters(filters: &DeadCodeFilters, results: &mut AnalysisResults) {
    if !filters.unused_catalog_entries {
        results.unused_catalog_entries.clear();
    }
    if !filters.empty_catalog_groups {
        results.empty_catalog_groups.clear();
    }
    if !filters.unresolved_catalog_references {
        results.unresolved_catalog_references.clear();
    }
    if !filters.unused_dependency_overrides {
        results.unused_dependency_overrides.clear();
    }
    if !filters.misconfigured_dependency_overrides {
        results.misconfigured_dependency_overrides.clear();
    }
}

fn detect_duplication_inner(
    options: &DuplicationOptions,
    resolved: &ResolvedAnalysisOptions,
) -> ProgrammaticResult<serde_json::Value> {
    let start = Instant::now();
    let session = load_duplication_session(options, resolved)?;
    let dupes_config = build_dupes_config(options, &session.config().duplicates);
    let changed_files = changed_files_for_run(resolved)?;
    let cache_dir = (!resolved.no_cache).then_some(session.config().cache_dir.as_path());
    let mut report = if let Some(changed_files) = changed_files.as_ref() {
        let changed_files = changed_files.iter().cloned().collect::<Vec<_>>();
        session
            .find_duplicates_touching_files_with_defaults(&dupes_config, &changed_files, cache_dir)
            .report
    } else {
        session
            .find_duplicates_with_defaults(&dupes_config, cache_dir)
            .report
    };

    if let Some(diff) = resolved.diff.as_ref() {
        filter_by_diff(&mut report, diff, session.root());
    }
    if let Some(workspace_roots) = resolved.workspace_roots.as_ref() {
        filter_by_workspaces(&mut report, workspace_roots, session.root());
    }
    if let Some(top) = options.top {
        apply_top(&mut report, top, session.root());
    }

    let payload = DupesReportPayload::from_report(&report);
    let envelope = DupesOutput {
        schema_version: SchemaVersion(SCHEMA_VERSION),
        version: ToolVersion(env!("CARGO_PKG_VERSION").to_string()),
        elapsed_ms: ElapsedMs(start.elapsed().as_millis() as u64),
        report: payload,
        total_issues: None,
    };
    let mut output = serde_json::to_value(envelope).map_err(|err| {
        ProgrammaticError::new(format!("failed to serialize duplication report: {err}"), 2)
            .with_code("FALLOW_SERIALIZE_DUPLICATION_REPORT")
            .with_context("dupes")
    })?;
    if let serde_json::Value::Object(map) = &mut output
        && !resolved.legacy_envelope
    {
        map.insert(
            "kind".to_string(),
            serde_json::Value::String("dupes".to_string()),
        );
    }
    let root_prefix = format!("{}/", session.root().display());
    strip_root_prefix(&mut output, &root_prefix);
    Ok(output)
}

fn load_duplication_session(
    options: &DuplicationOptions,
    resolved: &ResolvedAnalysisOptions,
) -> ProgrammaticResult<AnalysisSession> {
    let project_config =
        fallow_engine::config_for_project(&resolved.root, resolved.config_path.as_deref())
            .map_err(|err| {
                ProgrammaticError::new(format!("failed to load config: {err}"), 2)
                    .with_code("FALLOW_CONFIG_LOAD_FAILED")
                    .with_context("analysis.configPath")
            })?;
    let project_config = configure_project_for_duplication(project_config, options, resolved);
    Ok(AnalysisSession::from_config(project_config))
}

fn configure_project_for_duplication(
    mut project_config: ProjectConfig,
    options: &DuplicationOptions,
    resolved: &ResolvedAnalysisOptions,
) -> ProjectConfig {
    let production = resolved
        .production_override
        .unwrap_or(project_config.config.production);
    project_config.config.production = production;
    project_config.config.output = OutputFormat::Json;
    project_config.config.threads = resolved.threads;
    project_config.config.no_cache = resolved.no_cache;
    project_config.config.duplicates =
        build_dupes_config(options, &project_config.config.duplicates);
    project_config
}

fn build_dupes_config(options: &DuplicationOptions, config: &DuplicatesConfig) -> DuplicatesConfig {
    DuplicatesConfig {
        enabled: true,
        mode: duplication_mode_to_config(options.mode),
        min_tokens: options.min_tokens,
        min_lines: options.min_lines,
        min_occurrences: options.min_occurrences,
        threshold: options.threshold,
        ignore: config.ignore.clone(),
        ignore_defaults: config.ignore_defaults,
        skip_local: options.skip_local || config.skip_local,
        cross_language: options.cross_language || config.cross_language,
        ignore_imports: options.ignore_imports.unwrap_or(config.ignore_imports),
        normalization: config.normalization.clone(),
        min_corpus_size_for_shingle_filter: config.min_corpus_size_for_shingle_filter,
        min_corpus_size_for_token_cache: config.min_corpus_size_for_token_cache,
    }
}

const fn duplication_mode_to_config(mode: DuplicationMode) -> DetectionMode {
    match mode {
        DuplicationMode::Strict => DetectionMode::Strict,
        DuplicationMode::Mild => DetectionMode::Mild,
        DuplicationMode::Weak => DetectionMode::Weak,
        DuplicationMode::Semantic => DetectionMode::Semantic,
    }
}

fn resolve_analysis_options(
    options: &AnalysisOptions,
) -> ProgrammaticResult<ResolvedAnalysisOptions> {
    validate_analysis_option_shape(options)?;
    let root = resolve_analysis_root(options.root.as_deref())?;
    validate_analysis_config_path(options.config_path.as_deref())?;
    let threads = options.threads.unwrap_or_else(default_threads);
    let pool = rayon::ThreadPoolBuilder::new()
        .num_threads(threads)
        .build()
        .map_err(|err| {
            ProgrammaticError::new(format!("failed to build analysis thread pool: {err}"), 2)
                .with_code("FALLOW_THREAD_POOL_INIT_FAILED")
                .with_context("analysis.threads")
        })?;
    let diff = options
        .diff_file
        .as_deref()
        .map(|path| load_explicit_diff_file(path, &root))
        .transpose()?;
    let workspace_roots = resolve_workspace_scope(
        &root,
        options.workspace.as_deref(),
        options.changed_workspaces.as_deref(),
    )?;
    Ok(ResolvedAnalysisOptions {
        root,
        config_path: options.config_path.clone(),
        no_cache: options.no_cache,
        threads,
        pool,
        diff,
        production_override: options
            .production_override
            .or_else(|| options.production.then_some(true)),
        changed_since: options.changed_since.clone(),
        workspace_roots,
        legacy_envelope: options.legacy_envelope,
    })
}

fn validate_analysis_option_shape(options: &AnalysisOptions) -> ProgrammaticResult<()> {
    if options.threads == Some(0) {
        return Err(
            ProgrammaticError::new("`threads` must be greater than 0", 2)
                .with_code("FALLOW_INVALID_THREADS")
                .with_context("analysis.threads"),
        );
    }
    if options.workspace.is_some() && options.changed_workspaces.is_some() {
        return Err(ProgrammaticError::new(
            "`workspace` and `changed_workspaces` are mutually exclusive",
            2,
        )
        .with_code("FALLOW_MUTUALLY_EXCLUSIVE_SCOPE")
        .with_context("analysis.workspace"));
    }
    Ok(())
}

fn resolve_analysis_root(root: Option<&Path>) -> ProgrammaticResult<PathBuf> {
    let root = match root {
        Some(root) => root.to_path_buf(),
        None => std::env::current_dir().map_err(|err| {
            ProgrammaticError::new(
                format!("failed to resolve current working directory: {err}"),
                2,
            )
            .with_code("FALLOW_CWD_UNAVAILABLE")
            .with_context("analysis.root")
        })?,
    };
    if !root.exists() {
        return Err(ProgrammaticError::new(
            format!("analysis root does not exist: {}", root.display()),
            2,
        )
        .with_code("FALLOW_INVALID_ROOT")
        .with_context("analysis.root"));
    }
    if !root.is_dir() {
        return Err(ProgrammaticError::new(
            format!("analysis root is not a directory: {}", root.display()),
            2,
        )
        .with_code("FALLOW_INVALID_ROOT")
        .with_context("analysis.root"));
    }
    Ok(root)
}

fn validate_analysis_config_path(config_path: Option<&Path>) -> ProgrammaticResult<()> {
    if let Some(config_path) = config_path
        && !config_path.exists()
    {
        return Err(ProgrammaticError::new(
            format!("config file does not exist: {}", config_path.display()),
            2,
        )
        .with_code("FALLOW_INVALID_CONFIG_PATH")
        .with_context("analysis.configPath"));
    }
    Ok(())
}

impl ResolvedAnalysisOptions {
    fn install<R: Send>(&self, f: impl FnOnce() -> R + Send) -> R {
        self.pool.install(f)
    }
}

fn default_threads() -> usize {
    std::thread::available_parallelism().map_or(1, std::num::NonZeroUsize::get)
}

fn load_explicit_diff_file(path: &Path, root: &Path) -> ProgrammaticResult<DiffIndex> {
    if path == Path::new("-") {
        return Err(ProgrammaticError::new(
            "`diff_file` does not support stdin; pass a file path",
            2,
        )
        .with_code("FALLOW_INVALID_DIFF_FILE")
        .with_context("analysis.diffFile"));
    }
    let abs = if is_absolute_path_any_platform(path) {
        path.to_path_buf()
    } else {
        root.join(path)
    };
    let meta = std::fs::metadata(&abs).map_err(|err| {
        ProgrammaticError::new(
            format!(
                "diff file does not exist or cannot be read: {} ({err})",
                abs.display()
            ),
            2,
        )
        .with_code("FALLOW_INVALID_DIFF_FILE")
        .with_context("analysis.diffFile")
    })?;
    if !meta.is_file() {
        return Err(ProgrammaticError::new(
            format!("diff path is not a file: {}", abs.display()),
            2,
        )
        .with_code("FALLOW_INVALID_DIFF_FILE")
        .with_context("analysis.diffFile"));
    }
    if meta.len() > MAX_DIFF_BYTES {
        return Err(ProgrammaticError::new(
            format!(
                "diff file is {} bytes, above the {MAX_DIFF_BYTES} byte limit: {}",
                meta.len(),
                abs.display()
            ),
            2,
        )
        .with_code("FALLOW_INVALID_DIFF_FILE")
        .with_context("analysis.diffFile"));
    }
    let text = std::fs::read_to_string(&abs).map_err(|err| {
        ProgrammaticError::new(
            format!("failed to read diff file {}: {err}", abs.display()),
            2,
        )
        .with_code("FALLOW_INVALID_DIFF_FILE")
        .with_context("analysis.diffFile")
    })?;
    Ok(DiffIndex::from_unified_diff(&text))
}

fn changed_files_for_run(
    resolved: &ResolvedAnalysisOptions,
) -> ProgrammaticResult<Option<FxHashSet<PathBuf>>> {
    let Some(git_ref) = resolved.changed_since.as_deref() else {
        return Ok(None);
    };
    fallow_engine::changed_files(&resolved.root, git_ref)
        .map(Some)
        .map_err(|err| {
            ProgrammaticError::new(
                format!(
                    "failed to resolve changed files for ref `{git_ref}`: {}",
                    err.describe()
                ),
                2,
            )
            .with_code("FALLOW_CHANGED_FILES_FAILED")
            .with_context("analysis.changedSince")
        })
}

fn resolve_workspace_scope(
    root: &Path,
    workspace: Option<&[String]>,
    changed_workspaces: Option<&str>,
) -> ProgrammaticResult<Option<Vec<PathBuf>>> {
    match (workspace, changed_workspaces) {
        (Some(patterns), None) => resolve_workspace_filters(root, patterns).map(Some),
        (None, Some(git_ref)) => resolve_changed_workspaces(root, git_ref).map(Some),
        (None, None) => Ok(None),
        (Some(_), Some(_)) => Err(ProgrammaticError::new(
            "`workspace` and `changed_workspaces` are mutually exclusive",
            2,
        )
        .with_code("FALLOW_MUTUALLY_EXCLUSIVE_SCOPE")
        .with_context("analysis.workspace")),
    }
}

fn resolve_workspace_filters(root: &Path, patterns: &[String]) -> ProgrammaticResult<Vec<PathBuf>> {
    let workspaces = fallow_config::discover_workspaces(root);
    if workspaces.is_empty() {
        let joined = patterns
            .iter()
            .map(|pattern| format!("'{pattern}'"))
            .collect::<Vec<_>>()
            .join(", ");
        return Err(ProgrammaticError::new(
            format!(
                "`workspace` {joined} specified but no workspaces found. Ensure root package.json has a \"workspaces\" field, pnpm-workspace.yaml exists, or tsconfig.json has \"references\"."
            ),
            2,
        )
        .with_code("FALLOW_WORKSPACES_NOT_FOUND")
        .with_context("analysis.workspace"));
    }

    let rel_paths = workspaces
        .iter()
        .map(|workspace| relative_workspace_path(&workspace.root, root))
        .collect::<Vec<_>>();
    let (positive, negative) = split_workspace_patterns(patterns);
    let mut matched = match_positive_workspace_patterns(&positive, &workspaces, &rel_paths)?;

    for pattern in &negative {
        for index in find_workspace_matches(pattern, &workspaces, &rel_paths)? {
            matched.remove(&index);
        }
    }

    if matched.is_empty() {
        return Err(
            ProgrammaticError::new("`workspace` excluded every discovered workspace", 2)
                .with_code("FALLOW_WORKSPACE_SCOPE_EMPTY")
                .with_context("analysis.workspace"),
        );
    }

    let mut roots = matched
        .into_iter()
        .map(|index| workspaces[index].root.clone())
        .collect::<Vec<_>>();
    roots.sort();
    Ok(roots)
}

fn resolve_changed_workspaces(root: &Path, git_ref: &str) -> ProgrammaticResult<Vec<PathBuf>> {
    let workspaces = fallow_config::discover_workspaces(root);
    if workspaces.is_empty() {
        return Err(ProgrammaticError::new(
            format!(
                "`changed_workspaces` '{git_ref}' specified but no workspaces found. Ensure root package.json has a \"workspaces\" field, pnpm-workspace.yaml exists, or tsconfig.json has \"references\"."
            ),
            2,
        )
        .with_code("FALLOW_WORKSPACES_NOT_FOUND")
        .with_context("analysis.changedWorkspaces"));
    }
    let changed_files = fallow_engine::changed_files(root, git_ref).map_err(|err| {
        ProgrammaticError::new(
            format!(
                "failed to resolve changed workspaces for ref `{git_ref}`: {}",
                err.describe()
            ),
            2,
        )
        .with_code("FALLOW_CHANGED_WORKSPACES_FAILED")
        .with_context("analysis.changedWorkspaces")
    })?;
    let mut roots = workspaces
        .into_iter()
        .filter(|workspace| {
            changed_files
                .iter()
                .any(|file| file.starts_with(&workspace.root))
        })
        .map(|workspace| workspace.root)
        .collect::<Vec<_>>();
    roots.sort();
    Ok(roots)
}

fn match_positive_workspace_patterns(
    positive: &[&str],
    workspaces: &[WorkspaceInfo],
    rel_paths: &[String],
) -> ProgrammaticResult<FxHashSet<usize>> {
    let mut matched = FxHashSet::default();
    let mut unmatched = Vec::new();

    if positive.is_empty() {
        matched.extend(0..workspaces.len());
    } else {
        for pattern in positive {
            let hits = find_workspace_matches(pattern, workspaces, rel_paths)?;
            if hits.is_empty() {
                unmatched.push((*pattern).to_string());
            }
            matched.extend(hits);
        }
    }

    if !unmatched.is_empty() {
        return Err(ProgrammaticError::new(
            format!(
                "`workspace` matched no workspace for pattern{}: {}. Available: {}",
                if unmatched.len() == 1 { "" } else { "s" },
                unmatched
                    .iter()
                    .map(|pattern| format!("'{pattern}'"))
                    .collect::<Vec<_>>()
                    .join(", "),
                format_available_workspaces(workspaces),
            ),
            2,
        )
        .with_code("FALLOW_WORKSPACE_PATTERN_UNMATCHED")
        .with_context("analysis.workspace"));
    }

    Ok(matched)
}

fn find_workspace_matches(
    pattern: &str,
    workspaces: &[WorkspaceInfo],
    rel_paths: &[String],
) -> ProgrammaticResult<Vec<usize>> {
    if let Some(index) = workspaces
        .iter()
        .position(|workspace| workspace.name == pattern)
    {
        return Ok(vec![index]);
    }
    if let Some(index) = rel_paths.iter().position(|path| path == pattern) {
        return Ok(vec![index]);
    }

    let glob = Glob::new(pattern).map_err(|err| {
        ProgrammaticError::new(format!("invalid `workspace` pattern '{pattern}': {err}"), 2)
            .with_code("FALLOW_INVALID_WORKSPACE_PATTERN")
            .with_context("analysis.workspace")
    })?;
    let matcher = glob.compile_matcher();
    let hits = workspaces
        .iter()
        .enumerate()
        .filter_map(|(index, workspace)| {
            (matcher.is_match(&workspace.name) || matcher.is_match(&rel_paths[index]))
                .then_some(index)
        })
        .collect();
    Ok(hits)
}

fn split_workspace_patterns(patterns: &[String]) -> (Vec<&str>, Vec<&str>) {
    let mut positive = Vec::new();
    let mut negative = Vec::new();
    for pattern in patterns {
        let trimmed = pattern.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Some(negative_pattern) = trimmed.strip_prefix('!') {
            let negative_pattern = negative_pattern.trim();
            if !negative_pattern.is_empty() {
                negative.push(negative_pattern);
            }
        } else {
            positive.push(trimmed);
        }
    }
    (positive, negative)
}

fn format_available_workspaces(workspaces: &[WorkspaceInfo]) -> String {
    const MAX_SHOWN: usize = 10;
    let total = workspaces.len();
    if total <= MAX_SHOWN {
        return workspaces
            .iter()
            .map(|workspace| workspace.name.as_str())
            .collect::<Vec<_>>()
            .join(", ");
    }
    let shown = workspaces
        .iter()
        .take(MAX_SHOWN)
        .map(|workspace| workspace.name.as_str())
        .collect::<Vec<_>>()
        .join(", ");
    format!(
        "{shown}, ... and {} more ({total} total)",
        total - MAX_SHOWN
    )
}

fn relative_workspace_path(workspace_root: &Path, root: &Path) -> String {
    workspace_root
        .strip_prefix(root)
        .unwrap_or(workspace_root)
        .to_string_lossy()
        .replace('\\', "/")
}

#[derive(Debug, Clone, Default)]
struct DiffIndex {
    added_lines: FxHashMap<String, FxHashSet<u64>>,
}

impl DiffIndex {
    fn from_unified_diff(diff: &str) -> Self {
        let mut index = Self::default();
        let mut current_file: Option<String> = None;
        let mut new_line = 0_u64;
        for line in diff.lines() {
            if let Some(path) = line.strip_prefix("+++ b/") {
                current_file = Some(path.to_string());
                continue;
            }
            if line.starts_with("+++ /dev/null") {
                current_file = None;
                continue;
            }
            if let Some(rest) = line.strip_prefix("@@ ") {
                if let Some(pos) = rest.find(" +") {
                    let new = &rest[(pos + 2)..];
                    let end = new.find([' ', '@']).map_or(new.len(), |end| end);
                    let start = new[..end]
                        .split(',')
                        .next()
                        .and_then(|n| n.parse::<u64>().ok())
                        .unwrap_or(0);
                    new_line = start;
                }
                continue;
            }
            if line.starts_with('+') && !line.starts_with("+++") {
                if let Some(path) = current_file.as_ref() {
                    index
                        .added_lines
                        .entry(path.clone())
                        .or_default()
                        .insert(new_line);
                }
                new_line = new_line.saturating_add(1);
            } else if !line.starts_with('-') {
                new_line = new_line.saturating_add(1);
            }
        }
        index
    }

    fn range_overlaps_added(&self, path: &str, start: u64, end: u64) -> bool {
        self.added_lines
            .get(path)
            .is_some_and(|lines| (start..=end).any(|line| lines.contains(&line)))
    }
}

fn filter_by_diff(report: &mut DuplicationReport, diff_index: &DiffIndex, root: &Path) {
    let instance_overlaps = |instance: &CloneInstance| -> bool {
        let Some(rel) = relative_to_diff_path(&instance.file, root) else {
            return true;
        };
        let start = u64::try_from(instance.start_line).unwrap_or(u64::MAX);
        let end = u64::try_from(instance.end_line).unwrap_or(u64::MAX);
        diff_index.range_overlaps_added(&rel, start, end)
    };
    report
        .clone_groups
        .retain(|g| g.instances.iter().any(instance_overlaps));
    rebuild_duplication_derived_fields(report, root);
}

fn filter_by_workspaces(report: &mut DuplicationReport, workspace_roots: &[PathBuf], root: &Path) {
    report.clone_groups.retain(|group| {
        group.instances.iter().any(|instance| {
            workspace_roots
                .iter()
                .any(|workspace_root| instance.file.starts_with(workspace_root))
        })
    });
    rebuild_duplication_derived_fields(report, root);
}

fn apply_top(report: &mut DuplicationReport, n: usize, root: &Path) {
    report.clone_groups.sort_by(|a, b| {
        b.instances
            .len()
            .cmp(&a.instances.len())
            .then(b.line_count.cmp(&a.line_count))
            .then_with(|| match (a.instances.first(), b.instances.first()) {
                (Some(ai), Some(bi)) => ai
                    .file
                    .cmp(&bi.file)
                    .then(ai.start_line.cmp(&bi.start_line)),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => std::cmp::Ordering::Equal,
            })
    });
    report.clone_groups.truncate(n);
    rebuild_duplication_derived_fields(report, root);
    report.sort();
}

fn rebuild_duplication_derived_fields(report: &mut DuplicationReport, root: &Path) {
    report.clone_families =
        fallow_engine::duplicates::families::group_into_families(&report.clone_groups, root);
    report.mirrored_directories = fallow_engine::duplicates::families::detect_mirrored_directories(
        &report.clone_families,
        root,
    );
    report.stats = recompute_stats(report);
}

fn recompute_stats(report: &DuplicationReport) -> DuplicationStats {
    let mut files_with_clones: FxHashSet<&Path> = FxHashSet::default();
    let mut line_ranges: FxHashMap<&Path, Vec<(usize, usize)>> = FxHashMap::default();
    let mut clone_instances = 0_usize;
    let mut duplicated_tokens = 0_usize;
    for group in &report.clone_groups {
        duplicated_tokens += group.token_count * group.instances.len();
        for instance in &group.instances {
            files_with_clones.insert(&instance.file);
            clone_instances += 1;
            line_ranges
                .entry(&instance.file)
                .or_default()
                .push((instance.start_line, instance.end_line));
        }
    }
    let duplicated_lines = line_ranges
        .into_values()
        .map(count_merged_lines)
        .sum::<usize>();
    let duplication_percentage = if report.stats.total_lines == 0 {
        0.0
    } else {
        (duplicated_lines as f64 / report.stats.total_lines as f64) * 100.0
    };
    DuplicationStats {
        total_files: report.stats.total_files,
        files_with_clones: files_with_clones.len(),
        total_lines: report.stats.total_lines,
        duplicated_lines,
        total_tokens: report.stats.total_tokens,
        duplicated_tokens,
        clone_groups: report.clone_groups.len(),
        clone_instances,
        duplication_percentage,
        clone_groups_below_min_occurrences: report.stats.clone_groups_below_min_occurrences,
    }
}

fn count_merged_lines(mut ranges: Vec<(usize, usize)>) -> usize {
    if ranges.is_empty() {
        return 0;
    }
    ranges.sort_unstable();
    let mut total = 0_usize;
    let mut current = ranges[0];
    for (start, end) in ranges.into_iter().skip(1) {
        if start <= current.1.saturating_add(1) {
            current.1 = current.1.max(end);
        } else {
            total += current.1.saturating_sub(current.0).saturating_add(1);
            current = (start, end);
        }
    }
    total + current.1.saturating_sub(current.0).saturating_add(1)
}

fn relative_to_diff_path(path: &Path, root: &Path) -> Option<String> {
    if let Ok(stripped) = path.strip_prefix(root) {
        return Some(stripped.to_string_lossy().replace('\\', "/"));
    }
    if is_absolute_path_any_platform(path) {
        return None;
    }
    Some(path.to_string_lossy().replace('\\', "/"))
}

fn is_absolute_path_any_platform(path: &Path) -> bool {
    let s = path.as_os_str().to_string_lossy();
    path.is_absolute()
        || s.starts_with('/')
        || s.starts_with("\\\\")
        || s.as_bytes().get(1) == Some(&b':')
}

fn strip_root_prefix(value: &mut serde_json::Value, prefix: &str) {
    match value {
        serde_json::Value::String(s) => {
            if let Some(rest) = s.strip_prefix(prefix) {
                *s = rest.to_string();
            }
        }
        serde_json::Value::Array(items) => {
            for item in items {
                strip_root_prefix(item, prefix);
            }
        }
        serde_json::Value::Object(map) => {
            for value in map.values_mut() {
                strip_root_prefix(value, prefix);
            }
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;

    fn analysis_at(root: &Path) -> AnalysisOptions {
        AnalysisOptions {
            root: Some(root.to_path_buf()),
            ..AnalysisOptions::default()
        }
    }

    #[test]
    fn detect_duplication_returns_dupes_envelope() {
        let project = tempfile::tempdir().expect("temp dir");
        let root = project.path();
        std::fs::create_dir(root.join("src")).expect("src dir");
        let code = "export function repeated() {\n  return ['a', 'b', 'c'].join(',');\n}\n";
        std::fs::write(root.join("src/a.ts"), code).expect("file");
        std::fs::write(root.join("src/b.ts"), code).expect("file");

        let json = detect_duplication(&DuplicationOptions {
            analysis: analysis_at(root),
            min_tokens: 1,
            min_lines: 1,
            ..DuplicationOptions::default()
        })
        .expect("duplication succeeds");

        assert_eq!(json["kind"], "dupes");
        assert!(json["clone_groups"].is_array());
        assert!(json["stats"].is_object());
    }

    #[test]
    fn detect_duplication_legacy_envelope_removes_root_kind() {
        let project = tempfile::tempdir().expect("temp dir");
        let root = project.path();
        std::fs::create_dir(root.join("src")).expect("src dir");
        std::fs::write(root.join("src/a.ts"), "export const a = 1;\n").expect("file");

        let json = detect_duplication(&DuplicationOptions {
            analysis: AnalysisOptions {
                legacy_envelope: true,
                ..analysis_at(root)
            },
            ..DuplicationOptions::default()
        })
        .expect("duplication succeeds");

        assert!(json.get("kind").is_none());
    }

    #[test]
    fn detect_dead_code_returns_dead_code_envelope() {
        let project = dead_code_project();
        let root = project.path();

        let json = detect_dead_code(&DeadCodeOptions {
            analysis: analysis_at(root),
            filters: DeadCodeFilters {
                unused_exports: true,
                ..DeadCodeFilters::default()
            },
            ..DeadCodeOptions::default()
        })
        .expect("dead-code succeeds");

        assert_eq!(json["kind"], "dead_code");
        assert_eq!(json["schema_version"], CHECK_SCHEMA_VERSION);
        assert_eq!(unused_export_names(&json), vec!["deadA", "deadB"]);
    }

    #[test]
    fn detect_dead_code_legacy_envelope_removes_root_kind() {
        let project = dead_code_project();
        let root = project.path();

        let json = detect_dead_code(&DeadCodeOptions {
            analysis: AnalysisOptions {
                legacy_envelope: true,
                ..analysis_at(root)
            },
            filters: DeadCodeFilters {
                unused_exports: true,
                ..DeadCodeFilters::default()
            },
            ..DeadCodeOptions::default()
        })
        .expect("dead-code succeeds");

        assert!(json.get("kind").is_none());
    }

    #[test]
    fn detect_dead_code_file_filter_scopes_source_findings() {
        let project = dead_code_project();
        let root = project.path();

        let json = detect_dead_code(&DeadCodeOptions {
            analysis: analysis_at(root),
            filters: DeadCodeFilters {
                unused_exports: true,
                ..DeadCodeFilters::default()
            },
            files: vec![PathBuf::from("src/a.ts")],
            ..DeadCodeOptions::default()
        })
        .expect("dead-code succeeds");

        assert_eq!(unused_export_names(&json), vec!["deadA"]);
    }

    #[test]
    fn detect_dead_code_rejects_diff_file_until_diff_contract_moves() {
        let project = dead_code_project();
        let root = project.path();
        std::fs::write(
            root.join("scope.diff"),
            "diff --git a/src/a.ts b/src/a.ts\n",
        )
        .expect("diff");

        let err = detect_dead_code(&DeadCodeOptions {
            analysis: AnalysisOptions {
                diff_file: Some(PathBuf::from("scope.diff")),
                ..analysis_at(root)
            },
            ..DeadCodeOptions::default()
        })
        .expect_err("diff file is not supported yet");

        assert_eq!(
            err.code.as_deref(),
            Some("FALLOW_UNSUPPORTED_DEAD_CODE_DIFF_FILE")
        );
        assert_eq!(err.context.as_deref(), Some("analysis.diffFile"));
    }

    #[test]
    fn diff_file_filters_clone_groups() {
        let root = PathBuf::from("/repo");
        let mut report = DuplicationReport {
            clone_groups: vec![
                group(vec![
                    instance("/repo/src/a.ts", 1, 3),
                    instance("/repo/src/b.ts", 1, 3),
                ]),
                group(vec![
                    instance("/repo/src/c.ts", 10, 12),
                    instance("/repo/src/d.ts", 1, 3),
                ]),
            ],
            stats: DuplicationStats {
                total_files: 4,
                total_lines: 100,
                total_tokens: 100,
                clone_groups: 2,
                clone_instances: 4,
                ..DuplicationStats::default()
            },
            ..DuplicationReport::default()
        };
        let diff = DiffIndex::from_unified_diff(
            "diff --git a/src/a.ts b/src/a.ts\n+++ b/src/a.ts\n@@ -1,3 +1,3 @@\n+added\n context\n",
        );

        filter_by_diff(&mut report, &diff, &root);

        assert_eq!(report.clone_groups.len(), 1);
        assert_eq!(
            report.clone_groups[0].instances[0].file,
            root.join("src/a.ts")
        );
    }

    #[test]
    fn workspace_scope_filters_clone_groups() {
        let root = PathBuf::from("/repo");
        let mut report = DuplicationReport {
            clone_groups: vec![
                group(vec![
                    instance("/repo/packages/app/a.ts", 1, 3),
                    instance("/repo/packages/shared/b.ts", 1, 3),
                ]),
                group(vec![
                    instance("/repo/packages/docs/c.ts", 1, 3),
                    instance("/repo/packages/docs/d.ts", 1, 3),
                ]),
            ],
            stats: DuplicationStats {
                total_files: 4,
                total_lines: 100,
                total_tokens: 100,
                clone_groups: 2,
                clone_instances: 4,
                ..DuplicationStats::default()
            },
            ..DuplicationReport::default()
        };

        filter_by_workspaces(&mut report, &[root.join("packages/app")], &root);

        assert_eq!(report.clone_groups.len(), 1);
        assert_eq!(
            report.clone_groups[0].instances[0].file,
            root.join("packages/app/a.ts")
        );
    }

    #[test]
    fn workspace_patterns_match_names_paths_and_negation() {
        let project = tempfile::tempdir().expect("temp dir");
        let root = project.path();
        write_json(
            root.join("package.json"),
            r#"{"workspaces":["packages/*"]}"#,
        );
        write_workspace(root, "packages/app", "@scope/app");
        write_workspace(root, "packages/docs", "docs");

        let roots =
            resolve_workspace_filters(root, &["packages/*".to_string(), "!docs".to_string()])
                .expect("workspace filters resolve");

        assert_eq!(roots, vec![root.join("packages/app")]);
    }

    fn instance(path: &str, start_line: usize, end_line: usize) -> CloneInstance {
        CloneInstance {
            file: PathBuf::from(path),
            start_line,
            end_line,
            start_col: 0,
            end_col: 0,
            fragment: String::new(),
        }
    }

    fn group(instances: Vec<CloneInstance>) -> fallow_engine::duplicates::CloneGroup {
        fallow_engine::duplicates::CloneGroup {
            instances,
            token_count: 10,
            line_count: 3,
        }
    }

    fn dead_code_project() -> tempfile::TempDir {
        let project = tempfile::tempdir().expect("temp dir");
        let root = project.path();
        std::fs::create_dir(root.join("src")).expect("src dir");
        write_json(
            root.join("package.json"),
            r#"{"name":"api-dead-code","main":"src/index.ts"}"#,
        );
        std::fs::write(
            root.join("src/index.ts"),
            "import './a';\nimport './b';\nexport const entry = 1;\nconsole.log(entry);\n",
        )
        .expect("entry");
        std::fs::write(root.join("src/a.ts"), "export const deadA = 1;\n").expect("a");
        std::fs::write(root.join("src/b.ts"), "export const deadB = 1;\n").expect("b");
        project
    }

    fn unused_export_names(json: &serde_json::Value) -> Vec<&str> {
        json["unused_exports"]
            .as_array()
            .expect("unused exports array")
            .iter()
            .map(|item| {
                item["name"]
                    .as_str()
                    .or_else(|| item["export_name"].as_str())
                    .expect("unused export name")
            })
            .collect()
    }

    fn write_workspace(root: &Path, relative: &str, name: &str) {
        let dir = root.join(relative);
        std::fs::create_dir_all(&dir).expect("workspace dir");
        write_json(dir.join("package.json"), &format!(r#"{{"name":"{name}"}}"#));
    }

    fn write_json(path: PathBuf, json: &str) {
        std::fs::write(path, json).expect("json file");
    }
}
