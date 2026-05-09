use std::io::IsTerminal;
use std::process::ExitCode;
use std::time::Instant;

use colored::Colorize;
use fallow_config::OutputFormat;

use crate::check::{CheckOptions, CheckResult, IssueFilters, TraceOptions};
use crate::dupes::{DupesMode, DupesOptions, DupesResult};
use crate::health::{HealthOptions, HealthResult, SortBy};
use crate::regression;
use crate::report;
use crate::{AnalysisKind, error::emit_error, load_config_for_analysis};

pub struct CombinedOptions<'a> {
    pub root: &'a std::path::Path,
    pub config_path: &'a Option<std::path::PathBuf>,
    pub output: OutputFormat,
    pub no_cache: bool,
    pub threads: usize,
    pub quiet: bool,
    pub fail_on_issues: bool,
    pub sarif_file: Option<&'a std::path::Path>,
    pub changed_since: Option<&'a str>,
    pub diff_file: Option<&'a std::path::Path>,
    pub baseline: Option<&'a std::path::Path>,
    pub save_baseline: Option<&'a std::path::Path>,
    pub production: bool,
    pub production_dead_code: Option<bool>,
    pub production_health: Option<bool>,
    pub production_dupes: Option<bool>,
    pub workspace: Option<&'a [String]>,
    pub changed_workspaces: Option<&'a str>,
    pub group_by: Option<crate::GroupBy>,
    pub explain: bool,
    pub explain_skipped: bool,
    pub performance: bool,
    pub summary: bool,
    pub run_check: bool,
    pub run_dupes: bool,
    pub run_health: bool,
    pub dupes_mode: Option<DupesMode>,
    pub dupes_threshold: Option<f64>,
    pub score: bool,
    pub trend: bool,
    pub save_snapshot: Option<&'a Option<String>>,
    pub include_entry_exports: bool,
    pub regression_opts: regression::RegressionOpts<'a>,
}

/// Resolve which analyses to run based on --only/--skip flags.
/// Precondition: only and skip must not both be non-empty (validated in main.rs).
pub fn resolve_analyses(only: &[AnalysisKind], skip: &[AnalysisKind]) -> (bool, bool, bool) {
    if !only.is_empty() {
        (
            only.contains(&AnalysisKind::DeadCode),
            only.contains(&AnalysisKind::Dupes),
            only.contains(&AnalysisKind::Health),
        )
    } else if !skip.is_empty() {
        (
            !skip.contains(&AnalysisKind::DeadCode),
            !skip.contains(&AnalysisKind::Dupes),
            !skip.contains(&AnalysisKind::Health),
        )
    } else {
        (true, true, true)
    }
}

