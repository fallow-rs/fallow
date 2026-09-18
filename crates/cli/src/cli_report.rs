//! `fallow report --from <results.json>`: render an EXISTING fallow JSON
//! envelope in another format without re-running analysis (the analyze-once
//! flow: `fallow --format json -o results.json`, then one `report` call per
//! rendered surface).
//!
//! Supports GitHub-native text, CodeClimate, SARIF, and GitHub/GitLab PR
//! feedback formats. Dispatch is on the envelope's `kind` field, so any envelope
//! produced by `--format json`
//! (dead-code, dupes, health, audit, security, or the bare combined run)
//! renders byte-identically to the direct `--format` run. The `fallow fix`
//! envelope carries no `kind`; it is detected by its top-level fields and
//! rendered via [`EnvelopeKind::Fix`].

use std::path::Path;
use std::process::ExitCode;

use fallow_config::{FallowConfig, OutputFormat};
use fallow_output::{GroupByMode, PrDecisionConclusion};

use crate::report::ci::pr_comment::Provider;
use crate::report::github_annotations::{self, EnvelopeKind};
use crate::report::github_summary;
use crate::telemetry;

/// Run `fallow report --from <file>` with the global `--format` and `--root`.
pub fn run_report(
    from: &Path,
    output: OutputFormat,
    root: &Path,
    config_path: Option<&Path>,
) -> ExitCode {
    if let Some(path) = config_path
        && let Err(error) = FallowConfig::load(path)
    {
        return crate::emit_known_failure(
            &format!("failed to load report config {}: {error}", path.display()),
            2,
            output,
            telemetry::FailureReason::Validation,
        );
    }
    let target = match output {
        OutputFormat::GithubAnnotations => ReportTarget::GithubAnnotations,
        OutputFormat::GithubSummary => ReportTarget::GithubSummary,
        OutputFormat::CodeClimate => ReportTarget::CodeClimate,
        OutputFormat::Sarif => ReportTarget::Sarif,
        OutputFormat::PrCommentGithub => ReportTarget::PrComment(Provider::Github),
        OutputFormat::PrCommentGitlab => ReportTarget::PrComment(Provider::Gitlab),
        OutputFormat::ReviewGithub => ReportTarget::Review(Provider::Github),
        OutputFormat::ReviewGitlab => ReportTarget::Review(Provider::Gitlab),
        _ => {
            return crate::emit_known_failure(
                "fallow report supports --format github-annotations, github-summary, codeclimate, sarif, pr-comment-github, pr-comment-gitlab, review-github, or review-gitlab only",
                2,
                output,
                telemetry::FailureReason::UnsupportedFormat,
            );
        }
    };
    let envelope = match load_envelope(from, output) {
        Ok(envelope) => envelope,
        Err(code) => return code,
    };
    let saved = match prepare_saved_envelope(envelope, output) {
        Ok(saved) => saved,
        Err(code) => return code,
    };
    let kind = match envelope_kind(&saved.envelope, from, output) {
        Ok(kind) => kind,
        Err(code) => return code,
    };
    if let Err(error) = validate_report_target(target, kind) {
        return crate::emit_known_failure(
            &error,
            2,
            output,
            telemetry::FailureReason::UnsupportedFormat,
        );
    }
    if let Err(error) = validate_saved_report_envelope(kind, &saved.envelope) {
        return crate::emit_known_failure(&error, 2, output, telemetry::FailureReason::Validation);
    }
    let resolver = match saved_group_resolver(saved.grouped_by, root, config_path, output) {
        Ok(resolver) => resolver,
        Err(code) => return code,
    };
    match target {
        ReportTarget::GithubAnnotations => {
            github_annotations::print_annotations(kind, &saved.envelope, root)
        }
        ReportTarget::GithubSummary => github_summary::print_summary(kind, &saved.envelope, root),
        ReportTarget::CodeClimate => {
            crate::report::codeclimate::print_envelope_codeclimate_with_config(
                kind,
                &saved.envelope,
                root,
                config_path,
                resolver.as_ref(),
            )
        }
        ReportTarget::Sarif => crate::report::sarif::print_envelope_sarif_with_config(
            kind,
            &saved.envelope,
            root,
            config_path,
            resolver.as_ref(),
        ),
        ReportTarget::PrComment(provider) | ReportTarget::Review(provider) => {
            render_saved_ci_target(
                target,
                provider,
                kind,
                &saved.envelope,
                root,
                config_path,
                resolver.as_ref(),
                output,
            )
        }
    }
}

