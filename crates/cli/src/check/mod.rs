use std::process::ExitCode;
use std::time::{Duration, Instant};

use fallow_config::{OutputFormat, ResolvedConfig, RulesConfig, Severity};
use fallow_core::results::AnalysisResults;

use crate::baseline::{BaselineData, filter_new_issues};
use crate::error::emit_error;
use crate::load_config_for_analysis;
use crate::regression::{self, RegressionOpts, RegressionOutcome};
use crate::report;

mod filtering;
mod output;
mod rules;

pub use filtering::get_changed_files;
pub use filtering::resolve_workspace_scope;
pub use rules::has_error_severity_issues;

// ── Issue type filters ──────────────────────────────────────────

#[derive(Default)]
pub struct IssueFilters {
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
            || self.unresolved_imports
            || self.unlisted_deps
            || self.duplicate_exports
            || self.circular_deps
            || self.boundary_violations
            || self.stale_suppressions
            || self.unused_catalog_entries
            || self.unresolved_catalog_references
    }

    /// Enable off-by-default issue types when explicitly requested as filters.
    pub fn activate_explicit_opt_ins(&self, rules: &mut RulesConfig) {
        if self.private_type_leaks && rules.private_type_leaks == Severity::Off {
            rules.private_type_leaks = Severity::Warn;
        }
    }

    /// When any filter is active, clear issue types that were NOT requested.
    pub fn apply(&self, results: &mut fallow_core::results::AnalysisResults) {
        if !self.any_active() {
            return;
        }
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
        }
        if !self.unused_enum_members {
            results.unused_enum_members.clear();
        }
        if !self.unused_class_members {
            results.unused_class_members.clear();
        }
        if !self.unresolved_imports {
            results.unresolved_imports.clear();
        }
        if !self.unlisted_deps {
            results.unlisted_dependencies.clear();
        }
        if !self.duplicate_exports {
            results.duplicate_exports.clear();
        }
        if !self.circular_deps {
            results.circular_dependencies.clear();
        }
        if !self.boundary_violations {
            results.boundary_violations.clear();
        }
        if !self.stale_suppressions {
            results.stale_suppressions.clear();
        }
        if !self.unused_catalog_entries {
            results.unused_catalog_entries.clear();
        }
        if !self.unresolved_catalog_references {
            results.unresolved_catalog_references.clear();
        }
    }
}

// ── Trace options ───────────────────────────────────────────────

pub struct TraceOptions {
    pub trace_export: Option<String>,
    pub trace_file: Option<String>,
    pub trace_dependency: Option<String>,
    pub performance: bool,
}

impl TraceOptions {
    pub const fn any_active(&self) -> bool {
        self.trace_export.is_some()
            || self.trace_file.is_some()
            || self.trace_dependency.is_some()
            || self.performance
    }
}

// ── Check command ────────────────────────────────────────────────

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
    pub elapsed: Duration,
    pub fail_on_issues: bool,
    pub regression: Option<RegressionOutcome>,
    pub baseline_deltas: Option<crate::baseline::BaselineDeltas>,
    /// When a baseline was loaded: (total entries in baseline, entries that matched current issues).
    pub baseline_matched: Option<(usize, usize)>,
    pub timings: Option<fallow_core::trace::PipelineTimings>,
    /// Retained parse data for sharing with health (only populated when retain_modules_for_health=true).
    pub shared_parse: Option<crate::health::SharedParseData>,
}