pub fn run_combined(opts: &CombinedOptions<'_>) -> ExitCode {
    let start = Instant::now();
    let mut check_result: Option<CheckResult> = None;
    let mut dupes_result: Option<DupesResult> = None;
    let mut health_result: Option<HealthResult> = None;

    // Build CheckOptions up front. References to `filters` and `trace_opts` need
    // to outlive both the sequential and parallel branches below.
    let filters = IssueFilters::default();
    let trace_opts = TraceOptions {
        trace_export: None,
        trace_file: None,
        trace_dependency: None,
        performance: opts.performance,
    };
    let check_opts = if opts.run_check {
        Some(CheckOptions {
            root: opts.root,
            config_path: opts.config_path,
            output: opts.output,
            no_cache: opts.no_cache,
            threads: opts.threads,
            quiet: opts.quiet,
            fail_on_issues: opts.fail_on_issues,
            filters: &filters,
            changed_since: opts.changed_since,
            baseline: opts.baseline,
            save_baseline: opts.save_baseline,
            sarif_file: opts.sarif_file,
            production: opts.production_dead_code.unwrap_or(opts.production),
            production_override: opts.production_dead_code,
            workspace: opts.workspace,
            changed_workspaces: opts.changed_workspaces,
            group_by: opts.group_by,
            include_dupes: false,
            trace_opts: &trace_opts,
            explain: opts.explain,
            top: None,
            file: &[],
            include_entry_exports: opts.include_entry_exports,
            summary: opts.summary,
            regression_opts: opts.regression_opts,
            retain_modules_for_health: opts.run_health,
            defer_performance: true,
        })
    } else {
        None
    };

    // When both check and dupes are requested, run them concurrently. They share
    // no mutable state: each writes to a distinct cache subdir (parse-vN vs
    // dupes-tokens-vN), each returns a buffered result printed centrally below,
    // and each sorts its own outputs internally so rayon's work-stealing order
    // does not leak into the rendered output.
    //
    // Trade-off: the opportunistic share_files_with_dupes path (which let dupes
    // skip discover_files when health was also running and production flags
    // matched) is forfeited here. That saved ~8ms warm; the parallel join saves
    // ~100ms by overlapping the dupes suffix array with check's analyze pass.
    if let (Some(check_opts), true) = (check_opts.as_ref(), opts.run_dupes) {
        let (check_res, dupes_res) = rayon::join(
            || crate::check::execute_check(check_opts),
            || run_combined_dupes(opts, None),
        );
        match check_res {
            Ok(result) => check_result = Some(result),
            Err(code) => return code,
        }
        match dupes_res {
            Ok(result) => dupes_result = result,
            Err(code) => return code,
        }
    } else {
        if let Some(check_opts) = check_opts.as_ref() {
            match crate::check::execute_check(check_opts) {
                Ok(result) => check_result = Some(result),
                Err(code) => return code,
            }
        }
        if opts.run_dupes {
            match run_combined_dupes(opts, check_result.as_ref()) {
                Ok(result) => dupes_result = result,
                Err(code) => return code,
            }
        }
    }

    if opts.performance
        && let Some(ref mut check) = check_result
        && let Some(ref mut timings) = check.timings
    {
        timings.duplication_ms = dupes_result
            .as_ref()
            .map(|dupes| dupes.elapsed.as_secs_f64() * 1000.0);
        report::print_performance(timings, opts.output);
    }

    // Run health (complexity analysis)
    // When check already ran, reuse its parsed modules (with complexity data) to avoid
    // re-parsing all files. Saves ~1.9s on 21K-file projects like next.js.
    if opts.run_health {
        let health_opts = build_health_opts(opts);
        let check_production = opts.production_dead_code.unwrap_or(opts.production);
        let health_production = opts.production_health.unwrap_or(opts.production);
        let shared = if check_production == health_production {
            check_result.as_mut().and_then(|r| r.shared_parse.take())
        } else {
            None
        };
        let health_run = if let Some(shared_data) = shared {
            crate::health::execute_health_with_shared_parse(&health_opts, shared_data)
        } else {
            crate::health::execute_health(&health_opts)
        };
        match health_run {
            Ok(result) => {
                health_result = Some(result);
            }
            Err(code) => return code,
        }
    }

    let total_elapsed = start.elapsed();

    let mut max_exit = match print_combined_report(
        opts,
        check_result.as_ref(),
        dupes_result.as_ref(),
        health_result.as_ref(),
        total_elapsed,
    ) {
        Ok(exit) => exit,
        Err(code) => return code,
    };

    handle_regression_and_summary(
        &mut max_exit,
        opts.quiet,
        check_result.as_ref(),
        dupes_result.as_ref(),
        health_result.as_ref(),
    );

    ExitCode::from(max_exit)
}

