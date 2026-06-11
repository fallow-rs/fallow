use crate::report::sink::outln;
use std::io::IsTerminal;
use std::process::ExitCode;

use colored::Colorize;
use fallow_config::OutputFormat;

use crate::check::CheckResult;
use crate::dupes::DupesResult;
use crate::error::emit_error;
use crate::health::HealthResult;
use crate::regression;
use crate::report;

use super::CombinedOptions;
use super::orientation::{is_test_path, print_entry_point_summary, print_orientation_header};

/// Build ownership resolver, dispatch to format-specific printer, and return
/// the accumulated max exit code. Returns `Err(ExitCode)` for fatal output errors.
pub(super) fn print_combined_report(
    opts: &CombinedOptions<'_>,
    check_result: Option<&CheckResult>,
    dupes_result: Option<&DupesResult>,
    health_result: Option<&HealthResult>,
    total_elapsed: std::time::Duration,
) -> Result<u8, ExitCode> {
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
                opts.config_path.is_some()
                    || fallow_config::FallowConfig::find_config_path(opts.root).is_some(),
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
            let issues =
                build_combined_codeclimate_issues(check_result, dupes_result, health_result);
            let code = report::ci::pr_comment::print_pr_comment_from_codeclimate_issues(
                "combined",
                report::ci::pr_comment::Provider::Github,
                &issues,
            );
            if code != ExitCode::SUCCESS {
                return Err(code);
            }
        }
        OutputFormat::PrCommentGitlab => {
            let issues =
                build_combined_codeclimate_issues(check_result, dupes_result, health_result);
            let code = report::ci::pr_comment::print_pr_comment_from_codeclimate_issues(
                "combined",
                report::ci::pr_comment::Provider::Gitlab,
                &issues,
            );
            if code != ExitCode::SUCCESS {
                return Err(code);
            }
        }
        OutputFormat::ReviewGithub => {
            let issues =
                build_combined_codeclimate_issues(check_result, dupes_result, health_result);
            let code = report::ci::review::print_review_envelope_from_codeclimate_issues(
                "combined",
                report::ci::pr_comment::Provider::Github,
                &issues,
            );
            if code != ExitCode::SUCCESS {
                return Err(code);
            }
        }
        OutputFormat::ReviewGitlab => {
            let issues =
                build_combined_codeclimate_issues(check_result, dupes_result, health_result);
            let code = report::ci::review::print_review_envelope_from_codeclimate_issues(
                "combined",
                report::ci::pr_comment::Provider::Gitlab,
                &issues,
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

    if show_headers {
        if let Some(result) = health_result {
            print_orientation_header(result, check_result, opts.root);
        } else if let Some(result) = check_result {
            print_entry_point_summary(&result.results);
        }
    }

    let has_any_findings = check_result.is_some_and(|result| result.results.total_issues() > 0)
        || dupes_result.is_some_and(|result| !result.report.clone_groups.is_empty())
        || health_result.is_some_and(|result| !result.report.findings.is_empty());
    if show_headers
        && has_any_findings
        && std::io::stdout().is_terminal()
        && !crate::report::sink::is_redirected()
    {
        println!(
            "{}",
            "Tip: run `fallow explain <issue label>`; spaces and hyphens both work, e.g. `fallow explain unused files`."
                .dimmed()
        );
        println!();

        let dupes_payload = dupes_result
            .map(|result| crate::output_dupes::DupesReportPayload::from_report(&result.report));
        if let Some(step) = crate::report::suggestions::top_combined_next_step(
            check_result.map(|result| &result.results),
            dupes_payload.as_ref(),
            health_result.map(|result| &result.report),
            opts.root,
        ) {
            println!(
                "{}",
                format!("Next: {}  ({})", step.command, step.reason).dimmed()
            );
            println!();
        }
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
                summary_heading: !show_headers,
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
        let code = crate::dupes::print_dupes_result(
            result,
            opts.quiet,
            opts.explain,
            opts.summary,
            !show_headers,
            false,
        );
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
            crate::health::HealthPrintOptions {
                quiet: opts.quiet,
                explain: opts.explain,
                min_score: None,
                min_severity: None,
                report_only: false,
                summary: opts.summary,
                summary_heading: !show_headers,
                show_explain_tip: false,
                skip_score_and_trend: true,
            },
        );
        max_exit = max_exit.max(exit_code_to_u8(code));
    }

    max_exit
}

/// Handle regression outcome and print failure summary.
pub(super) fn handle_regression_and_summary(
    max_exit: &mut u8,
    quiet: bool,
    root: &std::path::Path,
    check_result: Option<&CheckResult>,
    dupes_result: Option<&DupesResult>,
    health_result: Option<&HealthResult>,
) {
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

    if *max_exit > 0 && !quiet {
        print_failure_summary(root, check_result, dupes_result, health_result);
    }
}