fn validate_report_target(target: ReportTarget, kind: EnvelopeKind) -> Result<(), String> {
    if matches!(target, ReportTarget::CodeClimate) && kind == EnvelopeKind::Security {
        return Err(
            "fallow security supports --format human, json, sarif, github-annotations, or github-summary only."
                .to_owned(),
        );
    }
    if matches!(target, ReportTarget::PrComment(_) | ReportTarget::Review(_))
        && matches!(kind, EnvelopeKind::Security | EnvelopeKind::Fix)
    {
        return Err(format!(
            "saved {} envelopes do not support --format {}",
            command_label(kind),
            report_target_label(target)
        ));
    }
    Ok(())
}

fn validate_saved_report_envelope(
    kind: EnvelopeKind,
    envelope: &serde_json::Value,
) -> Result<(), String> {
    match kind {
        EnvelopeKind::Security => fallow_output::validate_saved_security_envelope(envelope),
        EnvelopeKind::Fix => Ok(()),
        _ => crate::report::codeclimate::validate_saved_schema(kind, envelope),
    }
}

#[expect(
    clippy::too_many_arguments,
    reason = "saved CI dispatch carries the parsed envelope context without rebuilding it"
)]
fn render_saved_ci_target(
    target: ReportTarget,
    provider: Provider,
    kind: EnvelopeKind,
    envelope: &serde_json::Value,
    root: &Path,
    config_path: Option<&Path>,
    resolver: Option<&crate::report::OwnershipResolver>,
    output: OutputFormat,
) -> ExitCode {
    let issues = match crate::report::codeclimate::envelope_codeclimate_issues_with_config(
        kind,
        envelope,
        root,
        config_path,
        resolver,
    ) {
        Ok(issues) => issues,
        Err(error) => {
            return crate::emit_known_failure(
                &error,
                2,
                output,
                telemetry::FailureReason::Validation,
            );
        }
    };
    let (conclusion, status_message) = match saved_ci_conclusion(kind, envelope) {
        Ok(value) => value,
        Err(error) => {
            return crate::emit_known_failure(
                &error,
                2,
                output,
                telemetry::FailureReason::Validation,
            );
        }
    };
    let command = command_label(kind);
    // The comment and the job summary render from one envelope, so a run whose
    // gate failed must not say "Quality gate passed" on the surface a reviewer
    // actually reads while the summary says it failed. The gate line is carried
    // as the status note, which leaves the check-run `conclusion` alone: that
    // is a documented non-blocker and moving it is a separate decision.
    let status_message = saved_status_message(
        envelope,
        status_message,
        // The resolver exists exactly when the saved envelope was grouped, and
        // carries the same mode label the live render prints. The flattened
        // grouped dead-code envelope no longer has `grouped_by` to read.
        resolver.map(crate::report::OwnershipResolver::mode_label),
    );
    let status_message = status_message.as_deref();
    match target {
        ReportTarget::PrComment(_) => {
            crate::report::ci::pr_comment::print_pr_comment_from_codeclimate_issues(
                command,
                provider,
                &issues,
                conclusion,
                status_message,
            )
        }
        ReportTarget::Review(_) => match conclusion {
            Some(conclusion) => {
                crate::report::ci::review::print_review_envelope_from_codeclimate_issues_with_conclusion(
                    command,
                    provider,
                    &issues,
                    conclusion,
                    status_message,
                )
            }
            None => crate::report::ci::review::print_review_envelope_from_codeclimate_issues(
                command,
                provider,
                &issues,
                status_message,
            ),
        },
        _ => unreachable!("saved CI target dispatch only accepts comment and review targets"),
    }
}

