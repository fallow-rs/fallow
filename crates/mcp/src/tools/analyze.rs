use crate::params::AnalyzeParams;

use std::path::PathBuf;

use fallow_api::{
    AnalysisOptions, DeadCodeFilters, DeadCodeOptions, run_boundary_violations,
    run_circular_dependencies, run_dead_code, serialize_boundary_violations_programmatic_json,
    serialize_circular_dependencies_programmatic_json, serialize_dead_code_programmatic_json,
};
use rmcp::ErrorData as McpError;
use rmcp::model::{CallToolResult, Content};

use super::{
    ISSUE_TYPE_FLAGS,
    api_runtime::{env_diff_file, run_api_blocking},
    api_runtime::{json_success, non_empty_path, non_empty_string, programmatic_error_body},
    fallback_policy::{
        CliFallbackReason, baseline_fallback_reason, grouped_fallback_reason,
        regression_fallback_reason,
    },
    push_baseline, push_global, push_regression, push_scope, run_tool, validation_error_body,
};

/// Run `analyze` through the typed API when parameters map cleanly to the
/// programmatic contract, falling back to the CLI for CLI-only surfaces.
pub async fn run_analyze(binary: &str, params: AnalyzeParams) -> Result<CallToolResult, McpError> {
    if requires_cli_fallback(&params) {
        return match build_analyze_args(&params) {
            Ok(args) => run_tool(binary, "analyze", &args).await,
            Err(msg) => Ok(CallToolResult::error(vec![Content::text(msg)])),
        };
    }

    let family = analyze_family(&params);
    let options = match dead_code_options_from_params(&params) {
        Ok(options) => options,
        Err(msg) => return Ok(CallToolResult::error(vec![Content::text(msg)])),
    };

    let result = run_api_blocking("analyze", move || match family {
        AnalyzeFamily::Boundary => run_boundary_violations(&options)
            .and_then(serialize_boundary_violations_programmatic_json),
        AnalyzeFamily::Circular => run_circular_dependencies(&options)
            .and_then(serialize_circular_dependencies_programmatic_json),
        AnalyzeFamily::DeadCode => {
            run_dead_code(&options).and_then(serialize_dead_code_programmatic_json)
        }
    })
    .await?
    .map_or_else(
        |err| CallToolResult::error(vec![Content::text(programmatic_error_body(&err))]),
        |value| json_success(&value),
    );
    Ok(result)
}

pub fn run_analyze_api_value(params: &AnalyzeParams) -> Result<Option<serde_json::Value>, String> {
    if requires_cli_fallback(params) {
        return Ok(None);
    }

    let family = analyze_family(params);
    let options = dead_code_options_from_params(params)?;
    let value = match family {
        AnalyzeFamily::Boundary => run_boundary_violations(&options)
            .and_then(serialize_boundary_violations_programmatic_json),
        AnalyzeFamily::Circular => run_circular_dependencies(&options)
            .and_then(serialize_circular_dependencies_programmatic_json),
        AnalyzeFamily::DeadCode => {
            run_dead_code(&options).and_then(serialize_dead_code_programmatic_json)
        }
    }
    .map_err(|err| programmatic_error_body(&err))?;

    Ok(Some(value))
}

/// Build CLI arguments for the `analyze` tool.
/// Returns `Err(message)` if an invalid issue type is provided.
pub fn build_analyze_args(params: &AnalyzeParams) -> Result<Vec<String>, String> {
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
    push_scope(&mut args, params.production, params.workspace.as_deref());

    push_analyze_issue_type_flags(&mut args, params)?;
    push_baseline(
        &mut args,
        params.baseline.as_deref(),
        params.save_baseline.as_deref(),
    );
    push_regression(
        &mut args,
        params.fail_on_regression,
        params.tolerance.as_deref(),
        params.regression_baseline.as_deref(),
        params.save_regression_baseline.as_deref(),
    );
    if let Some(ref gb) = params.group_by {
        args.extend(["--group-by".to_string(), gb.clone()]);
    }
    if let Some(ref files) = params.file {
        for f in files {
            args.extend(["--file".to_string(), f.clone()]);
        }
    }
    if params.include_entry_exports == Some(true) {
        args.push("--include-entry-exports".to_string());
    }

    Ok(args)
}