/// Build ownership resolver, dispatch to format-specific printer, and return
/// the accumulated max exit code. Returns `Err(ExitCode)` for fatal output errors.
fn print_combined_report(
    opts: &CombinedOptions<'_>,
    check_result: Option<&CheckResult>,
    dupes_result: Option<&DupesResult>,
    health_result: Option<&HealthResult>,
    total_elapsed: std::time::Duration,
) -> Result<u8, ExitCode> {
    // Build ownership resolver once for human/compact/markdown rendering.
    // Structured formats (JSON/SARIF/CodeClimate) have their own envelope and skip grouping.
    let codeowners_cfg = check_result
        .map(|r| &r.config)
        .or_else(|| health_result.map(|r| &r.config))
        .or_else(|| dupes_result.map(|r| &r.config))
        .and_then(|c| c.codeowners.as_deref());
    let resolver =
        crate::build_ownership_resolver(opts.group_by, opts.root, codeowners_cfg, opts.output)?;

    match opts.output {
        OutputFormat::Json => {
            let code = print_combined_json(
                check_result,
                dupes_result,
                health_result,
                opts.root,
                total_elapsed,
                opts.explain,
            );
            if code != ExitCode::SUCCESS {
                return Err(code);
            }
        }
        OutputFormat::Sarif => {
            let code = print_combined_sarif(check_result, dupes_result, health_result);
            if code != ExitCode::SUCCESS {
                return Err(code);
            }
        }
        OutputFormat::CodeClimate => {
            let code = print_combined_codeclimate(check_result, dupes_result, health_result);
            if code != ExitCode::SUCCESS {
                return Err(code);
            }
        }
        OutputFormat::PrCommentGithub => {
            let value = build_combined_codeclimate(check_result, dupes_result, health_result);
            let code = report::ci::pr_comment::print_pr_comment(
                "combined",
                report::ci::pr_comment::Provider::Github,
                &value,
            );
            if code != ExitCode::SUCCESS {
                return Err(code);
            }
        }
        OutputFormat::PrCommentGitlab => {
            let value = build_combined_codeclimate(check_result, dupes_result, health_result);
            let code = report::ci::pr_comment::print_pr_comment(
                "combined",
                report::ci::pr_comment::Provider::Gitlab,
                &value,
            );
            if code != ExitCode::SUCCESS {
                return Err(code);
            }
        }
        OutputFormat::ReviewGithub => {
            let value = build_combined_codeclimate(check_result, dupes_result, health_result);
            let code = report::ci::review::print_review_envelope(
                "combined",
                report::ci::pr_comment::Provider::Github,
                &value,
            );
            if code != ExitCode::SUCCESS {
                return Err(code);
            }
        }
        OutputFormat::ReviewGitlab => {
            let value = build_combined_codeclimate(check_result, dupes_result, health_result);
            let code = report::ci::review::print_review_envelope(
                "combined",
                report::ci::pr_comment::Provider::Gitlab,
                &value,
            );
            if code != ExitCode::SUCCESS {
                return Err(code);
            }
        }
        _ => {
            return Ok(print_human_sections(
                opts,
                check_result,
                dupes_result,
                health_result,
                resolver,
            ));
        }
    }
    Ok(0)
}

/// Print human/compact/markdown sections with optional section headers.
fn print_human_sections(
    opts: &CombinedOptions<'_>,
    check_result: Option<&CheckResult>,
    dupes_result: Option<&DupesResult>,
    health_result: Option<&HealthResult>,
    resolver: Option<report::OwnershipResolver>,
) -> u8 {
    let mut max_exit: u8 = 0;
    let show_headers = matches!(opts.output, OutputFormat::Human) && !opts.quiet;

    // Orientation header: vital signs + analysis scope + start-here nudge
    if show_headers {
        if let Some(result) = health_result {
            print_orientation_header(result, check_result);
        } else if let Some(result) = check_result {
            print_entry_point_summary(&result.results);
        }
    }

    let has_any_findings = check_result.is_some_and(|result| result.results.total_issues() > 0)
        || dupes_result.is_some_and(|result| !result.report.clone_groups.is_empty())
        || health_result.is_some_and(|result| !result.report.findings.is_empty());
    if show_headers && has_any_findings && std::io::stdout().is_terminal() {
        println!(
            "{}",
            "Tip: run `fallow explain <issue-type>` for any finding below.".dimmed()
        );
        println!();
    }

    if let Some(result) = check_result {
        if show_headers {
            eprintln!();
            eprintln!("── Dead Code ──────────────────────────────────────");
        }
        let code = crate::check::print_check_result(
            result,
            crate::check::PrintCheckOptions {
                quiet: opts.quiet,
                explain: opts.explain,
                regression_json: false,
                group_by: resolver,
                top: None,
                summary: opts.summary,
                show_explain_tip: false,
            },
        );
        max_exit = max_exit.max(exit_code_to_u8(code));
    }

    if let Some(result) = dupes_result {
        if show_headers {
            eprintln!();
            eprintln!("── Duplication ────────────────────────────────────");
        }
        let code =
            crate::dupes::print_dupes_result(result, opts.quiet, opts.explain, opts.summary, false);
        max_exit = max_exit.max(exit_code_to_u8(code));
    }

    if let Some(result) = health_result {
        if show_headers {
            eprintln!();
            eprintln!("── Complexity ─────────────────────────────────────");
        }
        if let Some(ref timings) = result.timings {
            report::print_health_performance(timings, opts.output);
        }
        let code = crate::health::print_health_result(
            result,
            opts.quiet,
            opts.explain,
            None,
            None,
            opts.summary,
            false,
        );
        max_exit = max_exit.max(exit_code_to_u8(code));
    }

    max_exit
}

