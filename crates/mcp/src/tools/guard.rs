use crate::params::GuardParams;

use rmcp::ErrorData as McpError;
use rmcp::model::CallToolResult;

use super::{push_remote_extends, push_str_flag, run_tool};

/// Run the read-only architecture guard report through the CLI.
pub async fn run_guard(binary: &str, params: GuardParams) -> Result<CallToolResult, McpError> {
    let args = build_guard_args(&params);
    run_tool(binary, "guard", &args).await
}

/// Build CLI arguments for the `guard` tool.
pub fn build_guard_args(params: &GuardParams) -> Vec<String> {
    let mut args = vec![
        "guard".to_string(),
        "--format".to_string(),
        "json".to_string(),
        "--quiet".to_string(),
    ];

    push_str_flag(&mut args, "--root", params.root.as_deref());
    push_remote_extends(&mut args, params.allow_remote_extends);
    args.extend(params.files.iter().cloned());

    args
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn remote_extends_is_forwarded_only_when_explicitly_enabled() {
        for (value, expected) in [(Some(true), true), (Some(false), false), (None, false)] {
            let params = GuardParams {
                files: vec!["src/main.ts".to_string()],
                root: None,
                allow_remote_extends: value,
            };

            assert_eq!(
                build_guard_args(&params).contains(&"--allow-remote-extends".to_string()),
                expected
            );
        }
    }
}
