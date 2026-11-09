use std::process::ExitCode;

use fallow_output::CodeClimateIssue;

use super::diff_filter::DiffIndex;
use crate::exit_codes::exit_invalid_input;
use crate::report::emit_json;
use crate::report::mutation_evidence::MUTATION_SURVIVED_RULE;
use fallow_output::PrDecisionConclusion;
use fallow_output::{
    AuditReviewGate, CiIssue, CiProvider as Provider, ReviewCheckConclusion, ReviewEnvelopeOutput,
    ReviewEnvelopeRenderInput, ReviewEnvelopeTruncation, ReviewGitlabDiffRefs as GitlabDiffRefs,
    ReviewId, issues_from_codeclimate_issues,
};
use fallow_output::{
    MutationCiView, MutationHeading, mutation_review_summary_line, render_mutation_section,
};

/// The text a review summary body carries beside its inline-comment line.
///
/// The same two parts the sticky comment carries, so a setup that posts
/// only the review (`FALLOW_REVIEW=true`, `FALLOW_COMMENT=false`) sees them
/// when it posts a review. The review does not carry the gate rows: a review
/// is a point-in-time record, and the gate result travels on
/// `meta.check_conclusion`.
#[derive(Clone, Copy, Default)]
pub struct ReviewSummaryNotes<'a> {
    /// The blockquote note: the type-aware message, the baseline advisory,
    /// the gate inventory, or any combination.
    pub message: Option<&'a str>,
    /// The run diagnostics. The unmatched config patterns among them become
    /// a Markdown section, as in `--format markdown` and the sticky comment.
    pub config_patterns: &'a [fallow_config::WorkspaceDiagnostic],
    /// The gate policy of an audit run. The inline comments use it to say
    /// when the gate does not count a finding. `None` outside an audit.
    pub audit_gate: Option<AuditGatePolicy>,
}

impl<'a> From<super::pr_comment::PrCommentStatus<'a>> for ReviewSummaryNotes<'a> {
    fn from(status: super::pr_comment::PrCommentStatus<'a>) -> Self {
        Self {
            message: status.message,
            config_patterns: status.config_patterns,
            audit_gate: None,
        }
    }
}

/// The gate policy of an audit run: `--gate` and `--worsened`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AuditGatePolicy {
    /// `--gate new-only`: only introduced findings (and worsened findings
    /// under `--worsened error`) count in the gate.
    pub new_only: bool,
    /// How a worsened complexity finding affects the verdict.
    pub worsened: fallow_config::AuditWorsened,
}

impl AuditGatePolicy {
    /// Read the policy from the `attribution` object of a saved audit
    /// envelope. `None` when the envelope has no gate mode.
    #[must_use]
    pub fn from_audit_envelope(envelope: &serde_json::Value) -> Option<Self> {
        let attribution = envelope.get("attribution")?;
        let gate: fallow_config::AuditGate =
            serde_json::from_value(attribution.get("gate")?.clone()).ok()?;
        // An audit JSON from a release before `--worsened` has no
        // `worsened` field. Those releases used the `warn` behavior.
        let worsened = attribution
            .get("worsened")
            .and_then(|value| serde_json::from_value(value.clone()).ok())
            .unwrap_or_default();
        Some(Self {
            new_only: matches!(gate, fallow_config::AuditGate::NewOnly),
            worsened,
        })
    }

    /// The effect of one finding on the gate.
    const fn gate(self, context: fallow_output::AuditReviewContext) -> AuditReviewGate {
        if context.gate_exemption.is_some() {
            return AuditReviewGate::NotGatedTestFile;
        }
        if !self.new_only {
            return AuditReviewGate::Gated;
        }
        match context.attribution {
            fallow_output::AuditFindingAttribution::Introduced => AuditReviewGate::Gated,
            fallow_output::AuditFindingAttribution::Inherited => AuditReviewGate::NotGatedNewOnly,
            fallow_output::AuditFindingAttribution::Worsened => match self.worsened {
                fallow_config::AuditWorsened::Error => AuditReviewGate::Gated,
                fallow_config::AuditWorsened::Warn => AuditReviewGate::NotGatedWorsenedWarn,
                fallow_config::AuditWorsened::Off => AuditReviewGate::NotGatedWorsenedOff,
            },
        }
    }
}

/// Write the gate effect of the run into the audit facts of each issue.
fn apply_audit_gate(issues: &mut [CiIssue], policy: Option<AuditGatePolicy>) {
    let Some(policy) = policy else {
        return;
    };
    for issue in issues {
        if let Some(context) = issue.audit.as_mut() {
            context.gate = Some(policy.gate(*context));
        }
    }
}

#[must_use]
pub fn render_review_envelope(
    command: &str,
    provider: Provider,
    issues: &[CiIssue],
) -> ReviewEnvelopeOutput {
    render_review_envelope_with_diff(
        command,
        provider,
        issues,
        super::diff_filter::shared_diff_index(),
        None,
        None,
        ReviewSummaryNotes::default(),
    )
}

/// Render path the print site uses. Exposed so unit tests can pass a
/// hand-crafted `DiffIndex` without poking the process-wide `SHARED_DIFF`
/// cache (which is `OnceLock`-bounded and not reentrant under cargo test's
/// parallel runner).
#[must_use]
#[expect(
    clippy::too_many_arguments,
    reason = "the private render seam keeps test-injected diff and optional gate state explicit"
)]
fn render_review_envelope_with_diff(
    command: &str,
    provider: Provider,
    issues: &[CiIssue],
    diff_index: Option<&DiffIndex>,
    review_id: Option<&ReviewId>,
    conclusion: Option<ReviewCheckConclusion>,
    notes: ReviewSummaryNotes<'_>,
) -> ReviewEnvelopeOutput {
    render_review_envelope_with_mutation(
        command, provider, issues, diff_index, review_id, conclusion, notes, None,
    )
}

