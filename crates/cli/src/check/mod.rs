use std::process::ExitCode;
use std::time::{Duration, Instant};

use fallow_config::{OutputFormat, ResolvedConfig, RulesConfig, Severity};
use fallow_engine::results::AnalysisResults;

use crate::baseline::{BaselineData, filter_new_issues};
use crate::error::emit_error;
use crate::load_config_for_analysis;
use crate::regression::{self, RegressionOpts, RegressionOutcome};
use crate::report;

#[expect(
    clippy::redundant_pub_crate,
    reason = "reused by crate::security; check is crate-private so pub(crate) is the minimal widening that exposes filtering crate-wide"
)]
pub(crate) mod filtering;
mod output;
mod rules;

pub use filtering::get_changed_files;
pub use filtering::resolve_workspace_scope;
pub use rules::has_error_severity_issues;

#[derive(Default, Clone)]
pub struct IssueFilters {
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
    pub unused_component_inputs: bool,
    pub unused_component_outputs: bool,
    pub unused_svelte_events: bool,
    pub unused_server_actions: bool,
    pub unused_load_data_keys: bool,
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
    pub invalid_client_exports: bool,
    pub mixed_client_server_barrels: bool,
    pub misplaced_directives: bool,
    pub route_collisions: bool,
    pub dynamic_segment_name_conflicts: bool,
}

impl IssueFilters {
    pub const fn any_active(&self) -> bool {
        self.unused_files
            || self.unused_exports
            || self.unused_deps
            || self.unused_types
            || self.private_type_leaks
            || self.unused_enum_members
            || self.unused_class_members
            || self.unused_store_members
            || self.unprovided_injects
            || self.unrendered_components
            || self.unused_component_props
            || self.unused_component_emits
            || self.unused_component_inputs
            || self.unused_component_outputs
            || self.unused_svelte_events
            || self.unused_server_actions
            || self.unused_load_data_keys
            || self.unresolved_imports
            || self.unlisted_deps
            || self.duplicate_exports
            || self.circular_deps
            || self.re_export_cycles
            || self.boundary_violations
            || self.policy_violations
            || self.stale_suppressions
            || self.unused_catalog_entries
            || self.empty_catalog_groups
            || self.unresolved_catalog_references
            || self.unused_dependency_overrides
            || self.misconfigured_dependency_overrides
            || self.invalid_client_exports
            || self.mixed_client_server_barrels
            || self.misplaced_directives
            || self.route_collisions
            || self.dynamic_segment_name_conflicts
    }

    /// Enable off-by-default issue types when explicitly requested as filters.
    pub fn activate_explicit_opt_ins(&self, rules: &mut RulesConfig) {
        if self.private_type_leaks && rules.private_type_leaks == Severity::Off {
            rules.private_type_leaks = Severity::Warn;
        }
    }

    /// When any filter is active, clear issue types that were NOT requested.
    pub fn apply(&self, results: &mut fallow_engine::results::AnalysisResults) {
        if !self.any_active() {
            return;
        }
        self.apply_core_filters(results);
        self.apply_component_filters(results);
        self.apply_graph_filters(results);
        self.apply_policy_filters(results);
        self.apply_catalog_filters(results);
    }

    fn apply_core_filters(&self, results: &mut fallow_engine::results::AnalysisResults) {
        if !self.unused_files {
            results.unused_files.clear();
        }
        if !self.unused_exports {
            results.unused_exports.clear();
        }
        if !self.unused_types {
            results.unused_types.clear();
        }
        if !self.private_type_leaks {
            results.private_type_leaks.clear();
        }
        if !self.unused_deps {
            results.unused_dependencies.clear();
            results.unused_dev_dependencies.clear();
            results.unused_optional_dependencies.clear();
            results.type_only_dependencies.clear();
            results.test_only_dependencies.clear();
        }
        if !self.unused_enum_members {
            results.unused_enum_members.clear();
        }
        if !self.unused_class_members {
            results.unused_class_members.clear();
        }
        if !self.unused_store_members {
            results.unused_store_members.clear();
        }
        if !self.unlisted_deps {
            results.unlisted_dependencies.clear();
        }
    }

    fn apply_component_filters(&self, results: &mut fallow_engine::results::AnalysisResults) {
        if !self.unprovided_injects {
            results.unprovided_injects.clear();
        }
        if !self.unrendered_components {
            results.unrendered_components.clear();
        }
        if !self.unused_component_props {
            results.unused_component_props.clear();
        }
        if !self.unused_component_emits {
            results.unused_component_emits.clear();
        }
        if !self.unused_component_inputs {
            results.unused_component_inputs.clear();
        }
        if !self.unused_component_outputs {
            results.unused_component_outputs.clear();
        }
        if !self.unused_svelte_events {
            results.unused_svelte_events.clear();
        }
        if !self.unused_server_actions {
            results.unused_server_actions.clear();
        }
        if !self.unused_load_data_keys {
            results.unused_load_data_keys.clear();
        }
        if !self.unresolved_imports {
            results.unresolved_imports.clear();
        }
        if !self.invalid_client_exports {
            results.invalid_client_exports.clear();
        }
        if !self.mixed_client_server_barrels {
            results.mixed_client_server_barrels.clear();
        }
        if !self.misplaced_directives {
            results.misplaced_directives.clear();
        }
        if !self.route_collisions {
            results.route_collisions.clear();
        }
        if !self.dynamic_segment_name_conflicts {
            results.dynamic_segment_name_conflicts.clear();
        }
    }

    fn apply_graph_filters(&self, results: &mut fallow_engine::results::AnalysisResults) {
        if !self.duplicate_exports {
            results.duplicate_exports.clear();
        }
        if !self.circular_deps {
            results.circular_dependencies.clear();
        }
        if !self.re_export_cycles {
            results.re_export_cycles.clear();
        }
        if !self.boundary_violations {
            results.boundary_violations.clear();
            results.boundary_coverage_violations.clear();
            results.boundary_call_violations.clear();
        }
    }

    fn apply_policy_filters(&self, results: &mut fallow_engine::results::AnalysisResults) {
        if !self.policy_violations {
            results.policy_violations.clear();
        }
        if !self.stale_suppressions {
            results.stale_suppressions.clear();
        }
    }

