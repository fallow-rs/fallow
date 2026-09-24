use crate::params::FindDupesParams;

use fallow_api::{
    AnalysisOptions, DuplicationOptions, run_duplication, serialize_duplication_programmatic_json,
};
use rmcp::ErrorData as McpError;
use rmcp::model::{CallToolResult, ContentBlock};

use super::{
    VALID_DUPES_MODES,
    api_runtime::{
        changed_since_from_param, env_diff_file, json_success, non_empty_path,
        programmatic_error_body, run_api_blocking, workspace_patterns_from_param,
    },
    duplication_mode_from_param,
    fallback_policy::{baseline_requested, duplication_needs_cli},
    min_occurrences_from_param, push_baseline, push_global, push_remote_extends, push_str_flag,
    run_tool, validation_error_body,
};

/// Run `find_dupes` through the typed API when parameters map cleanly to the
/// programmatic contract, falling back to the CLI for CLI-only surfaces.
pub async fn run_find_dupes(
    binary: &str,
    params: FindDupesParams,
) -> Result<CallToolResult, McpError> {
    if requires_cli_fallback(&params) {
        return match build_find_dupes_args(&params) {
            Ok(args) => run_tool(binary, "find_dupes", &args).await,
            Err(msg) => Ok(CallToolResult::error(vec![ContentBlock::text(msg)])),
        };
    }

    let options = match duplication_options_from_params(&params) {
        Ok(options) => options,
        Err(msg) => return Ok(CallToolResult::error(vec![ContentBlock::text(msg)])),
    };

    let result = run_api_blocking("find_dupes", move || {
        run_duplication(&options).and_then(serialize_duplication_programmatic_json)
    })
    .await?
    .map_or_else(
        |err| CallToolResult::error(vec![ContentBlock::text(programmatic_error_body(&err))]),
        |value| json_success(&value),
    );
    Ok(result)
}