/// [`render_review_envelope_with_diff`] with the mutation evidence of a saved
/// render: the summary gets the mutation line and the mutation section before
/// the config-pattern section.
#[must_use]
#[expect(
    clippy::too_many_arguments,
    reason = "the private render seam keeps test-injected diff and optional gate state explicit"
)]
fn render_review_envelope_with_mutation(
    command: &str,
    provider: Provider,
    issues: &[CiIssue],
    diff_index: Option<&DiffIndex>,
    review_id: Option<&ReviewId>,
    conclusion: Option<ReviewCheckConclusion>,
    notes: ReviewSummaryNotes<'_>,
    mutation: Option<&MutationCiView>,
) -> ReviewEnvelopeOutput {
    let max = std::env::var("FALLOW_MAX_COMMENTS")
        .ok()
        .and_then(|v| v.parse::<usize>().ok())
        .unwrap_or(50);
    let gitlab_diff_refs = (provider == Provider::Gitlab)
        .then(gitlab_diff_refs_from_env)
        .flatten();
    let include_guidance = review_guidance_enabled();
    let trailing_section = super::pr_comment::compose_trailing_section(
        mutation.map(|view| {
            format!(
                "{}\n\n{}",
                mutation_review_summary_line(view, inline_survivor_count(issues, max)),
                render_mutation_section(view, MutationHeading::Review)
            )
        }),
        crate::report::config_pattern_text::markdown_section(notes.config_patterns),
    );
    let status_message = notes.message;

    let input = ReviewEnvelopeRenderInput {
        command,
        provider,
        issues,
        diff_index,
        path_prefix: crate::report::github::report_prefix(),
        max_comments: max,
        gitlab_diff_refs: gitlab_diff_refs.as_ref(),
        include_guidance,
        suggestion_block: &super::suggestion::suggestion_block,
        guidance_block: &review_guidance_block,
        trailing_section: trailing_section.as_deref(),
    };
    let rendered = match (review_id, conclusion) {
        (Some(review_id), Some(conclusion)) => {
            fallow_output::render_scoped_review_envelope_with_conclusion(
                &input,
                review_id,
                conclusion,
                status_message,
            )
        }
        (Some(review_id), None) => {
            fallow_output::render_scoped_review_envelope(&input, review_id, status_message)
        }
        (None, Some(conclusion)) => fallow_output::render_review_envelope_with_conclusion(
            &input,
            conclusion,
            status_message,
        ),
        (None, None) => fallow_output::render_review_envelope(&input, status_message),
    };
    note_review_truncation(rendered.truncation);
    rendered.envelope
}

/// The number of survivor findings that get an inline comment: the
/// survivors inside the first `max_groups` comment groups, as the renderer
/// groups them.
fn inline_survivor_count(issues: &[CiIssue], max_groups: usize) -> usize {
    fallow_output::group_review_issues_by_path_line(issues, max_groups)
        .groups
        .iter()
        .flatten()
        .filter(|issue| issue.rule_id == MUTATION_SURVIVED_RULE)
        .count()
}

/// The analysis findings and the survivor findings in one list for the
/// review. The renderer groups adjacent findings on one line into one
/// comment, so the merged list is sorted by location. Without survivors, the
/// list stays as it is.
fn merge_survivor_issues(mut issues: Vec<CiIssue>, survivors: Vec<CiIssue>) -> Vec<CiIssue> {
    if survivors.is_empty() {
        return issues;
    }
    issues.extend(survivors);
    issues
        .sort_by(|a, b| (&a.path, a.line, &a.fingerprint).cmp(&(&b.path, b.line, &b.fingerprint)));
    issues
}

/// Record telemetry for body-size or comment-count truncation of the review.
fn note_review_truncation(truncation: ReviewEnvelopeTruncation) {
    if truncation.body {
        crate::telemetry::note_report_truncation(
            true,
            crate::telemetry::TruncationReason::SizeLimit,
        );
    } else if truncation.comment_limit {
        crate::telemetry::note_report_truncation(
            true,
            crate::telemetry::TruncationReason::CommentLimit,
        );
    } else {
        crate::telemetry::note_report_truncation(
            false,
            crate::telemetry::TruncationReason::Unknown,
        );
    }
}

#[must_use]
pub(crate) fn print_review_envelope_from_codeclimate_issues(
    command: &str,
    provider: Provider,
    codeclimate: &[CodeClimateIssue],
    notes: ReviewSummaryNotes<'_>,
) -> ExitCode {
    print_review_envelope_with_mutation(command, provider, codeclimate, None, notes, None)
}

#[must_use]
pub(crate) fn print_review_envelope_from_codeclimate_issues_with_conclusion(
    command: &str,
    provider: Provider,
    codeclimate: &[CodeClimateIssue],
    conclusion: PrDecisionConclusion,
    notes: ReviewSummaryNotes<'_>,
) -> ExitCode {
    print_review_envelope_with_mutation(
        command,
        provider,
        codeclimate,
        Some(conclusion),
        notes,
        None,
    )
}