/// Handle regression outcome and print failure summary.
fn handle_regression_and_summary(
    max_exit: &mut u8,
    quiet: bool,
    check_result: Option<&CheckResult>,
    dupes_result: Option<&DupesResult>,
    health_result: Option<&HealthResult>,
) {
    // Regression exit code (applies regardless of output format)
    if let Some(result) = check_result
        && let Some(ref outcome) = result.regression
    {
        if !quiet {
            regression::print_regression_outcome(outcome);
        }
        if outcome.is_failure() {
            *max_exit = (*max_exit).max(1);
        }
    }

    // Summary on failure
    if *max_exit > 0 && !quiet {
        print_failure_summary(check_result, dupes_result, health_result);
    }
}

/// Print a summary line listing which analyses had failures.
fn print_failure_summary(
    check_result: Option<&CheckResult>,
    dupes_result: Option<&DupesResult>,
    health_result: Option<&HealthResult>,
) {
    let mut parts = Vec::new();
    if let Some(r) = check_result {
        let issues = r.results.total_issues();
        if issues > 0 {
            let delta_suffix = r.baseline_deltas.as_ref().map_or_else(String::new, |d| {
                match d.total_delta.cmp(&0) {
                    std::cmp::Ordering::Greater => {
                        format!(", +{} since baseline", d.total_delta)
                    }
                    std::cmp::Ordering::Less => format!(", {} since baseline", d.total_delta),
                    std::cmp::Ordering::Equal => ", \u{00b1}0 since baseline".to_string(),
                }
            });
            parts.push(format!("dead-code ({issues} issues{delta_suffix})"));
        }
    }
    if let Some(r) = dupes_result {
        let groups = r.report.clone_groups.len();
        if groups > 0 {
            parts.push(format!("dupes ({groups} clone groups)"));
        }
    }
    if let Some(r) = health_result {
        let above = r.report.summary.functions_above_threshold;
        if above > 0 {
            parts.push(format!("health ({above} above threshold)"));
        }
    }
    if !parts.is_empty() {
        // Repeat start-here nudge so it's visible at the bottom of scrolled output
        let nudge = health_result
            .filter(|r| !r.report.targets.is_empty())
            .map(|r| {
                // Prefer non-test/fixture target; skip nudge if all targets are noise
                if let Some(top) = r.report.targets.iter().find(|t| !is_test_path(&t.path)) {
                    let name = top
                        .path
                        .file_name()
                        .map(|f| f.to_string_lossy().to_string())
                        .unwrap_or_default();
                    format!(" \u{2014} start with {name}")
                } else {
                    String::new()
                }
            })
            .unwrap_or_default();
        eprintln!("\nFailed: {}{nudge}", parts.join(", "));
    }
}

