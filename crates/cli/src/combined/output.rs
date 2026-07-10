use crate::report::sink::outln;
use std::io::IsTerminal;
use std::path::Path;
use std::process::ExitCode;

use colored::Colorize;
use fallow_api::{CombinedCheckJsonSection, CombinedJsonOutputInput, DupesReportPayload};
use fallow_config::OutputFormat;
use fallow_output::{
    CiIssue, CodeClimateIssue, PR_DECISION_SCHEMA, PrCommentEnvelope, PrDecisionConclusion,
    PrDecisionDetails, PrDecisionGate, PrDecisionSurface, PrSummaryArea, PrSummaryFinding,
    PrSummaryInput, PrSummaryScope, PrSummaryStatus, codeclimate_issues_to_value,
    issues_from_codeclimate_issues, render_pr_summary,
};

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
            let code = print_combined_json(CombinedJsonPrintInput {
                check_result,
                dupes_result,
                health_result,
                root: opts.root,
                elapsed: total_elapsed,
                explain: opts.explain,
                config_fixable: opts.config_path.is_some()
                    || fallow_config::FallowConfig::find_config_path(opts.root).is_some(),
            });
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
            print_combined_pr_comment(opts, check_result, dupes_result, health_result, true)
        }
        OutputFormat::PrCommentGitlab => {
            print_combined_pr_comment(opts, check_result, dupes_result, health_result, false)
        }
        OutputFormat::ReviewGithub => {
            print_combined_review(check_result, dupes_result, health_result, true)
        }
        OutputFormat::ReviewGitlab => {
            print_combined_review(check_result, dupes_result, health_result, false)
        }
        OutputFormat::GithubAnnotations | OutputFormat::GithubSummary => {
            let code = print_combined_github_format(
                CombinedJsonPrintInput {
                    check_result,
                    dupes_result,
                    health_result,
                    root: opts.root,
                    elapsed: total_elapsed,
                    explain: opts.explain,
                    config_fixable: opts.config_path.is_some()
                        || fallow_config::FallowConfig::find_config_path(opts.root).is_some(),
                },
                matches!(opts.output, OutputFormat::GithubSummary),
            );
            combined_machine_success(code)
        }
        _ => Ok(None),
    }
}