/// The note a saved envelope's comment and review bodies carry: the existing
/// type-aware message, the gate verdict, whether the run did what it was
/// asked, a grouping this target cannot carry, or any combination.
///
/// Additive to whatever `saved_ci_conclusion` already produced, so the
/// type-aware message keeps its place and the later lines join it rather than
/// replacing it. The clause order matches `report::ci_status_note`, which is
/// what `the_live_and_saved_notes_agree` pins.
fn saved_status_message(
    envelope: &serde_json::Value,
    existing: Option<&'static str>,
    grouping_dropped: Option<&str>,
) -> Option<String> {
    let joined = [
        existing.map(str::to_owned),
        crate::report::gate_outcome_text::summary_line(envelope),
        crate::report::request_outcome_text::summary_line(envelope),
        grouping_dropped.map(crate::report::grouping_note::dropped_grouping_clause),
    ]
    .into_iter()
    .flatten()
    .collect::<Vec<_>>()
    .join(" ");
    if joined.is_empty() {
        return None;
    }
    Some(joined)
}

#[cfg(test)]
mod status_note_tests {
    fn gates() -> fallow_output::GateOutcomes {
        let mut gates = fallow_output::GateOutcomes::new();
        gates.insert(
            fallow_output::GateName::Regression,
            fallow_output::GateOutcome::measured(fallow_output::GateStatus::Fail, true, 5.0, 0.0),
        );
        gates
    }

    fn requests() -> fallow_output::RequestOutcomes {
        let mut requests = fallow_output::RequestOutcomes::new();
        requests.insert(
            fallow_output::RequestName::ChangedSince,
            fallow_output::RequestOutcome::not_applied(
                "origin/main",
                "invalid-ref",
                "--changed-since 'origin/main' was ignored.",
            ),
        );
        requests.insert(
            fallow_output::RequestName::DiffFilter,
            fallow_output::RequestOutcome::applied("--diff-file pr.diff"),
        );
        requests
    }

    /// The live path builds the note from typed gates and the saved path from
    /// the parsed envelope. They must produce the same string, or
    /// `report --from` stops being byte-identical to a direct render.
    #[test]
    fn the_live_and_saved_notes_agree() {
        let gates = gates();
        let envelope = serde_json::json!({ "gate_outcomes": gates });
        assert_eq!(
            super::saved_status_message(&envelope, None, None),
            crate::report::ci_status_note(None, Some(&gates), None, None),
        );
        assert_eq!(
            super::saved_status_message(&envelope, Some("Note."), None),
            crate::report::ci_status_note(Some("Note."), Some(&gates), None, None),
        );
    }

    /// A grouped envelope rendered into a body that cannot carry groups says
    /// so, and says it identically whether the body was rendered live or from
    /// the saved envelope (issue #2691).
    #[test]
    fn the_live_and_saved_grouping_notes_agree() {
        let envelope = serde_json::json!({ "kind": "health" });
        let saved = super::saved_status_message(&envelope, None, Some("owner"))
            .expect("a grouped render has something to say");
        assert_eq!(
            Some(saved.clone()),
            crate::report::ci_status_note(None, None, None, Some("owner")),
        );
        assert!(
            saved.contains("--group-by owner was requested"),
            "the requested mode reaches the rendered body: {saved}"
        );

        assert!(super::saved_status_message(&envelope, None, None).is_none());
    }