/// Run analysis, filtering, and baseline handling. Returns results without printing.
#[expect(
    clippy::too_many_lines,
    reason = "orchestration function: analysis + filtering + baseline + regression; split candidate"
)]
pub fn execute_check(opts: &CheckOptions<'_>) -> Result<CheckResult, ExitCode> {
    let start = Instant::now();

    let mut config = load_config_for_analysis(
        opts.root,
        opts.config_path,
        opts.output,
        opts.no_cache,
        opts.threads,
        opts.production_override
            .or_else(|| opts.production.then_some(true)),
        opts.quiet,
        fallow_config::ProductionAnalysis::DeadCode,
    )?;

    // Thread --include-entry-exports flag into config for analysis layer
    if opts.include_entry_exports {
        config.include_entry_exports = true;
    }

    opts.filters.activate_explicit_opt_ins(&mut config.rules);

    // Workspace filter resolution (either --workspace or --changed-workspaces)
    let ws_roots = filtering::resolve_workspace_scope(
        opts.root,
        opts.workspace,
        opts.changed_workspaces,
        opts.output,
    )?;

    // Changed-files resolution
    let changed_files: Option<rustc_hash::FxHashSet<std::path::PathBuf>> = opts
        .changed_since
        .and_then(|git_ref| filtering::get_changed_files(opts.root, git_ref));

    // Core analysis
    let use_trace = opts.trace_opts.any_active();
    let (
        mut results,
        trace_graph,
        trace_timings,
        retained_modules,
        retained_files,
        script_used_packages,
    ) = if opts.retain_modules_for_health {
        match fallow_core::analyze_retaining_modules(&config, true, true) {
            Ok(output) => (
                output.results,
                output.graph,
                output.timings,
                output.modules,
                output.files,
                output.script_used_packages,
            ),
            Err(e) => {
                return Err(emit_error(&format!("Analysis error: {e}"), 2, opts.output));
            }
        }
    } else if use_trace {
        match fallow_core::analyze_with_trace(&config) {
            Ok(output) => (
                output.results,
                output.graph,
                output.timings,
                None,
                None,
                output.script_used_packages,
            ),
            Err(e) => {
                return Err(emit_error(&format!("Analysis error: {e}"), 2, opts.output));
            }
        }
    } else {
        // `fallow_core::analyze` returns only `AnalysisResults`, not the wider
        // `AnalysisOutput`, so `script_used_packages` is intentionally empty here.
        // No code on this path reads it: trace dispatch is gated on `trace_graph`
        // (which is also `None` here), and `SharedParseData` is only constructed
        // when `retain_modules_for_health` is set (which routes through
        // `analyze_retaining_modules`, populating the real set).
        match fallow_core::analyze(&config) {
            Ok(r) => (r, None, None, None, None, rustc_hash::FxHashSet::default()),
            Err(e) => {
                return Err(emit_error(&format!("Analysis error: {e}"), 2, opts.output));
            }
        }
    };
    let elapsed = start.elapsed();

    // Performance output
    if let Some(ref timings) = trace_timings
        && opts.trace_opts.performance
        && !opts.defer_performance
    {
        report::print_performance(timings, config.output);
    }

    // Trace early-return
    if let Some(ref graph) = trace_graph
        && let Some(code) = output::handle_trace_output(
            graph,
            opts.trace_opts,
            &config.root,
            config.output,
            &script_used_packages,
        )
    {
        return Err(code);
    }

    // Workspace scoping
    if let Some(ref ws_roots) = ws_roots {
        filtering::filter_to_workspaces(&mut results, ws_roots);
    }

    // Changed-file filtering
    if let Some(ref changed) = changed_files {
        filtering::filter_changed_files(&mut results, changed);
    }

    // Single-file filtering (--file)
    if !opts.file.is_empty() {
        let file_set: rustc_hash::FxHashSet<std::path::PathBuf> = opts
            .file
            .iter()
            .map(|p| {
                if p.is_absolute() {
                    p.clone()
                } else {
                    opts.root.join(p)
                }
            })
            .collect();
        // Warn about paths that don't exist on disk (show resolved path for clarity)
        for (original, resolved) in opts.file.iter().zip(file_set.iter()) {
            if !resolved.exists() {
                eprintln!(
                    "Warning: --file '{}' (resolved to '{}') was not found in the project",
                    original.display(),
                    resolved.display()
                );
            }
        }
        filtering::filter_changed_files(&mut results, &file_set);
        // Suppress project-wide dependency issues in single-file mode.
        // Users expect --file to scope ALL output to the specified file(s).
        results.unused_dependencies.clear();
        results.unused_dev_dependencies.clear();
        results.unused_optional_dependencies.clear();
        results.type_only_dependencies.clear();
        results.test_only_dependencies.clear();
    }

    // Rules application
    rules::apply_rules(&mut results, &config);

    // CLI issue-type filters
    opts.filters.apply(&mut results);

    // Baseline handling
    let baseline_matched = handle_baseline(
        &mut results,
        opts.save_baseline,
        opts.baseline,
        &config.root,
        opts.quiet,
        opts.output,
    )?;

    // Warn if saving a baseline from scoped results (would produce misleading counts)
    if !matches!(
        opts.regression_opts.save_target,
        regression::SaveRegressionTarget::None
    ) && opts.regression_opts.scoped
    {
        eprintln!(
            "Warning: saving regression baseline with --changed-since, --workspace, or \
             --changed-workspaces active. The baseline will reflect only scoped results, \
             not the full project."
        );
    }

    // Save regression baseline if requested.
    // Track the just-saved counts so that if --fail-on-regression is also active,
    // the same-run comparison uses the fresh baseline (not the pre-save config state).
    let just_saved_baseline = match opts.regression_opts.save_target {
        regression::SaveRegressionTarget::File(save_path) => {
            let counts = regression::CheckCounts::from_results(&results);
            regression::save_regression_baseline(
                save_path,
                opts.root,
                Some(&counts),
                None,
                opts.output,
            )?;
            Some(counts)
        }
        regression::SaveRegressionTarget::Config => {
            let counts = regression::CheckCounts::from_results(&results);
            let config_path = opts.config_path.as_ref().map_or_else(
                || {
                    fallow_config::FallowConfig::find_config_path(opts.root)
                        .unwrap_or_else(|| opts.root.join(".fallowrc.json"))
                },
                |explicit| explicit.clone(),
            );
            regression::save_baseline_to_config(&config_path, &counts, opts.output)?;
            Some(counts)
        }
        regression::SaveRegressionTarget::None => None,
    };

    // Regression detection — use just-saved baseline if available, then config, then file
    let config_baseline_ref = just_saved_baseline
        .as_ref()
        .map(regression::CheckCounts::to_config_baseline);
    let config_baseline = config_baseline_ref
        .as_ref()
        .or_else(|| config.regression.as_ref().and_then(|r| r.baseline.as_ref()));
    let regression_outcome =
        regression::compare_check_regression(&results, &opts.regression_opts, config_baseline)?;

    // SARIF file write
    if let Some(sarif_path) = opts.sarif_file {
        output::write_sarif_file(&results, &config, sarif_path, opts.quiet);
    }

    let shared_parse = match (retained_modules, retained_files) {
        (Some(modules), Some(files)) => {
            let analysis_output = trace_graph.map(|graph| fallow_core::AnalysisOutput {
                results: results.clone(),
                timings: None,
                graph: Some(graph),
                modules: None,
                files: None,
                script_used_packages: script_used_packages.clone(),
            });
            Some(crate::health::SharedParseData {
                files,
                modules,
                analysis_output,
            })
        }
        _ => None,
    };

    Ok(CheckResult {
        results,
        config,
        elapsed,
        fail_on_issues: opts.fail_on_issues,
        regression: regression_outcome,
        baseline_deltas: None,
        baseline_matched,
        timings: trace_timings,
        shared_parse,
    })
}