/// Render the combined run in a GitHub-native format from the same combined
/// JSON envelope `--format json` serializes.
fn print_combined_github_format(input: CombinedJsonPrintInput<'_>, summary: bool) -> ExitCode {
    let root = input.root;
    match build_combined_json_output(input) {
        Ok(envelope) => {
            if summary {
                report::github_summary::print_summary(
                    report::github_annotations::EnvelopeKind::Combined,
                    &envelope,
                    root,
                )
            } else {
                report::github_annotations::print_annotations(
                    report::github_annotations::EnvelopeKind::Combined,
                    &envelope,
                    root,
                )
            }
        }
        Err(code) => code,
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
    opts: &CombinedOptions<'_>,
    check_result: Option<&CheckResult>,
    dupes_result: Option<&DupesResult>,
    health_result: Option<&HealthResult>,
    github: bool,
) -> Result<Option<u8>, ExitCode> {
    let envelope = build_combined_pr_summary(
        opts.fail_on_issues,
        check_result,
        dupes_result,
        health_result,
        combined_provider(github),
    );
    let decision = build_combined_pr_decision(
        opts.fail_on_issues,
        check_result,
        dupes_result,
        health_result,
        &envelope,
    );
    let details = build_combined_pr_details(check_result, dupes_result, health_result);
    report::ci::pr_comment::write_pr_comment_envelope_sidecar(&envelope);
    report::ci::pr_comment::write_pr_decision_sidecar(&decision);
    report::ci::pr_comment::write_pr_details_sidecar(&details);
    outln!("{}", envelope.body());
    combined_machine_success(ExitCode::SUCCESS)
}

fn build_combined_pr_summary(
    fail_on_issues: bool,
    check_result: Option<&CheckResult>,
    dupes_result: Option<&DupesResult>,
    health_result: Option<&HealthResult>,
    provider: report::ci::pr_comment::Provider,
) -> PrCommentEnvelope {
    let codeclimate = build_combined_codeclimate_issues(check_result, dupes_result, health_result);
    let findings = combined_pr_summary_findings(&codeclimate);
    let areas =
        combined_pr_summary_areas(fail_on_issues, check_result, dupes_result, health_result);

    render_pr_summary(&PrSummaryInput {
        command: "combined",
        provider,
        marker_id: report::ci::pr_comment::sticky_marker_id(),
        scope: PrSummaryScope::Project,
        areas: &areas,
        findings: &findings,
        max_findings: report::ci::pr_comment::max_comments(),
        details_url: None,
        layout: report::ci::pr_comment::pr_comment_layout_from_env(),
    })
}

fn build_combined_pr_decision(
    fail_on_issues: bool,
    check_result: Option<&CheckResult>,
    dupes_result: Option<&DupesResult>,
    health_result: Option<&HealthResult>,
    envelope: &PrCommentEnvelope,
) -> PrDecisionSurface {
    let codeclimate = build_combined_codeclimate_issues(check_result, dupes_result, health_result);
    let issues = report::ci::diff_filter::filter_issues_for_summary(
        issues_from_codeclimate_issues(&codeclimate),
    );
    let gates =
        combined_pr_summary_areas(fail_on_issues, check_result, dupes_result, health_result)
            .into_iter()
            .map(pr_decision_gate_from_summary_area)
            .collect::<Vec<_>>();
    let conclusion = combined_decision_conclusion(&gates);
    let summary_markdown = combined_decision_summary(conclusion, issues.len(), &gates);

    PrDecisionSurface {
        schema: PR_DECISION_SCHEMA.to_owned(),
        title: "Fallow".to_owned(),
        conclusion,
        gates,
        annotations: issues
            .iter()
            .take(report::ci::pr_comment::max_comments())
            .map(report::ci::pr_comment::decision_annotation_from_issue)
            .collect(),
        details: PrDecisionDetails {
            summary_markdown,
            full_report_path: None,
            details_url: envelope.details_url.clone(),
        },
    }
}

fn build_combined_pr_details(
    check_result: Option<&CheckResult>,
    dupes_result: Option<&DupesResult>,
    health_result: Option<&HealthResult>,
) -> fallow_output::PrDetailsArtifact {
    let codeclimate = build_combined_codeclimate_issues(check_result, dupes_result, health_result);
    let issues = report::ci::diff_filter::filter_issues_for_summary(
        issues_from_codeclimate_issues(&codeclimate),
    );
    report::ci::pr_comment::build_pr_details_artifact("combined", &issues)
}

fn pr_decision_gate_from_summary_area(area: PrSummaryArea) -> PrDecisionGate {
    PrDecisionGate {
        id: area.name.to_ascii_lowercase().replace(' ', "-"),
        label: area.name,
        status: pr_decision_conclusion_from_summary_status(area.status),
        observed: area.result,
        threshold: area.threshold,
        scope: "new code".to_owned(),
    }
}

fn pr_decision_conclusion_from_summary_status(status: PrSummaryStatus) -> PrDecisionConclusion {
    match status {
        PrSummaryStatus::Fail => PrDecisionConclusion::Failure,
        PrSummaryStatus::Warn | PrSummaryStatus::Info => PrDecisionConclusion::Neutral,
        PrSummaryStatus::Pass => PrDecisionConclusion::Success,
    }
}

fn combined_decision_conclusion(gates: &[PrDecisionGate]) -> PrDecisionConclusion {
    if gates
        .iter()
        .any(|gate| gate.status == PrDecisionConclusion::Failure)
    {
        return PrDecisionConclusion::Failure;
    }
    if gates
        .iter()
        .any(|gate| gate.status == PrDecisionConclusion::Neutral)
    {
        return PrDecisionConclusion::Neutral;
    }
    PrDecisionConclusion::Success
}

fn combined_decision_summary(
    conclusion: PrDecisionConclusion,
    issue_count: usize,
    gates: &[PrDecisionGate],
) -> String {
    if issue_count == 0
        && gates
            .iter()
            .all(|gate| gate.status == PrDecisionConclusion::Success)
    {
        return "Fallow found no actionable PR findings.".to_owned();
    }
    let notable = gates
        .iter()
        .filter(|gate| gate.status != PrDecisionConclusion::Success)
        .map(|gate| format!("{}: {}", gate.label, gate.observed))
        .collect::<Vec<_>>()
        .join("; ");
    let findings = (issue_count > 0).then(|| count_label(issue_count, "finding", "findings"));
    match conclusion {
        PrDecisionConclusion::Failure => match findings {
            Some(findings) => format!("Fallow quality gates failed with {findings}. {notable}"),
            None => format!("Fallow quality gates failed. {notable}"),
        },
        PrDecisionConclusion::Neutral => match findings {
            Some(findings) => format!("Fallow found {findings} for review. {notable}"),
            None => format!("Fallow found gate warnings for review. {notable}"),
        },
        PrDecisionConclusion::Success | PrDecisionConclusion::Skipped => findings.map_or_else(
            || "Fallow found no actionable PR findings.".to_owned(),
            |findings| format!("Fallow found {findings}."),
        ),
    }
}

fn combined_pr_summary_findings(codeclimate: &[CodeClimateIssue]) -> Vec<PrSummaryFinding> {
    let issues = report::ci::diff_filter::filter_issues_for_summary(
        issues_from_codeclimate_issues(codeclimate),
    );
    issues.iter().map(pr_summary_finding_from_ci).collect()
}

fn pr_summary_finding_from_ci(issue: &CiIssue) -> PrSummaryFinding {
    PrSummaryFinding {
        severity: issue.severity.clone(),
        rule_id: issue.rule_id.clone(),
        location: format!("{}:{}", issue.path, issue.line),
        description: issue.description.clone(),
        fix: report::ci::suggestion::fix_intent(issue).map(str::to_owned),
    }
}

fn combined_pr_summary_areas(
    fail_on_issues: bool,
    check_result: Option<&CheckResult>,
    dupes_result: Option<&DupesResult>,
    health_result: Option<&HealthResult>,
) -> Vec<PrSummaryArea> {
    let mut areas = Vec::new();
    if let Some(result) = check_result {
        areas.push(dead_code_pr_summary_area(result, fail_on_issues));
    }
    if let Some(result) = dupes_result {
        areas.push(dupes_pr_summary_area(result, fail_on_issues));
    }
    if let Some(result) = health_result {
        areas.push(health_pr_summary_area(result));
    }
    areas
}

fn dead_code_pr_summary_area(result: &CheckResult, fail_on_issues: bool) -> PrSummaryArea {
    let count = result.results.total_issues();
    PrSummaryArea {
        name: "Dead code".to_owned(),
        status: if count == 0 {
            PrSummaryStatus::Pass
        } else if fail_on_issues {
            PrSummaryStatus::Fail
        } else {
            PrSummaryStatus::Warn
        },
        result: count_label(count, "issue", "issues"),
        threshold: Some("configured rules".to_owned()),
        details: result
            .baseline_deltas
            .as_ref()
            .map(|deltas| baseline_delta_label(deltas.total_delta)),
    }
}

fn dupes_pr_summary_area(result: &DupesResult, fail_on_issues: bool) -> PrSummaryArea {
    let count = result.report.clone_groups.len();
    let exceeds_threshold =
        result.threshold > 0.0 && result.report.stats.duplication_percentage > result.threshold;
    PrSummaryArea {
        name: "Duplication".to_owned(),
        status: if count == 0 {
            PrSummaryStatus::Pass
        } else if exceeds_threshold || fail_on_issues {
            PrSummaryStatus::Fail
        } else {
            PrSummaryStatus::Warn
        },
        result: count_label(count, "clone group", "clone groups"),
        threshold: dupes_threshold_label(result.threshold),
        details: Some(format!(
            "{:.1}% duplicated lines",
            result.report.stats.duplication_percentage
        )),
    }
}

fn health_pr_summary_area(result: &HealthResult) -> PrSummaryArea {
    let count = result.report.findings.len();
    PrSummaryArea {
        name: "Health".to_owned(),
        status: if count == 0 {
            PrSummaryStatus::Pass
        } else {
            PrSummaryStatus::Fail
        },
        result: count_label(count, "finding", "findings"),
        threshold: Some("configured complexity gates".to_owned()),
        details: result
            .report
            .health_score
            .as_ref()
            .map(|score| format!("score {:.0} ({})", score.score.round(), score.grade)),
    }
}

fn count_label(count: usize, singular: &str, plural: &str) -> String {
    let noun = if count == 1 { singular } else { plural };
    format!("{count} {noun}")
}

fn baseline_delta_label(delta: i64) -> String {
    match delta.cmp(&0) {
        std::cmp::Ordering::Greater => format!("+{delta} since baseline"),
        std::cmp::Ordering::Less => format!("{delta} since baseline"),
        std::cmp::Ordering::Equal => "no baseline change".to_owned(),
    }
}

fn dupes_threshold_label(threshold: f64) -> Option<String> {
    (threshold > 0.0).then(|| format!("<= {threshold:.1}% duplicated lines"))
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

    let dupes_payload = dupes_result.map(|result| DupesReportPayload::from_report(&result.report));
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
            gates: fallow_engine::health::HealthGateOptions::default(),
            summary: opts.summary,
            summary_heading: !show_headers,
            show_explain_tip: false,
            skip_score_and_trend: true,
            css_requested: false,
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
    root: &Path,
    check_result: Option<&CheckResult>,
    dupes_result: Option<&DupesResult>,
    health_result: Option<&HealthResult>,
) {
    let parts = failure_summary_parts(check_result, dupes_result, health_result);
    if parts.is_empty() {
        return;
    }

    let nudge = health_failure_nudge(root, health_result);
    eprintln!("\nFailed: {}{nudge}", parts.join(", "));
    print_failure_followups(root);
}

fn failure_summary_parts(
    check_result: Option<&CheckResult>,
    dupes_result: Option<&DupesResult>,
    health_result: Option<&HealthResult>,
) -> Vec<String> {
    let mut parts = Vec::new();
    if let Some(r) = check_result
        && let Some(part) = check_failure_summary_part(r)
    {
        parts.push(part);
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
    parts
}

fn check_failure_summary_part(result: &CheckResult) -> Option<String> {
    let issues = result.results.total_issues();
    if issues == 0 {
        return None;
    }

    let delta_suffix = result
        .baseline_deltas
        .as_ref()
        .map_or_else(String::new, |d| match d.total_delta.cmp(&0) {
            std::cmp::Ordering::Greater => format!(", +{} since baseline", d.total_delta),
            std::cmp::Ordering::Less => format!(", {} since baseline", d.total_delta),
            std::cmp::Ordering::Equal => ", \u{00b1}0 since baseline".to_string(),
        });
    Some(format!("dead-code ({issues} issues{delta_suffix})"))
}

fn health_failure_nudge(root: &Path, health_result: Option<&HealthResult>) -> String {
    health_result
        .filter(|r| !r.report.targets.is_empty())
        .map(|r| {
            if let Some(top) = r.report.targets.iter().find(|t| !is_test_path(&t.path)) {
                let name = report::format_display_path(&top.path, root);
                format!(": start with {name}")
            } else {
                String::new()
            }
        })
        .unwrap_or_default()
}

fn print_failure_followups(root: &Path) {
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

/// Print combined JSON output wrapping check, dupes, and health results.
#[derive(Clone, Copy)]
struct CombinedJsonPrintInput<'a> {
    check_result: Option<&'a CheckResult>,
    dupes_result: Option<&'a DupesResult>,
    health_result: Option<&'a HealthResult>,
    root: &'a std::path::Path,
    elapsed: std::time::Duration,
    explain: bool,
    config_fixable: bool,
}

fn print_combined_json(input: CombinedJsonPrintInput<'_>) -> ExitCode {
    let output = match build_combined_json_output(input) {
        Ok(output) => output,
        Err(code) => return code,
    };
    emit_combined_json_output(&output)
}

fn build_combined_json_output(
    input: CombinedJsonPrintInput<'_>,
) -> Result<serde_json::Value, ExitCode> {
    let dupes_payload = input
        .dupes_result
        .map(|result| DupesReportPayload::from_report(&result.report));
    let next_steps = combined_next_steps(
        input.check_result,
        dupes_payload.as_ref(),
        input.health_result,
        input.root,
    );

    fallow_api::serialize_combined_json(CombinedJsonOutputInput {
        check: input.check_result.map(|result| CombinedCheckJsonSection {
            results: &result.results,
            root: &result.config.root,
            elapsed: result.elapsed,
            config_fixable: input.config_fixable,
            extras: check_json_extras_for_combined(result),
        }),
        dupes: dupes_payload.as_ref(),
        health: input.health_result.map(|result| &result.report),
        root: input.root,
        elapsed: input.elapsed,
        explain: input.explain,
        next_steps,
        envelope_mode: crate::output_runtime::current_root_envelope_mode(),
        telemetry_analysis_run_id: crate::output_runtime::telemetry_analysis_run_id().as_deref(),
    })
    .map_err(|err| json_output_error(&err))
}

fn combined_next_steps(
    check: Option<&CheckResult>,
    dupes_payload: Option<&DupesReportPayload>,
    health: Option<&HealthResult>,
    root: &std::path::Path,
) -> Vec<fallow_types::output::NextStep> {
    crate::report::suggestions::build_combined_next_steps(
        check.map(|result| &result.results),
        dupes_payload,
        health.map(|result| &result.report),
        root,
        crate::report::suggestions::setup_pointer_applicable(root),
        crate::report::suggestions::due_impact_digest(root),
    )
}

fn emit_combined_json_output(output: &serde_json::Value) -> ExitCode {
    match serde_json::to_string_pretty(output) {
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

fn check_json_extras_for_combined(result: &CheckResult) -> fallow_api::CheckJsonExtraOutputs {
    let baseline_deltas = result.baseline_deltas.as_ref().map(|deltas| {
        report::build_baseline_deltas_output(
            deltas.total_delta,
            deltas
                .per_category
                .iter()
                .map(|(cat, delta)| (cat.as_str(), delta.current, delta.baseline, delta.delta)),
        )
    });
    report::check_json_extras(
        result.regression.as_ref(),
        baseline_deltas,
        result.baseline_matched,
    )
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
        let sarif =
            report::api_sarif_document(&result.results, &result.config.root, &result.config.rules);
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
        let sarif = report::api_health_sarif_document(&result.report, &result.config.root);
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

fn build_combined_codeclimate(
    check: Option<&CheckResult>,
    dupes: Option<&DupesResult>,
    health: Option<&HealthResult>,
) -> serde_json::Value {
    let mut all_issues = build_combined_codeclimate_issues(check, dupes, health);
    // Rebase at the wire boundary only: the same issues feed the sticky summary,
    // whose diff filter matches on analysis-root-relative paths.
    crate::report::codeclimate::rebase_codeclimate_paths(&mut all_issues);
    codeclimate_issues_to_value(&all_issues)
}

/// Analysis-root-relative CodeClimate issues for every enabled analysis.
///
/// Deliberately un-rebased: the sticky summary filters these against the diff,
/// and the presentation prefix belongs at the wire boundary.
fn build_combined_codeclimate_issues(
    check: Option<&CheckResult>,
    dupes: Option<&DupesResult>,
    health: Option<&HealthResult>,
) -> Vec<CodeClimateIssue> {
    let mut all_issues: Vec<CodeClimateIssue> = Vec::new();
    if let Some(result) = check {
        all_issues.extend(fallow_api::build_codeclimate(
            &result.results,
            &result.config.root,
            &result.config.rules,
        ));
    }

    if let Some(result) = dupes {
        all_issues.extend(fallow_api::build_duplication_codeclimate(
            &result.report,
            &result.config.root,
        ));
    }

    if let Some(result) = health {
        all_issues.extend(fallow_api::build_health_codeclimate(
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

    use super::{
        build_combined_codeclimate, build_combined_pr_summary, combined_decision_summary,
        combined_machine_success, emit_combined_json_output, exit_code_to_u8,
        print_combined_codeclimate, print_combined_sarif,
    };
    use fallow_output::{PrDecisionConclusion, PrDecisionGate};

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
    fn empty_combined_pr_summary_is_clean() {
        let envelope = build_combined_pr_summary(
            false,
            None,
            None,
            None,
            crate::report::ci::pr_comment::Provider::Github,
        );

        assert!(envelope.is_clean);
        assert!(envelope.body.contains("No review-visible findings"));
    }

    #[test]
    fn combined_decision_summary_names_notable_gates() {
        let gates = [PrDecisionGate {
            id: "health".to_owned(),
            label: "Health".to_owned(),
            status: PrDecisionConclusion::Failure,
            observed: "1 finding".to_owned(),
            threshold: Some("configured complexity gates".to_owned()),
            scope: "new code".to_owned(),
        }];

        let summary = combined_decision_summary(PrDecisionConclusion::Failure, 1, &gates);

        assert!(summary.contains("quality gates failed"));
        assert!(summary.contains("Health: 1 finding"));
    }

    #[test]
    fn insert_empty_optional_sections_leaves_combined_map_unchanged() {
        let dupes = fallow_api::serialize_combined_dupes_json(None, std::path::Path::new("."))
            .expect("empty dupes section should serialize");
        let health = fallow_api::serialize_combined_health_json(None, std::path::Path::new("."))
            .expect("empty health section should serialize");

        assert!(dupes.is_none());
        assert!(health.is_none());
    }

    #[test]
    fn emit_combined_json_output_can_attach_empty_meta() {
        let combined = serde_json::json!({
            "kind": "combined",
            "schema_version": 7,
            "version": env!("CARGO_PKG_VERSION"),
            "elapsed_ms": 0,
        });

        assert_eq!(emit_combined_json_output(&combined), ExitCode::SUCCESS);
    }
}
