use std::io::IsTerminal;
use std::process::ExitCode;

use colored::Colorize;
use fallow_api::{
    AuditCodeClimateOutputInput, AuditJsonHeaderInput, AuditJsonOutputInput, AuditSarifOutputInput,
    DupesReportPayload,
};
use fallow_config::{AuditGate, OutputFormat, RulesConfig, Severity};
use fallow_output::PrDecisionConclusion;
use fallow_types::envelope::{ElapsedMs, SchemaVersion, ToolVersion};
use fallow_types::results::AnalysisResults;

use crate::error::emit_error;
use crate::report;
use crate::report::plural;
use crate::report::sink::outln;

use super::keys::{
    annotate_dead_code_json, annotate_dupes_json, annotate_health_json, styling_finding_key,
};
use super::{AuditResult, AuditSummary, AuditVerdict};

/// Print audit results and return the appropriate exit code.
#[must_use]
pub fn print_audit_result(result: &AuditResult, quiet: bool, explain: bool) -> ExitCode {
    let output = result.output;

    let format_exit = match output {
        OutputFormat::Json => print_audit_json(result),
        OutputFormat::Human | OutputFormat::Compact | OutputFormat::Markdown => {
            print_audit_human(result, quiet, explain, output);
            ExitCode::SUCCESS
        }
        OutputFormat::Sarif => print_audit_sarif(result),
        OutputFormat::CodeClimate => print_audit_codeclimate(result),
        OutputFormat::PrCommentGithub => {
            let value = build_audit_codeclimate(result);
            report::ci::pr_comment::print_pr_comment_with_conclusion(
                "audit",
                report::ci::pr_comment::Provider::Github,
                &value,
                audit_decision_conclusion(result.verdict),
            )
        }
        OutputFormat::PrCommentGitlab => {
            let value = build_audit_codeclimate(result);
            report::ci::pr_comment::print_pr_comment_with_conclusion(
                "audit",
                report::ci::pr_comment::Provider::Gitlab,
                &value,
                audit_decision_conclusion(result.verdict),
            )
        }
        OutputFormat::ReviewGithub => {
            let value = build_audit_codeclimate(result);
            report::ci::review::print_review_envelope(
                "audit",
                report::ci::pr_comment::Provider::Github,
                &value,
            )
        }
        OutputFormat::ReviewGitlab => {
            let value = build_audit_codeclimate(result);
            report::ci::review::print_review_envelope(
                "audit",
                report::ci::pr_comment::Provider::Gitlab,
                &value,
            )
        }
        OutputFormat::Badge => {
            eprintln!("Error: badge format is not supported for the audit command");
            return ExitCode::from(2);
        }
    };

    if format_exit != ExitCode::SUCCESS {
        return format_exit;
    }

    match result.verdict {
        AuditVerdict::Fail => ExitCode::from(1),
        AuditVerdict::Pass | AuditVerdict::Warn => ExitCode::SUCCESS,
    }
}

fn audit_decision_conclusion(verdict: AuditVerdict) -> PrDecisionConclusion {
    match verdict {
        AuditVerdict::Pass => PrDecisionConclusion::Success,
        AuditVerdict::Warn => PrDecisionConclusion::Neutral,
        AuditVerdict::Fail => PrDecisionConclusion::Failure,
    }
}

fn print_audit_human(result: &AuditResult, quiet: bool, explain: bool, output: OutputFormat) {
    let show_headers = matches!(output, OutputFormat::Human) && !quiet;

    if !quiet {
        let scope = format_scope_line(result);
        eprintln!();
        eprintln!("{scope}");
    }

    let has_check_issues = result.summary.dead_code_issues > 0;
    let has_health_findings = result.summary.complexity_findings > 0;
    let has_dupe_groups = result.summary.duplication_clone_groups > 0;
    // Styling is verdict-neutral but must still surface when it is the ONLY
    // signal (a project clean of dead-code/complexity/dupes but with styling
    // candidates), else the styling summary is invisible on the common case.
    let has_styling = result.health.as_ref().is_some_and(|h| {
        !h.report.styling_findings.is_empty()
            || h.report
                .css_analytics
                .as_ref()
                .is_some_and(|c| styling_candidate_count(&c.summary) > 0)
    });

    if has_check_issues || has_health_findings || has_dupe_groups || has_styling {
        print_audit_findings(result, quiet, explain, show_headers);
    }

    if !has_dupe_groups && let Some(ref dupes) = result.dupes {
        crate::dupes::print_default_ignore_note(dupes, quiet);
        crate::dupes::print_min_occurrences_note(dupes, quiet);
    }

    if !quiet {
        print_audit_status_line(result);
    }
}