pub struct PrintCheckOptions {
    pub quiet: bool,
    pub explain: bool,
    pub regression_json: bool,
    pub group_by: Option<report::OwnershipResolver>,
    pub top: Option<usize>,
    pub summary: bool,
    pub show_explain_tip: bool,
}

/// Print check results and return appropriate exit code.
pub fn print_check_result(result: &CheckResult, opts: PrintCheckOptions) -> ExitCode {
    let effective_rules = if result.fail_on_issues {
        let mut r = result.config.rules.clone();
        rules::promote_warns_to_errors(&mut r);
        r
    } else {
        result.config.rules.clone()
    };

    let ctx = report::ReportContext {
        root: &result.config.root,
        rules: &result.config.rules,
        elapsed: result.elapsed,
        quiet: opts.quiet,
        explain: opts.explain,
        group_by: opts.group_by,
        top: opts.top,
        summary: opts.summary,
        show_explain_tip: opts.show_explain_tip,
        baseline_matched: result.baseline_matched,
        health_action_opts: report::HealthActionOptions::default(),
    };
    let report_code = report::print_results(
        &result.results,
        &ctx,
        result.config.output,
        if opts.regression_json {
            result.regression.as_ref()
        } else {
            None
        },
    );
    if report_code != ExitCode::SUCCESS {
        return report_code;
    }

    // Print regression outcome to stderr
    if let Some(ref outcome) = result.regression {
        if !opts.quiet {
            regression::print_regression_outcome(outcome);
        }
        if outcome.is_failure() {
            return ExitCode::from(1);
        }
    }

    if rules::has_error_severity_issues(&result.results, &effective_rules, Some(&result.config)) {
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

    // Entry-point summary (standalone check mode; combined mode uses orientation header)
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
            show_explain_tip: true,
        },
    );

    // Cross-reference: run duplication analysis on the full results
    // (the combined command handles this separately)
    if opts.include_dupes && result.config.duplicates.enabled {
        output::run_cross_reference(&result.config, &result.results, opts.quiet);
    }

    exit
}