/// Print combined JSON output wrapping check, dupes, and health results.
#[expect(
    clippy::cast_possible_truncation,
    reason = "elapsed milliseconds won't exceed u64::MAX"
)]
fn print_combined_json(
    check: Option<&CheckResult>,
    dupes: Option<&DupesResult>,
    health: Option<&HealthResult>,
    root: &std::path::Path,
    elapsed: std::time::Duration,
    _explain: bool,
) -> ExitCode {
    let mut combined = serde_json::Map::new();
    combined.insert("schema_version".into(), serde_json::Value::Number(3.into()));
    combined.insert(
        "version".into(),
        serde_json::Value::String(env!("CARGO_PKG_VERSION").to_string()),
    );
    combined.insert(
        "elapsed_ms".into(),
        serde_json::Value::Number(serde_json::Number::from(elapsed.as_millis() as u64)),
    );

    if let Some(result) = check {
        match report::build_json(&result.results, &result.config.root, result.elapsed) {
            Ok(mut json) => {
                if let Some(ref outcome) = result.regression
                    && let serde_json::Value::Object(ref mut map) = json
                {
                    map.insert("regression".to_string(), outcome.to_json());
                }
                if let Some(ref deltas) = result.baseline_deltas
                    && let serde_json::Value::Object(ref mut map) = json
                {
                    map.insert(
                        "baseline_deltas".to_string(),
                        report::build_baseline_deltas_json(
                            deltas.total_delta,
                            deltas
                                .per_category
                                .iter()
                                .map(|(cat, d)| (cat.as_str(), d.current, d.baseline, d.delta)),
                        ),
                    );
                }
                if let Some((entries, matched)) = result.baseline_matched
                    && let serde_json::Value::Object(ref mut map) = json
                {
                    map.insert(
                        "baseline".to_string(),
                        serde_json::json!({
                            "entries": entries,
                            "matched": matched,
                        }),
                    );
                }
                combined.insert("check".into(), json);
            }
            Err(e) => {
                return emit_error(
                    &format!("JSON serialization error: {e}"),
                    2,
                    OutputFormat::Json,
                );
            }
        }
    }

    let root_prefix = format!("{}/", root.display());

    if let Some(result) = dupes {
        match serde_json::to_value(&result.report) {
            Ok(mut json) => {
                report::strip_root_prefix(&mut json, &root_prefix);
                combined.insert("dupes".into(), json);
            }
            Err(e) => {
                return emit_error(
                    &format!("JSON serialization error: {e}"),
                    2,
                    OutputFormat::Json,
                );
            }
        }
    }

    if let Some(result) = health {
        match serde_json::to_value(&result.report) {
            Ok(mut json) => {
                report::strip_root_prefix(&mut json, &root_prefix);
                report::inject_health_actions(&mut json, crate::health::health_action_opts(result));
                combined.insert("health".into(), json);
            }
            Err(e) => {
                return emit_error(
                    &format!("JSON serialization error: {e}"),
                    2,
                    OutputFormat::Json,
                );
            }
        }
    }

    match serde_json::to_string_pretty(&serde_json::Value::Object(combined)) {
        Ok(json) => {
            println!("{json}");
            ExitCode::SUCCESS
        }
        Err(e) => emit_error(
            &format!("JSON serialization error: {e}"),
            2,
            OutputFormat::Json,
        ),
    }
}

/// Print combined SARIF with multiple runs (one per analysis).
fn print_combined_sarif(
    check: Option<&CheckResult>,
    dupes: Option<&DupesResult>,
    health: Option<&HealthResult>,
) -> ExitCode {
    let mut all_runs = Vec::new();

    if let Some(result) = check {
        let sarif = report::build_sarif(&result.results, &result.config.root, &result.config.rules);
        if let Some(runs) = sarif.get("runs").and_then(|r| r.as_array()) {
            all_runs.extend(runs.iter().cloned());
        }
    }

    // Duplication SARIF builder is pub(super) — serialize the report as a simple run
    if let Some(result) = dupes.filter(|r| !r.report.clone_groups.is_empty()) {
        let run = serde_json::json!({
            "tool": {
                "driver": {
                    "name": "fallow",
                    "version": env!("CARGO_PKG_VERSION"),
                    "informationUri": "https://github.com/fallow-rs/fallow",
                }
            },
            "automationDetails": { "id": "fallow/dupes" },
            "results": result.report.clone_groups.iter().enumerate().map(|(i, g)| {
                serde_json::json!({
                    "ruleId": "fallow/code-duplication",
                    "level": "warning",
                    "message": { "text": format!("Clone group {} ({} lines, {} instances)", i + 1, g.line_count, g.instances.len()) },
                })
            }).collect::<Vec<_>>()
        });
        all_runs.push(run);
    }

    if let Some(result) = health {
        let sarif = report::build_health_sarif(&result.report, &result.config.root);
        if let Some(runs) = sarif.get("runs").and_then(|r| r.as_array()) {
            all_runs.extend(runs.iter().cloned());
        }
    }

    let combined = serde_json::json!({
        "$schema": "https://json.schemastore.org/sarif-2.1.0.json",
        "version": "2.1.0",
        "runs": all_runs,
    });

    match serde_json::to_string_pretty(&combined) {
        Ok(json) => {
            println!("{json}");
            ExitCode::SUCCESS
        }
        Err(e) => emit_error(
            &format!("SARIF serialization error: {e}"),
            2,
            OutputFormat::Sarif,
        ),
    }
}

/// Print combined `CodeClimate` output merging all analyses into one JSON array.
fn print_combined_codeclimate(
    check: Option<&CheckResult>,
    dupes: Option<&DupesResult>,
    health: Option<&HealthResult>,
) -> ExitCode {
    let value = build_combined_codeclimate(check, dupes, health);
    match serde_json::to_string_pretty(&value) {
        Ok(json) => {
            println!("{json}");
            ExitCode::SUCCESS
        }
        Err(e) => emit_error(
            &format!("CodeClimate serialization error: {e}"),
            2,
            OutputFormat::CodeClimate,
        ),
    }
}

