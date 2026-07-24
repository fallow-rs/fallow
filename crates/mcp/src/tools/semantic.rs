use crate::params::{SemanticImpactParams, SemanticImpactSelector, SemanticSymbolParams};

use rmcp::ErrorData as McpError;
use rmcp::model::CallToolResult;

use super::{push_global, push_remote_extends, push_type_aware, run_tool, validation_error_body};

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
        "--explain".to_string(),
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
        Ok(args) => run_tool(binary, "trace_symbol", &args).await,
        Err(error) => Ok(CallToolResult::error(vec![
            rmcp::model::ContentBlock::text(validation_error_body(error)),
        ])),
    }
}

pub async fn run_symbol_impact(
    binary: &str,
    params: SemanticImpactParams,
) -> Result<CallToolResult, McpError> {
    match build_symbol_impact_args(&params) {
        Ok(args) => run_tool(binary, "symbol_impact", &args).await,
        Err(error) => Ok(CallToolResult::error(vec![
            rmcp::model::ContentBlock::text(validation_error_body(error)),
        ])),
    }
}

fn build_symbol_impact_args(params: &SemanticImpactParams) -> Result<Vec<String>, String> {
    require_non_empty("file", &params.file)?;
    let target = match &params.target {
        SemanticImpactSelector::Export(selector) => {
            require_non_empty("export_name", &selector.export_name)?;
            selector.export_name.clone()
        }
        SemanticImpactSelector::ClassMethod(selector) => {
            require_non_empty("class_name", &selector.class_name)?;
            require_non_empty("member_name", &selector.member_name)?;
            if selector.class_name.contains('.') || selector.member_name.contains('.') {
                return Err(
                    "class_name and member_name must not contain the '.' selector delimiter"
                        .to_string(),
                );
            }
            format!("{}.{}", selector.class_name, selector.member_name)
        }
    };
    let mut args = vec![
        "dead-code".to_string(),
        "--format".to_string(),
        "json".to_string(),
        "--quiet".to_string(),
        "--explain".to_string(),
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
        "--symbol-impact".to_string(),
        format!("{}:{target}", params.file),
    ]);
    Ok(args)
}

#[cfg(test)]
mod tests {
    use super::{build_semantic_symbol_args, build_symbol_impact_args, run_symbol_trace};
    use crate::params::{
        SemanticClassMethodSelector, SemanticExportSelector, SemanticImpactParams,
        SemanticImpactSelector, SemanticSymbolParams,
    };

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
        assert!(args.iter().any(|arg| arg == "--explain"));
    }

    #[tokio::test]
    async fn semantic_symbol_validation_errors_are_structured_json() {
        let result = run_symbol_trace(
            "unused-test-binary",
            SemanticSymbolParams {
                file: " ".to_string(),
                export_name: String::new(),
                root: None,
                config: None,
                allow_remote_extends: None,
                type_aware_projects: None,
                type_aware_require: None,
                no_cache: None,
                threads: None,
            },
        )
        .await
        .expect("validation stays a tool result");

        assert_eq!(result.is_error, Some(true));
        let text = match &result.content[0] {
            rmcp::model::ContentBlock::Text(text) => &text.text,
            _ => panic!("expected text content"),
        };
        let body: serde_json::Value =
            serde_json::from_str(text).expect("structured validation error");
        assert_eq!(body["error"], true);
        assert_eq!(body["exit_code"], 2);
        assert_eq!(body["message"], "file must not be empty");
    }

    #[test]
    fn class_method_impact_args_use_exact_selector() {
        let params = SemanticImpactParams {
            file: "src/repository.ts".to_string(),
            target: SemanticImpactSelector::ClassMethod(SemanticClassMethodSelector {
                class_name: "UserRepository".to_string(),
                member_name: "save".to_string(),
            }),
            root: Some("/project".to_string()),
            config: None,
            allow_remote_extends: None,
            type_aware_projects: Some(vec!["tsconfig.json".to_string()]),
            type_aware_require: None,
            no_cache: None,
            threads: None,
        };
        let args = build_symbol_impact_args(&params).expect("valid class method selector");
        assert!(
            args.iter()
                .any(|arg| arg == "src/repository.ts:UserRepository.save")
        );
    }

    #[test]
    fn export_impact_args_remain_compatible() {
        let params = SemanticImpactParams {
            file: "src/api.ts".to_string(),
            target: SemanticImpactSelector::Export(SemanticExportSelector {
                export_name: "load".to_string(),
            }),
            root: None,
            config: None,
            allow_remote_extends: None,
            type_aware_projects: None,
            type_aware_require: None,
            no_cache: None,
            threads: None,
        };
        let args = build_symbol_impact_args(&params).expect("valid export selector");
        assert!(args.iter().any(|arg| arg == "src/api.ts:load"));
    }

    #[test]
    fn class_method_impact_rejects_ambiguous_selector_delimiters() {
        let params = SemanticImpactParams {
            file: "src/repository.ts".to_string(),
            target: SemanticImpactSelector::ClassMethod(SemanticClassMethodSelector {
                class_name: "Nested.Repository".to_string(),
                member_name: "save".to_string(),
            }),
            root: None,
            config: None,
            allow_remote_extends: None,
            type_aware_projects: None,
            type_aware_require: None,
            no_cache: None,
            threads: None,
        };

        assert!(build_symbol_impact_args(&params).is_err());
    }
}
