use std::path::{Path, PathBuf};

use fallow_config::{EmailMode, OutputFormat};
use fallow_core::results::AnalysisResults;
use serde::Serialize;

use crate::check::{CheckOptions, IssueFilters, TraceOptions};
use crate::dupes::{DupesMode, DupesOptions};
use crate::health::{HealthOptions, SortBy};
use crate::health_types::EffortEstimate;
use crate::report::{build_duplication_json, build_health_json, build_json};

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
    pub unresolved_imports: bool,
    pub unlisted_deps: bool,
    pub duplicate_exports: bool,
    pub circular_deps: bool,
    pub boundary_violations: bool,
    pub stale_suppressions: bool,
    pub unused_catalog_entries: bool,
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
    pub ignore_imports: bool,
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
            ignore_imports: false,
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
    Hash,
}

impl OwnershipEmailMode {
    const fn to_config(self) -> EmailMode {
        match self {
            Self::Raw => EmailMode::Raw,
            Self::Handle => EmailMode::Handle,
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

#[derive(Debug, Clone)]
struct ResolvedAnalysisOptions {
    root: PathBuf,
    config_path: Option<PathBuf>,
    no_cache: bool,
    threads: usize,
    production_override: Option<bool>,
    changed_since: Option<String>,
    workspace: Option<Vec<String>>,
    changed_workspaces: Option<String>,
    explain: bool,
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
        crate::rayon_pool::configure_global_pool(threads);
        let production_override = self
            .production_override
            .or_else(|| self.production.then_some(true));

        Ok(ResolvedAnalysisOptions {
            root,
            config_path: self.config_path.clone(),
            no_cache: self.no_cache,
            threads,
            production_override,
            changed_since: self.changed_since.clone(),
            workspace: self.workspace.clone(),
            changed_workspaces: self.changed_workspaces.clone(),
            explain: self.explain,
        })
    }
}

fn default_threads() -> usize {
    std::thread::available_parallelism().map_or(1, std::num::NonZeroUsize::get)
}

fn insert_meta(output: &mut serde_json::Value, meta: serde_json::Value) {
    if let serde_json::Value::Object(map) = output {
        map.insert("_meta".to_string(), meta);
    }
}

fn build_dead_code_json(
    results: &AnalysisResults,
    root: &Path,
    elapsed: std::time::Duration,
    explain: bool,
) -> ProgrammaticResult<serde_json::Value> {
    let mut output = build_json(results, root, elapsed).map_err(|err| {
        ProgrammaticError::new(format!("failed to serialize dead-code report: {err}"), 2)
            .with_code("FALLOW_SERIALIZE_DEAD_CODE_REPORT")
            .with_context("dead-code")
    })?;
    if explain {
        insert_meta(&mut output, crate::explain::check_meta());
    }
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
        unresolved_imports: filters.unresolved_imports,
        unlisted_deps: filters.unlisted_deps,
        duplicate_exports: filters.duplicate_exports,
        circular_deps: filters.circular_deps,
        boundary_violations: filters.boundary_violations,
        stale_suppressions: filters.stale_suppressions,
        unused_catalog_entries: filters.unused_catalog_entries,
        unresolved_catalog_references: filters.unresolved_catalog_references,
        unused_dependency_overrides: filters.unused_dependency_overrides,
        misconfigured_dependency_overrides: filters.misconfigured_dependency_overrides,
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
    filtered.unresolved_imports.clear();
    filtered.unlisted_dependencies.clear();
    filtered.duplicate_exports.clear();
    filtered.type_only_dependencies.clear();
    filtered.test_only_dependencies.clear();
    filtered.boundary_violations.clear();
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
    build_dead_code_json(
        &result.results,
        &result.config.root,
        result.elapsed,
        resolved.explain,
    )
}

/// Run the circular-dependency analysis and return the standard dead-code JSON envelope
/// filtered down to the `circular_dependencies` category.
pub fn detect_circular_dependencies(
    options: &DeadCodeOptions,
) -> ProgrammaticResult<serde_json::Value> {
    let resolved = options.analysis.resolve()?;
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
    build_dead_code_json(
        &filtered,
        &result.config.root,
        result.elapsed,
        resolved.explain,
    )
}

/// Run the boundary-violation analysis and return the standard dead-code JSON envelope
/// filtered down to the `boundary_violations` category.
pub fn detect_boundary_violations(
    options: &DeadCodeOptions,
) -> ProgrammaticResult<serde_json::Value> {
    let resolved = options.analysis.resolve()?;
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
    build_dead_code_json(
        &filtered,
        &result.config.root,
        result.elapsed,
        resolved.explain,
    )
}

/// Run the duplication analysis and return the CLI JSON contract as a value.
pub fn detect_duplication(options: &DuplicationOptions) -> ProgrammaticResult<serde_json::Value> {
    let resolved = options.analysis.resolve()?;
    let dupes_options = DupesOptions {
        root: &resolved.root,
        config_path: &resolved.config_path,
        output: OutputFormat::Human,
        no_cache: resolved.no_cache,
        threads: resolved.threads,
        quiet: true,
        // The programmatic API requires callers to provide concrete values
        // (the public `DuplicationOptions` has no Optional scalars), so we
        // forward each as an explicit override.
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
        changed_files: None,
        workspace: resolved.workspace.as_deref(),
        changed_workspaces: resolved.changed_workspaces.as_deref(),
        explain: resolved.explain,
        explain_skipped: false,
        summary: false,
        group_by: None,
        // The programmatic API returns structured JSON; performance panels go
        // to stderr in human mode and are not part of the public contract.
        performance: false,
    };
    let result =
        crate::dupes::execute_dupes(&dupes_options).map_err(|_| generic_analysis_error("dupes"))?;
    build_duplication_json(
        &result.report,
        &result.config.root,
        result.elapsed,
        resolved.explain,
    )
    .map_err(|err| {
        ProgrammaticError::new(format!("failed to serialize duplication report: {err}"), 2)
            .with_code("FALLOW_SERIALIZE_DUPLICATION_REPORT")
            .with_context("dupes")
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
        workspace: resolved.workspace.as_deref(),
        changed_workspaces: resolved.changed_workspaces.as_deref(),
        baseline: None,
        save_baseline: None,
        complexity: eff_complexity,
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
        runtime_coverage: None,
        // Programmatic API does not surface line-level PR scoping; callers
        // that want it use the CLI's `--diff-file` directly.
        diff_file: None,
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

    let health_options = build_complexity_options(&resolved, options);
    let result = crate::health::execute_health(&health_options)
        .map_err(|_| generic_analysis_error("health"))?;
    let action_opts = crate::health::health_action_opts(&result);
    build_health_json(
        &result.report,
        &result.config.root,
        result.elapsed,
        resolved.explain,
        action_opts,
    )
    .map_err(|err| {
        ProgrammaticError::new(format!("failed to serialize health report: {err}"), 2)
            .with_code("FALLOW_SERIALIZE_HEALTH_REPORT")
            .with_context("health")
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

    #[test]
    fn circular_dependency_filter_clears_other_issue_types() {
        let root = PathBuf::from("/project");
        let results = sample_results(&root);
        let filtered = filter_for_circular_dependencies(&results);
        let json = build_dead_code_json(&filtered, &root, std::time::Duration::ZERO, false)
            .expect("should serialize");

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
        let json = build_dead_code_json(&filtered, &root, std::time::Duration::ZERO, false)
            .expect("should serialize");

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

    fn unused_file_paths(json: &serde_json::Value) -> Vec<String> {
        json["unused_files"]
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|file| file["path"].as_str())
            .map(str::to_owned)
            .collect()
    }
}