/// Print the per-analysis findings sections (dead code, duplication, complexity)
/// plus the explain tip and vital signs, with section headers when enabled.
pub fn print_audit_findings(result: &AuditResult, quiet: bool, explain: bool, show_headers: bool) {
    print_audit_explain_tip(show_headers);

    if result.verdict != AuditVerdict::Fail && !quiet {
        print_audit_vital_signs(result);
    }

    if result.summary.dead_code_issues > 0
        && let Some(ref check) = result.check
    {
        print_audit_section_header(
            show_headers,
            "── Dead Code ──────────────────────────────────────",
        );
        crate::check::print_check_result(
            check,
            crate::check::PrintCheckOptions {
                quiet,
                explain,
                regression_json: false,
                group_by: None,
                top: None,
                summary: false,
                summary_heading: true,
                show_explain_tip: false,
            },
        );
    }

    if result.summary.duplication_clone_groups > 0
        && let Some(ref dupes) = result.dupes
    {
        print_audit_section_header(
            show_headers,
            "── Duplication ────────────────────────────────────",
        );
        crate::dupes::print_dupes_result(dupes, quiet, explain, false, true, false);
    }

    if result.summary.complexity_findings > 0
        && let Some(ref health) = result.health
    {
        print_audit_section_header(
            show_headers,
            "── Complexity ─────────────────────────────────────",
        );
        crate::health::print_health_result(
            health,
            crate::health::HealthPrintOptions {
                quiet,
                explain,
                gates: fallow_engine::health::HealthGateOptions::default(),
                summary: false,
                summary_heading: true,
                show_explain_tip: false,
                skip_score_and_trend: false,
                css_requested: false,
            },
        );
    }

    print_audit_styling_summary(result, show_headers);
}

/// Count the styling candidates in a CSS analytics summary (dead surface + token
/// drift + broken references). Saturating, matching the CSS-analytics
/// accumulation convention (`css_analytics.rs` uses `saturating_add` throughout).
fn styling_candidate_count(s: &fallow_output::CssAnalyticsSummary) -> u32 {
    [
        s.tailwind_arbitrary_values,
        s.duplicate_declaration_blocks,
        s.unreferenced_css_classes,
        s.unused_theme_tokens,
        s.unused_font_faces,
        s.unused_property_registrations,
        s.unused_layers,
        s.scoped_unused_classes,
        s.keyframes_unreferenced,
        s.keyframes_undefined,
        s.unresolved_class_references,
    ]
    .into_iter()
    .fold(0u32, u32::saturating_add)
}

/// Styling section in the audit view: the graduated, agent-actionable styling
/// FINDINGS (top-N per the noise budget), falling back to a candidate count for
/// the not-yet-graduated descriptive candidates. Verdict-neutral; deliberately NOT
/// the A-F styling grade (that stays in `fallow health` for trending, per plan).
fn print_audit_styling_summary(result: &AuditResult, show_headers: bool) {
    /// Noise budget: findings shown per the audit view (the rest via `--css`).
    const FIX_CONFIDENTLY_TOP_N: usize = 5;
    const VERIFY_FIRST_TOP_N: usize = 3;
    let Some(ref health) = result.health else {
        return;
    };
    let findings = &health.report.styling_findings;
    let descriptive = health
        .report
        .css_analytics
        .as_ref()
        .map_or(0, |css| styling_candidate_count(&css.summary));
    if findings.is_empty() && descriptive == 0 {
        return;
    }
    print_audit_section_header(
        show_headers,
        "── Styling ────────────────────────────────────────",
    );
    if findings.is_empty() {
        // Only not-yet-graduated descriptive candidates (dead surface, etc.).
        let noun = if descriptive == 1 {
            "candidate"
        } else {
            "candidates"
        };
        outln!(
            "  {descriptive} styling {noun} {}",
            "(run `fallow health --css` for detail)".dimmed()
        );
        return;
    }
    let rules = &health.config.rules;
    let mut sorted: Vec<_> = findings.iter().collect();
    sorted.sort_by(|a, b| {
        styling_finding_is_error_gated(rules, &b.code)
            .cmp(&styling_finding_is_error_gated(rules, &a.code))
            .then_with(|| a.path.cmp(&b.path))
            .then_with(|| a.line.cmp(&b.line))
            .then_with(|| a.code.cmp(&b.code))
            .then_with(|| a.value.cmp(&b.value))
    });
    let gated_count = sorted
        .iter()
        .filter(|finding| styling_finding_is_error_gated(rules, &finding.code))
        .count();
    let fix_confidently: Vec<_> = sorted
        .iter()
        .copied()
        .filter(|finding| styling_finding_is_fix_confidently(finding))
        .collect();
    let verify_first: Vec<_> = sorted
        .iter()
        .copied()
        .filter(|finding| !styling_finding_is_fix_confidently(finding))
        .collect();
    let show_group_labels = !fix_confidently.is_empty() && !verify_first.is_empty();
    print_audit_styling_group(
        result,
        rules,
        "Fix confidently",
        &fix_confidently,
        FIX_CONFIDENTLY_TOP_N.max(gated_count),
        show_group_labels,
    );
    print_audit_styling_group(
        result,
        rules,
        "Verify first",
        &verify_first,
        VERIFY_FIRST_TOP_N.max(gated_count),
        show_group_labels,
    );
    outln!(
        "  {}",
        "(run `fallow audit --format json` for full styling detail)".dimmed()
    );
}