    fn apply_catalog_filters(&self, results: &mut fallow_engine::results::AnalysisResults) {
        if !self.unused_catalog_entries {
            results.unused_catalog_entries.clear();
        }
        if !self.empty_catalog_groups {
            results.empty_catalog_groups.clear();
        }
        if !self.unresolved_catalog_references {
            results.unresolved_catalog_references.clear();
        }
        if !self.unused_dependency_overrides {
            results.unused_dependency_overrides.clear();
        }
        if !self.misconfigured_dependency_overrides {
            results.misconfigured_dependency_overrides.clear();
        }
    }
}

pub struct TraceOptions {
    pub trace_export: Option<String>,
    pub trace_file: Option<String>,
    pub trace_dependency: Option<String>,
    /// Impact closure for a single file as the seed: walk `reverse_deps` +
    /// re-export chains to the transitive affected set and report the
    /// coordination gap. Powers the `inspect_target` MCP tool's `impact_closure`
    /// evidence section.
    pub impact_closure: Option<String>,
    pub performance: bool,
}

impl TraceOptions {
    pub const fn any_active(&self) -> bool {
        self.trace_export.is_some()
            || self.trace_file.is_some()
            || self.trace_dependency.is_some()
            || self.impact_closure.is_some()
            || self.performance
    }
}

pub struct CheckOptions<'a> {
    pub root: &'a std::path::Path,
    pub config_path: &'a Option<std::path::PathBuf>,
    pub output: OutputFormat,
    pub no_cache: bool,
    pub threads: usize,
    pub quiet: bool,
    pub fail_on_issues: bool,
    pub filters: &'a IssueFilters,
    pub changed_since: Option<&'a str>,
    pub diff_index: Option<&'a crate::report::ci::diff_filter::DiffIndex>,
    pub use_shared_diff_index: bool,
    pub baseline: Option<&'a std::path::Path>,
    pub save_baseline: Option<&'a std::path::Path>,
    pub sarif_file: Option<&'a std::path::Path>,
    pub production: bool,
    pub production_override: Option<bool>,
    pub workspace: Option<&'a [String]>,
    pub changed_workspaces: Option<&'a str>,
    pub group_by: Option<crate::GroupBy>,
    pub include_dupes: bool,
    pub trace_opts: &'a TraceOptions,
    pub explain: bool,
    pub top: Option<usize>,
    /// Only report issues in these file(s). Empty means no file filter.
    pub file: &'a [std::path::PathBuf],
    /// Report unused exports in entry files instead of auto-marking them as used.
    pub include_entry_exports: bool,
    /// When true, emit a condensed summary instead of full item-level output.
    /// Consumed by combined mode only; standalone check ignores this flag.
    pub summary: bool,
    pub regression_opts: RegressionOpts<'a>,
    /// When true, retain parsed modules and discovered files for sharing with health.
    pub retain_modules_for_health: bool,
    /// When true, return timings without printing them so combined mode can add
    /// later stages before rendering the table.
    pub defer_performance: bool,
}

/// Result of executing check analysis without printing.
pub struct CheckResult {
    pub results: AnalysisResults,
    pub config: ResolvedConfig,
    pub config_fixable: bool,
    pub elapsed: Duration,
    pub fail_on_issues: bool,
    pub regression: Option<RegressionOutcome>,
    pub baseline_deltas: Option<crate::baseline::BaselineDeltas>,
    /// When a baseline was loaded: (total entries in baseline, entries that matched current issues).
    pub baseline_matched: Option<(usize, usize)>,
    pub timings: Option<fallow_engine::trace::PipelineTimings>,
    /// Retained parse data for sharing with health (only populated when retain_modules_for_health=true).
    pub shared_parse: Option<fallow_engine::HealthSharedParseData>,
    /// Impact closure for the review brief: the transitive
    /// affected-but-not-in-diff set plus coordination gaps. Populated by the
    /// audit brief path from the retained graph against the changed-file set;
    /// `None` outside the brief path. Holds root-relative paths so it survives
    /// the graph drop and serializes directly.
    pub impact_closure: Option<fallow_engine::graph::ImpactClosurePaths>,
    /// Exports-aware public-export key set for the review brief: the
    /// `<rel_path>::<name>` keys reachable through `package.json` `exports` +
    /// re-export reachability. Computed from the retained graph on the brief
    /// path before the graph is dropped; `None` outside the brief path. Diffed
    /// against the base snapshot's `public_api` set to produce the public-API
    /// surface delta.
    pub public_api_keys: Option<rustc_hash::FxHashSet<String>>,
    /// Partition + order for the review brief's stage 2: the by-module
    /// units the changed files cluster into, plus a dependency-sensible review
    /// order. Computed from the retained graph on the brief path against the
    /// changed-file set, before the graph is dropped; `None` outside the brief
    /// path. Holds root-relative paths so it survives the graph drop and
    /// serializes directly.
    pub partition_order: Option<fallow_engine::graph::PartitionOrderPaths>,
    /// Per-changed-file graph facts for the review brief's stage 4 weighted
    /// focus map: fan-in/out (blast radius) plus the dynamic-dispatch and
    /// re-export-indirection confidence-flag signals. Computed from the retained
    /// graph on the brief path against the changed-file set, before the graph is
    /// dropped; `None` outside the brief path. Holds root-relative paths so it
    /// survives the graph drop.
    pub focus_facts: Option<Vec<fallow_engine::graph::FocusFileFactsPaths>>,
    /// Per-changed-file `rel_path -> [(exported-symbol, 1-based declaration line)]`
    /// map for the decision surface, so a coordination / public-API decision can
    /// anchor an inline comment to the exact export line. Computed from the
    /// retained graph on the brief path BEFORE the graph is dropped; `None`
    /// otherwise. Internal (CheckResult is not serialized).
    pub export_lines: Option<rustc_hash::FxHashMap<String, Vec<(String, u32)>>>,
    /// Per-anchor `rel_path -> count of in-repo modules OUTSIDE the diff that
    /// directly import it`, for the decision surface's honest per-decision consumer
    /// number. Computed from the retained graph's reverse-deps on the brief path
    /// BEFORE the graph is dropped; `None` otherwise. Internal (not serialized).
    pub internal_consumers: Option<rustc_hash::FxHashMap<String, u64>>,
}

