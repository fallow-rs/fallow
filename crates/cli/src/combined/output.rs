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

    if let Some(code) = print_machine_combined_report(
        opts,
        check_result,
        dupes_result,
        health_result,
        total_elapsed,
    )? {
        return Ok(code);
    }

    Ok(print_human_sections(
        opts,
        check_result,
        dupes_result,
        health_result,
        resolver,
    ))
}

fn print_machine_combined_report(
    opts: &CombinedOptions<'_>,
    check_result: Option<&CheckResult>,
    dupes_result: Option<&DupesResult>,
    health_result: Option<&HealthResult>,
    total_elapsed: std::time::Duration,
) -> Result<Option<u8>, ExitCode> {
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
            combined_machine_success(code)
        }
        OutputFormat::Sarif => {
            let code = print_combined_sarif(check_result, dupes_result, health_result);
            combined_machine_success(code)
        }
        OutputFormat::CodeClimate => {
            let code = print_combined_codeclimate(check_result, dupes_result, health_result);
            combined_machine_success(code)
        }
        OutputFormat::PrCommentGithub => {
            print_combined_pr_comment(check_result, dupes_result, health_result, true)
        }
        OutputFormat::PrCommentGitlab => {
            print_combined_pr_comment(check_result, dupes_result, health_result, false)
        }
        OutputFormat::ReviewGithub => {
            print_combined_review(check_result, dupes_result, health_result, true)
        }
        OutputFormat::ReviewGitlab => {
            print_combined_review(check_result, dupes_result, health_result, false)
        }
        _ => Ok(None),
    }
}

fn combined_machine_success(code: ExitCode) -> Result<Option<u8>, ExitCode> {
    if code != ExitCode::SUCCESS {
        return Err(code);
    }
    Ok(Some(0))
}

fn combined_provider(github: bool) -> report::ci::pr_comment::Provider {
    if github {
        report::ci::pr_comment::Provider::Github
    } else {
        report::ci::pr_comment::Provider::Gitlab
    }
}

fn print_combined_pr_comment(
    check_result: Option<&CheckResult>,
    dupes_result: Option<&DupesResult>,
    health_result: Option<&HealthResult>,
    github: bool,
) -> Result<Option<u8>, ExitCode> {
    let issues = build_combined_codeclimate_issues(check_result, dupes_result, health_result);
    let code = report::ci::pr_comment::print_pr_comment_from_codeclimate_issues(
        "combined",
        combined_provider(github),
        &issues,
    );
    combined_machine_success(code)
}

fn print_combined_review(
    check_result: Option<&CheckResult>,
    dupes_result: Option<&DupesResult>,
    health_result: Option<&HealthResult>,
    github: bool,
) -> Result<Option<u8>, ExitCode> {
    let issues = build_combined_codeclimate_issues(check_result, dupes_result, health_result);
    let code = report::ci::review::print_review_envelope_from_codeclimate_issues(
        "combined",
        combined_provider(github),
        &issues,
    );
    combined_machine_success(code)
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
    print_combined_hints(
        opts,
        check_result,
        dupes_result,
        health_result,
        show_headers,
        has_any_findings,
    );

    max_exit = max_exit.max(print_check_section(
        opts,
        check_result,
        resolver,
        show_headers,
    ));
    max_exit = max_exit.max(print_dupes_section(opts, dupes_result, show_headers));
    max_exit = max_exit.max(print_health_section(opts, health_result, show_headers));

    max_exit
}

fn print_combined_hints(
    opts: &CombinedOptions<'_>,
    check_result: Option<&CheckResult>,
    dupes_result: Option<&DupesResult>,
    health_result: Option<&HealthResult>,
    show_headers: bool,
    has_any_findings: bool,
) {
    if !show_headers
        || !has_any_findings
        || !std::io::stdout().is_terminal()
        || crate::report::sink::is_redirected()
    {
        return;
    }

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

fn print_check_section(
    opts: &CombinedOptions<'_>,
    check_result: Option<&CheckResult>,
    resolver: Option<report::OwnershipResolver>,
    show_headers: bool,
) -> u8 {
    let Some(result) = check_result else {
        return 0;
    };
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
    exit_code_to_u8(code)
}

fn print_dupes_section(
    opts: &CombinedOptions<'_>,
    dupes_result: Option<&DupesResult>,
    show_headers: bool,
) -> u8 {
    let Some(result) = dupes_result else {
        return 0;
    };
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
    exit_code_to_u8(code)
}

fn print_health_section(
    opts: &CombinedOptions<'_>,
    health_result: Option<&HealthResult>,
    show_headers: bool,
) -> u8 {
    let Some(result) = health_result else {
        return 0;
    };
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
    exit_code_to_u8(code)
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
                    format!(": start with {name}")
                } else {
                    String::new()
                }
            })
            .unwrap_or_default();
        eprintln!("\nFailed: {}{nudge}", parts.join(", "));

        // Periodic value digest: prose counterpart of the `impact-report`
        // next-step, at most weekly (the cadence stamp lives in the impact
        // store) and only with non-zero numbers. Shares the caller's quiet
        // gate; CI and disabled suggestions suppress it inside the peek.
        if let Some(digest) = crate::report::suggestions::due_impact_digest(root) {
            eprintln!(
                "{}",
                crate::report::suggestions::impact_digest_line(digest).dimmed()
            );
        }

        // First-contact setup hint: prose counterpart of the `setup`
        // next-step, printed after the failure summary so it is the last
        // thing a human reads on a big first run instead of scrolling away
        // with the header. Deliberately not TTY-gated (agents reading piped
        // human output are a primary audience); quiet is gated by the caller,
        // and CI, configured projects, suggestions off, and a recorded
        // decline (`fallow init --decline`) suppress it here.
        if crate::report::suggestions::suggestions_enabled()
            && crate::report::suggestions::setup_pointer_applicable(root)
        {
            eprintln!("{}", crate::report::suggestions::SETUP_HINT.dimmed());
        }
    }
}