/// Print the review envelope of the analysis findings and, for a saved
/// render with mutation evidence, the survivor findings on the diff.
#[must_use]
pub(crate) fn print_review_envelope_with_mutation(
    command: &str,
    provider: Provider,
    codeclimate: &[CodeClimateIssue],
    conclusion: Option<PrDecisionConclusion>,
    notes: ReviewSummaryNotes<'_>,
    mutation: Option<super::pr_comment::MutationSurface<'_>>,
) -> ExitCode {
    let on_diff = |issues: &[CodeClimateIssue]| {
        super::diff_filter::filter_issues_from_env(issues_from_codeclimate_issues(issues))
    };
    let survivors = mutation.map_or_else(Vec::new, |mutation| on_diff(mutation.survivors));
    let mut issues = merge_survivor_issues(on_diff(codeclimate), survivors);
    apply_audit_gate(&mut issues, notes.audit_gate);
    print_review_envelope_from_ci_issues(
        command,
        provider,
        &issues,
        conclusion.map(review_conclusion),
        notes,
        mutation.map(|mutation| mutation.view),
    )
}

#[must_use]
fn print_review_envelope_from_ci_issues(
    command: &str,
    provider: Provider,
    issues: &[CiIssue],
    conclusion: Option<ReviewCheckConclusion>,
    notes: ReviewSummaryNotes<'_>,
    mutation: Option<&MutationCiView>,
) -> ExitCode {
    let review_id = match review_id_from_env() {
        Ok(review_id) => review_id,
        Err(error) => {
            eprintln!("Error: {error}");
            return exit_invalid_input();
        }
    };
    let envelope = render_review_envelope_with_mutation(
        command,
        provider,
        issues,
        super::diff_filter::shared_diff_index(),
        review_id.as_ref(),
        conclusion,
        notes,
        mutation,
    );
    let analysis_run_id = crate::output_runtime::telemetry_analysis_run_id();
    let value = match review_id.as_ref() {
        Some(review_id) => fallow_output::serialize_scoped_review_envelope_json_output(
            &envelope,
            review_id,
            analysis_run_id.as_deref(),
        ),
        None => fallow_output::serialize_review_envelope_json_output(
            envelope,
            analysis_run_id.as_deref(),
        )
        .map_err(|error| error.to_string()),
    };
    let value = match value {
        Ok(value) => value,
        Err(error) => {
            eprintln!("Error: failed to serialize review envelope: {error}");
            return exit_invalid_input();
        }
    };
    emit_json(&value, "review envelope")
}

/// Translate a gate conclusion into the review/check vocabulary. Shared with
/// the sticky comment renderer so both surfaces phrase one gate outcome the
/// same way.
pub(crate) const fn review_conclusion(conclusion: PrDecisionConclusion) -> ReviewCheckConclusion {
    match conclusion {
        PrDecisionConclusion::Success => ReviewCheckConclusion::Success,
        PrDecisionConclusion::Failure => ReviewCheckConclusion::Failure,
        PrDecisionConclusion::Neutral | PrDecisionConclusion::Skipped => {
            ReviewCheckConclusion::Neutral
        }
    }
}

fn review_id_from_env() -> Result<Option<ReviewId>, String> {
    let Some(value) = env_nonempty("FALLOW_REVIEW_ID") else {
        return Ok(None);
    };
    ReviewId::parse(value)
        .map(Some)
        .map_err(|error| format!("invalid FALLOW_REVIEW_ID: {error}"))
}

fn gitlab_diff_refs_from_env() -> Option<GitlabDiffRefs> {
    let base_sha = env_nonempty("FALLOW_GITLAB_BASE_SHA")
        .or_else(|| env_nonempty("CI_MERGE_REQUEST_DIFF_BASE_SHA"))?;
    let start_sha = env_nonempty("FALLOW_GITLAB_START_SHA").unwrap_or_else(|| base_sha.clone());
    let head_sha =
        env_nonempty("FALLOW_GITLAB_HEAD_SHA").or_else(|| env_nonempty("CI_COMMIT_SHA"))?;
    Some(GitlabDiffRefs {
        base_sha,
        start_sha,
        head_sha,
    })
}

fn env_nonempty(name: &str) -> Option<String> {
    std::env::var(name)
        .ok()
        .filter(|value| !value.trim().is_empty())
}

fn review_guidance_enabled() -> bool {
    std::env::var("FALLOW_REVIEW_GUIDANCE").is_ok_and(|value| env_truthy(&value))
}

fn env_truthy(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "1" | "true" | "yes" | "on"
    )
}

fn review_guidance_block(issue: &CiIssue) -> Option<String> {
    let rule = crate::explain::rule_by_id(&issue.rule_id)?;
    let guide = crate::explain::rule_guide(rule);
    let docs_url = crate::explain::rule_docs_url(rule);

    Some(format!(
        "\n\n<details><summary>What to do</summary>\n\n{}\n\n[Read the rule docs]({docs_url})\n\n</details>",
        guide.how_to_fix
    ))
}

#[cfg(test)]
fn render_merged_comment(
    provider: Provider,
    group: &[&CiIssue],
    gitlab_diff_refs: Option<&GitlabDiffRefs>,
    diff_index: Option<&DiffIndex>,
    include_guidance: bool,
) -> fallow_output::ReviewComment {
    fallow_output::render_review_comment_for_group(&fallow_output::ReviewCommentRenderInput {
        provider,
        group,
        gitlab_diff_refs,
        diff_index,
        path_prefix: "",
        include_guidance,
        suggestion_block: &super::suggestion::suggestion_block,
        guidance_block: &review_guidance_block,
    })
}

#[cfg(test)]
fn group_by_path_line(
    issues: &[CiIssue],
    max_groups: usize,
) -> fallow_output::GroupedReviewIssues<'_> {
    fallow_output::group_review_issues_by_path_line(issues, max_groups)
}