/// Print a summary line listing which analyses had failures.
fn print_failure_summary(
    root: &std::path::Path,
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
        let nudge = health_result
            .filter(|r| !r.report.targets.is_empty())
            .map(|r| {
                if let Some(top) = r.report.targets.iter().find(|t| !is_test_path(&t.path)) {
                    let name = report::format_display_path(&top.path, root);
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
    explain: bool,
    config_fixable: bool,
) -> ExitCode {
    let envelope = crate::output_envelope::CombinedOutput {
        schema_version: fallow_types::envelope::SchemaVersion(crate::report::SCHEMA_VERSION),
        version: fallow_types::envelope::ToolVersion(env!("CARGO_PKG_VERSION").to_string()),
        elapsed_ms: fallow_types::envelope::ElapsedMs(elapsed.as_millis() as u64),
        meta: None,
        check: None,
        dupes: None,
        health: None,
        // Aggregated and injected into the map below, after the sub-blocks.
        next_steps: Vec::new(),
    };
    let mut combined = match crate::output_envelope::serialize_root_output(
        crate::output_envelope::FallowOutput::Combined(envelope),
    ) {
        Ok(serde_json::Value::Object(map)) => map,
        Ok(_) => unreachable!("CombinedOutput serializes as a JSON object"),
        Err(e) => {
            return emit_error(
                &format!("JSON serialization error: {e}"),
                2,
                OutputFormat::Json,
            );
        }
    };

    if let Some(result) = check {
        match report::build_check_json_payload_with_config_fixable(
            &result.results,
            &result.config.root,
            result.elapsed,
            config_fixable,
        ) {
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

    // Build the dupes payload once: reused for the sub-block AND the aggregated
    // next-steps below (fingerprints live on the payload, not the raw report).
    let dupes_payload =
        dupes.map(|result| crate::output_dupes::DupesReportPayload::from_report(&result.report));

    if let Some(payload) = dupes_payload.as_ref() {
        match serde_json::to_value(payload) {
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

    let next_steps = crate::report::suggestions::build_combined_next_steps(
        check.map(|result| &result.results),
        dupes_payload.as_ref(),
        health.map(|result| &result.report),
        root,
    );
    if !next_steps.is_empty() {
        match serde_json::to_value(&next_steps) {
            Ok(value) => {
                combined.insert("next_steps".into(), value);
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

    let mut output = serde_json::Value::Object(combined);
    if explain && let serde_json::Value::Object(ref mut map) = output {
        map.insert(
            "_meta".to_string(),
            crate::explain::combined_meta(check.is_some(), dupes.is_some(), health.is_some()),
        );
    }
    report::harmonize_multi_kind_suppress_line_actions(&mut output);
    crate::output_envelope::attach_telemetry_meta(&mut output);

    match serde_json::to_string_pretty(&output) {
        Ok(json) => {
            outln!("{json}");
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
            outln!("{json}");
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
            outln!("{json}");
            ExitCode::SUCCESS
        }
        Err(e) => emit_error(
            &format!("CodeClimate serialization error: {e}"),
            2,
            OutputFormat::CodeClimate,
        ),
    }
}

#[expect(
    clippy::expect_used,
    reason = "CodeClimate issue envelope contains only infallibly serializable fields"
)]
fn build_combined_codeclimate(
    check: Option<&CheckResult>,
    dupes: Option<&DupesResult>,
    health: Option<&HealthResult>,
) -> serde_json::Value {
    let all_issues = build_combined_codeclimate_issues(check, dupes, health);
    serde_json::to_value(&all_issues).expect("CodeClimateIssue serializes infallibly")
}

fn build_combined_codeclimate_issues(
    check: Option<&CheckResult>,
    dupes: Option<&DupesResult>,
    health: Option<&HealthResult>,
) -> Vec<crate::output_envelope::CodeClimateIssue> {
    let mut all_issues: Vec<crate::output_envelope::CodeClimateIssue> = Vec::new();
    if let Some(result) = check {
        all_issues.extend(report::build_codeclimate(
            &result.results,
            &result.config.root,
            &result.config.rules,
        ));
    }

    if let Some(result) = dupes {
        all_issues.extend(report::build_duplication_codeclimate(
            &result.report,
            &result.config.root,
        ));
    }

    if let Some(result) = health {
        all_issues.extend(report::build_health_codeclimate(
            &result.report,
            &result.config.root,
        ));
    }

    all_issues
}

/// Convert an ExitCode to u8 for comparison.
/// ExitCode doesn't implement Ord, so we use this workaround.
pub(super) fn exit_code_to_u8(code: ExitCode) -> u8 {
    u8::from(code != ExitCode::SUCCESS)
}