fn print_audit_styling_group(
    result: &AuditResult,
    rules: &RulesConfig,
    label: &str,
    findings: &[&fallow_output::StylingFinding],
    top_n: usize,
    show_label: bool,
) {
    if findings.is_empty() {
        return;
    }
    if show_label {
        outln!("  {}", label.bold());
    }
    let gated_count = findings
        .iter()
        .filter(|finding| styling_finding_is_error_gated(rules, &finding.code))
        .count();
    let visible_count = top_n.max(gated_count);
    let indent = if show_label { "    " } else { "  " };
    for finding in findings.iter().take(visible_count) {
        outln!(
            "{}{}  {}  {}  {}",
            indent,
            format!("{}:{}", finding.path, finding.line).dimmed(),
            finding.code,
            finding.value,
            styling_finding_audit_context(result, finding).dimmed()
        );
    }
    let hidden = findings.len().saturating_sub(visible_count);
    if hidden > 0 {
        let noun = if hidden == 1 { "finding" } else { "findings" };
        outln!(
            "{}{}",
            indent,
            format!("+ {hidden} more styling {noun}").dimmed()
        );
    }
}

fn styling_finding_is_fix_confidently(finding: &fallow_output::StylingFinding) -> bool {
    matches!(
        finding.agent_disposition,
        Some(fallow_output::StylingAgentDisposition::FixConfidently)
    ) || matches!(
        finding.confidence,
        Some(fallow_output::StylingFindingConfidence::High)
    )
}

fn styling_finding_is_error_gated(rules: &RulesConfig, code: &str) -> bool {
    let (_, severity) = styling_finding_rule_context(rules, code);
    severity == Severity::Error
}

fn styling_finding_rule_context(rules: &RulesConfig, code: &str) -> (String, Severity) {
    let severity = match code {
        "css-token-drift" => rules.css_token_drift,
        "css-duplicate-block" => rules.css_duplicate_block,
        "css-selector-complexity" => rules.css_selector_complexity,
        "css-dead-surface" => rules.css_dead_surface,
        "css-broken-reference" => rules.css_broken_reference,
        _ => Severity::Warn,
    };
    (format!("rules.{code}"), severity)
}

fn styling_finding_audit_context(
    result: &AuditResult,
    finding: &fallow_output::StylingFinding,
) -> String {
    let Some(health) = result.health.as_ref() else {
        let (rule, severity) = styling_finding_rule_context(&RulesConfig::default(), &finding.code);
        return styling_finding_audit_context_label(severity, &rule, None, result.attribution.gate);
    };
    let (rule, severity) = styling_finding_rule_context(&health.config.rules, &finding.code);
    let base_state = result.base_snapshot.as_ref().map(|snapshot| {
        let key = styling_finding_key(finding, &health.config.root);
        if snapshot.styling.contains(&key) {
            format!(
                "inherited styling debt from {}",
                short_base_ref(&result.base_ref)
            )
        } else {
            format!(
                "introduced design-system drift since {}",
                short_base_ref(&result.base_ref)
            )
        }
    });
    styling_finding_audit_context_label(severity, &rule, base_state, result.attribution.gate)
}

fn styling_finding_audit_context_label(
    severity: Severity,
    rule: &str,
    base_state: Option<String>,
    gate: AuditGate,
) -> String {
    let severity_label = match severity {
        Severity::Off => "off",
        Severity::Warn => "warn",
        Severity::Error => "error",
    };
    let prefix = match (severity, gate, base_state.as_deref()) {
        (Severity::Error, AuditGate::NewOnly, Some(state)) if state.starts_with("inherited") => {
            "not gated"
        }
        (Severity::Error, _, _) => "gated",
        _ => "advisory",
    };
    match base_state {
        Some(state) => format!("({prefix}: {rule}={severity_label}, {state})"),
        None => format!("({prefix}: {rule}={severity_label})"),
    }
}

/// Print the TTY-only explain tip above the findings sections.
fn print_audit_explain_tip(show_headers: bool) {
    if show_headers && std::io::stdout().is_terminal() && !crate::report::sink::is_redirected() {
        println!(
            "{}",
            "Tip: run `fallow explain <issue label>`; spaces and hyphens both work, e.g. `fallow explain unused files`."
                .dimmed()
        );
        println!();
    }
}

/// Emit a blank line followed by a section header when headers are enabled.
fn print_audit_section_header(show_headers: bool, header: &str) {
    if show_headers {
        eprintln!();
        eprintln!("{header}");
    }
}

/// Abbreviate a 40-char hex SHA to 12 chars for display; leave anything else
/// (branch names, refspecs, the literal user typed for `--base`) untouched.
fn short_base_ref(base_ref: &str) -> &str {
    if base_ref.len() == 40 && base_ref.bytes().all(|b| b.is_ascii_hexdigit()) {
        &base_ref[..12]
    } else {
        base_ref
    }
}

