use crate::report::sink::outln;
use std::process::ExitCode;
use std::sync::OnceLock;

use serde_json::Value;

#[cfg(test)]
use fallow_output::is_project_level_rule;
use fallow_output::issues_from_codeclimate_issues;
pub use fallow_output::{
    CiIssue, CiProvider as Provider, CodeClimateIssue, PR_DECISION_SCHEMA, PR_DETAILS_SCHEMA,
    PrCommentEnvelope, PrCommentLayout, PrCommentTruncation, PrDecisionAnnotation,
    PrDecisionAnnotationLevel, PrDecisionConclusion, PrDecisionDetails, PrDecisionGate,
    PrDecisionSurface, PrDetailsArtifact, PrDetailsRow, PrDetailsSection, command_title,
    issues_from_codeclimate,
};

/// Workspace name, set once by `main()` when the binary is invoked with
/// `--workspace <name>`. Read by `sticky_marker_id` to auto-suffix the
/// sticky-comment marker per workspace, which keeps parallel per-workspace
/// jobs from racing each other's sticky body on the same PR/MR.
///
/// `OnceLock` gives us safe cross-function read-after-set without env-var
/// indirection. Only main writes; readers always observe the post-CLI-parse
/// state.
static WORKSPACE_MARKER: OnceLock<String> = OnceLock::new();

/// Set the workspace marker from a `--workspace` selection list.
///
/// Single workspace -> the name itself, sanitised for marker grammar.
/// N>1 workspaces -> a stable 6-char hex hash of the sorted, comma-joined
/// list, prefixed with `w-`. Sort + join is deterministic so the same
/// selection produces the same suffix across runs; two jobs with disjoint
/// selections get distinct markers and don't race.
#[allow(
    dead_code,
    reason = "called from main.rs bin target; lib target sees no caller"
)]
pub fn set_workspace_marker_from_list(values: &[String]) {
    let trimmed: Vec<&str> = values
        .iter()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .collect();
    if trimmed.is_empty() {
        return;
    }
    let marker = if let [single] = trimmed.as_slice() {
        (*single).to_owned()
    } else {
        let mut sorted = trimmed.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        sorted.sort();
        let joined = sorted.join(",");
        format!("w-{}", short_hex_hash(&joined))
    };
    let _ = WORKSPACE_MARKER.set(marker);
}

/// 6-char FNV-1a hex digest. Stable across Rust versions (FNV is content-
/// determined), short enough for a marker suffix, wide enough that the
/// chance of two real-world workspace selections colliding is ~1/16M.
fn short_hex_hash(value: &str) -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in value.bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    format!("{:06x}", (hash & 0x00ff_ffff) as u32)
}

/// What a run concluded, as the sticky comment and the decision sidecar carry
/// it: one blockquote note in the body, and one row per armed gate.
///
/// The two travel together because they are two renderings of the same
/// verdict, and a call that carried only the note is exactly how a tripped
/// gate reached the comment but not the Check Run (#2675).
#[derive(Clone, Copy)]
pub struct PrCommentStatus<'a> {
    /// The blockquote note appended to the body: the type-aware message, the
    /// baseline advisory, the gate inventory, or any combination.
    pub message: Option<&'a str>,
    /// One row per gate the run armed, appended to the decision surface's
    /// `gates` array after the command row.
    pub gates: &'a [PrDecisionGate],
}

/// Render the sticky comment body. `conclusion` is the gate outcome the caller
/// already computed, folded into the severity-derived verdict by the renderer;
/// `None` renders the severity-derived verdict alone.
#[must_use]
pub fn render_pr_comment(
    command: &str,
    provider: Provider,
    issues: &[CiIssue],
    conclusion: Option<PrDecisionConclusion>,
) -> String {
    fallow_output::render_pr_comment_with_verdict(
        &fallow_output::PrCommentRenderInput {
            command,
            provider,
            issues,
            marker_id: sticky_marker_id(),
            max_comments: max_comments(),
            category_for_rule: &category_for_rule,
        },
        conclusion.map(super::review::review_conclusion),
    )
}