#[cfg(test)]
mod tests {
    use super::*;
    use fallow_output::{MARKER_PREFIX_V3, MARKER_SUFFIX_V3, MAX_COMMENT_BODY_BYTES};
    use fallow_output::{MARKER_REGEX_V3, ReviewComment};
    use serde_json::Value;

    fn to_value(envelope: &ReviewEnvelopeOutput) -> Value {
        serde_json::to_value(envelope).expect("ReviewEnvelopeOutput serializes infallibly")
    }

    fn comment_to_value(comment: &ReviewComment) -> Value {
        serde_json::to_value(comment).expect("ReviewComment serializes infallibly")
    }

    fn issue(rule: &str, sev: &str, path: &str, line: u64, fp: &str) -> CiIssue {
        CiIssue {
            rule_id: rule.into(),
            description: "desc".into(),
            severity: sev.into(),
            path: path.into(),
            line,
            end_line: None,
            other_locations: Vec::new(),
            fingerprint: fp.into(),
            legacy_fingerprint: None,
            audit: None,
        }
    }

    fn issue_with_desc(
        rule: &str,
        desc: impl Into<String>,
        sev: &str,
        path: &str,
        line: u64,
        fp: &str,
    ) -> CiIssue {
        CiIssue {
            rule_id: rule.into(),
            description: desc.into(),
            severity: sev.into(),
            path: path.into(),
            line,
            end_line: None,
            other_locations: Vec::new(),
            fingerprint: fp.into(),
            legacy_fingerprint: None,
            audit: None,
        }
    }

    #[test]
    fn scoped_renderer_uses_typed_meta_and_exact_markers() {
        let issue = issue(
            "fallow/unused-file",
            "minor",
            "src/a.ts",
            1,
            "abcdef0123456789",
        );
        let review_id = ReviewId::parse("frontend").unwrap();
        let envelope = render_review_envelope_with_diff(
            "check",
            Provider::Github,
            &[issue],
            None,
            Some(&review_id),
            None,
            ReviewSummaryNotes::default(),
        );
        let envelope = fallow_output::serialize_scoped_review_envelope_json_output(
            &envelope, &review_id, None,
        )
        .unwrap();

        assert_eq!(envelope["meta"]["review_id"], "frontend");
        assert!(
            envelope["body"]
                .as_str()
                .unwrap()
                .ends_with("<!-- fallow-review-id: frontend -->")
        );
        assert!(
            envelope["comments"][0]["body"]
                .as_str()
                .unwrap()
                .ends_with("<!-- fallow-review-id: frontend -->")
        );
    }

    #[test]
    fn github_review_envelope_matches_api_shape() {
        let issues = vec![issue(
            "fallow/unused-file",
            "minor",
            "src/a.ts",
            1,
            "abc1234567890def",
        )];
        let envelope = to_value(&render_review_envelope("check", Provider::Github, &issues));
        assert_eq!(envelope["event"], "COMMENT");
        assert_eq!(envelope["meta"]["schema"], "fallow-review-envelope/v3");
        assert_eq!(envelope["comments"][0]["path"], "src/a.ts");
        assert!(
            envelope["comments"][0]["body"]
                .as_str()
                .unwrap()
                .contains("fallow-fingerprint:v3:")
        );
    }

    #[test]
    fn review_summary_body_points_at_the_inline_comments() {
        let issues = vec![issue(
            "fallow/unused-file",
            "major",
            "src/a.ts",
            1,
            "abc1234567890def",
        )];
        let envelope = to_value(&render_review_envelope(
            "combined",
            Provider::Github,
            &issues,
        ));
        let body = envelope["body"].as_str().expect("body is string");

        assert!(!body.contains("Quality gate"), "{body}");
        assert!(
            body.contains(
                "1 inline comment on the changed lines. Open the Files changed tab to review."
            ),
            "{body}"
        );
        assert!(body.contains("<!-- fallow-review -->"), "{body}");
        assert_eq!(
            envelope["meta"]["check_conclusion"], "failure",
            "{envelope}"
        );
    }

    #[test]
    fn review_summary_body_lists_unmatched_config_patterns_before_the_markers() {
        let root = std::path::Path::new("/project");
        let diagnostics = [fallow_config::WorkspaceDiagnostic::new(
            root,
            root.to_path_buf(),
            fallow_config::WorkspaceDiagnosticKind::IgnoreDependenciesGlobUnmatched {
                pattern: "@typo/*".to_owned(),
            },
        )];
        let envelope = to_value(&render_review_envelope_with_diff(
            "dead-code",
            Provider::Gitlab,
            &[],
            None,
            None,
            None,
            ReviewSummaryNotes {
                message: Some("Baseline advisory."),
                config_patterns: &diagnostics,
                audit_gate: None,
            },
        ));
        let body = envelope["body"].as_str().expect("body is string");
        let note = body.find("> Baseline advisory.").expect("status note");
        let section = body
            .find("## Unmatched config patterns")
            .expect("config pattern section");
        let marker = body.find("<!-- fallow-review -->").expect("marker");
        assert!(note < section && section < marker, "{body}");
        assert!(
            body.contains("- `ignoreDependencies`: `@typo/*` matched nothing in this run"),
            "{body}"
        );
        assert_eq!(envelope["summary"]["body"], envelope["body"]);
    }

