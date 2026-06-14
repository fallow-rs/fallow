use std::path::{Path, PathBuf};

use fallow_config::{EmailMode, OutputFormat};
use fallow_core::results::AnalysisResults;
use serde::Serialize;

use crate::check::{CheckOptions, IssueFilters, TraceOptions};
use crate::dupes::{DupesMode, DupesOptions};
use crate::health::{HealthOptions, SortBy};
use crate::health_types::EffortEstimate;
use crate::report::ci::diff_filter::{DiffIndex, LoadedDiff, MAX_DIFF_BYTES};
use crate::report::{build_duplication_json, build_health_json};

pub const COMMON_ANALYSIS_OPTION_FLAGS: &[&str] = &[
    "root",
    "config",
    "no-cache",
    "threads",
    "changed-since",
    "diff-file",
    "production",
    "workspace",
    "changed-workspaces",
    "explain",
    "legacy-envelope",
];

/// Structured error surface for the programmatic API.
#[derive(Debug, Clone, Serialize)]
pub struct ProgrammaticError {
    pub message: String,
    pub exit_code: u8,
    pub code: Option<String>,
    pub help: Option<String>,
    pub context: Option<String>,
}

impl ProgrammaticError {
    #[must_use]
    pub fn new(message: impl Into<String>, exit_code: u8) -> Self {
        Self {
            message: message.into(),
            exit_code,
            code: None,
            help: None,
            context: None,
        }
    }

    #[must_use]
    pub fn with_help(mut self, help: impl Into<String>) -> Self {
        self.help = Some(help.into());
        self
    }

    #[must_use]
    pub fn with_code(mut self, code: impl Into<String>) -> Self {
        self.code = Some(code.into());
        self
    }

    #[must_use]
    pub fn with_context(mut self, context: impl Into<String>) -> Self {
        self.context = Some(context.into());
        self
    }
}

impl std::fmt::Display for ProgrammaticError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for ProgrammaticError {}

type ProgrammaticResult<T> = Result<T, ProgrammaticError>;

/// Shared options for all one-shot analyses.
#[derive(Debug, Clone, Default)]
pub struct AnalysisOptions {
    pub root: Option<PathBuf>,
    pub config_path: Option<PathBuf>,
    pub no_cache: bool,
    pub threads: Option<usize>,
    pub diff_file: Option<PathBuf>,
    /// Legacy convenience override. `true` forces production mode; `false`
    /// defers to config unless `production_override` is set.
    pub production: bool,
    /// Explicit production override from an embedder option. `None` means
    /// use the project config for the current analysis.
    pub production_override: Option<bool>,
    pub changed_since: Option<String>,
    pub workspace: Option<Vec<String>>,
    pub changed_workspaces: Option<String>,
    pub explain: bool,
    /// Return the one-cycle legacy root envelope without top-level `kind`.
    pub legacy_envelope: bool,
}

/// Issue-type filters for the dead-code analysis.
#[derive(Debug, Clone, Default)]
pub struct DeadCodeFilters {
    pub unused_files: bool,
    pub unused_exports: bool,
    pub unused_deps: bool,
    pub unused_types: bool,
    pub private_type_leaks: bool,
    pub unused_enum_members: bool,
    pub unused_class_members: bool,
    pub unused_store_members: bool,
    pub unprovided_injects: bool,
    pub unrendered_components: bool,
    pub unused_component_props: bool,
    pub unused_component_emits: bool,
    pub unused_server_actions: bool,
    pub unresolved_imports: bool,
    pub unlisted_deps: bool,
    pub duplicate_exports: bool,
    pub circular_deps: bool,
    pub re_export_cycles: bool,
    pub boundary_violations: bool,
    pub policy_violations: bool,
    pub stale_suppressions: bool,
    pub unused_catalog_entries: bool,
    pub empty_catalog_groups: bool,
    pub unresolved_catalog_references: bool,
    pub unused_dependency_overrides: bool,
    pub misconfigured_dependency_overrides: bool,
}

/// Options for dead-code-oriented analyses.
#[derive(Debug, Clone, Default)]
pub struct DeadCodeOptions {
    pub analysis: AnalysisOptions,
    pub filters: DeadCodeFilters,
    pub files: Vec<PathBuf>,
    pub include_entry_exports: bool,
}

/// Programmatic duplication mode selection.
#[derive(Debug, Clone, Copy, Default)]
pub enum DuplicationMode {
    Strict,
    #[default]
    Mild,
    Weak,
    Semantic,
}

impl DuplicationMode {
    const fn to_cli(self) -> DupesMode {
        match self {
            Self::Strict => DupesMode::Strict,
            Self::Mild => DupesMode::Mild,
            Self::Weak => DupesMode::Weak,
            Self::Semantic => DupesMode::Semantic,
        }
    }
}

/// Options for duplication analysis.
#[derive(Debug, Clone)]
pub struct DuplicationOptions {
    pub analysis: AnalysisOptions,
    pub mode: DuplicationMode,
    pub min_tokens: usize,
    pub min_lines: usize,
    /// Minimum number of occurrences (instances) before a clone group is
    /// reported. Values below 2 are silently treated as 2 (a single
    /// occurrence isn't a duplicate, so the engine no-ops). The CLI and
    /// MCP surfaces hard-reject `< 2` at parse time; the programmatic
    /// path is permissive because callers may construct this from
    /// untyped configuration.
    pub min_occurrences: usize,
    pub threshold: f64,
    pub skip_local: bool,
    pub cross_language: bool,
    /// Exclude import declarations from clone detection. `None` defers to the
    /// project config (which defaults to `true` since #1224); `Some(false)`
    /// forces import blocks to be counted again.
    pub ignore_imports: Option<bool>,
    pub top: Option<usize>,
}

impl Default for DuplicationOptions {
    fn default() -> Self {
        Self {
            analysis: AnalysisOptions::default(),
            mode: DuplicationMode::Mild,
            min_tokens: 50,
            min_lines: 5,
            min_occurrences: 2,
            threshold: 0.0,
            skip_local: false,
            cross_language: false,
            ignore_imports: None,
            top: None,
        }
    }
}

/// Sort criteria for complexity findings.
#[derive(Debug, Clone, Copy, Default)]
pub enum ComplexitySort {
    #[default]
    Cyclomatic,
    Cognitive,
    Lines,
    Severity,
}