fn build_combined_codeclimate(
    check: Option<&CheckResult>,
    dupes: Option<&DupesResult>,
    health: Option<&HealthResult>,
) -> serde_json::Value {
    let mut all_issues = Vec::new();
    if let Some(result) = check
        && let serde_json::Value::Array(items) =
            report::build_codeclimate(&result.results, &result.config.root, &result.config.rules)
    {
        all_issues.extend(items);
    }

    if let Some(result) = dupes
        && let serde_json::Value::Array(items) =
            report::build_duplication_codeclimate(&result.report, &result.config.root)
    {
        all_issues.extend(items);
    }

    if let Some(result) = health
        && let serde_json::Value::Array(items) =
            report::build_health_codeclimate(&result.report, &result.config.root)
    {
        all_issues.extend(items);
    }

    serde_json::Value::Array(all_issues)
}

/// Build the dupes options and dispatch to either `execute_dupes` or
/// `execute_dupes_with_files` depending on whether dead-code already produced
/// a reusable file list (set when both health and dupes share dead-code's
/// production setting). Extracted out of `run_combined` to keep that function
/// under the cognitive-complexity / line-count limits.
fn run_combined_dupes(
    opts: &CombinedOptions<'_>,
    check_result: Option<&CheckResult>,
) -> Result<Option<DupesResult>, ExitCode> {
    let dupes_cfg = load_config_for_analysis(
        opts.root,
        opts.config_path,
        opts.output,
        opts.no_cache,
        opts.threads,
        opts.production_dupes
            .or_else(|| opts.production.then_some(true)),
        opts.quiet,
        fallow_config::ProductionAnalysis::Dupes,
    )?
    .duplicates;

    let dupes_opts = DupesOptions {
        root: opts.root,
        config_path: opts.config_path,
        output: opts.output,
        no_cache: opts.no_cache,
        threads: opts.threads,
        quiet: opts.quiet,
        // Combined mode has already resolved CLI overrides against
        // `dupes_cfg`; pass each as an explicit `Some(...)` so
        // `build_dupes_config` treats them as authoritative instead of
        // re-merging with the toml values a second time.
        mode: Some(
            opts.dupes_mode
                .unwrap_or_else(|| DupesMode::from(dupes_cfg.mode)),
        ),
        min_tokens: Some(dupes_cfg.min_tokens),
        min_lines: Some(dupes_cfg.min_lines),
        threshold: Some(opts.dupes_threshold.unwrap_or(dupes_cfg.threshold)),
        skip_local: dupes_cfg.skip_local,
        cross_language: dupes_cfg.cross_language,
        ignore_imports: dupes_cfg.ignore_imports,
        top: None,
        baseline_path: None,
        save_baseline_path: None,
        production: opts.production_dupes.unwrap_or(opts.production),
        production_override: opts.production_dupes,
        trace: None,
        changed_since: opts.changed_since,
        changed_files: None,
        workspace: opts.workspace,
        changed_workspaces: opts.changed_workspaces,
        explain: opts.explain,
        explain_skipped: opts.explain_skipped,
        summary: opts.summary,
        group_by: opts.group_by,
        // Combined mode renders the bare-`fallow` pipeline panel which already
        // shows the duplication stage; the standalone dupes panel is suppressed
        // here to avoid double-printing.
        performance: false,
    };

    let check_production = opts.production_dead_code.unwrap_or(opts.production);
    let health_production = opts.production_health.unwrap_or(opts.production);
    let dupes_production = opts.production_dupes.unwrap_or(opts.production);
    let share_files_with_dupes = opts.run_health
        && check_production == health_production
        && check_production == dupes_production;
    let dupes_files = if share_files_with_dupes {
        check_result.and_then(|r| r.shared_parse.as_ref().map(|sp| sp.files.clone()))
    } else {
        None
    };

    let dupes_run = if let Some(files) = dupes_files {
        crate::dupes::execute_dupes_with_files(&dupes_opts, files)
    } else {
        crate::dupes::execute_dupes(&dupes_opts)
    };
    dupes_run.map(Some)
}

