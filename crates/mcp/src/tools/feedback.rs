use rmcp::ErrorData as McpError;
use rmcp::model::{CallToolResult, ContentBlock};

use super::run_tool_with_limit;
use crate::params::SubmitFeedbackParams;

/// A submit needs the digest of a preview, so the issue holds the text that
/// the user saw.
const MISSING_CONFIRM: &str = r#"{"error":true,"message":"submit: true needs confirm: call submit_feedback without submit, show the preview to the user, then pass its feedback.digest as confirm","exit_code":2}"#;

/// Run `fallow feedback`. The CLI owns the report, the redaction and the
/// GitHub CLI call, so the tool stays a thin wrapper.
pub async fn run_submit_feedback(
    binary: &str,
    params: SubmitFeedbackParams,
) -> Result<CallToolResult, McpError> {
    if params.submit == Some(true) && params.confirm.is_none() {
        return Ok(CallToolResult::error(vec![ContentBlock::text(
            MISSING_CONFIRM.to_owned(),
        )]));
    }
    let args = build_submit_feedback_args(&params);
    run_tool_with_limit(binary, "submit_feedback", &args, None).await
}

/// Build the `fallow feedback` arguments for the tool parameters.
pub fn build_submit_feedback_args(params: &SubmitFeedbackParams) -> Vec<String> {
    let mut args = vec!["feedback".to_owned()];
    let fields = [
        ("--type", &params.r#type),
        ("--impact", &params.impact),
        ("--task", &params.task),
        ("--command", &params.command),
        ("--expected", &params.expected),
        ("--actual", &params.actual),
        ("--agent", &params.agent),
    ];
    // `--flag=value` keeps a value that starts with `-` a value.
    for (flag, value) in fields {
        if let Some(value) = value {
            args.push(format!("{flag}={value}"));
        }
    }
    if params.submit == Some(true) {
        args.push("--yes".to_owned());
        if let Some(confirm) = &params.confirm {
            args.push(format!("--confirm={confirm}"));
        }
    }
    if params.no_diagnostics == Some(true) {
        args.push("--no-diagnostics".to_owned());
    }
    args.extend(["--format", "json", "--quiet", "--"].map(str::to_owned));
    // After `--`, a message that starts with a dash stays a message.
    args.push(params.message.clone());
    args
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preview_is_the_default() {
        let args = build_submit_feedback_args(&SubmitFeedbackParams {
            message: "--looks like a flag".to_owned(),
            ..SubmitFeedbackParams::default()
        });
        assert_eq!(
            args,
            [
                "feedback",
                "--format",
                "json",
                "--quiet",
                "--",
                "--looks like a flag"
            ]
        );
    }

    #[tokio::test]
    async fn submit_without_confirm_runs_nothing() {
        let result = run_submit_feedback(
            "binary-that-must-not-run",
            SubmitFeedbackParams {
                message: "m".to_owned(),
                submit: Some(true),
                ..SubmitFeedbackParams::default()
            },
        )
        .await
        .unwrap();
        assert_eq!(result.is_error, Some(true));
    }

    #[test]
    fn submit_and_fields_map_to_flags() {
        let args = build_submit_feedback_args(&SubmitFeedbackParams {
            message: "m".to_owned(),
            r#type: Some("false-positive".to_owned()),
            impact: Some("worked-around".to_owned()),
            command: Some("fallow dead-code".to_owned()),
            actual: Some("-1 returned".to_owned()),
            agent: Some("claude-code".to_owned()),
            submit: Some(true),
            confirm: Some("abc".to_owned()),
            no_diagnostics: Some(true),
            ..SubmitFeedbackParams::default()
        });
        let joined = args.join(" ");
        assert!(joined.contains("--type=false-positive --impact=worked-around"));
        assert!(
            joined.contains("--command=fallow dead-code --actual=-1 returned --agent=claude-code")
        );
        assert!(joined.contains("--yes --confirm=abc --no-diagnostics --format json"));
    }
}