/// Print combined JSON output wrapping check, dupes, and health results.
fn print_combined_json(
    check: Option<&CheckResult>,
    dupes: Option<&DupesResult>,
    health: Option<&HealthResult>,
    root: &std::path::Path,
    elapsed: std::time::Duration,
    explain: bool,
    config_fixable: bool,
) -> ExitCode {
    let mut combined = match combined_json_root(elapsed) {
        Ok(combined) => combined,
        Err(code) => return code,
    };
    let root_prefix = format!("{}/", root.display());

    if let Some(result) = check
        && let Err(code) = insert_combined_check_json(&mut combined, result, config_fixable)
    {
        return code;
    }

    let dupes_payload =
        dupes.map(|result| crate::output_dupes::DupesReportPayload::from_report(&result.report));
    if let Err(code) = insert_combined_dupes_json(&mut combined, dupes_payload.as_ref(), root) {
        return code;
    }
    if let Err(code) = insert_combined_health_json(&mut combined, health, &root_prefix) {
        return code;
    }
    if let Err(code) =
        insert_combined_next_steps(&mut combined, check, dupes_payload.as_ref(), health, root)
    {
        return code;
    }

    emit_combined_json_output(combined, check, dupes, health, explain)
}

#[expect(
    clippy::cast_possible_truncation,
    reason = "elapsed milliseconds won't exceed u64::MAX"
)]
fn combined_json_root(
    elapsed: std::time::Duration,
) -> Result<serde_json::Map<String, serde_json::Value>, ExitCode> {
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
    match crate::output_envelope::serialize_root_output(
        crate::output_envelope::FallowOutput::Combined(envelope),
    ) {
        Ok(serde_json::Value::Object(map)) => Ok(map),
        Ok(_) => unreachable!("CombinedOutput serializes as a JSON object"),
        Err(e) => Err(emit_error(
            &format!("JSON serialization error: {e}"),
            2,
            OutputFormat::Json,
        )),
    }
}

fn insert_combined_dupes_json(
    combined: &mut serde_json::Map<String, serde_json::Value>,
    dupes_payload: Option<&crate::output_dupes::DupesReportPayload>,
    root: &std::path::Path,
) -> Result<(), ExitCode> {
    let root_prefix = format!("{}/", root.display());
    if let Some(payload) = dupes_payload {
        match serde_json::to_value(payload) {
            Ok(mut json) => {
                report::strip_root_prefix(&mut json, &root_prefix);
                combined.insert("dupes".into(), json);
            }
            Err(e) => {
                return Err(emit_error(
                    &format!("JSON serialization error: {e}"),
                    2,
                    OutputFormat::Json,
                ));
            }
        }
    }
    Ok(())
}

fn insert_combined_health_json(
    combined: &mut serde_json::Map<String, serde_json::Value>,
    health: Option<&HealthResult>,
    root_prefix: &str,
) -> Result<(), ExitCode> {
    if let Some(result) = health {
        match serde_json::to_value(&result.report) {
            Ok(mut json) => {
                report::strip_root_prefix(&mut json, root_prefix);
                combined.insert("health".into(), json);
            }
            Err(e) => {
                return Err(emit_error(
                    &format!("JSON serialization error: {e}"),
                    2,
                    OutputFormat::Json,
                ));
            }
        }
    }
    Ok(())
}

fn insert_combined_next_steps(
    combined: &mut serde_json::Map<String, serde_json::Value>,
    check: Option<&CheckResult>,
    dupes_payload: Option<&crate::output_dupes::DupesReportPayload>,
    health: Option<&HealthResult>,
    root: &std::path::Path,
) -> Result<(), ExitCode> {
    let next_steps = crate::report::suggestions::build_combined_next_steps(
        check.map(|result| &result.results),
        dupes_payload,
        health.map(|result| &result.report),
        root,
        crate::report::suggestions::setup_pointer_applicable(root),
        crate::report::suggestions::due_impact_digest(root),
    );
    if !next_steps.is_empty() {
        match serde_json::to_value(&next_steps) {
            Ok(value) => {
                combined.insert("next_steps".into(), value);
            }
            Err(e) => {
                return Err(emit_error(
                    &format!("JSON serialization error: {e}"),
                    2,
                    OutputFormat::Json,
                ));
            }
        }
    }
    Ok(())
}