impl ComplexitySort {
    const fn to_cli(self) -> SortBy {
        match self {
            Self::Severity => SortBy::Severity,
            Self::Cyclomatic => SortBy::Cyclomatic,
            Self::Cognitive => SortBy::Cognitive,
            Self::Lines => SortBy::Lines,
        }
    }
}

/// Privacy mode for ownership-aware hotspot output.
#[derive(Debug, Clone, Copy, Default)]
pub enum OwnershipEmailMode {
    Raw,
    #[default]
    Handle,
    Anonymized,
    /// Legacy spelling retained for embedders that already pass `hash`.
    Hash,
}

impl OwnershipEmailMode {
    const fn to_config(self) -> EmailMode {
        match self {
            Self::Raw => EmailMode::Raw,
            Self::Handle => EmailMode::Handle,
            Self::Anonymized => EmailMode::Anonymized,
            Self::Hash => EmailMode::Hash,
        }
    }
}

/// Effort filter for refactoring targets.
#[derive(Debug, Clone, Copy)]
pub enum TargetEffort {
    Low,
    Medium,
    High,
}

impl TargetEffort {
    const fn to_cli(self) -> EffortEstimate {
        match self {
            Self::Low => EffortEstimate::Low,
            Self::Medium => EffortEstimate::Medium,
            Self::High => EffortEstimate::High,
        }
    }
}

/// Options for complexity / health analysis.
#[derive(Debug, Clone, Default)]
pub struct ComplexityOptions {
    pub analysis: AnalysisOptions,
    pub max_cyclomatic: Option<u16>,
    pub max_cognitive: Option<u16>,
    pub max_crap: Option<f64>,
    pub top: Option<usize>,
    pub sort: ComplexitySort,
    pub complexity: bool,
    pub file_scores: bool,
    pub coverage_gaps: bool,
    pub hotspots: bool,
    pub ownership: bool,
    pub ownership_emails: Option<OwnershipEmailMode>,
    pub targets: bool,
    pub effort: Option<TargetEffort>,
    pub score: bool,
    pub since: Option<String>,
    pub min_commits: Option<u32>,
    pub coverage: Option<PathBuf>,
    pub coverage_root: Option<PathBuf>,
}

struct ResolvedAnalysisOptions {
    root: PathBuf,
    config_path: Option<PathBuf>,
    no_cache: bool,
    threads: usize,
    pool: rayon::ThreadPool,
    diff: Option<LoadedDiff>,
    production_override: Option<bool>,
    changed_since: Option<String>,
    workspace: Option<Vec<String>>,
    changed_workspaces: Option<String>,
    explain: bool,
    legacy_envelope: bool,
}

impl AnalysisOptions {
    fn resolve(&self) -> ProgrammaticResult<ResolvedAnalysisOptions> {
        if self.threads == Some(0) {
            return Err(
                ProgrammaticError::new("`threads` must be greater than 0", 2)
                    .with_code("FALLOW_INVALID_THREADS")
                    .with_context("analysis.threads"),
            );
        }
        if self.workspace.is_some() && self.changed_workspaces.is_some() {
            return Err(ProgrammaticError::new(
                "`workspace` and `changed_workspaces` are mutually exclusive",
                2,
            )
            .with_code("FALLOW_MUTUALLY_EXCLUSIVE_OPTIONS")
            .with_context("analysis.workspace"));
        }

        let root = if let Some(root) = &self.root {
            root.clone()
        } else {
            std::env::current_dir().map_err(|err| {
                ProgrammaticError::new(
                    format!("failed to resolve current working directory: {err}"),
                    2,
                )
                .with_code("FALLOW_CWD_UNAVAILABLE")
                .with_context("analysis.root")
            })?
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

        if let Some(config_path) = &self.config_path
            && !config_path.exists()
        {
            return Err(ProgrammaticError::new(
                format!("config file does not exist: {}", config_path.display()),
                2,
            )
            .with_code("FALLOW_INVALID_CONFIG_PATH")
            .with_context("analysis.configPath"));
        }

        let threads = self.threads.unwrap_or_else(default_threads);
        let pool = crate::rayon_pool::build_thread_pool(threads).map_err(|err| {
            ProgrammaticError::new(format!("failed to build analysis thread pool: {err}"), 2)
                .with_code("FALLOW_THREAD_POOL_INIT_FAILED")
                .with_context("analysis.threads")
        })?;
        let diff = self
            .diff_file
            .as_deref()
            .map(|path| load_explicit_diff_file(path, &root))
            .transpose()?;
        let production_override = self
            .production_override
            .or_else(|| self.production.then_some(true));

        Ok(ResolvedAnalysisOptions {
            root,
            config_path: self.config_path.clone(),
            no_cache: self.no_cache,
            threads,
            pool,
            diff,
            production_override,
            changed_since: self.changed_since.clone(),
            workspace: self.workspace.clone(),
            changed_workspaces: self.changed_workspaces.clone(),
            explain: self.explain,
            legacy_envelope: self.legacy_envelope,
        })
    }
}

impl ResolvedAnalysisOptions {
    fn install<R: Send>(&self, f: impl FnOnce() -> R + Send) -> R {
        self.pool.install(f)
    }