/// Format the scope context line. When the base ref was auto-detected (or set
/// via `FALLOW_AUDIT_BASE`), append the provenance so the comparison target is
/// checkable, e.g. `vs a1b2c3d4e5f6 (merge-base with origin/main)`.
fn format_scope_line(result: &AuditResult) -> String {
    format_scope_line_parts(
        result.changed_files_count,
        &result.base_ref,
        result.base_description.as_deref(),
        result.head_sha.as_deref(),
    )
}

fn format_scope_line_parts(
    changed_files_count: usize,
    base_ref: &str,
    base_description: Option<&str>,
    head_sha: Option<&str>,
) -> String {
    let sha_suffix = head_sha.map_or(String::new(), |sha| format!(" ({sha}..HEAD)"));
    let base_display = match base_description {
        Some(description) => format!("{} ({description})", short_base_ref(base_ref)),
        None => base_ref.to_string(),
    };
    format!(
        "Audit scope: {} changed file{} vs {}{}",
        changed_files_count,
        plural(changed_files_count),
        base_display,
        sha_suffix
    )
}

/// Print a dimmed vital-signs line summarizing warn-only findings.
fn print_audit_vital_signs(result: &AuditResult) {
    let line = build_vital_sign_parts(&result.summary).join(" \u{00b7} ");
    outln!(
        "{} {} {}",
        "\u{25a0}".dimmed(),
        "Metrics:".dimmed(),
        line.dimmed()
    );
}

fn build_vital_sign_parts(summary: &AuditSummary) -> Vec<String> {
    let mut parts = Vec::new();
    parts.push(format!("dead code {}", summary.dead_code_issues));
    if let Some(max) = summary.max_cyclomatic {
        parts.push(format!(
            "complexity {} (warn, max cyclomatic: {max})",
            summary.complexity_findings
        ));
    } else {
        parts.push(format!("complexity {}", summary.complexity_findings));
    }
    parts.push(format!("duplication {}", summary.duplication_clone_groups));
    parts
}

/// Build summary parts for the status line (shared between warn and fail).
fn build_status_parts(summary: &AuditSummary) -> Vec<String> {
    let mut parts = Vec::new();
    if summary.dead_code_issues > 0 {
        let n = summary.dead_code_issues;
        parts.push(format!("dead code: {n} issue{}", plural(n)));
    }
    if summary.complexity_findings > 0 {
        let n = summary.complexity_findings;
        parts.push(format!("complexity: {n} finding{}", plural(n)));
    }
    if summary.duplication_clone_groups > 0 {
        let n = summary.duplication_clone_groups;
        parts.push(format!("duplication: {n} clone group{}", plural(n)));
    }
    parts
}

/// Print the final status line on stderr.
fn print_audit_status_line(result: &AuditResult) {
    let elapsed_str = format!("{:.2}s", result.elapsed.as_secs_f64());
    let n = result.changed_files_count;
    let files_str = format!("{n} changed file{}", plural(n));

    match result.verdict {
        AuditVerdict::Pass => {
            eprintln!(
                "{}",
                format!("\u{2713} No issues in {files_str} ({elapsed_str})")
                    .green()
                    .bold()
            );
        }
        AuditVerdict::Warn => {
            let summary = build_status_parts(&result.summary).join(" \u{00b7} ");
            eprintln!(
                "{}",
                format!("\u{2713} {summary} (warn) \u{00b7} {files_str} ({elapsed_str})")
                    .green()
                    .bold()
            );
        }
        AuditVerdict::Fail => {
            let summary = build_status_parts(&result.summary).join(" \u{00b7} ");
            eprintln!(
                "{}",
                format!("\u{2717} {summary} \u{00b7} {files_str} ({elapsed_str})")
                    .red()
                    .bold()
            );
        }
    }

    if !matches!(result.attribution.gate, AuditGate::All) {
        let inherited = result.attribution.dead_code_inherited
            + result.attribution.complexity_inherited
            + result.attribution.duplication_inherited;
        if inherited > 0 {
            eprintln!(
                "  {}",
                format!(
                    "audit gate excluded {inherited} inherited finding{} (run with --gate all to enforce)",
                    plural(inherited)
                )
                .dimmed()
            );
        }
    }
    if result.performance {
        eprintln!(
            "  {}",
            format!("base_snapshot_skipped: {}", result.base_snapshot_skipped).dimmed()
        );
    }
}

fn print_audit_json(result: &AuditResult) -> ExitCode {
    let output = match build_audit_json_output(result) {
        Ok(output) => output,
        Err(code) => return code,
    };
    report::emit_json(&output, "audit")
}