/// [`render_pr_comment`] with the status note appended, which is the body both
/// integrations post.
///
/// Separate from the printing path so the body a reviewer reads is
/// snapshot-testable: the note carries the baseline advisory and the gate
/// inventory, and those are exactly the clauses a regression would drop.
#[must_use]
pub fn render_pr_comment_with_status_note(
    command: &str,
    provider: Provider,
    issues: &[CiIssue],
    conclusion: Option<PrDecisionConclusion>,
    status_message: Option<&str>,
) -> String {
    let mut body = render_pr_comment(command, provider, issues, conclusion);
    if let Some(message) = status_message {
        body.push_str("\n\n> ");
        body.push_str(message);
    }
    body
}

/// Map a fallow rule id to its category for sticky-comment grouping.
///
/// Single source of truth lives on `RuleDef::category` in `explain.rs`. This
/// helper does the lookup so callers don't need to know about the registry;
/// the look-up-then-fallback shape also keeps the renderer working for
/// rules a downstream consumer added without registering (rare). An
/// unregistered rule id is an absence of information, not evidence of
/// unreachable code, so it lands in "Other" rather than inflating the
/// "Dead code" section.
#[must_use]
fn category_for_rule(rule_id: &str) -> &'static str {
    crate::explain::rule_by_id(rule_id).map_or("Other", |def| def.category)
}

pub(crate) fn max_comments() -> usize {
    std::env::var("FALLOW_MAX_COMMENTS")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .unwrap_or(50)
}

#[must_use]
pub(crate) fn pr_comment_layout_from_env() -> PrCommentLayout {
    match std::env::var("FALLOW_PR_COMMENT_LAYOUT").as_deref() {
        Ok("compact") => PrCommentLayout::Compact,
        Ok("gate-only") => PrCommentLayout::GateOnly,
        Ok("details") => PrCommentLayout::Details,
        _ => PrCommentLayout::Default,
    }
}

/// Compute the sticky-comment marker id. Precedence (highest first):
///
/// 1. `FALLOW_COMMENT_ID` set by the user explicitly: use as-is.
/// 2. `WORKSPACE_MARKER` populated by `main()` from `--workspace <name>`:
///    suffix the default to avoid colliding with a sibling per-workspace
///    job's sticky on the same PR/MR.
/// 3. Plain `fallow-results`.
///
/// The collision case (2) is the common monorepo shape: parallel jobs each
/// run fallow scoped to one workspace package and post their own sticky.
/// Without a per-workspace suffix every job edits the same marker, racing
/// each other's bodies on every CI re-run.
pub(crate) fn sticky_marker_id() -> String {
    if let Ok(value) = std::env::var("FALLOW_COMMENT_ID")
        && !value.trim().is_empty()
    {
        return value;
    }
    let suffix = WORKSPACE_MARKER
        .get()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty())
        .map(sanitize_marker_segment);
    match suffix {
        Some(workspace) => format!("fallow-results-{workspace}"),
        None => "fallow-results".to_owned(),
    }
}