    fn diff_index(&self) -> Option<&DiffIndex> {
        self.diff.as_ref().map(|loaded| &loaded.index)
    }
}

fn default_threads() -> usize {
    std::thread::available_parallelism().map_or(1, std::num::NonZeroUsize::get)
}

fn load_explicit_diff_file(path: &Path, root: &Path) -> ProgrammaticResult<LoadedDiff> {
    if path == Path::new("-") {
        return Err(ProgrammaticError::new(
            "`diff_file` does not support stdin; pass a file path",
            2,
        )
        .with_code("FALLOW_INVALID_DIFF_FILE")
        .with_context("analysis.diffFile"));
    }

    let abs = if crate::path_util::is_absolute_path_any_platform(path) {
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

    Ok(LoadedDiff {
        index: DiffIndex::from_unified_diff(&text),
    })
}

fn insert_meta(output: &mut serde_json::Value, meta: serde_json::Value) {
    if let serde_json::Value::Object(map) = output {
        let telemetry = map
            .get("_meta")
            .and_then(|existing| existing.get("telemetry"))
            .cloned();
        let mut meta = meta;
        if let (Some(telemetry), Some(meta_map)) = (telemetry, meta.as_object_mut()) {
            meta_map.insert("telemetry".to_string(), telemetry);
        }
        map.insert("_meta".to_string(), meta);
    }
}

fn apply_programmatic_envelope_options(
    output: &mut serde_json::Value,
    resolved: &ResolvedAnalysisOptions,
) {
    if resolved.legacy_envelope {
        crate::output_envelope::remove_root_kind(output);
    }
}

fn build_dead_code_json(
    results: &AnalysisResults,
    root: &Path,
    elapsed: std::time::Duration,
    explain: bool,
    config_fixable: bool,
) -> ProgrammaticResult<serde_json::Value> {
    let mut output =
        crate::report::build_json_with_config_fixable(results, root, elapsed, config_fixable)
            .map_err(|err| {
                ProgrammaticError::new(format!("failed to serialize dead-code report: {err}"), 2)
                    .with_code("FALLOW_SERIALIZE_DEAD_CODE_REPORT")
                    .with_context("dead-code")
            })?;
    if explain {
        insert_meta(&mut output, crate::explain::check_meta());
    }
    // `build_dead_code_json` is only called after options have been resolved;
    // callers apply the root-envelope compatibility setting at the boundary.
    Ok(output)
}

fn to_issue_filters(filters: &DeadCodeFilters) -> IssueFilters {
    IssueFilters {
        unused_files: filters.unused_files,
        unused_exports: filters.unused_exports,
        unused_deps: filters.unused_deps,
        unused_types: filters.unused_types,
        private_type_leaks: filters.private_type_leaks,
        unused_enum_members: filters.unused_enum_members,
        unused_class_members: filters.unused_class_members,
        unused_store_members: filters.unused_store_members,
        unprovided_injects: filters.unprovided_injects,
        unrendered_components: filters.unrendered_components,
        unused_component_props: filters.unused_component_props,
        unused_component_emits: filters.unused_component_emits,
        unused_server_actions: filters.unused_server_actions,
        unresolved_imports: filters.unresolved_imports,
        unlisted_deps: filters.unlisted_deps,
        duplicate_exports: filters.duplicate_exports,
        circular_deps: filters.circular_deps,
        re_export_cycles: filters.re_export_cycles,
        boundary_violations: filters.boundary_violations,
        policy_violations: filters.policy_violations,
        stale_suppressions: filters.stale_suppressions,
        unused_catalog_entries: filters.unused_catalog_entries,
        empty_catalog_groups: filters.empty_catalog_groups,
        unresolved_catalog_references: filters.unresolved_catalog_references,
        unused_dependency_overrides: filters.unused_dependency_overrides,
        misconfigured_dependency_overrides: filters.misconfigured_dependency_overrides,
        // No programmatic filter for invalid-client-exports yet; the rule runs
        // and reports by default. Field exists for clear-parity only.
        invalid_client_exports: false,
        // No programmatic filter for mixed-client-server-barrels yet; the rule
        // runs and reports by default. Field exists for clear-parity only.
        mixed_client_server_barrels: false,
        // No programmatic filter for misplaced-directives yet; the rule runs and
        // reports by default. Field exists for clear-parity only.
        misplaced_directives: false,
        // No programmatic filter for route-collisions / dynamic-segment-name
        // -conflicts yet; the rules run and report by default. Fields exist for
        // clear-parity only.
        route_collisions: false,
        dynamic_segment_name_conflicts: false,
    }
}

fn generic_analysis_error(command: &str) -> ProgrammaticError {
    let code = format!(
        "FALLOW_{}_FAILED",
        command.replace('-', "_").to_ascii_uppercase()
    );
    ProgrammaticError::new(format!("{command} failed"), 2)
        .with_code(code)
        .with_context(format!("fallow {command}"))
        .with_help(format!(
            "Re-run `fallow {command} --format json --quiet` in the target project for CLI diagnostics"
        ))
}

fn build_check_options<'a>(
    resolved: &'a ResolvedAnalysisOptions,
    options: &'a DeadCodeOptions,
    filters: &'a IssueFilters,
    trace_opts: &'a TraceOptions,
) -> CheckOptions<'a> {
    CheckOptions {
        root: &resolved.root,
        config_path: &resolved.config_path,
        output: OutputFormat::Human,
        no_cache: resolved.no_cache,
        threads: resolved.threads,
        quiet: true,
        fail_on_issues: false,
        filters,
        changed_since: resolved.changed_since.as_deref(),
        diff_index: resolved.diff_index(),
        use_shared_diff_index: false,
        baseline: None,
        save_baseline: None,
        sarif_file: None,
        production: resolved.production_override.unwrap_or(false),
        production_override: resolved.production_override,
        workspace: resolved.workspace.as_deref(),
        changed_workspaces: resolved.changed_workspaces.as_deref(),
        group_by: None,
        include_dupes: false,
        trace_opts,
        explain: resolved.explain,
        top: None,
        file: &options.files,
        include_entry_exports: options.include_entry_exports,
        summary: false,
        regression_opts: crate::regression::RegressionOpts {
            fail_on_regression: false,
            tolerance: crate::regression::Tolerance::Absolute(0),
            regression_baseline_file: None,
            save_target: crate::regression::SaveRegressionTarget::None,
            scoped: false,
            quiet: true,
            output: fallow_config::OutputFormat::Json,
        },
        retain_modules_for_health: false,
        defer_performance: false,
    }
}

fn filter_for_circular_dependencies(results: &AnalysisResults) -> AnalysisResults {
    let mut filtered = results.clone();
    filtered.unused_files.clear();
    filtered.unused_exports.clear();
    filtered.unused_types.clear();
    filtered.private_type_leaks.clear();
    filtered.unused_dependencies.clear();
    filtered.unused_dev_dependencies.clear();
    filtered.unused_optional_dependencies.clear();
    filtered.unused_enum_members.clear();
    filtered.unused_class_members.clear();
    filtered.unused_store_members.clear();
    filtered.unprovided_injects.clear();
    filtered.unrendered_components.clear();
    filtered.unused_component_props.clear();
    filtered.unused_component_emits.clear();
    filtered.unused_server_actions.clear();
    filtered.unresolved_imports.clear();
    filtered.unlisted_dependencies.clear();
    filtered.duplicate_exports.clear();
    filtered.type_only_dependencies.clear();
    filtered.test_only_dependencies.clear();
    filtered.boundary_violations.clear();
    filtered.boundary_coverage_violations.clear();
    filtered.boundary_call_violations.clear();
    filtered.policy_violations.clear();
    filtered.stale_suppressions.clear();
    filtered
}