struct CheckAnalysisData {
    results: AnalysisResults,
    trace_graph: Option<fallow_engine::graph::ModuleGraph>,
    trace_timings: Option<fallow_engine::trace::PipelineTimings>,
    retained_modules: Option<Vec<fallow_engine::ModuleInfo>>,
    retained_files: Option<Vec<fallow_engine::DiscoveredFile>>,
    script_used_packages: rustc_hash::FxHashSet<String>,
}

fn run_check_analysis(
    opts: &CheckOptions<'_>,
    config: &ResolvedConfig,
) -> Result<CheckAnalysisData, ExitCode> {
    if opts.retain_modules_for_health {
        return fallow_engine::analyze_retaining_modules(config, true, true)
            .map(|output| CheckAnalysisData {
                results: output.results,
                trace_graph: output.graph,
                trace_timings: output.timings,
                retained_modules: output.modules,
                retained_files: output.files,
                script_used_packages: output.script_used_packages,
            })
            .map_err(|e| emit_error(&format!("Analysis error: {e}"), 2, opts.output));
    }

    if opts.trace_opts.any_active() {
        return fallow_engine::analyze_with_trace(config)
            .map(|output| CheckAnalysisData {
                results: output.results,
                trace_graph: output.graph,
                trace_timings: output.timings,
                retained_modules: None,
                retained_files: None,
                script_used_packages: output.script_used_packages,
            })
            .map_err(|e| emit_error(&format!("Analysis error: {e}"), 2, opts.output));
    }

    fallow_engine::analyze(config)
        .map(|analysis| CheckAnalysisData {
            results: analysis.results,
            trace_graph: None,
            trace_timings: None,
            retained_modules: None,
            retained_files: None,
            script_used_packages: rustc_hash::FxHashSet::default(),
        })
        .map_err(|e| emit_error(&format!("Analysis error: {e}"), 2, opts.output))
}

fn prepare_check_config(opts: &CheckOptions<'_>) -> Result<ResolvedConfig, ExitCode> {
    let mut config = load_config_for_analysis(
        opts.root,
        opts.config_path,
        crate::ConfigLoadOptions {
            output: opts.output,
            no_cache: opts.no_cache,
            threads: opts.threads,
            production_override: opts
                .production_override
                .or_else(|| opts.production.then_some(true)),
            quiet: opts.quiet,
        },
        fallow_config::ProductionAnalysis::DeadCode,
    )?;
    if opts.include_entry_exports {
        config.include_entry_exports = true;
    }
    opts.filters.activate_explicit_opt_ins(&mut config.rules);
    Ok(config)
}

fn handle_trace_side_effects(
    opts: &CheckOptions<'_>,
    config: &ResolvedConfig,
    trace_graph: Option<&fallow_engine::graph::ModuleGraph>,
    trace_timings: Option<&fallow_engine::trace::PipelineTimings>,
    script_used_packages: &rustc_hash::FxHashSet<String>,
) -> Result<(), ExitCode> {
    if let Some(timings) = trace_timings
        && opts.trace_opts.performance
        && !opts.defer_performance
    {
        report::print_performance(timings, config.output);
    }
    if let Some(graph) = trace_graph {
        crate::telemetry::note_graph_structure(graph);
        if let Some(code) = output::handle_trace_output(
            graph,
            opts.trace_opts,
            &config.root,
            config.output,
            script_used_packages,
        ) {
            return Err(code);
        }
    }
    Ok(())
}

fn apply_scope_filters(
    opts: &CheckOptions<'_>,
    results: &mut AnalysisResults,
    ws_roots: Option<&Vec<std::path::PathBuf>>,
    changed_files: Option<&rustc_hash::FxHashSet<std::path::PathBuf>>,
) {
    if let Some(ws_roots) = ws_roots {
        filtering::filter_to_workspaces(results, ws_roots);
    }
    if let Some(changed) = changed_files {
        filtering::filter_changed_files(results, changed);
    }
    let diff_index = match opts.diff_index {
        Some(index) => Some(index),
        None if opts.use_shared_diff_index => crate::report::ci::diff_filter::shared_diff_index(),
        None => None,
    };
    if let Some(diff_index) = diff_index {
        filtering::filter_results_by_diff(results, diff_index, opts.root);
    }
}

fn apply_rules_and_filters(
    opts: &CheckOptions<'_>,
    config: &ResolvedConfig,
    results: &mut AnalysisResults,
) {
    rules::apply_rules(results, config);
    if opts.fail_on_issues {
        rules::promote_policy_finding_warns(results);
    }
    opts.filters.apply(results);
}

fn apply_file_filter(opts: &CheckOptions<'_>, results: &mut AnalysisResults) {
    if opts.file.is_empty() {
        return;
    }
    let file_set: rustc_hash::FxHashSet<std::path::PathBuf> = opts
        .file
        .iter()
        .map(|path| {
            if crate::path_util::is_absolute_path_any_platform(path) {
                path.clone()
            } else {
                opts.root.join(path)
            }
        })
        .collect();
    for (original, resolved) in opts.file.iter().zip(file_set.iter()) {
        if !resolved.exists() {
            eprintln!(
                "Warning: --file '{}' (resolved to '{}') was not found in the project",
                original.display(),
                resolved.display()
            );
        }
    }
    filtering::filter_changed_files(results, &file_set);
    results.unused_dependencies.clear();
    results.unused_dev_dependencies.clear();
    results.unused_optional_dependencies.clear();
    results.type_only_dependencies.clear();
    results.test_only_dependencies.clear();
}

fn warn_scoped_regression_save(opts: &CheckOptions<'_>) {
    if matches!(
        opts.regression_opts.save_target,
        regression::SaveRegressionTarget::None
    ) || !opts.regression_opts.scoped
    {
        return;
    }
    eprintln!(
        "Warning: saving regression baseline with --changed-since, --workspace, or \
         --changed-workspaces active. The baseline will reflect only scoped results, \
         not the full project."
    );
}