/// Strip characters that would break the HTML-comment marker. The marker
/// shape is `<!-- fallow-id: <id> -->`; `<`, `>`, and `--` are reserved by
/// the HTML comment grammar, and whitespace would split the id when the
/// reader scans for it.
fn sanitize_marker_segment(value: &str) -> String {
    value
        .chars()
        .map(|ch| {
            if ch.is_ascii_alphanumeric() || ch == '-' || ch == '_' || ch == '.' {
                ch
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_owned()
}

#[must_use]
pub(crate) fn print_pr_comment(
    command: &str,
    provider: Provider,
    codeclimate: &Value,
    status: PrCommentStatus<'_>,
) -> ExitCode {
    let issues = rebase_issue_paths(super::diff_filter::filter_issues_for_summary(
        issues_from_codeclimate(codeclimate),
    ));
    let conclusion = issue_decision_conclusion(issues.is_empty());
    print_pr_comment_from_ci_issues(command, provider, &issues, conclusion, status)
}

#[must_use]
pub(crate) fn print_pr_comment_with_status(
    command: &str,
    provider: Provider,
    codeclimate: &Value,
    conclusion: PrDecisionConclusion,
    status: PrCommentStatus<'_>,
) -> ExitCode {
    let issues = rebase_issue_paths(super::diff_filter::filter_issues_for_summary(
        issues_from_codeclimate(codeclimate),
    ));
    print_pr_comment_from_ci_issues(command, provider, &issues, conclusion, status)
}

#[must_use]
pub(crate) fn print_pr_comment_from_codeclimate_issues(
    command: &str,
    provider: Provider,
    codeclimate: &[CodeClimateIssue],
    conclusion: Option<PrDecisionConclusion>,
    status: PrCommentStatus<'_>,
) -> ExitCode {
    let issues = rebase_issue_paths(super::diff_filter::filter_issues_for_summary(
        issues_from_codeclimate_issues(codeclimate),
    ));
    let conclusion = conclusion.unwrap_or_else(|| issue_decision_conclusion(issues.is_empty()));
    print_pr_comment_from_ci_issues(command, provider, &issues, conclusion, status)
}

fn rebase_issue_paths(mut issues: Vec<CiIssue>) -> Vec<CiIssue> {
    let prefix = crate::report::github::report_prefix();
    if !prefix.is_empty() {
        for issue in &mut issues {
            issue.path = fallow_output::apply_path_prefix(prefix, &issue.path);
        }
    }
    issues
}

#[must_use]
fn print_pr_comment_from_ci_issues(
    command: &str,
    provider: Provider,
    issues: &[CiIssue],
    conclusion: PrDecisionConclusion,
    status: PrCommentStatus<'_>,
) -> ExitCode {
    let body = render_pr_comment_with_status_note(
        command,
        provider,
        issues,
        Some(conclusion),
        status.message,
    );
    let max_comments = max_comments();
    let envelope = PrCommentEnvelope {
        marker_id: sticky_marker_id(),
        body,
        is_clean: issues.is_empty() && conclusion == PrDecisionConclusion::Success,
        details_url: None,
        check_summary: Some(decision_summary_label(conclusion).to_owned()),
        truncation: PrCommentTruncation {
            truncated: issues.len() > max_comments,
            shown_findings: issues.len().min(max_comments),
            total_findings: issues.len(),
        },
    };
    let decision = build_issue_decision_surface(command, issues, &envelope, conclusion, status);
    let details = build_pr_details_artifact(command, issues);
    write_pr_comment_envelope_sidecar(&envelope);
    write_pr_decision_sidecar(&decision);
    write_pr_details_sidecar(&details);
    outln!("{}", envelope.body());
    ExitCode::SUCCESS
}

#[must_use]
fn build_issue_decision_surface(
    command: &str,
    issues: &[CiIssue],
    envelope: &PrCommentEnvelope,
    conclusion: PrDecisionConclusion,
    status: PrCommentStatus<'_>,
) -> PrDecisionSurface {
    // The command row stays first and the caller's `conclusion` stays the
    // surface's: the gate rows are additive display, and deriving the check-run
    // conclusion from them would turn an advisory check into a merge blocker
    // for every consumer with a required check.
    let mut gates = vec![PrDecisionGate {
        id: command.to_owned(),
        label: command_title(command).to_owned(),
        status: conclusion,
        observed: count_label(issues.len(), "finding", "findings"),
        threshold: None,
        scope: "new code".to_owned(),
    }];
    gates.extend(status.gates.iter().cloned());
    PrDecisionSurface {
        schema: PR_DECISION_SCHEMA.to_owned(),
        title: "Fallow".to_owned(),
        conclusion,
        gates,
        annotations: issues
            .iter()
            .take(max_comments())
            .map(decision_annotation_from_issue)
            .collect(),
        details: PrDecisionDetails {
            summary_markdown: decision_summary_markdown(conclusion, issues.len(), status.message),
            full_report_path: None,
            details_url: envelope.details_url.clone(),
        },
    }
}

/// Gate outcome for the PR decision surface, which `ci_check_run` maps
/// straight onto the GitHub check-run `conclusion`. Findings alone stay
/// `Neutral` here on purpose: promoting them to `Failure` would turn an
/// advisory check into a merge blocker for every consumer with a required
/// check. The sticky comment's verdict line answers a different question
/// (how severe are the findings), so the two intentionally differ.
fn issue_decision_conclusion(is_clean: bool) -> PrDecisionConclusion {
    if is_clean {
        PrDecisionConclusion::Success
    } else {
        PrDecisionConclusion::Neutral
    }
}

fn decision_summary_label(conclusion: PrDecisionConclusion) -> &'static str {
    match conclusion {
        PrDecisionConclusion::Success => "pass",
        PrDecisionConclusion::Failure => "fail",
        PrDecisionConclusion::Neutral => "warn",
        PrDecisionConclusion::Skipped => "skipped",
    }
}

fn decision_summary_markdown(
    conclusion: PrDecisionConclusion,
    issue_count: usize,
    status_message: Option<&str>,
) -> String {
    let summary = if issue_count == 0 {
        match conclusion {
            PrDecisionConclusion::Failure => {
                "Fallow quality gates failed without renderable findings.".to_owned()
            }
            PrDecisionConclusion::Neutral => {
                "Fallow needs review without renderable findings.".to_owned()
            }
            PrDecisionConclusion::Success | PrDecisionConclusion::Skipped => {
                "Fallow found no actionable PR findings.".to_owned()
            }
        }
    } else {
        let findings = count_label(issue_count, "finding", "findings");
        match conclusion {
            PrDecisionConclusion::Failure => {
                format!("Fallow quality gates failed with {findings}.")
            }
            PrDecisionConclusion::Neutral => format!("Fallow found {findings} for review."),
            PrDecisionConclusion::Success | PrDecisionConclusion::Skipped => {
                format!("Fallow found {findings}.")
            }
        }
    };
    match status_message {
        Some(message) => format!("{summary}\n\n> {message}"),
        None => summary,
    }
}

#[must_use]
pub(crate) fn build_pr_details_artifact(command: &str, issues: &[CiIssue]) -> PrDetailsArtifact {
    PrDetailsArtifact {
        schema: PR_DETAILS_SCHEMA.to_owned(),
        title: format!("Fallow {}", command_title(command)),
        sections: vec![PrDetailsSection {
            id: "findings".to_owned(),
            title: "Findings".to_owned(),
            rows: issues.iter().map(pr_details_row_from_issue).collect(),
        }],
    }
}

fn pr_details_row_from_issue(issue: &CiIssue) -> PrDetailsRow {
    PrDetailsRow {
        location: format!("{}:{}", issue.path, issue.line),
        rule: issue.rule_id.clone(),
        description: issue.description.clone(),
        fix: super::suggestion::fix_intent(issue).map(str::to_owned),
        fingerprint: (!issue.fingerprint.trim().is_empty()).then(|| issue.fingerprint.clone()),
    }
}

#[must_use]
pub(crate) fn decision_annotation_from_issue(issue: &CiIssue) -> PrDecisionAnnotation {
    PrDecisionAnnotation {
        path: issue.path.clone(),
        line: u32::try_from(issue.line).unwrap_or(u32::MAX),
        level: decision_level_from_severity(&issue.severity),
        title: issue.rule_id.clone(),
        message: issue.description.clone(),
        raw_details: super::suggestion::fix_intent(issue).map(str::to_owned),
    }
}

fn decision_level_from_severity(severity: &str) -> PrDecisionAnnotationLevel {
    match severity {
        "blocker" | "critical" | "major" => PrDecisionAnnotationLevel::Failure,
        "minor" => PrDecisionAnnotationLevel::Warning,
        _ => PrDecisionAnnotationLevel::Notice,
    }
}

fn count_label(count: usize, singular: &str, plural: &str) -> String {
    let noun = if count == 1 { singular } else { plural };
    format!("{count} {noun}")
}

pub(crate) fn write_pr_comment_envelope_sidecar(envelope: &PrCommentEnvelope) {
    let Ok(path) = std::env::var("FALLOW_PR_COMMENT_ENVELOPE_FILE") else {
        return;
    };
    if path.trim().is_empty() {
        return;
    }
    match serde_json::to_string_pretty(envelope)
        .map_err(|e| e.to_string())
        .and_then(|json| std::fs::write(&path, json).map_err(|e| e.to_string()))
    {
        Ok(()) => {}
        Err(e) => eprintln!("warning: failed to write PR comment envelope '{path}': {e}"),
    }
}

pub(crate) fn write_pr_decision_sidecar(surface: &PrDecisionSurface) {
    let Ok(path) = std::env::var("FALLOW_PR_DECISION_FILE") else {
        return;
    };
    if path.trim().is_empty() {
        return;
    }
    match serde_json::to_string_pretty(surface)
        .map_err(|e| e.to_string())
        .and_then(|json| std::fs::write(&path, json).map_err(|e| e.to_string()))
    {
        Ok(()) => {}
        Err(e) => eprintln!("warning: failed to write PR decision '{path}': {e}"),
    }
}

pub(crate) fn write_pr_details_sidecar(artifact: &PrDetailsArtifact) {
    let Ok(path) = std::env::var("FALLOW_PR_DETAILS_FILE") else {
        return;
    };
    if path.trim().is_empty() {
        return;
    }
    match serde_json::to_string_pretty(artifact)
        .map_err(|e| e.to_string())
        .and_then(|json| std::fs::write(&path, json).map_err(|e| e.to_string()))
    {
        Ok(()) => {}
        Err(e) => eprintln!("warning: failed to write PR details '{path}': {e}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fallow_output::{
        CodeClimateIssueKind, CodeClimateLines, CodeClimateLocation, CodeClimateSeverity,
    };

    #[test]
    fn typed_codeclimate_issues_extract_like_json_codeclimate() {
        let severities = [
            (CodeClimateSeverity::Info, "info"),
            (CodeClimateSeverity::Minor, "minor"),
            (CodeClimateSeverity::Major, "major"),
            (CodeClimateSeverity::Critical, "critical"),
            (CodeClimateSeverity::Blocker, "blocker"),
        ];
        let typed = severities
            .iter()
            .enumerate()
            .map(|(index, (severity, _))| CodeClimateIssue {
                kind: CodeClimateIssueKind::Issue,
                check_name: format!("fallow/rule-{index}"),
                description: format!("Finding {index}"),
                categories: vec!["Complexity".to_owned()],
                severity: *severity,
                fingerprint: format!("fp-{index}"),
                location: CodeClimateLocation {
                    path: format!("src/{index}.ts"),
                    lines: CodeClimateLines {
                        begin: u32::try_from(index + 1).expect("small fixture index"),
                        end: None,
                    },
                },
                other_locations: Vec::new(),
                owner: None,
                group: None,
            })
            .collect::<Vec<_>>();
        let value = serde_json::to_value(&typed).expect("typed fixture serializes");

        assert_eq!(
            issues_from_codeclimate_issues(&typed),
            issues_from_codeclimate(&value)
        );
        let typed_labels = issues_from_codeclimate_issues(&typed)
            .into_iter()
            .map(|issue| issue.severity)
            .collect::<Vec<_>>();
        let expected_labels = severities
            .iter()
            .map(|(_, label)| (*label).to_owned())
            .collect::<Vec<_>>();
        assert_eq!(typed_labels, expected_labels);
    }

    #[test]
    fn sticky_marker_id_default_when_nothing_set() {
        let body = render_pr_comment("check", Provider::Github, &[], None);
        assert!(body.contains("<!-- fallow-id: fallow-results"));
        assert!(body.contains("No findings for this pull request."));
    }

    #[test]
    fn short_hex_hash_is_deterministic_and_six_chars() {
        let a = short_hex_hash("api,worker");
        assert_eq!(a.len(), 6);
        assert_eq!(a, short_hex_hash("api,worker"));
        assert_ne!(a, short_hex_hash("admin,web"));
    }

    #[test]
    fn sanitize_marker_segment_collapses_unsafe_chars_to_dashes() {
        assert_eq!(sanitize_marker_segment("@fallow/runtime"), "fallow-runtime");
        assert_eq!(
            sanitize_marker_segment("packages/web ui"),
            "packages-web-ui"
        );
        assert_eq!(sanitize_marker_segment("plain"), "plain");
        assert_eq!(
            sanitize_marker_segment("--leading-trailing--"),
            "leading-trailing"
        );
    }

    #[test]
    fn is_project_level_rule_covers_config_anchored_dependency_findings() {
        for rule_id in fallow_output::PROJECT_LEVEL_RULE_IDS {
            assert!(
                is_project_level_rule(rule_id),
                "{rule_id} must be project-level"
            );
        }
        for rule_id in [
            "fallow/unused-file",
            "fallow/unused-export",
            "fallow/unused-type",
            "fallow/unused-enum-member",
            "fallow/unused-class-member",
            "fallow/unused-store-member",
            "fallow/unresolved-import",
            "fallow/unlisted-dependency",
            "fallow/duplicate-export",
            "fallow/circular-dependency",
            "fallow/re-export-cycle",
            "fallow/boundary-violation",
            "fallow/stale-suppression",
            "fallow/private-type-leak",
            "fallow/high-complexity",
            "fallow/high-crap-score",
        ] {
            assert!(
                !is_project_level_rule(rule_id),
                "{rule_id} must NOT be project-level"
            );
        }
    }

    #[test]
    fn decision_surface_preserves_blocking_conclusion_for_issue_output() {
        let issues = [CiIssue {
            path: "src/app.ts".to_owned(),
            line: 12,
            end_line: None,
            other_locations: Vec::new(),
            rule_id: "fallow/high-crap-score".to_owned(),
            description: "Function is hard to safely change.".to_owned(),
            severity: "minor".to_owned(),
            fingerprint: "abc".to_owned(),
        }];
        let envelope = PrCommentEnvelope {
            marker_id: "fallow-results".to_owned(),
            body: "body".to_owned(),
            is_clean: false,
            details_url: None,
            check_summary: Some("fail".to_owned()),
            truncation: PrCommentTruncation {
                truncated: false,
                shown_findings: 1,
                total_findings: 1,
            },
        };

        let decision = build_issue_decision_surface(
            "audit",
            &issues,
            &envelope,
            PrDecisionConclusion::Failure,
            PrCommentStatus {
                message: Some(crate::report::ci::TYPE_AWARE_INCOMPLETE_MESSAGE),
                gates: &[],
            },
        );

        assert_eq!(decision.conclusion, PrDecisionConclusion::Failure);
        assert_eq!(decision.gates[0].status, PrDecisionConclusion::Failure);
        assert!(decision.details.summary_markdown.contains("incomplete"));
        assert!(
            decision
                .details
                .summary_markdown
                .contains("quality gates failed")
        );
    }

    /// A gate row is additive display: it lands after the command row and does
    /// not move the surface `conclusion`, which `ci post-check-run` maps
    /// straight onto the GitHub check-run conclusion.
    #[test]
    fn gate_rows_follow_the_command_row_without_moving_the_conclusion() {
        let envelope = PrCommentEnvelope {
            marker_id: "fallow-results".to_owned(),
            body: "body".to_owned(),
            is_clean: true,
            details_url: None,
            check_summary: Some("pass".to_owned()),
            truncation: PrCommentTruncation {
                truncated: false,
                shown_findings: 0,
                total_findings: 0,
            },
        };
        let gates = [PrDecisionGate {
            id: "stale-baseline".to_owned(),
            label: "Stale baseline".to_owned(),
            status: PrDecisionConclusion::Failure,
            observed: "fail".to_owned(),
            threshold: None,
            scope: "this run".to_owned(),
        }];

        let decision = build_issue_decision_surface(
            "dead-code",
            &[],
            &envelope,
            PrDecisionConclusion::Success,
            PrCommentStatus {
                message: None,
                gates: &gates,
            },
        );

        assert_eq!(decision.conclusion, PrDecisionConclusion::Success);
        assert_eq!(decision.gates[0].id, "dead-code");
        assert_eq!(decision.gates[1].id, "stale-baseline");
        assert_eq!(decision.gates[1].scope, "this run");
    }

    #[test]
    fn a_body_without_a_note_is_the_bare_render() {
        let issues: Vec<CiIssue> = Vec::new();
        assert_eq!(
            render_pr_comment_with_status_note("check", Provider::Github, &issues, None, None),
            render_pr_comment("check", Provider::Github, &issues, None)
        );
    }

    #[test]
    fn project_level_rule_ids_each_register_in_explain_registry() {
        for rule_id in fallow_output::PROJECT_LEVEL_RULE_IDS {
            assert!(
                crate::explain::rule_by_id(rule_id).is_some(),
                "{rule_id} listed in PROJECT_LEVEL_RULE_IDS but not in explain registry"
            );
        }
    }
}
