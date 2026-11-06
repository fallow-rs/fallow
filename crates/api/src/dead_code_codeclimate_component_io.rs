//! CodeClimate issues for unused Svelte events and unused Angular inputs
//! and outputs.

use std::path::Path;

use fallow_config::Severity;
use fallow_output::{CodeClimateIssue, CodeClimateIssueInput, codeclimate_fingerprint_hash};

use super::{cc_path, dead_code_issue, finding_codeclimate};

pub(super) fn push_unused_svelte_event_issues(
    issues: &mut Vec<CodeClimateIssue>,
    findings: &[fallow_types::output_dead_code::UnusedSvelteEventFinding],
    root: &Path,
    severity: Severity,
) {
    if findings.is_empty() {
        return;
    }
    for entry in findings {
        let level = finding_codeclimate(entry, severity);
        let e = &entry.event;
        let path = cc_path(&e.path, root);
        let fp = codeclimate_fingerprint_hash(&[
            "fallow/unused-svelte-event",
            &path,
            &e.line.to_string(),
            &e.event_name,
        ]);
        let line = if e.line > 0 { Some(e.line) } else { None };
        let message = format!(
            "event `{}` is dispatched by component `{}` but listened to nowhere in the project (remove it or listen for it)",
            e.event_name, e.component_name
        );
        issues.push(dead_code_issue(
            entry.finding_id.as_deref(),
            entry.introduced,
            &[],
            CodeClimateIssueInput {
                check_name: "fallow/unused-svelte-event",
                description: &message,
                severity: level,
                category: "Bug Risk",
                path: &path,
                begin_line: line,
                fingerprint: &fp,
            },
        ));
    }
}

pub(super) fn push_unused_component_input_issues(
    issues: &mut Vec<CodeClimateIssue>,
    findings: &[fallow_types::output_dead_code::UnusedComponentInputFinding],
    root: &Path,
    severity: Severity,
) {
    if findings.is_empty() {
        return;
    }
    for entry in findings {
        let level = finding_codeclimate(entry, severity);
        let i = &entry.input;
        let path = cc_path(&i.path, root);
        let fp = codeclimate_fingerprint_hash(&[
            "fallow/unused-component-input",
            &path,
            &i.line.to_string(),
            &i.input_name,
        ]);
        let line = if i.line > 0 { Some(i.line) } else { None };
        let message = format!(
            "input `{}` is declared but referenced nowhere in component `{}` (remove it or use it)",
            i.input_name, i.component_name
        );
        issues.push(dead_code_issue(
            entry.finding_id.as_deref(),
            entry.introduced,
            &[],
            CodeClimateIssueInput {
                check_name: "fallow/unused-component-input",
                description: &message,
                severity: level,
                category: "Bug Risk",
                path: &path,
                begin_line: line,
                fingerprint: &fp,
            },
        ));
    }
}

pub(super) fn push_unused_component_output_issues(
    issues: &mut Vec<CodeClimateIssue>,
    findings: &[fallow_types::output_dead_code::UnusedComponentOutputFinding],
    root: &Path,
    severity: Severity,
) {
    if findings.is_empty() {
        return;
    }
    for entry in findings {
        let level = finding_codeclimate(entry, severity);
        let o = &entry.output;
        let path = cc_path(&o.path, root);
        let fp = codeclimate_fingerprint_hash(&[
            "fallow/unused-component-output",
            &path,
            &o.line.to_string(),
            &o.output_name,
        ]);
        let line = if o.line > 0 { Some(o.line) } else { None };
        let message = format!(
            "output `{}` is declared but emitted nowhere in component `{}` (remove it or emit it)",
            o.output_name, o.component_name
        );
        issues.push(dead_code_issue(
            entry.finding_id.as_deref(),
            entry.introduced,
            &[],
            CodeClimateIssueInput {
                check_name: "fallow/unused-component-output",
                description: &message,
                severity: level,
                category: "Bug Risk",
                path: &path,
                begin_line: line,
                fingerprint: &fp,
            },
        ));
    }
}