fn save_check_regression_baseline(
    opts: &CheckOptions<'_>,
    results: &AnalysisResults,
) -> Result<Option<regression::CheckCounts>, ExitCode> {
    let counts = match opts.regression_opts.save_target {
        regression::SaveRegressionTarget::None => return Ok(None),
        regression::SaveRegressionTarget::File(save_path) => {
            let counts = regression::CheckCounts::from_results(results);
            regression::save_regression_baseline(
                save_path,
                opts.root,
                Some(&counts),
                None,
                opts.output,
            )?;
            counts
        }
        regression::SaveRegressionTarget::Config => {
            let counts = regression::CheckCounts::from_results(results);
            let config_path = regression_config_path(opts);
            regression::save_baseline_to_config(&config_path, &counts, opts.output)?;
            counts
        }
    };
    Ok(Some(counts))
}

fn regression_config_path(opts: &CheckOptions<'_>) -> std::path::PathBuf {
    opts.config_path.as_ref().map_or_else(
        || {
            fallow_config::FallowConfig::find_config_path(opts.root)
                .unwrap_or_else(|| opts.root.join(".fallowrc.json"))
        },
        Clone::clone,
    )
}

fn build_shared_parse_data(
    results: &AnalysisResults,
    trace_graph: Option<fallow_engine::graph::ModuleGraph>,
    retained_modules: Option<Vec<fallow_engine::ModuleInfo>>,
    retained_files: Option<Vec<fallow_engine::DiscoveredFile>>,
    script_used_packages: &rustc_hash::FxHashSet<String>,
) -> Option<fallow_engine::HealthSharedParseData> {
    fallow_engine::health_shared_parse_data_from_artifacts(
        results,
        trace_graph,
        retained_modules,
        retained_files,
        script_used_packages.iter().cloned(),
    )
}

/// Warn on a scoped regression save, persist any configured regression
/// baseline, then compare the current counts against the effective baseline
/// (a just-saved baseline wins over the config baseline).
fn resolve_check_regression(
    opts: &CheckOptions<'_>,
    config: &ResolvedConfig,
    results: &AnalysisResults,
) -> Result<Option<RegressionOutcome>, ExitCode> {
    warn_scoped_regression_save(opts);

    let just_saved_baseline = save_check_regression_baseline(opts, results)?;

    let config_baseline_ref = just_saved_baseline
        .as_ref()
        .map(regression::CheckCounts::to_config_baseline);
    let config_baseline = config_baseline_ref
        .as_ref()
        .or_else(|| config.regression.as_ref().and_then(|r| r.baseline.as_ref()));
    regression::compare_check_regression(results, &opts.regression_opts, config_baseline)
}

fn complete_check_execution(
    opts: &CheckOptions<'_>,
    config: ResolvedConfig,
    data: CheckAnalysisData,
    elapsed: Duration,
    regression_outcome: Option<RegressionOutcome>,
    baseline_matched: Option<(usize, usize)>,
) -> CheckResult {
    let CheckAnalysisData {
        results,
        trace_graph,
        trace_timings,
        retained_modules,
        retained_files,
        script_used_packages,
    } = data;

    if let Some(sarif_path) = opts.sarif_file {
        output::write_sarif_file(&results, &config, sarif_path, opts.quiet);
    }

    let shared_parse = build_shared_parse_data(
        &results,
        trace_graph,
        retained_modules,
        retained_files,
        &script_used_packages,
    );

    let config_fixable = crate::fix::is_config_fixable(opts.root, opts.config_path.as_ref());

    // Report result volume to telemetry from the real result, independent of
    // the exit-code gate. Exact counts are bucketed before serialization.
    crate::telemetry::note_result_count(results.total_issues());

    CheckResult {
        results,
        config,
        config_fixable,
        elapsed,
        fail_on_issues: opts.fail_on_issues,
        regression: regression_outcome,
        baseline_deltas: None,
        baseline_matched,
        timings: trace_timings,
        shared_parse,
        impact_closure: None,
        public_api_keys: None,
        partition_order: None,
        focus_facts: None,
        export_lines: None,
        internal_consumers: None,
    }
}

/// Run analysis, filtering, and baseline handling. Returns results without printing.
pub fn execute_check(opts: &CheckOptions<'_>) -> Result<CheckResult, ExitCode> {
    let start = Instant::now();

    let config = prepare_check_config(opts)?;

    let ws_roots = filtering::resolve_workspace_scope(
        opts.root,
        opts.workspace,
        opts.changed_workspaces,
        opts.output,
    )?;

    let changed_files: Option<rustc_hash::FxHashSet<std::path::PathBuf>> = opts
        .changed_since
        .and_then(|git_ref| filtering::get_changed_files(opts.root, git_ref));

    let mut data = run_check_analysis(opts, &config)?;
    let elapsed = start.elapsed();

    handle_trace_side_effects(
        opts,
        &config,
        data.trace_graph.as_ref(),
        data.trace_timings.as_ref(),
        &data.script_used_packages,
    )?;

    apply_scope_filters(
        opts,
        &mut data.results,
        ws_roots.as_ref(),
        changed_files.as_ref(),
    );
    apply_file_filter(opts, &mut data.results);

    apply_rules_and_filters(opts, &config, &mut data.results);

    let baseline_matched = handle_baseline(
        &mut data.results,
        opts.save_baseline,
        opts.baseline,
        &config.root,
        opts.quiet,
        opts.output,
    )?;

    let regression_outcome = resolve_check_regression(opts, &config, &data.results)?;

    Ok(complete_check_execution(
        opts,
        config,
        data,
        elapsed,
        regression_outcome,
        baseline_matched,
    ))
}

pub struct PrintCheckOptions {
    pub quiet: bool,
    pub explain: bool,
    pub regression_json: bool,
    pub group_by: Option<report::OwnershipResolver>,
    pub top: Option<usize>,
    pub summary: bool,
    pub summary_heading: bool,
    pub show_explain_tip: bool,
}

struct PreparedPrintCheck<'a> {
    effective_rules: RulesConfig,
    report_ctx: report::ReportContext<'a>,
    regression_json: bool,
    quiet: bool,
}