fn build_audit_json_output(result: &AuditResult) -> Result<serde_json::Value, ExitCode> {
    let mut check_results = result.check.as_ref().map(|check| check.results.clone());
    let mut health_report = result.health.as_ref().map(|health| health.report.clone());
    fallow_output::harmonize_dead_code_health_suppress_line_actions(
        check_results.as_mut(),
        health_report.as_mut(),
    );

    let dead_code = match (result.check.as_ref(), check_results.as_ref()) {
        (Some(check), Some(results)) => Some(build_audit_dead_code_json_with_results(
            result, check, results,
        )?),
        _ => None,
    };
    let duplication = result
        .dupes
        .as_ref()
        .map(|dupes| build_audit_duplication_json(result, dupes))
        .transpose()?;
    let complexity = match (result.health.as_ref(), health_report.as_ref()) {
        (Some(health), Some(report)) => {
            Some(build_audit_health_json_with_report(result, health, report)?)
        }
        _ => None,
    };

    fallow_api::serialize_audit_json(
        AuditJsonOutputInput {
            header: audit_json_header_input(result),
            dead_code,
            duplication,
            complexity,
            next_steps: audit_next_steps(result),
        },
        crate::output_runtime::current_root_envelope_mode(),
        crate::output_runtime::telemetry_analysis_run_id().as_deref(),
    )
    .map_err(|err| {
        emit_error(
            &format!("JSON serialization error: {err}"),
            2,
            OutputFormat::Json,
        )
    })
}

fn elapsed_ms_for_output(elapsed: std::time::Duration) -> u64 {
    u64::try_from(elapsed.as_millis()).unwrap_or(u64::MAX)
}

fn changed_files_count_for_output(changed_files_count: usize) -> u32 {
    u32::try_from(changed_files_count).unwrap_or(u32::MAX)
}

pub fn audit_json_header_input(result: &AuditResult) -> AuditJsonHeaderInput {
    AuditJsonHeaderInput {
        schema_version: SchemaVersion(crate::report::SCHEMA_VERSION),
        version: ToolVersion(env!("CARGO_PKG_VERSION").to_string()),
        verdict: result.verdict,
        changed_files_count: changed_files_count_for_output(result.changed_files_count),
        base_ref: result.base_ref.clone(),
        base_description: result.base_description.clone(),
        head_sha: result.head_sha.clone(),
        elapsed_ms: ElapsedMs(elapsed_ms_for_output(result.elapsed)),
        base_snapshot_skipped: result.performance.then_some(result.base_snapshot_skipped),
        summary: result.summary.clone(),
        attribution: result.attribution.clone(),
    }
}

pub fn insert_audit_dead_code_json(
    obj: &mut serde_json::Map<String, serde_json::Value>,
    result: &AuditResult,
    check: &crate::check::CheckResult,
) -> Result<(), ExitCode> {
    let json = build_audit_dead_code_json(result, check)?;
    obj.insert("dead_code".into(), json);
    Ok(())
}

fn build_audit_dead_code_json(
    result: &AuditResult,
    check: &crate::check::CheckResult,
) -> Result<serde_json::Value, ExitCode> {
    build_audit_dead_code_json_with_results(result, check, &check.results)
}

fn build_audit_dead_code_json_with_results(
    result: &AuditResult,
    check: &crate::check::CheckResult,
    results: &AnalysisResults,
) -> Result<serde_json::Value, ExitCode> {
    match report::api_check_json_payload_with_config_fixable(
        results,
        &check.config.root,
        check.elapsed,
        check.config_fixable,
    ) {
        Ok(mut json) => {
            if let Some(ref base) = result.base_snapshot {
                annotate_dead_code_json(&mut json, results, &check.config.root, &base.dead_code);
            }
            Ok(json)
        }
        Err(e) => Err(emit_error(
            &format!("JSON serialization error: {e}"),
            2,
            OutputFormat::Json,
        )),
    }
}

pub fn insert_audit_duplication_json(
    obj: &mut serde_json::Map<String, serde_json::Value>,
    result: &AuditResult,
    dupes: &crate::dupes::DupesResult,
) -> Result<(), ExitCode> {
    let json = build_audit_duplication_json(result, dupes)?;
    obj.insert("duplication".into(), json);
    Ok(())
}

fn build_audit_duplication_json(
    result: &AuditResult,
    dupes: &crate::dupes::DupesResult,
) -> Result<serde_json::Value, ExitCode> {
    let payload = DupesReportPayload::from_report(&dupes.report);
    match serde_json::to_value(&payload) {
        Ok(mut json) => {
            let root_prefix = format!("{}/", dupes.config.root.display());
            report::strip_root_prefix(&mut json, &root_prefix);
            if let Some(ref base) = result.base_snapshot {
                annotate_dupes_json(&mut json, &dupes.report, &dupes.config.root, &base.dupes);
            }
            Ok(json)
        }
        Err(e) => Err(emit_error(
            &format!("JSON serialization error: {e}"),
            2,
            OutputFormat::Json,
        )),
    }
}

pub fn insert_audit_health_json(
    obj: &mut serde_json::Map<String, serde_json::Value>,
    result: &AuditResult,
    health: &crate::health::HealthResult,
) -> Result<(), ExitCode> {
    let json = build_audit_health_json(result, health)?;
    obj.insert("complexity".into(), json);
    Ok(())
}