fn requires_cli_fallback(params: &AnalyzeParams) -> bool {
    cli_fallback_reason(params).is_some()
}

fn cli_fallback_reason(params: &AnalyzeParams) -> Option<CliFallbackReason> {
    baseline_fallback_reason(params.baseline.as_deref(), params.save_baseline.as_deref())
        .or_else(|| {
            regression_fallback_reason(
                params.fail_on_regression,
                params.tolerance.as_deref(),
                params.regression_baseline.as_deref(),
                params.save_regression_baseline.as_deref(),
            )
        })
        .or_else(|| grouped_fallback_reason(params.group_by.as_deref()))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum AnalyzeFamily {
    DeadCode,
    Circular,
    Boundary,
}

fn analyze_family(params: &AnalyzeParams) -> AnalyzeFamily {
    if params.boundary_violations == Some(true)
        && params
            .issue_types
            .as_ref()
            .is_none_or(|types| types.iter().all(|issue| issue == "boundary-violations"))
    {
        return AnalyzeFamily::Boundary;
    }
    if params.issue_types.as_ref().is_some_and(|types| {
        !types.is_empty()
            && types
                .iter()
                .all(|issue| matches!(issue.as_str(), "circular-deps" | "re-export-cycles"))
    }) {
        return AnalyzeFamily::Circular;
    }
    AnalyzeFamily::DeadCode
}

fn dead_code_options_from_params(params: &AnalyzeParams) -> Result<DeadCodeOptions, String> {
    Ok(DeadCodeOptions {
        analysis: AnalysisOptions {
            root: non_empty_path(params.root.as_deref()),
            config_path: non_empty_path(params.config.as_deref()),
            no_cache: params.no_cache.unwrap_or(false),
            threads: params.threads,
            production: params.production.unwrap_or(false),
            production_override: params.production,
            diff_file: env_diff_file(),
            workspace: non_empty_string(params.workspace.as_deref())
                .map(|workspace| vec![workspace]),
            explain: true,
            ..AnalysisOptions::default()
        },
        filters: filters_from_params(params)?,
        files: params
            .file
            .as_deref()
            .unwrap_or_default()
            .iter()
            .filter(|path| !path.is_empty())
            .map(PathBuf::from)
            .collect(),
        include_entry_exports: params.include_entry_exports.unwrap_or(false),
    })
}

fn filters_from_params(params: &AnalyzeParams) -> Result<DeadCodeFilters, String> {
    let mut filters = DeadCodeFilters::default();
    if params.boundary_violations == Some(true) {
        filters.boundary_violations = true;
    }
    let Some(issue_types) = params.issue_types.as_ref() else {
        return Ok(filters);
    };
    for issue_type in issue_types {
        apply_issue_type_filter(&mut filters, issue_type)?;
    }
    Ok(filters)
}

fn apply_issue_type_filter(filters: &mut DeadCodeFilters, issue_type: &str) -> Result<(), String> {
    match issue_type {
        "unused-files" => filters.unused_files = true,
        "unused-exports" => filters.unused_exports = true,
        "unused-types" => filters.unused_types = true,
        "private-type-leaks" => filters.private_type_leaks = true,
        "unused-deps" => filters.unused_deps = true,
        "unused-enum-members" => filters.unused_enum_members = true,
        "unused-class-members" => filters.unused_class_members = true,
        "unused-store-members" => filters.unused_store_members = true,
        "unprovided-injects" => filters.unprovided_injects = true,
        "unrendered-components" => filters.unrendered_components = true,
        "unused-component-props" => filters.unused_component_props = true,
        "unused-component-emits" => filters.unused_component_emits = true,
        "unused-component-inputs" => filters.unused_component_inputs = true,
        "unused-component-outputs" => filters.unused_component_outputs = true,
        "unused-svelte-events" => filters.unused_svelte_events = true,
        "unused-server-actions" => filters.unused_server_actions = true,
        "unused-load-data-keys" => filters.unused_load_data_keys = true,
        "unresolved-imports" => filters.unresolved_imports = true,
        "unlisted-deps" => filters.unlisted_deps = true,
        "duplicate-exports" => filters.duplicate_exports = true,
        "circular-deps" => filters.circular_deps = true,
        "re-export-cycles" => filters.re_export_cycles = true,
        "boundary-violations" => filters.boundary_violations = true,
        "policy-violations" => filters.policy_violations = true,
        "stale-suppressions" => filters.stale_suppressions = true,
        "unused-catalog-entries" => filters.unused_catalog_entries = true,
        "empty-catalog-groups" => filters.empty_catalog_groups = true,
        "unresolved-catalog-references" => filters.unresolved_catalog_references = true,
        "unused-dependency-overrides" => filters.unused_dependency_overrides = true,
        "misconfigured-dependency-overrides" => filters.misconfigured_dependency_overrides = true,
        unknown => return Err(unknown_issue_type_error(unknown)),
    }
    Ok(())
}

fn unknown_issue_type_error(issue_type: &str) -> String {
    let valid = ISSUE_TYPE_FLAGS
        .iter()
        .map(|&(name, _)| name)
        .collect::<Vec<_>>()
        .join(", ");
    validation_error_body(format!(
        "Unknown issue type '{issue_type}'. Valid values: {valid}"
    ))
}

/// Push the `--boundary-violations` convenience flag and validated
/// per-issue-type flags for the `analyze` tool.
fn push_analyze_issue_type_flags(
    args: &mut Vec<String>,
    params: &AnalyzeParams,
) -> Result<(), String> {
    let types_has_boundaries = params
        .issue_types
        .as_ref()
        .is_some_and(|types| types.iter().any(|t| t == "boundary-violations"));
    if params.boundary_violations == Some(true) && !types_has_boundaries {
        args.push("--boundary-violations".to_string());
    }
    let Some(ref types) = params.issue_types else {
        return Ok(());
    };
    for t in types {
        if let Some(&(_, flag)) = ISSUE_TYPE_FLAGS.iter().find(|&&(name, _)| name == t) {
            args.push(flag.to_string());
        } else {
            return Err(unknown_issue_type_error(t));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use rmcp::model::RawContent;

    use super::*;

    #[test]
    fn api_path_maps_supported_analyze_params() {
        let params = AnalyzeParams {
            root: Some(String::new()),
            config: Some(String::new()),
            production: Some(true),
            workspace: Some("apps/web".to_string()),
            issue_types: Some(vec![
                "unused-exports".to_string(),
                "circular-deps".to_string(),
            ]),
            file: Some(vec!["src/index.ts".to_string()]),
            include_entry_exports: Some(true),
            no_cache: Some(true),
            threads: Some(2),
            ..AnalyzeParams::default()
        };

        assert!(!requires_cli_fallback(&params));
        let options = dead_code_options_from_params(&params).expect("options");
        assert!(options.analysis.root.is_none());
        assert!(options.analysis.config_path.is_none());
        assert_eq!(
            options.analysis.workspace,
            Some(vec!["apps/web".to_string()])
        );
        assert!(options.analysis.production);
        assert_eq!(options.analysis.production_override, Some(true));
        assert!(options.analysis.no_cache);
        assert_eq!(options.analysis.threads, Some(2));
        assert!(options.filters.unused_exports);
        assert!(options.filters.circular_deps);
        assert_eq!(options.files, vec![PathBuf::from("src/index.ts")]);
        assert!(options.include_entry_exports);
    }

    #[test]
    fn analyze_family_uses_typed_family_runners_for_narrow_requests() {
        assert_eq!(
            analyze_family(&AnalyzeParams {
                issue_types: Some(vec!["circular-deps".to_string()]),
                ..AnalyzeParams::default()
            }),
            AnalyzeFamily::Circular
        );
        assert_eq!(
            analyze_family(&AnalyzeParams {
                issue_types: Some(vec!["re-export-cycles".to_string()]),
                ..AnalyzeParams::default()
            }),
            AnalyzeFamily::Circular
        );
        assert_eq!(
            analyze_family(&AnalyzeParams {
                boundary_violations: Some(true),
                ..AnalyzeParams::default()
            }),
            AnalyzeFamily::Boundary
        );
        assert_eq!(
            analyze_family(&AnalyzeParams {
                issue_types: Some(vec![
                    "unused-exports".to_string(),
                    "circular-deps".to_string(),
                ]),
                ..AnalyzeParams::default()
            }),
            AnalyzeFamily::DeadCode
        );
    }

    #[test]
    fn api_path_reuses_cli_validation_for_bad_issue_type() {
        let params = AnalyzeParams {
            issue_types: Some(vec!["not-real".to_string()]),
            ..AnalyzeParams::default()
        };

        let err = dead_code_options_from_params(&params).expect_err("invalid issue type");
        assert!(err.contains("Unknown issue type"));
    }

    #[test]
    fn cli_fallback_keeps_cli_only_analyze_surfaces() {
        for params in [
            AnalyzeParams {
                baseline: Some("baseline.json".to_string()),
                ..AnalyzeParams::default()
            },
            AnalyzeParams {
                save_baseline: Some("baseline.json".to_string()),
                ..AnalyzeParams::default()
            },
            AnalyzeParams {
                fail_on_regression: Some(true),
                ..AnalyzeParams::default()
            },
            AnalyzeParams {
                tolerance: Some("2%".to_string()),
                ..AnalyzeParams::default()
            },
            AnalyzeParams {
                regression_baseline: Some("regression.json".to_string()),
                ..AnalyzeParams::default()
            },
            AnalyzeParams {
                save_regression_baseline: Some("regression.json".to_string()),
                ..AnalyzeParams::default()
            },
            AnalyzeParams {
                group_by: Some("owner".to_string()),
                ..AnalyzeParams::default()
            },
        ] {
            assert!(requires_cli_fallback(&params));
        }
    }

    #[tokio::test]
    async fn run_analyze_api_path_returns_json_without_cli_binary() {
        let project = tempfile::tempdir().expect("project");
        std::fs::write(
            project.path().join("package.json"),
            r#"{"name":"fixture","type":"module","main":"src/index.ts"}"#,
        )
        .expect("write package");
        std::fs::create_dir(project.path().join("src")).expect("create src");
        std::fs::write(
            project.path().join("src/index.ts"),
            "export const used = 1;\nexport const dead = 2;\nconsole.log(used);\n",
        )
        .expect("write source");

        let result = run_analyze(
            "unused-binary-on-api-path",
            AnalyzeParams {
                root: Some(project.path().display().to_string()),
                issue_types: Some(vec!["unused-exports".to_string()]),
                no_cache: Some(true),
                ..AnalyzeParams::default()
            },
        )
        .await
        .expect("api result");

        assert_eq!(result.is_error, Some(false));
        let text = match &result.content[0].raw {
            RawContent::Text(text) => &text.text,
            _ => panic!("expected text content"),
        };
        let json: serde_json::Value = serde_json::from_str(text).expect("json");
        assert_eq!(json["kind"], "dead-code");
        assert!(json["unused_exports"].is_array());
    }

    #[tokio::test]
    async fn run_analyze_circular_only_uses_api_family_path_without_cli_binary() {
        let project = tempfile::tempdir().expect("project");
        std::fs::write(
            project.path().join("package.json"),
            r#"{"name":"fixture","type":"module","main":"src/a.ts"}"#,
        )
        .expect("write package");
        std::fs::create_dir(project.path().join("src")).expect("create src");
        std::fs::write(project.path().join("src/a.ts"), "import './b';\n").expect("write a");
        std::fs::write(project.path().join("src/b.ts"), "import './a';\n").expect("write b");

        let result = run_analyze(
            "unused-binary-on-api-path",
            AnalyzeParams {
                root: Some(project.path().display().to_string()),
                issue_types: Some(vec!["circular-deps".to_string()]),
                no_cache: Some(true),
                ..AnalyzeParams::default()
            },
        )
        .await
        .expect("api result");

        assert_eq!(result.is_error, Some(false));
        let text = match &result.content[0].raw {
            RawContent::Text(text) => &text.text,
            _ => panic!("expected text content"),
        };
        let json: serde_json::Value = serde_json::from_str(text).expect("json");
        assert_eq!(json["kind"], "dead-code");
        assert!(json["circular_dependencies"].is_array());
        assert_eq!(json["unused_exports"].as_array().map(Vec::len), Some(0));
    }
}