fn prepare_print_check(result: &CheckResult, opts: PrintCheckOptions) -> PreparedPrintCheck<'_> {
    PreparedPrintCheck {
        effective_rules: effective_check_rules(result),
        report_ctx: report::ReportContext {
            root: &result.config.root,
            rules: &result.config.rules,
            elapsed: result.elapsed,
            quiet: opts.quiet,
            explain: opts.explain,
            group_by: opts.group_by,
            top: opts.top,
            summary: opts.summary,
            summary_heading: opts.summary_heading,
            show_explain_tip: opts.show_explain_tip,
            baseline_matched: result.baseline_matched,
            config_fixable: result.config_fixable,
            skip_score_and_trend: false,
        },
        regression_json: opts.regression_json,
        quiet: opts.quiet,
    }
}

fn effective_check_rules(result: &CheckResult) -> RulesConfig {
    if result.fail_on_issues {
        let mut rules = result.config.rules.clone();
        rules::promote_warns_to_errors(&mut rules);
        rules
    } else {
        result.config.rules.clone()
    }
}

/// Print check results and return appropriate exit code.
pub fn print_check_result(result: &CheckResult, opts: PrintCheckOptions) -> ExitCode {
    let prepared = prepare_print_check(result, opts);
    let report_code = report::print_results(
        &result.results,
        &prepared.report_ctx,
        result.config.output,
        if prepared.regression_json {
            result.regression.as_ref()
        } else {
            None
        },
    );
    if report_code != ExitCode::SUCCESS {
        return report_code;
    }

    if let Some(exit) = check_regression_exit_code(result.regression.as_ref(), prepared.quiet) {
        return exit;
    }

    print_load_data_key_abstain_note(result, prepared.quiet);
    print_unused_component_props_exempted_note(result, prepared.quiet);
    issue_severity_exit_code(result, &prepared.effective_rules)
}

fn check_regression_exit_code(
    outcome: Option<&RegressionOutcome>,
    quiet: bool,
) -> Option<ExitCode> {
    let outcome = outcome?;
    if !quiet {
        regression::print_regression_outcome(outcome);
    }
    outcome.is_failure().then(|| ExitCode::from(1))
}

fn print_load_data_key_abstain_note(result: &CheckResult, quiet: bool) {
    if !result.results.unused_load_data_keys_global_abstain
        || quiet
        || !matches!(result.config.output, OutputFormat::Human)
    {
        return;
    }
    eprintln!(
        "Note: unused-load-data-key abstained project-wide (a whole-object use of \
         page.data / $page.data was seen; any returned key could be read reflectively)."
    );
}

/// Human-output note when `unusedComponentProps.ignorePattern` exempted at least
/// one prop this run. Closes the silent-no-op loop (a typo'd pattern matching
/// nothing prints nothing) and teaches that the match is on the LOCAL destructure
/// binding name (`_stage`), not the public prop name the finding would report.
fn print_unused_component_props_exempted_note(result: &CheckResult, quiet: bool) {
    if result.config.unused_component_props_ignore.is_none()
        || result.results.unused_component_props_exempted == 0
        || quiet
        || !matches!(result.config.output, OutputFormat::Human)
    {
        return;
    }
    let count = result.results.unused_component_props_exempted;
    let noun = if count == 1 { "prop" } else { "props" };
    eprintln!(
        "Note: {count} component {noun} exempted by unusedComponentProps.ignorePattern \
         (matched on the local binding name, e.g. _stage, not the public prop name)."
    );
}

