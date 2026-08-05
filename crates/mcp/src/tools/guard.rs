use crate::params::GuardParams;

use rmcp::ErrorData as McpError;
use rmcp::model::{CallToolResult, ContentBlock};

use super::{push_remote_extends, push_str_flag, run_tool, validation_error_body};

/// Run the read-only architecture guard report through the CLI.
pub async fn run_guard(binary: &str, params: GuardParams) -> Result<CallToolResult, McpError> {
    match build_guard_args(&params) {
        Ok(args) => run_tool(binary, "guard", &args).await,
        Err(msg) => Ok(CallToolResult::error(vec![ContentBlock::text(msg)])),
    }
}

/// Build CLI arguments for the `guard` tool.
pub fn build_guard_args(params: &GuardParams) -> Result<Vec<String>, String> {
    let mut args = vec![
        "guard".to_string(),
        "--format".to_string(),
        "json".to_string(),
        "--quiet".to_string(),
    ];

    push_str_flag(&mut args, "--root", params.root.as_deref());
    push_remote_extends(&mut args, params.allow_remote_extends);
    for file in &params.files {
        if file.trim().is_empty() {
            return Err(validation_error_body(format!(
                "files entries must not be empty (got '{file}')"
            )));
        }
        // Files are appended as positional argv; a leading '-' would let clap
        // consume the entry as a flag (e.g. --allow-remote-extends, --config).
        if file.starts_with('-') {
            return Err(validation_error_body(format!(
                "files entries must not start with '-' (got '{file}')"
            )));
        }
        args.push(file.clone());
    }

    Ok(args)
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
                build_guard_args(&params)
                    .expect("plain file entries are valid")
                    .contains(&"--allow-remote-extends".to_string()),
                expected
            );
        }
    }

    #[test]
    fn flag_like_file_entries_are_rejected_not_parsed_as_flags() {
        let params = GuardParams {
            files: vec!["--allow-remote-extends".to_string()],
            root: None,
            allow_remote_extends: None,
        };

        let msg = build_guard_args(&params).expect_err("flag-like entry must be rejected");
        assert!(msg.contains("must not start with '-'"));
        assert!(msg.contains("'--allow-remote-extends'"));
    }

    #[test]
    fn empty_file_entries_are_rejected() {
        let params = GuardParams {
            files: vec!["  ".to_string()],
            root: None,
            allow_remote_extends: None,
        };

        let msg = build_guard_args(&params).expect_err("empty entry must be rejected");
        assert!(msg.contains("must not be empty"));
        assert!(msg.contains("'  '"));
    }
}
