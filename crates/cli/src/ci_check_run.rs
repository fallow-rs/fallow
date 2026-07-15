use std::path::Path;
use std::process::ExitCode;

use fallow_config::OutputFormat;
use fallow_output::{PrDecisionAnnotation, PrDecisionAnnotationLevel, PrDecisionConclusion};
use serde_json::{Value, json};

use crate::error::emit_error_with_style;

use super::{emit_ci_command_json, github_post_json, github_token, read_text_file};

#[derive(Clone, Copy)]
pub(super) struct PostCheckRunInput<'a> {
    pub decision: &'a Path,
    pub repo: &'a str,
    pub head_sha: &'a str,
    pub api_url: Option<&'a str>,
    pub split_gates: bool,
    pub dry_run: bool,
}

pub(super) fn post_check_run(
    input: &PostCheckRunInput<'_>,
    output: OutputFormat,
    json_style: crate::json_style::JsonStyle,
) -> ExitCode {
    let decision = match read_decision(input.decision) {
        Ok(decision) => decision,
        Err(e) => return emit_error_with_style(&e, 2, output, json_style),
    };
    let payloads = github_check_run_payloads(&decision, input.head_sha, input.split_gates);
    if input.dry_run {
        let value = if input.split_gates {
            Value::Array(payloads)
        } else {
            payloads.into_iter().next().unwrap_or_else(|| json!({}))
        };
        return emit_ci_command_json(&value, "check run payload", output, json_style);
    }
    let token = match github_token() {
        Ok(token) => token,
        Err(e) => {
            return emit_error_with_style(&e, crate::api::NETWORK_EXIT_CODE, output, json_style);
        }
    };
    let agent = match crate::api::try_api_agent() {
        Ok(agent) => agent,
        Err(e) => {
            return emit_error_with_style(
                &e.to_string(),
                crate::api::NETWORK_EXIT_CODE,
                output,
                json_style,
            );
        }
    };
    let api = input
        .api_url
        .unwrap_or("https://api.github.com")
        .trim_end_matches('/');
    let url = format!("{api}/repos/{}/check-runs", input.repo);
    let mut results = Vec::new();
    for payload in &payloads {
        match github_post_json(&agent, &url, &token, payload) {
            Ok(value) => results.push(value),
            Err(e) => {
                return emit_error_with_style(
                    &e,
                    crate::api::NETWORK_EXIT_CODE,
                    output,
                    json_style,
                );
            }
        }
    }
    emit_ci_command_json(
        &Value::Array(results),
        "check run result",
        output,
        json_style,
    )
}

fn read_decision(path: &Path) -> Result<fallow_output::PrDecisionSurface, String> {
    let text = read_text_file(path, "PR decision surface")?;
    serde_json::from_str(&text).map_err(|e| {
        format!(
            "failed to parse PR decision surface '{}': {e}",
            path.display()
        )
    })
}

fn github_check_run_payload(decision: &fallow_output::PrDecisionSurface, head_sha: &str) -> Value {
    let annotations = decision
        .annotations
        .iter()
        .take(50)
        .map(github_annotation)
        .collect::<Vec<_>>();
    github_check_run_payload_with_output(
        "Fallow",
        head_sha,
        decision.conclusion,
        &decision.title,
        &decision.details.summary_markdown,
        &annotations,
    )
}

fn github_check_run_payloads(
    decision: &fallow_output::PrDecisionSurface,
    head_sha: &str,
    split_gates: bool,
) -> Vec<Value> {
    if !split_gates {
        return vec![github_check_run_payload(decision, head_sha)];
    }
    let contexts = fallow_output::pr_status_contexts_with_mode(
        decision,
        status_mode_from_env(input_split_mode(split_gates)),
    );
    if contexts.is_empty() {
        return vec![github_check_run_payload(decision, head_sha)];
    }
    contexts
        .into_iter()
        .map(|context| {
            github_check_run_payload_with_output(
                &context.name,
                head_sha,
                context.conclusion,
                &context.name,
                &context.summary,
                &[],
            )
        })
        .collect()
}

fn input_split_mode(split_gates: bool) -> fallow_output::PrStatusMode {
    if split_gates {
        fallow_output::PrStatusMode::Split
    } else {
        fallow_output::PrStatusMode::Aggregate
    }
}

fn status_mode_from_env(default: fallow_output::PrStatusMode) -> fallow_output::PrStatusMode {
    if default != fallow_output::PrStatusMode::Split {
        return default;
    }
    match std::env::var("FALLOW_CONSOLIDATED_STATUS").as_deref() {
        Ok("1" | "true" | "yes" | "on") => fallow_output::PrStatusMode::AggregateAndSplit,
        _ => default,
    }
}