    /// The same parity for the run's requests, and for a body that carries
    /// every clause at once: the clause order is part of the contract, not an
    /// accident of which renderer ran.
    #[test]
    fn the_live_and_saved_request_notes_agree() {
        let requests = requests();
        let envelope = serde_json::json!({ "request_outcomes": requests });
        assert_eq!(
            super::saved_status_message(&envelope, None, None),
            crate::report::ci_status_note(None, None, Some(&requests), None),
        );

        let gates = gates();
        let both = serde_json::json!({
            "gate_outcomes": gates,
            "request_outcomes": requests,
        });
        let saved = super::saved_status_message(&both, Some("Note."), Some("package"))
            .expect("a note for a run with something to say");
        assert_eq!(
            Some(saved.clone()),
            crate::report::ci_status_note(
                Some("Note."),
                Some(&gates),
                Some(&requests),
                Some("package")
            ),
        );
        assert!(
            saved.starts_with("Note. Gate outcomes:"),
            "the type-aware message keeps its place ahead of the verdicts: {saved}"
        );
        assert!(
            saved.contains("Request outcomes: not applied changed-since (invalid-ref)"),
            "an unapplied request reaches the rendered body: {saved}"
        );
        assert!(
            saved.ends_with("run --format json for the grouped envelope."),
            "the grouping clause comes last: {saved}"
        );
    }

    /// A run with nothing to say renders no note, so a body produced before
    /// either object existed stays byte-identical.
    #[test]
    fn a_run_with_nothing_to_say_renders_no_note() {
        let bare = serde_json::json!({ "kind": "dead-code" });
        assert!(super::saved_status_message(&bare, None, None).is_none());
        assert!(crate::report::ci_status_note(None, None, None, None).is_none());
    }
}

fn saved_ci_conclusion(
    kind: EnvelopeKind,
    envelope: &serde_json::Value,
) -> Result<(Option<PrDecisionConclusion>, Option<&'static str>), String> {
    let type_aware = crate::report::ci::saved_type_aware_metadata(envelope)?;
    if type_aware
        .iter()
        .any(|meta| crate::report::ci::required_type_aware_incomplete(Some(meta)))
    {
        return Ok((
            Some(PrDecisionConclusion::Failure),
            Some(crate::report::ci::TYPE_AWARE_INCOMPLETE_MESSAGE),
        ));
    }
    if kind != EnvelopeKind::Audit {
        return Ok((None, None));
    }
    let verdict = envelope
        .get("verdict")
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| "saved audit envelope is missing its `verdict`".to_owned())?;
    let conclusion = match verdict {
        "pass" => PrDecisionConclusion::Success,
        "warn" => PrDecisionConclusion::Neutral,
        "fail" => PrDecisionConclusion::Failure,
        other => {
            return Err(format!(
                "saved audit envelope has unsupported verdict `{other}`"
            ));
        }
    };
    Ok((Some(conclusion), None))
}

const fn command_label(kind: EnvelopeKind) -> &'static str {
    match kind {
        EnvelopeKind::DeadCode => "dead-code",
        EnvelopeKind::Dupes => "dupes",
        EnvelopeKind::Health => "health",
        EnvelopeKind::Audit => "audit",
        EnvelopeKind::Combined => "combined",
        EnvelopeKind::Security => "security",
        EnvelopeKind::Fix => "fix",
    }
}

const fn report_target_label(target: ReportTarget) -> &'static str {
    match target {
        ReportTarget::PrComment(Provider::Github) => "pr-comment-github",
        ReportTarget::PrComment(Provider::Gitlab) => "pr-comment-gitlab",
        ReportTarget::Review(Provider::Github) => "review-github",
        ReportTarget::Review(Provider::Gitlab) => "review-gitlab",
        ReportTarget::GithubAnnotations => "github-annotations",
        ReportTarget::GithubSummary => "github-summary",
        ReportTarget::CodeClimate => "codeclimate",
        ReportTarget::Sarif => "sarif",
    }
}

#[derive(Debug)]
struct SavedEnvelope {
    envelope: serde_json::Value,
    grouped_by: Option<GroupByMode>,
}

pub const NORMALIZED_GROUPED_DEAD_CODE_MARKER: &str = "_fallow_report_normalized_grouped_dead_code";