fn build_audit_health_json(
    result: &AuditResult,
    health: &crate::health::HealthResult,
) -> Result<serde_json::Value, ExitCode> {
    build_audit_health_json_with_report(result, health, &health.report)
}

fn build_audit_health_json_with_report(
    result: &AuditResult,
    health: &crate::health::HealthResult,
    report: &fallow_output::HealthReport,
) -> Result<serde_json::Value, ExitCode> {
    match serde_json::to_value(report) {
        Ok(mut json) => {
            let root_prefix = format!("{}/", health.config.root.display());
            report::strip_root_prefix(&mut json, &root_prefix);
            if let Some(ref base) = result.base_snapshot {
                annotate_health_json(&mut json, report, &health.config.root, &base.health);
            }
            Ok(json)
        }
        Err(e) => Err(emit_error(
            &format!("JSON serialization error: {e}"),
            2,
            OutputFormat::Json,
        )),
    }
}

fn audit_next_steps(result: &AuditResult) -> Vec<fallow_types::output::NextStep> {
    let input = fallow_output::build_audit_next_steps_input(
        result
            .check
            .as_ref()
            .map(|check| (&check.results, check.config.root.as_path())),
        result.health.as_ref().map(|health| &health.report),
        crate::report::suggestions::suggestions_enabled(),
    );
    fallow_output::build_audit_next_steps(&input)
}

fn print_audit_sarif(result: &AuditResult) -> ExitCode {
    let check_sarif = result.check.as_ref().map(|check| {
        report::api_sarif_document(&check.results, &check.config.root, &check.config.rules)
    });
    let health_sarif = result
        .health
        .as_ref()
        .map(|health| report::api_health_sarif_document(&health.report, &health.config.root));
    let combined = fallow_api::build_audit_sarif(AuditSarifOutputInput {
        dead_code: check_sarif.as_ref(),
        duplication: result.dupes.as_ref().map(|dupes| &dupes.report),
        health: health_sarif.as_ref(),
    });

    report::emit_json(&combined, "SARIF audit")
}

fn print_audit_codeclimate(result: &AuditResult) -> ExitCode {
    let value = build_audit_codeclimate(result);
    report::emit_json(&value, "CodeClimate audit")
}

fn build_audit_codeclimate(result: &AuditResult) -> serde_json::Value {
    fallow_api::build_audit_codeclimate(AuditCodeClimateOutputInput {
        dead_code: result.check.as_ref().map_or_else(Vec::new, |check| {
            fallow_api::build_codeclimate(&check.results, &check.config.root, &check.config.rules)
        }),
        duplication: result.dupes.as_ref().map_or_else(Vec::new, |dupes| {
            fallow_api::build_duplication_codeclimate(&dupes.report, &dupes.config.root)
        }),
        health: result.health.as_ref().map_or_else(Vec::new, |health| {
            fallow_api::build_health_codeclimate(&health.report, &health.config.root)
        }),
    })
}

#[cfg(test)]
mod tests {
    use std::process::ExitCode;
    use std::time::Duration;

    use fallow_config::{AuditGate, OutputFormat};
    use fallow_output::PrDecisionConclusion;

    use crate::audit::{AuditAttribution, AuditResult, AuditSummary, AuditVerdict};

    use super::{
        audit_decision_conclusion, build_audit_codeclimate, build_audit_json_output,
        build_status_parts, build_vital_sign_parts, format_scope_line_parts, print_audit_result,
        short_base_ref, styling_finding_audit_context_label,
    };

    fn audit_result(verdict: AuditVerdict, output: OutputFormat) -> AuditResult {
        AuditResult {
            verdict,
            summary: AuditSummary {
                dead_code_issues: 0,
                dead_code_has_errors: false,
                complexity_findings: 0,
                max_cyclomatic: None,
                duplication_clone_groups: 0,
            },
            attribution: AuditAttribution {
                gate: AuditGate::NewOnly,
                ..AuditAttribution::default()
            },
            base_snapshot: None,
            base_snapshot_skipped: false,
            changed_files_count: 0,
            changed_files: Vec::new(),
            base_ref: "origin/main".to_string(),
            base_description: None,
            head_sha: None,
            output,
            performance: false,
            check: None,
            dupes: None,
            health: None,
            elapsed: Duration::ZERO,
            review_deltas: None,
            weakening_signals: Vec::new(),
            routing: None,
            decision_surface: None,
            graph_snapshot_hash: None,
            change_anchors: Vec::new(),
        }
    }

    #[test]
    fn short_base_ref_abbreviates_full_sha() {
        assert_eq!(
            short_base_ref("611d151e8250146426ff3178e94207f8a8d3cc7b"),
            "611d151e8250"
        );
    }