fn build_health_opts<'a>(opts: &'a CombinedOptions<'a>) -> HealthOptions<'a> {
    HealthOptions {
        root: opts.root,
        config_path: opts.config_path,
        output: opts.output,
        no_cache: opts.no_cache,
        threads: opts.threads,
        quiet: opts.quiet,
        max_cyclomatic: None,
        max_cognitive: None,
        max_crap: None,
        top: None,
        sort: SortBy::Cyclomatic,
        production: opts.production_health.unwrap_or(opts.production),
        production_override: opts.production_health,
        changed_since: opts.changed_since,
        workspace: opts.workspace,
        changed_workspaces: opts.changed_workspaces,
        baseline: None,
        save_baseline: None,
        complexity: true,
        file_scores: true,
        coverage_gaps: false,
        config_activates_coverage_gaps: false,
        hotspots: true,
        ownership: false,
        ownership_emails: None,
        targets: true,
        force_full: false,
        score_only_output: false,
        enforce_coverage_gap_gate: false,
        effort: None,
        score: opts.score || opts.trend,
        min_score: None,
        since: None,
        min_commits: None,
        explain: opts.explain,
        summary: opts.summary,
        save_snapshot: opts
            .save_snapshot
            .map(|opt| std::path::PathBuf::from(opt.as_deref().unwrap_or_default())),
        trend: opts.trend,
        group_by: opts.group_by,
        coverage: None,
        coverage_root: None,
        performance: opts.performance,
        min_severity: None,
        runtime_coverage: None,
        diff_file: opts.diff_file,
    }
}

/// Print orientation header: vital signs summary + start-here nudge.
///
/// Renders a compact one-or-two-line block at the top of combined mode output
/// so users immediately see the project's vital signs and top refactoring target.
fn print_orientation_header(health: &HealthResult, check: Option<&CheckResult>) {
    // Vital signs line (skip when trend table is active — it replaces vital signs)
    if let Some(ref vs) = health.report.vital_signs
        && health.report.health_trend.is_none()
    {
        let mut parts = Vec::new();
        if let Some(dfp) = vs.dead_file_pct {
            if let Some(ref c) = vs.counts {
                parts.push(format!(
                    "dead files {dfp:.1}% ({} of {})",
                    c.dead_files, c.total_files
                ));
            } else {
                parts.push(format!("dead files {dfp:.1}%"));
            }
        }
        if let Some(dep) = vs.dead_export_pct {
            if let Some(ref c) = vs.counts {
                parts.push(format!(
                    "dead exports {dep:.1}% ({} of {})",
                    c.dead_exports, c.total_exports
                ));
            } else {
                parts.push(format!("dead exports {dep:.1}%"));
            }
        }
        if let Some(mi) = vs.maintainability_avg {
            let label = if mi >= 85.0 {
                "good"
            } else if mi >= 65.0 {
                "moderate"
            } else {
                "low"
            };
            parts.push(format!("MI {mi:.1} ({label})"));
        }
        if let Some(hc) = vs.hotspot_count
            && hc > 0
        {
            parts.push(format!(
                "{hc} churn hotspot{}",
                if hc == 1 { "" } else { "s" }
            ));
        }
        if let Some(cd) = vs.circular_dep_count
            && cd > 0
        {
            parts.push(format!(
                "{cd} circular {}",
                if cd == 1 {
                    "dependency"
                } else {
                    "dependencies"
                }
            ));
        }
        if !parts.is_empty() {
            eprintln!();
            eprintln!(
                "{} {} {}",
                "\u{25a0}".dimmed(),
                "Metrics:".dimmed(),
                parts.join(" \u{00b7} ").dimmed()
            );
        }
    }

    // Analysis scope: file count + active plugins
    let files = health.report.summary.files_analyzed;
    let config = check.map_or(&health.config, |c| &c.config);
    let plugin_count = config.external_plugins.len();
    if files > 0 {
        use std::fmt::Write as _;
        let mut scope = format!("  {files} files analyzed");
        if plugin_count > 0 {
            let names: Vec<&str> = config
                .external_plugins
                .iter()
                .take(5)
                .map(|p| p.name.as_str())
                .collect();
            let _ = write!(
                scope,
                ", {plugin_count} plugin{}",
                if plugin_count == 1 { "" } else { "s" }
            );
            let _ = write!(scope, " ({})", names.join(", "));
            if plugin_count > 5 {
                let _ = write!(scope, " +{}", plugin_count - 5);
            }
        }
        eprintln!("{}", scope.dimmed());
    }

    // Entry-point detection summary
    if let Some(result) = check {
        print_entry_point_summary(&result.results);
    }

    // "Start here" nudge: point to top refactoring target
    if !health.report.targets.is_empty() {
        let target_count = health.report.targets.len();
        let total_issues = check.map_or(0, |c| c.results.total_issues());

        if total_issues > 500 {
            // Scale-aware: suggest scoping instead of a specific file
            eprintln!(
                "{}",
                format!(
                    "  {target_count} refactoring target{} \u{2014} try `fallow dead-code --workspace <name>` to scope",
                    if target_count == 1 { "" } else { "s" },
                )
                .dimmed()
            );
        } else {
            // Prefer non-test target; skip nudge if all targets are noise
            if let Some(top) = health
                .report
                .targets
                .iter()
                .find(|t| !is_test_path(&t.path))
            {
                let file_name = top
                    .path
                    .file_name()
                    .map(|f| f.to_string_lossy().to_string())
                    .unwrap_or_default();
                eprintln!(
                    "{}",
                    format!(
                        "  {target_count} refactoring target{} \u{2014} start with {file_name} ({})",
                        if target_count == 1 { "" } else { "s" },
                        top.category.label()
                    )
                    .dimmed()
                );
            } else {
                eprintln!(
                    "{}",
                    format!(
                        "  {target_count} refactoring target{}",
                        if target_count == 1 { "" } else { "s" },
                    )
                    .dimmed()
                );
            }
        }
    }
}

