use crate::params::ExplainParams;

use fallow_api::{RootEnvelopeMode, serialize_explain_programmatic_json};
use rmcp::ErrorData as McpError;
use rmcp::model::CallToolResult;

use super::api_runtime::{json_success, programmatic_error_body};

/// Run the `fallow_explain` tool through the typed programmatic API.
pub async fn run_explain(_binary: &str, params: ExplainParams) -> Result<CallToolResult, McpError> {
    match serialize_explain_programmatic_json(&params.issue_type, RootEnvelopeMode::Tagged, None) {
        Ok(value) => Ok(json_success(&value)),
        Err(error) => Ok(CallToolResult::error(vec![rmcp::model::Content::text(
            programmatic_error_body(&error),
        )])),
    }
}

/// Build legacy CLI arguments for Code Mode compatibility and tests.
pub fn build_explain_args(params: &ExplainParams) -> Vec<String> {
    vec![
        "explain".to_string(),
        params.issue_type.clone(),
        "--format".to_string(),
        "json".to_string(),
        "--quiet".to_string(),
    ]
}

#[cfg(test)]
mod tests {
    use rmcp::model::RawContent;

    use super::*;

    #[tokio::test]
    async fn run_explain_uses_api_path_without_cli_binary() {
        let result = run_explain(
            "unused-binary-on-api-path",
            ExplainParams {
                issue_type: "unused-export".to_string(),
            },
        )
        .await
        .expect("mcp result");

        assert_eq!(result.is_error, Some(false));
        let [content] = result.content.as_slice() else {
            panic!("expected one content item");
        };
        let RawContent::Text(text) = &content.raw else {
            panic!("expected text content");
        };
        let json: serde_json::Value = serde_json::from_str(&text.text).expect("json");
        assert_eq!(json["kind"], "explain");
        assert_eq!(json["id"], "fallow/unused-export");
    }

    #[tokio::test]
    async fn run_explain_returns_structured_api_error_for_unknown_issue_type() {
        let result = run_explain(
            "unused-binary-on-api-path",
            ExplainParams {
                issue_type: "not-a-real-rule".to_string(),
            },
        )
        .await
        .expect("mcp result");

        assert_eq!(result.is_error, Some(true));
        let [content] = result.content.as_slice() else {
            panic!("expected one content item");
        };
        let RawContent::Text(text) = &content.raw else {
            panic!("expected text content");
        };
        let json: serde_json::Value = serde_json::from_str(&text.text).expect("json");
        assert_eq!(json["error"], true);
        assert_eq!(json["exit_code"], 2);
        assert_eq!(json["code"], "unknown_issue_type");
    }
}