fn emit_combined_json_output(
    combined: serde_json::Map<String, serde_json::Value>,
    check: Option<&CheckResult>,
    dupes: Option<&DupesResult>,
    health: Option<&HealthResult>,
    explain: bool,
) -> ExitCode {
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

fn insert_combined_check_json(
    combined: &mut serde_json::Map<String, serde_json::Value>,
    result: &CheckResult,
    config_fixable: bool,
) -> Result<(), ExitCode> {
    let mut json = report::build_check_json_payload_with_config_fixable(
        &result.results,
        &result.config.root,
        result.elapsed,
        config_fixable,
    )
    .map_err(|error| json_output_error(&error))?;
    attach_combined_check_extras(&mut json, result);
    combined.insert("check".into(), json);
    Ok(())
}

fn attach_combined_check_extras(json: &mut serde_json::Value, result: &CheckResult) {
    let serde_json::Value::Object(map) = json else {
        return;
    };
    if let Some(ref outcome) = result.regression {
        map.insert("regression".to_string(), outcome.to_json());
    }
    if let Some(ref deltas) = result.baseline_deltas {
        map.insert(
            "baseline_deltas".to_string(),
            report::build_baseline_deltas_json(
                deltas.total_delta,
                deltas
                    .per_category
                    .iter()
                    .map(|(cat, delta)| (cat.as_str(), delta.current, delta.baseline, delta.delta)),
            ),
        );
    }
    if let Some((entries, matched)) = result.baseline_matched {
        map.insert(
            "baseline".to_string(),
            serde_json::json!({
                "entries": entries,
                "matched": matched,
            }),
        );
    }
}

fn json_output_error(error: &serde_json::Error) -> ExitCode {
    emit_error(
        &format!("JSON serialization error: {error}"),
        2,
        OutputFormat::Json,
    )
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

#[cfg(test)]
mod tests {
    use std::process::ExitCode;
    use std::time::Duration;

    use super::{
        build_combined_codeclimate, combined_json_root, combined_machine_success,
        emit_combined_json_output, exit_code_to_u8, insert_combined_dupes_json,
        insert_combined_health_json, print_combined_codeclimate, print_combined_sarif,
    };

    #[test]
    fn combined_machine_success_maps_success_to_zero_exit() {
        assert_eq!(combined_machine_success(ExitCode::SUCCESS), Ok(Some(0)));
    }

    #[test]
    fn combined_machine_success_preserves_output_error() {
        let code = ExitCode::from(2);

        assert_eq!(combined_machine_success(code), Err(code));
    }

    #[test]
    fn exit_code_to_u8_collapses_non_success_codes() {
        assert_eq!(exit_code_to_u8(ExitCode::SUCCESS), 0);
        assert_eq!(exit_code_to_u8(ExitCode::from(1)), 1);
        assert_eq!(exit_code_to_u8(ExitCode::from(2)), 1);
    }

    #[test]
    fn combined_json_root_contains_stable_envelope_fields() {
        let root = combined_json_root(Duration::from_millis(42)).expect("combined JSON root");

        assert_eq!(
            root.get("kind").and_then(serde_json::Value::as_str),
            Some("combined")
        );
        assert_eq!(
            root.get("elapsed_ms").and_then(serde_json::Value::as_u64),
            Some(42)
        );
        assert!(root.get("schema_version").is_some());
        assert!(root.get("version").is_some());
    }

    #[test]
    fn empty_combined_codeclimate_output_is_an_empty_issue_list() {
        assert_eq!(
            build_combined_codeclimate(None, None, None),
            serde_json::json!([])
        );
    }

    #[test]
    fn empty_combined_machine_printers_succeed() {
        assert_eq!(print_combined_sarif(None, None, None), ExitCode::SUCCESS);
        assert_eq!(
            print_combined_codeclimate(None, None, None),
            ExitCode::SUCCESS
        );
    }

    #[test]
    fn insert_empty_optional_sections_leaves_combined_map_unchanged() {
        let mut combined = serde_json::Map::new();
        insert_combined_dupes_json(&mut combined, None, std::path::Path::new("."))
            .expect("empty dupes section should serialize");
        insert_combined_health_json(&mut combined, None, ".")
            .expect("empty health section should serialize");

        assert!(combined.is_empty());
    }

    #[test]
    fn emit_combined_json_output_can_attach_empty_meta() {
        let combined = combined_json_root(Duration::ZERO).expect("combined JSON root");

        assert_eq!(
            emit_combined_json_output(combined, None, None, None, true),
            ExitCode::SUCCESS
        );
    }
}