fn prepare_saved_envelope(
    envelope: serde_json::Value,
    output: OutputFormat,
) -> Result<SavedEnvelope, ExitCode> {
    normalize_saved_envelope(envelope).map_err(|error| {
        crate::emit_known_failure(&error, 2, output, telemetry::FailureReason::Validation)
    })
}

fn normalize_saved_envelope(mut envelope: serde_json::Value) -> Result<SavedEnvelope, String> {
    if let Some(root) = envelope.as_object_mut() {
        root.remove(NORMALIZED_GROUPED_DEAD_CODE_MARKER);
    }
    let grouped_by = envelope
        .get("grouped_by")
        .and_then(serde_json::Value::as_str)
        .and_then(parse_group_by_mode);
    if envelope.get("kind").and_then(serde_json::Value::as_str) != Some("dead-code-grouped") {
        return Ok(SavedEnvelope {
            envelope,
            grouped_by,
        });
    }
    let Some(root) = envelope.as_object_mut() else {
        return Err("saved grouped dead-code envelope must be an object".to_owned());
    };
    let grouped_by = grouped_by.ok_or_else(|| {
        "saved grouped dead-code envelope has an unsupported or missing `grouped_by`".to_owned()
    })?;
    let groups_value = root.remove("groups").ok_or_else(|| {
        "saved grouped dead-code envelope is missing required field `groups`".to_owned()
    })?;
    let groups = groups_value.as_array().ok_or_else(|| {
        "saved grouped dead-code envelope field `groups` must be an array".to_owned()
    })?;
    let current_schema = root
        .get("schema_version")
        .and_then(serde_json::Value::as_u64)
        == Some(u64::from(fallow_output::CHECK_SCHEMA_VERSION));
    if current_schema {
        validate_current_grouped_dead_code(root, groups)?;
    }
    root.remove("grouped_by");
    root.insert(
        "kind".to_string(),
        serde_json::Value::String("dead-code".to_string()),
    );
    root.insert(
        NORMALIZED_GROUPED_DEAD_CODE_MARKER.to_string(),
        serde_json::Value::Bool(true),
    );
    for group in groups {
        let Some(group) = group.as_object() else {
            continue;
        };
        for (key, value) in group {
            if matches!(key.as_str(), "key" | "owners" | "total_issues") {
                continue;
            }
            let Some(items) = value.as_array() else {
                continue;
            };
            let target = root
                .entry(key.clone())
                .or_insert_with(|| serde_json::Value::Array(Vec::new()));
            if let Some(target) = target.as_array_mut() {
                target.extend(items.iter().cloned());
            }
        }
    }
    Ok(SavedEnvelope {
        envelope,
        grouped_by: Some(grouped_by),
    })
}

fn validate_current_grouped_dead_code(
    root: &serde_json::Map<String, serde_json::Value>,
    groups: &[serde_json::Value],
) -> Result<(), String> {
    let root_total = root
        .get("total_issues")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| {
            "saved grouped dead-code envelope is missing a non-negative integer `total_issues`"
                .to_owned()
        })?;
    let mut grouped_total = 0_u64;
    for (index, group) in groups.iter().enumerate() {
        let Some(group_object) = group.as_object() else {
            return Err(format!(
                "saved grouped dead-code envelope group {index} must be an object"
            ));
        };
        if !group_object
            .get("key")
            .is_some_and(serde_json::Value::is_string)
        {
            return Err(format!(
                "saved grouped dead-code envelope group {index} is missing a string `key`"
            ));
        }
        let declared_total = group_object
            .get("total_issues")
            .and_then(serde_json::Value::as_u64)
            .ok_or_else(|| {
                format!(
                    "saved grouped dead-code envelope group {index} is missing a non-negative integer `total_issues`"
                )
            })?;
        let results =
            serde_json::from_value::<fallow_types::results::AnalysisResults>(group.clone())
                .map_err(|error| {
                    format!(
                        "saved grouped dead-code envelope group {index} is incompatible with this Fallow version: {error}"
                    )
                })?;
        let actual_total = u64::try_from(results.total_issues()).unwrap_or(u64::MAX);
        if declared_total != actual_total {
            return Err(format!(
                "saved grouped dead-code envelope group {index} declares {declared_total} findings but contains {actual_total}"
            ));
        }
        grouped_total = grouped_total.saturating_add(actual_total);
    }
    if root_total != grouped_total {
        return Err(format!(
            "saved grouped dead-code envelope declares {root_total} findings but its groups contain {grouped_total}"
        ));
    }
    Ok(())
}

