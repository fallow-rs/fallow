use std::path::PathBuf;

use crate::params::ArchitectureParams;

use fallow_api::{
    AnalysisOptions, ArchitectureOptions, run_architecture as run_api_architecture,
    serialize_architecture_programmatic_json,
};
use rmcp::ErrorData as McpError;
use rmcp::model::{CallToolResult, ContentBlock};

use super::{
    api_runtime::{
        env_changed_since, env_diff_file, json_success, non_empty_path, programmatic_error_body,
        run_api_blocking, workspace_patterns_from_param,
    },
    fallback_policy::{baseline_requested, filled, grouped_requested},
    push_baseline, push_global, push_remote_extends, push_scope, push_str_flag, run_tool,
};

/// Run `check_architecture` through the typed API. A baseline or a grouping
/// is a CLI-only surface, so those calls run `fallow architecture` instead.
pub async fn run_architecture(
    binary: &str,
    params: ArchitectureParams,
) -> Result<CallToolResult, McpError> {
    if requires_cli_fallback(&params) {
        let args = build_architecture_args(&params);
        return run_tool(binary, "check_architecture", &args).await;
    }

    let options = architecture_options_from_params(&params);
    let result = run_api_blocking("check_architecture", move || {
        run_api_architecture(&options).and_then(serialize_architecture_programmatic_json)
    })
    .await?
    .map_or_else(
        |err| CallToolResult::error(vec![ContentBlock::text(programmatic_error_body(&err))]),
        |value| json_success(&value),
    );
    Ok(result)
}

/// Build CLI arguments for the `check_architecture` tool.
pub fn build_architecture_args(params: &ArchitectureParams) -> Vec<String> {
    let mut args = vec![
        "architecture".to_string(),
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
    push_scope(&mut args, params.production, params.workspace.as_deref());
    push_str_flag(
        &mut args,
        "--changed-since",
        params.changed_since.as_deref(),
    );
    for (flag, selected) in [
        ("--cycles", params.cycles),
        ("--boundaries", params.boundaries),
        ("--policy", params.policy),
    ] {
        if selected == Some(true) {
            args.push(flag.to_string());
        }
    }
    push_baseline(&mut args, params.baseline.as_deref(), None);
    push_str_flag(&mut args, "--group-by", params.group_by.as_deref());
    for file in params.file.as_deref().unwrap_or_default() {
        if !file.is_empty() {
            args.extend(["--file".to_string(), file.clone()]);
        }
    }
    args
}

fn requires_cli_fallback(params: &ArchitectureParams) -> bool {
    baseline_requested(params.baseline.as_deref(), None)
        || grouped_requested(params.group_by.as_deref())
}

fn architecture_options_from_params(params: &ArchitectureParams) -> ArchitectureOptions {
    let changed_since = params
        .changed_since
        .as_deref()
        .filter(|value| filled(Some(value)))
        .map(str::to_string);
    ArchitectureOptions {
        analysis: AnalysisOptions {
            root: non_empty_path(params.root.as_deref()),
            config_path: non_empty_path(params.config.as_deref()),
            allow_remote_extends: params.allow_remote_extends.unwrap_or(false),
            no_cache: params.no_cache.unwrap_or(false),
            threads: params.threads,
            production: params.production.unwrap_or(false),
            production_override: params.production,
            ambient_changed_since: env_changed_since(),
            ambient_diff_file: env_diff_file(),
            changed_since,
            workspace: workspace_patterns_from_param(params.workspace.as_deref()),
            explain: true,
            ..AnalysisOptions::default()
        },
        files: params
            .file
            .as_deref()
            .unwrap_or_default()
            .iter()
            .filter(|path| !path.is_empty())
            .map(PathBuf::from)
            .collect(),
        cycles: params.cycles == Some(true),
        boundaries: params.boundaries == Some(true),
        policy: params.policy == Some(true),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn architecture_args_start_with_the_architecture_command() {
        let args = build_architecture_args(&ArchitectureParams::default());
        assert_eq!(
            args,
            ["architecture", "--format", "json", "--quiet", "--explain"]
        );
    }

    #[test]
    fn architecture_args_forward_selectors_scope_and_baseline() {
        let args = build_architecture_args(&ArchitectureParams {
            root: Some("/project".to_string()),
            cycles: Some(true),
            policy: Some(true),
            boundaries: Some(false),
            changed_since: Some("main".to_string()),
            baseline: Some("baseline.json".to_string()),
            group_by: Some("directory".to_string()),
            file: Some(vec!["src/a.ts".to_string(), String::new()]),
            ..ArchitectureParams::default()
        });
        let joined = args.join(" ");
        for expected in [
            "--root /project",
            "--cycles",
            "--policy",
            "--changed-since main",
            "--baseline baseline.json",
            "--group-by directory",
            "--file src/a.ts",
        ] {
            assert!(joined.contains(expected), "{expected} missing: {joined}");
        }
        assert!(!args.contains(&"--boundaries".to_string()));
        assert_eq!(args.iter().filter(|arg| *arg == "--file").count(), 1);
    }

    #[test]
    fn baseline_and_grouping_use_the_cli() {
        assert!(!requires_cli_fallback(&ArchitectureParams::default()));
        assert!(requires_cli_fallback(&ArchitectureParams {
            baseline: Some("baseline.json".to_string()),
            ..ArchitectureParams::default()
        }));
        assert!(requires_cli_fallback(&ArchitectureParams {
            group_by: Some("owner".to_string()),
            ..ArchitectureParams::default()
        }));
    }

    #[test]
    fn options_map_the_selectors() {
        let options = architecture_options_from_params(&ArchitectureParams {
            boundaries: Some(true),
            ..ArchitectureParams::default()
        });
        assert!(options.boundaries && !options.cycles && !options.policy);
        assert!(options.analysis.explain);
    }
}