/// Build CLI arguments for the `find_dupes` tool.
/// Returns `Err(message)` if an invalid mode is provided.
pub fn build_find_dupes_args(params: &FindDupesParams) -> Result<Vec<String>, String> {
    let mut args = vec![
        "dupes".to_string(),
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
    push_str_flag(&mut args, "--workspace", params.workspace.as_deref());
    push_dupes_detection_flags(&mut args, params)?;
    push_dupes_toggle_flags(&mut args, params);
    push_baseline(
        &mut args,
        params.baseline.as_deref(),
        params.save_baseline.as_deref(),
    );
    push_str_flag(
        &mut args,
        "--changed-since",
        params.changed_since.as_deref(),
    );
    push_str_flag(&mut args, "--group-by", params.group_by.as_deref());

    Ok(args)
}

fn requires_cli_fallback(params: &FindDupesParams) -> bool {
    baseline_requested(params.baseline.as_deref(), params.save_baseline.as_deref())
        || duplication_needs_cli(
            params.group_by.as_deref(),
            params.explain_skipped,
            params.threshold,
        )
}

fn duplication_options_from_params(params: &FindDupesParams) -> Result<DuplicationOptions, String> {
    Ok(DuplicationOptions {
        analysis: AnalysisOptions {
            root: non_empty_path(params.root.as_deref()),
            config_path: non_empty_path(params.config.as_deref()),
            allow_remote_extends: params.allow_remote_extends.unwrap_or(false),
            no_cache: params.no_cache.unwrap_or(false),
            threads: params.threads,
            changed_since: changed_since_from_param(params.changed_since.as_deref()),
            ambient_diff_file: env_diff_file(),
            workspace: workspace_patterns_from_param(params.workspace.as_deref()),
            explain: true,
            ..AnalysisOptions::default()
        },
        mode: duplication_mode_from_param(params.mode.as_deref())?,
        near: params.near,
        min_tokens: params.min_tokens.map(|value| value as usize),
        min_lines: params.min_lines.map(|value| value as usize),
        min_occurrences: min_occurrences_from_param(params.min_occurrences)?,
        threshold: params.threshold,
        skip_local: params.skip_local,
        cross_language: params.cross_language,
        ignore_imports: params.ignore_imports,
        top: params.top,
        include_fragments: Some(params.include_fragments.unwrap_or(false)),
    })
}

/// Push the validated detection-tuning flags (`--mode`, `--min-tokens`,
/// `--min-lines`, `--min-occurrences`, `--threshold`) for `find_dupes`.
fn push_dupes_detection_flags(
    args: &mut Vec<String>,
    params: &FindDupesParams,
) -> Result<(), String> {
    if let Some(ref mode) = params.mode
        && !mode.is_empty()
    {
        if !VALID_DUPES_MODES.contains(&mode.as_str()) {
            return Err(validation_error_body(format!(
                "Invalid mode '{mode}'. Valid values: {}",
                VALID_DUPES_MODES.join(", ")
            )));
        }
        args.extend(["--mode".to_string(), mode.clone()]);
    }
    if let Some(min_tokens) = params.min_tokens {
        args.extend(["--min-tokens".to_string(), min_tokens.to_string()]);
    }
    if let Some(min_lines) = params.min_lines {
        args.extend(["--min-lines".to_string(), min_lines.to_string()]);
    }
    if let Some(min_occurrences) = params.min_occurrences {
        if min_occurrences < 2 {
            return Err(validation_error_body(format!(
                "min_occurrences must be at least 2 (got {min_occurrences})"
            )));
        }
        args.extend(["--min-occurrences".to_string(), min_occurrences.to_string()]);
    }
    if let Some(threshold) = params.threshold {
        args.extend(["--threshold".to_string(), threshold.to_string()]);
    }
    Ok(())
}

/// Push the boolean toggle flags (`--skip-local`, `--cross-language`,
/// ignore-imports, `--explain-skipped`, `--top`, `--no-fragments`) for
/// `find_dupes`.
///
/// Fragments are opt-in over MCP: the CLI emits them by default, so the
/// fallback has to pass `--no-fragments` unless the caller asked for them.
fn push_dupes_toggle_flags(args: &mut Vec<String>, params: &FindDupesParams) {
    if params.near == Some(true) {
        args.push("--near".to_string());
    }
    if params.skip_local == Some(true) {
        args.push("--skip-local".to_string());
    }
    if params.cross_language == Some(true) {
        args.push("--cross-language".to_string());
    }
    match params.ignore_imports {
        Some(true) => args.push("--ignore-imports".to_string()),
        Some(false) => args.push("--no-ignore-imports".to_string()),
        None => {}
    }
    if params.explain_skipped == Some(true) {
        args.push("--explain-skipped".to_string());
    }
    if let Some(top) = params.top {
        args.extend(["--top".to_string(), top.to_string()]);
    }
    if params.include_fragments != Some(true) {
        args.push("--no-fragments".to_string());
    }
}

#[cfg(test)]
mod tests {
    use fallow_api::DuplicationMode;
    use rmcp::model::ContentBlock;

    use super::*;

    /// `threshold` is not a pure analysis parameter: it arms the duplication
    /// gate, and the programmatic route has no gate to arm. A typed call would
    /// compare nothing, publish no `gate_outcomes`, and read as a pass.
    #[test]
    fn a_threshold_takes_the_cli_because_the_api_evaluates_no_gate() {
        let params = FindDupesParams {
            threshold: Some(0.1),
            ..FindDupesParams::default()
        };

        assert!(requires_cli_fallback(&params));
        let args = build_find_dupes_args(&params).expect("args build");
        assert!(
            args.windows(2).any(|pair| pair == ["--threshold", "0.1"]),
            "{args:?}"
        );
    }

    #[test]
    fn api_path_accepts_pure_analysis_params() {
        let params = FindDupesParams {
            root: Some(String::new()),
            config: Some(String::new()),
            mode: Some("semantic".to_string()),
            near: Some(true),
            workspace: Some("apps/web".to_string()),
            min_tokens: Some(12),
            min_lines: Some(3),
            min_occurrences: Some(4),
            skip_local: Some(true),
            cross_language: Some(true),
            ignore_imports: Some(false),
            top: Some(7),
            changed_since: Some("main".to_string()),
            no_cache: Some(true),
            threads: Some(2),
            ..FindDupesParams::default()
        };

        assert!(!requires_cli_fallback(&params));
        let options = duplication_options_from_params(&params).expect("options");
        assert!(options.analysis.root.is_none());
        assert!(options.analysis.config_path.is_none());
        assert_eq!(
            options.analysis.workspace,
            Some(vec!["apps/web".to_string()])
        );
        assert_eq!(options.analysis.changed_since.as_deref(), Some("main"));
        assert!(options.analysis.no_cache);
        assert_eq!(options.analysis.threads, Some(2));
        assert!(matches!(options.mode, Some(DuplicationMode::Semantic)));
        assert_eq!(options.near, Some(true));
        assert_eq!(options.min_tokens, Some(12));
        assert_eq!(options.min_lines, Some(3));
        assert_eq!(options.min_occurrences, Some(4));
        assert_eq!(options.skip_local, Some(true));
        assert_eq!(options.cross_language, Some(true));
        assert_eq!(options.ignore_imports, Some(false));
        assert_eq!(options.top, Some(7));
    }

    #[test]
    fn api_path_reuses_cli_validation_for_bad_mode() {
        let params = FindDupesParams {
            mode: Some("SEMANTIC".to_string()),
            ..FindDupesParams::default()
        };
        let err = duplication_options_from_params(&params).expect_err("invalid mode");
        assert!(err.contains("Invalid mode"));
    }

    #[test]
    fn api_path_reuses_cli_validation_for_min_occurrences() {
        let params = FindDupesParams {
            min_occurrences: Some(1),
            ..FindDupesParams::default()
        };
        let err = duplication_options_from_params(&params).expect_err("invalid min occurrences");
        assert!(err.contains("min_occurrences must be at least 2"));
    }

    #[test]
    fn cli_fallback_keeps_cli_only_surfaces() {
        for params in [
            FindDupesParams {
                baseline: Some("baseline.json".to_string()),
                ..FindDupesParams::default()
            },
            FindDupesParams {
                save_baseline: Some("baseline.json".to_string()),
                ..FindDupesParams::default()
            },
            FindDupesParams {
                group_by: Some("owner".to_string()),
                ..FindDupesParams::default()
            },
            FindDupesParams {
                explain_skipped: Some(true),
                ..FindDupesParams::default()
            },
        ] {
            assert!(requires_cli_fallback(&params));
        }
    }

    #[test]
    fn fragments_are_opt_in_over_mcp() {
        let default_options =
            duplication_options_from_params(&FindDupesParams::default()).expect("options");
        assert_eq!(default_options.include_fragments, Some(false));

        let requested = duplication_options_from_params(&FindDupesParams {
            include_fragments: Some(true),
            ..FindDupesParams::default()
        })
        .expect("options");
        assert_eq!(requested.include_fragments, Some(true));
    }

    #[test]
    fn cli_fallback_suppresses_fragments_unless_requested() {
        let default_args = build_find_dupes_args(&FindDupesParams::default()).expect("dupes args");
        assert!(default_args.contains(&"--no-fragments".to_string()));

        let requested_args = build_find_dupes_args(&FindDupesParams {
            include_fragments: Some(true),
            ..FindDupesParams::default()
        })
        .expect("dupes args");
        assert!(!requested_args.contains(&"--no-fragments".to_string()));
    }

    #[test]
    fn cli_arguments_forward_near_detection() {
        let args = build_find_dupes_args(&FindDupesParams {
            near: Some(true),
            ..FindDupesParams::default()
        })
        .expect("dupes args");

        assert!(args.contains(&"--near".to_string()));
    }

    #[tokio::test]
    async fn run_find_dupes_api_path_returns_json_without_cli_binary() {
        let project = tempfile::tempdir().expect("project");
        std::fs::write(
            project.path().join("a.ts"),
            "export function first() {\n  const value = 1;\n  const next = value + 1;\n  return next;\n}\n",
        )
        .expect("write a");
        std::fs::write(
            project.path().join("b.ts"),
            "export function second() {\n  const value = 1;\n  const next = value + 1;\n  return next;\n}\n",
        )
        .expect("write b");

        let result = run_find_dupes(
            "unused-binary-on-api-path",
            FindDupesParams {
                root: Some(project.path().display().to_string()),
                min_tokens: Some(5),
                min_lines: Some(1),
                no_cache: Some(true),
                ..FindDupesParams::default()
            },
        )
        .await
        .expect("api result");

        assert_eq!(result.is_error, Some(false));
        let text = match &result.content[0] {
            ContentBlock::Text(text) => &text.text,
            _ => panic!("expected text content"),
        };
        let json: serde_json::Value = serde_json::from_str(text).expect("json");
        assert_eq!(json["kind"], "dupes");
        assert!(json["clone_groups"].is_array());
    }
}