fn parse_group_by_mode(value: &str) -> Option<GroupByMode> {
    match value {
        "owner" => Some(GroupByMode::Owner),
        "directory" => Some(GroupByMode::Directory),
        "package" => Some(GroupByMode::Package),
        "section" => Some(GroupByMode::Section),
        _ => None,
    }
}

fn saved_group_resolver(
    grouped_by: Option<GroupByMode>,
    root: &Path,
    config_path: Option<&Path>,
    output: OutputFormat,
) -> Result<Option<crate::report::OwnershipResolver>, ExitCode> {
    let codeowners = config_path
        .and_then(|path| FallowConfig::load(path).ok())
        .and_then(|config| config.codeowners)
        .or_else(|| {
            FallowConfig::find_and_load(root)
                .ok()
                .flatten()
                .and_then(|(config, _)| config.codeowners)
        });
    crate::runtime_support::build_ownership_resolver_for_mode(
        grouped_by,
        root,
        codeowners.as_deref(),
        output,
    )
}

#[derive(Clone, Copy)]
enum ReportTarget {
    GithubAnnotations,
    GithubSummary,
    CodeClimate,
    Sarif,
    PrComment(Provider),
    Review(Provider),
}

fn load_envelope(from: &Path, output: OutputFormat) -> Result<serde_json::Value, ExitCode> {
    let source = std::fs::read_to_string(from).map_err(|err| {
        crate::emit_known_failure(
            &format!("failed to read {}: {err}", from.display()),
            2,
            output,
            telemetry::FailureReason::Validation,
        )
    })?;
    serde_json::from_str(&source).map_err(|err| {
        crate::emit_known_failure(
            &format!(
                "{} is not valid JSON ({err}); generate it with `fallow ... --format json`",
                from.display()
            ),
            2,
            output,
            telemetry::FailureReason::Validation,
        )
    })
}

fn envelope_kind(
    envelope: &serde_json::Value,
    from: &Path,
    output: OutputFormat,
) -> Result<EnvelopeKind, ExitCode> {
    let Some(kind) = envelope.get("kind").and_then(serde_json::Value::as_str) else {
        // The `fallow fix --format json` envelope is the only kind-less document
        // fallow emits (crates/output/src/fix.rs: no top-level `kind`). Resolve it
        // by field detection so `report --from <fix-results.json>` renders the fix
        // job summary natively; genuinely unrecognized documents keep erroring.
        if is_fix_envelope(envelope) {
            return Ok(EnvelopeKind::Fix);
        }
        return Err(crate::emit_known_failure(
            &format!(
                "{} is not a fallow results envelope (missing top-level `kind`); \
                 generate it with `fallow ... --format json`",
                from.display()
            ),
            2,
            output,
            telemetry::FailureReason::Validation,
        ));
    };
    parse_envelope_kind(kind).ok_or_else(|| {
        crate::emit_known_failure(
            &format!(
                "unsupported envelope kind `{kind}` in {}; fallow report renders dead-code, \
                 dupes, health, audit, security, and combined envelopes",
                from.display()
            ),
            2,
            output,
            telemetry::FailureReason::Validation,
        )
    })
}

/// Map the `--format json` root `kind` onto the renderer dispatch. The fix
/// envelope has no `kind` field; it is resolved separately via
/// [`is_fix_envelope`] field detection (see [`envelope_kind`]).
fn parse_envelope_kind(kind: &str) -> Option<EnvelopeKind> {
    match kind {
        "dead-code" => Some(EnvelopeKind::DeadCode),
        "dupes" => Some(EnvelopeKind::Dupes),
        "health" => Some(EnvelopeKind::Health),
        "audit" => Some(EnvelopeKind::Audit),
        "security" => Some(EnvelopeKind::Security),
        "combined" => Some(EnvelopeKind::Combined),
        _ => None,
    }
}