    #[test]
    fn short_base_ref_leaves_branch_names_and_refspecs_untouched() {
        assert_eq!(short_base_ref("main"), "main");
        assert_eq!(short_base_ref("origin/main"), "origin/main");
        assert_eq!(short_base_ref("HEAD~5"), "HEAD~5");
        // Not 40 chars, so not treated as a SHA.
        assert_eq!(short_base_ref("611d151e8250"), "611d151e8250");
        // 40 chars but contains a non-hex character: left untouched.
        assert_eq!(
            short_base_ref("611d151e8250146426ff3178e94207f8a8d3ccZZ"),
            "611d151e8250146426ff3178e94207f8a8d3ccZZ"
        );
    }

    #[test]
    fn format_scope_line_parts_uses_plural_ref_provenance_and_head_sha() {
        assert_eq!(
            format_scope_line_parts(
                1,
                "611d151e8250146426ff3178e94207f8a8d3cc7b",
                Some("merge-base with origin/main"),
                Some("HEADSHA")
            ),
            "Audit scope: 1 changed file vs 611d151e8250 (merge-base with origin/main) (HEADSHA..HEAD)"
        );
        assert_eq!(
            format_scope_line_parts(3, "origin/main", None, None),
            "Audit scope: 3 changed files vs origin/main"
        );
    }

    #[test]
    fn styling_finding_audit_context_label_explains_gate_state() {
        assert_eq!(
            styling_finding_audit_context_label(
                fallow_config::Severity::Error,
                "rules.css-selector-complexity",
                Some("introduced design-system drift since HEAD".to_string()),
                AuditGate::NewOnly,
            ),
            "(gated: rules.css-selector-complexity=error, introduced design-system drift since HEAD)"
        );
        assert_eq!(
            styling_finding_audit_context_label(
                fallow_config::Severity::Error,
                "rules.css-selector-complexity",
                Some("inherited styling debt from HEAD".to_string()),
                AuditGate::NewOnly,
            ),
            "(not gated: rules.css-selector-complexity=error, inherited styling debt from HEAD)"
        );
        assert_eq!(
            styling_finding_audit_context_label(
                fallow_config::Severity::Warn,
                "rules.css-selector-complexity",
                None,
                AuditGate::All,
            ),
            "(advisory: rules.css-selector-complexity=warn)"
        );
    }

    #[test]
    fn build_status_parts_describes_only_non_empty_categories() {
        let summary = AuditSummary {
            dead_code_issues: 1,
            dead_code_has_errors: true,
            complexity_findings: 2,
            max_cyclomatic: Some(12),
            duplication_clone_groups: 3,
        };

        assert_eq!(
            build_status_parts(&summary),
            vec![
                "dead code: 1 issue".to_string(),
                "complexity: 2 findings".to_string(),
                "duplication: 3 clone groups".to_string(),
            ]
        );

        let empty = AuditSummary {
            dead_code_issues: 0,
            dead_code_has_errors: false,
            complexity_findings: 0,
            max_cyclomatic: None,
            duplication_clone_groups: 0,
        };
        assert!(build_status_parts(&empty).is_empty());
    }

    #[test]
    fn build_vital_sign_parts_includes_warn_threshold_when_present() {
        let summary = AuditSummary {
            dead_code_issues: 0,
            dead_code_has_errors: false,
            complexity_findings: 2,
            max_cyclomatic: Some(18),
            duplication_clone_groups: 1,
        };

        assert_eq!(
            build_vital_sign_parts(&summary),
            vec![
                "dead code 0".to_string(),
                "complexity 2 (warn, max cyclomatic: 18)".to_string(),
                "duplication 1".to_string(),
            ]
        );
    }

    #[test]
    fn build_vital_sign_parts_omits_threshold_when_absent() {
        let summary = AuditSummary {
            dead_code_issues: 3,
            dead_code_has_errors: false,
            complexity_findings: 0,
            max_cyclomatic: None,
            duplication_clone_groups: 0,
        };

        assert_eq!(
            build_vital_sign_parts(&summary),
            vec![
                "dead code 3".to_string(),
                "complexity 0".to_string(),
                "duplication 0".to_string(),
            ]
        );
    }

    #[test]
    fn build_audit_codeclimate_returns_empty_issue_list_without_findings() {
        let result = audit_result(AuditVerdict::Pass, OutputFormat::CodeClimate);

        assert_eq!(build_audit_codeclimate(&result), serde_json::json!([]));
    }

    #[test]
    fn print_audit_result_rejects_badge_format() {
        let result = audit_result(AuditVerdict::Pass, OutputFormat::Badge);

        assert_eq!(print_audit_result(&result, true, false), ExitCode::from(2));
    }

    #[test]
    fn print_audit_result_maps_fail_verdict_to_error_exit() {
        let result = audit_result(AuditVerdict::Fail, OutputFormat::Human);

        assert_eq!(print_audit_result(&result, true, false), ExitCode::from(1));
    }

    #[test]
    fn audit_verdict_maps_to_pr_decision_conclusion() {
        assert_eq!(
            audit_decision_conclusion(AuditVerdict::Pass),
            PrDecisionConclusion::Success
        );
        assert_eq!(
            audit_decision_conclusion(AuditVerdict::Warn),
            PrDecisionConclusion::Neutral
        );
        assert_eq!(
            audit_decision_conclusion(AuditVerdict::Fail),
            PrDecisionConclusion::Failure
        );
    }