    fn mutation_view() -> fallow_output::MutationCiView {
        fallow_output::MutationCiView {
            scope: Some(fallow_output::MutationScopeKind::Changed),
            counts: fallow_output::MutationStatusCounts {
                total: 16,
                killed: 10,
                survived: 6,
                ..fallow_output::MutationStatusCounts::default()
            },
            score: Some(0.625),
            ..fallow_output::MutationCiView::new(fallow_output::MutationCiState::Ran)
        }
    }

    #[test]
    fn review_summary_states_the_mutation_line_and_section_before_the_markers() {
        let root = std::path::Path::new("/project");
        let diagnostics = [fallow_config::WorkspaceDiagnostic::new(
            root,
            root.to_path_buf(),
            fallow_config::WorkspaceDiagnosticKind::IgnoreDependenciesGlobUnmatched {
                pattern: "@typo/*".to_owned(),
            },
        )];
        let issues = vec![
            issue("fallow/unused-file", "minor", "src/a.ts", 1, "aaa"),
            issue(
                crate::report::mutation_evidence::MUTATION_SURVIVED_RULE,
                "minor",
                "src/age.ts",
                14,
                "bbb",
            ),
        ];
        let view = mutation_view();
        let envelope = to_value(&render_review_envelope_with_mutation(
            "dead-code",
            Provider::Github,
            &issues,
            None,
            None,
            None,
            ReviewSummaryNotes {
                message: None,
                config_patterns: &diagnostics,
                audit_gate: None,
            },
            Some(&view),
        ));
        let body = envelope["body"].as_str().expect("body is string");
        let line = body
            .find("Mutation testing: score 62.5%, 6 survivors (1 inline).")
            .expect("summary line");
        let section = body.find("#### Mutation testing").expect("section");
        let patterns = body
            .find("## Unmatched config patterns")
            .expect("config pattern section");
        let marker = body.find("<!-- fallow-review -->").expect("marker");
        assert!(
            line < section && section < patterns && patterns < marker,
            "{body}"
        );
        assert_eq!(envelope["comments"].as_array().map(Vec::len), Some(2));
    }

    #[test]
    fn the_inline_count_names_only_the_survivors_inside_the_comment_cap() {
        let survived = crate::report::mutation_evidence::MUTATION_SURVIVED_RULE;
        let issues = vec![
            issue("fallow/unused-file", "minor", "src/a.ts", 1, "aaa"),
            issue(survived, "minor", "src/age.ts", 14, "bbb"),
            issue(survived, "minor", "src/age.ts", 14, "ccc"),
            issue(survived, "minor", "src/age.ts", 20, "ddd"),
        ];
        assert_eq!(inline_survivor_count(&issues, 50), 3);
        assert_eq!(inline_survivor_count(&issues, 2), 2);
        assert_eq!(inline_survivor_count(&issues, 1), 0);
    }

    #[test]
    fn survivors_join_the_findings_in_location_order() {
        let analysis = vec![
            issue("fallow/unused-file", "minor", "src/b.ts", 5, "bbb"),
            issue("fallow/unused-file", "minor", "src/a.ts", 1, "aaa"),
        ];
        let unchanged = merge_survivor_issues(analysis.clone(), Vec::new());
        assert_eq!(
            unchanged, analysis,
            "no survivor keeps the analysis order as it is"
        );

        let survivor = issue(
            crate::report::mutation_evidence::MUTATION_SURVIVED_RULE,
            "minor",
            "src/a.ts",
            3,
            "ccc",
        );
        let merged = merge_survivor_issues(analysis, vec![survivor]);
        let locations: Vec<(&str, u64)> = merged
            .iter()
            .map(|issue| (issue.path.as_str(), issue.line))
            .collect();
        assert_eq!(
            locations,
            [("src/a.ts", 1), ("src/a.ts", 3), ("src/b.ts", 5)]
        );
    }

    #[test]
    fn a_review_without_mutation_evidence_is_the_plain_render() {
        let issues = vec![issue(
            "fallow/unused-file",
            "minor",
            "src/a.ts",
            1,
            "abc1234567890def",
        )];
        let plain = render_review_envelope_with_diff(
            "check",
            Provider::Github,
            &issues,
            None,
            None,
            None,
            ReviewSummaryNotes::default(),
        );
        let with_none = render_review_envelope_with_mutation(
            "check",
            Provider::Github,
            &issues,
            None,
            None,
            None,
            ReviewSummaryNotes::default(),
            None,
        );
        assert_eq!(to_value(&plain), to_value(&with_none));
    }

    #[test]
    fn github_comments_target_current_state_side() {
        let issue = issue("fallow/unused-file", "minor", "src/a.ts", 1, "abc");
        let comment = comment_to_value(&render_merged_comment(
            Provider::Github,
            &[&issue],
            None,
            None,
            false,
        ));
        assert_eq!(comment["side"], "RIGHT");
    }

    #[test]
    fn labels_major_issues_as_errors() {
        let issue = issue("fallow/unused-file", "major", "src/a.ts", 1, "abc");
        let comment = comment_to_value(&render_merged_comment(
            Provider::Github,
            &[&issue],
            None,
            None,
            false,
        ));
        assert!(comment["body"].as_str().unwrap().starts_with("**error**"));
    }

    fn audit_issue(
        attribution: fallow_output::AuditFindingAttribution,
        gate_exemption: Option<fallow_output::AuditGateExemption>,
    ) -> CiIssue {
        let mut issue = issue("fallow/high-complexity", "major", "src/a.ts", 1, "abc");
        issue.audit = Some(fallow_output::AuditReviewContext {
            attribution,
            gate_exemption,
            gate: None,
        });
        issue
    }