/// Recognize a kind-less `fallow fix --format json` envelope by its stable
/// top-level keys. The fix root always carries both a `fixes` array and a
/// numeric `total_fixed` (see `crates/output/src/fix.rs::FixJsonOutput`); no
/// other fallow envelope is kind-less, so the two keys together are an
/// unambiguous signal.
fn is_fix_envelope(envelope: &serde_json::Value) -> bool {
    envelope
        .get("fixes")
        .is_some_and(serde_json::Value::is_array)
        && envelope
            .get("total_fixed")
            .is_some_and(serde_json::Value::is_number)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_envelope_kind_covers_supported_kinds() {
        assert_eq!(
            parse_envelope_kind("dead-code"),
            Some(EnvelopeKind::DeadCode)
        );
        assert_eq!(parse_envelope_kind("dupes"), Some(EnvelopeKind::Dupes));
        assert_eq!(parse_envelope_kind("health"), Some(EnvelopeKind::Health));
        assert_eq!(parse_envelope_kind("audit"), Some(EnvelopeKind::Audit));
        assert_eq!(
            parse_envelope_kind("security"),
            Some(EnvelopeKind::Security)
        );
        assert_eq!(
            parse_envelope_kind("combined"),
            Some(EnvelopeKind::Combined)
        );
    }

    #[test]
    fn parse_envelope_kind_rejects_unknown_and_grouped_kinds() {
        assert_eq!(parse_envelope_kind("dead-code-grouped"), None);
        assert_eq!(parse_envelope_kind("feature-flags"), None);
        assert_eq!(parse_envelope_kind(""), None);
    }

    #[test]
    fn saved_audit_verdict_maps_to_ci_conclusion() {
        for (verdict, expected) in [
            ("pass", PrDecisionConclusion::Success),
            ("warn", PrDecisionConclusion::Neutral),
            ("fail", PrDecisionConclusion::Failure),
        ] {
            let envelope = serde_json::json!({ "verdict": verdict });
            let (conclusion, status) =
                saved_ci_conclusion(EnvelopeKind::Audit, &envelope).expect("audit verdict");
            assert_eq!(conclusion, Some(expected));
            assert_eq!(status, None);
        }
    }

    #[test]
    fn saved_required_incomplete_type_aware_result_fails_closed() {
        let identity = fallow_types::semantic::SemanticAnalysisIdentity {
            completeness: fallow_types::semantic::SemanticCompleteness::Partial,
            ..fallow_types::semantic::SemanticAnalysisIdentity::default()
        };
        let meta = fallow_types::envelope::TypeAwareMeta {
            identity: Some(identity),
            required_completeness: Some(
                fallow_types::semantic::SemanticCompletenessRequirement::Complete,
            ),
            ..fallow_types::envelope::TypeAwareMeta::default()
        };
        let envelope = serde_json::json!({
            "verdict": "pass",
            "_meta": { "type_aware": meta }
        });
        let (conclusion, status) =
            saved_ci_conclusion(EnvelopeKind::Audit, &envelope).expect("type-aware gate");
        assert_eq!(conclusion, Some(PrDecisionConclusion::Failure));
        assert_eq!(
            status,
            Some(crate::report::ci::TYPE_AWARE_INCOMPLETE_MESSAGE)
        );
    }

    #[test]
    fn saved_required_type_aware_result_without_identity_fails_closed() {
        let meta = fallow_types::envelope::TypeAwareMeta {
            required_completeness: Some(
                fallow_types::semantic::SemanticCompletenessRequirement::Complete,
            ),
            ..fallow_types::envelope::TypeAwareMeta::default()
        };
        let envelope = serde_json::json!({
            "_meta": { "type_aware": meta }
        });
        let (conclusion, status) =
            saved_ci_conclusion(EnvelopeKind::DeadCode, &envelope).expect("missing identity gate");
        assert_eq!(conclusion, Some(PrDecisionConclusion::Failure));
        assert_eq!(
            status,
            Some(crate::report::ci::TYPE_AWARE_INCOMPLETE_MESSAGE)
        );
    }

    #[test]
    fn malformed_saved_type_aware_metadata_is_rejected() {
        let mut meta = serde_json::to_value(fallow_types::envelope::TypeAwareMeta::default())
            .expect("serialize type-aware metadata");
        meta["queries"] = serde_json::json!("not-an-array");
        let envelope = serde_json::json!({
            "_meta": { "type_aware": meta }
        });

        let error = saved_ci_conclusion(EnvelopeKind::DeadCode, &envelope)
            .expect_err("malformed type-aware metadata must fail closed");
        assert!(error.contains("saved type-aware metadata at `/_meta/type_aware`"));
        assert!(error.contains("invalid type"));
    }

    #[test]
    fn grouped_dead_code_is_flattened_for_saved_renderers() {
        let normalized = normalize_saved_envelope(serde_json::json!({
            "kind": "dead-code-grouped",
            "grouped_by": "owner",
            "total_issues": 1,
            "groups": [{
                "key": "@team",
                "owners": ["@team"],
                "total_issues": 1,
                "unused_files": [{"path": "src/dead.ts", "actions": []}]
            }]
        }))
        .expect("valid grouped envelope");

        assert_eq!(normalized.grouped_by, Some(GroupByMode::Owner));
        assert_eq!(normalized.envelope["kind"], "dead-code");
        assert_eq!(
            normalized.envelope["unused_files"][0]["path"],
            "src/dead.ts"
        );
        assert!(normalized.envelope.get("groups").is_none());
        assert!(normalized.envelope.get("grouped_by").is_none());
        assert_eq!(
            normalized.envelope[NORMALIZED_GROUPED_DEAD_CODE_MARKER],
            true
        );
    }

    #[test]
    fn malformed_current_grouped_dead_code_fails_before_flattening() {
        let error = normalize_saved_envelope(serde_json::json!({
            "kind": "dead-code-grouped",
            "schema_version": fallow_output::CHECK_SCHEMA_VERSION,
            "version": env!("CARGO_PKG_VERSION"),
            "elapsed_ms": 0,
            "grouped_by": "owner",
            "total_issues": 1,
            "groups": [{
                "key": "@team",
                "total_issues": 1,
                "unused_files": [{"path": "src/dead.ts", "actions": []}],
                "unused_exports": "invalid"
            }]
        }))
        .expect_err("malformed current group must fail closed");

        assert!(error.contains("group 0 is incompatible with this Fallow version"));
    }

    #[test]
    fn is_fix_envelope_detects_kindless_fix_document() {
        let fix = serde_json::json!({
            "dry_run": false,
            "total_fixed": 3,
            "skipped": 0,
            "fixes": [{ "type": "remove_export", "applied": true }],
        });
        assert!(is_fix_envelope(&fix));
    }

    #[test]
    fn is_fix_envelope_rejects_other_kindless_documents() {
        // A dead-code envelope stripped of its `kind` must NOT masquerade as
        // fix: it has neither `fixes` nor `total_fixed`.
        assert!(!is_fix_envelope(&serde_json::json!({
            "total_issues": 4,
            "unused_files": [{ "path": "src/a.ts" }],
        })));
        // `fixes` alone (no `total_fixed`) is not enough.
        assert!(!is_fix_envelope(&serde_json::json!({ "fixes": [] })));
        // `total_fixed` alone (no `fixes` array) is not enough.
        assert!(!is_fix_envelope(&serde_json::json!({ "total_fixed": 0 })));
        // A `fixes` value that is not an array is rejected.
        assert!(!is_fix_envelope(&serde_json::json!({
            "fixes": "nope",
            "total_fixed": 0,
        })));
    }
}