fn github_check_run_payload_with_output(
    name: &str,
    head_sha: &str,
    conclusion: PrDecisionConclusion,
    title: &str,
    summary: &str,
    annotations: &[Value],
) -> Value {
    json!({
        "name": name,
        "head_sha": head_sha,
        "status": "completed",
        "conclusion": github_conclusion(conclusion),
        "output": {
            "title": title,
            "summary": summary,
            "annotations": annotations
        }
    })
}

fn github_conclusion(conclusion: PrDecisionConclusion) -> &'static str {
    match conclusion {
        PrDecisionConclusion::Success => "success",
        PrDecisionConclusion::Failure => "failure",
        PrDecisionConclusion::Neutral => "neutral",
        PrDecisionConclusion::Skipped => "skipped",
    }
}

fn github_annotation(annotation: &PrDecisionAnnotation) -> Value {
    let mut value = json!({
        "path": annotation.path,
        "start_line": annotation.line.max(1),
        "end_line": annotation.line.max(1),
        "annotation_level": github_annotation_level(annotation.level),
        "message": annotation.message,
        "title": annotation.title,
    });
    if let Some(raw_details) = annotation.raw_details.as_ref()
        && let Some(object) = value.as_object_mut()
    {
        object.insert("raw_details".to_owned(), json!(raw_details));
    }
    value
}

fn github_annotation_level(level: PrDecisionAnnotationLevel) -> &'static str {
    match level {
        PrDecisionAnnotationLevel::Notice => "notice",
        PrDecisionAnnotationLevel::Warning => "warning",
        PrDecisionAnnotationLevel::Failure => "failure",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn github_payload_maps_decision_to_check_run() {
        let decision = fallow_output::PrDecisionSurface {
            schema: fallow_output::PR_DECISION_SCHEMA.to_owned(),
            title: "Fallow".to_owned(),
            conclusion: PrDecisionConclusion::Failure,
            gates: vec![],
            annotations: vec![PrDecisionAnnotation {
                path: "src/app.ts".to_owned(),
                line: 12,
                level: PrDecisionAnnotationLevel::Warning,
                title: "fallow/high-crap-score".to_owned(),
                message: "Function is hard to safely change.".to_owned(),
                raw_details: Some("Extract smaller units.".to_owned()),
            }],
            details: fallow_output::PrDecisionDetails {
                summary_markdown: "Quality gate failed.".to_owned(),
                full_report_path: None,
                details_url: None,
            },
        };

        let payload = github_check_run_payload(&decision, "abc123");

        assert_eq!(payload["name"], "Fallow");
        assert_eq!(payload["head_sha"], "abc123");
        assert_eq!(payload["conclusion"], "failure");
        assert_eq!(payload["output"]["summary"], "Quality gate failed.");
        assert_eq!(
            payload["output"]["annotations"][0]["annotation_level"],
            "warning"
        );
    }

    #[test]
    fn github_annotation_uses_line_one_for_zero_line_findings() {
        let annotation = PrDecisionAnnotation {
            path: "package.json".to_owned(),
            line: 0,
            level: PrDecisionAnnotationLevel::Notice,
            title: "fallow/unused-dependency".to_owned(),
            message: "Dependency appears unused.".to_owned(),
            raw_details: None,
        };

        let value = github_annotation(&annotation);

        assert_eq!(value["start_line"], 1);
        assert!(value.get("raw_details").is_none());
    }

    #[test]
    fn split_gate_payloads_use_gate_names_without_annotations() {
        let decision = fallow_output::PrDecisionSurface {
            schema: fallow_output::PR_DECISION_SCHEMA.to_owned(),
            title: "Fallow".to_owned(),
            conclusion: PrDecisionConclusion::Neutral,
            gates: vec![fallow_output::PrDecisionGate {
                id: "health".to_owned(),
                label: "Health".to_owned(),
                status: PrDecisionConclusion::Neutral,
                observed: "1 finding".to_owned(),
                threshold: Some("configured complexity gates".to_owned()),
                scope: "new code".to_owned(),
            }],
            annotations: vec![PrDecisionAnnotation {
                path: "src/app.ts".to_owned(),
                line: 12,
                level: PrDecisionAnnotationLevel::Warning,
                title: "fallow/high-crap-score".to_owned(),
                message: "Function is hard to safely change.".to_owned(),
                raw_details: None,
            }],
            details: fallow_output::PrDecisionDetails {
                summary_markdown: "Review recommended.".to_owned(),
                full_report_path: None,
                details_url: None,
            },
        };

        let payloads = github_check_run_payloads(&decision, "abc123", true);

        assert_eq!(payloads.len(), 1);
        assert_eq!(payloads[0]["name"], "Fallow / health");
        assert_eq!(payloads[0]["conclusion"], "neutral");
        assert_eq!(
            payloads[0]["output"]["annotations"]
                .as_array()
                .expect("annotations array")
                .len(),
            0
        );
    }
}