/// Check if a path is a test, fixture, or generated file that shouldn't be
/// recommended as a refactoring starting point.
fn is_test_path(path: &std::path::Path) -> bool {
    // Check directory components for test/fixture/example directories
    if path.components().any(|c| {
        let s = c.as_os_str().to_string_lossy();
        matches!(
            s.as_ref(),
            "test"
                | "tests"
                | "__tests__"
                | "__test__"
                | "spec"
                | "specs"
                | "__mocks__"
                | "__fixtures__"
                | "fixtures"
                | "examples"
                | "example"
                | "__snapshots__"
                | "snapshots"
                | "benchmark"
                | "benchmarks"
                | "bench"
                | "e2e"
                | "playground"
                | "playgrounds"
        )
    }) {
        return true;
    }
    // Check file name patterns
    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
        if name.contains(".test.")
            || name.contains(".spec.")
            || name.contains(".fixture.")
            || name.contains(".e2e.")
            || name.contains(".bench.")
            || name.contains(".story.")
            || name.contains(".stories.")
        {
            return true;
        }
        // Generated file heuristic: single letter + digits (a0.js, b1.mjs)
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        if stem.len() <= 3
            && stem.starts_with(|c: char| c.is_ascii_lowercase())
            && stem[1..].bytes().all(|b| b.is_ascii_digit())
        {
            return true;
        }
    }
    false
}

/// Print entry-point detection summary to stderr.
///
/// Shows a dimmed informational line so users can verify that fallow found the
/// right entry points. When zero entry points are detected, emits a warning
/// with a remediation command.
pub fn print_entry_point_summary(results: &fallow_core::results::AnalysisResults) {
    let Some(ref summary) = results.entry_point_summary else {
        return;
    };
    if summary.total == 0 {
        eprintln!(
            "{}",
            "  \u{26a0} No entry points detected \u{2014} exports may appear unused. Run: fallow list --entry-points"
                .yellow()
        );
        return;
    }
    use std::fmt::Write as _;
    let mut line = format!(
        "  {} entry point{} detected",
        summary.total,
        if summary.total == 1 { "" } else { "s" }
    );
    if !summary.by_source.is_empty() {
        let parts: Vec<String> = summary
            .by_source
            .iter()
            .map(|(source, count)| format!("{count} {source}"))
            .collect();
        let _ = write!(line, " ({})", parts.join(", "));
    }
    eprintln!("{}", line.dimmed());
}
/// Convert an ExitCode to u8 for comparison.
/// ExitCode doesn't implement Ord, so we use this workaround.
fn exit_code_to_u8(code: ExitCode) -> u8 {
    u8::from(code != ExitCode::SUCCESS)
}