fn filter_for_boundary_violations(results: &AnalysisResults) -> AnalysisResults {
    let mut filtered = results.clone();
    filtered.unused_files.clear();
    filtered.unused_exports.clear();
    filtered.unused_types.clear();
    filtered.private_type_leaks.clear();
    filtered.unused_dependencies.clear();
    filtered.unused_dev_dependencies.clear();
    filtered.unused_optional_dependencies.clear();
    filtered.unused_enum_members.clear();
    filtered.unused_class_members.clear();
    filtered.unused_store_members.clear();
    filtered.unprovided_injects.clear();
    filtered.unrendered_components.clear();
    filtered.unused_component_props.clear();
    filtered.unused_component_emits.clear();
    filtered.unused_server_actions.clear();
    filtered.unresolved_imports.clear();
    filtered.unlisted_dependencies.clear();
    filtered.duplicate_exports.clear();
    filtered.type_only_dependencies.clear();
    filtered.test_only_dependencies.clear();
    filtered.circular_dependencies.clear();
    filtered.stale_suppressions.clear();
    filtered
}

/// Run the dead-code analysis and return the CLI JSON contract as a value.
pub fn detect_dead_code(options: &DeadCodeOptions) -> ProgrammaticResult<serde_json::Value> {
    let resolved = options.analysis.resolve()?;
    resolved.install(|| {
        let filters = to_issue_filters(&options.filters);
        let trace_opts = TraceOptions {
            trace_export: None,
            trace_file: None,
            trace_dependency: None,
            performance: false,
        };
        let check_options = build_check_options(&resolved, options, &filters, &trace_opts);
        let result = crate::check::execute_check(&check_options)
            .map_err(|_| generic_analysis_error("dead-code"))?;
        let mut output = build_dead_code_json(
            &result.results,
            &result.config.root,
            result.elapsed,
            resolved.explain,
            result.config_fixable,
        )?;
        apply_programmatic_envelope_options(&mut output, &resolved);
        Ok(output)
    })
}

/// Run the circular-dependency analysis and return the standard dead-code JSON envelope
/// filtered down to the `circular_dependencies` category.
pub fn detect_circular_dependencies(
    options: &DeadCodeOptions,
) -> ProgrammaticResult<serde_json::Value> {
    let resolved = options.analysis.resolve()?;
    resolved.install(|| {
        let filters = to_issue_filters(&options.filters);
        let trace_opts = TraceOptions {
            trace_export: None,
            trace_file: None,
            trace_dependency: None,
            performance: false,
        };
        let check_options = build_check_options(&resolved, options, &filters, &trace_opts);
        let result = crate::check::execute_check(&check_options)
            .map_err(|_| generic_analysis_error("dead-code"))?;
        let filtered = filter_for_circular_dependencies(&result.results);
        let mut output = build_dead_code_json(
            &filtered,
            &result.config.root,
            result.elapsed,
            resolved.explain,
            result.config_fixable,
        )?;
        apply_programmatic_envelope_options(&mut output, &resolved);
        Ok(output)
    })
}

/// Run the boundary-violation analysis and return the standard dead-code JSON envelope
/// filtered down to the boundary family: `boundary_violations`,
/// `boundary_coverage_violations`, and `boundary_call_violations`.
pub fn detect_boundary_violations(
    options: &DeadCodeOptions,
) -> ProgrammaticResult<serde_json::Value> {
    let resolved = options.analysis.resolve()?;
    resolved.install(|| {
        let filters = to_issue_filters(&options.filters);
        let trace_opts = TraceOptions {
            trace_export: None,
            trace_file: None,
            trace_dependency: None,
            performance: false,
        };
        let check_options = build_check_options(&resolved, options, &filters, &trace_opts);
        let result = crate::check::execute_check(&check_options)
            .map_err(|_| generic_analysis_error("dead-code"))?;
        let filtered = filter_for_boundary_violations(&result.results);
        let mut output = build_dead_code_json(
            &filtered,
            &result.config.root,
            result.elapsed,
            resolved.explain,
            result.config_fixable,
        )?;
        apply_programmatic_envelope_options(&mut output, &resolved);
        Ok(output)
    })
}

/// Run the duplication analysis and return the CLI JSON contract as a value.
pub fn detect_duplication(options: &DuplicationOptions) -> ProgrammaticResult<serde_json::Value> {
    let resolved = options.analysis.resolve()?;
    resolved.install(|| {
        let dupes_options = DupesOptions {
            root: &resolved.root,
            config_path: &resolved.config_path,
            output: OutputFormat::Human,
            no_cache: resolved.no_cache,
            threads: resolved.threads,
            quiet: true,
            mode: Some(options.mode.to_cli()),
            min_tokens: Some(options.min_tokens),
            min_lines: Some(options.min_lines),
            min_occurrences: Some(options.min_occurrences),
            threshold: Some(options.threshold),
            skip_local: options.skip_local,
            cross_language: options.cross_language,
            ignore_imports: options.ignore_imports,
            top: options.top,
            baseline_path: None,
            save_baseline_path: None,
            production: resolved.production_override.unwrap_or(false),
            production_override: resolved.production_override,
            trace: None,
            changed_since: resolved.changed_since.as_deref(),
            diff_index: resolved.diff_index(),
            use_shared_diff_index: false,
            changed_files: None,
            workspace: resolved.workspace.as_deref(),
            changed_workspaces: resolved.changed_workspaces.as_deref(),
            explain: resolved.explain,
            explain_skipped: false,
            summary: false,
            group_by: None,
            performance: false,
        };
        let result = crate::dupes::execute_dupes(&dupes_options)
            .map_err(|_| generic_analysis_error("dupes"))?;
        let mut output = build_duplication_json(
            &result.report,
            &result.config.root,
            result.elapsed,
            resolved.explain,
        )
        .map_err(|err| {
            ProgrammaticError::new(format!("failed to serialize duplication report: {err}"), 2)
                .with_code("FALLOW_SERIALIZE_DUPLICATION_REPORT")
                .with_context("dupes")
        })?;
        apply_programmatic_envelope_options(&mut output, &resolved);
        Ok(output)
    })
}