    fn audit_result_with_findings(verdict: AuditVerdict, output: OutputFormat) -> AuditResult {
        let mut result = audit_result(verdict, output);
        result.summary = AuditSummary {
            dead_code_issues: 2,
            dead_code_has_errors: true,
            complexity_findings: 1,
            max_cyclomatic: Some(14),
            duplication_clone_groups: 3,
        };
        result.changed_files_count = 4;
        result
    }

    #[test]
    fn print_audit_json_emits_optional_header_fields() {
        let mut result = audit_result(AuditVerdict::Pass, OutputFormat::Json);
        result.base_description = Some("merge-base with origin/main".to_string());
        result.head_sha = Some("abc123".to_string());
        result.performance = true;
        result.base_snapshot_skipped = true;
        result.changed_files_count = 5;

        // Pass verdict + successful JSON emit (no sub-results) maps to success;
        // Exercises the typed audit header's optional base_description /
        // head_sha / performance branches and the empty next-steps path.
        assert_eq!(print_audit_result(&result, true, false), ExitCode::SUCCESS);
    }

    #[test]
    fn build_audit_json_output_uses_typed_audit_contract() {
        let mut result = audit_result(AuditVerdict::Pass, OutputFormat::Json);
        result.base_description = Some("merge-base with origin/main".to_string());
        result.head_sha = Some("abc123".to_string());
        result.performance = true;
        result.base_snapshot_skipped = true;
        result.changed_files_count = 5;

        let json = build_audit_json_output(&result).expect("audit JSON should build");

        assert_eq!(json["kind"], "audit");
        assert_eq!(json["command"], "audit");
        assert_eq!(json["base_description"], "merge-base with origin/main");
        assert_eq!(json["head_sha"], "abc123");
        assert_eq!(json["base_snapshot_skipped"], true);
        assert_eq!(json["changed_files_count"], 5);
    }

    #[test]
    fn print_audit_result_renders_sarif_skeleton_without_findings() {
        let result = audit_result(AuditVerdict::Pass, OutputFormat::Sarif);

        assert_eq!(print_audit_result(&result, true, false), ExitCode::SUCCESS);
    }

    #[test]
    fn print_audit_result_renders_codeclimate_without_findings() {
        let result = audit_result(AuditVerdict::Pass, OutputFormat::CodeClimate);

        assert_eq!(print_audit_result(&result, true, false), ExitCode::SUCCESS);
    }

    #[test]
    fn print_audit_result_renders_pr_comment_for_both_providers() {
        for format in [OutputFormat::PrCommentGithub, OutputFormat::PrCommentGitlab] {
            let result = audit_result(AuditVerdict::Pass, format);
            assert_eq!(print_audit_result(&result, true, false), ExitCode::SUCCESS);
        }
    }

    #[test]
    fn print_audit_result_renders_review_envelope_for_both_providers() {
        for format in [OutputFormat::ReviewGithub, OutputFormat::ReviewGitlab] {
            let result = audit_result(AuditVerdict::Pass, format);
            assert_eq!(print_audit_result(&result, true, false), ExitCode::SUCCESS);
        }
    }

    #[test]
    fn print_audit_result_compact_and_markdown_use_human_path() {
        for format in [OutputFormat::Compact, OutputFormat::Markdown] {
            let result = audit_result(AuditVerdict::Pass, format);
            assert_eq!(print_audit_result(&result, true, false), ExitCode::SUCCESS);
        }
    }

    #[test]
    fn print_audit_result_human_pass_renders_scope_and_status_line() {
        let mut result = audit_result(AuditVerdict::Pass, OutputFormat::Human);
        result.changed_files_count = 2;

        // quiet=false drives the scope line + the green "no issues" status line.
        assert_eq!(print_audit_result(&result, false, false), ExitCode::SUCCESS);
    }

    #[test]
    fn print_audit_result_human_warn_renders_vital_signs_and_notes() {
        let mut result = audit_result_with_findings(AuditVerdict::Warn, OutputFormat::Human);
        result.attribution = AuditAttribution {
            gate: AuditGate::NewOnly,
            dead_code_inherited: 2,
            complexity_inherited: 1,
            duplication_inherited: 0,
            ..AuditAttribution::default()
        };
        result.performance = true;

        // Warn + findings (without sub-results) covers the explain tip, vital
        // signs, the gate-excluded inherited note, and the performance note.
        assert_eq!(print_audit_result(&result, false, false), ExitCode::SUCCESS);
    }

    #[test]
    fn print_audit_result_human_fail_renders_red_status_line() {
        let result = audit_result_with_findings(AuditVerdict::Fail, OutputFormat::Human);

        // Fail maps to exit 1 and renders the red status line via build_status_parts.
        assert_eq!(print_audit_result(&result, false, false), ExitCode::from(1));
    }
}
