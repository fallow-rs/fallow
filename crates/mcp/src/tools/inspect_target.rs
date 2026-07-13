use rmcp::ErrorData as McpError;
use rmcp::model::{CallToolResult, ContentBlock};

use crate::params::{InspectTarget, InspectTargetParams};

use super::{push_global, push_remote_extends, push_scope, run_tool, validation_error_body};

const TOOL: &str = "inspect_target";

/// Run the composed `inspect_target` MCP tool through the shared CLI inspect flow.
pub async fn inspect_target(
    binary: &str,
    params: &InspectTargetParams,
) -> Result<CallToolResult, McpError> {
    match build_inspect_args(params) {
        Ok(args) => run_tool(binary, TOOL, &args).await,
        Err(message) => Ok(CallToolResult::error(vec![ContentBlock::text(
            validation_error_body(message),
        )])),
    }
}

fn build_inspect_args(params: &InspectTargetParams) -> Result<Vec<String>, String> {
    let mut args = vec![
        "inspect".to_string(),
        "--format".to_string(),
        "json".to_string(),
        "--quiet".to_string(),
    ];
    push_global(
        &mut args,
        params.root.as_deref(),
        params.config.as_deref(),
        params.no_cache,
        params.threads,
    );
    push_remote_extends(&mut args, params.allow_remote_extends);
    if params.production == Some(false) {
        args.push("--no-production".to_string());
        push_scope(&mut args, None, params.workspace.as_deref());
    } else {
        push_scope(&mut args, params.production, params.workspace.as_deref());
    }
    if params.include_churn == Some(true) {
        args.push("--churn".to_string());
    }

    match &params.target {
        InspectTarget::File { file } => {
            require_non_empty("target.file", file)?;
            args.extend(["--file".to_string(), file.clone()]);
        }
        InspectTarget::Symbol { file, export_name } => {
            require_non_empty("target.file", file)?;
            require_non_empty("target.export_name", export_name)?;
            args.extend(["--symbol".to_string(), format!("{file}:{export_name}")]);
            // OPT-IN: the symbol-level call chain is only meaningful for a
            // symbol target, and only attached when explicitly requested.
            if params.symbol_chain == Some(true) {
                args.push("--symbol-chain".to_string());
            }
        }
    }

    Ok(args)
}

fn require_non_empty(field: &str, value: &str) -> Result<(), String> {
    if value.trim().is_empty() {
        return Err(format!("{field} must not be empty"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn production_false_forces_inspect_out_of_production_mode() {
        let params = InspectTargetParams {
            root: None,
            config: None,
            allow_remote_extends: None,
            no_cache: None,
            threads: None,
            production: Some(false),
            workspace: Some("pkg-a".to_string()),
            target: InspectTarget::File {
                file: "src/api.ts".to_string(),
            },
            symbol_chain: None,
            include_churn: None,
        };

        let args = build_inspect_args(&params).unwrap();

        assert!(args.contains(&"--no-production".to_string()));
        assert!(args.windows(2).any(|pair| pair == ["--workspace", "pkg-a"]));
        assert!(!args.contains(&"--production".to_string()));
    }

    #[test]
    fn symbol_chain_opt_in_forwards_flag_only_for_symbol_target() {
        let params = InspectTargetParams {
            root: None,
            config: None,
            allow_remote_extends: None,
            no_cache: None,
            threads: None,
            production: None,
            workspace: None,
            target: InspectTarget::Symbol {
                file: "src/api.ts".to_string(),
                export_name: "handler".to_string(),
            },
            symbol_chain: Some(true),
            include_churn: None,
        };

        let args = build_inspect_args(&params).unwrap();
        assert!(args.contains(&"--symbol-chain".to_string()));
        assert!(
            args.windows(2)
                .any(|pair| pair == ["--symbol", "src/api.ts:handler"])
        );
    }

    #[test]
    fn symbol_chain_default_off_omits_flag() {
        let params = InspectTargetParams {
            root: None,
            config: None,
            allow_remote_extends: None,
            no_cache: None,
            threads: None,
            production: None,
            workspace: None,
            target: InspectTarget::Symbol {
                file: "src/api.ts".to_string(),
                export_name: "handler".to_string(),
            },
            symbol_chain: None,
            include_churn: None,
        };

        let args = build_inspect_args(&params).unwrap();
        assert!(!args.contains(&"--symbol-chain".to_string()));
    }

    #[test]
    fn symbol_chain_ignored_for_file_target() {
        let params = InspectTargetParams {
            root: None,
            config: None,
            allow_remote_extends: None,
            no_cache: None,
            threads: None,
            production: None,
            workspace: None,
            target: InspectTarget::File {
                file: "src/api.ts".to_string(),
            },
            symbol_chain: Some(true),
            include_churn: None,
        };

        let args = build_inspect_args(&params).unwrap();
        // A file target carries no symbol, so the chain flag is never forwarded.
        assert!(!args.contains(&"--symbol-chain".to_string()));
    }

    #[test]
    fn churn_opt_in_forwards_flag_for_any_target() {
        let params = InspectTargetParams {
            root: None,
            config: None,
            allow_remote_extends: None,
            no_cache: None,
            threads: None,
            production: None,
            workspace: None,
            target: InspectTarget::File {
                file: "src/api.ts".to_string(),
            },
            symbol_chain: None,
            include_churn: Some(true),
        };

        let args = build_inspect_args(&params).unwrap();

        assert!(args.contains(&"--churn".to_string()));
    }
}