// ── Baseline helpers ────────────────────────────────────────────

/// Save baseline and/or compare against an existing baseline.
///
/// Returns `Some(ExitCode)` on fatal errors (serialization/IO failure),
/// `Ok(None)` when no baseline was loaded, `Ok(Some((entries, matched)))` when
/// a baseline was loaded, or `Err(ExitCode)` on fatal errors.
fn handle_baseline(
    results: &mut fallow_core::results::AnalysisResults,
    save_path: Option<&std::path::Path>,
    load_path: Option<&std::path::Path>,
    root: &std::path::Path,
    quiet: bool,
    output: OutputFormat,
) -> Result<Option<(usize, usize)>, ExitCode> {
    // Save baseline if requested
    if let Some(baseline_path) = save_path {
        let baseline_data = BaselineData::from_results(results, root);
        match serde_json::to_string_pretty(&baseline_data) {
            Ok(json) => {
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
            }
            Err(e) => {
                return Err(emit_error(
                    &format!("failed to serialize baseline: {e}"),
                    2,
                    output,
                ));
            }
        }
    }

    // Compare against baseline if provided
    if let Some(baseline_path) = load_path {
        match std::fs::read_to_string(baseline_path) {
            Ok(content) => match serde_json::from_str::<BaselineData>(&content) {
                Ok(baseline_data) => {
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
                    return Ok(Some((baseline_entries, matched)));
                }
                Err(e) => {
                    return Err(emit_error(
                        &format!("failed to parse baseline: {e}"),
                        2,
                        output,
                    ));
                }
            },
            Err(e) => {
                return Err(emit_error(
                    &format!("failed to read baseline: {e}"),
                    2,
                    output,
                ));
            }
        }
    }

    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use fallow_core::extract::MemberKind;
    use fallow_core::results::*;
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
            unresolved_imports: false,
            unlisted_deps: false,
            duplicate_exports: false,
            circular_deps: false,
            boundary_violations: false,
            stale_suppressions: false,
            unused_catalog_entries: false,
            unresolved_catalog_references: false,
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

    fn make_results() -> AnalysisResults {
        let mut r = AnalysisResults::default();
        r.unused_files.push(UnusedFile {
            path: PathBuf::from("/project/src/a.ts"),
        });
        r.unused_exports.push(UnusedExport {
            path: PathBuf::from("/project/src/b.ts"),
            export_name: "foo".into(),
            is_type_only: false,
            line: 1,
            col: 0,
            span_start: 0,
            is_re_export: false,
        });
        r.unused_types.push(UnusedExport {
            path: PathBuf::from("/project/src/c.ts"),
            export_name: "MyType".into(),
            is_type_only: true,
            line: 5,
            col: 0,
            span_start: 0,
            is_re_export: false,
        });
        r.unused_dependencies.push(UnusedDependency {
            package_name: "lodash".into(),
            location: DependencyLocation::Dependencies,
            path: PathBuf::from("/project/package.json"),
            line: 5,
            used_in_workspaces: Vec::new(),
        });
        r.unused_dev_dependencies.push(UnusedDependency {
            package_name: "jest".into(),
            location: DependencyLocation::DevDependencies,
            path: PathBuf::from("/project/package.json"),
            line: 5,
            used_in_workspaces: Vec::new(),
        });
        r.unused_enum_members.push(UnusedMember {
            path: PathBuf::from("/project/src/d.ts"),
            parent_name: "Status".into(),
            member_name: "Pending".into(),
            kind: MemberKind::EnumMember,
            line: 3,
            col: 0,
        });
        r.unused_class_members.push(UnusedMember {
            path: PathBuf::from("/project/src/e.ts"),
            parent_name: "Service".into(),
            member_name: "helper".into(),
            kind: MemberKind::ClassMethod,
            line: 10,
            col: 0,
        });
        r.unresolved_imports.push(UnresolvedImport {
            path: PathBuf::from("/project/src/f.ts"),
            specifier: "./missing".into(),
            line: 1,
            col: 0,
            specifier_col: 0,
        });
        r.unlisted_dependencies.push(UnlistedDependency {
            package_name: "chalk".into(),
            imported_from: vec![ImportSite {
                path: PathBuf::from("/project/src/g.ts"),
                line: 1,
                col: 0,
            }],
        });
        r.duplicate_exports.push(DuplicateExport {
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
        });
        r
    }

    // ── IssueFilters::any_active ─────────────────────────────────

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
            |f| f.boundary_violations = true,
        ];
        for setter in flags {
            let mut f = no_filters();
            setter(&mut f);
            assert!(f.any_active());
        }
    }

    // ── IssueFilters::apply ──────────────────────────────────────

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
        assert!(results.unused_files.is_empty());
        assert!(results.unused_exports.is_empty());
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
        // Add circular dependency to results
        results
            .circular_dependencies
            .push(fallow_core::results::CircularDependency {
                files: vec![
                    PathBuf::from("/project/src/a.ts"),
                    PathBuf::from("/project/src/b.ts"),
                ],
                length: 2,
                line: 1,
                col: 0,
                is_cross_package: false,
            });
        let mut f = no_filters();
        f.circular_deps = true;
        f.apply(&mut results);

        assert_eq!(results.circular_dependencies.len(), 1);
        assert!(results.unused_files.is_empty());
        assert!(results.unused_exports.is_empty());
        assert!(results.unused_dependencies.is_empty());
    }

    // ── TraceOptions::any_active ─────────────────────────────────

    #[test]
    fn no_trace_options_means_none_active() {
        let t = TraceOptions {
            trace_export: None,
            trace_file: None,
            trace_dependency: None,
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
            performance: true,
        };
        assert!(t.any_active());
    }

    // ── Boundary violations filter ──────────────────────────────

    #[test]
    fn apply_boundary_violations_filter() {
        let mut results = make_results();
        results
            .boundary_violations
            .push(fallow_core::results::BoundaryViolation {
                from_path: PathBuf::from("/project/src/bad.ts"),
                to_path: PathBuf::from("/project/lib/secret.ts"),
                from_zone: "src".to_string(),
                to_zone: "lib".to_string(),
                import_specifier: "../lib/secret".to_string(),
                line: 1,
                col: 0,
            });
        let mut f = no_filters();
        f.boundary_violations = true;
        f.apply(&mut results);

        assert_eq!(results.boundary_violations.len(), 1);
        assert!(results.unused_files.is_empty());
        assert!(results.unused_exports.is_empty());
        assert!(results.unused_dependencies.is_empty());
        assert!(results.circular_dependencies.is_empty());
    }

    // ── Combined filter for multiple types ──────────────────────

    #[test]
    fn apply_all_filter_types_simultaneously() {
        let mut results = make_results();
        results
            .circular_dependencies
            .push(fallow_core::results::CircularDependency {
                files: vec![
                    PathBuf::from("/project/src/a.ts"),
                    PathBuf::from("/project/src/b.ts"),
                ],
                length: 2,
                line: 1,
                col: 0,
                is_cross_package: false,
            });
        results
            .boundary_violations
            .push(fallow_core::results::BoundaryViolation {
                from_path: PathBuf::from("/project/src/x.ts"),
                to_path: PathBuf::from("/project/lib/y.ts"),
                from_zone: "src".to_string(),
                to_zone: "lib".to_string(),
                import_specifier: "../lib/y".to_string(),
                line: 1,
                col: 0,
            });

        // Enable all filters
        let f = IssueFilters {
            unused_files: true,
            unused_exports: true,
            unused_deps: true,
            unused_types: true,
            private_type_leaks: true,
            unused_enum_members: true,
            unused_class_members: true,
            unresolved_imports: true,
            unlisted_deps: true,
            duplicate_exports: true,
            circular_deps: true,
            boundary_violations: true,
            stale_suppressions: true,
            unused_catalog_entries: true,
            unresolved_catalog_references: true,
        };
        let total_before = results.total_issues();
        f.apply(&mut results);
        // With all filters enabled, all issues should be preserved
        assert_eq!(results.total_issues(), total_before);
    }

    // ── Optional and type-only dependency filters ───────────────

    #[test]
    fn apply_unused_deps_clears_optional_and_type_only() {
        let mut results = make_results();
        results.unused_optional_dependencies.push(UnusedDependency {
            package_name: "fsevents".into(),
            location: DependencyLocation::OptionalDependencies,
            path: PathBuf::from("/project/package.json"),
            line: 5,
            used_in_workspaces: Vec::new(),
        });
        results
            .type_only_dependencies
            .push(fallow_core::results::TypeOnlyDependency {
                package_name: "zod".into(),
                path: PathBuf::from("/project/package.json"),
                line: 8,
            });

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