    fn audit_comment_body(issue: &CiIssue, policy: Option<AuditGatePolicy>) -> String {
        let mut issues = vec![issue.clone()];
        apply_audit_gate(&mut issues, policy);
        let comment = comment_to_value(&render_merged_comment(
            Provider::Github,
            &[&issues[0]],
            None,
            None,
            false,
        ));
        comment["body"].as_str().unwrap().to_owned()
    }

    #[test]
    fn audit_comment_label_follows_the_gate_policy() {
        use fallow_config::AuditWorsened;
        use fallow_output::{AuditFindingAttribution as A, AuditGateExemption};
        let new_only = |worsened| {
            Some(AuditGatePolicy {
                new_only: true,
                worsened,
            })
        };
        let cases = [
            (
                A::Introduced,
                None,
                new_only(AuditWorsened::Warn),
                "**error** · introduced `",
            ),
            (
                A::Inherited,
                None,
                new_only(AuditWorsened::Warn),
                "**error** · inherited · not gated (`--gate new-only`) `",
            ),
            (
                A::Worsened,
                None,
                new_only(AuditWorsened::Error),
                "**error** · worsened `",
            ),
            (
                A::Worsened,
                None,
                new_only(AuditWorsened::Warn),
                "**error** · worsened · not gated (`--worsened warn`) `",
            ),
            (
                A::Worsened,
                None,
                new_only(AuditWorsened::Off),
                "**error** · worsened · not gated (`--worsened off`) `",
            ),
            (
                A::Introduced,
                Some(AuditGateExemption::TestFile),
                new_only(AuditWorsened::Warn),
                "**error** · introduced · not gated (test file) `",
            ),
            (
                A::Inherited,
                None,
                Some(AuditGatePolicy {
                    new_only: false,
                    worsened: AuditWorsened::Warn,
                }),
                "**error** · inherited `",
            ),
            (A::Inherited, None, None, "**error** · inherited `"),
        ];
        for (attribution, exemption, policy, expected) in cases {
            let body = audit_comment_body(&audit_issue(attribution, exemption), policy);
            assert!(body.starts_with(expected), "{expected} != {body}");
        }
    }

    #[test]
    fn audit_gate_policy_reads_the_saved_attribution() {
        use fallow_config::AuditWorsened;
        let read = |value: serde_json::Value| AuditGatePolicy::from_audit_envelope(&value);
        assert_eq!(
            read(serde_json::json!({"attribution": {"gate": "new-only"}})),
            Some(AuditGatePolicy {
                new_only: true,
                worsened: AuditWorsened::Warn,
            })
        );
        assert_eq!(
            read(serde_json::json!({"attribution": {"gate": "all", "worsened": "error"}})),
            Some(AuditGatePolicy {
                new_only: false,
                worsened: AuditWorsened::Error,
            })
        );
        assert_eq!(read(serde_json::json!({"attribution": {}})), None);
        assert_eq!(read(serde_json::json!({})), None);
        assert_eq!(
            read(serde_json::json!({"attribution": {"gate": "new-only", "worsened": "off"}})),
            Some(AuditGatePolicy {
                new_only: true,
                worsened: AuditWorsened::Off,
            })
        );
        assert_eq!(
            read(serde_json::json!({"attribution": {"gate": "unknown"}})),
            None
        );
    }

    #[test]
    fn gitlab_comment_accepts_diff_refs() {
        let issue = issue("fallow/unused-file", "minor", "src/a.ts", 1, "abc");
        let refs = GitlabDiffRefs {
            base_sha: "base".into(),
            start_sha: "start".into(),
            head_sha: "head".into(),
        };
        let comment = comment_to_value(&render_merged_comment(
            Provider::Gitlab,
            &[&issue],
            Some(&refs),
            None,
            false,
        ));
        assert_eq!(comment["position"]["position_type"], "text");
        assert_eq!(comment["position"]["base_sha"], "base");
        assert_eq!(comment["position"]["start_sha"], "start");
        assert_eq!(comment["position"]["head_sha"], "head");
    }

    #[test]
    fn guidance_toggle_accepts_common_truthy_values() {
        for value in ["1", "true", "TRUE", "yes", "on", " On "] {
            assert!(env_truthy(value), "{value:?} should enable guidance");
        }
        for value in ["", "0", "false", "no", "off", "enabled"] {
            assert!(!env_truthy(value), "{value:?} should not enable guidance");
        }
    }

    #[test]
    fn guidance_disabled_omits_details_block() {
        let issue = issue(
            "fallow/high-complexity",
            "major",
            "src/a.ts",
            10,
            "abc1234567890def",
        );
        let comment = comment_to_value(&render_merged_comment(
            Provider::Github,
            &[&issue],
            None,
            None,
            false,
        ));
        let body = comment["body"].as_str().unwrap();
        assert!(!body.contains("<details><summary>What to do</summary>"));
        assert!(!body.contains("For function findings"));
    }

    #[test]
    fn guidance_enabled_appends_rule_guide_details() {
        let issue = issue(
            "fallow/high-complexity",
            "major",
            "src/a.ts",
            10,
            "abc1234567890def",
        );
        let comment = comment_to_value(&render_merged_comment(
            Provider::Github,
            &[&issue],
            None,
            None,
            true,
        ));
        let body = comment["body"].as_str().unwrap();
        assert!(body.contains("<details><summary>What to do</summary>"));
        assert!(body.contains("For function findings"));
        assert!(body.contains("[Read the rule docs]("));
        assert!(
            body.find("</details>").unwrap() < body.find("fallow-fingerprint:v3:").unwrap(),
            "guidance should render before the marker"
        );
    }

