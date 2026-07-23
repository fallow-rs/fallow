use crate::params::SemanticSymbolParams;

use rmcp::ErrorData as McpError;
use rmcp::model::CallToolResult;

use super::{push_global, push_remote_extends, push_type_aware, run_tool};

fn require_non_empty(field: &str, value: &str) -> Result<(), String> {
    if value.trim().is_empty() {
        return Err(format!("{field} must not be empty"));
    }
    Ok(())
}

fn build_semantic_symbol_args(
    params: &SemanticSymbolParams,
    flag: &str,
) -> Result<Vec<String>, String> {
    require_non_empty("file", &params.file)?;
    require_non_empty("export_name", &params.export_name)?;

    let mut args = vec![
        "dead-code".to_string(),
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
    push_type_aware(
        &mut args,
        Some(true),
        params.type_aware_projects.as_deref(),
        params.type_aware_require,
    );
    args.extend([
        flag.to_string(),
        format!("{}:{}", params.file, params.export_name),
    ]);
    Ok(args)
}

pub async fn run_symbol_trace(
    binary: &str,
    params: SemanticSymbolParams,
) -> Result<CallToolResult, McpError> {
    match build_semantic_symbol_args(&params, "--trace") {
        Ok(args) => run_tool(binary, "symbol_trace", &args).await,
        Err(error) => Ok(CallToolResult::error(vec![
            rmcp::model::ContentBlock::text(error),
        ])),
    }
}

pub async fn run_symbol_impact(
    binary: &str,
    params: SemanticSymbolParams,
) -> Result<CallToolResult, McpError> {
    match build_semantic_symbol_args(&params, "--symbol-impact") {
        Ok(args) => run_tool(binary, "symbol_impact", &args).await,
        Err(error) => Ok(CallToolResult::error(vec![
            rmcp::model::ContentBlock::text(error),
        ])),
    }
}

#[cfg(test)]
mod tests {
    use super::build_semantic_symbol_args;
    use crate::params::SemanticSymbolParams;

    #[test]
    fn semantic_symbol_args_enable_type_aware_mode() {
        let params = SemanticSymbolParams {
            file: "src/api.ts".to_string(),
            export_name: "load".to_string(),
            root: Some("/project".to_string()),
            config: None,
            allow_remote_extends: None,
            type_aware_projects: Some(vec!["tsconfig.json".to_string()]),
            type_aware_require: None,
            no_cache: None,
            threads: None,
        };
        let args = build_semantic_symbol_args(&params, "--symbol-impact")
            .expect("valid semantic symbol args");

        assert!(args.iter().any(|arg| arg == "--type-aware"));
        assert!(args.iter().any(|arg| arg == "--type-aware-project"));
        assert!(args.iter().any(|arg| arg == "--symbol-impact"));
    }
}