fn build_complexity_options<'a>(
    resolved: &'a ResolvedAnalysisOptions,
    options: &'a ComplexityOptions,
) -> HealthOptions<'a> {
    let ownership = options.ownership || options.ownership_emails.is_some();
    let hotspots = options.hotspots || ownership;
    let targets = options.targets || options.effort.is_some();
    let any_section = options.complexity
        || options.file_scores
        || options.coverage_gaps
        || hotspots
        || targets
        || options.score;
    let eff_score = if any_section { options.score } else { true };
    let force_full = eff_score;
    let score_only_output = options.score
        && !options.complexity
        && !options.file_scores
        && !options.coverage_gaps
        && !hotspots
        && !targets;
    let eff_file_scores = if any_section {
        options.file_scores
    } else {
        true
    } || force_full;
    let eff_hotspots = if any_section { hotspots } else { true };
    let eff_complexity = if any_section {
        options.complexity
    } else {
        true
    };
    let eff_targets = if any_section { targets } else { true };
    let eff_coverage_gaps = if any_section {
        options.coverage_gaps
    } else {
        false
    };

    HealthOptions {
        root: &resolved.root,
        config_path: &resolved.config_path,
        output: OutputFormat::Human,
        no_cache: resolved.no_cache,
        threads: resolved.threads,
        quiet: true,
        max_cyclomatic: options.max_cyclomatic,
        max_cognitive: options.max_cognitive,
        max_crap: options.max_crap,
        top: options.top,
        sort: options.sort.to_cli(),
        production: resolved.production_override.unwrap_or(false),
        production_override: resolved.production_override,
        changed_since: resolved.changed_since.as_deref(),
        diff_index: resolved.diff_index(),
        use_shared_diff_index: false,
        workspace: resolved.workspace.as_deref(),
        changed_workspaces: resolved.changed_workspaces.as_deref(),
        baseline: None,
        save_baseline: None,
        complexity: eff_complexity,
        complexity_breakdown: false,
        file_scores: eff_file_scores,
        coverage_gaps: eff_coverage_gaps,
        config_activates_coverage_gaps: !any_section,
        hotspots: eff_hotspots,
        ownership: ownership && eff_hotspots,
        ownership_emails: options.ownership_emails.map(OwnershipEmailMode::to_config),
        targets: eff_targets,
        force_full,
        score_only_output,
        enforce_coverage_gap_gate: true,
        effort: options.effort.map(TargetEffort::to_cli),
        score: eff_score,
        min_score: None,
        since: options.since.as_deref(),
        min_commits: options.min_commits,
        explain: resolved.explain,
        summary: false,
        save_snapshot: None,
        trend: false,
        group_by: None,
        coverage: options.coverage.as_deref(),
        coverage_root: options.coverage_root.as_deref(),
        performance: false,
        min_severity: None,
        report_only: false,
        runtime_coverage: None,
        // The programmatic facade has no churn-file knob; embedders that want
        // imported hotspots call the CLI. Git churn is used when available.
        churn_file: None,
    }
}

/// Run the health / complexity analysis and return the CLI JSON contract as a value.
pub fn compute_complexity(options: &ComplexityOptions) -> ProgrammaticResult<serde_json::Value> {
    let resolved = options.analysis.resolve()?;
    if let Some(path) = &options.coverage
        && !path.exists()
    {
        return Err(ProgrammaticError::new(
            format!("coverage path does not exist: {}", path.display()),
            2,
        )
        .with_code("FALLOW_INVALID_COVERAGE_PATH")
        .with_context("health.coverage"));
    }
    if let Err(message) =
        crate::health::scoring::validate_coverage_root_absolute(options.coverage_root.as_deref())
    {
        return Err(ProgrammaticError::new(message, 2)
            .with_code("FALLOW_INVALID_COVERAGE_ROOT")
            .with_context("health.coverage_root"));
    }

    resolved.install(|| {
        let health_options = build_complexity_options(&resolved, options);
        let result = crate::health::execute_health(&health_options)
            .map_err(|_| generic_analysis_error("health"))?;
        let mut output = build_health_json(
            &result.report,
            &result.config.root,
            result.elapsed,
            resolved.explain,
        )
        .map_err(|err| {
            ProgrammaticError::new(format!("failed to serialize health report: {err}"), 2)
                .with_code("FALLOW_SERIALIZE_HEALTH_REPORT")
                .with_context("health")
        })?;
        apply_programmatic_envelope_options(&mut output, &resolved);
        Ok(output)
    })
}