    #[test]
    fn guidance_attaches_to_each_merged_finding() {
        let complexity = issue("fallow/high-complexity", "major", "src/foo.ts", 42, "fp_a");
        let duplication = issue("fallow/code-duplication", "minor", "src/foo.ts", 42, "fp_b");
        let comment = comment_to_value(&render_merged_comment(
            Provider::Github,
            &[&complexity, &duplication],
            None,
            None,
            true,
        ));
        let body = comment["body"].as_str().unwrap();
        assert_eq!(
            body.matches("<details><summary>What to do</summary>")
                .count(),
            2
        );
        assert!(body.contains("For function findings"));
        assert!(body.contains("Extract the shared logic"));
    }

    #[test]
    fn envelope_emits_marker_regex_field_at_root() {
        let issues = vec![issue("fallow/unused-file", "minor", "src/a.ts", 1, "abc")];
        let env = to_value(&render_review_envelope("check", Provider::Github, &issues));
        let regex = env["marker_regex"].as_str().expect("marker_regex present");
        assert_eq!(regex, MARKER_REGEX_V3);
        assert!(regex.contains("[0-9a-f]{16}"));
        assert!(regex.starts_with('^'));
        assert!(regex.ends_with("\\s*$"));
        assert!(!regex.contains("(?m)"));
        assert!(regex.contains("((?:[a-z]+:)?[0-9a-f]{16})"));
        let flags = env["marker_regex_flags"]
            .as_str()
            .expect("marker_regex_flags present");
        assert_eq!(flags, "m");
    }

    #[test]
    fn envelope_emits_summary_block_with_fingerprint() {
        let issues = vec![issue("fallow/unused-file", "minor", "src/a.ts", 1, "abc")];
        let env = to_value(&render_review_envelope("check", Provider::Github, &issues));
        assert_eq!(env["summary"]["body"], env["body"]);
        let summary_fp = env["summary"]["fingerprint"].as_str().expect("fingerprint");
        assert_eq!(summary_fp.len(), 16);
        assert!(summary_fp.chars().all(|c| c.is_ascii_hexdigit()));
        let body_str = env["body"].as_str().unwrap();
        let marker_line = format!("{MARKER_PREFIX_V3}{summary_fp}{MARKER_SUFFIX_V3}");
        assert!(
            body_str.contains(&marker_line),
            "body must carry summary marker:\nbody={body_str}\nmarker={marker_line}"
        );
    }

    #[test]
    fn same_line_findings_merge_into_one_comment_with_composite_fingerprint() {
        let a = issue("fallow/unused-export", "minor", "src/foo.ts", 42, "fp_a");
        let b = issue("fallow/duplicate-export", "minor", "src/foo.ts", 42, "fp_b");
        let env = to_value(&render_review_envelope("check", Provider::Github, &[a, b]));
        assert_eq!(
            env["comments"].as_array().unwrap().len(),
            1,
            "two same-line findings must collapse to one comment"
        );
        let merged = &env["comments"][0];
        let fp = merged["fingerprint"].as_str().unwrap();
        assert!(
            fp.starts_with("merged:"),
            "merged comment fingerprint must start with merged:, got {fp}"
        );
        assert_eq!(fp.len(), 23);
        let body = merged["body"].as_str().unwrap();
        assert!(body.contains("fallow/unused-export"));
        assert!(body.contains("fallow/duplicate-export"));
        assert_eq!(
            body.matches("fallow-fingerprint:v3:").count(),
            1,
            "merged body must carry exactly one fingerprint marker"
        );
        assert!(
            merged.get("constituent_fingerprints").is_none(),
            "v2 hashed-composite design does not emit constituent_fingerprints"
        );
    }

    #[test]
    fn group_by_path_line_respects_max_groups_without_splitting_same_line_findings() {
        let a = issue("fallow/unused-export", "minor", "src/foo.ts", 42, "fp_a");
        let b = issue("fallow/duplicate-export", "minor", "src/foo.ts", 42, "fp_b");
        let c = issue("fallow/unused-type", "minor", "src/z.ts", 7, "fp_c");
        let issues = vec![a, b, c];

        let max_zero = group_by_path_line(&issues, 0);
        assert!(max_zero.groups.is_empty());
        assert!(max_zero.truncated);

        let max_one = group_by_path_line(&issues, 1);
        assert_eq!(max_one.groups.len(), 1);
        assert!(max_one.truncated);
        assert_eq!(max_one.groups[0].len(), 2);
        assert_eq!(max_one.groups[0][0].path, "src/foo.ts");
        assert_eq!(max_one.groups[0][0].line, 42);

        let max_two = group_by_path_line(&issues, 2);
        assert_eq!(max_two.groups.len(), 2);
        assert!(!max_two.truncated);
        assert_eq!(max_two.groups[0].len(), 2);
        assert_eq!(max_two.groups[1].len(), 1);
        assert_eq!(
            max_two.groups[0]
                .iter()
                .map(|issue| issue.fingerprint.as_str())
                .collect::<Vec<_>>(),
            ["fp_a", "fp_b"]
        );
    }

    #[test]
    fn single_finding_keeps_v1_fingerprint_shape() {
        let issues = vec![issue(
            "fallow/unused-file",
            "minor",
            "src/a.ts",
            1,
            "abc1234567890def",
        )];
        let env = to_value(&render_review_envelope("check", Provider::Github, &issues));
        let comment = &env["comments"][0];
        assert_eq!(comment["fingerprint"], "abc1234567890def");
        assert!(
            comment.get("constituent_fingerprints").is_none(),
            "single-finding comment must NOT emit constituent_fingerprints"
        );
        assert!(
            comment.get("truncated").is_none(),
            "non-truncated comment must NOT emit truncated"
        );
    }

