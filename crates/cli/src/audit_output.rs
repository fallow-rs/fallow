use std::io::IsTerminal;
use std::process::ExitCode;

use colored::Colorize;
use fallow_config::{AuditGate, OutputFormat};

use crate::error::emit_error;
use crate::report;
use crate::report::plural;
use crate::report::sink::outln;

use super::keys::{annotate_dead_code_json, annotate_dupes_json, annotate_health_json};
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
            report::ci::pr_comment::print_pr_comment(
                "audit",
                report::ci::pr_comment::Provider::Github,
                &value,
            )
        }
        OutputFormat::PrCommentGitlab => {
            let value = build_audit_codeclimate(result);
            report::ci::pr_comment::print_pr_comment(
                "audit",
                report::ci::pr_comment::Provider::Gitlab,
                &value,
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
    let has_any_findings = has_check_issues || has_health_findings || has_dupe_groups;

    if has_any_findings {
        if show_headers && std::io::stdout().is_terminal() && !crate::report::sink::is_redirected()
        {
            println!(
                "{}",
                "Tip: run `fallow explain <issue label>`; spaces and hyphens both work, e.g. `fallow explain unused files`."
                    .dimmed()
            );
            println!();
        }

        if result.verdict != AuditVerdict::Fail && !quiet {
            print_audit_vital_signs(result);
        }

        if has_check_issues && let Some(ref check) = result.check {
            if show_headers {
                eprintln!();
                eprintln!("── Dead Code ──────────────────────────────────────");
            }
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

        if has_dupe_groups && let Some(ref dupes) = result.dupes {
            if show_headers {
                eprintln!();
                eprintln!("── Duplication ────────────────────────────────────");
            }
            crate::dupes::print_dupes_result(dupes, quiet, explain, false, true, false);
        }

        if has_health_findings && let Some(ref health) = result.health {
            if show_headers {
                eprintln!();
                eprintln!("── Complexity ─────────────────────────────────────");
            }
            crate::health::print_health_result(
                health,
                crate::health::HealthPrintOptions {
                    quiet,
                    explain,
                    min_score: None,
                    min_severity: None,
                    report_only: false,
                    summary: false,
                    summary_heading: true,
                    show_explain_tip: false,
                    skip_score_and_trend: false,
                },
            );
        }
    }

    if !has_dupe_groups && let Some(ref dupes) = result.dupes {
        crate::dupes::print_default_ignore_note(dupes, quiet);
        crate::dupes::print_min_occurrences_note(dupes, quiet);
    }

    if !quiet {
        print_audit_status_line(result);
    }
}

/// Format the scope context line.
fn format_scope_line(result: &AuditResult) -> String {
    let sha_suffix = result
        .head_sha
        .as_ref()
        .map_or(String::new(), |sha| format!(" ({sha}..HEAD)"));
    format!(
        "Audit scope: {} changed file{} vs {}{}",
        result.changed_files_count,
        plural(result.changed_files_count),
        result.base_ref,
        sha_suffix
    )
}

/// Print a dimmed vital-signs line summarizing warn-only findings.
fn print_audit_vital_signs(result: &AuditResult) {
    let mut parts = Vec::new();
    parts.push(format!("dead code {}", result.summary.dead_code_issues));
    if let Some(max) = result.summary.max_cyclomatic {
        parts.push(format!(
            "complexity {} (warn, max cyclomatic: {max})",
            result.summary.complexity_findings
        ));
    } else {
        parts.push(format!("complexity {}", result.summary.complexity_findings));
    }
    parts.push(format!(
        "duplication {}",
        result.summary.duplication_clone_groups
    ));

    let line = parts.join(" \u{00b7} ");
    outln!(
        "{} {} {}",
        "\u{25a0}".dimmed(),
        "Metrics:".dimmed(),
        line.dimmed()
    );
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

#[expect(
    clippy::cast_possible_truncation,
    reason = "elapsed milliseconds won't exceed u64::MAX"
)]
fn print_audit_json(result: &AuditResult) -> ExitCode {
    let mut obj = serde_json::Map::new();
    obj.insert(
        "schema_version".into(),
        serde_json::Value::Number(crate::report::SCHEMA_VERSION.into()),
    );
    obj.insert(
        "version".into(),
        serde_json::Value::String(env!("CARGO_PKG_VERSION").to_string()),
    );
    obj.insert(
        "command".into(),
        serde_json::Value::String("audit".to_string()),
    );
    obj.insert(
        "verdict".into(),
        serde_json::to_value(result.verdict).unwrap_or(serde_json::Value::Null),
    );
    obj.insert(
        "changed_files_count".into(),
        serde_json::Value::Number(result.changed_files_count.into()),
    );
    obj.insert(
        "base_ref".into(),
        serde_json::Value::String(result.base_ref.clone()),
    );
    if let Some(ref sha) = result.head_sha {
        obj.insert("head_sha".into(), serde_json::Value::String(sha.clone()));
    }
    obj.insert(
        "elapsed_ms".into(),
        serde_json::Value::Number(serde_json::Number::from(result.elapsed.as_millis() as u64)),
    );
    if result.performance {
        obj.insert(
            "base_snapshot_skipped".into(),
            serde_json::Value::Bool(result.base_snapshot_skipped),
        );
    }

    if let Ok(summary_val) = serde_json::to_value(&result.summary) {
        obj.insert("summary".into(), summary_val);
    }
    if let Ok(attribution_val) = serde_json::to_value(&result.attribution) {
        obj.insert("attribution".into(), attribution_val);
    }

    if let Some(ref check) = result.check {
        match report::build_check_json_payload_with_config_fixable(
            &check.results,
            &check.config.root,
            check.elapsed,
            check.config_fixable,
        ) {
            Ok(mut json) => {
                if let Some(ref base) = result.base_snapshot {
                    annotate_dead_code_json(
                        &mut json,
                        &check.results,
                        &check.config.root,
                        &base.dead_code,
                    );
                }
                obj.insert("dead_code".into(), json);
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

    if let Some(ref dupes) = result.dupes {
        let payload = crate::output_dupes::DupesReportPayload::from_report(&dupes.report);
        match serde_json::to_value(&payload) {
            Ok(mut json) => {
                let root_prefix = format!("{}/", dupes.config.root.display());
                report::strip_root_prefix(&mut json, &root_prefix);
                if let Some(ref base) = result.base_snapshot {
                    annotate_dupes_json(&mut json, &dupes.report, &dupes.config.root, &base.dupes);
                }
                obj.insert("duplication".into(), json);
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

    if let Some(ref health) = result.health {
        match serde_json::to_value(&health.report) {
            Ok(mut json) => {
                let root_prefix = format!("{}/", health.config.root.display());
                report::strip_root_prefix(&mut json, &root_prefix);
                if let Some(ref base) = result.base_snapshot {
                    annotate_health_json(
                        &mut json,
                        &health.report,
                        &health.config.root,
                        &base.health,
                    );
                }
                obj.insert("complexity".into(), json);
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

    let mut output = serde_json::Value::Object(obj);
    crate::output_envelope::apply_root_kind(&mut output, "audit");
    report::harmonize_multi_kind_suppress_line_actions(&mut output);
    report::emit_json(&output, "audit")
}

fn print_audit_sarif(result: &AuditResult) -> ExitCode {
    let mut all_runs = Vec::new();

    if let Some(ref check) = result.check {
        let sarif = report::build_sarif(&check.results, &check.config.root, &check.config.rules);
        if let Some(runs) = sarif.get("runs").and_then(|r| r.as_array()) {
            all_runs.extend(runs.iter().cloned());
        }
    }

    if let Some(ref dupes) = result.dupes
        && !dupes.report.clone_groups.is_empty()
    {
        let run = serde_json::json!({
            "tool": {
                "driver": {
                    "name": "fallow",
                    "version": env!("CARGO_PKG_VERSION"),
                    "informationUri": "https://github.com/fallow-rs/fallow",
                }
            },
            "automationDetails": { "id": "fallow/audit/dupes" },
            "results": dupes.report.clone_groups.iter().enumerate().map(|(i, g)| {
                serde_json::json!({
                    "ruleId": "fallow/code-duplication",
                    "level": "warning",
                    "message": { "text": format!("Clone group {} ({} lines, {} instances)", i + 1, g.line_count, g.instances.len()) },
                })
            }).collect::<Vec<_>>()
        });
        all_runs.push(run);
    }

    if let Some(ref health) = result.health {
        let sarif = report::build_health_sarif(&health.report, &health.config.root);
        if let Some(runs) = sarif.get("runs").and_then(|r| r.as_array()) {
            all_runs.extend(runs.iter().cloned());
        }
    }

    let combined = serde_json::json!({
        "$schema": "https://json.schemastore.org/sarif-2.1.0.json",
        "version": "2.1.0",
        "runs": all_runs,
    });

    report::emit_json(&combined, "SARIF audit")
}

fn print_audit_codeclimate(result: &AuditResult) -> ExitCode {
    let value = build_audit_codeclimate(result);
    report::emit_json(&value, "CodeClimate audit")
}

#[expect(
    clippy::expect_used,
    reason = "CodeClimate issue envelope contains only infallibly serializable fields"
)]
fn build_audit_codeclimate(result: &AuditResult) -> serde_json::Value {
    let mut all_issues: Vec<crate::output_envelope::CodeClimateIssue> = Vec::new();

    if let Some(ref check) = result.check {
        all_issues.extend(report::build_codeclimate(
            &check.results,
            &check.config.root,
            &check.config.rules,
        ));
    }

    if let Some(ref dupes) = result.dupes {
        all_issues.extend(report::build_duplication_codeclimate(
            &dupes.report,
            &dupes.config.root,
        ));
    }

    if let Some(ref health) = result.health {
        all_issues.extend(report::build_health_codeclimate(
            &health.report,
            &health.config.root,
        ));
    }

    serde_json::to_value(&all_issues).expect("CodeClimateIssue serializes infallibly")
}