/// Alias for `compute_complexity` with a more product-oriented name.
pub fn compute_health(options: &ComplexityOptions) -> ProgrammaticResult<serde_json::Value> {
    compute_complexity(options)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::report::test_helpers::sample_results;
    use std::process::Command;

    const SHARED_DIFF_CHILD_ENV: &str = "FALLOW_PROGRAMMATIC_SHARED_DIFF_CHILD";
    const SHARED_DIFF_CHILD_TEST: &str =
        "programmatic::tests::programmatic_without_diff_file_ignores_shared_diff_cache";

    #[test]
    fn circular_dependency_filter_clears_other_issue_types() {
        let root = PathBuf::from("/project");
        let results = sample_results(&root);
        let filtered = filter_for_circular_dependencies(&results);
        let json = build_dead_code_json(&filtered, &root, std::time::Duration::ZERO, false, false)
            .expect("should serialize");

        assert_eq!(json["kind"], "dead-code");
        assert_eq!(json["circular_dependencies"].as_array().unwrap().len(), 1);
        assert_eq!(json["boundary_violations"].as_array().unwrap().len(), 0);
        assert_eq!(json["unused_files"].as_array().unwrap().len(), 0);
        assert_eq!(json["summary"]["total_issues"], serde_json::Value::from(1));
    }

    #[test]
    fn boundary_violation_filter_clears_other_issue_types() {
        let root = PathBuf::from("/project");
        let results = sample_results(&root);
        let filtered = filter_for_boundary_violations(&results);
        let json = build_dead_code_json(&filtered, &root, std::time::Duration::ZERO, false, false)
            .expect("should serialize");

        assert_eq!(json["kind"], "dead-code");
        assert_eq!(json["boundary_violations"].as_array().unwrap().len(), 1);
        assert_eq!(json["circular_dependencies"].as_array().unwrap().len(), 0);
        assert_eq!(json["unused_exports"].as_array().unwrap().len(), 0);
        assert_eq!(json["summary"]["total_issues"], serde_json::Value::from(1));
    }

    #[test]
    fn dead_code_without_production_override_uses_per_analysis_config() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(
            root.join("package.json"),
            r#"{"name":"programmatic-production","main":"src/index.ts"}"#,
        )
        .unwrap();
        std::fs::write(root.join("src/index.ts"), "export const ok = 1;\n").unwrap();
        std::fs::write(root.join("src/utils.test.ts"), "export const dead = 1;\n").unwrap();
        std::fs::write(
            root.join(".fallowrc.json"),
            r#"{"production":{"deadCode":true,"health":false,"dupes":false}}"#,
        )
        .unwrap();

        let options = DeadCodeOptions {
            analysis: AnalysisOptions {
                root: Some(root.to_path_buf()),
                ..AnalysisOptions::default()
            },
            ..DeadCodeOptions::default()
        };
        let json = detect_dead_code(&options).expect("analysis should succeed");
        let paths = unused_file_paths(&json);

        assert!(
            !paths.iter().any(|path| path.ends_with("utils.test.ts")),
            "omitted production option should defer to production.deadCode=true config: {paths:?}"
        );
    }

    #[test]
    fn dead_code_legacy_envelope_removes_root_kind() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(
            root.join("package.json"),
            r#"{"name":"programmatic-legacy","main":"src/index.ts"}"#,
        )
        .unwrap();
        std::fs::write(root.join("src/index.ts"), "export const ok = 1;\n").unwrap();

        let options = DeadCodeOptions {
            analysis: AnalysisOptions {
                root: Some(root.to_path_buf()),
                legacy_envelope: true,
                ..AnalysisOptions::default()
            },
            ..DeadCodeOptions::default()
        };
        let json = detect_dead_code(&options).expect("analysis should succeed");

        assert!(json.get("kind").is_none());
        assert_eq!(json["schema_version"], crate::report::SCHEMA_VERSION);
    }

    #[test]
    fn dead_code_explicit_production_false_overrides_config() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(
            root.join("package.json"),
            r#"{"name":"programmatic-production","main":"src/index.ts"}"#,
        )
        .unwrap();
        std::fs::write(root.join("src/index.ts"), "export const ok = 1;\n").unwrap();
        std::fs::write(root.join("src/utils.test.ts"), "export const dead = 1;\n").unwrap();
        std::fs::write(
            root.join(".fallowrc.json"),
            r#"{"production":{"deadCode":true,"health":false,"dupes":false}}"#,
        )
        .unwrap();

        let options = DeadCodeOptions {
            analysis: AnalysisOptions {
                root: Some(root.to_path_buf()),
                production_override: Some(false),
                ..AnalysisOptions::default()
            },
            ..DeadCodeOptions::default()
        };
        let json = detect_dead_code(&options).expect("analysis should succeed");
        let paths = unused_file_paths(&json);

        assert!(
            paths.iter().any(|path| path.ends_with("utils.test.ts")),
            "explicit production=false should include test files despite config: {paths:?}"
        );
    }

    #[test]
    fn analysis_resolve_uses_per_call_thread_pool() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path();

        let one = AnalysisOptions {
            root: Some(root.to_path_buf()),
            threads: Some(1),
            ..AnalysisOptions::default()
        }
        .resolve()
        .expect("one-thread options should resolve");
        let two = AnalysisOptions {
            root: Some(root.to_path_buf()),
            threads: Some(2),
            ..AnalysisOptions::default()
        }
        .resolve()
        .expect("two-thread options should resolve");

        assert_eq!(one.install(rayon::current_num_threads), 1);
        assert_eq!(two.install(rayon::current_num_threads), 2);
    }

    #[test]
    fn explicit_diff_file_scopes_dead_code_per_call() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(
            root.join("package.json"),
            r#"{"name":"programmatic-diff","main":"src/index.ts"}"#,
        )
        .unwrap();
        std::fs::write(
            root.join("src/index.ts"),
            "import { used } from './used';\nimport './a';\nimport './b';\nconsole.log(used);\n",
        )
        .unwrap();
        std::fs::write(root.join("src/used.ts"), "export const used = 1;\n").unwrap();
        std::fs::write(root.join("src/a.ts"), "export const deadA = 1;\n").unwrap();
        std::fs::write(root.join("src/b.ts"), "export const deadB = 1;\n").unwrap();
        std::fs::write(
            root.join("a.diff"),
            diff_for("src/a.ts", "export const deadA = 1;\n"),
        )
        .unwrap();
        std::fs::write(
            root.join("b.diff"),
            diff_for("src/b.ts", "export const deadB = 1;\n"),
        )
        .unwrap();

        let filters = DeadCodeFilters {
            unused_exports: true,
            ..DeadCodeFilters::default()
        };

        let a_json = detect_dead_code(&DeadCodeOptions {
            analysis: AnalysisOptions {
                root: Some(root.to_path_buf()),
                diff_file: Some(PathBuf::from("a.diff")),
                ..AnalysisOptions::default()
            },
            filters: filters.clone(),
            ..DeadCodeOptions::default()
        })
        .expect("a-scoped analysis should succeed");
        let b_json = detect_dead_code(&DeadCodeOptions {
            analysis: AnalysisOptions {
                root: Some(root.to_path_buf()),
                diff_file: Some(PathBuf::from("b.diff")),
                ..AnalysisOptions::default()
            },
            filters,
            ..DeadCodeOptions::default()
        })
        .expect("b-scoped analysis should succeed");

        assert_eq!(unused_export_names(&a_json), vec!["deadA"]);
        assert_eq!(unused_export_names(&b_json), vec!["deadB"]);
    }

    #[test]
    fn programmatic_without_diff_file_ignores_shared_diff_cache() {
        if std::env::var_os(SHARED_DIFF_CHILD_ENV).is_some() {
            run_programmatic_shared_diff_child();
            return;
        }

        let current_exe = std::env::current_exe().expect("current test binary should be known");
        let output = Command::new(current_exe)
            .arg("--exact")
            .arg(SHARED_DIFF_CHILD_TEST)
            .arg("--nocapture")
            .env(SHARED_DIFF_CHILD_ENV, "1")
            .output()
            .expect("shared diff child should start");

        assert!(
            output.status.success(),
            "shared diff child failed with status {:?}\nstdout:\n{}\nstderr:\n{}",
            output.status.code(),
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
    }

    fn run_programmatic_shared_diff_child() {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(
            root.join("package.json"),
            r#"{"name":"programmatic-shared-diff","main":"src/index.ts"}"#,
        )
        .unwrap();
        std::fs::write(
            root.join("src/index.ts"),
            "import { used } from './used';\nimport './a';\nimport './b';\nconsole.log(used);\n",
        )
        .unwrap();
        std::fs::write(root.join("src/used.ts"), "export const used = 1;\n").unwrap();
        std::fs::write(root.join("src/a.ts"), "export const deadA = 1;\n").unwrap();
        std::fs::write(root.join("src/b.ts"), "export const deadB = 1;\n").unwrap();
        std::fs::write(
            root.join("a.diff"),
            diff_for("src/a.ts", "export const deadA = 1;\n"),
        )
        .unwrap();

        let source = crate::report::ci::diff_filter::DiffSource::Flag(root.join("a.diff"));
        let loaded = crate::report::ci::diff_filter::init_shared_diff(Some(&source), true);
        assert!(loaded.is_some(), "shared diff should load in child process");

        let json = detect_dead_code(&DeadCodeOptions {
            analysis: AnalysisOptions {
                root: Some(root.to_path_buf()),
                ..AnalysisOptions::default()
            },
            filters: DeadCodeFilters {
                unused_exports: true,
                ..DeadCodeFilters::default()
            },
            ..DeadCodeOptions::default()
        })
        .expect("analysis without explicit diff should succeed");

        assert_eq!(unused_export_names(&json), vec!["deadA", "deadB"]);
    }

    #[test]
    fn explicit_diff_file_rejects_stdin_sentinel() {
        let dir = tempfile::tempdir().expect("temp dir");
        let Err(error) = AnalysisOptions {
            root: Some(dir.path().to_path_buf()),
            diff_file: Some(PathBuf::from("-")),
            ..AnalysisOptions::default()
        }
        .resolve() else {
            panic!("stdin sentinel is not part of the programmatic API");
        };

        assert_eq!(error.code.as_deref(), Some("FALLOW_INVALID_DIFF_FILE"));
        assert_eq!(error.context.as_deref(), Some("analysis.diffFile"));
    }

    /// Minimal valid project used by the end-to-end programmatic entry points.
    fn tiny_project() -> tempfile::TempDir {
        let dir = tempfile::tempdir().expect("temp dir");
        let root = dir.path();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(
            root.join("package.json"),
            r#"{"name":"prog-e2e","main":"src/index.ts"}"#,
        )
        .unwrap();
        std::fs::write(
            root.join("src/index.ts"),
            "export const ok = 1;\nconsole.log(ok);\n",
        )
        .unwrap();
        dir
    }

    fn analysis_at(root: &Path) -> AnalysisOptions {
        AnalysisOptions {
            root: Some(root.to_path_buf()),
            ..AnalysisOptions::default()
        }
    }

    #[test]
    fn resolve_rejects_zero_threads() {
        let err = AnalysisOptions {
            threads: Some(0),
            ..AnalysisOptions::default()
        }
        .resolve()
        .err()
        .expect("zero threads must be rejected");
        assert_eq!(err.exit_code, 2);
        assert_eq!(err.code.as_deref(), Some("FALLOW_INVALID_THREADS"));
        assert_eq!(err.context.as_deref(), Some("analysis.threads"));
    }

    #[test]
    fn resolve_rejects_mutually_exclusive_workspace_flags() {
        let err = AnalysisOptions {
            workspace: Some(vec!["packages/*".to_owned()]),
            changed_workspaces: Some("HEAD~1".to_owned()),
            ..AnalysisOptions::default()
        }
        .resolve()
        .err()
        .expect("workspace + changed_workspaces must be rejected");
        assert_eq!(
            err.code.as_deref(),
            Some("FALLOW_MUTUALLY_EXCLUSIVE_OPTIONS")
        );
        assert_eq!(err.context.as_deref(), Some("analysis.workspace"));
    }

    #[test]
    fn resolve_rejects_nonexistent_root() {
        let err = AnalysisOptions {
            root: Some(PathBuf::from("/definitely/not/a/real/path/xyzzy")),
            ..AnalysisOptions::default()
        }
        .resolve()
        .err()
        .expect("nonexistent root must be rejected");
        assert_eq!(err.code.as_deref(), Some("FALLOW_INVALID_ROOT"));
        assert_eq!(err.context.as_deref(), Some("analysis.root"));
    }

    #[test]
    fn resolve_rejects_root_that_is_a_file() {
        let dir = tempfile::tempdir().expect("temp dir");
        let file = dir.path().join("not-a-dir.txt");
        std::fs::write(&file, "x").unwrap();
        let err = AnalysisOptions {
            root: Some(file),
            ..AnalysisOptions::default()
        }
        .resolve()
        .err()
        .expect("a file root must be rejected");
        assert_eq!(err.code.as_deref(), Some("FALLOW_INVALID_ROOT"));
    }

    #[test]
    fn resolve_rejects_nonexistent_config_path() {
        let dir = tempfile::tempdir().expect("temp dir");
        let err = AnalysisOptions {
            root: Some(dir.path().to_path_buf()),
            config_path: Some(dir.path().join("missing.fallowrc.json")),
            ..AnalysisOptions::default()
        }
        .resolve()
        .err()
        .expect("nonexistent config must be rejected");
        assert_eq!(err.code.as_deref(), Some("FALLOW_INVALID_CONFIG_PATH"));
        assert_eq!(err.context.as_deref(), Some("analysis.configPath"));
    }

    #[test]
    fn resolve_rejects_missing_diff_file() {
        let dir = tempfile::tempdir().expect("temp dir");
        let err = AnalysisOptions {
            root: Some(dir.path().to_path_buf()),
            diff_file: Some(PathBuf::from("nope.diff")),
            ..AnalysisOptions::default()
        }
        .resolve()
        .err()
        .expect("missing diff file must be rejected");
        assert_eq!(err.code.as_deref(), Some("FALLOW_INVALID_DIFF_FILE"));
        assert_eq!(err.context.as_deref(), Some("analysis.diffFile"));
    }

    #[test]
    fn resolve_rejects_diff_path_that_is_a_directory() {
        let dir = tempfile::tempdir().expect("temp dir");
        std::fs::create_dir_all(dir.path().join("a-dir")).unwrap();
        let err = AnalysisOptions {
            root: Some(dir.path().to_path_buf()),
            diff_file: Some(PathBuf::from("a-dir")),
            ..AnalysisOptions::default()
        }
        .resolve()
        .err()
        .expect("a directory diff path must be rejected");
        assert_eq!(err.code.as_deref(), Some("FALLOW_INVALID_DIFF_FILE"));
    }

    #[test]
    fn detect_circular_dependencies_returns_dead_code_envelope() {
        let project = tiny_project();
        let json = detect_circular_dependencies(&DeadCodeOptions {
            analysis: analysis_at(project.path()),
            ..DeadCodeOptions::default()
        })
        .expect("circular-dependency analysis should succeed");
        assert_eq!(json["kind"], "dead-code");
        assert!(json["circular_dependencies"].is_array());
    }

    #[test]
    fn detect_boundary_violations_returns_dead_code_envelope() {
        let project = tiny_project();
        let json = detect_boundary_violations(&DeadCodeOptions {
            analysis: analysis_at(project.path()),
            ..DeadCodeOptions::default()
        })
        .expect("boundary-violation analysis should succeed");
        assert_eq!(json["kind"], "dead-code");
        assert!(json["boundary_violations"].is_array());
    }

    #[test]
    fn detect_boundary_violations_includes_boundary_coverage() {
        let project = tiny_project();
        let root = project.path();
        std::fs::write(
            root.join(".fallowrc.json"),
            r#"{
              "boundaries": {
                "zones": [
                  { "name": "domain", "patterns": ["src/domain/**"] }
                ],
                "coverage": { "requireAllFiles": true }
              }
            }"#,
        )
        .unwrap();

        let json = detect_boundary_violations(&DeadCodeOptions {
            analysis: analysis_at(root),
            ..DeadCodeOptions::default()
        })
        .expect("boundary-violation analysis should succeed");

        let coverage = json["boundary_coverage_violations"]
            .as_array()
            .expect("coverage findings should be an array");
        assert_eq!(coverage.len(), 1);
        assert_eq!(coverage[0]["path"], "src/index.ts");
        assert_eq!(json["summary"]["boundary_coverage_violations"], 1);
    }

    #[test]
    fn detect_boundary_violations_includes_boundary_calls() {
        let project = tiny_project();
        let root = project.path();
        std::fs::write(
            root.join("src/index.ts"),
            "console.log('hello');\nexport const x = 1;\n",
        )
        .unwrap();
        std::fs::write(
            root.join(".fallowrc.json"),
            r#"{
              "boundaries": {
                "zones": [
                  { "name": "domain", "patterns": ["src/**"] }
                ],
                "calls": {
                  "forbidden": [
                    { "from": "domain", "callee": "console.*" }
                  ]
                }
              }
            }"#,
        )
        .unwrap();

        let json = detect_boundary_violations(&DeadCodeOptions {
            analysis: analysis_at(root),
            ..DeadCodeOptions::default()
        })
        .expect("boundary-violation analysis should succeed");

        let calls = json["boundary_call_violations"]
            .as_array()
            .expect("boundary call findings should be an array");
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0]["path"], "src/index.ts");
        assert_eq!(calls[0]["zone"], "domain");
        assert_eq!(calls[0]["callee"], "console.log");
        assert_eq!(calls[0]["pattern"], "console.*");
        assert_eq!(json["summary"]["boundary_call_violations"], 1);
    }

    #[test]
    fn detect_duplication_returns_dupes_envelope() {
        let project = tiny_project();
        let json = detect_duplication(&DuplicationOptions {
            analysis: analysis_at(project.path()),
            ..DuplicationOptions::default()
        })
        .expect("duplication analysis should succeed");
        assert_eq!(json["kind"], "dupes");
        // DupesOutput.report is `#[serde(flatten)]`, so its fields are top-level.
        assert!(json["clone_groups"].is_array());
        assert!(json["stats"].is_object());
    }

    #[test]
    fn compute_health_returns_health_envelope() {
        let project = tiny_project();
        let options = ComplexityOptions {
            analysis: analysis_at(project.path()),
            ..ComplexityOptions::default()
        };
        // compute_health is a thin alias for compute_complexity.
        let json = compute_health(&options).expect("health analysis should succeed");
        assert_eq!(json["kind"], "health");
        // HealthOutput.report is `#[serde(flatten)]`, so its fields are top-level.
        assert!(json["summary"].is_object());
        assert!(json["findings"].is_array());
    }

    #[test]
    fn compute_complexity_rejects_missing_coverage_path() {
        let project = tiny_project();
        let err = compute_complexity(&ComplexityOptions {
            analysis: analysis_at(project.path()),
            coverage: Some(project.path().join("missing-coverage.json")),
            ..ComplexityOptions::default()
        })
        .expect_err("a missing coverage path must be rejected");
        assert_eq!(err.code.as_deref(), Some("FALLOW_INVALID_COVERAGE_PATH"));
        assert_eq!(err.context.as_deref(), Some("health.coverage"));
    }

    #[test]
    fn compute_complexity_rejects_relative_coverage_root() {
        let project = tiny_project();
        let err = compute_complexity(&ComplexityOptions {
            analysis: analysis_at(project.path()),
            coverage_root: Some(PathBuf::from("relative/prefix")),
            ..ComplexityOptions::default()
        })
        .expect_err("a relative coverage_root must be rejected");
        assert_eq!(err.code.as_deref(), Some("FALLOW_INVALID_COVERAGE_ROOT"));
        assert_eq!(err.context.as_deref(), Some("health.coverage_root"));
    }

    #[test]
    fn programmatic_error_builders_compose_and_display() {
        let err = ProgrammaticError::new("boom", 7)
            .with_code("FALLOW_X")
            .with_help("try again")
            .with_context("ctx.path");
        assert_eq!(err.message, "boom");
        assert_eq!(err.exit_code, 7);
        assert_eq!(err.code.as_deref(), Some("FALLOW_X"));
        assert_eq!(err.help.as_deref(), Some("try again"));
        assert_eq!(err.context.as_deref(), Some("ctx.path"));
        // Display surfaces only the message.
        assert_eq!(format!("{err}"), "boom");
    }

    #[test]
    fn generic_analysis_error_uppercases_command_into_code() {
        let err = generic_analysis_error("dead-code");
        assert_eq!(err.code.as_deref(), Some("FALLOW_DEAD_CODE_FAILED"));
        assert_eq!(err.exit_code, 2);
        assert_eq!(err.context.as_deref(), Some("fallow dead-code"));
        assert!(err.help.is_some(), "diagnostics hint should be attached");
    }

    fn unused_file_paths(json: &serde_json::Value) -> Vec<String> {
        json["unused_files"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|file| file["path"].as_str())
            .map(str::to_owned)
            .collect()
    }

    fn unused_export_names(json: &serde_json::Value) -> Vec<String> {
        let mut names: Vec<String> = json["unused_exports"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|export| export["export_name"].as_str())
            .map(str::to_owned)
            .collect();
        names.sort();
        names
    }

    fn diff_for(path: &str, line: &str) -> String {
        format!("diff --git a/{path} b/{path}\n--- /dev/null\n+++ b/{path}\n@@ -0,0 +1 @@\n+{line}")
    }
}