    #[test]
    fn composite_fingerprint_shifts_when_constituents_change() {
        let a = issue("fallow/unused-export", "minor", "src/foo.ts", 42, "fp_a");
        let b = issue("fallow/duplicate-export", "minor", "src/foo.ts", 42, "fp_b");
        let c = issue("fallow/unused-type", "minor", "src/foo.ts", 42, "fp_c");
        let run1 = to_value(&render_review_envelope(
            "check",
            Provider::Github,
            &[a.clone(), b, c.clone()],
        ));
        let run2_drop_b = to_value(&render_review_envelope("check", Provider::Github, &[a, c]));
        assert_ne!(
            run1["comments"][0]["fingerprint"], run2_drop_b["comments"][0]["fingerprint"],
            "primary fingerprint must shift when a constituent drops"
        );
    }

    #[test]
    fn gitlab_old_path_pulls_from_diff_rename_map() {
        let rename_diff = "\
diff --git a/src/old.ts b/src/new.ts
similarity index 90%
rename from src/old.ts
rename to src/new.ts
--- a/src/old.ts
+++ b/src/new.ts
@@ -1,2 +1,3 @@
 keep
+added
 still
";
        let diff_index = DiffIndex::from_unified_diff(rename_diff);
        let issue = issue("fallow/unused-export", "minor", "src/new.ts", 2, "abc");
        let envelope = to_value(&render_review_envelope_with_diff(
            "check",
            Provider::Gitlab,
            &[issue],
            Some(&diff_index),
            None,
            None,
            ReviewSummaryNotes::default(),
        ));
        let position = &envelope["comments"][0]["position"];
        assert_eq!(position["old_path"], "src/old.ts");
        assert_eq!(position["new_path"], "src/new.ts");
    }

    #[test]
    fn gitlab_old_path_falls_back_to_new_path_without_rename() {
        let issue = issue("fallow/unused-export", "minor", "src/edit.ts", 5, "abc");
        let envelope = to_value(&render_review_envelope_with_diff(
            "check",
            Provider::Gitlab,
            &[issue],
            None,
            None,
            None,
            ReviewSummaryNotes::default(),
        ));
        let position = &envelope["comments"][0]["position"];
        assert_eq!(position["old_path"], "src/edit.ts");
        assert_eq!(position["new_path"], "src/edit.ts");
    }

    #[test]
    fn oversized_body_truncates_at_char_boundary_and_preserves_marker() {
        let huge_desc = "x".repeat(MAX_COMMENT_BODY_BYTES * 2);
        let issue = CiIssue {
            rule_id: "fallow/unused-export".into(),
            description: huge_desc,
            severity: "minor".into(),
            path: "src/a.ts".into(),
            line: 1,
            end_line: None,
            other_locations: Vec::new(),
            fingerprint: "abc1234567890def".into(),
            legacy_fingerprint: None,
            audit: None,
        };
        let comment = comment_to_value(&render_merged_comment(
            Provider::Github,
            &[&issue],
            None,
            None,
            false,
        ));
        let body = comment["body"].as_str().unwrap();
        assert!(
            body.len() <= MAX_COMMENT_BODY_BYTES,
            "body len {} must not exceed cap {MAX_COMMENT_BODY_BYTES}",
            body.len()
        );
        assert!(
            body.contains("fallow-fingerprint:v3:"),
            "marker must be preserved under truncation"
        );
        assert!(body.contains("<!-- fallow-truncated -->"));
        assert!(body.contains("> Body truncated by fallow."));
        assert_eq!(comment["truncated"], true);
        assert!(std::str::from_utf8(body.as_bytes()).is_ok());
    }

    #[test]
    fn oversized_guidance_body_truncates_and_preserves_marker() {
        let issue = issue_with_desc(
            "fallow/high-complexity",
            "x".repeat(MAX_COMMENT_BODY_BYTES * 2),
            "major",
            "src/a.ts",
            1,
            "abc1234567890def",
        );
        let comment = comment_to_value(&render_merged_comment(
            Provider::Github,
            &[&issue],
            None,
            None,
            true,
        ));
        let body = comment["body"].as_str().unwrap();
        assert!(body.len() <= MAX_COMMENT_BODY_BYTES);
        assert!(body.contains("<!-- fallow-truncated -->"));
        assert!(body.contains("fallow-fingerprint:v3:"));
        assert_eq!(comment["truncated"], true);
    }

    #[test]
    fn multibyte_body_truncates_at_char_boundary() {
        let huge_desc = "あ".repeat(MAX_COMMENT_BODY_BYTES);
        let issue = CiIssue {
            rule_id: "fallow/unused-export".into(),
            description: huge_desc,
            severity: "minor".into(),
            path: "src/a.ts".into(),
            line: 1,
            end_line: None,
            other_locations: Vec::new(),
            fingerprint: "abc1234567890def".into(),
            legacy_fingerprint: None,
            audit: None,
        };
        let comment = comment_to_value(&render_merged_comment(
            Provider::Github,
            &[&issue],
            None,
            None,
            false,
        ));
        let body = comment["body"].as_str().unwrap();
        assert!(std::str::from_utf8(body.as_bytes()).is_ok());
        assert!(body.len() <= MAX_COMMENT_BODY_BYTES);
        assert_eq!(comment["truncated"], true);
    }
}
