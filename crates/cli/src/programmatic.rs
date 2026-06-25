use std::path::{Path, PathBuf};

use fallow_config::OutputFormat;
use fallow_engine::results::AnalysisResults;

use crate::check::{CheckOptions, IssueFilters, TraceOptions};
use crate::dupes::{DupesMode, DupesOptions};
use crate::health::HealthOptions;
use crate::report::build_duplication_json;
use crate::report::ci::diff_filter::{DiffIndex, LoadedDiff, MAX_DIFF_BYTES};

pub use fallow_api::{
    AnalysisOptions, COMMON_ANALYSIS_OPTION_FLAGS, ComplexityOptions, ComplexitySort,
    DeadCodeFilters, DeadCodeOptions, DuplicationMode, DuplicationOptions, OwnershipEmailMode,
    ProgrammaticError, TargetEffort, derive_complexity_options, derive_complexity_run_options,
};

type ProgrammaticResult<T> = Result<T, ProgrammaticError>;

const fn duplication_mode_to_cli(mode: DuplicationMode) -> DupesMode {
    match mode {
        DuplicationMode::Strict => DupesMode::Strict,
        DuplicationMode::Mild => DupesMode::Mild,
        DuplicationMode::Weak => DupesMode::Weak,
        DuplicationMode::Semantic => DupesMode::Semantic,
    }
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

fn resolve_analysis_options(
    options: &AnalysisOptions,
) -> ProgrammaticResult<ResolvedAnalysisOptions> {
    validate_analysis_option_shape(options)?;
    let root = resolve_analysis_root(options.root.as_deref())?;
    validate_analysis_config_path(options.config_path.as_deref())?;

    let threads = options.threads.unwrap_or_else(default_threads);
    let pool = build_analysis_thread_pool(threads)?;
    let diff = options
        .diff_file
        .as_deref()
        .map(|path| load_explicit_diff_file(path, &root))
        .transpose()?;
    let production_override = options
        .production_override
        .or_else(|| options.production.then_some(true));

    Ok(ResolvedAnalysisOptions {
        root,
        config_path: options.config_path.clone(),
        no_cache: options.no_cache,
        threads,
        pool,
        diff,
        production_override,
        changed_since: options.changed_since.clone(),
        workspace: options.workspace.clone(),
        changed_workspaces: options.changed_workspaces.clone(),
        explain: options.explain,
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
        .with_code("FALLOW_MUTUALLY_EXCLUSIVE_OPTIONS")
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

fn build_analysis_thread_pool(threads: usize) -> ProgrammaticResult<rayon::ThreadPool> {
    crate::rayon_pool::build_thread_pool(threads).map_err(|err| {
        ProgrammaticError::new(format!("failed to build analysis thread pool: {err}"), 2)
            .with_code("FALLOW_THREAD_POOL_INIT_FAILED")
            .with_context("analysis.threads")
    })
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
        raw: text,
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
        fallow_output::remove_root_kind(output);
    }
}

fn programmatic_root_envelope_mode(
    resolved: &ResolvedAnalysisOptions,
) -> fallow_output::RootEnvelopeMode {
    fallow_output::RootEnvelopeMode::from_legacy(resolved.legacy_envelope)
}

fn workspace_diagnostics_for_programmatic_output(
    root: &Path,
) -> Vec<fallow_output::WorkspaceDiagnosticOutput> {
    fallow_output::workspace_diagnostics_output(crate::runtime_support::workspace_diagnostics_for(
        root,
    ))
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
        unused_component_inputs: filters.unused_component_inputs,
        unused_component_outputs: filters.unused_component_outputs,
        unused_svelte_events: filters.unused_svelte_events,
        unused_server_actions: filters.unused_server_actions,
        unused_load_data_keys: filters.unused_load_data_keys,
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
    filtered.unused_component_inputs.clear();
    filtered.unused_component_outputs.clear();
    filtered.unused_svelte_events.clear();
    filtered.unused_server_actions.clear();
    filtered.unused_load_data_keys.clear();
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
    filtered.unused_component_inputs.clear();
    filtered.unused_component_outputs.clear();
    filtered.unused_svelte_events.clear();
    filtered.unused_server_actions.clear();
    filtered.unused_load_data_keys.clear();
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
    let resolved = resolve_analysis_options(&options.analysis)?;
    resolved.install(|| {
        let filters = to_issue_filters(&options.filters);
        let trace_opts = TraceOptions {
            trace_export: None,
            trace_file: None,
            trace_dependency: None,
            impact_closure: None,
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
    let resolved = resolve_analysis_options(&options.analysis)?;
    resolved.install(|| {
        let filters = to_issue_filters(&options.filters);
        let trace_opts = TraceOptions {
            trace_export: None,
            trace_file: None,
            trace_dependency: None,
            impact_closure: None,
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
    let resolved = resolve_analysis_options(&options.analysis)?;
    resolved.install(|| {
        let filters = to_issue_filters(&options.filters);
        let trace_opts = TraceOptions {
            trace_export: None,
            trace_file: None,
            trace_dependency: None,
            impact_closure: None,
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
    let resolved = resolve_analysis_options(&options.analysis)?;
    resolved.install(|| {
        let dupes_options = DupesOptions {
            root: &resolved.root,
            config_path: &resolved.config_path,
            output: OutputFormat::Human,
            no_cache: resolved.no_cache,
            threads: resolved.threads,
            quiet: true,
            mode: Some(duplication_mode_to_cli(options.mode)),
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
    let run = derive_complexity_run_options(options);

    HealthOptions {
        execution: fallow_engine::HealthExecutionOptions {
            root: &resolved.root,
            config_path: &resolved.config_path,
            output: OutputFormat::Human,
            no_cache: resolved.no_cache,
            threads: resolved.threads,
            quiet: true,
            thresholds: run.thresholds,
            top: run.top,
            sort: run.sort,
            production: resolved.production_override.unwrap_or(false),
            production_override: resolved.production_override,
            changed_since: resolved.changed_since.as_deref(),
            diff_index: resolved.diff_index(),
            use_shared_diff_index: false,
            workspace: resolved.workspace.as_deref(),
            changed_workspaces: resolved.changed_workspaces.as_deref(),
            baseline: None,
            save_baseline: None,
            complexity: run.sections.complexity,
            file_scores: run.sections.file_scores,
            coverage_gaps: run.sections.coverage_gaps,
            config_activates_coverage_gaps: !run.sections.any_section,
            hotspots: run.sections.hotspots,
            ownership: run.sections.ownership,
            ownership_emails: run.ownership_emails,
            targets: run.sections.targets,
            css: run.css,
            force_full: run.sections.force_full,
            score_only_output: run.sections.score_only_output,
            enforce_coverage_gap_gate: true,
            effort: run.effort,
            score: run.sections.score,
            gates: fallow_engine::HealthGateOptions::default(),
            since: run.since,
            min_commits: run.min_commits,
            explain: resolved.explain,
            summary: false,
            save_snapshot: None,
            trend: false,
            coverage_inputs: run.coverage_inputs,
            performance: false,
            runtime_coverage: None,
            churn_file: None,
        },
        complexity_breakdown: false,
        group_by: None,
    }
}

pub struct CliProgrammaticHealthRunner;

impl fallow_api::ProgrammaticHealthRunner for CliProgrammaticHealthRunner {
    fn run_programmatic_health(
        &self,
        options: &ComplexityOptions,
    ) -> ProgrammaticResult<fallow_api::ProgrammaticHealthRun> {
        let resolved = resolve_analysis_options(&options.analysis)?;
        resolved.install(|| {
            let health_options = build_complexity_options(&resolved, options);
            let result = crate::health::execute_health(&health_options)
                .map_err(|_| generic_analysis_error("health"))?;
            let root = &result.config.root;
            let workspace_diagnostics = workspace_diagnostics_for_programmatic_output(root);
            let next_steps = fallow_output::build_health_next_steps(
                fallow_output::build_health_next_steps_input(
                    &result.report,
                    crate::report::suggestions::suggestions_enabled(),
                    crate::report::suggestions::setup_pointer_applicable(root),
                    crate::report::suggestions::due_impact_digest(root)
                        .map(crate::report::suggestions::impact_counts),
                    crate::report::suggestions::audit_changed_applicable(root),
                ),
            );
            Ok(fallow_api::ProgrammaticHealthRun {
                analysis: result.without_group_resolver(),
                workspace_diagnostics,
                next_steps,
                envelope_mode: programmatic_root_envelope_mode(&resolved),
                telemetry_analysis_run_id: crate::output_envelope::telemetry_analysis_run_id(),
            })
        })
    }
}

/// Run the health / complexity analysis and return the CLI JSON contract as a value.
pub fn compute_complexity(options: &ComplexityOptions) -> ProgrammaticResult<serde_json::Value> {
    fallow_api::compute_complexity_with_runner(options, &CliProgrammaticHealthRunner)
}

/// Alias for `compute_complexity` with a more product-oriented name.
pub fn compute_health(options: &ComplexityOptions) -> ProgrammaticResult<serde_json::Value> {
    fallow_api::compute_health_with_runner(options, &CliProgrammaticHealthRunner)
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

        let one_options = AnalysisOptions {
            root: Some(root.to_path_buf()),
            threads: Some(1),
            ..AnalysisOptions::default()
        };
        let one =
            resolve_analysis_options(&one_options).expect("one-thread options should resolve");
        let two_options = AnalysisOptions {
            root: Some(root.to_path_buf()),
            threads: Some(2),
            ..AnalysisOptions::default()
        };
        let two =
            resolve_analysis_options(&two_options).expect("two-thread options should resolve");

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
        let options = AnalysisOptions {
            root: Some(dir.path().to_path_buf()),
            diff_file: Some(PathBuf::from("-")),
            ..AnalysisOptions::default()
        };
        let Err(error) = resolve_analysis_options(&options) else {
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
        let options = AnalysisOptions {
            threads: Some(0),
            ..AnalysisOptions::default()
        };
        let err = resolve_analysis_options(&options)
            .err()
            .expect("zero threads must be rejected");
        assert_eq!(err.exit_code, 2);
        assert_eq!(err.code.as_deref(), Some("FALLOW_INVALID_THREADS"));
        assert_eq!(err.context.as_deref(), Some("analysis.threads"));
    }

    #[test]
    fn resolve_rejects_mutually_exclusive_workspace_flags() {
        let options = AnalysisOptions {
            workspace: Some(vec!["packages/*".to_owned()]),
            changed_workspaces: Some("HEAD~1".to_owned()),
            ..AnalysisOptions::default()
        };
        let err = resolve_analysis_options(&options)
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
        let options = AnalysisOptions {
            root: Some(PathBuf::from("/definitely/not/a/real/path/xyzzy")),
            ..AnalysisOptions::default()
        };
        let err = resolve_analysis_options(&options)
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
        let options = AnalysisOptions {
            root: Some(file),
            ..AnalysisOptions::default()
        };
        let err = resolve_analysis_options(&options)
            .err()
            .expect("a file root must be rejected");
        assert_eq!(err.code.as_deref(), Some("FALLOW_INVALID_ROOT"));
    }

    #[test]
    fn resolve_rejects_nonexistent_config_path() {
        let dir = tempfile::tempdir().expect("temp dir");
        let options = AnalysisOptions {
            root: Some(dir.path().to_path_buf()),
            config_path: Some(dir.path().join("missing.fallowrc.json")),
            ..AnalysisOptions::default()
        };
        let err = resolve_analysis_options(&options)
            .err()
            .expect("nonexistent config must be rejected");
        assert_eq!(err.code.as_deref(), Some("FALLOW_INVALID_CONFIG_PATH"));
        assert_eq!(err.context.as_deref(), Some("analysis.configPath"));
    }

    #[test]
    fn resolve_rejects_missing_diff_file() {
        let dir = tempfile::tempdir().expect("temp dir");
        let options = AnalysisOptions {
            root: Some(dir.path().to_path_buf()),
            diff_file: Some(PathBuf::from("nope.diff")),
            ..AnalysisOptions::default()
        };
        let err = resolve_analysis_options(&options)
            .err()
            .expect("missing diff file must be rejected");
        assert_eq!(err.code.as_deref(), Some("FALLOW_INVALID_DIFF_FILE"));
        assert_eq!(err.context.as_deref(), Some("analysis.diffFile"));
    }

    #[test]
    fn resolve_rejects_diff_path_that_is_a_directory() {
        let dir = tempfile::tempdir().expect("temp dir");
        std::fs::create_dir_all(dir.path().join("a-dir")).unwrap();
        let options = AnalysisOptions {
            root: Some(dir.path().to_path_buf()),
            diff_file: Some(PathBuf::from("a-dir")),
            ..AnalysisOptions::default()
        };
        let err = resolve_analysis_options(&options)
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
    fn compute_health_css_option_returns_css_analytics() {
        let project = tempfile::tempdir().expect("temp dir");
        let root = project.path();
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(
            root.join("package.json"),
            r#"{"name":"prog-css","main":"src/index.ts","dependencies":{"tailwindcss":"4.0.0"}}"#,
        )
        .unwrap();
        std::fs::write(
            root.join("src/index.ts"),
            "import './style.css';\nexport const ok = true;\n",
        )
        .unwrap();
        std::fs::write(
            root.join("src/style.css"),
            r"
@theme {
  --color-brand: #0055cc;
}

.used { color: var(--color-brand); }
",
        )
        .unwrap();

        let json = compute_health(&ComplexityOptions {
            analysis: analysis_at(root),
            css: true,
            ..ComplexityOptions::default()
        })
        .expect("CSS health analysis should succeed");

        assert_eq!(json["kind"], "health");
        assert!(json["css_analytics"].is_object());
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
