use crate::params::FindDupesParams;

use fallow_api::{
    AnalysisOptions, DuplicationMode, DuplicationOptions, run_duplication,
    serialize_duplication_programmatic_json,
};
use rmcp::ErrorData as McpError;
use rmcp::model::{CallToolResult, Content};

use super::{
    VALID_DUPES_MODES,
    api_runtime::{
        env_diff_file, json_success, non_empty_path, non_empty_string, programmatic_error_body,
        run_api_blocking,
    },
    fallback_policy::{CliFallbackReason, baseline_fallback_reason, duplication_fallback_reason},
    push_baseline, push_global, push_str_flag, run_tool, validation_error_body,
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
            Err(msg) => Ok(CallToolResult::error(vec![Content::text(msg)])),
        };
    }

    let options = match duplication_options_from_params(&params) {
        Ok(options) => options,
        Err(msg) => return Ok(CallToolResult::error(vec![Content::text(msg)])),
    };

    let result = run_api_blocking("find_dupes", move || {
        run_duplication(&options).and_then(serialize_duplication_programmatic_json)
    })
    .await?
    .map_or_else(
        |err| CallToolResult::error(vec![Content::text(programmatic_error_body(&err))]),
        |value| json_success(&value),
    );
    Ok(result)
}

pub fn run_find_dupes_api_value(
    params: &FindDupesParams,
) -> Result<Option<serde_json::Value>, String> {
    if requires_cli_fallback(params) {
        return Ok(None);
    }

    let options = duplication_options_from_params(params)?;
    let value = run_duplication(&options)
        .and_then(serialize_duplication_programmatic_json)
        .map_err(|err| programmatic_error_body(&err))?;

    Ok(Some(value))
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
    cli_fallback_reason(params).is_some()
}

fn cli_fallback_reason(params: &FindDupesParams) -> Option<CliFallbackReason> {
    baseline_fallback_reason(params.baseline.as_deref(), params.save_baseline.as_deref())
        .or_else(|| duplication_fallback_reason(params.group_by.as_deref(), params.explain_skipped))
}

fn duplication_options_from_params(params: &FindDupesParams) -> Result<DuplicationOptions, String> {
    Ok(DuplicationOptions {
        analysis: AnalysisOptions {
            root: non_empty_path(params.root.as_deref()),
            config_path: non_empty_path(params.config.as_deref()),
            no_cache: params.no_cache.unwrap_or(false),
            threads: params.threads,
            changed_since: non_empty_string(params.changed_since.as_deref()),
            diff_file: env_diff_file(),
            workspace: non_empty_string(params.workspace.as_deref())
                .map(|workspace| vec![workspace]),
            explain: true,
            ..AnalysisOptions::default()
        },
        mode: duplication_mode_from_param(params.mode.as_deref())?,
        min_tokens: params.min_tokens.map(|value| value as usize),
        min_lines: params.min_lines.map(|value| value as usize),
        min_occurrences: min_occurrences_from_param(params.min_occurrences)?,
        threshold: params.threshold,
        skip_local: params.skip_local,
        cross_language: params.cross_language,
        ignore_imports: params.ignore_imports,
        top: params.top,
    })
}

fn duplication_mode_from_param(mode: Option<&str>) -> Result<Option<DuplicationMode>, String> {
    match mode {
        None | Some("") => Ok(None),
        Some("strict") => Ok(Some(DuplicationMode::Strict)),
        Some("mild") => Ok(Some(DuplicationMode::Mild)),
        Some("weak") => Ok(Some(DuplicationMode::Weak)),
        Some("semantic") => Ok(Some(DuplicationMode::Semantic)),
        Some(mode) => Err(validation_error_body(format!(
            "Invalid mode '{mode}'. Valid values: strict, mild, weak, semantic"
        ))),
    }
}

fn min_occurrences_from_param(value: Option<u32>) -> Result<Option<usize>, String> {
    match value {
        Some(value) if value < 2 => Err(validation_error_body(format!(
            "min_occurrences must be at least 2 (got {value})"
        ))),
        Some(value) => Ok(Some(value as usize)),
        None => Ok(None),
    }
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
                "Invalid mode '{mode}'. Valid values: strict, mild, weak, semantic"
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
/// ignore-imports, `--explain-skipped`, `--top`) for `find_dupes`.
fn push_dupes_toggle_flags(args: &mut Vec<String>, params: &FindDupesParams) {
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
}

#[cfg(test)]
mod tests {
    use rmcp::model::RawContent;

    use super::*;

    #[test]
    fn api_path_accepts_pure_analysis_params() {
        let params = FindDupesParams {
            root: Some(String::new()),
            config: Some(String::new()),
            mode: Some("semantic".to_string()),
            workspace: Some("apps/web".to_string()),
            min_tokens: Some(12),
            min_lines: Some(3),
            min_occurrences: Some(4),
            threshold: Some(5.5),
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
        assert_eq!(options.min_tokens, Some(12));
        assert_eq!(options.min_lines, Some(3));
        assert_eq!(options.min_occurrences, Some(4));
        assert_eq!(options.threshold, Some(5.5));
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
        let text = match &result.content[0].raw {
            RawContent::Text(text) => &text.text,
            _ => panic!("expected text content"),
        };
        let json: serde_json::Value = serde_json::from_str(text).expect("json");
        assert_eq!(json["kind"], "dupes");
        assert!(json["clone_groups"].is_array());
    }
}