fn issue_severity_exit_code(result: &CheckResult, effective_rules: &RulesConfig) -> ExitCode {
    if rules::has_error_severity_issues(&result.results, effective_rules, Some(&result.config)) {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

pub fn run_check(opts: &CheckOptions<'_>) -> ExitCode {
    let result = match execute_check(opts) {
        Ok(r) => r,
        Err(code) => return code,
    };

    if !opts.quiet && matches!(opts.output, OutputFormat::Human) {
        crate::combined::print_entry_point_summary(&result.results);
    }

    let resolver = match crate::build_ownership_resolver(
        opts.group_by,
        opts.root,
        result.config.codeowners.as_deref(),
        opts.output,
    ) {
        Ok(r) => r,
        Err(code) => return code,
    };
    let exit = print_check_result(
        &result,
        PrintCheckOptions {
            quiet: opts.quiet,
            explain: opts.explain,
            regression_json: true,
            group_by: resolver,
            top: opts.top,
            summary: opts.summary,
            summary_heading: true,
            show_explain_tip: true,
        },
    );

    if opts.include_dupes && result.config.duplicates.enabled {
        output::run_cross_reference(&result.config, &result.results, opts.quiet);
    }

    exit
}

/// Save baseline and/or compare against an existing baseline.
///
/// Returns `Some(ExitCode)` on fatal errors (serialization/IO failure),
/// `Ok(None)` when no baseline was loaded, `Ok(Some((entries, matched)))` when
/// a baseline was loaded, or `Err(ExitCode)` on fatal errors.
fn handle_baseline(
    results: &mut fallow_engine::results::AnalysisResults,
    save_path: Option<&std::path::Path>,
    load_path: Option<&std::path::Path>,
    root: &std::path::Path,
    quiet: bool,
    output: OutputFormat,
) -> Result<Option<(usize, usize)>, ExitCode> {
    if let Some(baseline_path) = save_path {
        save_baseline_file(results, baseline_path, root, quiet, output)?;
    }

    if let Some(baseline_path) = load_path {
        return load_and_compare_baseline(results, baseline_path, root, quiet, output).map(Some);
    }

    Ok(None)
}

/// Serialize the current results to a baseline file, creating parent dirs.
fn save_baseline_file(
    results: &fallow_engine::results::AnalysisResults,
    baseline_path: &std::path::Path,
    root: &std::path::Path,
    quiet: bool,
    output: OutputFormat,
) -> Result<(), ExitCode> {
    let baseline_data = BaselineData::from_results(results, root);
    let mut json = serde_json::to_string_pretty(&baseline_data)
        .map_err(|e| emit_error(&format!("failed to serialize baseline: {e}"), 2, output))?;
    json.push('\n');
    if let Some(parent) = baseline_path.parent()
        && !parent.as_os_str().is_empty()
        && let Err(e) = std::fs::create_dir_all(parent)
    {
        return Err(emit_error(
            &format!("failed to create baseline directory: {e}"),
            2,
            output,
        ));
    }
    if let Err(e) = std::fs::write(baseline_path, json) {
        return Err(emit_error(
            &format!("failed to save baseline: {e}"),
            2,
            output,
        ));
    }
    if !quiet {
        eprintln!("Baseline saved to {}", baseline_path.display());
    }
    Ok(())
}

/// Load a baseline file, filter out matched issues, and return
/// `(baseline_entries, matched)`.
fn load_and_compare_baseline(
    results: &mut fallow_engine::results::AnalysisResults,
    baseline_path: &std::path::Path,
    root: &std::path::Path,
    quiet: bool,
    output: OutputFormat,
) -> Result<(usize, usize), ExitCode> {
    let content = std::fs::read_to_string(baseline_path)
        .map_err(|e| emit_error(&format!("failed to read baseline: {e}"), 2, output))?;
    let baseline_data = serde_json::from_str::<BaselineData>(&content)
        .map_err(|e| emit_error(&format!("failed to parse baseline: {e}"), 2, output))?;
    let baseline_entries = baseline_data.total_entries();
    let before = results.total_issues();
    *results = filter_new_issues(std::mem::take(results), &baseline_data, root);
    let matched = before.saturating_sub(results.total_issues());
    if !quiet {
        eprintln!("Comparing against baseline: {}", baseline_path.display());
    }
    if baseline_entries > 0 && matched == 0 && !quiet {
        eprintln!(
            "Warning: baseline has {baseline_entries} entries but matched \
             0 current issues. Your paths may have changed, or the baseline \
             was saved on a different machine. Re-save with: \
             --save-baseline {}",
            baseline_path.display(),
        );
    }
    Ok((baseline_entries, matched))
}

#[cfg(test)]
mod tests {
    use super::*;
    use fallow_engine::results::*;
    use fallow_types::extract::MemberKind;
    use std::path::PathBuf;

    fn no_filters() -> IssueFilters {
        IssueFilters {
            unused_files: false,
            unused_exports: false,
            unused_deps: false,
            unused_types: false,
            private_type_leaks: false,
            unused_enum_members: false,
            unused_class_members: false,
            unused_store_members: false,
            unprovided_injects: false,
            unrendered_components: false,
            unused_component_props: false,
            unused_component_emits: false,
            unused_component_inputs: false,
            unused_component_outputs: false,
            unused_svelte_events: false,
            unused_server_actions: false,
            unused_load_data_keys: false,
            unresolved_imports: false,
            unlisted_deps: false,
            duplicate_exports: false,
            circular_deps: false,
            re_export_cycles: false,
            boundary_violations: false,
            policy_violations: false,
            stale_suppressions: false,
            unused_catalog_entries: false,
            empty_catalog_groups: false,
            unresolved_catalog_references: false,
            unused_dependency_overrides: false,
            misconfigured_dependency_overrides: false,
            invalid_client_exports: false,
            mixed_client_server_barrels: false,
            misplaced_directives: false,
            route_collisions: false,
            dynamic_segment_name_conflicts: false,
        }
    }

    #[test]
    fn private_type_leaks_filter_opts_in_off_by_default_rule() {
        let mut rules = fallow_config::RulesConfig::default();
        assert_eq!(rules.private_type_leaks, fallow_config::Severity::Off);

        let mut filters = no_filters();
        filters.private_type_leaks = true;
        filters.activate_explicit_opt_ins(&mut rules);

        assert_eq!(rules.private_type_leaks, fallow_config::Severity::Warn);
    }

    #[expect(
        clippy::too_many_lines,
        reason = "test fixture; linear setup/assert, length is not a maintainability concern"
    )]
    fn make_results() -> AnalysisResults {
        let mut r = AnalysisResults::default();
        r.unused_files
            .push(UnusedFileFinding::with_actions(UnusedFile {
                path: PathBuf::from("/project/src/a.ts"),
            }));
        r.unused_exports
            .push(UnusedExportFinding::with_actions(UnusedExport {
                path: PathBuf::from("/project/src/b.ts"),
                export_name: "foo".into(),
                is_type_only: false,
                line: 1,
                col: 0,
                span_start: 0,
                is_re_export: false,
            }));
        r.unused_types
            .push(UnusedTypeFinding::with_actions(UnusedExport {
                path: PathBuf::from("/project/src/c.ts"),
                export_name: "MyType".into(),
                is_type_only: true,
                line: 5,
                col: 0,
                span_start: 0,
                is_re_export: false,
            }));
        r.unused_dependencies
            .push(UnusedDependencyFinding::with_actions(UnusedDependency {
                package_name: "lodash".into(),
                location: DependencyLocation::Dependencies,
                path: PathBuf::from("/project/package.json"),
                line: 5,
                used_in_workspaces: Vec::new(),
            }));
        r.unused_dev_dependencies
            .push(UnusedDevDependencyFinding::with_actions(UnusedDependency {
                package_name: "jest".into(),
                location: DependencyLocation::DevDependencies,
                path: PathBuf::from("/project/package.json"),
                line: 5,
                used_in_workspaces: Vec::new(),
            }));
        r.test_only_dependencies
            .push(TestOnlyDependencyFinding::with_actions(
                TestOnlyDependency {
                    package_name: "msw".into(),
                    path: PathBuf::from("/project/package.json"),
                    line: 9,
                },
            ));
        r.unused_enum_members
            .push(UnusedEnumMemberFinding::with_actions(UnusedMember {
                path: PathBuf::from("/project/src/d.ts"),
                parent_name: "Status".into(),
                member_name: "Pending".into(),
                kind: MemberKind::EnumMember,
                line: 3,
                col: 0,
            }));
        r.unused_class_members
            .push(UnusedClassMemberFinding::with_actions(UnusedMember {
                path: PathBuf::from("/project/src/e.ts"),
                parent_name: "Service".into(),
                member_name: "helper".into(),
                kind: MemberKind::ClassMethod,
                line: 10,
                col: 0,
            }));
        r.unresolved_imports
            .push(UnresolvedImportFinding::with_actions(UnresolvedImport {
                path: PathBuf::from("/project/src/f.ts"),
                specifier: "./missing".into(),
                line: 1,
                col: 0,
                specifier_col: 0,
            }));
        r.unlisted_dependencies
            .push(UnlistedDependencyFinding::with_actions(
                UnlistedDependency {
                    package_name: "chalk".into(),
                    imported_from: vec![ImportSite {
                        path: PathBuf::from("/project/src/g.ts"),
                        line: 1,
                        col: 0,
                    }],
                },
            ));
        r.duplicate_exports
            .push(DuplicateExportFinding::with_actions(DuplicateExport {
                export_name: "helper".into(),
                locations: vec![
                    DuplicateLocation {
                        path: PathBuf::from("/project/src/h.ts"),
                        line: 15,
                        col: 0,
                    },
                    DuplicateLocation {
                        path: PathBuf::from("/project/src/i.ts"),
                        line: 30,
                        col: 0,
                    },
                ],
            }));
        r
    }

    #[test]
    fn save_baseline_writes_trailing_newline() {
        let dir = tempfile::tempdir().expect("tempdir");
        let baseline_path = dir.path().join("baseline.json");
        let mut results = make_results();

        handle_baseline(
            &mut results,
            Some(&baseline_path),
            None,
            std::path::Path::new("/project"),
            true,
            OutputFormat::Json,
        )
        .expect("baseline save succeeds");

        let saved = std::fs::read_to_string(&baseline_path).expect("baseline is written");
        assert!(saved.ends_with('\n'));
        assert!(!saved.ends_with("\n\n"));
    }

    #[test]
    fn no_filters_means_none_active() {
        assert!(!no_filters().any_active());
    }

    #[test]
    fn single_filter_is_active() {
        let mut f = no_filters();
        f.unused_files = true;
        assert!(f.any_active());
    }

    #[test]
    fn each_filter_flag_registers_as_active() {
        let flags: Vec<fn(&mut IssueFilters)> = vec![
            |f| f.unused_files = true,
            |f| f.unused_exports = true,
            |f| f.unused_deps = true,
            |f| f.unused_types = true,
            |f| f.unused_enum_members = true,
            |f| f.unused_class_members = true,
            |f| f.unresolved_imports = true,
            |f| f.unlisted_deps = true,
            |f| f.duplicate_exports = true,
            |f| f.circular_deps = true,
            |f| f.re_export_cycles = true,
            |f| f.boundary_violations = true,
        ];
        for setter in flags {
            let mut f = no_filters();
            setter(&mut f);
            assert!(f.any_active());
        }
    }

    #[test]
    fn apply_no_active_filters_preserves_all_results() {
        let mut results = make_results();
        let original_total = results.total_issues();
        no_filters().apply(&mut results);
        assert_eq!(results.total_issues(), original_total);
    }

    #[test]
    fn apply_unused_files_filter_keeps_only_unused_files() {
        let mut results = make_results();
        let mut f = no_filters();
        f.unused_files = true;
        f.apply(&mut results);

        assert_eq!(results.unused_files.len(), 1);
        assert!(results.unused_exports.is_empty());
        assert!(results.unused_types.is_empty());
        assert!(results.unused_dependencies.is_empty());
        assert!(results.unused_dev_dependencies.is_empty());
        assert!(results.test_only_dependencies.is_empty());
        assert!(results.unused_enum_members.is_empty());
        assert!(results.unused_class_members.is_empty());
        assert!(results.unresolved_imports.is_empty());
        assert!(results.unlisted_dependencies.is_empty());
        assert!(results.duplicate_exports.is_empty());
    }

    #[test]
    fn apply_unused_deps_filter_keeps_both_dep_types() {
        let mut results = make_results();
        let mut f = no_filters();
        f.unused_deps = true;
        f.apply(&mut results);

        assert_eq!(results.unused_dependencies.len(), 1);
        assert_eq!(results.unused_dev_dependencies.len(), 1);
        assert_eq!(results.test_only_dependencies.len(), 1);
        assert!(results.unused_files.is_empty());
        assert!(results.unused_exports.is_empty());
    }

    #[test]
    fn apply_single_type_filter_clears_test_only_dependencies() {
        // Regression for #1192: a single-type filter that is not --unused-deps must clear
        // test-only-dependency findings, matching every other dependency kind. Before the fix the
        // --unused-deps clear arm omitted test_only_dependencies, so it leaked into the output of
        // any single-type filter run (e.g. `fallow dead-code --unused-files`).
        let mut results = make_results();
        assert_eq!(results.test_only_dependencies.len(), 1);

        let mut f = no_filters();
        f.unused_files = true;
        f.apply(&mut results);

        assert!(
            results.test_only_dependencies.is_empty(),
            "test-only-dependency findings must be cleared when --unused-deps is not active"
        );
    }

    #[test]
    fn apply_multiple_filters_keeps_selected_types() {
        let mut results = make_results();
        let mut f = no_filters();
        f.unused_files = true;
        f.unresolved_imports = true;
        f.apply(&mut results);

        assert_eq!(results.unused_files.len(), 1);
        assert_eq!(results.unresolved_imports.len(), 1);
        assert!(results.unused_exports.is_empty());
        assert!(results.unused_types.is_empty());
        assert!(results.duplicate_exports.is_empty());
    }

    #[test]
    fn apply_circular_deps_filter_keeps_only_circular_deps() {
        let mut results = make_results();
        results.circular_dependencies.push(
            fallow_types::output_dead_code::CircularDependencyFinding::with_actions(
                fallow_engine::results::CircularDependency {
                    files: vec![
                        PathBuf::from("/project/src/a.ts"),
                        PathBuf::from("/project/src/b.ts"),
                    ],
                    length: 2,
                    line: 1,
                    col: 0,
                    edges: Vec::new(),
                    is_cross_package: false,
                },
            ),
        );
        let mut f = no_filters();
        f.circular_deps = true;
        f.apply(&mut results);

        assert_eq!(results.circular_dependencies.len(), 1);
        assert!(results.unused_files.is_empty());
        assert!(results.unused_exports.is_empty());
        assert!(results.unused_dependencies.is_empty());
    }

    #[test]
    fn no_trace_options_means_none_active() {
        let t = TraceOptions {
            trace_export: None,
            trace_file: None,
            trace_dependency: None,
            impact_closure: None,
            performance: false,
        };
        assert!(!t.any_active());
    }

    #[test]
    fn trace_export_is_active() {
        let t = TraceOptions {
            trace_export: Some("src/foo.ts:bar".into()),
            trace_file: None,
            trace_dependency: None,
            impact_closure: None,
            performance: false,
        };
        assert!(t.any_active());
    }

    #[test]
    fn trace_file_is_active() {
        let t = TraceOptions {
            trace_export: None,
            trace_file: Some("src/foo.ts".into()),
            trace_dependency: None,
            impact_closure: None,
            performance: false,
        };
        assert!(t.any_active());
    }

    #[test]
    fn trace_dependency_is_active() {
        let t = TraceOptions {
            trace_export: None,
            trace_file: None,
            trace_dependency: Some("lodash".into()),
            impact_closure: None,
            performance: false,
        };
        assert!(t.any_active());

        let t = TraceOptions {
            trace_export: None,
            trace_file: None,
            trace_dependency: None,
            impact_closure: Some("src/foo.ts".into()),
            performance: false,
        };
        assert!(t.any_active());
    }

    #[test]
    fn performance_flag_is_active() {
        let t = TraceOptions {
            trace_export: None,
            trace_file: None,
            trace_dependency: None,
            impact_closure: None,
            performance: true,
        };
        assert!(t.any_active());
    }

    #[test]
    fn apply_boundary_violations_filter() {
        let mut results = make_results();
        results.boundary_violations.push(
            fallow_types::output_dead_code::BoundaryViolationFinding::with_actions(
                fallow_engine::results::BoundaryViolation {
                    from_path: PathBuf::from("/project/src/bad.ts"),
                    to_path: PathBuf::from("/project/lib/secret.ts"),
                    from_zone: "src".to_string(),
                    to_zone: "lib".to_string(),
                    import_specifier: "../lib/secret".to_string(),
                    line: 1,
                    col: 0,
                },
            ),
        );
        let mut f = no_filters();
        f.boundary_violations = true;
        f.apply(&mut results);

        assert_eq!(results.boundary_violations.len(), 1);
        assert!(results.unused_files.is_empty());
        assert!(results.unused_exports.is_empty());
        assert!(results.unused_dependencies.is_empty());
        assert!(results.circular_dependencies.is_empty());
    }

    #[test]
    fn apply_all_filter_types_simultaneously() {
        let mut results = make_results();
        results.circular_dependencies.push(
            fallow_types::output_dead_code::CircularDependencyFinding::with_actions(
                fallow_engine::results::CircularDependency {
                    files: vec![
                        PathBuf::from("/project/src/a.ts"),
                        PathBuf::from("/project/src/b.ts"),
                    ],
                    length: 2,
                    line: 1,
                    col: 0,
                    edges: Vec::new(),
                    is_cross_package: false,
                },
            ),
        );
        results.boundary_violations.push(
            fallow_types::output_dead_code::BoundaryViolationFinding::with_actions(
                fallow_engine::results::BoundaryViolation {
                    from_path: PathBuf::from("/project/src/x.ts"),
                    to_path: PathBuf::from("/project/lib/y.ts"),
                    from_zone: "src".to_string(),
                    to_zone: "lib".to_string(),
                    import_specifier: "../lib/y".to_string(),
                    line: 1,
                    col: 0,
                },
            ),
        );

        let f = IssueFilters {
            unused_files: true,
            unused_exports: true,
            unused_deps: true,
            unused_types: true,
            private_type_leaks: true,
            unused_enum_members: true,
            unused_class_members: true,
            unused_store_members: true,
            unprovided_injects: true,
            unrendered_components: true,
            unused_component_props: true,
            unused_component_emits: true,
            unused_component_inputs: true,
            unused_component_outputs: true,
            unused_svelte_events: true,
            unused_server_actions: true,
            unused_load_data_keys: true,
            unresolved_imports: true,
            unlisted_deps: true,
            duplicate_exports: true,
            circular_deps: true,
            re_export_cycles: true,
            boundary_violations: true,
            policy_violations: true,
            stale_suppressions: true,
            unused_catalog_entries: true,
            empty_catalog_groups: true,
            unresolved_catalog_references: true,
            unused_dependency_overrides: true,
            misconfigured_dependency_overrides: true,
            invalid_client_exports: true,
            mixed_client_server_barrels: true,
            misplaced_directives: true,
            route_collisions: true,
            dynamic_segment_name_conflicts: true,
        };
        let total_before = results.total_issues();
        f.apply(&mut results);
        assert_eq!(results.total_issues(), total_before);
    }

    #[test]
    fn apply_unused_deps_clears_optional_and_type_only() {
        let mut results = make_results();
        results
            .unused_optional_dependencies
            .push(UnusedOptionalDependencyFinding::with_actions(
                UnusedDependency {
                    package_name: "fsevents".into(),
                    location: DependencyLocation::OptionalDependencies,
                    path: PathBuf::from("/project/package.json"),
                    line: 5,
                    used_in_workspaces: Vec::new(),
                },
            ));
        results.type_only_dependencies.push(
            fallow_engine::results::TypeOnlyDependencyFinding::with_actions(TypeOnlyDependency {
                package_name: "zod".into(),
                path: PathBuf::from("/project/package.json"),
                line: 8,
            }),
        );

        let mut f = no_filters();
        f.unused_exports = true; // Only keep unused exports
        f.apply(&mut results);

        assert!(results.unused_dependencies.is_empty());
        assert!(results.unused_dev_dependencies.is_empty());
        assert!(results.unused_optional_dependencies.is_empty());
        assert!(results.type_only_dependencies.is_empty());
        assert_eq!(results.unused_exports.len(), 1);
    }
}
